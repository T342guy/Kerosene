// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The fixed-step simulation: one tick, and the systems it runs (streaming,
//! acoustics, triggers).

use super::*;

impl Engine {
    /// Set the master volume from `volume`, or silence while in the
    /// background under `snd_mute_losefocus`. Every frame and every tick, so
    /// a paused game's volume still follows the options slider.
    pub(super) fn update_volume(&mut self) {
        let muted = self.background && self.console.bool("snd_mute_losefocus");
        let volume = if muted {
            0.0
        } else {
            self.console.float("volume")
        };
        self.audio.set_volume(volume);
        self.audio.set_world_paused(self.is_paused());
    }

    /// One fixed simulation step.
    pub fn tick(&mut self, dt: f32, input: &InputState) {
        self.time += dt;
        self.tick_count += 1;
        self.player.previous_origin = self.player.movement.origin;
        let player_fov = self.console.float("cl_fov");
        self.view.tick(self.time, dt, player_fov);

        self.player.view_angles = input.view_angles.clamped_view();
        // Read every tick rather than at map load, so `developer 2` typed
        // mid-map starts tracing entity I/O at once.
        self.entities.set_trace(self.console.int("developer") >= 2);

        self.with_game_mut(|game, engine| game.pre_tick(engine, input, dt));

        if let Some(level) = &self.level {
            // Rebuilt each tick: a door that moved since the last one has to
            // block where it is now, not where it was -- and so does a prop
            // the player just kicked.
            let world = PlayerCollision::new(&level.bsp, &self.entities, &self.physics);
            let mut params = self.movement_params();
            if input.walk {
                params.max_speed = params
                    .max_speed
                    .min(self.console.float("sv_walkspeed").max(0.0));
            }
            // The dead do not walk: a game that took over a death has the
            // player where they fell until it says otherwise.
            let alive = self.player.health > 0.0;
            let move_input = MoveInput {
                forward: if alive { input.forward } else { 0.0 },
                side: if alive { input.side } else { 0.0 },
                up: if alive { input.up } else { 0.0 },
                jump: alive && input.jump,
                duck: alive && input.duck,
                view_angles: self.player.view_angles,
            };

            self.player.movement.noclip = self.console.bool("sv_noclip");
            let result = kerosene_physics::player_move(
                &mut self.player.movement,
                &move_input,
                &params,
                &world,
                dt,
            );

            if let Some(speed) = result.landed_at_speed {
                self.apply_fall_damage(speed);
            }

            self.update_footsteps(dt);
        }

        // Keep the player's entity in step, so `!player` targets and trigger
        // tests both see where they actually are.
        if let Some(id) = self.player.entity {
            let origin = self.player.movement.origin;
            if let Some(e) = self.entities.get_mut(id) {
                e.origin = origin;
            }
        }

        // The ears follow the player. Done here rather than in the renderer
        // so that a headless run mixes the same audio a windowed one does.
        self.audio.set_listener(
            self.player.movement.eye_position(),
            self.player.view_angles.vectors(),
        );
        self.update_volume();
        self.update_acoustics();

        // An interactive world panel under the crosshair takes the press
        // first: pressing use on a keypad presses its key, not the wall.
        let alive = self.player.health > 0.0;
        let use_edge = alive && input.use_key && !self.player.use_held;
        let attack_edge = alive && input.attack && !self.player.attack_held;
        let panel_took = self.world_panel_input(input.use_key, input.attack, use_edge, attack_edge);

        if use_edge && !panel_took {
            self.use_what_is_in_front();
        }
        self.player.use_held = input.use_key;

        // Attack throws what is being carried. Two buttons rather than one:
        // putting a crate down where you are standing and hurling it across
        // the room are different intentions, and a single key that always did
        // the second gave no way to express the first.
        if attack_edge && !panel_took {
            self.throw_held_prop();
        }
        self.player.attack_held = input.attack;

        // Snapshot every brush model's pose before entity I/O has a chance to
        // move it this tick, so rendering has both ends of the motion to
        // interpolate between.
        self.previous_brush_poses = self
            .entities
            .iter()
            .filter_map(|e| e.brush_model.map(|m| (m, (e.origin, e.angles))))
            .collect();

        self.update_triggers(dt);
        self.update_touchers();
        self.entities.run(dt);
        self.take_entity_requests();
        self.with_game_mut(|game, engine| game.tick(engine, input, dt));

        // Rigid-body props: give new ones bodies, simulate, and write each
        // body's pose back so the renderer draws it where the physics put it.
        if self.level.is_some() {
            // The player goes in first, so a prop meets them where they are
            // this tick rather than where they were last one.
            let hull = self.player.movement.hull();
            let origin = self.player.movement.origin;
            self.physics
                .sync_player(origin, hull.mins, hull.maxs, self.player.movement.velocity);
            // Then lean on whatever is in the way. Solidity and pushing are
            // separate things: the body above makes props bounce off the
            // player, this makes the player able to move them.
            self.physics.push_props(
                Aabb::new(origin + hull.mins, origin + hull.maxs),
                self.player_push_direction(input),
                self.console.float("phys_player_push_force"),
                self.console.float("sv_maxspeed").max(0.0),
                dt,
            );
            // A carried prop is steered toward a point in front of the player,
            // and this happens before the world is stepped so that the step is
            // what actually moves it. The point is traced against the world so
            // the hold point itself does not land inside a wall, and the yaw
            // follows the player so the face that was grabbed keeps facing them.
            if let Some(held) = self.held_prop {
                if self.entities.exists(held.id) {
                    let distance = self.console.float("phys_hold_distance").max(32.0);
                    let eye = self.player.movement.eye_position();
                    let forward = self.player.view_angles.forward();

                    // Clamp the reach to whatever the world allows, keeping the
                    // prop's own half-extent clear of the surface it meets.
                    let mut reach = distance;
                    if let (Some(level), Some(half)) =
                        (&self.level, self.physics.prop_half_extent(held.id))
                    {
                        let margin = half.x.max(half.y).max(half.z);
                        let world = LevelCollision::new(&level.bsp, &self.entities);
                        let trace = world.trace(
                            eye,
                            eye + forward * distance,
                            Vec3::ZERO,
                            Vec3::ZERO,
                            contents::MASK_PLAYER_SOLID,
                        );
                        if trace.fraction < 1.0 {
                            reach = (trace.fraction * distance - margin).clamp(0.0, distance);
                        }
                    }

                    let position = eye + forward * reach - Vec3::Z * 8.0;
                    let yaw = kerosene_math::wrap180(self.player.view_angles.yaw + held.yaw_offset);
                    let rotation =
                        Quat::from_mat3(&Angles::new(held.pitch, yaw, held.roll).to_mat3());
                    self.physics.steer_prop(
                        held.id,
                        position,
                        rotation,
                        dt,
                        crate::physics::HoldLimits {
                            max_speed: self.console.float("phys_hold_speed"),
                            max_accel: self.console.float("phys_hold_accel"),
                            max_spin: self.console.float("phys_hold_spin"),
                            max_angular_accel: self.console.float("phys_hold_spin_accel"),
                        },
                    );
                } else {
                    self.held_prop = None;
                }
            }

            self.physics
                .sync_and_step(dt, &mut self.entities, &self.vfs);
            self.animations.tick(&mut self.entities, &self.vfs);
        }
        self.update_streaming(dt);

        // Only when a script asked for it: the snapshot a hook reads is
        // O(entities) to build, and most maps define no tick hook at all.
        if self.script.has_function(kerosene_script::hooks::TICK) {
            self.call_script_hook(
                kerosene_script::hooks::TICK,
                vec![rhai::Dynamic::from(dt as f64)],
            );
        }

        self.dispatch_platform_events();
        self.publish_ui_state();

        // Last, once the tick is whole: saved mid-tick, the clock would have
        // moved on while the props were still where the last step left them.
        if let Some(name) = self.pending_save.take()
            && let Err(e) = self.save_game(&name)
        {
            self.console.warn(format!("autosave: {e}"));
        }
    }

