// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `env_acoustic_override`: the designer having the last word.
//!
//! The probe is right about geometry and materials and knows nothing about
//! intent. A boiler room that should ring like a cathedral for the scene
//! that happens in it, a corridor whose measured tail is a second too long
//! for comfort -- those are one point entity each. The override replaces
//! the measured figures of the room its origin is in, and of every room a
//! non-zero radius reaches, and the room is flagged so a designer reading
//! the numbers later knows they were placed rather than found.

use kerosene_bsp::Bsp;
use kerosene_bsp::acoustics::{Acoustics, room_flags};
use kerosene_kv::{FromKvValue, KeyValues, Vec3Value};
use kerosene_math::{Aabb, Vec3};

pub const CLASSNAME: &str = "env_acoustic_override";

/// One placed override.
#[derive(Clone, Debug, PartialEq)]
pub struct Override {
    pub origin: Vec3,
    /// Reaches every room with a leaf within this of the origin. Zero is
    /// the room the origin is in and nothing more.
    pub radius: f32,
    /// Seconds per band; `None` keeps the measured figure.
    pub rt60: Option<[f32; 4]>,
    pub wet: Option<f32>,
    pub predelay: Option<f32>,
    pub openness: Option<f32>,
}

impl Override {
    /// Read one from its entity block, or `None` if it is not one.
    pub fn parse(entity: &KeyValues) -> Option<Override> {
        if entity.get("classname") != Some(CLASSNAME) {
            return None;
        }
        let origin = entity
            .get("origin")
            .and_then(|s| Vec3Value::from_kv(s).ok())
            .map(|v| Vec3::from_array(v.to_array()))
            .unwrap_or(Vec3::ZERO);
        let number = |key: &str| -> Option<f32> {
            let v: f32 = entity.get(key)?.trim().parse().ok()?;
            (v >= 0.0 && v.is_finite()).then_some(v)
        };
        let rt60 = entity.get("rt60").and_then(|s| {
            let parts: Vec<f32> = s
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            match parts.as_slice() {
                [a, b, c, d] => Some([*a, *b, *c, *d]),
                // One number is every band.
                [one] => Some([*one; 4]),
                _ => None,
            }
        });
        Some(Override {
            origin,
            radius: number("radius").unwrap_or(0.0),
            rt60,
            wet: number("wet"),
            predelay: number("predelay"),
            openness: number("openness"),
        })
    }

    /// Every override in a map.
    pub fn all(bsp: &Bsp) -> Vec<Override> {
        match bsp.entities_kv() {
            Ok(kv) => kv.blocks("entity").filter_map(Override::parse).collect(),
            Err(e) => {
                log::warn!("resonance: entity lump did not parse ({e}); no overrides");
                Vec::new()
            }
        }
    }
}

/// Apply every override. Returns how many rooms were changed.
pub fn apply(
    bsp: &Bsp,
    acoustics: &mut Acoustics,
    bounds: &[Aabb],
    overrides: &[Override],
) -> usize {
    let mut changed = 0;
    for o in overrides {
        let mut rooms: Vec<u16> = Vec::new();
        if let Some((room, _)) = acoustics.room_of_leaf(bsp.point_leaf(o.origin)) {
            rooms.push(room);
        }
        if o.radius > 0.0 {
            for (leaf, &room) in acoustics.leaf_room.iter().enumerate() {
                if room != kerosene_bsp::NO_ROOM
                    && !rooms.contains(&room)
                    && sphere_touches(bounds[leaf], o.origin, o.radius)
                {
                    rooms.push(room);
                }
            }
        }
        if rooms.is_empty() {
            log::warn!(
                "resonance: {CLASSNAME} at {} is not in any room and reaches none",
                o.origin
            );
        }
        for room in rooms {
            let r = &mut acoustics.rooms[room as usize];
            if let Some(rt60) = o.rt60 {
                r.rt60 = rt60.map(|t| t.clamp(crate::probe::MIN_RT60, crate::probe::MAX_RT60));
            }
            if let Some(wet) = o.wet {
                r.wet = wet.clamp(0.0, 1.0);
            }
            if let Some(predelay) = o.predelay {
                r.predelay = predelay.clamp(0.0, crate::probe::MAX_PREDELAY);
            }
            if let Some(openness) = o.openness {
                r.openness = openness.clamp(0.0, 1.0);
                if openness > crate::probe::OUTDOOR_OPENNESS {
                    r.flags |= room_flags::OUTDOOR;
                } else {
                    r.flags &= !room_flags::OUTDOOR;
                }
            }
            r.flags |= room_flags::OVERRIDE;
            changed += 1;
        }
    }
    changed
}

fn sphere_touches(bounds: Aabb, centre: Vec3, radius: f32) -> bool {
    let nearest = centre.clamp(bounds.min, bounds.max);
    nearest.distance_squared(centre) <= radius * radius
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_override_reads_its_keys_and_leaves_the_rest_measured() {
        let kv = KeyValues::parse(
            r#"entity {
                "classname" "env_acoustic_override"
                "origin" "10 20 30"
                "radius" "128"
                "rt60" "3 2.5 2 1"
                "wet" "0.5"
            }"#,
        )
        .unwrap();
        let o = Override::parse(kv.blocks("entity").next().unwrap()).unwrap();
        assert_eq!(o.origin, Vec3::new(10.0, 20.0, 30.0));
        assert_eq!(o.radius, 128.0);
        assert_eq!(o.rt60, Some([3.0, 2.5, 2.0, 1.0]));
        assert_eq!(o.wet, Some(0.5));
        assert_eq!(o.predelay, None);
        assert_eq!(o.openness, None);
    }

    #[test]
    fn one_number_is_every_band_and_nonsense_is_ignored() {
        let kv = KeyValues::parse(
            r#"entity {
                "classname" "env_acoustic_override"
                "rt60" "1.5"
                "wet" "-3"
                "predelay" "banana"
            }"#,
        )
        .unwrap();
        let o = Override::parse(kv.blocks("entity").next().unwrap()).unwrap();
        assert_eq!(o.rt60, Some([1.5; 4]));
        assert_eq!(o.wet, None, "a negative value keeps the measured one");
        assert_eq!(o.predelay, None);
        assert_eq!(o.radius, 0.0);
    }

    #[test]
    fn other_classes_are_not_overrides() {
        let kv = KeyValues::parse(r#"entity { "classname" "light" "rt60" "1" }"#).unwrap();
        assert_eq!(Override::parse(kv.blocks("entity").next().unwrap()), None);
    }

    #[test]
    fn a_sphere_touches_the_box_it_reaches() {
        let b = Aabb::new(Vec3::ZERO, Vec3::splat(10.0));
        assert!(sphere_touches(b, Vec3::splat(5.0), 1.0));
        assert!(sphere_touches(b, Vec3::new(15.0, 5.0, 5.0), 5.0));
        assert!(!sphere_touches(b, Vec3::new(15.0, 5.0, 5.0), 4.9));
    }
}
