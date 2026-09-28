// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
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

use kerosene_kv::KeyValues;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
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
    #[error(
        "`{0}` is not a helper type (expected model, lightradius, lightcone, sphere, frustum, direction, line or rect)"
    )]
    UnknownHelper(String),
    /// Any of the above, with the class it happened in. A schema is hundreds
    /// of classes; "`strng` is not a key type" is a search, "in class
    /// `func_door`: ..." is a fix.
    #[error("in class `{class}`: {source}")]
    InClass {
        class: String,
        #[source]
        source: Box<SchemaError>,
    },
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
    pub fn takes_brushes(self) -> bool {
        matches!(self, ClassKind::Brush | ClassKind::Any)
    }
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

/// What kind of thing an editor draws for an entity, besides its marker.
///
/// Hammer's FGD calls these helpers: `studio()`, `lightcone()`, `sphere()`
/// and friends. They change nothing in the game; they are how a level stays
/// readable when it is full of entities.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum HelperKind {
    /// The entity's model, where it stands. Parameter `key`: which key holds
    /// the model (default `model`).
    Model,
    /// How far a light reaches: two spheres, where it lights a surface fully
    /// and where it stops being worth tracing. Reads `_light` and the three
    /// `_*_attn` keys, as Radiance does.
    LightRadius,
    /// Where a spot light shines: a cone along its angles, from `_cone` and
    /// `_inner_cone`, honouring the `pitch` override.
    LightCone,
    /// A sphere. Parameters `radius` (a key name or a number) and `color`.
    Sphere,
    /// What a camera sees: a pyramid along its angles. Parameters `fov`
    /// (default 90) and `length` (default 256).
    Frustum,
    /// An arrow along the entity's angles. Parameter `length` (default 48).
    Direction,
    /// A line to every entity named by a key. Parameter `key` (default
    /// `target`).
    Line,
    /// A flat rectangle facing along the entity's angles, for a panel or a
    /// decal. Parameters `width` and `height`.
    Rect,
}

impl HelperKind {
    fn parse(s: &str) -> Result<HelperKind, SchemaError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "model" | "studio" => Ok(HelperKind::Model),
            "lightradius" | "light" => Ok(HelperKind::LightRadius),
            "lightcone" => Ok(HelperKind::LightCone),
            "sphere" => Ok(HelperKind::Sphere),
            "frustum" => Ok(HelperKind::Frustum),
            "direction" | "arrow" => Ok(HelperKind::Direction),
            "line" => Ok(HelperKind::Line),
            "rect" => Ok(HelperKind::Rect),
            other => Err(SchemaError::UnknownHelper(other.to_string())),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            HelperKind::Model => "model",
            HelperKind::LightRadius => "lightradius",
            HelperKind::LightCone => "lightcone",
            HelperKind::Sphere => "sphere",
            HelperKind::Frustum => "frustum",
            HelperKind::Direction => "direction",
            HelperKind::Line => "line",
            HelperKind::Rect => "rect",
        }
    }
}

/// One helper on a class, with its parameters as the schema wrote them.
///
/// A numeric parameter may name a key or be a number: `"radius" "radius"`
/// reads the entity's own `radius`, `"radius" "256"` is always 256.
#[derive(Clone, Debug, PartialEq)]
pub struct HelperSpec {
    pub kind: HelperKind,
    pub params: Vec<(String, String)>,
}

impl HelperSpec {
    pub fn new(kind: HelperKind) -> HelperSpec {
        HelperSpec {
            kind,
            params: Vec::new(),
        }
    }

    /// A parameter's raw value.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
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
    /// What an editor draws for it: its model, a light's cone, a sound's
    /// radius. Inherited from bases, like keys.
    pub helpers: Vec<HelperSpec>,
}

impl ClassSpec {
    pub fn key(&self, name: &str) -> Option<&KeySpec> {
        self.keys.iter().find(|k| k.name.eq_ignore_ascii_case(name))
    }

