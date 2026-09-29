// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Per-frame setup: bone palettes, instances, render targets, tone mapping.
use kerosene_rhi::wgpu;

use super::*;

impl Renderer {
    /// Upload this frame's animated models' palettes into slots 1 onward:
    /// `palettes[i]` is bone slot `i + 1`. Past [`MAX_SKINNED`] they are
    /// dropped, and a draw asking for one gets the rest pose.
    pub fn update_palettes(&self, queue: &wgpu::Queue, palettes: &[Vec<Mat4>]) {
        let stride = self.palette_stride as usize;
        for (i, palette) in palettes.iter().take(MAX_SKINNED).enumerate() {
            let mut matrices = vec![Mat4::IDENTITY.to_cols_array_2d(); kerosene_asset::MAX_BONES];
            for (slot, m) in matrices.iter_mut().zip(palette) {
                *slot = m.to_cols_array_2d();
            }
            queue.write_buffer(
                &self.bones_buffer,
                ((i + 1) * stride) as u64,
                bytemuck::cast_slice(&matrices),
            );
        }
    }

    pub(super) fn palette_offset(&self, slot: usize) -> u32 {
        let slot = if slot <= MAX_SKINNED { slot } else { 0 };
        self.palette_stride * slot as u32
    }

    /// Upload this frame's static-prop instances, growing the buffer when
    /// there are more than last time. Draws then address them by range.
    pub fn update_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[ModelInstance],
    ) {
        if instances.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(instances);
        let big_enough = self
            .instance_buffer
            .as_ref()
            .is_some_and(|b| b.size() >= bytes.len() as u64);
        if !big_enough {
            // Doubled, so a map that adds props as it plays reallocates a
            // handful of times rather than every frame.
            let size = (bytes.len() as u64).next_power_of_two().max(4096);
            self.instance_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("static prop instances"),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = &self.instance_buffer {
            queue.write_buffer(buffer, 0, bytes);
        }
    }

    /// Draw instances `first..first + count` of the last upload, all copies
    /// of `gpu_model`, in one draw per mesh rather than one per copy.
    pub fn draw_studio_instances<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame_bind_group: &'a wgpu::BindGroup,
        gpu_model: &'a GpuModel,
        first: u32,
        count: u32,
    ) -> FrameStats {
        let mut stats = FrameStats::default();
        let Some(instances) = &self.instance_buffer else {
            return stats;
        };
        if gpu_model.meshes.is_empty() || count == 0 {
            return stats;
        }
        pass.set_pipeline(&self.pipelines[&PipelineKey::from(Pass::ModelInstanced)]);
        pass.set_bind_group(0, frame_bind_group, &[]);
        // The pipeline layout has the model slot; the instanced shader reads
        // its transform from the instance buffer instead, but a layout's
        // groups must all be bound.
        pass.set_bind_group(2, &self.model_bind_group, &[0]);
        pass.set_bind_group(3, &self.bones_bind_group, &[0]);
        pass.set_vertex_buffer(0, gpu_model.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, instances.slice(..));
        pass.set_index_buffer(gpu_model.index_buffer.slice(..), wgpu::IndexFormat::Uint32);

        let mut current_material = u32::MAX;
        for &(index_first, index_count, material) in &gpu_model.meshes {
            if material != current_material
                && let Some(group) = gpu_model
                    .material_bind_groups
                    .get(material as usize)
                    .and_then(|g| g.as_ref())
            {
                pass.set_bind_group(1, group, &[]);
                current_material = material;
            }
            pass.draw_indexed(
                index_first..index_first + index_count,
                0,
                first..first + count,
            );
            stats.draw_calls += 1;
            stats.triangles += (index_count / 3) as usize * count as usize;
        }
        stats
    }

    /// [`Renderer::draw_studio_instances`], into a shadow layer.
    pub fn draw_studio_shadow_instances<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        layer: usize,
        gpu_model: &'a GpuModel,
        first: u32,
        count: u32,
    ) {
        let Some(instances) = &self.instance_buffer else {
            return;
        };
        if gpu_model.meshes.is_empty() || count == 0 {
            return;
        }
        pass.set_pipeline(&self.shadow_instanced_pipeline);
        pass.set_bind_group(
            0,
            &self.shadow_view_bind_group,
            &[self.shadow_view_offset(layer)],
        );
        pass.set_bind_group(1, &self.model_bind_group, &[0]);
        pass.set_vertex_buffer(0, gpu_model.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, instances.slice(..));
        pass.set_index_buffer(gpu_model.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for &(index_first, index_count, _) in &gpu_model.meshes {
            pass.draw_indexed(
                index_first..index_first + index_count,
                0,
                first..first + count,
            );
        }
    }

    /// The swapchain format the tone-map pass writes.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Samples per pixel the scene is drawn with.
    pub fn msaa_samples(&self) -> u32 {
        self.samples
    }

    /// Ask for multisampling. Returns the sample count actually used.
    ///
    /// Anything above 1 means [`MSAA_SAMPLES`]: four samples is the one count
    /// every backend wgpu runs on supports for both the HDR and the depth
    /// format, and asking the adapter for more would make the answer depend
    /// on the machine for a difference few people can see. A change rebuilds
    /// the scene pipelines, since the sample count is baked into them, and
    /// drops the targets for [`Renderer::ensure_targets`] to remake.
    pub fn set_msaa(&mut self, device: &wgpu::Device, requested: u32) -> u32 {
        let samples = msaa_samples_for(requested);
        if samples != self.samples {
            self.samples = samples;
            self.pipelines = scene_pipelines(device, &self.scene, samples);
            self.targets = None;
        }
        samples
    }

    /// Create or resize the scene targets: HDR colour, its multisampled
    /// partner if MSAA is on, and depth.
    pub fn ensure_targets(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if let Some(t) = &self.targets
            && t.width == width
            && t.height == height
        {
            return;
        }
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let target = |label: &str, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };

        let resolved = target(
            "scene",
            HDR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let multisampled = (self.samples > 1).then(|| {
            target(
                "scene msaa",
                HDR_FORMAT,
                self.samples,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            )
        });
        let depth = target(
            "depth",
            DEPTH_FORMAT,
            self.samples,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let tonemap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tonemap"),
            layout: &self.tonemap_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&resolved),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.tonemap_buffer.as_entire_binding(),
                },
            ],
        });
        self.targets = Some(Targets {
            width,
            height,
            multisampled,
            resolved,
            depth,
            tonemap_bind_group,
        });
    }

    /// Begin the pass the scene is drawn in: HDR colour, cleared to `clear`,
    /// resolved from the multisampled target if there is one, and depth.
    ///
    /// Panics if [`Renderer::ensure_targets`] has not been called since the
    /// last change of size or sample count; that is a bug in the frame loop,
    /// not a condition to recover from.
    pub fn begin_scene_pass<'e>(
        &'e self,
        encoder: &'e mut wgpu::CommandEncoder,
        clear: wgpu::Color,
    ) -> wgpu::RenderPass<'e> {
        let targets = self
            .targets
            .as_ref()
            .expect("ensure_targets before begin_scene_pass");
        // With MSAA the samples are drawn into the multisampled target and
        // averaged into `resolved` when the pass ends. The samples themselves
        // are never read again, so they are not stored: on a tiled GPU that
        // is the difference between MSAA being nearly free and not.
        let (view, resolve_target, store) = match &targets.multisampled {
            Some(ms) => (ms, Some(&targets.resolved), wgpu::StoreOp::Discard),
            None => (&targets.resolved, None, wgpu::StoreOp::Store),
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// Set the exposure and curve the next [`Renderer::tonemap`] applies.
    pub fn update_tonemap(&self, queue: &wgpu::Queue, exposure: f32, operator: ToneMapOperator) {
        self.update_display(queue, exposure, operator, 1.0);
    }

    /// [`update_tonemap`](Renderer::update_tonemap), with a gamma adjustment
    /// for the display: see [`ToneMapUniform::gamma`].
    pub fn update_display(
        &self,
        queue: &wgpu::Queue,
        exposure: f32,
        operator: ToneMapOperator,
        gamma: f32,
    ) {
        let uniform = ToneMapUniform {
            exposure: exposure.max(0.0),
            curve: operator as u32,
            gamma: gamma.clamp(0.1, 4.0),
            ..Default::default()
        };
        queue.write_buffer(&self.tonemap_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    /// Fold the HDR scene down into `output`, the swapchain image.
    ///
    /// Overwrites all of `output`; anything drawn after this -- the UI --
    /// loads what it leaves.
    pub fn tonemap(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let Some(targets) = &self.targets else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tonemap"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                resolve_target: None,
                ops: wgpu::Operations {
                    // Every pixel is written, so there is nothing to load.
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.tonemap_pipeline);
        pass.set_bind_group(0, &targets.tonemap_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
