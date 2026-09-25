// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The wgpu layer.
//!
//! Deliberately thin: every decision about *what* to draw has already been
//! made by [`crate::mesh::WorldMesh::visible_surfaces`], so this is buffer
//! management, pipeline setup, and a draw loop.
//!
//! Materials each get their own bind group, and surfaces arrive sorted by
//! material, so the loop rebinds only when the material actually changes.

use crate::FrameStats;
use crate::camera::Camera;
use crate::lightmap::{ATLAS_FORMAT, ATLAS_SIZE, LightmapAtlas};
use crate::lights::{ClusterMasks, LightFrame, LightsUniform, SHADOW_LAYERS, SHADOW_SIZE};
use crate::mesh::{NO_PROBE, WorldMesh, WorldVertex};
use crate::probes::ProbeChain;
use bytemuck::{Pod, Zeroable};
use kerosene_asset::{MapKind, Material, Model, Texture};
use kerosene_bsp::surf;
use kerosene_math::{Mat4, Pose, Vec3};
use kerosene_vfs::Vfs;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

/// A vertex in a studio model, as uploaded to the GPU.
///
/// The same forty bytes as a `.keromdl` vertex, bone influences included: a
/// static model is fully weighted to bone 0, which the identity palette in
/// bone slot 0 leaves where it is, so one vertex format and one shader serve
/// a crate and a character alike.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct ModelVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub bone_indices: [u8; 4],
    /// Normalised: 255 is all of the vertex.
    pub bone_weights: [u8; 4],
}

/// How many animated models can be drawn in one frame, each with its own
/// palette. Past it, a model is drawn in its rest pose.
pub const MAX_SKINNED: usize = 64;

/// One skinning palette: a matrix per bone, as the shaders declare it.
const PALETTE_BYTES: u64 = (kerosene_anim::MAX_BONES * 64) as u64;

/// One copy of a studio model, for instanced drawing: where it is and which
/// probe it reflects.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ModelInstance {
    pub transform: [[f32; 4]; 4],
    pub probe: u32,
    pub _pad: [u32; 3],
}

impl ModelInstance {
    pub fn new(pose: Pose, probe: u32) -> ModelInstance {
        ModelInstance {
            transform: pose.to_mat4().to_cols_array_2d(),
            probe,
            _pad: [0; 3],
        }
    }
}

/// The instance buffer's attributes: the transform's four columns at
/// locations 3 to 6 and the probe at 7, after the model's own 0 to 2.
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = [
    wgpu::VertexAttribute {
        offset: 0,
        shader_location: 3,
        format: wgpu::VertexFormat::Float32x4,
    },
    wgpu::VertexAttribute {
        offset: 16,
        shader_location: 4,
        format: wgpu::VertexFormat::Float32x4,
    },
    wgpu::VertexAttribute {
        offset: 32,
        shader_location: 5,
        format: wgpu::VertexFormat::Float32x4,
    },
    wgpu::VertexAttribute {
        offset: 48,
        shader_location: 6,
        format: wgpu::VertexFormat::Float32x4,
    },
    wgpu::VertexAttribute {
        offset: 64,
        shader_location: 7,
        format: wgpu::VertexFormat::Uint32,
    },
];

/// A debug wireframe vertex: a position and a colour, nothing else.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct LineVertex {
    pub position: [f32; 3],
    pub color: [f32; 3],
}

/// Uniforms shared by every draw in a frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub position: [f32; 4],
    /// `[unused, time, lightmaps_enabled, fullbright]`.
    ///
    /// The first slot was exposure. It moved to the tone-map pass, which is
    /// the only place it can be applied once to everything; the slot stays so
    /// the layout both shaders declare does not shift under them.
    pub params: [f32; 4],
    pub sky_color: [f32; 4],
    /// `[bumpmap_scale, specular_scale, 0, 0]`.
    ///
    /// The material half of the debug toggles. They live on the per-frame
    /// uniform rather than on the per-material one because they are global and
    /// change at the console: putting them beside `present` would mean
    /// rewriting every material's buffer to answer `r_bumpmap 0`, and the
    /// shader has to read both uniforms anyway.
    ///
    /// Scales rather than flags, so `r_bumpmap 2` exaggerates a normal map to
    /// see what it is doing -- the reason to reach for the convar at all.
    pub render: [f32; 4],
}

impl CameraUniform {
    pub fn from_camera(camera: &Camera, time: f32) -> Self {
        CameraUniform {
            view_proj: camera.view_projection().to_cols_array_2d(),
            position: camera.position.extend(1.0).to_array(),
            params: [0.0, time, 1.0, 0.0],
            sky_color: [1.0, 1.0, 1.0, 1.0],
            render: [1.0, 1.0, 0.0, 0.0],
        }
    }

    pub fn set_lightmaps(&mut self, on: bool) {
        self.params[2] = if on { 1.0 } else { 0.0 };
    }
    pub fn set_fullbright(&mut self, on: bool) {
        self.params[3] = if on { 1.0 } else { 0.0 };
    }
    pub fn set_sky_color(&mut self, c: Vec3) {
        self.sky_color = c.extend(1.0).to_array();
    }
    /// How far normal maps are allowed to tilt a surface. 0 flattens them.
    pub fn set_bumpmap(&mut self, scale: f32) {
        self.render[0] = scale.max(0.0);
    }
    /// Overall specular level. 0 removes every highlight.
    pub fn set_specular(&mut self, scale: f32) {
        self.render[1] = scale.max(0.0);
    }
}

/// Anisotropic filtering samples. 16 is the usual maximum and is supported
/// everywhere wgpu runs; a device that cannot manage it clamps down rather
/// than failing.
const MAX_ANISOTROPY: u16 = 16;

