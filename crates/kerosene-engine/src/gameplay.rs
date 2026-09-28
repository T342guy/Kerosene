// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! What entities do to the player, and the player to them: damage, touch,
//! and the requests the gameplay classes leave.
//!
//! A class says what being touched or damaged *means* -- a pickup vanishes,
//! glass breaks -- with [`ClassDef::on_touch`] and [`ClassDef::on_damage`].
//! Noticing that it happened is the engine's, because the engine has the
//! player's box and the traces: [`Engine::damage_entity`] is how a weapon,
//! an explosion or a script hurts something, and every tick the player's box
//! is tested against every class that listens for touch.
//!
//! [`ClassDef::on_touch`]: kerosene_entity::ClassDef::on_touch
//! [`ClassDef::on_damage`]: kerosene_entity::ClassDef::on_damage

use crate::engine::Engine;
use kerosene_entity::{EntityId, Value, host_requests};
use kerosene_math::{Aabb, Vec3};

/// Half the size of a point entity's touch box, unless it sets
/// `touch_size`: about a pickup's.
const DEFAULT_TOUCH_SIZE: f32 = 32.0;

/// The field in which the engine keeps whether the player touched an
/// entity last tick, so its touch handler runs on the tick they meet.
const TOUCHING: &str = "player_touching";

impl Engine {
    /// Damage an entity: what a weapon, an explosion or a script does. Its
    /// class decides what that means (see
    /// [`ClassDef::on_damage`](kerosene_entity::ClassDef::on_damage)), and
    /// every entity fires `OnDamaged` with the amount as its parameter.
    /// Returns whether the class took the damage; `false` for one with no
    /// damage handler, or an amount that is not positive.
    pub fn damage_entity(&mut self, id: EntityId, amount: f32, attacker: Option<EntityId>) -> bool {
        if amount.is_nan() || amount <= 0.0 || !amount.is_finite() {
            return false;
        }
        let Some(class) = self.entities.get(id).map(|e| e.classname.clone()) else {
            return false;
        };
        self.entities
            .fire_output(id, "OnDamaged", attacker, Some(&amount.to_string()));
        match self.entities.registry.damage_handler(&class) {
            Some(handler) => handler(&mut self.entities, id, amount, attacker),
            None => false,
        }
    }

    /// Run touch handlers for what the player walked into this tick.
    pub(crate) fn update_touchers(&mut self) {
        let Some(level) = &self.level else { return };
        let Some(player) = self.player.entity else {
            return;
        };
        let hull = self.player.movement.hull();
        let player_box = Aabb::new(
            self.player.movement.origin + hull.mins,
            self.player.movement.origin + hull.maxs,
        );
        let alive = self.player.health > 0.0;

        let registry = self.entities.registry.clone();
        let listening: Vec<_> = self
            .entities
            .iter()
            .filter(|e| e.id != player)
            .filter_map(|e| registry.touch_handler(&e.classname).map(|h| (e.id, h)))
            .collect();
        for (id, handler) in listening {
            let Some(entity) = self.entities.get(id) else {
                continue;
            };
            let bounds = match entity.brush_model.and_then(|m| level.bsp.models.get(m)) {
                Some(model) => crate::engine::brush_pose(
                    Some(&level.bsp),
                    entity.brush_model.unwrap_or(0),
                    entity.origin,
                    entity.angles,
                )
                .bounds_of(model.bounds()),
                None => {
                    let half = self
                        .entities
                        .keyvalue_f32(id, "touch_size", DEFAULT_TOUCH_SIZE)
                        .max(1.0)
                        * 0.5;
                    Aabb::new(
                        entity.origin - Vec3::splat(half),
                        entity.origin + Vec3::splat(half),
                    )
                }
            };
            let inside =
                alive && !self.entities.keyvalue_bool(id, "disabled", false) && bounds.intersects(&player_box);
            let was = entity.fields.bool(TOUCHING, false);
            if inside != was
                && let Some(e) = self.entities.get_mut(id)
            {
                e.fields.set(TOUCHING, Value::Bool(inside));
            }
            if inside && !was {
                handler(&mut self.entities, id, player);
            }
        }
    }

    /// Carry out a request one of the gameplay classes left. `false` for a
    /// kind that is not one of these.
    pub(crate) fn gameplay_entity_request(
        &mut self,
        kind: &str,
        payload: &str,
        caller: EntityId,
    ) -> bool {
        match kind {
            host_requests::TELEPORT_PLAYER => {
                let Some((origin, angles)) =
                    self.entities.get(caller).map(|e| (e.origin, e.angles))
                else {
                    return true;
                };
                self.teleport_player(origin, Some(angles));
            }
            host_requests::HURT_PLAYER => {
                let mut words = payload.split_whitespace();
                let damage = words
                    .next()
                    .and_then(|w| w.parse::<f32>().ok())
                    .unwrap_or(0.0);
                let radius = words
                    .next()
                    .and_then(|w| w.parse::<f32>().ok())
                    .unwrap_or(0.0);
                let amount = match (radius > 0.0, self.entities.get(caller)) {
                    (true, Some(e)) => {
                        let eye = self.player.movement.eye_position();
                        let distance = e
                            .origin
                            .distance(self.player.movement.origin)
                            .min(e.origin.distance(eye));
                        // Full at the centre, nothing at the edge.
                        damage * (1.0 - distance / radius).max(0.0)
                    }
                    _ => damage,
                };
                let name = self
                    .entities
                    .get(caller)
                    .map(|e| e.classname.clone())
                    .unwrap_or_default();
                self.hurt_player(amount, &name);
            }
            host_requests::HEAL_PLAYER => {
                let amount = payload.trim().parse::<f32>().unwrap_or(0.0).max(0.0);
                let room = (self.max_health - self.player.health).max(0.0);
                let player = self.player.entity;
                if room <= 0.0 || self.player.health <= 0.0 {
                    self.entities
                        .fire_output(caller, "OnHealthFull", player, None);
                } else {
                    let given = amount.min(room);
                    self.player.health += given;
                    self.ui_emit("player_healed", format!("{given:.0}"));
                    self.entities.fire_output(
                        caller,
                        "OnPlayerHealed",
                        player,
                        Some(&given.to_string()),
                    );
                    self.entities.remove(caller);
                }
            }
            host_requests::END_GAME => self.end_game(),
            _ => return false,
        }
        true
    }
}
