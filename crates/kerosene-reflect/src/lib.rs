// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Reflection: every entity field declared once, and read by everything that
//! needs it.
//!
//! A door's `speed` used to be written down four times -- read with a
//! default in the class, listed with another default in the editor's
//! schema, copied into saves as an untyped field, and read by name again in
//! the engine. Now it is declared once, as a field of a component, with
//! what each of those consumers needs to know:
//!
//! ```
//! use kerosene_reflect::{Help, Key, Label, Reflect};
//!
//! #[derive(Reflect, Clone, Debug)]
//! struct Mover {
//!     #[reflect(@Key("speed"), @Label("Speed"), @Help("Units per second."))]
//!     speed: f32,
//!     /// Where it has got to. Saved, never set from a map.
//!     progress: f32,
//! }
//!
//! impl Default for Mover {
//!     fn default() -> Self {
//!         Mover { speed: 100.0, progress: 0.0 }
//!     }
//! }
//!
//! let mut door = Mover::default();
//! kerosene_reflect::apply_keyvalues(&mut door, |key| (key == "speed").then(|| "250".into()));
//! assert_eq!(door.speed, 250.0);
//! ```
//!
//! | Consumer | Reads |
//! |---|---|
//! | Map loader | [`Key`]: which keyvalue fills the field; the type parses it |
//! | Editor schema | [`Key`], [`Label`], [`Help`], [`Widget`], and the default from `Default` |
//! | Saves | every field not marked [`Transient`] |
//! | Network | fields marked [`Networked`] |
//! | Scripts, I/O, the console | fields by keyvalue name, as a [`Value`] |
//!
//! The mechanism is `bevy_reflect`, pinned and re-exported from here so a
//! Bevy upgrade is a change to this crate; the attributes are Kerosene's.
//! The field types a keyvalue can fill are the ones a map can spell:
//! `f32`, `i32`, `u32`, `bool`, `String`, `Vec3` and `Angles`, and
//! `Option<f32>` for a number a map may leave blank -- a counter with no
//! maximum. Any other reflectable type can be a field too -- it is saved,
//! and never read from a map.

mod value;

pub use bevy_reflect;
pub use bevy_reflect::structs::Struct;
pub use bevy_reflect::{
    GetTypeRegistration, PartialReflect, Reflect, TypeInfo, TypeRegistry, Typed,
};
pub use value::Value;

use kerosene_math::{Angles, Vec3};

// ---- the attributes ---------------------------------------------------------

