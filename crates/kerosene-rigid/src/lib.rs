// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Rigid-body dynamics for Kerosene, backed by
//! [box3d-rust](https://crates.io/crates/box3d-rust).
//!
//! Kerosene's player movement is a faithful reimplementation of Source's
//! `gamemovement` in [`kerosene_physics`], and it stays that way: the way a
//! Source game *feels* is that code, and swapping it for a general-purpose
//! engine would change the game.
//!
//! What a general-purpose engine is for is everything the player is *not*:
//! physics props. A crate that tumbles down stairs, a barrel that rolls when
//! shot, a door panel that breaks off and falls -- those are rigid bodies, and
//! this crate provides them through Box3D, Erin Catto's 3D physics engine, via
//! its pure-Rust port.
//!
//! Box3D is unit- and orientation-agnostic: its solver tolerances, gravity and
//! density defaults are all derived from a single "length units per metre"
//! scale, and there is no fixed up-axis. [`init`] sets that scale to inches,
//! and after that every vector this crate accepts or returns is a plain
//! Kerosene vector -- inches, Z-up, no conversion anywhere.

use box3d_rust as b3;
use kerosene_math::{Quat, Vec3};

/// Kerosene gravity, units per second squared (Source's default: about 20 m/s²,
/// a little stronger than Earth).
pub const GRAVITY: Vec3 = Vec3::new(0.0, 0.0, -800.0);

/// One metre in Kerosene units (inches).
pub const INCHES_PER_METRE: f32 = 39.370_078_74;

/// Largest hull Box3D will build. A BSP brush has at most a handful of faces,
/// so this is far more than enough.
const MAX_HULL_VERTICES: i32 = 255;

/// Configure Box3D for Kerosene once per process.
///
/// Must run before any Box3D default definition is built, because those
/// defaults (`default_world_def`, `default_shape_def`, ...) bake the length
/// scale into their gravity, density and tolerance values. [`RigidWorld::new`]
/// calls this itself, so the engine normally never has to; it is exposed for
/// code that builds Box3D defaults directly.
pub fn init() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        b3::core::set_length_units_per_meter(INCHES_PER_METRE);
    });
}

fn to_b3(v: Vec3) -> b3::Vec3 {
    b3::Vec3 { x: v.x, y: v.y, z: v.z }
}

