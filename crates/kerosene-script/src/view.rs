// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! What a script is allowed to see of the world.
//!
//! A snapshot, not the world itself. The reason is in the crate docs; the
//! consequence is here: this type is the whole of a script's read access, so
//! anything not on it is something scripts cannot know, and adding a field is
//! a deliberate widening rather than a side effect of exposing a struct.

use crate::Fields;
use kerosene_math::Vec3;

/// One entity, as a script sees it.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct EntityView {
    /// The engine's handle for this entity, passed back with any action.
    ///
    /// Opaque to scripts. The engine packs a slot index and a generation into
    /// it, so a handle a script held on to across a death cannot come back
    /// pointing at whatever was put in the slot next.
    pub id: u64,
    pub classname: String,
    pub targetname: String,
    pub origin: Vec3,
    pub fields: Fields,
}

impl EntityView {
    pub fn new(id: u64, classname: &str) -> EntityView {
        EntityView { id, classname: classname.to_string(), ..Default::default() }
    }

    pub fn with_name(mut self, name: &str) -> EntityView {
        self.targetname = name.to_string();
        self
    }

    pub fn with_origin(mut self, origin: Vec3) -> EntityView {
        self.origin = origin;
        self
    }

    pub fn with_field(mut self, key: &str, value: &str) -> EntityView {
        self.fields.insert(key.to_string(), value.to_string());
        self
    }

    pub fn field(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }
}

/// The world, as a script sees it.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct WorldView {
    pub entities: Vec<EntityView>,
    /// Convars, so a script can read the engine's own settings without
    /// needing a binding per convar.
    pub cvars: Fields,
    /// Simulated seconds since the map loaded.
    pub time: f32,
    pub tick: u64,
    pub map: String,
    /// Where the player is, if there is one.
    pub player: Option<EntityView>,
}

impl WorldView {
    /// Every entity with this name. Several may share one -- that is how a
    /// single output drives a group -- so this is a list, not an option.
    pub fn by_name<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a EntityView> + 'a {
        self.entities.iter().filter(move |e| e.targetname.eq_ignore_ascii_case(name))
    }

    pub fn by_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a EntityView> + 'a {
        self.entities.iter().filter(move |e| e.classname.eq_ignore_ascii_case(class))
    }

    pub fn by_id(&self, id: u64) -> Option<&EntityView> {
        self.entities.iter().find(|e| e.id == id)
    }
}
