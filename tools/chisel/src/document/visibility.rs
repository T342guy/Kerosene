// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! What is shown, what selects together, and what the cordon keeps.
//!
//! Past a couple of hundred brushes a map is unreadable in the 2D panes: the
//! roof is drawn over the rooms, the rooms over the corridors. Every editor
//! in this lineage answers that the same way -- VisGroups to hide sets of
//! things by name, a quick-hide for whatever is in the way right now, and a
//! cordon to work inside one box of the map -- and this module is that
//! answer here. There is one rule for whether an object is visible,
//! [`Document::is_visible`], and every pane, picker and select-all asks it.
//!
//! VisGroup state and quick-hide live in the map, so they are undoable and
//! they survive a save; the *auto* visgroups (all entities, all tool
//! brushes, one per class) are computed from what the map holds and only
//! their hidden-or-not is remembered, here, for the session.

use super::Document;
use kerosene_map::{Entity, ObjectId, Solid};
use kerosene_math::Aabb;
use std::collections::HashSet;

/// A visgroup nobody made: a set of objects that share a property, offered
/// alongside the user's own so that "hide every light" is a checkbox.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AutoGroup {
    /// Every point entity.
    Entities,
    /// Brushes that belong to no entity.
    WorldBrushes,
    /// Every entity built from brushes.
    BrushEntities,
    /// A brush with any face on a `tools/` material.
    ToolBrushes,
    /// A brush the compiler treats as detail: `func_detail` or `"detail" "1"`.
    Detail,
    /// Triggers, by class.
    Triggers,
    /// Player and NPC clip brushes.
    Clip,
    /// Every point entity of one class.
    Class(String),
}

impl AutoGroup {
    /// The fixed groups, in the order the panel lists them; classes follow.
    pub fn fixed() -> [AutoGroup; 7] {
        [
            AutoGroup::Entities,
            AutoGroup::WorldBrushes,
            AutoGroup::BrushEntities,
            AutoGroup::ToolBrushes,
            AutoGroup::Detail,
            AutoGroup::Triggers,
            AutoGroup::Clip,
        ]
    }

    pub fn label(&self) -> String {
        match self {
            AutoGroup::Entities => "entities".into(),
            AutoGroup::WorldBrushes => "world brushes".into(),
            AutoGroup::BrushEntities => "brush entities".into(),
            AutoGroup::ToolBrushes => "tool brushes".into(),
            AutoGroup::Detail => "detail".into(),
            AutoGroup::Triggers => "triggers".into(),
            AutoGroup::Clip => "clip".into(),
            AutoGroup::Class(c) => c.clone(),
        }
    }

    fn holds_solid(&self, owner: &Entity, solid: &Solid) -> bool {
        let world = owner.classname() == "worldspawn";
        match self {
            AutoGroup::Entities | AutoGroup::Class(_) => false,
            AutoGroup::WorldBrushes => world,
            AutoGroup::BrushEntities => !world,
            AutoGroup::ToolBrushes => solid.sides.iter().any(|s| s.is_tool_material()),
            AutoGroup::Detail => {
                owner.classname() == "func_detail" || solid.get("detail").is_some_and(is_on)
            }
            AutoGroup::Triggers => owner.classname().starts_with("trigger_"),
            AutoGroup::Clip => solid.sides.iter().any(|s| {
                let m = s.material.to_ascii_lowercase();
                m.ends_with("playerclip") || m.ends_with("npcclip") || m.ends_with("clip")
            }),
        }
    }

    fn holds_entity(&self, entity: &Entity) -> bool {
        match self {
            AutoGroup::Entities => entity.solids.is_empty(),
            AutoGroup::Class(c) => entity.solids.is_empty() && entity.classname() == c,
            AutoGroup::BrushEntities => !entity.solids.is_empty(),
            AutoGroup::Triggers => entity.classname().starts_with("trigger_"),
            AutoGroup::Detail => entity.classname() == "func_detail",
            _ => false,
        }
    }
}

fn is_on(v: &str) -> bool {
    matches!(v.trim(), "1" | "true" | "yes")
}

impl Document {
    // ---- the one visibility rule -----------------------------------------

    /// Whether an object is shown, pickable and selectable.
    pub fn is_visible(&self, id: ObjectId) -> bool {
        match id {
            ObjectId::Solid(solid_id) => {
                let Some((owner, solid)) = self.map.all_solids().find(|(_, s)| s.id == solid_id)
                else {
                    return false;
                };
                // A brush belonging to a hidden entity is hidden with it.
                if owner.classname() != "worldspawn" && !self.entity_visible(owner) {
                    return false;
                }
                self.solid_visible(owner, solid)
            }
            ObjectId::Entity(entity_id) => self
                .map
                .entities
                .iter()
                .find(|e| e.id == entity_id)
                .is_some_and(|e| self.entity_visible(e)),
        }
    }

