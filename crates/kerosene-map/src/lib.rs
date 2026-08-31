// SPDX-License-Identifier: MPL-2.0
//! `.keromap` -- Kerosene's editable map source format.
//!
//! This is the analogue of Source's `.vmf`: what Chisel saves, what version
//! control tracks, and what Cleave compiles into a `.kerobsp`. It is KeyValues
//! text, deliberately, so that a map is reviewable in a diff and repairable in
//! a text editor when a tool corrupts it.
//!
//! ```text
//! versioninfo { "formatversion" "1" }
//! world
//! {
//!     "id" "1"
//!     "classname" "worldspawn"
//!     solid
//!     {
//!         "id" "2"
//!         side
//!         {
//!             "id" "3"
//!             "plane" "(0 0 0) (0 64 0) (64 64 0)"
//!             "material" "dev/grid"
//!             "uaxis" "[1 0 0 0] 0.25"
//!             "vaxis" "[0 -1 0 0] 0.25"
//!         }
//!     }
//! }
//! entity { "id" "9" "classname" "info_player_start" "origin" "0 0 32" }
//! ```
//!
//! **Brushes are stored as planes, not as vertices.** A solid is the
//! intersection of its faces' half-spaces. That is the single most important
//! property of the format: it makes a brush convex by construction, it makes
//! it impossible to author a brush with a hole in it, and it is why Cleave can
//! do CSG at all. The cost is that a solid's actual polygons only exist once
//! something computes them -- see [`Solid::windings`].

mod entity;
mod solid;
pub mod texture;

pub use entity::{Connection, Entity, ParseConnectionError};
pub use solid::{Side, Solid, SolidError};
pub use texture::{TextureAxis, default_axes_for_plane, rotate_axes};

use std::collections::HashSet;
use thiserror::Error;
use kerosene_kv::{Entry, KeyValues};
use kerosene_math::{Aabb, Vec3};

/// Format version written into new files.
pub const FORMAT_VERSION: u32 = 1;

/// Default lightmap resolution, in world units per luxel.
///
/// 16 matches Source. It is coarse -- a 512-unit wall gets 32 luxels across --
/// but lightmap memory grows with the square of this, and detail comes from
/// normal maps rather than from baked resolution.
pub const DEFAULT_LIGHTMAP_SCALE: f32 = 16.0;

#[derive(Debug, Error)]
pub enum MapError {
    #[error(transparent)]
    Kv(#[from] kerosene_kv::ParseError),
    #[error("map has no 'world' block")]
    NoWorld,
    #[error("solid {id}: {source}")]
    Solid {
        id: u32,
        #[source]
        source: SolidError,
    },
    #[error("entity {id} ({classname}): {detail}")]
    Entity { id: u32, classname: String, detail: String },
    #[error("{count} objects share id {id}; ids must be unique within a map")]
    DuplicateId { id: u32, count: usize },
}

/// A whole map: the world, plus every point and brush entity in it.
#[derive(Clone, Debug)]
pub struct Map {
    pub format_version: u32,
    pub editor_version: u32,
    /// The `worldspawn` entity. Its solids are the static world geometry that
    /// Cleave feeds into the BSP tree; every other entity's solids become
    /// separate models.
    pub world: Entity,
    pub entities: Vec<Entity>,
    /// Highest id handed out so far, so [`Map::next_id`] never collides.
    next_id: u32,
}

impl Default for Map {
    fn default() -> Self { Self::new() }
}

impl Map {
    /// An empty map with a bare `worldspawn`.
    pub fn new() -> Self {
        let mut world = Entity::new(1, "worldspawn");
        world.set("skyname", "sky_kero");
        Map {
            format_version: FORMAT_VERSION,
            editor_version: 100,
            world,
            entities: Vec::new(),
            next_id: 2,
        }
    }

    /// Allocate an id that nothing in this map is using.
    pub fn next_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Every entity including `worldspawn`.
    pub fn all_entities(&self) -> impl Iterator<Item = &Entity> {
        std::iter::once(&self.world).chain(self.entities.iter())
    }

    /// Every solid in the map, paired with the entity that owns it.
    pub fn all_solids(&self) -> impl Iterator<Item = (&Entity, &Solid)> {
        self.all_entities().flat_map(|e| e.solids.iter().map(move |s| (e, s)))
    }

    pub fn solid_count(&self) -> usize {
        self.all_entities().map(|e| e.solids.len()).sum()
    }

    /// Entities with the given classname.
    pub fn by_classname<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a Entity> + 'a {
        self.all_entities().filter(move |e| e.classname() == class)
    }

