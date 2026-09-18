// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The property inspector: what an entity's keys are, and how to edit them.
//!
//! Before this existed the inspector could only list the keys an entity
//! already carried, which for a newly placed entity is `classname` and
//! nothing else -- so half the classes in the game appeared to have no
//! settings at all. The keys were there; there was just no way to find out
//! their names short of reading the game's source.
//!
//! Now the game's [`Schema`] drives the panel: every key a class reads is
//! shown, with its type, its default and a line of help, whether or not the
//! entity has been given a value for it yet.
//!
//! The row-building is separated from the drawing because "which properties
//! should this entity show" is a question with a testable answer, and the two
//! rules that matter are easy to get wrong: an unset key must still appear,
//! and a key the schema has never heard of must not disappear.
//!
//! Since every object in a map carries key-values -- a brush and a face as
//! much as an entity -- the rows are built for a [`Target`] rather than for
//! an entity, and several targets at once merge into one set of rows: a key
//! they disagree on is shown as *mixed* and left alone unless someone types
//! over it, which is how a speed is set on six doors in one go.

use crate::document::Document;
use kerosene_entity::{ClassSpec, KeyKind, Schema};
use kerosene_map::{Entity, Side, Solid};

/// Something whose key-values can be edited, by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetId {
    Entity(u32),
    Solid(u32),
    /// `(solid id, side id)`.
    Face(u32, u32),
}

/// A borrowed target, with the class definition when it has one.
#[derive(Clone, Copy, Debug)]
pub enum Target<'a> {
    Entity(&'a Entity, Option<&'a ClassSpec>),
    Solid(&'a Solid),
    Face(&'a Side),
}

impl<'a> Target<'a> {
    /// Resolve an id against the document.
    pub fn resolve(document: &'a Document, schema: &'a Schema, id: TargetId) -> Option<Target<'a>> {
        match id {
            TargetId::Entity(id) => {
                let e = document.find_entity(id)?;
                Some(Target::Entity(e, schema.get(e.classname())))
            }
            TargetId::Solid(id) => document.map.find_solid(id).map(Target::Solid),
            TargetId::Face(solid, side) => document
                .map
                .find_solid(solid)?
                .sides
                .iter()
                .find(|s| s.id == side)
                .map(Target::Face),
        }
    }

    fn pairs(&self) -> &'a [(String, String)] {
        match self {
            Target::Entity(e, _) => &e.properties,
            Target::Solid(s) => &s.properties,
            Target::Face(f) => &f.properties,
        }
    }
}

/// One line of the property panel.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyRow {
    pub key: String,
    pub label: String,
    pub kind: KeyKind,
    pub help: String,
    pub choices: Vec<(String, String)>,
    /// What the game assumes when the key is absent.
    pub default: String,
    /// The value on the entity. `None` means the key is not set, and the
    /// default applies -- which is a different thing from being set to the
    /// same text, and the map records the difference.
    pub value: Option<String>,
    /// False for a key the entity carries that the schema does not describe.
    /// Those are kept and shown rather than hidden: an unknown key is usually
    /// a typo or a key from a newer game, and silently dropping either would
    /// be worse than showing it.
    pub described: bool,
    /// The key as it was when the row was built, so renaming a custom key
    /// removes the old one instead of leaving both behind.
    pub original_key: String,
    /// Several objects are being edited and they do not agree on this key.
    /// The row shows a placeholder and is written back only once someone
    /// has typed over it.
    pub mixed: bool,
    /// Someone changed this row. A mixed row that nobody touched is left as
    /// it was on every object.
    pub dirty: bool,
}

impl PropertyRow {
    /// A row for a key nothing describes.
    pub fn custom(key: &str, value: Option<String>) -> PropertyRow {
        PropertyRow {
            key: key.to_string(),
            label: key.to_string(),
            kind: KeyKind::String,
            help: String::new(),
            choices: Vec::new(),
            default: String::new(),
            value,
            described: false,
            original_key: key.to_string(),
            mixed: false,
            dirty: false,
        }
    }

    /// The text to edit: the value if set, otherwise the default.
    pub fn text(&self) -> &str {
        self.value.as_deref().unwrap_or(&self.default)
    }

    pub fn is_set(&self) -> bool {
        self.value.is_some()
    }

