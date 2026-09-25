// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The move and rotate gizmo: on-screen handles in the 3D pane that do
//! directly what the transform dialog already can, for a hand that would
//! rather drag an axis than type three numbers into a box.
//!
//! Kept away from the UI, same as [`crate::tools`]: hit-testing and drag
//! math are plain geometry against a [`Viewport`], so they are testable
//! without an egui context and agree with the painter by construction --
//! both read the same [`project`].
//!
//! The 3D pane otherwise never drags geometry (see `viewport_input`'s own
//! comment on why): a free drag has no unambiguous depth from a single
//! camera. An axis or a rotation plane removes exactly that ambiguity, which
//! is what makes a gizmo drag possible here where a plain one is not.

use crate::draw::NEAR;
use crate::viewport::Viewport;
use kerosene_math::{Quat, Vec3};

/// One of the three axes a gizmo handle works along or about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GizmoAxis {
    X,
    Y,
    Z,
}

impl GizmoAxis {
    pub fn all() -> [GizmoAxis; 3] {
        [GizmoAxis::X, GizmoAxis::Y, GizmoAxis::Z]
    }

    pub fn vector(self) -> Vec3 {
        match self {
            GizmoAxis::X => Vec3::X,
            GizmoAxis::Y => Vec3::Y,
            GizmoAxis::Z => Vec3::Z,
        }
    }

    /// Two vectors spanning the plane this axis is normal to, used to trace
    /// out its rotation ring. Their own directions do not matter -- only
    /// that they are orthonormal to each other and to the axis -- so the
    /// ring comes out circular whichever axis it is drawn for.
    pub fn plane_basis(self) -> (Vec3, Vec3) {
        match self {
            GizmoAxis::X => (Vec3::Y, Vec3::Z),
            GizmoAxis::Y => (Vec3::Z, Vec3::X),
            GizmoAxis::Z => (Vec3::X, Vec3::Y),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GizmoAxis::X => "x",
            GizmoAxis::Y => "y",
            GizmoAxis::Z => "z",
        }
    }
}

/// Which handles are on offer: the three move arrows, or the three rotation
/// rings. Never both at once -- Hammer's own transform tools are exclusive,
/// and two kinds of handle stacked on the same selection is more to parse,
/// not more to do with.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GizmoMode {
    #[default]
    Move,
    Rotate,
}

/// How close, in pixels, the pointer must land to a handle to grab it.
/// Matches [`crate::tools::HANDLE_GRAB`]'s reasoning: generous, because
/// missing costs nothing but falling through to a plain pick, and a gizmo
/// that only grabs on the third try trains people to stop trusting it.
pub const GRAB_PIXELS: f32 = 8.0;

/// How many segments a drawn or hit-tested rotation ring is approximated
/// with. Coarse enough to cost nothing, fine enough that the facets do not
/// show at the ring's usual on-screen size. `pub` so the painter traces the
/// exact same polygon this hit-tests against, rather than a similar-looking
/// one with its own idea of how round a circle is.
pub const RING_SEGMENTS: usize = 32;

/// The on-screen size a gizmo handle is drawn and hit-tested at, in pixels.
/// One constant so sizing can never disagree with itself between the two.
pub const TARGET_PIXELS: f32 = 70.0;

/// Project a world point into pane-local pixels (`(0, 0)` at the pane's own
/// top-left corner, matching the coordinates `viewport_input` already hit-
/// tests everything else in), or `None` when it falls behind the near plane.
///
/// The drawing code adds the pane's own on-screen offset to these same
/// coordinates, so a handle drawn where this says it is is a handle
/// grabbable where this says it is.
pub fn project(viewport: &Viewport, world: Vec3) -> Option<(f32, f32)> {
    let basis = viewport.angles.vectors();
    let relative = world - viewport.eye;
    let forward = relative.dot(basis.forward);
    if forward < NEAR {
        return None;
    }
    let aspect = viewport.size.0 / viewport.size.1.max(1.0);
    let half_y = (kerosene_render::vertical_fov(viewport.fov, aspect) * 0.5)
        .tan()
        .max(1e-4);
    let half_x = half_y * aspect;
    let right = relative.dot(basis.right);
    let up = relative.dot(basis.up);
    let x = viewport.size.0 * 0.5 + (right / (forward * half_x)) * viewport.size.0 * 0.5;
    let y = viewport.size.1 * 0.5 - (up / (forward * half_y)) * viewport.size.1 * 0.5;
    Some((x, y))
}

/// The world-space arrow/ring size that reads as `target_px` pixels on
/// screen at `origin`'s distance from the camera -- the Blender/Hammer
/// convention of a gizmo that neither swamps a close object nor vanishes on
/// a distant one.
pub fn world_radius(viewport: &Viewport, origin: Vec3, target_px: f32) -> f32 {
    let distance = (origin - viewport.eye).length().max(1.0);
    let aspect = viewport.size.0 / viewport.size.1.max(1.0);
    let half_y = (kerosene_render::vertical_fov(viewport.fov, aspect) * 0.5).tan();
    distance * half_y * (target_px / (viewport.size.1.max(1.0) * 0.5))
}

