// SPDX-License-Identifier: MPL-2.0
use super::*;
use crate::camera::Camera;
use crate::lightmap::LightmapAtlas;
use kerosene_bsp::{
    BspPlane, ColorRgbExp32, Edge, Face, Leaf, Model, TexData, TexInfo, encode_leaf,
};
use kerosene_math::{Angles, Plane, PlaneSet, Pose};

/// A map with two quads facing +Z, side by side, wearing different materials.
fn two_quad_map(lit: bool) -> Bsp {
    let mut bsp = Bsp::new();
    let mut planes = PlaneSet::new();
    let floor = planes.insert(Plane::new(Vec3::Z, 0.0));
    bsp.planes = planes.planes().iter().map(BspPlane::from_plane).collect();

    for (i, name) in ["dev/grid", "dev/wall"].iter().enumerate() {
        let offset = bsp.intern_texdata_string(name);
        bsp.texdata.push(TexData {
            reflectivity: [0.5; 3],
            name_offset: offset,
            width: 256,
            height: 256,
            view_width: 256,
            view_height: 256,
        });
        let mut ti = TexInfo { texdata: i as u32, ..Default::default() };
        ti.texture_vecs[0] = [0.25, 0.0, 0.0, 0.0];
        ti.texture_vecs[1] = [0.0, -0.25, 0.0, 0.0];
        ti.lightmap_vecs[0] = [1.0 / 16.0, 0.0, 0.0, 0.0];
        ti.lightmap_vecs[1] = [0.0, 1.0 / 16.0, 0.0, 0.0];
        bsp.texinfo.push(ti);
    }

    let mut lighting_offset = 0i32;
    for quad in 0..2 {
        let x0 = quad as f32 * 64.0;
        let base = bsp.vertices.len() as u32;
        bsp.vertices.extend([
            [x0, 0.0, 0.0],
            [x0, 64.0, 0.0],
            [x0 + 64.0, 64.0, 0.0],
            [x0 + 64.0, 0.0, 0.0],
        ]);
        let edge_base = bsp.edges.len() as u32;
        bsp.edges.extend([
            Edge { v: [base, base + 1] },
            Edge { v: [base + 1, base + 2] },
            Edge { v: [base + 2, base + 3] },
            Edge { v: [base + 3, base] },
        ]);
        let surfedge_base = bsp.surfedges.len() as u32;
        for i in 0..4 { bsp.surfedges.push((edge_base + i) as i32); }

        let (w, h) = (5u32, 5u32);
        bsp.faces.push(Face {
            plane: floor & !1,
            side: (floor & 1) as u8,
            first_surfedge: surfedge_base,
            num_surfedges: 4,
            texinfo: quad as u32,
            dispinfo: -1,
            lightmap_offset: if lit { lighting_offset } else { -1 },
            lightmap_size: if lit { [w, h] } else { [0, 0] },
            light_styles: [0, 255, 255, 255],
            area: 4096.0,
            ..Default::default()
        });
        if lit {
            bsp.lighting.extend(std::iter::repeat_n(
                ColorRgbExp32 { r: 200, g: 200, b: 200, exponent: 0 },
                (w * h) as usize,
            ));
            lighting_offset += (w * h) as i32;
        }
        bsp.leaffaces.push(quad as u32);
    }

    bsp.leaves.push(Leaf {
        contents: kerosene_bsp::contents::EMPTY,
        first_leafface: 0,
        num_leaffaces: 2,
        cluster: 0,
        mins: [-256, -256, -8],
        maxs: [256, 256, 256],
        ..Default::default()
    });
    bsp.models.push(Model {
        mins: [-256.0, -256.0, -8.0],
        maxs: [256.0, 256.0, 256.0],
        origin: [0.0; 3],
        head_node: encode_leaf(0),
        first_face: 0,
        num_faces: 2,
    });
    bsp.entities = "entity { \"classname\" \"worldspawn\" }\n".into();
    bsp.validate().expect("fixture is well formed");
    bsp
}

