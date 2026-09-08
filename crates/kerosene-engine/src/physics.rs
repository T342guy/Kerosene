// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Rigid-body props, driven by [`kerosene_rigid`].
//!
//! Player movement stays in [`kerosene_physics`] -- that is Source's
//! `gamemovement`, and it is the feel of the game. Everything the player is
//! *not* lives here: physics props, the bodies that tumble, roll and settle.
//! Box3D (through `kerosene-rigid`) simulates them in native Kerosene units,
//! so there is no coordinate conversion anywhere in this file.
//!
//! The world's own brushes become static convex hulls so props have something
//! to land on. Each `prop_physics` entity gets one dynamic body, shaped from
//! its model's bounding box, and every tick the body's pose is written back to
//! the entity so the renderer draws the prop exactly where the simulation put
//! it.

use kerosene_bsp::{Bsp, contents};
use kerosene_entity::{EntityId, EntityWorld};
use kerosene_math::{Aabb, Angles, ON_EPSILON, Quat, Vec3, Winding};
use kerosene_rigid::{Body, RigidWorld};
use kerosene_vfs::Vfs;
use std::collections::HashMap;

/// A dynamic body, plus the model bounds that told it how big to be.
struct PropBody {
    body: Body,
    /// The model's bounds centre, in model space. The box body is centred on
    /// this point, so a model built off-centre still sits where it draws.
    center: Vec3,
    /// Box half-extents in model space.
    half_extent: Vec3,
}

/// One moving brush entity (a door, a shutter, a rotating brush) as a set of
/// static bodies that are teleported to the entity's pose every tick.
struct Mover {
    bodies: Vec<Body>,
    /// The point the entity's angles turn about, in its own space.
    pivot: Vec3,
}

/// The rigid-body simulation and the entities it drives.
pub struct PhysicsProps {
    pub rigid: RigidWorld,
    props: HashMap<EntityId, PropBody>,
    /// Moving brush entities, by BSP model index.
    movers: HashMap<usize, Mover>,
    /// Static world body count, for reporting.
    static_bodies: usize,
    /// The player, as the simulation sees them.
    player: Option<PlayerBody>,
}

/// The player's presence in the rigid world.
///
/// Without one the player was a hole in the simulation: props fell through
/// them, and no amount of walking into a crate moved it, because nothing the
/// player did ever reached the solver. It is kinematic rather than dynamic --
/// the player's own movement code is the authority on where they are, and a
/// dynamic body would be shoved around by the very props it is meant to shove.
struct PlayerBody {
    body: Body,
    /// Half-extents the body was built with. Ducking changes the hull, and a
    /// body cannot be resized, so a change means building a new one.
    half_extent: Vec3,
}

impl PhysicsProps {
    pub fn new() -> PhysicsProps {
        PhysicsProps {
            rigid: RigidWorld::new(),
            props: HashMap::new(),
            movers: HashMap::new(),
            static_bodies: 0,
            player: None,
        }
    }

