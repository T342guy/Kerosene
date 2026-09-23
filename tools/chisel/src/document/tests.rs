// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
use super::*;
use kerosene_math::Quat;

fn doc() -> Document {
    Document::new()
}

fn block(doc: &mut Document, lo: f32, hi: f32) -> u32 {
    doc.create_block(Vec3::splat(lo), Vec3::splat(hi))
}

#[test]
fn creating_a_block_adds_it_to_the_world_and_selects_it() {
    let mut d = doc();
    let id = block(&mut d, 0.0, 64.0);
    assert_eq!(d.map.world.solids.len(), 1);
    assert_eq!(d.selection.solids.len(), 1);
    assert!(d.selection.solids.contains(&id));
    assert!(d.is_modified());
}

#[test]
fn a_new_block_is_a_valid_brush() {
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    assert!(d.problems().is_empty(), "{:?}", d.problems());
    assert!(d.map.world.solids[0].validate().is_ok());
}

#[test]
fn blocks_snap_to_the_grid() {
    let mut d = doc();
    d.grid.size = 16.0;
    d.create_block(Vec3::new(3.0, 3.0, 3.0), Vec3::new(60.0, 60.0, 60.0));
    let bounds = d.map.world.solids[0].bounds();
    assert_eq!(bounds.min, Vec3::ZERO);
    assert_eq!(bounds.max, Vec3::splat(64.0));
}

#[test]
fn every_object_gets_a_unique_id() {
    let mut d = doc();
    for i in 0..5 {
        d.create_block(
            Vec3::splat(i as f32 * 64.0),
            Vec3::splat(i as f32 * 64.0 + 32.0),
        );
    }
    d.create_entity("light", Vec3::ZERO);

    let mut ids = Vec::new();
    for e in d.map.all_entities() {
        ids.push(e.id);
        for s in &e.solids {
            ids.push(s.id);
            ids.extend(s.sides.iter().map(|x| x.id));
        }
    }
    let unique: HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "ids collided");
}

#[test]
fn undo_restores_the_previous_state() {
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    block(&mut d, 128.0, 192.0);
    assert_eq!(d.map.world.solids.len(), 2);

    assert_eq!(d.undo().as_deref(), Some("create block"));
    assert_eq!(d.map.world.solids.len(), 1);
    d.undo();
    assert_eq!(d.map.world.solids.len(), 0);
    assert!(d.undo().is_none(), "nothing left to undo");
}

#[test]
fn undo_restores_the_selection_too() {
    // Undoing to a state where different things were selected is disorienting
    // if the selection does not come back with it.
    let mut d = doc();
    let first = block(&mut d, 0.0, 64.0);
    block(&mut d, 128.0, 192.0);
    d.undo();
    assert_eq!(d.selection.solids.len(), 1);
    assert!(d.selection.solids.contains(&first));
}

#[test]
fn redo_reapplies_what_undo_took_away() {
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    d.undo();
    assert_eq!(d.map.world.solids.len(), 0);
    assert_eq!(d.redo().as_deref(), Some("create block"));
    assert_eq!(d.map.world.solids.len(), 1);
}

#[test]
fn a_new_edit_discards_the_redo_history() {
    // Otherwise redo would reapply a change onto a state it was never
    // recorded against.
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    d.undo();
    assert_eq!(d.redo_depth(), 1);
    block(&mut d, 200.0, 264.0);
    assert_eq!(d.redo_depth(), 0);
}

#[test]
fn the_undo_history_is_bounded() {
    let mut d = doc();
    for i in 0..(MAX_UNDO + 20) {
        d.create_block(Vec3::splat(i as f32), Vec3::splat(i as f32 + 8.0));
    }
    assert_eq!(d.undo_depth(), MAX_UNDO);
}

#[test]
fn every_edit_is_undoable() {
    // An editor where some operations are undoable and others are not is
    // worse than one with no undo at all.
    let mut d = doc();
    let before = d.map.to_text();

    block(&mut d, 0.0, 64.0);
    d.create_entity("light", Vec3::new(0.0, 0.0, 64.0));
    d.selection.solids.insert(d.map.world.solids[0].id);
    d.current_material = "dev/wall".into();
    d.apply_material();
    d.tie_to_entity("func_door");
    d.untie_to_world();
    d.selection.solids.insert(d.map.world.solids[0].id);
    d.move_selection(Vec3::new(32.0, 0.0, 0.0));
    d.delete_selection();

    while d.undo().is_some() {}
    assert_eq!(
        d.map.to_text(),
        before,
        "undoing everything should restore the empty map"
    );
}

#[test]
fn deleting_removes_what_was_selected_and_nothing_else() {
    let mut d = doc();
    let keep = block(&mut d, 0.0, 64.0);
    let go = block(&mut d, 128.0, 192.0);
    d.selection.clear();
    d.selection.solids.insert(go);

    assert_eq!(d.delete_selection(), 1);
    assert_eq!(d.map.world.solids.len(), 1);
    assert_eq!(d.map.world.solids[0].id, keep);
    assert!(d.selection.is_empty());
}

#[test]
fn deleting_nothing_does_not_touch_the_history() {
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    let depth = d.undo_depth();
    d.selection.clear();
    assert_eq!(d.delete_selection(), 0);
    assert_eq!(
        d.undo_depth(),
        depth,
        "a no-op must not fill the undo stack"
    );
}