    fn solid_visible(&self, owner: &Entity, solid: &Solid) -> bool {
        if !solid.editor.visible {
            return false;
        }
        if !solid
            .editor
            .visgroups
            .iter()
            .all(|&g| self.map.is_visgroup_visible(g))
        {
            return false;
        }
        if self.auto_hidden.iter().any(|g| g.holds_solid(owner, solid)) {
            return false;
        }
        self.inside_cordon(solid.bounds())
    }

    fn entity_visible(&self, entity: &Entity) -> bool {
        if !entity.editor.visible {
            return false;
        }
        if !entity
            .editor
            .visgroups
            .iter()
            .all(|&g| self.map.is_visgroup_visible(g))
        {
            return false;
        }
        if self.auto_hidden.iter().any(|g| g.holds_entity(entity)) {
            return false;
        }
        if entity.solids.is_empty() {
            let o = entity.origin();
            self.inside_cordon(Aabb::new(o, o))
        } else {
            let mut b = Aabb::EMPTY;
            for s in &entity.solids {
                b = b.union(&s.bounds());
            }
            self.inside_cordon(b)
        }
    }

    fn inside_cordon(&self, bounds: Aabb) -> bool {
        match &self.map.cordon {
            Some(c) if c.active => c.bounds.intersects(&bounds),
            _ => true,
        }
    }

    /// Every visible solid, with the entity that owns it.
    pub fn visible_solids(&self) -> impl Iterator<Item = (&Entity, &Solid)> {
        self.map.all_solids().filter(|(owner, solid)| {
            (owner.classname() == "worldspawn" || self.entity_visible(owner))
                && self.solid_visible(owner, solid)
        })
    }

    /// Every visible point entity.
    pub fn visible_point_entities(&self) -> impl Iterator<Item = &Entity> {
        self.map
            .entities
            .iter()
            .filter(|e| e.solids.is_empty() && self.entity_visible(e))
    }

    /// How many objects are hidden by any means, for the status bar.
    pub fn hidden_count(&self) -> usize {
        // Counted directly rather than through `is_visible`, which looks
        // each solid up by id: this runs every frame for the status bar.
        let solids = self.map.solid_count() - self.visible_solids().count();
        let entities = self
            .map
            .entities
            .iter()
            .filter(|e| !self.entity_visible(e))
            .count();
        solids + entities
    }

    /// Drop anything hidden from the selection. Called after every change
    /// to what is visible: a hidden thing that stayed selected would move,
    /// delete and retexture with the rest, unseen.
    pub fn prune_hidden_selection(&mut self) {
        let solids: Vec<u32> = self
            .selection
            .solids
            .iter()
            .copied()
            .filter(|&id| !self.is_visible(ObjectId::Solid(id)))
            .collect();
        for id in solids {
            self.selection.solids.remove(&id);
            self.selection.faces.retain(|(s, _)| *s != id);
        }
        let entities: Vec<u32> = self
            .selection
            .entities
            .iter()
            .copied()
            .filter(|&id| !self.is_visible(ObjectId::Entity(id)))
            .collect();
        for id in entities {
            self.selection.entities.remove(&id);
        }
    }

    /// The selection as objects.
    pub fn selected_objects(&self) -> Vec<ObjectId> {
        let mut out: Vec<ObjectId> = self
            .selection
            .solids
            .iter()
            .map(|&id| ObjectId::Solid(id))
            .collect();
        out.extend(
            self.selection
                .entities
                .iter()
                .map(|&id| ObjectId::Entity(id)),
        );
        out.sort();
        out
    }

    fn select_objects(&mut self, objects: &[ObjectId]) {
        for &o in objects {
            match o {
                ObjectId::Solid(id) => {
                    // A brush of a brush entity selects the entity, the same
                    // rule a click follows.
                    match self.map.owner_of_solid(id) {
                        Some(owner) => {
                            self.selection.entities.insert(owner.id);
                        }
                        None => {
                            self.selection.solids.insert(id);
                        }
                    }
                }
                ObjectId::Entity(id) => {
                    self.selection.entities.insert(id);
                }
            }
        }
    }

    // ---- quick hide ------------------------------------------------------

    /// Hide the selection (`H`). Undoable, like everything in the map.
    pub fn hide_selection(&mut self) -> usize {
        let objects = self.selected_objects();
        if objects.is_empty() {
            return 0;
        }
        let n = objects.len();
        self.apply("hide", |doc| {
            for o in objects {
                if let Some(data) = doc.map.editor_data_mut(o) {
                    data.visible = false;
                }
                // A brush entity's brushes go with it.
                if let ObjectId::Entity(id) = o
                    && let Some(e) = doc.map.entities.iter_mut().find(|e| e.id == id)
                {
                    for s in &mut e.solids {
                        s.editor.visible = false;
                    }
                }
            }
            doc.selection.clear();
        });
        n
    }

