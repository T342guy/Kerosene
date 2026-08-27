//! Reading the `.prt` portal graph Cleave writes.
//!
//! ```text
//! VPRT1
//! <cluster count>
//! <portal count>
//! <points> <cluster a> <cluster b> (x y z) (x y z) ...
//! ```
//!
//! Each line describes one portal joining two clusters. Visibility is
//! directional, so every line becomes *two* directed portals: one looking from
//! `a` into `b`, one the other way. The winding's own plane normal points
//! toward `a` (Cleave writes `a` as the front side), so a portal looking into
//! `b` carries the flipped plane -- a portal's plane always points the way it
//! looks.

use thiserror::Error;
use void_math::{Plane, Vec3, Winding};

#[derive(Debug, Error)]
pub enum PrtError {
    #[error("not a portal file (expected a VPRT1 header)")]
    BadHeader,
    #[error("line {line}: {detail}")]
    Malformed { line: usize, detail: String },
    #[error("header promises {expected} portals but the file holds {found}")]
    CountMismatch { expected: usize, found: usize },
}

/// One direction of one portal.
#[derive(Clone, Debug)]
pub struct VisPortal {
    pub winding: Winding,
    /// Points into [`VisPortal::into_cluster`] -- the direction this portal looks.
    pub plane: Plane,
    /// The cluster this portal leads into.
    pub into_cluster: usize,
    /// The cluster this portal sits in.
    pub from_cluster: usize,
    /// Bounding sphere, used to skip clipping work that cannot matter.
    pub center: Vec3,
    pub radius: f32,
}

pub struct PortalGraph {
    pub clusters: usize,
    /// Directed portals; `2i` and `2i + 1` are the two directions of one wall.
    pub portals: Vec<VisPortal>,
    /// Portal indices leaving each cluster.
    pub by_cluster: Vec<Vec<usize>>,
}

impl PortalGraph {
    pub fn parse(text: &str) -> Result<PortalGraph, PrtError> {
        let mut lines = text.lines().enumerate();

        let (_, header) = lines.next().ok_or(PrtError::BadHeader)?;
        if header.trim() != "VPRT1" { return Err(PrtError::BadHeader); }

        let parse_usize = |lines: &mut std::iter::Enumerate<std::str::Lines>| -> Result<usize, PrtError> {
            let (n, l) = lines.next().ok_or(PrtError::BadHeader)?;
            l.trim().parse().map_err(|_| PrtError::Malformed {
                line: n + 1,
                detail: format!("expected a count, found {l:?}"),
            })
        };
        let clusters = parse_usize(&mut lines)?;
        let expected = parse_usize(&mut lines)?;

        let mut portals = Vec::with_capacity(expected * 2);
        let mut found = 0usize;

        for (n, line) in lines {
            let line = line.trim();
            if line.is_empty() { continue; }
            found += 1;

            let head: Vec<&str> = line.split('(').next().unwrap_or("").split_whitespace().collect();
            if head.len() != 3 {
                return Err(PrtError::Malformed {
                    line: n + 1,
                    detail: "expected <points> <cluster a> <cluster b>".into(),
                });
            }
            let bad = |what: &str| PrtError::Malformed {
                line: n + 1,
                detail: format!("unreadable {what}"),
            };
            let count: usize = head[0].parse().map_err(|_| bad("point count"))?;
            let a: usize = head[1].parse().map_err(|_| bad("cluster a"))?;
            let b: usize = head[2].parse().map_err(|_| bad("cluster b"))?;
            if a >= clusters || b >= clusters {
                return Err(PrtError::Malformed {
                    line: n + 1,
                    detail: format!("cluster {a} or {b} is outside the declared {clusters}"),
                });
            }

            let mut points = Vec::with_capacity(count);
            for group in line.split('(').skip(1) {
                let Some(close) = group.find(')') else { continue };
                let nums: Vec<f32> = group[..close]
                    .split_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect();
                if nums.len() != 3 {
                    return Err(PrtError::Malformed { line: n + 1, detail: "malformed point".into() });
                }
                points.push(Vec3::new(nums[0], nums[1], nums[2]));
            }
            if points.len() != count {
                return Err(PrtError::Malformed {
                    line: n + 1,
                    detail: format!("declared {count} points but listed {}", points.len()),
                });
            }

            let winding = Winding::new(points);
            let Some(plane) = winding.plane() else {
                return Err(PrtError::Malformed { line: n + 1, detail: "degenerate portal".into() });
            };

            // The plane points toward cluster `a`, so looking into `b` means
            // looking along the flipped normal.
            portals.push(make(winding.clone(), plane.flipped(), b, a));
            portals.push(make(winding.reversed(), plane, a, b));
        }

        if found != expected {
            return Err(PrtError::CountMismatch { expected, found });
        }

        let mut by_cluster = vec![Vec::new(); clusters];
        for (i, p) in portals.iter().enumerate() {
            by_cluster[p.from_cluster].push(i);
        }

        Ok(PortalGraph { clusters, portals, by_cluster })
    }

