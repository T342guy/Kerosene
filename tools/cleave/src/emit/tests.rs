// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! End-to-end compiler tests.
//!
//! These build a map in memory, run the whole pipeline, and check the compiled
//! result -- which is the only way to catch the failures that matter, since
//! every stage can look correct in isolation and still produce an unloadable
//! map together.

use crate::pipeline::{CompileOptions, compile};
use kerosene_bsp::contents;
use kerosene_map::{Connection, Map, Solid};
use kerosene_math::{Aabb, Vec3};

/// A sealed room: six slabs enclosing a 256-unit cube of air.
///
/// `hole` leaves a gap in one wall so leak handling can be exercised.
fn room_map(hole: bool) -> Map {
    let mut map = Map::new();
    let t = 16.0;
    let (lo, hi) = (0.0f32, 256.0);
    let front_hi = if hole { hi - 32.0 } else { hi + t };

    let slabs = [
        Aabb::new(
            Vec3::new(lo - t, lo - t, lo - t),
            Vec3::new(hi + t, hi + t, lo),
        ), // floor
        Aabb::new(
            Vec3::new(lo - t, lo - t, hi),
            Vec3::new(hi + t, hi + t, hi + t),
        ), // ceiling
        Aabb::new(Vec3::new(lo - t, lo - t, lo), Vec3::new(lo, hi + t, hi)), // -X
        Aabb::new(Vec3::new(hi, lo - t, lo), Vec3::new(hi + t, hi + t, hi)), // +X
        Aabb::new(Vec3::new(lo, lo - t, lo), Vec3::new(front_hi, lo, hi)),   // -Y
        Aabb::new(Vec3::new(lo, hi, lo), Vec3::new(hi, hi + t, hi)),         // +Y
    ];
    for slab in slabs {
        map.add_world_solid(Solid::cube(slab, "dev/grid"));
    }

    let id = map.next_id();
    let mut spawn = kerosene_map::Entity::new(id, "info_player_start");
    spawn.set_origin(Vec3::new(128.0, 128.0, 32.0));
    map.entities.push(spawn);

    map
}

fn compile_ok(map: &Map) -> crate::pipeline::CompileOutput {
    compile(map, &CompileOptions::default()).expect("the map should compile")
}

#[test]
fn a_sealed_room_compiles_to_a_valid_map() {
    let out = compile_ok(&room_map(false));
    out.bsp
        .validate()
        .expect("the compiled map must be structurally sound");
    assert!(out.leak.is_none());
    assert!(
        out.bsp.faces.len() >= 6,
        "a room has at least six visible faces"
    );
    assert!(!out.bsp.vertices.is_empty());
    assert!(!out.bsp.nodes.is_empty());
    assert!(out.stats.clusters > 0);
}

#[test]
fn the_compiled_map_survives_a_file_round_trip() {
    let out = compile_ok(&room_map(false));
    let bytes = out.bsp.to_bytes();
    let back = kerosene_bsp::Bsp::from_bytes(&bytes, "room.kerobsp").expect("should reload");
    assert_eq!(back.faces.len(), out.bsp.faces.len());
    assert_eq!(back.leaves.len(), out.bsp.leaves.len());
    assert_eq!(back.to_bytes(), bytes, "writing must be stable");
}

#[test]
fn the_air_is_open_and_the_walls_are_solid() {
    let out = compile_ok(&room_map(false));
    let bsp = &out.bsp;

    assert!(
        !bsp.point_is_solid(Vec3::new(128.0, 128.0, 128.0)),
        "the room's air"
    );
    for p in [
        Vec3::new(128.0, 128.0, -8.0),  // floor
        Vec3::new(128.0, 128.0, 264.0), // ceiling
        Vec3::new(-8.0, 128.0, 128.0),  // -X wall
        Vec3::new(128.0, -8.0, 128.0),  // -Y wall
    ] {
        assert!(bsp.point_is_solid(p), "{p:?} should be inside a wall");
    }
}

#[test]
fn the_space_outside_a_sealed_map_is_filled_in() {
    // The payoff of sealing: everything beyond the walls becomes solid, so the
    // map's entire outer shell stops existing.
    let out = compile_ok(&room_map(false));
    assert!(out.stats.leaves_filled > 0, "nothing outside was filled");
    assert!(
        out.bsp.point_is_solid(Vec3::new(128.0, 128.0, 1000.0)),
        "far outside the map should be solid after filling"
    );
}

