// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Rooms built in memory, compiled, and listened to.
//!
//! The reference is Eyring's formula, which for a box is exact enough to
//! hold a simulation to: a concrete cube of a known size has a known decay,
//! and a probe that disagrees by more than a fifth is measuring something
//! other than the room.

use super::*;
use crate::probe::SPEED_OF_SOUND;
use kerosene_asset::{AcousticProfile, SurfaceProperty};
use kerosene_bsp::acoustics::{NO_ROOM, room_flags};
use kerosene_map::{Entity, Map, Solid};
use kerosene_math::{Aabb, Vec3};

const WALL: f32 = 16.0;

/// The six slabs around a box, each in its own material so a test can make
/// the floor carpet and the ceiling sky.
fn shell(map: &mut Map, inner: Aabb, materials: [&str; 6]) {
    let (lo, hi, t) = (inner.min, inner.max, WALL);
    let slabs = [
        // floor, ceiling, -x, +x, -y, +y
        Aabb::new(
            Vec3::new(lo.x - t, lo.y - t, lo.z - t),
            Vec3::new(hi.x + t, hi.y + t, lo.z),
        ),
        Aabb::new(
            Vec3::new(lo.x - t, lo.y - t, hi.z),
            Vec3::new(hi.x + t, hi.y + t, hi.z + t),
        ),
        Aabb::new(
            Vec3::new(lo.x - t, lo.y - t, lo.z),
            Vec3::new(lo.x, hi.y + t, hi.z),
        ),
        Aabb::new(
            Vec3::new(hi.x, lo.y - t, lo.z),
            Vec3::new(hi.x + t, hi.y + t, hi.z),
        ),
        Aabb::new(Vec3::new(lo.x, lo.y - t, lo.z), Vec3::new(hi.x, lo.y, hi.z)),
        Aabb::new(Vec3::new(lo.x, hi.y, lo.z), Vec3::new(hi.x, hi.y + t, hi.z)),
    ];
    for (slab, material) in slabs.into_iter().zip(materials) {
        map.add_world_solid(Solid::cube(slab, material));
    }
}

fn spawn_at(map: &mut Map, at: Vec3) {
    let id = map.next_id();
    let mut spawn = Entity::new(id, "info_player_start");
    spawn.set_origin(at);
    map.entities.push(spawn);
}

/// A sealed cube `size` on a side, centred on the origin, in one material.
fn cube_map(size: f32, material: &str) -> Map {
    let mut map = Map::new();
    let half = size / 2.0;
    shell(
        &mut map,
        Aabb::new(Vec3::splat(-half), Vec3::splat(half)),
        [material; 6],
    );
    spawn_at(&mut map, Vec3::new(0.0, 0.0, -half + 32.0));
    map
}

/// Compile, and read the materials off their names: `concrete/x` absorbs
/// like concrete. No content tree needed.
fn compiled(map: &Map) -> (kerosene_bsp::Bsp, PortalGraph, Absorption) {
    let out = cleave::compile(map, &cleave::CompileOptions::default()).expect("should compile");
    assert!(out.leak.is_none(), "the test map leaks");
    let bsp = kerosene_bsp::Bsp::from_bytes(&out.bsp.to_bytes(), "test.kerobsp").unwrap();
    let graph = PortalGraph::parse(&out.prt).expect("portal graph");
    let absorption = Absorption::build(&bsp, |name| {
        let family = name.split('/').next().unwrap_or(name);
        Some(AcousticProfile::of(&SurfaceProperty::parse(family)))
    });
    (bsp, graph, absorption)
}

/// Eyring's decay for a box: mean free path `4V/S`, and 60 dB is `ln 10^-6`
/// of energy lost at `ln(1 - a)` per hit plus air.
fn eyring(size: Vec3, absorption: f32, band: usize) -> (f32, f32) {
    let volume = size.x * size.y * size.z;
    let surface = 2.0 * (size.x * size.y + size.y * size.z + size.x * size.z);
    let path = 4.0 * volume / surface;
    let per_unit = -(1.0 - absorption).ln() / path + probe::AIR[band];
    (13.816 / (SPEED_OF_SOUND * per_unit), path)
}

