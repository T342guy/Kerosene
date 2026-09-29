// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The plain data the shaders read: vertices, uniforms, formats and passes.
use kerosene_rhi::wgpu;

use super::*;

/// A vertex in a studio model, as uploaded to the GPU.
///
/// The same forty bytes as a `.kmdl` vertex, bone influences included: a
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
pub(super) const PALETTE_BYTES: u64 = (kerosene_asset::MAX_BONES * 64) as u64;

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
pub(super) const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = [
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
    /// The display's gamma adjustment, `mat_gamma`: 1 changes nothing, more
    /// lifts the mid-tones -- a brightness slider that leaves black black
    /// and white white.
    pub gamma: f32,
    pub _pad: f32,
}

impl Default for ToneMapUniform {
    fn default() -> Self {
        ToneMapUniform {
            exposure: 1.0,
            curve: ToneMapOperator::default() as u32,
            gamma: 1.0,
            _pad: 0.0,
        }
    }
}

/// Which pipeline draws a surface.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Pass {
    World,
    Sky,
    Unlit,
    /// The world's shading, seen from both sides: `$nocull`.
    WorldTwoSided,
    UnlitTwoSided,
    /// Blended over what is already drawn, writing no depth: `$translucent`.
    Translucent,
    TranslucentUnlit,
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
