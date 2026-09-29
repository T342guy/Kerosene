// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! One loaded map's GPU resources.
use kerosene_rhi::wgpu;

use super::*;

/// GPU resources for one loaded map.
pub struct MapResources {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub lightmap_view: wgpu::TextureView,
    /// One bind group per material, indexed the same way `WorldMesh` does.
    pub(super) material_bind_groups: Vec<Option<wgpu::BindGroup>>,
    /// How each material is drawn, indexed the same way.
    pub(super) material_modes: Vec<MaterialMode>,
    /// Materials that failed to load, so the engine can report them once.
    pub missing_materials: Vec<String>,
    /// Keeps every uploaded texture alive alongside its bind group.
    pub(super) _textures: Vec<wgpu::Texture>,
    /// And the per-material presence uniforms, for the same reason.
    pub(super) _buffers: Vec<wgpu::Buffer>,
}

impl MapResources {
    pub fn material_bind_group(&self, index: u32) -> Option<&wgpu::BindGroup> {
        self.material_bind_groups.get(index as usize)?.as_ref()
    }

    /// How a material is drawn: see [`MaterialMode`].
    pub fn material_mode(&self, index: u32) -> MaterialMode {
        self.material_modes
            .get(index as usize)
            .copied()
            .unwrap_or_default()
    }

    /// Upload a map's geometry, lightmap atlas and materials.
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &Renderer,
        mesh: &WorldMesh,
        atlas: &LightmapAtlas,
        vfs: &Vfs,
    ) -> MapResources {
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("world vertices"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("world indices"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Four bytes a texel, like RGBA8, but linear light with an exponent:
        // see `ATLAS_FORMAT`.
        let lightmap = upload_rgba_format(
            device,
            queue,
            "lightmap atlas",
            ATLAS_SIZE,
            ATLAS_SIZE,
            &atlas.pixels,
            ATLAS_FORMAT,
        );
        let lightmap_view = lightmap.create_view(&wgpu::TextureViewDescriptor::default());

        // A flat texture stands in for anything that will not load, so a
        // missing material is a visibly wrong surface rather than a crash.
        let fallback = fallback_texture(device, queue);
        let fallback_view = fallback.create_view(&wgpu::TextureViewDescriptor::default());

        // Neutral stand-ins for the maps a material does not have. A missing
        // normal map is a flat surface, not an error: most materials have
        // none, and the checkerboard is reserved for the one map whose
        // absence really is a mistake -- the colour.
        let neutrals = NeutralMaps::new(device, queue);

        let mut material_bind_groups = Vec::with_capacity(mesh.materials.len());
        let mut material_modes = Vec::with_capacity(mesh.materials.len());
        let mut missing_materials = Vec::new();
        let mut textures = vec![lightmap, fallback];
        let mut buffers = Vec::with_capacity(mesh.materials.len());

        for name in &mesh.materials {
            let loaded = load_material_maps(device, queue, vfs, name);
            if loaded.is_none() {
                missing_materials.push(name.clone());
            }
            let loaded = loaded.unwrap_or_default();
            material_modes.push(loaded.mode);

            let (group, buffer) = loaded.bind_group(
                device,
                &renderer.materials,
                name,
                &neutrals,
                &fallback_view,
                &mut textures,
            );
            material_bind_groups.push(Some(group));
            buffers.push(buffer);
        }

        textures.extend(neutrals.into_textures());

        MapResources {
            vertices,
            indices,
            lightmap_view,
            material_bind_groups,
            material_modes,
            missing_materials,
            _textures: textures,
            _buffers: buffers,
        }
    }
}