/// The depth format. 32-bit float because a Source-scale map spans 32768
/// units, and 24-bit depth z-fights visibly at that range.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The scene's colour format: linear, half-float, with room above 1.0.
///
/// Lighting adds up -- a lightmap, a highlight on top of it, an emissive
/// sign beside it -- and an 8-bit target clipped each of those as it was
/// drawn. Half floats keep the sum and leave the tone-map pass to decide what
/// "too bright" looks like, once, for the whole frame.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Shadow map depth format. 32-bit float for the same reason the scene's
/// depth is: a light's range can be thousands of units.
pub const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Samples per pixel when multisampling is on.
pub const MSAA_SAMPLES: u32 = 4;

/// The curve that folds HDR scene colour into what a display can show.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u32)]
pub enum ToneMapOperator {
    /// Clip at 1.0. For looking at raw values, not for playing.
    None = 0,
    /// `x / (x + 1)`. What the renderer did per surface before it had an HDR
    /// target; kept so a map lit for it can be compared.
    Reinhard = 1,
    /// A filmic curve: a toe that keeps shadows dense and a shoulder that
    /// rolls highlights off rather than clipping them.
    #[default]
    Aces = 2,
}

impl ToneMapOperator {
    /// The operator a `mat_tonemap` value names. Out-of-range values get the
    /// default rather than an error, since the convar is typed at a console.
    pub fn from_index(index: i32) -> ToneMapOperator {
        match index {
            0 => ToneMapOperator::None,
            1 => ToneMapOperator::Reinhard,
            _ => ToneMapOperator::Aces,
        }
    }
}

/// What the tone-map pass reads besides the scene.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ToneMapUniform {
    pub exposure: f32,
    /// A [`ToneMapOperator`] as its discriminant.
    pub curve: u32,
    pub _pad: [f32; 2],
}

impl Default for ToneMapUniform {
    fn default() -> Self {
        ToneMapUniform {
            exposure: 1.0,
            curve: ToneMapOperator::default() as u32,
            _pad: [0.0; 2],
        }
    }
}

/// Which pipeline draws a surface.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pass {
    World,
    Sky,
    Unlit,
    /// A studio model, drawn without a lightmap.
    Model,
    /// Many copies of one studio model in one draw: static props.
    ModelInstanced,
    /// Debug wireframe lines.
    Lines,
    /// Projected decals: the world's shading, blended over it.
    Decal,
}

/// Where one brush model has got to since it was compiled.
///
/// A full transform rather than a displacement. It was three floats while the
/// only movers were doors that slide, and that was exactly enough until
/// something needed to turn -- at which point a translation cannot express
/// the answer at all, and neither can the collision code that has to agree
/// with it.
///
/// A whole uniform per model, because a dynamic offset is the portable way to
/// change a value between draws inside one render pass: push constants are an
/// optional feature and rewriting a buffer mid-pass does not do what it looks
/// like it does.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ModelUniform {
    pub transform: [[f32; 4]; 4],
    /// `x`: the cubemap probe a studio model reflects, or [`NO_PROBE`]. Brush
    /// models ignore it -- their vertices carry their own, like the world's.
    pub probe: [u32; 4],
}

impl Default for ModelUniform {
    fn default() -> Self {
        ModelUniform {
            transform: Mat4::IDENTITY.to_cols_array_2d(),
            probe: [NO_PROBE, 0, 0, 0],
        }
    }
}

impl From<Pose> for ModelUniform {
    fn from(pose: Pose) -> Self {
        ModelUniform {
            transform: pose.to_mat4().to_cols_array_2d(),
            ..Default::default()
        }
    }
}

impl ModelUniform {
    /// A pose that reflects `probe`.
    pub fn with_probe(pose: Pose, probe: u32) -> Self {
        ModelUniform {
            probe: [probe, 0, 0, 0],
            ..ModelUniform::from(pose)
        }
    }
}

// ---- materials --------------------------------------------------------------

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
    pub _pad: [f32; 2],
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
            _pad: [0.0; 2],
        }
    }
}

/// How many brush models one map may have on screen.
///
/// Generous: a map with more moving brush entities than this has other
/// problems. Anything past it is drawn unmoved rather than not at all, which
/// is the failure that leaves a door in the wrong place rather than the one
/// that makes it vanish.
pub const MAX_MODELS: usize = 512;

/// GPU resources that outlive any one map.
pub struct Renderer {
    /// Everything that draws into the HDR scene target, keyed by pass. Built
    /// for one sample count and rebuilt when `r_msaa` changes it.
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    /// What the scene pipelines are built from, kept so a change of sample
    /// count can rebuild them without recompiling a shader.
    scene: SceneShaders,
    pub frame_layout: wgpu::BindGroupLayout,
    pub material_layout: wgpu::BindGroupLayout,
    camera_buffer: wgpu::Buffer,
    /// One [`ModelUniform`] per brush model, indexed by a dynamic offset.
    model_buffer: wgpu::Buffer,
    /// CPU-side span the poses are laid out in before upload, kept so it is
    /// not reallocated every frame. A mutex only because `update_models`
    /// takes `&self`; it is never contended.
    model_staging: std::sync::Mutex<Vec<u8>>,
    model_bind_group: wgpu::BindGroup,
    /// Distance between two entries in `model_buffer`, honouring the device's
    /// uniform alignment.
    model_stride: u32,
    sampler: wgpu::Sampler,
    lightmap_sampler: wgpu::Sampler,
    /// Trilinear and clamped: the mip level is how blurry a reflection is,
    /// and a face must not wrap round to its own far edge.
    probe_sampler: wgpu::Sampler,
    /// Samples per pixel in the scene target: 1, or [`MSAA_SAMPLES`].
    samples: u32,
    /// The scene's colour and depth, sized to the window. `None` until the
    /// first [`Renderer::ensure_targets`] and after a change of sample count.
    targets: Option<Targets>,
    tonemap_pipeline: wgpu::RenderPipeline,
    tonemap_layout: wgpu::BindGroupLayout,
    tonemap_buffer: wgpu::Buffer,
    /// The swapchain's format: what the tone-map pass writes.
    format: wgpu::TextureFormat,
    /// This frame's dynamic lights, and which cluster each can reach.
    lights_buffer: wgpu::Buffer,
    clusters_buffer: wgpu::Buffer,
    /// Every shadow layer, as one array the scene samples...
    shadow_array: wgpu::TextureView,
    /// ...and each layer on its own, to render into.
    shadow_layers: Vec<wgpu::TextureView>,
    _shadow_texture: wgpu::Texture,
    shadow_sampler: wgpu::Sampler,
    /// One light view-projection per shadow layer, by dynamic offset.
    shadow_view_buffer: wgpu::Buffer,
    shadow_view_bind_group: wgpu::BindGroup,
    shadow_view_stride: u32,
    /// Depth-only pipelines for world geometry and studio models.
    shadow_world_pipeline: wgpu::RenderPipeline,
    shadow_model_pipeline: wgpu::RenderPipeline,
    shadow_instanced_pipeline: wgpu::RenderPipeline,
    /// Every static prop's [`ModelInstance`] this frame, grown as needed.
    instance_buffer: Option<wgpu::Buffer>,
    /// Bone palettes: slot 0 the identity, then one per animated model.
    bones_buffer: wgpu::Buffer,
    bones_bind_group: wgpu::BindGroup,
    palette_stride: u32,
}

