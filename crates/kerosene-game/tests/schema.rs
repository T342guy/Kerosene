// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The shipped class schema must describe the game that is actually here.
//!
//! `kerosene_game::schema::BUILTIN` is what Chisel shows in its property
//! inspector. Nothing at runtime reads it, so without a test it would rot the
//! first time someone added an input -- and the failure mode is miserable: a
//! designer wires up something the editor offered and the map silently does
//! nothing.
//!
//! So both directions are checked, by `kerosene_entity::schema::check` --
//! the same function a game runs over its own classes and schema.

use kerosene_entity::{ClassKind, Schema};

fn schema() -> Schema {
    Schema::parse(kerosene_game::schema::BUILTIN).expect("the embedded schema must parse")
}

#[test]
fn the_schema_and_the_registry_agree() {
    let problems = kerosene_entity::schema::check(&kerosene_game::registry(), &schema());
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn brush_classes_are_marked_as_such() {
    let schema = schema();
    // A class tied to brushes that the schema calls a point entity would be
    // offered in the wrong menu and refuse the brushes it needs.
    for name in [
        "func_door",
        "func_brush",
        "func_detail",
        "trigger_multiple",
        "trigger_once",
    ] {
        let spec = schema
            .get(name)
            .unwrap_or_else(|| panic!("{name} is in the schema"));
        assert!(
            spec.kind.takes_brushes(),
            "{name} must be a brush class, not {:?}",
            spec.kind
        );
    }
    for name in [
        "info_player_start",
        "light",
        "light_spot",
        "logic_relay",
        "math_counter",
    ] {
        let spec = schema
            .get(name)
            .unwrap_or_else(|| panic!("{name} is in the schema"));
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
    assert!(
        silent.is_empty(),
        "a class with no help is a class nobody can use: {silent:?}"
    );
}

#[test]
fn the_keys_the_game_reads_are_all_offered() {
    // Spot-check the ones with real behaviour behind them. This is the list a
    // designer would otherwise have to learn from the source.
    let schema = schema();
    for (class, keys) in [
        (
            "func_door",
            &["speed", "lip", "movedir", "locked", "spawnflags"][..],
        ),
        ("func_brush", &["startdisabled"][..]),
        ("trigger_multiple", &["startdisabled"][..]),
        ("trigger_hurt", &["damage"][..]),
        ("logic_relay", &["startdisabled", "spawnflags"][..]),
        ("logic_timer", &["refiretime", "startdisabled"][..]),
        ("math_counter", &["startvalue", "min", "max"][..]),
        ("point_message", &["message"][..]),
        (
            "light",
            &[
                "_light",
                "_constant_attn",
                "_linear_attn",
                "_quadratic_attn",
            ][..],
        ),
        (
            "light_spot",
            &["_light", "_cone", "_inner_cone", "_exponent", "pitch"][..],
        ),
        ("light_environment", &["_light", "_ambient", "pitch"][..]),
        ("worldspawn", &["skyname"][..]),
        ("prop_static", &["model"][..]),
        (
            "prop_physics",
            &["model", "mass", "friction", "elasticity", "pickable"][..],
        ),
        (
            "prop_dynamic_spawner",
            &["model", "mass", "friction", "elasticity", "pickable"][..],
        ),
    ] {
        let spec = schema
            .get(class)
            .unwrap_or_else(|| panic!("{class} is in the schema"));
        for key in keys {
            assert!(
                spec.key(key).is_some(),
                "{class} reads `{key}` but the schema does not offer it"
            );
        }
    }
}