#[test]
fn moving_a_selection_moves_only_it() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    let b = block(&mut d, 128.0, 192.0);
    d.selection.clear();
    d.selection.solids.insert(a);
    d.move_selection(Vec3::new(0.0, 0.0, 32.0));

    let moved = d.find_solid(a).unwrap().bounds();
    let still = d.find_solid(b).unwrap().bounds();
    assert_eq!(moved.min.z, 32.0);
    assert_eq!(still.min.z, 128.0);
}

#[test]
fn moving_a_point_entity_moves_its_origin() {
    let mut d = doc();
    let id = d.create_entity("info_player_start", Vec3::new(0.0, 0.0, 16.0));
    d.move_selection(Vec3::new(64.0, 0.0, 0.0));
    assert_eq!(
        d.find_entity(id).unwrap().origin(),
        Vec3::new(64.0, 0.0, 16.0)
    );
}

#[test]
fn a_move_smaller_than_the_grid_does_nothing() {
    let mut d = doc();
    d.grid.size = 16.0;
    let id = block(&mut d, 0.0, 64.0);
    let depth = d.undo_depth();
    d.move_selection(Vec3::new(3.0, 0.0, 0.0));
    assert_eq!(d.find_solid(id).unwrap().bounds().min, Vec3::ZERO);
    assert_eq!(d.undo_depth(), depth);
}

#[test]
fn applying_a_material_covers_a_whole_selected_brush() {
    let mut d = doc();
    let id = block(&mut d, 0.0, 64.0);
    d.current_material = "dev/wall".into();
    assert_eq!(d.apply_material(), 6);
    assert!(
        d.find_solid(id)
            .unwrap()
            .sides
            .iter()
            .all(|s| s.material == "dev/wall")
    );
}

#[test]
fn applying_a_material_to_one_face_leaves_the_others() {
    let mut d = doc();
    let id = block(&mut d, 0.0, 64.0);
    let side = d.find_solid(id).unwrap().sides[0].id;
    d.selection.clear();
    d.selection.faces.insert((id, side));

    d.current_material = "dev/wall".into();
    assert_eq!(d.apply_material(), 1);
    let solid = d.find_solid(id).unwrap();
    assert_eq!(solid.sides[0].material, "dev/wall");
    assert!(solid.sides[1..].iter().all(|s| s.material == "dev/grid"));
}

#[test]
fn applying_a_walkmap_rule_covers_a_whole_selected_brush() {
    let mut d = doc();
    let id = block(&mut d, 0.0, 64.0);
    assert_eq!(d.apply_walkmap(WalkmapRule::Deny), 6);
    assert!(
        d.find_solid(id)
            .unwrap()
            .sides
            .iter()
            .all(|s| s.walkmap == WalkmapRule::Deny)
    );
}

#[test]
fn applying_a_material_to_a_selected_brush_entity_retextures_it() {
    // Whole-brush selection of a door selects the *entity*, and applying must
    // reach its brushes -- otherwise retexturing a door silently does nothing.
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    let entity = d.tie_to_entity("func_door").unwrap();
    assert_eq!(
        d.map.world.solids.len(),
        0,
        "the brush should have left the world"
    );

    // `tie_to_entity` selects the new entity.
    assert_eq!(d.selection.entities.len(), 1);
    d.current_material = "dev/wall".into();
    assert_eq!(d.apply_material(), 6);

    let door = d.find_entity(entity).unwrap();
    assert!(
        door.solids
            .iter()
            .all(|s| s.sides.iter().all(|x| x.material == "dev/wall"))
    );
}

#[test]
fn applying_a_walkmap_rule_to_one_face_leaves_the_others() {
    let mut d = doc();
    let id = block(&mut d, 0.0, 64.0);
    let side = d.find_solid(id).unwrap().sides[0].id;
    d.selection.clear();
    d.selection.faces.insert((id, side));

    assert_eq!(d.apply_walkmap(WalkmapRule::Avoid), 1);
    let solid = d.find_solid(id).unwrap();
    assert_eq!(solid.sides[0].walkmap, WalkmapRule::Avoid);
    assert!(
        solid.sides[1..]
            .iter()
            .all(|s| s.walkmap == WalkmapRule::Allow)
    );
}

#[test]
fn a_walkmap_rule_is_one_undo_step_for_a_whole_selection() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    let b = block(&mut d, 64.0, 128.0);
    d.selection.clear();
    d.selection.solids.insert(a);
    d.selection.solids.insert(b);

    let depth = d.undo_depth();
    d.apply_walkmap(WalkmapRule::Always);
    assert_eq!(d.undo_depth(), depth + 1, "two brushes, one undo step");
    assert!(
        d.find_solid(a)
            .unwrap()
            .sides
            .iter()
            .all(|s| s.walkmap == WalkmapRule::Always)
    );
    assert!(
        d.find_solid(b)
            .unwrap()
            .sides
            .iter()
            .all(|s| s.walkmap == WalkmapRule::Always)
    );
}

#[test]
fn tying_brushes_to_an_entity_moves_them_out_of_the_world() {
    // How a designer makes a door: build it in the world, then tie it.
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    let b = block(&mut d, 64.0, 128.0);
    d.selection.clear();
    d.selection.solids.insert(a);
    d.selection.solids.insert(b);

    let entity = d
        .tie_to_entity("func_door")
        .expect("should create an entity");
    assert!(
        d.map.world.solids.is_empty(),
        "the brushes should have left the world"
    );
    let door = d.find_entity(entity).unwrap();
    assert_eq!(door.classname(), "func_door");
    assert_eq!(door.solids.len(), 2);
    assert_eq!(d.selection.entities.len(), 1);
}