/// The scene shaders and layouts, everything a pipeline needs except a
/// sample count.
struct SceneShaders {
    world: wgpu::ShaderModule,
    model: wgpu::ShaderModule,
    line: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    /// The world's layout plus the bone palettes, for studio models.
    model_layout: wgpu::PipelineLayout,
    line_layout: wgpu::PipelineLayout,
}

/// The render targets a frame is drawn into before tone-mapping.
struct Targets {
    width: u32,
    height: u32,
    /// The multisampled colour target, when there is one. Resolved into
    /// `resolved` at the end of the scene pass, and never read otherwise.
    multisampled: Option<wgpu::TextureView>,
    /// The single-sampled HDR colour the tone-map pass reads.
    resolved: wgpu::TextureView,
    depth: wgpu::TextureView,
    tonemap_bind_group: wgpu::BindGroup,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct PipelineKey(u8);

impl From<Pass> for PipelineKey {
    fn from(p: Pass) -> Self {
        PipelineKey(p as u8)
    }
}

impl Renderer {
    /// `format` is the swapchain's. The scene itself is drawn in
    /// [`HDR_FORMAT`] and reaches `format` only through [`Renderer::tonemap`].
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/world.wgsl").into()),
        });
        let model_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("model"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/model.wgsl").into()),
        });
        let line_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("line"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/line.wgsl").into()),
        });
        let tonemap_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tonemap"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/tonemap.wgsl").into()),
        });

        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // The map's cubemap probes, six layers each.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Dynamic lights, then the cluster masks that say which of
                // them each part of the view can see.
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<LightsUniform>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<ClusterMasks>() as u64,
                        ),
                    },
                    count: None,
                },
                // The shadow maps, compared in hardware.
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });

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
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &material_entries,
        });

        let model_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<ModelUniform>() as u64
                    ),
                },
                count: None,
            }],
        });

        // Entries have to start on the device's uniform alignment, which is
        // 256 bytes on most hardware for a structure that needs 16.
        let alignment = device.limits().min_uniform_buffer_offset_alignment.max(1);
        let size = std::mem::size_of::<ModelUniform>() as u32;
        let model_stride = size.div_ceil(alignment) * alignment;

        let model_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("models"),
            size: (model_stride as u64) * MAX_MODELS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let model_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model"),
            layout: &model_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &model_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<ModelUniform>() as u64),
                }),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world"),
            bind_group_layouts: &[&frame_layout, &material_layout, &model_layout],
            push_constant_ranges: &[],
        });

        // ---- skinning ----
        // One palette per animated model, addressed by dynamic offset like
        // the model transforms. Slot 0 is all identities and never
        // rewritten: what every static model binds.
        let bones_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bones"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(PALETTE_BYTES),
                },
                count: None,
            }],
        });
        let palette_stride = (PALETTE_BYTES as u32).div_ceil(alignment) * alignment;
        let identity: Vec<[[f32; 4]; 4]> =
            vec![Mat4::IDENTITY.to_cols_array_2d(); kerosene_anim::MAX_BONES];
        let mut palette_init = vec![0u8; palette_stride as usize * (MAX_SKINNED + 1)];
        palette_init[..PALETTE_BYTES as usize].copy_from_slice(bytemuck::cast_slice(&identity));
        let bones_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bone palettes"),
            contents: &palette_init,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bones_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bones"),
            layout: &bones_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &bones_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(PALETTE_BYTES),
                }),
            }],
        });
        let model_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("model"),
                bind_group_layouts: &[
                    &frame_layout,
                    &material_layout,
                    &model_layout,
                    &bones_layout,
                ],
                push_constant_ranges: &[],
            });

        // ---- dynamic lights and shadows ----
        let lights_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("dynamic lights"),
            contents: bytemuck::bytes_of(&LightsUniform::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let clusters_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("light clusters"),
            contents: bytemuck::bytes_of(&ClusterMasks::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let shadow_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow maps"),
            size: wgpu::Extent3d {
                width: SHADOW_SIZE,
                height: SHADOW_SIZE,
                depth_or_array_layers: SHADOW_LAYERS as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_array = shadow_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("shadow maps"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let shadow_layers = (0..SHADOW_LAYERS as u32)
            .map(|layer| {
                shadow_texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("shadow layer"),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow compare"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            // Linear with a comparison is hardware 2x2 percentage-closer
            // filtering: four depth tests, blended, for the price of one.
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let shadow_view_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("shadow view"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(64),
                    },
                    count: None,
                }],
            });
        let shadow_view_stride = 64u32.div_ceil(alignment) * alignment;
        let shadow_view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow views"),
            size: shadow_view_stride as u64 * SHADOW_LAYERS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_view_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow view"),
            layout: &shadow_view_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &shadow_view_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(64),
                }),
            }],
        });
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shadow.wgsl").into()),
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow"),
            bind_group_layouts: &[&shadow_view_layout, &model_layout],
            push_constant_ranges: &[],
        });
        let shadow_skinned_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow skinned"),
                bind_group_layouts: &[&shadow_view_layout, &model_layout, &bones_layout],
                push_constant_ranges: &[],
            });
        // `skinned` builds the studio model's: bones as well as position.
        let shadow_pipeline = |label: &str, stride: usize, instanced: bool, skinned: bool| {
            let position = wgpu::VertexBufferLayout {
                array_stride: stride as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                }],
            };
            let skinned_position = wgpu::VertexBufferLayout {
                array_stride: stride as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x3,
                    },
                    wgpu::VertexAttribute {
                        offset: 32,
                        shader_location: 8,
                        format: wgpu::VertexFormat::Uint8x4,
                    },
                    wgpu::VertexAttribute {
                        offset: 36,
                        shader_location: 9,
                        format: wgpu::VertexFormat::Unorm8x4,
                    },
                ],
            };
            let buffers = if skinned {
                vec![skinned_position]
            } else if instanced {
                vec![
                    position,
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<ModelInstance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &INSTANCE_ATTRIBUTES,
                    },
                ]
            } else {
                vec![position]
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(if skinned {
                    &shadow_skinned_layout
                } else {
                    &shadow_layout
                }),
                vertex: wgpu::VertexState {
                    module: &shadow_shader,
                    entry_point: Some(if skinned {
                        "vs_shadow_skinned"
                    } else if instanced {
                        "vs_shadow_instanced"
                    } else {
                        "vs_shadow"
                    }),
                    buffers: &buffers,
                    compilation_options: Default::default(),
                },
                fragment: None,
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    // Both sides: a brush wall is one-sided, and a light on
                    // its far side must still be stopped by it.
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: SHADOW_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    // Pushes the stored depth back a little, more on slopes,
                    // so a lit surface does not shadow itself in stripes.
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 2.5,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let shadow_world_pipeline = shadow_pipeline(
            "shadow world",
            std::mem::size_of::<WorldVertex>(),
            false,
            false,
        );
        let shadow_model_pipeline = shadow_pipeline(
            "shadow model",
            std::mem::size_of::<ModelVertex>(),
            false,
            true,
        );
        let shadow_instanced_pipeline = shadow_pipeline(
            "shadow instanced",
            std::mem::size_of::<ModelVertex>(),
            true,
            false,
        );

        // Debug lines: the same camera uniform, a line-list topology, and a
        // colour straight through. Only the camera is bound.
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("line"),
            bind_group_layouts: &[&frame_layout],
            push_constant_ranges: &[],
        });

        let scene = SceneShaders {
            world: shader,
            model: model_shader,
            line: line_shader,
            layout: pipeline_layout,
            model_layout: model_pipeline_layout,
            line_layout,
        };
        let samples = 1;
        let pipelines = scene_pipelines(device, &scene, samples);

        // The tone-map pass reads the resolved scene by texel, so it needs no
        // sampler: the source and the target are the same size.
        let tonemap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tonemap"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<ToneMapUniform>() as u64,
                        ),
                    },
                    count: None,
                },
            ],
        });
        let tonemap_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("tonemap"),
                bind_group_layouts: &[&tonemap_layout],
                push_constant_ranges: &[],
            });
        let tonemap_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tonemap"),
            layout: Some(&tonemap_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &tonemap_shader,
                entry_point: Some("vs_fullscreen"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &tonemap_shader,
                entry_point: Some("fs_tonemap"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let tonemap_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tonemap"),
            contents: bytemuck::bytes_of(&ToneMapUniform::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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

        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lightmap"),
            // Clamped, because a lightmap patch that wraps samples whatever
            // was packed on the far side of the atlas.
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let probe_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("probes"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Renderer {
            pipelines,
            scene,
            frame_layout,
            material_layout,
            camera_buffer,
            model_buffer,
            model_staging: std::sync::Mutex::new(Vec::new()),
            model_bind_group,
            model_stride,
            sampler,
            lightmap_sampler,
            probe_sampler,
            samples,
            targets: None,
            tonemap_pipeline,
            tonemap_layout,
            tonemap_buffer,
            format,
            lights_buffer,
            clusters_buffer,
            shadow_array,
            shadow_layers,
            _shadow_texture: shadow_texture,
            shadow_sampler,
            shadow_view_buffer,
            shadow_view_bind_group,
            shadow_view_stride,
            shadow_world_pipeline,
            shadow_model_pipeline,
            shadow_instanced_pipeline,
            instance_buffer: None,
            bones_buffer,
            bones_bind_group,
            palette_stride,
        }
    }

    /// Upload this frame's animated models' palettes into slots 1 onward:
    /// `palettes[i]` is bone slot `i + 1`. Past [`MAX_SKINNED`] they are
    /// dropped, and a draw asking for one gets the rest pose.
    pub fn update_palettes(&self, queue: &wgpu::Queue, palettes: &[Vec<Mat4>]) {
        let stride = self.palette_stride as usize;
        for (i, palette) in palettes.iter().take(MAX_SKINNED).enumerate() {
            let mut matrices = vec![Mat4::IDENTITY.to_cols_array_2d(); kerosene_anim::MAX_BONES];
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

    fn palette_offset(&self, slot: usize) -> u32 {
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
        let uniform = ToneMapUniform {
            exposure: exposure.max(0.0),
            curve: operator as u32,
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

    fn shadow_view_offset(&self, layer: usize) -> u32 {
        self.shadow_view_stride * layer.min(SHADOW_LAYERS - 1) as u32
    }

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
    fn model_offset(&self, model: usize) -> u32 {
        let slot = if model < MAX_MODELS { model } else { 0 };
        self.model_stride * slot as u32
    }

    /// Draw one uploaded studio model, at the pose in model slot `slot`.
    ///
    /// Physics props and other dynamic models are drawn through here: the
    /// geometry comes from a `.keromdl` (not from the BSP), and the transform
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
        self.draw_surfaces(pass, frame_bind_group, resources, mesh, visible, 0)
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
        self.draw_surfaces(pass, frame_bind_group, resources, mesh, surfaces, model)
    }

    fn draw_surfaces<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame_bind_group: &'a wgpu::BindGroup,
        resources: &'a MapResources,
        mesh: &WorldMesh,
        visible: &[u32],
        model: usize,
    ) -> FrameStats {
        let mut stats = FrameStats {
            surfaces_total: mesh.surfaces.len(),
            ..Default::default()
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
            let wanted_pass = if surface.flags & surf::SKY != 0 {
                Pass::Sky
            } else if surface.lit {
                Pass::World
            } else {
                Pass::Unlit
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

/// How many samples a request for `requested` gets. See [`Renderer::set_msaa`].
pub fn msaa_samples_for(requested: u32) -> u32 {
    if requested > 1 { MSAA_SAMPLES } else { 1 }
}

/// Build every pipeline that draws into the scene target, at one sample count.
fn scene_pipelines(
    device: &wgpu::Device,
    scene: &SceneShaders,
    samples: u32,
) -> HashMap<PipelineKey, wgpu::RenderPipeline> {
    let multisample = wgpu::MultisampleState {
        count: samples,
        mask: !0,
        alpha_to_coverage_enabled: false,
    };
    let hdr_target = [Some(wgpu::ColorTargetState {
        format: HDR_FORMAT,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    })];

    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<WorldVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 24,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 40,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 56,
                shader_location: 5,
                format: wgpu::VertexFormat::Uint32,
            },
        ],
    };

    let mut pipelines = HashMap::new();
    for (pass, entry) in [
        (Pass::World, "fs_world"),
        (Pass::Sky, "fs_sky"),
        (Pass::Unlit, "fs_unlit"),
        (Pass::Decal, "fs_world"),
    ] {
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(entry),
            layout: Some(&scene.layout),
            vertex: wgpu::VertexState {
                module: &scene.world,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&vertex_layout),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &scene.world,
                entry_point: Some(entry),
                targets: &hdr_target,
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                // The mesh builder emits counter-clockwise triangles; see
                // its docs for why the source data is the other way round.
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // The sky is behind everything, so it tests but does not
                // write, letting geometry drawn later sit in front of it. A
                // decal lies on a surface that already wrote its depth.
                depth_write_enabled: !matches!(pass, Pass::Sky | Pass::Decal),
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample,
            multiview: None,
            cache: None,
        });
        pipelines.insert(PipelineKey::from(pass), pipeline);
    }

    // Studio models carry position, normal and uv only -- no lightmap --
    // so they get their own vertex layout and pipeline, while sharing the
    // same bind groups (camera, material, per-model transform).
    let model_vertex_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ModelVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 24,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x2,
            },
            // After the instance attributes' 3 to 7.
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 8,
                format: wgpu::VertexFormat::Uint8x4,
            },
            wgpu::VertexAttribute {
                offset: 36,
                shader_location: 9,
                format: wgpu::VertexFormat::Unorm8x4,
            },
        ],
    };
    let model_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("model"),
        layout: Some(&scene.model_layout),
        vertex: wgpu::VertexState {
            module: &scene.model,
            entry_point: Some("vs_model"),
            buffers: std::slice::from_ref(&model_vertex_layout),
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &scene.model,
            entry_point: Some("fs_model"),
            targets: &hdr_target,
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample,
        multiview: None,
        cache: None,
    });
    pipelines.insert(PipelineKey::from(Pass::Model), model_pipeline);

    let instanced_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("model instanced"),
        layout: Some(&scene.model_layout),
        vertex: wgpu::VertexState {
            module: &scene.model,
            entry_point: Some("vs_model_instanced"),
            buffers: &[
                model_vertex_layout.clone(),
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ModelInstance>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &INSTANCE_ATTRIBUTES,
                },
            ],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &scene.model,
            entry_point: Some("fs_model"),
            targets: &hdr_target,
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample,
        multiview: None,
        cache: None,
    });
    pipelines.insert(PipelineKey::from(Pass::ModelInstanced), instanced_pipeline);

    let line_vertex_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<LineVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
        ],
    };
    let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("line"),
        layout: Some(&scene.line_layout),
        vertex: wgpu::VertexState {
            module: &scene.line,
            entry_point: Some("vs_line"),
            buffers: &[line_vertex_layout],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &scene.line,
            entry_point: Some("fs_line"),
            targets: &hdr_target,
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: false,
            // Lines are an overlay; draw them over the world but keep the
            // depth test so occluded props are visibly behind walls.
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample,
        multiview: None,
        cache: None,
    });
    pipelines.insert(PipelineKey::from(Pass::Lines), line_pipeline);

    pipelines
}

