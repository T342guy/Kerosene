// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! A small room to start from.
//!
//! What `kerosene-tools new` puts in a new game, and what the engine opens
//! when a project has no map of its own (`kerosene_room`, in the base
//! content): a lit box in developer textures, a player start, a crate to
//! push and a plinth. Nothing in it is a game.

use crate::{Connection, Entity, Map, Solid};
use kerosene_math::{Aabb, Vec3};

/// The name the engine's base content gives the room.
pub const NAME: &str = "kerosene_room";

/// A room to start in: lit, with a crate, and -- in a game -- an
/// `item_pickup` on a plinth that a `trigger_once` hands to whoever walks
/// up to it. Every texture is the engine's base content, so it builds and
/// looks right before the game has any art of its own.
pub fn room(with_pickup: bool) -> Map {
    const T: f32 = 16.0;
    const W: f32 = 768.0;
    const D: f32 = 512.0;
    const H: f32 = 256.0;
    let mut map = Map::new();
    map.world.set("skyname", "sky_kero");
    let shell = [
        (
            Aabb::new(Vec3::new(-T, -T, -T), Vec3::new(W + T, D + T, 0.0)),
            "dev/floor",
        ),
        (
            Aabb::new(Vec3::new(-T, -T, H), Vec3::new(W + T, D + T, H + T)),
            "dev/ceiling",
        ),
        (
            Aabb::new(Vec3::new(-T, -T, 0.0), Vec3::new(0.0, D + T, H)),
            "dev/wall",
        ),
        (
            Aabb::new(Vec3::new(W, -T, 0.0), Vec3::new(W + T, D + T, H)),
            "dev/wall",
        ),
        (
            Aabb::new(Vec3::new(0.0, -T, 0.0), Vec3::new(W, 0.0, H)),
            "dev/wall",
        ),
        (
            Aabb::new(Vec3::new(0.0, D, 0.0), Vec3::new(W, D + T, H)),
            "dev/wall",
        ),
    ];
    for (bounds, material) in shell {
        map.add_world_solid(Solid::cube(bounds, material));
    }
    // A plinth for the pickup to sit on, across the room from the start.
    let plinth = Aabb::new(Vec3::new(576.0, 224.0, 0.0), Vec3::new(640.0, 288.0, 32.0));
    map.add_world_solid(Solid::cube(plinth, "dev/orange"));

    let point = |map: &mut Map, class: &str, at: Vec3, keys: &[(&str, &str)]| {
        let id = map.next_id();
        let mut e = Entity::new(id, class);
        e.set_origin(at);
        for (k, v) in keys {
            e.set(k, *v);
        }
        map.entities.push(e);
        map.entities.len() - 1
    };
    point(
        &mut map,
        "info_player_start",
        Vec3::new(96.0, D / 2.0, 16.0),
        &[("angles", "0 0 0")],
    );
    point(
        &mut map,
        "light",
        Vec3::new(W / 2.0, D / 2.0, H - 48.0),
        &[("_light", "255 240 220 360")],
    );
    point(
        &mut map,
        "light_environment",
        Vec3::new(W / 2.0, D / 2.0, H - 24.0),
        &[
            ("pitch", "-50"),
            ("angles", "0 210 0"),
            ("_light", "255 250 235 120"),
            ("_ambient", "70 80 100 80"),
        ],
    );
    point(
        &mut map,
        "prop_physics",
        Vec3::new(320.0, 128.0, 24.0),
        &[("model", "props/crate")],
    );

    if with_pickup {
        point(
            &mut map,
            "item_pickup",
            Vec3::new(608.0, 256.0, 48.0),
            &[("targetname", "gem"), ("item", "gem")],
        );
        // The trigger around the plinth, handing the gem to whoever walks in.
        let entity = map.next_id();
        let solid = map.next_id();
        let sides: Vec<u32> = (0..6).map(|_| map.next_id()).collect();
        let mut brush = Solid::cube(
            Aabb::new(Vec3::new(528.0, 176.0, 0.0), Vec3::new(688.0, 336.0, 128.0)),
            "tools/trigger",
        );
        brush.id = solid;
        for (side, id) in brush.sides.iter_mut().zip(sides) {
            side.id = id;
        }
        let mut trigger = Entity::new(entity, "trigger_once");
        trigger.solids.push(brush);
        trigger.connect(Connection::new("OnTrigger", "gem", "Pickup"));
        map.entities.push(trigger);
    }
    map
}
