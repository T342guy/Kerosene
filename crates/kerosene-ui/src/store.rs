// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The store: what the UI is allowed to know about the game.
//!
//! A HUD that reached into the engine for the player's health would have to
//! know where the engine keeps it, and would break the day it moved. Instead
//! everything a layout can show is *published* here under a dotted name --
//! `player.health`, `weapon.active`, `ability.dash.cooldown` -- by whoever owns
//! it: the engine, the game, a map script. The layout names what it wants and
//! never learns where it came from. That is the same contract Panorama keeps
//! with its game-state events, and the reason a modder can rewrite a HUD
//! without touching Rust.
//!
//! Two kinds of thing pass through:
//!
//! * **Values**, which persist and are compared on write, so publishing the
//!   same health sixty times a second costs nothing downstream: only a real
//!   change bumps the generation a binding watches.
//! * **Events**, which are fire-and-forget (`weapon_changed`,
//!   `player_damaged`) and are handed to every document once, the frame after
//!   they were emitted.

use std::collections::BTreeMap;
use std::fmt;

/// One value in the store.
#[derive(Clone, PartialEq, Debug)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl Value {
    /// Read text the way a console argument reads: numbers become numbers,
    /// `true`/`false` become booleans, everything else stays text.
    pub fn parse(text: &str) -> Value {
        let t = text.trim();
        match t {
            "true" => return Value::Bool(true),
            "false" => return Value::Bool(false),
            _ => {}
        }
        if let Ok(i) = t.parse::<i64>() {
            return Value::Int(i);
        }
        if let Ok(f) = t.parse::<f64>() {
            return Value::Float(f);
        }
        Value::Str(text.to_string())
    }

    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Int(i) => *i as f64,
            Value::Float(f) => *f,
            Value::Str(s) => s.trim().parse().unwrap_or(0.0),
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty() && s != "0" && s != "false",
        }
    }

    pub(crate) fn to_dynamic(&self) -> rhai::Dynamic {
        match self {
            Value::Bool(b) => (*b).into(),
            Value::Int(i) => (*i).into(),
            Value::Float(f) => (*f).into(),
            Value::Str(s) => s.clone().into(),
        }
    }

    pub(crate) fn from_dynamic(value: &rhai::Dynamic) -> Value {
        if let Some(b) = value.clone().try_cast::<bool>() {
            Value::Bool(b)
        } else if let Some(i) = value.clone().try_cast::<i64>() {
            Value::Int(i)
        } else if let Some(f) = value.clone().try_cast::<f64>() {
            Value::Float(f)
        } else {
            Value::Str(value.to_string())
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            // Whole floats print without the `.0`: `{player.health}` should
            // read "100", not "100.0", whichever type the game published.
            Value::Float(v) if v.fract() == 0.0 && v.abs() < 1e15 => write!(f, "{}", *v as i64),
            Value::Float(v) => write!(f, "{v}"),
            Value::Str(s) => f.write_str(s),
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v.into())
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::Int(v.into())
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v.into())
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}

/// A named, one-shot event.
#[derive(Clone, PartialEq, Debug)]
pub struct Event {
    pub name: String,
    pub data: String,
}

/// How many events may wait before the oldest are dropped.
///
/// Nothing drains the queue in a headless run, and a game emitting an event
/// a tick for an hour should not grow without bound because of it.
pub const MAX_PENDING_EVENTS: usize = 1024;

#[derive(Clone, Debug)]
struct Entry {
    value: Value,
    generation: u64,
}

/// Published game state, and the events that happened since it was last read.
#[derive(Default, Debug)]
pub struct UiStore {
    values: BTreeMap<String, Entry>,
    generation: u64,
    removed: BTreeMap<String, u64>,
    events: Vec<Event>,
}

impl UiStore {
    pub fn new() -> UiStore {
        UiStore::default()
    }

