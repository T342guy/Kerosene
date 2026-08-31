// SPDX-License-Identifier: MPL-2.0
//! Where a rigid body is and which way it faces.
//!
//! Brush models used to carry a position and nothing else, which was enough
//! while the only movers were doors that slide. It stops being enough the
//! moment anything turns: a rotating brush, a platform on a curved track, a
//! prop that has fallen over. All three need the same thing, and all three
//! need the *renderer* and the *collision* to agree about it, which is the
//! real reason this is a type rather than two fields passed around in pairs.
//!
//! Rigid only -- rotation and translation, never scale. That is what lets
//! [`Pose::to_local`] invert the rotation by transposing it, and what lets a
//! normal be rotated by the same matrix as a position.

use crate::{Aabb, Angles, Mat3, Mat4, Vec3};

/// A rigid placement: a displacement, a turn, and the point turned about.
///
/// The pivot is not decoration. Brush models are compiled in world
/// coordinates and their `origin` is a *displacement* from where they were
/// built -- zero for a door that has not opened. Turning such a model about
/// its origin would swing it around the world origin rather than spinning it
/// where it stands, so the point to turn about has to be stated separately.
/// Source solves the same problem with an `origin` brush; this is the same
/// answer without the extra brush.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub origin: Vec3,
    pub angles: Angles,
    /// The point, in the body's own space, that the angles turn about.
    pub pivot: Vec3,
}

impl Pose {
    /// Unmoved and unturned -- what the world model always is.
    pub const IDENTITY: Pose =
        Pose { origin: Vec3::ZERO, angles: Angles::ZERO, pivot: Vec3::ZERO };

    /// A pose turning about its own origin -- what a point entity wants.
    pub const fn new(origin: Vec3, angles: Angles) -> Pose {
        Pose { origin, angles, pivot: Vec3::ZERO }
    }

    /// A pose turning about a stated point -- what a brush model wants.
    pub const fn about(origin: Vec3, angles: Angles, pivot: Vec3) -> Pose {
        Pose { origin, angles, pivot }
    }

    /// A pose that only moves.
    pub const fn at(origin: Vec3) -> Pose {
        Pose { origin, angles: Angles::ZERO, pivot: Vec3::ZERO }
    }

    /// Whether this pose turns anything.
    ///
    /// Worth asking: the overwhelming majority of brush models never rotate,
    /// and the untuned path for them is a vector add rather than a matrix
    /// multiply and a transpose.
    pub fn is_rotated(&self) -> bool {
        self.angles != Angles::ZERO
    }

    /// The rotation taking local coordinates into world ones.
    pub fn rotation(&self) -> Mat3 {
        self.angles.to_mat3()
    }

    /// The full transform, for a shader that wants one matrix.
    pub fn to_mat4(&self) -> Mat4 {
        Mat4::from_translation(self.origin + self.pivot)
            * Mat4::from_mat3(self.rotation())
            * Mat4::from_translation(-self.pivot)
    }

    /// A point in the body's own space, placed into the world.
    pub fn to_world(&self, local: Vec3) -> Vec3 {
        if self.is_rotated() {
            self.rotation() * (local - self.pivot) + self.pivot + self.origin
        } else {
            local + self.origin
        }
    }

    /// A point in the world, expressed in the body's own space.
    ///
    /// The inverse of [`Pose::to_world`]. Because the rotation is orthonormal
    /// its inverse is its transpose, so this costs no more than the forward
    /// direction does.
    pub fn to_local(&self, world: Vec3) -> Vec3 {
        if self.is_rotated() {
            self.rotation().transpose() * (world - self.origin - self.pivot) + self.pivot
        } else {
            world - self.origin
        }
    }

    /// A direction in the body's space, turned into a world direction.
    ///
    /// Separate from [`Pose::to_world`] because a direction is not moved by
    /// the origin, and a plane normal that had the translation applied to it
    /// would point somewhere meaningless.
    pub fn direction_to_world(&self, local: Vec3) -> Vec3 {
        if self.is_rotated() { self.rotation() * local } else { local }
    }

    /// The world-space box enclosing a box given in the body's space.
    ///
    /// Every corner, not the two extremes: rotating a box's min and max gives
    /// two points that are no longer the extremes of anything.
    pub fn bounds_of(&self, local: Aabb) -> Aabb {
        if local.is_empty() {
            return local;
        }
        if !self.is_rotated() {
            return Aabb::new(local.min + self.origin, local.max + self.origin);
        }
        let mut out = Aabb::EMPTY;
        for i in 0..8 {
            let corner = Vec3::new(
                if i & 1 == 0 { local.min.x } else { local.max.x },
                if i & 2 == 0 { local.min.y } else { local.max.y },
                if i & 4 == 0 { local.min.z } else { local.max.z },
            );
            out.add_point(self.to_world(corner));
        }
        out
    }
}

#[cfg(test)]
mod tests;
