// SPDX-License-Identifier: LGPL-3.0-or-later
//! Core math for VoidEngine.
//!
//! VoidEngine inherits Source's conventions, because the tools and the map
//! formats only make sense in them:
//!
//! * **Units are inches.** One world unit is one inch. A player is 72 units
//!   tall and 32 wide. The world is bounded to +/- [`MAX_MAP_COORD`].
//! * **Z is up.** `+X` is forward, `+Y` is left, `+Z` is up. This is a
//!   right-handed system and it is *not* what most modern engines use, but it
//!   is what brush geometry, `.voidmap` files and the entity angle conventions
//!   all assume.
//! * **Angles are pitch/yaw/roll**, in degrees, in that order -- see
//!   [`Angles`]. Pitch is positive *downward*, which is a Quake inheritance
//!   that Source never corrected and neither do we.
//!
//! [`Vec3`] and friends come from `glam`; this crate adds the pieces a BSP
//! engine needs on top: planes, polygon windings with exact clipping, and
//! axis-aligned bounds.

mod aabb;
mod angles;
mod plane;
pub mod units;
mod winding;

pub use aabb::Aabb;
pub use angles::{Angles, Basis, angle_diff, wrap180};
pub use plane::{Plane, PlaneKind, PlaneSet, PlaneSide};
pub use winding::Winding;

/// Format a number the way level data is written: no trailing zeroes, and no
/// decimal point on a whole number.
///
/// Lives here rather than beside the text formats because everything that
/// shows a coordinate needs it -- the editor's readouts as much as the file
/// writers -- and two implementations would drift.
pub fn format_float(v: f32) -> String {
    if v == v.trunc() && v.abs() < 1e9 {
        // `-0.0` is a whole number whose sign nobody wants to read.
        format!("{}", (v + 0.0) as i64)
    } else {
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4, vec2, vec3, vec4};

/// Half-extent of the legal world, in inches. Matches Source's `MAX_COORD`.
///
/// Geometry outside this is rejected by the compiler rather than silently
/// producing a broken tree; the value is chosen so that coordinates stay
/// exactly representable in f32 at the precision the tools assume.
pub const MAX_MAP_COORD: f32 = 16_384.0;

/// Diagonal of the world cube -- the length of the longest possible ray.
pub const MAX_MAP_RANGE: f32 = MAX_MAP_COORD * 3.4641016; // sqrt(3) * 2 * half

/// Distance under which a point counts as lying *on* a plane.
///
/// 0.1 inch, straight from Quake/Source. Loose enough to absorb the drift of
/// repeated plane intersections during CSG, tight enough that a 1-unit grid
/// never collapses.
pub const ON_EPSILON: f32 = 0.1;

/// Collision skin thickness, 1/32 inch. Traces stop this far short of a
/// surface so that the next frame's start point is never *inside* it.
pub const DIST_EPSILON: f32 = 0.031_25;

/// Tolerance for treating two plane normals as identical during dedup.
pub const NORMAL_EPSILON: f32 = 0.000_01;

/// Tolerance for treating two plane distances as identical during dedup.
pub const PLANE_DIST_EPSILON: f32 = 0.01;

/// Snap a value to the nearest integer if it is within [`NORMAL_EPSILON`].
///
/// Axis-aligned brushes are overwhelmingly common, and their normals should
/// come out of arithmetic as exactly +/-1 so that plane dedup collapses them.
#[inline]
pub fn snap_near_integer(v: f32) -> f32 {
    let r = v.round();
    if (v - r).abs() < NORMAL_EPSILON { r } else { v }
}

/// Snap each component of a normal, then renormalize if a snap fired.
#[inline]
pub fn snap_normal(mut n: Vec3) -> Vec3 {
    let before = n;
    n.x = snap_near_integer(n.x);
    n.y = snap_near_integer(n.y);
    n.z = snap_near_integer(n.z);
    if n != before { n = n.normalize_or_zero(); }
    n
}

/// Index of the largest-magnitude component of `v` (0 = X, 1 = Y, 2 = Z).
///
/// Used to pick a projection axis for texture mapping and for choosing which
/// plane of a box a point escaped through.
#[inline]
pub fn major_axis(v: Vec3) -> usize {
    let a = v.abs();
    if a.x >= a.y && a.x >= a.z { 0 } else if a.y >= a.z { 1 } else { 2 }
}

/// Linear interpolation that is exact at both endpoints.
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }
