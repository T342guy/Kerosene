// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Viewports: the panes a level is built in.
//!
//! Six are orthographic and axis-aligned -- top, bottom, front, back, left and
//! right -- and one is a perspective 3D view. That layout is Hammer's, and it
//! is not nostalgia: brush geometry is axis-aligned far more often than not,
//! and an orthographic view along an axis is the only way to place a vertex
//! exactly without a numeric entry box.
//!
//! Each 2D view maps two world axes onto the screen. Screen Y grows downward
//! while every world axis it shows grows upward, so the vertical mapping is
//! always inverted -- a detail that produces a vertically mirrored editor if
//! it is missed in one direction and not the other.
//!
//! The horizontal mapping is not always the same either. Opposite views show
//! the same pair of axes but from opposite sides, so one of each pair runs its
//! horizontal axis backwards: walking left in a `Left` view is walking right
//! in a `Right` one. Getting that wrong does not look broken, which is what
//! makes it worth being explicit about -- it just quietly mirrors half the
//! editor.

use kerosene_math::{Aabb, Angles, Vec3};

/// Which view a pane is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewportKind {
    /// Looking down, from +Z: X right, Y up.
    Top,
    /// Looking up, from -Z: X left, Y up.
    Bottom,
    /// Looking along +Y, from -Y: X right, Z up.
    Front,
    /// Looking along -Y, from +Y: X left, Z up.
    Back,
    /// Looking along -X, from +X: Y right, Z up.
    Right,
    /// Looking along +X, from -X: Y left, Z up.
    Left,
    /// The 3D view.
    Perspective,
}

impl ViewportKind {
    pub fn label(self) -> &'static str {
        match self {
            ViewportKind::Top => "top (x/y)",
            ViewportKind::Bottom => "bottom (x/y)",
            ViewportKind::Front => "front (x/z)",
            ViewportKind::Back => "back (x/z)",
            ViewportKind::Right => "right (y/z)",
            ViewportKind::Left => "left (y/z)",
            ViewportKind::Perspective => "3D",
        }
    }

    /// Every kind a pane can be set to, in the order the menu shows them.
    pub fn all() -> [ViewportKind; 7] {
        [
            ViewportKind::Perspective,
            ViewportKind::Top,
            ViewportKind::Bottom,
            ViewportKind::Front,
            ViewportKind::Back,
            ViewportKind::Left,
            ViewportKind::Right,
        ]
    }

    /// `(horizontal, vertical, depth)` world axes, as indices.
    ///
    /// The depth axis is the one the view looks along, which is what a click
    /// cannot determine and what a drag in this view must leave alone.
    pub fn axes(self) -> (usize, usize, usize) {
        match self {
            ViewportKind::Top | ViewportKind::Bottom => (0, 1, 2),
            ViewportKind::Front | ViewportKind::Back => (0, 2, 1),
            ViewportKind::Right | ViewportKind::Left => (1, 2, 0),
            // Meaningless for the 3D view; callers check the kind first.
            ViewportKind::Perspective => (0, 1, 2),
        }
    }

    /// The two axes this view shows, as `(horizontal, vertical)` indices.
    pub fn plane_axes(self) -> (usize, usize) {
        let (h, v, _) = self.axes();
        (h, v)
    }

    /// Which way the horizontal axis runs on screen: `+1` right, `-1` left.
    ///
    /// A view and its opposite show the same plane from opposite sides, and
    /// only this distinguishes them.
    pub fn h_sign(self) -> f32 {
        match self {
            ViewportKind::Bottom | ViewportKind::Back | ViewportKind::Left => -1.0,
            _ => 1.0,
        }
    }

    /// The view looking at the same plane from the other side.
    pub fn opposite(self) -> ViewportKind {
        match self {
            ViewportKind::Top => ViewportKind::Bottom,
            ViewportKind::Bottom => ViewportKind::Top,
            ViewportKind::Front => ViewportKind::Back,
            ViewportKind::Back => ViewportKind::Front,
            ViewportKind::Right => ViewportKind::Left,
            ViewportKind::Left => ViewportKind::Right,
            ViewportKind::Perspective => ViewportKind::Perspective,
        }
    }

    pub fn is_2d(self) -> bool { self != ViewportKind::Perspective }
}