    pub fn portal_count(&self) -> usize { self.portals.len() }
}

fn make(winding: Winding, plane: Plane, into_cluster: usize, from_cluster: usize) -> VisPortal {
    let center = winding.center();
    let radius = winding
        .points
        .iter()
        .map(|p| (*p - center).length())
        .fold(0.0f32, f32::max);
    VisPortal { winding, plane, into_cluster, from_cluster, center, radius }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_ROOMS: &str = "VPRT1\n2\n1\n4 0 1 (0 0 0) (0 64 0) (0 64 64) (0 0 64)\n";

    #[test]
    fn a_portal_becomes_two_directed_portals() {
        let g = PortalGraph::parse(TWO_ROOMS).unwrap();
        assert_eq!(g.clusters, 2);
        assert_eq!(g.portal_count(), 2);
        assert_eq!(g.portals[0].from_cluster, 0);
        assert_eq!(g.portals[0].into_cluster, 1);
        assert_eq!(g.portals[1].from_cluster, 1);
        assert_eq!(g.portals[1].into_cluster, 0);
    }

    #[test]
    fn each_direction_faces_the_way_it_looks() {
        // The two directions must have opposite planes, or the flow algorithm
        // sees through walls in one direction and nothing in the other.
        let g = PortalGraph::parse(TWO_ROOMS).unwrap();
        let a = g.portals[0].plane.normal;
        let b = g.portals[1].plane.normal;
        assert!((a + b).length() < 1e-5, "{a:?} and {b:?} are not opposites");
    }

    #[test]
    fn portals_are_indexed_by_the_cluster_they_leave() {
        let g = PortalGraph::parse(TWO_ROOMS).unwrap();
        assert_eq!(g.by_cluster[0], vec![0]);
        assert_eq!(g.by_cluster[1], vec![1]);
    }

    #[test]
    fn the_bounding_sphere_covers_the_winding() {
        let g = PortalGraph::parse(TWO_ROOMS).unwrap();
        let p = &g.portals[0];
        for pt in &p.winding.points {
            assert!((*pt - p.center).length() <= p.radius + 1e-4);
        }
    }

    #[test]
    fn malformed_files_are_rejected_with_a_line_number() {
        assert!(matches!(PortalGraph::parse("nonsense"), Err(PrtError::BadHeader)));
        assert!(matches!(
            PortalGraph::parse("VPRT1\n2\n1\n4 0 1 (0 0 0)\n"),
            Err(PrtError::Malformed { line: 4, .. })
        ));
        assert!(matches!(
            PortalGraph::parse("VPRT1\n2\n5\n4 0 1 (0 0 0) (0 64 0) (0 64 64) (0 0 64)\n"),
            Err(PrtError::CountMismatch { expected: 5, found: 1 })
        ));
        assert!(matches!(
            PortalGraph::parse("VPRT1\n2\n1\n4 0 9 (0 0 0) (0 64 0) (0 64 64) (0 0 64)\n"),
            Err(PrtError::Malformed { .. })
        ));
    }

    #[test]
    fn an_empty_graph_is_valid() {
        let g = PortalGraph::parse("VPRT1\n3\n0\n").unwrap();
        assert_eq!(g.clusters, 3);
        assert_eq!(g.portal_count(), 0);
    }
}
