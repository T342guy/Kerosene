//! Viewports: the four panes a level is built in.
//!
//! Three are orthographic and axis-aligned -- top, front and side -- and one
//! is a perspective 3D view. That layout is Hammer's, and it is not
//! nostalgia: brush geometry is axis-aligned far more often than not, and an
//! orthographic view along an axis is the only way to place a vertex exactly
//! without a numeric entry box.
//!
//! Each 2D view maps two world axes onto the screen. Screen Y grows downward
//! while every world axis it shows grows upward, so the vertical mapping is
//! always inverted -- a detail that produces a vertically mirrored editor if
//! it is missed in one direction and not the other.

use void_math::{Aabb, Angles, Vec3};

/// Which view a pane is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewportKind {
    /// Looking down: X right, Y up.
    Top,
    /// Looking along +Y: X right, Z up.
    Front,
    /// Looking along -X: Y right, Z up.
    Side,
    /// The 3D view.
    Perspective,
}

impl ViewportKind {
    pub fn label(self) -> &'static str {
        match self {
            ViewportKind::Top => "top (x/y)",
            ViewportKind::Front => "front (x/z)",
            ViewportKind::Side => "side (y/z)",
            ViewportKind::Perspective => "3D",
        }
    }

    /// `(horizontal, vertical, depth)` world axes, as indices.
    ///
    /// The depth axis is the one the view looks along, which is what a click
    /// cannot determine and what a drag in this view must leave alone.
    pub fn axes(self) -> (usize, usize, usize) {
        match self {
            ViewportKind::Top => (0, 1, 2),
            ViewportKind::Front => (0, 2, 1),
            ViewportKind::Side => (1, 2, 0),
            // Meaningless for the 3D view; callers check the kind first.
            ViewportKind::Perspective => (0, 1, 2),
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
            Viewport::new(ViewportKind::Side),
        ]
    }

    /// Project a world point into pane-local pixels.
    pub fn world_to_screen(&self, world: Vec3) -> (f32, f32) {
        let (h, v, _) = self.kind.axes();
        let x = (world[h] - self.center[h]) * self.zoom + self.size.0 * 0.5;
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
        world[h] = (x - self.size.0 * 0.5) / self.zoom + self.center[h];
        world[v] = (self.size.1 * 0.5 - y) / self.zoom + self.center[v];
        world[d] = depth;
        world
    }

    /// The world rectangle this pane is showing.
    pub fn visible_bounds(&self) -> Aabb {
        let (h, v, d) = self.kind.axes();
        let half_h = self.size.0 * 0.5 / self.zoom;
        let half_v = self.size.1 * 0.5 / self.zoom;
        let mut min = Vec3::splat(-void_math::MAX_MAP_COORD);
        let mut max = Vec3::splat(void_math::MAX_MAP_COORD);
        min[h] = self.center[h] - half_h;
        max[h] = self.center[h] + half_h;
        min[v] = self.center[v] - half_v;
        max[v] = self.center[v] + half_v;
        // The depth axis is unbounded: a 2D view sees all the way through.
        min[d] = -void_math::MAX_MAP_COORD;
        max[d] = void_math::MAX_MAP_COORD;
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
        self.center[h] -= dx / self.zoom;
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

    /// A ray from the 3D camera through a pane pixel, for picking.
    pub fn pick_ray(&self, x: f32, y: f32) -> (Vec3, Vec3) {
        let basis = self.angles.vectors();
        let aspect = self.size.0 / self.size.1.max(1.0);
        let half_y = (void_render::vertical_fov(self.fov, aspect) * 0.5).tan();
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
    fn each_2d_view_shows_the_axes_it_should() {
        assert_eq!(ViewportKind::Top.axes(), (0, 1, 2));
        assert_eq!(ViewportKind::Front.axes(), (0, 2, 1));
        assert_eq!(ViewportKind::Side.axes(), (1, 2, 0));
    }

    #[test]
    fn the_view_centre_maps_to_the_pane_centre() {
        for kind in [ViewportKind::Top, ViewportKind::Front, ViewportKind::Side] {
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
        for kind in [ViewportKind::Top, ViewportKind::Front, ViewportKind::Side] {
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
