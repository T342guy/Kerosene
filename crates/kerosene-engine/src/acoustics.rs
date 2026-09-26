// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! What the map says the listener's surroundings sound like.
//!
//! Resonance wrote a room record per region of the map; the mixer's reverb
//! wants a [`ReverbParams`]; this is the walk from the first to the second,
//! every tick. It is a leaf lookup and a copy, plus one refinement: near a
//! boundary between two rooms the two are blended by how close the boundary
//! is, so stepping through a doorway is a slide rather than a switch. The
//! mixer smooths on top of that, but its smoothing is in time and this is
//! in space -- standing still in a doorway should sound like a doorway.
//!
//! Occlusion lives here too: whether a sound can reach the ear at all, and
//! how much wall is in the way. Both are map questions, and the map is the
//! engine's, not the mixer's.

use kerosene_audio::ReverbParams;
use kerosene_bsp::{AcousticRoom, Bsp, contents};
use kerosene_math::{Basis, Vec3};

/// How close to a room boundary the blend towards the next room begins,
/// in units. A couple of paces.
pub const BLEND_DISTANCE: f32 = 64.0;

/// The mixer's description of a room.
pub fn params_for(room: &AcousticRoom) -> ReverbParams {
    ReverbParams {
        rt60: room.rt60,
        predelay: room.predelay,
        diffusion: room.diffusion,
        wet: room.wet,
        enabled: true,
    }
}

/// Where the listener is, acoustically.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surroundings {
    /// The room the listener stands in.
    pub room: u16,
    /// The room's figures, blended towards a neighbour when near one.
    pub params: ReverbParams,
    /// The neighbouring room being blended towards, and how much.
    pub blending: Option<(u16, f32)>,
}

/// The listener's surroundings, or `None` if the map has no acoustics or
/// the listener is somewhere nothing was measured.
pub fn surroundings(bsp: &Bsp, eye: Vec3) -> Option<Surroundings> {
    let acoustics = bsp.acoustics.as_ref()?;
    let leaf = bsp.point_leaf(eye);
    let (room, record) = acoustics.room_of_leaf(leaf)?;
    let mut params = params_for(record);

    // Look across each face of the leaf's box that the eye is near. The
    // leaf on the other side may be the same room, a wall, or another room;
    // only the last matters, and the nearest such wins.
    let bounds = bsp.leaves[leaf].bounds();
    let mut nearest: Option<(u16, f32)> = None;
    for axis in 0..3 {
        for (face, sign) in [(bounds.min[axis], -1.0f32), (bounds.max[axis], 1.0)] {
            let distance = (eye[axis] - face).abs();
            if distance >= BLEND_DISTANCE {
                continue;
            }
            let mut probe = eye;
            probe[axis] = face + sign * 1.0;
            let other_leaf = bsp.point_leaf(probe);
            let Some((other, _)) = acoustics.room_of_leaf(other_leaf) else {
                continue;
            };
            if other == room {
                continue;
            }
            if nearest.is_none_or(|(_, d)| distance < d) {
                nearest = Some((other, distance));
            }
        }
    }

    let blending = nearest.map(|(other, distance)| {
        // Half way at the boundary itself, so the two sides agree there.
        let t = 0.5 * (1.0 - distance / BLEND_DISTANCE);
        let towards = params_for(&acoustics.rooms[other as usize]);
        params = blend(&params, &towards, t);
        (other, t)
    });

    Some(Surroundings {
        room,
        params,
        blending,
    })
}

/// `t` of the way from `a` to `b`. Decay times blend geometrically, the way
/// the ear hears them.
pub fn blend(a: &ReverbParams, b: &ReverbParams, t: f32) -> ReverbParams {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    let mut rt60 = [0.0; 4];
    for (i, r) in rt60.iter_mut().enumerate() {
        *r = (a.rt60[i].max(1e-3).ln() + (b.rt60[i].max(1e-3).ln() - a.rt60[i].max(1e-3).ln()) * t)
            .exp();
    }
    ReverbParams {
        rt60,
        predelay: lerp(a.predelay, b.predelay),
        diffusion: lerp(a.diffusion, b.diffusion),
        wet: lerp(a.wet, b.wet),
        enabled: a.enabled || b.enabled,
    }
}

/// What lies between a sound and the ear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reach {
    /// Nothing the map knows of: heard as is.
    Clear,
    /// Some wall in the way: `0` is none, `1` is fully behind one.
    Occluded(f32),
    /// Not audible from here at all -- no path sound could take.
    Unreachable,
}

