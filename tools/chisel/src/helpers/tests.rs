// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
use super::*;

#[test]
fn a_model_path_is_named_the_same_however_it_was_written() {
    for written in [
        "props/crate",
        "models/props/crate",
        "models/props/crate.keromdl",
        " props/crate ",
    ] {
        assert_eq!(model_name(written), "props/crate");
    }
}

#[test]
fn an_entity_shows_its_own_model_before_its_classes_default() {
    let schema = kerosene_entity::Schema::parse(
        r#"class { "name" "prop_static" key { "name" "model" "type" "model" "default" "props/default" } }"#,
    )
    .unwrap();
    let spec = schema.get("prop_static");
    let mut entity = kerosene_map::Entity::new(1, "prop_static");
    assert_eq!(
        entity_model(&entity, spec).as_deref(),
        Some("props/default")
    );
    entity.set("model", "models/props/own.keromdl");
    assert_eq!(entity_model(&entity, spec).as_deref(), Some("props/own"));
    // A class with no model key and an entity with none: nothing.
    let plain = kerosene_map::Entity::new(2, "info_target");
    assert_eq!(entity_model(&plain, None), None);
}

#[test]
fn a_modelled_entity_is_posed_where_it_stands() {
    let mut entity = kerosene_map::Entity::new(7, "prop_static");
    entity.set("model", "props/crate");
    entity.set_origin(Vec3::new(10.0, 20.0, 30.0));
    let helpers = for_entity(&entity, None, entity.origin(), true, true, &|_| Vec::new());
    let [
        Helper::Model {
            owner,
            pose,
            selected,
            ..
        },
    ] = helpers.as_slice()
    else {
        panic!("{helpers:?}");
    };
    assert_eq!(*owner, Some(7));
    assert_eq!(pose.origin, Vec3::new(10.0, 20.0, 30.0));
    assert!(*selected);
}

fn stock() -> kerosene_entity::Schema {
    crate::classes::load_with(std::path::Path::new("/nonexistent"), &[]).schema
}

fn nowhere(_: &str) -> Vec<Vec3> {
    Vec::new()
}

/// Every point any line helper draws.
fn line_points(helpers: &[Helper]) -> Vec<Vec3> {
    helpers
        .iter()
        .filter_map(|h| match h {
            Helper::Lines { segments, .. } => Some(segments),
            _ => None,
        })
        .flatten()
        .flat_map(|[a, b]| [*a, *b])
        .collect()
}

#[test]
fn a_spot_lights_cone_points_where_its_angles_say() {
    let schema = stock();
    let mut light = kerosene_map::Entity::new(1, "light_spot");
    light.set("angles", "0 90 0");
    light.set("_cone", "30");
    let helpers = for_entity(
        &light,
        schema.get("light_spot"),
        Vec3::ZERO,
        true,
        true,
        &nowhere,
    );
    let fill = helpers
        .iter()
        .find_map(|h| match h {
            Helper::Fill { triangles, .. } => Some(triangles),
            _ => None,
        })
        .expect("a filled cone");
    // Yaw 90 faces +Y: every vertex of the cone is at or ahead of the apex.
    for p in fill.iter().flatten() {
        assert!(p.y >= -1e-3, "{p}");
        assert!(
            p.x.abs() <= p.y * 30f32.to_radians().tan() + 1e-2,
            "inside 30 degrees: {p}"
        );
    }
    assert!(fill.iter().flatten().any(|p| p.y > 32.0), "and has length");
}

#[test]
fn the_pitch_key_overrides_a_spot_lights_angles() {
    let schema = stock();
    let mut light = kerosene_map::Entity::new(1, "light_spot");
    light.set("angles", "0 0 0");
    light.set("pitch", "-90");
    let helpers = for_entity(
        &light,
        schema.get("light_spot"),
        Vec3::ZERO,
        true,
        true,
        &nowhere,
    );
    // Stored upward-positive, so -90 points straight down.
    let points = line_points(&helpers);
    assert!(points.iter().any(|p| p.z < -32.0), "points down");
    assert!(
        points.iter().all(|p| p.z <= 1e-3 || p.length() > 64.0),
        "not up"
    );
}

