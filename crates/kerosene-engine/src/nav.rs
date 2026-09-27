// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Where an NPC may walk, and how to get there.
//!
//! Cleave writes `maps/<name>.kwalk` beside the compiled map: the floor
//! faces a character can stand on, each marked allow or avoid. The engine
//! loads it with the map and builds the graph over it once, so a game asks
//! for a path with one call and nothing is worked out per request that
//! could have been worked out at load.

use crate::engine::Engine;
use kerosene_math::Vec3;
use kerosene_walk::{NavGraph, Walkmap};

/// How far above or below a face a point may be and still count as on it,
/// when a game asks from a character's feet.
pub const STAND_TOLERANCE: f32 = 32.0;

/// A map's walk data and the graph over it.
pub struct Nav {
    /// The faces.
    pub walkmap: Walkmap,
    /// Which faces touch.
    pub graph: NavGraph,
}

impl Nav {
    /// Build the graph over a walkmap.
    pub fn new(walkmap: Walkmap) -> Nav {
        let graph = NavGraph::build(&walkmap);
        Nav { walkmap, graph }
    }

    /// Waypoints from `start` to `goal` over walkable faces, both ends
    /// included; `None` when either end is off the walkable floor or no
    /// route joins them.
    pub fn find_path(&self, start: Vec3, goal: Vec3) -> Option<Vec<Vec3>> {
        self.graph
            .find_path(&self.walkmap, start, goal, STAND_TOLERANCE)
    }

    /// Whether a character could stand at `point`.
    pub fn walkable(&self, point: Vec3) -> bool {
        self.walkmap.walkable(point, STAND_TOLERANCE)
    }
}

impl Engine {
    /// The current map's walk data, when it has any: Cleave writes it for
    /// every map with walkable floor, and a map compiled without it has
    /// none.
    pub fn nav(&self) -> Option<&Nav> {
        self.level.as_ref()?.nav.as_ref()
    }

    /// Waypoints for a character from `start` to `goal`. See
    /// [`Nav::find_path`].
    pub fn find_path(&self, start: Vec3, goal: Vec3) -> Option<Vec<Vec3>> {
        self.nav()?.find_path(start, goal)
    }

    /// Read `maps/<name>.kwalk`, if the map has one.
    pub(crate) fn load_nav(&mut self, name: &str) -> Option<Nav> {
        let path = format!("maps/{name}.kwalk");
        let bytes = self.vfs.read(&path).ok()?;
        match Walkmap::parse(&bytes) {
            Ok(walkmap) => {
                let nav = Nav::new(walkmap);
                self.console.developer(format!(
                    "  walk: {} faces, {} links",
                    nav.graph.node_count(),
                    nav.graph.edge_count()
                ));
                Some(nav)
            }
            Err(e) => {
                self.console
                    .warn(format!("{path}: {e}; no navigation on this map"));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineConfig;

    #[test]
    fn the_base_room_has_a_path_across_its_floor() {
        let mut engine = Engine::new(&EngineConfig::default());
        engine.load_map(crate::base::FALLBACK_MAP).unwrap();
        let nav = engine.nav().expect("the room was compiled with walk data");
        assert!(nav.walkable(Vec3::new(96.0, 256.0, 0.0)));
        // Five faces -- the floor split around the plinth, and its top --
        // that meet at T-junctions, not whole edges.
        assert!(nav.graph.edge_count() > 0, "the floor's faces are linked");
        let path = engine
            .find_path(Vec3::new(96.0, 256.0, 0.0), Vec3::new(700.0, 100.0, 0.0))
            .expect("one room, one floor");
        assert_eq!(path.first(), Some(&Vec3::new(96.0, 256.0, 0.0)));
        assert_eq!(path.last(), Some(&Vec3::new(700.0, 100.0, 0.0)));
        assert_eq!(
            engine.find_path(Vec3::new(96.0, 256.0, 0.0), Vec3::new(5000.0, 0.0, 0.0)),
            None
        );
    }
}
