// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

/// A minimal but structurally complete map: one plane splitting space into an
/// empty leaf above and a solid leaf below, with one square face on the plane.
fn tiny_bsp() -> Bsp {
    let mut bsp = Bsp::new();

    // Plane pair: +Z at the origin, and its inverse.
    bsp.planes.push(BspPlane::from_plane(&Plane::new(Vec3::Z, 0.0)));
    bsp.planes.push(BspPlane::from_plane(&Plane::new(-Vec3::Z, 0.0)));

    bsp.vertices = vec![
        [0.0, 0.0, 0.0],
        [64.0, 0.0, 0.0],
        [64.0, 64.0, 0.0],
        [0.0, 64.0, 0.0],
    ];
    bsp.edges = vec![
        Edge { v: [0, 1] },
        Edge { v: [1, 2] },
        Edge { v: [2, 3] },
        Edge { v: [3, 0] },
    ];
    bsp.surfedges = vec![0, 1, 2, 3];

    let name = bsp.intern_texdata_string("dev/grid");
    bsp.texdata.push(TexData {
        reflectivity: [0.5, 0.5, 0.5],
        name_offset: name,
        width: 512,
        height: 512,
        view_width: 512,
        view_height: 512,
    });
    let mut ti = TexInfo { texdata: 0, ..Default::default() };
    ti.texture_vecs[0] = [1.0, 0.0, 0.0, 0.0];
    ti.texture_vecs[1] = [0.0, -1.0, 0.0, 0.0];
    bsp.texinfo.push(ti);

    bsp.faces.push(Face {
        plane: 0,
        side: 0,
        on_node: 1,
        first_surfedge: 0,
        num_surfedges: 4,
        texinfo: 0,
        dispinfo: -1,
        lightmap_offset: -1,
        area: 4096.0,
        light_styles: [0, 255, 255, 255],
        ..Default::default()
    });

    bsp.nodes.push(Node {
        plane: 0,
        children: [encode_leaf(0), encode_leaf(1)],
        mins: [-128; 3],
        maxs: [128; 3],
        first_face: 0,
        num_faces: 1,
        area: 0,
    });

    bsp.leaves.push(Leaf {
        contents: contents::EMPTY,
        cluster: 0,
        first_leafface: 0,
        num_leaffaces: 1,
        mins: [-128, -128, 0],
        maxs: [128, 128, 128],
        ..Default::default()
    });
    bsp.leaves.push(Leaf {
        contents: contents::SOLID,
        cluster: -1,
        mins: [-128, -128, -128],
        maxs: [128, 128, 0],
        ..Default::default()
    });
    bsp.leaffaces.push(0);

    bsp.models.push(Model {
        mins: [-128.0; 3],
        maxs: [128.0; 3],
        origin: [0.0; 3],
        head_node: 0,
        first_face: 0,
        num_faces: 1,
    });

    bsp.entities = "entity { \"classname\" \"worldspawn\" }\n".to_string();
    bsp
}

#[test]
fn a_synthetic_map_validates() {
    tiny_bsp().validate().expect("the fixture should be well formed");
}

#[test]
fn walking_the_tree_finds_the_right_leaf() {
    let bsp = tiny_bsp();
    assert_eq!(bsp.point_leaf(Vec3::new(0.0, 0.0, 32.0)), 0, "above the plane");
    assert_eq!(bsp.point_leaf(Vec3::new(0.0, 0.0, -32.0)), 1, "below the plane");
    assert!(!bsp.point_is_solid(Vec3::new(0.0, 0.0, 32.0)));
    assert!(bsp.point_is_solid(Vec3::new(0.0, 0.0, -32.0)));
    assert_eq!(bsp.point_cluster(Vec3::new(0.0, 0.0, 32.0)), 0);
    assert_eq!(bsp.point_cluster(Vec3::new(0.0, 0.0, -32.0)), -1, "solid leaves have no cluster");
}

#[test]
fn a_point_exactly_on_a_plane_lands_in_front() {
    // Ties have to break deterministically or a player standing precisely on a
    // node plane flickers between leaves.
    let bsp = tiny_bsp();
    assert_eq!(bsp.point_leaf(Vec3::new(10.0, 10.0, 0.0)), 0);
}

