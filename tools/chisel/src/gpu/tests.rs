// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
use super::*;
use crate::document::Document;
use crate::draw::colors;
use kerosene_math::{Aabb, Angles};

/// Where a world point lands in normalised device coordinates.
fn ndc(m: Mat4, p: Vec3) -> Vec3 {
    let clip = m * p.extend(1.0);
    clip.truncate() / clip.w
}

#[test]
fn a_point_straight_ahead_is_in_the_middle_of_the_pane() {
    let m = view_projection(Vec3::ZERO, Angles::ZERO.vectors(), 90.0, 4.0 / 3.0);
    let p = ndc(m, Vec3::new(100.0, 0.0, 0.0));
    assert!(p.x.abs() < 1e-5 && p.y.abs() < 1e-5, "{p}");
    assert!(p.z > 0.0 && p.z < 1.0, "depth in range: {}", p.z);
}

#[test]
fn nearer_is_less_deep() {
    let m = view_projection(Vec3::ZERO, Angles::ZERO.vectors(), 90.0, 1.0);
    let near = ndc(m, Vec3::new(10.0, 0.0, 0.0)).z;
    let far = ndc(m, Vec3::new(1000.0, 0.0, 0.0)).z;
    assert!(near < far);
}

#[test]
fn the_matrix_agrees_with_the_pick_ray() {
    // Where a click at a pixel casts its ray, the matrix must draw that
    // same point at that same pixel -- or clicks select the wrong thing.
    let mut viewport = crate::viewport::Viewport::new(crate::viewport::ViewportKind::Perspective);
    viewport.size = (640.0, 360.0);
    viewport.eye = Vec3::new(-50.0, 20.0, 64.0);
    viewport.angles = Angles::new(20.0, 35.0, 0.0);
    let m = view_projection(
        viewport.eye,
        viewport.angles.vectors(),
        viewport.fov,
        640.0 / 360.0,
    );
    for (x, y) in [(100.0, 50.0), (320.0, 180.0), (600.0, 300.0)] {
        let (origin, direction) = viewport.pick_ray(x, y);
        let p = ndc(m, origin + direction * 300.0);
        let px = (p.x * 0.5 + 0.5) * 640.0;
        let py = (0.5 - p.y * 0.5) * 360.0;
        assert!(
            (px - x).abs() < 0.05 && (py - y).abs() < 0.05,
            "{x},{y} -> {px},{py}"
        );
    }
}

#[test]
fn a_face_wound_towards_the_camera_is_counter_clockwise_on_screen() {
    // The face pipelines cull clockwise triangles, so this is what decides
    // whether a brush is drawn from outside or inside out.
    let m = view_projection(
        Vec3::new(-256.0, 0.0, 0.0),
        Angles::ZERO.vectors(),
        90.0,
        1.0,
    );
    let mut document = Document::new();
    document.create_block(Vec3::splat(-64.0), Vec3::splat(64.0));
    let scene = scene::build(
        &document,
        &[],
        &mut scene::Options {
            shading: crate::raster::Shading::Shaded,
            textures: None,
            models: None,
        },
    );
    let mut facing_us = 0;
    for triangle in scene.triangles.chunks(3) {
        let normal = Vec3::from_array(triangle[0].normal);
        if normal.dot(Vec3::NEG_X) < 0.99 {
            continue;
        }
        let [a, b, c] = [0, 1, 2].map(|i| ndc(m, Vec3::from_array(triangle[i].position)));
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        assert!(area > 0.0, "front face wound clockwise: {area}");
        facing_us += 1;
    }
    assert_eq!(facing_us, 2, "the near face is two triangles");
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
        label: Some("chisel-test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()
}

const SIZE: u32 = 64;

/// Render a document from a camera on a real device, and read it back.
pub(crate) fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    document: &Document,
    helpers: &[crate::helpers::Helper],
    eye: Vec3,
    angles: Angles,
    models: &mut dyn FnMut(&str) -> Option<Arc<kerosene_asset::Model>>,
) -> Vec<[u8; 4]> {
    let scene = scene::build(
        document,
        helpers,
        &mut scene::Options {
            shading: crate::raster::Shading::Shaded,
            textures: None,
            models: Some(models),
        },
    );
    let background = scene::linear(colors::BACKGROUND);
    render_offscreen(
        device,
        queue,
        Arc::new(scene),
        &CameraUniform::new(eye, angles.vectors(), 90.0, 1.0),
        [SIZE, SIZE],
        [background[0], background[1], background[2]],
    )
}

