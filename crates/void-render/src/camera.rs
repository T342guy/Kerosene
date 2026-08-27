//! View and projection, and the frustum that falls out of them.
//!
//! The awkward part is that VoidEngine's world is Z-up with +X forward, while
//! every graphics API expects a view space with +X right, +Y up and -Z
//! forward. The conversion happens here, once, rather than being smeared
//! through the renderer.
//!
//! Field of view is specified *horizontally*, as Source's `fov` convar is,
//! because that is what a player is actually judging. It is converted to the
//! vertical angle the projection matrix wants, taking aspect ratio into
//! account -- so a wider monitor shows more at the sides rather than
//! stretching what a 4:3 one shows.

use void_math::{Angles, Mat4, Plane, Vec3, Vec4};

/// The aspect ratio a horizontal FOV is quoted at.
///
/// Source's convention: `fov 90` means 90 degrees horizontally on a 4:3
/// display. Widescreen then widens the view rather than cropping it.
pub const REFERENCE_ASPECT: f32 = 4.0 / 3.0;

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: Vec3,
    pub angles: Angles,
    /// Horizontal field of view in degrees, at [`REFERENCE_ASPECT`].
    pub fov: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            position: Vec3::ZERO,
            angles: Angles::ZERO,
            fov: 90.0,
            aspect: 16.0 / 9.0,
            // 3 inches: closer than a player's hull can ever bring a surface,
            // and far enough out to keep depth precision usable.
            near: 3.0,
            far: 32_768.0,
        }
    }
}

impl Camera {
    /// World-to-view transform.
    pub fn view_matrix(&self) -> Mat4 {
        let basis = self.angles.vectors();
        Mat4::look_to_rh(self.position, basis.forward, basis.up)
    }

    /// Vertical field of view in radians, derived from the horizontal one.
    pub fn fov_y(&self) -> f32 { vertical_fov(self.fov, self.aspect) }

    /// View-to-clip transform, with depth in `0..1` as wgpu expects.
    pub fn projection_matrix(&self) -> Mat4 {
        Mat4::perspective_rh(vertical_fov(self.fov, self.aspect), self.aspect, self.near, self.far)
    }

    pub fn view_projection(&self) -> Mat4 {
        self.projection_matrix() * self.view_matrix()
    }

    pub fn forward(&self) -> Vec3 { self.angles.forward() }

    /// The six planes bounding what this camera can see, facing inward.
    pub fn frustum(&self) -> Frustum {
        Frustum::from_view_projection(self.view_projection())
    }
}

/// Vertical field of view in radians for a horizontal FOV quoted at
/// [`REFERENCE_ASPECT`].
///
/// Widening the display widens the view: the vertical angle stays what it
/// would be at 4:3, so a 21:9 monitor genuinely shows more to the sides.
pub fn vertical_fov(fov_x_degrees: f32, _aspect: f32) -> f32 {
    let half_x = (fov_x_degrees.clamp(1.0, 179.0).to_radians() * 0.5).tan();
    2.0 * (half_x / REFERENCE_ASPECT).atan()
}

/// Six inward-facing planes bounding the visible volume.
#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    pub planes: [Plane; 6],
}

