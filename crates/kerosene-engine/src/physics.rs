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

use std::collections::HashMap;
use kerosene_bsp::{Bsp, contents};
use kerosene_entity::{EntityId, EntityWorld};
use kerosene_math::{Aabb, Angles, ON_EPSILON, Quat, Vec3, Winding};
use kerosene_rigid::{Body, RigidWorld};
use kerosene_vfs::Vfs;

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
}

impl PhysicsProps {
    pub fn new() -> PhysicsProps {
        PhysicsProps {
            rigid: RigidWorld::new(),
            props: HashMap::new(),
            movers: HashMap::new(),
            static_bodies: 0,
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
            if brush.contents & contents::SOLID == 0 { continue; }
            if brush.contents & contents::MOVEABLE != 0 { continue; }
            if brush.contents & (contents::PLAYER_CLIP | contents::MONSTER_CLIP) != 0 { continue; }

            let Some(points) = brush_vertices(bsp, brush) else { continue };
            // Static hulls live in world coordinates already.
            if self.rigid.add_static_hull(&points, Vec3::ZERO, Quat::IDENTITY).is_none() {
                log::debug!("physics: skipped degenerate world brush {i}");
            } else {
                self.static_bodies += 1;
            }
        }

        // Moving brush entities. Each gets a static hull (or several, for a
        // door built from multiple brushes) placed at its current pose, and
        // `sync_and_step` moves the bodies when the entity moves.
        for entity in entities.iter() {
            let Some(model) = entity.brush_model else { continue };
            if model == 0 { continue; }

            let mut bodies = Vec::new();
            for &brush_index in &model_brush_indices(bsp, model) {
                let Some(brush) = bsp.brushes.get(brush_index) else { continue };
                if brush.contents & contents::MOVEABLE == 0 { continue; }
                if brush.contents & contents::SOLID == 0 { continue; }
                let Some(points) = brush_vertices(bsp, brush) else { continue };
                // Brushes are compiled in world coordinates; the body's local
                // space is centred on the pivot so the entity's angles turn
                // the body about the same point the renderer does.
                let pivot = bsp.models.get(model).map(|m| m.bounds().center()).unwrap_or(Vec3::ZERO);
                let local: Vec<Vec3> = points.iter().map(|&p| p - pivot).collect();
                let rotation = Quat::from_mat3(&entity.angles.to_mat3());
                let position = entity.origin + pivot;
                if let Some(body) = self.rigid.add_static_hull(&local, position, rotation) {
                    bodies.push(body);
                }
            }
            if !bodies.is_empty() {
                let pivot = bsp.models.get(model).map(|m| m.bounds().center()).unwrap_or(Vec3::ZERO);
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
            // The body sits at the model's visual centre, so an off-centre
            // model still rests where it draws.
            let body = self.rigid.add_dynamic_box(half_extent, origin + rotation * center, rotation);
            self.props.insert(id, PropBody { body, center, half_extent });
        }

        // Moving brush entities follow their entity's pose, so a door that
        // opened or closed this tick blocks (or stops blocking) immediately.
        for entity in entities.iter() {
            let Some(model) = entity.brush_model else { continue };
            let Some(mover) = self.movers.get(&model) else { continue };
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
    pub fn prop_count(&self) -> usize { self.props.len() }

    /// Number of static world hulls added at map load.
    pub fn static_body_count(&self) -> usize { self.static_bodies }

    /// Number of moving brush entities (doors, shutters) with bodies.
    pub fn mover_count(&self) -> usize { self.movers.len() }

    /// Total bodies in the simulation (static world plus movers plus props).
    pub fn body_count(&self) -> usize { self.rigid.body_count() }

    /// Apply an instantaneous impulse to one prop's centre of mass, in world
    /// space -- a shot, a kick, an explosion.
    pub fn apply_impulse(&mut self, id: EntityId, impulse: Vec3) -> bool {
        match self.props.get(&id) {
            Some(prop) => { self.rigid.apply_impulse(prop.body, impulse); true }
            None => false,
        }
    }

    /// Wake one prop with a small upward nudge, so a `Wake` input is visibly
    /// a reaction rather than nothing.
    pub fn wake(&mut self, id: EntityId) -> bool {
        let Some(prop) = self.props.get(&id) else { return false };
        self.rigid.apply_impulse(prop.body, Vec3::new(0.0, 0.0, 120.0));
        true
    }

    /// Stop one prop dead, as `Sleep` asks.
    pub fn sleep(&mut self, id: EntityId) -> bool {
        let Some(prop) = self.props.get(&id) else { return false };
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
            out.push(aabb);
        }
        out
    }

    /// The nearest prop whose box a ray from `start` along `dir` hits within
    /// `range`, plus its rotation at that moment.
    ///
    /// Used by the pick-up tool: aim at a prop and press use.
    pub fn pick_prop(&self, start: Vec3, dir: Vec3, range: f32) -> Option<(EntityId, Quat)> {
        let end = start + dir * range;
        let mut best: Option<(f32, EntityId)> = None;
        for (&id, prop) in &self.props {
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
                if best.map_or(true, |(bd, _)| distance < bd) {
                    best = Some((distance, id));
                }
            }
        }
        best.map(|(_, id)| (id, self.rigid.body_transform(self.props[&id].body).1))
    }

    /// Hold a prop still at `position` (its model centre) and update its
    /// entity to match, so the renderer and the simulation agree.
    pub fn hold_prop(
        &mut self,
        id: EntityId,
        position: Vec3,
        rotation: Quat,
        entities: &mut EntityWorld,
    ) {
        let Some(prop) = self.props.get(&id) else { return };
        self.rigid.set_body_transform(prop.body, position, rotation);
        self.rigid.set_linear_velocity(prop.body, Vec3::ZERO);
        self.rigid.set_angular_velocity(prop.body, Vec3::ZERO);
        let origin = position - rotation * prop.center;
        if let Some(e) = entities.get_mut(id) {
            e.origin = origin;
            e.angles = Angles::from_quat(rotation);
        }
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
                lines.push(DebugLine { a: corners[a], b: corners[b], color: [0.2, 1.0, 0.3] });
            }
        }
        lines
    }
}

impl Default for PhysicsProps {
    fn default() -> Self { Self::new() }
}

/// Whether a class is a physics prop (a dynamic rigid body).
pub fn is_physics_prop(classname: &str) -> bool {
    classname.eq_ignore_ascii_case("prop_physics")
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
    (0, 1), (0, 2), (0, 4), (1, 3), (1, 5), (2, 3),
    (2, 6), (3, 7), (4, 5), (4, 6), (5, 7), (6, 7),
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
    if planes.len() < 4 { return None; }

    let mut points = Vec::new();
    for (i, plane) in planes.iter().enumerate() {
        let mut w = Winding::base_for_plane(plane);
        for (j, other) in planes.iter().enumerate() {
            if i == j { continue; }
            // Keep the half of the brush we are inside: the other face's
            // plane, flipped to point inward.
            w = w.clipped(&other.flipped(), ON_EPSILON)?;
        }
        w.remove_collinear();
        if w.is_tiny() { continue; }
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
    let Some(m) = bsp.models.get(model) else { return Vec::new() };
    let kerosene_bsp::Child::Leaf(leaf) = kerosene_bsp::decode_child(m.head_node) else {
        return Vec::new();
    };
    let Some(leaf) = bsp.leaves.get(leaf) else { return Vec::new() };
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
        assert!(degree.iter().all(|&d| d == 3), "corners degrees: {degree:?}");
    }
}
