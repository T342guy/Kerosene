// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The 3D pane on the GPU.
//!
//! The software rasteriser in [`crate::raster`] was right by construction
//! and slow by construction: a whole map re-rasterised on the CPU whenever
//! the camera moved a pixel. Fine for a room, not for a level, and it left
//! no room for what an editor's 3D view is actually for -- models where
//! props stand, cones where lights point, a selection you can see through a
//! wall.
//!
//! So the pane renders here, through wgpu, in the window the toolset
//! already draws egui into. Each pane is drawn into its own multisampled
//! target with a depth buffer, then copied into egui's pass inside an
//! [`egui_wgpu::Callback`], which is how egui lets a widget draw with the
//! device directly.
//!
//! The renderer is deliberately separate from `kerosene-render`'s. That one
//! draws a *compiled* map -- BSP faces, lightmaps, probes -- and an editor
//! draws a map that has not been compiled and changes on every drag. What
//! they share is the device.
//!
//! The rasteriser stays: for tests, for thumbnails, and for a toolset that
//! came up without a GPU callback path.

pub mod scene;

use scene::{Batch, LineVertex, Scene, Vertex};
use std::collections::HashMap;
use std::sync::Arc;

use kerosene_math::{Basis, Mat4, Vec3, Vec4};

/// How far the far plane is. Past the legal world from any point in it.
pub const FAR: f32 = 131_072.0;

/// Samples per pixel in a pane. Four is guaranteed for the formats used.
const SAMPLES: u32 = 4;

/// The format panes render in. sRGB, so blending happens in linear light.
const PANE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The matrix taking world space to clip space for a pane's camera.
///
/// Built from the same half-angles [`crate::viewport::Viewport::pick_ray`]
/// uses, so a click lands on what the pane drew under it.
pub fn view_projection(eye: Vec3, basis: Basis, fov: f32, aspect: f32) -> Mat4 {
    let aspect = aspect.max(1e-4);
    let half_y = (kerosene_render::vertical_fov(fov, aspect) * 0.5)
        .tan()
        .max(1e-4);
    let half_x = half_y * aspect;
    let near = crate::draw::NEAR;
    let a = FAR / (FAR - near);
    let b = -FAR * near / (FAR - near);

    let row = |v: Vec3, scale: f32, offset: f32| {
        Vec4::new(
            v.x * scale,
            v.y * scale,
            v.z * scale,
            -v.dot(eye) * scale + offset,
        )
    };
    let rows = [
        row(basis.right, 1.0 / half_x, 0.0),
        row(basis.up, 1.0 / half_y, 0.0),
        row(basis.forward, a, b),
        row(basis.forward, 1.0, 0.0),
    ];
    Mat4::from_cols(rows[0], rows[1], rows[2], rows[3]).transpose()
}

/// The camera as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
}

impl CameraUniform {
    pub fn new(eye: Vec3, basis: Basis, fov: f32, aspect: f32) -> CameraUniform {
        CameraUniform {
            view_proj: view_projection(eye, basis, fov, aspect).to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 1.0],
        }
    }
}

/// A scene, with the number that says whether it is the one already on the
/// GPU.
#[derive(Clone)]
pub struct SharedScene {
    pub generation: u64,
    pub scene: Arc<Scene>,
    /// Changes when the editor's textures were reloaded, so uploaded copies
    /// of the old ones are dropped rather than drawn forever.
    pub texture_epoch: u64,
}

/// One frame of one pane, handed to egui to draw.
pub struct PaneCallback {
    pub pane: usize,
    /// Size in physical pixels.
    pub size: [u32; 2],
    pub camera: CameraUniform,
    pub scene: SharedScene,
    /// What egui's pass renders into, so the blit pipeline matches it.
    pub target: wgpu::TextureFormat,
    /// Linear RGB of the empty background.
    pub background: [f32; 3],
}

