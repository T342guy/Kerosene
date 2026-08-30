// SPDX-License-Identifier: LGPL-3.0-or-later
//! Brush face texture projection.
//!
//! A brush face carries no UVs. Instead it carries two *texture axes* -- world
//! vectors that a point is projected onto to get U and V. This is what makes
//! brush texturing feel the way it does in Hammer: drag a brush and the
//! texture stays locked to world space rather than sliding with the geometry.
//!
//! `u = (point . uaxis) / uscale + uoffset`
//!
//! The `.keromap` spelling matches Source's: `"[x y z offset] scale"`.

use kerosene_math::{Plane, Vec3};

/// One texture axis: a world direction, a texel offset, and a scale in
//  world-units-per-texel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureAxis {
    pub axis: Vec3,
    pub offset: f32,
    /// World units per texel. Larger means the texture stretches further.
    /// Never zero -- see [`TextureAxis::safe_scale`].
    pub scale: f32,
}

impl TextureAxis {
    pub fn new(axis: Vec3, offset: f32, scale: f32) -> Self { Self { axis, offset, scale } }

    /// The scale to actually divide by.
    ///
    /// A zero scale reaches the compiler more often than you would think --
    /// hand-edited files, a tool that wrote a default-initialised struct -- and
    /// dividing by it produces infinite UVs that poison the lightmap packer
    /// several stages later. Substituting the Hammer default keeps the face.
    #[inline]
    pub fn safe_scale(&self) -> f32 {
        if self.scale.abs() < 1e-6 { 0.25 } else { self.scale }
    }

    /// Texture coordinate of a world point, in texels.
    #[inline]
    pub fn project(&self, point: Vec3) -> f32 {
        point.dot(self.axis) / self.safe_scale() + self.offset
    }

    pub fn parse(s: &str) -> Option<TextureAxis> {
        // "[x y z offset] scale"
        let open = s.find('[')?;
        let close = s.find(']')?;
        let inner: Vec<f32> = s[open + 1..close]
            .split_whitespace()
            .map(|t| t.parse().ok())
            .collect::<Option<Vec<f32>>>()?;
        if inner.len() != 4 { return None; }
        let scale: f32 = s[close + 1..].trim().parse().unwrap_or(0.25);
        Some(TextureAxis {
            axis: Vec3::new(inner[0], inner[1], inner[2]),
            offset: inner[3],
            scale,
        })
    }

    pub fn to_kv(&self) -> String {
        use kerosene_kv::format_float as f;
        format!(
            "[{} {} {} {}] {}",
            f(self.axis.x), f(self.axis.y), f(self.axis.z), f(self.offset), f(self.scale)
        )
    }
}

impl Default for TextureAxis {
    fn default() -> Self {
        Self { axis: Vec3::X, offset: 0.0, scale: 0.25 }
    }
}

/// The six candidate projection bases, one per axis-aligned face direction.
///
/// Taken from Quake's `baseaxis` table and carried through Source unchanged.
/// Entry `i` is `[normal, uaxis, vaxis]`. The `-Y` and `-Z` choices are what
/// make textures read right side up on walls and ceilings rather than mirrored.
const BASE_AXES: [[Vec3; 3]; 6] = [
    // floor
    [Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0)],
    // ceiling
    [Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0)],
    // west wall
    [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, -1.0)],
    // east wall
    [Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, -1.0)],
    // south wall
    [Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0)],
    // north wall
    [Vec3::new(0.0, -1.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0)],
];

/// Default world-aligned texture axes for a plane.
///
/// Picks whichever of the six cardinal projections faces most like the plane
/// does. This is what a face gets when it is first created, before anyone
/// touches it in the face editor.
pub fn default_axes_for_plane(plane: &Plane, scale: f32) -> (TextureAxis, TextureAxis) {
    let mut best = -f32::INFINITY;
    let mut best_i = 0;
    for (i, entry) in BASE_AXES.iter().enumerate() {
        let dot = plane.normal.dot(entry[0]);
        if dot > best { best = dot; best_i = i; }
    }
    (
        TextureAxis::new(BASE_AXES[best_i][1], 0.0, scale),
        TextureAxis::new(BASE_AXES[best_i][2], 0.0, scale),
    )
}

