// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Typed entity fields.
//!
//! Entities carry a bag of named values rather than typed structs, because the
//! set of meaningful fields belongs to the *game*, not the engine. A mod adds
//! a field by writing it in the editor; nothing in the engine needs to change.
//!
//! This is Source's datadesc idea with the boilerplate removed.

pub use kerosene_reflect::Value;
use kerosene_math::Vec3;
use std::collections::HashMap;

/// A named bag of entity fields.
#[derive(Clone, Debug, Default)]
pub struct Fields {
    map: HashMap<String, Value>,
}

impl Fields {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: &str, value: Value) -> &mut Self {
        self.map.insert(key.to_lowercase(), value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.map.get(&key.to_lowercase())
    }
    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(&key.to_lowercase())
    }
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.map.remove(&key.to_lowercase())
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.map.iter()
    }

    pub fn f32(&self, key: &str, default: f32) -> f32 {
        self.get(key).and_then(Value::as_f32).unwrap_or(default)
    }
    pub fn i32(&self, key: &str, default: i32) -> i32 {
        self.get(key).and_then(Value::as_i32).unwrap_or(default)
    }
    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(default)
    }
    /// A field as text, however it was typed.
    ///
    /// A value that parsed as a number or a vector -- a `target` of `"1"`, a
    /// `message` of `"3 2 1"` -- is written back out the way level data
    /// spells it, rather than reported missing because it happened to look
    /// like something else. Only a key that was never set is `None`.
    pub fn text(&self, key: &str) -> Option<std::borrow::Cow<'_, str>> {
        self.get(key).map(|v| match v {
            Value::Text(t) => std::borrow::Cow::Borrowed(t.as_str()),
            other => std::borrow::Cow::Owned(other.to_string()),
        })
    }
    pub fn vec3(&self, key: &str, default: Vec3) -> Vec3 {
        self.get(key).and_then(Value::as_vec3).unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_names_are_case_insensitive() {
        // Map files are inconsistent about this, and always have been.
        let mut f = Fields::new();
        f.set("TargetName", Value::Text("door1".into()));
        assert_eq!(f.text("targetname").as_deref(), Some("door1"));
        // A name that happens to be numeric is still a name.
        f.set("target", Value::from_keyvalue("1"));
        f.set("message", Value::from_keyvalue("3 2 1"));
        assert_eq!(f.text("target").as_deref(), Some("1"));
        assert_eq!(f.text("message").as_deref(), Some("3 2 1"));
        assert_eq!(f.text("nothing"), None);
        assert!(f.contains("TARGETNAME"));
    }

    #[test]
    fn defaults_apply_to_missing_and_unconvertible_fields() {
        let mut f = Fields::new();
        f.set("speed", Value::Text("not a number".into()));
        assert_eq!(f.f32("speed", 42.0), 42.0);
        assert_eq!(f.f32("absent", 7.0), 7.0);
    }
}
