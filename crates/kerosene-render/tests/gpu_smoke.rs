// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The renderer on a real device.
//!
//! The unit tests validate each shader with naga, which catches a typo but not
//! a pipeline that disagrees with its render pass: a sample count, a target
//! format, a texel format the atlas is uploaded in. Those only fail when wgpu
//! is asked to build and run them, so this test does: one lit quad, drawn
//! through the HDR target with and without MSAA, tone-mapped, and read back.
//!
//! A machine with no adapter at all -- a CI runner without even a software
//! rasteriser -- skips rather than fails. Where there is one, a validation
//! error panics inside wgpu and fails the test, which is the point.

use kerosene_bsp::cubemaps::FACES;
use kerosene_bsp::{
    Bsp, BspPlane, ColorRgbExp32, Edge, Face, Leaf, Model, TexData, TexInfo, encode_leaf,
};
use kerosene_bsp::{Cubemaps, Probe, encode_rgb9e5};
use kerosene_math::{Angles, Plane, PlaneSet, Vec3};
use kerosene_render::gpu::{CameraUniform, GpuProbes, MapResources, Renderer, ToneMapOperator};
use kerosene_render::{Camera, LightmapAtlas, WorldMesh};

const SIZE: u32 = 64;
const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// One 64-unit floor quad facing +Z, lit to `light` (in 0..255 terms),
/// wearing `material`.
fn lit_quad(light: u8, material: &str) -> Bsp {
    let mut bsp = Bsp::new();
    let mut planes = PlaneSet::new();
    let floor = planes.insert(Plane::new(Vec3::Z, 0.0));
    bsp.planes = planes.planes().iter().map(BspPlane::from_plane).collect();

    let name = bsp.intern_texdata_string(material);
    bsp.texdata.push(TexData {
        reflectivity: [0.5; 3],
        name_offset: name,
        width: 64,
        height: 64,
        view_width: 64,
        view_height: 64,
    });
    let mut texinfo = TexInfo::default();
    texinfo.texture_vecs[0] = [1.0, 0.0, 0.0, 0.0];
    texinfo.texture_vecs[1] = [0.0, -1.0, 0.0, 0.0];
    texinfo.lightmap_vecs[0] = [1.0 / 16.0, 0.0, 0.0, 0.0];
    texinfo.lightmap_vecs[1] = [0.0, 1.0 / 16.0, 0.0, 0.0];
    bsp.texinfo.push(texinfo);

    bsp.vertices.extend([
        [-32.0, -32.0, 0.0],
        [-32.0, 32.0, 0.0],
        [32.0, 32.0, 0.0],
        [32.0, -32.0, 0.0],
    ]);
    bsp.edges.extend((0..4).map(|i| Edge {
        v: [i, (i + 1) % 4],
    }));
    bsp.surfedges.extend(0..4);

    let (w, h) = (5u32, 5u32);
    bsp.faces.push(Face {
        plane: floor & !1,
        side: (floor & 1) as u8,
        first_surfedge: 0,
        num_surfedges: 4,
        texinfo: 0,
        dispinfo: -1,
        lightmap_offset: 0,
        lightmap_size: [w, h],
        light_styles: [0, 255, 255, 255],
        area: 4096.0,
        ..Default::default()
    });
    bsp.lighting.extend(std::iter::repeat_n(
        ColorRgbExp32 {
            r: light,
            g: light,
            b: light,
            exponent: 0,
        },
        (w * h) as usize,
    ));
    bsp.leaffaces.push(0);
    bsp.leaves.push(Leaf {
        contents: kerosene_bsp::contents::EMPTY,
        first_leafface: 0,
        num_leaffaces: 1,
        cluster: 0,
        mins: [-64, -64, -8],
        maxs: [64, 64, 128],
        ..Default::default()
    });
    bsp.models.push(Model {
        mins: [-64.0, -64.0, -8.0],
        maxs: [64.0, 64.0, 128.0],
        origin: [0.0; 3],
        head_node: encode_leaf(0),
        first_face: 0,
        num_faces: 1,
    });
    bsp.entities = "entity { \"classname\" \"worldspawn\" }\n".into();
    bsp.validate().expect("fixture is well formed");
    bsp
}

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("smoke"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()
}

/// Draw the quad from above and return the tone-mapped centre pixel.
fn render_centre(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    resources: &MapResources,
    mesh: &WorldMesh,
    frame_bind_group: &wgpu::BindGroup,
) -> [u8; 4] {
    renderer.ensure_targets(device, SIZE, SIZE);

    let camera = Camera {
        position: Vec3::new(0.0, 0.0, 64.0),
        angles: Angles::new(89.0, 0.0, 0.0),
        aspect: 1.0,
        ..Default::default()
    };
    renderer.update_camera(queue, &CameraUniform::from_camera(&camera, 0.0));
    // Every model slot at the identity, the world's included; an unwritten
    // buffer is a zero matrix, which draws everything at one point.
    renderer.update_models(queue, &[]);
    renderer.update_tonemap(queue, 1.0, ToneMapOperator::Aces);

    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("output"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OUTPUT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());

    // 256 bytes a row is wgpu's copy alignment; 64 texels of four bytes is
    // exactly that.
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (SIZE * SIZE * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = renderer.begin_scene_pass(&mut encoder, wgpu::Color::BLACK);
        renderer.draw_world(
            &mut pass,
            frame_bind_group,
            resources,
            mesh,
            &mesh.world_surfaces(),
        );
    }
    renderer.tonemap(&mut encoder, &output_view);
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        output.size(),
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("readback maps"));
    device.poll(wgpu::PollType::Wait).expect("device finishes");
    let bytes = slice.get_mapped_range();
    let at = (((SIZE / 2) * SIZE + SIZE / 2) * 4) as usize;
    [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
}

