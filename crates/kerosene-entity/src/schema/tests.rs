// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;

const SAMPLE: &str = r#"
// A comment, because these files are written by hand.
base
{
    "name" "Targetname"
    key { "name" "targetname" "type" "target_source" "label" "Name"
          "help" "What other entities call this one." }
    input { "name" "Kill" "help" "Remove this entity." }
}

base
{
    "name" "Origin"
    key { "name" "origin" "type" "vec3" "default" "0 0 0" }
}

class
{
    "name" "func_door"
    "kind" "brush"
    "base" "Targetname"
    "help" "A brush that slides open and shut."
    key { "name" "speed" "type" "float" "default" "100" "help" "Units per second." }
    key { "name" "movedir" "type" "vec3" "default" "0 0 1" }
    input  { "name" "Open" }
    input  { "name" "SetSpeed" "parameter" "units per second" }
    output { "name" "OnFullyOpen" }
}

class
{
    "name" "light"
    "base" "Targetname"
    "base" "Origin"
    key { "name" "_light" "type" "color" "default" "255 255 255 200" }
    key {
        "name" "style"
        "type" "choices"
        "default" "0"
        choice { "value" "0" "label" "Normal" }
        choice { "value" "1" "label" "Flicker" }
    }
}
"#;

fn sample() -> Schema { Schema::parse(SAMPLE).expect("sample schema parses") }

#[test]
fn classes_come_back_in_file_order() {
    let schema = sample();
    let names: Vec<&str> = schema.classes().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["func_door", "light"]);
}

#[test]
fn lookup_ignores_case() {
    let schema = sample();
    assert!(schema.get("FUNC_DOOR").is_some());
    assert!(schema.get("func_door").is_some());
    assert!(schema.get("no_such_class").is_none());
}

#[test]
fn a_class_inherits_its_bases_keys_before_its_own() {
    let schema = sample();
    let door = schema.get("func_door").unwrap();
    let names: Vec<&str> = door.keys.iter().map(|k| k.name.as_str()).collect();
    // Inherited first: the common keys stay at the top of the inspector.
    assert_eq!(names, ["targetname", "speed", "movedir"]);
    assert!(door.has_input("Kill"), "inherited inputs come through too");
    assert!(door.has_input("Open"));
}

#[test]
fn several_bases_accumulate() {
    let schema = sample();
    let light = schema.get("light").unwrap();
    let names: Vec<&str> = light.keys.iter().map(|k| k.name.as_str()).collect();
    assert_eq!(names, ["targetname", "origin", "_light", "style"]);
}

#[test]
fn key_types_and_defaults_survive() {
    let schema = sample();
    let door = schema.get("func_door").unwrap();
    let speed = door.key("speed").unwrap();
    assert_eq!(speed.kind, KeyKind::Float);
    assert_eq!(speed.default, "100");
    assert_eq!(speed.help, "Units per second.");
    // A key with no label is labelled by its own name.
    assert_eq!(door.key("movedir").unwrap().label, "movedir");
    // ...and one with a label keeps it, through inheritance.
    assert_eq!(door.key("targetname").unwrap().label, "Name");
}

#[test]
fn choices_are_read_in_order() {
    let schema = sample();
    let style = schema.get("light").unwrap().key("style").unwrap();
    assert_eq!(style.kind, KeyKind::Choices);
    assert_eq!(
        style.choices,
        vec![("0".to_string(), "Normal".to_string()), ("1".to_string(), "Flicker".to_string())]
    );
}

#[test]
fn an_input_can_document_its_parameter() {
    let schema = sample();
    let door = schema.get("func_door").unwrap();
    let set_speed = door.inputs.iter().find(|i| i.name == "SetSpeed").unwrap();
    assert_eq!(set_speed.parameter.as_deref(), Some("units per second"));
    assert_eq!(door.inputs.iter().find(|i| i.name == "Open").unwrap().parameter, None);
}

#[test]
fn kinds_decide_what_can_be_tied_to_brushes() {
    let schema = sample();
    assert_eq!(schema.get("func_door").unwrap().kind, ClassKind::Brush);
    // Unstated kind means a point entity, which is the common case.
    assert_eq!(schema.get("light").unwrap().kind, ClassKind::Point);
    assert!(ClassKind::Brush.takes_brushes());
    assert!(ClassKind::Any.takes_brushes());
    assert!(!ClassKind::Point.takes_brushes());
    assert_eq!(schema.names_of_kind(ClassKind::Point), ["light"]);
    assert_eq!(schema.names_of_kind(ClassKind::Brush), ["func_door"]);
}

#[test]
fn a_class_overrides_a_key_it_inherits() {
    let schema = Schema::parse(
        r#"
        base  { "name" "B" key { "name" "speed" "type" "float" "default" "100" } }
        class { "name" "c" "base" "B" key { "name" "speed" "type" "int" "default" "5" } }
        "#,
    )
    .unwrap();
    let keys = &schema.get("c").unwrap().keys;
    assert_eq!(keys.len(), 1, "the override replaces rather than duplicates");
    assert_eq!(keys[0].kind, KeyKind::Integer);
    assert_eq!(keys[0].default, "5");
}

#[test]
fn merging_lets_a_later_file_replace_a_class() {
    let mut schema = sample();
    let extra = Schema::parse(r#"class { "name" "func_door" "help" "mine now" }"#).unwrap();
    schema.merge(extra);
    assert_eq!(schema.len(), 2, "replacing a class does not add one");
    assert_eq!(schema.get("func_door").unwrap().help, "mine now");
}

#[test]
fn a_missing_base_is_an_error_rather_than_a_silent_gap() {
    let err = Schema::parse(r#"class { "name" "c" "base" "Nope" }"#).unwrap_err();
    assert!(matches!(err, SchemaError::UnknownBase { .. }), "{err}");
}

#[test]
fn an_unknown_key_type_is_rejected() {
    let err = Schema::parse(r#"class { "name" "c" key { "name" "k" "type" "wat" } }"#).unwrap_err();
    assert!(matches!(err, SchemaError::UnknownKeyType(_)), "{err}");
}

#[test]
fn a_nameless_class_is_rejected() {
    let err = Schema::parse(r#"class { "help" "no name" }"#).unwrap_err();
    assert!(matches!(err, SchemaError::Unnamed { .. }), "{err}");
}

#[test]
fn an_empty_file_is_an_empty_schema_rather_than_an_error() {
    let schema = Schema::parse("// nothing here\n").unwrap();
    assert!(schema.is_empty());
}
