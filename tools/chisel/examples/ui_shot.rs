// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Render the whole editor window, off screen, and write it out as a PNG.
//!
//! ```text
//! cargo run -p kerosene-chisel --example ui_shot -- [map.keromap] [out.png] [--select <id>]... [--size WxH]
//!     [--tool select|block|shape|entity|texture|clip] [--assets] [--layout four|two|one] [--hover X,Y]
//!     [--tab properties|outputs|inputs] [--mode object|vertex|edge|face]
//! ```
//!
//! egui is run for a few frames against a fixed screen size and drawn
//! through egui-wgpu into a texture, paint callbacks and all -- so the 3D
//! panes are in the picture exactly as the window would show them. For
//! looking at a layout change without opening a window, and for attaching
//! to a bug report.
use anyhow::{Context, Result};

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut select = Vec::new();
    while let Some(at) = args.iter().position(|a| a == "--select") {
        args.remove(at);
        if at < args.len() {
            select.push(args.remove(at).parse::<u32>()?);
        }
    }
    let mut size = (1600u32, 900u32);
    if let Some(at) = args.iter().position(|a| a == "--size") {
        args.remove(at);
        let value = args.remove(at);
        let (w, h) = value.split_once('x').context("--size WxH")?;
        size = (w.parse()?, h.parse()?);
    }
    let mut take = |flag: &str| -> Option<String> {
        let at = args.iter().position(|a| a == flag)?;
        args.remove(at);
        (at < args.len()).then(|| args.remove(at))
    };
    let tool = take("--tool");
    let layout = take("--layout");
    let tab = take("--tab");
    let mode = take("--mode");
    let hover = take("--hover").and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some(egui::pos2(x.parse().ok()?, y.parse().ok()?))
    });
    let assets = args.iter().any(|a| a == "--assets");
    args.retain(|a| a != "--assets");
    let map = args
        .first()
        .map(String::as_str)
        .unwrap_or("content/maps/kerosene_room.keromap");
    let out = args.get(1).map(String::as_str).unwrap_or("ui_shot.png");

    let map_path = std::path::PathBuf::from(map);
    let root = kerosene_vfs::root::find(None, Some(&map_path))
        .map(|f| f.root)
        .unwrap_or_else(|| std::path::PathBuf::from("content"));
    let mut app = chisel::ChiselApp::new(root);
    app.open(map_path);
    for id in select {
        if app.document.find_entity(id).is_some() {
            app.document.selection.entities.insert(id);
        } else {
            app.document.selection.solids.insert(id);
        }
    }

    if let Some(tool) = tool {
        let kind = chisel::ToolKind::all()
            .into_iter()
            .find(|k| k.label() == tool)
            .context("no such tool")?;
        app.tool.set_kind(kind);
    }
    app.show_assets = assets;
    if let Some(mode) = mode {
        app.select_mode = chisel::app::SelectMode::all()
            .into_iter()
            .find(|m| m.label() == mode)
            .context("no such select mode")?;
    }
    app.entity_tab = match tab.as_deref() {
        Some("outputs") => chisel::app::EntityTab::Outputs,
        Some("inputs") => chisel::app::EntityTab::Inputs,
        _ => chisel::app::EntityTab::Properties,
    };
    if let Some(layout) = layout {
        app.set_pane_layout(match layout.as_str() {
            "two" => chisel::app::PaneLayout::Two,
            "one" => chisel::app::PaneLayout::One,
            _ => chisel::app::PaneLayout::Four,
        });
    }

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .context("no GPU adapter")?;
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    app.gpu_target = Some(format);

    let ctx = egui::Context::default();
    kerosene_toolui::theme::install(&ctx);
    let mut renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui-shot"),
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
    let mut output = None;
    let frames = if hover.is_some() { 12 } else { 4 };
    for frame in 0..frames {
        // Moved once, then held still: a tooltip waits for the pointer to
        // rest.
        let events = hover
            .filter(|_| frame == 0)
            .map(|p| vec![egui::Event::PointerMoved(p)])
            .unwrap_or_default();
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size.0 as f32, size.1 as f32),
            )),
            // Slow enough that a tooltip's delay passes.
            time: Some(frame as f64 * 0.25),
            ..Default::default()
        };
        let full = ctx.run(input, |ctx| app.ui(ctx));
        for (id, delta) in &full.textures_delta.set {
            renderer.update_texture(&device, &queue, *id, delta);
        }
        output = Some(full);
    }
    let output = output.expect("ran a frame");
    let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [size.0, size.1],
        pixels_per_point: output.pixels_per_point,
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    let extra = renderer.update_buffers(&device, &queue, &mut encoder, &jobs, &screen);
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui-shot"),
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
        label: Some("ui-shot-readback"),
        size: (row * size.1) as u64,
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