    /// Hide everything that is not selected (`ctrl-H`): isolate.
    pub fn hide_unselected(&mut self) -> usize {
        let keep = self.selected_objects();
        let all = self.map.object_ids();
        let hide: Vec<ObjectId> = all
            .into_iter()
            .filter(|o| !keep.contains(o))
            .filter(|&o| self.map.editor_data(o).is_some_and(|d| d.visible))
            .filter(|&o| match o {
                // A brush of a selected entity stays.
                ObjectId::Solid(id) => self
                    .map
                    .owner_of_solid(id)
                    .is_none_or(|e| !keep.contains(&ObjectId::Entity(e.id))),
                ObjectId::Entity(_) => true,
            })
            .collect();
        if hide.is_empty() {
            return 0;
        }
        let n = hide.len();
        self.apply("hide unselected", |doc| {
            for o in hide {
                if let Some(data) = doc.map.editor_data_mut(o) {
                    data.visible = false;
                }
            }
        });
        n
    }

    /// Show everything quick-hidden (`U`). Visgroups are not touched: they
    /// were hidden on purpose, by name.
    pub fn unhide_all(&mut self) -> usize {
        let hidden: Vec<ObjectId> = self
            .map
            .object_ids()
            .into_iter()
            .filter(|&o| self.map.editor_data(o).is_some_and(|d| !d.visible))
            .collect();
        if hidden.is_empty() {
            return 0;
        }
        let n = hidden.len();
        self.apply("unhide all", |doc| {
            for o in hidden {
                if let Some(data) = doc.map.editor_data_mut(o) {
                    data.visible = true;
                }
            }
        });
        n
    }

    // ---- groups ----------------------------------------------------------

    /// Put the selection in one new group (`ctrl-G`). Objects already in a
    /// group move to the new one.
    pub fn group_selection(&mut self) -> Option<u32> {
        let objects = self.selected_objects();
        if objects.len() < 2 {
            return None;
        }
        Some(self.apply("group", |doc| {
            let id = doc.map.next_id();
            doc.map.groups.push(kerosene_map::Group {
                id,
                editor: Default::default(),
            });
            for o in objects {
                if let Some(data) = doc.map.editor_data_mut(o) {
                    data.group = Some(id);
                }
            }
            id
        }))
    }

    /// Take the selection out of its groups (`ctrl-U`).
    pub fn ungroup_selection(&mut self) -> usize {
        let objects: Vec<ObjectId> = self
            .selected_objects()
            .into_iter()
            .filter(|&o| self.map.editor_data(o).is_some_and(|d| d.group.is_some()))
            .collect();
        if objects.is_empty() {
            return 0;
        }
        let n = objects.len();
        self.apply("ungroup", |doc| {
            for o in objects {
                if let Some(data) = doc.map.editor_data_mut(o) {
                    data.group = None;
                }
            }
            // A group nothing is in any more is gone.
            let used: HashSet<u32> = doc
                .map
                .object_ids()
                .into_iter()
                .filter_map(|o| doc.map.editor_data(o).and_then(|d| d.group))
                .collect();
            doc.map.groups.retain(|g| used.contains(&g.id));
        });
        n
    }

    /// Grow the selection to whole groups, unless groups are being ignored.
    /// Called after every pick.
    pub fn expand_selection_groups(&mut self) {
        if self.ignore_groups {
            return;
        }
        let groups: HashSet<u32> = self
            .selected_objects()
            .into_iter()
            .filter_map(|o| self.map.editor_data(o).and_then(|d| d.group))
            .collect();
        if groups.is_empty() {
            return;
        }
        let members: Vec<ObjectId> = self
            .map
            .object_ids()
            .into_iter()
            .filter(|&o| {
                self.map
                    .editor_data(o)
                    .and_then(|d| d.group)
                    .is_some_and(|g| groups.contains(&g))
                    && self.is_visible(o)
            })
            .collect();
        self.select_objects(&members);
    }

    // ---- visgroups -------------------------------------------------------

    /// Make a visgroup of the selection (`ctrl-shift-G`) and return its id.
    pub fn new_visgroup_from_selection(&mut self, name: &str) -> u32 {
        let objects = self.selected_objects();
        let name = name.to_string();
        self.apply("new visgroup", |doc| {
            let id = doc.map.add_visgroup(&name, None);
            for o in objects {
                if let Some(data) = doc.map.editor_data_mut(o) {
                    data.add_to_visgroup(id);
                }
            }
            id
        })
    }

    pub fn add_selection_to_visgroup(&mut self, group: u32) -> usize {
        let objects = self.selected_objects();
        let n = objects.len();
        if n == 0 {
            return 0;
        }
        self.apply("add to visgroup", |doc| {
            for o in objects {
                if let Some(data) = doc.map.editor_data_mut(o) {
                    data.add_to_visgroup(group);
                }
            }
        });
        n
    }

