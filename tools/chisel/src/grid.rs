//! The editing grid.
//!
//! Everything in a brush-based editor snaps to a grid, and the grid is a power
//! of two. That is not arbitrary aesthetics: brush vertices land on exact
//! binary fractions, so the planes derived from them are exact, and coplanar
//! faces in different brushes intern to the same plane instead of a hair's
//! width apart. A level built off-grid produces a worse BSP, a slower compile,
//! and visible cracks.

use void_math::Vec3;

/// Grid sizes, in inches. Powers of two from a sixteenth of an inch up to 512.
pub const SIZES: [f32; 14] = [
    0.0625, 0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0,
];

/// The grid a new document starts on.
///
/// 16 units is a quarter of a player's width and an eighth of their height,
/// which is fine enough to detail with and coarse enough that a room stays
/// aligned.
pub const DEFAULT_SIZE: f32 = 16.0;

#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub size: f32,
    pub visible: bool,
    pub snap: bool,
}

impl Default for Grid {
    fn default() -> Self { Grid { size: DEFAULT_SIZE, visible: true, snap: true } }
}

impl Grid {
    /// Snap one coordinate.
    pub fn snap_value(&self, v: f32) -> f32 {
        if !self.snap || self.size <= 0.0 { return v; }
        (v / self.size).round() * self.size
    }

    pub fn snap_point(&self, p: Vec3) -> Vec3 {
        Vec3::new(self.snap_value(p.x), self.snap_value(p.y), self.snap_value(p.z))
    }

    /// Snap outward, so a box never shrinks below the geometry it was drawn
    /// around. Dragging out a brush and having it come back smaller than the
    /// rubber band is maddening.
    pub fn snap_box(&self, min: Vec3, max: Vec3) -> (Vec3, Vec3) {
        if !self.snap || self.size <= 0.0 { return (min, max); }
        let floor = |v: f32| (v / self.size).floor() * self.size;
        let ceil = |v: f32| (v / self.size).ceil() * self.size;
        let lo = Vec3::new(floor(min.x), floor(min.y), floor(min.z));
        let mut hi = Vec3::new(ceil(max.x), ceil(max.y), ceil(max.z));
        // A degenerate axis still has to end up at least one grid unit thick,
        // or the brush encloses no volume and the compiler drops it.
        for axis in 0..3 {
            if (hi[axis] - lo[axis]).abs() < self.size * 0.5 {
                hi[axis] = lo[axis] + self.size;
            }
        }
        (lo, hi)
    }

    /// Step to a coarser grid.
    pub fn coarser(&mut self) {
        let index = self.index();
        self.size = SIZES[(index + 1).min(SIZES.len() - 1)];
    }

    /// Step to a finer grid.
    pub fn finer(&mut self) {
        let index = self.index();
        self.size = SIZES[index.saturating_sub(1)];
    }

    fn index(&self) -> usize {
        SIZES
            .iter()
            .position(|&s| (s - self.size).abs() < 1e-4)
            .unwrap_or(SIZES.iter().position(|&s| s >= self.size).unwrap_or(0))
    }

    /// How far apart grid lines should be drawn at a given zoom.
    ///
    /// Returns `None` when the grid is finer than a couple of pixels: drawing
    /// it then is a grey wash that hides the geometry rather than helping
    /// place it.
    pub fn draw_spacing(&self, pixels_per_unit: f32) -> Option<f32> {
        if !self.visible { return None; }
        let mut spacing = self.size;
        while spacing * pixels_per_unit < 4.0 {
            spacing *= 2.0;
            if spacing > SIZES[SIZES.len() - 1] { return None; }
        }
        Some(spacing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_rounds_to_the_nearest_line() {
        let grid = Grid { size: 16.0, ..Default::default() };
        assert_eq!(grid.snap_value(0.0), 0.0);
        assert_eq!(grid.snap_value(7.0), 0.0);
        assert_eq!(grid.snap_value(9.0), 16.0);
        assert_eq!(grid.snap_value(-9.0), -16.0);
        assert_eq!(grid.snap_value(64.0), 64.0);
    }

    #[test]
    fn snapping_can_be_turned_off() {
        let grid = Grid { size: 16.0, snap: false, ..Default::default() };
        assert_eq!(grid.snap_value(7.3), 7.3);
    }

    #[test]
    fn boxes_snap_outward_so_a_brush_never_shrinks() {
        let grid = Grid { size: 16.0, ..Default::default() };
        let (lo, hi) = grid.snap_box(Vec3::new(1.0, 1.0, 1.0), Vec3::new(30.0, 30.0, 30.0));
        assert_eq!(lo, Vec3::ZERO);
        assert_eq!(hi, Vec3::splat(32.0));
    }

    #[test]
    fn a_flat_drag_still_produces_a_solid_brush() {
        // Dragging out a zero-thickness box must not create a brush that
        // encloses no volume; the compiler would simply drop it.
        let grid = Grid { size: 16.0, ..Default::default() };
        let (lo, hi) = grid.snap_box(Vec3::new(0.0, 0.0, 0.0), Vec3::new(64.0, 64.0, 0.0));
        assert!(hi.z - lo.z >= 16.0, "z was {} to {}", lo.z, hi.z);
    }

    #[test]
    fn grid_sizes_step_through_powers_of_two() {
        let mut grid = Grid { size: 16.0, ..Default::default() };
        grid.coarser();
        assert_eq!(grid.size, 32.0);
        grid.finer();
        grid.finer();
        assert_eq!(grid.size, 8.0);
    }

    #[test]
    fn stepping_stops_at_the_ends_rather_than_wrapping() {
        let mut grid = Grid { size: SIZES[0], ..Default::default() };
        grid.finer();
        assert_eq!(grid.size, SIZES[0]);

        grid.size = SIZES[SIZES.len() - 1];
        grid.coarser();
        assert_eq!(grid.size, SIZES[SIZES.len() - 1]);
    }

    #[test]
    fn the_drawn_grid_coarsens_rather_than_becoming_a_grey_wash() {
        let grid = Grid { size: 1.0, ..Default::default() };
        // Zoomed right out, a 1-unit grid would be sub-pixel.
        let spacing = grid.draw_spacing(0.05).expect("some spacing");
        assert!(spacing >= 64.0, "spacing {spacing} is still too fine to draw");
        // Zoomed in, it draws at its real size.
        assert_eq!(grid.draw_spacing(20.0), Some(1.0));
    }

    #[test]
    fn a_hidden_grid_draws_nothing() {
        let grid = Grid { visible: false, ..Default::default() };
        assert_eq!(grid.draw_spacing(10.0), None);
    }
}