    /// Add the static world's solid brushes as convex hulls.
    ///
    /// Brush entities are handled in two groups. Detail brushes (`func_detail`)
    /// are static geometry already in `bsp.brushes`, so they become static
    /// hulls like the world's own brushes. Moving brushes (doors, shutters,
    /// rotating brushes -- anything with [`contents::MOVEABLE`]) become static
    /// bodies too, but are re-placed to their entity's pose every tick, so a
    /// closed door blocks a thrown prop and an open one lets it through.
    /// Triggers, water, ladders and the player/monster clip volumes are solid
    /// only to specific actors and are skipped entirely.
    pub fn build_static_world(&mut self, bsp: &Bsp, entities: &EntityWorld) {
        self.static_bodies = 0;
        for (i, brush) in bsp.brushes.iter().enumerate() {
            if brush.contents & contents::SOLID == 0 {
                continue;
            }
            if brush.contents & contents::MOVEABLE != 0 {
                continue;
            }
            if brush.contents & (contents::PLAYER_CLIP | contents::MONSTER_CLIP) != 0 {
                continue;
            }

            let Some(points) = brush_vertices(bsp, brush) else {
                continue;
            };
            // Static hulls live in world coordinates already.
            if self
                .rigid
                .add_static_hull(&points, Vec3::ZERO, Quat::IDENTITY)
                .is_none()
            {
                log::debug!("physics: skipped degenerate world brush {i}");
            } else {
                self.static_bodies += 1;
            }
        }

        // Moving brush entities. Each gets a static hull (or several, for a
        // door built from multiple brushes) placed at its current pose, and
        // `sync_and_step` moves the bodies when the entity moves.
        for entity in entities.iter() {
            let Some(model) = entity.brush_model else {
                continue;
            };
            if model == 0 {
                continue;
            }

            let mut bodies = Vec::new();
            for &brush_index in &model_brush_indices(bsp, model) {
                let Some(brush) = bsp.brushes.get(brush_index) else {
                    continue;
                };
                if brush.contents & contents::MOVEABLE == 0 {
                    continue;
                }
                if brush.contents & contents::SOLID == 0 {
                    continue;
                }
                let Some(points) = brush_vertices(bsp, brush) else {
                    continue;
                };
                // Brushes are compiled in world coordinates; the body's local
                // space is centred on the pivot so the entity's angles turn
                // the body about the same point the renderer does.
                let pivot = bsp
                    .models
                    .get(model)
                    .map(|m| m.bounds().center())
                    .unwrap_or(Vec3::ZERO);
                let local: Vec<Vec3> = points.iter().map(|&p| p - pivot).collect();
                let rotation = Quat::from_mat3(&entity.angles.to_mat3());
                let position = entity.origin + pivot;
                if let Some(body) = self.rigid.add_static_hull(&local, position, rotation) {
                    bodies.push(body);
                }
            }
            if !bodies.is_empty() {
                let pivot = bsp
                    .models
                    .get(model)
                    .map(|m| m.bounds().center())
                    .unwrap_or(Vec3::ZERO);
                self.movers.insert(model, Mover { bodies, pivot });
            }
        }
    }

    /// Create bodies for props that just appeared, drop bodies whose entity is
    /// gone, and write the simulation's pose back to every prop's entity.
    ///
    /// Called once per tick, after entity I/O has run (so a spawner's new
    /// props exist) and before anything draws.
    pub fn sync_and_step(&mut self, dt: f32, entities: &mut EntityWorld, vfs: &Vfs) {
        // Drop bodies for entities that no longer exist.
        let gone: Vec<EntityId> = self
            .props
            .keys()
            .copied()
            .filter(|&id| !entities.exists(id))
            .collect();
        for id in gone {
            if let Some(prop) = self.props.remove(&id) {
                self.rigid.destroy_body(prop.body);
            }
        }

        // Give every physics prop without a body one, shaped from its model.
        // The body's mass, friction and bounciness come from the entity's
        // object properties (`mass`, `friction`, `elasticity`) so a designer
        // can make a heavy crate or a slippery one without touching code.
        let new: Vec<(EntityId, Aabb)> = entities
            .iter()
            .filter(|e| is_physics_prop(&e.classname))
            .filter(|e| !self.props.contains_key(&e.id))
            .filter_map(|e| {
                let name = e.fields.text("model")?;
                let bounds = model_bounds(vfs, name)?;
                Some((e.id, bounds))
            })
            .collect();

        for (id, bounds) in new {
            let (origin, angles) = match entities.get(id) {
                Some(e) => (e.origin, e.angles),
                None => continue,
            };
            let rotation = Quat::from_mat3(&angles.to_mat3());
            let center = bounds.center();
            let half_extent = (bounds.size() * 0.5).max(Vec3::splat(0.5));
            let material = entities
                .get(id)
                .map(|e| prop_material(e, half_extent))
                .unwrap_or_default();
            // The body sits at the model's visual centre, so an off-centre
            // model still rests where it draws.
            let body = self.rigid.add_dynamic_box_material(
                half_extent,
                origin + rotation * center,
                rotation,
                material,
            );
            self.props.insert(
                id,
                PropBody {
                    body,
                    center,
                    half_extent,
                },
            );
        }

        // Moving brush entities follow their entity's pose, so a door that
        // opened or closed this tick blocks (or stops blocking) immediately.
        for entity in entities.iter() {
            let Some(model) = entity.brush_model else {
                continue;
            };
            let Some(mover) = self.movers.get(&model) else {
                continue;
            };
            let rotation = Quat::from_mat3(&entity.angles.to_mat3());
            let position = entity.origin + mover.pivot;
            for &body in &mover.bodies {
                self.rigid.set_body_transform(body, position, rotation);
            }
        }

        // Advance, then push every body's pose back into its entity.
        self.rigid.step(dt);
        for (&id, prop) in &self.props {
            let (position, rotation) = self.rigid.body_transform(prop.body);
            let origin = position - rotation * prop.center;
            let angles = Angles::from_quat(rotation);
            if let Some(e) = entities.get_mut(id) {
                e.origin = origin;
                e.angles = angles;
            }
        }
    }