#[test]
fn untying_puts_the_brushes_back() {
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    let entity = d.tie_to_entity("func_door").unwrap();
    assert_eq!(d.map.world.solids.len(), 0);

    d.selection.clear();
    d.selection.entities.insert(entity);
    assert_eq!(d.untie_to_world(), 1);
    assert_eq!(d.map.world.solids.len(), 1);
    assert!(
        d.find_entity(entity).is_none(),
        "the emptied entity should be gone"
    );
}

#[test]
fn tying_nothing_does_nothing() {
    let mut d = doc();
    assert!(d.tie_to_entity("func_door").is_none());
}

#[test]
fn selection_bounds_cover_everything_selected() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    let b = block(&mut d, 128.0, 192.0);
    d.selection.solids.insert(a);
    d.selection.solids.insert(b);

    let bounds = d.selection_bounds().unwrap();
    assert_eq!(bounds.min, Vec3::ZERO);
    assert_eq!(bounds.max, Vec3::splat(192.0));
}

#[test]
fn a_point_entity_still_has_bounds_to_grab() {
    let mut d = doc();
    d.create_entity("light", Vec3::new(100.0, 100.0, 100.0));
    let bounds = d
        .selection_bounds()
        .expect("a point entity needs something to drag");
    assert!(bounds.size().length() > 0.0);
    assert!(bounds.contains_point(Vec3::splat(100.0)));
}

#[test]
fn an_empty_selection_has_no_bounds() {
    assert!(doc().selection_bounds().is_none());
}

#[test]
fn only_world_brushes_are_resizable() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    let bounds = d.resizable_bounds().unwrap();
    assert_eq!(bounds.min, Vec3::ZERO);
    assert_eq!(bounds.max, Vec3::splat(64.0));
    assert!(d.selection.solids.contains(&a));
}

#[test]
fn a_point_entity_is_not_resizable() {
    let mut d = doc();
    d.create_entity("light", Vec3::new(100.0, 100.0, 100.0));
    // It still has bounds to grab and drag, but no resize grips.
    assert!(d.selection_bounds().is_some());
    assert!(d.resizable_bounds().is_none());
}

#[test]
fn a_brush_entity_is_not_resizable() {
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    let entity = d.tie_to_entity("func_door").unwrap();
    assert!(d.selection.entities.contains(&entity));
    assert!(d.selection.solids.is_empty());
    assert!(d.selection_bounds().is_some(), "the door still has bounds");
    assert!(
        d.resizable_bounds().is_none(),
        "a door is configured, not stretched"
    );
}

#[test]
fn a_mixed_selection_is_not_resizable() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    let light = d.create_entity("light", Vec3::new(100.0, 100.0, 100.0));
    // create_entity selects the entity and clears the brush; select both.
    d.selection.solids.insert(a);
    d.selection.entities.insert(light);
    assert!(d.resizable_bounds().is_none());
}

#[test]
fn a_document_round_trips_through_a_file() {
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    d.create_entity("info_player_start", Vec3::new(32.0, 32.0, 16.0));

    let path = std::env::temp_dir().join(format!("chisel-test-{}.keromap", std::process::id()));
    d.save(Some(path.clone())).unwrap();
    assert!(!d.is_modified(), "saving should clear the modified flag");

    let reopened = Document::open(path.clone()).unwrap();
    assert_eq!(reopened.map.world.solids.len(), 1);
    assert_eq!(reopened.map.entities.len(), 1);
    assert!(!reopened.is_modified());
    let _ = std::fs::remove_file(path);
}

#[test]
fn undoing_back_to_the_saved_state_is_clean_again() {
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    d.mark_clean();
    assert!(!d.is_modified());

    block(&mut d, 128.0, 192.0);
    assert!(d.is_modified());
    d.undo();
    assert!(!d.is_modified(), "undone to exactly what was saved");
    d.redo();
    assert!(d.is_modified());
    d.undo();
    d.undo();
    assert!(d.is_modified(), "now short of the saved state");
    d.redo();
    assert!(!d.is_modified());

    // Editing from below the saved depth abandons the branch it was on.
    d.undo();
    block(&mut d, 256.0, 320.0);
    assert!(d.is_modified());
    d.undo();
    assert!(d.is_modified(), "the saved state is not on this branch");
}

#[test]
fn the_title_shows_unsaved_changes() {
    let mut d = doc();
    // Not "untitled.keromap": a map that has never been saved has no file,
    // and showing one is how saving came to look as though it had happened.
    assert_eq!(d.title(), "untitled");
    block(&mut d, 0.0, 64.0);
    assert!(d.title().ends_with('*'), "{}", d.title());
}

// ---- face editing ---------------------------------------------------------

/// Two cubes, with the whole of the first one's faces selected.
fn two_cubes_with_one_selected() -> (Document, u32, u32) {
    let mut document = Document::new();
    document.grid.size = 16.0;
    let a = document.create_block(Vec3::ZERO, Vec3::splat(64.0));
    let b = document.create_block(Vec3::new(256.0, 0.0, 0.0), Vec3::new(320.0, 64.0, 64.0));
    document.selection.clear();
    let sides: Vec<u32> = document
        .find_solid(a)
        .unwrap()
        .sides
        .iter()
        .map(|s| s.id)
        .collect();
    for side in sides {
        document.selection.faces.insert((a, side));
    }
    (document, a, b)
}

