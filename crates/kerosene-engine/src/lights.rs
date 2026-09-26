// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The dynamic lights in the world this frame.
//!
//! `light_dynamic` entities, read from the same keys Radiance reads for a
//! baked light -- `_light`, the three `_*_attn`, `_cone`, `_inner_cone`,
//! `_exponent`, `pitch` -- so swapping a baked light for a live one is a
//! classname change. And the player's flashlight, which is a spot on the eye.

use kerosene_entity::EntityWorld;
use kerosene_math::light::Attenuation;
use kerosene_math::{Angles, Vec3};
use kerosene_render::lights::{DynamicLight, Spot};

/// Spawnflag 1: start switched off.
///
/// The engine reads the flags and the `on` field itself -- it has no game
/// crate to ask -- so the values are fixed here; `kerosene_game::lights`
/// carries copies for its own use and the schema.
pub const SF_START_OFF: u32 = 1;
/// Spawnflag 2: cast no shadows.
pub const SF_NO_SHADOWS: u32 = 2;
/// The field a game's `light_dynamic` class keeps its switch in. A game that
/// registers no class for it still gets the light, on unless flagged off.
pub const ON_FIELD: &str = "on";

/// Every `light_dynamic` that is switched on.
pub fn entity_lights(entities: &EntityWorld) -> Vec<DynamicLight> {
    entities
        .iter()
        .filter(|e| e.classname == "light_dynamic")
        .filter(|e| e.fields.bool(ON_FIELD, !e.has_spawnflag(SF_START_OFF)))
        .filter_map(|e| {
            let (color, brightness) = light_value(e.fields.text("_light").as_deref())?;
            let cone = e.fields.f32("_cone", 0.0);
            let spot = (cone > 0.0).then(|| {
                let mut angles = e.angles;
                // Upward-positive, the opposite of the engine's pitch, as
                // on a baked light_spot.
                let pitch = e.fields.f32("pitch", 0.0);
                if pitch != 0.0 {
                    angles.pitch = -pitch;
                }
                Spot {
                    direction: angles.forward(),
                    outer: cone.min(89.0),
                    inner: e.fields.f32("_inner_cone", cone * 0.5).min(cone),
                    exponent: e.fields.f32("_exponent", 1.0),
                }
            });
            let distance = e.fields.f32("distance", 0.0);
            Some(DynamicLight {
                origin: e.origin,
                color,
                brightness,
                attenuation: Attenuation {
                    constant: e.fields.f32("_constant_attn", 0.0),
                    linear: e.fields.f32("_linear_attn", 0.0),
                    quadratic: e.fields.f32("_quadratic_attn", 1.0),
                },
                max_distance: (distance > 0.0).then_some(distance),
                spot,
                shadows: !e.has_spawnflag(SF_NO_SHADOWS),
            })
        })
        .collect()
}

/// The player's flashlight: a warm, fairly tight spot from just below and to
/// the right of the eye, the way a torch is held, casting shadows.
///
/// Held off-centre on purpose. A light exactly at the eye throws every
/// shadow straight away from the viewer, where it cannot be seen, and the
/// scene looks flat; a few inches off is what makes the shadows read.
///
/// Linear falloff, not the physical square law. A real torch at that law is
/// a hot spot at arm's length and nothing across a room, and every game with
/// one has bent the curve the same way.
pub fn flashlight(eye: Vec3, angles: Angles) -> DynamicLight {
    let basis = angles.vectors();
    DynamicLight {
        origin: eye - basis.up * 6.0 - basis.right * 6.0 + basis.forward * 4.0,
        color: Vec3::new(1.0, 0.94, 0.82),
        brightness: 300.0,
        attenuation: Attenuation {
            constant: 0.0,
            linear: 1.0,
            quadratic: 0.0,
        },
        max_distance: Some(1024.0),
        spot: Some(Spot {
            direction: basis.forward,
            outer: 28.0,
            inner: 14.0,
            exponent: 1.5,
        }),
        shadows: true,
    }
}

/// Read a `"r g b brightness"` key: Source's four-number spelling, or three
/// numbers for brightness 200, the editor default. The same rule as
/// Radiance's, so a baked and a live light with one `_light` agree.
fn light_value(raw: Option<&str>) -> Option<(Vec3, f32)> {
    let nums: Vec<f32> = raw?
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    match nums.len() {
        3 => Some((Vec3::new(nums[0], nums[1], nums[2]) / 255.0, 200.0)),
        4 => Some((Vec3::new(nums[0], nums[1], nums[2]) / 255.0, nums[3])),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kerosene_kv::KeyValues;

    fn world(src: &str) -> EntityWorld {
        let mut w = EntityWorld::new(kerosene_game::registry());
        w.load_from_kv(&KeyValues::parse(src).unwrap()).unwrap();
        w.run(1.0 / 64.0);
        w
    }

    #[test]
    fn a_light_dynamic_reads_the_keys_a_baked_light_does() {
        let w = world(
            r#"
entity
{
    "classname" "light_dynamic"
    "origin" "10 20 30"
    "_light" "255 128 0 300"
    "_quadratic_attn" "2"
    "distance" "512"
}
"#,
        );
        let lights = entity_lights(&w);
        assert_eq!(lights.len(), 1);
        let l = lights[0];
        assert_eq!(l.origin, Vec3::new(10.0, 20.0, 30.0));
        assert!((l.color - Vec3::new(1.0, 128.0 / 255.0, 0.0)).length() < 1e-5);
        assert_eq!(l.brightness, 300.0);
        assert_eq!(l.attenuation.quadratic, 2.0);
        assert_eq!(l.max_distance, Some(512.0));
        assert!(l.spot.is_none());
        assert!(l.shadows);
    }

    #[test]
    fn a_cone_makes_it_a_spot_aimed_by_its_pitch() {
        let w = world(
            r#"
entity
{
    "classname" "light_dynamic"
    "_light" "255 255 255"
    "_cone" "40"
    "_inner_cone" "20"
    "pitch" "-90"
    "spawnflags" "2"
}
"#,
        );
        let l = entity_lights(&w)[0];
        let spot = l.spot.unwrap();
        assert!(
            (spot.direction - -Vec3::Z).length() < 1e-4,
            "{}",
            spot.direction
        );
        assert_eq!((spot.outer, spot.inner), (40.0, 20.0));
        assert_eq!(l.brightness, 200.0, "three numbers mean brightness 200");
        assert!(!l.shadows, "spawnflag 2");
    }

    #[test]
    fn a_light_that_is_off_is_not_drawn() {
        let w = world(
            r#"entity { "classname" "light_dynamic" "_light" "255 255 255 200" "spawnflags" "1" }"#,
        );
        assert!(entity_lights(&w).is_empty());
    }

    #[test]
    fn the_flashlight_points_where_the_player_looks() {
        let angles = Angles::new(0.0, 90.0, 0.0);
        let light = flashlight(Vec3::ZERO, angles);
        let spot = light.spot.unwrap();
        assert!((spot.direction - Vec3::Y).length() < 1e-4);
        assert!(light.shadows);
    }
}
