// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Entity class definitions -- what an editor needs to show for a class.
//!
//! The engine reads whatever keys a map happens to carry: an entity is a bag
//! of fields, and that is deliberate. But an editor cannot work that way. A
//! designer who places a `func_door` and is shown an empty property list has
//! no way to discover that `speed`, `lip` and `wait` exist, and typing them
//! from memory is not an editor feature.
//!
//! So the game ships a *schema* -- the same relationship Hammer has with an
//! FGD, and for the same reason. It is a plain text file, loaded by Chisel and
//! ignored by the engine, listing for each class its keys (with types,
//! defaults and help), its inputs, and the outputs it fires.
//!
//! ```text
//! base
//! {
//!     "name" "Targetname"
//!     key { "name" "targetname" "type" "target_source"
//!           "help" "The name other entities use to address this one." }
//! }
//!
//! class
//! {
//!     "name" "func_door"
//!     "kind" "brush"
//!     "base" "Targetname"
//!     "help" "A brush that slides open and shut."
//!     key    { "name" "speed" "type" "float" "default" "100" }
//!     input  { "name" "Open" }
//!     output { "name" "OnFullyOpen" }
//! }
//! ```
//!
//! Keeping this as data rather than code is what lets the tools stay separate
//! programs. Chisel never links the game; it reads the game's file.

use std::collections::BTreeMap;
use thiserror::Error;
use kerosene_kv::KeyValues;

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error(transparent)]
    Parse(#[from] kerosene_kv::ParseError),
    #[error("a {block} block has no \"name\"")]
    Unnamed { block: &'static str },
    #[error("class `{class}` inherits from `{base}`, which is not defined")]
    UnknownBase { class: String, base: String },
    #[error("`{0}` is not a key type")]
    UnknownKeyType(String),
    #[error("`{0}` is not a class kind (expected point, brush or any)")]
    UnknownKind(String),
}

/// Whether a class is placed as a point or built out of brushes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ClassKind {
    /// Placed at a position: lights, player starts, logic.
    #[default]
    Point,
    /// Made of brushes tied to the entity: doors, triggers.
    Brush,
    /// Either. `worldspawn` is the only real case.
    Any,
}

impl ClassKind {
    fn parse(s: &str) -> Result<ClassKind, SchemaError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "point" => Ok(ClassKind::Point),
            "brush" | "solid" => Ok(ClassKind::Brush),
            "any" | "both" => Ok(ClassKind::Any),
            other => Err(SchemaError::UnknownKind(other.to_string())),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ClassKind::Point => "point",
            ClassKind::Brush => "brush",
            ClassKind::Any => "any",
        }
    }

    /// Whether a class of this kind may be tied to brushes.
    pub fn takes_brushes(self) -> bool { matches!(self, ClassKind::Brush | ClassKind::Any) }
}

/// What kind of value a key holds, so an editor can pick a widget for it.
///
/// This is a closed set on purpose: an unrecognised type in a schema file is
/// an error at load rather than a text box at edit time, because the point of
/// the schema is to stop keys being guessed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum KeyKind {
    #[default]
    String,
    Integer,
    Float,
    /// `0` or `1`, shown as a checkbox.
    Boolean,
    /// Three numbers.
    Vector,
    /// Pitch/yaw/roll, in degrees.
    Angles,
    /// Three numbers 0-255, optionally a fourth for brightness.
    Color,
    /// This entity's own name -- the thing other entities address.
    TargetSource,
    /// Another entity's name, so the editor can offer the names in the map.
    TargetDestination,
    /// A material path, offered from the content tree.
    Material,
    /// A model path, offered from the content tree.
    Model,
    /// One of a fixed list, given by [`KeySpec::choices`].
    Choices,
    /// A bit field, with named bits in [`KeySpec::choices`].
    Flags,
}