#[test]
fn an_edit_reaches_every_selected_face_and_no_others() {
    let (mut document, a, b) = two_cubes_with_one_selected();
    let changed = document.edit_faces("shift", |side, _, _| {
        crate::faces::shift_by(side, 8.0, 0.0);
    });
    assert_eq!(changed, 6, "a cube has six faces");

    assert!(
        document
            .find_solid(a)
            .unwrap()
            .sides
            .iter()
            .all(|s| s.uaxis.offset == 8.0),
        "not every selected face moved"
    );
    assert!(
        document
            .find_solid(b)
            .unwrap()
            .sides
            .iter()
            .all(|s| s.uaxis.offset == 0.0),
        "an unselected brush was edited"
    );
}

#[test]
fn editing_a_whole_selection_is_one_undo_step() {
    // Six presses of ctrl-Z to take back one nudge is a bug in everything but
    // name.
    let (mut document, a, _) = two_cubes_with_one_selected();
    let before = document.undo_depth();

    document.edit_faces("shift", |side, _, _| crate::faces::shift_by(side, 8.0, 0.0));
    assert_eq!(document.undo_depth(), before + 1);

    document.undo();
    assert!(
        document
            .find_solid(a)
            .unwrap()
            .sides
            .iter()
            .all(|s| s.uaxis.offset == 0.0),
        "one undo did not take the whole edit back"
    );
}

#[test]
fn editing_with_nothing_selected_does_nothing_and_costs_no_undo() {
    let mut document = Document::new();
    document.create_block(Vec3::ZERO, Vec3::splat(64.0));
    document.selection.clear();
    let before = document.undo_depth();

    assert_eq!(
        document.edit_faces("shift", |side, _, _| side.uaxis.offset = 99.0),
        0
    );
    assert_eq!(
        document.undo_depth(),
        before,
        "an empty edit pushed an undo step"
    );
}

#[test]
fn an_edit_is_handed_the_faces_own_shape() {
    // Fit and justify need the face's winding, and the plane it lies on. An
    // edit given the wrong face's shape would fit the texture to the wrong
    // rectangle -- which looks almost right, and is the worst kind of wrong.
    let mut document = Document::new();
    document.grid.size = 16.0;
    let id = document.create_block(Vec3::ZERO, Vec3::new(256.0, 64.0, 64.0));
    document.selection.clear();

    // Just the top face.
    let side = document
        .find_solid(id)
        .unwrap()
        .sides
        .iter()
        .find(|s| s.plane().is_some_and(|p| p.normal.z > 0.9))
        .unwrap()
        .id;
    document.selection.faces.insert((id, side));

    document.edit_faces("fit", |side, _, winding| {
        crate::faces::justify(side, winding, crate::faces::Justify::Fit, (256, 256));
    });

    let solid = document.find_solid(id).unwrap();
    let edited = solid.sides.iter().find(|s| s.id == side).unwrap();
    let (_, winding) = crate::faces::winding_of(solid, side).unwrap();
    let (min, max) = crate::faces::texel_bounds(edited, &winding).unwrap();
    assert!(
        (max.0 - min.0 - 256.0).abs() < 1e-1,
        "u span {}",
        max.0 - min.0
    );
    assert!(
        (max.1 - min.1 - 256.0).abs() < 1e-1,
        "v span {}",
        max.1 - min.1
    );
}

#[test]
fn the_selected_faces_come_back_in_a_stable_order() {
    // A panel showing "the first selected face" has to show the same one from
    // frame to frame.
    let (document, _, _) = two_cubes_with_one_selected();
    let first: Vec<(u32, u32)> = document
        .selected_face_specs()
        .iter()
        .map(|f| (f.solid, f.side.id))
        .collect();
    let again: Vec<(u32, u32)> = document
        .selected_face_specs()
        .iter()
        .map(|f| (f.solid, f.side.id))
        .collect();
    assert_eq!(first, again);
    assert_eq!(first.len(), 6);
    assert_eq!(document.selected_face_count(), 6);
}

#[test]
fn a_face_spec_carries_the_plane_and_winding_that_face_sits_on() {
    let (document, _, _) = two_cubes_with_one_selected();
    for spec in document.selected_face_specs() {
        assert!(spec.winding.points.len() >= 3, "a face with no shape");
        for point in &spec.winding.points {
            assert!(
                (spec.plane.normal.dot(*point) - spec.plane.dist).abs() < 0.1,
                "a winding point is off its own plane"
            );
        }
    }
}

// ---- what a brush is -------------------------------------------------------

/// A document with one world brush selected.
fn one_selected_brush() -> (Document, u32) {
    let mut d = Document::new();
    d.map.world.solids.clear();
    let id = d.create_block(Vec3::ZERO, Vec3::new(64.0, 64.0, 64.0));
    d.selection.clear();
    d.selection.solids.insert(id);
    (d, id)
}

#[test]
fn a_world_brush_belongs_to_no_class() {
    let (d, _) = one_selected_brush();
    assert_eq!(d.selected_brush_class(), None);
}

#[test]
fn setting_a_class_makes_the_brushes_into_one_entity() {
    let (mut d, _) = one_selected_brush();
    assert!(d.set_brush_class(Some("func_door")));

    assert!(d.map.world.solids.is_empty(), "it left the world");
    assert_eq!(d.map.entities.len(), 1);
    assert_eq!(
        d.selected_brush_class().map(|(_, c)| c),
        Some("func_door".into())
    );
    assert_eq!(
        d.selection.entities.len(),
        1,
        "and the entity is what is selected"
    );
}

