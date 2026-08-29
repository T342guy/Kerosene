// SPDX-License-Identifier: LGPL-3.0-or-later
//! What a selection of brushes is, as the compiler will see it.
//!
//! A brush has no keyvalues -- everything about it lives in its faces and its
//! geometry -- so for a long time selecting one showed a page headed
//! "properties" that contained nothing but a list of entity classes to tie it
//! to. That is not what a brush's properties are, and it left the one thing a
//! designer most needs to know unanswerable without compiling the map: *what
//! will this brush actually be?*
//!
//! The answer comes from Cleave's own material table rather than from a copy
//! of it here. An editor that explained tool textures differently from the
//! tool that acts on them would be worse than one that did not explain them.

use crate::document::Document;
use void_map::Solid;
use void_math::Aabb;

/// Everything the inspector shows for a brush selection.
#[derive(Clone, Debug, PartialEq)]
pub struct BrushInfo {
    pub brushes: usize,
    pub faces: usize,
    /// The extent of the whole selection.
    pub bounds: Aabb,
    /// Every material used, once each, in the order first seen.
    pub materials: Vec<String>,
    /// What the brushes compile as, in words.
    pub compiles_as: String,
    /// The class these brushes belong to, when they are part of a brush
    /// entity rather than the world.
    pub classname: Option<String>,
    /// Brushes whose faces do not all agree on a material.
    pub mixed_materials: bool,
}

impl BrushInfo {
    /// Describe whatever brushes are selected, or `None` if none are.
    pub fn of_selection(document: &Document) -> Option<BrushInfo> {
        // Whatever the selection is *about*: the brushes picked directly, or
        // the brushes of a picked brush entity. Clicking a door selects the
        // door, and a panel that then said "nothing selected" would be the
        // same split this panel exists to remove.
        let selected = document.selected_solid_ids();
        if selected.is_empty() { return None }

        let mut bounds = Aabb::EMPTY;
        let mut materials: Vec<String> = Vec::new();
        let mut owners: Vec<Option<String>> = Vec::new();
        let mut faces = 0;
        let mut brushes = 0;
        let mut mixed = false;

        for (entity, solid) in document.map.all_solids() {
            if !selected.contains(&solid.id) { continue }
            brushes += 1;
            faces += solid.sides.len();
            let b = solid.bounds();
            bounds.add_point(b.min);
            bounds.add_point(b.max);

            // The world is not a class. `all_solids` hands back worldspawn as
            // the owner of world brushes, and calling a wall a "worldspawn"
            // would be true and useless -- nobody thinks of a wall as an
            // entity, and the word would only invite tying it to something.
            let owner = entity.classname();
            owners.push((owner != "worldspawn").then(|| owner.to_string()));

            mixed |= !all_one_material(solid);
            for side in &solid.sides {
                if !materials.iter().any(|m| m == &side.material) {
                    materials.push(side.material.clone());
                }
            }
        }
        if brushes == 0 { return None }

        // Only when every selected brush agrees. A selection spanning a door
        // and a wall has no single class, and naming one of them would be a
        // worse answer than naming none.
        let classname = match owners.first() {
            Some(first) if owners.iter().all(|o| o == first) => first.clone(),
            _ => None,
        };

        let compiles_as = cleave::material::describe_brush(&materials, classname.as_deref());
        Some(BrushInfo {
            brushes,
            faces,
            bounds,
            materials,
            compiles_as,
            classname,
            mixed_materials: mixed,
        })
    }

    /// What each material in the selection does, paired with its name.
    pub fn material_meanings(&self) -> Vec<(&str, &'static str)> {
        self.materials
            .iter()
            .map(|m| (m.as_str(), cleave::material::describe(m)))
            .collect()
    }

    /// Materials that are `tools/` names the compiler does not recognise.
    ///
    /// A typo here does not fail the compile: the brush silently becomes
    /// ordinary world geometry, which is how a doorway gets walled off by a
    /// misspelling nobody sees.
    pub fn unknown_tools(&self) -> Vec<&str> {
        self.materials
            .iter()
            .filter(|m| m.to_lowercase().starts_with("tools/"))
            .filter(|m| !cleave::material::is_known_tool(m))
            .map(String::as_str)
            .collect()
    }
}

/// The material a class wants its brushes to wear, if it wants one.
///
/// A trigger has to be invisible: it is a region, not a wall, and one you can
/// see is one drawn across the middle of a room for no reason. In Hammer you
/// texture it yourself and the map compiles wrong -- as a solid block in a
/// doorway -- if you forget. There is no reason for that to be a thing anyone
/// has to remember, because the class already says what the brush is for.
///
/// `None` means the class has no opinion and whatever is on the faces stays.
/// A door is textured like a door, and only a designer knows which door.
pub fn material_for_class(classname: &str) -> Option<&'static str> {
    let lower = classname.to_lowercase();
    if lower.starts_with("trigger_") { return Some("tools/trigger") }
    match lower.as_str() {
        "func_areaportal" | "func_occluder" => Some("tools/nodraw"),
        // A ladder is invisible by definition, so texturing it by hand is a
        // step with exactly one right answer -- which is a step to remove.
        "func_ladder" => Some("tools/ladder"),
        _ => None,
    }
}

fn all_one_material(solid: &Solid) -> bool {
    let mut sides = solid.sides.iter();
    let Some(first) = sides.next() else { return true };
    sides.all(|s| s.material == first.material)
}

#[cfg(test)]
mod tests;