/// Distance from `(x, y)` to the segment `a`-`b`, in the same units as the
/// points.
fn point_segment_distance(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq < 1e-9 {
        0.0
    } else {
        (((x - a.0) * dx + (y - a.1) * dy) / len_sq).clamp(0.0, 1.0)
    };
    let (px, py) = (a.0 + dx * t, a.1 + dy * t);
    ((x - px).powi(2) + (y - py).powi(2)).sqrt()
}

/// The move-arrow, or rotation-ring, handle nearest a screen point, if the
/// pointer landed close enough to one to grab it.
pub fn hit(
    mode: GizmoMode,
    pivot: Vec3,
    radius: f32,
    viewport: &Viewport,
    x: f32,
    y: f32,
) -> Option<GizmoAxis> {
    let mut best: Option<(f32, GizmoAxis)> = None;
    for axis in GizmoAxis::all() {
        let distance = match mode {
            GizmoMode::Move => {
                let (Some(a), Some(b)) = (
                    project(viewport, pivot),
                    project(viewport, pivot + axis.vector() * radius),
                ) else {
                    continue;
                };
                point_segment_distance(x, y, a, b)
            }
            GizmoMode::Rotate => {
                let (u, v) = axis.plane_basis();
                let mut points = Vec::with_capacity(RING_SEGMENTS);
                for i in 0..RING_SEGMENTS {
                    let theta = (i as f32 / RING_SEGMENTS as f32) * std::f32::consts::TAU;
                    let world = pivot + (u * theta.cos() + v * theta.sin()) * radius;
                    points.push(project(viewport, world));
                }
                let mut nearest = f32::INFINITY;
                for i in 0..RING_SEGMENTS {
                    let (Some(a), Some(b)) = (points[i], points[(i + 1) % RING_SEGMENTS]) else {
                        continue;
                    };
                    nearest = nearest.min(point_segment_distance(x, y, a, b));
                }
                nearest
            }
        };
        if distance > GRAB_PIXELS {
            continue;
        }
        if best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, axis));
        }
    }
    best.map(|(_, axis)| axis)
}

/// Where the ray comes closest to the line through `pivot` along `axis`
/// (both worldspace, `axis` normalized), as a signed distance from `pivot`.
///
/// `None` when the ray runs nearly parallel to the axis: the two lines'
/// closest point is then unstable, sliding to infinity for a fraction of a
/// pixel of mouse movement, which is worse than refusing the drag.
fn axis_parameter(origin: Vec3, direction: Vec3, pivot: Vec3, axis: Vec3) -> Option<f32> {
    let r = origin - pivot;
    let b = direction.dot(axis);
    let denom = 1.0 - b * b;
    if denom.abs() < 1e-4 {
        return None;
    }
    let c = direction.dot(r);
    let f = axis.dot(r);
    Some((f - b * c) / denom)
}

/// Where the ray meets the plane through `pivot` normal to `axis`, if it
/// meets it at all in front of the camera.
fn plane_hit(origin: Vec3, direction: Vec3, pivot: Vec3, axis: Vec3) -> Option<Vec3> {
    let denom = direction.dot(axis);
    if denom.abs() < 1e-5 {
        return None;
    }
    let t = (pivot - origin).dot(axis) / denom;
    if t < 0.0 {
        return None;
    }
    Some(origin + direction * t)
}

/// What a gizmo drag has produced so far: the whole delta from where it
/// began, not an increment -- so a caller can preview it every frame and
/// commit it once, on release, as a single undo step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GizmoUpdate {
    Move(Vec3),
    /// The pivot to turn about, and the rotation to turn by.
    Rotate(Vec3, Quat),
}

/// A move or rotate drag in progress, anchored to the ray it began on.
#[derive(Clone, Copy, Debug)]
pub struct GizmoDrag {
    pub axis: GizmoAxis,
    pivot: Vec3,
    start: DragStart,
}

#[derive(Clone, Copy, Debug)]
enum DragStart {
    Move(f32),
    /// The direction from `pivot` to the ray's hit on the rotation plane, at
    /// the moment the drag began -- the reference every later angle is
    /// measured against.
    Rotate(Vec3),
}