    /// Decide what the world around the listener sounds like, and tell the
    /// mixer: the room's reverb, and per voice the air and walls between it
    /// and the ear.
    ///
    /// A forced preset wins over the map, so a designer can audition a hall
    /// without compiling one. A map with no acoustics is dry.
    /// Decide which sections should be resident, and keep the physics
    /// hulls in step with the answer. The host reads the same answer to
    /// build and drop the GPU data.
    pub(super) fn update_streaming(&mut self, dt: f32) {
        let Some(level) = self.level.as_mut() else {
            return;
        };
        if level.streaming.is_static() {
            return;
        }
        let cluster = level.bsp.point_cluster(self.player.movement.origin);
        let keep_alive = self.physics.awake_prop_positions();
        let enabled = self.console.bool("sv_stream");
        let linger = self.console.float("sv_stream_linger").max(0.0);
        if level
            .streaming
            .update(&level.bsp, cluster, &keep_alive, dt, enabled, linger)
        {
            self.physics.sync_sections(&level.bsp, &level.streaming);
        }
    }

    /// The streaming state of the loaded level, for the host.
    pub fn streaming(&self) -> Option<&crate::streaming::Streaming> {
        self.level.as_ref().map(|l| &l.streaming)
    }

    /// The host finished building a section's GPU data.
    pub fn section_loaded(&mut self, section: usize) {
        if let Some(level) = self.level.as_mut()
            && level.streaming.mark_loaded(section)
        {
            self.physics.sync_sections(&level.bsp, &level.streaming);
        }
    }