#[test]
fn setting_the_class_it_already_is_does_nothing() {
    // Including nothing to undo: a no-op that costs a ctrl-Z is worse than
    // no operation at all.
    let (mut d, _) = one_selected_brush();
    d.set_brush_class(Some("func_door"));
    let depth = d.undo_depth();

    assert!(!d.set_brush_class(Some("func_door")));
    assert_eq!(d.undo_depth(), depth);
}

#[test]
fn changing_the_class_is_one_step_not_two() {
    // A designer turning a trigger into a door is doing one thing.
    let (mut d, _) = one_selected_brush();
    d.set_brush_class(Some("trigger_multiple"));
    let depth = d.undo_depth();

    assert!(d.set_brush_class(Some("func_door")));
    assert_eq!(d.undo_depth(), depth + 1);
    assert_eq!(
        d.selected_brush_class().map(|(_, c)| c),
        Some("func_door".into())
    );

    d.undo();
    assert_eq!(
        d.selected_brush_class().map(|(_, c)| c),
        Some("trigger_multiple".into())
    );
}

#[test]
fn changing_the_class_keeps_the_name_and_the_wiring() {
    // Dropping a targetname silently breaks every output wired to it.
    let (mut d, _) = one_selected_brush();
    d.set_brush_class(Some("trigger_multiple"));
    let id = d.selected_brush_class().unwrap().0;
    if let Some(e) = d.find_entity_mut(id) {
        e.set("targetname", "gate_trigger");
        e.connections.push(kerosene_map::Connection::new(
            "OnStartTouch",
            "gate",
            "Open",
        ));
    }

    d.set_brush_class(Some("func_door"));
    let id = d.selected_brush_class().unwrap().0;
    let entity = d.find_entity(id).unwrap();
    assert_eq!(entity.get("targetname"), Some("gate_trigger"));
    assert_eq!(entity.connections.len(), 1);
}

#[test]
fn putting_it_back_in_the_world_leaves_no_empty_entity_behind() {
    // An entity with no brushes still reaches the compiler's entity lump and
    // still gets spawned, which is a ghost that is very hard to find.
    let (mut d, _) = one_selected_brush();
    d.set_brush_class(Some("func_door"));
    assert!(d.set_brush_class(None));

    assert_eq!(d.map.world.solids.len(), 1);
    assert!(d.map.entities.is_empty(), "{:?}", d.map.entities.len());
    assert_eq!(d.selection.solids.len(), 1);
}

#[test]
fn a_trigger_textures_itself_so_it_is_not_a_visible_block_in_a_doorway() {
    // Forgetting to do this by hand compiles a solid wall where a region was
    // meant to be, and the map looks broken in a way that has nothing to do
    // with triggers.
    let (mut d, _) = one_selected_brush();
    d.set_brush_class(Some("trigger_multiple"));

    let id = d.selected_brush_class().unwrap().0;
    let entity = d.find_entity(id).unwrap();
    assert!(
        entity.solids[0]
            .sides
            .iter()
            .all(|s| s.material == "tools/trigger"),
        "a trigger has to be invisible"
    );
}

#[test]
fn a_door_keeps_whatever_it_was_textured_with() {
    // Only a designer knows which door.
    let (mut d, _) = one_selected_brush();
    d.current_material = "dev/door".into();
    let id = d.create_block(Vec3::new(128.0, 0.0, 0.0), Vec3::new(192.0, 64.0, 64.0));
    d.selection.clear();
    d.selection.solids.insert(id);

    d.set_brush_class(Some("func_door"));
    let entity_id = d.selected_brush_class().unwrap().0;
    let entity = d.find_entity(entity_id).unwrap();
    assert!(
        entity.solids[0]
            .sides
            .iter()
            .all(|s| s.material == "dev/door")
    );
}

#[test]
fn selecting_a_brush_entity_is_the_same_as_selecting_its_brushes() {
    // Clicking a door selects the door, and "what am I editing" has to mean
    // the same thing either way.
    let (mut d, _) = one_selected_brush();
    d.set_brush_class(Some("func_door"));
    let by_entity = d.selected_solid_ids();

    let entity_id = d.selected_brush_class().unwrap().0;
    let solids: Vec<u32> = d
        .find_entity(entity_id)
        .unwrap()
        .solids
        .iter()
        .map(|s| s.id)
        .collect();
    d.selection.clear();
    for id in &solids {
        d.selection.solids.insert(*id);
    }

    assert_eq!(d.selected_solid_ids(), by_entity);
    assert_eq!(
        d.selected_brush_class().map(|(_, c)| c),
        Some("func_door".into())
    );
}

#[test]
fn a_selection_spanning_two_entities_has_no_single_class() {
    let (mut d, _) = one_selected_brush();
    d.set_brush_class(Some("func_door"));
    let first: Vec<u32> = d.map.entities[0].solids.iter().map(|s| s.id).collect();

    let other = d.create_block(Vec3::new(256.0, 0.0, 0.0), Vec3::new(320.0, 64.0, 64.0));
    d.selection.clear();
    d.selection.solids.insert(other);
    d.set_brush_class(Some("trigger_once"));

    d.selection.clear();
    for id in first {
        d.selection.solids.insert(id);
    }
    for e in &d.map.entities {
        if e.classname() == "trigger_once" {
            for s in &e.solids {
                d.selection.solids.insert(s.id);
            }
        }
    }
    assert_eq!(d.selected_brush_class(), None);
}

