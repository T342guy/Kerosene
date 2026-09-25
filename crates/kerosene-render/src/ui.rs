// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Drawing the game UI.
//!
//! `kerosene-ui` does everything up to a [`DisplayList`] of quads; this puts
//! them on the GPU. One pipeline (`shaders/ui.wgsl`) draws every quad, in two
//! blend variants -- normal and additive -- per target format. The list is
//! uploaded as one instance buffer and drawn in as few calls as the textures,
//! blend modes and clip rectangles in it allow: a HUD of text and panels with
//! one image is typically three or four draws.
//!
//! The same renderer draws **world panels**: a document rendered into its own
//! texture, only when what it draws changed, and that texture put on a quad in
//! the scene pass (`shaders/panel.wgsl`), depth-tested and lit by itself.
//!
//! Images are loaded here, not by the UI: [`UiRenderer::sync_images`] reads
//! any `.kerotex` the UI has asked for since the last frame and reports its
//! size back, so `contain` and `cover` can fit it.

use crate::gpu::{DEPTH_FORMAT, HDR_FORMAT, load_texture};
use bytemuck::{Pod, Zeroable};
use kerosene_math::{Mat4, Vec3};
use kerosene_ui::{ATLAS_SIZE, DisplayList, DrawItem, GlyphAtlas, Images, TextureRef};
use kerosene_vfs::Vfs;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

/// Format of a world panel's texture.
pub const PANEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// One quad as the shader reads it. Mirrors `QuadIn` in `ui.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct GpuQuad {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub color2: [f32; 4],
    pub border_color: [f32; 4],
    /// Radius, border width, softness, fill amount.
    pub params: [f32; 4],
    /// Texture mode, gradient, fill kind, unused.
    pub modes: [u32; 4],
    pub transform_x: [f32; 3],
    pub transform_y: [f32; 3],
}

const QUAD_ATTRIBUTES: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
    0 => Float32x4,
    1 => Float32x4,
    2 => Float32x4,
    3 => Float32x4,
    4 => Float32x4,
    5 => Float32x4,
    6 => Uint32x4,
    7 => Float32x3,
    8 => Float32x3,
];

/// The shader's `Screen` uniform.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct ScreenUniform {
    size: [f32; 2],
    output_srgb: u32,
    _pad: u32,
}

/// A run of quads drawn in one call.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Batch {
    pub start: u32,
    pub end: u32,
    pub image: Option<u32>,
    pub additive: bool,
    /// `None` is the whole target.
    pub scissor: Option<[u32; 4]>,
}

/// Turn a display list into instances and the draws that cover them.
///
/// `has_image` says which images are ready; a quad showing one that is not
/// is left out, rather than taking the batch it would have joined down with
/// it. Pure, so the batching can be tested without a device.
pub fn pack(list: &DisplayList, has_image: impl Fn(u32) -> bool) -> (Vec<GpuQuad>, Vec<Batch>) {
    let mut quads = Vec::with_capacity(list.items.len());
    let mut batches: Vec<Batch> = Vec::new();
    let mut scissor: Option<[u32; 4]> = None;
    for item in &list.items {
        let q = match item {
            DrawItem::Clip(c) => {
                scissor = *c;
                continue;
            }
            DrawItem::Quad(q) => q,
        };
        let (mode, image) = match q.texture {
            TextureRef::None => (0, None),
            TextureRef::Glyphs => (1, None),
            TextureRef::Image(id) if has_image(id) => (2, Some(id)),
            TextureRef::Image(_) => continue,
        };
        let index = quads.len() as u32;
        quads.push(GpuQuad {
            rect: q.rect,
            uv: q.uv,
            color: q.color,
            color2: q.color2,
            border_color: q.border_color,
            params: [q.radius, q.border_width, q.softness, q.fill.1],
            modes: [mode, q.gradient, q.fill.0, 0],
            transform_x: [q.transform[0], q.transform[1], q.transform[2]],
            transform_y: [q.transform[3], q.transform[4], q.transform[5]],
        });
        // Quads that sample no image can join any batch: the image binding
        // is simply unused by them.
        match batches.last_mut() {
            Some(b)
                if b.end == index
                    && b.additive == q.additive
                    && b.scissor == scissor
                    && (image.is_none() || b.image.is_none() || b.image == image) =>
            {
                b.end += 1;
                if image.is_some() {
                    b.image = image;
                }
            }
            _ => batches.push(Batch {
                start: index,
                end: index + 1,
                image,
                additive: q.additive,
                scissor,
            }),
        }
    }
    (quads, batches)
}

