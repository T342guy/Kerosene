// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Loading a map into the engine, spawning the player into it, and asking
//! for the next one.

use super::*;

impl Engine {
    /// Where each brush entity's model has moved to, as `(model, offset)`.
    ///
    /// The renderer and the collision system both need this, and they must
    /// agree: a door drawn where it is not is worse than a door that does not
    /// move at all, because the second is obvious. Model 0 is the static
    /// world and never appears here.
    pub fn brush_model_poses(&self) -> Vec<(usize, Pose)> {
        let level = self.level.as_ref();
        self.entities
            .iter()
            .filter_map(|e| {
                let model = e.brush_model?;
                // A disabled `brush` or `wall_toggle` is not
                // there to see, as it is not there to walk into.
                if model == 0 || self.entities.is_disabled(e.id) {
                    return None;
                }
                Some((
                    model,
                    brush_pose(level.map(|l| &*l.bsp), model, e.origin, e.angles),
                ))
            })
            .collect()
    }

    /// [`Self::brush_model_poses`], blended `alpha` of the way from where each
    /// brush model stood at the start of this tick to where it stands now.
    ///
    /// Brush movers (doors, rotating brushes) only update on their own think
    /// schedule, coarser than the render frame rate, so drawing the raw
    /// current pose makes them visibly snap into a new position each time
    /// they think. This mirrors [`Self::interpolated_eye`]'s treatment of the
    /// camera, extended to brush geometry.
    pub fn interpolated_brush_model_poses(&self, alpha: f32) -> Vec<(usize, Pose)> {
        let level = self.level.as_ref();
        let alpha = alpha.clamp(0.0, 1.0);
        self.entities
            .iter()
            .filter_map(|e| {
                let model = e.brush_model?;
                if model == 0 || self.entities.is_disabled(e.id) {
                    return None;
                }
                let (prev_origin, prev_angles) = self
                    .previous_brush_poses
                    .get(&model)
                    .copied()
                    .unwrap_or((e.origin, e.angles));
                let origin = prev_origin.lerp(e.origin, alpha);
                let angles = prev_angles.slerp(e.angles, alpha);
                Some((
                    model,
                    brush_pose(level.map(|l| &*l.bsp), model, origin, angles),
                ))
            })
            .collect()
    }

    /// Load a map by name, e.g. `kero_start`.
    pub fn load_map(&mut self, name: &str) -> anyhow::Result<()> {
        self.load_level(name, None)
    }

