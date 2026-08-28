// SPDX-License-Identifier: LGPL-3.0-or-later
//! The document being edited: a map, a selection, and an undo history.
//!
//! Every change goes through [`Document::apply`], which is what makes undo
//! work. An editor where some operations are undoable and others are not is
//! worse than one with no undo at all, because you stop trusting it.
//!
//! Undo is implemented by snapshotting the map. That is the unglamorous
//! choice -- a command pattern with inverse operations is more elegant and
//! uses far less memory -- but a `.voidmap` for a large level is a few megabytes,
//! and correctness here is worth more than the memory. An inverse operation
//! that is subtly wrong corrupts the level silently.

use crate::grid::Grid;
use std::collections::HashSet;
use std::path::PathBuf;
use void_map::{Entity, Map, Side, Solid};
use void_math::{Aabb, Plane, Vec3, Winding};

/// How many undo steps to keep.
pub const MAX_UNDO: usize = 128;

/// What is currently selected.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    pub solids: HashSet<u32>,
    pub entities: HashSet<u32>,
    /// Individual faces, for the face editor: `(solid id, side id)`.
    pub faces: HashSet<(u32, u32)>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.solids.is_empty() && self.entities.is_empty() && self.faces.is_empty()
    }

    pub fn len(&self) -> usize {
        self.solids.len() + self.entities.len() + self.faces.len()
    }

    pub fn clear(&mut self) {
        self.solids.clear();
        self.entities.clear();
        self.faces.clear();
    }
}

/// What an edit did, for the undo history and the status bar.
#[derive(Clone, Debug, PartialEq)]
pub struct EditLabel(pub String);

impl EditLabel {
    pub fn new(text: impl Into<String>) -> Self { EditLabel(text.into()) }
}

struct Snapshot {
    map: Map,
    selection: Selection,
    label: EditLabel,
}

/// The editor's state.
pub struct Document {
    pub map: Map,
    pub selection: Selection,
    pub grid: Grid,
    pub path: Option<PathBuf>,
    /// Material applied to newly created brushes.
    pub current_material: String,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Set on every edit, cleared on save.
    modified: bool,
    /// Bumped whenever the map changes, by an edit or by undo.
    ///
    /// The UI holds edit buffers -- a half-typed property value is not in the
    /// document yet -- and needs to know when what it is buffering has gone
    /// stale underneath it. Comparing the whole map would work and would cost
    /// a clone per frame.
    revision: u64,
}

impl Default for Document {
    fn default() -> Self { Document::new() }
}

impl Document {
    pub fn new() -> Self {
        Document {
            map: Map::new(),
            selection: Selection::default(),
            grid: Grid::default(),
            path: None,
            current_material: "dev/grid".to_string(),
            undo: Vec::new(),
            redo: Vec::new(),
            modified: false,
            revision: 0,
        }
    }

    pub fn open(path: PathBuf) -> anyhow::Result<Document> {
        let text = std::fs::read_to_string(&path)?;
        let map = Map::parse(&text)?;
        Ok(Document { map, path: Some(path), ..Document::new() })
    }

