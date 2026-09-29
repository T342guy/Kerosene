// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The player: movement, damage, footsteps, and what they use and hold.

use super::*;

impl Engine {
    /// Which way the player is leaning, on the floor plane.
    ///
    /// The direction they are *asking* to go, not the direction they are
    /// managing to: props are solid to the player's hull trace, so walking
    /// into a crate stops the player dead and leaves them with no velocity at
    /// all. Reading their velocity would mean the harder you pressed into a
    /// crate the less you pushed it, which is how leaning on an 8 kg box came
    /// to do nothing whatsoever.
    pub(super) fn player_push_direction(&self, input: &InputState) -> Vec3 {
        let wish = Vec3::new(input.forward, input.side, 0.0);
        if wish.length_squared() < 1e-6 {
            return Vec3::ZERO;
        }
        let basis = self.player.view_angles.vectors();
        let mut direction = basis.forward * wish.x + basis.right * wish.y;
        direction.z = 0.0;
        direction.normalize_or_zero()
    }

    pub(super) fn movement_params(&self) -> MoveParams {
        let gravity = self.console.float("sv_gravity");
        let mut params = MoveParams {
            gravity,
            max_speed: self.console.float("sv_maxspeed"),
            accelerate: self.console.float("sv_accelerate"),
            air_accelerate: self.console.float("sv_airaccelerate"),
            friction: self.console.float("sv_friction"),
            stop_speed: self.console.float("sv_stopspeed"),
            step_size: self.console.float("sv_stepsize"),
            air_speed_cap: self.console.float("sv_air_max_wishspeed"),
            ..Default::default()
        };
        // Derived rather than a convar of its own, so changing gravity keeps
        // jump height where the designer put it.
        params.jump_impulse = params.jump_for_height(self.console.float("sv_jump_height"));
        // `player_speedmod`'s, kept on the player's entity so a save keeps it.
        let scale = self
            .player
            .entity
            .and_then(|id| self.entities.get(id))
            .map_or(1.0, |e| e.fields.f32("speed_scale", 1.0));
        if scale.is_finite() && scale >= 0.0 {
            params.max_speed *= scale;
        }
        params
    }

    pub(super) fn apply_fall_damage(&mut self, speed: f32) {
        // Below the safe speed, landing costs nothing. Above it, damage rises
        // with the excess -- Source's curve, near enough.
        let safe = self.console.float("sv_falldamage_safe");
        if speed <= safe {
            return;
        }
        let scale = self.console.float("sv_falldamage_scale");
        self.hurt_player(
            (speed - safe) * scale,
            &format!("fall damage at {speed:.0} ku/s"),
        );
    }

    /// Emit a footstep when the player has travelled a stride on the ground.
    ///
    /// The surface the player is standing on is resolved each step through the
    /// trace's material, so a footstep on metal sounds different from one on
    /// concrete — which is the whole point of `$surfaceprop`. Distance is
    /// accumulated only while grounded, so jumping and air-strafing do not
    /// tick the stride forward.
    pub(super) fn update_footsteps(&mut self, dt: f32) {
        let (horizontal, origin) = {
            let movement = &self.player.movement;
            if !movement.on_ground {
                return;
            }
            // Horizontal speed only: standing still, however long, is silent.
            let horizontal = Vec3::new(movement.velocity.x, movement.velocity.y, 0.0).length();
            (horizontal, movement.origin)
        };

        let min_speed = self.console.float("sv_footstep_min_speed");
        if horizontal < min_speed {
            return;
        }

        self.player.step_distance += horizontal * dt;
        let stride = self.console.float("sv_footstep_stride");
        if self.player.step_distance < stride {
            return;
        }
        self.player.step_distance = 0.0;

        let Some(surface) = self.surface_property_at(origin) else {
            return;
        };
        let vfs = self.vfs.clone();
        let sound = surface.footstep_sound(self.player.step_index);
        self.player.step_index = self.player.step_index.wrapping_add(1);
        self.audio.play_if_present(&vfs, &sound, Some(origin), 0.5);
    }