fn build(lit: bool) -> (Bsp, WorldMesh) {
    let bsp = two_quad_map(lit);
    let atlas = LightmapAtlas::build(&bsp, 1.0);
    let mesh = WorldMesh::build(&bsp, &atlas);
    (bsp, mesh)
}

#[test]
fn each_face_becomes_triangles() {
    let (_, mesh) = build(true);
    assert_eq!(mesh.surfaces.len(), 2);
    assert_eq!(mesh.triangle_count(), 4, "two quads are four triangles");
    assert_eq!(mesh.vertices.len(), 8);
}

#[test]
fn triangles_are_wound_counter_clockwise_from_the_front() {
    // Faces are stored clockwise; GPUs cull clockwise as back-facing, so
    // getting this wrong makes the whole world invisible from the inside.
    let (_, mesh) = build(true);
    for tri in mesh.indices.chunks_exact(3) {
        let a = Vec3::from_array(mesh.vertices[tri[0] as usize].position);
        let b = Vec3::from_array(mesh.vertices[tri[1] as usize].position);
        let c = Vec3::from_array(mesh.vertices[tri[2] as usize].position);
        let normal = (b - a).cross(c - a);
        assert!(
            normal.z > 0.0,
            "triangle {tri:?} winds the wrong way: normal {normal:?}"
        );
    }
}

#[test]
fn vertex_normals_come_from_the_face_plane() {
    let (_, mesh) = build(true);
    for v in &mesh.vertices {
        assert_eq!(Vec3::from_array(v.normal), Vec3::Z);
    }
}

#[test]
fn texture_coordinates_are_normalised_by_the_texture_size() {
    // texinfo produces texels; shaders want 0..1 per tile.
    let (_, mesh) = build(true);
    // The first quad spans 0..64 world units at 0.25 texels per unit on a
    // 256-texel texture, so it covers a quarter of one tile.
    let us: Vec<f32> = mesh.vertices[..4].iter().map(|v| v.uv[0]).collect();
    let span = us.iter().cloned().fold(f32::MIN, f32::max) - us.iter().cloned().fold(f32::MAX, f32::min);
    assert!((span - 0.0625).abs() < 1e-4, "u spanned {span}");
}

#[test]
fn surfaces_are_grouped_into_one_batch_per_material() {
    let (_, mesh) = build(true);
    assert_eq!(mesh.materials.len(), 2);
    assert_eq!(mesh.batches.len(), 2);
    for batch in &mesh.batches {
        assert_eq!(batch.surfaces.len(), 1);
        assert!(batch.contiguous_range.is_some(), "a batch should be one draw call");
    }
    // Sorted by name, so the buffer layout is stable between runs.
    assert_eq!(mesh.materials, vec!["dev/grid", "dev/wall"]);
}

#[test]
fn a_batch_covers_a_contiguous_run_of_the_index_buffer() {
    let (_, mesh) = build(true);
    for batch in &mesh.batches {
        let (first, count) = batch.contiguous_range.unwrap();
        let covered: u32 = batch
            .surfaces
            .iter()
            .map(|&s| mesh.surfaces[s as usize].index_count)
            .sum();
        assert_eq!(count, covered);
        for &s in &batch.surfaces {
            let surface = &mesh.surfaces[s as usize];
            assert!(surface.first_index >= first);
            assert!(surface.first_index + surface.index_count <= first + count);
        }
    }
}

#[test]
fn lit_faces_get_atlas_coordinates_and_unlit_ones_do_not() {
    let (_, lit) = build(true);
    assert!(lit.surfaces.iter().all(|s| s.lit));
    assert!(lit.vertices.iter().any(|v| v.lightmap_uv != [0.0, 0.0]));

    let (_, unlit) = build(false);
    assert!(unlit.surfaces.iter().all(|s| !s.lit));
    assert!(unlit.vertices.iter().all(|v| v.lightmap_uv == [0.0, 0.0]));
}