#[test]
fn outer_faces_are_removed_from_a_sealed_map() {
    // Every face the compile keeps should face into open space. A face on the
    // *outside* of the room can never be seen and must not survive.
    let out = compile_ok(&room_map(false));
    let bsp = &out.bsp;
    for i in 0..bsp.faces.len() {
        let verts = bsp.face_vertices(i);
        if verts.is_empty() {
            continue;
        }
        let plane = bsp.face_plane(i).unwrap();
        let center: Vec3 = verts.iter().copied().sum::<Vec3>() / verts.len() as f32;
        let in_front = center + plane.normal * 2.0;
        assert!(
            !bsp.point_is_solid(in_front),
            "face {i} at {center:?} faces into solid rock and should have been removed"
        );
    }
}

#[test]
fn texture_sizes_are_written_from_the_caller_and_defaulted_with_a_warning() {
    let mut options = CompileOptions::default();
    options
        .texture_sizes
        .insert("DEV/GRID".to_string(), (256, 128));
    let out = compile(&room_map(false), &options).expect("compiles");
    let td = &out.bsp.texdata[0];
    assert_eq!(out.bsp.texdata_name(0), "dev/grid");
    assert_eq!(
        (td.width, td.height),
        (256, 128),
        "sized from the caller, case aside"
    );
    let stray: Vec<_> = out
        .warnings
        .iter()
        .filter(|w| w.message.contains("no compiled texture"))
        .map(|w| w.message.clone())
        .collect();
    assert!(stray.is_empty(), "{stray:?}");

    let out = compile_ok(&room_map(false));
    let td = &out.bsp.texdata[0];
    assert_eq!((td.width, td.height), crate::emit::DEFAULT_TEXTURE_SIZE);
    assert!(
        out.warnings.iter().any(|w| w.message.contains("dev/grid")),
        "an unsized material is named in a warning"
    );
}

#[test]
fn the_walkmap_holds_only_floors_inside_the_room() {
    // The top of the ceiling slab is an upward face too, on the outside of
    // the world; it used to be a walkmap floor an NPC could path across.
    let out = compile_ok(&room_map(false));
    assert!(!out.walk.is_empty(), "the room has a floor");
    for face in &out.walk.faces {
        let center: Vec3 = face.vertices.iter().copied().sum::<Vec3>() / face.vertices.len() as f32;
        assert!(
            !out.bsp.point_is_solid(center + face.normal * 2.0),
            "walk face at {center:?} is on the outside of the map"
        );
        assert!(
            center.z < 1.0,
            "the only floor is at z = 0, not the roof: {center:?}"
        );
    }
}

#[test]
fn a_leaking_map_is_refused_by_default() {
    match compile(&room_map(true), &CompileOptions::default()) {
        Err(crate::pipeline::CompileError::Leaked(_)) => {}
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("a leaking map must not compile by default"),
    }
}

#[test]
fn a_leaking_map_compiles_with_the_override_and_reports_the_route() {
    let options = CompileOptions {
        ignore_leaks: true,
        ..Default::default()
    };
    let out = compile(&room_map(true), &options).expect("should build anyway");
    let leak = out.leak.expect("the leak must still be reported");
    assert!(leak.points.len() >= 2);
    out.bsp
        .validate()
        .expect("even a leaking map must be structurally valid");
}

#[test]
fn a_leaking_map_is_not_filled_solid() {
    // Filling a map that leaks would turn the whole level to rock, which is
    // far more confusing to debug than leaving it open.
    let options = CompileOptions {
        ignore_leaks: true,
        ..Default::default()
    };
    let out = compile(&room_map(true), &options).unwrap();
    assert_eq!(out.stats.leaves_filled, 0);
    assert!(!out.bsp.point_is_solid(Vec3::new(128.0, 128.0, 128.0)));
}

#[test]
fn the_entity_lump_carries_every_entity() {
    let out = compile_ok(&room_map(false));
    let kv = out.bsp.entities_kv().expect("entity lump should parse");
    let classes: Vec<&str> = kv
        .blocks("entity")
        .filter_map(|e| e.get("classname"))
        .collect();
    assert!(classes.contains(&"worldspawn"));
    assert!(classes.contains(&"info_player_start"));
}

