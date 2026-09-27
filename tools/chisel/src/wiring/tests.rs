// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
use super::*;

fn wire(output: &str, target: &str, input: &str, delay: f32) -> Connection {
    let mut c = Connection::new(output, target, input);
    c.delay = delay;
    c
}

#[test]
fn nothing_wired_up_is_no_events() {
    assert!(events(&[]).is_empty());
}

#[test]
fn one_event_gathers_everything_wired_to_it() {
    // The thing a flat list makes you reconstruct in your head.
    let wires = vec![
        wire("OnStartTouch", "door", "Open", 0.0),
        wire("OnStartTouch", "siren", "Trigger", 0.5),
        wire("OnStartTouch", "lights", "Disable", 0.2),
    ];
    let events = events(&wires);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "OnStartTouch");
    assert_eq!(events[0].steps.len(), 3);
}

#[test]
fn steps_are_shown_in_the_order_they_will_fire() {
    // Any other order would be showing something untrue.
    let wires = vec![
        wire("OnStartTouch", "siren", "Trigger", 0.5),
        wire("OnStartTouch", "door", "Open", 0.0),
        wire("OnStartTouch", "lights", "Disable", 0.2),
    ];
    let events = events(&wires);
    let order: Vec<&str> = events[0]
        .steps
        .iter()
        .map(|i| wires[*i].target.as_str())
        .collect();
    assert_eq!(order, vec!["door", "lights", "siren"]);
}

#[test]
fn two_actions_at_the_same_instant_keep_a_stable_order() {
    // Otherwise the list reshuffles itself while someone is editing it.
    let wires = vec![
        wire("OnTrigger", "b", "Trigger", 0.0),
        wire("OnTrigger", "a", "Trigger", 0.0),
    ];
    let first = events(&wires);
    let second = events(&wires);
    assert_eq!(first, second);
    assert_eq!(first[0].steps, vec![0, 1], "file order breaks the tie");
}

#[test]
fn events_keep_the_order_they_first_appear_in() {
    // The list must not jump about as delays are edited.
    let wires = vec![
        wire("OnFullyOpen", "a", "Trigger", 5.0),
        wire("OnOpen", "b", "Trigger", 0.0),
    ];
    let names: Vec<String> = events(&wires).into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec!["OnFullyOpen", "OnOpen"]);
}

#[test]
fn a_then_fires_after_everything_already_on_the_event() {
    let wires = vec![
        wire("OnStartTouch", "door", "Open", 0.0),
        wire("OnStartTouch", "siren", "Trigger", 0.5),
    ];
    let event = &events(&wires)[0];
    let next = then(&wires, event);

    assert_eq!(next.output, "OnStartTouch");
    assert!(next.delay > 0.5, "{}", next.delay);
}

#[test]
fn a_then_is_never_simultaneous_with_what_it_follows() {
    // Two actions at the same instant fire in whatever order the file happens
    // to hold, and a sequence whose steps you cannot see is undebuggable.
    let wires = vec![wire("OnTrigger", "a", "Trigger", 0.0)];
    let next = then(&wires, &events(&wires)[0]);
    assert!(next.delay > 0.0);
}

#[test]
fn a_then_carries_the_target_forward() {
    // A "then" is nearly always another thing done to the same object, and
    // when it is not, changing one field beats filling in three.
    let wires = vec![wire("OnStartTouch", "door", "Open", 0.0)];
    let next = then(&wires, &events(&wires)[0]);
    assert_eq!(next.target, "door");
    assert_eq!(next.input, "Open");
}

#[test]
fn a_then_on_an_event_with_nothing_on_it_yet_starts_blank() {
    let event = Event {
        name: "OnTrigger".into(),
        steps: Vec::new(),
    };
    let next = then(&[], &event);
    assert_eq!(next.output, "OnTrigger");
    assert_eq!(
        next.delay, 0.0,
        "the first step of a sequence waits for nothing"
    );
    assert!(next.target.is_empty());
}

#[test]
fn evening_out_the_timing_spaces_the_steps() {
    assert_eq!(evenly_spaced(0), Vec::<f32>::new());
    assert_eq!(evenly_spaced(1), vec![0.0]);
    let three = evenly_spaced(3);
    assert_eq!(three[0], 0.0, "the first waits for nothing");
    assert!(three[1] > three[0] && three[2] > three[1]);
}

#[test]
fn the_two_sides_of_a_choice_know_about_each_other() {
    // `OnTrue` with no `OnFalse` beside it is a branch that silently does
    // nothing half the time -- a bug you find by playing, not by reading.
    assert_eq!(opposite_of("OnTrue"), Some("OnFalse"));
    assert_eq!(opposite_of("OnFalse"), Some("OnTrue"));
    assert_eq!(opposite_of("OnFullyOpen"), Some("OnFullyClosed"));
}

#[test]
fn an_ordinary_event_has_no_opposite() {
    assert_eq!(opposite_of("OnStartTouch"), None);
    assert_eq!(opposite_of("OnTrigger"), None);
}

