// SPDX-License-Identifier: LGPL-3.0-or-later
//! Generates `content/maps/void_start.voidmap`, the sample level.
//!
//! Run with `cargo run -p void-map --example sample_map`. Writing it in code
//! rather than by hand keeps the brush geometry exact -- a `.voidmap` stores
//! planes as three points, and getting those right by hand for a two-room
//! level is a miserable way to spend an afternoon.

use void_map::{Connection, Entity, Map, Solid};
use void_math::{Aabb, Vec3};

/// Wall thickness, in inches.
const T: f32 = 16.0;
/// Interior size of each room.
const ROOM: f32 = 512.0;
const HEIGHT: f32 = 256.0;

fn main() -> std::io::Result<()> {
    let mut map = Map::new();
    map.world.set("skyname", "sky_void");
    map.world.set("message", "The Void -- sample level");

    // Two rooms side by side along X, separated by a wall with a doorway in
    // it. The doorway is what gives the visibility compile something to do:
    // standing in one room, most of the other is not visible.
    let total_x = ROOM * 2.0 + T;
    let shell = [
        // floor and ceiling
        Aabb::new(Vec3::new(-T, -T, -T), Vec3::new(total_x + T, ROOM + T, 0.0)),
        Aabb::new(Vec3::new(-T, -T, HEIGHT), Vec3::new(total_x + T, ROOM + T, HEIGHT + T)),
        // side walls
        Aabb::new(Vec3::new(-T, -T, 0.0), Vec3::new(0.0, ROOM + T, HEIGHT)),
        Aabb::new(Vec3::new(total_x, -T, 0.0), Vec3::new(total_x + T, ROOM + T, HEIGHT)),
        Aabb::new(Vec3::new(0.0, -T, 0.0), Vec3::new(total_x, 0.0, HEIGHT)),
        Aabb::new(Vec3::new(0.0, ROOM, 0.0), Vec3::new(total_x, ROOM + T, HEIGHT)),
    ];
    for b in shell {
        map.add_world_solid(Solid::cube(b, "dev/grid"));
    }

    // The dividing wall, in three pieces around a 96-wide, 128-tall doorway.
    let (wall_x0, wall_x1) = (ROOM, ROOM + T);
    let (door_y0, door_y1) = (ROOM / 2.0 - 48.0, ROOM / 2.0 + 48.0);
    let door_top = 128.0;
    for b in [
        Aabb::new(Vec3::new(wall_x0, 0.0, 0.0), Vec3::new(wall_x1, door_y0, HEIGHT)),
        Aabb::new(Vec3::new(wall_x0, door_y1, 0.0), Vec3::new(wall_x1, ROOM, HEIGHT)),
        Aabb::new(Vec3::new(wall_x0, door_y0, door_top), Vec3::new(wall_x1, door_y1, HEIGHT)),
    ] {
        map.add_world_solid(Solid::cube(b, "dev/wall"));
    }

    // Detail pillars. Marked func_detail so they decorate the room without
    // carving the world tree into slivers -- see the Cleave docs.
    for i in 0..4 {
        let x = 96.0 + (i % 2) as f32 * 320.0;
        let y = 96.0 + (i / 2) as f32 * 320.0;
        let pillar = Aabb::new(Vec3::new(x, y, 0.0), Vec3::new(x + 32.0, y + 32.0, HEIGHT));
        add_brush_entity(&mut map, "func_detail", pillar, "dev/wall", |_| {});
    }

    // A door filling the gap, wired to open when the player touches a trigger
    // in front of it.
    add_brush_entity(
        &mut map,
        "func_door",
        Aabb::new(
            Vec3::new(wall_x0, door_y0, 0.0),
            Vec3::new(wall_x1, door_y1, door_top),
        ),
        "dev/door",
        |e| {
            e.set("targetname", "gate");
            e.set("speed", "100");
            e.set("wait", "4");
            // Slides straight up into the lintel.
            e.set("movedir", "0 0 1");
            e.set("lip", "8");
        },
    );

    add_brush_entity(
        &mut map,
        "trigger_multiple",
        Aabb::new(
            Vec3::new(wall_x0 - 128.0, door_y0 - 32.0, 0.0),
            Vec3::new(wall_x1 + 128.0, door_y1 + 32.0, door_top),
        ),
        "tools/trigger",
        |e| {
            e.set("targetname", "gate_trigger");
            e.set("wait", "1");
            e.connect(Connection::new("OnStartTouch", "gate", "Open"));
        },
    );

    // A slowly turning bar under the ceiling of the first room. Visible from
    // the spawn, reachable by nothing, and there for one reason: a brush model
    // that turns is the case the model transform exists for, and a sample
    // level with no rotating geometry in it never exercises it.
    add_brush_entity(
        &mut map,
        "func_rotating",
        Aabb::new(Vec3::new(200.0, 240.0, 200.0), Vec3::new(312.0, 272.0, 216.0)),
        "dev/door",
        |e| {
            e.set("targetname", "ceiling_fan");
            // Flag 1 starts it turning; no flag for the axis means about up.
            e.set("spawnflags", "1");
            e.set("maxspeed", "60");
        },
    );

    // A raised ledge across the back of the second room, and a ladder up its
    // face. Somewhere to go that is not on the floor, which is the point of
    // having a climb at all.
    let ledge_x = ROOM * 1.5 + T;
    map.add_world_solid(Solid::cube(
        Aabb::new(Vec3::new(ledge_x, 0.0, 0.0), Vec3::new(total_x, ROOM, 128.0)),
        "dev/wall",
    ));
    add_brush_entity(
        &mut map,
        "func_ladder",
        // Standing proud of the ledge face, and reaching above its top so a
        // climber is still holding on when they can step off.
        Aabb::new(
            Vec3::new(ledge_x - 16.0, ROOM * 0.5 - 32.0, 0.0),
            Vec3::new(ledge_x, ROOM * 0.5 + 32.0, 176.0),
        ),
        "tools/ladder",
        |e| { e.set("targetname", "ledge_ladder"); },
    );

    // A button on the south wall of the first room, and the shutter it
    // controls. The gate opens by walking into a trigger; this is the other
    // half of the vocabulary -- something the player has to decide to do --
    // and the sample level should demonstrate both.
    add_brush_entity(
        &mut map,
        "func_button",
        // Centred on eye height (64) and generous with it, so it is easy to
        // hit whether the player is standing or halfway through a duck.
        Aabb::new(Vec3::new(224.0, 0.0, 40.0), Vec3::new(256.0, 4.0, 88.0)),
        "dev/door",
        |e| {
            e.set("targetname", "shutter_switch");
            // Presses into the wall it sits on.
            e.set("movedir", "0 -1 0");
            e.set("speed", "20");
            e.set("lip", "1");
            e.set("wait", "1");
            e.connect(Connection::new("OnPressed", "shutter", "Toggle"));
            e.connect(Connection::new("OnPressed", "switch_click", "Play"));
        },
    );

    // A one-shot noise for the button, which is what point_sound is for: the
    // room tone is a bed and this is an event, and making one entity do both
    // is how a chime ends up looping forever.
    let id = map.next_id();
    let mut click = Entity::new(id, "point_sound");
    click.set_origin(Vec3::new(240.0, 8.0, 64.0));
    click.set("targetname", "switch_click");
    click.set("sound", "ui/click");
    map.entities.push(click);

    add_brush_entity(
        &mut map,
        "func_brush",
        // Off the main route rather than across it: the sample level should
        // still be walkable by someone who has not found the switch.
        Aabb::new(Vec3::new(240.0, 320.0, 0.0), Vec3::new(272.0, 480.0, 64.0)),
        "dev/wall",
        |e| { e.set("targetname", "shutter"); },
    );

    // Lighting: a lamp in each room, plus sun and sky.
    for (i, x) in [ROOM * 0.5, ROOM * 1.5 + T].iter().enumerate() {
        let id = map.next_id();
        let mut light = Entity::new(id, "light");
        light.set_origin(Vec3::new(*x, ROOM * 0.5, HEIGHT - 64.0));
        light.set("_light", if i == 0 { "255 240 214 320" } else { "214 230 255 320" });
        map.entities.push(light);
    }

    let id = map.next_id();
    let mut sun = Entity::new(id, "light_environment");
    sun.set_origin(Vec3::new(ROOM, ROOM * 0.5, HEIGHT - 32.0));
    sun.set("pitch", "-45");
    sun.set("angles", "0 200 0");
    sun.set("_light", "255 250 235 180");
    sun.set("_ambient", "70 80 100 90");
    map.entities.push(sun);

    let id = map.next_id();
    let mut spawn = Entity::new(id, "info_player_start");
    spawn.set_origin(Vec3::new(128.0, ROOM * 0.5, 16.0));
    spawn.set("angles", "0 0 0");
    map.entities.push(spawn);

    let problems = map.validate();
    if !problems.is_empty() {
        for p in &problems { eprintln!("error: {p}"); }
        std::process::exit(1);
    }

    let path = std::path::Path::new("content/maps/void_start.voidmap");
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, map.to_text())?;
    println!(
        "wrote {} -- {} brushes, {} entities",
        path.display(),
        map.solid_count(),
        map.entities.len()
    );
    Ok(())
}

/// Add a brush entity with fresh ids for its solid and every side.
fn add_brush_entity(
    map: &mut Map,
    classname: &str,
    bounds: Aabb,
    material: &str,
    configure: impl FnOnce(&mut Entity),
) {
    let entity_id = map.next_id();
    let solid_id = map.next_id();
    let side_ids: Vec<u32> = (0..6).map(|_| map.next_id()).collect();

    let mut solid = Solid::cube(bounds, material);
    solid.id = solid_id;
    for (side, id) in solid.sides.iter_mut().zip(side_ids) {
        side.id = id;
    }

    let mut entity = Entity::new(entity_id, classname);
    entity.solids.push(solid);
    configure(&mut entity);
    map.entities.push(entity);
}
