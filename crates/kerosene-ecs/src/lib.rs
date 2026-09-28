// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The entity-component-system every entity lives in: a pinned `bevy_ecs`,
//! behind this crate so an upgrade is a change here and nowhere else.
//!
//! Game code imports from [`prelude`] rather than from Bevy:
//!
//! ```
//! use kerosene_ecs::prelude::*;
//!
//! /// A door, as far as its keyvalues and its state go.
//! #[derive(Component, Reflect, Default)]
//! #[reflect(Component, Default)]
//! struct Mover {
//!     #[reflect(@Key("speed"), @Help("Units per second."))]
//!     speed: f32,
//! }
//!
//! let mut world = World::new();
//! let door = world.spawn(Mover { speed: 100.0 }).id();
//! assert_eq!(world.get::<Mover>(door).unwrap().speed, 100.0);
//! ```
//!
//! The prelude brings `bevy_ecs` and `bevy_reflect` into scope by those
//! names, because that is where the derive macros look for them: a crate
//! that uses the prelude needs neither as a dependency of its own.
//!
//! Why an ECS, and why this one, is in the refactor design document
//! (`src/refactor/`): the world owns every entity's data and entities refer
//! to each other by generational ID, which suits Rust's ownership rules
//! where a graph of references does not.

pub use bevy_ecs;
pub use kerosene_reflect;

pub use bevy_ecs::change_detection::Mut;
pub use bevy_ecs::component::{Component, Mutable};
pub use bevy_ecs::entity::Entity;
pub use bevy_ecs::world::World;

/// What a module that declares or uses components needs.
pub mod prelude {
    pub use crate::{Component, Entity, Mut, World};
    pub use bevy_ecs;
    pub use bevy_ecs::reflect::ReflectComponent;
    pub use kerosene_reflect::bevy_reflect;
    pub use kerosene_reflect::bevy_reflect::std_traits::ReflectDefault;
    pub use kerosene_reflect::{Help, Hidden, Key, Label, Networked, Reflect, Transient, Widget};
}