    /// Number of dynamic prop bodies currently simulated.
    pub fn prop_count(&self) -> usize {
        self.props.len()
    }

    /// Number of static world hulls added at map load.
    pub fn static_body_count(&self) -> usize {
        self.static_bodies
    }

    /// Number of moving brush entities (doors, shutters) with bodies.
    pub fn mover_count(&self) -> usize {
        self.movers.len()
    }

    /// Total bodies in the simulation (static world plus movers plus props).
    pub fn body_count(&self) -> usize {
        self.rigid.body_count()
    }

    /// Apply an instantaneous impulse to one prop's centre of mass, in world
    /// space -- a shot, a kick, an explosion.
    pub fn apply_impulse(&mut self, id: EntityId, impulse: Vec3) -> bool {
        match self.props.get(&id) {
            Some(prop) => {
                self.rigid.apply_impulse(prop.body, impulse);
                true
            }
            None => false,
        }
    }

    /// Wake one prop with a small upward nudge, so a `Wake` input is visibly
    /// a reaction rather than nothing.
    pub fn wake(&mut self, id: EntityId) -> bool {
        let Some(prop) = self.props.get(&id) else {
            return false;
        };
        self.rigid
            .apply_impulse(prop.body, Vec3::new(0.0, 0.0, 120.0));
        true
    }

    /// Stop one prop dead, as `Sleep` asks.
    pub fn sleep(&mut self, id: EntityId) -> bool {
        let Some(prop) = self.props.get(&id) else {
            return false;
        };
        self.rigid.set_linear_velocity(prop.body, Vec3::ZERO);
        self.rigid.set_angular_velocity(prop.body, Vec3::ZERO);
        true
    }

    /// The body handle for an entity, if it is a physics prop.
    pub fn body_of(&self, id: EntityId) -> Option<Body> {
        self.props.get(&id).map(|p| p.body)
    }

    /// World-space axis-aligned boxes of every prop, for player collision.
    ///
    /// A prop can be rotated, so each returned box is the axis-aligned bounds
    /// of its oriented collision box -- a good enough approximation for the
    /// player's hull trace, and exactly what the debug overlay draws.
    pub fn prop_aabbs(&self) -> Vec<Aabb> {
        let mut out = Vec::with_capacity(self.props.len());
        for prop in self.props.values() {
            out.push(prop_aabb(&self.rigid, prop));
        }
        out
    }

    /// The nearest prop whose box a ray from `start` along `dir` hits within
    /// `range`, plus its rotation at that moment.
    ///
    /// Used by the pick-up tool: aim at a prop and press use. A prop whose
    /// `pickable` object property is off is skipped, so a crate a designer
    /// glued down cannot be scooped up.
    pub fn pick_prop(
        &self,
        start: Vec3,
        dir: Vec3,
        range: f32,
        entities: &EntityWorld,
    ) -> Option<(EntityId, Quat)> {
        let end = start + dir * range;
        let mut best: Option<(f32, EntityId)> = None;
        for (&id, prop) in &self.props {
            // A prop marked unpickable is not a candidate, whatever is behind
            // it stays reachable because the ray simply continues past it.
            if entities
                .get(id)
                .is_some_and(|e| !e.fields.bool("pickable", true))
            {
                continue;
            }
            let (position, rotation) = self.rigid.body_transform(prop.body);
            let h = prop.half_extent;
            let mut aabb = Aabb::EMPTY;
            for i in 0..8 {
                let local = Vec3::new(
                    if i & 1 == 0 { -h.x } else { h.x },
                    if i & 2 == 0 { -h.y } else { h.y },
                    if i & 4 == 0 { -h.z } else { h.z },
                );
                aabb.add_point(position + rotation * local);
            }
            if let Some((t, _)) =
                kerosene_physics::sweep_point_vs_box(start, end, aabb.min, aabb.max)
            {
                let distance = t * range;
                if best.is_none_or(|(bd, _)| distance < bd) {
                    best = Some((distance, id));
                }
            }
        }
        best.map(|(_, id)| (id, self.rigid.body_transform(self.props[&id].body).1))
    }

