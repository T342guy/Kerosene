// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Camera and model uniforms, and the draw calls.
use kerosene_rhi::wgpu;

use super::*;

impl Renderer {
    pub fn update_camera(&self, queue: &wgpu::Queue, uniform: &CameraUniform) {
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(uniform));
    }

    /// Tell the GPU where each brush model has got to.
    ///
    /// Written once per frame, before the pass, because a buffer written
    /// during a pass is not read by the draws in it -- every draw would see
    /// whichever value was written last, and the doors would all move
    /// together.
    ///
    /// Index 0 is the world and is always the identity; a caller may pass it
    /// or not.
    pub fn update_models(&self, queue: &wgpu::Queue, poses: &[Pose]) {
        let entries: Vec<ModelUniform> = poses.iter().map(|p| ModelUniform::from(*p)).collect();
        self.update_model_uniforms(queue, &entries);
    }

    /// [`Renderer::update_models`], with a probe for each slot as well.
    pub fn update_model_uniforms(&self, queue: &wgpu::Queue, entries: &[ModelUniform]) {
        // Written as one span with the device's stride between entries, so
        // the same buffer can be addressed by dynamic offset. The staging
        // buffer is kept between frames: this runs every frame, and the
        // span is over a hundred kilobytes.
        let stride = self.model_stride as usize;
        let mut bytes = self.model_staging.lock().unwrap_or_else(|e| e.into_inner());
        bytes.clear();
        bytes.resize(stride * MAX_MODELS, 0);
        let identity = ModelUniform::default();
        for i in 0..MAX_MODELS {
            let entry = entries.get(i).copied().unwrap_or(identity);
            let at = i * stride;
            bytes[at..at + std::mem::size_of::<ModelUniform>()]
                .copy_from_slice(bytemuck::bytes_of(&entry));
        }
        queue.write_buffer(&self.model_buffer, 0, &bytes);
    }

    /// The dynamic offset that addresses one model's entry.
    ///
    /// A model past the cap addresses slot 0, the world's identity, which is
    /// what "drawn unmoved" means; clamping to the last slot instead drew it
    /// at whatever pose happened to live there.
    pub(super) fn model_offset(&self, model: usize) -> u32 {
        let slot = if model < MAX_MODELS { model } else { 0 };
        self.model_stride * slot as u32
    }

    /// Draw one uploaded studio model, at the pose in model slot `slot`.
    ///
    /// Physics props and other dynamic models are drawn through here: the
    /// geometry comes from a `.kmdl` (not from the BSP), and the transform
    /// comes from the same model buffer the brush models use.
    ///
    /// `bones` is the palette slot from [`Renderer::update_palettes`] -- 0,
    /// the identity, for a model that is not animated.
    pub fn draw_studio_model<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame_bind_group: &'a wgpu::BindGroup,
        gpu_model: &'a GpuModel,
        slot: usize,
        bones: usize,
    ) -> FrameStats {
        let mut stats = FrameStats::default();
        if gpu_model.meshes.is_empty() {
            return stats;
        }

        pass.set_bind_group(0, frame_bind_group, &[]);
        pass.set_bind_group(2, &self.model_bind_group, &[self.model_offset(slot)]);
        pass.set_bind_group(3, &self.bones_bind_group, &[self.palette_offset(bones)]);
        pass.set_vertex_buffer(0, gpu_model.vertex_buffer.slice(..));
        pass.set_index_buffer(gpu_model.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_pipeline(&self.pipelines[&PipelineKey::from(Pass::Model)]);

        let mut current_material = u32::MAX;
        for &(first, count, material) in &gpu_model.meshes {
            if material != current_material
                && let Some(group) = gpu_model
                    .material_bind_groups
                    .get(material as usize)
                    .and_then(|g| g.as_ref())
            {
                pass.set_bind_group(1, group, &[]);
                current_material = material;
            }
            pass.draw_indexed(first..first + count, 0, 0..1);
            stats.draw_calls += 1;
            stats.triangles += (count / 3) as usize;
        }
        stats.surfaces_drawn = gpu_model.meshes.len();
        stats
    }

    /// Draw debug wireframe lines.
    ///
    /// `vertex_buffer` holds [`LineVertex`] pairs (two vertices per segment)
    /// and `vertex_count` is how many to draw. The caller uploads the buffer
    /// once per frame; the debug overlay is small and changes every frame.
    pub fn draw_lines<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame_bind_group: &'a wgpu::BindGroup,
        vertex_buffer: &'a wgpu::Buffer,
        vertex_count: u32,
    ) {
        if vertex_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipelines[&PipelineKey::from(Pass::Lines)]);
        pass.set_bind_group(0, frame_bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.draw(0..vertex_count, 0..1);
    }

    /// Everything a frame's draws share for one section: the camera, that
    /// section's lightmap atlas, and the map's probes.
    pub fn create_frame_bind_group(
        &self,
        device: &wgpu::Device,
        lightmap_view: &wgpu::TextureView,
        probes: &GpuProbes,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &self.frame_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(lightmap_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.lightmap_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&probes.view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.probe_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: self.lights_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: self.clusters_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&self.shadow_array),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                },
            ],
        })
    }

    /// Draw the visible surfaces of a map.
    ///
    /// `visible` must be sorted by material, which is what
    /// [`WorldMesh::visible_surfaces`] returns. Runs of surfaces that are
    /// adjacent in the index buffer are merged into one draw call.
    pub fn draw_world<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame_bind_group: &'a wgpu::BindGroup,
        resources: &'a MapResources,
        mesh: &WorldMesh,
        visible: &[u32],
    ) -> FrameStats {
        // The world is model 0, which never moves.
        self.draw_surfaces(pass, frame_bind_group, resources, mesh, visible, 0, None)
    }

    /// The world's `$translucent` surfaces among `visible`, back to front
    /// from `eye`. Call after everything solid -- the world, the brush
    /// models, the props -- so glass blends over what is behind it.
    pub fn draw_world_translucent<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame_bind_group: &'a wgpu::BindGroup,
        resources: &'a MapResources,
        mesh: &WorldMesh,
        visible: &[u32],
        eye: Vec3,
    ) -> FrameStats {
        self.draw_surfaces(
            pass,
            frame_bind_group,
            resources,
            mesh,
            visible,
            0,
            Some(eye),
        )
    }

    /// One brush model's `$translucent` surfaces, back to front from `eye`.
    /// See [`draw_world_translucent`](Renderer::draw_world_translucent).
    pub fn draw_model_translucent<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame_bind_group: &'a wgpu::BindGroup,
        resources: &'a MapResources,
        mesh: &WorldMesh,
        model: usize,
        eye: Vec3,
    ) -> FrameStats {
        let Some(surfaces) = mesh.model_surfaces.get(model) else {
            return FrameStats::default();
        };
        self.draw_surfaces(
            pass,
            frame_bind_group,
            resources,
            mesh,
            surfaces,
            model,
            Some(eye),
        )
    }

    /// Draw one brush model's surfaces, wherever it has moved to.
    ///
    /// Doors, lifts, anything tied to a class. Their leaves are not in the
    /// world's PVS, so they cannot be found by the leaf walk that finds
    /// everything else -- they are drawn by asking each model whether it is in
    /// front of the camera.
    pub fn draw_model<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame_bind_group: &'a wgpu::BindGroup,
        resources: &'a MapResources,
        mesh: &WorldMesh,
        model: usize,
    ) -> FrameStats {
        let Some(surfaces) = mesh.model_surfaces.get(model) else {
            return FrameStats::default();
        };
        self.draw_surfaces(
            pass,
            frame_bind_group,
            resources,
            mesh,
            surfaces,
            model,
            None,
        )
    }

    /// Draw `visible`'s surfaces of one model. With no `eye` this is the
    /// solid phase, and translucent surfaces are skipped; with one, it is
    /// the translucent phase, and only they are drawn, farthest first.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_surfaces<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame_bind_group: &'a wgpu::BindGroup,
        resources: &'a MapResources,
        mesh: &WorldMesh,
        visible: &[u32],
        model: usize,
        translucent_from: Option<Vec3>,
    ) -> FrameStats {
        let mut stats = FrameStats {
            surfaces_total: mesh.surfaces.len(),
            ..Default::default()
        };
        let is_translucent = |index: u32| {
            resources
                .material_mode(mesh.surfaces[index as usize].material)
                .translucent
        };
        let sorted: Vec<u32>;
        let visible: &[u32] = match translucent_from {
            None => visible,
            Some(eye) => {
                // Back to front by each surface's centre. Per surface rather
                // than per triangle: two panes crossing each other can still
                // sort wrong, the usual price, and the one Source pays.
                let mut glass: Vec<(f32, u32)> = visible
                    .iter()
                    .copied()
                    .filter(|&i| is_translucent(i))
                    .map(|i| {
                        let centre = mesh.surfaces[i as usize].bounds.center();
                        ((centre - eye).length_squared(), i)
                    })
                    .collect();
                glass.sort_by(|a, b| b.0.total_cmp(&a.0));
                sorted = glass.into_iter().map(|(_, i)| i).collect();
                &sorted
            }
        };
        if visible.is_empty() {
            return stats;
        }

        pass.set_bind_group(0, frame_bind_group, &[]);
        pass.set_bind_group(2, &self.model_bind_group, &[self.model_offset(model)]);
        pass.set_vertex_buffer(0, resources.vertices.slice(..));
        pass.set_index_buffer(resources.indices.slice(..), wgpu::IndexFormat::Uint32);

        let mut current_material = u32::MAX;
        let mut current_pass: Option<Pass> = None;
        // Accumulate adjacent surfaces into one draw.
        let mut run: Option<(u32, u32)> = None;

        let flush = |pass: &mut wgpu::RenderPass<'a>,
                     run: &mut Option<(u32, u32)>,
                     stats: &mut FrameStats| {
            if let Some((first, count)) = run.take() {
                pass.draw_indexed(first..first + count, 0, 0..1);
                stats.draw_calls += 1;
                stats.triangles += (count / 3) as usize;
            }
        };

        for &index in visible {
            let surface = &mesh.surfaces[index as usize];
            let mode = resources.material_mode(surface.material);
            if translucent_from.is_none() && mode.translucent {
                continue;
            }
            let wanted_pass = if surface.flags & surf::SKY != 0 {
                Pass::Sky
            } else {
                match (mode.translucent, mode.two_sided, surface.lit) {
                    (true, _, true) => Pass::Translucent,
                    (true, _, false) => Pass::TranslucentUnlit,
                    (false, true, true) => Pass::WorldTwoSided,
                    (false, true, false) => Pass::UnlitTwoSided,
                    (false, false, true) => Pass::World,
                    (false, false, false) => Pass::Unlit,
                }
            };

            if current_pass != Some(wanted_pass) {
                flush(pass, &mut run, &mut stats);
                let Some(pipeline) = self.pipelines.get(&PipelineKey::from(wanted_pass)) else {
                    continue;
                };
                pass.set_pipeline(pipeline);
                current_pass = Some(wanted_pass);
                // A pipeline change invalidates nothing about bindings, but
                // the material must be re-bound after the first set_pipeline.
                current_material = u32::MAX;
            }

            if surface.material != current_material {
                flush(pass, &mut run, &mut stats);
                let Some(bind_group) = resources.material_bind_group(surface.material) else {
                    continue;
                };
                pass.set_bind_group(1, bind_group, &[]);
                current_material = surface.material;
            }

            run = match run {
                Some((first, count)) if first + count == surface.first_index => {
                    Some((first, count + surface.index_count))
                }
                other => {
                    if let Some((f, c)) = other {
                        pass.draw_indexed(f..f + c, 0, 0..1);
                        stats.draw_calls += 1;
                        stats.triangles += (c / 3) as usize;
                    }
                    Some((surface.first_index, surface.index_count))
                }
            };
            stats.surfaces_drawn += 1;
        }

        flush(pass, &mut run, &mut stats);
        stats
    }
}