/// A map's cubemap probes on the GPU. One per map, shared by every section's
/// frame bind group.
pub struct GpuProbes {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// How many real probes there are; 0 for the black placeholder.
    pub count: usize,
}

impl GpuProbes {
    /// Upload a map's probes with their mip chain. `None` -- a map without
    /// probes -- uploads a black placeholder no vertex points at.
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cubemaps: Option<&kerosene_bsp::Cubemaps>,
    ) -> GpuProbes {
        let chain = ProbeChain::build(cubemaps);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cubemap probes"),
            size: wgpu::Extent3d {
                width: chain.face_size,
                height: chain.face_size,
                depth_or_array_layers: chain.layers,
            },
            mip_level_count: chain.levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ATLAS_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, texels) in chain.levels.iter().enumerate() {
            let size = chain.level_size(level);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(texels),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * size),
                    rows_per_image: Some(size),
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: chain.layers,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        GpuProbes {
            _texture: texture,
            view,
            count: cubemaps.map_or(0, |c| c.probes.len()),
        }
    }
}

/// GPU resources for one loaded map.
pub struct MapResources {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub lightmap_view: wgpu::TextureView,
    /// One bind group per material, indexed the same way `WorldMesh` does.
    material_bind_groups: Vec<Option<wgpu::BindGroup>>,
    /// Materials that failed to load, so the engine can report them once.
    pub missing_materials: Vec<String>,
    /// Keeps every uploaded texture alive alongside its bind group.
    _textures: Vec<wgpu::Texture>,
    /// And the per-material presence uniforms, for the same reason.
    _buffers: Vec<wgpu::Buffer>,
}