fn within(actual: f32, expected: f32, fraction: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= expected * fraction,
        "{what}: measured {actual:.3}, expected {expected:.3} (±{:.0}%)",
        fraction * 100.0
    );
}

#[test]
fn a_concrete_cube_rings_as_eyring_says() {
    let (bsp, graph, absorption) = compiled(&cube_map(256.0, "concrete/wall"));
    let (acoustics, report) = compute(&bsp, Some(&graph), &absorption, &Options::DEFAULT);
    assert!(report.probed > 0);
    assert_eq!(report.unplaced, 0, "{report:?}");
    assert_eq!(
        acoustics.rooms.len(),
        1,
        "a bare cube is one room: {report:?}"
    );
    let room = &acoustics.rooms[0];
    let concrete = AcousticProfile::of(&SurfaceProperty::Concrete);
    let size = Vec3::splat(256.0);
    for band in 0..4 {
        let (expected, path) = eyring(size, concrete.band(band), band);
        within(room.rt60[band], expected, 0.2, &format!("band {band} rt60"));
        within(room.mean_free_path, path, 0.2, "mean free path");
        within(
            room.absorption[band],
            concrete.band(band),
            0.1,
            "absorption",
        );
    }
    assert!(!room.has(room_flags::OUTDOOR));
    assert_eq!(room.openness, 0.0);
    assert!(
        room.predelay > 0.004 && room.predelay < 0.02,
        "{}",
        room.predelay
    );
    assert!(
        room.wet > 0.3,
        "a concrete cube should be wet: {}",
        room.wet
    );
    assert_eq!(bsp.leaves.len(), acoustics.leaf_room.len());
    let (index, _) = acoustics.room_of_leaf(bsp.point_leaf(Vec3::ZERO)).unwrap();
    assert_eq!(index, 0);
}

#[test]
fn a_carpeted_cube_is_deader() {
    let (bsp, graph, absorption) = compiled(&cube_map(256.0, "carpet/floor"));
    let (acoustics, _) = compute(&bsp, Some(&graph), &absorption, &Options::DEFAULT);
    let carpet = &acoustics.rooms[0];
    let (bsp, graph, absorption) = compiled(&cube_map(256.0, "concrete/wall"));
    let (acoustics, _) = compute(&bsp, Some(&graph), &absorption, &Options::DEFAULT);
    let concrete = &acoustics.rooms[0];
    for band in 1..4 {
        assert!(
            carpet.rt60[band] < concrete.rt60[band] * 0.5,
            "band {band}: carpet {} vs concrete {}",
            carpet.rt60[band],
            concrete.rt60[band]
        );
    }
    assert!(carpet.wet < concrete.wet);
    let (expected, _) = eyring(
        Vec3::splat(256.0),
        AcousticProfile::of(&SurfaceProperty::Carpet).band(2),
        2,
    );
    within(carpet.rt60[2], expected, 0.2, "carpet 2 kHz rt60");
}

#[test]
fn a_box_open_to_the_sky_is_outdoors() {
    let mut map = Map::new();
    shell(
        &mut map,
        Aabb::new(Vec3::splat(-128.0), Vec3::splat(128.0)),
        [
            "concrete/a",
            "tools/skybox",
            "concrete/a",
            "concrete/a",
            "concrete/a",
            "concrete/a",
        ],
    );
    spawn_at(&mut map, Vec3::new(0.0, 0.0, -96.0));
    let (bsp, graph, absorption) = compiled(&map);
    let (acoustics, report) = compute(&bsp, Some(&graph), &absorption, &Options::DEFAULT);
    assert!(!acoustics.rooms.is_empty(), "{report:?}");
    let room = room_at_or_first(&acoustics, &bsp);
    assert!(room.has(room_flags::OUTDOOR), "openness {}", room.openness);
    assert!(room.openness > 0.5, "{}", room.openness);
    assert!(
        room.wet < 0.2,
        "the sky should soak up the room: wet {}",
        room.wet
    );
    assert!(
        room.rt60[1] < 1.0,
        "an open box should not ring: {}",
        room.rt60[1]
    );
}

