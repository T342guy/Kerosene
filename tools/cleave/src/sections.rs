// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Which section each brush belongs to, and the cordon.
//!
//! A section is a visgroup the designer marked as streamed; the engine loads
//! and unloads its geometry around the player. Cleave numbers them in the
//! order the visgroup tree lists them, section 0 being the world -- every
//! brush in no streamed group -- and tags each brush and each face with
//! its number. A brush can also name its section outright with
//! `"section" "name"`, for a map written by hand or by a tool that has no
//! visgroups.
//!
//! The cordon is the other thing the editor says about which brushes to
//! compile: a box, and only what is inside it is built, with the box's own
//! walls sealing it so the result does not leak.

use kerosene_bsp::Section;
use kerosene_map::{Map, Solid};
use kerosene_math::{Aabb, Plane, Vec3};
use std::collections::HashMap;

/// The sections of one compile.
#[derive(Debug, Default)]
pub struct SectionTable {
    pub sections: Vec<Section>,
    by_visgroup: HashMap<u32, u16>,
    by_name: HashMap<String, u16>,
}

impl SectionTable {
    /// One section per streamed visgroup, depth first, after the world.
    pub fn of(map: &Map) -> SectionTable {
        let mut table = SectionTable {
            sections: vec![Section::world()],
            ..Default::default()
        };
        for group in map.sections() {
            let index = table.sections.len() as u16;
            table.sections.push(Section {
                name: group.name.clone(),
                bounds: Aabb::EMPTY,
            });
            table.by_visgroup.insert(group.id, index);
            table
                .by_name
                .entry(group.name.to_ascii_lowercase())
                .or_insert(index);
        }
        table
    }

    /// The section a world brush is in, growing that section's bounds to
    /// hold it. A brush in two streamed groups takes the first and is
    /// reported; a `section` key names one by name, making it if need be.
    pub fn assign(&mut self, solid: &Solid, warnings: &mut Vec<crate::brush::Warning>) -> u16 {
        let mut index = 0u16;
        let mut groups = solid
            .editor
            .visgroups
            .iter()
            .filter_map(|id| self.by_visgroup.get(id).copied());
        if let Some(first) = groups.next() {
            index = first;
            if groups.next().is_some() {
                warnings.push(crate::brush::Warning {
                    brush_id: solid.id,
                    message: format!(
                        "is in more than one streamed visgroup; compiled into '{}'",
                        self.sections[first as usize].name
                    ),
                });
            }
        }
        if index == 0
            && let Some(name) = solid
                .get("section")
                .map(str::trim)
                .filter(|n| !n.is_empty())
        {
            let key = name.to_ascii_lowercase();
            index = match self.by_name.get(&key) {
                Some(&i) => i,
                None => {
                    let i = self.sections.len() as u16;
                    self.sections.push(Section {
                        name: name.to_string(),
                        bounds: Aabb::EMPTY,
                    });
                    self.by_name.insert(key, i);
                    i
                }
            };
        }
        if index != 0 {
            let section = &mut self.sections[index as usize];
            section.bounds = section.bounds.union(&solid.bounds());
        }
        index
    }
}

impl SectionTable {
    /// Drop sections nothing ended up in -- a streamed visgroup with no
    /// brushes, or one the cordon cut away -- renumbering the rest.
    pub fn compact(&mut self, brushes: &mut [crate::brush::BrushWork]) {
        let mut used = vec![false; self.sections.len()];
        used[0] = true;
        for b in brushes.iter() {
            if let Some(u) = used.get_mut(b.section as usize) {
                *u = true;
            }
        }
        if used.iter().all(|&u| u) {
            return;
        }
        let mut remap = vec![0u16; self.sections.len()];
        let mut kept = Vec::new();
        for (i, section) in self.sections.drain(..).enumerate() {
            if used[i] {
                remap[i] = kept.len() as u16;
                kept.push(section);
            }
        }
        self.sections = kept;
        for b in brushes.iter_mut() {
            b.section = remap[b.section as usize];
        }
        self.by_visgroup.clear();
        self.by_name.clear();
    }
}

/// The material the cordon's own walls wear: drawn by nothing, solid to
/// everything.
pub const CORDON_MATERIAL: &str = "tools/nodraw";

/// How thick the cordon's sealing walls are.
const CORDON_WALL: f32 = 16.0;