impl MapResources {
    pub fn material_bind_group(&self, index: u32) -> Option<&wgpu::BindGroup> {
        self.material_bind_groups.get(index as usize)?.as_ref()
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
        let mut missing_materials = Vec::new();
        let mut textures = vec![lightmap, fallback];
        let mut buffers = Vec::with_capacity(mesh.materials.len());

        for name in &mesh.materials {
            let loaded = load_material_maps(device, queue, vfs, name);
            if loaded.is_none() {
                missing_materials.push(name.clone());
            }
            let loaded = loaded.unwrap_or_default();

            let (group, buffer) = loaded.bind_group(
                device,
                renderer,
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
            missing_materials,
            _textures: textures,
            _buffers: buffers,
        }
    }
}

/// Decals on the GPU: their materials, and this frame's geometry.
///
/// [`crate::decals`] cuts the geometry; this keeps it in one vertex buffer,
/// re-uploaded only when the set of decals changes, and draws it per
/// section -- each section has its own lightmap atlas, and a decal's
/// lightmap coordinates are the section's it was cut from.
pub struct GpuDecals {
    neutrals: NeutralMaps,
    fallback_view: wgpu::TextureView,
    textures: Vec<wgpu::Texture>,
    materials: HashMap<String, (wgpu::BindGroup, wgpu::Buffer)>,
    /// Materials that would not load, reported once each.
    pub missing: Vec<String>,
    vertices: Option<(wgpu::Buffer, u64)>,
    /// `(section, material, first vertex, vertex count)`.
    draws: Vec<(usize, String, u32, u32)>,
    revision: Option<u64>,
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
                    renderer,
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

/// The 1x1 textures that stand in for maps a material does not have.
///
/// One set per map load rather than one per material: every material without a
/// normal map can point at the same flat blue pixel, and a map with four
/// hundred materials would otherwise make four hundred copies of it.
struct NeutralMaps {
    textures: Vec<wgpu::Texture>,
    views: Vec<wgpu::TextureView>,
}

impl NeutralMaps {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> NeutralMaps {
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