#[test]
fn nothing_selected_cannot_be_given_a_class() {
    let mut d = Document::new();
    d.selection.clear();
    assert!(!d.set_brush_class(Some("func_door")));
}

#[test]
fn duplicating_a_selection_makes_fresh_copies_and_selects_them() {
    let mut d = doc();
    let original = block(&mut d, 0.0, 64.0);
    let door = d.create_entity("func_door", Vec3::ZERO);
    d.find_entity_mut(door).unwrap().set("targetname", "gate");
    d.selection.solids.insert(original);
    d.selection.entities.insert(door);

    let n = d.duplicate_selection(Vec3::new(128.0, 0.0, 0.0));
    assert_eq!(n, 2);
    assert_eq!(d.map.world.solids.len(), 2);
    assert_eq!(d.map.entities.len(), 2);
    assert!(
        !d.selection.solids.contains(&original),
        "the copies are selected, not the originals"
    );
    let copy = d
        .map
        .world
        .solids
        .iter()
        .find(|s| s.id != original)
        .unwrap();
    assert!(copy.sides.iter().all(|side| side.id != 0));
    assert_eq!(copy.bounds().min.x, 128.0, "offset by the delta");
    let copied_door = d.map.entities.iter().find(|e| e.id != door).unwrap();
    assert_eq!(
        copied_door.get("targetname"),
        None,
        "a name names one thing"
    );

    d.undo();
    assert_eq!(d.map.world.solids.len(), 1);
}

#[test]
fn select_all_takes_everything() {
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    block(&mut d, 128.0, 192.0);
    d.create_entity("light", Vec3::ZERO);
    assert_eq!(d.select_all(), 3);
}

// ---- visibility, groups, visgroups, cordon --------------------------------

#[test]
fn a_hidden_visgroup_hides_its_members_and_drops_them_from_the_selection() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    let b = block(&mut d, 128.0, 192.0);
    d.selection.solids.insert(a);
    d.selection.solids.insert(b);
    let vg = d.new_visgroup_from_selection("both");
    assert_eq!(d.map.visgroup_members(vg).len(), 2);

    d.set_visgroup_visible(vg, false);
    assert!(!d.is_visible(ObjectId::Solid(a)));
    assert!(d.selection.is_empty(), "hidden things cannot stay selected");
    assert_eq!(d.visible_solids().count(), 0);
    assert_eq!(d.select_all(), 0, "select-all skips the hidden");
    assert_eq!(d.hidden_count(), 2);

    d.undo();
    assert!(d.is_visible(ObjectId::Solid(a)), "hiding is undoable");
}

#[test]
fn a_child_visgroup_is_hidden_by_its_parent() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    let parent = d.add_visgroup("outside", None);
    let child = d.add_visgroup("shed", Some(parent));
    d.selection.solids.insert(a);
    d.add_selection_to_visgroup(child);
    d.set_visgroup_visible(parent, false);
    assert!(!d.is_visible(ObjectId::Solid(a)));
    d.set_visgroup_visible(parent, true);
    assert!(d.is_visible(ObjectId::Solid(a)));
}

#[test]
fn quick_hide_and_unhide_all() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    let b = block(&mut d, 128.0, 192.0);
    d.selection.clear();
    d.selection.solids.insert(a);
    assert_eq!(d.hide_selection(), 1);
    assert!(!d.is_visible(ObjectId::Solid(a)));
    assert!(d.is_visible(ObjectId::Solid(b)));
    assert!(d.selection.is_empty());

    d.selection.solids.insert(b);
    assert_eq!(
        d.hide_unselected(),
        0,
        "a is already hidden; nothing else to hide"
    );
    assert_eq!(d.unhide_all(), 1);
    assert!(d.is_visible(ObjectId::Solid(a)));
    d.undo();
    assert!(
        !d.is_visible(ObjectId::Solid(a)),
        "unhiding is undoable too"
    );
}

#[test]
fn auto_visgroups_hide_by_kind_without_touching_the_map() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    let light = d.create_entity("light", Vec3::ZERO);
    let text_before = d.map.to_text();
    d.set_auto_visible(AutoGroup::Class("light".into()), false);
    assert!(!d.is_visible(ObjectId::Entity(light)));
    assert!(d.is_visible(ObjectId::Solid(a)));
    d.set_auto_visible(AutoGroup::WorldBrushes, false);
    assert!(!d.is_visible(ObjectId::Solid(a)));
    assert_eq!(d.map.to_text(), text_before, "session state, not map state");
}

#[test]
fn picking_a_group_member_selects_the_group_unless_groups_are_ignored() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    let b = block(&mut d, 128.0, 192.0);
    d.selection.solids.insert(a);
    d.selection.solids.insert(b);
    let group = d.group_selection().expect("two things make a group");
    assert_eq!(d.map.group_members(group).len(), 2);

    d.selection.clear();
    d.selection.solids.insert(a);
    d.expand_selection_groups();
    assert_eq!(d.selection.solids.len(), 2, "the whole group came along");

    d.ignore_groups = true;
    d.selection.clear();
    d.selection.solids.insert(a);
    d.expand_selection_groups();
    assert_eq!(d.selection.solids.len(), 1);

    d.ignore_groups = false;
    assert_eq!(d.ungroup_selection(), 1);
    assert_eq!(d.map.groups.len(), 1, "b is still in it");
    d.selection.solids.insert(b);
    assert_eq!(d.ungroup_selection(), 1);
    assert!(d.map.groups.is_empty(), "a group nobody is in is gone");
}