#[test]
fn a_lit_quad_draws_through_hdr_msaa_and_the_tonemap() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };

    let dim_bsp = lit_quad(40, "missing/on/purpose");
    let bright_bsp = lit_quad(255, "missing/on/purpose");
    let mut renderer = Renderer::new(&device, OUTPUT_FORMAT);
    let vfs = kerosene_vfs::Vfs::new();
    let no_probes = GpuProbes::upload(&device, &queue, None);

    let mut centres = Vec::new();
    for bsp in [&dim_bsp, &bright_bsp] {
        let atlas = LightmapAtlas::build(bsp, 1.0);
        let mesh = WorldMesh::build(bsp, &atlas);
        let resources = MapResources::upload(&device, &queue, &renderer, &mesh, &atlas, &vfs);
        let frame = renderer.create_frame_bind_group(&device, &resources.lightmap_view, &no_probes);

        for msaa in [1, 4] {
            assert_eq!(renderer.set_msaa(&device, msaa), msaa);
            let pixel = render_centre(&device, &queue, &mut renderer, &resources, &mesh, &frame);
            centres.push(pixel);
        }
    }

    for pixel in &centres {
        // The material is missing on purpose, so the quad wears the
        // checkerboard; either of its colours is far from the black clear.
        let sum: u32 = pixel[..3].iter().map(|&c| c as u32).sum();
        assert!(sum > 0, "the quad was not drawn: {pixel:?}");
    }
    // MSAA must not change the colour of a pixel in the middle of a face.
    for pair in centres.chunks(2) {
        for c in 0..3 {
            assert!(
                (pair[0][c] as i32 - pair[1][c] as i32).abs() <= 2,
                "msaa changed an interior pixel: {pair:?}"
            );
        }
    }
    // And the lightmap carries its range through: six times the light reads
    // brighter, rather than both saturating in an 8-bit atlas.
    let dim: u32 = centres[0][..3].iter().map(|&c| c as u32).sum();
    let bright: u32 = centres[2][..3].iter().map(|&c| c as u32).sum();
    assert!(
        bright > dim,
        "brighter lightmap read no brighter: {centres:?}"
    );
}

#[test]
fn a_metal_floor_reflects_its_probe() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };

    // A material that is all metal and nothing else: no diffuse, so all the
    // light in the picture is the reflection.
    let dir = std::env::temp_dir().join(format!("kerosene-probe-smoke-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("materials/test")).unwrap();
    std::fs::write(
        dir.join("materials/test/chrome.keromat"),
        "lit { \"$metalness\" \"1\" }\n",
    )
    .unwrap();
    let mut vfs = kerosene_vfs::Vfs::new();
    vfs.add_directory(&dir, "test");

    let mut renderer = Renderer::new(&device, OUTPUT_FORMAT);
    let centre_with_probe = |renderer: &mut Renderer, brightness: f32| {
        let mut bsp = lit_quad(128, "test/chrome");
        let size = 4u32;
        bsp.cubemaps = Some(Cubemaps {
            face_size: size,
            probes: vec![Probe {
                origin: Vec3::new(0.0, 0.0, 32.0),
                texels: vec![
                    encode_rgb9e5(Vec3::splat(brightness));
                    FACES * (size * size) as usize
                ],
            }],
        });
        let atlas = LightmapAtlas::build(&bsp, 1.0);
        let mesh = WorldMesh::build(&bsp, &atlas);
        assert!(
            mesh.vertices.iter().all(|v| v.probe == 0),
            "the quad picks the only probe"
        );
        let resources = MapResources::upload(&device, &queue, renderer, &mesh, &atlas, &vfs);
        assert!(resources.missing_materials.is_empty());
        let probes = GpuProbes::upload(&device, &queue, bsp.cubemaps.as_ref());
        let frame = renderer.create_frame_bind_group(&device, &resources.lightmap_view, &probes);
        render_centre(&device, &queue, renderer, &resources, &mesh, &frame)
    };

    let dark = centre_with_probe(&mut renderer, 0.02);
    let bright = centre_with_probe(&mut renderer, 2.0);
    let _ = std::fs::remove_dir_all(&dir);

    let sum = |p: [u8; 4]| p[..3].iter().map(|&c| c as u32).sum::<u32>();
    assert!(
        sum(bright) > sum(dark) + 60,
        "a bright probe must make the metal brighter: {dark:?} vs {bright:?}"
    );
}
