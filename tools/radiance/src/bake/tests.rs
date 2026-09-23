// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
use super::*;
use crate::lights::LightSet;
use kerosene_bsp::{
    Brush, BrushSide, BspPlane, Edge, Face, Leaf, Model, TexData, TexInfo, encode_leaf,
};
use kerosene_kv::KeyValues;
use kerosene_math::{Plane, PlaneSet};

/// A 64x64 floor at z = 0 facing up, with a 5x5 luxel grid.
///
/// `blocker` puts a solid cube floating over one corner so shadows can be
/// checked.
fn floor_world(blocker: bool) -> Bsp {
    let mut bsp = Bsp::new();
    let mut planes = PlaneSet::new();

    let floor_plane = planes.insert(Plane::new(Vec3::Z, 0.0));

    let mut brushsides = Vec::new();
    if blocker {
        // A cube from (0,0,32) to (32,32,48), over the floor's -X/-Y corner.
        for (n, d) in [
            (Vec3::X, 32.0),
            (-Vec3::X, 0.0),
            (Vec3::Y, 32.0),
            (-Vec3::Y, 0.0),
            (Vec3::Z, 48.0),
            (-Vec3::Z, -32.0),
        ] {
            brushsides.push(BrushSide {
                plane: planes.insert(Plane::new(n, d)),
                texinfo: 0,
                bevel: 0,
            });
        }
    }

    bsp.planes = planes.planes().iter().map(BspPlane::from_plane).collect();
    bsp.brushsides = brushsides;
    if blocker {
        bsp.brushes.push(Brush {
            first_side: 0,
            num_sides: 6,
            contents: kerosene_bsp::contents::SOLID,
        });
        bsp.leafbrushes.push(0);
    }

    bsp.vertices = vec![
        [0.0, 0.0, 0.0],
        [0.0, 64.0, 0.0],
        [64.0, 64.0, 0.0],
        [64.0, 0.0, 0.0],
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

    // 16 world units per luxel.
    let mut ti = TexInfo {
        texdata: 0,
        ..Default::default()
    };
    ti.texture_vecs[0] = [0.25, 0.0, 0.0, 0.0];
    ti.texture_vecs[1] = [0.0, -0.25, 0.0, 0.0];
    ti.lightmap_vecs[0] = [1.0 / 16.0, 0.0, 0.0, 0.0];
    ti.lightmap_vecs[1] = [0.0, 1.0 / 16.0, 0.0, 0.0];
    bsp.texinfo.push(ti);

    bsp.faces.push(Face {
        plane: floor_plane & !1,
        side: (floor_plane & 1) as u8,
        first_surfedge: 0,
        num_surfedges: 4,
        texinfo: 0,
        dispinfo: -1,
        lightmap_offset: -1,
        lightmap_mins: [0, 0],
        lightmap_size: [5, 5],
        light_styles: [0, 255, 255, 255],
        area: 4096.0,
        ..Default::default()
    });
    bsp.leaffaces.push(0);

    bsp.leaves.push(Leaf {
        contents: kerosene_bsp::contents::EMPTY,
        first_leafface: 0,
        num_leaffaces: 1,
        first_leafbrush: 0,
        num_leafbrushes: if blocker { 1 } else { 0 },
        cluster: 0,
        mins: [-512; 3],
        maxs: [512; 3],
        ..Default::default()
    });
    bsp.models.push(Model {
        mins: [-512.0; 3],
        maxs: [512.0; 3],
        origin: [0.0; 3],
        head_node: encode_leaf(0),
        first_face: 0,
        num_faces: 1,
    });
    bsp.validate().expect("fixture is well formed");
    bsp
}

/// A floor at z = 0 (face 0) meeting a wall at x = 64 facing -X (face 1),
/// each backed by a solid slab, so a bounce from one to the other has
/// geometry to be occluded by and nothing in the way.
fn corner_world() -> Bsp {
    let mut bsp = Bsp::new();
    let mut planes = PlaneSet::new();

    let floor_plane = planes.insert(Plane::new(Vec3::Z, 0.0));
    let wall_plane = planes.insert(Plane::new(-Vec3::X, -64.0));

    // Floor slab: x 0..64, y 0..64, z -16..0. Wall slab: x 64..80, y 0..64,
    // z 0..64.
    let slabs: [[(Vec3, f32); 6]; 2] = [
        [
            (Vec3::X, 64.0),
            (-Vec3::X, 0.0),
            (Vec3::Y, 64.0),
            (-Vec3::Y, 0.0),
            (Vec3::Z, 0.0),
            (-Vec3::Z, 16.0),
        ],
        [
            (Vec3::X, 80.0),
            (-Vec3::X, -64.0),
            (Vec3::Y, 64.0),
            (-Vec3::Y, 0.0),
            (Vec3::Z, 64.0),
            (-Vec3::Z, 0.0),
        ],
    ];
    for (i, slab) in slabs.iter().enumerate() {
        for (n, d) in slab {
            bsp.brushsides.push(BrushSide {
                plane: planes.insert(Plane::new(*n, *d)),
                texinfo: 0,
                bevel: 0,
            });
        }
        bsp.brushes.push(Brush {
            first_side: (i * 6) as u32,
            num_sides: 6,
            contents: kerosene_bsp::contents::SOLID,
        });
        bsp.leafbrushes.push(i as u32);
    }
    bsp.planes = planes.planes().iter().map(BspPlane::from_plane).collect();

    bsp.vertices = vec![
        [0.0, 0.0, 0.0],
        [0.0, 64.0, 0.0],
        [64.0, 64.0, 0.0],
        [64.0, 0.0, 0.0],
        [64.0, 0.0, 64.0],
        [64.0, 64.0, 64.0],
    ];
    bsp.edges = vec![
        Edge { v: [0, 1] },
        Edge { v: [1, 2] },
        Edge { v: [2, 3] },
        Edge { v: [3, 0] },
        // The wall, wound so it faces -X: (64,0,0) (64,0,64) (64,64,64) (64,64,0).
        Edge { v: [3, 4] },
        Edge { v: [4, 5] },
        Edge { v: [5, 2] },
        Edge { v: [2, 3] },
    ];
    bsp.surfedges = vec![0, 1, 2, 3, 4, 5, 6, 7];

    let name = bsp.intern_texdata_string("dev/grid");
    bsp.texdata.push(TexData {
        reflectivity: [0.8, 0.8, 0.8],
        name_offset: name,
        width: 512,
        height: 512,
        view_width: 512,
        view_height: 512,
    });
    // Floor: luxels along X and Y. Wall: along Y and Z.
    let mut floor_ti = TexInfo {
        texdata: 0,
        ..Default::default()
    };
    floor_ti.lightmap_vecs[0] = [1.0 / 16.0, 0.0, 0.0, 0.0];
    floor_ti.lightmap_vecs[1] = [0.0, 1.0 / 16.0, 0.0, 0.0];
    let mut wall_ti = TexInfo {
        texdata: 0,
        ..Default::default()
    };
    wall_ti.lightmap_vecs[0] = [0.0, 1.0 / 16.0, 0.0, 0.0];
    wall_ti.lightmap_vecs[1] = [0.0, 0.0, 1.0 / 16.0, 0.0];
    bsp.texinfo.push(floor_ti);
    bsp.texinfo.push(wall_ti);

    for (plane, first, texinfo) in [(floor_plane, 0, 0), (wall_plane, 4, 1)] {
        bsp.faces.push(Face {
            plane: plane & !1,
            side: (plane & 1) as u8,
            first_surfedge: first,
            num_surfedges: 4,
            texinfo,
            dispinfo: -1,
            lightmap_offset: -1,
            lightmap_mins: [0, 0],
            lightmap_size: [5, 5],
            light_styles: [0, 255, 255, 255],
            area: 4096.0,
            ..Default::default()
        });
    }
    bsp.leaffaces = vec![0, 1];
    bsp.leaves.push(Leaf {
        contents: kerosene_bsp::contents::EMPTY,
        first_leafface: 0,
        num_leaffaces: 2,
        first_leafbrush: 0,
        num_leafbrushes: 2,
        cluster: 0,
        mins: [-512; 3],
        maxs: [512; 3],
        ..Default::default()
    });
    bsp.models.push(Model {
        mins: [-512.0; 3],
        maxs: [512.0; 3],
        origin: [0.0; 3],
        head_node: encode_leaf(0),
        first_face: 0,
        num_faces: 2,
    });
    bsp.validate().expect("fixture is well formed");
    bsp
}

fn lights_from(text: &str) -> LightSet {
    LightSet::from_kv(&KeyValues::parse(text).unwrap())
}

fn quick() -> BakeOptions {
    BakeOptions {
        supersample: 1,
        bounces: 0,
        scale: 1.0,
        ambient_scale: 1.0,
    }
}

/// Decoded luxels of face 0, row-major.
fn luxels(bsp: &Bsp) -> Vec<Vec3> {
    bsp.face_lightmap(0)
        .unwrap()
        .iter()
        .map(|c| c.to_linear())
        .collect()
}

#[test]
fn a_light_above_a_floor_lights_it() {
    let mut bsp = floor_world(false);
    let lights = lights_from(
        r#"entity { "classname" "light" "origin" "32 32 128" "_light" "255 255 255 200" }"#,
    );
    let stats = bake(&mut bsp, &lights, &quick());

    assert_eq!(stats.faces_lit, 1);
    assert_eq!(stats.luxels, 25);
    assert_eq!(bsp.lighting.len(), 25);
    assert_eq!(bsp.faces[0].lightmap_offset, 0);

    let l = luxels(&bsp);
    assert!(
        l.iter().all(|c| c.x > 0.0),
        "every luxel should receive light"
    );
    // The centre luxel is directly under the light and should be brightest.
    let centre = l[2 * 5 + 2].x;
    let corner = l[0].x;
    assert!(
        centre > corner,
        "centre {centre} should beat corner {corner}"
    );
}

#[test]
fn a_map_with_no_lights_bakes_black() {
    let mut bsp = floor_world(false);
    let lights = lights_from(r#"entity { "classname" "info_player_start" "origin" "0 0 0" }"#);
    bake(&mut bsp, &lights, &quick());
    assert!(luxels(&bsp).iter().all(|c| c.length() == 0.0));
}

#[test]
fn ambient_reaches_everywhere() {
    let mut bsp = floor_world(true);
    let lights = lights_from(
        r#"entity { "classname" "light_environment" "pitch" "-90" "_light" "0 0 0 0" "_ambient" "64 64 64 100" }"#,
    );
    bake(&mut bsp, &lights, &quick());
    let l = luxels(&bsp);
    assert!(
        l.iter().all(|c| c.x > 0.0),
        "ambient is not occluded by anything"
    );
}

#[test]
fn a_blocker_casts_a_shadow() {
    let mut bsp = floor_world(true);
    let lights = lights_from(
        r#"entity { "classname" "light" "origin" "32 32 256" "_light" "255 255 255 400" }"#,
    );
    bake(&mut bsp, &lights, &quick());
    let l = luxels(&bsp);

    // Luxel (0,0) is at world (0,0,0), under the blocker; (4,4) is at
    // (64,64,0), well clear of it.
    let shadowed = l[0].x;
    let lit = l[4 * 5 + 4].x;
    assert_eq!(
        shadowed, 0.0,
        "a luxel under a solid blocker must be in shadow"
    );
    assert!(lit > 0.0, "a luxel clear of the blocker must be lit");
}

#[test]
fn the_sun_only_reaches_surfaces_that_can_see_the_sky() {
    let mut bsp = floor_world(true);
    // Mark the blocker's faces as ordinary solid (not sky), so it shadows.
    let lights = lights_from(
        r#"entity { "classname" "light_environment" "pitch" "-90" "_light" "255 255 255 300" }"#,
    );
    bake(&mut bsp, &lights, &quick());
    let l = luxels(&bsp);
    assert_eq!(l[0].x, 0.0, "under the blocker there is no sky");
    assert!(
        l[4 * 5 + 4].x > 0.0,
        "the open part of the floor sees the sun"
    );
}

#[test]
fn a_light_below_the_floor_does_not_light_its_top() {
    // Lambert must reject it: the surface faces away.
    let mut bsp = floor_world(false);
    let lights = lights_from(
        r#"entity { "classname" "light" "origin" "32 32 -128" "_light" "255 255 255 200" }"#,
    );
    bake(&mut bsp, &lights, &quick());
    assert!(luxels(&bsp).iter().all(|c| c.x == 0.0));
}

#[test]
fn sky_and_nodraw_faces_take_no_lightmap() {
    for flag in [surf::SKY, surf::NODRAW, surf::NOLIGHT] {
        let mut bsp = floor_world(false);
        bsp.texinfo[0].flags = flag;
        let lights = lights_from(
            r#"entity { "classname" "light" "origin" "32 32 128" "_light" "255 255 255 200" }"#,
        );
        let stats = bake(&mut bsp, &lights, &quick());
        assert_eq!(stats.faces_lit, 0);
        assert_eq!(bsp.faces[0].lightmap_offset, -1);
        assert!(bsp.lighting.is_empty());
    }
}

#[test]
fn bouncing_only_adds_light() {
    let mut direct = floor_world(false);
    let lights = lights_from(
        r#"entity { "classname" "light" "origin" "32 32 128" "_light" "255 255 255 200" }"#,
    );
    bake(&mut direct, &lights, &quick());
    let before: f32 = luxels(&direct).iter().map(|c| c.x).sum();

    let mut bounced = floor_world(false);
    let opts = BakeOptions {
        supersample: 1,
        bounces: 1,
        ..quick()
    };
    bake(&mut bounced, &lights, &opts);
    let after: f32 = luxels(&bounced).iter().map(|c| c.x).sum();

    assert!(
        after >= before,
        "a bounce pass must not darken anything: {after} vs {before}"
    );
}

#[test]
fn a_lit_floor_bounces_light_onto_the_wall_beside_it() {
    // Regression: the bounce trace ended exactly on the emitting face, which
    // the tracer counts as entering its brush, so every patch occluded
    // itself and `--bounces` added nothing at all.
    let lights = lights_from(
        r#"entity { "classname" "light" "origin" "16 32 8" "_light" "255 255 255 300" }"#,
    );
    let wall_total = |bounces: u32| -> f32 {
        let mut world = corner_world();
        let opts = BakeOptions {
            supersample: 1,
            bounces,
            ..quick()
        };
        bake(&mut world, &lights, &opts);
        world
            .face_lightmap(1)
            .unwrap()
            .iter()
            .map(|c| c.to_linear().x)
            .sum()
    };
    let direct = wall_total(0);
    let bounced = wall_total(1);
    assert!(
        bounced > direct * 1.05,
        "the wall must be brighter for the floor's bounce: {bounced} vs {direct}"
    );
}

#[test]
fn supersampling_changes_nothing_on_a_uniformly_lit_face() {
    // With no occluders, extra samples should agree with a single one --
    // a sanity check that the jittered offsets stay on the surface.
    let lights = lights_from(
        r#"entity { "classname" "light_environment" "pitch" "-90" "_light" "255 255 255 300" }"#,
    );
    let mut one = floor_world(false);
    bake(
        &mut one,
        &lights,
        &BakeOptions {
            supersample: 1,
            bounces: 0,
            ..quick()
        },
    );
    let mut many = floor_world(false);
    bake(
        &mut many,
        &lights,
        &BakeOptions {
            supersample: 3,
            bounces: 0,
            ..quick()
        },
    );

    for (a, b) in luxels(&one).iter().zip(luxels(&many).iter()) {
        assert!((a.x - b.x).abs() < 1.0, "{a:?} vs {b:?}");
    }
}

#[test]
fn the_exposure_scale_multiplies_the_result() {
    let lights = lights_from(
        r#"entity { "classname" "light" "origin" "32 32 128" "_light" "255 255 255 200" }"#,
    );
    let mut normal = floor_world(false);
    bake(&mut normal, &lights, &quick());
    let mut bright = floor_world(false);
    bake(
        &mut bright,
        &lights,
        &BakeOptions {
            scale: 2.0,
            ..quick()
        },
    );

    let a = luxels(&normal)[12].x;
    let b = luxels(&bright)[12].x;
    assert!((b / a - 2.0).abs() < 0.05, "{b} should be twice {a}");
}

#[test]
fn baking_is_deterministic() {
    let lights = lights_from(
        r#"entity { "classname" "light" "origin" "20 40 128" "_light" "255 200 150 250" }"#,
    );
    let mut a = floor_world(true);
    let mut b = floor_world(true);
    let opts = BakeOptions {
        supersample: 2,
        bounces: 1,
        ..quick()
    };
    bake(&mut a, &lights, &opts);
    bake(&mut b, &lights, &opts);
    assert_eq!(a.lighting.len(), b.lighting.len());
    for (x, y) in a.lighting.iter().zip(b.lighting.iter()) {
        assert_eq!((x.r, x.g, x.b, x.exponent), (y.r, y.g, y.b, y.exponent));
    }
}

#[test]
fn a_lit_map_still_round_trips_through_a_file() {
    let mut bsp = floor_world(true);
    let lights = lights_from(
        r#"entity { "classname" "light" "origin" "32 32 128" "_light" "255 255 255 200" }"#,
    );
    bake(&mut bsp, &lights, &quick());
    let bytes = bsp.to_bytes();
    let back = Bsp::from_bytes(&bytes, "lit.kerobsp").expect("should reload");
    assert_eq!(back.lighting.len(), bsp.lighting.len());
    assert_eq!(back.faces[0].lightmap_offset, bsp.faces[0].lightmap_offset);
    assert!(back.face_lightmap(0).is_some());
}

#[test]
fn a_probe_sees_the_lit_floor_below_it_the_wall_beside_it_and_sky_above() {
    use crate::probes;
    use kerosene_bsp::cubemaps::{FACES, direction_to_face};
    use kerosene_bsp::decode_rgb9e5;

    let mut world = corner_world();
    let lights = lights_from(
        r#"entity { "classname" "light" "origin" "32 32 48" "_light" "255 255 255 300" }"#,
    );
    bake(&mut world, &lights, &quick());

    let size = 8;
    // A sky colour nothing else could produce, to tell escaping rays apart.
    let sky = Vec3::new(0.0, 0.0, 3.0);
    let (cubemaps, stats) = probes::bake(&world, &[Vec3::new(32.0, 32.0, 32.0)], size, sky);
    assert_eq!(stats.probes, 1);
    assert_eq!(stats.buried, 0);
    let probe = &cubemaps.probes[0];
    assert_eq!(probe.texels.len(), FACES * (size * size) as usize);

    let look = |dir: Vec3| {
        let (face, s, t) = direction_to_face(dir.normalize());
        let (x, y) = ((s * size as f32) as u32, (t * size as f32) as u32);
        decode_rgb9e5(probe.texels[face * (size * size) as usize + (y * size + x) as usize])
    };

    let floor = look(-Vec3::Z);
    let wall = look(Vec3::X);
    let up = look(Vec3::Z);
    assert!(
        floor.x > 0.0 && floor.z == floor.x,
        "lit grey floor: {floor}"
    );
    assert!(wall.x > 0.0, "lit wall: {wall}");
    assert_eq!(up, sky, "nothing above but sky");

    // The floor texel is the floor's luxel times its 0.8 reflectivity, on
    // the atlas's scale: the same light the engine draws the floor with.
    let under = world.face_lightmap(0).unwrap()[2 * 5 + 2].to_linear() / 255.0 * 0.8;
    assert!(
        (floor.x - under.x).abs() < under.x * 0.05,
        "probe {floor} vs floor luxel {under}"
    );
}