    /// Resolve the physical surface under a point by tracing down and looking
    /// up the material's `$surfaceprop`.
    ///
    /// The trace reports the texinfo of the face it hit, and the BSP names that
    /// texinfo's material; the material file then carries the `$surfaceprop`.
    /// This is the runtime end of the chain the compiler stores — the first
    /// consumer the format's key was added for.
    pub(super) fn surface_property_at(
        &self,
        origin: Vec3,
    ) -> Option<kerosene_asset::SurfaceProperty> {
        let bsp = &self.level.as_ref()?.bsp;
        let down = origin + Vec3::new(0.0, 0.0, -self.console.float("sv_footstep_trace"));
        let hull = self.player.movement.hull();
        let trace = bsp.trace_box(
            origin + Vec3::Z,
            down,
            hull.mins,
            hull.maxs,
            contents::MASK_PLAYER_SOLID,
        );
        if trace.texture_index < 0 {
            return None;
        }
        let name = bsp.texinfo_name(trace.texture_index as usize);
        if name.is_empty() {
            return None;
        }
        let source = kerosene_asset::WithMaterialSources(&*self.vfs);
        let material = self
            .resources
            .load::<kerosene_asset::Material>(&source, &kerosene_asset::material_path(name))
            .get()?;
        Some(material.surface_type())
    }

    /// Take health off the player, and respawn them if it runs out.
    ///
    /// One place rather than one per source of damage, because "what happens
    /// at zero" is a rule about the player and not about the thing that hurt
    /// them -- and because a second copy would be the one that forgot to
    /// respawn.
    ///
    /// The game hears of it first, through [`Game::player_damaged`], and may
    /// change the amount -- armour, difficulty, god mode. At zero it hears
    /// [`Game::player_died`]; unless it takes the death over, the player
    /// respawns at the map's start.
    ///
    /// [`Game::player_damaged`]: crate::Game::player_damaged
    /// [`Game::player_died`]: crate::Game::player_died
    pub fn hurt_player(&mut self, amount: f32, reason: &str) {
        if amount <= 0.0 || self.player.health <= 0.0 || self.god() {
            return;
        }
        let amount = self
            .with_game_mut(|game, engine| game.player_damaged(engine, amount, reason))
            .unwrap_or(amount);
        if amount <= 0.0 || !amount.is_finite() {
            return;
        }
        self.player.health -= amount;
        if self.buddha() {
            self.player.health = self.player.health.max(1.0);
        }
        self.console.developer(format!(
            "{reason}: -{amount:.0} hp ({:.0} left)",
            self.player.health.max(0.0)
        ));
        self.ui_emit("player_damaged", format!("{amount:.0}"));
        if self.player.health <= 0.0 {
            self.kill_player(reason);
        }
    }

    /// Kill the player outright, god mode or not: `kill`, and the end of
    /// [`hurt_player`](Engine::hurt_player). The game hears of it through
    /// [`Game::player_died`] and may take the death over; otherwise the
    /// player respawns at the start.
    ///
    /// [`Game::player_died`]: crate::Game::player_died
    pub fn kill_player(&mut self, reason: &str) {
        self.player.health = 0.0;
        self.ui_emit("player_died", reason);
        self.console.print("you died");
        let handled = self
            .with_game_mut(|game, engine| game.player_died(engine, reason))
            .unwrap_or(false);
        if !handled {
            self.spawn_player();
        }
    }

