use super::*;

#[test]
fn writing_the_same_value_changes_nothing() {
    let mut store = UiStore::new();
    store.set("player.health", 100);
    let g = store.generation();
    store.set("player.health", 100);
    assert_eq!(store.generation(), g);
    store.set("player.health", 99);
    assert_eq!(store.changed_since(g), vec!["player.health"]);
}

#[test]
fn removal_is_a_change() {
    let mut store = UiStore::new();
    store.set("a", 1);
    let g = store.generation();
    store.remove("a");
    assert_eq!(store.changed_since(g), vec!["a"]);
    assert!(store.get("a").is_none());
}

#[test]
fn dotted_keys_become_nested_maps() {
    let mut store = UiStore::new();
    store.set("weapon.active", "pistol");
    store.set("weapon.ammo", 12);
    let roots = store.to_scope_maps();
    let weapon = roots["weapon"].clone().cast::<rhai::Map>();
    assert_eq!(weapon["active"].clone().cast::<String>(), "pistol");
    assert_eq!(weapon["ammo"].as_int().unwrap(), 12);
}

#[test]
fn key_affects_prefixes_both_ways() {
    assert!(key_affects("weapon.ammo", "weapon"));
    assert!(key_affects("weapon", "weapon.ammo"));
    assert!(key_affects("weapon.ammo", "weapon.ammo"));
    assert!(!key_affects("weapons", "weapon"));
    assert!(!key_affects("weapon.ammo", "weapon.active"));
}

#[test]
fn whole_floats_print_as_integers() {
    assert_eq!(Value::Float(100.0).to_string(), "100");
    assert_eq!(Value::Float(0.5).to_string(), "0.5");
    assert_eq!(Value::parse("12"), Value::Int(12));
    assert_eq!(Value::parse("true"), Value::Bool(true));
}

#[test]
fn the_event_queue_is_bounded() {
    let mut store = UiStore::new();
    for i in 0..MAX_PENDING_EVENTS + 10 {
        store.emit("tick", i.to_string());
    }
    let events = store.take_events();
    assert_eq!(events.len(), MAX_PENDING_EVENTS);
    assert_eq!(events[0].data, "10");
}