    /// Entity carrying a given `targetname`, which is how I/O connections
    /// address their targets.
    pub fn by_targetname<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Entity> + 'a {
        self.all_entities().filter(move |e| e.get("targetname") == Some(name))
    }

    /// Bounding box of every brush in the map.
    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for (_, solid) in self.all_solids() {
            b = b.union(&solid.bounds());
        }
        b
    }

    // ---- parsing ---------------------------------------------------------

    pub fn parse(text: &str) -> Result<Map, MapError> {
        let root = KeyValues::parse(text)?;

        let (format_version, editor_version) = match root.block("versioninfo") {
            Some(v) => (v.get_or("formatversion", FORMAT_VERSION), v.get_or("editorversion", 100u32)),
            None => (FORMAT_VERSION, 100),
        };

        let world_kv = root.block("world").ok_or(MapError::NoWorld)?;
        let world = Entity::from_kv(world_kv)?;

        let mut entities = Vec::new();
        for kv in root.blocks("entity") {
            entities.push(Entity::from_kv(kv)?);
        }

        let mut map = Map {
            format_version,
            editor_version,
            world,
            entities,
            next_id: 1,
        };
        map.reseed_next_id();
        map.check_unique_ids()?;
        Ok(map)
    }

    /// Point `next_id` past everything already in the map.
    ///
    /// Called after load so that ids handed out by the editor cannot collide
    /// with ids that came from the file.
    pub fn reseed_next_id(&mut self) {
        let mut max = 0;
        for e in self.all_entities() {
            max = max.max(e.id);
            for s in &e.solids {
                max = max.max(s.id);
                for side in &s.sides { max = max.max(side.id); }
            }
        }
        self.next_id = max + 1;
    }

    /// Reject duplicate ids.
    ///
    /// Ids address objects across undo history, entity I/O and editor
    /// selection; two objects sharing one is a corruption that produces
    /// baffling behaviour much later, so it is caught at load.
    fn check_unique_ids(&self) -> Result<(), MapError> {
        let mut seen: HashSet<u32> = HashSet::new();
        let mut report = |id: u32| -> Result<(), MapError> {
            if !seen.insert(id) {
                return Err(MapError::DuplicateId { id, count: 2 });
            }
            Ok(())
        };
        for e in self.all_entities() {
            report(e.id)?;
            for s in &e.solids {
                report(s.id)?;
                for side in &s.sides { report(side.id)?; }
            }
        }
        Ok(())
    }

    /// Check every solid is well formed, collecting all problems at once.
    ///
    /// Reporting every bad brush in one pass rather than stopping at the first
    /// is the difference between one fix-compile cycle and twenty.
    pub fn validate(&self) -> Vec<MapError> {
        let mut problems = Vec::new();
        for (_, solid) in self.all_solids() {
            if let Err(source) = solid.validate() {
                problems.push(MapError::Solid { id: solid.id, source });
            }
        }
        for e in self.entities.iter() {
            if e.classname().is_empty() {
                problems.push(MapError::Entity {
                    id: e.id,
                    classname: String::new(),
                    detail: "entity has no classname".into(),
                });
            }
            if e.solids.is_empty() && !e.has("origin") {
                problems.push(MapError::Entity {
                    id: e.id,
                    classname: e.classname().to_string(),
                    detail: "point entity has no origin".into(),
                });
            }
        }
        problems
    }

    // ---- writing ---------------------------------------------------------

    pub fn to_text(&self) -> String {
        let mut root = KeyValues::new("");
        let mut vi = KeyValues::new("versioninfo");
        vi.push_value("editorversion", self.editor_version);
        vi.push_value("formatversion", self.format_version);
        root.push_block(vi);
        root.push_block(self.world.to_kv("world"));
        for e in &self.entities {
            root.push_block(e.to_kv("entity"));
        }
        root.to_document()
    }

    /// Add a brush entity or point entity, assigning it a fresh id.
    pub fn add_entity(&mut self, classname: &str) -> &mut Entity {
        let id = self.next_id();
        self.entities.push(Entity::new(id, classname));
        self.entities.last_mut().expect("just pushed")
    }

    /// Add a solid to the world, assigning fresh ids to it and its sides.
    pub fn add_world_solid(&mut self, mut solid: Solid) -> u32 {
        solid.id = self.next_id();
        for side in &mut solid.sides { side.id = self.next_id(); }
        let id = solid.id;
        self.world.solids.push(solid);
        id
    }

    /// Remove an entity by id, reporting whether it was there.
    pub fn remove_entity(&mut self, id: u32) -> bool {
        let before = self.entities.len();
        self.entities.retain(|e| e.id != id);
        before != self.entities.len()
    }

    /// Find a solid anywhere in the map by id.
    pub fn find_solid(&self, id: u32) -> Option<&Solid> {
        self.all_solids().map(|(_, s)| s).find(|s| s.id == id)
    }

    pub fn find_solid_mut(&mut self, id: u32) -> Option<&mut Solid> {
        std::iter::once(&mut self.world)
            .chain(self.entities.iter_mut())
            .flat_map(|e| e.solids.iter_mut())
            .find(|s| s.id == id)
    }
}

/// Helper used by the KeyValues readers to pull an id, defaulting to 0.
pub(crate) fn read_id(kv: &KeyValues) -> u32 { kv.get_or("id", 0u32) }

