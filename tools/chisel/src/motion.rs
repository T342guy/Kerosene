// SPDX-License-Identifier: MPL-2.0
//! Where a selected entity is going, and which way it faces.
//!
//! A `func_door` is a box until you compile the map and walk into it. Nothing
//! in the editor said how far it opened, which way, or whether it would clear
//! the doorway -- all of which are decided by its own size and are therefore
//! knowable while you are drawing it. The same goes for anything with
//! `angles`: an entity's facing is a number in a text field and a mystery
//! everywhere else.
//!
//! The travel comes from `kerosene_game` rather than from a copy of the formula,
//! so the arrow and the door agree by construction.

use crate::document::Document;
use kerosene_math::{Aabb, Vec3};

/// What to draw over a selected entity.
#[derive(Clone, Debug, PartialEq)]
pub struct Motion {
    /// The selection's outline where it will end up, as world polygons.
    pub ghost: Vec<Vec<Vec3>>,
    /// From, to -- an arrow along the movement or the facing.
    pub arrow: (Vec3, Vec3),
    /// One line saying what it does.
    pub label: String,
}

/// How long a facing arrow is drawn, in kerosene units.
///
/// A fixed length, because a facing has no distance: it is a direction, and
/// scaling the arrow by the entity's size would imply a reach it does not
/// have.
const FACING_ARROW: f32 = 64.0;

/// What the selected entity's motion or facing is, if it has one.
pub fn of_selection(document: &Document) -> Option<Motion> {
    let id = *document.selection.entities.iter().next()?;
    let entity = document.find_entity(id)?;

    if !entity.solids.is_empty() {
        return moving_brushes(document, entity);
    }
    facing(entity)
}

/// A brush entity that travels: a door, a platform.
fn moving_brushes(document: &Document, entity: &kerosene_map::Entity) -> Option<Motion> {
    let movedir = entity.get_vec3("movedir")?;

    let mut bounds = Aabb::EMPTY;
    for solid in &entity.solids {
        let b = solid.bounds();
        bounds.add_point(b.min);
        bounds.add_point(b.max);
    }
    if bounds.is_empty() { return None }

    let lip = entity
        .get("lip")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(8.0);
    let (dir, distance) = kerosene_game::doors::travel(bounds.size(), movedir, lip);
    let offset = dir * distance;

    let centre = bounds.center();
    Some(Motion {
        ghost: crate::draw::transformed_outline_of(document, &entity.solids, |p| p + offset),
        arrow: (centre, centre + offset),
        label: format!(
            "opens {} along {}",
            kerosene_math::units::length_short(distance),
            axis_words(dir),
        ),
    })
}

/// A point entity that faces somewhere.
fn facing(entity: &kerosene_map::Entity) -> Option<Motion> {
    let angles = entity.get_vec3("angles")?;
    // Stored as pitch, yaw, roll -- the order the file writes them, which is
    // not the order a vector reads in.
    let a = kerosene_math::Angles::new(angles.x, angles.y, angles.z);
    if a.pitch == 0.0 && a.yaw == 0.0 && a.roll == 0.0 { return None }

    let at = entity.origin();
    let forward = a.forward();
    Some(Motion {
        ghost: Vec::new(),
        arrow: (at, at + forward * FACING_ARROW),
        label: format!("faces {}", axis_words(forward)),
    })
}

/// A direction in words, for a label.
///
/// Named axes where it is one, and the raw vector where it is not: "+Z" is
/// worth more than "0 0 1", and "0.7 0 0.7" is worth more than a wrong guess
/// at which diagonal it is.
pub fn axis_words(dir: Vec3) -> String {
    const NAMED: [(Vec3, &str); 6] = [
        (Vec3::X, "+X (east)"),
        (Vec3::NEG_X, "-X (west)"),
        (Vec3::Y, "+Y (north)"),
        (Vec3::NEG_Y, "-Y (south)"),
        (Vec3::Z, "+Z (up)"),
        (Vec3::NEG_Z, "-Z (down)"),
    ];
    let dir = dir.normalize_or_zero();
    for (axis, name) in NAMED {
        if dir.dot(axis) > 0.999 { return name.to_string() }
    }
    format!(
        "{} {} {}",
        kerosene_math::format_float(dir.x),
        kerosene_math::format_float(dir.y),
        kerosene_math::format_float(dir.z),
    )
}

#[cfg(test)]
mod tests;
