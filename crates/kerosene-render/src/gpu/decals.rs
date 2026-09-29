// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Decals on the GPU.
use kerosene_rhi::wgpu;

use super::*;

/// Decals on the GPU: their materials, and this frame's geometry.
///
/// [`crate::decals`] cuts the geometry; this keeps it in one vertex buffer,
/// re-uploaded only when the set of decals changes, and draws it per
/// section -- each section has its own lightmap atlas, and a decal's
/// lightmap coordinates are the section's it was cut from.
pub struct GpuDecals {
    pub(super) neutrals: NeutralMaps,
    pub(super) fallback_view: wgpu::TextureView,
    pub(super) textures: Vec<wgpu::Texture>,
    pub(super) materials: HashMap<String, (wgpu::BindGroup, wgpu::Buffer)>,
    /// Materials that would not load, reported once each.
    pub missing: Vec<String>,
    pub(super) vertices: Option<(wgpu::Buffer, u64)>,
    /// `(section, material, first vertex, vertex count)`.
    pub(super) draws: Vec<(usize, String, u32, u32)>,
    pub(super) revision: Option<u64>,
}

/// One decal to draw: its section, material and cut geometry.
pub struct DecalDraw<'a> {
    pub section: usize,
    pub material: &'a str,
    pub vertices: &'a [WorldVertex],
}

impl GpuDecals {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> GpuDecals {
        let fallback = fallback_texture(device, queue);
        let fallback_view = fallback.create_view(&wgpu::TextureViewDescriptor::default());
        GpuDecals {
            neutrals: NeutralMaps::new(device, queue),
            fallback_view,
            textures: vec![fallback],
            materials: HashMap::new(),
            missing: Vec::new(),
            vertices: None,
            draws: Vec::new(),
            revision: None,
        }
    }

    /// Upload the decals, if `revision` says they changed since last time.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &Renderer,
        vfs: &Vfs,
        revision: u64,
        decals: &[DecalDraw<'_>],
    ) {
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        self.draws.clear();
        let mut vertices: Vec<WorldVertex> = Vec::new();
        for decal in decals {
            if decal.vertices.is_empty() {
                continue;
            }
            if !self.materials.contains_key(decal.material) {
                let loaded = load_material_maps(device, queue, vfs, decal.material);
                if loaded.is_none() && !self.missing.iter().any(|m| m == decal.material) {
                    self.missing.push(decal.material.to_string());
                }
                let entry = loaded.unwrap_or_default().bind_group(
                    device,
                    &renderer.materials,
                    decal.material,
                    &self.neutrals,
                    &self.fallback_view,
                    &mut self.textures,
                );
                self.materials.insert(decal.material.to_string(), entry);
            }
            // Adjacent decals of one material in one section draw together.
            let first = vertices.len() as u32;
            let count = decal.vertices.len() as u32;
            vertices.extend_from_slice(decal.vertices);
            match self.draws.last_mut() {
                Some((section, material, start, n))
                    if *section == decal.section
                        && material == decal.material
                        && *start + *n == first =>
                {
                    *n += count;
                }
                _ => self
                    .draws
                    .push((decal.section, decal.material.to_string(), first, count)),
            }
        }
        if vertices.is_empty() {
            return;
        }
        let bytes = (vertices.len() * std::mem::size_of::<WorldVertex>()) as u64;
        if self.vertices.as_ref().is_none_or(|(_, cap)| *cap < bytes) {
            let capacity = bytes.next_power_of_two();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("decals"),
                size: capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vertices = Some((buffer, capacity));
        }
        if let Some((buffer, _)) = &self.vertices {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&vertices));
        }
    }

    /// Draw one section's decals, inside the scene pass after its world.
    pub fn draw_section<'a>(
        &'a self,
        renderer: &'a Renderer,
        pass: &mut wgpu::RenderPass<'a>,
        frame_bind_group: &'a wgpu::BindGroup,
        section: usize,
    ) -> FrameStats {
        let mut stats = FrameStats::default();
        let Some((buffer, _)) = &self.vertices else {
            return stats;
        };
        let Some(pipeline) = renderer.pipelines.get(&PipelineKey::from(Pass::Decal)) else {
            return stats;
        };
        let mut bound = false;
        for (s, material, first, count) in &self.draws {
            if *s != section {
                continue;
            }
            let Some((group, _)) = self.materials.get(material) else {
                continue;
            };
            if !bound {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, frame_bind_group, &[]);
                pass.set_bind_group(2, &renderer.model_bind_group, &[renderer.model_offset(0)]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                bound = true;
            }
            pass.set_bind_group(1, group, &[]);
            pass.draw(*first..*first + *count, 0..1);
            stats.draw_calls += 1;
            stats.triangles += (*count / 3) as usize;
        }
        stats
    }
}