#[test]
fn a_hall_and_a_closet_through_a_door_are_two_rooms() {
    // A concrete hall and a carpeted closet, joined by a 64-wide doorway.
    let mut map = Map::new();
    let hall = Aabb::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(768.0, 384.0, 256.0));
    let closet = Aabb::new(Vec3::new(784.0, 128.0, 0.0), Vec3::new(912.0, 256.0, 128.0));
    // Hall shell minus its +x wall, which the doorway pieces replace.
    shell(
        &mut map,
        hall,
        [
            "concrete/a",
            "concrete/a",
            "concrete/a",
            "tools/nodraw",
            "concrete/a",
            "concrete/a",
        ],
    );
    // Closet shell minus its -x wall.
    shell(
        &mut map,
        closet,
        [
            "carpet/a",
            "carpet/a",
            "tools/nodraw",
            "carpet/a",
            "carpet/a",
            "carpet/a",
        ],
    );
    // Now knock the shared wall down to a doorway: remove the two nodraw
    // slabs and put a wall with a 64-wide, 96-tall gap between the rooms.
    map.world
        .solids
        .retain(|s| s.sides.iter().all(|side| side.material != "tools/nodraw"));
    let (x0, x1) = (hall.max.x, closet.min.x);
    for slab in [
        // Left of the door, right of it, above it: the wall between.
        Aabb::new(
            Vec3::new(x0, -WALL, -WALL),
            Vec3::new(x1, 160.0, 256.0 + WALL),
        ),
        Aabb::new(
            Vec3::new(x0, 224.0, -WALL),
            Vec3::new(x1, 384.0 + WALL, 256.0 + WALL),
        ),
        Aabb::new(
            Vec3::new(x0, 160.0, 96.0),
            Vec3::new(x1, 224.0, 256.0 + WALL),
        ),
        Aabb::new(Vec3::new(x0, 160.0, -WALL), Vec3::new(x1, 224.0, 0.0)),
    ] {
        map.add_world_solid(Solid::cube(slab, "concrete/a"));
    }
    spawn_at(&mut map, Vec3::new(384.0, 192.0, 32.0));

    let (bsp, graph, absorption) = compiled(&map);
    let (acoustics, report) = compute(&bsp, Some(&graph), &absorption, &Options::DEFAULT);
    assert!(acoustics.rooms.len() >= 2, "{report:?}");
    let (hall_room, hall_acoustics) = acoustics
        .room_of_leaf(bsp.point_leaf(hall.center()))
        .expect("the hall's centre should be in a room");
    let (closet_room, closet_acoustics) = acoustics
        .room_of_leaf(bsp.point_leaf(closet.center()))
        .expect("the closet's centre should be in a room");
    assert_ne!(hall_room, closet_room, "the doorway should keep them apart");
    assert!(
        hall_acoustics.rt60[2] > closet_acoustics.rt60[2] * 1.5,
        "hall {} vs closet {}",
        hall_acoustics.rt60[2],
        closet_acoustics.rt60[2]
    );
    assert!(hall_acoustics.mean_free_path > closet_acoustics.mean_free_path);
    // Every non-solid leaf ended up somewhere.
    for (i, leaf) in bsp.leaves.iter().enumerate() {
        if !leaf.is_solid() && leaf.has_vis() {
            assert_ne!(acoustics.leaf_room[i], NO_ROOM, "leaf {i} is in no room");
        }
    }
}