/// Per-target state: the instance buffer and the uniform with its size.
struct Target {
    instances: wgpu::Buffer,
    capacity: u64,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// A world panel's texture.
struct PanelTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    /// The document revision last drawn into it.
    revision: Option<u64>,
    bind_group: wgpu::BindGroup,
}

/// One world panel to draw this frame: where its corners are and how bright.
#[derive(Clone, Debug)]
pub struct PanelQuad {
    pub name: String,
    /// Top-left, top-right, bottom-left, bottom-right, in world units.
    pub corners: [Vec3; 4],
    /// Linear scene brightness of a white texel.
    pub brightness: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct PanelVertex {
    position: [f32; 3],
    uv: [f32; 2],
    brightness: f32,
}

pub struct UiRenderer {
    shader: wgpu::ShaderModule,
    screen_layout: wgpu::BindGroupLayout,
    image_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<(wgpu::TextureFormat, bool), wgpu::RenderPipeline>,
    sampler: wgpu::Sampler,
    atlas: wgpu::Texture,
    atlas_view: wgpu::TextureView,
    white: wgpu::BindGroup,
    _white_texture: wgpu::Texture,
    /// `None` for an image that would not load: asked once, not every frame.
    images: HashMap<u32, Option<(wgpu::Texture, wgpu::BindGroup)>>,
    targets: HashMap<String, Target>,