/// One pane.
#[derive(Clone, Debug)]
pub struct Viewport {
    pub kind: ViewportKind,
    /// World point at the centre of a 2D view.
    pub center: Vec3,
    /// Pixels per world unit in a 2D view.
    pub zoom: f32,
    /// Pane size in pixels.
    pub size: (f32, f32),
    /// 3D camera position and orientation.
    pub eye: Vec3,
    pub angles: Angles,
    pub fov: f32,
}

/// Zoom limits. Below the minimum a whole level is a few pixels wide; above
/// the maximum, floating-point coordinates stop being meaningful on screen.
pub const MIN_ZOOM: f32 = 0.005;
pub const MAX_ZOOM: f32 = 32.0;

impl Viewport {
    pub fn new(kind: ViewportKind) -> Viewport {
        Viewport {
            kind,
            center: Vec3::ZERO,
            zoom: 0.25,
            size: (400.0, 300.0),
            eye: Vec3::new(-256.0, 0.0, 128.0),
            angles: Angles::new(15.0, 0.0, 0.0),
            fov: 90.0,
        }
    }

    /// The default four-pane layout.
    pub fn default_layout() -> [Viewport; 4] {
        [
            Viewport::new(ViewportKind::Perspective),
            Viewport::new(ViewportKind::Top),
            Viewport::new(ViewportKind::Front),
            Viewport::new(ViewportKind::Right),
        ]
    }

    /// Change what this pane shows, keeping where it is looking.
    ///
    /// Switching between 2D and 3D keeps the centre: the 3D camera is put a
    /// sensible distance back from whatever the flat view was framing, and a
    /// flat view is centred on wherever the camera was. Losing your place on
    /// every switch is what makes people stop using the other views.
    pub fn set_kind(&mut self, kind: ViewportKind) {
        if kind == self.kind { return }
        match (self.kind.is_2d(), kind.is_2d()) {
            (true, false) => self.eye = self.center - self.angles.forward() * 512.0,
            (false, true) => self.center = self.eye + self.angles.forward() * 256.0,
            _ => {}
        }
        self.kind = kind;
    }

    /// Project a world point into pane-local pixels.
    pub fn world_to_screen(&self, world: Vec3) -> (f32, f32) {
        let (h, v, _) = self.kind.axes();
        let x = (world[h] - self.center[h]) * self.zoom * self.kind.h_sign() + self.size.0 * 0.5;
        // Screen Y grows downward; every world axis shown grows upward.
        let y = self.size.1 * 0.5 - (world[v] - self.center[v]) * self.zoom;
        (x, y)
    }

    /// Unproject pane-local pixels back into the world.
    ///
    /// The depth axis cannot be recovered from a 2D click, so it is taken from
    /// `depth` -- usually the current selection's position, or zero.
    pub fn screen_to_world(&self, x: f32, y: f32, depth: f32) -> Vec3 {
        let (h, v, d) = self.kind.axes();
        let mut world = Vec3::ZERO;
        world[h] = (x - self.size.0 * 0.5) / (self.zoom * self.kind.h_sign()) + self.center[h];
        world[v] = (self.size.1 * 0.5 - y) / self.zoom + self.center[v];
        world[d] = depth;
        world
    }

    /// The world rectangle this pane is showing.
    pub fn visible_bounds(&self) -> Aabb {
        let (h, v, d) = self.kind.axes();
        let half_h = self.size.0 * 0.5 / self.zoom;
        let half_v = self.size.1 * 0.5 / self.zoom;
        let mut min = Vec3::splat(-kerosene_math::MAX_MAP_COORD);
        let mut max = Vec3::splat(kerosene_math::MAX_MAP_COORD);
        min[h] = self.center[h] - half_h;
        max[h] = self.center[h] + half_h;
        min[v] = self.center[v] - half_v;
        max[v] = self.center[v] + half_v;
        // The depth axis is unbounded: a 2D view sees all the way through.
        min[d] = -kerosene_math::MAX_MAP_COORD;
        max[d] = kerosene_math::MAX_MAP_COORD;
        Aabb::new(min, max)
    }

