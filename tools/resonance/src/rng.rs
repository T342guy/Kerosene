// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! A small, fast, seedable random source: PCG32.
//!
//! Seeded from the leaf index, so a map compiles to the same bytes every
//! time and a room that rings differently after a rebuild does so because
//! the map changed, not because the dice did. Written out rather than pulled
//! in because it is twelve lines and the point is that they never change.

use kerosene_math::Vec3;
use std::f32::consts::TAU;

#[derive(Clone, Debug)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    pub fn new(seed: u64, stream: u64) -> Pcg32 {
        let mut rng = Pcg32 {
            state: 0,
            inc: (stream << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// A direction spread evenly over the sphere.
    pub fn unit_sphere(&mut self) -> Vec3 {
        let z = 1.0 - 2.0 * self.next_f32();
        let r = (1.0 - z * z).max(0.0).sqrt();
        let phi = TAU * self.next_f32();
        Vec3::new(r * phi.cos(), r * phi.sin(), z)
    }

    /// A direction leaving a surface the way a matte one reflects: weighted
    /// toward the normal by the cosine, which is what a Lambertian surface
    /// does and what makes the ray count come out proportional to energy.
    pub fn cosine_hemisphere(&mut self, normal: Vec3) -> Vec3 {
        let (t, b) = normal.any_orthonormal_pair();
        let u = self.next_f32();
        let phi = TAU * self.next_f32();
        let r = u.sqrt();
        let x = r * phi.cos();
        let y = r * phi.sin();
        let z = (1.0 - u).max(0.0).sqrt();
        (t * x + b * y + normal * z).normalize_or_zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_numbers() {
        let mut a = Pcg32::new(42, 7);
        let mut b = Pcg32::new(42, 7);
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
        let mut c = Pcg32::new(43, 7);
        assert_ne!(a.next_u32(), c.next_u32());
    }

    #[test]
    fn floats_stay_in_range_and_directions_stay_unit() {
        let mut rng = Pcg32::new(1, 1);
        let mut sum = Vec3::ZERO;
        for _ in 0..10_000 {
            let f = rng.next_f32();
            assert!((0.0..1.0).contains(&f));
            let d = rng.unit_sphere();
            assert!((d.length() - 1.0).abs() < 1e-4);
            sum += d;
            let h = rng.cosine_hemisphere(Vec3::Z);
            assert!(h.z >= 0.0 && (h.length() - 1.0).abs() < 1e-4);
        }
        // Uniform over the sphere means the mean direction is near zero.
        assert!(sum.length() / 10_000.0 < 0.03, "{sum}");
    }
}