    pub fn remove_selection_from_visgroup(&mut self, group: u32) -> usize {
        let objects = self.selected_objects();
        let n = objects.len();
        if n == 0 {
            return 0;
        }
        self.apply("remove from visgroup", |doc| {
            for o in objects {
                if let Some(data) = doc.map.editor_data_mut(o) {
                    data.remove_from_visgroup(group);
                }
            }
        });
        n
    }

    /// Show or hide a visgroup. The selection loses whatever this hides.
    pub fn set_visgroup_visible(&mut self, group: u32, visible: bool) {
        let label = if visible {
            "show visgroup"
        } else {
            "hide visgroup"
        };
        self.apply(label, |doc| {
            if let Some(g) = doc.map.visgroup_mut(group) {
                g.visible = visible;
            }
            doc.prune_hidden_selection();
        });
    }

    pub fn set_visgroup_stream(&mut self, group: u32, stream: bool) {
        self.apply("mark section", |doc| {
            if let Some(g) = doc.map.visgroup_mut(group) {
                g.stream = stream;
            }
        });
    }

    pub fn rename_visgroup(&mut self, group: u32, name: &str) {
        let name = name.to_string();
        self.apply("rename visgroup", |doc| {
            if let Some(g) = doc.map.visgroup_mut(group) {
                g.name = name;
            }
        });
    }

    pub fn set_visgroup_color(&mut self, group: u32, color: [u8; 3]) {
        self.apply("recolour visgroup", |doc| {
            if let Some(g) = doc.map.visgroup_mut(group) {
                g.color = color;
            }
        });
    }

    pub fn add_visgroup(&mut self, name: &str, parent: Option<u32>) -> u32 {
        let name = name.to_string();
        self.apply("new visgroup", |doc| doc.map.add_visgroup(&name, parent))
    }

    pub fn remove_visgroup(&mut self, group: u32) -> bool {
        self.apply("delete visgroup", |doc| {
            let removed = doc.map.remove_visgroup(group);
            doc.prune_hidden_selection();
            removed
        })
    }

    /// Select the visible members of a visgroup.
    pub fn select_visgroup(&mut self, group: u32) -> usize {
        self.selection.clear();
        let members: Vec<ObjectId> = self
            .map
            .visgroup_members(group)
            .into_iter()
            .filter(|&o| self.is_visible(o))
            .collect();
        self.select_objects(&members);
        self.selection.len()
    }

    /// Show or hide an auto visgroup. Session state, not in the map.
    pub fn set_auto_visible(&mut self, group: AutoGroup, visible: bool) {
        if visible {
            self.auto_hidden.remove(&group);
        } else {
            self.auto_hidden.insert(group);
        }
        self.prune_hidden_selection();
    }

    pub fn is_auto_visible(&self, group: &AutoGroup) -> bool {
        !self.auto_hidden.contains(group)
    }

    /// The point-entity classes in the map, sorted, for the auto list.
    pub fn point_classes_present(&self) -> Vec<String> {
        let mut classes: Vec<String> = self
            .map
            .entities
            .iter()
            .filter(|e| e.solids.is_empty())
            .map(|e| e.classname().to_string())
            .collect();
        classes.sort();
        classes.dedup();
        classes
    }

    // ---- cordon ----------------------------------------------------------

    /// Turn the cordon on or off. Turning it on with none defined draws one
    /// around the selection, or around the whole map.
    pub fn set_cordon_active(&mut self, active: bool) {
        let fallback = self
            .selection_bounds()
            .unwrap_or_else(|| self.map.bounds())
            .expanded(self.grid.size);
        self.apply(if active { "cordon on" } else { "cordon off" }, |doc| {
            match doc.map.cordon.as_mut() {
                Some(c) => c.active = active,
                None => {
                    doc.map.cordon = Some(kerosene_map::Cordon {
                        bounds: fallback,
                        active,
                    })
                }
            }
            doc.prune_hidden_selection();
        });
    }

    pub fn cordon_active(&self) -> bool {
        self.map.cordon.as_ref().is_some_and(|c| c.active)
    }

    pub fn set_cordon_bounds(&mut self, bounds: Aabb) {
        let (lo, hi) = self.grid.snap_box(bounds.min, bounds.max);
        let bounds = Aabb::new(lo, hi);
        self.apply("resize cordon", |doc| {
            match doc.map.cordon.as_mut() {
                Some(c) => c.bounds = bounds,
                None => {
                    doc.map.cordon = Some(kerosene_map::Cordon {
                        bounds,
                        active: false,
                    })
                }
            }
            doc.prune_hidden_selection();
        });
    }
}
