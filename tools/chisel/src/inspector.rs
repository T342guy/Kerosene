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

use crate::document::Document;
use kerosene_entity::{ClassSpec, KeyKind, Schema};
use kerosene_map::Entity;

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
}

impl PropertyRow {
    /// The text to edit: the value if set, otherwise the default.
    pub fn text(&self) -> &str {
        self.value.as_deref().unwrap_or(&self.default)
    }

    pub fn is_set(&self) -> bool { self.value.is_some() }
}

/// Build the rows for an entity: every key its class defines, then every key
/// it carries that the class does not.
pub fn rows(spec: Option<&ClassSpec>, entity: &Entity) -> Vec<PropertyRow> {
    let mut rows: Vec<PropertyRow> = Vec::new();

    if let Some(spec) = spec {
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
            });
        }
    }

    for (key, value) in &entity.properties {
        // `classname` is the entity's identity, changed by retying rather than
        // by typing, so it is shown as a heading instead of as a row.
        if key.eq_ignore_ascii_case("classname") { continue }
        if rows.iter().any(|r| r.key.eq_ignore_ascii_case(key)) { continue }
        rows.push(PropertyRow {
            key: key.clone(),
            label: key.clone(),
            kind: KeyKind::String,
            help: String::new(),
            choices: Vec::new(),
            default: String::new(),
            value: Some(value.clone()),
            described: false,
        });
    }

    rows
}

/// Write edited rows back onto an entity.
///
/// A row left unset removes the key rather than writing the default into it,
/// so a map only carries the keys someone actually chose. That keeps a diff
/// between two saves readable, which matters more than it sounds: it is how
/// you find what a change actually did.
pub fn apply(entity: &mut Entity, rows: &[PropertyRow]) {
    for row in rows {
        match &row.value {
            Some(value) => { entity.set(&row.key, value.clone()); }
            None => { entity.remove(&row.key); }
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
        if entity.targetname() != Some(target) { continue }
        let Some(spec) = schema.get(entity.classname()) else { continue };
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
    let mut parts = text.split_whitespace().filter_map(|p| p.parse::<f32>().ok());
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

fn clamp_byte(v: f32) -> u8 { v.clamp(0.0, 255.0) as u8 }

/// Parse a vector keyvalue, tolerating the several ways one gets written.
pub fn parse_vec3(text: &str) -> [f32; 3] {
    let cleaned: String = text
        .chars()
        .map(|c| if c == ',' || c == '[' || c == ']' || c == '(' || c == ')' { ' ' } else { c })
        .collect();
    let mut parts = cleaned.split_whitespace().filter_map(|p| p.parse::<f32>().ok());
    [parts.next().unwrap_or(0.0), parts.next().unwrap_or(0.0), parts.next().unwrap_or(0.0)]
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
