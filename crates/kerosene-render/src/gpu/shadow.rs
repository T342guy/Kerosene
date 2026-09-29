// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Dynamic lights and the shadow pass.
use kerosene_rhi::wgpu;

use super::*;

impl Renderer {
    /// Upload this frame's dynamic lights, their clusters and their shadow
    /// views. Before the shadow passes and the scene pass, like every other
    /// per-frame write.
    pub fn update_lights(&self, queue: &wgpu::Queue, frame: &LightFrame) {
        queue.write_buffer(&self.lights_buffer, 0, bytemuck::bytes_of(&frame.uniform));
        queue.write_buffer(
            &self.clusters_buffer,
            0,
            bytemuck::bytes_of(&frame.clusters),
        );
        if frame.shadow_views.is_empty() {
            return;
        }
        let stride = self.shadow_view_stride as usize;
        let mut bytes = vec![0u8; stride * frame.shadow_views.len()];
        for (layer, (view, _)) in frame.shadow_views.iter().enumerate() {
            let at = layer * stride;
            bytes[at..at + 64].copy_from_slice(bytemuck::bytes_of(&view.to_cols_array_2d()));
        }
        queue.write_buffer(&self.shadow_view_buffer, 0, &bytes);
    }

    /// Begin rendering shadow layer `layer`: depth only, cleared to far.
    pub fn begin_shadow_pass<'e>(
        &'e self,
        encoder: &'e mut wgpu::CommandEncoder,
        layer: usize,
    ) -> wgpu::RenderPass<'e> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.shadow_layers[layer.min(SHADOW_LAYERS - 1)],
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// Draw world or brush-model surfaces into a shadow layer, at the pose in
    /// model slot `model` (0 for the world). Sky casts no shadow.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_world_shadow<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        layer: usize,
        resources: &'a MapResources,
        mesh: &WorldMesh,
        surfaces: &[u32],
        model: usize,
    ) -> FrameStats {
        let mut stats = FrameStats::default();
        if surfaces.is_empty() {
            return stats;
        }
        pass.set_pipeline(&self.shadow_world_pipeline);
        pass.set_bind_group(
            0,
            &self.shadow_view_bind_group,
            &[self.shadow_view_offset(layer)],
        );
        pass.set_bind_group(1, &self.model_bind_group, &[self.model_offset(model)]);
        pass.set_vertex_buffer(0, resources.vertices.slice(..));
        pass.set_index_buffer(resources.indices.slice(..), wgpu::IndexFormat::Uint32);

        // Depth only, so the material does not matter and any two surfaces
        // adjacent in the index buffer are one draw.
        let mut run: Option<(u32, u32)> = None;
        for &index in surfaces {
            let Some(surface) = mesh.surfaces.get(index as usize) else {
                continue;
            };
            if surface.flags & surf::SKY != 0 {
                continue;
            }
            run = match run {
                Some((first, count)) if first + count == surface.first_index => {
                    Some((first, count + surface.index_count))
                }
                other => {
                    if let Some((f, c)) = other {
                        pass.draw_indexed(f..f + c, 0, 0..1);
                        stats.draw_calls += 1;
                    }
                    Some((surface.first_index, surface.index_count))
                }
            };
            stats.triangles += (surface.index_count / 3) as usize;
        }
        if let Some((f, c)) = run {
            pass.draw_indexed(f..f + c, 0, 0..1);
            stats.draw_calls += 1;
        }
        stats
    }

    /// Draw a studio model into a shadow layer, at the pose in model slot
    /// `slot`.
    pub fn draw_studio_shadow<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        layer: usize,
        gpu_model: &'a GpuModel,
        slot: usize,
        bones: usize,
    ) {
        if gpu_model.meshes.is_empty() {
            return;
        }
        pass.set_pipeline(&self.shadow_model_pipeline);
        pass.set_bind_group(
            0,
            &self.shadow_view_bind_group,
            &[self.shadow_view_offset(layer)],
        );
        pass.set_bind_group(1, &self.model_bind_group, &[self.model_offset(slot)]);
        pass.set_bind_group(2, &self.bones_bind_group, &[self.palette_offset(bones)]);
        pass.set_vertex_buffer(0, gpu_model.vertex_buffer.slice(..));
        pass.set_index_buffer(gpu_model.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for &(first, count, _) in &gpu_model.meshes {
            pass.draw_indexed(first..first + count, 0, 0..1);
        }
    }

    pub(super) fn shadow_view_offset(&self, layer: usize) -> u32 {
        self.shadow_view_stride * layer.min(SHADOW_LAYERS - 1) as u32
    }
}
