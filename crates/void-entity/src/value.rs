// SPDX-License-Identifier: LGPL-3.0-or-later
//! Typed entity fields.
//!
//! Entities carry a bag of named values rather than typed structs, because the
//! set of meaningful fields belongs to the *game*, not the engine. A mod adds
//! a field by writing it in the editor; nothing in the engine needs to change.
//!
//! This is Source's datadesc idea with the boilerplate removed.

use std::collections::HashMap;
use void_math::{Angles, Vec3};

/// A value an entity field can hold.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i32),
    Float(f32),
    Text(String),
    Vector(Vec3),
    Angle(Angles),
}

impl std::fmt::Display for Value {
    /// How a value is written where text is what is wanted -- a script
    /// reading a keyvalue, a trace line, a debug dump. Numbers come out the
    /// way level data spells them, so a float that happens to be whole does
    /// not read as `250.000000`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Bool(v) => write!(f, "{}", if *v { 1 } else { 0 }),
            Value::Int(v) => write!(f, "{v}"),
            Value::Float(v) => write!(f, "{}", void_math::format_float(*v)),
            Value::Text(v) => write!(f, "{v}"),
            Value::Vector(v) => write!(
                f,
                "{} {} {}",
                void_math::format_float(v.x),
                void_math::format_float(v.y),
                void_math::format_float(v.z)
            ),
            Value::Angle(a) => write!(
                f,
                "{} {} {}",
                void_math::format_float(a.pitch),
                void_math::format_float(a.yaw),
                void_math::format_float(a.roll)
            ),
        }
    }
}

impl Value {
    /// Read as a float, converting where it makes sense.
    ///
    /// Conversions are lenient on purpose: entity I/O carries parameters as
    /// text, and a `SetSpeed` input receiving `"100"` should just work.
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f32),
            Value::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
            Value::Text(t) => t.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Float(v) => Some(*v as i32),
            Value::Bool(v) => Some(*v as i32),
            Value::Text(t) => t.trim().parse().ok().or_else(|| t.trim().parse::<f32>().ok().map(|f| f as i32)),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(v) => Some(*v),
            Value::Int(v) => Some(*v != 0),
            Value::Float(v) => Some(*v != 0.0),
            Value::Text(t) => match t.trim() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" | "" => Some(false),
                other => other.parse::<f32>().ok().map(|v| v != 0.0),
            },
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_vec3(&self) -> Option<Vec3> {
        match self {
            Value::Vector(v) => Some(*v),
            Value::Angle(a) => Some(Vec3::new(a.pitch, a.yaw, a.roll)),
            Value::Text(t) => parse_vec3(t),
            _ => None,
        }
    }

    /// Parse a keyvalue string, guessing the type from its shape.
    ///
    /// Everything in a map file is text, so the guess is what turns
    /// `"0 64 128"` into a vector and `"100"` into a number without every
    /// entity class having to declare its schema up front.
    pub fn from_keyvalue(raw: &str) -> Value {
        let trimmed = raw.trim();
        if let Some(v) = parse_vec3(trimmed) { return Value::Vector(v); }
        if let Ok(i) = trimmed.parse::<i32>() { return Value::Int(i); }
        if let Ok(f) = trimmed.parse::<f32>() { return Value::Float(f); }
        Value::Text(raw.to_string())
    }
}

fn parse_vec3(s: &str) -> Option<Vec3> {
    let cleaned: String = s
        .chars()
        .map(|c| if matches!(c, '[' | ']' | '(' | ')' | ',') { ' ' } else { c })
        .collect();
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if parts.len() != 3 { return None; }
    let mut out = [0.0f32; 3];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p.parse().ok()?;
    }
    Some(Vec3::from_array(out))
}

/// A named bag of entity fields.
#[derive(Clone, Debug, Default)]
pub struct Fields {
    map: HashMap<String, Value>,
}

impl Fields {
    pub fn new() -> Self { Self::default() }

    pub fn set(&mut self, key: &str, value: Value) -> &mut Self {
        self.map.insert(key.to_lowercase(), value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&Value> { self.map.get(&key.to_lowercase()) }
    pub fn contains(&self, key: &str) -> bool { self.map.contains_key(&key.to_lowercase()) }
    pub fn remove(&mut self, key: &str) -> Option<Value> { self.map.remove(&key.to_lowercase()) }
    pub fn len(&self) -> usize { self.map.len() }
    pub fn is_empty(&self) -> bool { self.map.is_empty() }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> { self.map.iter() }

    pub fn f32(&self, key: &str, default: f32) -> f32 {
        self.get(key).and_then(Value::as_f32).unwrap_or(default)
    }
    pub fn i32(&self, key: &str, default: i32) -> i32 {
        self.get(key).and_then(Value::as_i32).unwrap_or(default)
    }
    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(default)
    }
    pub fn text(&self, key: &str) -> Option<&str> { self.get(key).and_then(Value::as_str) }
    pub fn vec3(&self, key: &str, default: Vec3) -> Vec3 {
        self.get(key).and_then(Value::as_vec3).unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyvalue_types_are_guessed_from_shape() {
        assert_eq!(Value::from_keyvalue("0 64 128"), Value::Vector(Vec3::new(0.0, 64.0, 128.0)));
        assert_eq!(Value::from_keyvalue("100"), Value::Int(100));
        assert_eq!(Value::from_keyvalue("1.5"), Value::Float(1.5));
        assert_eq!(Value::from_keyvalue("door1"), Value::Text("door1".into()));
    }

    #[test]
    fn text_converts_to_numbers_for_io_parameters() {
        // Entity I/O carries everything as text; a SetSpeed input receiving
        // "100" has to work.
        assert_eq!(Value::Text("100".into()).as_f32(), Some(100.0));
        assert_eq!(Value::Text("100".into()).as_i32(), Some(100));
        assert_eq!(Value::Text("yes".into()).as_bool(), Some(true));
        assert_eq!(Value::Text("banana".into()).as_f32(), None);
    }

    #[test]
    fn field_names_are_case_insensitive() {
        // Map files are inconsistent about this, and always have been.
        let mut f = Fields::new();
        f.set("TargetName", Value::Text("door1".into()));
        assert_eq!(f.text("targetname"), Some("door1"));
        assert!(f.contains("TARGETNAME"));
    }

    #[test]
    fn defaults_apply_to_missing_and_unconvertible_fields() {
        let mut f = Fields::new();
        f.set("speed", Value::Text("not a number".into()));
        assert_eq!(f.f32("speed", 42.0), 42.0);
        assert_eq!(f.f32("absent", 7.0), 7.0);
    }

    #[test]
    fn vectors_read_back_from_several_spellings() {
        for src in ["0 64 128", "[0 64 128]", "(0 64 128)"] {
            assert_eq!(Value::Text(src.into()).as_vec3(), Some(Vec3::new(0.0, 64.0, 128.0)));
        }
    }

    #[test]
    fn a_float_spelled_as_a_vector_is_not_mistaken_for_one() {
        assert_eq!(Value::from_keyvalue("1.5"), Value::Float(1.5));
        assert!(matches!(Value::from_keyvalue("1 2"), Value::Text(_)));
    }
}
