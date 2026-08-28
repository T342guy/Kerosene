// SPDX-License-Identifier: LGPL-3.0-or-later
//! Primitives that are not boxes.
//!
//! A brush is a convex solid, and no convex solid is curved. That is not a
//! limitation to be worked around -- it is what makes the BSP compiler, the
//! collision system and the lighting all tractable -- but it does mean the
//! answer to "I want an archway" cannot be one brush. It is *several*,
//! arranged so that together they read as a curve.
//!
//! Which is a fiddly thing to do by hand: eight brushes at 22.5 degrees to
//! each other, each one a slightly different trapezoid, all meeting exactly.
//! Doing it by dragging boxes is how people give up on curves. So the editor
//! generates them, from a box drawn the same way any other brush is drawn and
//! a couple of numbers.
//!
//! Everything here is a pure function of a bounding box: no document, no
//! viewport, no undo. What a cylinder *is* should be answerable in a test.

use void_map::Solid;
use void_math::{Aabb, Vec3};

/// What the shape tool draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Shape {
    /// A box with one edge collapsed: a ramp.
    #[default]
    Wedge,
    /// An n-sided prism. One brush however many sides it has, because a
    /// convex polygon swept along a line is still convex.
    Cylinder,
    /// An n-sided base drawn to a point.
    Cone,
    /// A ring, or a slice of one, as a fan of brushes.
    Arch,
    /// A staircase of solid steps.
    Stairs,
}

impl Shape {
    pub fn all() -> [Shape; 5] {
        [Shape::Wedge, Shape::Cylinder, Shape::Cone, Shape::Arch, Shape::Stairs]
    }

    pub fn label(self) -> &'static str {
        match self {
            Shape::Wedge => "wedge",
            Shape::Cylinder => "cylinder",
            Shape::Cone => "cone",
            Shape::Arch => "arch",
            Shape::Stairs => "stairs",
        }
    }

    /// What the shape is for, in one line, for a tooltip.
    pub fn help(self) -> &'static str {
        match self {
            Shape::Wedge => "A ramp: a box with one edge collapsed. One brush.",
            Shape::Cylinder => "A pillar or a pipe. One brush, however many sides.",
            Shape::Cone => "A spike or a pyramid. One brush.",
            Shape::Arch => "A doorway or a tunnel: a fan of brushes around a curve.",
            Shape::Stairs => "Solid steps, one brush each.",
        }
    }

    /// Whether the number of sides means anything for this shape.
    pub fn uses_sides(self) -> bool {
        !matches!(self, Shape::Wedge)
    }

    /// Whether the wall thickness means anything for this shape.
    pub fn uses_wall(self) -> bool {
        matches!(self, Shape::Arch)
    }

    /// Whether the arc means anything for this shape.
    pub fn uses_arc(self) -> bool {
        matches!(self, Shape::Arch)
    }
}

/// The knobs on the shape tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// Segments around a curve, or steps in a staircase.
    pub sides: u32,
    /// How much of a full turn an arch covers, in degrees.
    pub arc: f32,
    /// How thick an arch's wall is, in void units.
    pub wall: f32,
}

impl Default for Options {
    fn default() -> Self {
        // Eight sides is Hammer's default and a good one: enough that a
        // pillar reads as round at arm's length, few enough that a room full
        // of them still compiles quickly.
        Options { sides: 8, arc: 180.0, wall: 32.0 }
    }
}

/// The narrowest and widest anything here will go.
pub const MIN_SIDES: u32 = 3;
pub const MAX_SIDES: u32 = 64;

impl Options {
    /// The options, forced into the range the generators can honour.
    pub fn sane(self) -> Options {
        Options {
            sides: self.sides.clamp(MIN_SIDES, MAX_SIDES),
            arc: self.arc.clamp(1.0, 360.0),
            wall: self.wall.max(1.0),
        }
    }
}