fn at(pixels: &[[u8; 4]], x: u32, y: u32) -> [u8; 4] {
    pixels[(y * SIZE + x) as usize]
}

#[test]
fn a_brush_draws_on_the_gpu_and_the_sky_around_it_stays_background() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let mut document = Document::new();
    document.create_block(Vec3::splat(-64.0), Vec3::splat(64.0));
    document.selection.clear();
    let pixels = render(
        &device,
        &queue,
        &document,
        &[],
        Vec3::new(-400.0, 0.0, 0.0),
        Angles::ZERO,
        &mut |_| None,
    );
    let bg = colors::BACKGROUND;
    let centre = at(&pixels, SIZE / 2, SIZE / 2);
    assert_ne!(
        &centre[..3],
        &[bg.r(), bg.g(), bg.b()],
        "the brush is drawn"
    );
    // Grey, and lit: the shaded view's brush colour darkened by facing.
    assert!(centre[0] > 60 && centre[0] < 200, "{centre:?}");
    let corner = at(&pixels, 0, 0);
    assert_eq!(
        &corner[..3],
        &[bg.r(), bg.g(), bg.b()],
        "nothing in the corner"
    );
}

#[test]
fn a_prop_is_drawn_as_its_model_and_not_as_a_marker() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let mut document = Document::new();
    let id = document.create_entity("prop_static", Vec3::ZERO);
    document.selection.clear();
    // A tall thin model: a marker box would be square on screen, the model
    // is not -- so its top shows at a height a marker's never would.
    let mut model = kerosene_asset::Model::new();
    let solid = kerosene_map::Solid::cube(
        Aabb::new(Vec3::new(-4.0, -4.0, -8.0), Vec3::new(4.0, 4.0, 120.0)),
        "x",
    );
    for (side, winding) in solid.face_windings() {
        let normal = side.plane().unwrap().normal;
        let base = model.vertices.len() as u32;
        let mut points = winding.points.clone();
        let facing = (points[1] - points[0])
            .cross(points[2] - points[0])
            .dot(normal);
        if facing < 0.0 {
            points.reverse();
        }
        for p in &points {
            model
                .vertices
                .push(kerosene_asset::Vertex::rigid(*p, normal, [0.0, 0.0]));
        }
        for i in 1..points.len() as u32 - 1 {
            model.indices.extend([base, base + i, base + i + 1]);
        }
    }
    model.recompute_bounds();
    let model = Arc::new(model);
    let entity = document.find_entity(id).unwrap().clone();
    let helpers = vec![crate::helpers::Helper::Model {
        owner: Some(entity.id),
        path: "tall".into(),
        pose: kerosene_math::Pose::new(entity.origin(), entity.angles()),
        selected: false,
        opacity: 1.0,
    }];
    let pixels = render(
        &device,
        &queue,
        &document,
        &helpers,
        Vec3::new(-128.0, 0.0, 40.0),
        Angles::ZERO,
        &mut |_| Some(model.clone()),
    );
    let bg = colors::BACKGROUND;
    let is_bg = |p: [u8; 4]| p[..3] == [bg.r(), bg.g(), bg.b()];
    // Well above where an 8-unit marker would end, the model is there.
    assert!(
        !is_bg(at(&pixels, SIZE / 2, 8)),
        "the model's upper half is drawn"
    );
    assert!(is_bg(at(&pixels, 0, 8)), "and only the model");
}
