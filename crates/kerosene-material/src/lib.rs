// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Materials on the GPU.
//!
//! A material is a set of texture maps plus a few numbers, loaded from
//! compiled `.kmat_c` and `.ktex` resources through the VFS. This crate turns
//! one into a bind group: [`MaterialBindings`] is the layout and sampler every
//! material shares, [`maps`] loads a material's maps, and [`texture`] uploads
//! them. Neither the renderer nor the scene knows how a material is put
//! together; they bind the group and draw.
use kerosene_rhi::wgpu;

pub mod maps;
pub mod texture;

pub use maps::{LoadedMaps, NeutralMaps, load_material_maps};
pub use texture::{fallback_texture, load_texture, upload_rgba_format};

use bytemuck::{Pod, Zeroable};
use kerosene_asset::MapKind;

/// Anisotropic filtering samples. 16 is the usual maximum and is supported
/// everywhere wgpu runs; a device that cannot manage it clamps down rather
/// than failing.
const MAX_ANISOTROPY: u16 = 16;

/// The maps a material is made of, in binding order.
///
/// The renderer's copy of [`kerosene_asset::MapKind`]: the same six, in the
/// same order, because the shader indexes them by position. A test holds the
/// two lists against each other, so adding a kind on one side and forgetting
/// the other fails the build rather than binding roughness where the emissive
/// map should be.
pub const MAP_KINDS: [MapKind; MAP_COUNT] = [
    MapKind::Base,
    MapKind::Normal,
    MapKind::Roughness,
    MapKind::Emissive,
    MapKind::Ao,
    MapKind::Metalness,
];

/// How many texture bindings a material has.
pub const MAP_COUNT: usize = 6;

/// The first binding the material textures occupy; 0 and 1 are the sampler and
/// the presence uniform.
pub const MAP_BINDING_BASE: u32 = 2;

/// Which of a material's maps are real, and how strongly they act.
///
/// The alternative to shader variants: rather than compiling a pipeline per
/// combination of maps present, every material binds all six slots -- the
/// absent ones getting a 1x1 neutral texture -- and this says which of them
/// carry anything. A branch on a uniform is uniform across the draw, so it
/// costs about what a constant would.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct MaterialUniform {
    /// Bit `n` set means [`MAP_KINDS`]`[n]` is a real texture.
    pub present: u32,
    /// How much the emissive map adds. 1.0 unless a material says otherwise.
    pub emissive_strength: f32,
    /// How far the normal map is allowed to tilt the surface. 1.0 is as
    /// authored; 0 flattens it, which is what `r_bumpmap 0` sets.
    pub normal_strength: f32,
    /// Overall specular level, scaled by `r_specular`.
    pub specular_strength: f32,
    /// `$metalness`: the whole answer without a metalness map, a scale on the
    /// map with one.
    pub metalness: f32,
    /// `$roughnessfactor`, the same arrangement for roughness.
    pub roughness_factor: f32,
    /// [`MATERIAL_ALPHA_TEST`] and [`MATERIAL_TRANSLUCENT`].
    pub flags: u32,
    /// The alpha below which an alpha-tested texel is cut out.
    pub alpha_cutoff: f32,
}

/// [`MaterialUniform::flags`]: cut out texels below the alpha cutoff.
pub const MATERIAL_ALPHA_TEST: u32 = 1;
/// [`MaterialUniform::flags`]: blend with what is behind, by the texture's
/// alpha. Otherwise a surface is opaque whatever its texture's alpha says.
pub const MATERIAL_TRANSLUCENT: u32 = 2;

/// How a material's surfaces are drawn, beyond their textures: what picks
/// the pipeline and the pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterialMode {
    /// `$alphatest`: holes where the texture's alpha is low.
    pub alpha_test: bool,
    /// `$translucent`: blended, after everything solid, back to front.
    pub translucent: bool,
    /// `$nocull`: seen from both sides.
    pub two_sided: bool,
}

impl Default for MaterialUniform {
    fn default() -> Self {
        MaterialUniform {
            present: 0,
            emissive_strength: 1.0,
            normal_strength: 1.0,
            specular_strength: 1.0,
            metalness: 0.0,
            roughness_factor: 1.0,
            flags: 0,
            alpha_cutoff: 0.5,
        }
    }
}

/// The bind group layout and sampler every material shares.
pub struct MaterialBindings {
    /// Group layout: the sampler, the presence uniform and the six maps.
    pub layout: wgpu::BindGroupLayout,
    /// Repeating, trilinear, anisotropic.
    pub sampler: wgpu::Sampler,
}

impl MaterialBindings {
    /// Create the layout and sampler on `device`.
    pub fn new(device: &wgpu::Device) -> MaterialBindings {
        // A material is six textures, one sampler and a uniform saying which
        // of the six are real.
        //
        // The presence word is why there is one pipeline rather than sixty-four.
        // The alternative -- a shader variant per combination of maps -- means
        // compiling pipelines for combinations no material in the game uses,
        // and a stall the first time one turns up that was not predicted.
        // Branching on a uniform costs a coherent branch per draw, which on
        // any GPU built this century is close to nothing, because every
        // fragment in a draw takes the same side of it.
        let mut material_entries = vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<MaterialUniform>() as u64
                    ),
                },
                count: None,
            },
        ];
        for slot in 0..MAP_COUNT {
            material_entries.push(wgpu::BindGroupLayoutEntry {
                binding: MAP_BINDING_BASE + slot as u32,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &material_entries,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            // Brush geometry is floors and walls seen at grazing angles, which
            // is precisely the case trilinear filtering handles worst: the
            // mip is chosen for the shortest axis, so a corridor floor blurs
            // to mush a few metres out. Anisotropy costs a sampler flag.
            anisotropy_clamp: MAX_ANISOTROPY,
            ..Default::default()
        });
        MaterialBindings { layout, sampler }
    }
}
