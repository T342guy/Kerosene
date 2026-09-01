// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

fn doc() -> Document { Document::new() }

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
        d.create_block(Vec3::splat(i as f32 * 64.0), Vec3::splat(i as f32 * 64.0 + 32.0));
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
    assert_eq!(d.map.to_text(), before, "undoing everything should restore the empty map");
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
    assert_eq!(d.undo_depth(), depth, "a no-op must not fill the undo stack");
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
    assert_eq!(d.find_entity(id).unwrap().origin(), Vec3::new(64.0, 0.0, 16.0));
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
    assert!(d.find_solid(id).unwrap().sides.iter().all(|s| s.material == "dev/wall"));
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
    assert!(d.find_solid(id).unwrap().sides.iter().all(|s| s.walkmap == WalkmapRule::Deny));
}

#[test]
fn applying_a_material_to_a_selected_brush_entity_retextures_it() {
    // Whole-brush selection of a door selects the *entity*, and applying must
    // reach its brushes -- otherwise retexturing a door silently does nothing.
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    let entity = d.tie_to_entity("func_door").unwrap();
    assert_eq!(d.map.world.solids.len(), 0, "the brush should have left the world");

    // `tie_to_entity` selects the new entity.
    assert_eq!(d.selection.entities.len(), 1);
    d.current_material = "dev/wall".into();
    assert_eq!(d.apply_material(), 6);

    let door = d.find_entity(entity).unwrap();
    assert!(door.solids.iter().all(|s| s.sides.iter().all(|x| x.material == "dev/wall")));
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
    assert!(solid.sides[1..].iter().all(|s| s.walkmap == WalkmapRule::Allow));
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
    assert!(d.find_solid(a).unwrap().sides.iter().all(|s| s.walkmap == WalkmapRule::Always));
    assert!(d.find_solid(b).unwrap().sides.iter().all(|s| s.walkmap == WalkmapRule::Always));
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

    let entity = d.tie_to_entity("func_door").expect("should create an entity");
    assert!(d.map.world.solids.is_empty(), "the brushes should have left the world");
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
    assert!(d.find_entity(entity).is_none(), "the emptied entity should be gone");
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
    let bounds = d.selection_bounds().expect("a point entity needs something to drag");
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
    assert!(d.resizable_bounds().is_none(), "a door is configured, not stretched");
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
    let sides: Vec<u32> =
        document.find_solid(a).unwrap().sides.iter().map(|s| s.id).collect();
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
        document.find_solid(a).unwrap().sides.iter().all(|s| s.uaxis.offset == 8.0),
        "not every selected face moved"
    );
    assert!(
        document.find_solid(b).unwrap().sides.iter().all(|s| s.uaxis.offset == 0.0),
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
        document.find_solid(a).unwrap().sides.iter().all(|s| s.uaxis.offset == 0.0),
        "one undo did not take the whole edit back"
    );
}

#[test]
fn editing_with_nothing_selected_does_nothing_and_costs_no_undo() {
    let mut document = Document::new();
    document.create_block(Vec3::ZERO, Vec3::splat(64.0));
    document.selection.clear();
    let before = document.undo_depth();

    assert_eq!(document.edit_faces("shift", |side, _, _| side.uaxis.offset = 99.0), 0);
    assert_eq!(document.undo_depth(), before, "an empty edit pushed an undo step");
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
    assert!((max.0 - min.0 - 256.0).abs() < 1e-1, "u span {}", max.0 - min.0);
    assert!((max.1 - min.1 - 256.0).abs() < 1e-1, "v span {}", max.1 - min.1);
}

#[test]
fn the_selected_faces_come_back_in_a_stable_order() {
    // A panel showing "the first selected face" has to show the same one from
    // frame to frame.
    let (document, _, _) = two_cubes_with_one_selected();
    let first: Vec<(u32, u32)> =
        document.selected_face_specs().iter().map(|f| (f.solid, f.side.id)).collect();
    let again: Vec<(u32, u32)> =
        document.selected_face_specs().iter().map(|f| (f.solid, f.side.id)).collect();
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
    assert_eq!(d.selected_brush_class().map(|(_, c)| c), Some("func_door".into()));
    assert_eq!(d.selection.entities.len(), 1, "and the entity is what is selected");
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
    assert_eq!(d.selected_brush_class().map(|(_, c)| c), Some("func_door".into()));

    d.undo();
    assert_eq!(d.selected_brush_class().map(|(_, c)| c), Some("trigger_multiple".into()));
}

#[test]
fn changing_the_class_keeps_the_name_and_the_wiring() {
    // Dropping a targetname silently breaks every output wired to it.
    let (mut d, _) = one_selected_brush();
    d.set_brush_class(Some("trigger_multiple"));
    let id = d.selected_brush_class().unwrap().0;
    if let Some(e) = d.find_entity_mut(id) {
        e.set("targetname", "gate_trigger");
        e.connections.push(kerosene_map::Connection::new("OnStartTouch", "gate", "Open"));
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
        entity.solids[0].sides.iter().all(|s| s.material == "tools/trigger"),
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
    assert!(entity.solids[0].sides.iter().all(|s| s.material == "dev/door"));
}

#[test]
fn selecting_a_brush_entity_is_the_same_as_selecting_its_brushes() {
    // Clicking a door selects the door, and "what am I editing" has to mean
    // the same thing either way.
    let (mut d, _) = one_selected_brush();
    d.set_brush_class(Some("func_door"));
    let by_entity = d.selected_solid_ids();

    let entity_id = d.selected_brush_class().unwrap().0;
    let solids: Vec<u32> = d.find_entity(entity_id).unwrap().solids.iter().map(|s| s.id).collect();
    d.selection.clear();
    for id in &solids { d.selection.solids.insert(*id); }

    assert_eq!(d.selected_solid_ids(), by_entity);
    assert_eq!(d.selected_brush_class().map(|(_, c)| c), Some("func_door".into()));
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
    for id in first { d.selection.solids.insert(id); }
    for e in &d.map.entities {
        if e.classname() == "trigger_once" {
            for s in &e.solids { d.selection.solids.insert(s.id); }
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