#[test]
fn face_vertices_follow_the_surfedge_ring() {
    let bsp = tiny_bsp();
    let verts = bsp.face_vertices(0);
    assert_eq!(verts.len(), 4);
    assert_eq!(verts[0], Vec3::new(0.0, 0.0, 0.0));
    assert_eq!(verts[1], Vec3::new(64.0, 0.0, 0.0));
    assert_eq!(verts[2], Vec3::new(64.0, 64.0, 0.0));
    assert_eq!(verts[3], Vec3::new(0.0, 64.0, 0.0));
}

#[test]
fn a_negative_surfedge_walks_its_edge_backwards() {
    // This indirection is the whole point of the edge lump; getting the
    // direction wrong silently reverses face winding.
    let mut bsp = tiny_bsp();
    bsp.surfedges = vec![-1, 0, 1, 2]; // edge 1 reversed, then 0,1,2 forward
    let verts = bsp.face_vertices(0);
    assert_eq!(verts[0], Vec3::new(64.0, 64.0, 0.0), "reversed edge should yield v[1]");
}

#[test]
fn materials_read_back_from_the_string_lump() {
    let bsp = tiny_bsp();
    assert_eq!(bsp.texdata_name(0), "dev/grid");
    assert_eq!(bsp.face_material(0), "dev/grid");
    assert_eq!(bsp.materials(), vec!["dev/grid"]);
}

#[test]
fn interning_a_material_twice_reuses_the_entry() {
    let mut bsp = Bsp::new();
    let a = bsp.intern_texdata_string("tools/nodraw");
    let b = bsp.intern_texdata_string("dev/grid");
    let c = bsp.intern_texdata_string("tools/nodraw");
    assert_eq!(a, c);
    assert_ne!(a, b);
    assert_eq!(bsp.texdata_strings.len(), "tools/nodraw".len() + "dev/grid".len() + 2);
}

#[test]
fn interning_does_not_match_a_prefix() {
    // "dev" must not be found inside "dev/grid".
    let mut bsp = Bsp::new();
    bsp.intern_texdata_string("dev/grid");
    let a = bsp.intern_texdata_string("dev");
    assert_eq!(read_c_string(&bsp.texdata_strings, a as usize), "dev");
}

#[test]
fn round_trips_through_bytes() {
    let bsp = tiny_bsp();
    let bytes = bsp.to_bytes();
    let back = Bsp::from_bytes(&bytes, "test.kerobsp").expect("should reload");

    assert_eq!(back.planes.len(), bsp.planes.len());
    assert_eq!(back.faces.len(), bsp.faces.len());
    assert_eq!(back.vertices, bsp.vertices);
    assert_eq!(back.entities, bsp.entities);
    assert_eq!(back.face_material(0), "dev/grid");
    assert_eq!(back.face_vertices(0), bsp.face_vertices(0));
    // And byte-identical on a second write.
    assert_eq!(back.to_bytes(), bytes);
}

#[test]
fn every_lump_starts_four_byte_aligned() {
    // Records are cast straight out of the buffer, so a misaligned lump would
    // be a correctness problem on strict platforms and a slow path elsewhere.
    let mut bsp = tiny_bsp();
    bsp.entities = "x".repeat(37); // deliberately odd length
    let bytes = bsp.to_bytes();
    let dir: &[LumpDir] = bytemuck::cast_slice(&bytes[8..8 + LUMP_COUNT * 16]);
    for (i, d) in dir.iter().enumerate() {
        assert_eq!(d.offset % 4, 0, "lump {i} ({}) is misaligned", lumps::NAMES[i]);
    }
}

#[test]
fn garbage_and_wrong_versions_are_rejected() {
    assert!(matches!(
        Bsp::from_bytes(&vec![0u8; 512], "x"),
        Err(BspError::BadMagic { .. })
    ));
    let mut bytes = tiny_bsp().to_bytes();
    bytes[4] = 99; // bump the version
    assert!(matches!(Bsp::from_bytes(&bytes, "x"), Err(BspError::BadVersion { found: 99, .. })));
    assert!(Bsp::from_bytes(b"KROS", "x").is_err(), "a truncated header must not be read");
}