    pub fn save(&mut self, path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
        let target = path
            .or_else(|| self.path.clone())
            .ok_or_else(|| anyhow::anyhow!("no path to save to"))?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, self.map.to_text())?;
        self.path = Some(target.clone());
        self.modified = false;
        Ok(target)
    }

    pub fn is_modified(&self) -> bool { self.modified }

    /// Treat the map as it stands as a starting point rather than as work.
    ///
    /// The starter room is built by the same calls a designer would make, so
    /// it arrives looking like unsaved changes. Without this, a fresh editor
    /// asks whether to throw away a room nobody made, every time -- and a
    /// question that is always wrong is one people learn to click through,
    /// including the time it is right.
    pub fn mark_clean(&mut self) { self.modified = false; }

    /// A counter that changes whenever the map does. See [`Document::revision`].
    pub fn revision(&self) -> u64 { self.revision }
    pub fn undo_depth(&self) -> usize { self.undo.len() }
    pub fn redo_depth(&self) -> usize { self.redo.len() }

    /// Name of the change that would be undone next.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.0.as_str())
    }

    /// Run an edit, recording it in the history.
    ///
    /// The snapshot is taken *before* the closure runs, so undo restores the
    /// state the user was looking at when they started.
    pub fn apply<T>(&mut self, label: impl Into<String>, edit: impl FnOnce(&mut Document) -> T) -> T {
        self.undo.push(Snapshot {
            map: self.map.clone(),
            selection: self.selection.clone(),
            label: EditLabel::new(label),
        });
        if self.undo.len() > MAX_UNDO { self.undo.remove(0); }
        // A new edit invalidates anything that was redoable.
        self.redo.clear();
        self.modified = true;
        self.revision += 1;
        edit(self)
    }

    pub fn undo(&mut self) -> Option<String> {
        let snapshot = self.undo.pop()?;
        self.redo.push(Snapshot {
            map: std::mem::replace(&mut self.map, snapshot.map),
            selection: std::mem::replace(&mut self.selection, snapshot.selection),
            label: snapshot.label.clone(),
        });
        self.modified = true;
        self.revision += 1;
        Some(snapshot.label.0)
    }

    pub fn redo(&mut self) -> Option<String> {
        let snapshot = self.redo.pop()?;
        self.undo.push(Snapshot {
            map: std::mem::replace(&mut self.map, snapshot.map),
            selection: std::mem::replace(&mut self.selection, snapshot.selection),
            label: snapshot.label.clone(),
        });
        self.modified = true;
        self.revision += 1;
        Some(snapshot.label.0)
    }

    // ---- editing ---------------------------------------------------------

    /// Create a box brush, snapped to the grid, and select it.
    pub fn create_block(&mut self, min: Vec3, max: Vec3) -> u32 {
        let (lo, hi) = self.grid.snap_box(min.min(max), min.max(max));
        let material = self.current_material.clone();
        self.apply("create block", move |doc| {
            let solid = Solid::cube(Aabb::new(lo, hi), &material);
            let id = doc.map.add_world_solid(solid);
            doc.selection.clear();
            doc.selection.solids.insert(id);
            id
        })
    }

    /// Add a group of brushes as one edit, and select them.
    ///
    /// One undo step for the whole group, because a sixteen-segment arch is
    /// one thing a person made and sixteen presses of ctrl-Z to take it back
    /// is not undo, it is punishment.
    pub fn create_shape(&mut self, solids: Vec<Solid>, label: &str) -> Vec<u32> {
        if solids.is_empty() { return Vec::new() }
        let label = label.to_string();
        self.apply(format!("create {label}"), move |doc| {
            doc.selection.clear();
            solids
                .into_iter()
                .map(|solid| {
                    let id = doc.map.add_world_solid(solid);
                    doc.selection.solids.insert(id);
                    id
                })
                .collect()
        })
    }

    /// Create a point entity at a position.
    pub fn create_entity(&mut self, classname: &str, position: Vec3) -> u32 {
        let position = self.grid.snap_point(position);
        let classname = classname.to_string();
        self.apply(format!("create {classname}"), move |doc| {
            let id = doc.map.next_id();
            let mut entity = Entity::new(id, &classname);
            entity.set_origin(position);
            doc.map.entities.push(entity);
            doc.selection.clear();
            doc.selection.entities.insert(id);
            id
        })
    }

    /// Delete everything selected.
    pub fn delete_selection(&mut self) -> usize {
        if self.selection.is_empty() { return 0; }
        self.apply("delete", |doc| {
            let solids = doc.selection.solids.clone();
            let entities = doc.selection.entities.clone();

            doc.map.world.solids.retain(|s| !solids.contains(&s.id));
            for entity in &mut doc.map.entities {
                entity.solids.retain(|s| !solids.contains(&s.id));
            }
            // A brush entity with no brushes left is a ghost; remove it too.
            doc.map
                .entities
                .retain(|e| !entities.contains(&e.id) && !(e.solids.is_empty() && e.get("origin").is_none()));

            let count = solids.len() + entities.len();
            doc.selection.clear();
            count
        })
    }

    /// Move everything selected.
    pub fn move_selection(&mut self, delta: Vec3) {
        if self.selection.is_empty() || delta == Vec3::ZERO { return; }
        let delta = self.grid.snap_point(delta);
        if delta == Vec3::ZERO { return; }

        self.apply("move", |doc| {
            let solids = doc.selection.solids.clone();
            let entities = doc.selection.entities.clone();

            for solid in doc.map.world.solids.iter_mut() {
                if solids.contains(&solid.id) { solid.translate(delta); }
            }
            for entity in doc.map.entities.iter_mut() {
                let selected = entities.contains(&entity.id);
                for solid in entity.solids.iter_mut() {
                    // A brush entity moves as a unit when the entity is
                    // selected, or brush by brush when its brushes are.
                    if selected || solids.contains(&solid.id) { solid.translate(delta); }
                }
                if selected {
                    if let Some(origin) = entity.get_vec3("origin") {
                        entity.set_origin(origin + delta);
                    }
                }
            }
        });
    }

    /// Scale everything selected about a point.
    ///
    /// The counterpart to [`Document::move_selection`], and the thing a
    /// resize handle does. A factor of exactly one on an axis leaves that
    /// axis alone, which is what dragging an edge handle rather than a corner
    /// asks for.
    ///
    /// A point entity has no size, so it is *moved* by the same transform
    /// rather than scaled: scaling a group of lights should spread them out,
    /// not leave them where they were while the walls slide past.
    pub fn scale_selection(&mut self, anchor: Vec3, factor: Vec3) {
        if self.selection.is_empty() || factor == Vec3::ONE { return }
        // Zero collapses a brush to nothing, and there is no undoing that in
        // any way a person would recognise as undoing -- the brush is still
        // there, and infinitely thin.
        if factor.x == 0.0 || factor.y == 0.0 || factor.z == 0.0 { return }

        self.apply("resize", |doc| {
            let solids = doc.selection.solids.clone();
            let entities = doc.selection.entities.clone();

            for solid in doc.map.world.solids.iter_mut() {
                if solids.contains(&solid.id) { solid.scale(anchor, factor); }
            }
            for entity in doc.map.entities.iter_mut() {
                let selected = entities.contains(&entity.id);
                for solid in entity.solids.iter_mut() {
                    if selected || solids.contains(&solid.id) { solid.scale(anchor, factor); }
                }
                if selected
                    && let Some(origin) = entity.get_vec3("origin")
                {
                    entity.set_origin(anchor + (origin - anchor) * factor);
                }
            }
        });
    }

    /// Apply the current material to every selected face, or to every face of
    /// every selected brush when no individual faces are picked.
    pub fn apply_material(&mut self) -> usize {
        let material = self.current_material.clone();
        self.apply(format!("apply {material}"), move |doc| {
            let faces = doc.selection.faces.clone();
            let solids = doc.selection.solids.clone();
            let mut changed = 0;

            for solid in all_solids_mut(&mut doc.map) {
                if solids.contains(&solid.id) {
                    solid.set_material(&material);
                    changed += solid.sides.len();
                    continue;
                }
                for side in solid.sides.iter_mut() {
                    if faces.contains(&(solid.id, side.id)) {
                        side.material = material.clone();
                        changed += 1;
                    }
                }
            }
            changed
        })
    }

    // ---- face editing ----------------------------------------------------

    pub fn selected_face_count(&self) -> usize { self.selection.faces.len() }

    /// The selected faces, each with the plane and winding it sits on.
    ///
    /// Copies rather than borrows, because every caller is a panel that wants
    /// to read the numbers and then hand back an edit -- and holding a borrow
    /// across that is the shape of code that cannot compile.
    pub fn selected_face_specs(&self) -> Vec<FaceSpec> {
        let mut out = Vec::with_capacity(self.selection.faces.len());
        for (_, solid) in self.map.all_solids() {
            for side in &solid.sides {
                if !self.selection.faces.contains(&(solid.id, side.id)) { continue }
                let Some((plane, winding)) = crate::faces::winding_of(solid, side.id) else {
                    continue;
                };
                out.push(FaceSpec {
                    solid: solid.id,
                    side: side.clone(),
                    plane,
                    winding,
                });
            }
        }
        // Stable order, so a panel showing "the first selected face" shows the
        // same one from frame to frame.
        out.sort_by_key(|f| (f.solid, f.side.id));
        out
    }

    /// Apply an edit to every selected face, as one undo step.
    ///
    /// One step for the whole selection rather than one per face: a designer
    /// who nudged a texture across six faces means one nudge, and six presses
    /// of ctrl-Z to take it back is a bug in everything but name.
    ///
    /// The winding is computed before the edit and passed in, because every
    /// operation that needs it -- fit, justify, rotate about the centre --
    /// wants the face's shape as it is now, and the shape does not change.
    pub fn edit_faces(
        &mut self,
        label: impl Into<String>,
        edit: impl Fn(&mut Side, &Plane, &Winding),
    ) -> usize {
        if self.selection.faces.is_empty() { return 0 }

        // Worked out first: `all_solids_mut` hands out one solid at a time, and
        // a face's winding needs the whole solid.
        let shapes: std::collections::HashMap<(u32, u32), (Plane, Winding)> = self
            .selected_face_specs()
            .into_iter()
            .map(|f| ((f.solid, f.side.id), (f.plane, f.winding)))
            .collect();
        if shapes.is_empty() { return 0 }

        self.apply(label, move |doc| {
            let faces = doc.selection.faces.clone();
            let mut changed = 0;
            for solid in all_solids_mut(&mut doc.map) {
                for side in solid.sides.iter_mut() {
                    if !faces.contains(&(solid.id, side.id)) { continue }
                    let Some((plane, winding)) = shapes.get(&(solid.id, side.id)) else { continue };
                    edit(side, plane, winding);
                    changed += 1;
                }
            }
            changed
        })
    }

    /// Turn the selected brushes into a brush entity of the given class.
    ///
    /// This is how a designer makes a door: build the brush in the world, then
    /// tie it to a `func_door`.
    pub fn tie_to_entity(&mut self, classname: &str) -> Option<u32> {
        if self.selection.solids.is_empty() { return None; }
        let classname = classname.to_string();
        self.apply(format!("tie to {classname}"), move |doc| {
            let selected = doc.selection.solids.clone();
            let mut moved: Vec<Solid> = Vec::new();

            doc.map.world.solids.retain(|s| {
                if selected.contains(&s.id) {
                    moved.push(s.clone());
                    false
                } else {
                    true
                }
            });
            for entity in doc.map.entities.iter_mut() {
                entity.solids.retain(|s| {
                    if selected.contains(&s.id) {
                        moved.push(s.clone());
                        false
                    } else {
                        true
                    }
                });
            }
            if moved.is_empty() { return None; }

            let id = doc.map.next_id();
            let mut entity = Entity::new(id, &classname);
            entity.solids = moved;
            doc.map.entities.push(entity);

            doc.selection.clear();
            doc.selection.entities.insert(id);
            Some(id)
        })
    }

    /// Every solid the current selection is about.
    ///
    /// Either the brushes picked directly, or the brushes of a picked brush
    /// entity -- clicking a door selects the door, and "what am I editing" has
    /// to mean the same thing either way.
    pub fn selected_solid_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.selection.solids.iter().copied().collect();
        for entity in &self.map.entities {
            if self.selection.entities.contains(&entity.id) {
                ids.extend(entity.solids.iter().map(|s| s.id));
            }
        }
        ids.sort();
        ids.dedup();
        ids
    }

    /// The brush entity the selection belongs to, if it belongs to exactly one.
    pub fn selected_brush_class(&self) -> Option<(u32, String)> {
        let ids = self.selected_solid_ids();
        if ids.is_empty() { return None }
        let mut found: Option<(u32, String)> = None;
        for entity in &self.map.entities {
            if entity.solids.is_empty() { continue }
            if !entity.solids.iter().any(|s| ids.contains(&s.id)) { continue }
            match &found {
                // Two different entities: no single answer.
                Some((id, _)) if *id != entity.id => return None,
                Some(_) => {}
                None => found = Some((entity.id, entity.classname().to_string())),
            }
        }
        found
    }

    /// Set what the selected brushes *are*.
    ///
    /// `None` puts them back in the world. This is one operation rather than
    /// an untie followed by a tie, because a designer changing a trigger into
    /// a door is doing one thing and expects one press of ctrl-Z to undo it.
    ///
    /// Keys the new class does not have are kept rather than dropped. A
    /// `targetname` should survive changing a door into a platform, and the
    /// compiler ignores keys nothing reads -- whereas throwing away a name
    /// silently breaks every output wired to it.
    pub fn set_brush_class(&mut self, class: Option<&str>) -> bool {
        let ids = self.selected_solid_ids();
        if ids.is_empty() { return false }
        let current = self.selected_brush_class();

        match (class, &current) {
            // Already what it is.
            (Some(want), Some((_, have))) if want == have => return false,
            (None, None) => return false,
            _ => {}
        }

        let label = match class {
            Some(class) => format!("make it a {class}"),
            None => "make it world geometry".to_string(),
        };
        let class = class.map(str::to_string);

        self.apply(label, move |doc| {
            // Collect the brushes wherever they are, world or entity, and
            // leave nothing behind: an entity emptied of brushes is a ghost
            // that still shows up in the compiler's entity lump.
            let mut moved: Vec<Solid> = Vec::new();
            let mut keys: Vec<(String, String)> = Vec::new();
            let mut connections = Vec::new();

            doc.map.world.solids.retain(|s| {
                if ids.contains(&s.id) { moved.push(s.clone()); false } else { true }
            });
            doc.map.entities.retain_mut(|e| {
                if e.solids.is_empty() { return true }
                let mine = e.solids.iter().any(|s| ids.contains(&s.id));
                if !mine { return true }
                e.solids.retain(|s| {
                    if ids.contains(&s.id) { moved.push(s.clone()); false } else { true }
                });
                if e.solids.is_empty() {
                    // Its keys and wiring come with the brushes.
                    keys = e.properties.clone();
                    connections = std::mem::take(&mut e.connections);
                    return false;
                }
                true
            });
            if moved.is_empty() { return false }

            doc.selection.clear();
            match class {
                None => {
                    for solid in moved {
                        let id = doc.map.add_world_solid(solid);
                        doc.selection.solids.insert(id);
                    }
                }
                Some(class) => {
                    // A class that knows what its brushes should look like
                    // says so now, rather than leaving a trigger visible and
                    // solid until someone remembers to texture it.
                    if let Some(material) = crate::brush::material_for_class(&class) {
                        for solid in &mut moved { solid.set_material(material) }
                    }

                    let id = doc.map.next_id();
                    let mut entity = Entity::new(id, &class);
                    for (key, value) in keys {
                        if key == "classname" { continue }
                        entity.set(&key, value);
                    }
                    entity.connections = connections;
                    entity.solids = moved;
                    doc.map.entities.push(entity);
                    doc.selection.entities.insert(id);
                }
            }
            true
        })
    }

    /// Move a brush entity's brushes back into the world.
    pub fn untie_to_world(&mut self) -> usize {
        if self.selection.entities.is_empty() { return 0; }
        self.apply("move to world", |doc| {
            let selected = doc.selection.entities.clone();
            let mut freed = Vec::new();

            doc.map.entities.retain_mut(|e| {
                if !selected.contains(&e.id) || e.solids.is_empty() { return true; }
                freed.append(&mut e.solids);
                false
            });

            let count = freed.len();
            doc.map.world.solids.extend(freed);
            doc.selection.clear();
            count
        })
    }

    // ---- queries ---------------------------------------------------------

    /// Bounds of everything selected.
    pub fn selection_bounds(&self) -> Option<Aabb> {
        let mut bounds = Aabb::EMPTY;
        for (entity, solid) in self.map.all_solids() {
            if self.selection.solids.contains(&solid.id)
                || self.selection.entities.contains(&entity.id)
            {
                bounds = bounds.union(&solid.bounds());
            }
        }
        for entity in self.map.all_entities() {
            if self.selection.entities.contains(&entity.id) && entity.solids.is_empty() {
                // Point entities have no geometry, so give them a small box to
                // select and drag by.
                let o = entity.origin();
                bounds = bounds.union(&Aabb::from_center_half(o, Vec3::splat(8.0)));
            }
        }
        (!bounds.is_empty()).then_some(bounds)
    }

    pub fn find_solid(&self, id: u32) -> Option<&Solid> { self.map.find_solid(id) }

    pub fn find_entity(&self, id: u32) -> Option<&Entity> {
        self.map.all_entities().find(|e| e.id == id)
    }

    pub fn find_entity_mut(&mut self, id: u32) -> Option<&mut Entity> {
        std::iter::once(&mut self.map.world)
            .chain(self.map.entities.iter_mut())
            .find(|e| e.id == id)
    }

    /// Problems that would stop the map compiling, for the status bar.
    pub fn problems(&self) -> Vec<String> {
        self.map.validate().iter().map(|p| p.to_string()).collect()
    }

    /// A one-line summary for the title bar.
    ///
    /// A map with no path is "untitled", not "untitled.voidmap": the second
    /// looks like a file, and a name that looks like a file is why saving
    /// appeared to have already happened.
    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".to_string());
        if self.modified { format!("{name} *") } else { name }
    }
}

/// Every solid in the map, mutably.
/// One selected face, with everything a panel needs to describe it.
#[derive(Clone, Debug)]
pub struct FaceSpec {
    pub solid: u32,
    pub side: Side,
    pub plane: Plane,
    pub winding: Winding,
}

fn all_solids_mut(map: &mut Map) -> impl Iterator<Item = &mut Solid> {
    std::iter::once(&mut map.world)
        .chain(map.entities.iter_mut())
        .flat_map(|e| e.solids.iter_mut())
}

#[cfg(test)]
mod tests;