    /// Load a map, fresh from its file or as a saved game left it.
    ///
    /// One path for both, so a restored level is built exactly the way a
    /// fresh one is -- geometry, physics, streaming, the UI's reset -- and
    /// differs only where the save has something to say: which entities
    /// exist, where the player is, what the scripts remember.
    pub(crate) fn load_level(
        &mut self,
        name: &str,
        save: Option<&crate::save::SaveGame>,
    ) -> anyhow::Result<()> {
        let path = format!("maps/{name}.kbsp");
        let bytes = match self.vfs.read(&path) {
            Ok(bytes) => bytes,
            Err(e) => anyhow::bail!("{}", explain_missing_map(&self.vfs, name, &e)),
        };
        let bsp = Bsp::from_bytes(&bytes, &path)?;

        self.console.print(format!("loading {path}"));
        for (name, count) in bsp.stats() {
            self.console.developer(format!("  {name}: {count}"));
        }
        if bsp.visibility.is_empty() {
            self.console
                .warn("this map has no visibility data; run Umbra on it");
        }
        if bsp.lighting.is_empty() {
            self.console
                .warn("this map has no lighting; run Radiance on it");
        }
        if bsp.acoustics.is_none() {
            self.console
                .warn("this map has no acoustics; run Resonance on it");
        }

        // The game's last look at the old map, now that the new one is known
        // to load.
        if self.level.is_some() {
            self.with_game_mut(|game, engine| game.map_unloading(engine));
        }

        // A fresh entity world per map: nothing from the last one should
        // survive, and a stale handle must not resolve.
        // t3; this part appears to carefully reset and load map data.
        match save {
            None => {
                self.entities = EntityWorld::new(self.registry.clone());
                self.entities.set_trace(self.console.int("developer") >= 2);
                let lump = bsp.entities_kv().map_err(SpawnError::from)?;
                let count = self.entities.load_from_lump(&lump, &bsp.model_bounds())?;
                self.console.print(format!("{count} entities"));
            }
            Some(save) => {
                // Into a world of its own first: a save that will not go
                // back must leave the level that is running untouched.
                let mut world = EntityWorld::new(self.registry.clone());
                world.set_trace(self.console.int("developer") >= 2);
                let count = world
                    .restore(&save.world)
                    .map_err(|e| anyhow::anyhow!("the save's entities would not restore: {e}"))?;
                self.entities = world;
                self.console.print(format!("{count} entities restored"));
            }
        }

        // Static world geometry, so rigid-body props have something to land
        // on. Built before `bsp` moves into `level`.
        self.physics = PhysicsProps::new(); // t3; this wipes the phys entity data. But brushes were not wiped
        // Models may have been recompiled since the last map.
        self.animations = crate::animation::Animations::new();
        self.previous_brush_poses.clear(); // t3; Now they get cleared. Fixes a bug.
        // Shakes and punches are timed by the game clock, which the new map
        // (or the save) sets back: left alone, a shake started late in the
        // last map would last until this one caught up with it.
        self.view = Default::default();
        self.entity_voices.clear();

        // Only the world section's hulls now; the streamed sections' come
        // and go with them.
        self.physics.build_static_world(&bsp, &self.entities);
        self.console.developer(format!(
            "  physics: {} static hulls, {} movers",
            self.physics.static_body_count(),
            self.physics.mover_count()
        ));

        let sky_color = self.sky_color_from_map();
        let streaming = crate::streaming::Streaming::new(&bsp);
        if !streaming.is_static() {
            self.console.print(format!(
                "{} streamed sections: {}",
                bsp.section_count() - 1,
                bsp.sections
                    .iter()
                    .skip(1)
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let nav = self.load_nav(name);
        self.level = Some(Level {
            name: name.to_string(),
            bsp: std::sync::Arc::new(bsp),
            streaming,
            sky_color,
            nav,
        });
        self.load_generation += 1;
        self.resources.collect();
        // Before the entities' spawn requests are taken: an `infodecal`
        // places its decal there, and clearing the old map's after would
        // take the new one with them.
        self.ui_map_loaded();
        self.accumulator = 0.0;
        // Whatever the last level was playing is not playing any more.
        self.audio.stop_all();

        let Some(save) = save else {
            // Seeded by the map, so a fresh start of a map rolls the same way
            // each time, which a bug report can then reproduce.
            self.rng = kerosene_math::Rng::new(kerosene_math::Rng::seed_from(name));
            self.spawn_player();
            self.time = 0.0;
            self.tick_count = 0;
            // A map's script loads after every entity exists, so
            // `on_map_start` can find them. A map without one is the normal
            // case and is silent.
            self.load_map_script(name);
            // ...and any `logic_script` that named a file of its own got its
            // request in during spawn.
            self.take_entity_requests();
            self.call_script_hook(kerosene_script::hooks::MAP_START, vec![]);
            self.with_game_mut(|game, engine| game.map_loaded(engine));
            return Ok(());
        };

        self.time = save.time;
        self.tick_count = save.tick;
        self.restore_from(save);
        Ok(())
    }

    /// The sun's colour, which is also what the sky is tinted.
    ///
    /// Radiance reads the same `_light` value to bake the lighting, so a map
    /// lit by a warm sun gets a warm sky without anyone stating it twice.
    /// White when a map has no `light_environment`, since a tint of nothing
    /// is the same as no tint.
    pub(super) fn sky_color_from_map(&self) -> Vec3 {
        let Some(id) = self
            .entities
            .find_by_class("light_environment")
            .first()
            .copied()
        else {
            return Vec3::ONE;
        };
        let Some(entity) = self.entities.get(id) else {
            return Vec3::ONE;
        };
        // "r g b brightness"; the brightness belongs to the lighting compile
        // and would blow the sky out if it were applied here as well.
        let raw = entity.fields.text("_light").unwrap_or_default();
        let numbers: Vec<f32> = raw
            .split_whitespace()
            .take(3)
            .filter_map(|n| n.parse::<f32>().ok())
            .collect();
        if numbers.len() < 3 {
            return Vec3::ONE;
        }
        Vec3::new(numbers[0], numbers[1], numbers[2]) / 255.0
    }

    /// Put the player at an `info_player_start`, or somewhere sane if there is
    /// none.
    pub(super) fn spawn_player(&mut self) {
        let spawn = self
            .entities
            .find_by_class("info_player_start")
            .first()
            .copied()
            .and_then(|id| self.entities.get(id).map(|e| (e.origin, e.angles)));

        let (origin, angles) = match spawn {
            Some(found) => found,
            None => {
                self.console
                    .warn("no info_player_start; spawning at the world origin");
                (Vec3::ZERO, Angles::ZERO)
            }
        };

        // Dying is letting go. Without this the hold outlives its holder: the
        // next tick steers the prop from wherever it was toward a hold point
        // in front of the spawn, dragging it across the level.
        if let Some(held) = self.held_prop.take() {
            self.physics.launch_prop(held.id, Vec3::ZERO);
        }

        // The old body goes before the new one arrives: a respawn that
        // spawned a second `player` entity left the first behind, at the
        // place of death, for `find_by_class` and scripts to trip over.
        if let Some(old) = self.player.entity.take() {
            self.entities.remove(old);
        }
        let player = self.entities.spawn("player");
        self.entities.player = Some(player);
        if let Some(e) = self.entities.get_mut(player) {
            e.origin = origin;
        }

        self.player = PlayerState {
            entity: Some(player),
            // Lifted slightly so the first ground trace has somewhere to land
            // rather than starting flush with the floor.
            movement: MoveState {
                origin: origin + Vec3::Z,
                ..Default::default()
            },
            view_angles: angles.clamped_view(),
            previous_origin: origin,
            health: self.max_health,
            // Carried across a respawn rather than cleared: a player who died
            // with the use key held should have to let go and press again,
            // not immediately use whatever they spawn facing. The same goes
            // for a held attack button.
            use_held: self.player.use_held,
            attack_held: self.player.attack_held,
            step_distance: 0.0,
            step_index: self.player.step_index,
        };
        self.view_forced = true;
        self.with_game_mut(|game, engine| game.player_spawned(engine));
    }

    /// Put the player back at the map's start, alive and at full health:
    /// what the engine does on death unless [`Game::player_died`] took it
    /// over, for a game that did and has finished its death screen.
    ///
    /// [`Game::player_died`]: crate::Game::player_died
    pub fn respawn_player(&mut self) {
        if self.level.is_some() {
            self.spawn_player();
        }
    }

    /// Move the player to `origin`, standing still, and face them along
    /// `angles` if given: `setpos`, `point_teleport`, a reloaded map. The
    /// move is not traced; somewhere solid is the caller's mistake to make.
    pub fn teleport_player(&mut self, origin: Vec3, angles: Option<Angles>) {
        self.player.movement.origin = origin;
        self.player.previous_origin = origin;
        self.player.movement.velocity = Vec3::ZERO;
        self.player.movement.on_ground = false;
        if let Some(e) = self.player.entity.and_then(|id| self.entities.get_mut(id)) {
            e.origin = origin;
        }
        if let Some(angles) = angles {
            self.set_view_angles(angles);
        }
    }

    /// Whether the player has health left. A game that handles
    /// [`Game::player_died`] itself leaves the
    /// player dead until it calls [`respawn_player`](Engine::respawn_player)
    /// or loads a save; meanwhile they neither move nor use anything.
    pub fn player_alive(&self) -> bool {
        self.player.health > 0.0
    }

    /// The health the player spawns with; 100 unless the game says.
    pub fn player_max_health(&self) -> f32 {
        self.max_health
    }

    /// Set the health the player spawns with. The player's current health is
    /// capped to it, and is otherwise left alone.
    pub fn set_player_max_health(&mut self, max: f32) {
        self.max_health = max.max(1.0);
        self.player.health = self.player.health.min(self.max_health);
    }

    /// Queue a map change for the start of the next frame.
    ///
    /// Deferred rather than immediate because a map change unloads everything
    /// the current tick is standing on.
    pub fn request_map(&mut self, name: &str) {
        self.pending_map = Some(name.to_string());
        self.pending_change = None;
    }

    pub fn has_pending_map(&self) -> bool {
        self.pending_map.is_some() || self.pending_change.is_some()
    }

    /// Take the requested map without loading it, for a host that wants to
    /// handle the error itself.
    pub fn take_pending_map(&mut self) -> Option<String> {
        self.pending_map.take()
    }

    /// Load the map the console asked for, if it asked for one.
    ///
    /// `frame` does this on its own; a host that drives `tick` directly --
    /// the headless runner -- calls it between ticks, or `map` typed from a
    /// script or a `+map` on the command line is set and never read.
    pub fn load_pending_map(&mut self) {
        if let Some(map) = self.pending_map.take()
            && let Err(e) = self.load_map(&map)
        {
            self.load_failed(format!("{e}"));
        }
        if let Some(change) = self.pending_change.take() {
            self.make_change(change);
        }
        self.loading_done();
    }
}
