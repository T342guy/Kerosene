// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The wgpu layer.
//!
//! Deliberately thin: every decision about *what* to draw has already been
//! made by [`crate::mesh::WorldMesh::visible_surfaces`], so this is buffer
//! management, pipeline setup, and a draw loop.
//!
//! Materials each get their own bind group, and surfaces arrive sorted by
//! material, so the loop rebinds only when the material actually changes.

use crate::camera::Camera;
use crate::lightmap::{ATLAS_SIZE, LightmapAtlas};
use crate::mesh::{WorldMesh, WorldVertex};
use crate::FrameStats;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use kerosene_asset::{Material, Model, Shader, Texture};
use kerosene_bsp::surf;
use kerosene_math::{Mat4, Pose, Vec3};
use kerosene_vfs::Vfs;
use wgpu::util::DeviceExt;

/// A vertex in a studio model, as uploaded to the GPU.
///
/// Skinning data is not uploaded: rigid rendering uses bone 0 (the identity),
/// which is all a physics prop needs. Position, normal and uv only.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct ModelVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

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
    /// `[exposure, time, lightmaps_enabled, fullbright]`.
    pub params: [f32; 4],
    pub sky_color: [f32; 4],
}

impl CameraUniform {
    pub fn from_camera(camera: &Camera, exposure: f32, time: f32) -> Self {
        CameraUniform {
            view_proj: camera.view_projection().to_cols_array_2d(),
            position: camera.position.extend(1.0).to_array(),
            params: [exposure, time, 1.0, 0.0],
            sky_color: [1.0, 1.0, 1.0, 1.0],
        }
    }

    pub fn set_lightmaps(&mut self, on: bool) { self.params[2] = if on { 1.0 } else { 0.0 }; }
    pub fn set_fullbright(&mut self, on: bool) { self.params[3] = if on { 1.0 } else { 0.0 }; }
    pub fn set_sky_color(&mut self, c: Vec3) { self.sky_color = c.extend(1.0).to_array(); }
}

/// Anisotropic filtering samples. 16 is the usual maximum and is supported
/// everywhere wgpu runs; a device that cannot manage it clamps down rather
/// than failing.
const MAX_ANISOTROPY: u16 = 16;

/// The depth format. 32-bit float because a Source-scale map spans 32768
/// units, and 24-bit depth z-fights visibly at that range.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Which pipeline draws a surface.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pass {
    World,
    Sky,
    Unlit,
    /// A studio model, drawn without a lightmap.
    Model,
    /// Debug wireframe lines.
    Lines,
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
}

impl Default for ModelUniform {
    fn default() -> Self {
        ModelUniform { transform: Mat4::IDENTITY.to_cols_array_2d() }
    }
}

