// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Components: typed data a class declares, filled from keyvalues.
use super::*;
use crate::registry::ClassDef;
use kerosene_ecs::prelude::*;

#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
struct Mover {
    #[reflect(@Key("speed"), @Label("Speed"))]
    speed: f32,
    #[reflect(@Key("targetsound"))]
    sound: String,
    /// Where it has got to. No keyvalue.
    progress: f32,
}

impl Default for Mover {
    fn default() -> Self {
        Mover {
            speed: 100.0,
            sound: "door/move".into(),
            progress: 0.0,
        }
    }
}

fn world() -> EntityWorld {
    let mut registry = ClassRegistry::new();
    registry.register(ClassDef::new("func_door").component::<Mover>());
    EntityWorld::new(Arc::new(registry))
}

fn door(w: &mut EntityWorld, keys: &[(&str, &str)]) -> EntityId {
    w.spawn_with("func_door", keys)
}

#[test]
fn a_map_key_fills_the_component_and_not_the_loose_fields() {
    let mut w = world();
    let id = door(&mut w, &[("speed", "250"), ("my_mod_key", "kept")]);
    let m = w.component::<Mover>(id).unwrap();
    assert_eq!(m.speed, 250.0);
    assert_eq!(m.sound, "door/move", "absent keys take the default");
    let e = w.get(id).unwrap();
    assert!(!e.fields.contains("speed"), "one place for each value");
    assert_eq!(e.fields.text("my_mod_key").as_deref(), Some("kept"));
}

#[test]
fn a_key_that_will_not_parse_keeps_the_default() {
    let mut w = world();
    let id = door(&mut w, &[("speed", "fast")]);
    assert_eq!(w.component::<Mover>(id).unwrap().speed, 100.0);
    assert!(!w.get(id).unwrap().fields.contains("speed"));
}

#[test]
fn keyvalues_read_and_write_through_the_component() {
    let mut w = world();
    let id = door(&mut w, &[]);
    assert_eq!(w.keyvalue(id, "SPEED"), Some(Value::Float(100.0)));
    assert!(w.set_keyvalue(id, "speed", Value::Text("40".into())));
    assert_eq!(w.component::<Mover>(id).unwrap().speed, 40.0);
    // A field with no keyvalue is still reachable by its name.
    assert!(w.set_keyvalue(id, "progress", Value::Float(0.5)));
    assert_eq!(w.keyvalue(id, "progress"), Some(Value::Float(0.5)));
    // Anything no component claims is a loose field, as before.
    assert!(w.set_keyvalue(id, "other", Value::Int(3)));
    assert_eq!(w.keyvalue(id, "other"), Some(Value::Int(3)));
    // A value that does not fit is refused.
    assert!(!w.set_keyvalue(id, "speed", Value::Text("fast".into())));
    assert_eq!(w.component::<Mover>(id).unwrap().speed, 40.0);
}

#[test]
fn component_mut_changes_what_everyone_reads() {
    let mut w = world();
    let id = door(&mut w, &[]);
    w.component_mut::<Mover>(id).unwrap().progress = 0.25;
    assert_eq!(w.keyvalue(id, "progress"), Some(Value::Float(0.25)));
    assert_eq!(w.components(id).len(), 1);
    assert_eq!(w.components(id)[0].0, "Mover");
}

#[test]
fn a_class_without_the_component_has_none() {
    let mut w = world();
    let id = w.spawn_with("info_target", &[("speed", "5")]);
    assert!(w.component::<Mover>(id).is_none());
    assert_eq!(w.keyvalue(id, "speed"), Some(Value::Int(5)));
}

#[test]
fn a_removed_entity_takes_its_components_with_it() {
    let mut w = world();
    let id = door(&mut w, &[]);
    let handle = w.get(id).unwrap().handle;
    w.remove(id);
    w.run(0.0);
    assert!(w.component::<Mover>(id).is_none());
    assert!(w.ecs.get_entity(handle).is_err(), "despawned, not leaked");
}

#[test]
fn components_are_saved_and_restored_by_name() {
    let mut w = world();
    let id = door(&mut w, &[("speed", "250")]);
    w.component_mut::<Mover>(id).unwrap().progress = 0.75;
    let snap = w.snapshot();
    let saved = &snap.entities[0];
    assert!(saved.fields.is_empty(), "{:?}", saved.fields);
    assert_eq!(
        saved.components["Mover"]["speed"],
        crate::SavedValue::Float(250.0)
    );

    let text = serde_json::to_string(&snap).unwrap();
    let back: crate::WorldSnapshot = serde_json::from_str(&text).unwrap();
    let mut w2 = world();
    w2.restore(&back).unwrap();
    assert_eq!(
        w2.component::<Mover>(id),
        Some(&Mover {
            speed: 250.0,
            sound: "door/move".into(),
            progress: 0.75
        })
    );
}

#[test]
fn a_save_from_before_the_component_still_loads() {
    // The same door as a save file wrote it when its speed and progress
    // were loose fields.
    let mut w = world();
    let id = door(&mut w, &[]);
    let mut snap = w.snapshot();
    let saved = &mut snap.entities[0];
    saved.components.clear();
    saved
        .fields
        .insert("speed".into(), crate::SavedValue::Float(60.0));
    saved
        .fields
        .insert("progress".into(), crate::SavedValue::Float(0.5));
    saved
        .fields
        .insert("unclaimed".into(), crate::SavedValue::Int(1));

    let mut w2 = world();
    w2.restore(&snap).unwrap();
    let m = w2.component::<Mover>(id).unwrap();
    assert_eq!((m.speed, m.progress), (60.0, 0.5));
    let e = w2.get(id).unwrap();
    assert!(!e.fields.contains("speed") && !e.fields.contains("progress"));
    assert!(e.fields.contains("unclaimed"));
}

#[test]
fn the_editor_is_offered_every_key_a_component_declares() {
    let registry = world().registry.clone();
    let schema = crate::Schema::parse(r#"class { "name" "func_door" "kind" "brush" }"#)
        .unwrap()
        .with_component_keys(&registry);
    let spec = schema.get("func_door").unwrap();
    let speed = spec.key("speed").unwrap();
    assert_eq!(speed.label, "Speed");
    assert_eq!(speed.kind, crate::KeyKind::Float);
    assert_eq!(speed.default, "100");
    assert_eq!(spec.key("targetsound").unwrap().default, "door/move");
    assert!(spec.key("progress").is_none(), "state has no key to offer");
    let keys: Vec<String> = crate::schema::check(&registry, &schema)
        .into_iter()
        .filter(|p| p.contains("key"))
        .collect();
    assert!(keys.is_empty(), "{keys:?}");
}

#[test]
fn a_written_key_that_disagrees_with_the_component_is_caught() {
    let registry = world().registry.clone();
    let text = r#"class { "name" "func_door" "kind" "brush"
        key { "name" "speed" "type" "float" "default" "90" } }"#;
    let schema = crate::Schema::parse(text)
        .unwrap()
        .with_component_keys(&registry);
    assert_eq!(schema.get("func_door").unwrap().key("speed").unwrap().default, "90");
    let problems = crate::schema::check(&registry, &schema);
    assert!(
        problems.iter().any(|p| p.contains("func_door.speed") && p.contains("90")),
        "{problems:?}"
    );
    let unoffered = crate::schema::check(&registry, &crate::Schema::parse(text).unwrap());
    assert!(
        unoffered.iter().any(|p| p.contains("targetsound") && p.contains("not offered")),
        "{unoffered:?}"
    );
}