    /// Change the value, marking the row as touched.
    pub fn set(&mut self, value: Option<String>) {
        self.value = value;
        self.dirty = true;
        self.mixed = false;
    }
}

/// Build the rows for an entity: every key its class defines, then every key
/// it carries that the class does not.
pub fn rows(spec: Option<&ClassSpec>, entity: &Entity) -> Vec<PropertyRow> {
    rows_for(Target::Entity(entity, spec))
}

/// Build the rows for any target. An entity gets its class's keys first; a
/// brush or a face has no class, so its rows are exactly the keys it carries.
pub fn rows_for(target: Target) -> Vec<PropertyRow> {
    let mut rows: Vec<PropertyRow> = Vec::new();

    if let Target::Entity(entity, Some(spec)) = target {
        for key in &spec.keys {
            rows.push(PropertyRow {
                key: key.name.clone(),
                label: key.label.clone(),
                kind: key.kind,
                help: key.help.clone(),
                choices: key.choices.clone(),
                default: key.default.clone(),
                value: entity.get(&key.name).map(str::to_string),
                described: true,
                original_key: key.name.clone(),
                mixed: false,
                dirty: false,
            });
        }
    }

    for (key, value) in target.pairs() {
        // `classname` is the entity's identity, changed by retying rather than
        // by typing, so it is shown as a heading instead of as a row.
        if key.eq_ignore_ascii_case("classname") {
            continue;
        }
        if rows.iter().any(|r| r.key.eq_ignore_ascii_case(key)) {
            continue;
        }
        rows.push(PropertyRow::custom(key, Some(value.clone())));
    }

    rows
}

/// Rows for several targets at once: the union of their keys, in the order
/// the first target lists them, with any key they disagree on marked mixed.
pub fn merged_rows(targets: &[Target]) -> Vec<PropertyRow> {
    let per_target: Vec<Vec<PropertyRow>> = targets.iter().map(|t| rows_for(*t)).collect();
    let mut merged: Vec<PropertyRow> = Vec::new();
    for rows in &per_target {
        for row in rows {
            match merged
                .iter_mut()
                .find(|m| m.key.eq_ignore_ascii_case(&row.key))
            {
                Some(existing) => {
                    if existing.value != row.value {
                        existing.mixed = true;
                    }
                    // The schema's description wins over a plain custom row,
                    // whichever target contributed it.
                    if row.described && !existing.described {
                        existing.described = true;
                        existing.label = row.label.clone();
                        existing.kind = row.kind;
                        existing.help = row.help.clone();
                        existing.choices = row.choices.clone();
                        existing.default = row.default.clone();
                    }
                }
                None => merged.push(row.clone()),
            }
        }
    }
    // A key one target carries and another does not is a disagreement too.
    for row in &mut merged {
        if row.is_set()
            && per_target.iter().any(|rows| {
                !rows
                    .iter()
                    .any(|r| r.key.eq_ignore_ascii_case(&row.key) && r.is_set())
            })
        {
            row.mixed = true;
        }
    }
    merged
}

/// Write edited rows back onto an entity.
///
/// A row left unset removes the key rather than writing the default into it,
/// so a map only carries the keys someone actually chose. That keeps a diff
/// between two saves readable, which matters more than it sounds: it is how
/// you find what a change actually did.
pub fn apply(entity: &mut Entity, rows: &[PropertyRow]) {
    apply_pairs(&mut entity.properties, rows, false);
}

/// Write rows onto any key-value list.
///
/// With `touched_only`, a row nobody edited is skipped -- the rule for a
/// multi-object edit, where writing every row would stamp the first
/// object's values over the rest. A renamed key takes its old name with it.
pub fn apply_pairs(pairs: &mut Vec<(String, String)>, rows: &[PropertyRow], touched_only: bool) {
    fn set(pairs: &mut Vec<(String, String)>, key: &str, value: String) {
        match pairs.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
            Some((_, v)) => *v = value,
            None => pairs.push((key.to_string(), value)),
        }
    }
    fn remove(pairs: &mut Vec<(String, String)>, key: &str) {
        pairs.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    }
    for row in rows {
        if row.mixed && !row.dirty {
            continue;
        }
        if touched_only && !row.dirty {
            continue;
        }
        if !row.original_key.is_empty() && !row.original_key.eq_ignore_ascii_case(&row.key) {
            remove(pairs, &row.original_key);
        }
        let key = row.key.trim();
        if key.is_empty() {
            continue;
        }
        match &row.value {
            Some(value) => set(pairs, key, value.clone()),
            None => remove(pairs, key),
        }
    }
}

