// SPDX-License-Identifier: LGPL-3.0-or-later
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
fn a_document_round_trips_through_a_file() {
    let mut d = doc();
    block(&mut d, 0.0, 64.0);
    d.create_entity("info_player_start", Vec3::new(32.0, 32.0, 16.0));

    let path = std::env::temp_dir().join(format!("chisel-test-{}.voidmap", std::process::id()));
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
    assert_eq!(d.title(), "untitled.voidmap");
    block(&mut d, 0.0, 64.0);
    assert!(d.title().ends_with('*'), "{}", d.title());
}
