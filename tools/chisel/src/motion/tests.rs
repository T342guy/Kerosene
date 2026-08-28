// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;

/// A door: a 16-deep, 96-wide, 128-tall slab tied to `func_door`.
fn door_document(keys: &[(&str, &str)]) -> Document {
    let mut document = Document::new();
    document.map.world.solids.clear();
    let id = document.create_block(Vec3::new(0.0, 0.0, 0.0), Vec3::new(16.0, 96.0, 128.0));
    document.selection.clear();
    document.selection.solids.insert(id);
    let entity = document.tie_to_entity("func_door").unwrap();
    if let Some(e) = document.find_entity_mut(entity) {
        for (k, v) in keys { e.set(k, *v); }
    }
    document.selection.clear();
    document.selection.entities.insert(entity);
    document
}

#[test]
fn nothing_selected_has_no_motion_to_draw() {
    let mut document = door_document(&[("movedir", "0 0 1")]);
    document.selection.clear();
    assert!(of_selection(&document).is_none());
}

#[test]
fn a_door_shows_how_far_it_opens_and_which_way() {
    let document = door_document(&[("movedir", "0 0 1"), ("lip", "8")]);
    let motion = of_selection(&document).expect("a door moves");

    // 128 tall, less an 8-unit lip.
    assert_eq!(motion.arrow.1 - motion.arrow.0, Vec3::new(0.0, 0.0, 120.0));
    assert!(motion.label.contains("120"), "{}", motion.label);
    assert!(motion.label.contains("+Z"), "{}", motion.label);
}

#[test]
fn the_travel_is_the_one_the_game_will_use() {
    // Two copies of the formula would agree only by luck, and the picture
    // being wrong about the door is worse than no picture.
    let document = door_document(&[("movedir", "1 0 0"), ("lip", "4")]);
    let motion = of_selection(&document).unwrap();

    let (dir, distance) = void_game::doors::travel(Vec3::new(16.0, 96.0, 128.0), Vec3::X, 4.0);
    assert_eq!(motion.arrow.1 - motion.arrow.0, dir * distance);
}

#[test]
fn a_door_moves_along_whichever_axis_it_is_told_to() {
    // A wide door sliding sideways travels by its width, not its thickness.
    let document = door_document(&[("movedir", "0 1 0"), ("lip", "0")]);
    let motion = of_selection(&document).unwrap();
    assert_eq!(motion.arrow.1 - motion.arrow.0, Vec3::new(0.0, 96.0, 0.0));
}

#[test]
fn the_ghost_is_the_door_where_it_will_end_up() {
    let document = door_document(&[("movedir", "0 0 1"), ("lip", "8")]);
    let motion = of_selection(&document).unwrap();

    assert_eq!(motion.ghost.len(), 6, "six faces of one slab");
    let lowest = motion
        .ghost
        .iter()
        .flatten()
        .map(|p| p.z)
        .fold(f32::INFINITY, f32::min);
    assert_eq!(lowest, 120.0, "the closed door's floor was at 0");
}

#[test]
fn a_door_with_no_movedir_says_nothing_rather_than_guessing() {
    // The game defaults to +Z, but a door nobody has set a direction on is
    // one nobody has finished, and an arrow would look like a decision.
    let document = door_document(&[]);
    assert!(of_selection(&document).is_none());
}

#[test]
fn a_lip_bigger_than_the_door_still_leaves_it_moving() {
    // The game clamps to 1 rather than travelling backwards; the editor must
    // draw the same clamp or it draws a door going the wrong way.
    let document = door_document(&[("movedir", "0 0 1"), ("lip", "1000")]);
    let motion = of_selection(&document).unwrap();
    assert_eq!(motion.arrow.1 - motion.arrow.0, Vec3::new(0.0, 0.0, 1.0));
}

#[test]
fn a_point_entity_with_angles_shows_which_way_it_faces() {
    let mut document = Document::new();
    let id = document.create_entity("light_spot", Vec3::new(64.0, 64.0, 128.0));
    if let Some(e) = document.find_entity_mut(id) { e.set("angles", "0 90 0"); }
    document.selection.clear();
    document.selection.entities.insert(id);

    let motion = of_selection(&document).expect("it faces somewhere");
    assert!(motion.ghost.is_empty(), "a point entity has no shape to ghost");
    let direction = (motion.arrow.1 - motion.arrow.0).normalize();
    assert!((direction - Vec3::Y).length() < 0.01, "yaw 90 faces +Y: {direction:?}");
    assert!(motion.label.contains("+Y"), "{}", motion.label);
}

#[test]
fn an_entity_facing_straight_ahead_is_not_annotated() {
    // Zero angles is the default, and drawing an arrow for it would put one
    // on nearly every entity in the map.
    let mut document = Document::new();
    let id = document.create_entity("info_player_start", Vec3::ZERO);
    if let Some(e) = document.find_entity_mut(id) { e.set("angles", "0 0 0"); }
    document.selection.clear();
    document.selection.entities.insert(id);
    assert!(of_selection(&document).is_none());
}

#[test]
fn the_axes_are_named_where_they_have_names() {
    assert!(axis_words(Vec3::Z).contains("+Z"));
    assert!(axis_words(Vec3::NEG_Y).contains("-Y"));
    assert!(axis_words(Vec3::new(0.0, 0.0, 5.0)).contains("+Z"), "length does not matter");
    // And a diagonal is given as numbers rather than as a wrong guess.
    let diagonal = axis_words(Vec3::new(1.0, 1.0, 0.0));
    assert!(!diagonal.contains('X'), "{diagonal}");
}
