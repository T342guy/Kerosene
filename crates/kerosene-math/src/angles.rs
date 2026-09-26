// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
use glam::{Mat3, Quat, Vec3};
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
    pub const ZERO: Angles = Angles {
        pitch: 0.0,
        yaw: 0.0,
        roll: 0.0,
    };

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

    /// Euler angles from a rotation matrix, the inverse of [`Self::to_mat3`].
    ///
    /// The decomposition matches Source's `MatrixAngles`: pitch from the
    /// forward vector's fall, yaw from its heading, roll from how far the up
    /// vector has rolled about the forward axis. At pitch +/-90 the yaw/roll
    /// split is degenerate (it always is with Euler angles) and yaw wins.
    pub fn from_mat3(m: &Mat3) -> Self {
        // Columns: col0 = forward, col1 = -right, col2 = up.
        let forward = m.col(0);
        let right_neg = m.col(1); // -right, so right = -right_neg
        let up = m.col(2);

        let pitch = (-forward.z).atan2(forward.truncate().length()).to_degrees();
        let yaw = forward.y.atan2(forward.x).to_degrees();
        // roll = atan2(-right.z, up.z), and right = -right_neg, so the two
        // negations cancel: this is atan2(right_neg.z, up.z).
        let roll = right_neg.z.atan2(up.z).to_degrees();
        Self::new(pitch, yaw, roll)
    }

    /// Euler angles from a rotation quaternion.
    pub fn from_quat(q: Quat) -> Self {
        Self::from_mat3(&Mat3::from_quat(q))
    }

    /// The rotation quaternion for these angles, the inverse of [`Self::from_quat`].
    pub fn to_quat(&self) -> Quat {
        Quat::from_mat3(&self.to_mat3())
    }

    /// Spherical interpolation between two orientations, `t` in `0..=1`.
    ///
    /// A naive per-component lerp of pitch/yaw/roll is wrong the moment more
    /// than one axis is moving at once -- yaw races ahead of a pitch that has
    /// further to swing, and the object visibly wobbles off the straight
    /// path. Going through quaternions gets the straight path instead.
    pub fn slerp(self, other: Angles, t: f32) -> Angles {
        if self == other {
            return self;
        }
        Angles::from_quat(self.to_quat().slerp(other.to_quat(), t.clamp(0.0, 1.0)))
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
    if a >= 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}

/// Shortest signed difference `a - b` in degrees.
#[inline]
pub fn angle_diff(a: f32, b: f32) -> f32 {
    wrap180(a - b)
}

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

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

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
    fn from_mat3_inverts_to_mat3() {
        for &(p, y, r) in &[
            (0.0, 0.0, 0.0),
            (13.0, 47.0, 21.0),
            (-80.0, 200.0, -33.0),
            (0.0, 90.0, 0.0),
        ] {
            let a = Angles::new(p, y, r);
            let back = Angles::from_mat3(&a.to_mat3());
            // Roll and yaw may wrap, but every component must land back within
            // a degree.
            assert!(
                (angle_diff(back.pitch, p)).abs() < 0.01,
                "{a:?} -> {back:?}"
            );
            assert!((angle_diff(back.yaw, y)).abs() < 0.01, "{a:?} -> {back:?}");
            assert!((angle_diff(back.roll, r)).abs() < 0.01, "{a:?} -> {back:?}");
        }
    }

    #[test]
    fn from_quat_matches_the_matrix_path() {
        for &(p, y, r) in &[(5.0, 20.0, -10.0), (40.0, -120.0, 80.0)] {
            let a = Angles::new(p, y, r);
            let q = Quat::from_mat3(&a.to_mat3());
            let back = Angles::from_quat(q);
            assert!(
                (angle_diff(back.pitch, p)).abs() < 0.01,
                "{a:?} -> {back:?}"
            );
            assert!((angle_diff(back.yaw, y)).abs() < 0.01, "{a:?} -> {back:?}");
            assert!((angle_diff(back.roll, r)).abs() < 0.01, "{a:?} -> {back:?}");
        }
    }

    #[test]
    fn to_quat_matches_from_quat() {
        for &(p, y, r) in &[(5.0, 20.0, -10.0), (40.0, -120.0, 80.0)] {
            let a = Angles::new(p, y, r);
            let back = Angles::from_quat(a.to_quat());
            assert!(
                (angle_diff(back.pitch, p)).abs() < 0.01,
                "{a:?} -> {back:?}"
            );
            assert!((angle_diff(back.yaw, y)).abs() < 0.01, "{a:?} -> {back:?}");
            assert!((angle_diff(back.roll, r)).abs() < 0.01, "{a:?} -> {back:?}");
        }
    }

    #[test]
    fn slerp_returns_exact_endpoints() {
        let a = Angles::new(10.0, 20.0, -30.0);
        let b = Angles::new(-40.0, 170.0, 5.0);
        assert!(close(a.slerp(b, 0.0).forward(), a.forward()));
        assert!(close(a.slerp(b, 1.0).forward(), b.forward()));
    }

    #[test]
    fn slerp_halfway_yaw_turn_is_the_bisector() {
        let a = Angles::new(0.0, 0.0, 0.0);
        let b = Angles::new(0.0, 90.0, 0.0);
        let mid = a.slerp(b, 0.5);
        assert!((angle_diff(mid.yaw, 45.0)).abs() < 0.01, "{mid:?}");
    }

    #[test]
    fn wrap_keeps_range() {
        assert_eq!(wrap180(190.0), -170.0);
        assert_eq!(wrap180(-190.0), 170.0);
        assert_eq!(wrap180(180.0), -180.0);
        assert_eq!(angle_diff(179.0, -179.0), -2.0);
    }
}
