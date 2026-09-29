// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Render the toolset window, off screen, and write it out as a PNG.
//!
//! ```text
//! cargo run -p kerosene-tools --example shot -- [home|assets|editor|models|sound|build|archive|start]
//!     [out.png] [--content DIR] [--size WxH] [--palette QUERY] [--output]
//! ```
//!
//! egui is run for a few frames against a fixed screen size and drawn
//! through egui-wgpu into a texture, the same way Chisel's `ui_shot` draws
//! the editor. For looking at a page without opening a window, and for
//! attaching to a bug report.

use anyhow::{Context, Result, bail};
use kerosene_rhi::wgpu;
use kerosene_tools::{Action, Launch, Tab, Toolset};
use kerosene_toolui::App as _;

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut take = |flag: &str| -> Option<String> {
        let at = args.iter().position(|a| a == flag)?;
        args.remove(at);
        (at < args.len()).then(|| args.remove(at))
    };
    let size = match take("--size") {
        Some(value) => {
            let (w, h) = value.split_once('x').context("--size WxH")?;
            (w.parse::<u32>()?, h.parse::<u32>()?)
        }
        None => (1600, 950),
    };
    let content = take("--content").unwrap_or_else(|| "content".to_string());
    let palette = take("--palette");
    let output = args.iter().any(|a| a == "--output");
    args.retain(|a| a != "--output");
    let page = args.first().map(String::as_str).unwrap_or("home");
    let out = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("toolset_shot.png");

    let mut toolset = Toolset::open(Launch {
        content: Some(content.into()),
        ..Default::default()
    })?;
    match page {
        "start" => toolset.act(Action::ShowStart),
        name => {
            let tab = Tab::ALL
                .into_iter()
                .find(|t| t.name().eq_ignore_ascii_case(name));
            let Some(tab) = tab else {
                bail!("no page called {name}");
            };
            toolset.act(Action::Goto(tab));
        }
    }
    if output {
        toolset.act(Action::ShowOutput);
    }
    if let Some(query) = palette {
        toolset.act(Action::OpenPalette);
        toolset.palette_mut().set_query(query);
    }

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .context("no GPU adapter")?;
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    toolset.gpu_ready(format);

    let ctx = egui::Context::default();
    kerosene_toolui::theme::install(&ctx);
    let mut renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("toolset-shot"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());

    // A few frames: egui sizes some things from the frame before.
    let mut last = None;
    for frame in 0..4 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size.0 as f32, size.1 as f32),
            )),
            time: Some(f64::from(frame) * 0.25),
            ..Default::default()
        };
        let full = ctx.run(input, |ctx| toolset.ui(ctx));
        for (id, delta) in &full.textures_delta.set {
            renderer.update_texture(&device, &queue, *id, delta);
        }
        last = Some(full);
    }
    let output = last.expect("ran a frame");
    let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [size.0, size.1],
        pixels_per_point: output.pixels_per_point,
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    let extra = renderer.update_buffers(&device, &queue, &mut encoder, &jobs, &screen);
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("toolset-shot"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let mut pass = pass.forget_lifetime();
        renderer.render(&mut pass, &jobs, &screen);
    }
    let row = (size.0 * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("toolset-shot-readback"),
        size: u64::from(row * size.1),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(size.1),
            },
        },
        target.size(),
    );
    queue.submit(extra.into_iter().chain([encoder.finish()]));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("readback maps"));
    let _ = device.poll(wgpu::PollType::Wait);
    let bytes = slice.get_mapped_range();
    let mut rgb = Vec::with_capacity((size.0 * size.1 * 3) as usize);
    for y in 0..size.1 {
        let start = (y * row) as usize;
        for x in 0..size.0 as usize {
            let at = start + x * 4;
            rgb.extend_from_slice(&bytes[at..at + 3]);
        }
    }
    image::RgbImage::from_raw(size.0, size.1, rgb)
        .expect("dimensions match")
        .save(out)?;
    println!("wrote {out}");
    Ok(())
}