    /// Zoom about a fixed screen point, so the world point under the cursor
    /// stays under it.
    ///
    /// Zooming about the centre instead makes the thing you were looking at
    /// slide away, which is the single most irritating thing an editor can do.
    pub fn zoom_at(&mut self, factor: f32, screen_x: f32, screen_y: f32) {
        if !self.kind.is_2d() { return; }
        let before = self.screen_to_world(screen_x, screen_y, 0.0);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.screen_to_world(screen_x, screen_y, 0.0);

        let (h, v, _) = self.kind.axes();
        self.center[h] += before[h] - after[h];
        self.center[v] += before[v] - after[v];
    }

    /// Pan by a screen-space delta.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let (h, v, _) = self.kind.axes();
        self.center[h] -= dx / (self.zoom * self.kind.h_sign());
        self.center[v] += dy / self.zoom;
    }

    /// Frame a bounding box, with a little margin.
    pub fn focus_on(&mut self, bounds: Aabb) {
        if bounds.is_empty() { return; }
        self.center = bounds.center();
        if !self.kind.is_2d() {
            // Back off far enough that the box fits in the view.
            let radius = bounds.size().length() * 0.5;
            let back = (radius / (self.fov.to_radians() * 0.5).tan()).max(64.0);
            self.eye = bounds.center() - self.angles.forward() * back;
            return;
        }
        let (h, v, _) = self.kind.axes();
        let size = bounds.size();
        let fit_h = self.size.0 / size[h].max(1.0);
        let fit_v = self.size.1 / size[v].max(1.0);
        self.zoom = (fit_h.min(fit_v) * 0.8).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// How far the camera moves for one step of fly controls.
    ///
    /// Forward and sideways follow where the camera is looking; up and down do
    /// not. Pressing "up" while looking at the floor has to go up, because
    /// that is what a person means by up -- tying it to the camera's own up
    /// vector makes the control useless at exactly the angles you need it.
    ///
    /// The direction is normalised before scaling, so holding two keys is not
    /// faster than holding one.
    pub fn fly_step(&self, forward: f32, side: f32, up: f32, distance: f32) -> Vec3 {
        let basis = self.angles.vectors();
        let motion = basis.forward * forward + basis.right * side + Vec3::Z * up;
        motion.normalize_or_zero() * distance
    }

    /// A ray from the 3D camera through a pane pixel, for picking.
    pub fn pick_ray(&self, x: f32, y: f32) -> (Vec3, Vec3) {
        let basis = self.angles.vectors();
        let aspect = self.size.0 / self.size.1.max(1.0);
        let half_y = (kerosene_render::vertical_fov(self.fov, aspect) * 0.5).tan();
        let half_x = half_y * aspect;

        // Normalised device coordinates, with Y flipped for screen space.
        let ndc_x = (x / self.size.0.max(1.0)) * 2.0 - 1.0;
        let ndc_y = 1.0 - (y / self.size.1.max(1.0)) * 2.0;

        let direction =
            (basis.forward + basis.right * (ndc_x * half_x) + basis.up * (ndc_y * half_y))
                .normalize_or_zero();
        (self.eye, direction)
    }

    /// Whether a box is at least partly visible in a 2D pane.
    pub fn shows(&self, bounds: Aabb) -> bool {
        if !self.kind.is_2d() { return true; }
        self.visible_bounds().intersects(&bounds)
    }
}

