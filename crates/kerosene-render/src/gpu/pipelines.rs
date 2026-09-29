// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The scene's render pipelines.
use kerosene_rhi::wgpu;

use super::*;

/// How many samples a request for `requested` gets. See [`Renderer::set_msaa`].
pub fn msaa_samples_for(requested: u32) -> u32 {
    if requested > 1 { MSAA_SAMPLES } else { 1 }
}

/// Build every pipeline that draws into the scene target, at one sample count.
pub(super) fn scene_pipelines(
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
        (Pass::WorldTwoSided, "fs_world"),
        (Pass::UnlitTwoSided, "fs_unlit"),
        (Pass::Translucent, "fs_world"),
        (Pass::TranslucentUnlit, "fs_unlit"),
    ] {
        let two_sided = matches!(
            pass,
            Pass::WorldTwoSided | Pass::UnlitTwoSided | Pass::Translucent | Pass::TranslucentUnlit
        );
        let translucent = matches!(pass, Pass::Translucent | Pass::TranslucentUnlit);
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
                cull_mode: (!two_sided).then_some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // The sky is behind everything, so it tests but does not
                // write, letting geometry drawn later sit in front of it. A
                // decal lies on a surface that already wrote its depth, and
                // glass must not hide what is behind it from later glass.
                depth_write_enabled: !matches!(pass, Pass::Sky | Pass::Decal) && !translucent,
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