#[test]
fn every_opposite_is_mutual() {
    for name in [
        "OnTrue", "OnFalse", "OnHitMax", "OnHitMin", "OnOpen", "OnClose",
    ] {
        let other = opposite_of(name).unwrap_or_else(|| panic!("{name} has no opposite"));
        assert_eq!(opposite_of(other), Some(name), "{name} <-> {other}");
    }
}

// ---- validation and inputs --------------------------------------------

fn schema() -> kerosene_entity::Schema {
    kerosene_entity::Schema::parse(
        r#"
class { "name" "func_button" output { "name" "OnPressed" } }
class { "name" "func_door" input { "name" "Open" } input { "name" "Close" } }
"#,
    )
    .unwrap()
}

fn named(id: u32, class: &str, name: &str) -> kerosene_map::Entity {
    let mut e = kerosene_map::Entity::new(id, class);
    e.set("targetname", name);
    e
}

#[test]
fn a_connection_to_a_door_that_opens_is_fine() {
    let button = kerosene_map::Entity::new(1, "func_button");
    let door = named(2, "func_door", "Door");
    let c = Connection::new("OnPressed", "door", "Open");
    assert_eq!(
        validate(&button, &c, &[button.clone(), door], &schema()),
        Status::Ok
    );
}

#[test]
fn each_way_a_connection_can_do_nothing_is_named() {
    let button = kerosene_map::Entity::new(1, "func_button");
    let door = named(2, "func_door", "door");
    let entities = [button.clone(), door];
    let check = |c: Connection| validate(&button, &c, &entities, &schema());
    let broken = |c: Connection| match check(c) {
        Status::Broken(why) => why,
        other => panic!("{other:?}"),
    };
    assert!(
        broken(Connection::new("OnPressed", "gate", "Open")).contains("no entity is called gate")
    );
    assert!(broken(Connection::new("OnPressed", "door", "Explode")).contains("no input Explode"));
    assert!(broken(Connection::new("OnHeld", "door", "Open")).contains("never fires OnHeld"));
    assert!(broken(Connection::new("OnPressed", "", "Open")).contains("no target"));
    assert!(broken(Connection::new("OnPressed", "door", "")).contains("no input"));
    assert!(broken(Connection::new("OnPressed", "do*", "Open")).contains("no wildcards"));
}

#[test]
fn the_activator_is_checked_only_as_far_as_it_can_be() {
    let button = kerosene_map::Entity::new(1, "func_button");
    let entities = [button.clone()];
    let c = Connection::new("OnPressed", "!activator", "Open");
    assert!(matches!(
        validate(&button, &c, &entities, &schema()),
        Status::Runtime(_)
    ));
    let c = Connection::new("OnPressed", "!activator", "Fly");
    assert!(validate(&button, &c, &entities, &schema()).is_broken());
}

#[test]
fn self_is_this_entity_and_is_checked_like_a_name() {
    let door = named(2, "func_door", "door");
    let mut schema = schema();
    schema.merge(
        kerosene_entity::Schema::parse(
            r#"class { "name" "func_door" input { "name" "Open" } output { "name" "OnOpen" } }"#,
        )
        .unwrap(),
    );
    let c = Connection::new("OnOpen", "!self", "Open");
    assert_eq!(
        validate(&door, &c, std::slice::from_ref(&door), &schema),
        Status::Ok
    );
    let c = Connection::new("OnOpen", "!self", "Press");
    assert!(validate(&door, &c, std::slice::from_ref(&door), &schema).is_broken());
}

#[test]
fn a_class_with_no_definition_is_taken_on_trust() {
    let button = kerosene_map::Entity::new(1, "mod_button");
    let thing = named(2, "mod_thing", "thing");
    let c = Connection::new("OnWhatever", "thing", "Anything");
    assert_eq!(
        validate(&button, &c, &[button.clone(), thing], &schema()),
        Status::Ok
    );
}

#[test]
fn inputs_are_every_connection_that_fires_at_an_entity() {
    let mut a = kerosene_map::Entity::new(1, "func_button");
    a.connect(Connection::new("OnPressed", "DOOR", "Open"));
    a.connect(Connection::new("OnPressed", "light", "TurnOn"));
    let mut b = kerosene_map::Entity::new(3, "trigger_once");
    b.connect(Connection::new("OnTrigger", "door", "Close"));
    let door = named(2, "func_door", "door");
    let mut selfish = named(4, "func_door", "other");
    selfish.connect(Connection::new("OnOpen", "!self", "Close"));
    let entities = [a, door.clone(), b, selfish.clone()];
    assert_eq!(inputs_to(&entities, &door), vec![(1, 0), (3, 0)]);
    assert_eq!(inputs_to(&entities, &selfish), vec![(4, 0)]);
}

#[test]
fn a_maps_broken_wires_are_listed_by_the_entity_they_are_on() {
    let mut button = named(1, "func_button", "switch");
    button.connect(Connection::new("OnPressed", "door", "Open"));
    button.connect(Connection::new("OnPressed", "nowhere", "Open"));
    let door = named(2, "func_door", "door");
    let problems = broken_wires(&[button, door], &schema());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("`switch`") && problems[0].contains("nowhere"),
        "{}",
        problems[0]
    );
}