#[test]
fn brush_entities_become_numbered_models() {
    let mut map = room_map(false);
    let id = map.next_id();
    let mut door = kerosene_map::Entity::new(id, "func_door");
    door.set("targetname", "door1");
    let mut solid = Solid::cube(
        Aabb::new(Vec3::new(64.0, 120.0, 0.0), Vec3::new(96.0, 128.0, 96.0)),
        "dev/grid",
    );
    solid.id = map.next_id();
    for s in &mut solid.sides {
        s.id = map.next_id();
    }
    door.solids.push(solid);
    door.connect(Connection::new("OnFullyOpen", "relay", "Trigger").with_delay(0.5));
    map.entities.push(door);

    let out = compile_ok(&map);
    assert_eq!(out.bsp.models.len(), 2, "world plus one brush model");

    let kv = out.bsp.entities_kv().unwrap();
    let door_kv = kv
        .blocks("entity")
        .find(|e| e.get("classname") == Some("func_door"))
        .expect("the door should be in the entity lump");
    assert_eq!(
        door_kv.get("model"),
        Some("*1"),
        "brush entities are addressed by model index"
    );
    assert_eq!(
        kv.blocks("entity")
            .find(|e| e.get("classname") == Some("worldspawn"))
            .unwrap()
            .get("model"),
        Some("*0")
    );

    // The connection must survive the compile.
    let conn = door_kv
        .block("connections")
        .expect("connections should be preserved");
    assert!(
        conn.get("OnFullyOpen")
            .unwrap()
            .starts_with("relay,Trigger")
    );

    // And the door's own geometry becomes faces on its model.
    let model = out.bsp.models[1];
    assert!(model.num_faces > 0, "the door should have visible faces");
}

#[test]
fn a_moving_brush_entity_does_not_carve_the_world() {
    // The door sits flush inside the room; if entity brushes cut world faces,
    // the floor beneath it would gain a hole that stays behind when it opens.
    let mut plain = room_map(false);
    let plain_faces = compile_ok(&plain).bsp.faces.len();

    let id = plain.next_id();
    let mut door = kerosene_map::Entity::new(id, "func_door");
    let mut solid = Solid::cube(
        Aabb::new(Vec3::new(64.0, 64.0, 0.0), Vec3::new(96.0, 96.0, 96.0)),
        "dev/grid",
    );
    solid.id = plain.next_id();
    for s in &mut solid.sides {
        s.id = plain.next_id();
    }
    door.solids.push(solid);
    plain.entities.push(door);

    let out = compile_ok(&plain);
    let world_faces = out.bsp.models[0].num_faces as usize;
    assert_eq!(
        world_faces, plain_faces,
        "the world's faces must be unchanged by an entity"
    );
}

#[test]
fn nodraw_faces_never_reach_the_face_lump() {
    let mut map = Map::new();
    let t = 16.0;
    let (lo, hi) = (0.0f32, 256.0);
    // Same room, but every surface is nodraw.
    for slab in [
        Aabb::new(
            Vec3::new(lo - t, lo - t, lo - t),
            Vec3::new(hi + t, hi + t, lo),
        ),
        Aabb::new(
            Vec3::new(lo - t, lo - t, hi),
            Vec3::new(hi + t, hi + t, hi + t),
        ),
        Aabb::new(Vec3::new(lo - t, lo - t, lo), Vec3::new(lo, hi + t, hi)),
        Aabb::new(Vec3::new(hi, lo - t, lo), Vec3::new(hi + t, hi + t, hi)),
        Aabb::new(Vec3::new(lo, lo - t, lo), Vec3::new(hi + t, lo, hi)),
        Aabb::new(Vec3::new(lo, hi, lo), Vec3::new(hi, hi + t, hi)),
    ] {
        map.add_world_solid(Solid::cube(slab, "tools/nodraw"));
    }
    let id = map.next_id();
    let mut spawn = kerosene_map::Entity::new(id, "info_player_start");
    spawn.set_origin(Vec3::splat(128.0));
    map.entities.push(spawn);

    let out = compile_ok(&map);
    assert_eq!(
        out.bsp.faces.len(),
        0,
        "nodraw geometry must not produce faces"
    );
    // But it still seals and still collides.
    assert!(out.leak.is_none());
    assert!(out.bsp.point_is_solid(Vec3::new(128.0, 128.0, -8.0)));
    assert!(
        !out.bsp.brushes.is_empty(),
        "collision brushes must survive"
    );
}