    pub(super) fn update_acoustics(&mut self) {
        let eye = self.player.movement.eye_position();
        let basis = self.player.view_angles.vectors();
        let reverb_on = self.console.bool("snd_reverb");
        let occlusion_on = self.console.bool("snd_occlusion");
        let air_on = self.console.bool("snd_air");
        let debug = self.console.int("snd_acoustics_debug");

        let mut room = None;
        let params = if !reverb_on {
            ReverbParams::default()
        } else {
            let preset = self.console.string("snd_reverb_preset");
            match ReverbParams::preset(preset) {
                Some(p) => p,
                None => {
                    if !preset.trim().is_empty() {
                        self.audio.warn_once(format!(
                            "snd_reverb_preset `{preset}` is not one of {}",
                            ReverbParams::PRESETS.join(", ")
                        ));
                    }
                    match self
                        .level
                        .as_ref()
                        .and_then(|level| acoustics::surroundings(&level.bsp, eye))
                    {
                        Some(here) => {
                            room = Some(here.room);
                            here.params
                        }
                        None => ReverbParams::default(),
                    }
                }
            }
        };
        if debug >= 1 && room != self.audio.room {
            match (
                room,
                self.level.as_ref().and_then(|l| l.bsp.acoustics.as_ref()),
            ) {
                (Some(index), Some(a)) => {
                    let r = &a.rooms[index as usize];
                    self.console.print(format!(
                        "acoustics: room {index} ({} leaves) rt60 {:.2}/{:.2}/{:.2}/{:.2}s \
                         pre {:.0}ms open {:.2} wet {:.2} path {:.0}",
                        r.leaf_count,
                        r.rt60[0],
                        r.rt60[1],
                        r.rt60[2],
                        r.rt60[3],
                        r.predelay * 1000.0,
                        r.openness,
                        r.wet,
                        r.mean_free_path
                    ));
                }
                _ => self.console.print("acoustics: no room here"),
            }
        }
        self.audio.room = room;
        self.audio.set_reverb(params);

        let wet = if params.enabled { params.wet } else { 0.0 };
        match &self.level {
            Some(level) if occlusion_on => {
                let bsp = &level.bsp;
                self.audio.update_voices(eye, wet, air_on, |source| {
                    match acoustics::reach(bsp, eye, &basis, source) {
                        acoustics::Reach::Clear => Some(0.0),
                        acoustics::Reach::Occluded(o) => Some(o),
                        acoustics::Reach::Unreachable => None,
                    }
                });
            }
            _ => self.audio.update_voices(eye, wet, air_on, |_| Some(0.0)),
        }
    }

