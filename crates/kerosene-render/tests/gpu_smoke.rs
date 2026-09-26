// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
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
use kerosene_render::lights::{DynamicLight, LightFrame};
use kerosene_render::{Camera, LightmapAtlas, WorldMesh};

const SIZE: u32 = 64;
const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// One 64-unit floor quad facing +Z, lit to `light` (in 0..255 terms),
/// wearing `material`.
fn lit_quad(light: u8, material: &str) -> Bsp {
    quads(light, material, false)
}

/// The floor quad, and with `roof` a 16-unit quad at z = 32 over its centre,
/// facing down: seen from above it is back-facing and culled, so it draws
/// nothing -- but it is still there to cast a shadow.
fn quads(light: u8, material: &str, roof: bool) -> Bsp {
    let mut bsp = Bsp::new();
    let mut planes = PlaneSet::new();
    let floor = planes.insert(Plane::new(Vec3::Z, 0.0));
    let roof_plane = planes.insert(Plane::new(-Vec3::Z, -32.0));
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
    if roof {
        // Clockwise seen from below, its front.
        bsp.vertices.extend([
            [8.0, -8.0, 32.0],
            [8.0, 8.0, 32.0],
            [-8.0, 8.0, 32.0],
            [-8.0, -8.0, 32.0],
        ]);
        bsp.edges.extend((0..4).map(|i| Edge {
            v: [4 + i, 4 + (i + 1) % 4],
        }));
        bsp.surfedges.extend(4..8);
        bsp.faces.push(Face {
            plane: roof_plane & !1,
            side: (roof_plane & 1) as u8,
            first_surfedge: 4,
            num_surfedges: 4,
            texinfo: 0,
            dispinfo: -1,
            lightmap_offset: -1,
            light_styles: [0, 255, 255, 255],
            area: 256.0,
            ..Default::default()
        });
        bsp.leaffaces.push(1);
    }
    bsp.leaves.push(Leaf {
        contents: kerosene_bsp::contents::EMPTY,
        first_leafface: 0,
        num_leaffaces: bsp.leaffaces.len() as u16,
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
        num_faces: bsp.faces.len() as u32,
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
    let bsp = Bsp::new();
    let image = render(
        device,
        queue,
        renderer,
        resources,
        mesh,
        frame_bind_group,
        &bsp,
        &[],
    );
    pixel(&image, SIZE / 2, SIZE / 2)
}

fn pixel(image: &[u8], x: u32, y: u32) -> [u8; 4] {
    let at = ((y * SIZE + x) * 4) as usize;
    [image[at], image[at + 1], image[at + 2], image[at + 3]]
}

fn brightness(p: [u8; 4]) -> u32 {
    p[..3].iter().map(|&c| c as u32).sum()
}

/// Draw the scene from above, with `lights` and their shadows, and return
/// the tone-mapped image, four bytes a pixel.
#[allow(clippy::too_many_arguments)]
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    resources: &MapResources,
    mesh: &WorldMesh,
    frame_bind_group: &wgpu::BindGroup,
    bsp: &Bsp,
    lights: &[DynamicLight],
) -> Vec<u8> {
    renderer.ensure_targets(device, SIZE, SIZE);

    let camera = Camera {
        position: Vec3::new(0.0, 0.0, 64.0),
        angles: Angles::new(89.0, 0.0, 0.0),
        aspect: 1.0,
        ..Default::default()
    };
    renderer.update_camera(queue, &CameraUniform::from_camera(&camera, 0.0));
    let light_frame = LightFrame::build(lights, &camera, SIZE, SIZE);
    renderer.update_lights(queue, &light_frame);
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
    for (layer, (view, _)) in light_frame.shadow_views.iter().enumerate() {
        let frustum = kerosene_render::Frustum::from_view_projection(*view);
        let origin = lights[light_frame.shadow_views[layer].1].origin;
        let surfaces = mesh.visible_surfaces(bsp, origin, &frustum);
        let mut pass = renderer.begin_shadow_pass(&mut encoder, layer);
        renderer.draw_world_shadow(&mut pass, layer, resources, mesh, &surfaces, 0);
    }
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
    slice.get_mapped_range().to_vec()
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

#[test]
fn a_dynamic_light_lights_the_floor_and_a_roof_shadows_it() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    // A dim bake, so what the dynamic light adds is plain to see.
    let bsp = quads(10, "missing/on/purpose", true);
    let mut renderer = Renderer::new(&device, OUTPUT_FORMAT);
    let vfs = kerosene_vfs::Vfs::new();
    let atlas = LightmapAtlas::build(&bsp, 1.0);
    let mesh = WorldMesh::build(&bsp, &atlas);
    let resources = MapResources::upload(&device, &queue, &renderer, &mesh, &atlas, &vfs);
    let probes = GpuProbes::upload(&device, &queue, None);
    let frame = renderer.create_frame_bind_group(&device, &resources.lightmap_view, &probes);

    let light = DynamicLight {
        brightness: 300.0,
        ..DynamicLight::point(Vec3::new(0.0, 0.0, 48.0))
    };
    let mut shoot = |lights: &[DynamicLight]| {
        render(
            &device,
            &queue,
            &mut renderer,
            &resources,
            &mesh,
            &frame,
            &bsp,
            lights,
        )
    };
    let dark = shoot(&[]);
    let unshadowed = shoot(&[light]);
    let shadowed = shoot(&[DynamicLight {
        shadows: true,
        ..light
    }]);

    // The floor under the roof, and floor well outside its shadow (which
    // reaches 24 units out; the floor 32, and this pixel is at about 28).
    let (under, beside) = ((SIZE / 2, SIZE / 2), (51, SIZE / 2));
    let at = |img: &[u8], (x, y): (u32, u32)| brightness(pixel(img, x, y));

    assert!(
        at(&unshadowed, under) > at(&dark, under) + 60,
        "the light must brighten the floor: {} vs {}",
        at(&unshadowed, under),
        at(&dark, under)
    );
    assert!(
        at(&shadowed, under) + 60 < at(&unshadowed, under),
        "the roof must shadow the floor under it: {} vs {}",
        at(&shadowed, under),
        at(&unshadowed, under)
    );
    assert!(
        at(&shadowed, beside) + 20 > at(&unshadowed, beside),
        "and leave the rest lit: {} vs {}",
        at(&shadowed, beside),
        at(&unshadowed, beside)
    );
}

/// A 32-unit cube `.keromdl`, wound counter-clockwise from outside, in a
/// material that will not load (so it draws as the checkerboard).
fn cube_model() -> kerosene_asset::Model {
    use kerosene_asset::{Mesh, Model, Vertex};
    let mut m = Model::new();
    let mat = m.intern("missing/on/purpose");
    let h = 16.0f32;
    for c in [
        [-h, -h, -h],
        [h, -h, -h],
        [h, h, -h],
        [-h, h, -h],
        [-h, -h, h],
        [h, -h, h],
        [h, h, h],
        [-h, h, h],
    ] {
        let pos = Vec3::from_array(c);
        m.vertices.push(Vertex::rigid(
            pos,
            (pos / h).normalize_or_zero(),
            [0.0, 0.0],
        ));
    }
    for t in [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [2, 3, 7],
        [2, 7, 6],
        [0, 4, 7],
        [0, 7, 3],
        [1, 2, 6],
        [1, 6, 5],
    ] {
        m.indices.extend(t.iter().map(|&i| i as u32));
    }
    m.meshes.push(Mesh {
        first_index: 0,
        index_count: 36,
        material_offset: mat,
        flags: 0,
    });
    m.recompute_bounds();
    m
}

#[test]
fn copies_of_a_model_draw_in_one_instanced_call() {
    use kerosene_math::Pose;
    use kerosene_render::gpu::{ModelInstance, load_model};

    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("kerosene-instances-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("models/test")).unwrap();
    std::fs::write(
        dir.join("models/test/cube.keromdl"),
        cube_model().to_bytes(),
    )
    .unwrap();
    let mut vfs = kerosene_vfs::Vfs::new();
    vfs.add_directory(&dir, "test");

    let mut renderer = Renderer::new(&device, OUTPUT_FORMAT);
    // A world only for its frame bind group: nothing of it is drawn.
    let bsp = lit_quad(0, "missing/on/purpose");
    let atlas = LightmapAtlas::build(&bsp, 1.0);
    let mesh = WorldMesh::build(&bsp, &atlas);
    let resources = MapResources::upload(&device, &queue, &renderer, &mesh, &atlas, &vfs);
    let probes = GpuProbes::upload(&device, &queue, None);
    let frame = renderer.create_frame_bind_group(&device, &resources.lightmap_view, &probes);
    let model = load_model(&device, &queue, &renderer, &vfs, "test/cube").expect("the cube loads");

    // Two cubes either side of the view's centre, a gap between them.
    renderer.update_instances(
        &device,
        &queue,
        &[
            ModelInstance::new(
                Pose::new(Vec3::new(0.0, 30.0, 40.0), Angles::ZERO),
                u32::MAX,
            ),
            ModelInstance::new(
                Pose::new(Vec3::new(0.0, -30.0, 40.0), Angles::ZERO),
                u32::MAX,
            ),
        ],
    );
    renderer.ensure_targets(&device, SIZE, SIZE);
    let camera = Camera {
        position: Vec3::new(0.0, 0.0, 120.0),
        angles: Angles::new(89.0, 0.0, 0.0),
        aspect: 1.0,
        ..Default::default()
    };
    renderer.update_camera(&queue, &CameraUniform::from_camera(&camera, 0.0));
    renderer.update_models(&queue, &[]);
    renderer.update_lights(&queue, &LightFrame::build(&[], &camera, SIZE, SIZE));
    renderer.update_tonemap(&queue, 1.0, ToneMapOperator::Aces);

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
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (SIZE * SIZE * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let drawn = {
        let mut pass = renderer.begin_scene_pass(&mut encoder, wgpu::Color::BLACK);
        renderer.draw_studio_instances(&mut pass, &frame, &model, 0, 2)
    };
    renderer.tonemap(&mut encoder, &output.create_view(&Default::default()));
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
    let image = slice.get_mapped_range().to_vec();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(drawn.draw_calls, 1, "one mesh, one draw, two copies");
    assert_eq!(drawn.triangles, 24);
    // Looking straight down with yaw 0, world +Y is screen left.
    let left = brightness(pixel(&image, 12, SIZE / 2));
    let right = brightness(pixel(&image, 52, SIZE / 2));
    let gap = brightness(pixel(&image, SIZE / 2, SIZE / 2));
    assert!(left > 0 && right > 0, "both copies drawn: {left} {right}");
    assert_eq!(gap, 0, "and nothing between them");
}

#[test]
fn a_bone_palette_moves_the_vertices_bound_to_it() {
    use kerosene_math::Mat4;
    use kerosene_render::gpu::load_model;

    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("kerosene-skin-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("models/test")).unwrap();
    std::fs::write(
        dir.join("models/test/cube.keromdl"),
        cube_model().to_bytes(),
    )
    .unwrap();
    let mut vfs = kerosene_vfs::Vfs::new();
    vfs.add_directory(&dir, "test");

    let mut renderer = Renderer::new(&device, OUTPUT_FORMAT);
    let bsp = lit_quad(0, "missing/on/purpose");
    let atlas = LightmapAtlas::build(&bsp, 1.0);
    let mesh = WorldMesh::build(&bsp, &atlas);
    let resources = MapResources::upload(&device, &queue, &renderer, &mesh, &atlas, &vfs);
    let probes = GpuProbes::upload(&device, &queue, None);
    let frame = renderer.create_frame_bind_group(&device, &resources.lightmap_view, &probes);
    let model = load_model(&device, &queue, &renderer, &vfs, "test/cube").unwrap();
    // Palette slot 1: bone 0 -- every vertex of the cube -- moved 30 along +Y.
    renderer.update_palettes(
        &queue,
        &[vec![Mat4::from_translation(Vec3::new(0.0, 30.0, 0.0))]],
    );

    let camera = Camera {
        position: Vec3::new(0.0, 0.0, 120.0),
        angles: Angles::new(89.0, 0.0, 0.0),
        aspect: 1.0,
        ..Default::default()
    };
    let shoot = |renderer: &mut Renderer, bones: usize| -> Vec<u8> {
        renderer.ensure_targets(&device, SIZE, SIZE);
        renderer.update_camera(&queue, &CameraUniform::from_camera(&camera, 0.0));
        renderer.update_models(&queue, &[]);
        renderer.update_lights(&queue, &LightFrame::build(&[], &camera, SIZE, SIZE));
        renderer.update_tonemap(&queue, 1.0, ToneMapOperator::Aces);
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
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
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (SIZE * SIZE * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = renderer.begin_scene_pass(&mut encoder, wgpu::Color::BLACK);
            renderer.draw_studio_model(&mut pass, &frame, &model, 1, bones);
        }
        renderer.tonemap(&mut encoder, &output.create_view(&Default::default()));
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
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("maps"));
        device.poll(wgpu::PollType::Wait).expect("finishes");
        slice.get_mapped_range().to_vec()
    };

    let rest = shoot(&mut renderer, 0);
    let moved = shoot(&mut renderer, 1);
    let _ = std::fs::remove_dir_all(&dir);

    let centre = (SIZE / 2, SIZE / 2);
    // +Y is screen left, looking down with yaw 0: about 12 pixels over.
    let left = (SIZE / 2 - 12, SIZE / 2);
    let at = |img: &[u8], (x, y): (u32, u32)| brightness(pixel(img, x, y));
    assert!(
        at(&rest, centre) > 0,
        "the identity palette draws it where it is"
    );
    assert_eq!(
        at(&moved, centre),
        0,
        "the palette took it away from the centre"
    );
    assert!(at(&moved, left) > 0, "and put it 30 units along +Y");
}
