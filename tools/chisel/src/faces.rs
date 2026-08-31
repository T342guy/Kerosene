// SPDX-License-Identifier: MPL-2.0
//! Editing how a texture sits on a face.
//!
//! Once the 3D pane draws textures, the next thing anyone wants is to move
//! one: a doorframe that lines up with its opening, a floor whose grid starts
//! at a corner, a sign that is not sideways. All of it is arithmetic on the
//! face's two texture axes, and none of it needs a window -- so it is here,
//! and it is tested here.
//!
//! The operations are the ones Hammer's face editor has, because they are the
//! ones the job actually needs:
//!
//! * **Scale** -- world units per texel. Larger stretches the texture.
//! * **Shift** -- offset in texels, for lining a pattern up with geometry.
//! * **Rotate** -- turn the axes within the face's own plane.
//! * **Fit** -- make the texture span the face exactly once.
//! * **Align to world** -- the default projection, so adjacent faces of a wall
//!   share a continuous texture.
//! * **Align to face** -- axes in the face's own plane, so a texture on a
//!   sloped surface is not foreshortened.
//! * **Justify** -- push the texture to an edge or the middle of the face.

use kerosene_map::{Side, Solid, TextureAxis};
use kerosene_math::{Plane, Vec3, Winding};

/// Which edge to push a texture towards.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Justify {
    Left,
    Right,
    Top,
    Bottom,
    Centre,
    /// Scale to span the face exactly once, and sit at its corner.
    Fit,
}

impl Justify {
    pub fn label(self) -> &'static str {
        match self {
            Justify::Left => "left",
            Justify::Right => "right",
            Justify::Top => "top",
            Justify::Bottom => "bottom",
            Justify::Centre => "centre",
            Justify::Fit => "fit",
        }
    }
}

/// The texel bounds a face covers with its current axes.
///
/// Returned in texels rather than normalised, because the texture's size is
/// not known here and every operation below works in texels anyway.
pub fn texel_bounds(side: &Side, winding: &Winding) -> Option<((f32, f32), (f32, f32))> {
    if winding.points.is_empty() { return None }
    let mut min = (f32::MAX, f32::MAX);
    let mut max = (f32::MIN, f32::MIN);
    for point in &winding.points {
        let u = point.dot(side.uaxis.axis) / side.uaxis.safe_scale() + side.uaxis.offset;
        let v = point.dot(side.vaxis.axis) / side.vaxis.safe_scale() + side.vaxis.offset;
        min = (min.0.min(u), min.1.min(v));
        max = (max.0.max(u), max.1.max(v));
    }
    Some((min, max))
}

/// Multiply both scales, keeping the texture anchored where it is.
pub fn scale_by(side: &mut Side, factor_u: f32, factor_v: f32) {
    set_scale(side, side.uaxis.scale * factor_u, side.vaxis.scale * factor_v);
}

/// Set the scales outright.
///
/// A zero scale reaches the compiler more often than you would think, and
/// dividing by it produces infinite texture coordinates that poison the
/// lightmap packer several stages later. Refusing it here is cheaper than
/// finding it there.
pub fn set_scale(side: &mut Side, u: f32, v: f32) {
    side.uaxis.scale = clean_scale(u);
    side.vaxis.scale = clean_scale(v);
}

fn clean_scale(v: f32) -> f32 {
    if !v.is_finite() || v.abs() < 1e-4 { 0.25 } else { v.clamp(-1024.0, 1024.0) }
}

/// Move the texture across the face, in texels.
pub fn shift_by(side: &mut Side, u: f32, v: f32) {
    side.uaxis.offset += u;
    side.vaxis.offset += v;
}

pub fn set_shift(side: &mut Side, u: f32, v: f32) {
    side.uaxis.offset = if u.is_finite() { u } else { 0.0 };
    side.vaxis.offset = if v.is_finite() { v } else { 0.0 };
}

