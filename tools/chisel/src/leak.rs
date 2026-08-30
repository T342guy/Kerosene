// SPDX-License-Identifier: LGPL-3.0-or-later
//! Loading and showing a leak trace.
//!
//! When a map is not sealed, Cleave writes a `.keroleak` beside it: the route
//! a flood fill took from an entity out into the void. One `x y z` per line,
//! and the trace is only useful if something draws it -- a coordinate list
//! does not tell you which wall has the gap, and finding a one-unit hole in a
//! large map by eye is not a reasonable thing to ask of anyone.
//!
//! So Chisel loads it after a compile and draws it through every pane. Follow
//! the line to the wall it passes through.

use std::path::Path;
use kerosene_math::Vec3;

/// A loaded leak trace.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LeakTrace {
    pub points: Vec<Vec3>,
}

impl LeakTrace {
    /// Parse the point file. Blank lines and comments are skipped, and a line
    /// that is not three numbers is skipped rather than failing the load: a
    /// partly readable trace still points at the hole.
    pub fn parse(text: &str) -> LeakTrace {
        let points = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with('#'))
            .filter_map(|line| {
                let mut n = line.split_whitespace().filter_map(|v| v.parse::<f32>().ok());
                Some(Vec3::new(n.next()?, n.next()?, n.next()?))
            })
            .collect();
        LeakTrace { points }
    }

    /// Load the trace beside a compiled map, if there is one.
    ///
    /// Returns `None` both when the map is sealed and when the file cannot be
    /// read, because those are the same thing from here: nothing to draw.
    pub fn beside(map: &Path) -> Option<LeakTrace> {
        let text = std::fs::read_to_string(map.with_extension("keroleak")).ok()?;
        let trace = LeakTrace::parse(&text);
        (trace.points.len() >= 2).then_some(trace)
    }

    pub fn is_empty(&self) -> bool { self.points.len() < 2 }

    /// Where the leak starts -- the entity that could see out.
    pub fn origin(&self) -> Option<Vec3> { self.points.first().copied() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trace_is_a_list_of_points() {
        let trace = LeakTrace::parse("256 256 192\n256 256 128\n520 256 0\n");
        assert_eq!(trace.points.len(), 3);
        assert_eq!(trace.origin(), Some(Vec3::new(256.0, 256.0, 192.0)));
        assert!(!trace.is_empty());
    }

    #[test]
    fn negative_and_fractional_coordinates_survive() {
        let trace = LeakTrace::parse("-24 256 -4.5\n0 0 0\n");
        assert_eq!(trace.points[0], Vec3::new(-24.0, 256.0, -4.5));
    }

    #[test]
    fn blank_lines_and_comments_are_ignored() {
        let trace = LeakTrace::parse("// from the player start\n\n1 2 3\n\n# and out\n4 5 6\n");
        assert_eq!(trace.points.len(), 2);
    }

    #[test]
    fn a_malformed_line_does_not_lose_the_rest_of_the_trace() {
        // A partly readable trace still points at the hole, which is the whole
        // job. Failing the load would leave a person with nothing.
        let trace = LeakTrace::parse("1 2 3\nnot a point\n4 5\n7 8 9\n");
        assert_eq!(trace.points, vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(7.0, 8.0, 9.0)]);
    }

    #[test]
    fn one_point_is_not_a_line_and_counts_as_nothing_to_draw() {
        assert!(LeakTrace::parse("1 2 3\n").is_empty());
        assert!(LeakTrace::parse("").is_empty());
    }

    #[test]
    fn a_sealed_map_has_no_trace_beside_it() {
        let dir = std::env::temp_dir().join(format!("chisel-leak-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let map = dir.join("sealed.kerobsp");
        std::fs::write(&map, "").unwrap();
        assert_eq!(LeakTrace::beside(&map), None);

        std::fs::write(map.with_extension("keroleak"), "0 0 0\n64 0 0\n").unwrap();
        assert!(LeakTrace::beside(&map).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