#[test]
fn adjacent_faces_share_welded_vertices() {
    // If two faces meeting at an edge do not share vertex records, their seam
    // cracks open. Check that the vertex count is far below the naive total.
    let out = compile_ok(&room_map(false));
    let loose: usize = out.bsp.faces.iter().map(|f| f.num_surfedges as usize).sum();
    assert!(
        out.bsp.vertices.len() < loose,
        "{} vertices for {loose} face corners: nothing was welded",
        out.bsp.vertices.len()
    );
}

#[test]
fn shared_edges_are_reused_in_both_directions() {
    let out = compile_ok(&room_map(false));
    let reversed = out.bsp.surfedges.iter().filter(|&&s| s < 0).count();
    assert!(reversed > 0, "no edge was shared between two faces");
}

#[test]
fn every_face_references_a_real_material() {
    let out = compile_ok(&room_map(false));
    assert!(out.bsp.materials().contains(&"dev/grid"));
    for i in 0..out.bsp.faces.len() {
        assert!(
            !out.bsp.face_material(i).is_empty(),
            "face {i} has no material"
        );
    }
}

#[test]
fn faces_get_lightmap_extents_but_no_samples_yet() {
    // Cleave sizes the lightmaps; Radiance fills them. A face must arrive at
    // Radiance knowing how many luxels it needs.
    let out = compile_ok(&room_map(false));
    let lit: Vec<_> = out
        .bsp
        .faces
        .iter()
        .filter(|f| f.lightmap_size[0] > 0)
        .collect();
    assert!(!lit.is_empty(), "room walls should want lightmaps");
    for f in &lit {
        assert!(
            f.lightmap_size[0] <= 64 && f.lightmap_size[1] <= 64,
            "lightmap is unbounded"
        );
        assert_eq!(
            f.lightmap_offset, -1,
            "Cleave must not claim to have baked lighting"
        );
    }
    assert!(out.bsp.lighting.is_empty());
}

#[test]
fn detail_brushes_do_not_split_the_world_tree() {
    let plain = compile_ok(&room_map(false));

    let mut with_detail = room_map(false);
    // A row of pillars, which as world geometry would carve the room up badly.
    for i in 0..6 {
        let x = 32.0 + i as f32 * 32.0;
        let id = with_detail.next_id();
        let mut e = kerosene_map::Entity::new(id, "func_detail");
        let mut s = Solid::cube(
            Aabb::new(Vec3::new(x, 32.0, 0.0), Vec3::new(x + 8.0, 40.0, 256.0)),
            "dev/grid",
        );
        s.id = with_detail.next_id();
        for side in &mut s.sides {
            side.id = with_detail.next_id();
        }
        e.solids.push(s);
        with_detail.entities.push(e);
    }
    let detailed = compile_ok(&with_detail);

    assert_eq!(
        detailed.stats.tree_nodes, plain.stats.tree_nodes,
        "detail geometry must leave the structural tree alone"
    );
    assert!(
        detailed.bsp.faces.len() > plain.bsp.faces.len(),
        "but it still draws"
    );
}

#[test]
fn clip_brushes_block_without_drawing() {
    // Authored the normal way: a world brush textured with tools/clip. (On a
    // brush entity the classname would decide the contents instead, which is
    // why clip volumes belong in the world.)
    let mut map = room_map(false);
    map.add_world_solid(Solid::cube(
        Aabb::new(Vec3::new(100.0, 100.0, 0.0), Vec3::new(140.0, 140.0, 128.0)),
        "tools/clip",
    ));

    let out = compile_ok(&map);
    let clip_brush = out
        .bsp
        .brushes
        .iter()
        .find(|b| b.contents & contents::PLAYER_CLIP != 0)
        .expect("the clip brush should reach the collision lumps");
    assert!(
        clip_brush.contents & contents::SOLID == 0,
        "a clip brush is not world solid"
    );
    // It blocks players but never becomes a drawable face.
    assert!(contents::MASK_PLAYER_SOLID & clip_brush.contents != 0);
    for i in 0..out.bsp.faces.len() {
        assert_ne!(out.bsp.face_material(i), "tools/clip", "clip must not draw");
    }
}