    /// The collision box half-extents of one prop, for placing it without
    /// pushing it through a wall.
    pub fn prop_half_extent(&self, id: EntityId) -> Option<Vec3> {
        self.props.get(&id).map(|p| p.half_extent)
    }

    /// Steer a carried prop towards where the player is holding it.
    ///
    /// A velocity, not a teleport. The prop used to be placed at the hold
    /// point outright, with its velocity zeroed, every tick -- which took it
    /// out of the simulation entirely for as long as it was carried. Nothing
    /// could stop it, so it passed through other props and through anything
    /// the world trace did not catch, and it sat at the hold point as though
    /// welded there: pressing a carried box into a crate moved neither.
    ///
    /// Driving it by velocity leaves it an ordinary dynamic body, so the
    /// solver resolves its contacts like any other: it shoves what it can
    /// move, and what it cannot move stops it, leaving it lagging behind the
    /// hold point until the way is clear. That lag *is* the behaviour -- a
    /// carried object yielding when it meets something solid.
    ///
    /// `max_speed`, `max_accel` and `max_spin` cap how hard it is driven, so a
    /// prop that cannot reach its target leans on the obstacle rather than
    /// detonating against it.
    pub fn steer_prop(
        &mut self,
        id: EntityId,
        position: Vec3,
        rotation: Quat,
        dt: f32,
        limits: HoldLimits,
    ) {
        let HoldLimits {
            max_speed,
            max_accel,
            max_spin,
        } = limits;
        let Some(prop) = self.props.get(&id) else {
            return;
        };
        if dt <= 0.0 {
            return;
        }
        let (current_position, current_rotation) = self.rigid.body_transform(prop.body);

        // The velocity that would close the gap in exactly one tick, capped.
        let mut velocity = (position - current_position) / dt;
        let speed = velocity.length();
        if speed > max_speed {
            velocity *= max_speed / speed;
        }

        // Asked for as an impulse with a ceiling on it, not set outright.
        //
        // This is the whole difference between a hold that collides and one
        // that does not. Assigning the velocity every tick overwrites whatever
        // the contact solver decided last step, so a prop driven at a wall
        // simply burrows into it -- measured at 8 units of centre separation
        // between two 32-unit cubes, which is most of the way through. An
        // impulse is one more force among the contacts, so the solver can
        // refuse it, and the prop stops at the surface and hangs back from the
        // hold point until the way is clear.
        //
        // A ceiling on acceleration rather than on force means a heavy prop is
        // carried as responsively as a light one, which is what a carrying
        // tool is for; the prop's mass still decides every collision it has on
        // the way.
        let mass = self.rigid.mass(prop.body);
        let mut change = velocity - self.rigid.linear_velocity(prop.body);
        let wanted = change.length();
        let ceiling = max_accel * dt;
        if wanted > ceiling {
            change *= ceiling / wanted;
        }

        // The same for the turn: the rotation taking the body where it is to
        // where it should be, as a spin to apply over one tick. Negated when
        // it points the long way round, so a prop turns the short way.
        let mut delta = rotation * current_rotation.inverse();
        if delta.w < 0.0 {
            delta = -delta;
        }
        let (axis, angle) = delta.to_axis_angle();
        let mut spin = axis * (angle / dt);
        let rate = spin.length();
        if rate > max_spin {
            spin *= max_spin / rate;
        }

        // A prop that has settled is asleep, and a sleeping body would ignore
        // being steered.
        self.rigid.set_awake(prop.body, true);
        self.rigid.apply_impulse(prop.body, change * mass);
        // The turn is still assigned rather than asked for: without the body's
        // inertia tensor there is no honest way to turn a wanted change in
        // spin into an angular impulse. It matters far less -- a prop wedged
        // against something is stopped by the contact whatever it is spinning
        // at, and it is the linear drive that was burrowing.
        self.rigid.set_angular_velocity(prop.body, spin);
    }