    panel_shader: wgpu::ShaderModule,
    panel_texture_layout: wgpu::BindGroupLayout,
    panel_pipeline_layout: wgpu::PipelineLayout,
    panel_pipeline: Option<(u32, wgpu::RenderPipeline)>,
    panel_uniform: wgpu::Buffer,
    panel_view_group: wgpu::BindGroup,
    panel_vertices: Option<(wgpu::Buffer, u64)>,
    panels: HashMap<String, PanelTexture>,
    /// This frame's world panel draws: which panel, and its first vertex.
    panel_draws: Vec<(String, u32)>,
}

impl UiRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> UiRenderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/ui.wgsl").into()),
        });
        let screen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui screen"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<ScreenUniform>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(2),
            ],
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui image"),
            entries: &[texture_entry(0)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[&screen_layout, &image_layout],
            push_constant_ranges: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ui"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui glyph atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());

        let white_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("ui white"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &[255, 255, 255, 255],
        );
        let white = image_bind_group(device, &image_layout, &white_texture);

        let panel_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world panel"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/panel.wgsl").into()),
        });
        let panel_view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world panel view"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(64),
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
        let panel_texture_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("world panel texture"),
                entries: &[texture_entry(0)],
            });
        let panel_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("world panel"),
                bind_group_layouts: &[&panel_view_layout, &panel_texture_layout],
                push_constant_ranges: &[],
            });
        let panel_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world panel view"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let panel_view_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world panel view"),
            layout: &panel_view_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: panel_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        UiRenderer {
            shader,
            screen_layout,
            image_layout,
            pipeline_layout,
            pipelines: HashMap::new(),
            sampler,
            atlas,
            atlas_view,
            white,
            _white_texture: white_texture,
            images: HashMap::new(),
            targets: HashMap::new(),
            panel_shader,
            panel_texture_layout,
            panel_pipeline_layout,
            panel_pipeline: None,
            panel_uniform,
            panel_view_group,
            panel_vertices: None,
            panels: HashMap::new(),
            panel_draws: Vec::new(),
        }
    }

    /// Copy the glyph atlas up if anything was added to it.
    pub fn upload_atlas(&self, queue: &wgpu::Queue, atlas: &mut GlyphAtlas) {
        if !atlas.dirty {
            return;
        }
        atlas.dirty = false;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_SIZE),
                rows_per_image: Some(ATLAS_SIZE),
            },
            wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Load any image the UI has named and this has not tried yet.
    pub fn sync_images(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vfs: &Vfs,
        images: &mut Images,
    ) {
        for id in 0..images.len() as u32 {
            if self.images.contains_key(&id) {
                continue;
            }
            let Some(path) = images.path(id) else {
                continue;
            };
            let loaded = load_texture(device, queue, vfs, path);
            match &loaded {
                Some(t) => images.set_size(id, t.width(), t.height()),
                None => log::warn!(
                    "ui: image {path} would not load (is it compiled? materials/{path}.kerotex)"
                ),
            }
            let entry = loaded.map(|t| {
                let group = image_bind_group(device, &self.image_layout, &t);
                (t, group)
            });
            self.images.insert(id, entry);
        }
    }

    /// Forget every loaded image, so they load again: `ui_reload`.
    pub fn forget_images(&mut self) {
        self.images.clear();
    }

    fn pipeline(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat, additive: bool) {
        if self.pipelines.contains_key(&(format, additive)) {
            return;
        }
        let blend = if additive {
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
            }
        } else {
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(if additive { "ui additive" } else { "ui" }),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_ui"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuQuad>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &QUAD_ATTRIBUTES,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_ui"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend),
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
        self.pipelines.insert((format, additive), pipeline);
    }

    /// Draw a display list into `view`. `key` names the target's buffers
    /// (`"screen"`, a panel name) so two targets in one frame do not share
    /// them. `clear` wipes the target to transparent first; the screen
    /// loads what the scene left instead.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        key: &str,
        list: &DisplayList,
        clear: bool,
    ) {
        let (quads, batches) = pack(list, |id| matches!(self.images.get(&id), Some(Some(_))));
        let (w, h) = (list.size.0.max(1), list.size.1.max(1));
        if quads.is_empty() && !clear {
            return;
        }
        self.pipeline(device, format, false);
        self.pipeline(device, format, true);

        let bytes = (quads.len().max(1) * std::mem::size_of::<GpuQuad>()) as u64;
        let needs_new = self.targets.get(key).is_none_or(|t| t.capacity < bytes);
        if needs_new {
            // Grow by doubling so a HUD that gains a line of text does not
            // reallocate every frame.
            let capacity = bytes
                .next_power_of_two()
                .max(64 * std::mem::size_of::<GpuQuad>() as u64);
            let instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ui quads"),
                size: capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ui screen"),
                size: std::mem::size_of::<ScreenUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ui screen"),
                layout: &self.screen_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&self.atlas_view),
                    },
                ],
            });
            self.targets.insert(
                key.to_string(),
                Target {
                    instances,
                    capacity,
                    uniform,
                    bind_group,
                },
            );
        }
        let target = &self.targets[key];
        queue.write_buffer(
            &target.uniform,
            0,
            bytemuck::bytes_of(&ScreenUniform {
                size: [w as f32, h as f32],
                output_srgb: u32::from(!format.is_srgb()),
                _pad: 0,
            }),
        );
        if !quads.is_empty() {
            queue.write_buffer(&target.instances, 0, bytemuck::cast_slice(&quads));
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("game ui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if clear {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &target.bind_group, &[]);
        pass.set_vertex_buffer(0, target.instances.slice(..));
        for batch in batches {
            let Some(pipeline) = self.pipelines.get(&(format, batch.additive)) else {
                continue;
            };
            let image = match batch.image.and_then(|id| self.images.get(&id)) {
                Some(Some((_, group))) => group,
                _ => &self.white,
            };
            match batch.scissor {
                Some([x, y, sw, sh]) => {
                    let x0 = x.min(w);
                    let y0 = y.min(h);
                    let sw = sw.min(w - x0);
                    let sh = sh.min(h - y0);
                    if sw == 0 || sh == 0 {
                        continue;
                    }
                    pass.set_scissor_rect(x0, y0, sw, sh);
                }
                None => pass.set_scissor_rect(0, 0, w, h),
            }
            pass.set_pipeline(pipeline);
            pass.set_bind_group(1, image, &[]);
            pass.draw(0..6, batch.start..batch.end);
        }
    }

    // ---- world panels ------------------------------------------------------

    /// Render a world panel's document into its texture, if it changed since
    /// the last time.
    pub fn render_panel(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        name: &str,
        list: &DisplayList,
        revision: u64,
    ) {
        let size = (list.size.0.max(1), list.size.1.max(1));
        let stale = self.panels.get(name).is_none_or(|p| p.size != size);
        if stale {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(name),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: PANEL_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(name),
                layout: &self.panel_texture_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                }],
            });
            self.panels.insert(
                name.to_string(),
                PanelTexture {
                    texture,
                    view,
                    size,
                    revision: None,
                    bind_group,
                },
            );
        }
        if self.panels[name].revision == Some(revision) {
            return;
        }
        let view = self.panels[name].view.clone();
        self.draw(
            device,
            queue,
            encoder,
            &view,
            PANEL_FORMAT,
            &format!("panel:{name}"),
            list,
            true,
        );
        if let Some(p) = self.panels.get_mut(name) {
            p.revision = Some(revision);
        }
    }

    /// Drop textures for panels that no longer exist.
    pub fn retain_panels(&mut self, alive: impl Fn(&str) -> bool) {
        self.panels.retain(|name, _| alive(name));
        self.targets
            .retain(|key, _| key.strip_prefix("panel:").is_none_or(&alive));
    }

    /// Upload this frame's world panel quads. Before the scene pass;
    /// [`UiRenderer::draw_world_panels`] draws them inside it.
    pub fn prepare_world_panels(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_proj: Mat4,
        samples: u32,
        quads: &[PanelQuad],
    ) {
        self.panel_draws.clear();
        if self
            .panel_pipeline
            .as_ref()
            .is_none_or(|(s, _)| *s != samples)
        {
            self.panel_pipeline = Some((samples, self.create_panel_pipeline(device, samples)));
        }
        queue.write_buffer(
            &self.panel_uniform,
            0,
            bytemuck::cast_slice(&view_proj.to_cols_array()),
        );

        let mut vertices = Vec::with_capacity(quads.len() * 6);
        for quad in quads {
            if !self.panels.contains_key(&quad.name) {
                continue;
            }
            self.panel_draws
                .push((quad.name.clone(), vertices.len() as u32));
            let v = |i: usize, uv: [f32; 2]| PanelVertex {
                position: quad.corners[i].to_array(),
                uv,
                brightness: quad.brightness,
            };
            let (tl, tr, bl, br) = (
                v(0, [0.0, 0.0]),
                v(1, [1.0, 0.0]),
                v(2, [0.0, 1.0]),
                v(3, [1.0, 1.0]),
            );
            vertices.extend_from_slice(&[tl, bl, tr, tr, bl, br]);
        }
        if vertices.is_empty() {
            return;
        }
        let bytes = (vertices.len() * std::mem::size_of::<PanelVertex>()) as u64;
        if self
            .panel_vertices
            .as_ref()
            .is_none_or(|(_, cap)| *cap < bytes)
        {
            let capacity = bytes.next_power_of_two();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("world panels"),
                size: capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.panel_vertices = Some((buffer, capacity));
        }
        if let Some((buffer, _)) = &self.panel_vertices {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&vertices));
        }
    }

    /// Draw the panels [`UiRenderer::prepare_world_panels`] set up, inside
    /// the scene pass.
    pub fn draw_world_panels<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let (Some((_, pipeline)), Some((buffer, _))) = (&self.panel_pipeline, &self.panel_vertices)
        else {
            return;
        };
        if self.panel_draws.is_empty() {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.panel_view_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for (name, first) in &self.panel_draws {
            if let Some(panel) = self.panels.get(name) {
                pass.set_bind_group(1, &panel.bind_group, &[]);
                pass.draw(*first..*first + 6, 0..1);
            }
        }
    }

    fn create_panel_pipeline(&self, device: &wgpu::Device, samples: u32) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world panel"),
            layout: Some(&self.panel_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.panel_shader,
                entry_point: Some("vs_panel"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<PanelVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.panel_shader,
                entry_point: Some("fs_panel"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Two-sided: a panel is placed by an entity's angles, and a
                // mistake there should show a mirrored screen, not none.
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        })
    }

    /// A world panel's texture, for tests and tools.
    pub fn panel_texture(&self, name: &str) -> Option<&wgpu::Texture> {
        self.panels.get(name).map(|p| &p.texture)
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn image_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ui image"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    })
}

#[cfg(test)]
mod tests;
