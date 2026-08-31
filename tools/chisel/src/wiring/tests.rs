// SPDX-License-Identifier: MPL-2.0
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
    let order: Vec<&str> = events[0].steps.iter().map(|i| wires[*i].target.as_str()).collect();
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
    let event = Event { name: "OnTrigger".into(), steps: Vec::new() };
    let next = then(&[], &event);
    assert_eq!(next.output, "OnTrigger");
    assert_eq!(next.delay, 0.0, "the first step of a sequence waits for nothing");
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
    for name in ["OnTrue", "OnFalse", "OnHitMax", "OnHitMin", "OnOpen", "OnClose"] {
        let other = opposite_of(name).unwrap_or_else(|| panic!("{name} has no opposite"));
        assert_eq!(opposite_of(other), Some(name), "{name} <-> {other}");
    }
}