    fn into_textures(self) -> Vec<wgpu::Texture> {
        self.textures
    }
}

/// A material's maps, as far as they loaded.
struct LoadedMaps {
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
        }
    }
}

impl LoadedMaps {
    /// Build the bind group for these maps, filling the gaps with neutrals.
    ///
    /// Takes `textures` to push into: every uploaded texture has to outlive
    /// the bind group that points at it, and the map's resource list is where
    /// they are kept alive.
    fn bind_group(
        self,
        device: &wgpu::Device,
        renderer: &Renderer,
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
                resource: wgpu::BindingResource::Sampler(&renderer.sampler),
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
            layout: &renderer.material_layout,
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
fn load_material_maps(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vfs: &Vfs,
    name: &str,
) -> Option<LoadedMaps> {
    let material_path = kerosene_asset::material_path(name);
    let text = vfs.read_string(&material_path).ok()?;
    let material = Material::parse(&text).ok()?;

    // A sky material's base texture is sampled by direction, but it is loaded
    // exactly the same way as any other.
    let mut loaded = LoadedMaps {
        metalness: material.metalness(),
        roughness_factor: material.roughness_factor(),
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

/// Read one compiled texture and put it on the GPU.
///
/// The texture's own flags decide how: a normal map or a roughness map holds
/// measurements rather than colour, and sampling it through an sRGB transfer
/// would bend every value in it. That intent was recorded at compile time
/// precisely so this decision did not have to be made from the filename.
pub(crate) fn load_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vfs: &Vfs,
    name: &str,
) -> Option<wgpu::Texture> {
    let bytes = vfs.read(&kerosene_asset::texture_path(name)).ok()?;
    let texture = Texture::from_bytes(&bytes).ok()?;

    let format = if texture.flags.is_color() {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };

    // The whole chain, not just level 0: Alchemy compiled the mips so the
    // sampler's trilinear and anisotropic settings have something to pick
    // from. Uploading one level made both a no-op, and every tiled floor
    // shimmer at distance.
    let levels: Vec<(u32, u32, Vec<u8>)> = (0..texture.mip_count())
        .filter_map(|level| {
            let mip = &texture.mips[level];
            texture
                .mip_as_rgba8(level)
                .map(|pixels| (mip.width, mip.height, pixels))
        })
        .collect();
    if levels.is_empty() {
        return None;
    }
    Some(upload_rgba_chain(device, queue, name, &levels, format))
}

/// Upload a full mip chain, largest first, in a chosen format.
fn upload_rgba_chain(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    levels: &[(u32, u32, Vec<u8>)],
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    let (width, height, _) = levels[0];
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (w, h, pixels)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(*h),
            },
            wgpu::Extent3d {
                width: *w,
                height: *h,
                depth_or_array_layers: 1,
            },
        );
    }
    texture
}

/// Upload RGBA8 texels as linear data.
///
/// For what is not material art: the missing-texture checkerboard, which
/// only has to be visible.
fn upload_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> wgpu::Texture {
    upload_rgba_format(
        device,
        queue,
        label,
        width,
        height,
        pixels,
        wgpu::TextureFormat::Rgba8Unorm,
    )
}

/// Upload four-byte texels in a chosen format.
#[allow(clippy::too_many_arguments)]
fn upload_rgba_format(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        size,
    );
    texture
}

