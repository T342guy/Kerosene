// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! What a trigger volume does to the player: the engine's side of triggers.
//!
//! A trigger is a brush that is not solid but that the player can be inside
//! of. Working out *whether* the player is inside is the engine's business
//! -- it has the BSP, the player box and the brush models -- and so is what
//! happens then, because every consequence is to the player: a door's
//! outputs fire, a hurt volume takes health, a push pad throws them, a
//! teleporter moves them. None of that needs a game's opinion, so it is an
//! engine convention rather than a hook, and a game gets it whatever
//! classes it registers.
//!
//! The convention, for anything that wants to be a trigger: the classname
//! starts with `trigger_` (that is what makes the engine test the volume at
//! all), `disabled` turns it off, `occupied` is the engine's own record of
//! whether the player was inside last tick, and the three outputs are
//! `OnStartTouch`, `OnEndTouch` and `OnTrigger`. `trigger_hurt` reads
//! `damage` per second; `trigger_push` reads `pushdir` and `speed`;
//! `trigger_teleport` reads `target`; `trigger_once` removes itself. The
//! stock game registers those five with matching inputs and outputs; a game
//! that registers its own `trigger_*` class gets the touch outputs for free.

use kerosene_entity::{EntityId, EntityWorld, Value};
use kerosene_math::Vec3;

fn set_field(world: &mut EntityWorld, id: EntityId, key: &str, value: Value) {
    if let Some(e) = world.get_mut(id) {
        e.fields.set(key, value);
    }
}

/// Tell a trigger whether something is inside it this tick.
///
/// The engine calls this for every trigger each tick; the edge detection lives
/// here so that a trigger fires on the *transition* rather than continuously.
/// `trigger_once` removes itself after firing, which is the only difference
/// between it and `trigger_multiple`.
pub fn update_touch(
    world: &mut EntityWorld,
    id: EntityId,
    inside: bool,
    activator: Option<EntityId>,
) {
    let Some(entity) = world.get(id) else { return };
    if entity.fields.bool("disabled", false) {
        return;
    }

    let was_inside = entity.fields.bool("occupied", false);
    if inside == was_inside {
        return;
    }

    set_field(world, id, "occupied", Value::Bool(inside));

    if inside {
        world.fire_output(id, "OnStartTouch", activator, None);
        world.fire_output(id, "OnTrigger", activator, None);

        let once = world
            .get(id)
            .map(|e| e.classname.eq_ignore_ascii_case("trigger_once"))
            .unwrap_or(false);
        if once {
            world.remove(id);
        }
    } else {
        world.fire_output(id, "OnEndTouch", activator, None);
    }
}

/// The shove a `trigger_push` gives, if it is one.
///
/// An impulse applied on entry rather than a force applied while inside. A
/// launch pad is what these are almost always for, and a continuous push
/// would also mean a player standing in one could not walk out of it.
pub fn push_of(world: &EntityWorld, id: EntityId) -> Option<(Vec3, f32)> {
    let entity = world.get(id)?;
    if !entity.classname.eq_ignore_ascii_case("trigger_push") {
        return None;
    }
    let dir = entity.fields.vec3("pushdir", Vec3::Z);
    let speed = entity.fields.f32("speed", 400.0);
    let dir = dir.normalize_or_zero();
    if dir.length_squared() < 1e-6 || speed == 0.0 {
        return None;
    }
    Some((dir, speed))
}

/// Where a `trigger_teleport` sends things, if it is one.
///
/// A targetname rather than a position, so the destination can be moved in the
/// editor without anyone editing a number, and so several teleports can share
/// one.
pub fn teleport_target(world: &EntityWorld, id: EntityId) -> Option<String> {
    let entity = world.get(id)?;
    if !entity.classname.eq_ignore_ascii_case("trigger_teleport") {
        return None;
    }
    let target = entity.fields.text("target")?;
    let target = target.trim();
    (!target.is_empty()).then(|| target.to_string())
}

/// Damage a `trigger_hurt` deals per second, if any.
pub fn hurt_per_second(world: &EntityWorld, id: EntityId) -> f32 {
    world.get(id).map_or(0.0, |e| {
        if e.classname.eq_ignore_ascii_case("trigger_hurt") {
            e.fields.f32("damage", 10.0)
        } else {
            0.0
        }
    })
}

#[cfg(test)]
mod tests {
    //! Driven through the stock game's classes, so the inputs that flip
    //! `disabled` are the real ones and not a copy.

    use super::*;
    use kerosene_entity::InputEvent;
    use kerosene_kv::KeyValues;

    const TICK: f32 = 1.0 / 64.0;

