// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Clip, carve, hollow, rotate, flip and align, on whatever is selected.
//!
//! The geometry itself is in `kerosene_map` ([`Solid::clip`] and friends);
//! this is the part that knows what a selection is -- which brushes it
//! covers, that a brush entity's brushes go with it, that a point entity
//! turns with the rest -- and that every one of these is one undo step.

use super::Document;
use kerosene_map::Solid;
use kerosene_math::{Aabb, Plane, Quat, Vec3};

/// Which side of the clipping plane to keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClipMode {
    /// Keep what is in front of the plane (the side its normal points to).
    Front,
    /// Keep what is behind it.
    Back,
    /// Keep both, as two brushes.
    #[default]
    Both,
}

impl ClipMode {
    pub fn next(self) -> ClipMode {
        match self {
            ClipMode::Both => ClipMode::Front,
            ClipMode::Front => ClipMode::Back,
            ClipMode::Back => ClipMode::Both,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ClipMode::Front => "keep front",
            ClipMode::Back => "keep back",
            ClipMode::Both => "keep both",
        }
    }
}

impl Document {
    /// Replace each selected brush with what `cut` makes of it, keeping the
    /// pieces where the original was (world or entity) and selecting them.
    /// Returns how many pieces there are now.
    fn replace_selected_solids(
        &mut self,
        label: &str,
        cut: impl Fn(&Solid) -> Vec<Solid>,
    ) -> usize {
        let mut made = 0;
        self.apply(label, |doc| {
            let selected_entities = doc.selection.entities.clone();
            let selected_solids = doc.selection.solids.clone();
            let mut new_world: Vec<u32> = Vec::new();
            let owners: Vec<Option<u32>> = std::iter::once(None)
                .chain(doc.map.entities.iter().map(|e| Some(e.id)))
                .collect();
            for owner in owners {
                let owner_id = owner.unwrap_or(doc.map.world.id);
                let solids = match owner {
                    None => std::mem::take(&mut doc.map.world.solids),
                    Some(id) => std::mem::take(
                        &mut doc
                            .map
                            .entities
                            .iter_mut()
                            .find(|e| e.id == id)
                            .expect("listed above")
                            .solids,
                    ),
                };
                let mut kept = Vec::with_capacity(solids.len());
                for solid in solids {
                    let selected = selected_solids.contains(&solid.id)
                        || selected_entities.contains(&owner_id);
                    if !selected {
                        kept.push(solid);
                        continue;
                    }
                    for mut piece in cut(&solid) {
                        doc.map.assign_ids(&mut piece);
                        piece.editor = solid.editor.clone();
                        piece.properties = solid.properties.clone();
                        if owner.is_none() {
                            new_world.push(piece.id);
                        }
                        kept.push(piece);
                        made += 1;
                    }
                }
                match owner {
                    None => doc.map.world.solids = kept,
                    Some(id) => {
                        doc.map
                            .entities
                            .iter_mut()
                            .find(|e| e.id == id)
                            .expect("listed above")
                            .solids = kept
                    }
                }
            }
            // The pieces of world brushes are the selection now; a brush
            // entity stays selected as itself.
            doc.selection.solids = new_world.into_iter().collect();
            doc.selection.faces.clear();
            // A brush entity left with no brushes is gone.
            let empty: Vec<u32> = doc
                .map
                .entities
                .iter()
                .filter(|e| e.solids.is_empty() && !e.has("origin"))
                .map(|e| e.id)
                .collect();
            for id in empty {
                doc.map.remove_entity(id);
                doc.selection.entities.remove(&id);
            }
        });
        made
    }

    /// Cut every selected brush along `plane`.
    pub fn clip_selection(&mut self, plane: Plane, mode: ClipMode) -> usize {
        if self.selection.solids.is_empty() && self.selection.entities.is_empty() {
            return 0;
        }
        self.replace_selected_solids("clip", |solid| {
            let (behind, front) = solid.clip(&plane);
            match mode {
                ClipMode::Both => behind.into_iter().chain(front).collect(),
                ClipMode::Front => front.into_iter().collect(),
                ClipMode::Back => behind.into_iter().collect(),
            }
        })
    }

