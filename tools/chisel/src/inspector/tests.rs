// SPDX-License-Identifier: MPL-2.0
use super::*;
use kerosene_map::Connection;

fn schema() -> Schema {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    crate::classes::load(&root).schema
}

fn door() -> Entity { Entity::new(7, "func_door") }

#[test]
fn a_freshly_placed_entity_still_shows_every_key_its_class_reads() {
    // The bug this replaces: a new func_door carries only `classname`, so the
    // inspector had nothing to list and the class looked like it had no
    // settings at all.
    let schema = schema();
    let rows = rows(schema.get("func_door"), &door());
    let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    for expected in ["targetname", "spawnflags", "movedir", "speed", "lip", "locked"] {
        assert!(keys.contains(&expected), "{expected} missing from {keys:?}");
    }
    assert!(rows.iter().all(|r| !r.is_set()), "nothing is set on a fresh entity");
}

#[test]
fn an_unset_key_offers_the_games_default_without_writing_it() {
    let schema = schema();
    let rows = rows(schema.get("func_door"), &door());
    let speed = rows.iter().find(|r| r.key == "speed").unwrap();
    assert_eq!(speed.text(), "100", "the default is what the game would use");
    assert_eq!(speed.value, None, "but the entity has not been given one");
    assert_eq!(speed.kind, KeyKind::Float);
}

#[test]
fn a_set_key_shows_its_value_rather_than_the_default() {
    let schema = schema();
    let mut entity = door();
    entity.set("speed", "250");
    let rows = rows(schema.get("func_door"), &entity);
    let speed = rows.iter().find(|r| r.key == "speed").unwrap();
    assert_eq!(speed.text(), "250");
    assert!(speed.is_set());
}

#[test]
fn class_keys_come_before_the_ones_only_this_entity_has() {
    let schema = schema();
    let mut entity = door();
    entity.set("some_mod_key", "value");
    let rows = rows(schema.get("func_door"), &entity);
    let described: Vec<&str> =
        rows.iter().filter(|r| r.described).map(|r| r.key.as_str()).collect();
    assert_eq!(described.first(), Some(&"targetname"), "inherited keys lead");
    assert_eq!(rows.last().unwrap().key, "some_mod_key");
    assert!(!rows.last().unwrap().described);
}

#[test]
fn a_key_the_schema_does_not_know_is_kept_rather_than_dropped() {
    let schema = schema();
    let mut entity = door();
    entity.set("speeed", "100"); // a typo, and the only way to notice is to see it
    let rows = rows(schema.get("func_door"), &entity);
    let typo = rows.iter().find(|r| r.key == "speeed").expect("kept");
    assert!(!typo.described);
    assert_eq!(typo.value.as_deref(), Some("100"));
}

#[test]
fn classname_is_never_an_editable_row() {
    let schema = schema();
    let rows = rows(schema.get("func_door"), &door());
    assert!(rows.iter().all(|r| r.key != "classname"));
}

#[test]
fn an_entity_of_an_unknown_class_still_shows_what_it_carries() {
    let mut entity = Entity::new(9, "prop_from_another_mod");
    entity.set("model", "props/thing");
    let rows = rows(None, &entity);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, "model");
    assert!(!rows[0].described);
}

#[test]
fn applying_rows_writes_set_keys_and_removes_unset_ones() {
    let schema = schema();
    let mut entity = door();
    entity.set("lip", "16");

    let mut rows = rows(schema.get("func_door"), &entity);
    rows.iter_mut().find(|r| r.key == "speed").unwrap().value = Some("250".into());
    rows.iter_mut().find(|r| r.key == "lip").unwrap().value = None;
    apply(&mut entity, &rows);

    assert_eq!(entity.get("speed"), Some("250"));
    assert_eq!(entity.get("lip"), None, "clearing a row removes the key rather than writing the default");
    assert_eq!(entity.classname(), "func_door", "identity survives");
}

#[test]
fn applying_does_not_write_defaults_into_the_map() {
    let schema = schema();
    let mut entity = door();
    let rows = rows(schema.get("func_door"), &entity);
    apply(&mut entity, &rows);
    let keys: Vec<&str> = entity.properties.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["classname"], "an untouched entity gains nothing");
}