#[test]
fn the_cordon_hides_what_is_outside_and_can_be_resized() {
    let mut d = doc();
    let inside = block(&mut d, 0.0, 64.0);
    let outside = block(&mut d, 512.0, 576.0);
    d.selection.clear();
    d.selection.solids.insert(inside);
    d.set_cordon_active(true);
    assert!(d.cordon_active());
    assert!(d.is_visible(ObjectId::Solid(inside)));
    assert!(!d.is_visible(ObjectId::Solid(outside)));

    d.set_cordon_bounds(Aabb::new(Vec3::splat(-16.0), Vec3::splat(1024.0)));
    assert!(d.is_visible(ObjectId::Solid(outside)));
    d.set_cordon_active(false);
    assert!(d.map.cordon.is_some(), "turning it off keeps the box");
    let again = Map::parse(&d.map.to_text()).unwrap();
    assert_eq!(again.cordon, d.map.cordon);
}

#[test]
fn deleting_a_visgroup_leaves_its_members_visible() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 64.0);
    d.selection.solids.insert(a);
    let vg = d.new_visgroup_from_selection("v");
    d.set_visgroup_visible(vg, false);
    assert!(d.remove_visgroup(vg));
    assert!(d.is_visible(ObjectId::Solid(a)));
    assert!(d.map.find_solid(a).unwrap().editor.visgroups.is_empty());
}

// ---- clip, carve, hollow, transform ---------------------------------------

#[test]
fn clipping_the_selection_replaces_it_with_the_pieces() {
    let mut d = doc();
    let id = block(&mut d, 0.0, 64.0);
    d.map.find_solid_mut(id).unwrap().set("detail", "1");
    let plane = Plane::new(Vec3::X, 32.0);
    assert_eq!(d.clip_selection(plane, ClipMode::Both), 2);
    assert_eq!(d.map.world.solids.len(), 2);
    assert!(d.map.find_solid(id).is_none(), "the original is gone");
    assert_eq!(d.selection.solids.len(), 2, "the pieces are selected");
    assert!(
        d.map
            .world
            .solids
            .iter()
            .all(|s| s.get("detail") == Some("1")),
        "keys carry over"
    );
    d.undo();
    assert!(d.map.find_solid(id).is_some());

    d.selection.solids.insert(id);
    assert_eq!(d.clip_selection(plane, ClipMode::Front), 1);
    assert_eq!(d.map.world.solids[0].bounds().min.x, 32.0);
}

#[test]
fn clipping_a_brush_entity_keeps_the_pieces_in_it() {
    let mut d = doc();
    let id = block(&mut d, 0.0, 64.0);
    d.selection.solids.insert(id);
    d.set_brush_class(Some("func_door"));
    let door = d.selection.entities.iter().copied().next().unwrap();
    assert_eq!(
        d.clip_selection(Plane::new(Vec3::Z, 32.0), ClipMode::Both),
        2
    );
    let e = d.find_entity(door).unwrap();
    assert_eq!(e.solids.len(), 2);
    assert!(d.selection.entities.contains(&door));
}

#[test]
fn carving_cuts_a_hole_and_removes_the_carver() {
    let mut d = doc();
    let wall = block(&mut d, 0.0, 128.0);
    let hole = d.create_block(Vec3::new(32.0, -16.0, 32.0), Vec3::new(96.0, 144.0, 96.0));
    d.selection.clear();
    d.selection.solids.insert(hole);
    assert_eq!(d.carve_selection(), 1);
    assert!(d.map.find_solid(hole).is_none());
    assert!(d.map.find_solid(wall).is_none());
    let pieces = &d.map.world.solids;
    assert_eq!(
        pieces.len(),
        4,
        "a doorway through a block leaves four pieces"
    );
    let volume: f32 = pieces.iter().map(Solid::volume).sum();
    assert!((volume - (128f32.powi(3) - 64.0 * 128.0 * 64.0)).abs() < 1.0);
    assert!(d.selection.is_empty());
}

#[test]
fn hidden_brushes_are_not_carved() {
    let mut d = doc();
    let wall = block(&mut d, 0.0, 128.0);
    let hole = d.create_block(Vec3::splat(32.0), Vec3::splat(96.0));
    d.selection.clear();
    d.selection.solids.insert(wall);
    d.hide_selection();
    d.selection.solids.insert(hole);
    assert_eq!(d.carve_selection(), 0);
    assert!(d.map.find_solid(wall).is_some());
}

#[test]
fn hollowing_makes_walls_of_the_asked_thickness() {
    let mut d = doc();
    let id = block(&mut d, 0.0, 128.0);
    assert_eq!(d.hollow_selection(16.0), 6);
    assert!(d.map.find_solid(id).is_none());
    assert_eq!(d.map.world.solids.len(), 6);
    assert!(
        !d.map
            .world
            .solids
            .iter()
            .any(|s| s.contains_point(Vec3::splat(64.0)))
    );
}