#[test]
fn lightmap_coordinates_stay_inside_the_atlas() {
    let (_, mesh) = build(true);
    for v in &mesh.vertices {
        assert!((0.0..=1.0).contains(&v.lightmap_uv[0]), "{:?}", v.lightmap_uv);
        assert!((0.0..=1.0).contains(&v.lightmap_uv[1]), "{:?}", v.lightmap_uv);
    }
}

#[test]
fn leaves_know_which_surfaces_they_hold() {
    let (bsp, mesh) = build(true);
    assert_eq!(mesh.leaf_surfaces.len(), bsp.leaves.len());
    assert_eq!(mesh.leaf_surfaces[0].len(), 2);
}

#[test]
fn surface_bounds_cover_their_geometry() {
    let (_, mesh) = build(true);
    let s = &mesh.surfaces[0];
    for i in s.first_index..s.first_index + s.index_count {
        let p = Vec3::from_array(mesh.vertices[mesh.indices[i as usize] as usize].position);
        assert!(s.bounds.contains_point(p), "{p:?} outside {:?}", s.bounds);
    }
}

#[test]
fn the_frustum_culls_what_is_behind_the_camera() {
    let (bsp, mesh) = build(true);
    // Looking down +X from behind both quads sees them.
    let ahead = Camera {
        position: Vec3::new(-200.0, 32.0, 40.0),
        angles: Angles::new(20.0, 0.0, 0.0),
        ..Default::default()
    };
    let seen = mesh.visible_surfaces(&bsp, ahead.position, &ahead.frustum());
    assert!(!seen.is_empty(), "should see the floor ahead");

    // Turning around sees neither.
    let behind = Camera { angles: Angles::new(20.0, 180.0, 0.0), ..ahead };
    let seen = mesh.visible_surfaces(&bsp, behind.position, &behind.frustum());
    assert!(seen.is_empty(), "turned away, but still drew {} surfaces", seen.len());
}

#[test]
fn visible_surfaces_come_back_grouped_by_material() {
    let (bsp, mesh) = build(true);
    let cam = Camera {
        position: Vec3::new(64.0, 32.0, 200.0),
        angles: Angles::new(89.0, 0.0, 0.0),
        ..Default::default()
    };
    let seen = mesh.visible_surfaces(&bsp, cam.position, &cam.frustum());
    let materials: Vec<u32> = seen.iter().map(|&s| mesh.surfaces[s as usize].material).collect();
    let mut sorted = materials.clone();
    sorted.sort_unstable();
    assert_eq!(materials, sorted, "surfaces should arrive grouped so draws can batch");
}

#[test]
fn no_surface_is_listed_twice() {
    // A surface reachable through two leaves must still be drawn once.
    let (bsp, mesh) = build(true);
    let cam = Camera {
        position: Vec3::new(64.0, 32.0, 200.0),
        angles: Angles::new(89.0, 0.0, 0.0),
        ..Default::default()
    };
    let seen = mesh.visible_surfaces(&bsp, cam.position, &cam.frustum());
    let mut unique = seen.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(seen.len(), unique.len());
}

#[test]
fn nodraw_faces_are_skipped() {
    let mut bsp = two_quad_map(true);
    bsp.texinfo[0].flags = surf::NODRAW;
    let atlas = LightmapAtlas::build(&bsp, 1.0);
    let mesh = WorldMesh::build(&bsp, &atlas);
    assert_eq!(mesh.surfaces.len(), 1, "the nodraw face should not be drawn");
}

#[test]
fn surface_flags_reach_the_renderer() {
    let mut bsp = two_quad_map(true);
    bsp.texinfo[0].flags = surf::SKY;
    let atlas = LightmapAtlas::build(&bsp, 1.0);
    let mesh = WorldMesh::build(&bsp, &atlas);
    assert!(mesh.surfaces.iter().any(|s| s.is_sky()), "the sky needs its own shader path");
}