/// A checkerboard for materials that will not load.
///
/// Deliberately garish: a missing texture should be obvious in a screenshot,
/// not blend in as a slightly wrong grey.
fn fallback_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    const SIZE: u32 = 32;
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let on = ((x / 8) + (y / 8)) % 2 == 0;
            if on {
                pixels.extend_from_slice(&[255, 0, 220, 255]);
            } else {
                pixels.extend_from_slice(&[20, 20, 20, 255]);
            }
        }
    }
    upload_rgba(device, queue, "missing material", SIZE, SIZE, &pixels)
}

/// One studio model uploaded to the GPU, ready to draw.
pub struct GpuModel {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    /// `(first_index, index_count, material_index)` per mesh.
    meshes: Vec<(u32, u32, u32)>,
    material_bind_groups: Vec<Option<wgpu::BindGroup>>,
    /// Keeps every uploaded texture alive alongside its bind group.
    _textures: Vec<wgpu::Texture>,
    _buffers: Vec<wgpu::Buffer>,
    /// The model's compiled bounds, for culling and hull fitting.
    pub bounds: kerosene_math::Aabb,
}

/// Load and upload a `.keromdl` model by the name an entity refers to it by.
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
                    renderer,
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

#[cfg(test)]
mod tests {
    //! Shader validation without a GPU.
    //!
    //! `naga` is the same compiler wgpu uses internally, so parsing and
    //! validating the WGSL here catches exactly the errors that would
    //! otherwise only surface at pipeline creation on a machine with a
    //! display -- which is a slow way to find a typo.

    const WORLD_WGSL: &str = include_str!("shaders/world.wgsl");
    const MODEL_WGSL: &str = include_str!("shaders/model.wgsl");
    const LINE_WGSL: &str = include_str!("shaders/line.wgsl");
    const TONEMAP_WGSL: &str = include_str!("shaders/tonemap.wgsl");
    const SHADOW_WGSL: &str = include_str!("shaders/shadow.wgsl");
    const UI_WGSL: &str = include_str!("shaders/ui.wgsl");
    const PANEL_WGSL: &str = include_str!("shaders/panel.wgsl");

    #[test]
    fn the_ui_shaders_compile() {
        validate("ui.wgsl", UI_WGSL);
        validate("panel.wgsl", PANEL_WGSL);
    }

    #[test]
    fn the_ui_quad_matches_what_the_shader_declares() {
        // Nine attributes: six vec4s, a uvec4 and two vec3s.
        assert_eq!(std::mem::size_of::<crate::ui::GpuQuad>(), 7 * 16 + 2 * 12);
    }

