// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Loading a material's maps.
use kerosene_rhi::wgpu;

use crate::texture::{load_texture, upload_rgba_format};
use crate::{
    MAP_BINDING_BASE, MAP_COUNT, MAP_KINDS, MATERIAL_ALPHA_TEST, MATERIAL_TRANSLUCENT,
    MaterialBindings, MaterialMode, MaterialUniform,
};
use kerosene_asset::{MapKind, Material};
use kerosene_vfs::Vfs;
use kerosene_rhi::wgpu::util::DeviceExt;

/// The 1x1 textures that stand in for maps a material does not have.
///
/// One set per map load rather than one per material: every material without a
/// normal map can point at the same flat blue pixel, and a map with four
/// hundred materials would otherwise make four hundred copies of it.
pub struct NeutralMaps {
    textures: Vec<wgpu::Texture>,
    pub(crate) views: Vec<wgpu::TextureView>,
}

impl NeutralMaps {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> NeutralMaps {
        let mut textures = Vec::with_capacity(MAP_COUNT);
        let mut views = Vec::with_capacity(MAP_COUNT);
        for kind in MAP_KINDS {
            // What each map means when it is absent: white light through the
            // colour, a normal pointing straight out, fully rough (the
            // lightmap is diffuse, so a surface with nothing said about it
            // should not shine), nothing emitted, nothing occluded, and
            // metal exactly as far as `$metalness` says.
            let texel: [u8; 4] = match kind {
                MapKind::Base => [255, 255, 255, 255],
                MapKind::Normal => [128, 128, 255, 255],
                MapKind::Roughness => [255, 255, 255, 255],
                MapKind::Emissive => [0, 0, 0, 255],
                MapKind::Ao => [255, 255, 255, 255],
                MapKind::Metalness => [255, 255, 255, 255],
            };
            let texture = upload_rgba_format(
                device,
                queue,
                "neutral map",
                1,
                1,
                &texel,
                // Neutral texels are the same number in either encoding, so
                // the format only has to match the binding, not the value.
                wgpu::TextureFormat::Rgba8Unorm,
            );
            views.push(texture.create_view(&wgpu::TextureViewDescriptor::default()));
            textures.push(texture);
        }
        NeutralMaps { textures, views }
    }

    pub fn into_textures(self) -> Vec<wgpu::Texture> {
        self.textures
    }
}

/// A material's maps, as far as they loaded.
pub struct LoadedMaps {
    /// One slot per [`MAP_KINDS`] entry; `None` where the material named no
    /// such map, or named one that would not load.
    maps: [Option<wgpu::Texture>; MAP_COUNT],
    /// Whether the base colour loaded. False means the checkerboard, and a
    /// line in the console.
    has_base: bool,
    /// The material's `$metalness`.
    metalness: f32,
    /// The material's `$roughnessfactor`.
    roughness_factor: f32,
    /// How it is drawn.
    pub mode: MaterialMode,
    /// `$alphatestreference`.
    alpha_cutoff: f32,
}

/// What a material that did not load gets: no maps, not metal, fully rough.
/// Written out rather than derived, because a derived zero roughness would
/// turn every missing material into a mirror.
impl Default for LoadedMaps {
    fn default() -> Self {
        LoadedMaps {
            maps: Default::default(),
            has_base: false,
            metalness: 0.0,
            roughness_factor: 1.0,
            mode: MaterialMode::default(),
            alpha_cutoff: 0.5,
        }
    }
}

impl LoadedMaps {
    /// Build the bind group for these maps, filling the gaps with neutrals.
    ///
    /// Takes `textures` to push into: every uploaded texture has to outlive
    /// the bind group that points at it, and the map's resource list is where
    /// they are kept alive.
    pub fn bind_group(
        self,
        device: &wgpu::Device,
        bindings: &MaterialBindings,
        label: &str,
        neutrals: &NeutralMaps,
        fallback_view: &wgpu::TextureView,
        textures: &mut Vec<wgpu::Texture>,
    ) -> (wgpu::BindGroup, wgpu::Buffer) {
        let mut present = 0u32;
        let mut views = Vec::with_capacity(MAP_COUNT);

        for (slot, texture) in self.maps.into_iter().enumerate() {
            match texture {
                Some(texture) => {
                    present |= 1 << slot;
                    views.push(texture.create_view(&wgpu::TextureViewDescriptor::default()));
                    textures.push(texture);
                }
                // The colour is the one map whose absence is worth shouting
                // about, so it gets the checkerboard rather than white.
                None if slot == 0 && !self.has_base => views.push(fallback_view.clone()),
                None => views.push(neutrals.views[slot].clone()),
            }
        }

        let uniform = MaterialUniform {
            present,
            metalness: self.metalness,
            roughness_factor: self.roughness_factor,
            flags: (self.mode.alpha_test as u32 * MATERIAL_ALPHA_TEST)
                | (self.mode.translucent as u32 * MATERIAL_TRANSLUCENT),
            alpha_cutoff: self.alpha_cutoff,
            ..Default::default()
        };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(&bindings.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buffer.as_entire_binding(),
            },
        ];
        for (slot, view) in views.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: MAP_BINDING_BASE + slot as u32,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }

        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &bindings.layout,
            entries: &entries,
        });
        (group, buffer)
    }
}

/// Load a material and every map it names, through the VFS.
///
/// `None` means the material itself would not load -- no file, or not
/// parseable -- which is the case the checkerboard exists for. A material that
/// loads but whose bump map is missing is not that case: it comes back with
/// the maps it does have, and the surface renders without bumps rather than
/// rendering as an error.
pub fn load_material_maps(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vfs: &Vfs,
    name: &str,
) -> Option<LoadedMaps> {
    let material = Material::load(vfs, name).ok()?;

    // A sky material's base texture is sampled by direction, but it is loaded
    // exactly the same way as any other.
    let mut loaded = LoadedMaps {
        metalness: material.metalness(),
        roughness_factor: material.roughness_factor(),
        mode: MaterialMode {
            alpha_test: material.is_alpha_tested(),
            translucent: material.is_blended(),
            two_sided: material.is_two_sided(),
        },
        alpha_cutoff: material.alpha_test_reference(),
        ..Default::default()
    };
    for (slot, kind) in MAP_KINDS.into_iter().enumerate() {
        // The base colour falls back to the material's own name, which is the
        // convention a material with no `$basetexture` has always relied on.
        let texture_name = match kind {
            MapKind::Base => Some(material.base_texture().unwrap_or(name)),
            MapKind::Normal => material.bump_map(),
            MapKind::Roughness => material.roughness_map(),
            MapKind::Emissive => material.emissive_map(),
            MapKind::Ao => material.ao_map(),
            MapKind::Metalness => material.metalness_map(),
        };
        let Some(texture_name) = texture_name.filter(|n| !n.is_empty()) else {
            continue;
        };

        match load_texture(device, queue, vfs, texture_name) {
            Some(texture) => {
                loaded.maps[slot] = Some(texture);
                if kind == MapKind::Base {
                    loaded.has_base = true;
                }
            }
            // Named but absent: worth a line, because somebody wrote the key
            // and it is not doing anything.
            None => log::warn!(
                "material {name}: {} names {texture_name}, which would not load",
                kind.material_param()
            ),
        }
    }

    Some(loaded)
}