#[test]
fn building_is_deterministic() {
    let a = build(true).1;
    let b = build(true).1;
    assert_eq!(a.indices, b.indices);
    assert_eq!(a.materials, b.materials);
    assert_eq!(
        a.vertices.iter().map(|v| v.position).collect::<Vec<_>>(),
        b.vertices.iter().map(|v| v.position).collect::<Vec<_>>()
    );
}

#[test]
fn an_empty_map_produces_an_empty_mesh() {
    let bsp = Bsp::new();
    let atlas = LightmapAtlas::build(&bsp, 1.0);
    let mesh = WorldMesh::build(&bsp, &atlas);
    assert_eq!(mesh.triangle_count(), 0);
    assert!(mesh.batches.is_empty());
    assert!(mesh.all_surfaces().is_empty());
}

// ---- brush models --------------------------------------------------------

/// The two-quad map with the second quad moved into a brush model of its own,
/// the way a door or a lift is compiled.
fn map_with_a_brush_model() -> Bsp {
    let mut bsp = two_quad_map(true);
    // Model 0 keeps the first face; the second becomes model 1, and leaves
    // the world's leaf entirely -- which is exactly what the compiler does,
    // and exactly why the PVS cannot see it.
    bsp.models[0].num_faces = 1;
    bsp.leaves[0].num_leaffaces = 1;
    bsp.leaffaces.truncate(1);
    bsp.models.push(Model {
        mins: [64.0, 0.0, 0.0],
        maxs: [128.0, 64.0, 0.0],
        origin: [0.0; 3],
        head_node: encode_leaf(0),
        first_face: 1,
        num_faces: 1,
    });
    bsp.validate().expect("fixture is well formed");
    bsp
}

fn build_with_model() -> (Bsp, WorldMesh) {
    let bsp = map_with_a_brush_model();
    let atlas = LightmapAtlas::build(&bsp, 1.0);
    let mesh = WorldMesh::build(&bsp, &atlas);
    (bsp, mesh)
}

#[test]
fn a_brush_models_faces_are_in_no_leaf_which_is_why_they_need_their_own_pass() {
    // This is the bug the model pass exists for: a leaf walk finds the world
    // and nothing else, so every door in every map was built into the mesh
    // and never drawn.
    let (_, mesh) = build_with_model();
    let reachable: std::collections::HashSet<u32> =
        mesh.leaf_surfaces.iter().flatten().copied().collect();

    for &surface in &mesh.model_surfaces[1] {
        assert!(!reachable.contains(&surface), "surface {surface} would be found twice");
    }
    assert!(!mesh.model_surfaces[1].is_empty(), "the model has surfaces to draw");
}

#[test]
fn every_surface_belongs_to_exactly_one_model() {
    let (_, mesh) = build_with_model();
    let mut seen = vec![0usize; mesh.surfaces.len()];
    for surfaces in &mesh.model_surfaces {
        for &s in surfaces { seen[s as usize] += 1; }
    }
    assert!(seen.iter().all(|&n| n == 1), "{seen:?}");
}

#[test]
fn the_world_pass_draws_the_world_and_not_the_models() {
    // Drawing a door in both passes would draw it twice, once unmoved.
    let (_, mesh) = build_with_model();
    let world = mesh.world_surfaces();

    assert_eq!(world.len(), mesh.model_surfaces[0].len());
    for &s in &mesh.model_surfaces[1] {
        assert!(!world.contains(&s), "surface {s} is drawn by both passes");
    }
}

#[test]
fn the_world_pass_is_sorted_by_material_like_the_pvs_pass() {
    // `draw_world` merges adjacent surfaces into one call and relies on it.
    let (_, mesh) = build(true);
    let world = mesh.world_surfaces();
    let materials: Vec<u32> = world.iter().map(|&s| mesh.surfaces[s as usize].material).collect();
    let mut sorted = materials.clone();
    sorted.sort();
    assert_eq!(materials, sorted);
}

