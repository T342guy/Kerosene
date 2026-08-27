use super::*;
use crate::camera::Camera;
use crate::lightmap::LightmapAtlas;
use void_bsp::{
    BspPlane, ColorRgbExp32, Edge, Face, Leaf, Model, TexData, TexInfo, encode_leaf,
};
use void_math::{Angles, Plane, PlaneSet};

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
        contents: void_bsp::contents::EMPTY,
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