    const TRIGGER_MAP: &str = r#"
entity
{
    "classname" "trigger_multiple"
    "targetname" "zone"
    "model" "*2"
    connections { "OnStartTouch" "counter,Add,1,0,-1" "OnEndTouch" "counter,Subtract,1,0,-1" }
}
entity { "classname" "math_counter" "targetname" "counter" }
"#;

    fn world_from(src: &str) -> EntityWorld {
        let mut w = EntityWorld::new(kerosene_game::registry());
        let kv = KeyValues::parse(src).expect("test map parses");
        w.load_from_kv(&kv).expect("entities load");
        w
    }

    fn named(w: &EntityWorld, name: &str) -> EntityId {
        *w.find_by_name(name)
            .first()
            .unwrap_or_else(|| panic!("no entity named {name}"))
    }

    fn field(w: &EntityWorld, id: EntityId, key: &str) -> f32 {
        w.get(id).map(|e| e.fields.f32(key, -1.0)).unwrap_or(-1.0)
    }

    #[test]
    fn a_trigger_fires_on_entering_and_leaving_not_continuously() {
        let mut w = world_from(TRIGGER_MAP);
        let zone = named(&w, "zone");
        let counter = named(&w, "counter");

        // Standing inside for many ticks should fire once, not once per tick.
        for _ in 0..20 {
            update_touch(&mut w, zone, true, None);
            w.run(TICK);
        }
        assert_eq!(field(&w, counter, "value"), 1.0);

        for _ in 0..20 {
            update_touch(&mut w, zone, false, None);
            w.run(TICK);
        }
        assert_eq!(field(&w, counter, "value"), 0.0);
    }

    #[test]
    fn a_trigger_once_removes_itself_after_firing() {
        let src = TRIGGER_MAP.replace("trigger_multiple", "trigger_once");
        let mut w = world_from(&src);
        let zone = named(&w, "zone");
        update_touch(&mut w, zone, true, None);
        w.run(TICK);
        assert!(
            !w.exists(zone),
            "a trigger_once should be gone after it fires"
        );
        assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);
    }

    #[test]
    fn a_disabled_trigger_does_not_fire() {
        let mut w = world_from(TRIGGER_MAP);
        let zone = named(&w, "zone");
        w.accept_input(zone, &InputEvent::new("Disable"));
        update_touch(&mut w, zone, true, None);
        w.run(TICK);
        assert_eq!(field(&w, named(&w, "counter"), "value"), 0.0);

        w.accept_input(zone, &InputEvent::new("Enable"));
        update_touch(&mut w, zone, true, None);
        w.run(TICK);
        assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);
    }

    #[test]
    fn disabling_an_occupied_trigger_releases_it() {
        // Otherwise the trigger believes it is occupied forever and never
        // fires OnStartTouch again.
        let mut w = world_from(TRIGGER_MAP);
        let zone = named(&w, "zone");
        update_touch(&mut w, zone, true, None);
        w.run(TICK);
        assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);

        w.accept_input(zone, &InputEvent::new("Disable"));
        w.run(TICK);
        assert_eq!(
            field(&w, named(&w, "counter"), "value"),
            0.0,
            "OnEndTouch should have fired"
        );

        w.accept_input(zone, &InputEvent::new("Enable"));
        update_touch(&mut w, zone, true, None);
        w.run(TICK);
        assert_eq!(
            field(&w, named(&w, "counter"), "value"),
            1.0,
            "it should fire again"
        );
    }

    #[test]
    fn push_and_teleport_and_hurt_read_their_fields() {
        let mut w = world_from(
            r#"
entity { "classname" "trigger_push" "targetname" "pad" "pushdir" "0 0 2" "speed" "500" }
entity { "classname" "trigger_push" "targetname" "dud" "speed" "0" }
entity { "classname" "trigger_teleport" "targetname" "tp" "target" " dest " }
entity { "classname" "trigger_hurt" "targetname" "lava" "damage" "25" }
entity { "classname" "trigger_multiple" "targetname" "plain" }
"#,
        );
        let (pad, dud, tp, lava, plain) = (
            named(&w, "pad"),
            named(&w, "dud"),
            named(&w, "tp"),
            named(&w, "lava"),
            named(&w, "plain"),
        );
        assert_eq!(push_of(&w, pad), Some((Vec3::Z, 500.0)));
        assert_eq!(push_of(&w, dud), None);
        assert_eq!(push_of(&w, plain), None);
        assert_eq!(teleport_target(&w, tp).as_deref(), Some("dest"));
        assert_eq!(teleport_target(&w, plain), None);
        assert_eq!(hurt_per_second(&w, lava), 25.0);
        assert_eq!(hurt_per_second(&w, plain), 0.0);
        // Gone once the world has reclaimed it.
        w.remove(lava);
        w.run(TICK);
        assert_eq!(hurt_per_second(&w, lava), 0.0);
    }
}
