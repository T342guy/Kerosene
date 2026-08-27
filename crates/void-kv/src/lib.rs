// SPDX-License-Identifier: LGPL-3.0-or-later
//! KeyValues -- the text format VoidEngine uses for anything human-editable.
//!
//! This is Source's KeyValues, and it shows up in the same places: `.voidmap`
//! source maps, `.voidmat` materials, the entity lump inside a compiled `.voidbsp`,
//! game configuration, and the FGD-adjacent entity metadata Chisel reads.
//!
//! ```text
//! world
//! {
//!     "classname" "worldspawn"
//!     "skyname"   "sky_void"
//!     solid
//!     {
//!         "id" "1"
//!         // faces follow
//!     }
//! }
//! ```
//!
//! Two properties of the format drive the whole design here:
//!
//! * **Keys repeat.** A `world` block holds many `solid` blocks; an entity's
//!   `connections` block holds several outputs with the same name. So entries
//!   are an ordered [`Vec`], never a map -- a `HashMap` would silently eat
//!   brushes.
//! * **Order is meaningful.** Round-tripping a map through Chisel must not
//!   reshuffle it, or every save produces a noisy diff.

use std::fmt::Write as _;
use thiserror::Error;

mod parse;
mod value;

pub use parse::ParseError;
pub use value::{FromKvValue, ParseValueError, ToKvValue, Vec3Value, format_float};

/// One entry inside a block: either a `"key" "value"` pair or a nested block.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Pair(String, String),
    Block(KeyValues),
}

/// A named block of entries.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct KeyValues {
    pub name: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Error)]
pub enum KvError {
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("key {key:?} is missing from block {block:?}")]
    Missing { block: String, key: String },
    #[error("key {key:?} in block {block:?}: {source}")]
    BadValue {
        block: String,
        key: String,
        #[source]
        source: ParseValueError,
    },
}

impl KeyValues {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), entries: Vec::new() }
    }

    /// Parse a document. The result is a synthetic root block named `""`
    /// holding every top-level block, since KeyValues files routinely have
    /// several roots (`versioninfo`, `world`, `entity`, ...).
    pub fn parse(text: &str) -> Result<KeyValues, ParseError> {
        parse::parse_document(text)
    }

    // ---- reading ---------------------------------------------------------

    /// First value for `key`, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find_map(|e| match e {
            Entry::Pair(k, v) if k == key => Some(v.as_str()),
            _ => None,
        })
    }

    /// Every value for `key`, in file order.
    pub fn get_all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.entries.iter().filter_map(move |e| match e {
            Entry::Pair(k, v) if k == key => Some(v.as_str()),
            _ => None,
        })
    }

    /// First child block named `name`.
    pub fn block(&self, name: &str) -> Option<&KeyValues> {
        self.entries.iter().find_map(|e| match e {
            Entry::Block(b) if b.name == name => Some(b),
            _ => None,
        })
    }

    pub fn block_mut(&mut self, name: &str) -> Option<&mut KeyValues> {
        self.entries.iter_mut().find_map(|e| match e {
            Entry::Block(b) if b.name == name => Some(b),
            _ => None,
        })
    }

    /// Every child block named `name`, in file order.
    pub fn blocks<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a KeyValues> + 'a {
        self.entries.iter().filter_map(move |e| match e {
            Entry::Block(b) if b.name == name => Some(b),
            _ => None,
        })
    }

    /// Every child block, whatever it is called.
    pub fn all_blocks(&self) -> impl Iterator<Item = &KeyValues> {
        self.entries.iter().filter_map(|e| match e {
            Entry::Block(b) => Some(b),
            _ => None,
        })
    }

    /// Every `key`/`value` pair, whatever the key.
    pub fn pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().filter_map(|e| match e {
            Entry::Pair(k, v) => Some((k.as_str(), v.as_str())),
            _ => None,
        })
    }

    pub fn contains_key(&self, key: &str) -> bool { self.get(key).is_some() }

    /// Parse a value into any supported type, defaulting when absent.
    pub fn get_or<T: value::FromKvValue>(&self, key: &str, default: T) -> T {
        self.get(key).and_then(|v| T::from_kv(v).ok()).unwrap_or(default)
    }

    /// Parse a value, erroring when absent or malformed.
    pub fn require<T: value::FromKvValue>(&self, key: &str) -> Result<T, KvError> {
        let raw = self.get(key).ok_or_else(|| KvError::Missing {
            block: self.name.clone(),
            key: key.to_string(),
        })?;
        T::from_kv(raw).map_err(|source| KvError::BadValue {
            block: self.name.clone(),
            key: key.to_string(),
            source,
        })
    }

    /// Parse a value if present, propagating malformed values as errors.
    ///
    /// Distinct from [`Self::get_or`]: a key that is present but garbage is a
    /// mistake worth reporting, not something to paper over with a default.
    pub fn optional<T: value::FromKvValue>(&self, key: &str) -> Result<Option<T>, KvError> {
        match self.get(key) {
            None => Ok(None),
            Some(raw) => T::from_kv(raw)
                .map(Some)
                .map_err(|source| KvError::BadValue {
                    block: self.name.clone(),
                    key: key.to_string(),
                    source,
                }),
        }
    }

    // ---- writing ---------------------------------------------------------

    /// Append a pair, allowing duplicates.
    pub fn push(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.entries.push(Entry::Pair(key.into(), value.into()));
        self
    }

    /// Append a pair holding any displayable value.
    pub fn push_value(&mut self, key: impl Into<String>, value: impl value::ToKvValue) -> &mut Self {
        self.entries.push(Entry::Pair(key.into(), value.to_kv()));
        self
    }

    /// Replace the first pair with this key, or append if there is none.
    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        for e in &mut self.entries {
            if let Entry::Pair(k, v) = e {
                if k == key {
                    *v = value.into();
                    return self;
                }
            }
        }
        self.push(key.to_string(), value)
    }

    pub fn push_block(&mut self, block: KeyValues) -> &mut Self {
        self.entries.push(Entry::Block(block));
        self
    }

    /// Remove every pair with this key; returns how many went.
    pub fn remove(&mut self, key: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| !matches!(e, Entry::Pair(k, _) if k == key));
        before - self.entries.len()
    }

    /// Serialise this block, name and braces included.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out, 0);
        out
    }

    /// Serialise a root produced by [`Self::parse`] -- children only, no
    /// wrapping braces, so that parse/write round-trips.
    pub fn to_document(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            match e {
                Entry::Pair(k, v) => {
                    let _ = writeln!(out, "\"{}\" \"{}\"", escape(k), escape(v));
                }
                Entry::Block(b) => b.write_into(&mut out, 0),
            }
        }
        out
    }

    fn write_into(&self, out: &mut String, depth: usize) {
        let pad = "\t".repeat(depth);
        let _ = writeln!(out, "{pad}{}", self.name);
        let _ = writeln!(out, "{pad}{{");
        let inner = "\t".repeat(depth + 1);
        for e in &self.entries {
            match e {
                Entry::Pair(k, v) => {
                    let _ = writeln!(out, "{inner}\"{}\" \"{}\"", escape(k), escape(v));
                }
                Entry::Block(b) => b.write_into(out, depth + 1),
            }
        }
        let _ = writeln!(out, "{pad}}}");
    }
}