#[test]
fn compiling_is_deterministic() {
    // Two compiles of the same source must produce byte-identical output, or
    // content patching and build caching are both impossible.
    let map = room_map(false);
    let a = compile_ok(&map).bsp.to_bytes();
    let b = compile_ok(&map).bsp.to_bytes();
    assert_eq!(a, b);
}

#[test]
fn the_portal_file_matches_the_cluster_count() {
    let out = compile_ok(&room_map(false));
    let mut lines = out.prt.lines();
    assert_eq!(lines.next(), Some("VPRT1"));
    let clusters: usize = lines.next().unwrap().parse().unwrap();
    assert_eq!(clusters, out.stats.clusters);
    let portals: usize = lines.next().unwrap().parse().unwrap();
    assert_eq!(lines.count(), portals);
}

#[test]
fn an_empty_map_is_refused_rather_than_producing_a_broken_file() {
    let map = Map::new();
    assert!(matches!(
        compile(&map, &CompileOptions::default()),
        Err(crate::pipeline::CompileError::NoBrushes)
    ));
}

#[test]
fn an_entity_buried_in_a_wall_is_warned_about() {
    let mut map = room_map(false);
    let id = map.next_id();
    let mut e = kerosene_map::Entity::new(id, "light");
    e.set_origin(Vec3::new(128.0, 128.0, -8.0)); // inside the floor
    map.entities.push(e);
    let out = compile_ok(&map);
    assert!(
        out.warnings
            .iter()
            .any(|w| w.message.contains("inside solid")),
        "{:?}",
        out.warnings
    );
}

/// Two rooms side by side with a doorway between them, the second in a
/// streamed visgroup.
fn two_rooms_map() -> Map {
    let mut map = Map::new();
    let t = 16.0;
    // One long box, 0..512 on x, split by a wall at x = 256 with a doorway.
    let (lo, hi) = (0.0f32, 256.0);
    let far = 512.0;
    let outer = [
        Aabb::new(
            Vec3::new(lo - t, lo - t, lo - t),
            Vec3::new(far + t, hi + t, lo),
        ),
        Aabb::new(
            Vec3::new(lo - t, lo - t, hi),
            Vec3::new(far + t, hi + t, hi + t),
        ),
        Aabb::new(Vec3::new(lo - t, lo - t, lo), Vec3::new(lo, hi + t, hi)),
        Aabb::new(Vec3::new(far, lo - t, lo), Vec3::new(far + t, hi + t, hi)),
        Aabb::new(Vec3::new(lo, lo - t, lo), Vec3::new(far, lo, hi)),
        Aabb::new(Vec3::new(lo, hi, lo), Vec3::new(far, hi + t, hi)),
    ];
    for slab in outer {
        map.add_world_solid(Solid::cube(slab, "dev/grid"));
    }
    // The dividing wall, with a doorway 64 wide in the middle.
    map.add_world_solid(Solid::cube(
        Aabb::new(Vec3::new(248.0, lo, lo), Vec3::new(264.0, 96.0, hi)),
        "dev/grid",
    ));
    map.add_world_solid(Solid::cube(
        Aabb::new(Vec3::new(248.0, 160.0, lo), Vec3::new(264.0, hi, hi)),
        "dev/grid",
    ));
    map.add_world_solid(Solid::cube(
        Aabb::new(Vec3::new(248.0, 96.0, 128.0), Vec3::new(264.0, 160.0, hi)),
        "dev/grid",
    ));
    // Furniture in the far room, in a streamed visgroup.
    let cave = map.add_visgroup("Far room", None);
    map.visgroup_mut(cave).unwrap().stream = true;
    let mut crate_ = Solid::cube(
        Aabb::new(Vec3::new(384.0, 96.0, 0.0), Vec3::new(448.0, 160.0, 64.0)),
        "dev/grid",
    );
    crate_.editor.add_to_visgroup(cave);
    map.add_world_solid(crate_);

    let id = map.next_id();
    let mut spawn = kerosene_map::Entity::new(id, "info_player_start");
    spawn.set_origin(Vec3::new(128.0, 128.0, 32.0));
    map.entities.push(spawn);
    map
}

