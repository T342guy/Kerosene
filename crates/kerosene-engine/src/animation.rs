// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Posing animated models.
//!
//! A `prop_dynamic` keeps what it is playing in component fields (see
//! `kerosene_game::animated`); this reads them, with the model's clips, into
//! a pose. It runs headless: the tick uses it to notice a one-shot clip
//! ending and fire `OnAnimationDone`, and the host uses the same poses to
//! skin the model it draws.
//!
//! The field names are fixed here -- the engine has no game crate to ask --
//! and are read and written through the entity world's keyvalues.

use kerosene_anim::{Playback, Skeleton, Transform};
use kerosene_asset::Model;
use kerosene_entity::{EntityId, EntityWorld, ModelRole, Value};
use kerosene_math::Mat4;
use kerosene_vfs::Vfs;
use std::collections::HashMap;
use std::sync::Arc;

pub const ANIMATION: &str = "animation";
pub const STARTED: &str = "anim_start";
pub const RATE: &str = "anim_rate";
pub const PREVIOUS: &str = "anim_previous";
pub const PREVIOUS_STARTED: &str = "anim_previous_start";
pub const FADE_STARTED: &str = "anim_fade_start";
pub const DONE: &str = "anim_done";

/// Whether a class is the stock animated prop, by name. See
/// [`crate::physics::is_physics_prop`].
pub fn is_animated_prop(classname: &str) -> bool {
    ModelRole::of_stock_class(classname) == Some(ModelRole::Animated)
}

/// A model with its skeleton worked out, ready to pose.
pub struct Animated {
    pub model: Model,
    pub skeleton: Skeleton,
}

/// Loaded models, by name. A model that failed to load is remembered as
/// `None`, so it is warned about once rather than every tick.
#[derive(Default)]
pub struct Animations {
    models: HashMap<String, Option<Arc<Animated>>>,
}

impl Animations {
    pub fn new() -> Animations {
        Animations::default()
    }

    pub fn model(&mut self, vfs: &Vfs, name: &str) -> Option<Arc<Animated>> {
        self.models
            .entry(name.to_string())
            .or_insert_with(|| {
                let loaded = vfs
                    .read(&kerosene_asset::model_path(name))
                    .ok()
                    .and_then(|b| Model::from_bytes(&b).ok());
                if loaded.is_none() {
                    log::warn!("prop_dynamic: model {name} would not load");
                }
                loaded.map(|model| {
                    if model.bones.len() > kerosene_anim::MAX_BONES {
                        log::warn!(
                            "{name} has {} bones; the renderer skins {}, and the rest hold their rest pose",
                            model.bones.len(),
                            kerosene_anim::MAX_BONES
                        );
                    }
                    Arc::new(Animated {
                        skeleton: Skeleton::of(&model),
                        model,
                    })
                })
            })
            .clone()
    }

    /// An entity's playback, read from its fields.
    pub fn playback(entities: &EntityWorld, id: EntityId, animated: &Animated) -> Playback {
        let clip = |key: &str| {
            entities
                .keyvalue_text(id, key)
                .and_then(|n| animated.model.animation_index(n.trim()))
        };
        Playback {
            clip: clip(ANIMATION),
            started: entities.keyvalue_f32(id, STARTED, 0.0),
            rate: entities.keyvalue_f32(id, RATE, 1.0),
            fading: clip(PREVIOUS).map(|previous| {
                (
                    previous,
                    entities.keyvalue_f32(id, PREVIOUS_STARTED, 0.0),
                    entities.keyvalue_f32(id, FADE_STARTED, f32::NEG_INFINITY),
                )
            }),
        }
    }

    /// An animated entity's pose at game time `now`, as skinning matrices.
    /// `None` for an entity with no model or one that will not load.
    pub fn palette(
        &mut self,
        vfs: &Vfs,
        entities: &EntityWorld,
        id: EntityId,
        now: f32,
    ) -> Option<Vec<Mat4>> {
        let name = entities.keyvalue_text(id, "model")?;
        let animated = self.model(vfs, &name)?;
        let pose: Vec<Transform> =
            Self::playback(entities, id, &animated).pose(&animated.model, &animated.skeleton, now);
        Some(animated.skeleton.palette(&pose))
    }

    /// Fire `OnAnimationDone` for every one-shot clip that has ended, once,
    /// and send the prop back to its default clip.
    pub fn tick(&mut self, entities: &mut EntityWorld, vfs: &Vfs) {
        let now = entities.time;
        let mut finished: Vec<(EntityId, Option<String>)> = Vec::new();
        let registry = std::sync::Arc::clone(&entities.registry);
        for entity in entities
            .iter()
            .filter(|e| registry.model_role(&e.classname) == Some(ModelRole::Animated))
        {
            let id = entity.id;
            if entities.keyvalue_bool(id, DONE, false) {
                continue;
            }
            let Some(name) = entities.keyvalue_text(id, "model") else {
                continue;
            };
            let Some(animated) = self.model(vfs, &name) else {
                continue;
            };
            if Self::playback(entities, id, &animated).finished(&animated.model, now) {
                let current = entities.keyvalue_text(id, ANIMATION);
                let default = entities
                    .keyvalue_text(id, "defaultanim")
                    .map(|s| s.trim().to_string())
                    .filter(|d| !d.is_empty() && Some(d) != current.as_ref());
                finished.push((id, default));
            }
        }
        for (id, default) in finished {
            entities.set_keyvalue(id, DONE, Value::Bool(true));
            entities.fire_output(id, "OnAnimationDone", None, None);
            if let Some(clip) = default {
                play(entities, id, &clip);
            }
        }
    }
}

/// Start `clip` on an animated entity, fading from what it was playing.
/// The same as the game class's `SetAnimation`.
pub fn play(entities: &mut EntityWorld, id: EntityId, clip: &str) {
    let now = entities.time;
    if !entities.exists(id) {
        return;
    }
    let current = entities.keyvalue_text(id, ANIMATION).unwrap_or_default();
    let started = entities.keyvalue_f32(id, STARTED, now);
    if !current.is_empty() {
        entities.set_keyvalue(id, PREVIOUS, Value::Text(current));
        entities.set_keyvalue(id, PREVIOUS_STARTED, Value::Float(started));
        entities.set_keyvalue(id, FADE_STARTED, Value::Float(now));
    }
    entities.set_keyvalue(id, ANIMATION, Value::Text(clip.to_string()));
    entities.set_keyvalue(id, STARTED, Value::Float(now));
    entities.set_keyvalue(id, DONE, Value::Bool(false));
}