/// The map keyvalue a field is filled from, and edited as.
///
/// A field with no key is state the game keeps -- how far a door has
/// opened -- and never comes from a map.
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key(pub &'static str);

/// What the editor calls the key. The key itself when there is none.
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label(pub &'static str);

/// A sentence for the editor's help text.
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Help(pub &'static str);

/// Replicated to clients.
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Networked;

/// Not written to a saved game: meaningless after a load, or rebuilt from
/// the rest.
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transient;

/// A keyvalue the map may set but the editor does not offer, because the
/// compiler or another tool writes it.
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hidden;

/// How the editor should offer a key, when its type does not say enough: a
/// `String` may be a plain string, another entity's name, or a sound.
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Widget {
    /// This entity's own name, which others address it by.
    TargetSource,
    /// Another entity's name.
    TargetDestination,
    /// A material path.
    Material,
    /// A model path.
    Model,
    /// Red, green and blue, 0-255.
    Color,
}

// ---- what a type declares ---------------------------------------------------

/// One field of a reflected struct, and what it declares.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    /// The Rust field name.
    pub name: &'static str,
    /// The keyvalue that fills it, if any.
    pub key: Option<&'static str>,
    pub label: Option<&'static str>,
    pub help: Option<&'static str>,
    pub widget: Option<Widget>,
    pub kind: FieldKind,
    pub networked: bool,
    pub saved: bool,
    pub hidden: bool,
}

/// What a field holds, as far as a map or an editor cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Float,
    Integer,
    /// A `u32`, which is how bit fields are held.
    Unsigned,
    Boolean,
    String,
    Vector,
    Angles,
    /// Any other reflectable type. Saved; never read from a map.
    Other,
}

impl FieldKind {
    fn of(type_id: std::any::TypeId) -> FieldKind {
        use std::any::TypeId;
        [
            (TypeId::of::<f32>(), FieldKind::Float),
            (TypeId::of::<Option<f32>>(), FieldKind::Float),
            (TypeId::of::<i32>(), FieldKind::Integer),
            (TypeId::of::<u32>(), FieldKind::Unsigned),
            (TypeId::of::<bool>(), FieldKind::Boolean),
            (TypeId::of::<String>(), FieldKind::String),
            (TypeId::of::<Vec3>(), FieldKind::Vector),
            (TypeId::of::<Angles>(), FieldKind::Angles),
        ]
        .into_iter()
        .find(|(t, _)| *t == type_id)
        .map_or(FieldKind::Other, |(_, k)| k)
    }
}

/// Every field a struct type declares, in declaration order. Empty for a
/// type that is not a struct with named fields.
pub fn fields_of(info: &'static TypeInfo) -> Vec<Field> {
    let Ok(info) = info.as_struct() else {
        return Vec::new();
    };
    info.iter()
        .map(|f| Field {
            name: f.name(),
            key: f.get_attribute::<Key>().map(|k| k.0),
            label: f.get_attribute::<Label>().map(|l| l.0),
            help: f.get_attribute::<Help>().map(|h| h.0),
            widget: f.get_attribute::<Widget>().copied(),
            kind: FieldKind::of(f.type_id()),
            networked: f.has_attribute::<Networked>(),
            saved: !f.has_attribute::<Transient>(),
            hidden: f.has_attribute::<Hidden>(),
        })
        .collect()
}

/// The fields of `T`.
pub fn fields<T: Typed>() -> Vec<Field> {
    fields_of(T::type_info())
}

/// The field a keyvalue fills, matched without regard to case, as map
/// files have never agreed on it.
pub fn field_for_key(info: &'static TypeInfo, key: &str) -> Option<Field> {
    fields_of(info)
        .into_iter()
        .find(|f| f.key.is_some_and(|k| k.eq_ignore_ascii_case(key)))
}

// ---- reading and writing fields ---------------------------------------------

/// Why a field could not be set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReflectError {
    /// The struct has no field of that name.
    NoField(String),
    /// The value does not convert to the field's type.
    Mismatch { field: String, value: String },
    /// The field's type is not one a value can be written into.
    Unsupported(String),
}

impl std::fmt::Display for ReflectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReflectError::NoField(name) => write!(f, "no field `{name}`"),
            ReflectError::Mismatch { field, value } => {
                write!(f, "`{value}` does not fit field `{field}`")
            }
            ReflectError::Unsupported(name) => {
                write!(
                    f,
                    "field `{name}` is not a type a value can be written into"
                )
            }
        }
    }
}

impl std::error::Error for ReflectError {}

/// A field's value, if the field is one of the types a [`Value`] holds.
/// An `Option` that is `None` has no value, as a key a map left out has
/// none.
pub fn value_of(field: &dyn PartialReflect) -> Option<Value> {
    if let Some(v) = field.try_downcast_ref::<f32>() {
        Some(Value::Float(*v))
    } else if let Some(v) = field.try_downcast_ref::<Option<f32>>() {
        v.map(Value::Float)
    } else if let Some(v) = field.try_downcast_ref::<i32>() {
        Some(Value::Int(*v))
    } else if let Some(v) = field.try_downcast_ref::<u32>() {
        Some(Value::Int(*v as i32))
    } else if let Some(v) = field.try_downcast_ref::<bool>() {
        Some(Value::Bool(*v))
    } else if let Some(v) = field.try_downcast_ref::<String>() {
        Some(Value::Text(v.clone()))
    } else if let Some(v) = field.try_downcast_ref::<Vec3>() {
        Some(Value::Vector(*v))
    } else {
        field.try_downcast_ref::<Angles>().map(|a| Value::Angle(*a))
    }
}