/// The map, cut down to a box.
///
/// Every brush is clipped to the box, brushes and point entities outside it
/// are dropped, and six walls seal it so the result compiles without a
/// leak. A `worldspawn` and an `info_player_start` survive whatever the box
/// is: a map with no start is a map that cannot be played, and a cordon is
/// for playing a corner of a map.
pub fn cordon(map: &Map, bounds: Aabb) -> (Map, Vec<crate::brush::Warning>) {
    let mut out = map.clone();
    let mut warnings = Vec::new();
    let planes: Vec<Plane> = box_planes(bounds);

    let clip_all = |solids: &mut Vec<Solid>, map: &mut Map| {
        let mut kept = Vec::new();
        for solid in solids.drain(..) {
            if !solid.bounds().intersects(&bounds) {
                continue;
            }
            let mut piece = Some(solid);
            for plane in &planes {
                piece = piece.and_then(|p| p.behind(plane));
            }
            if let Some(mut piece) = piece {
                // The cut may have given it new sides with no id.
                if piece.sides.iter().any(|s| s.id == 0) {
                    let id = piece.id;
                    map.assign_ids(&mut piece);
                    piece.id = id;
                }
                kept.push(piece);
            }
        }
        *solids = kept;
    };

    let mut world = std::mem::take(&mut out.world.solids);
    clip_all(&mut world, &mut out);
    out.world.solids = world;

    let mut kept_entities = Vec::new();
    let mut had_start = false;
    for mut entity in std::mem::take(&mut out.entities) {
        if entity.solids.is_empty() {
            let inside = bounds.contains_point(entity.origin());
            let is_start = entity.classname() == "info_player_start";
            if is_start && inside {
                had_start = true;
            }
            if inside {
                kept_entities.push(entity);
            } else if is_start && !had_start {
                // Moved inside rather than lost, so the cordon is playable.
                entity.set_origin(bounds.center());
                warnings.push(crate::brush::Warning {
                    brush_id: 0,
                    message: "info_player_start was outside the cordon and was moved to its centre"
                        .into(),
                });
                kept_entities.push(entity);
                had_start = true;
            }
            continue;
        }
        let mut solids = std::mem::take(&mut entity.solids);
        clip_all(&mut solids, &mut out);
        if solids.is_empty() {
            continue;
        }
        entity.solids = solids;
        kept_entities.push(entity);
    }
    out.entities = kept_entities;

    // The walls: six slabs just outside the box, so what was cut on the
    // box's face now meets a wall there.
    let outer = bounds.expanded(CORDON_WALL);
    let (lo, hi, olo, ohi) = (bounds.min, bounds.max, outer.min, outer.max);
    let slabs = [
        Aabb::new(
            Vec3::new(olo.x, olo.y, olo.z),
            Vec3::new(ohi.x, ohi.y, lo.z),
        ),
        Aabb::new(
            Vec3::new(olo.x, olo.y, hi.z),
            Vec3::new(ohi.x, ohi.y, ohi.z),
        ),
        Aabb::new(Vec3::new(olo.x, olo.y, lo.z), Vec3::new(lo.x, ohi.y, hi.z)),
        Aabb::new(Vec3::new(hi.x, olo.y, lo.z), Vec3::new(ohi.x, ohi.y, hi.z)),
        Aabb::new(Vec3::new(lo.x, olo.y, lo.z), Vec3::new(hi.x, lo.y, hi.z)),
        Aabb::new(Vec3::new(lo.x, hi.y, lo.z), Vec3::new(hi.x, ohi.y, hi.z)),
    ];
    for slab in slabs {
        out.add_world_solid(Solid::cube(slab, CORDON_MATERIAL));
    }
    (out, warnings)
}

/// The six planes of a box, facing outward, so `behind` each keeps the
/// inside.
fn box_planes(b: Aabb) -> Vec<Plane> {
    vec![
        Plane::new(Vec3::X, b.max.x),
        Plane::new(-Vec3::X, -b.min.x),
        Plane::new(Vec3::Y, b.max.y),
        Plane::new(-Vec3::Y, -b.min.y),
        Plane::new(Vec3::Z, b.max.z),
        Plane::new(-Vec3::Z, -b.min.z),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_with_two_rooms() -> Map {
        let mut map = Map::new();
        map.add_world_solid(Solid::cube(
            Aabb::new(Vec3::ZERO, Vec3::new(256.0, 64.0, 64.0)),
            "dev/grid",
        ));
        let light = map.add_entity("light");
        light.set_origin(Vec3::new(32.0, 32.0, 32.0));
        let far = map.add_entity("light");
        far.set_origin(Vec3::new(500.0, 32.0, 32.0));
        let start = map.add_entity("info_player_start");
        start.set_origin(Vec3::new(500.0, 32.0, 32.0));
        map
    }

    #[test]
    fn a_cordon_keeps_what_is_inside_and_seals_it() {
        let map = map_with_two_rooms();
        let (cut, warnings) = cordon(&map, Aabb::new(Vec3::ZERO, Vec3::splat(128.0)));
        // One clipped brush plus six walls.
        assert_eq!(cut.world.solids.len(), 7);
        assert_eq!(cut.world.solids[0].bounds().max.x, 128.0);
        assert!(
            cut.world.solids[1..]
                .iter()
                .all(|s| s.sides[0].material == CORDON_MATERIAL)
        );
        // The far light is gone; the start was moved in rather than lost.
        assert_eq!(cut.by_classname("light").count(), 1);
        let start = cut.by_classname("info_player_start").next().unwrap();
        assert!(Aabb::new(Vec3::ZERO, Vec3::splat(128.0)).contains_point(start.origin()));
        assert_eq!(warnings.len(), 1);
        // Everything still has a unique id.
        assert!(Map::parse(&cut.to_text()).is_ok());
    }

    #[test]
    fn sections_come_from_streamed_visgroups_and_section_keys() {
        let mut map = Map::new();
        let cave = map.add_visgroup("Cave", None);
        map.visgroup_mut(cave).unwrap().stream = true;
        let plain = map.add_visgroup("Not streamed", None);
        let mut a = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        a.editor.add_to_visgroup(plain);
        a.editor.add_to_visgroup(cave);
        let mut b = Solid::cube(
            Aabb::new(Vec3::splat(128.0), Vec3::splat(192.0)),
            "dev/grid",
        );
        b.set("section", "Attic");
        let c = Solid::cube(
            Aabb::new(Vec3::splat(256.0), Vec3::splat(300.0)),
            "dev/grid",
        );

        let mut table = SectionTable::of(&map);
        let mut warnings = Vec::new();
        assert_eq!(table.assign(&a, &mut warnings), 1);
        assert_eq!(
            table.assign(&b, &mut warnings),
            2,
            "a named section is made"
        );
        assert_eq!(table.assign(&c, &mut warnings), 0);
        assert!(warnings.is_empty());
        assert_eq!(table.sections.len(), 3);
        assert_eq!(table.sections[1].name, "Cave");
        assert_eq!(table.sections[1].bounds, a.bounds());
        assert_eq!(table.sections[2].name, "Attic");
    }
}