impl KeyKind {
    fn parse(s: &str) -> Result<KeyKind, SchemaError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "string" => Ok(KeyKind::String),
            "int" | "integer" => Ok(KeyKind::Integer),
            "float" => Ok(KeyKind::Float),
            "bool" | "boolean" => Ok(KeyKind::Boolean),
            "vec3" | "vector" => Ok(KeyKind::Vector),
            "angles" | "angle" => Ok(KeyKind::Angles),
            "color" | "colour" => Ok(KeyKind::Color),
            "target_source" => Ok(KeyKind::TargetSource),
            "target_destination" | "target" => Ok(KeyKind::TargetDestination),
            "material" => Ok(KeyKind::Material),
            "model" => Ok(KeyKind::Model),
            "choices" => Ok(KeyKind::Choices),
            "flags" => Ok(KeyKind::Flags),
            other => Err(SchemaError::UnknownKeyType(other.to_string())),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            KeyKind::String => "string",
            KeyKind::Integer => "int",
            KeyKind::Float => "float",
            KeyKind::Boolean => "bool",
            KeyKind::Vector => "vec3",
            KeyKind::Angles => "angles",
            KeyKind::Color => "color",
            KeyKind::TargetSource => "target_source",
            KeyKind::TargetDestination => "target_destination",
            KeyKind::Material => "material",
            KeyKind::Model => "model",
            KeyKind::Choices => "choices",
            KeyKind::Flags => "flags",
        }
    }
}

/// One editable key on a class.
#[derive(Clone, Debug, Default)]
pub struct KeySpec {
    pub name: String,
    /// What the editor shows instead of the raw key name.
    pub label: String,
    pub kind: KeyKind,
    /// The value the game assumes when the key is absent. Shown as a
    /// placeholder rather than written into every new entity, so a map only
    /// carries the keys someone actually set.
    pub default: String,
    pub help: String,
    /// For [`KeyKind::Choices`] and [`KeyKind::Flags`]: `(value, label)`.
    pub choices: Vec<(String, String)>,
}

/// One input or output on a class.
#[derive(Clone, Debug, Default)]
pub struct IoSpec {
    pub name: String,
    pub help: String,
    /// What the parameter means, if the input takes one.
    pub parameter: Option<String>,
}

/// Everything an editor knows about one class.
#[derive(Clone, Debug, Default)]
pub struct ClassSpec {
    pub name: String,
    pub kind: ClassKind,
    pub help: String,
    pub keys: Vec<KeySpec>,
    pub inputs: Vec<IoSpec>,
    pub outputs: Vec<IoSpec>,
}

impl ClassSpec {
    pub fn key(&self, name: &str) -> Option<&KeySpec> {
        self.keys.iter().find(|k| k.name.eq_ignore_ascii_case(name))
    }

    pub fn has_input(&self, name: &str) -> bool {
        self.inputs.iter().any(|i| i.name.eq_ignore_ascii_case(name))
    }

    pub fn has_output(&self, name: &str) -> bool {
        self.outputs.iter().any(|o| o.name.eq_ignore_ascii_case(name))
    }
}

/// A parsed set of class definitions.
///
/// Classes are stored in file order, because that is the order a designer sees
/// them in and file order is something a person controls.
#[derive(Clone, Debug, Default)]
pub struct Schema {
    classes: Vec<ClassSpec>,
    index: BTreeMap<String, usize>,
}

impl Schema {
    pub fn parse(text: &str) -> Result<Schema, SchemaError> {
        let kv = KeyValues::parse(text)?;

        // Bases first: a class may inherit from one defined later in the file,
        // and requiring otherwise would make the file order matter for the
        // wrong reason.
        let mut bases: BTreeMap<String, ClassSpec> = BTreeMap::new();
        for block in kv.blocks("base") {
            let spec = parse_class_body(block, "base")?;
            bases.insert(spec.name.to_ascii_lowercase(), spec);
        }

        let mut schema = Schema::default();
        for block in kv.blocks("class") {
            let mut spec = parse_class_body(block, "class")?;

            // Inherited members go first, so the common keys every entity has
            // stay at the top of the inspector where a person expects them.
            let mut keys = Vec::new();
            let mut inputs = Vec::new();
            let mut outputs = Vec::new();
            for base_name in block.get_all("base") {
                let base = bases.get(&base_name.to_ascii_lowercase()).ok_or_else(|| {
                    SchemaError::UnknownBase {
                        class: spec.name.clone(),
                        base: base_name.to_string(),
                    }
                })?;
                keys.extend(base.keys.iter().cloned());
                inputs.extend(base.inputs.iter().cloned());
                outputs.extend(base.outputs.iter().cloned());
            }
            // A class redefining an inherited key wins: the base supplies the
            // common case and the class narrows it.
            merge_keys(&mut keys, std::mem::take(&mut spec.keys));
            merge_io(&mut inputs, std::mem::take(&mut spec.inputs));
            merge_io(&mut outputs, std::mem::take(&mut spec.outputs));
            spec.keys = keys;
            spec.inputs = inputs;
            spec.outputs = outputs;

            schema.push(spec);
        }
        Ok(schema)
    }