impl GizmoDrag {
    /// Start a drag on `axis`, about `pivot`, from the ray through the
    /// pointer's press position. `None` when that ray cannot establish a
    /// reference to measure against (see [`axis_parameter`] and
    /// [`plane_hit`]) -- the press falls through to an ordinary pick instead
    /// of starting a drag that cannot go anywhere.
    pub fn begin(
        mode: GizmoMode,
        axis: GizmoAxis,
        pivot: Vec3,
        origin: Vec3,
        direction: Vec3,
    ) -> Option<GizmoDrag> {
        let start = match mode {
            GizmoMode::Move => {
                DragStart::Move(axis_parameter(origin, direction, pivot, axis.vector())?)
            }
            GizmoMode::Rotate => {
                let hit = plane_hit(origin, direction, pivot, axis.vector())?;
                let reference = hit - pivot;
                if reference.length() < 1e-4 {
                    return None;
                }
                DragStart::Rotate(reference.normalize())
            }
        };
        Some(GizmoDrag { axis, pivot, start })
    }

    /// The total delta from the drag's start to where the ray now points.
    /// `None` on a frame where the ray has become degenerate (see
    /// [`Self::begin`]) -- the caller keeps showing the last good update
    /// rather than snapping to a meaningless one.
    pub fn update(&self, origin: Vec3, direction: Vec3) -> Option<GizmoUpdate> {
        match self.start {
            DragStart::Move(start_t) => {
                let t = axis_parameter(origin, direction, self.pivot, self.axis.vector())?;
                Some(GizmoUpdate::Move(self.axis.vector() * (t - start_t)))
            }
            DragStart::Rotate(reference) => {
                let hit = plane_hit(origin, direction, self.pivot, self.axis.vector())?;
                let current = hit - self.pivot;
                if current.length() < 1e-4 {
                    return None;
                }
                let current = current.normalize();
                let axis = self.axis.vector();
                let angle = reference
                    .cross(current)
                    .dot(axis)
                    .atan2(reference.dot(current));
                Some(GizmoUpdate::Rotate(
                    self.pivot,
                    Quat::from_axis_angle(axis, angle),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::ViewportKind;
    use kerosene_math::Angles;

    /// A pane looking down +Y from `(0, -500, 0)`, so `right` is +X and `up`
    /// is +Z -- easy to reason about, and non-degenerate for both a move
    /// along X or Z and a rotation about Y (the axis the camera looks along,
    /// which is the one angle a gizmo ring is actually meant to be viewed
    /// down).
    fn looking_down_y() -> Viewport {
        Viewport {
            size: (800.0, 600.0),
            eye: Vec3::new(0.0, -500.0, 0.0),
            angles: Angles::new(0.0, 90.0, 0.0),
            fov: 90.0,
            ..Viewport::new(ViewportKind::Perspective)
        }
    }

    #[test]
    fn the_pivot_projects_to_the_pane_centre() {
        let v = looking_down_y();
        let (x, y) = project(&v, Vec3::ZERO).expect("in front of the camera");
        assert!(
            (x - 400.0).abs() < 1e-3 && (y - 300.0).abs() < 1e-3,
            "{x} {y}"
        );
    }

    #[test]
    fn a_point_behind_the_camera_does_not_project() {
        let v = looking_down_y();
        assert_eq!(project(&v, Vec3::new(0.0, -600.0, 0.0)), None);
    }

    #[test]
    fn the_x_arrow_projects_to_the_right_and_the_z_arrow_upward() {
        let v = looking_down_y();
        let (cx, cy) = project(&v, Vec3::ZERO).unwrap();
        let (xx, xy) = project(&v, Vec3::new(50.0, 0.0, 0.0)).unwrap();
        let (zx, zy) = project(&v, Vec3::new(0.0, 0.0, 50.0)).unwrap();
        assert!(xx > cx, "the X arrow should read to the right");
        assert!((xy - cy).abs() < 1e-3, "the X arrow should stay level");
        assert!(zy < cy, "the Z arrow should read upward");
        assert!((zx - cx).abs() < 1e-3, "the Z arrow should stay centred");
    }

    #[test]
    fn world_radius_grows_with_distance_so_the_screen_size_stays_put() {
        let v = looking_down_y();
        let near = world_radius(&v, Vec3::ZERO, 60.0);
        let far = world_radius(&v, Vec3::new(0.0, 500.0, 0.0), 60.0);
        // Twice as far from the camera calls for twice the world size, to
        // read as the same number of pixels.
        assert!((far / near - 2.0).abs() < 0.01, "{near} {far}");
    }

    #[test]
    fn hitting_near_the_x_arrow_picks_x_and_a_miss_picks_nothing() {
        let v = looking_down_y();
        let (hx, hy) = project(&v, Vec3::new(50.0, 0.0, 0.0)).unwrap();
        assert_eq!(
            hit(GizmoMode::Move, Vec3::ZERO, 100.0, &v, hx, hy),
            Some(GizmoAxis::X)
        );
        assert_eq!(
            hit(GizmoMode::Move, Vec3::ZERO, 100.0, &v, 10.0, 590.0),
            None
        );
    }

    #[test]
    fn hitting_near_the_z_arrow_picks_z() {
        let v = looking_down_y();
        let (hx, hy) = project(&v, Vec3::new(0.0, 0.0, 50.0)).unwrap();
        assert_eq!(
            hit(GizmoMode::Move, Vec3::ZERO, 100.0, &v, hx, hy),
            Some(GizmoAxis::Z)
        );
    }

    #[test]
    fn hitting_near_the_y_rotation_ring_picks_y() {
        let v = looking_down_y();
        // The ring for rotating about Y is traced in the XZ plane -- the one
        // the camera, looking straight down Y, sees face-on. A point at 45
        // degrees round it, rather than on an axis, keeps this off the X and
        // Z rings too: seen edge-on from here, those two collapse to lines
        // through the very same axis points this test would otherwise use.
        let (u, w) = GizmoAxis::Y.plane_basis();
        let theta = std::f32::consts::FRAC_PI_4;
        let point = (u * theta.cos() + w * theta.sin()) * 80.0;
        let (hx, hy) = project(&v, point).unwrap();
        assert_eq!(
            hit(GizmoMode::Rotate, Vec3::ZERO, 80.0, &v, hx, hy),
            Some(GizmoAxis::Y)
        );
    }

    #[test]
    fn a_move_drag_along_x_reports_the_ray_s_own_travel() {
        let v = looking_down_y();
        let start = v.pick_ray(400.0, 300.0);
        let drag = GizmoDrag::begin(GizmoMode::Move, GizmoAxis::X, Vec3::ZERO, start.0, start.1)
            .expect("not parallel to X");

        // No movement yet: the update at the start position is a no-op.
        match drag.update(start.0, start.1) {
            Some(GizmoUpdate::Move(delta)) => assert!(delta.length() < 1e-3, "{delta:?}"),
            other => panic!("{other:?}"),
        }

        let moved = v.pick_ray(500.0, 300.0);
        match drag.update(moved.0, moved.1).expect("still not parallel") {
            GizmoUpdate::Move(delta) => {
                assert!(delta.x > 0.0, "dragging right should move +X: {delta:?}");
                assert!(delta.y.abs() < 1e-3 && delta.z.abs() < 1e-3, "{delta:?}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_move_drag_parallel_to_its_own_axis_refuses_to_begin() {
        // Staring straight down the axis you are trying to slide along has
        // no stable answer -- every point on the axis looks the same.
        let v = Viewport {
            eye: Vec3::new(-500.0, 0.0, 0.0),
            angles: Angles::ZERO,
            ..looking_down_y()
        };
        let (origin, direction) = v.pick_ray(400.0, 300.0);
        assert!(
            GizmoDrag::begin(GizmoMode::Move, GizmoAxis::X, Vec3::ZERO, origin, direction)
                .is_none()
        );
    }

    #[test]
    fn a_rotate_drag_reports_no_turn_at_its_own_start() {
        let v = looking_down_y();
        let start = v.pick_ray(450.0, 300.0);
        let drag = GizmoDrag::begin(
            GizmoMode::Rotate,
            GizmoAxis::Y,
            Vec3::ZERO,
            start.0,
            start.1,
        )
        .expect("off-centre, so a reference direction exists");
        match drag.update(start.0, start.1) {
            Some(GizmoUpdate::Rotate(_, q)) => {
                assert!(q.angle_between(Quat::IDENTITY) < 1e-3, "{q:?}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_quarter_turn_on_screen_is_a_quarter_turn_about_the_axis() {
        let v = looking_down_y();
        let start = v.pick_ray(450.0, 300.0);
        let drag = GizmoDrag::begin(
            GizmoMode::Rotate,
            GizmoAxis::Y,
            Vec3::ZERO,
            start.0,
            start.1,
        )
        .expect("off-centre, so a reference direction exists");

        // From level with the pivot to straight above it: a quarter of the
        // way round the ring.
        let turned = v.pick_ray(400.0, 220.0);
        match drag.update(turned.0, turned.1).expect("still on the plane") {
            GizmoUpdate::Rotate(pivot, q) => {
                assert_eq!(pivot, Vec3::ZERO);
                let (_, angle) = q.to_axis_angle();
                assert!(
                    (angle.to_degrees().abs() - 90.0).abs() < 5.0,
                    "{}",
                    angle.to_degrees()
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_rotate_drag_at_dead_centre_refuses_to_begin() {
        // The ray through dead centre hits the pivot itself: no direction to
        // measure an angle from.
        let v = looking_down_y();
        let (origin, direction) = v.pick_ray(400.0, 300.0);
        assert!(
            GizmoDrag::begin(
                GizmoMode::Rotate,
                GizmoAxis::Y,
                Vec3::ZERO,
                origin,
                direction
            )
            .is_none()
        );
    }
}
