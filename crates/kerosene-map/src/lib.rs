// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
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

pub mod editor;
mod entity;
pub mod mesh;
mod ops;
mod solid;
pub mod texture;
mod walk;

pub use editor::{Cordon, EditorData, Group, ObjectId, VisGroup};
pub use entity::{Connection, Entity, ParseConnectionError};
pub use mesh::{Mesh, MeshError, MeshFace, polygon_normal};
pub use ops::bounds_of;
pub use solid::{Side, Solid, SolidError};
pub use texture::{TextureAxis, default_axes_for_plane, rotate_axes};
pub use walk::WalkmapRule;

use kerosene_kv::{Entry, KeyValues};
use kerosene_math::{Aabb, Vec3};
use std::collections::HashMap;
use thiserror::Error;

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
    #[error("mesh {id}: {source}")]
    Mesh {
        id: u32,
        #[source]
        source: MeshError,
    },
    #[error("entity {id} ({classname}): {detail}")]
    Entity {
        id: u32,
        classname: String,
        detail: String,
    },
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
    /// The editor's visgroup tree. Objects name their groups by id in their
    /// [`EditorData`].
    pub visgroups: Vec<VisGroup>,
    /// The editor's groups. Flat; membership is by id on each object.
    pub groups: Vec<Group>,
    /// A box the editor and compiler restrict themselves to, when active.
    pub cordon: Option<Cordon>,
    /// Highest id handed out so far, so [`Map::next_id`] never collides.
    next_id: u32,
}

