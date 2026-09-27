// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Lines a game draws in the world to see what its code is doing: a path an
//! AI means to walk, where a shot went, a trigger's reach.
//!
//! Drawn over everything by the same pass as `phys_debug`, and only while
//! `r_debugdraw` is on (it is, by default; a shipped game turns it off in
//! its config, or simply stops drawing). A line lasts for a number of
//! seconds of game time, so a line drawn once on a shot stays long enough to
//! look at; zero seconds is one frame, for something redrawn every tick.

use crate::engine::Engine;
use crate::physics::{BOX_EDGES, DebugLine};
use kerosene_math::{Aabb, Vec3};

/// A line and when it goes.
#[derive(Clone, Copy, Debug)]
struct Timed {
    line: DebugLine,
    /// Game time after which it is not drawn; `None` for one frame.
    until: Option<f32>,
}

/// The lines a game has asked for.
#[derive(Default)]
pub struct DebugDraw {
    lines: Vec<Timed>,
}

impl DebugDraw {
    /// The lines to draw at game time `now`, forgetting the ones that are
    /// done: those past their time, and every one-frame line.
    pub(crate) fn take(&mut self, now: f32) -> Vec<DebugLine> {
        let out = self
            .lines
            .iter()
            .filter(|t| t.until.is_none_or(|until| now <= until))
            .map(|t| t.line)
            .collect();
        self.lines
            .retain(|t| t.until.is_some_and(|until| now <= until));
        out
    }

    /// How many lines are waiting to be drawn.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether nothing is waiting to be drawn.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

impl Engine {
    /// Draw a line from `a` to `b` for `seconds` of game time; zero for one
    /// frame. `color` is RGB, 0 to 1.
    pub fn debug_line(&mut self, a: Vec3, b: Vec3, color: [f32; 3], seconds: f32) {
        // A game's debug drawing costs nothing when it is off, however much
        // of it there is.
        if !self.console.bool("r_debugdraw") {
            return;
        }
        let until = (seconds > 0.0).then_some(self.time + seconds);
        self.debug_draw.lines.push(Timed {
            line: DebugLine { a, b, color },
            until,
        });
    }

    /// Draw the edges of a box. See [`debug_line`](Engine::debug_line).
    pub fn debug_box(&mut self, bounds: Aabb, color: [f32; 3], seconds: f32) {
        // BOX_EDGES counts corners in binary -- bit 0 is x, 1 is y, 2 is z --
        // which is not `Aabb::corners`'s order.
        let (lo, hi) = (bounds.min, bounds.max);
        let corners: [Vec3; 8] = std::array::from_fn(|i| {
            Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            )
        });
        for (i, j) in BOX_EDGES {
            self.debug_line(corners[i], corners[j], color, seconds);
        }
    }

    /// Draw a small three-axis cross at a point, `size` across. See
    /// [`debug_line`](Engine::debug_line).
    pub fn debug_point(&mut self, at: Vec3, size: f32, color: [f32; 3], seconds: f32) {
        let h = size / 2.0;
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            self.debug_line(at - axis * h, at + axis * h, color, seconds);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineConfig;
    use crate::input::InputState;

    #[test]
    fn a_line_lasts_its_time_and_a_frame_line_one_frame() {
        let mut engine = Engine::new(&EngineConfig::default());
        engine.debug_line(Vec3::ZERO, Vec3::X, [1.0, 0.0, 0.0], 0.5);
        engine.debug_point(Vec3::ZERO, 8.0, [0.0, 1.0, 0.0], 0.0);
        assert_eq!(engine.debug_draw.take(engine.time).len(), 4);
        // The cross was for one frame; the line is still there.
        assert_eq!(engine.debug_draw.take(engine.time).len(), 1);
        for _ in 0..64 {
            engine.tick(1.0 / 64.0, &InputState::default());
        }
        assert!(engine.debug_draw.take(engine.time).is_empty());
        assert!(engine.debug_draw.is_empty());
    }

    #[test]
    fn nothing_is_kept_while_it_is_turned_off() {
        let mut engine = Engine::new(&EngineConfig::default());
        engine.console.execute_user("r_debugdraw 0");
        engine.debug_box(
            Aabb::new(Vec3::ZERO, Vec3::splat(16.0)),
            [1.0, 1.0, 1.0],
            10.0,
        );
        assert!(engine.debug_draw.is_empty());
    }
}