fn from_b3(v: b3::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn to_b3_quat(q: Quat) -> b3::Quat {
    b3::Quat { v: to_b3(Vec3::new(q.x, q.y, q.z)), s: q.w }
}

fn from_b3_quat(q: b3::Quat) -> Quat {
    Quat::from_xyzw(q.v.x, q.v.y, q.v.z, q.s)
}

/// A rigid body in a [`RigidWorld`]. Copyable handle; null until created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Body(pub(crate) b3::BodyId);

/// A simulated rigid-body world.
///
/// Owns the Box3D simulation. Bodies are created and destroyed through it, and
/// their transforms are read back in Kerosene space after [`step`](Self::step).
pub struct RigidWorld {
    world: b3::world::World,
    bodies: Vec<Body>,
}

impl RigidWorld {
    /// Create an empty world with Kerosene gravity (Z-up, 800 units/s² down).
    pub fn new() -> RigidWorld {
        init();
        let mut def = b3::types::default_world_def();
        def.gravity = to_b3(GRAVITY);
        RigidWorld { world: b3::world::World::new(&def), bodies: Vec::new() }
    }

    /// Advance the simulation by `dt` seconds.
    ///
    /// The engine ticks at a fixed 64 Hz, so `dt` is normally `1.0 / 64.0`.
    /// Box3D sub-steps internally for stability; one sub-step per tick is
    /// enough at that rate, and calling [`step`](Self::step) more often with a
    /// smaller `dt` is always safe.
    pub fn step(&mut self, dt: f32) {
        self.world.step(dt, 1);
    }

    /// Add a static box (immovable level geometry).
    pub fn add_static_box(&mut self, half_extent: Vec3, position: Vec3, rotation: Quat) -> Body {
        self.add_box(half_extent, position, rotation, b3::types::BodyType::Static)
    }

    /// Add a dynamic box (a physics prop).
    pub fn add_dynamic_box(&mut self, half_extent: Vec3, position: Vec3, rotation: Quat) -> Body {
        self.add_box(half_extent, position, rotation, b3::types::BodyType::Dynamic)
    }

    fn add_box(
        &mut self,
        half_extent: Vec3,
        position: Vec3,
        rotation: Quat,
        body_type: b3::types::BodyType,
    ) -> Body {
        // A half-extent is unsigned; clamp to a small positive minimum so a
        // zero-thickness "box" (a face someone selected) still has volume.
        let min = 0.5;
        let h = Vec3::new(
            half_extent.x.abs().max(min),
            half_extent.y.abs().max(min),
            half_extent.z.abs().max(min),
        );
        let hull = b3::hull::make_box_hull(h.x, h.y, h.z);
        self.add_hull_shape(&hull.base, position, rotation, body_type)
    }

    /// Add a static convex hull through the given points (level geometry).
    ///
    /// Returns `None` when the points do not form a valid hull (fewer than four
    /// points, or all coplanar); callers extracting hulls from BSP brushes
    /// should skip such degenerate brushes.
    pub fn add_static_hull(
        &mut self,
        points: &[Vec3],
        position: Vec3,
        rotation: Quat,
    ) -> Option<Body> {
        self.add_hull(points, position, rotation, b3::types::BodyType::Static)
    }

    /// Add a dynamic convex hull through the given points (a physics prop).
    pub fn add_dynamic_hull(
        &mut self,
        points: &[Vec3],
        position: Vec3,
        rotation: Quat,
    ) -> Option<Body> {
        self.add_hull(points, position, rotation, b3::types::BodyType::Dynamic)
    }

    fn add_hull(
        &mut self,
        points: &[Vec3],
        position: Vec3,
        rotation: Quat,
        body_type: b3::types::BodyType,
    ) -> Option<Body> {
        let b3_points: Vec<b3::Vec3> = points.iter().map(|&p| to_b3(p)).collect();
        let hull = b3::hull::create_hull(&b3_points, MAX_HULL_VERTICES)?;
        Some(self.add_hull_shape(&hull, position, rotation, body_type))
    }

    fn add_hull_shape(
        &mut self,
        hull: &b3::hull::HullData,
        position: Vec3,
        rotation: Quat,
        body_type: b3::types::BodyType,
    ) -> Body {
        let mut body_def = b3::types::default_body_def();
        body_def.type_ = body_type;
        body_def.position = to_b3(position);
        body_def.rotation = to_b3_quat(rotation);
        let id = b3::body::create_body(&mut self.world, &body_def);

        let shape_def = b3::types::default_shape_def();
        b3::shape::create_hull_shape(&mut self.world, id, &shape_def, hull);

        let body = Body(id);
        self.bodies.push(body);
        body
    }

    /// Remove and destroy a body.
    pub fn destroy_body(&mut self, body: Body) {
        b3::body::destroy_body(&mut self.world, body.0);
        self.bodies.retain(|&b| b != body);
    }

    /// The current transform of a body, in Kerosene space.
    pub fn body_transform(&self, body: Body) -> (Vec3, Quat) {
        let t = b3::body::body_get_transform(&self.world, body.0);
        (from_b3(t.p), from_b3_quat(t.q))
    }

    /// Teleport a body (used when an entity is moved by game logic rather than
    /// the simulation).
    pub fn set_body_transform(&mut self, body: Body, position: Vec3, rotation: Quat) {
        b3::body::body_set_transform(&mut self.world, body.0, to_b3(position), to_b3_quat(rotation));
    }

    /// A body's linear velocity, in Kerosene units per second.
    pub fn linear_velocity(&self, body: Body) -> Vec3 {
        from_b3(b3::body::body_get_linear_velocity(&self.world, body.0))
    }

    /// Set a body's linear velocity, in Kerosene units per second.
    pub fn set_linear_velocity(&mut self, body: Body, velocity: Vec3) {
        b3::body::body_set_linear_velocity(&mut self.world, body.0, to_b3(velocity));
    }

    /// Set a body's angular velocity (axis-and-angle), in radians per second.
    pub fn set_angular_velocity(&mut self, body: Body, velocity: Vec3) {
        b3::body::body_set_angular_velocity(&mut self.world, body.0, to_b3(velocity));
    }

    /// Whether a body is awake (moving or recently disturbed).
    pub fn is_awake(&self, body: Body) -> bool {
        b3::body::body_is_awake(&self.world, body.0)
    }

    /// Apply an instantaneous impulse to a body's centre of mass (a shot, an
    /// explosion, a kick).
    pub fn apply_impulse(&mut self, body: Body, impulse: Vec3) {
        b3::body::body_apply_linear_impulse_to_center(&mut self.world, body.0, to_b3(impulse), true);
    }

    /// Number of bodies currently in the world.
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }
}

impl Default for RigidWorld {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crate_half_extent() -> Vec3 {
        Vec3::new(8.0, 8.0, 8.0)
    }