/// Split a KeyValues block into its plain properties, skipping known
/// sub-blocks that have dedicated handling.
pub(crate) fn plain_properties(kv: &KeyValues) -> Vec<(String, String)> {
    kv.entries
        .iter()
        .filter_map(|e| match e {
            Entry::Pair(k, v) => Some((k.clone(), v.clone())),
            Entry::Block(_) => None,
        })
        .collect()
}

/// Format a `Vec3` the way map files spell it.
pub(crate) fn vec3_to_kv(v: Vec3) -> String {
    use kerosene_kv::format_float as f;
    format!("{} {} {}", f(v.x), f(v.y), f(v.z))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
versioninfo { "editorversion" "100" "formatversion" "1" }
world
{
    "id" "1"
    "classname" "worldspawn"
    "skyname" "sky_kero"
    solid
    {
        "id" "2"
        side { "id" "3"  "plane" "(64 0 64) (0 0 64) (0 64 64)" "material" "dev/grid" }
        side { "id" "4"  "plane" "(0 64 0) (0 0 0) (64 0 0)"     "material" "dev/grid" }
        side { "id" "5"  "plane" "(64 64 0) (64 0 0) (64 0 64)"  "material" "dev/grid" }
        side { "id" "6"  "plane" "(0 0 64) (0 0 0) (0 64 0)"     "material" "dev/grid" }
        side { "id" "7"  "plane" "(0 64 64) (0 64 0) (64 64 0)"  "material" "dev/grid" }
        side { "id" "8"  "plane" "(64 0 0) (0 0 0) (0 0 64)"     "material" "dev/grid" }
    }
}
entity
{
    "id" "9"
    "classname" "info_player_start"
    "origin" "32 32 80"
}
entity
{
    "id" "10"
    "classname" "func_door"
    "targetname" "door1"
    connections { "OnFullyOpen" "relay1,Trigger,,0.5,-1" }
}
"#;

    #[test]
    fn parses_world_and_entities() {
        let map = Map::parse(SAMPLE).unwrap();
        assert_eq!(map.world.classname(), "worldspawn");
        assert_eq!(map.world.get("skyname"), Some("sky_kero"));
        assert_eq!(map.world.solids.len(), 1);
        assert_eq!(map.entities.len(), 2);
        assert_eq!(map.by_classname("info_player_start").count(), 1);
    }

    #[test]
    fn round_trips_through_text() {
        let map = Map::parse(SAMPLE).unwrap();
        let again = Map::parse(&map.to_text()).unwrap();
        assert_eq!(map.world.solids.len(), again.world.solids.len());
        assert_eq!(map.entities.len(), again.entities.len());
        assert_eq!(again.entities[1].connections.len(), 1);
        assert_eq!(again.to_text(), map.to_text(), "writing must be stable");
    }

    #[test]
    fn a_missing_world_is_an_error() {
        assert!(matches!(Map::parse("versioninfo { }"), Err(MapError::NoWorld)));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let bad = SAMPLE.replace("\"id\" \"9\"", "\"id\" \"1\"");
        assert!(matches!(Map::parse(&bad), Err(MapError::DuplicateId { id: 1, .. })));
    }

    #[test]
    fn new_ids_never_collide_with_loaded_ones() {
        let mut map = Map::parse(SAMPLE).unwrap();
        let fresh = map.next_id();
        assert!(fresh > 10, "next id {fresh} must clear every id in the file");
        assert!(map.all_entities().all(|e| e.id != fresh));
    }

    #[test]
    fn bounds_cover_the_world_brush() {
        let map = Map::parse(SAMPLE).unwrap();
        let b = map.bounds();
        assert_eq!(b.min, Vec3::ZERO);
        assert_eq!(b.max, Vec3::splat(64.0));
    }

    #[test]
    fn validate_reports_every_bad_brush_not_just_the_first() {
        // Two solids with too few faces to enclose anything.
        let src = r#"
world { "id" "1" "classname" "worldspawn"
  solid { "id" "2" side { "id" "3" "plane" "(0 0 0) (0 64 0) (64 64 0)" "material" "x" } }
  solid { "id" "4" side { "id" "5" "plane" "(0 0 0) (0 64 0) (64 64 0)" "material" "x" } }
}"#;
        let map = Map::parse(src).unwrap();
        assert_eq!(map.validate().len(), 2);
    }

    #[test]
    fn editing_helpers_keep_ids_unique() {
        let mut map = Map::new();
        let cube = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        map.add_world_solid(cube.clone());
        map.add_world_solid(cube);
        map.add_entity("light");
        let mut ids = Vec::new();
        for e in map.all_entities() {
            ids.push(e.id);
            for s in &e.solids {
                ids.push(s.id);
                ids.extend(s.sides.iter().map(|x| x.id));
            }
        }
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "ids collided: {ids:?}");
    }
}
