// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Render a map through Chisel's GPU 3D pane and write it out as a PNG.
//!
//! ```text
//! cargo run -p kerosene-chisel --example gpu_shot -- <map.kmap> <out.png> [x y z yaw pitch] [--select <id>] [--all-helpers]
//! ```
//!
//! The same scene the editor builds -- models, helpers, selection -- drawn
//! off screen on whatever adapter there is. `preview_shot` is the software
//! rasteriser's equivalent.
use anyhow::{Context, Result};
use kerosene_math::{Angles, Vec3};
use kerosene_rhi::wgpu;

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut select = Vec::new();
    let all_helpers = args.iter().any(|a| a == "--all-helpers");
    args.retain(|a| a != "--all-helpers");
    while let Some(at) = args.iter().position(|a| a == "--select") {
        args.remove(at);
        if at < args.len() {
            select.push(args.remove(at).parse::<u32>()?);
        }
    }
    let map = args
        .first()
        .map(String::as_str)
        .unwrap_or("content/maps/kerosene_room.kmap");
    let out = args.get(1).map(String::as_str).unwrap_or("gpu_shot.png");
    let number = |i: usize, fallback: f32| -> f32 {
        args.get(i).and_then(|v| v.parse().ok()).unwrap_or(fallback)
    };
    let eye = Vec3::new(number(2, 80.0), number(3, 80.0), number(4, 96.0));
    let angles = Angles::new(number(6, 10.0), number(5, 35.0), 0.0);

    let map_path = std::path::PathBuf::from(map);
    let found = kerosene_vfs::root::find(None, Some(&map_path));
    eprintln!("{}", kerosene_vfs::root::describe(&found));
    let root = found
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
    if all_helpers {
        app.helper_mode = chisel::helpers::HelperMode::All;
    }
    let scene = app.shared_scene();

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .context("no GPU adapter")?;
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;

    let (w, h) = (960u32, 600u32);
    let background = chisel::gpu::scene::linear(chisel::draw::colors::BACKGROUND);
    let pixels = chisel::gpu::render_offscreen(
        &device,
        &queue,
        scene.scene.clone(),
        &chisel::gpu::CameraUniform::new(eye, angles.vectors(), 90.0, w as f32 / h as f32),
        [w, h],
        [background[0], background[1], background[2]],
    );
    let flat: Vec<u8> = pixels.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    image::RgbImage::from_raw(w, h, flat)
        .expect("dimensions match")
        .save(out)?;
    println!(
        "wrote {out} ({} triangles, {} batches)",
        scene.scene.triangle_count(),
        scene.scene.batches.len()
    );
    Ok(())
}