/// Rotate a pair of texture axes within the face plane, in degrees.
pub fn rotate_axes(
    plane: &Plane,
    u: TextureAxis,
    v: TextureAxis,
    degrees: f32,
) -> (TextureAxis, TextureAxis) {
    let n = plane.normal;
    let (s, c) = degrees.to_radians().sin_cos();
    // Rodrigues rotation about the face normal.
    let rot = |x: Vec3| x * c + n.cross(x) * s + n * n.dot(x) * (1.0 - c);
    (
        TextureAxis { axis: rot(u.axis), ..u },
        TextureAxis { axis: rot(v.axis), ..v },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_writes_the_vmap_spelling() {
        let a = TextureAxis::parse("[1 0 0 32] 0.25").unwrap();
        assert_eq!(a.axis, Vec3::X);
        assert_eq!(a.offset, 32.0);
        assert_eq!(a.scale, 0.25);
        assert_eq!(a.to_kv(), "[1 0 0 32] 0.25");
    }

    #[test]
    fn malformed_axes_are_rejected_not_guessed() {
        assert!(TextureAxis::parse("1 0 0 32 0.25").is_none());
        assert!(TextureAxis::parse("[1 0 0] 0.25").is_none());
    }

    #[test]
    fn zero_scale_falls_back_instead_of_producing_infinity() {
        let a = TextureAxis::new(Vec3::X, 0.0, 0.0);
        let u = a.project(Vec3::new(64.0, 0.0, 0.0));
        assert!(u.is_finite(), "a zero scale must not produce infinite UVs");
        assert_eq!(u, 256.0);
    }

    #[test]
    fn projection_scales_and_offsets() {
        let a = TextureAxis::new(Vec3::X, 10.0, 0.25);
        assert_eq!(a.project(Vec3::new(64.0, 0.0, 0.0)), 64.0 / 0.25 + 10.0);
    }

    #[test]
    fn floor_and_wall_get_different_default_projections() {
        let (u, _) = default_axes_for_plane(&Plane::new(Vec3::Z, 0.0), 0.25);
        assert_eq!(u.axis, Vec3::X);
        let (u, v) = default_axes_for_plane(&Plane::new(Vec3::X, 0.0), 0.25);
        assert_eq!(u.axis, Vec3::Y);
        assert_eq!(v.axis, -Vec3::Z);
    }

    #[test]
    fn default_axes_are_perpendicular_to_the_face_normal() {
        // A projection axis with a component along the normal would smear the
        // texture as the face is walked.
        for n in [Vec3::Z, -Vec3::Z, Vec3::X, -Vec3::Y] {
            let plane = Plane::new(n, 0.0);
            let (u, v) = default_axes_for_plane(&plane, 0.25);
            assert!(u.axis.dot(n).abs() < 1e-6, "{n:?}");
            assert!(v.axis.dot(n).abs() < 1e-6, "{n:?}");
        }
    }

    #[test]
    fn rotation_stays_in_the_face_plane() {
        let plane = Plane::new(Vec3::Z, 0.0);
        let (u, v) = default_axes_for_plane(&plane, 0.25);
        let (ru, rv) = rotate_axes(&plane, u, v, 45.0);
        assert!(ru.axis.dot(Vec3::Z).abs() < 1e-6);
        assert!(rv.axis.dot(Vec3::Z).abs() < 1e-6);
        assert!((ru.axis.length() - 1.0).abs() < 1e-5);
        // The floor basis has V pointing at -Y, so it is a -90 degree turn
        // about the normal that carries U onto V.
        let (ru, _) = rotate_axes(&plane, u, v, -90.0);
        assert!((ru.axis - v.axis).length() < 1e-5, "{:?} vs {:?}", ru.axis, v.axis);
    }
}