#[test]
fn validation_catches_dangling_indices() {
    let mut bsp = tiny_bsp();
    bsp.faces[0].plane = 99;
    assert!(bsp.validate().unwrap_err().contains("plane"));

    let mut bsp = tiny_bsp();
    bsp.edges[0].v[0] = 99;
    assert!(bsp.validate().unwrap_err().contains("vertex"));

    let mut bsp = tiny_bsp();
    bsp.nodes[0].children[0] = 50;
    assert!(bsp.validate().unwrap_err().contains("node"));

    let mut bsp = tiny_bsp();
    bsp.leaves[0].num_leaffaces = 99;
    assert!(bsp.validate().unwrap_err().contains("leafface"));

    let mut bsp = tiny_bsp();
    bsp.faces[0].num_surfedges = 99;
    assert!(bsp.validate().unwrap_err().contains("surfedge"));
}

#[test]
fn a_file_with_a_dangling_index_fails_to_load() {
    // Validation must run at load, not only when asked.
    let mut bsp = tiny_bsp();
    bsp.faces[0].texinfo = 42;
    let bytes = bsp.to_bytes();
    assert!(matches!(Bsp::from_bytes(&bytes, "x"), Err(BspError::Invalid { .. })));
}

#[test]
fn a_cyclic_tree_terminates_instead_of_hanging() {
    // A broken compile could emit a node pointing at itself. The engine must
    // return a wrong leaf, not lock up.
    let mut bsp = tiny_bsp();
    bsp.nodes[0].children[0] = 0;
    let _ = bsp.point_leaf(Vec3::new(0.0, 0.0, 32.0));
}

#[test]
fn without_vis_data_everything_is_visible() {
    let bsp = tiny_bsp();
    assert_eq!(bsp.visible_leaves(0).len(), bsp.leaves.len());
    assert!(bsp.cluster_visible(0, 5));
}

#[test]
fn compiled_vis_culls_leaves() {
    let mut bsp = tiny_bsp();
    // Give both leaves real clusters, then make cluster 0 see only itself.
    bsp.leaves[1].cluster = 1;
    bsp.leaves[1].contents = contents::EMPTY;
    let mut vb = VisBuilder::new(2);
    vb.set_visible(0, 0);
    vb.set_visible(1, 0);
    vb.set_visible(1, 1);
    vb.derive_pas();
    bsp.visibility = vb.build();

    assert_eq!(bsp.visible_leaves(0), vec![0]);
    assert_eq!(bsp.visible_leaves(1), vec![0, 1]);
    assert!(!bsp.cluster_visible(0, 1));
    assert!(bsp.cluster_visible(1, 0));
}

#[test]
fn entity_lump_parses() {
    let bsp = tiny_bsp();
    let kv = bsp.entities_kv().unwrap();
    assert_eq!(kv.block("entity").unwrap().get("classname"), Some("worldspawn"));
}

#[test]
fn face_lightmap_slices_the_lighting_lump() {
    let mut bsp = tiny_bsp();
    bsp.lighting = vec![ColorRgbExp32 { r: 10, g: 20, b: 30, exponent: 0 }; 16];
    bsp.faces[0].lightmap_offset = 4;
    bsp.faces[0].lightmap_size = [2, 3];
    let lm = bsp.face_lightmap(0).unwrap();
    assert_eq!(lm.len(), 6);

    bsp.faces[0].lightmap_offset = -1;
    assert!(bsp.face_lightmap(0).is_none(), "an unlit face has no samples");
}

#[test]
fn stats_reports_every_lump() {
    let bsp = tiny_bsp();
    let stats = bsp.stats();
    assert_eq!(stats.iter().find(|(n, _)| *n == "faces").unwrap().1, 1);
    assert_eq!(stats.iter().find(|(n, _)| *n == "leaves").unwrap().1, 2);
}