/// Write rows onto a target in the document.
pub fn apply_to(document: &mut Document, id: TargetId, rows: &[PropertyRow], touched_only: bool) {
    match id {
        TargetId::Entity(id) => {
            if let Some(e) = document.find_entity_mut(id) {
                apply_pairs(&mut e.properties, rows, touched_only);
            }
        }
        TargetId::Solid(id) => {
            if let Some(s) = document.map.find_solid_mut(id) {
                apply_pairs(&mut s.properties, rows, touched_only);
            }
        }
        TargetId::Face(solid, side) => {
            if let Some(f) = document
                .map
                .find_solid_mut(solid)
                .and_then(|s| s.sides.iter_mut().find(|f| f.id == side))
            {
                apply_pairs(&mut f.properties, rows, touched_only);
            }
        }
    }
}

/// Every name in the map that an output could address, sorted and deduplicated.
pub fn target_names(document: &Document) -> Vec<String> {
    let mut names: Vec<String> = document
        .map
        .all_entities()
        .filter_map(|e| e.targetname())
        .filter(|n| !n.trim().is_empty())
        .map(str::to_string)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The inputs an output aimed at `target` could fire.
///
/// Several entities may share a name -- that is how one output drives a whole
/// group -- so the answer is the union of what all of them accept. The special
/// `!activator`-style targets are not names in the map, so nothing is known
/// about them and the editor falls back to free text.
pub fn inputs_for_target(schema: &Schema, document: &Document, target: &str) -> Vec<String> {
    let mut inputs: Vec<String> = Vec::new();
    for entity in document.map.all_entities() {
        if entity.targetname() != Some(target) {
            continue;
        }
        let Some(spec) = schema.get(entity.classname()) else {
            continue;
        };
        for input in &spec.inputs {
            if !inputs.iter().any(|i| i.eq_ignore_ascii_case(&input.name)) {
                inputs.push(input.name.clone());
            }
        }
    }
    inputs
}

/// Split a colour keyvalue -- `"255 240 220 300"` -- into RGB and brightness.
///
/// Brightness is a fourth number rather than a scale on the first three
/// because a light can be brighter than white, and clamping it into a byte
/// would quietly cap every bright light in a map at the same value.
pub fn parse_color(text: &str) -> ([u8; 3], f32) {
    let mut parts = text
        .split_whitespace()
        .filter_map(|p| p.parse::<f32>().ok());
    let r = parts.next().unwrap_or(255.0);
    let g = parts.next().unwrap_or(255.0);
    let b = parts.next().unwrap_or(255.0);
    let brightness = parts.next().unwrap_or(200.0);
    ([clamp_byte(r), clamp_byte(g), clamp_byte(b)], brightness)
}

pub fn format_color(rgb: [u8; 3], brightness: f32) -> String {
    format!(
        "{} {} {} {}",
        rgb[0],
        rgb[1],
        rgb[2],
        kerosene_kv::format_float(brightness)
    )
}

fn clamp_byte(v: f32) -> u8 {
    v.clamp(0.0, 255.0) as u8
}

/// Parse a vector keyvalue, tolerating the several ways one gets written.
pub fn parse_vec3(text: &str) -> [f32; 3] {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c == ',' || c == '[' || c == ']' || c == '(' || c == ')' {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut parts = cleaned
        .split_whitespace()
        .filter_map(|p| p.parse::<f32>().ok());
    [
        parts.next().unwrap_or(0.0),
        parts.next().unwrap_or(0.0),
        parts.next().unwrap_or(0.0),
    ]
}

pub fn format_vec3(v: [f32; 3]) -> String {
    format!(
        "{} {} {}",
        kerosene_kv::format_float(v[0]),
        kerosene_kv::format_float(v[1]),
        kerosene_kv::format_float(v[2])
    )
}

#[cfg(test)]
mod tests;
