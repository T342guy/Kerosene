// SPDX-License-Identifier: MPL-2.0
//! The shipped class schema must describe the game that is actually here.
//!
//! `content/kerosene.kerodef` is what Chisel shows in its property
//! inspector. Nothing at runtime reads it, so without a test it would rot the
//! first time someone added an input -- and the failure mode is miserable: a
//! designer wires up something the editor offered and the map silently does
//! nothing.
//!
//! So both directions are checked. Every class, input and output the game
//! registers must be offered by the schema, and everything the schema offers
//! must exist in the game.

use std::collections::BTreeSet;
use kerosene_entity::{ClassKind, Schema};

fn schema() -> Schema {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../content/kerosene.kerodef");
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("the shipped schema must be readable at {path}: {e}"));
    Schema::parse(&text).expect("the shipped schema must parse")
}

#[test]
fn every_registered_class_is_described() {
    let schema = schema();
    let registry = kerosene_game::registry();
    let missing: Vec<&str> = registry
        .class_names()
        .into_iter()
        .filter(|name| schema.get(name).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "these classes exist in the game but not in content/kerosene.kerodef, \
         so Chisel would show no properties for them: {missing:?}"
    );
}

#[test]
fn every_described_class_exists() {
    let schema = schema();
    let registry = kerosene_game::registry();
    let unknown: Vec<&str> = schema
        .classes()
        .iter()
        .map(|c| c.name.as_str())
        .filter(|name| !registry.is_registered(name))
        .collect();
    assert!(
        unknown.is_empty(),
        "the schema offers classes the game does not implement, so placing one \
         would produce an entity that does nothing: {unknown:?}"
    );
}

#[test]
fn every_input_the_game_handles_is_offered() {
    let schema = schema();
    let registry = kerosene_game::registry();
    let common: BTreeSet<&str> = registry.common_inputs().into_iter().collect();

    let mut missing = Vec::new();
    for name in registry.class_names() {
        let Some(spec) = schema.get(name) else { continue };
        let def = registry.get(name).expect("just listed");
        for (input, _) in &def.inputs {
            if !spec.has_input(input) {
                missing.push(format!("{name}.{input}"));
            }
        }
        // The universal inputs have to reach every class, which in the schema
        // means every class inherits the base that carries them.
        for input in &common {
            if !spec.has_input(input) {
                missing.push(format!("{name}.{input} (common)"));
            }
        }
    }
    assert!(missing.is_empty(), "inputs the game handles but the schema does not offer: {missing:#?}");
}

#[test]
fn every_input_the_schema_offers_is_handled() {
    let schema = schema();
    let registry = kerosene_game::registry();
    let mut unknown = Vec::new();
    for spec in schema.classes() {
        for input in &spec.inputs {
            if registry.find_input(&spec.name, &input.name).is_none() {
                unknown.push(format!("{}.{}", spec.name, input.name));
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "the schema offers inputs nothing handles, so wiring one up would do nothing: {unknown:#?}"
    );
}

#[test]
fn outputs_agree_in_both_directions() {
    let schema = schema();
    let registry = kerosene_game::registry();
    let common: BTreeSet<&str> = registry.common_outputs().into_iter().collect();

    let mut missing = Vec::new();
    for name in registry.class_names() {
        let Some(spec) = schema.get(name) else { continue };
        let def = registry.get(name).expect("just listed");
        for output in def.outputs.iter().copied().chain(common.iter().copied()) {
            if !spec.has_output(output) {
                missing.push(format!("{name}.{output}"));
            }
        }
    }
    assert!(missing.is_empty(), "outputs the game fires but the schema does not offer: {missing:#?}");

    let mut unknown = Vec::new();
    for spec in schema.classes() {
        let Some(def) = registry.get(&spec.name) else { continue };
        for output in &spec.outputs {
            let known = def.outputs.iter().any(|o| o.eq_ignore_ascii_case(&output.name))
                || common.iter().any(|o| o.eq_ignore_ascii_case(&output.name));
            if !known {
                unknown.push(format!("{}.{}", spec.name, output.name));
            }
        }
    }
    assert!(unknown.is_empty(), "the schema offers outputs the game never fires: {unknown:#?}");
}

#[test]
fn brush_classes_are_marked_as_such() {
    let schema = schema();
    // A class tied to brushes that the schema calls a point entity would be
    // offered in the wrong menu and refuse the brushes it needs.
    for name in ["func_door", "func_brush", "func_detail", "trigger_multiple", "trigger_once"] {
        let spec = schema.get(name).unwrap_or_else(|| panic!("{name} is in the schema"));
        assert!(spec.kind.takes_brushes(), "{name} must be a brush class, not {:?}", spec.kind);
    }
    for name in ["info_player_start", "light", "light_spot", "logic_relay", "math_counter"] {
        let spec = schema.get(name).unwrap_or_else(|| panic!("{name} is in the schema"));
        assert_eq!(spec.kind, ClassKind::Point, "{name} must be a point class");
    }
}

#[test]
fn every_class_has_help_text() {
    let schema = schema();
    let silent: Vec<&str> = schema
        .classes()
        .iter()
        .filter(|c| c.help.trim().is_empty())
        .map(|c| c.name.as_str())
        .collect();
    assert!(silent.is_empty(), "a class with no help is a class nobody can use: {silent:?}");
}

#[test]
fn the_keys_the_game_reads_are_all_offered() {
    // Spot-check the ones with real behaviour behind them. This is the list a
    // designer would otherwise have to learn from the source.
    let schema = schema();
    for (class, keys) in [
        ("func_door", &["speed", "lip", "movedir", "locked", "spawnflags"][..]),
        ("func_brush", &["startdisabled"][..]),
        ("trigger_multiple", &["startdisabled"][..]),
        ("trigger_hurt", &["damage"][..]),
        ("logic_relay", &["startdisabled", "spawnflags"][..]),
        ("logic_timer", &["refiretime", "startdisabled"][..]),
        ("math_counter", &["startvalue", "min", "max"][..]),
        ("point_message", &["message"][..]),
        ("light", &["_light", "_constant_attn", "_linear_attn", "_quadratic_attn"][..]),
        ("light_spot", &["_light", "_cone", "_inner_cone", "_exponent", "pitch"][..]),
        ("light_environment", &["_light", "_ambient", "pitch"][..]),
        ("worldspawn", &["skyname"][..]),
        ("prop_static", &["model"][..]),
    ] {
        let spec = schema.get(class).unwrap_or_else(|| panic!("{class} is in the schema"));
        for key in keys {
            assert!(spec.key(key).is_some(), "{class} reads `{key}` but the schema does not offer it");
        }
    }
}