impl egui_wgpu::CallbackTrait for PaneCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if resources.get::<EditorRenderer>().is_none() {
            resources.insert(EditorRenderer::new(device, queue, self.target));
        }
        let renderer = resources
            .get_mut::<EditorRenderer>()
            .expect("inserted above");
        renderer.upload_scene(device, queue, &self.scene);
        renderer.render_pane(
            device,
            queue,
            encoder,
            self.pane,
            self.size,
            &self.camera,
            self.background,
        );
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(renderer) = resources.get::<EditorRenderer>() {
            renderer.blit(pass, self.pane);
        }
    }
}

struct GpuTexture {
    bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
}

struct GpuScene {
    generation: u64,
    triangles: Option<wgpu::Buffer>,
    lines: Option<wgpu::Buffer>,
    line_count: u32,
    xray: Option<wgpu::Buffer>,
    xray_count: u32,
    batches: Vec<Batch>,
}

struct Pane {
    size: [u32; 2],
    color: wgpu::TextureView,
    resolve: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    depth: wgpu::TextureView,
    camera: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    blit_group: wgpu::BindGroup,
}

/// The pipelines, the uploaded scene and each pane's targets.
///
/// Lives in egui's callback resources for as long as the window does.
pub struct EditorRenderer {
    target: wgpu::TextureFormat,
    /// Indexed by `translucent as usize * 2 + two_sided as usize`.
    faces: [wgpu::RenderPipeline; 4],
    lines: wgpu::RenderPipeline,
    xray: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    blit_sampler: wgpu::Sampler,
    white: GpuTexture,
    textures: HashMap<String, GpuTexture>,
    texture_epoch: u64,
    scene: Option<GpuScene>,
    panes: HashMap<usize, Pane>,
}