#[test]
fn a_placed_override_has_the_last_word() {
    let mut map = cube_map(256.0, "concrete/wall");
    let id = map.next_id();
    let mut e = Entity::new(id, overrides::CLASSNAME);
    e.set_origin(Vec3::new(0.0, 0.0, 0.0));
    e.set("rt60", "0.4 0.3 0.2 0.1");
    e.set("wet", "0.1");
    map.entities.push(e);
    let (bsp, graph, absorption) = compiled(&map);
    let (acoustics, report) = compute(&bsp, Some(&graph), &absorption, &Options::FAST);
    assert_eq!(report.overridden_rooms, 1);
    let room = &acoustics.rooms[0];
    assert_eq!(room.rt60, [0.4, 0.3, 0.2, 0.1]);
    assert_eq!(room.wet, 0.1);
    assert!(room.has(room_flags::OVERRIDE));
    // What was not set was measured, and measured the same as without.
    let (plain, _, _) = compiled(&cube_map(256.0, "concrete/wall"));
    let (plain_acoustics, _) = compute(&plain, Some(&graph), &absorption, &Options::FAST);
    assert_eq!(room.predelay, plain_acoustics.rooms[0].predelay);
    assert_eq!(room.mean_free_path, plain_acoustics.rooms[0].mean_free_path);
}

#[test]
fn the_same_map_compiles_to_the_same_bytes() {
    let (bsp, graph, absorption) = compiled(&cube_map(192.0, "wood/panel"));
    let (a, _) = compute(&bsp, Some(&graph), &absorption, &Options::FAST);
    let (b, _) = compute(&bsp, Some(&graph), &absorption, &Options::FAST);
    assert_eq!(a, b);
    assert_eq!(a.encode(), b.encode());
}

#[test]
fn without_a_portal_file_bounds_join_the_leaves() {
    let (bsp, graph, absorption) = compiled(&cube_map(256.0, "concrete/wall"));
    let (with, _) = compute(&bsp, Some(&graph), &absorption, &Options::FAST);
    let (without, _) = compute(&bsp, None, &absorption, &Options::FAST);
    assert_eq!(with.rooms.len(), without.rooms.len());
    assert_eq!(with.leaf_room, without.leaf_room);
}

// ---- the pieces on their own --------------------------------------------

fn leaf(rt60: f32, path: f32, openness: f32) -> LeafAcoustics {
    LeafAcoustics {
        rt60: [rt60; 4],
        absorption: [0.1; 4],
        mean_free_path: path,
        predelay: 0.01,
        openness,
        diffusion: 0.5,
        wet: 0.4,
        water: false,
        inherited: false,
    }
}

fn unit_boxes(n: usize) -> Vec<Aabb> {
    (0..n)
        .map(|i| {
            let x = i as f32 * 64.0;
            Aabb::new(Vec3::new(x, 0.0, 0.0), Vec3::new(x + 64.0, 64.0, 64.0))
        })
        .collect()
}

#[test]
fn a_tiny_leaf_takes_its_widest_neighbours_figures() {
    let mut leaves = vec![
        Ok(leaf(2.0, 200.0, 0.0)),
        Err(Skipped::Tiny),
        Err(Skipped::NoPoint),
        Ok(leaf(0.3, 50.0, 0.0)),
        Err(Skipped::Solid),
    ];
    let adjacency = Adjacency {
        edges: vec![(0, 1, 10.0), (1, 2, 5.0), (2, 3, 50.0), (3, 4, 1.0)],
    };
    assert_eq!(rooms::inherit(&mut leaves, &adjacency), 2);
    let one = leaves[1].unwrap();
    assert!(one.inherited);
    assert_eq!(one.rt60, [2.0; 4], "leaf 1 borders only leaf 0 with data");
    let two = leaves[2].unwrap();
    assert_eq!(two.rt60, [0.3; 4], "leaf 2's widest join is to leaf 3");
    assert_eq!(leaves[4], Err(Skipped::Solid), "solid stays solid");
}