/// How much wall lies between a sound at `source` and an ear at `eye`.
///
/// Three questions, cheapest first. Is the source's cluster in the
/// listener's *potentially audible set* -- the PVS grown by one room, which
/// Umbra computes and nothing else has used until now? If not, no path
/// exists and the sound is not heard. Otherwise, how many of three lines
/// from the ear to the source, one straight and one to each side of it, are
/// blocked? A wall blocks all three; a doorway lets one or two through, and
/// the sound comes round it thinned rather than cut. Being in the same room
/// halves the result: a pillar between two people in a hall is not a wall.
pub fn reach(bsp: &Bsp, eye: Vec3, basis: &Basis, source: Vec3) -> Reach {
    let source_leaf = bsp.point_leaf(source);
    let eye_leaf = bsp.point_leaf(eye);
    let source_cluster = bsp.leaves[source_leaf].cluster;
    let eye_cluster = bsp.leaves[eye_leaf].cluster;
    // A source inside the rock has no way out. The ear inside it -- noclip
    // through a wall -- is left hearing everything rather than nothing.
    if !bsp.visibility.is_empty() && source_cluster < 0 {
        return Reach::Unreachable;
    }
    if !bsp.cluster_audible(eye_cluster, source_cluster) {
        return Reach::Unreachable;
    }

    let mask = contents::SOLID | contents::MOVEABLE | contents::WINDOW;
    let side = basis.right * 40.0;
    let mut blocked = 0;
    for end in [source, source + side, source - side] {
        if bsp.trace_ray(eye, end, mask).fraction < 1.0 {
            blocked += 1;
        }
    }
    if blocked == 0 {
        return Reach::Clear;
    }
    let mut occlusion = blocked as f32 / 3.0;
    let same_room = match &bsp.acoustics {
        Some(a) => {
            let (Some((r1, _)), Some((r2, _))) =
                (a.room_of_leaf(eye_leaf), a.room_of_leaf(source_leaf))
            else {
                return Reach::Occluded(occlusion);
            };
            r1 == r2
        }
        None => false,
    };
    if same_room {
        occlusion *= 0.5;
    }
    Reach::Occluded(occlusion)
}

/// How far from the eye room boxes are drawn, in units. Beyond it the
/// overlay is a haze of lines with nothing to learn from.
const DEBUG_RANGE: f32 = 2048.0;

/// Wireframes for `snd_acoustics_debug 2`: every leaf near the eye boxed
/// in its room's colour -- blue for a dead room, red for a live one -- and a
/// line from the eye to every followed sound, green where it gets through
/// and red where it does not.
pub fn debug_lines(
    bsp: &Bsp,
    eye: Vec3,
    voices: impl Iterator<Item = (Vec3, Option<f32>)>,
) -> Vec<crate::physics::DebugLine> {
    use crate::physics::{BOX_EDGES, DebugLine};
    let mut lines = Vec::new();
    if let Some(acoustics) = &bsp.acoustics {
        for (i, leaf) in bsp.leaves.iter().enumerate() {
            let Some((_, room)) = acoustics.room_of_leaf(i) else {
                continue;
            };
            let bounds = leaf.bounds();
            if bounds.center().distance(eye) > DEBUG_RANGE {
                continue;
            }
            // Blue at a fifth of a second, red at three seconds, on a log
            // scale between.
            let t = ((room.rt60[1].max(0.01).ln() - 0.2f32.ln()) / (3.0f32.ln() - 0.2f32.ln()))
                .clamp(0.0, 1.0);
            let color = [t, 0.2, 1.0 - t];
            // Corners in the bit order `BOX_EDGES` expects: x, y, z.
            let corners: [Vec3; 8] = std::array::from_fn(|i| {
                Vec3::new(
                    if i & 1 == 0 {
                        bounds.min.x
                    } else {
                        bounds.max.x
                    },
                    if i & 2 == 0 {
                        bounds.min.y
                    } else {
                        bounds.max.y
                    },
                    if i & 4 == 0 {
                        bounds.min.z
                    } else {
                        bounds.max.z
                    },
                )
            });
            for (a, b) in BOX_EDGES {
                lines.push(DebugLine {
                    a: corners[a],
                    b: corners[b],
                    color,
                });
            }
        }
    }
    for (position, occlusion) in voices {
        let color = match occlusion {
            None => [1.0, 0.0, 0.0],
            Some(o) => [o, 1.0 - o, 0.0],
        };
        lines.push(DebugLine {
            a: eye,
            b: position,
            color,
        });
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(rt: f32, wet: f32) -> ReverbParams {
        ReverbParams {
            rt60: [rt; 4],
            predelay: 0.01,
            diffusion: 0.5,
            wet,
            enabled: true,
        }
    }

    #[test]
    fn blending_is_geometric_in_decay_and_linear_in_level() {
        let a = params(1.0, 0.2);
        let b = params(4.0, 0.6);
        let mid = blend(&a, &b, 0.5);
        assert!((mid.rt60[0] - 2.0).abs() < 1e-4, "{}", mid.rt60[0]);
        assert!((mid.wet - 0.4).abs() < 1e-6);
        assert_eq!(blend(&a, &b, 0.0), a);
        assert_eq!(blend(&a, &b, 1.0), b);
        assert_eq!(blend(&a, &b, 7.0), b, "clamped");
    }

    #[test]
    fn a_map_without_acoustics_has_no_surroundings() {
        let bsp = Bsp::new();
        assert_eq!(surroundings(&bsp, Vec3::ZERO), None);
    }
}