impl From<Pose> for ModelUniform {
    fn from(pose: Pose) -> Self {
        ModelUniform { transform: pose.to_mat4().to_cols_array_2d() }
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
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    pub frame_layout: wgpu::BindGroupLayout,
    pub material_layout: wgpu::BindGroupLayout,
    camera_buffer: wgpu::Buffer,
    /// One [`ModelUniform`] per brush model, indexed by a dynamic offset.
    model_buffer: wgpu::Buffer,
    model_bind_group: wgpu::BindGroup,
    /// Distance between two entries in `model_buffer`, honouring the device's
    /// uniform alignment.
    model_stride: u32,
    sampler: wgpu::Sampler,
    lightmap_sampler: wgpu::Sampler,
    depth: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    format: wgpu::TextureFormat,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct PipelineKey(u8);

impl From<Pass> for PipelineKey {
    fn from(p: Pass) -> Self { PipelineKey(p as u8) }
}

impl Renderer {
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
            ],
        });

        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
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
                        std::mem::size_of::<ModelUniform>() as u64,
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

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<WorldVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 24, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 32, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
            ],
        };

        let mut pipelines = HashMap::new();
        for (pass, entry) in [(Pass::World, "fs_world"), (Pass::Sky, "fs_sky"), (Pass::Unlit, "fs_unlit")] {
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[vertex_layout.clone()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
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
                    // write, letting geometry drawn later sit in front of it.
                    depth_write_enabled: pass != Pass::Sky,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
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
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 24, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
            ],
        };
        let model_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("model"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &model_shader,
                entry_point: Some("vs_model"),
                buffers: &[model_vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &model_shader,
                entry_point: Some("fs_model"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
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
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        pipelines.insert(PipelineKey::from(Pass::Model), model_pipeline);

        // Debug lines: the same camera uniform, a line-list topology, and a
        // colour straight through. Only the camera is bound.
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("line"),
            bind_group_layouts: &[&frame_layout],
            push_constant_ranges: &[],
        });
        let line_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
            ],
        };
        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("line"),
            layout: Some(&line_layout),
            vertex: wgpu::VertexState {
                module: &line_shader,
                entry_point: Some("vs_line"),
                buffers: &[line_vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: Some("fs_line"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
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
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        pipelines.insert(PipelineKey::from(Pass::Lines), line_pipeline);

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

        Renderer {
            pipelines,
            frame_layout,
            material_layout,
            camera_buffer,
            model_buffer,
            model_bind_group,
            model_stride,
            sampler,
            lightmap_sampler,
            depth: None,
            format,
        }
    }

    pub fn format(&self) -> wgpu::TextureFormat { self.format }

    /// Create or resize the depth buffer.
    pub fn ensure_depth(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if let Some((_, _, w, h)) = &self.depth {
            if *w == width && *h == height { return; }
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth = Some((texture, view, width, height));
    }

    pub fn depth_view(&self) -> Option<&wgpu::TextureView> {
        self.depth.as_ref().map(|(_, v, _, _)| v)
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
        let mut data = vec![ModelUniform::default(); MAX_MODELS];
        for (i, pose) in poses.iter().enumerate().take(MAX_MODELS) {
            data[i] = ModelUniform::from(*pose);
        }
        // Written as one span with the device's stride between entries, so
        // the same buffer can be addressed by dynamic offset.
        let stride = self.model_stride as usize;
        let mut bytes = vec![0u8; stride * MAX_MODELS];
        for (i, entry) in data.iter().enumerate() {
            let at = i * stride;
            bytes[at..at + std::mem::size_of::<ModelUniform>()]
                .copy_from_slice(bytemuck::bytes_of(entry));
        }
        queue.write_buffer(&self.model_buffer, 0, &bytes);
    }

    /// The dynamic offset that addresses one model's entry.
    fn model_offset(&self, model: usize) -> u32 {
        self.model_stride * model.min(MAX_MODELS - 1) as u32
    }

    /// Draw one uploaded studio model, at the pose in model slot `slot`.
    ///
    /// Physics props and other dynamic models are drawn through here: the
    /// geometry comes from a `.keromdl` (not from the BSP), and the transform
    /// comes from the same model buffer the brush models use.
    pub fn draw_studio_model<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame_bind_group: &'a wgpu::BindGroup,
        gpu_model: &'a GpuModel,
        slot: usize,
    ) -> FrameStats {
        let mut stats = FrameStats::default();
        if gpu_model.meshes.is_empty() { return stats; }

        pass.set_bind_group(0, frame_bind_group, &[]);
        pass.set_bind_group(2, &self.model_bind_group, &[self.model_offset(slot)]);
        pass.set_vertex_buffer(0, gpu_model.vertex_buffer.slice(..));
        pass.set_index_buffer(gpu_model.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_pipeline(&self.pipelines[&PipelineKey::from(Pass::Model)]);

        let mut current_material = u32::MAX;
        for &(first, count, material) in &gpu_model.meshes {
            if material != current_material {
                if let Some(group) = gpu_model
                    .material_bind_groups
                    .get(material as usize)
                    .and_then(|g| g.as_ref())
                {
                    pass.set_bind_group(1, group, &[]);
                    current_material = material;
                }
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
        if vertex_count == 0 { return; }
        pass.set_pipeline(&self.pipelines[&PipelineKey::from(Pass::Lines)]);
        pass.set_bind_group(0, frame_bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.draw(0..vertex_count, 0..1);
    }

    pub fn create_frame_bind_group(
        &self,
        device: &wgpu::Device,
        lightmap_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &self.frame_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.camera_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(lightmap_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.lightmap_sampler) },
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
        let Some(surfaces) = mesh.model_surfaces.get(model) else { return FrameStats::default() };
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
        if visible.is_empty() { return stats; }

        pass.set_bind_group(0, frame_bind_group, &[]);
        pass.set_bind_group(2, &self.model_bind_group, &[self.model_offset(model)]);
        pass.set_vertex_buffer(0, resources.vertices.slice(..));
        pass.set_index_buffer(resources.indices.slice(..), wgpu::IndexFormat::Uint32);

        let mut current_material = u32::MAX;
        let mut current_pass: Option<Pass> = None;
        // Accumulate adjacent surfaces into one draw.
        let mut run: Option<(u32, u32)> = None;

        let flush = |pass: &mut wgpu::RenderPass<'a>, run: &mut Option<(u32, u32)>, stats: &mut FrameStats| {
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
                let Some(pipeline) = self.pipelines.get(&PipelineKey::from(wanted_pass)) else { continue };
                pass.set_pipeline(pipeline);
                current_pass = Some(wanted_pass);
                // A pipeline change invalidates nothing about bindings, but
                // the material must be re-bound after the first set_pipeline.
                current_material = u32::MAX;
            }

            if surface.material != current_material {
                flush(pass, &mut run, &mut stats);
                let Some(bind_group) = resources.material_bind_group(surface.material) else { continue };
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

        let lightmap = upload_rgba(
            device,
            queue,
            "lightmap atlas",
            ATLAS_SIZE,
            ATLAS_SIZE,
            &atlas.pixels,
        );
        let lightmap_view = lightmap.create_view(&wgpu::TextureViewDescriptor::default());

        // A flat texture stands in for anything that will not load, so a
        // missing material is a visibly wrong surface rather than a crash.
        let fallback = fallback_texture(device, queue);
        let fallback_view = fallback.create_view(&wgpu::TextureViewDescriptor::default());

        let mut material_bind_groups = Vec::with_capacity(mesh.materials.len());
        let mut missing_materials = Vec::new();
        let mut textures = vec![lightmap, fallback];

        for name in &mesh.materials {
            let loaded = load_material_texture(device, queue, vfs, name);
            let view = match loaded {
                Some(texture) => {
                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    textures.push(texture);
                    view
                }
                None => {
                    missing_materials.push(name.clone());
                    fallback_view.clone()
                }
            };
            material_bind_groups.push(Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(name),
                layout: &renderer.material_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&renderer.sampler) },
                ],
            })));
        }

        MapResources {
            vertices,
            indices,
            lightmap_view,
            material_bind_groups,
            missing_materials,
            _textures: textures,
        }
    }
}