    /// Put the player into the simulation where they now stand, moving at
    /// `push_velocity`.
    ///
    /// Called every tick, after the player's own movement has run and before
    /// the world is stepped, so props meet the player where the player
    /// actually is.
    ///
    /// `push_velocity` is what the player is *trying* to do, not only what
    /// they managed. The two differ exactly when it matters: props are solid
    /// to the player's hull trace, so walking into a crate stops the player
    /// dead and leaves them with no velocity at all. Handing the solver that
    /// zero would mean leaning on a crate could never move it. Handing it the
    /// attempted velocity shoves the crate at walking pace, and the player
    /// follows it as it goes.
    pub fn sync_player(&mut self, origin: Vec3, mins: Vec3, maxs: Vec3, push_velocity: Vec3) {
        let half_extent = ((maxs - mins) * 0.5).max(Vec3::splat(0.5));
        let center = origin + (mins + maxs) * 0.5;

        // Ducking changes the hull, and a body's shape is fixed once built.
        let rebuild = match &self.player {
            Some(p) => (p.half_extent - half_extent).length() > 0.01,
            None => true,
        };
        if rebuild {
            if let Some(old) = self.player.take() {
                self.rigid.destroy_body(old.body);
            }
            let body = self
                .rigid
                .add_kinematic_box(half_extent, center, Quat::IDENTITY);
            self.player = Some(PlayerBody { body, half_extent });
        }

        let Some(player) = &self.player else { return };
        self.rigid
            .set_body_transform(player.body, center, Quat::IDENTITY);
        self.rigid.set_linear_velocity(player.body, push_velocity);
    }

    /// Shove the props the player is leaning on.
    ///
    /// Being solid is not the same as being able to push. The player's body is
    /// kinematic, which means the solver treats it as infinitely massive: it
    /// would shove a half-tonne crate down a corridor exactly as fast as an
    /// empty carton, which is worse than not pushing at all. So the pushing is
    /// done here instead, as a force a person can exert -- light things move
    /// readily, heavy things barely budge, and the difference is the prop's
    /// own `mass` keyvalue rather than a rule about which props are pushable.
    ///
    /// Capped at `max_speed` because you cannot push something faster than you
    /// can walk: past that the player would be shoved along by their own crate.
    ///
    /// Returns how many props were touched, for the benefit of tests.
    pub fn push_props(
        &mut self,
        player: Aabb,
        direction: Vec3,
        force: f32,
        max_speed: f32,
        dt: f32,
    ) -> usize {
        if direction.length_squared() < 1e-6 || force <= 0.0 {
            return 0;
        }
        // A little slack, because a prop the player is walking into is stopped
        // just short of them by their own hull trace and never quite overlaps.
        let reach = Aabb::new(player.min - Vec3::splat(2.0), player.max + Vec3::splat(2.0));

        let bodies: Vec<Body> = self
            .props
            .values()
            .filter(|prop| reach.intersects(&prop_aabb(&self.rigid, prop)))
            .map(|prop| prop.body)
            .collect();

        for body in &bodies {
            let mass = self.rigid.mass(*body);
            if mass <= 0.0 {
                continue;
            }
            let along = self.rigid.linear_velocity(*body).dot(direction);
            let room = (max_speed - along).max(0.0);
            let gain = (force * dt / mass).min(room);
            if gain > 0.0 {
                self.rigid.apply_impulse(*body, direction * gain * mass);
            }
        }
        bodies.len()
    }

    /// Release a prop at a given world velocity -- a throw, or a plain drop
    /// with whatever the player was already carrying it at.
    ///
    /// A velocity rather than an impulse, because a throw is a throw: the
    /// launch used to be a fixed impulse, which divides by mass, so the same
    /// press sent a light crate across the room and barely nudged a heavy one.
    /// What a player means by "throw this" is a speed.
    pub fn launch_prop(&mut self, id: EntityId, velocity: Vec3) -> bool {
        let Some(prop) = self.props.get(&id) else {
            return false;
        };
        // Explicitly, before the velocity: a carried prop has been held
        // perfectly still and is asleep, and setting a velocity only wakes a
        // body when that velocity is non-zero -- so a prop dropped while
        // standing still would hang in the air.
        self.rigid.set_awake(prop.body, true);
        self.rigid.set_linear_velocity(prop.body, velocity);
        self.rigid.set_angular_velocity(prop.body, Vec3::ZERO);
        true
    }