    /// Press whatever the player is looking at.
    ///
    /// A trace from the eye rather than a radius around the player: standing
    /// between two buttons and pressing the one you are facing is the whole
    /// expectation, and a proximity test cannot honour it.
    pub(super) fn use_what_is_in_front(&mut self) {
        let Some(level) = &self.level else { return };
        let range = self.console.float("sv_use_range").max(1.0);
        let eye = self.player.movement.eye_position();
        let forward = self.player.view_angles.forward();
        let end = eye + forward * range;

        // Carrying a prop? The use key puts it down: it keeps the player's own
        // motion so letting go while walking sets it down in front of them
        // rather than stopping it dead in the air, but nothing is added to it.
        if let Some(held) = self.held_prop.take() {
            if self
                .physics
                .launch_prop(held.id, self.player.movement.velocity)
            {
                self.console.developer("dropped prop");
            } else {
                self.console.warn("held prop no longer has a body");
            }
            return;
        }

        // Otherwise, a prop in the crosshair is picked up first; only when
        // there is none does the press fall through to a brush entity's Use.
        if let Some((prop, rotation)) = self.physics.pick_prop(eye, forward, range, &self.entities)
        {
            let grabbed = Angles::from_quat(rotation);
            self.held_prop = Some(HeldProp {
                id: prop,
                yaw_offset: kerosene_math::wrap180(grabbed.yaw - self.player.view_angles.yaw),
                pitch: grabbed.pitch,
                roll: grabbed.roll,
            });
            self.console.developer("picked up prop");
            return;
        }

        let world = LevelCollision::new(&level.bsp, &self.entities);
        let trace = world.trace(
            eye,
            end,
            Vec3::ZERO,
            Vec3::ZERO,
            contents::MASK_PLAYER_SOLID,
        );
        // Model 0 is the world itself. Walls are not usable, and reporting a
        // hit on one as a failed use would be noise on every missed press.
        if !trace.hit() || trace.model == 0 {
            return;
        }

        let Some(target) = self
            .entities
            .iter()
            .find(|e| e.brush_model == Some(trace.model))
            .map(|e| e.id)
        else {
            return;
        };

        // Through the queue, so a use arrives the same way a wired output
        // would: same ordering, same delays, same rules.
        self.entities.queue_input(
            kerosene_entity::Target::Handle(target),
            "Use",
            "",
            0.0,
            self.player.entity,
            self.player.entity,
        );
    }

    /// Throw the carried prop, hard, along the view direction.
    ///
    /// The player's own motion carries into it, so running forward and
    /// throwing sends a crate further than standing still does -- which is
    /// what anyone who has thrown something while moving expects.
    pub(super) fn throw_held_prop(&mut self) {
        let Some(held) = self.held_prop.take() else {
            return;
        };
        let speed = self.console.float("phys_launch_speed");
        let velocity = self.player.movement.velocity + self.player.view_angles.forward() * speed;
        if self.physics.launch_prop(held.id, velocity) {
            self.console.developer("threw prop");
        } else {
            self.console.warn("held prop no longer has a body");
        }
    }

    /// Where the eye is, interpolated between the last two ticks.
    ///
    /// Without this the view snaps at the tick rate, which is visible as a
    /// judder on any display refreshing faster than 64 Hz -- which is all of
    /// them now.
    pub fn interpolated_eye(&self, alpha: f32) -> Vec3 {
        let hull = self.player.movement.hull();
        let position = self
            .player
            .previous_origin
            .lerp(self.player.movement.origin, alpha.clamp(0.0, 1.0));
        position + Vec3::Z * hull.view_height
    }

    /// Forget the carried prop without throwing it: its physics world is
    /// about to be replaced.
    pub(crate) fn held_prop_clear(&mut self) {
        self.held_prop = None;
    }

    /// The prop the pick-up tool is carrying, if any.
    pub fn held_prop(&self) -> Option<EntityId> {
        self.held_prop.map(|h| h.id)
    }

    /// Contents the player is standing in, for water and trigger checks.
    pub fn player_contents(&self) -> u32 {
        match &self.level {
            Some(level) => level
                .bsp
                .point_contents_brushes(self.player.movement.origin + Vec3::Z * 4.0),
            None => contents::EMPTY,
        }
    }
}
