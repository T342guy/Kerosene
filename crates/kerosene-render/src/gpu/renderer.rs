// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The [`Renderer`]: its GPU state, and how it is built.
use kerosene_rhi::wgpu;

use super::*;

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
    pub(super) pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    /// What the scene pipelines are built from, kept so a change of sample
    /// count can rebuild them without recompiling a shader.
    pub(super) scene: SceneShaders,
    pub frame_layout: wgpu::BindGroupLayout,
    /// The layout and sampler every material binds through.
    pub materials: MaterialBindings,
    pub(super) camera_buffer: wgpu::Buffer,
    /// One [`ModelUniform`] per brush model, indexed by a dynamic offset.
    pub(super) model_buffer: wgpu::Buffer,
    /// CPU-side span the poses are laid out in before upload, kept so it is
    /// not reallocated every frame. A mutex only because `update_models`
    /// takes `&self`; it is never contended.
    pub(super) model_staging: std::sync::Mutex<Vec<u8>>,
    pub(super) model_bind_group: wgpu::BindGroup,
    /// Distance between two entries in `model_buffer`, honouring the device's
    /// uniform alignment.
    pub(super) model_stride: u32,
    pub(super) lightmap_sampler: wgpu::Sampler,
    /// Trilinear and clamped: the mip level is how blurry a reflection is,
    /// and a face must not wrap round to its own far edge.
    pub(super) probe_sampler: wgpu::Sampler,
    /// Samples per pixel in the scene target: 1, or [`MSAA_SAMPLES`].
    pub(super) samples: u32,
    /// The scene's colour and depth, sized to the window. `None` until the
    /// first [`Renderer::ensure_targets`] and after a change of sample count.
    pub(super) targets: Option<Targets>,
    pub(super) tonemap_pipeline: wgpu::RenderPipeline,
    pub(super) tonemap_layout: wgpu::BindGroupLayout,
    pub(super) tonemap_buffer: wgpu::Buffer,
    /// The swapchain's format: what the tone-map pass writes.
    pub(super) format: wgpu::TextureFormat,
    /// This frame's dynamic lights, and which cluster each can reach.
    pub(super) lights_buffer: wgpu::Buffer,
    pub(super) clusters_buffer: wgpu::Buffer,
    /// Every shadow layer, as one array the scene samples...
    pub(super) shadow_array: wgpu::TextureView,
    /// ...and each layer on its own, to render into.
    pub(super) shadow_layers: Vec<wgpu::TextureView>,
    pub(super) _shadow_texture: wgpu::Texture,
    pub(super) shadow_sampler: wgpu::Sampler,
    /// One light view-projection per shadow layer, by dynamic offset.
    pub(super) shadow_view_buffer: wgpu::Buffer,
    pub(super) shadow_view_bind_group: wgpu::BindGroup,
    pub(super) shadow_view_stride: u32,
    /// Depth-only pipelines for world geometry and studio models.
    pub(super) shadow_world_pipeline: wgpu::RenderPipeline,
    pub(super) shadow_model_pipeline: wgpu::RenderPipeline,
    pub(super) shadow_instanced_pipeline: wgpu::RenderPipeline,
    /// Every static prop's [`ModelInstance`] this frame, grown as needed.
    pub(super) instance_buffer: Option<wgpu::Buffer>,
    /// Bone palettes: slot 0 the identity, then one per animated model.
    pub(super) bones_buffer: wgpu::Buffer,
    pub(super) bones_bind_group: wgpu::BindGroup,
    pub(super) palette_stride: u32,
}

/// The scene shaders and layouts, everything a pipeline needs except a
/// sample count.
pub(super) struct SceneShaders {
    pub(super) world: wgpu::ShaderModule,
    pub(super) model: wgpu::ShaderModule,
    pub(super) line: wgpu::ShaderModule,
    pub(super) layout: wgpu::PipelineLayout,
    /// The world's layout plus the bone palettes, for studio models.
    pub(super) model_layout: wgpu::PipelineLayout,
    pub(super) line_layout: wgpu::PipelineLayout,
}

/// The render targets a frame is drawn into before tone-mapping.
pub(super) struct Targets {
    pub(super) width: u32,
    pub(super) height: u32,
    /// The multisampled colour target, when there is one. Resolved into
    /// `resolved` at the end of the scene pass, and never read otherwise.
    pub(super) multisampled: Option<wgpu::TextureView>,
    /// The single-sampled HDR colour the tone-map pass reads.
    pub(super) resolved: wgpu::TextureView,
    pub(super) depth: wgpu::TextureView,
    pub(super) tonemap_bind_group: wgpu::BindGroup,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct PipelineKey(u8);

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
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/world.wgsl").into()),
        });
        let model_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("model"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/model.wgsl").into()),
        });
        let line_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("line"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/line.wgsl").into()),
        });
        let tonemap_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tonemap"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/tonemap.wgsl").into()),
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

        let materials = MaterialBindings::new(device);

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
            bind_group_layouts: &[&frame_layout, &materials.layout, &model_layout],
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
            vec![Mat4::IDENTITY.to_cols_array_2d(); kerosene_asset::MAX_BONES];
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
                    &materials.layout,
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
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/shadow.wgsl").into()),
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
            materials,
            camera_buffer,
            model_buffer,
            model_staging: std::sync::Mutex::new(Vec::new()),
            model_bind_group,
            model_stride,
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
}