    /// Wireframe boxes for every prop, for the in-game physics debug view.
    pub fn debug_lines(&self) -> Vec<DebugLine> {
        let mut lines = Vec::with_capacity(self.props.len() * 12);
        for prop in self.props.values() {
            let (position, rotation) = self.rigid.body_transform(prop.body);
            let h = prop.half_extent;
            let mut corners = [Vec3::ZERO; 8];
            for i in 0..8 {
                let local = Vec3::new(
                    if i & 1 == 0 { -h.x } else { h.x },
                    if i & 2 == 0 { -h.y } else { h.y },
                    if i & 4 == 0 { -h.z } else { h.z },
                );
                corners[i] = position + rotation * local;
            }
            for (a, b) in BOX_EDGES {
                lines.push(DebugLine {
                    a: corners[a],
                    b: corners[b],
                    color: [0.2, 1.0, 0.3],
                });
            }
        }
        lines
    }
}

impl Default for PhysicsProps {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether a class is a physics prop (a dynamic rigid body).
pub fn is_physics_prop(classname: &str) -> bool {
    classname.eq_ignore_ascii_case("prop_physics")
}

/// Read the physical material a prop's object properties describe.
///
/// `mass` is kilograms; the prop's box volume turns it into a density for
/// Box3D. `friction` and `elasticity` are the usual 0..1 physical
/// coefficients. Any key left unset falls back to a wood crate.
/// How hard a carried prop may be driven toward the hold point.
///
/// Ceilings, not targets: what the prop collides with on the way is still free
/// to refuse it, which is the point of steering it rather than placing it.
#[derive(Clone, Copy, Debug)]
pub struct HoldLimits {
    /// Fastest it will be moved, in units per second.
    pub max_speed: f32,
    /// Hardest it will be accelerated, in units per second squared. This is
    /// what a contact has to overcome to hold the prop back.
    pub max_accel: f32,
    /// Fastest it will be turned, in radians per second.
    pub max_spin: f32,
}

/// The world-space axis-aligned bounds of one prop's oriented box.
fn prop_aabb(rigid: &RigidWorld, prop: &PropBody) -> Aabb {
    let (position, rotation) = rigid.body_transform(prop.body);
    let h = prop.half_extent;
    let mut aabb = Aabb::EMPTY;
    for i in 0..8 {
        let local = Vec3::new(
            if i & 1 == 0 { -h.x } else { h.x },
            if i & 2 == 0 { -h.y } else { h.y },
            if i & 4 == 0 { -h.z } else { h.z },
        );
        aabb.add_point(position + rotation * local);
    }
    aabb
}

fn prop_material(e: &kerosene_entity::Entity, half_extent: Vec3) -> kerosene_rigid::BodyMaterial {
    let mass_kg = e.fields.f32("mass", -1.0);
    let friction = e.fields.f32("friction", 0.8);
    let restitution = e.fields.f32("elasticity", 0.1);
    let mut material = kerosene_rigid::BodyMaterial {
        density: kerosene_rigid::BodyMaterial::wood().density,
        friction,
        restitution,
    };
    if mass_kg > 0.0 {
        let volume = half_extent.x * half_extent.y * half_extent.z * 8.0;
        material.density = kerosene_rigid::BodyMaterial::density_for_mass(mass_kg, volume);
    }
    material
}

/// A wireframe segment for the debug overlay.
#[derive(Clone, Copy, Debug)]
pub struct DebugLine {
    pub a: Vec3,
    pub b: Vec3,
    pub color: [f32; 3],
}

/// The 12 edges of a box, as corner index pairs.
const BOX_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (0, 2),
    (0, 4),
    (1, 3),
    (1, 5),
    (2, 3),
    (2, 6),
    (3, 7),
    (4, 5),
    (4, 6),
    (5, 7),
    (6, 7),
];

/// The unique vertices of one BSP brush, computed by clipping each face's base
/// winding against every other face. Returns `None` for a degenerate brush.
fn brush_vertices(bsp: &Bsp, brush: &kerosene_bsp::Brush) -> Option<Vec<Vec3>> {
    let mut planes = Vec::with_capacity(brush.num_sides as usize);
    for i in 0..brush.num_sides as usize {
        let side = bsp.brushsides.get(brush.first_side as usize + i)?;
        let plane = bsp.planes.get(side.plane as usize)?.to_plane();
        planes.push(plane);
    }
    if planes.len() < 4 {
        return None;
    }

    let mut points = Vec::new();
    for (i, plane) in planes.iter().enumerate() {
        let mut w = Winding::base_for_plane(plane);
        for (j, other) in planes.iter().enumerate() {
            if i == j {
                continue;
            }
            // Keep the half of the brush we are inside: the other face's
            // plane, flipped to point inward.
            w = w.clipped(&other.flipped(), ON_EPSILON)?;
        }
        w.remove_collinear();
        if w.is_tiny() {
            continue;
        }
        points.extend(w.points);
    }

    let mut unique: Vec<Vec3> = Vec::new();
    for p in points {
        if !unique.iter().any(|&q| (q - p).length_squared() < 0.01) {
            unique.push(p);
        }
    }
    (unique.len() >= 4).then_some(unique)
}