/// Write `value` into a field, converting as leniently as entity I/O does.
///
/// Returns `Ok(false)` when the value does not convert, leaving the field
/// as it was, and an error when the field is not a type values can go into.
fn write_value(field: &mut dyn PartialReflect, value: &Value) -> Result<bool, ()> {
    if let Some(f) = field.try_downcast_mut::<f32>() {
        Ok(value.as_f32().map(|v| *f = v).is_some())
    } else if let Some(f) = field.try_downcast_mut::<Option<f32>>() {
        // Blank is how a map leaves an optional number unset.
        if matches!(value, Value::Text(t) if t.trim().is_empty()) {
            *f = None;
            return Ok(true);
        }
        Ok(value.as_f32().map(|v| *f = Some(v)).is_some())
    } else if let Some(f) = field.try_downcast_mut::<i32>() {
        Ok(value.as_i32().map(|v| *f = v).is_some())
    } else if let Some(f) = field.try_downcast_mut::<u32>() {
        Ok(value
            .as_i32()
            .filter(|v| *v >= 0)
            .map(|v| *f = v as u32)
            .is_some())
    } else if let Some(f) = field.try_downcast_mut::<bool>() {
        Ok(value.as_bool().map(|v| *f = v).is_some())
    } else if let Some(f) = field.try_downcast_mut::<String>() {
        *f = match value {
            Value::Text(t) => t.clone(),
            other => other.to_string(),
        };
        Ok(true)
    } else if let Some(f) = field.try_downcast_mut::<Vec3>() {
        Ok(value.as_vec3().map(|v| *f = v).is_some())
    } else if let Some(f) = field.try_downcast_mut::<Angles>() {
        Ok(value
            .as_vec3()
            .map(|v| *f = Angles::new(v.x, v.y, v.z))
            .is_some())
    } else {
        Err(())
    }
}

/// A field's value by its Rust name.
pub fn get(target: &dyn Struct, field: &str) -> Option<Value> {
    target.field(field).and_then(value_of)
}

/// Set a field by its Rust name.
pub fn set(target: &mut dyn Struct, field: &str, value: &Value) -> Result<(), ReflectError> {
    let slot = target
        .field_mut(field)
        .ok_or_else(|| ReflectError::NoField(field.to_string()))?;
    match write_value(slot, value) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ReflectError::Mismatch {
            field: field.to_string(),
            value: value.to_string(),
        }),
        Err(()) => Err(ReflectError::Unsupported(field.to_string())),
    }
}

/// Set a field from keyvalue text. A `String` field takes the text as it
/// is -- a `targetname` of `007` stays `007` -- and anything else parses it.
pub fn set_from_text(target: &mut dyn Struct, field: &str, text: &str) -> Result<(), ReflectError> {
    let is_string = target
        .field(field)
        .is_some_and(|f| f.try_downcast_ref::<String>().is_some());
    let value = if is_string {
        Value::Text(text.to_string())
    } else {
        Value::from_keyvalue(text)
    };
    set(target, field, &value)
}

/// The value of the field a keyvalue fills.
pub fn get_key(target: &dyn Struct, key: &str) -> Option<Value> {
    let info = target.get_represented_type_info()?;
    let field = field_for_key(info, key)?;
    get(target, field.name)
}

/// Set the field a keyvalue fills. `Ok(false)` when no field takes that key.
pub fn set_key(target: &mut dyn Struct, key: &str, value: &Value) -> Result<bool, ReflectError> {
    let Some(info) = target.get_represented_type_info() else {
        return Ok(false);
    };
    let Some(field) = field_for_key(info, key) else {
        return Ok(false);
    };
    set(target, field.name, value).map(|()| true)
}

/// Fill every keyed field that `lookup` has a value for. Fields it has none
/// for keep what they had -- their defaults, for a fresh component.
///
/// Returns what would not parse, for the loader to report; a bad keyvalue
/// costs its own field, never the entity.
pub fn apply_keyvalues(
    target: &mut dyn Struct,
    lookup: impl Fn(&str) -> Option<String>,
) -> Vec<ReflectError> {
    let Some(info) = target.get_represented_type_info() else {
        return Vec::new();
    };
    let mut errors = Vec::new();
    for field in fields_of(info) {
        let Some(key) = field.key else { continue };
        let Some(text) = lookup(key) else { continue };
        if let Err(e) = set_from_text(target, field.name, &text) {
            errors.push(e);
        }
    }
    errors
}

/// Every field a saved game keeps, by Rust name, with its value. Fields of
/// a type a [`Value`] cannot hold are left out.
pub fn saved(target: &dyn Struct) -> Vec<(&'static str, Value)> {
    let Some(info) = target.get_represented_type_info() else {
        return Vec::new();
    };
    fields_of(info)
        .into_iter()
        .filter(|f| f.saved)
        .filter_map(|f| get(target, f.name).map(|v| (f.name, v)))
        .collect()
}

