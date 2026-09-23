// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Posing animated models.
//!
//! A `prop_dynamic` keeps what it is playing in plain fields (see
//! `kerosene_game::animated`); this reads them, with the model's clips, into
//! a pose. It runs headless: the tick uses it to notice a one-shot clip
//! ending and fire `OnAnimationDone`, and the host uses the same poses to
//! skin the model it draws.
//!
//! The field names are fixed here -- the engine has no game crate to ask --
//! and `kerosene_game::animated` carries the same names for its own use.

use kerosene_anim::{Playback, Skeleton, Transform};
use kerosene_asset::Model;
use kerosene_entity::{Entity, EntityId, EntityWorld, Value};
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

/// Whether an entity is an animated model.
pub fn is_animated_prop(classname: &str) -> bool {
    classname.eq_ignore_ascii_case("prop_dynamic")
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
    pub fn playback(entity: &Entity, animated: &Animated) -> Playback {
        let clip = |key: &str| {
            entity
                .fields
                .text(key)
                .and_then(|n| animated.model.animation_index(n.trim()))
        };
        Playback {
            clip: clip(ANIMATION),
            started: entity.fields.f32(STARTED, 0.0),
            rate: entity.fields.f32(RATE, 1.0),
            fading: clip(PREVIOUS).map(|previous| {
                (
                    previous,
                    entity.fields.f32(PREVIOUS_STARTED, 0.0),
                    entity.fields.f32(FADE_STARTED, f32::NEG_INFINITY),
                )
            }),
        }
    }

    /// An animated entity's pose at game time `now`, as skinning matrices.
    /// `None` for an entity with no model or one that will not load.
    pub fn palette(&mut self, vfs: &Vfs, entity: &Entity, now: f32) -> Option<Vec<Mat4>> {
        let name = entity.fields.text("model")?;
        let animated = self.model(vfs, &name)?;
        let pose: Vec<Transform> =
            Self::playback(entity, &animated).pose(&animated.model, &animated.skeleton, now);
        Some(animated.skeleton.palette(&pose))
    }

    /// Fire `OnAnimationDone` for every one-shot clip that has ended, once,
    /// and send the prop back to its default clip.
    pub fn tick(&mut self, entities: &mut EntityWorld, vfs: &Vfs) {
        let now = entities.time;
        let mut finished: Vec<(EntityId, Option<String>)> = Vec::new();
        for entity in entities.iter().filter(|e| is_animated_prop(&e.classname)) {
            if entity.fields.bool(DONE, false) {
                continue;
            }
            let Some(name) = entity.fields.text("model") else {
                continue;
            };
            let Some(animated) = self.model(vfs, &name) else {
                continue;
            };
            if Self::playback(entity, &animated).finished(&animated.model, now) {
                let current = entity.fields.text(ANIMATION).map(|s| s.into_owned());
                let default = entity
                    .fields
                    .text("defaultanim")
                    .map(|s| s.trim().to_string())
                    .filter(|d| !d.is_empty() && Some(d) != current.as_ref());
                finished.push((entity.id, default));
            }
        }
        for (id, default) in finished {
            if let Some(e) = entities.get_mut(id) {
                e.fields.set(DONE, Value::Bool(true));
            }
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
    let Some(e) = entities.get_mut(id) else {
        return;
    };
    let current = e
        .fields
        .text(ANIMATION)
        .map(|s| s.into_owned())
        .unwrap_or_default();
    let started = e.fields.f32(STARTED, now);
    if !current.is_empty() {
        e.fields.set(PREVIOUS, Value::Text(current));
        e.fields.set(PREVIOUS_STARTED, Value::Float(started));
        e.fields.set(FADE_STARTED, Value::Float(now));
    }
    e.fields.set(ANIMATION, Value::Text(clip.to_string()));
    e.fields.set(STARTED, Value::Float(now));
    e.fields.set(DONE, Value::Bool(false));
}