/// Where a ray enters a box, if it does.
pub fn ray_box(origin: Vec3, direction: Vec3, bounds: Aabb) -> Option<f32> {
    let mut enter = 0.0f32;
    let mut exit = f32::INFINITY;

    for axis in 0..3 {
        if direction[axis].abs() < 1e-9 {
            if origin[axis] < bounds.min[axis] || origin[axis] > bounds.max[axis] { return None; }
            continue;
        }
        let inv = 1.0 / direction[axis];
        let mut t0 = (bounds.min[axis] - origin[axis]) * inv;
        let mut t1 = (bounds.max[axis] - origin[axis]) * inv;
        if t0 > t1 { std::mem::swap(&mut t0, &mut t1); }
        enter = enter.max(t0);
        exit = exit.min(t1);
        if enter > exit { return None; }
    }
    (exit >= 0.0).then_some(enter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(kind: ViewportKind) -> Viewport {
        Viewport { size: (800.0, 600.0), zoom: 1.0, ..Viewport::new(kind) }
    }

    #[test]
    fn opposite_views_show_the_same_plane_from_the_other_side() {
        for kind in ViewportKind::all() {
            if !kind.is_2d() { continue }
            let other = kind.opposite();
            assert_ne!(other, kind, "{kind:?} has no opposite");
            assert_eq!(other.axes(), kind.axes(), "{kind:?} and its opposite show different axes");
            assert_eq!(
                other.h_sign(),
                -kind.h_sign(),
                "{kind:?} and its opposite run their horizontal axis the same way"
            );
            assert_eq!(other.opposite(), kind);
        }
    }

    #[test]
    fn a_point_lands_on_opposite_sides_of_two_opposing_views() {
        // The whole reason `h_sign` exists: the same world point must appear
        // mirrored, not identical, in a view and its opposite.
        for kind in [ViewportKind::Top, ViewportKind::Front, ViewportKind::Right] {
            let (h, _, _) = kind.axes();
            let mut point = Vec3::ZERO;
            point[h] = 100.0;

            let a = pane(kind);
            let b = pane(kind.opposite());
            let (ax, _) = a.world_to_screen(point);
            let (bx, _) = b.world_to_screen(point);
            let middle = a.size.0 * 0.5;
            assert!(ax > middle, "{kind:?} should show +{h} to the right");
            assert!(bx < middle, "{:?} should show +{h} to the left", kind.opposite());
            assert!((ax - middle + (bx - middle)).abs() < 1e-3, "not a mirror image");
        }
    }

    #[test]
    fn a_click_in_an_opposite_view_still_unprojects_to_where_it_was() {
        for kind in ViewportKind::all() {
            if !kind.is_2d() { continue }
            let view = pane(kind);
            let (h, v, _) = kind.axes();
            let mut point = Vec3::ZERO;
            point[h] = -37.0;
            point[v] = 91.0;
            let (x, y) = view.world_to_screen(point);
            let back = view.screen_to_world(x, y, 0.0);
            assert!((back[h] - point[h]).abs() < 1e-3, "{kind:?} horizontal round trip");
            assert!((back[v] - point[v]).abs() < 1e-3, "{kind:?} vertical round trip");
        }
    }

    #[test]
    fn panning_moves_the_view_the_way_the_pointer_went_in_every_kind() {
        for kind in ViewportKind::all() {
            if !kind.is_2d() { continue }
            let mut view = pane(kind);
            let under_cursor = view.screen_to_world(100.0, 100.0, 0.0);
            view.pan(40.0, 25.0);
            let now = view.screen_to_world(140.0, 125.0, 0.0);
            let (h, v, _) = kind.axes();
            assert!((now[h] - under_cursor[h]).abs() < 1e-3, "{kind:?} horizontal pan drifted");
            assert!((now[v] - under_cursor[v]).abs() < 1e-3, "{kind:?} vertical pan drifted");
        }
    }

    #[test]
    fn switching_a_pane_between_2d_and_3d_keeps_your_place() {
        let mut view = pane(ViewportKind::Top);
        view.center = Vec3::new(500.0, 300.0, 64.0);
        view.set_kind(ViewportKind::Perspective);
        // The camera is behind the point it was centred on, looking at it.
        let to_centre = Vec3::new(500.0, 300.0, 64.0) - view.eye;
        assert!(to_centre.length() > 1.0, "the camera landed on top of the target");
        assert!(
            to_centre.normalize().dot(view.angles.forward()) > 0.99,
            "the camera is not looking at what the flat view was showing"
        );

        view.set_kind(ViewportKind::Front);
        assert!(
            (view.center - Vec3::new(500.0, 300.0, 64.0)).length() < 300.0,
            "the flat view came back somewhere else entirely"
        );
    }

    #[test]
    fn flying_forward_follows_where_the_camera_looks() {
        let mut view = pane(ViewportKind::Perspective);
        view.angles = Angles::new(0.0, 90.0, 0.0);
        let step = view.fly_step(1.0, 0.0, 0.0, 100.0);
        assert!((step - view.angles.forward() * 100.0).length() < 1e-3);
        assert!((step.length() - 100.0).abs() < 1e-3);
    }

    #[test]
    fn flying_up_is_world_up_however_the_camera_is_pointed() {
        for pitch in [-89.0f32, -45.0, 0.0, 45.0, 89.0] {
            let mut view = pane(ViewportKind::Perspective);
            view.angles = Angles::new(pitch, 37.0, 0.0);
            let step = view.fly_step(0.0, 0.0, 1.0, 100.0);
            assert!(
                (step - Vec3::Z * 100.0).length() < 1e-3,
                "at pitch {pitch} 'up' went {step:?} instead of straight up"
            );
        }
    }

    #[test]
    fn holding_two_directions_is_not_faster_than_one() {
        let mut view = pane(ViewportKind::Perspective);
        view.angles = Angles::new(0.0, 0.0, 0.0);
        let one = view.fly_step(1.0, 0.0, 0.0, 100.0);
        let two = view.fly_step(1.0, 1.0, 0.0, 100.0);
        assert!((one.length() - two.length()).abs() < 1e-3, "diagonal flying was faster");
    }

    #[test]
    fn pressing_nothing_moves_nothing() {
        let view = pane(ViewportKind::Perspective);
        assert_eq!(view.fly_step(0.0, 0.0, 0.0, 100.0), Vec3::ZERO);
    }

    #[test]
    fn strafing_is_perpendicular_to_looking() {
        let mut view = pane(ViewportKind::Perspective);
        view.angles = Angles::new(20.0, 145.0, 0.0);
        let side = view.fly_step(0.0, 1.0, 0.0, 1.0);
        assert!(side.dot(view.angles.forward()).abs() < 1e-3);
        assert!(side.z.abs() < 1e-3, "strafing should stay level, went {side:?}");
    }

    #[test]
    fn each_2d_view_shows_the_axes_it_should() {
        assert_eq!(ViewportKind::Top.axes(), (0, 1, 2));
        assert_eq!(ViewportKind::Front.axes(), (0, 2, 1));
        assert_eq!(ViewportKind::Right.axes(), (1, 2, 0));
    }

    #[test]
    fn the_view_centre_maps_to_the_pane_centre() {
        for kind in [ViewportKind::Top, ViewportKind::Front, ViewportKind::Right] {
            let v = pane(kind);
            let (x, y) = v.world_to_screen(v.center);
            assert!((x - 400.0).abs() < 1e-3 && (y - 300.0).abs() < 1e-3, "{kind:?}");
        }
    }

    #[test]
    fn the_vertical_axis_is_inverted_not_mirrored() {
        // Getting this wrong in one direction and not the other gives a
        // vertically mirrored editor that still round-trips.
        let v = pane(ViewportKind::Top);
        let (_, up) = v.world_to_screen(Vec3::new(0.0, 100.0, 0.0));
        let (_, down) = v.world_to_screen(Vec3::new(0.0, -100.0, 0.0));
        assert!(up < down, "moving +Y should move up the screen: {up} vs {down}");
    }

    #[test]
    fn projection_round_trips() {
        for kind in [ViewportKind::Top, ViewportKind::Front, ViewportKind::Right] {
            let v = pane(kind);
            let world = Vec3::new(123.0, -45.0, 67.0);
            let (x, y) = v.world_to_screen(world);
            let (_, _, d) = kind.axes();
            let back = v.screen_to_world(x, y, world[d]);
            assert!((back - world).length() < 1e-3, "{kind:?}: {back:?} vs {world:?}");
        }
    }

    #[test]
    fn zooming_keeps_the_point_under_the_cursor_still() {
        // Zooming about the centre makes what you were looking at slide away.
        let mut v = pane(ViewportKind::Top);
        let (cursor_x, cursor_y) = (600.0, 200.0);
        let before = v.screen_to_world(cursor_x, cursor_y, 0.0);
        v.zoom_at(2.0, cursor_x, cursor_y);
        let after = v.screen_to_world(cursor_x, cursor_y, 0.0);
        assert!((before - after).length() < 1e-3, "{before:?} moved to {after:?}");
        assert_eq!(v.zoom, 2.0);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut v = pane(ViewportKind::Top);
        for _ in 0..50 { v.zoom_at(2.0, 400.0, 300.0); }
        assert_eq!(v.zoom, MAX_ZOOM);
        for _ in 0..100 { v.zoom_at(0.5, 400.0, 300.0); }
        assert_eq!(v.zoom, MIN_ZOOM);
    }

    #[test]
    fn panning_moves_the_view_with_the_drag() {
        let mut v = pane(ViewportKind::Top);
        v.pan(100.0, 0.0);
        // Dragging right shows what was to the left.
        assert!(v.center.x < 0.0, "{:?}", v.center);
        v.pan(0.0, 100.0);
        assert!(v.center.y > 0.0);
    }

    #[test]
    fn a_2d_view_sees_all_the_way_through_its_depth_axis() {
        // A brush far away along the view axis must still be visible and
        // selectable; otherwise a top view only shows one slab of the level.
        let v = pane(ViewportKind::Top);
        let far_below = Aabb::new(Vec3::new(-10.0, -10.0, -8000.0), Vec3::new(10.0, 10.0, -7000.0));
        assert!(v.shows(far_below));
    }

    #[test]
    fn a_box_outside_the_pane_is_not_shown() {
        let v = pane(ViewportKind::Top);
        let away = Aabb::new(Vec3::new(5000.0, 5000.0, 0.0), Vec3::new(5100.0, 5100.0, 10.0));
        assert!(!v.shows(away));
    }

    #[test]
    fn focusing_frames_the_selection() {
        let mut v = pane(ViewportKind::Top);
        let bounds = Aabb::new(Vec3::new(100.0, 100.0, 0.0), Vec3::new(200.0, 200.0, 64.0));
        v.focus_on(bounds);
        assert_eq!(v.center.x, 150.0);
        assert_eq!(v.center.y, 150.0);
        assert!(v.shows(bounds));
    }

    #[test]
    fn a_ray_through_the_middle_of_the_pane_goes_straight_ahead() {
        let mut v = pane(ViewportKind::Perspective);
        v.eye = Vec3::ZERO;
        v.angles = Angles::ZERO;
        let (origin, direction) = v.pick_ray(400.0, 300.0);
        assert_eq!(origin, Vec3::ZERO);
        assert!((direction - Vec3::X).length() < 1e-4, "{direction:?}");
    }

    #[test]
    fn rays_toward_the_edges_of_the_pane_spread_out() {
        let mut v = pane(ViewportKind::Perspective);
        v.eye = Vec3::ZERO;
        v.angles = Angles::ZERO;
        // Right of centre points to the viewer's right, which is -Y.
        let (_, right) = v.pick_ray(790.0, 300.0);
        assert!(right.y < -0.1, "{right:?}");
        // Above centre points up.
        let (_, up) = v.pick_ray(400.0, 10.0);
        assert!(up.z > 0.1, "{up:?}");
    }

    #[test]
    fn ray_box_finds_the_near_face() {
        let bounds = Aabb::new(Vec3::new(100.0, -50.0, -50.0), Vec3::new(200.0, 50.0, 50.0));
        let hit = ray_box(Vec3::ZERO, Vec3::X, bounds).expect("should hit");
        assert!((hit - 100.0).abs() < 1e-3);
        assert!(ray_box(Vec3::ZERO, -Vec3::X, bounds).is_none(), "behind the ray");
        assert!(ray_box(Vec3::ZERO, Vec3::Y, bounds).is_none(), "missing entirely");
    }

    #[test]
    fn a_ray_starting_inside_a_box_still_hits_it() {
        let bounds = Aabb::new(Vec3::splat(-10.0), Vec3::splat(10.0));
        assert_eq!(ray_box(Vec3::ZERO, Vec3::X, bounds), Some(0.0));
    }
}