    fn validate(name: &str, source: &str) -> naga::valid::ModuleInfo {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|e| panic!("{name} failed to parse:\n{}", e.emit_to_string(source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name} failed validation: {e:?}"))
    }

    #[test]
    fn the_world_shader_compiles() {
        validate("world.wgsl", WORLD_WGSL);
    }

    #[test]
    fn the_model_shader_compiles() {
        validate("model.wgsl", MODEL_WGSL);
    }

    #[test]
    fn the_line_shader_compiles() {
        validate("line.wgsl", LINE_WGSL);
    }

    #[test]
    fn the_shadow_shader_compiles() {
        let module = naga::front::wgsl::parse_str(SHADOW_WGSL).expect("parses");
        assert!(module.entry_points.iter().any(|e| e.name == "vs_shadow"));
        validate("shadow.wgsl", SHADOW_WGSL);
    }

    #[test]
    fn the_tonemap_shader_compiles() {
        let module = naga::front::wgsl::parse_str(TONEMAP_WGSL).expect("parses");
        let names: Vec<&str> = module
            .entry_points
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert!(names.contains(&"vs_fullscreen") && names.contains(&"fs_tonemap"));
        validate("tonemap.wgsl", TONEMAP_WGSL);
    }

    #[test]
    fn every_entry_point_the_pipelines_ask_for_exists() {
        let module = naga::front::wgsl::parse_str(WORLD_WGSL).expect("parses");
        let names: Vec<&str> = module
            .entry_points
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        for wanted in ["vs_main", "fs_world", "fs_sky", "fs_unlit"] {
            assert!(
                names.contains(&wanted),
                "missing entry point {wanted}; have {names:?}"
            );
        }
        let module = naga::front::wgsl::parse_str(MODEL_WGSL).expect("parses");
        let names: Vec<&str> = module
            .entry_points
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        for wanted in ["vs_model", "fs_model"] {
            assert!(
                names.contains(&wanted),
                "missing entry point {wanted}; have {names:?}"
            );
        }
        let module = naga::front::wgsl::parse_str(LINE_WGSL).expect("parses");
        let names: Vec<&str> = module
            .entry_points
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        for wanted in ["vs_line", "fs_line"] {
            assert!(
                names.contains(&wanted),
                "missing entry point {wanted}; have {names:?}"
            );
        }
    }

    #[test]
    fn the_camera_uniform_matches_what_the_shader_declares() {
        // A mismatch here writes the wrong bytes into the wrong fields and
        // produces a picture that is subtly, inexplicably wrong.
        assert_eq!(
            std::mem::size_of::<super::CameraUniform>(),
            64 + 16 + 16 + 16 + 16
        );
    }

    #[test]
    fn the_vertex_layout_matches_the_mesh_vertex() {
        use crate::mesh::WorldVertex;
        assert_eq!(std::mem::size_of::<WorldVertex>(), 60);
        // The attribute offsets in `Renderer::new` assume this layout.
        assert_eq!(std::mem::offset_of!(WorldVertex, position), 0);
        assert_eq!(std::mem::offset_of!(WorldVertex, normal), 12);
        assert_eq!(std::mem::offset_of!(WorldVertex, uv), 24);
        assert_eq!(std::mem::offset_of!(WorldVertex, lightmap_uv), 32);
        assert_eq!(std::mem::offset_of!(WorldVertex, tangent), 40);
        assert_eq!(std::mem::offset_of!(WorldVertex, probe), 56);
    }

    #[test]
    fn the_material_uniform_matches_what_the_shader_declares() {
        // Both shaders declare this struct; getting it wrong here binds
        // `normal_strength` where `present` should be and turns every map off
        // at once, or on at once, depending on the float.
        assert_eq!(std::mem::size_of::<super::MaterialUniform>(), 32);
        assert_eq!(std::mem::offset_of!(super::MaterialUniform, present), 0);
        assert_eq!(
            std::mem::offset_of!(super::MaterialUniform, emissive_strength),
            4
        );
        assert_eq!(
            std::mem::offset_of!(super::MaterialUniform, normal_strength),
            8
        );
        assert_eq!(
            std::mem::offset_of!(super::MaterialUniform, specular_strength),
            12
        );
        assert_eq!(std::mem::offset_of!(super::MaterialUniform, metalness), 16);
        assert_eq!(
            std::mem::offset_of!(super::MaterialUniform, roughness_factor),
            20
        );
    }

    #[test]
    fn the_model_uniform_matches_what_the_shaders_declare() {
        assert_eq!(std::mem::size_of::<super::ModelUniform>(), 80);
        assert_eq!(std::mem::offset_of!(super::ModelUniform, probe), 64);
        assert_eq!(super::ModelUniform::default().probe[0], crate::NO_PROBE);
    }

    #[test]
    fn the_tonemap_uniform_matches_what_the_shader_declares() {
        assert_eq!(std::mem::size_of::<super::ToneMapUniform>(), 16);
        assert_eq!(std::mem::offset_of!(super::ToneMapUniform, exposure), 0);
        assert_eq!(std::mem::offset_of!(super::ToneMapUniform, curve), 4);
    }

    #[test]
    fn msaa_is_off_or_four_samples() {
        assert_eq!(super::msaa_samples_for(0), 1);
        assert_eq!(super::msaa_samples_for(1), 1);
        assert_eq!(super::msaa_samples_for(2), super::MSAA_SAMPLES);
        assert_eq!(super::msaa_samples_for(16), super::MSAA_SAMPLES);
    }

    #[test]
    fn tonemap_operators_come_from_the_convar_by_index() {
        use super::ToneMapOperator;
        assert_eq!(ToneMapOperator::from_index(0), ToneMapOperator::None);
        assert_eq!(ToneMapOperator::from_index(1), ToneMapOperator::Reinhard);
        assert_eq!(ToneMapOperator::from_index(2), ToneMapOperator::Aces);
        assert_eq!(ToneMapOperator::from_index(99), ToneMapOperator::Aces);
        assert_eq!(ToneMapOperator::default(), ToneMapOperator::Aces);
    }

    #[test]
    fn the_renderers_map_order_matches_the_asset_crates() {
        // The shader indexes maps by bit position, so these two lists being
        // in different orders would bind roughness where the emissive map
        // should be -- and look like an art bug, not a code one.
        assert_eq!(super::MAP_COUNT, kerosene_asset::MapKind::ALL.len());
        assert_eq!(super::MAP_KINDS, kerosene_asset::MapKind::ALL);
    }

    #[test]
    fn a_material_with_no_maps_turns_every_optional_path_off() {
        // The regression that matters most: an albedo-only material -- which
        // is every material written before texture sets existed -- must take
        // none of the new branches.
        let uniform = super::MaterialUniform::default();
        assert_eq!(uniform.present, 0);
        for slot in 0..super::MAP_COUNT {
            assert_eq!(uniform.present & (1 << slot), 0);
        }
    }

    #[test]
    fn the_model_vertex_layout_matches_what_the_pipeline_declares() {
        use super::ModelVertex;
        assert_eq!(std::mem::size_of::<ModelVertex>(), 40);
        assert_eq!(
            std::mem::size_of::<ModelVertex>(),
            std::mem::size_of::<kerosene_asset::Vertex>(),
            "the same layout as the file, so upload is a copy"
        );
        assert_eq!(std::mem::offset_of!(ModelVertex, bone_indices), 32);
        assert_eq!(std::mem::offset_of!(ModelVertex, bone_weights), 36);
        assert_eq!(std::mem::offset_of!(ModelVertex, position), 0);
        assert_eq!(std::mem::offset_of!(ModelVertex, normal), 12);
        assert_eq!(std::mem::offset_of!(ModelVertex, uv), 24);
    }

    #[test]
    fn the_line_vertex_layout_matches_what_the_pipeline_declares() {
        use super::LineVertex;
        assert_eq!(std::mem::size_of::<LineVertex>(), 24);
        assert_eq!(std::mem::offset_of!(LineVertex, position), 0);
        assert_eq!(std::mem::offset_of!(LineVertex, color), 12);
    }
}