/// Build a shape inside a box.
///
/// `axis` is the one the box was drawn *through* -- the depth axis of the
/// view it was drawn in -- so a cylinder drawn in the top view stands up and
/// one drawn in the front view lies on its side. That is the behaviour that
/// makes the tool usable without a separate orientation control.
///
/// An empty result means the box was too small or too flat to hold the shape;
/// the caller should say so rather than silently doing nothing.
pub fn build(shape: Shape, bounds: Aabb, axis: usize, options: Options, material: &str) -> Vec<Solid> {
    let options = options.sane();
    let size = bounds.size();
    if size.x <= 0.0 || size.y <= 0.0 || size.z <= 0.0 { return Vec::new() }

    match shape {
        Shape::Wedge => wedge(bounds, axis, material),
        Shape::Cylinder => cylinder(bounds, axis, options, material),
        Shape::Cone => cone(bounds, axis, options, material),
        Shape::Arch => arch(bounds, axis, options, material),
        Shape::Stairs => stairs(bounds, axis, options, material),
    }
}

/// The two axes a shape's cross-section lives in, given the one it is swept
/// along. In ascending order, so the pair is the same whichever way it is
/// asked for.
fn cross_axes(axis: usize) -> (usize, usize) {
    match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// A point in the cross-section plane, at a given height along the axis.
fn at(axis: usize, u: f32, v: f32, height: f32) -> Vec3 {
    let (a, b) = cross_axes(axis);
    let mut p = Vec3::ZERO;
    p[a] = u;
    p[b] = v;
    p[axis] = height;
    p
}

/// A ramp: the box with one of its top edges pulled down to the floor.
///
/// The slope is in the vertical plane -- the one containing the axis the box
/// was drawn *through* -- because that is what makes it a ramp rather than a
/// triangular wall standing on end. It rises along the first cross-section
/// axis and is full width along the other, so a wedge drawn in the top view
/// climbs along x across the whole of its y.
///
/// Which way it faces is settled by turning it, not by offering four
/// near-identical entries in a menu.
fn wedge(bounds: Aabb, axis: usize, material: &str) -> Vec<Solid> {
    let (rise, width) = cross_axes(axis);
    let (lo, hi) = (bounds.min, bounds.max);

    // A triangle in the (rise, height) plane: floor at the low end, full
    // height at the high one.
    let corner = |along: f32, height: f32| {
        let mut p = Vec3::ZERO;
        p[rise] = along;
        p[axis] = height;
        p[width] = lo[width];
        p
    };
    let profile = vec![
        corner(lo[rise], lo[axis]),
        corner(hi[rise], lo[axis]),
        corner(hi[rise], hi[axis]),
    ];
    // Swept sideways, so the ramp is as wide as the box was drawn.
    Solid::prism(&profile, width, lo[width], hi[width], material)
        .into_iter()
        .collect()
}

/// The cross-section of a cylinder or cone: a regular polygon inscribed in
/// the box, so the shape fills the space that was drawn for it.
fn ring(bounds: Aabb, axis: usize, sides: u32, height: f32) -> Vec<Vec3> {
    let (a, b) = cross_axes(axis);
    let centre = bounds.center();
    let (ra, rb) = (bounds.size()[a] * 0.5, bounds.size()[b] * 0.5);

    (0..sides)
        .map(|i| {
            // Started half a segment round, so an even-sided cylinder has
            // flat faces square to the world rather than corners poking out
            // of the box it was drawn in.
            let angle = std::f32::consts::TAU * (i as f32 + 0.5) / sides as f32;
            at(axis, centre[a] + angle.cos() * ra, centre[b] + angle.sin() * rb, height)
        })
        .collect()
}

fn cylinder(bounds: Aabb, axis: usize, options: Options, material: &str) -> Vec<Solid> {
    let profile = ring(bounds, axis, options.sides, bounds.min[axis]);
    Solid::prism(&profile, axis, bounds.min[axis], bounds.max[axis], material)
        .into_iter()
        .collect()
}

fn cone(bounds: Aabb, axis: usize, options: Options, material: &str) -> Vec<Solid> {
    let (a, b) = cross_axes(axis);
    let base = ring(bounds, axis, options.sides, bounds.min[axis]);
    let centre = bounds.center();
    let apex = at(axis, centre[a], centre[b], bounds.max[axis]);
    Solid::pyramid(&base, axis, bounds.min[axis], apex, material)
        .into_iter()
        .collect()
}

/// A ring, or a slice of one, as a fan of brushes.
///
/// This is the shape the whole module exists for: a doorway, a tunnel mouth,
/// a round window. Each segment is a four-sided prism between the inner and
/// outer radius over one slice of the arc, and the segments share their
/// corner points exactly, so the seams between them are seams and not cracks.
fn arch(bounds: Aabb, axis: usize, options: Options, material: &str) -> Vec<Solid> {
    let (a, b) = cross_axes(axis);
    let centre = bounds.center();
    let (ra, rb) = (bounds.size()[a] * 0.5, bounds.size()[b] * 0.5);

    // A wall thicker than the arch is not a thinner arch, it is a solid disc
    // -- and one the generator would build inside out. Held to just under the
    // radius so there is always a hole.
    let wall = options.wall.min(ra.min(rb) * 0.95);
    if wall <= 0.0 { return Vec::new() }

    let sweep = options.arc.to_radians();
    let step = sweep / options.sides as f32;
    // Swept from one side over the top to the other, so a half arch is the
    // upper half. Starting at the far side instead would sweep underneath and
    // give a bowl, which is a thing but is not what anybody types "arch"
    // expecting: an arch is the bit you walk under.
    let start = 0.0;

    let mut solids = Vec::new();
    for i in 0..options.sides {
        let (from, to) = (start + step * i as f32, start + step * (i + 1) as f32);

        // The outer corners are on the box; the inner ones are pulled in
        // along the same directions, so the wall has an even thickness all
        // the way round rather than only at the ends.
        let corner = |angle: f32, inset: f32| {
            at(
                axis,
                centre[a] + angle.cos() * (ra - inset),
                centre[b] + angle.sin() * (rb - inset),
                bounds.min[axis],
            )
        };

        let profile = vec![
            corner(from, 0.0),
            corner(to, 0.0),
            corner(to, wall),
            corner(from, wall),
        ];
        if let Some(solid) = Solid::prism(&profile, axis, bounds.min[axis], bounds.max[axis], material) {
            solids.push(solid);
        }
    }
    solids
}

/// Solid steps filling the box, rising along the sweep axis.
///
/// Solid rather than hollow: a staircase of thin treads is a staircase the
/// player falls through the moment the physics tick lands between two of
/// them.
fn stairs(bounds: Aabb, axis: usize, options: Options, material: &str) -> Vec<Solid> {
    let (a, b) = cross_axes(axis);
    let steps = options.sides;
    let rise = bounds.size()[axis] / steps as f32;
    // Along the longer of the two cross-section axes, which is the direction
    // a staircase drawn as a long thin box obviously goes.
    let (run_axis, wide_axis) = if bounds.size()[a] >= bounds.size()[b] { (a, b) } else { (b, a) };
    let run = bounds.size()[run_axis] / steps as f32;
    if rise <= 0.0 || run <= 0.0 { return Vec::new() }

    let mut solids = Vec::new();
    for i in 0..steps {
        let mut min = bounds.min;
        let mut max = bounds.max;
        // Each step reaches the floor rather than floating: a stack of boxes
        // is a staircase, a stack of slabs is a set of shelves.
        max[axis] = bounds.min[axis] + rise * (i + 1) as f32;
        min[run_axis] = bounds.min[run_axis] + run * i as f32;
        max[run_axis] = min[run_axis] + run;
        min[wide_axis] = bounds.min[wide_axis];
        max[wide_axis] = bounds.max[wide_axis];

        solids.push(Solid::cube(Aabb::new(min, max), material));
    }
    solids
}

#[cfg(test)]
mod tests;
