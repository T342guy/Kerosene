// SPDX-License-Identifier: MPL-2.0
use glam::{Mat3, Vec3};
use std::fmt;

/// Euler angles in degrees, ordered pitch/yaw/roll -- Source's `QAngle`.
///
/// * `pitch` rotates about the Y (left) axis and is **positive downward**.
/// * `yaw` rotates about the Z (up) axis, positive counter-clockwise seen
///   from above, so yaw 0 looks down `+X` and yaw 90 looks down `+Y`.
/// * `roll` rotates about the X (forward) axis, positive rolling right.
///
/// The inverted pitch is a Quake inheritance. It is preserved deliberately:
/// every `.keromap` angle key, every entity `angles` value and every recorded
/// view angle in the wild assumes it.
#[derive(Clone, Copy, PartialEq, Default)]
pub struct Angles {
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
}

impl Angles {
    pub const ZERO: Angles = Angles { pitch: 0.0, yaw: 0.0, roll: 0.0 };

    #[inline]
    pub const fn new(pitch: f32, yaw: f32, roll: f32) -> Self {
        Self { pitch, yaw, roll }
    }

    /// Build the forward/right/up basis for these angles.
    ///
    /// This is Source's `AngleVectors`, component for component. `right` is
    /// genuinely the vector pointing to the viewer's right, which in a
    /// +Y-is-left world means it is the *negated* Y-ish axis -- hence the
    /// sign pattern below, which looks wrong and is not.
    pub fn vectors(&self) -> Basis {
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        let (sy, cy) = self.yaw.to_radians().sin_cos();
        let (sr, cr) = self.roll.to_radians().sin_cos();

        Basis {
            forward: Vec3::new(cp * cy, cp * sy, -sp),
            right: Vec3::new(-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp),
            up: Vec3::new(cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp),
        }
    }

    /// Just the forward vector; cheaper than building the whole basis.
    #[inline]
    pub fn forward(&self) -> Vec3 {
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        let (sy, cy) = self.yaw.to_radians().sin_cos();
        Vec3::new(cp * cy, cp * sy, -sp)
    }

    /// Angles that look along `dir`. Roll is always zero -- a direction
    /// vector cannot express roll.
    pub fn from_direction(dir: Vec3) -> Self {
        if dir.x == 0.0 && dir.y == 0.0 {
            // Straight up or straight down; yaw is arbitrary, pick 0.
            Self::new(if dir.z > 0.0 { -90.0 } else { 90.0 }, 0.0, 0.0)
        } else {
            let yaw = dir.y.atan2(dir.x).to_degrees();
            let pitch = (-dir.z).atan2(dir.truncate().length()).to_degrees();
            Self::new(pitch, yaw, 0.0)
        }
    }

    /// Rotation matrix mapping local space into world space.
    pub fn to_mat3(&self) -> Mat3 {
        let b = self.vectors();
        // Columns are the images of local +X/+Y/+Z. Local +Y is *left*, which
        // is -right, matching the Z-up left-handed-looking convention.
        Mat3::from_cols(b.forward, -b.right, b.up)
    }

    /// Wrap every component into `[-180, 180)`.
    pub fn normalized(self) -> Self {
        Self::new(wrap180(self.pitch), wrap180(self.yaw), wrap180(self.roll))
    }

    /// Clamp pitch to the range a player's neck allows and wrap the rest.
    ///
    /// Source clamps to +/-89 rather than 90 so that the view basis never
    /// becomes degenerate at the poles.
    pub fn clamped_view(self) -> Self {
        Self::new(self.pitch.clamp(-89.0, 89.0), wrap180(self.yaw), 0.0)
    }
}

/// An orthonormal basis derived from [`Angles`].
#[derive(Clone, Copy, Debug)]
pub struct Basis {
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
}

/// Wrap an angle in degrees into `[-180, 180)`.
#[inline]
pub fn wrap180(a: f32) -> f32 {
    let mut a = a % 360.0;
    if a >= 180.0 { a -= 360.0; }
    if a < -180.0 { a += 360.0; }
    a
}

/// Shortest signed difference `a - b` in degrees.
#[inline]
pub fn angle_diff(a: f32, b: f32) -> f32 { wrap180(a - b) }

impl fmt::Debug for Angles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Angles({} {} {})", self.pitch, self.yaw, self.roll)
    }
}

impl fmt::Display for Angles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.pitch, self.yaw, self.roll)
    }
}

impl std::ops::Add for Angles {
    type Output = Angles;
    fn add(self, o: Angles) -> Angles {
        Angles::new(self.pitch + o.pitch, self.yaw + o.yaw, self.roll + o.roll)
    }
}

impl std::ops::Sub for Angles {
    type Output = Angles;
    fn sub(self, o: Angles) -> Angles {
        Angles::new(self.pitch - o.pitch, self.yaw - o.yaw, self.roll - o.roll)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool { (a - b).length() < 1e-5 }

    #[test]
    fn zero_angles_look_down_positive_x() {
        let b = Angles::ZERO.vectors();
        assert!(close(b.forward, Vec3::X), "{:?}", b.forward);
        assert!(close(b.up, Vec3::Z), "{:?}", b.up);
        // +Y is left, so right is -Y.
        assert!(close(b.right, -Vec3::Y), "{:?}", b.right);
    }

    #[test]
    fn yaw_90_looks_down_positive_y() {
        let b = Angles::new(0.0, 90.0, 0.0).vectors();
        assert!(close(b.forward, Vec3::Y), "{:?}", b.forward);
    }

    #[test]
    fn positive_pitch_looks_down() {
        let f = Angles::new(90.0, 0.0, 0.0).forward();
        assert!(close(f, -Vec3::Z), "{f:?}");
    }

    #[test]
    fn basis_is_orthonormal_for_arbitrary_angles() {
        for &(p, y, r) in &[(13.0, 47.0, 21.0), (-80.0, 200.0, -33.0), (0.0, 0.0, 90.0)] {
            let b = Angles::new(p, y, r).vectors();
            for v in [b.forward, b.right, b.up] {
                assert!((v.length() - 1.0).abs() < 1e-5);
            }
            assert!(b.forward.dot(b.right).abs() < 1e-5);
            assert!(b.forward.dot(b.up).abs() < 1e-5);
            assert!(b.right.dot(b.up).abs() < 1e-5);
        }
    }

    #[test]
    fn from_direction_round_trips() {
        for d in [Vec3::X, Vec3::Y, Vec3::new(1.0, 2.0, -3.0).normalize()] {
            let a = Angles::from_direction(d);
            assert!(close(a.forward(), d), "{d:?} -> {a:?} -> {:?}", a.forward());
        }
    }

    #[test]
    fn wrap_keeps_range() {
        assert_eq!(wrap180(190.0), -170.0);
        assert_eq!(wrap180(-190.0), 170.0);
        assert_eq!(wrap180(180.0), -180.0);
        assert_eq!(angle_diff(179.0, -179.0), -2.0);
    }
}