    pub fn has_input(&self, name: &str) -> bool {
        self.inputs
            .iter()
            .any(|i| i.name.eq_ignore_ascii_case(name))
    }

    pub fn has_output(&self, name: &str) -> bool {
        self.outputs
            .iter()
            .any(|o| o.name.eq_ignore_ascii_case(name))
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
    /// The bases this schema's classes were built from, kept so a schema
    /// parsed *after* this one can inherit from them.
    bases: BTreeMap<String, ClassSpec>,
}

impl Schema {
    pub fn parse(text: &str) -> Result<Schema, SchemaError> {
        Schema::parse_after(text, None)
    }

    /// Parse with another schema's bases available to inherit from.
    ///
    /// A game's own definitions, or a mod's file on disk, say
    /// `"base" "Point"` and mean the engine's `Point`; without this each
    /// file would have to carry a copy of every base it uses, and the
    /// copies would drift.
    pub fn parse_after(text: &str, earlier: Option<&Schema>) -> Result<Schema, SchemaError> {
        let kv = KeyValues::parse(text)?;

        // Bases first: a class may inherit from one defined later in the file,
        // and requiring otherwise would make the file order matter for the
        // wrong reason. The earlier schema's bases come first, so this
        // file's own definition of a name wins.
        let mut bases: BTreeMap<String, ClassSpec> =
            earlier.map(|e| e.bases.clone()).unwrap_or_default();
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
            let mut helpers = Vec::new();
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
                helpers.extend(base.helpers.iter().cloned());
            }
            // A class redefining an inherited key wins: the base supplies the
            // common case and the class narrows it.
            merge_keys(&mut keys, std::mem::take(&mut spec.keys));
            merge_io(&mut inputs, std::mem::take(&mut spec.inputs));
            merge_io(&mut outputs, std::mem::take(&mut spec.outputs));
            // A class's own helper of a kind replaces an inherited one.
            let own = std::mem::take(&mut spec.helpers);
            helpers.retain(|h: &HelperSpec| !own.iter().any(|o| o.kind == h.kind));
            helpers.extend(own);
            spec.keys = keys;
            spec.inputs = inputs;
            spec.outputs = outputs;
            spec.helpers = helpers;

            schema.push(spec);
        }
        schema.bases = bases;
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

    /// Fold another schema into this one, later definitions winning, and
    /// its bases with it.
    pub fn merge(&mut self, other: Schema) {
        for spec in other.classes {
            self.push(spec);
        }
        self.bases.extend(other.bases);
    }

    pub fn get(&self, classname: &str) -> Option<&ClassSpec> {
        self.index
            .get(&classname.to_ascii_lowercase())
            .map(|&at| &self.classes[at])
    }

    pub fn classes(&self) -> &[ClassSpec] {
        &self.classes
    }
    pub fn len(&self) -> usize {
        self.classes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }

    /// Class names of a given kind, for a "place entity" menu.
    pub fn names_of_kind(&self, kind: ClassKind) -> Vec<&str> {
        self.classes
            .iter()
            .filter(|c| c.kind == kind || c.kind == ClassKind::Any)
            .map(|c| c.name.as_str())
            .collect()
    }
}

impl Schema {
    /// Give each class the keys its components declare (see
    /// [`ClassDef::component`](crate::ClassDef::component)), after the keys
    /// the text already gives it.
    ///
    /// A key the text defines is left as the text has it: that is where a
    /// key that needs a list of choices or named flags says so. [`check`]
    /// holds such a key's default to the component's.
    pub fn with_component_keys(mut self, registry: &crate::ClassRegistry) -> Schema {
        for spec in &mut self.classes {
            for decl in registry.components(&spec.name) {
                for key in component_keys(decl) {
                    if spec.key(&key.name).is_none() {
                        spec.keys.push(key);
                    }
                }
            }
        }
        self
    }
}