/// Escape the two characters that would otherwise end a quoted token.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
// a comment
versioninfo
{
    "editorversion" "100"
}
world
{
    "classname" "worldspawn"
    solid { "id" "1" }
    solid { "id" "2" }
}
"#;

    #[test]
    fn parses_multiple_roots() {
        let kv = KeyValues::parse(SAMPLE).unwrap();
        assert!(kv.block("versioninfo").is_some());
        assert!(kv.block("world").is_some());
    }

    #[test]
    fn duplicate_blocks_are_all_kept() {
        let kv = KeyValues::parse(SAMPLE).unwrap();
        let world = kv.block("world").unwrap();
        let ids: Vec<_> = world.blocks("solid").map(|s| s.get("id").unwrap()).collect();
        assert_eq!(ids, vec!["1", "2"], "a map format cannot afford to drop brushes");
    }

    #[test]
    fn round_trips_through_text() {
        let kv = KeyValues::parse(SAMPLE).unwrap();
        let text = kv.to_document();
        let again = KeyValues::parse(&text).unwrap();
        assert_eq!(kv, again);
    }

    #[test]
    fn set_replaces_and_push_appends() {
        let mut kv = KeyValues::new("entity");
        kv.push("classname", "light");
        kv.set("classname", "light_spot");
        assert_eq!(kv.get("classname"), Some("light_spot"));
        assert_eq!(kv.get_all("classname").count(), 1);
        kv.push("classname", "extra");
        assert_eq!(kv.get_all("classname").count(), 2);
    }

    #[test]
    fn missing_key_reports_its_block() {
        let kv = KeyValues::new("solid");
        let err = kv.require::<i32>("id").unwrap_err();
        assert!(matches!(err, KvError::Missing { .. }), "{err}");
        assert!(err.to_string().contains("solid"));
    }

    #[test]
    fn present_but_malformed_is_an_error_not_a_default() {
        let mut kv = KeyValues::new("entity");
        kv.push("origin", "not a vector");
        assert!(kv.optional::<Vec3Value>("origin").is_err());
        assert!(kv.optional::<Vec3Value>("absent").unwrap().is_none());
    }

    #[test]
    fn escapes_survive_a_round_trip() {
        let mut kv = KeyValues::new("m");
        kv.push("path", r#"materials\dev\a "quoted" name"#);
        let parsed = KeyValues::parse(&kv.to_text()).unwrap();
        assert_eq!(
            parsed.block("m").unwrap().get("path"),
            Some(r#"materials\dev\a "quoted" name"#)
        );
    }
}