#[test]
fn rotating_turns_brushes_and_entities_about_the_pivot() {
    let mut d = doc();
    let id = d.create_block(Vec3::new(64.0, 0.0, 0.0), Vec3::new(128.0, 64.0, 64.0));
    let light = d.create_entity("light", Vec3::new(96.0, 32.0, 32.0));
    d.find_entity_mut(light).unwrap().set("angles", "0 0 0");
    d.selection.solids.insert(id);
    d.selection.entities.insert(light);
    d.rotate_selection(Vec3::ZERO, Quat::from_rotation_z(90f32.to_radians()));
    let b = d.map.find_solid(id).unwrap().bounds();
    assert!(
        (b.min - Vec3::new(-64.0, 64.0, 0.0)).length() < 1e-2,
        "{b:?}"
    );
    let e = d.find_entity(light).unwrap();
    assert!((e.origin() - Vec3::new(-32.0, 96.0, 32.0)).length() < 1e-2);
    assert!((e.angles().yaw - 90.0).abs() < 1e-3);
}

#[test]
fn flipping_mirrors_about_the_selection_centre() {
    let mut d = doc();
    let a = block(&mut d, 0.0, 32.0);
    let b = d.create_block(Vec3::new(64.0, 0.0, 0.0), Vec3::new(128.0, 32.0, 32.0));
    d.selection.solids.insert(a);
    d.selection.solids.insert(b);
    d.flip_selection(0);
    let a = d.map.find_solid(a).unwrap().bounds();
    let b = d.map.find_solid(b).unwrap().bounds();
    assert_eq!(a.min.x, 96.0, "the small one is on the right now");
    assert_eq!(b.min.x, 0.0);
    assert!(d.map.world.solids.iter().all(|s| s.validate().is_ok()));
}

#[test]
fn aligning_to_the_grid_moves_the_whole_selection_together() {
    let mut d = doc();
    d.grid.snap = false;
    let a = d.create_block(Vec3::splat(3.0), Vec3::splat(19.0));
    let b = d.create_block(Vec3::new(35.0, 3.0, 3.0), Vec3::new(51.0, 19.0, 19.0));
    d.selection.solids.insert(a);
    d.selection.solids.insert(b);
    d.grid.size = 16.0;
    let delta = d.align_selection_to_grid();
    assert_eq!(delta, Vec3::splat(-3.0));
    assert_eq!(d.map.find_solid(a).unwrap().bounds().min, Vec3::ZERO);
    assert_eq!(d.map.find_solid(b).unwrap().bounds().min.x, 32.0);
}

// ---- meshes -------------------------------------------------------------------

#[test]
fn converting_a_brush_makes_a_selected_mesh_of_the_same_shape_and_undoes() {
    let mut d = doc();
    let id = block(&mut d, 0.0, 64.0);
    let material = d.map.world.solids[0].sides[0].material.clone();

    assert_eq!(d.convert_selection_to_meshes(), 1);
    assert!(d.map.world.solids.is_empty());
    assert_eq!(d.map.world.meshes.len(), 1);
    let mesh = &d.map.world.meshes[0];
    assert_eq!(mesh.vertices.len(), 8);
    assert_eq!(mesh.faces.len(), 6);
    assert_eq!(mesh.bounds().min, Vec3::ZERO);
    assert_eq!(mesh.bounds().max, Vec3::splat(64.0));
    assert!(mesh.faces.iter().all(|f| f.material == material));
    assert!(d.selection.meshes.contains(&mesh.id));
    assert!(d.selection.solids.is_empty());
    // Fresh ids, still unique across the map.
    assert!(d.problems().is_empty(), "{:?}", d.problems());
    assert!(Map::parse(&d.map.to_text()).is_ok());

    d.undo();
    assert_eq!(d.map.world.solids.len(), 1);
    assert_eq!(d.map.world.solids[0].id, id);
    assert!(d.map.world.meshes.is_empty());
}

#[test]
fn a_mesh_moves_duplicates_resizes_and_deletes_with_the_selection() {
    let mut d = doc();
    d.grid.size = 16.0;
    block(&mut d, 0.0, 64.0);
    d.convert_selection_to_meshes();
    let original = d.map.world.meshes[0].id;

    d.move_selection(Vec3::new(64.0, 0.0, 0.0));
    assert_eq!(
        d.map.world.meshes[0].bounds().min,
        Vec3::new(64.0, 0.0, 0.0)
    );
    assert_eq!(d.selection_bounds().unwrap().min, Vec3::new(64.0, 0.0, 0.0));

    assert_eq!(d.duplicate_selection(Vec3::new(0.0, 128.0, 0.0)), 1);
    assert_eq!(d.map.world.meshes.len(), 2);
    let copy = d.map.world.meshes[1].id;
    assert_ne!(copy, original);
    assert!(d.selection.meshes.contains(&copy));
    assert!(d.problems().is_empty(), "{:?}", d.problems());

    d.scale_selection(Vec3::new(64.0, 128.0, 0.0), Vec3::new(2.0, 1.0, 1.0));
    let b = d.map.world.meshes[1].bounds();
    assert_eq!(b.size().x, 128.0);

    assert_eq!(d.delete_selection(), 1);
    assert_eq!(d.map.world.meshes.len(), 1);
    assert_eq!(d.map.world.meshes[0].id, original);
}

#[test]
fn select_all_and_hiding_include_meshes() {
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    d.convert_selection_to_meshes();
    let mesh = d.map.world.meshes[0].id;
    d.selection.clear();
    d.select_all();
    assert!(d.selection.meshes.contains(&mesh));

    d.hide_selection();
    assert!(!d.is_visible(kerosene_map::ObjectId::Mesh(mesh)));
    assert_eq!(d.visible_meshes().count(), 0);
}