impl Default for Map {
    fn default() -> Self {
        Self::new()
    }
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
            visgroups: Vec::new(),
            groups: Vec::new(),
            cordon: None,
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
        self.all_entities()
            .flat_map(|e| e.solids.iter().map(move |s| (e, s)))
    }

    /// Every mesh in the map, with the entity that owns it.
    pub fn all_meshes(&self) -> impl Iterator<Item = (&Entity, &Mesh)> {
        self.all_entities()
            .flat_map(|e| e.meshes.iter().map(move |m| (e, m)))
    }

    pub fn find_mesh(&self, id: u32) -> Option<&Mesh> {
        self.all_meshes().map(|(_, m)| m).find(|m| m.id == id)
    }

    pub fn find_mesh_mut(&mut self, id: u32) -> Option<&mut Mesh> {
        std::iter::once(&mut self.world)
            .chain(self.entities.iter_mut())
            .flat_map(|e| e.meshes.iter_mut())
            .find(|m| m.id == id)
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
        self.all_entities()
            .filter(move |e| e.get("targetname") == Some(name))
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
            Some(v) => (
                v.get_or("formatversion", FORMAT_VERSION),
                v.get_or("editorversion", 100u32),
            ),
            None => (FORMAT_VERSION, 100),
        };

        let world_kv = root.block("world").ok_or(MapError::NoWorld)?;
        let world = Entity::from_kv(world_kv)?;

        let mut entities = Vec::new();
        for kv in root.blocks("entity") {
            entities.push(Entity::from_kv(kv)?);
        }

        let visgroups = root
            .block("visgroups")
            .map(|vg| vg.blocks("visgroup").map(VisGroup::from_kv).collect())
            .unwrap_or_default();
        let groups = root.blocks("group").map(Group::from_kv).collect();
        let cordon = root.block("cordon").and_then(Cordon::from_kv);

        let mut map = Map {
            format_version,
            editor_version,
            world,
            entities,
            visgroups,
            groups,
            cordon,
            next_id: 1,
        };
        map.reseed_next_id();
        map.assign_missing_ids();
        map.check_unique_ids()?;
        Ok(map)
    }

    /// Give an id to every object that came in without one.
    ///
    /// A hand-written map need not number its brushes; `read_id` leaves
    /// those at 0, and two of them would otherwise be reported as a
    /// duplicate of an id nobody wrote. Called after [`Self::reseed_next_id`]
    /// so the fresh ids land past everything the file did number.
    fn assign_missing_ids(&mut self) {
        let mut next = self.next_id;
        let mut fill = |id: &mut u32| {
            if *id == 0 {
                *id = next;
                next += 1;
            }
        };
        for e in std::iter::once(&mut self.world).chain(self.entities.iter_mut()) {
            fill(&mut e.id);
            for s in &mut e.solids {
                fill(&mut s.id);
                for side in &mut s.sides {
                    fill(&mut side.id);
                }
            }
            for m in &mut e.meshes {
                fill(&mut m.id);
                for face in &mut m.faces {
                    fill(&mut face.id);
                }
            }
        }
        for g in &mut self.groups {
            fill(&mut g.id);
        }
        fn fill_visgroups(groups: &mut [VisGroup], fill: &mut impl FnMut(&mut u32)) {
            for g in groups {
                fill(&mut g.id);
                fill_visgroups(&mut g.children, fill);
            }
        }
        fill_visgroups(&mut self.visgroups, &mut fill);
        self.next_id = next;
    }

    /// Point `next_id` past everything already in the map.
    ///
    /// Called after load so that ids handed out by the editor cannot collide
    /// with ids that came from the file.
    pub fn reseed_next_id(&mut self) {
        let mut max = 0;
        for id in self.all_ids() {
            max = max.max(id);
        }
        self.next_id = max + 1;
    }

    /// Every id in the map: entities, solids, sides, groups and visgroups.
    fn all_ids(&self) -> Vec<u32> {
        let mut ids = Vec::new();
        for e in self.all_entities() {
            ids.push(e.id);
            for s in &e.solids {
                ids.push(s.id);
                ids.extend(s.sides.iter().map(|side| side.id));
            }
            for m in &e.meshes {
                ids.push(m.id);
                ids.extend(m.faces.iter().map(|face| face.id));
            }
        }
        ids.extend(self.groups.iter().map(|g| g.id));
        for root in &self.visgroups {
            ids.extend(root.walk().iter().map(|(g, _)| g.id));
        }
        ids
    }

    /// Reject duplicate ids.
    ///
    /// Ids address objects across undo history, entity I/O and editor
    /// selection; two objects sharing one is a corruption that produces
    /// baffling behaviour much later, so it is caught at load.
    fn check_unique_ids(&self) -> Result<(), MapError> {
        let mut seen: HashMap<u32, usize> = HashMap::new();
        for id in self.all_ids() {
            *seen.entry(id).or_default() += 1;
        }
        // The lowest offending id, so the report is the same every load.
        match seen
            .iter()
            .filter(|(_, n)| **n > 1)
            .min_by_key(|(id, _)| **id)
        {
            Some((id, count)) => Err(MapError::DuplicateId {
                id: *id,
                count: *count,
            }),
            None => Ok(()),
        }
    }

    /// Check every solid is well formed, collecting all problems at once.
    ///
    /// Reporting every bad brush in one pass rather than stopping at the first
    /// is the difference between one fix-compile cycle and twenty.
    pub fn validate(&self) -> Vec<MapError> {
        let mut problems = Vec::new();
        for (_, solid) in self.all_solids() {
            if let Err(source) = solid.validate() {
                problems.push(MapError::Solid {
                    id: solid.id,
                    source,
                });
            }
        }
        for (_, mesh) in self.all_meshes() {
            if let Err(source) = mesh.validate() {
                problems.push(MapError::Mesh {
                    id: mesh.id,
                    source,
                });
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
            if e.solids.is_empty() && e.meshes.is_empty() && !e.has("origin") {
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
        if !self.visgroups.is_empty() {
            let mut vg = KeyValues::new("visgroups");
            for g in &self.visgroups {
                vg.push_block(g.to_kv());
            }
            root.push_block(vg);
        }
        root.push_block(self.world.to_kv("world"));
        for e in &self.entities {
            root.push_block(e.to_kv("entity"));
        }
        for g in &self.groups {
            root.push_block(g.to_kv());
        }
        if let Some(cordon) = &self.cordon {
            root.push_block(cordon.to_kv());
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
        for side in &mut solid.sides {
            side.id = self.next_id();
        }
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

    /// Give fresh ids to a solid and its sides, for a brush the editor made
    /// by cutting another one up.
    pub fn assign_ids(&mut self, solid: &mut Solid) {
        solid.id = self.next_id();
        for side in &mut solid.sides {
            side.id = self.next_id();
        }
    }

    // ---- editor metadata -------------------------------------------------

    /// Every object the editor can address, in map order.
    pub fn object_ids(&self) -> Vec<ObjectId> {
        let mut ids: Vec<ObjectId> = self
            .world
            .solids
            .iter()
            .map(|s| ObjectId::Solid(s.id))
            .collect();
        ids.extend(self.world.meshes.iter().map(|m| ObjectId::Mesh(m.id)));
        for e in &self.entities {
            ids.push(ObjectId::Entity(e.id));
            ids.extend(e.solids.iter().map(|s| ObjectId::Solid(s.id)));
            ids.extend(e.meshes.iter().map(|m| ObjectId::Mesh(m.id)));
        }
        ids
    }

    pub fn editor_data(&self, id: ObjectId) -> Option<&EditorData> {
        match id {
            ObjectId::Solid(id) => self.find_solid(id).map(|s| &s.editor),
            ObjectId::Mesh(id) => self.find_mesh(id).map(|m| &m.editor),
            ObjectId::Entity(id) => self.entities.iter().find(|e| e.id == id).map(|e| &e.editor),
        }
    }

    pub fn editor_data_mut(&mut self, id: ObjectId) -> Option<&mut EditorData> {
        match id {
            ObjectId::Solid(id) => self.find_solid_mut(id).map(|s| &mut s.editor),
            ObjectId::Mesh(id) => self.find_mesh_mut(id).map(|m| &mut m.editor),
            ObjectId::Entity(id) => self
                .entities
                .iter_mut()
                .find(|e| e.id == id)
                .map(|e| &mut e.editor),
        }
    }

    /// The entity a solid belongs to, or `None` for a world brush.
    pub fn owner_of_solid(&self, solid: u32) -> Option<&Entity> {
        self.entities
            .iter()
            .find(|e| e.solids.iter().any(|s| s.id == solid))
    }

    /// Every visgroup, depth first, with its depth.
    pub fn walk_visgroups(&self) -> Vec<(&VisGroup, usize)> {
        self.visgroups.iter().flat_map(|g| g.walk()).collect()
    }

    pub fn visgroup(&self, id: u32) -> Option<&VisGroup> {
        self.visgroups.iter().find_map(|g| g.find(id))
    }

    pub fn visgroup_mut(&mut self, id: u32) -> Option<&mut VisGroup> {
        self.visgroups.iter_mut().find_map(|g| g.find_mut(id))
    }

    /// Create a visgroup, under `parent` when given, and return its id.
    pub fn add_visgroup(&mut self, name: &str, parent: Option<u32>) -> u32 {
        let id = self.next_id();
        let group = VisGroup::new(id, name);
        match parent.and_then(|p| self.visgroup_mut(p)) {
            Some(parent) => parent.children.push(group),
            None => self.visgroups.push(group),
        }
        id
    }

    /// Delete a visgroup. Its children move up to where it was, and every
    /// object that was in it forgets it.
    pub fn remove_visgroup(&mut self, id: u32) -> bool {
        fn take(groups: &mut Vec<VisGroup>, id: u32) -> Option<VisGroup> {
            if let Some(i) = groups.iter().position(|g| g.id == id) {
                let removed = groups.remove(i);
                let children = removed.children.clone();
                for (offset, child) in children.into_iter().enumerate() {
                    groups.insert(i + offset, child);
                }
                return Some(removed);
            }
            groups.iter_mut().find_map(|g| take(&mut g.children, id))
        }
        let Some(_) = take(&mut self.visgroups, id) else {
            return false;
        };
        for object in self.object_ids() {
            if let Some(data) = self.editor_data_mut(object) {
                data.remove_from_visgroup(id);
            }
        }
        true
    }

    /// Whether a visgroup is shown: it and every ancestor must be visible.
    /// An id that names no visgroup is treated as visible, so a stale
    /// membership hides nothing.
    pub fn is_visgroup_visible(&self, id: u32) -> bool {
        fn visible_in(groups: &[VisGroup], id: u32) -> Option<bool> {
            for g in groups {
                if g.id == id {
                    return Some(g.visible);
                }
                if let Some(below) = visible_in(&g.children, id) {
                    return Some(g.visible && below);
                }
            }
            None
        }
        visible_in(&self.visgroups, id).unwrap_or(true)
    }

    /// The objects in a visgroup.
    pub fn visgroup_members(&self, id: u32) -> Vec<ObjectId> {
        self.object_ids()
            .into_iter()
            .filter(|&o| self.editor_data(o).is_some_and(|d| d.in_visgroup(id)))
            .collect()
    }

    /// The objects in a group.
    pub fn group_members(&self, id: u32) -> Vec<ObjectId> {
        self.object_ids()
            .into_iter()
            .filter(|&o| self.editor_data(o).is_some_and(|d| d.group == Some(id)))
            .collect()
    }

    /// The visgroups marked as streamed sections, depth first -- the order
    /// the compiler numbers sections in, so section `n` is `sections()[n - 1]`
    /// (section 0 is the unassigned world).
    pub fn sections(&self) -> Vec<&VisGroup> {
        self.walk_visgroups()
            .into_iter()
            .filter_map(|(g, _)| g.stream.then_some(g))
            .collect()
    }
}

/// Helper used by the KeyValues readers to pull an id, defaulting to 0.
pub(crate) fn read_id(kv: &KeyValues) -> u32 {
    kv.get_or("id", 0u32)
}

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

    #[test]
    fn a_world_mesh_survives_the_file_and_shares_the_id_space() {
        let text = r#"
world
{
    "id" "1"
    "classname" "worldspawn"
    mesh
    {
        "id" "2"
        vertices { "v" "0 0 0" "v" "0 64 0" "v" "64 64 0" "v" "64 0 0" }
        face { "id" "3" "v" "0 1 2 3" "material" "dev/floor" }
    }
}
"#;
        let map = Map::parse(text).unwrap();
        assert_eq!(map.world.meshes.len(), 1);
        assert!(map.validate().is_empty());
        let back = Map::parse(&map.to_text()).unwrap();
        assert_eq!(back.world.meshes, map.world.meshes);
        assert_eq!(back.find_mesh(2).unwrap().faces[0].material, "dev/floor");

        // A brush reusing the mesh's id is the corruption ids exist to catch.
        let clash = text.replace(
            "    mesh",
            "    solid { \"id\" \"2\" side { \"id\" \"9\" \"plane\" \"(0 0 0) (0 1 0) (1 1 0)\" } }\n    mesh",
        );
        assert!(matches!(
            Map::parse(&clash),
            Err(MapError::DuplicateId { id: 2, .. })
        ));
    }

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
        assert!(matches!(
            Map::parse("versioninfo { }"),
            Err(MapError::NoWorld)
        ));
    }

    #[test]
    fn objects_written_without_ids_are_numbered_rather_than_rejected() {
        // Strip the side ids: a hand-written map need not number anything.
        let unnumbered = SAMPLE
            .replace("\"id\" \"3\"", "")
            .replace("\"id\" \"4\"", "");
        let map = Map::parse(&unnumbered).expect("missing ids are assigned, not duplicates");
        let sides = &map.world.solids[0].sides;
        assert!(sides.iter().all(|s| s.id != 0));
        assert_ne!(sides[0].id, sides[1].id);
        assert!(sides[0].id > 10, "fresh ids land past the file's own");
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let bad = SAMPLE.replace("\"id\" \"9\"", "\"id\" \"1\"");
        assert!(matches!(
            Map::parse(&bad),
            Err(MapError::DuplicateId { id: 1, .. })
        ));
    }

    #[test]
    fn new_ids_never_collide_with_loaded_ones() {
        let mut map = Map::parse(SAMPLE).unwrap();
        let fresh = map.next_id();
        assert!(
            fresh > 10,
            "next id {fresh} must clear every id in the file"
        );
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
    fn editor_blocks_round_trip_and_are_absent_when_unused() {
        let mut map = Map::parse(SAMPLE).unwrap();
        let plain = map.to_text();
        assert!(
            !plain
                .lines()
                .any(|l| matches!(l.trim(), "visgroups" | "editor" | "group" | "cordon")),
            "a map that uses none of the editor blocks must not grow any"
        );

        let cave = map.add_visgroup("Cave", None);
        let pool = map.add_visgroup("Pool", Some(cave));
        map.visgroup_mut(pool).unwrap().stream = true;
        map.visgroup_mut(cave).unwrap().visible = false;
        let solid = map.world.solids[0].id;
        map.editor_data_mut(ObjectId::Solid(solid))
            .unwrap()
            .add_to_visgroup(pool);
        let group = map.next_id();
        map.groups.push(Group {
            id: group,
            editor: EditorData::default(),
        });
        map.editor_data_mut(ObjectId::Entity(9)).unwrap().group = Some(group);
        map.editor_data_mut(ObjectId::Entity(9)).unwrap().comments = "spawn".into();
        map.cordon = Some(Cordon {
            bounds: Aabb::new(Vec3::ZERO, Vec3::splat(128.0)),
            active: true,
        });

        let text = map.to_text();
        let again = Map::parse(&text).unwrap();
        assert_eq!(again.visgroups, map.visgroups);
        assert_eq!(again.groups, map.groups);
        assert_eq!(again.cordon, map.cordon);
        assert_eq!(again.world.solids[0].editor.visgroups, vec![pool]);
        assert_eq!(again.entities[0].editor.group, Some(group));
        assert_eq!(again.entities[0].editor.comments, "spawn");
        assert_eq!(again.to_text(), text, "writing must be stable");

        // The editor block is not a property: it must not reach the game.
        assert!(
            again.entities[0]
                .properties
                .iter()
                .all(|(k, _)| k != "comments")
        );

        assert_eq!(again.visgroup_members(pool), vec![ObjectId::Solid(solid)]);
        assert_eq!(again.group_members(group), vec![ObjectId::Entity(9)]);
        assert_eq!(again.sections().len(), 1);
        assert!(
            !again.is_visgroup_visible(pool),
            "a child is hidden by its hidden parent"
        );
        assert!(again.is_visgroup_visible(9999), "a stale id hides nothing");
    }

    #[test]
    fn removing_a_visgroup_reparents_children_and_strips_members() {
        let mut map = Map::parse(SAMPLE).unwrap();
        let cave = map.add_visgroup("Cave", None);
        let pool = map.add_visgroup("Pool", Some(cave));
        let solid = map.world.solids[0].id;
        let data = map.editor_data_mut(ObjectId::Solid(solid)).unwrap();
        data.add_to_visgroup(cave);
        data.add_to_visgroup(pool);

        assert!(map.remove_visgroup(cave));
        assert!(!map.remove_visgroup(cave));
        assert_eq!(map.visgroups.len(), 1);
        assert_eq!(map.visgroups[0].id, pool, "the child moved up");
        assert_eq!(map.world.solids[0].editor.visgroups, vec![pool]);
    }

    #[test]
    fn visgroup_and_group_ids_share_the_map_id_space() {
        let mut map = Map::parse(SAMPLE).unwrap();
        let vg = map.add_visgroup("A", None);
        let text = map.to_text();
        let again = Map::parse(&text).unwrap();
        assert!(again.all_ids().contains(&vg));
        let colliding = text.replacen(&format!("\"id\" \"{vg}\""), "\"id\" \"9\"", 1);
        assert!(matches!(
            Map::parse(&colliding),
            Err(MapError::DuplicateId { id: 9, .. })
        ));
    }

    #[test]
    fn the_shipped_map_survives_a_round_trip_unchanged() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../content/maps/kero_start.keromap");
        let text = std::fs::read_to_string(path).unwrap();
        let map = Map::parse(&text).unwrap();
        let again = Map::parse(&map.to_text()).unwrap();
        assert_eq!(again.to_text(), map.to_text());
        assert_eq!(again.solid_count(), map.solid_count());
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
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "ids collided: {ids:?}");
    }
}