    #[test]
    fn conversions_round_trip() {
        let q = Quat::from_axis_angle(Vec3::new(1.0, -2.0, 3.0).normalize(), 0.9);
        let back = from_b3_quat(to_b3_quat(q));
        let same = (q - back).length() < 1e-5 || (q + back).length() < 1e-5;
        assert!(same, "{q:?} -> {back:?}");

        let v = Vec3::new(12.5, -30.0, 7.25);
        assert_eq!(from_b3(to_b3(v)), v);
    }

    #[test]
    fn a_dropped_crate_comes_to_rest_on_the_floor() {
        let mut world = RigidWorld::new();
        // Floor: a wide, thin slab whose top face is at z = 0.
        world.add_static_box(
            Vec3::new(200.0, 200.0, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
            Quat::IDENTITY,
        );

        let body = world.add_dynamic_box(crate_half_extent(), Vec3::new(0.0, 0.0, 96.0), Quat::IDENTITY);

        let dt = 1.0 / 64.0;
        for _ in 0..64 * 8 {
            world.step(dt);
        }

        let (pos, _) = world.body_transform(body);
        // Resting: the crate centre ends one half-extent above the floor. A
        // little horizontal drift is normal Box3D settling; the point is that
        // it stopped at the floor, not that it landed perfectly on the spot.
        assert!(pos.z > 7.0 && pos.z < 9.0, "crate came to rest at z={}", pos.z);
        assert!(pos.x.abs() < 5.0 && pos.y.abs() < 5.0, "crate slid away to {pos:?}");
    }

    #[test]
    fn a_body_with_nothing_under_it_falls() {
        let mut world = RigidWorld::new();
        let body = world.add_dynamic_box(crate_half_extent(), Vec3::new(0.0, 0.0, 1000.0), Quat::IDENTITY);

        let dt = 1.0 / 64.0;
        for _ in 0..64 {
            world.step(dt);
        }
        let (pos, _) = world.body_transform(body);
        assert!(pos.z < 990.0, "crate should have fallen from 1000 to {}", pos.z);
    }

    #[test]
    fn gravity_pulls_straight_down_in_kerosene_z() {
        let mut world = RigidWorld::new();
        let body = world.add_dynamic_box(crate_half_extent(), Vec3::ZERO, Quat::IDENTITY);
        world.step(1.0 / 64.0);
        let v = world.linear_velocity(body);
        assert!(v.z < -8.0, "velocity {v:?} should point down");
        assert!(v.x.abs() < 0.1 && v.y.abs() < 0.1, "velocity {v:?} should have no sideways part");
    }

    #[test]
    fn a_convex_hull_prop_can_be_created_and_simulated() {
        let mut world = RigidWorld::new();
        world.add_static_box(Vec3::new(100.0, 100.0, 1.0), Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);

        // A simple tetrahedron: four non-coplanar points.
        let points = [
            Vec3::new(0.0, 0.0, 16.0),
            Vec3::new(16.0, 0.0, 0.0),
            Vec3::new(0.0, 16.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
        ];
        let body = world
            .add_dynamic_hull(&points, Vec3::new(0.0, 0.0, 96.0), Quat::IDENTITY)
            .expect("a tetrahedron is a valid hull");

        let dt = 1.0 / 64.0;
        for _ in 0..64 * 8 {
            world.step(dt);
        }
        let (pos, _) = world.body_transform(body);
        assert!(pos.z < 90.0, "tetrahedron should have fallen, now at z={}", pos.z);
    }

    #[test]
    fn coplanar_points_are_rejected_as_a_hull() {
        let mut world = RigidWorld::new();
        // Four points all in the z = 0 plane: no volume.
        let points = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
        ];
        assert!(world.add_static_hull(&points, Vec3::ZERO, Quat::IDENTITY).is_none());
        assert_eq!(world.body_count(), 0);
    }

    #[test]
    fn destroying_a_body_removes_it() {
        let mut world = RigidWorld::new();
        let body = world.add_dynamic_box(crate_half_extent(), Vec3::ZERO, Quat::IDENTITY);
        assert_eq!(world.body_count(), 1);
        world.destroy_body(body);
        assert_eq!(world.body_count(), 0);
    }

    #[test]
    fn teleporting_moves_a_body() {
        let mut world = RigidWorld::new();
        let body = world.add_dynamic_box(crate_half_extent(), Vec3::ZERO, Quat::IDENTITY);
        world.set_body_transform(body, Vec3::new(10.0, -20.0, 30.0), Quat::IDENTITY);
        let (pos, _) = world.body_transform(body);
        assert!(pos.abs_diff_eq(Vec3::new(10.0, -20.0, 30.0), 1e-3));
    }
}