/// The editor's keys for a component: each field with a keyvalue that is
/// not [`Hidden`](kerosene_reflect::Hidden), with its label, help and the
/// type's default.
pub fn component_keys(decl: &crate::ComponentDecl) -> Vec<KeySpec> {
    use kerosene_reflect::{FieldKind, Widget};
    decl.fields
        .iter()
        .filter(|f| !f.hidden)
        .filter_map(|f| {
            let key = f.key?;
            let kind = match (f.widget, f.kind) {
                (Some(Widget::TargetSource), _) => KeyKind::TargetSource,
                (Some(Widget::TargetDestination), _) => KeyKind::TargetDestination,
                (Some(Widget::Material), _) => KeyKind::Material,
                (Some(Widget::Model), _) => KeyKind::Model,
                (Some(Widget::Color), _) => KeyKind::Color,
                (None, FieldKind::Float) => KeyKind::Float,
                (None, FieldKind::Integer | FieldKind::Unsigned) => KeyKind::Integer,
                (None, FieldKind::Boolean) => KeyKind::Boolean,
                (None, FieldKind::Vector) => KeyKind::Vector,
                (None, FieldKind::Angles) => KeyKind::Angles,
                (None, FieldKind::String | FieldKind::Other) => KeyKind::String,
            };
            Some(KeySpec {
                name: key.to_string(),
                label: f.label.unwrap_or(key).to_string(),
                kind,
                default: decl.default_value(f.name)
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                help: f.help.unwrap_or("").to_string(),
                choices: Vec::new(),
            })
        })
        .collect()
}

/// Whether a schema's default text means the same as a component's default.
fn same_default(value: &crate::Value, text: &str) -> bool {
    use crate::Value;
    let written = Value::from_keyvalue(text);
    match value {
        Value::Float(v) => written.as_f32() == Some(*v),
        Value::Int(v) => written.as_i32() == Some(*v),
        Value::Bool(v) => written.as_bool() == Some(*v),
        Value::Text(v) => text == v,
        Value::Vector(v) => written.as_vec3() == Some(*v),
        Value::Angle(a) => {
            written.as_vec3() == Some(kerosene_math::Vec3::new(a.pitch, a.yaw, a.roll))
        }
    }
}

/// Every way a schema and a class registry disagree, as sentences.
///
/// A schema is what the editor offers; a registry is what the game does.
/// Nothing at runtime reads the schema, so the two drift the first time
/// someone adds an input and forgets the other side -- and the failure is
/// a designer wiring up something the editor offered that silently does
/// nothing. Both directions are checked: every class, input and output the
/// registry has must be offered, and everything offered must exist. An
/// empty answer means they agree; a game's own test asserts exactly that.
pub fn check(registry: &crate::ClassRegistry, schema: &Schema) -> Vec<String> {
    let mut problems = Vec::new();
    let common_inputs = registry.common_inputs();
    let common_outputs = registry.common_outputs();

    for name in registry.class_names() {
        let Some(spec) = schema.get(name) else {
            problems.push(format!(
                "class `{name}` is registered but not described, so the editor shows no properties for it"
            ));
            continue;
        };
        let def = registry.get(name).expect("just listed");
        for decl in &def.components {
            for field in decl.fields.iter() {
                let Some(key) = field.key.filter(|_| !field.hidden) else {
                    continue;
                };
                let Some(offered) = spec.key(key) else {
                    problems.push(format!(
                        "key `{name}.{key}` is read ({}) but not offered",
                        decl.name
                    ));
                    continue;
                };
                if let Some(default) = decl.default_value(field.name)
                    && !same_default(&default, &offered.default)
                {
                    problems.push(format!(
                        "key `{name}.{key}` defaults to {default} in {}, but the schema says {:?}",
                        decl.name, offered.default
                    ));
                }
            }
        }
        for (input, _) in &def.inputs {
            if !spec.has_input(input) {
                problems.push(format!("input `{name}.{input}` is handled but not offered"));
            }
        }
        // The universal inputs have to reach every class, which in the
        // schema means every class inherits the base that carries them.
        for input in &common_inputs {
            if !spec.has_input(input) {
                problems.push(format!(
                    "common input `{name}.{input}` is not offered; inherit the base that carries it"
                ));
            }
        }
        for output in def.outputs.iter().chain(common_outputs.iter()) {
            if !spec.has_output(output) {
                problems.push(format!("output `{name}.{output}` is fired but not offered"));
            }
        }
    }

    for spec in schema.classes() {
        let Some(def) = registry.get(&spec.name) else {
            problems.push(format!(
                "class `{}` is described but not registered, so placing one gives an entity that does nothing",
                spec.name
            ));
            continue;
        };
        for input in &spec.inputs {
            if registry.find_input(&spec.name, &input.name).is_none() {
                problems.push(format!(
                    "input `{}.{}` is offered but nothing handles it",
                    spec.name, input.name
                ));
            }
        }
        for output in &spec.outputs {
            let known = def
                .outputs
                .iter()
                .chain(common_outputs.iter())
                .any(|o| o.eq_ignore_ascii_case(&output.name));
            if !known {
                problems.push(format!(
                    "output `{}.{}` is offered but never fired",
                    spec.name, output.name
                ));
            }
        }
    }
    problems
}