/// Load a material and its base texture through the VFS.
fn load_material_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vfs: &Vfs,
    name: &str,
) -> Option<wgpu::Texture> {
    let material_path = kerosene_asset::material_path(name);
    let text = vfs.read_string(&material_path).ok()?;
    let material = Material::parse(&text).ok()?;

    // A sky material's base texture is sampled by direction, but it is loaded
    // exactly the same way.
    let _ = matches!(material.shader, Shader::Sky);

    let texture_name = material.base_texture().unwrap_or(name);
    let bytes = vfs.read(&kerosene_asset::texture_path(texture_name)).ok()?;
    let texture = Texture::from_bytes(&bytes).ok()?;
    let pixels = texture.mip_as_rgba8(0)?;

    Some(upload_rgba(device, queue, name, texture.width(), texture.height(), &pixels))
}

fn upload_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> wgpu::Texture {
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
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
        .map(|v| ModelVertex { position: v.position, normal: v.normal, uv: v.uv })
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
    let mut textures = vec![fallback];

    let mut name_to_group: HashMap<String, u32> = HashMap::new();
    let mut material_bind_groups: Vec<Option<wgpu::BindGroup>> = Vec::new();
    let mut meshes = Vec::with_capacity(model.meshes.len());

    for i in 0..model.meshes.len() {
        let material_name = model.mesh_material(i).to_string();
        let material_index = match name_to_group.get(&material_name) {
            Some(&idx) => idx,
            None => {
                let view = match load_material_texture(device, queue, vfs, &material_name) {
                    Some(texture) => {
                        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                        textures.push(texture);
                        view
                    }
                    None => fallback_view.clone(),
                };
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&material_name),
                    layout: &renderer.material_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&renderer.sampler) },
                    ],
                });
                let idx = material_bind_groups.len() as u32;
                material_bind_groups.push(Some(group));
                name_to_group.insert(material_name, idx);
                idx
            }
        };
        let mesh = &model.meshes[i];
        meshes.push((mesh.first_index, mesh.index_count, material_index));
    }

    Some(GpuModel {
        vertex_buffer,
        index_buffer,
        meshes,
        material_bind_groups,
        _textures: textures,
        bounds: model.bounds,
    })
}

/// The view matrix a camera would use, exposed for tools that want it without
/// building a whole renderer.
pub fn view_projection(camera: &Camera) -> Mat4 { camera.view_projection() }

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
    fn every_entry_point_the_pipelines_ask_for_exists() {
        let module = naga::front::wgsl::parse_str(WORLD_WGSL).expect("parses");
        let names: Vec<&str> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
        for wanted in ["vs_main", "fs_world", "fs_sky", "fs_unlit"] {
            assert!(names.contains(&wanted), "missing entry point {wanted}; have {names:?}");
        }
        let module = naga::front::wgsl::parse_str(MODEL_WGSL).expect("parses");
        let names: Vec<&str> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
        for wanted in ["vs_model", "fs_model"] {
            assert!(names.contains(&wanted), "missing entry point {wanted}; have {names:?}");
        }
        let module = naga::front::wgsl::parse_str(LINE_WGSL).expect("parses");
        let names: Vec<&str> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
        for wanted in ["vs_line", "fs_line"] {
            assert!(names.contains(&wanted), "missing entry point {wanted}; have {names:?}");
        }
    }

    #[test]
    fn the_camera_uniform_matches_what_the_shader_declares() {
        // A mismatch here writes the wrong bytes into the wrong fields and
        // produces a picture that is subtly, inexplicably wrong.
        assert_eq!(std::mem::size_of::<super::CameraUniform>(), 64 + 16 + 16 + 16);
    }

    #[test]
    fn the_vertex_layout_matches_the_mesh_vertex() {
        use crate::mesh::WorldVertex;
        assert_eq!(std::mem::size_of::<WorldVertex>(), 40);
        // The attribute offsets in `Renderer::new` assume this layout.
        assert_eq!(std::mem::offset_of!(WorldVertex, position), 0);
        assert_eq!(std::mem::offset_of!(WorldVertex, normal), 12);
        assert_eq!(std::mem::offset_of!(WorldVertex, uv), 24);
        assert_eq!(std::mem::offset_of!(WorldVertex, lightmap_uv), 32);
    }

    #[test]
    fn the_model_vertex_layout_matches_what_the_pipeline_declares() {
        use super::ModelVertex;
        assert_eq!(std::mem::size_of::<ModelVertex>(), 32);
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