impl EditorRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: wgpu::TextureFormat,
    ) -> EditorRenderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chisel-editor"),
            source: wgpu::ShaderSource::Wgsl(include_str!("editor.wgsl").into()),
        });

        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chisel-camera"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_entries = [
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
        ];
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chisel-texture"),
            entries: &texture_entries,
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chisel-blit"),
            entries: &texture_entries,
        });

        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("chisel-scene"),
            bind_group_layouts: &[&camera_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("chisel-lines"),
            bind_group_layouts: &[&camera_layout],
            push_constant_ranges: &[],
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("chisel-blit"),
            bind_group_layouts: &[&blit_layout],
            push_constant_ranges: &[],
        });

        let face_attributes = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4, 4 => Float32x4
        ];
        let line_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let multisample = wgpu::MultisampleState {
            count: SAMPLES,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };

        let face_pipeline = |translucent: bool, two_sided: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("chisel-faces"),
                layout: Some(&scene_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_face"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &face_attributes,
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_face"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: PANE_FORMAT,
                        blend: translucent.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: (!two_sided).then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: !translucent,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: Default::default(),
                    // Pushed back a little, so the edge lines drawn exactly
                    // on a face win against it.
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 1.5,
                        clamp: 0.0,
                    },
                }),
                multisample,
                multiview: None,
                cache: None,
            })
        };
        let faces = [
            face_pipeline(false, false),
            face_pipeline(false, true),
            face_pipeline(true, false),
            face_pipeline(true, true),
        ];

        let line_pipeline = |xray: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if xray { "chisel-xray" } else { "chisel-lines" }),
                layout: Some(&line_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_line"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<LineVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &line_attributes,
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_line"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: PANE_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: if xray {
                        // Only where something is in front: what is in
                        // plain sight is drawn by the ordinary pass.
                        wgpu::CompareFunction::Greater
                    } else {
                        wgpu::CompareFunction::LessEqual
                    },
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample,
                multiview: None,
                cache: None,
            })
        };
        let lines = line_pipeline(false);
        let xray = line_pipeline(true);

        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("chisel-blit"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_blit"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_blit"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("chisel-faces"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("chisel-blit"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let white = upload_texture(
            device,
            queue,
            &texture_layout,
            &sampler,
            &[(1, 1, vec![[255u8; 4]])],
        );

        EditorRenderer {
            target,
            faces,
            lines,
            xray,
            blit,
            camera_layout,
            texture_layout,
            blit_layout,
            sampler,
            blit_sampler,
            white,
            textures: HashMap::new(),
            texture_epoch: 0,
            scene: None,
            panes: HashMap::new(),
        }
    }

    /// Put a scene on the GPU, unless it is already there.
    pub fn upload_scene(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        shared: &SharedScene,
    ) {
        if self
            .scene
            .as_ref()
            .is_some_and(|s| s.generation == shared.generation)
        {
            return;
        }
        if shared.texture_epoch != self.texture_epoch {
            self.textures.clear();
            self.texture_epoch = shared.texture_epoch;
        }
        let scene = &shared.scene;
        for (name, texture) in &scene.textures {
            if self.textures.contains_key(name) {
                continue;
            }
            let levels = mip_chain(texture);
            if levels.is_empty() {
                continue;
            }
            let gpu = upload_texture(device, queue, &self.texture_layout, &self.sampler, &levels);
            self.textures.insert(name.clone(), gpu);
        }

        use wgpu::util::DeviceExt;
        let buffer = |label: &str, bytes: &[u8]| {
            (!bytes.is_empty()).then(|| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                })
            })
        };
        self.scene = Some(GpuScene {
            generation: shared.generation,
            triangles: buffer("chisel-triangles", bytemuck::cast_slice(&scene.triangles)),
            lines: buffer("chisel-lines", bytemuck::cast_slice(&scene.lines)),
            line_count: scene.lines.len() as u32,
            xray: buffer("chisel-xray", bytemuck::cast_slice(&scene.xray)),
            xray_count: scene.xray.len() as u32,
            batches: scene.batches.clone(),
        });
    }

    fn pane(&mut self, device: &wgpu::Device, index: usize, size: [u32; 2]) -> &Pane {
        let stale = self.panes.get(&index).is_none_or(|p| p.size != size);
        if stale {
            let pane = self.make_pane(device, size);
            self.panes.insert(index, pane);
        }
        &self.panes[&index]
    }

    fn make_pane(&self, device: &wgpu::Device, size: [u32; 2]) -> Pane {
        let extent = wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        };
        let color = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("chisel-pane-msaa"),
                size: extent,
                mip_level_count: 1,
                sample_count: SAMPLES,
                dimension: wgpu::TextureDimension::D2,
                format: PANE_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let resolve = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("chisel-pane"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PANE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        });
        let resolve_view = resolve.create_view(&Default::default());
        // Into an sRGB window the pane is sampled as sRGB and re-encoded on
        // write; into a linear one its bytes are passed through as they are.
        let blit_view = resolve.create_view(&wgpu::TextureViewDescriptor {
            format: Some(if self.target.is_srgb() {
                PANE_FORMAT
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            }),
            ..Default::default()
        });
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("chisel-pane-depth"),
                size: extent,
                mip_level_count: 1,
                sample_count: SAMPLES,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chisel-camera"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chisel-camera"),
            layout: &self.camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            }],
        });
        let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chisel-blit"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&blit_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
                },
            ],
        });
        Pane {
            size,
            color,
            resolve,
            resolve_view,
            depth,
            camera,
            camera_group,
            blit_group,
        }
    }

    /// Draw the uploaded scene into a pane's own target.
    #[allow(clippy::too_many_arguments)]
    pub fn render_pane(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        index: usize,
        size: [u32; 2],
        camera: &CameraUniform,
        background: [f32; 3],
    ) {
        self.pane(device, index, size);
        let pane = &self.panes[&index];
        queue.write_buffer(&pane.camera, 0, bytemuck::bytes_of(camera));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("chisel-pane"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &pane.color,
                resolve_target: Some(&pane.resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: background[0] as f64,
                        g: background[1] as f64,
                        b: background[2] as f64,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &pane.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let Some(scene) = &self.scene else { return };
        pass.set_bind_group(0, &pane.camera_group, &[]);

        let draw_batches = |pass: &mut wgpu::RenderPass<'_>, translucent: bool| {
            let Some(buffer) = &scene.triangles else {
                return;
            };
            pass.set_vertex_buffer(0, buffer.slice(..));
            for batch in scene
                .batches
                .iter()
                .filter(|b| b.translucent == translucent)
            {
                pass.set_pipeline(&self.faces[translucent as usize * 2 + batch.two_sided as usize]);
                let texture = batch
                    .texture
                    .as_ref()
                    .and_then(|name| self.textures.get(name))
                    .unwrap_or(&self.white);
                pass.set_bind_group(1, &texture.bind_group, &[]);
                pass.draw(batch.first..batch.first + batch.count, 0..1);
            }
        };

        draw_batches(&mut pass, false);
        if let Some(lines) = &scene.lines {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, lines.slice(..));
            pass.draw(0..scene.line_count, 0..1);
        }
        draw_batches(&mut pass, true);
        if let Some(xray) = &scene.xray {
            pass.set_pipeline(&self.xray);
            pass.set_vertex_buffer(0, xray.slice(..));
            pass.draw(0..scene.xray_count, 0..1);
        }
    }

    /// Copy a pane into egui's pass, over the rectangle egui set as the
    /// viewport.
    pub fn blit(&self, pass: &mut wgpu::RenderPass<'static>, index: usize) {
        let Some(pane) = self.panes.get(&index) else {
            return;
        };
        pass.set_pipeline(&self.blit);
        pass.set_bind_group(0, &pane.blit_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// A pane's rendered image, for tests.
    pub fn pane_texture(&self, index: usize) -> Option<&wgpu::Texture> {
        self.panes.get(&index).map(|p| &p.resolve)
    }
}

/// Render a scene off screen and read it back, row by row, as RGBA8 sRGB.
///
/// What the tests and `examples/gpu_shot.rs` use to look at the pane
/// without a window.
pub fn render_offscreen(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: Arc<Scene>,
    camera: &CameraUniform,
    size: [u32; 2],
    background: [f32; 3],
) -> Vec<[u8; 4]> {
    let mut renderer = EditorRenderer::new(device, queue, PANE_FORMAT);
    renderer.upload_scene(
        device,
        queue,
        &SharedScene {
            generation: 1,
            scene,
            texture_epoch: 0,
        },
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.render_pane(device, queue, &mut encoder, 0, size, camera, background);
    let texture = renderer.pane_texture(0).expect("pane was rendered");
    // Rows are padded to what a copy requires.
    let row = (size[0] * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("chisel-readback"),
        size: (row * size[1]) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(size[1]),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("readback maps"));
    let _ = device.poll(wgpu::PollType::Wait);
    let bytes = slice.get_mapped_range();
    let mut out = Vec::with_capacity((size[0] * size[1]) as usize);
    for y in 0..size[1] {
        let start = (y * row) as usize;
        for x in 0..size[0] as usize {
            let at = start + x * 4;
            out.push([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        }
    }
    out
}

/// A texture's mip chain as `(width, height, pixels)`, stopping at the
/// first level that is not half the one before -- a GPU texture's mips must
/// be, and a file that disagrees still draws from its good levels.
fn mip_chain(texture: &crate::textures::Texture) -> Vec<(u32, u32, Vec<[u8; 4]>)> {
    let mut out = Vec::new();
    for (i, level) in texture.mips.iter().enumerate() {
        let (w, h) = (texture.width(), texture.height());
        let expected = ((w >> i).max(1), (h >> i).max(1));
        if (level.width, level.height) != expected
            || level.pixels.len() != (level.width * level.height) as usize
        {
            break;
        }
        out.push((level.width, level.height, level.pixels.clone()));
    }
    out
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    levels: &[(u32, u32, Vec<[u8; 4]>)],
) -> GpuTexture {
    use wgpu::util::DeviceExt;
    let (width, height, _) = levels[0];
    let descriptor = wgpu::TextureDescriptor {
        label: Some("chisel-texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    };
    let bytes: Vec<u8> = levels
        .iter()
        .flat_map(|(_, _, pixels)| bytemuck::cast_slice::<[u8; 4], u8>(pixels).iter().copied())
        .collect();
    let texture = device.create_texture_with_data(
        queue,
        &descriptor,
        wgpu::util::TextureDataOrder::LayerMajor,
        &bytes,
    );
    let view = texture.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("chisel-texture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    GpuTexture {
        bind_group,
        _texture: texture,
    }
}

#[cfg(test)]
mod tests;