#[test]
fn a_streamed_visgroup_becomes_a_section_with_its_faces_tagged() {
    let map = two_rooms_map();
    let out = compile_ok(&map);
    let bsp = &out.bsp;
    assert_eq!(bsp.sections.len(), 2, "the world and the far room");
    assert_eq!(bsp.sections[1].name, "Far room");
    assert_eq!(
        bsp.sections[1].bounds,
        Aabb::new(Vec3::new(384.0, 96.0, 0.0), Vec3::new(448.0, 160.0, 64.0))
    );
    assert_eq!(bsp.face_sections.len(), bsp.faces.len());
    assert_eq!(bsp.brush_sections.len(), bsp.brushes.len());
    let tagged = bsp.face_sections.iter().filter(|&&s| s == 1).count();
    assert!(
        (5..=6).contains(&tagged),
        "the crate's visible faces: {tagged}"
    );
    assert_eq!(bsp.brush_sections.iter().filter(|&&s| s == 1).count(), 1);
    // Every tagged face really is on the crate.
    for (i, &s) in bsp.face_sections.iter().enumerate() {
        if s == 1 {
            let b = bsp.face_bounds(i);
            assert!(b.min.x >= 384.0 - 0.01 && b.max.x <= 448.0 + 0.01, "{b:?}");
        }
    }
    // And the round trip keeps it all.
    let again = kerosene_bsp::Bsp::from_bytes(&bsp.to_bytes(), "t.kerobsp").unwrap();
    assert_eq!(again.face_sections, bsp.face_sections);
    assert_eq!(again.sections, bsp.sections);
    // The section's faces mark the far room's cluster and not the near one.
    let masks = again.section_cluster_masks();
    // A corner of the near room: the leaf through the doorway at crate
    // height runs the length of the map and rightly sees the crate.
    let near = again.point_cluster(Vec3::new(64.0, 32.0, 32.0));
    let far = again.point_cluster(Vec3::new(416.0, 200.0, 32.0));
    assert!(near >= 0 && far >= 0 && near != far, "{near} {far}");
    let bit = |mask: &[u8], c: i16| mask[c as usize / 8] & (1 << (c as usize % 8)) != 0;
    assert!(bit(&masks[1], far));
    assert!(!bit(&masks[1], near));
}

#[test]
fn a_detail_key_keeps_a_brush_out_of_the_tree() {
    let mut map = room_map(false);
    let mut pillar = Solid::cube(
        Aabb::new(Vec3::new(100.0, 100.0, 0.0), Vec3::new(140.0, 140.0, 128.0)),
        "dev/grid",
    );
    pillar.set("detail", "1");
    map.add_world_solid(pillar);
    let out = compile_ok(&map);
    assert_eq!(out.stats.detail_brushes, 1);
    let plain = compile_ok(&room_map(false));
    assert_eq!(
        out.stats.tree_leaves, plain.stats.tree_leaves,
        "detail does not split"
    );
}

#[test]
fn a_cordon_compiles_a_corner_of_the_map_without_leaking() {
    let map = two_rooms_map();
    let options = CompileOptions {
        cordon: Some(Aabb::new(
            Vec3::new(-32.0, -32.0, -32.0),
            Vec3::new(128.0, 288.0, 288.0),
        )),
        ..Default::default()
    };
    let out = compile(&map, &options).expect("a cordoned map is sealed by its walls");
    assert!(out.leak.is_none());
    let b = out.bsp.world_bounds();
    assert!(b.max.x < 248.0, "the far room is outside the box: {b:?}");
    assert_eq!(out.bsp.sections.len(), 1, "the far room is outside");
}

// ---- meshes -------------------------------------------------------------------

/// The sealed room with a mesh ramp across it: a single sloped quad rising
/// from z = 0 at x = 64 to z = 64 at x = 192, in `material`.
fn room_with_ramp(material: &str) -> Map {
    let mut map = room_map(false);
    let id = map.next_id();
    let mut ramp = kerosene_map::Mesh::new(id);
    ramp.vertices = vec![
        Vec3::new(64.0, 64.0, 0.0),
        Vec3::new(64.0, 192.0, 0.0),
        Vec3::new(192.0, 192.0, 64.0),
        Vec3::new(192.0, 64.0, 64.0),
    ];
    let points = ramp.vertices.clone();
    let face_id = map.next_id();
    ramp.faces.push(kerosene_map::MeshFace::new(
        face_id,
        vec![0, 1, 2, 3],
        &points,
        material,
    ));
    map.world.meshes.push(ramp);
    map
}