/// Every field replicated to clients, by Rust name, with its value.
pub fn networked(target: &dyn Struct) -> Vec<(&'static str, Value)> {
    let Some(info) = target.get_represented_type_info() else {
        return Vec::new();
    };
    fields_of(info)
        .into_iter()
        .filter(|f| f.networked)
        .filter_map(|f| get(target, f.name).map(|v| (f.name, v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Reflect, Clone, Debug, PartialEq)]
    struct Door {
        #[reflect(@Key("speed"), @Label("Speed"), @Help("Units per second."))]
        speed: f32,
        #[reflect(@Key("targetname"), @Widget::TargetSource)]
        name: String,
        #[reflect(@Key("movedir"))]
        movedir: Vec3,
        #[reflect(@Key("angles"), @Hidden)]
        angles: Angles,
        #[reflect(@Key("spawnflags"))]
        flags: u32,
        #[reflect(@Networked)]
        progress: f32,
        #[reflect(@Transient)]
        sound_playing: bool,
    }

    impl Default for Door {
        fn default() -> Self {
            Door {
                speed: 100.0,
                name: String::new(),
                movedir: Vec3::Z,
                angles: Angles::ZERO,
                flags: 0,
                progress: 0.0,
                sound_playing: false,
            }
        }
    }

    fn map(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn the_declaration_says_everything_each_consumer_needs() {
        let fields = fields::<Door>();
        let speed = &fields[0];
        assert_eq!(speed.name, "speed");
        assert_eq!(speed.key, Some("speed"));
        assert_eq!(speed.label, Some("Speed"));
        assert_eq!(speed.help, Some("Units per second."));
        assert_eq!(speed.kind, FieldKind::Float);
        assert!(speed.saved && !speed.networked && !speed.hidden);
        assert_eq!(fields[1].widget, Some(Widget::TargetSource));
        assert!(fields[3].hidden);
        assert_eq!(fields[4].kind, FieldKind::Unsigned);
        assert!(fields[5].networked && fields[5].key.is_none());
        assert!(!fields[6].saved);
    }

    #[test]
    fn a_map_fills_keyed_fields_and_leaves_the_rest_at_their_defaults() {
        let mut door = Door::default();
        let errors = apply_keyvalues(
            &mut door,
            map(&[
                ("Speed", "250"),
                ("targetname", "007"),
                ("movedir", "1 0 0"),
                ("angles", "0 90 0"),
                ("spawnflags", "5"),
                ("progress", "0.5"),
            ]),
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(door.speed, 250.0);
        assert_eq!(door.name, "007", "a string key is taken as written");
        assert_eq!(door.movedir, Vec3::X);
        assert_eq!(door.angles.yaw, 90.0);
        assert_eq!(door.flags, 5);
        assert_eq!(door.progress, 0.0, "no key, so never from the map");
    }

    #[test]
    fn a_bad_keyvalue_costs_its_field_and_says_so() {
        let mut door = Door::default();
        let errors = apply_keyvalues(&mut door, map(&[("speed", "fast"), ("spawnflags", "-1")]));
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert_eq!(door.speed, 100.0);
        assert_eq!(door.flags, 0);
    }

    #[test]
    fn fields_read_and_write_by_key_as_values() {
        let mut door = Door::default();
        assert_eq!(get_key(&door, "SPEED"), Some(Value::Float(100.0)));
        assert_eq!(
            set_key(&mut door, "speed", &Value::Text("40".into())),
            Ok(true)
        );
        assert_eq!(door.speed, 40.0);
        assert_eq!(set_key(&mut door, "nothing", &Value::Int(1)), Ok(false));
        assert!(set_key(&mut door, "speed", &Value::Text("x".into())).is_err());
        assert_eq!(get(&door, "progress"), Some(Value::Float(0.0)));
    }

    #[derive(Reflect, Default)]
    struct Limits {
        #[reflect(@Key("max"))]
        max: Option<f32>,
    }

    #[test]
    fn an_optional_number_is_absent_until_a_map_sets_it() {
        let mut l = Limits::default();
        assert_eq!(fields::<Limits>()[0].kind, FieldKind::Float);
        assert_eq!(get_key(&l, "max"), None);
        assert!(saved(&l).is_empty(), "nothing to save");
        apply_keyvalues(&mut l, |_| Some("5".into()));
        assert_eq!(l.max, Some(5.0));
        assert_eq!(get_key(&l, "max"), Some(Value::Float(5.0)));
        apply_keyvalues(&mut l, |_| Some(" ".into()));
        assert_eq!(l.max, None, "blank unsets it");
    }

    #[test]
    fn saves_and_the_network_see_their_own_fields() {
        let door = Door::default();
        let saved: Vec<&str> = saved(&door).into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            saved,
            ["speed", "name", "movedir", "angles", "flags", "progress"]
        );
        let net: Vec<&str> = networked(&door).into_iter().map(|(n, _)| n).collect();
        assert_eq!(net, ["progress"]);
    }
}
