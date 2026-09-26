// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! How a light falls off, shared by the lights Radiance bakes and the ones the
//! renderer draws live.
//!
//! One copy on purpose. A `light_dynamic` next to a baked `light` with the
//! same keys has to read the same, or a designer cannot swap one for the other
//! -- and two copies of a falloff curve drift the first time someone tunes
//! one. The shaders carry the same arithmetic in WGSL; this is its reference.

/// The distance, in units, at which a light's brightness reads as written.
///
/// Brightness is "how bright at a normal room distance", as Source's is: a
/// `_light` of `255 255 255 200` delivers 200 at a hundred inches with the
/// default quadratic falloff, whatever the other terms are.
pub const ATTN_REFERENCE: f32 = 100.0;

/// Constant, linear and quadratic falloff, as `_constant_attn`,
/// `_linear_attn` and `_quadratic_attn` spell them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attenuation {
    pub constant: f32,
    pub linear: f32,
    pub quadratic: f32,
}

impl Default for Attenuation {
    /// Quadratic: real light falls off with the square of distance, and
    /// anything else looks wrong in a room.
    fn default() -> Self {
        Attenuation {
            constant: 0.0,
            linear: 0.0,
            quadratic: 1.0,
        }
    }
}

impl Attenuation {
    /// The fraction of a light's brightness that reaches `dist` units away.
    /// The linear and quadratic terms are normalised at [`ATTN_REFERENCE`].
    pub fn falloff(&self, dist: f32) -> f32 {
        let d = dist.max(1.0);
        let denom = self.constant
            + self.linear * d / ATTN_REFERENCE
            + self.quadratic * (d * d) / (ATTN_REFERENCE * ATTN_REFERENCE);
        if denom <= 0.0 { 0.0 } else { 1.0 / denom }
    }

    /// Distance past which a light of peak brightness `peak` delivers less
    /// than `threshold`.
    ///
    /// Without a cutoff, every luxel fires a shadow ray at every light in the
    /// map and every pixel tests every light on screen.
    pub fn range(&self, peak: f32, threshold: f32) -> f32 {
        if peak <= 0.0 {
            return 0.0;
        }
        let (c, l, q) = (self.constant, self.linear, self.quadratic);
        if q > 0.0 {
            // peak * ref^2 / (q * d^2) = threshold
            (peak * ATTN_REFERENCE * ATTN_REFERENCE / (q * threshold)).sqrt()
        } else if l > 0.0 {
            peak * ATTN_REFERENCE / (l * threshold)
        } else if c > 0.0 {
            // No distance falloff at all; only the constant term limits it.
            if peak / c > threshold {
                f32::INFINITY
            } else {
                0.0
            }
        } else {
            f32::INFINITY
        }
    }
}

/// How much of a spot light reaches a direction `cos_angle` off its axis.
///
/// Full inside the inner cone, nothing outside the outer, and between them a
/// ramp raised to `exponent` -- Source's `_inner_cone`, `_cone` and
/// `_exponent`. Angles are the half-angles, in degrees.
pub fn spot_cone(cos_angle: f32, outer_deg: f32, inner_deg: f32, exponent: f32) -> f32 {
    let cos_outer = outer_deg.to_radians().cos();
    if cos_angle < cos_outer {
        return 0.0;
    }
    let cos_inner = inner_deg.to_radians().cos();
    if cos_angle >= cos_inner {
        return 1.0;
    }
    let t = (cos_angle - cos_outer) / (cos_inner - cos_outer).max(1e-6);
    t.powf(exponent.max(0.01))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_reads_as_written_at_the_reference_distance() {
        let a = Attenuation::default();
        assert!((a.falloff(ATTN_REFERENCE) - 1.0).abs() < 1e-6);
        assert!((a.falloff(2.0 * ATTN_REFERENCE) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_light_right_on_a_surface_does_not_divide_by_zero() {
        assert!(Attenuation::default().falloff(0.0).is_finite());
    }

    #[test]
    fn the_range_is_where_the_light_drops_below_the_threshold() {
        let a = Attenuation::default();
        let r = a.range(200.0, 1.0);
        assert!((200.0 * a.falloff(r) - 1.0).abs() < 1e-3);
        assert_eq!(
            Attenuation {
                constant: 1.0,
                linear: 0.0,
                quadratic: 0.0
            }
            .range(200.0, 1.0),
            f32::INFINITY
        );
    }

    #[test]
    fn a_spot_is_full_inside_nothing_outside_and_ramps_between() {
        let full = spot_cone(1.0, 45.0, 30.0, 1.0);
        let edge = spot_cone(40f32.to_radians().cos(), 45.0, 30.0, 1.0);
        let out = spot_cone(50f32.to_radians().cos(), 45.0, 30.0, 1.0);
        assert_eq!(full, 1.0);
        assert!(edge > 0.0 && edge < 1.0, "{edge}");
        assert_eq!(out, 0.0);
    }
}
