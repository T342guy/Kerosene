// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Entities and their I/O connections.

use crate::{plain_properties, read_id, vec3_to_kv};
use crate::solid::{Solid, SolidError};
use crate::MapError;
use thiserror::Error;
use kerosene_kv::{KeyValues, Vec3Value};
use kerosene_math::{Angles, Vec3};

/// Separator used when a connection's parameter contains a comma.
///
/// Source hit exactly this problem: connections are comma-delimited, and then
/// somebody needed to pass `"1,2,3"` as a parameter. The fix there and here is
/// an alternate delimiter that cannot occur in authored text.
const ALT_DELIM: char = '\x1b';

#[derive(Debug, Error)]
pub enum ParseConnectionError {
    #[error("connection {0:?} has {1} fields; expected 5 (target,input,parameter,delay,times)")]
    WrongFieldCount(String, usize),
    #[error("connection {0:?} has an unreadable delay")]
    BadDelay(String),
}

/// One entity output wired to another entity's input.
///
/// This is the mechanism that makes Source maps behave without scripting: a
/// button's `OnPressed` fires a door's `Open`, after a delay, a set number of
/// times. Chisel edits these directly, and the engine walks them at runtime.
#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    /// Output on this entity, e.g. `OnPressed`.
    pub output: String,
    /// `targetname` of the entity to fire at. May name several entities.
    pub target: String,
    /// Input to fire, e.g. `Open`.
    pub input: String,
    /// Parameter passed to the input; often empty.
    pub parameter: String,
    /// Seconds to wait before firing.
    pub delay: f32,
    /// How many times this may fire; `-1` means unlimited.
    pub times_to_fire: i32,
}

impl Connection {
    pub fn new(output: &str, target: &str, input: &str) -> Self {
        Connection {
            output: output.to_string(),
            target: target.to_string(),
            input: input.to_string(),
            parameter: String::new(),
            delay: 0.0,
            times_to_fire: -1,
        }
    }

    pub fn with_delay(mut self, delay: f32) -> Self { self.delay = delay; self }
    pub fn with_parameter(mut self, p: &str) -> Self { self.parameter = p.to_string(); self }
    pub fn once(mut self) -> Self { self.times_to_fire = 1; self }

    pub fn is_unlimited(&self) -> bool { self.times_to_fire < 0 }

    pub fn parse(output: &str, value: &str) -> Result<Connection, ParseConnectionError> {
        let delim = if value.contains(ALT_DELIM) { ALT_DELIM } else { ',' };
        let parts: Vec<&str> = value.split(delim).collect();
        if parts.len() != 5 {
            return Err(ParseConnectionError::WrongFieldCount(value.to_string(), parts.len()));
        }
        Ok(Connection {
            output: output.to_string(),
            target: parts[0].trim().to_string(),
            input: parts[1].trim().to_string(),
            parameter: parts[2].to_string(),
            delay: parts[3]
                .trim()
                .parse()
                .map_err(|_| ParseConnectionError::BadDelay(value.to_string()))?,
            // A missing or unreadable count means "forever", which is the
            // forgiving reading and matches the engine default.
            times_to_fire: parts[4].trim().parse().unwrap_or(-1),
        })
    }

    /// Serialise the value half of the KeyValues pair.
    pub fn to_value(&self) -> String {
        use kerosene_kv::format_float as f;
        // Only reach for the escape delimiter when a comma would be ambiguous.
        let delim = if self.parameter.contains(',') { ALT_DELIM } else { ',' };
        format!(
            "{}{delim}{}{delim}{}{delim}{}{delim}{}",
            self.target, self.input, self.parameter, f(self.delay), self.times_to_fire
        )
    }
}

/// An entity: a bag of string properties, optionally with brushes.
///
/// Properties stay as strings in an ordered list rather than being parsed into
/// a typed struct, because the set of meaningful keys is defined by the *game*,
/// not the format. Chisel shows them through entity definitions; the engine
/// reads the ones its classes care about; anything neither understands still
/// round-trips instead of being silently dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub id: u32,
    pub properties: Vec<(String, String)>,
    /// Brushes belonging to this entity. Empty for point entities; non-empty
    /// makes it a brush entity like `func_door`.
    pub solids: Vec<Solid>,
    pub connections: Vec<Connection>,
}