    /// Publish a value. Writing what is already there changes nothing.
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        let value = value.into();
        self.removed.remove(key);
        if let Some(entry) = self.values.get_mut(key) {
            if entry.value == value {
                return;
            }
            self.generation += 1;
            entry.value = value;
            entry.generation = self.generation;
            return;
        }
        self.generation += 1;
        self.values.insert(
            key.to_string(),
            Entry {
                value,
                generation: self.generation,
            },
        );
    }

    /// Forget a value; a binding that read it then sees nothing.
    pub fn remove(&mut self, key: &str) {
        if self.values.remove(key).is_some() {
            self.generation += 1;
            // A removal has no entry left to carry its generation, so it
            // leaves a tombstone for `changed_since` to find.
            self.removed.insert(key.to_string(), self.generation);
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key).map(|e| &e.value)
    }

    pub fn get_f64(&self, key: &str) -> f64 {
        self.get(key).map_or(0.0, Value::as_f64)
    }

    /// The generation of the last change to anything.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Keys changed after `since`.
    pub fn changed_since(&self, since: u64) -> Vec<&str> {
        if since >= self.generation {
            return Vec::new();
        }
        self.values
            .iter()
            .map(|(k, e)| (k, e.generation))
            .chain(self.removed.iter().map(|(k, g)| (k, *g)))
            .filter(|(_, generation)| *generation > since)
            .map(|(k, _)| k.as_str())
            .collect()
    }

    /// Every value, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.values.iter().map(|(k, e)| (k.as_str(), &e.value))
    }

    /// Queue an event for every document to hear.
    pub fn emit(&mut self, name: &str, data: impl Into<String>) {
        if self.events.len() >= MAX_PENDING_EVENTS {
            self.events.remove(0);
        }
        self.events.push(Event {
            name: name.to_string(),
            data: data.into(),
        });
    }

    /// Everything emitted since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Events waiting, without taking them.
    pub fn pending_events(&self) -> &[Event] {
        &self.events
    }

    /// The whole store as nested rhai maps, so `player.health` in an
    /// expression is ordinary property access.
    pub(crate) fn to_scope_maps(&self) -> BTreeMap<String, rhai::Dynamic> {
        let mut roots: BTreeMap<String, rhai::Dynamic> = BTreeMap::new();
        for (key, entry) in &self.values {
            let mut parts = key.split('.');
            let Some(first) = parts.next() else { continue };
            let rest: Vec<&str> = parts.collect();
            if rest.is_empty() {
                roots.insert(first.to_string(), entry.value.to_dynamic());
                continue;
            }
            let root = roots
                .entry(first.to_string())
                .or_insert_with(|| rhai::Map::new().into());
            if !root.is_map() {
                // `a` and `a.b` both published: the map wins, since the
                // nested name is the more specific claim.
                *root = rhai::Map::new().into();
            }
            insert_path(root, &rest, entry.value.to_dynamic());
        }
        roots
    }
}

fn insert_path(node: &mut rhai::Dynamic, path: &[&str], value: rhai::Dynamic) {
    let Some(mut map) = node.write_lock::<rhai::Map>() else {
        return;
    };
    let (head, tail) = (path[0], &path[1..]);
    if tail.is_empty() {
        map.insert(head.into(), value);
        return;
    }
    let child = map
        .entry(head.into())
        .or_insert_with(|| rhai::Map::new().into());
    if !child.is_map() {
        *child = rhai::Map::new().into();
    }
    insert_path(child, tail, value);
}

/// Whether a change to `changed` can affect something that reads `dep`.
///
/// Either may be the longer: a binding on `weapon` cares about
/// `weapon.ammo`, and one on `weapon.ammo` cares when `weapon` is replaced.
pub fn key_affects(changed: &str, dep: &str) -> bool {
    fn prefix(long: &str, short: &str) -> bool {
        long.len() > short.len() && long.starts_with(short) && long.as_bytes()[short.len()] == b'.'
    }
    changed == dep || prefix(changed, dep) || prefix(dep, changed)
}

#[cfg(test)]
mod tests;