#[test]
fn a_brighter_light_reaches_further() {
    let schema = stock();
    let radius = |brightness: &str| {
        let mut light = kerosene_map::Entity::new(1, "light");
        light.set("_light", format!("255 255 255 {brightness}"));
        let helpers = for_entity(
            &light,
            schema.get("light"),
            Vec3::ZERO,
            true,
            true,
            &nowhere,
        );
        line_points(&helpers)
            .iter()
            .map(|p| p.length())
            .fold(0.0, f32::max)
    };
    assert!(radius("400") > radius("100"));
    assert!(radius("100") > 0.0);
}

#[test]
fn a_sounds_sphere_is_its_radius_key_and_zero_draws_none() {
    let schema = stock();
    let mut sound = kerosene_map::Entity::new(1, "ambient_generic");
    let helpers = |e: &kerosene_map::Entity| {
        for_entity(
            e,
            schema.get("ambient_generic"),
            Vec3::ZERO,
            true,
            true,
            &nowhere,
        )
    };
    assert!(
        line_points(&helpers(&sound)).is_empty(),
        "radius 0: nothing"
    );
    sound.set("radius", "300");
    let far = line_points(&helpers(&sound))
        .iter()
        .map(|p| p.length())
        .fold(0.0, f32::max);
    assert!((far - 300.0).abs() < 0.5, "{far}");
}

#[test]
fn a_key_naming_another_entity_draws_a_line_to_it() {
    let schema = stock();
    let mut teleport = kerosene_map::Entity::new(1, "trigger_teleport");
    teleport.set("target", "exit");
    let targets = |name: &str| {
        if name == "exit" {
            vec![Vec3::new(500.0, 0.0, 0.0)]
        } else {
            Vec::new()
        }
    };
    let helpers = for_entity(
        &teleport,
        schema.get("trigger_teleport"),
        Vec3::ZERO,
        true,
        true,
        &targets,
    );
    let lines: Vec<_> = helpers
        .iter()
        .filter(|h| matches!(h, Helper::Lines { xray: true, .. }))
        .collect();
    assert_eq!(lines.len(), 1, "one line, not one per way of declaring it");
    assert!(line_points(&helpers).contains(&Vec3::new(500.0, 0.0, 0.0)));
}

#[test]
fn without_extras_only_the_model_is_drawn() {
    let schema = stock();
    let mut light = kerosene_map::Entity::new(1, "light_spot");
    light.set("_cone", "30");
    assert!(
        for_entity(
            &light,
            schema.get("light_spot"),
            Vec3::ZERO,
            false,
            false,
            &nowhere
        )
        .is_empty()
    );
    let prop = kerosene_map::Entity::new(2, "prop_static");
    let helpers = for_entity(
        &prop,
        schema.get("prop_static"),
        Vec3::ZERO,
        false,
        false,
        &nowhere,
    );
    assert!(matches!(helpers.as_slice(), [Helper::Model { .. }]));
}

#[test]
fn target_names_match_case_blind_and_exactly() {
    let mut a = kerosene_map::Entity::new(1, "info_target");
    a.set("targetname", "Door_1");
    a.set_origin(Vec3::X);
    let mut b = kerosene_map::Entity::new(2, "info_target");
    b.set("targetname", "door_2");
    b.set_origin(Vec3::Y);
    let names = named_positions(&[a, b]);
    assert_eq!(lookup(&names, "door_1"), vec![Vec3::X]);
    assert!(
        lookup(&names, "door*").is_empty(),
        "the engine has no wildcards"
    );
    assert!(lookup(&names, "!activator").is_empty());
    assert!(lookup(&names, "").is_empty());
}

#[test]
fn a_brush_entity_stands_in_the_middle_of_its_brushes() {
    let mut door = kerosene_map::Entity::new(1, "func_door");
    door.solids.push(kerosene_map::Solid::cube(
        kerosene_math::Aabb::new(Vec3::new(100.0, 0.0, 0.0), Vec3::new(200.0, 10.0, 50.0)),
        "x",
    ));
    assert_eq!(entity_centre(&door), Vec3::new(150.0, 5.0, 25.0));
}
