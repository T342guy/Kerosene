// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The reflectance model, on the CPU.
//!
//! The shaders do the real work; this is their reference. Each function here
//! is the same arithmetic as the function of the same name in `world.wgsl`
//! and `model.wgsl`, so the properties a BRDF has to have -- a distribution
//! that integrates to one, Fresnel that starts at F0 and ends at white, a
//! response that never exceeds what arrived -- can be tested without a GPU.
//! A change to one side that is not made to the other is a bug in whichever
//! was not changed.
//!
//! The model is the one Source 2, Unreal and Filament all converged on: GGX
//! for the distribution of microfacets, height-correlated Smith for the
//! shadowing between them, Schlick for Fresnel, and a metalness parameter
//! that blends the specular colour from 4% grey (every dielectric, near
//! enough) to the base colour (every metal).

use kerosene_math::Vec3;
use std::f32::consts::PI;

/// Reflectance at normal incidence for a dielectric. Water, plastic, stone
/// and wood all sit within a percent or two of it.
pub const DIELECTRIC_F0: f32 = 0.04;

/// Roughness is clamped above this before squaring. A perfectly smooth GGX
/// lobe is a delta function, which a single light sample turns into a
/// single-pixel sparkle that aliases as the camera moves.
pub const MIN_ROUGHNESS: f32 = 0.045;

/// Roughness as artists author it, to the alpha GGX takes. Squared, because
/// that makes the perceived blur change evenly along the slider.
pub fn alpha(roughness: f32) -> f32 {
    let r = roughness.clamp(MIN_ROUGHNESS, 1.0);
    r * r
}

/// The GGX (Trowbridge-Reitz) normal distribution.
pub fn d_ggx(n_dot_h: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    a2 / (PI * d * d)
}

/// Height-correlated Smith visibility, with the BRDF's `4 n.l n.v`
/// denominator folded in.
pub fn v_smith(n_dot_v: f32, n_dot_l: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let gv = n_dot_l * (n_dot_v * n_dot_v * (1.0 - a2) + a2).sqrt();
    let gl = n_dot_v * (n_dot_l * n_dot_l * (1.0 - a2) + a2).sqrt();
    0.5 / (gv + gl).max(1e-5)
}

/// Schlick's Fresnel.
pub fn f_schlick(f0: Vec3, v_dot_h: f32) -> Vec3 {
    let f = (1.0 - v_dot_h.clamp(0.0, 1.0)).powi(5);
    f0 + (Vec3::ONE - f0) * f
}

/// Specular colour at normal incidence.
pub fn f0(albedo: Vec3, metalness: f32) -> Vec3 {
    Vec3::splat(DIELECTRIC_F0).lerp(albedo, metalness.clamp(0.0, 1.0))
}

/// Specular reflected toward `v` from a light arriving along `l`, per unit of
/// irradiance on a surface facing the light. Multiply by the light's
/// irradiance to get radiance.
pub fn specular_ggx(n: Vec3, v: Vec3, l: Vec3, roughness: f32, f0: Vec3) -> Vec3 {
    let n_dot_l = n.dot(l);
    let n_dot_v = n.dot(v);
    if n_dot_l <= 0.0 || n_dot_v <= 0.0 {
        return Vec3::ZERO;
    }
    let h = (v + l).normalize_or_zero();
    let a = alpha(roughness);
    f_schlick(f0, v.dot(h)) * (d_ggx(n.dot(h).max(0.0), a) * v_smith(n_dot_v, n_dot_l, a) * n_dot_l)
}