impl Frustum {
    /// Extract the planes from a view-projection matrix.
    ///
    /// The Gribb-Hartmann method: each clip-space bound `-w <= x <= w` becomes
    /// a world-space plane by adding or subtracting the matrix's `w` row from
    /// the corresponding row. Cheaper and less error-prone than unprojecting
    /// the eight corners and building planes from them.
    pub fn from_view_projection(vp: Mat4) -> Frustum {
        // glam is column-major, so a "row" is one component across the columns.
        let row = |i: usize| Vec4::new(vp.x_axis[i], vp.y_axis[i], vp.z_axis[i], vp.w_axis[i]);
        let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));

        let make = |v: Vec4| -> Plane {
            let normal = Vec3::new(v.x, v.y, v.z);
            let length = normal.length();
            if length < 1e-9 {
                return Plane::new(Vec3::Z, 0.0);
            }
            // Gribb-Hartmann gives `a . p + d >= 0` for inside. `Plane` reads
            // the other way round -- inside is a non-positive distance, so
            // that `box_distances` rejecting on `lo > 0` means "entirely
            // outside" -- so the normal is negated on the way in. Getting this
            // backwards culls everything you can see and draws everything you
            // cannot.
            Plane::new(-normal / length, v.w / length)
        };

        Frustum {
            planes: [
                make(r3 + r0), // left
                make(r3 - r0), // right
                make(r3 + r1), // bottom
                make(r3 - r1), // top
                make(r2),      // near (depth range starts at 0)
                make(r3 - r2), // far
            ],
        }
    }

    /// Whether a box is at least partly inside.
    ///
    /// Conservative: a box straddling a plane counts as visible. Being wrong
    /// in this direction costs a few wasted draws; being wrong the other way
    /// makes geometry vanish.
    pub fn intersects_box(&self, min: Vec3, max: Vec3) -> bool {
        let center = (min + max) * 0.5;
        let half = (max - min) * 0.5;
        for plane in &self.planes {
            let (lo, _) = plane.box_distances(center, half);
            if lo > 0.0 { return false; }
        }
        true
    }

    pub fn contains_point(&self, p: Vec3) -> bool {
        self.planes.iter().all(|plane| plane.distance_to(p) <= 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera_at(position: Vec3, yaw: f32) -> Camera {
        Camera { position, angles: Angles::new(0.0, yaw, 0.0), ..Default::default() }
    }

    #[test]
    fn the_view_matrix_puts_the_camera_at_the_origin() {
        let cam = camera_at(Vec3::new(100.0, 200.0, 50.0), 0.0);
        let view = cam.view_matrix();
        let seen = view.transform_point3(cam.position);
        assert!(seen.length() < 1e-3, "the camera should see itself at the origin: {seen:?}");
    }

    #[test]
    fn looking_down_positive_x_puts_that_direction_down_negative_z_in_view_space() {
        // The whole point of the conversion: world +X forward becomes view -Z.
        let cam = camera_at(Vec3::ZERO, 0.0);
        let view = cam.view_matrix();
        let ahead = view.transform_point3(Vec3::new(100.0, 0.0, 0.0));
        assert!(ahead.z < -50.0, "forward should be down -Z, got {ahead:?}");
        assert!(ahead.x.abs() < 1e-3 && ahead.y.abs() < 1e-3);
    }

    #[test]
    fn world_up_becomes_view_up() {
        let cam = camera_at(Vec3::ZERO, 0.0);
        let above = cam.view_matrix().transform_point3(Vec3::new(100.0, 0.0, 50.0));
        assert!(above.y > 0.0, "up should stay up, got {above:?}");
    }

    #[test]
    fn world_left_becomes_view_left() {
        let cam = camera_at(Vec3::ZERO, 0.0);
        // +Y is left in world space, so it should land at negative view X.
        let left = cam.view_matrix().transform_point3(Vec3::new(100.0, 50.0, 0.0));
        assert!(left.x < 0.0, "world +Y is to the left, got {left:?}");
    }

    #[test]
    fn a_point_ahead_projects_inside_the_screen() {
        let cam = camera_at(Vec3::ZERO, 0.0);
        let clip = cam.view_projection() * Vec3::new(500.0, 0.0, 0.0).extend(1.0);
        assert!(clip.w > 0.0, "should be in front of the camera");
        let ndc = clip.truncate() / clip.w;
        assert!(ndc.x.abs() < 0.01 && ndc.y.abs() < 0.01, "should be centred: {ndc:?}");
        assert!((0.0..=1.0).contains(&ndc.z), "depth must land in 0..1 for wgpu: {}", ndc.z);
    }

    #[test]
    fn a_point_behind_the_camera_projects_behind() {
        let cam = camera_at(Vec3::ZERO, 0.0);
        let clip = cam.view_projection() * Vec3::new(-500.0, 0.0, 0.0).extend(1.0);
        assert!(clip.w < 0.0, "behind the camera means negative w");
    }

    #[test]
    fn a_wider_display_shows_more_rather_than_stretching() {
        // The vertical angle should not change with aspect ratio.
        let narrow = vertical_fov(90.0, 4.0 / 3.0);
        let wide = vertical_fov(90.0, 21.0 / 9.0);
        assert!((narrow - wide).abs() < 1e-5, "vertical FOV changed with aspect: {narrow} vs {wide}");
    }

    #[test]
    fn fov_is_clamped_to_something_projectable() {
        // A 180-degree FOV has an infinite tangent, and 0 has none at all.
        assert!(vertical_fov(0.0, 1.0).is_finite() && vertical_fov(0.0, 1.0) > 0.0);
        assert!(vertical_fov(180.0, 1.0).is_finite());
        assert!(vertical_fov(-50.0, 1.0) > 0.0);
    }

    #[test]
    fn the_frustum_contains_what_is_ahead_and_rejects_what_is_behind() {
        let cam = camera_at(Vec3::ZERO, 0.0);
        let f = cam.frustum();
        assert!(f.contains_point(Vec3::new(500.0, 0.0, 0.0)), "straight ahead");
        assert!(!f.contains_point(Vec3::new(-500.0, 0.0, 0.0)), "behind");
        assert!(!f.contains_point(Vec3::new(1.0, 0.0, 0.0)), "closer than the near plane");
    }

    #[test]
    fn the_frustum_rejects_things_off_to_the_side() {
        let cam = camera_at(Vec3::ZERO, 0.0);
        let f = cam.frustum();
        assert!(!f.contains_point(Vec3::new(100.0, 10000.0, 0.0)));
        assert!(!f.contains_point(Vec3::new(100.0, 0.0, 10000.0)));
    }

    #[test]
    fn box_culling_keeps_anything_that_straddles_a_plane() {
        // Being conservative costs a wasted draw; being wrong the other way
        // makes geometry disappear.
        let cam = camera_at(Vec3::ZERO, 0.0);
        let f = cam.frustum();
        // A box mostly behind the camera but poking into view.
        assert!(f.intersects_box(Vec3::new(-100.0, -50.0, -50.0), Vec3::new(200.0, 50.0, 50.0)));
        // One entirely behind.
        assert!(!f.intersects_box(Vec3::new(-500.0, -50.0, -50.0), Vec3::new(-200.0, 50.0, 50.0)));
    }

    #[test]
    fn turning_the_camera_moves_what_is_visible() {
        let mut cam = camera_at(Vec3::ZERO, 0.0);
        let target = Vec3::new(0.0, 500.0, 0.0);
        assert!(!cam.frustum().contains_point(target), "not visible looking down +X");

        cam.angles.yaw = 90.0;
        assert!(cam.frustum().contains_point(target), "should be visible after turning to face it");
    }

    #[test]
    fn frustum_planes_are_unit_length() {
        // Box distance tests assume normalised planes.
        let cam = camera_at(Vec3::new(10.0, 20.0, 30.0), 37.0);
        for plane in &cam.frustum().planes {
            assert!((plane.normal.length() - 1.0).abs() < 1e-4, "{:?}", plane.normal);
        }
    }
}