#[test]
fn alike_leaves_merge_and_different_ones_do_not() {
    let leaves: Vec<Result<LeafAcoustics, Skipped>> = vec![
        Ok(leaf(2.0, 200.0, 0.0)),
        Ok(leaf(2.1, 210.0, 0.0)),
        Ok(leaf(1.9, 190.0, 0.0)),
        Ok(leaf(0.4, 60.0, 0.0)),
        Ok(leaf(0.42, 62.0, 0.0)),
        Err(Skipped::Solid),
    ];
    let bounds = unit_boxes(6);
    let adjacency = Adjacency {
        edges: vec![
            (0, 1, 100.0),
            (1, 2, 100.0),
            (2, 3, 100.0),
            (3, 4, 100.0),
            (4, 5, 100.0),
        ],
    };
    let acoustics = rooms::cluster(&leaves, &bounds, &adjacency, Tolerance::DEFAULT);
    assert_eq!(acoustics.rooms.len(), 2);
    assert_eq!(acoustics.leaf_room[0], acoustics.leaf_room[1]);
    assert_eq!(acoustics.leaf_room[1], acoustics.leaf_room[2]);
    assert_eq!(acoustics.leaf_room[3], acoustics.leaf_room[4]);
    assert_ne!(acoustics.leaf_room[2], acoustics.leaf_room[3]);
    assert_eq!(acoustics.leaf_room[5], NO_ROOM);
    // Largest room first, and the record is the mean of its leaves.
    assert_eq!(acoustics.leaf_room[0], 0);
    assert_eq!(acoustics.rooms[0].leaf_count, 3);
    within(acoustics.rooms[0].rt60[0], 2.0, 0.05, "merged rt60");
    within(
        acoustics.rooms[0].mean_free_path,
        200.0,
        0.05,
        "merged path",
    );
    assert_eq!(acoustics.rooms[0].volume, 3.0 * 64.0 * 64.0 * 64.0);
    assert!(acoustics.validate(6).is_ok());
}

#[test]
fn water_never_merges_with_air() {
    let mut wet = leaf(1.0, 100.0, 0.0);
    wet.water = true;
    let leaves = vec![Ok(leaf(1.0, 100.0, 0.0)), Ok(wet)];
    let adjacency = Adjacency {
        edges: vec![(0, 1, 100.0)],
    };
    let acoustics = rooms::cluster(&leaves, &unit_boxes(2), &adjacency, Tolerance::DEFAULT);
    assert_eq!(acoustics.rooms.len(), 2);
    assert!(acoustics.rooms.iter().any(|r| r.has(room_flags::WATER)));
}

#[test]
fn a_niche_joins_the_room_it_opens_onto() {
    // Two big leaves that disagree, and a niche between them that is
    // closest to the second but shares more portal with the first.
    let leaves = vec![
        Ok(leaf(2.0, 200.0, 0.0)),
        Ok(leaf(1.0, 200.0, 0.0)),
        Ok(leaf(0.5, 100.0, 0.0)),
    ];
    let bounds = vec![
        Aabb::new(Vec3::ZERO, Vec3::splat(256.0)),
        Aabb::new(Vec3::new(256.0, 0.0, 0.0), Vec3::new(272.0, 16.0, 16.0)),
        Aabb::new(Vec3::new(272.0, 0.0, 0.0), Vec3::new(528.0, 256.0, 256.0)),
    ];
    let adjacency = Adjacency {
        edges: vec![(0, 1, 200.0), (1, 2, 50.0)],
    };
    let acoustics = rooms::cluster(&leaves, &bounds, &adjacency, Tolerance::DEFAULT);
    assert_eq!(acoustics.rooms.len(), 2);
    assert_eq!(acoustics.leaf_room[1], acoustics.leaf_room[0]);
}

/// The room at the map's origin, or the first, for tests on a single space
/// that may be split.
fn room_at_or_first<'a>(
    acoustics: &'a Acoustics,
    bsp: &kerosene_bsp::Bsp,
) -> &'a kerosene_bsp::AcousticRoom {
    acoustics
        .room_of_leaf(bsp.point_leaf(Vec3::ZERO))
        .map(|(_, r)| r)
        .unwrap_or(&acoustics.rooms[0])
}