/// What a surface reflects of an environment that is the same brightness in
/// every direction, as a fraction of that brightness.
///
/// Karis's analytic fit to the split-sum integral. This is how a lightmapped
/// surface gets a specular term without a light direction: the bake says how
/// much light arrived, not from where, and "evenly from everywhere" is the
/// assumption that does not invent a highlight nothing cast. A cubemap probe
/// replaces the even environment with a real one and keeps this factor.
pub fn env_brdf(f0: Vec3, roughness: f32, n_dot_v: f32) -> Vec3 {
    let (c0x, c0y, c0z, c0w) = (-1.0, -0.0275, -0.572, 0.022);
    let (c1x, c1y, c1z, c1w) = (1.0, 0.0425, 1.04, -0.04);
    let r = roughness.clamp(0.0, 1.0);
    let (rx, ry, rz, rw) = (r * c0x + c1x, r * c0y + c1y, r * c0z + c1z, r * c0w + c1w);
    let a004 = (rx * rx).min((-9.28 * n_dot_v.max(0.0)).exp2()) * rx + ry;
    let scale = -1.04 * a004 + rz;
    let bias = 1.04 * a004 + rw;
    f0 * scale + Vec3::splat(bias)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Integrate `f(theta)` over the hemisphere, with the `sin` Jacobian.
    fn hemisphere(f: impl Fn(f32) -> f32) -> f32 {
        let steps = 20_000;
        let d_theta = (PI / 2.0) / steps as f32;
        (0..steps)
            .map(|i| {
                let theta = (i as f32 + 0.5) * d_theta;
                f(theta) * theta.sin() * d_theta
            })
            .sum::<f32>()
            * 2.0
            * PI
    }

    #[test]
    fn the_distribution_projects_to_one_at_every_roughness() {
        // The defining property of a microfacet distribution: the projected
        // area of all the facets is the area of the surface.
        for roughness in [0.2, 0.5, 0.8, 1.0] {
            let a = alpha(roughness);
            let total = hemisphere(|theta| d_ggx(theta.cos(), a) * theta.cos());
            assert!((total - 1.0).abs() < 0.01, "roughness {roughness}: {total}");
        }
    }

    #[test]
    fn fresnel_is_f0_head_on_and_white_at_grazing() {
        let f0 = Vec3::splat(DIELECTRIC_F0);
        assert!((f_schlick(f0, 1.0) - f0).abs().max_element() < 1e-6);
        assert!((f_schlick(f0, 0.0) - Vec3::ONE).abs().max_element() < 1e-6);
    }

    #[test]
    fn metalness_moves_f0_from_grey_to_the_base_colour() {
        let gold = Vec3::new(1.0, 0.78, 0.34);
        assert_eq!(f0(gold, 0.0), Vec3::splat(DIELECTRIC_F0));
        assert!((f0(gold, 1.0) - gold).abs().max_element() < 1e-6);
    }

    #[test]
    fn a_surface_never_reflects_more_than_arrives() {
        // White-furnace style: integrate the specular response over every
        // light direction for a fixed view, with F0 of one. It may lose
        // energy (single-scattering GGX does), but must not make any.
        let n = Vec3::Z;
        for roughness in [0.1, 0.4, 0.7, 1.0] {
            for view_angle in [0.0f32, 0.6, 1.2] {
                let v = Vec3::new(view_angle.sin(), 0.0, view_angle.cos());
                let steps = 200;
                let mut total = 0.0;
                for i in 0..steps {
                    let theta = (i as f32 + 0.5) / steps as f32 * PI / 2.0;
                    for j in 0..steps {
                        let phi = (j as f32 + 0.5) / steps as f32 * 2.0 * PI;
                        let l = Vec3::new(
                            theta.sin() * phi.cos(),
                            theta.sin() * phi.sin(),
                            theta.cos(),
                        );
                        let d_omega =
                            theta.sin() * (PI / 2.0 / steps as f32) * (2.0 * PI / steps as f32);
                        total += specular_ggx(n, v, l, roughness, Vec3::ONE).x * d_omega;
                    }
                }
                assert!(
                    total <= 1.02,
                    "roughness {roughness}, view {view_angle}: reflected {total}"
                );
            }
        }
    }

    #[test]
    fn the_even_environment_fit_stays_between_nothing_and_everything() {
        for roughness in [0.0, 0.25, 0.5, 0.75, 1.0] {
            for n_dot_v in [0.01, 0.2, 0.5, 1.0] {
                let dielectric = env_brdf(Vec3::splat(DIELECTRIC_F0), roughness, n_dot_v).x;
                let mirror = env_brdf(Vec3::ONE, roughness, n_dot_v).x;
                assert!((0.0..=1.0).contains(&dielectric), "{dielectric}");
                assert!((0.0..=1.05).contains(&mirror), "{mirror}");
                assert!(dielectric <= mirror);
            }
        }
    }

    #[test]
    fn a_smooth_dielectric_reflects_more_at_grazing_than_head_on() {
        let f0 = Vec3::splat(DIELECTRIC_F0);
        let head_on = env_brdf(f0, 0.1, 1.0).x;
        let grazing = env_brdf(f0, 0.1, 0.05).x;
        assert!((head_on - DIELECTRIC_F0).abs() < 0.02, "{head_on}");
        assert!(grazing > head_on * 3.0, "{grazing} vs {head_on}");
    }
}