    /// Take the selected world brushes out of every visible world brush they
    /// overlap, then delete them. Returns how many brushes were carved.
    pub fn carve_selection(&mut self) -> usize {
        let carvers: Vec<Solid> = self
            .map
            .world
            .solids
            .iter()
            .filter(|s| self.selection.solids.contains(&s.id))
            .cloned()
            .collect();
        if carvers.is_empty() {
            return 0;
        }
        let visible: Vec<u32> = self
            .visible_solids()
            .filter(|(owner, _)| owner.classname() == "worldspawn")
            .map(|(_, s)| s.id)
            .collect();
        let carver_ids: Vec<u32> = carvers.iter().map(|s| s.id).collect();
        let mut carved = 0;
        self.apply("carve", |doc| {
            let targets = std::mem::take(&mut doc.map.world.solids);
            let mut kept = Vec::with_capacity(targets.len());
            for solid in targets {
                if carver_ids.contains(&solid.id) {
                    continue;
                }
                let touched =
                    visible.contains(&solid.id) && carvers.iter().any(|c| c.overlaps(&solid));
                if !touched {
                    kept.push(solid);
                    continue;
                }
                carved += 1;
                let mut pieces = vec![solid.clone()];
                for carver in &carvers {
                    pieces = pieces.iter().flat_map(|p| p.subtract(carver)).collect();
                }
                for mut piece in pieces {
                    doc.map.assign_ids(&mut piece);
                    piece.editor = solid.editor.clone();
                    piece.properties = solid.properties.clone();
                    kept.push(piece);
                }
            }
            doc.map.world.solids = kept;
            doc.selection.clear();
        });
        carved
    }

    /// Turn each selected brush into walls.
    pub fn hollow_selection(&mut self, thickness: f32) -> usize {
        if self.selection.solids.is_empty() && self.selection.entities.is_empty() {
            return 0;
        }
        self.replace_selected_solids("hollow", |solid| solid.hollow(thickness))
    }

    /// Turn the selection about a point.
    pub fn rotate_selection(&mut self, pivot: Vec3, rotation: Quat) {
        if self.selection.is_empty() || rotation == Quat::IDENTITY {
            return;
        }
        self.apply("rotate", |doc| {
            let solids = doc.selection.solids.clone();
            let entities = doc.selection.entities.clone();
            for solid in doc.map.world.solids.iter_mut() {
                if solids.contains(&solid.id) {
                    solid.rotate(pivot, rotation);
                }
            }
            for entity in doc.map.entities.iter_mut() {
                let selected = entities.contains(&entity.id);
                for solid in entity.solids.iter_mut() {
                    if selected || solids.contains(&solid.id) {
                        solid.rotate(pivot, rotation);
                    }
                }
                if selected {
                    if let Some(origin) = entity.get_vec3("origin") {
                        entity.set_origin(pivot + rotation * (origin - pivot));
                    }
                    // A turn about the up axis is a yaw; anything else has
                    // no honest expression in pitch/yaw/roll for every
                    // class, so the entity keeps facing the way it did.
                    let (axis, angle) = rotation.to_axis_angle();
                    if axis.z.abs() > 0.999 && entity.has("angles") {
                        let mut angles = entity.angles();
                        angles.yaw += angle.to_degrees() * axis.z.signum();
                        entity.set_angles(angles);
                    }
                }
            }
        });
    }

    /// Mirror the selection across its centre, along one axis.
    pub fn flip_selection(&mut self, axis: usize) {
        let Some(bounds) = self.selection_bounds() else {
            return;
        };
        let mut factor = Vec3::ONE;
        factor[axis] = -1.0;
        let centre = bounds.center();
        self.apply("flip", |doc| {
            let solids = doc.selection.solids.clone();
            let entities = doc.selection.entities.clone();
            for solid in doc.map.world.solids.iter_mut() {
                if solids.contains(&solid.id) {
                    solid.scale(centre, factor);
                }
            }
            for entity in doc.map.entities.iter_mut() {
                let selected = entities.contains(&entity.id);
                for solid in entity.solids.iter_mut() {
                    if selected || solids.contains(&solid.id) {
                        solid.scale(centre, factor);
                    }
                }
                if selected && let Some(origin) = entity.get_vec3("origin") {
                    entity.set_origin(centre + (origin - centre) * factor);
                }
            }
        });
    }

    /// Move the selection so its lowest corner sits on the grid.
    pub fn align_selection_to_grid(&mut self) -> Vec3 {
        let Some(bounds) = self.selection_bounds() else {
            return Vec3::ZERO;
        };
        let grid = self.grid.size;
        let target = (bounds.min / grid).round() * grid;
        let delta = target - bounds.min;
        if delta.length() < 1e-4 {
            return Vec3::ZERO;
        }
        let snap = std::mem::replace(&mut self.grid.snap, false);
        self.move_selection(delta);
        self.grid.snap = snap;
        delta
    }

    /// The bounds of the selection, for a pivot.
    pub fn selection_centre(&self) -> Option<Vec3> {
        self.selection_bounds().as_ref().map(Aabb::center)
    }
}