/// The brush indices belonging to one BSP model (0 = world, 1.. = brush
/// entities). A brush model's head node is a single leaf whose leafbrushes
/// reference exactly its brushes.
fn model_brush_indices(bsp: &Bsp, model: usize) -> Vec<usize> {
    let Some(m) = bsp.models.get(model) else {
        return Vec::new();
    };
    let kerosene_bsp::Child::Leaf(leaf) = kerosene_bsp::decode_child(m.head_node) else {
        return Vec::new();
    };
    let Some(leaf) = bsp.leaves.get(leaf) else {
        return Vec::new();
    };
    let first = leaf.first_leafbrush as usize;
    let count = leaf.num_leafbrushes as usize;
    (first..first + count)
        .filter_map(|i| bsp.leafbrushes.get(i).map(|&bi| bi as usize))
        .collect()
}

/// The bounding box of a `.keromdl` model, by the name an entity refers to it.
fn model_bounds(vfs: &Vfs, name: &str) -> Option<Aabb> {
    let path = kerosene_asset::model_path(name);
    let bytes = vfs.read(&path).ok()?;
    let model = kerosene_asset::Model::from_bytes(&bytes).ok()?;
    Some(model.bounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kerosene_entity::Entity;

    #[test]
    fn physics_props_are_recognised_by_class() {
        assert!(is_physics_prop("prop_physics"));
        assert!(is_physics_prop("PROP_PHYSICS"));
        assert!(!is_physics_prop("prop_static"));
        assert!(!is_physics_prop("prop_dynamic_spawner"));
    }

    #[test]
    fn box_edges_cover_every_corner() {
        // Every corner appears in exactly three edges (a box corner).
        let mut degree = [0usize; 8];
        for (a, b) in BOX_EDGES {
            degree[a] += 1;
            degree[b] += 1;
        }
        assert!(
            degree.iter().all(|&d| d == 3),
            "corners degrees: {degree:?}"
        );
    }

    #[test]
    fn object_properties_become_the_body_material() {
        // A designer writes `mass`, `friction` and `elasticity`; the engine
        // turns them into the rigid body's material.
        let mut e = Entity {
            id: kerosene_entity::EntityId {
                index: 0,
                generation: 0,
            },
            classname: "prop_physics".into(),
            fields: kerosene_entity::Fields::new(),
            origin: Vec3::ZERO,
            angles: Angles::ZERO,
            connections: Vec::new(),
            next_think: None,
            brush_model: None,
            pending_removal: false,
        };
        let half = Vec3::new(8.0, 8.0, 8.0);
        let volume = half.x * half.y * half.z * 8.0;

        // Unset keys fall back to wood.
        let wood = prop_material(&e, half);
        assert_eq!(wood.friction, 0.8);
        assert_eq!(wood.restitution, 0.1);

        e.fields.set("friction", kerosene_entity::Value::Float(0.2));
        e.fields
            .set("elasticity", kerosene_entity::Value::Float(0.9));
        let slippery = prop_material(&e, half);
        assert_eq!(slippery.friction, 0.2);
        assert_eq!(slippery.restitution, 0.9);

        // A set mass turns into the density that makes that total mass.
        e.fields.set("mass", kerosene_entity::Value::Float(64.0));
        let heavy = prop_material(&e, half);
        assert!((heavy.density - 64.0 / volume).abs() < 1e-6);

        // Zero or negative mass means "derive it", not "weightless".
        e.fields.set("mass", kerosene_entity::Value::Float(0.0));
        let derived = prop_material(&e, half);
        assert_eq!(
            derived.density,
            kerosene_rigid::BodyMaterial::wood().density
        );
    }
}