fn merge_keys(into: &mut Vec<KeySpec>, from: Vec<KeySpec>) {
    for key in from {
        match into
            .iter_mut()
            .find(|k| k.name.eq_ignore_ascii_case(&key.name))
        {
            Some(existing) => *existing = key,
            None => into.push(key),
        }
    }
}

fn merge_io(into: &mut Vec<IoSpec>, from: Vec<IoSpec>) {
    for io in from {
        match into
            .iter_mut()
            .find(|i| i.name.eq_ignore_ascii_case(&io.name))
        {
            Some(existing) => *existing = io,
            None => into.push(io),
        }
    }
}

fn parse_class_body(
    block: &KeyValues,
    kind_of_block: &'static str,
) -> Result<ClassSpec, SchemaError> {
    let name = block
        .get("name")
        .filter(|n| !n.trim().is_empty())
        .ok_or(SchemaError::Unnamed {
            block: kind_of_block,
        })?
        .to_string();

    let kind = match block.get("kind") {
        Some(k) => ClassKind::parse(k).map_err(|e| SchemaError::InClass {
            class: name.clone(),
            source: Box::new(e),
        })?,
        None => ClassKind::Point,
    };

    let mut spec = ClassSpec {
        name,
        kind,
        help: block.get("help").unwrap_or_default().to_string(),
        ..Default::default()
    };
    let in_class = |e: SchemaError| SchemaError::InClass {
        class: spec.name.clone(),
        source: Box::new(e),
    };

    for key_block in block.blocks("key") {
        let key_name = key_block
            .get("name")
            .filter(|n| !n.trim().is_empty())
            .ok_or_else(|| in_class(SchemaError::Unnamed { block: "key" }))?
            .to_string();
        let kind = match key_block.get("type") {
            Some(t) => KeyKind::parse(t).map_err(in_class)?,
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
            label: key_block
                .get("label")
                .unwrap_or(key_name.as_str())
                .to_string(),
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
                .ok_or(SchemaError::Unnamed {
                    block: "input or output",
                })?
                .to_string();
            out.push(IoSpec {
                name: io_name,
                help: io_block.get("help").unwrap_or_default().to_string(),
                parameter: io_block.get("parameter").map(str::to_string),
            });
        }
    }

    for helper_block in block.blocks("helper") {
        let kind =
            HelperKind::parse(helper_block.get("type").unwrap_or_default()).map_err(|e| {
                SchemaError::InClass {
                    class: spec.name.clone(),
                    source: Box::new(e),
                }
            })?;
        let params = helper_block
            .pairs()
            .filter(|(k, _)| !k.eq_ignore_ascii_case("type"))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        spec.helpers.push(HelperSpec { kind, params });
    }

    Ok(spec)
}

#[cfg(test)]
mod tests;