    /// Tell every trigger whether the player is inside it, and take the
    /// damage the ones that deal it are dealing.
    pub(super) fn update_triggers(&mut self, dt: f32) {
        let Some(level) = &self.level else { return };
        let hull = self.player.movement.hull();
        let player_box = Aabb::new(
            self.player.movement.origin + hull.mins,
            self.player.movement.origin + hull.maxs,
        );

        let triggers: Vec<(EntityId, usize, Vec3, Angles)> = self
            .entities
            .iter()
            .filter(|e| is_trigger_class(&e.classname))
            .filter_map(|e| e.brush_model.map(|m| (e.id, m, e.origin, e.angles)))
            .collect();

        let player_entity = self.player.entity;
        let mut hurt = 0.0;
        let mut entered: Vec<EntityId> = Vec::new();
        // Asked for once, and only if some trigger wants them.
        let mut prop_boxes: Option<Vec<(EntityId, Aabb)>> = None;
        for (id, model_index, origin, angles) in triggers {
            let Some(model) = level.bsp.models.get(model_index) else {
                continue;
            };
            // Placed the way it is drawn and collided with, so a trigger
            // brush given `angles` fires where it appears to be.
            let moved =
                brush_pose(Some(&level.bsp), model_index, origin, angles).bounds_of(model.bounds());
            // A box overlap is enough: trigger brushes are convex volumes and
            // the exact brush test costs more than it is worth here.
            let inside = moved.intersects(&player_box);

            // Gathered before the touch update, because a `trigger_once`
            // removes itself in there and would otherwise deal nothing on the
            // tick it fired.
            let live = !self.entities.is_disabled(id);
            // The player's own arrival, kept apart from `occupied`, which a
            // prop may have set first: a teleporter a crate is sitting in
            // still takes the player who walks in.
            let was = self.entities.keyvalue_bool(id, "player_inside", false);
            if inside && live {
                hurt += crate::triggers::hurt_per_second(&self.entities, id) * dt;
                if !was {
                    entered.push(id)
                }
            }
            // Left alone while disabled, as `occupied` is, so enabling a
            // trigger around the player sets it off.
            if live && inside != was {
                self.entities
                    .set_keyvalue(id, "player_inside", Value::Bool(inside));
            }
            // A trigger flagged for physics objects (spawnflag 8, as Source
            // numbers it) notices a prop too: its outputs fire as the first
            // thing enters and the last leaves, with the prop as the
            // activator when the player is not the one inside. What it does
            // to the player -- hurt, push, teleport -- stays the player's.
            let wants_props = self
                .entities
                .get(id)
                .is_some_and(|e| e.has_spawnflag(crate::triggers::SF_PHYSICS_OBJECTS));
            let prop = if wants_props && !inside {
                prop_boxes
                    .get_or_insert_with(|| self.physics.prop_boxes())
                    .iter()
                    .find(|(_, b)| b.intersects(&moved))
                    .map(|(prop, _)| *prop)
            } else {
                None
            };
            let activator = if inside { player_entity } else { prop };
            crate::triggers::update_touch(
                &mut self.entities,
                id,
                inside || prop.is_some(),
                activator,
            );
        }

        // Volumes that act on the player when they arrive rather than while
        // they stay. Applied after the touch pass so that a teleport lands
        // the player somewhere the same tick's outputs have already fired
        // from -- the wire and the move belong to the same moment.
        for id in entered {
            self.enter_trigger(id);
        }

        // Applied once, after the loop: two overlapping hurt volumes should
        // cost two lots of damage, but should not be able to kill and respawn
        // the player halfway through a list they are still being iterated
        // against.
        if hurt > 0.0 {
            self.hurt_player(hurt, "hurt");
        }
    }

    /// Act on a trigger the player has just entered.
    pub(super) fn enter_trigger(&mut self, id: EntityId) {
        if let Some((dir, speed)) = crate::triggers::push_of(&self.entities, id) {
            // Added to what the player already had, so running onto a pad
            // carries your speed with you instead of replacing it. Leaving
            // the ground explicitly, or the next tick's ground check would
            // flatten a straight-up launch before it started.
            self.player.movement.velocity += dir * speed;
            self.player.movement.on_ground = false;
        }

        if let Some((map, landmark)) = crate::triggers::changelevel_of(&self.entities, id) {
            self.change_level(&map, landmark.as_deref());
        }

        if let Some(target) = crate::triggers::teleport_target(&self.entities, id) {
            let destination = self
                .entities
                .find_by_name(&target)
                .first()
                .copied()
                .and_then(|to| self.entities.get(to).map(|e| e.origin));
            match destination {
                Some(origin) => {
                    // The view is left alone. Turning the player's head is a
                    // thing the client owns -- angles come from input every
                    // tick, so setting them here would be overwritten before
                    // anyone saw it, and pretending otherwise would be worse
                    // than not doing it.
                    self.player.movement.origin = origin;
                    self.player.previous_origin = origin;
                    self.player.movement.velocity = Vec3::ZERO;
                    self.player.movement.on_ground = false;
                }
                None => self.console.warn(format!(
                    "trigger_teleport points at `{target}`, which is not in this map"
                )),
            }
        }
    }
}