impl Entity {
    pub fn new(id: u32, classname: &str) -> Self {
        Entity {
            id,
            properties: vec![("classname".to_string(), classname.to_string())],
            solids: Vec::new(),
            connections: Vec::new(),
        }
    }

    pub fn classname(&self) -> &str { self.get("classname").unwrap_or("") }
    pub fn targetname(&self) -> Option<&str> { self.get("targetname") }
    pub fn is_brush_entity(&self) -> bool { !self.solids.is_empty() }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.properties.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn has(&self, key: &str) -> bool { self.get(key).is_some() }

    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        let value = value.into();
        match self.properties.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value,
            None => self.properties.push((key.to_string(), value)),
        }
        self
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.properties.len();
        self.properties.retain(|(k, _)| k != key);
        before != self.properties.len()
    }

    pub fn get_f32(&self, key: &str, default: f32) -> f32 {
        self.get(key).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }

    pub fn get_i32(&self, key: &str, default: i32) -> i32 {
        self.get(key)
            .and_then(|v| v.trim().parse::<i32>().ok().or_else(|| v.trim().parse::<f32>().ok().map(|f| f as i32)))
            .unwrap_or(default)
    }

    pub fn get_vec3(&self, key: &str) -> Option<Vec3> {
        use kerosene_kv::FromKvValue;
        Vec3Value::from_kv(self.get(key)?).ok().map(|v| Vec3::from_array(v.to_array()))
    }

    /// Where the entity sits.
    ///
    /// A point entity carries an explicit `origin`. A brush entity usually has
    /// none, and its position *is* its geometry -- so the brush centre stands
    /// in, which is what a mover like `func_door` rotates about.
    pub fn origin(&self) -> Vec3 {
        if let Some(o) = self.get_vec3("origin") { return o; }
        if self.solids.is_empty() { return Vec3::ZERO; }
        let mut b = kerosene_math::Aabb::EMPTY;
        for s in &self.solids { b = b.union(&s.bounds()); }
        if b.is_empty() { Vec3::ZERO } else { b.center() }
    }

    pub fn set_origin(&mut self, o: Vec3) -> &mut Self { self.set("origin", vec3_to_kv(o)) }

    pub fn angles(&self) -> Angles {
        match self.get_vec3("angles") {
            Some(v) => Angles::new(v.x, v.y, v.z),
            None => Angles::ZERO,
        }
    }

    pub fn set_angles(&mut self, a: Angles) -> &mut Self {
        self.set("angles", format!("{a}"))
    }

    /// Spawnflags: a bitfield whose meaning is per-classname.
    pub fn spawnflags(&self) -> u32 { self.get_i32("spawnflags", 0) as u32 }

    pub fn has_spawnflag(&self, bit: u32) -> bool { self.spawnflags() & bit != 0 }

    /// Outputs with a given name.
    pub fn outputs<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Connection> + 'a {
        self.connections.iter().filter(move |c| c.output.eq_ignore_ascii_case(name))
    }

    pub fn connect(&mut self, c: Connection) -> &mut Self {
        self.connections.push(c);
        self
    }

    pub(crate) fn from_kv(kv: &KeyValues) -> Result<Entity, MapError> {
        let id = read_id(kv);
        let mut properties = plain_properties(kv);
        // `id` is structural, not a game property; keeping it in the bag would
        // let it round-trip into the compiled entity lump as a fake keyvalue.
        properties.retain(|(k, _)| k != "id");

        let mut solids = Vec::new();
        for s in kv.blocks("solid") {
            let solid = Solid::from_kv(s).map_err(|source| MapError::Solid {
                id: read_id(s),
                source,
            })?;
            solids.push(solid);
        }

        let mut connections = Vec::new();
        if let Some(conn) = kv.block("connections") {
            for (output, value) in conn.pairs() {
                match Connection::parse(output, value) {
                    Ok(c) => connections.push(c),
                    // A malformed connection loses one wire; dropping the whole
                    // map over it helps nobody, so report and carry on.
                    Err(e) => log::warn!("entity {id}: {e}"),
                }
            }
        }

        Ok(Entity { id, properties, solids, connections })
    }

    pub(crate) fn to_kv(&self, block_name: &str) -> KeyValues {
        let mut kv = KeyValues::new(block_name);
        kv.push_value("id", self.id);
        for (k, v) in &self.properties {
            kv.push(k.clone(), v.clone());
        }
        for s in &self.solids { kv.push_block(s.to_kv()); }
        if !self.connections.is_empty() {
            let mut conn = KeyValues::new("connections");
            for c in &self.connections {
                conn.push(c.output.clone(), c.to_value());
            }
            kv.push_block(conn);
        }
        kv
    }

    /// Validate the entity's brushes.
    pub fn validate_solids(&self) -> Vec<(u32, SolidError)> {
        self.solids
            .iter()
            .filter_map(|s| s.validate().err().map(|e| (s.id, e)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connections_round_trip() {
        let c = Connection::parse("OnPressed", "door1,Open,,0.5,-1").unwrap();
        assert_eq!(c.target, "door1");
        assert_eq!(c.input, "Open");
        assert_eq!(c.delay, 0.5);
        assert!(c.is_unlimited());
        assert_eq!(c.to_value(), "door1,Open,,0.5,-1");
    }

    #[test]
    fn a_parameter_containing_a_comma_survives() {
        // The failure this prevents: a parameter with a comma in it turns one
        // connection into six fields and the wire is silently lost.
        let c = Connection::new("OnTrigger", "logic", "SetValue")
            .with_parameter("1,2,3");
        let encoded = c.to_value();
        let back = Connection::parse("OnTrigger", &encoded).unwrap();
        assert_eq!(back.parameter, "1,2,3");
        assert_eq!(back.target, "logic");
    }

    #[test]
    fn malformed_connections_are_reported_not_guessed() {
        assert!(matches!(
            Connection::parse("OnX", "too,few,fields"),
            Err(ParseConnectionError::WrongFieldCount(_, 3))
        ));
        assert!(matches!(
            Connection::parse("OnX", "a,b,c,notanumber,-1"),
            Err(ParseConnectionError::BadDelay(_))
        ));
    }

    #[test]
    fn missing_fire_count_defaults_to_unlimited() {
        let c = Connection::parse("OnX", "a,b,c,0,").unwrap();
        assert!(c.is_unlimited());
    }

    #[test]
    fn point_entity_origin_comes_from_the_key() {
        let mut e = Entity::new(1, "info_player_start");
        e.set_origin(Vec3::new(0.0, 64.0, 32.0));
        assert_eq!(e.origin(), Vec3::new(0.0, 64.0, 32.0));
        assert_eq!(e.get("origin"), Some("0 64 32"));
    }

    #[test]
    fn brush_entity_origin_falls_back_to_its_geometry() {
        use kerosene_math::Aabb;
        let mut e = Entity::new(1, "func_door");
        e.solids.push(Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid"));
        assert_eq!(e.origin(), Vec3::splat(32.0));
        assert!(e.is_brush_entity());
    }

    #[test]
    fn structural_id_does_not_leak_into_properties() {
        let kv = KeyValues::parse(r#"entity { "id" "7" "classname" "light" }"#).unwrap();
        let e = Entity::from_kv(kv.block("entity").unwrap()).unwrap();
        assert_eq!(e.id, 7);
        assert!(!e.has("id"), "id is structural and must not become a keyvalue");
        assert_eq!(e.classname(), "light");
    }

    #[test]
    fn unknown_properties_survive_a_round_trip() {
        // The engine will not know every key a game mod invents; the format
        // must not eat them.
        let kv = KeyValues::parse(r#"entity { "id" "1" "classname" "x" "my_mod_key" "value" }"#).unwrap();
        let e = Entity::from_kv(kv.block("entity").unwrap()).unwrap();
        let text = e.to_kv("entity").to_text();
        assert!(text.contains("my_mod_key"), "{text}");
    }

    #[test]
    fn angles_and_spawnflags_read_back() {
        let mut e = Entity::new(1, "light_spot");
        e.set_angles(Angles::new(-45.0, 90.0, 0.0));
        e.set("spawnflags", "5");
        assert_eq!(e.angles().pitch, -45.0);
        assert_eq!(e.angles().yaw, 90.0);
        assert!(e.has_spawnflag(1));
        assert!(e.has_spawnflag(4));
        assert!(!e.has_spawnflag(2));
    }

    #[test]
    fn outputs_are_matched_case_insensitively() {
        let mut e = Entity::new(1, "func_button");
        e.connect(Connection::new("OnPressed", "d", "Open"));
        assert_eq!(e.outputs("onpressed").count(), 1);
        assert_eq!(e.outputs("OnPressed").count(), 1);
        assert_eq!(e.outputs("OnDamaged").count(), 0);
    }
}
