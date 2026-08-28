// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;
use void_math::Vec3;

/// A document with one box in it, selected.
fn one_brush(material: &str) -> Document {
    let mut document = Document::new();
    document.map.world.solids.clear();
    document.current_material = material.to_string();
    let id = document.create_block(Vec3::ZERO, Vec3::new(128.0, 96.0, 64.0));
    document.selection.clear();
    document.selection.solids.insert(id);
    document
}

#[test]
fn nothing_selected_describes_nothing() {
    let mut document = one_brush("dev/grid");
    document.selection.clear();
    assert!(BrushInfo::of_selection(&document).is_none());
}

#[test]
fn a_brush_reports_its_size_and_its_faces() {
    let info = BrushInfo::of_selection(&one_brush("dev/grid")).unwrap();
    assert_eq!(info.brushes, 1);
    assert_eq!(info.faces, 6);
    assert_eq!(info.bounds.size(), Vec3::new(128.0, 96.0, 64.0));
    assert_eq!(info.materials, vec!["dev/grid"]);
    assert_eq!(info.compiles_as, "solid");
    assert!(!info.mixed_materials);
}

#[test]
fn a_tool_brush_says_what_the_tool_does() {
    // The whole point: "the tool textures do not do anything" was a report
    // about the editor never saying what they do.
    let info = BrushInfo::of_selection(&one_brush("tools/clip")).unwrap();
    assert_eq!(info.compiles_as, "blocks players");

    let meanings = info.material_meanings();
    assert_eq!(meanings.len(), 1);
    assert_eq!(meanings[0].0, "tools/clip");
    assert!(meanings[0].1.contains("players"), "{}", meanings[0].1);
}

#[test]
fn what_it_says_is_what_the_compiler_will_do() {
    // Read from Cleave's table rather than from a copy, so the two cannot
    // drift apart.
    for material in ["dev/grid", "tools/clip", "tools/trigger", "tools/nodraw", "tools/water"] {
        let info = BrushInfo::of_selection(&one_brush(material)).unwrap();
        assert_eq!(
            info.compiles_as,
            cleave::material::describe_brush(&[material.to_string()], None),
            "{material}"
        );
    }
}

#[test]
fn a_brush_with_one_tool_face_reports_what_that_does_to_the_whole_brush() {
    // Solid is the absence of anything more specific, so a single clip face
    // stops the brush being a wall. Nobody expects that, which is why it has
    // to be on screen.
    let mut document = one_brush("dev/grid");
    let id = *document.selection.solids.iter().next().unwrap();
    if let Some(solid) = document.map.find_solid_mut(id) {
        solid.sides[0].material = "tools/clip".into();
    }

    let info = BrushInfo::of_selection(&document).unwrap();
    assert!(info.mixed_materials, "the faces do not agree");
    assert_eq!(info.compiles_as, "blocks players");
    assert_eq!(info.materials.len(), 2);
}

#[test]
fn a_misspelt_tool_material_is_called_out() {
    // It compiles as world geometry rather than failing, which is how a
    // doorway gets walled off by a typo nobody sees.
    let info = BrushInfo::of_selection(&one_brush("tools/clipp")).unwrap();
    assert_eq!(info.unknown_tools(), vec!["tools/clipp"]);
    assert_eq!(info.compiles_as, "solid", "and it really will be a wall");
}

#[test]
fn an_ordinary_material_is_not_reported_as_an_unknown_tool() {
    let info = BrushInfo::of_selection(&one_brush("dev/grid")).unwrap();
    assert!(info.unknown_tools().is_empty());
}

#[test]
fn brushes_tied_to_a_class_are_described_by_it() {
    let mut document = one_brush("dev/door");
    document.tie_to_entity("func_door");
    // Tying selects the entity; select its brushes to ask about them.
    let ids: Vec<u32> = document.map.entities.iter().flat_map(|e| e.solids.iter().map(|s| s.id)).collect();
    document.selection.clear();
    for id in ids { document.selection.solids.insert(id); }

    let info = BrushInfo::of_selection(&document).unwrap();
    assert_eq!(info.classname.as_deref(), Some("func_door"));
    assert!(info.compiles_as.contains("moves"), "{}", info.compiles_as);
}

#[test]
fn a_world_brush_is_not_called_a_worldspawn() {
    // True, and useless: nobody thinks of a wall as an entity.
    let info = BrushInfo::of_selection(&one_brush("dev/grid")).unwrap();
    assert_eq!(info.classname, None);
}

#[test]
fn a_selection_spanning_two_classes_claims_neither() {
    let mut document = one_brush("dev/door");
    document.tie_to_entity("func_door");
    let tied: Vec<u32> = document.map.entities.iter().flat_map(|e| e.solids.iter().map(|s| s.id)).collect();

    let wall = document.create_block(Vec3::new(512.0, 0.0, 0.0), Vec3::new(640.0, 96.0, 64.0));
    document.selection.clear();
    document.selection.solids.insert(wall);
    for id in tied { document.selection.solids.insert(id); }

    let info = BrushInfo::of_selection(&document).unwrap();
    assert_eq!(info.brushes, 2);
    assert_eq!(info.classname, None, "no single answer, so no answer");
}

#[test]
fn several_brushes_are_summed_and_their_extent_is_the_whole_selection() {
    let mut document = one_brush("dev/grid");
    let first = *document.selection.solids.iter().next().unwrap();
    // `create_block` selects what it made, so the first has to be put back.
    let other = document.create_block(Vec3::new(256.0, 0.0, 0.0), Vec3::new(320.0, 96.0, 64.0));
    document.selection.solids.insert(first);
    document.selection.solids.insert(other);

    let info = BrushInfo::of_selection(&document).unwrap();
    assert_eq!(info.brushes, 2);
    assert_eq!(info.faces, 12);
    assert_eq!(info.bounds.min, Vec3::ZERO);
    assert_eq!(info.bounds.max, Vec3::new(320.0, 96.0, 64.0));
}

#[test]
fn every_material_is_listed_once_however_many_faces_wear_it() {
    let info = BrushInfo::of_selection(&one_brush("dev/grid")).unwrap();
    assert_eq!(info.materials.len(), 1, "six faces, one material");
}

#[test]
fn selecting_a_brush_entity_describes_its_brushes() {
    // Clicking a door selects the door. A panel that then said "nothing
    // selected" would be the same split this panel exists to remove.
    let mut document = one_brush("dev/door");
    document.set_brush_class(Some("func_door"));
    assert!(document.selection.solids.is_empty(), "the entity is what is selected");

    let info = BrushInfo::of_selection(&document).expect("the door is what we are looking at");
    assert_eq!(info.brushes, 1);
    assert_eq!(info.classname.as_deref(), Some("func_door"));
}

#[test]
fn a_trigger_reports_itself_as_a_region_rather_than_a_wall() {
    // Which is the whole reason the class textures its brushes for you.
    let mut document = one_brush("dev/grid");
    document.set_brush_class(Some("trigger_multiple"));

    let info = BrushInfo::of_selection(&document).unwrap();
    assert_eq!(info.materials, vec!["tools/trigger"]);
    assert!(info.compiles_as.starts_with("a trigger volume"), "{}", info.compiles_as);
}