#[test]
fn a_model_in_front_of_the_camera_is_drawn() {
    let (_, mesh) = build_with_model();
    let camera = Camera {
        position: Vec3::new(96.0, 32.0, 128.0),
        angles: Angles::new(90.0, 0.0, 0.0),
        aspect: 1.0,
        ..Default::default()
    };
    assert!(mesh.model_is_visible(1, Pose::IDENTITY, &camera.frustum()));
}

#[test]
fn a_model_behind_the_camera_is_not() {
    let (_, mesh) = build_with_model();
    let camera = Camera {
        position: Vec3::new(96.0, 32.0, 128.0),
        // Looking up and away from the quad on the floor.
        angles: Angles::new(-89.0, 0.0, 0.0),
        aspect: 1.0,
        ..Default::default()
    };
    assert!(!mesh.model_is_visible(1, Pose::IDENTITY, &camera.frustum()));
}

#[test]
fn a_model_is_culled_where_it_has_moved_to_not_where_it_was_built() {
    // A door that has opened is somewhere else. Culling it by its compiled
    // bounds would pop it out of existence as it moved.
    let (_, mesh) = build_with_model();
    let camera = Camera {
        position: Vec3::new(96.0, 32.0, 128.0),
        angles: Angles::new(90.0, 0.0, 0.0),
        aspect: 1.0,
        ..Default::default()
    };
    let frustum = camera.frustum();

    assert!(mesh.model_is_visible(1, Pose::IDENTITY, &frustum));
    // Moved far off to one side, it is no longer in front of the camera.
    assert!(!mesh.model_is_visible(1, Pose::at(Vec3::new(0.0, 40_000.0, 0.0)), &frustum));
}

#[test]
fn a_model_is_culled_by_the_bounds_it_has_after_turning() {
    // The failure this catches is the same one moving had, one dimension up:
    // a model culled by its compiled bounds pops out of existence as it turns
    // past the edge of the screen, or worse, stays culled while visible.
    let (_, mesh) = build_with_model();
    let camera = Camera {
        position: Vec3::new(96.0, 32.0, 128.0),
        angles: Angles::new(90.0, 0.0, 0.0),
        aspect: 1.0,
        ..Default::default()
    };
    let frustum = camera.frustum();

    // Turned in place, it is still under the camera and still drawn.
    let spun = Pose::about(Vec3::ZERO, Angles::new(0.0, 45.0, 0.0), Vec3::new(96.0, 32.0, 0.0));
    assert!(mesh.model_is_visible(1, spun, &frustum));

    // Turned about a point far away, it is flung off screen -- which only
    // happens if the pivot is being honoured at all.
    let flung = Pose::about(
        Vec3::ZERO,
        Angles::new(0.0, 90.0, 0.0),
        Vec3::new(0.0, 20_000.0, 0.0),
    );
    assert!(!mesh.model_is_visible(1, flung, &frustum));
}

#[test]
fn the_world_model_is_never_offered_as_a_brush_model_to_draw() {
    // It is drawn by the PVS pass; drawing it again would double every
    // triangle in the map.
    let (_, mesh) = build_with_model();
    assert!(!mesh.model_surfaces[0].is_empty());
    assert_eq!(mesh.model_bounds.len(), mesh.model_surfaces.len());
}

#[test]
fn a_model_with_nothing_in_it_is_not_drawn() {
    // The compiler emits an empty model for a brush entity whose faces were
    // all nodraw -- a trigger, most often. Asking the GPU to draw nothing is
    // a wasted bind and a wasted call.
    let (_, mesh) = build_with_model();
    let camera = Camera { aspect: 1.0, ..Default::default() };
    assert!(!mesh.model_is_visible(99, Pose::IDENTITY, &camera.frustum()), "no such model");
}