    pub fn push(&mut self, spec: ClassSpec) {
        let key = spec.name.to_ascii_lowercase();
        match self.index.get(&key) {
            // A later definition replaces an earlier one, so a project can
            // load its own file after the game's and override a class.
            Some(&at) => self.classes[at] = spec,
            None => {
                self.index.insert(key, self.classes.len());
                self.classes.push(spec);
            }
        }
    }

    /// Fold another schema into this one, later definitions winning.
    pub fn merge(&mut self, other: Schema) {
        for spec in other.classes {
            self.push(spec);
        }
    }

    pub fn get(&self, classname: &str) -> Option<&ClassSpec> {
        self.index.get(&classname.to_ascii_lowercase()).map(|&at| &self.classes[at])
    }

    pub fn classes(&self) -> &[ClassSpec] { &self.classes }
    pub fn len(&self) -> usize { self.classes.len() }
    pub fn is_empty(&self) -> bool { self.classes.is_empty() }

    /// Class names of a given kind, for a "place entity" menu.
    pub fn names_of_kind(&self, kind: ClassKind) -> Vec<&str> {
        self.classes
            .iter()
            .filter(|c| c.kind == kind || c.kind == ClassKind::Any)
            .map(|c| c.name.as_str())
            .collect()
    }
}

fn merge_keys(into: &mut Vec<KeySpec>, from: Vec<KeySpec>) {
    for key in from {
        match into.iter_mut().find(|k| k.name.eq_ignore_ascii_case(&key.name)) {
            Some(existing) => *existing = key,
            None => into.push(key),
        }
    }
}

fn merge_io(into: &mut Vec<IoSpec>, from: Vec<IoSpec>) {
    for io in from {
        match into.iter_mut().find(|i| i.name.eq_ignore_ascii_case(&io.name)) {
            Some(existing) => *existing = io,
            None => into.push(io),
        }
    }
}

fn parse_class_body(block: &KeyValues, kind_of_block: &'static str) -> Result<ClassSpec, SchemaError> {
    let name = block
        .get("name")
        .filter(|n| !n.trim().is_empty())
        .ok_or(SchemaError::Unnamed { block: kind_of_block })?
        .to_string();

    let kind = match block.get("kind") {
        Some(k) => ClassKind::parse(k)?,
        None => ClassKind::Point,
    };

    let mut spec = ClassSpec {
        name,
        kind,
        help: block.get("help").unwrap_or_default().to_string(),
        ..Default::default()
    };

    for key_block in block.blocks("key") {
        let key_name = key_block
            .get("name")
            .filter(|n| !n.trim().is_empty())
            .ok_or(SchemaError::Unnamed { block: "key" })?
            .to_string();
        let kind = match key_block.get("type") {
            Some(t) => KeyKind::parse(t)?,
            None => KeyKind::String,
        };
        let choices = key_block
            .blocks("choice")
            .filter_map(|c| {
                let value = c.get("value")?.to_string();
                let label = c.get("label").unwrap_or(value.as_str()).to_string();
                Some((value, label))
            })
            .collect();

        spec.keys.push(KeySpec {
            label: key_block.get("label").unwrap_or(key_name.as_str()).to_string(),
            name: key_name,
            kind,
            default: key_block.get("default").unwrap_or_default().to_string(),
            help: key_block.get("help").unwrap_or_default().to_string(),
            choices,
        });
    }

    for (field, out) in [("input", &mut spec.inputs), ("output", &mut spec.outputs)] {
        for io_block in block.blocks(field) {
            let io_name = io_block
                .get("name")
                .filter(|n| !n.trim().is_empty())
                .ok_or(SchemaError::Unnamed { block: "input or output" })?
                .to_string();
            out.push(IoSpec {
                name: io_name,
                help: io_block.get("help").unwrap_or_default().to_string(),
                parameter: io_block.get("parameter").map(str::to_string),
            });
        }
    }

    Ok(spec)
}

#[cfg(test)]
mod tests;
