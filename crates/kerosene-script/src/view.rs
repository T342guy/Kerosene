// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
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
    /// The entity's class.
    pub classname: String,
    /// Its name, empty for none.
    pub targetname: String,
    /// Where it is.
    pub origin: Vec3,
    /// Every keyvalue, as text.
    pub fields: Fields,
}

impl EntityView {
    /// A view of an entity with no name, at the origin, with no keyvalues.
    pub fn new(id: u64, classname: &str) -> EntityView {
        EntityView {
            id,
            classname: classname.to_string(),
            ..Default::default()
        }
    }

    /// The same view, named.
    pub fn with_name(mut self, name: &str) -> EntityView {
        self.targetname = name.to_string();
        self
    }

    /// The same view, placed.
    pub fn with_origin(mut self, origin: Vec3) -> EntityView {
        self.origin = origin;
        self
    }

    /// The same view, with one more keyvalue.
    pub fn with_field(mut self, key: &str, value: &str) -> EntityView {
        self.fields.insert(key.to_string(), value.to_string());
        self
    }

    /// One keyvalue's text, if the entity has it.
    pub fn field(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }
}

/// The world, as a script sees it.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct WorldView {
    /// Every entity in the map.
    pub entities: Vec<EntityView>,
    /// Convars, so a script can read the engine's own settings without
    /// needing a binding per convar.
    pub cvars: Fields,
    /// Simulated seconds since the map loaded.
    pub time: f32,
    /// Ticks simulated since the map loaded.
    pub tick: u64,
    /// The map's name, `kerosene_room`.
    pub map: String,
    /// Where the player is, if there is one.
    pub player: Option<EntityView>,
    /// The store: achievements, stats, the player's name. Read through the
    /// `platform` object.
    pub platform: kerosene_platform::PlatformView,
}

impl WorldView {
    /// Every entity with this name. Several may share one -- that is how a
    /// single output drives a group -- so this is a list, not an option.
    pub fn by_name<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a EntityView> + 'a {
        self.entities
            .iter()
            .filter(move |e| e.targetname.eq_ignore_ascii_case(name))
    }

    /// Every entity of this class, however it is capitalised.
    pub fn by_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a EntityView> + 'a {
        self.entities
            .iter()
            .filter(move |e| e.classname.eq_ignore_ascii_case(class))
    }

    /// The entity with this handle, if it still exists.
    pub fn by_id(&self, id: u64) -> Option<&EntityView> {
        self.entities.iter().find(|e| e.id == id)
    }
}