/// Turn the texture within the face's plane, in degrees.
///
/// The offsets are recomputed so the texture pivots about the face's centre
/// rather than about the world origin -- otherwise rotating a face a long way
/// from the origin flings its texture off somewhere unreachable.
pub fn rotate_by(side: &mut Side, plane: &Plane, winding: &Winding, degrees: f32) {
    let pivot = winding.center();
    let before = (
        pivot.dot(side.uaxis.axis) / side.uaxis.safe_scale() + side.uaxis.offset,
        pivot.dot(side.vaxis.axis) / side.vaxis.safe_scale() + side.vaxis.offset,
    );

    let (u, v) = kerosene_map::texture::rotate_axes(plane, side.uaxis, side.vaxis, degrees);
    side.uaxis = u;
    side.vaxis = v;
    side.rotation = (side.rotation + degrees).rem_euclid(360.0);

    // Put the pivot back where it was in texture space.
    side.uaxis.offset = before.0 - pivot.dot(side.uaxis.axis) / side.uaxis.safe_scale();
    side.vaxis.offset = before.1 - pivot.dot(side.vaxis.axis) / side.vaxis.safe_scale();
}

/// Give the face the default world-aligned projection.
///
/// What every face starts with, and what makes the faces of a long wall share
/// a continuous texture rather than each starting over.
pub fn align_to_world(side: &mut Side, plane: &Plane) {
    let scale = (side.uaxis.scale.abs().max(1e-4), side.vaxis.scale.abs().max(1e-4));
    let (mut u, mut v) = kerosene_map::texture::default_axes_for_plane(plane, 0.25);
    u.scale = scale.0;
    v.scale = scale.1;
    side.uaxis = u;
    side.vaxis = v;
    side.rotation = 0.0;
}

/// Lay the texture in the face's own plane.
///
/// On a sloped surface the world-aligned projection foreshortens the texture;
/// this makes it square again, at the cost of no longer lining up with the
/// faces around it. Which one you want depends on the surface, which is why
/// both are here.
pub fn align_to_face(side: &mut Side, plane: &Plane) {
    let normal = plane.normal;
    // Any vector not parallel to the normal gives a starting tangent; the
    // world axis the face is least aligned with is the stablest choice.
    let helper = if normal.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
    let u = normal.cross(helper).normalize_or_zero();
    let v = normal.cross(u).normalize_or_zero();
    if u.length_squared() < 0.5 || v.length_squared() < 0.5 { return }

    side.uaxis = TextureAxis::new(u, 0.0, side.uaxis.scale.abs().max(1e-4));
    side.vaxis = TextureAxis::new(v, 0.0, side.vaxis.scale.abs().max(1e-4));
    side.rotation = 0.0;
}

/// Push the texture to an edge, the centre, or make it span the face once.
pub fn justify(side: &mut Side, winding: &Winding, how: Justify, texture: (u32, u32)) {
    let (width, height) = (texture.0.max(1) as f32, texture.1.max(1) as f32);

    if how == Justify::Fit {
        // Work out the face's extent along each axis in world units, then
        // choose the scale that makes the texture cover it exactly once.
        let span = axis_span(side, winding);
        if span.0 > 1e-4 && span.1 > 1e-4 {
            set_scale(side, span.0 / width, span.1 / height);
        }
    }

    let Some((min, max)) = texel_bounds(side, winding) else { return };
    let (du, dv) = match how {
        Justify::Left | Justify::Fit => (-min.0, 0.0),
        Justify::Right => (width - max.0, 0.0),
        Justify::Top => (0.0, -min.1),
        Justify::Bottom => (0.0, height - max.1),
        Justify::Centre => (
            (width - (max.0 - min.0)) * 0.5 - min.0,
            (height - (max.1 - min.1)) * 0.5 - min.1,
        ),
    };
    // Fit wants both axes at the corner, not just the one.
    let (du, dv) = if how == Justify::Fit { (-min.0, -min.1) } else { (du, dv) };
    shift_by(side, du, dv);
}

/// How far the face reaches along each texture axis, in world units.
fn axis_span(side: &Side, winding: &Winding) -> (f32, f32) {
    let mut min = (f32::MAX, f32::MAX);
    let mut max = (f32::MIN, f32::MIN);
    for point in &winding.points {
        let u = point.dot(side.uaxis.axis);
        let v = point.dot(side.vaxis.axis);
        min = (min.0.min(u), min.1.min(v));
        max = (max.0.max(u), max.1.max(v));
    }
    if winding.points.is_empty() { return (0.0, 0.0) }
    (max.0 - min.0, max.1 - min.1)
}

/// The winding of one side of a solid, for the operations that need the
/// face's shape.
pub fn winding_of(solid: &Solid, side_id: u32) -> Option<(Plane, Winding)> {
    solid
        .face_windings()
        .into_iter()
        .find(|(s, _)| s.id == side_id)
        .and_then(|(s, w)| s.plane().map(|p| (p, w)))
}

#[cfg(test)]
mod tests;
