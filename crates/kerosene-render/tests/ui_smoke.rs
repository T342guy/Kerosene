// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The game UI on a real device: a layout run through `kerosene-ui`, drawn
//! by the UI pipeline, and read back.
//!
//! Skips without an adapter, like `gpu_smoke`. With `KEROSENE_UI_SHOT` set to
//! a directory, it also draws the shipped HUD and pause menu at 1280x720 and
//! writes them there as PNGs -- the quickest way to see a stylesheet change
//! without starting the game.

use kerosene_render::ui::UiRenderer;
use kerosene_ui::{Loader, UiStore, UiSystem};
use std::collections::BTreeMap;
use std::path::PathBuf;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("ui smoke"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()
}

/// Draw what `ui` shows over a flat background and read it back, RGBA8.
fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    ui: &mut UiSystem,
    vfs: &kerosene_vfs::Vfs,
    size: (u32, u32),
    background: wgpu::Color,
) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui target"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(size.0 * size.1 * 4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = UiRenderer::new(device, queue);
    renderer.upload_atlas(queue, &mut ui.fonts.atlas);
    renderer.sync_images(device, queue, vfs, &mut ui.images);

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("background"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(background),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    renderer.draw(
        device,
        queue,
        &mut encoder,
        &view,
        FORMAT,
        "screen",
        ui.display_list(),
        false,
    );
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * 4),
                rows_per_image: Some(size.1),
            },
        },
        target.size(),
    );
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("readback maps"));
    device.poll(wgpu::PollType::Wait).expect("device finishes");
    slice.get_mapped_range().to_vec()
}

fn pixel(image: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    [image[at], image[at + 1], image[at + 2], image[at + 3]]
}

#[test]
fn a_layout_draws_boxes_text_and_a_radial_fill() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let mut files = BTreeMap::new();
    files.insert(
        "ui/t.keroui".to_string(),
        r#"<root reference-height="128">
            <style>
                #red { position: absolute; left: 0px; top: 0px; width: 64px; height: 64px; background-color: #ff0000; }
                #half { position: absolute; left: 64px; top: 0px; width: 64px; height: 64px; background-color: #0000ff; -kero-fill: horizontal(0.5); }
                #text { position: absolute; left: 0px; top: 70px; font-size: 40px; color: white; }
                #glow { position: absolute; left: 192px; top: 0px; width: 64px; height: 64px; background-color: #404040; -kero-blend: additive; }
            </style>
            <Panel id="red"/>
            <Panel id="half"/>
            <Label id="text" text="HUD"/>
            <Panel id="glow"/>
        </root>"#
            .to_string(),
    );
    let mut ui = UiSystem::new();
    ui.show("hud", "ui/t.keroui", &files).unwrap();
    let mut store = UiStore::new();
    ui.update(0.016, (256, 128), &mut store, &files);
    let vfs = kerosene_vfs::Vfs::new();
    let grey = wgpu::Color {
        r: 0.2,
        g: 0.2,
        b: 0.2,
        a: 1.0,
    };
    let image = draw(&device, &queue, &mut ui, &vfs, (256, 128), grey);

    assert_eq!(
        &pixel(&image, 256, 32, 32)[..3],
        &[255, 0, 0],
        "a solid box"
    );
    assert_eq!(
        &pixel(&image, 256, 80, 32)[..3],
        &[0, 0, 255],
        "the filled half"
    );
    let background = pixel(&image, 256, 120, 32);
    assert!(
        background[2] < 150,
        "the clipped half is background: {background:?}"
    );
    // Text: somewhere in its box, a pixel much brighter than the background.
    let lit = (0..120)
        .flat_map(|x| (72..120).map(move |y| (x, y)))
        .filter(|&(x, y)| pixel(&image, 256, x, y)[0] > 200)
        .count();
    assert!(lit > 50, "the label drew {lit} bright pixels");
    // Additive: the grey panel adds to the background rather than covering it.
    let glow = pixel(&image, 256, 224, 32);
    let bg = pixel(&image, 256, 160, 32);
    assert!(glow[0] > bg[0] + 8, "{glow:?} over {bg:?}");
}

/// The repository's content tree.
fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

#[test]
fn the_shipped_hud_draws() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let mut vfs = kerosene_vfs::Vfs::new();
    vfs.add_directory(&content(), "GAME");
    let files: &dyn Loader = &vfs;

    let mut store = UiStore::new();
    for (k, v) in [
        ("player.health", "20"),
        ("player.alive", "true"),
        ("map.name", "kero_start"),
        ("weapon.active", "shotgun"),
        ("weapon.ammo", "2"),
        ("weapon.clip", "6"),
        ("weapon.reserve", "18"),
        ("weapon.count", "3"),
        ("weapons.0.name", "pistol"),
        ("weapons.1.name", "shotgun"),
        ("weapons.1.active", "true"),
        ("weapons.2.name", "rifle"),
        ("ability.dash.ready", "false"),
        ("ability.dash.charge", "0.6"),
        ("ability.dash.remaining", "1.2"),
        (
            "objective.text",
            "Open the shutter: the keypad code is 1234",
        ),
        ("cvar.volume", "0.8"),
        ("cvar.cl_fov", "90"),
        ("cvar.sensitivity", "3"),
        ("cvar.m_invert", "0"),
        ("cvar.ui_debug", "0"),
    ] {
        store.set(k, kerosene_ui::Value::parse(v));
    }
    let size = (1280, 720);
    let mut ui = UiSystem::new();
    ui.show("hud", "ui/hud.keroui", files).unwrap();
    for _ in 0..20 {
        ui.update(0.05, size, &mut store, files);
    }
    let scene = wgpu::Color {
        r: 0.09,
        g: 0.1,
        b: 0.11,
        a: 1.0,
    };
    let hud = draw(&device, &queue, &mut ui, &vfs, size, scene);
    let changed = hud.as_chunks::<4>().0.iter().filter(|p| p[0] > 60).count();
    assert!(
        changed > 1000,
        "the HUD drew almost nothing: {changed} pixels"
    );

    ui.show("menu", "ui/menus/pause.keroui", files).unwrap();
    for _ in 0..20 {
        ui.update(0.05, size, &mut store, files);
    }
    let menu = draw(&device, &queue, &mut ui, &vfs, size, scene);

    if let Some(dir) = std::env::var_os("KEROSENE_UI_SHOT") {
        let dir = PathBuf::from(dir);
        for (name, image) in [("hud.png", &hud), ("menu.png", &menu)] {
            let file = std::fs::File::create(dir.join(name)).unwrap();
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size.0, size.1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(image)
                .unwrap();
        }
    }
}