#[test]
fn choices_and_flags_carry_their_labels() {
    let schema = schema();
    let rows = rows(schema.get("func_door"), &door());
    let flags = rows.iter().find(|r| r.key == "spawnflags").unwrap();
    assert_eq!(flags.kind, KeyKind::Flags);
    assert_eq!(flags.choices, vec![("1".to_string(), "Starts open".to_string())]);
}

#[test]
fn target_names_are_offered_from_the_map() {
    let mut document = Document::new();
    let mut a = Entity::new(10, "func_door");
    a.set("targetname", "gate");
    let mut b = Entity::new(11, "func_door");
    b.set("targetname", "gate"); // a shared name: one output drives both
    let c = Entity::new(12, "func_door"); // unnamed, so not addressable
    document.map.entities.extend([a, b, c]);

    assert_eq!(target_names(&document), vec!["gate"]);
}

#[test]
fn the_inputs_offered_are_the_ones_the_target_actually_accepts() {
    let schema = schema();
    let mut document = Document::new();
    let mut counter = Entity::new(10, "math_counter");
    counter.set("targetname", "score");
    document.map.entities.push(counter);

    let inputs = inputs_for_target(&schema, &document, "score");
    assert!(inputs.iter().any(|i| i == "Add"));
    assert!(inputs.iter().any(|i| i == "SetValue"));
    assert!(inputs.iter().any(|i| i == "Kill"), "the universal inputs are offered too");
    assert!(!inputs.iter().any(|i| i == "Open"), "a counter is not a door");
}

#[test]
fn several_entities_sharing_a_name_offer_the_union_of_their_inputs() {
    let schema = schema();
    let mut document = Document::new();
    let mut door = Entity::new(10, "func_door");
    door.set("targetname", "both");
    let mut counter = Entity::new(11, "math_counter");
    counter.set("targetname", "both");
    document.map.entities.extend([door, counter]);

    let inputs = inputs_for_target(&schema, &document, "both");
    assert!(inputs.iter().any(|i| i == "Open"));
    assert!(inputs.iter().any(|i| i == "Add"));
}

#[test]
fn an_unknown_target_offers_nothing_rather_than_guessing() {
    let schema = schema();
    let document = Document::new();
    assert!(inputs_for_target(&schema, &document, "!activator").is_empty());
    assert!(inputs_for_target(&schema, &document, "nothing_called_this").is_empty());
}

#[test]
fn connections_survive_a_round_trip_through_their_text_form() {
    let c = Connection::new("OnFullyOpen", "relay", "Trigger").with_delay(0.5);
    let back = Connection::parse("OnFullyOpen", &c.to_value()).unwrap();
    assert_eq!(back.target, "relay");
    assert_eq!(back.input, "Trigger");
    assert!((back.delay - 0.5).abs() < 1e-6);
}

#[test]
fn colours_split_into_rgb_and_a_brightness_that_can_exceed_white() {
    assert_eq!(parse_color("255 240 220 300"), ([255, 240, 220], 300.0));
    assert_eq!(format_color([255, 240, 220], 300.0), "255 240 220 300");
    // Missing pieces fall back rather than failing: half-typed values are
    // normal while someone is editing.
    assert_eq!(parse_color("128 128 128"), ([128, 128, 128], 200.0));
    assert_eq!(parse_color(""), ([255, 255, 255], 200.0));
    // A brightness above 255 is not a colour channel and must not be clamped.
    let (_, bright) = parse_color("255 255 255 1000");
    assert_eq!(bright, 1000.0);
}

#[test]
fn vectors_parse_however_they_were_written() {
    assert_eq!(parse_vec3("1 2 3"), [1.0, 2.0, 3.0]);
    assert_eq!(parse_vec3("[1 2 3]"), [1.0, 2.0, 3.0]);
    assert_eq!(parse_vec3("1, 2, 3"), [1.0, 2.0, 3.0]);
    assert_eq!(parse_vec3("(1 2 3)"), [1.0, 2.0, 3.0]);
    assert_eq!(parse_vec3("nonsense"), [0.0, 0.0, 0.0]);
    assert_eq!(format_vec3([1.0, -2.5, 0.0]), "1 -2.5 0");
}
