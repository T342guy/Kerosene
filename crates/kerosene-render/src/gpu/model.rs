// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Studio models on the GPU.
use kerosene_rhi::wgpu;

use super::*;

/// One studio model uploaded to the GPU, ready to draw.
pub struct GpuModel {
    pub(super) vertex_buffer: wgpu::Buffer,
    pub(super) index_buffer: wgpu::Buffer,
    /// `(first_index, index_count, material_index)` per mesh.
    pub(super) meshes: Vec<(u32, u32, u32)>,
    pub(super) material_bind_groups: Vec<Option<wgpu::BindGroup>>,
    /// Keeps every uploaded texture alive alongside its bind group.
    pub(super) _textures: Vec<wgpu::Texture>,
    pub(super) _buffers: Vec<wgpu::Buffer>,
    /// The model's compiled bounds, for culling and hull fitting.
    pub bounds: kerosene_math::Aabb,
}

/// Load and upload a `.kmdl` model by the name an entity refers to it by.
///
/// Returns `None` when the model is missing or malformed -- a missing prop
/// should be a logged warning and nothing drawn, not a crash.
pub fn load_model(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &Renderer,
    vfs: &Vfs,
    name: &str,
) -> Option<GpuModel> {
    let path = kerosene_asset::model_path(name);
    let bytes = vfs.read(&path).ok()?;
    let model = Model::from_bytes(&bytes).ok()?;

    let vertices: Vec<ModelVertex> = model
        .vertices
        .iter()
        .map(|v| ModelVertex {
            position: v.position,
            normal: v.normal,
            uv: v.uv,
            bone_indices: v.bone_indices,
            bone_weights: v.bone_weights,
        })
        .collect();
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("model vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("model indices"),
        contents: bytemuck::cast_slice(&model.indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    let fallback = fallback_texture(device, queue);
    let fallback_view = fallback.create_view(&wgpu::TextureViewDescriptor::default());
    let neutrals = NeutralMaps::new(device, queue);
    let mut textures = vec![fallback];
    let mut buffers = Vec::new();

    let mut name_to_group: HashMap<String, u32> = HashMap::new();
    let mut material_bind_groups: Vec<Option<wgpu::BindGroup>> = Vec::new();
    let mut meshes = Vec::with_capacity(model.meshes.len());

    for i in 0..model.meshes.len() {
        let material_name = model.mesh_material(i).to_string();
        let material_index = match name_to_group.get(&material_name) {
            Some(&idx) => idx,
            None => {
                // Props share the world's material group, so they load the
                // same six maps. Binding a layout the pipeline declares but
                // the data does not fill is not an option: it is a validation
                // error, not a blank surface.
                let loaded =
                    load_material_maps(device, queue, vfs, &material_name).unwrap_or_default();
                let (group, buffer) = loaded.bind_group(
                    device,
                    &renderer.materials,
                    &material_name,
                    &neutrals,
                    &fallback_view,
                    &mut textures,
                );
                buffers.push(buffer);
                let idx = material_bind_groups.len() as u32;
                material_bind_groups.push(Some(group));
                name_to_group.insert(material_name, idx);
                idx
            }
        };
        let mesh = &model.meshes[i];
        meshes.push((mesh.first_index, mesh.index_count, material_index));
    }

    textures.extend(neutrals.into_textures());

    Some(GpuModel {
        vertex_buffer,
        index_buffer,
        meshes,
        material_bind_groups,
        _textures: textures,
        _buffers: buffers,
        bounds: model.bounds,
    })
}

/// The view matrix a camera would use, exposed for tools that want it without
/// building a whole renderer.
pub fn view_projection(camera: &Camera) -> Mat4 {
    camera.view_projection()
}