#[test]
fn a_mesh_is_drawn_lit_and_walked_on_without_touching_the_tree() {
    let plain = compile_ok(&room_map(false));
    let out = compile_ok(&room_with_ramp("dev/floor"));
    out.bsp.validate().expect("structurally sound");

    // Detail: the tree is the same shape with or without it.
    assert_eq!(out.stats.tree_nodes, plain.stats.tree_nodes);
    assert_eq!(out.stats.clusters, plain.stats.clusters);

    // Drawn: one more face, in the ramp's material, facing up the slope.
    assert_eq!(out.bsp.faces.len(), plain.bsp.faces.len() + 1);
    let ramp = (0..out.bsp.faces.len())
        .find(|&f| out.bsp.face_material(f) == "dev/floor")
        .expect("the ramp's face is in the map");
    let normal = out.bsp.face_plane(ramp).unwrap().normal;
    let expected = Vec3::new(-64.0, 0.0, 128.0).normalize();
    assert!(normal.dot(expected) > 0.999, "{normal}");
    // Wound the way every compiled face is, so the renderer does not cull it.
    let verts = out.bsp.face_vertices(ramp);
    assert!(kerosene_map::polygon_normal(&verts).dot(normal) > 0.999);
    // And in a leaf, where the renderer's PVS walk will find it.
    assert!(out.bsp.leaffaces.contains(&(ramp as u32)));

    // Walked on: a trace dropped onto the middle of the ramp stops on it,
    // and reports the ramp's material for footsteps.
    let trace = out.bsp.trace_ray(
        Vec3::new(128.0, 128.0, 200.0),
        Vec3::new(128.0, 128.0, -50.0),
        contents::MASK_PLAYER_SOLID,
    );
    assert!(trace.hit());
    assert!(
        (trace.endpos.z - 32.0).abs() < 0.5,
        "stopped at {}",
        trace.endpos
    );
    assert_eq!(
        out.bsp.texinfo_name(trace.texture_index as usize),
        "dev/floor"
    );
    // A box the size of a player standing on it rests there too.
    let standing = out.bsp.trace_box(
        Vec3::new(128.0, 128.0, 200.0),
        Vec3::new(128.0, 128.0, -50.0),
        Vec3::new(-16.0, -16.0, 0.0),
        Vec3::new(16.0, 16.0, 72.0),
        contents::MASK_PLAYER_SOLID,
    );
    assert!(
        standing.endpos.z > 20.0,
        "the box fell through: {}",
        standing.endpos
    );
}

#[test]
fn a_flat_mesh_floor_is_in_the_walkmap() {
    // A 32-unit platform: flat, so walkable by the default rule.
    let mut map = room_map(false);
    let id = map.next_id();
    let mut platform = kerosene_map::Mesh::new(id);
    platform.vertices = vec![
        Vec3::new(32.0, 32.0, 32.0),
        Vec3::new(32.0, 96.0, 32.0),
        Vec3::new(96.0, 96.0, 32.0),
        Vec3::new(96.0, 32.0, 32.0),
    ];
    let points = platform.vertices.clone();
    let face_id = map.next_id();
    platform.faces.push(kerosene_map::MeshFace::new(
        face_id,
        vec![0, 1, 2, 3],
        &points,
        "dev/floor",
    ));
    map.world.meshes.push(platform);

    let out = compile_ok(&map);
    assert!(
        out.walk
            .faces
            .iter()
            .any(|f| (f.bounds.min.z - 32.0).abs() < 0.01 && (f.bounds.max.z - 32.0).abs() < 0.01),
        "the platform top should be ground"
    );
}

#[test]
fn a_mesh_over_a_brush_floor_does_not_erase_it() {
    // The ramp's slab reaches below z = 0, into the floor brush. It must not
    // bury the floor's face the way a real brush would.
    let plain = compile_ok(&room_map(false));
    let out = compile_ok(&room_with_ramp("dev/floor"));
    let grid_faces = |bsp: &kerosene_bsp::Bsp| {
        (0..bsp.faces.len())
            .filter(|&f| bsp.face_material(f) == "dev/grid")
            .count()
    };
    assert_eq!(grid_faces(&out.bsp), grid_faces(&plain.bsp));
}
