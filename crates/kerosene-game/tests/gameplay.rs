// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Behaviour tests for the game's entity classes.
//!
//! These drive the classes through the same entity world the engine uses, so
//! they exercise the real I/O routing rather than calling handlers directly.

use kerosene_entity::{Connection, EntityId, EntityWorld, InputEvent, Value};
use kerosene_kv::KeyValues;
use kerosene_math::Vec3;

const TICK: f32 = 1.0 / 64.0;

fn world_from(src: &str) -> EntityWorld {
    let mut w = EntityWorld::new(kerosene_game::registry());
    let kv = KeyValues::parse(src).expect("test map parses");
    w.load_from_kv(&kv).expect("entities load");
    w
}

fn run(w: &mut EntityWorld, seconds: f32) {
    let ticks = (seconds / TICK).ceil() as usize;
    for _ in 0..ticks { w.run(TICK); }
}

fn named(w: &EntityWorld, name: &str) -> EntityId {
    *w.find_by_name(name).first().unwrap_or_else(|| panic!("no entity named {name}"))
}

fn field(w: &EntityWorld, id: EntityId, key: &str) -> f32 {
    w.get(id).map(|e| e.fields.f32(key, -1.0)).unwrap_or(-1.0)
}

// ---- doors ---------------------------------------------------------------

const DOOR_MAP: &str = r#"
entity
{
    "classname" "func_door"
    "targetname" "gate"
    "model" "*1"
    "movedir" "0 0 1"
    "speed" "100"
    "lip" "8"
    "wait" "-1"
    "model_mins" "0 0 0"
    "model_maxs" "16 96 128"
}
"#;

#[test]
fn a_door_computes_its_travel_from_its_geometry() {
    // 128 tall, less an 8-unit lip. Taking it from the model rather than a
    // keyvalue is what lets a designer resize a door and have it still work.
    let w = world_from(DOOR_MAP);
    let gate = named(&w, "gate");
    assert_eq!(field(&w, gate, "travel"), 120.0);
    assert_eq!(w.get(gate).unwrap().origin, Vec3::ZERO);
}

#[test]
fn a_door_opens_and_stops_at_the_top() {
    let mut w = world_from(DOOR_MAP);
    let gate = named(&w, "gate");
    w.accept_input(gate, &InputEvent::new("Open"));

    run(&mut w, 0.5);
    let part_way = w.get(gate).unwrap().origin.z;
    assert!(part_way > 0.0 && part_way < 120.0, "should be moving, at {part_way}");

    // 120 units at 100 units/s takes 1.2 seconds, and should do so regardless
    // of how think times quantise onto the tick rate.
    run(&mut w, 1.3);
    assert_eq!(w.get(gate).unwrap().origin.z, 120.0, "should have stopped at the top");
    assert_eq!(field(&w, gate, "progress"), 1.0);
}

#[test]
fn a_door_fires_when_it_finishes_opening() {
    let mut w = world_from(&format!(
        "{DOOR_MAP}\nentity {{ \"classname\" \"math_counter\" \"targetname\" \"witness\" }}"
    ));
    let gate = named(&w, "gate");
    let witness = named(&w, "witness");
    w.get_mut(gate)
        .unwrap()
        .connections
        .push(Connection::new("OnFullyOpen", "witness", "Add").with_parameter("1"));

    w.accept_input(gate, &InputEvent::new("Open"));
    run(&mut w, 2.0);
    assert_eq!(field(&w, witness, "value"), 1.0, "OnFullyOpen should have fired exactly once");
}

#[test]
fn a_door_with_a_wait_closes_itself() {
    let src = DOOR_MAP.replace("\"wait\" \"-1\"", "\"wait\" \"0.5\"");
    let mut w = world_from(&src);
    let gate = named(&w, "gate");
    w.accept_input(gate, &InputEvent::new("Open"));

    // 120 units at 100 units/s opens in 1.2s, then the 0.5s wait means the
    // door starts closing again at about 1.7s.
    run(&mut w, 1.4);
    assert_eq!(w.get(gate).unwrap().origin.z, 120.0, "open, and still waiting");
    assert!(w.get(gate).unwrap().fields.f32("progress", 0.0) == 1.0);
    run(&mut w, 3.0);
    assert_eq!(w.get(gate).unwrap().origin.z, 0.0, "should have closed itself again");
}

#[test]
fn a_door_with_no_wait_stays_open() {
    let mut w = world_from(DOOR_MAP);
    let gate = named(&w, "gate");
    w.accept_input(gate, &InputEvent::new("Open"));
    run(&mut w, 8.0);
    assert_eq!(w.get(gate).unwrap().origin.z, 120.0);
}

#[test]
fn opening_a_door_that_is_already_open_does_nothing() {
    let mut w = world_from(DOOR_MAP);
    let gate = named(&w, "gate");
    w.accept_input(gate, &InputEvent::new("Open"));
    run(&mut w, 2.0);
    w.accept_input(gate, &InputEvent::new("Open"));
    run(&mut w, 0.5);
    assert_eq!(w.get(gate).unwrap().origin.z, 120.0);
}

#[test]
fn a_door_can_be_reversed_mid_travel() {
    let mut w = world_from(DOOR_MAP);
    let gate = named(&w, "gate");
    w.accept_input(gate, &InputEvent::new("Open"));
    run(&mut w, 0.4);
    let part_way = w.get(gate).unwrap().origin.z;
    assert!(part_way > 0.0 && part_way < 120.0);

    w.accept_input(gate, &InputEvent::new("Close"));
    run(&mut w, 2.0);
    assert_eq!(w.get(gate).unwrap().origin.z, 0.0);
}

#[test]
fn a_locked_door_refuses_to_open_and_says_so() {
    let mut w = world_from(&format!(
        "{DOOR_MAP}\nentity {{ \"classname\" \"math_counter\" \"targetname\" \"witness\" }}"
    ));
    let gate = named(&w, "gate");
    w.get_mut(gate)
        .unwrap()
        .connections
        .push(Connection::new("OnLockedUse", "witness", "Add").with_parameter("1"));

    w.accept_input(gate, &InputEvent::new("Lock"));
    w.accept_input(gate, &InputEvent::new("Open"));
    run(&mut w, 2.0);
    assert_eq!(w.get(gate).unwrap().origin.z, 0.0, "a locked door must not move");
    assert_eq!(field(&w, named(&w, "witness"), "value"), 1.0);

    w.accept_input(gate, &InputEvent::new("Unlock"));
    w.accept_input(gate, &InputEvent::new("Open"));
    run(&mut w, 2.0);
    assert_eq!(w.get(gate).unwrap().origin.z, 120.0);
}

#[test]
fn toggle_alternates() {
    let mut w = world_from(DOOR_MAP);
    let gate = named(&w, "gate");
    w.accept_input(gate, &InputEvent::new("Toggle"));
    run(&mut w, 2.0);
    assert_eq!(w.get(gate).unwrap().origin.z, 120.0);
    w.accept_input(gate, &InputEvent::new("Toggle"));
    run(&mut w, 2.0);
    assert_eq!(w.get(gate).unwrap().origin.z, 0.0);
}

// ---- triggers ------------------------------------------------------------

const TRIGGER_MAP: &str = r#"
entity
{
    "classname" "trigger_multiple"
    "targetname" "zone"
    "model" "*2"
    connections { "OnStartTouch" "counter,Add,1,0,-1" "OnEndTouch" "counter,Subtract,1,0,-1" }
}
entity { "classname" "math_counter" "targetname" "counter" }
"#;

#[test]
fn a_trigger_fires_on_entering_and_leaving_not_continuously() {
    let mut w = world_from(TRIGGER_MAP);
    let zone = named(&w, "zone");
    let counter = named(&w, "counter");

    // Standing inside for many ticks should fire once, not once per tick.
    for _ in 0..20 {
        kerosene_game::triggers::update_touch(&mut w, zone, true, None);
        w.run(TICK);
    }
    assert_eq!(field(&w, counter, "value"), 1.0);

    for _ in 0..20 {
        kerosene_game::triggers::update_touch(&mut w, zone, false, None);
        w.run(TICK);
    }
    assert_eq!(field(&w, counter, "value"), 0.0);
}

#[test]
fn a_trigger_once_removes_itself_after_firing() {
    let src = TRIGGER_MAP.replace("trigger_multiple", "trigger_once");
    let mut w = world_from(&src);
    let zone = named(&w, "zone");
    kerosene_game::triggers::update_touch(&mut w, zone, true, None);
    w.run(TICK);
    assert!(!w.exists(zone), "a trigger_once should be gone after it fires");
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);
}

#[test]
fn a_disabled_trigger_does_not_fire() {
    let mut w = world_from(TRIGGER_MAP);
    let zone = named(&w, "zone");
    w.accept_input(zone, &InputEvent::new("Disable"));
    kerosene_game::triggers::update_touch(&mut w, zone, true, None);
    w.run(TICK);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 0.0);

    w.accept_input(zone, &InputEvent::new("Enable"));
    kerosene_game::triggers::update_touch(&mut w, zone, true, None);
    w.run(TICK);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);
}

#[test]
fn disabling_an_occupied_trigger_releases_it() {
    // Otherwise the trigger believes it is occupied forever and never fires
    // OnStartTouch again.
    let mut w = world_from(TRIGGER_MAP);
    let zone = named(&w, "zone");
    kerosene_game::triggers::update_touch(&mut w, zone, true, None);
    w.run(TICK);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);

    w.accept_input(zone, &InputEvent::new("Disable"));
    w.run(TICK);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 0.0, "OnEndTouch should have fired");

    w.accept_input(zone, &InputEvent::new("Enable"));
    kerosene_game::triggers::update_touch(&mut w, zone, true, None);
    w.run(TICK);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0, "it should fire again");
}

// ---- logic ---------------------------------------------------------------

#[test]
fn logic_auto_fires_once_when_the_map_starts() {
    let mut w = world_from(
        r#"
entity { "classname" "logic_auto" connections { "OnMapSpawn" "counter,Add,1,0,-1" } }
entity { "classname" "math_counter" "targetname" "counter" }
"#,
    );
    run(&mut w, 1.0);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);
    run(&mut w, 5.0);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0, "and never again");
}

#[test]
fn a_counter_clamps_and_fires_at_its_limits() {
    let mut w = world_from(
        r#"
entity
{
    "classname" "math_counter"
    "targetname" "counter"
    "startvalue" "0"
    "min" "0"
    "max" "3"
    connections { "OnHitMax" "witness,Add,1,0,-1" }
}
entity { "classname" "math_counter" "targetname" "witness" }
"#,
    );
    let counter = named(&w, "counter");
    for _ in 0..5 {
        w.accept_input(counter, &InputEvent::new("Add").with_parameter("1"));
        w.run(TICK);
    }
    assert_eq!(field(&w, counter, "value"), 3.0, "should clamp at its maximum");
    assert_eq!(
        field(&w, named(&w, "witness"), "value"),
        1.0,
        "OnHitMax fires on the transition, not on every add at the limit"
    );
}

#[test]
fn a_relay_passes_the_activator_along() {
    // So that `!activator` several relays down still resolves to the player.
    let mut w = world_from(
        r#"
entity { "classname" "logic_relay" "targetname" "a" connections { "OnTrigger" "b,Trigger,,0,-1" } }
entity { "classname" "logic_relay" "targetname" "b" connections { "OnTrigger" "!activator,Kill,,0,-1" } }
entity { "classname" "info_target" "targetname" "victim" "origin" "0 0 0" }
"#,
    );
    let victim = named(&w, "victim");
    let a = named(&w, "a");
    w.accept_input(a, &InputEvent { activator: Some(victim), ..InputEvent::new("Trigger") });
    run(&mut w, 0.1);
    assert!(!w.exists(victim), "the activator should have survived two relays");
}

#[test]
fn a_disabled_relay_swallows_its_signal() {
    let mut w = world_from(
        r#"
entity { "classname" "logic_relay" "targetname" "a" "startdisabled" "1"
         connections { "OnTrigger" "counter,Add,1,0,-1" } }
entity { "classname" "math_counter" "targetname" "counter" }
"#,
    );
    let a = named(&w, "a");
    w.accept_input(a, &InputEvent::new("Trigger"));
    run(&mut w, 0.1);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 0.0);

    w.accept_input(a, &InputEvent::new("Enable"));
    w.accept_input(a, &InputEvent::new("Trigger"));
    run(&mut w, 0.1);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);
}

#[test]
fn a_timer_fires_repeatedly() {
    let mut w = world_from(
        r#"
entity { "classname" "logic_timer" "targetname" "t" "refiretime" "0.25"
         connections { "OnTimer" "counter,Add,1,0,-1" } }
entity { "classname" "math_counter" "targetname" "counter" }
"#,
    );
    run(&mut w, 1.1);
    let fired = field(&w, named(&w, "counter"), "value");
    assert!((3.0..=5.0).contains(&fired), "fired {fired} times in 1.1s at 0.25s intervals");
}

#[test]
fn add_output_rewires_an_entity_at_runtime() {
    let mut w = world_from(
        r#"
entity { "classname" "logic_relay" "targetname" "a" }
entity { "classname" "math_counter" "targetname" "counter" }
"#,
    );
    let a = named(&w, "a");
    w.accept_input(a, &InputEvent::new("Trigger"));
    run(&mut w, 0.1);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 0.0, "nothing wired yet");

    w.accept_input(
        a,
        &InputEvent::new("AddOutput").with_parameter("OnTrigger counter,Add,1,0,-1"),
    );
    w.accept_input(a, &InputEvent::new("Trigger"));
    run(&mut w, 0.1);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);
}

#[test]
fn the_whole_sample_chain_works_end_to_end() {
    // A trigger opens a door, and the door tells a counter when it is open --
    // the shape of nearly every scripted moment in a Source level.
    let mut w = world_from(&format!(
        r#"
{DOOR_MAP}
entity
{{
    "classname" "trigger_multiple"
    "targetname" "zone"
    "model" "*2"
    connections {{ "OnStartTouch" "gate,Open,,0,-1" }}
}}
entity {{ "classname" "math_counter" "targetname" "counter" }}
"#
    ));
    let gate = named(&w, "gate");
    w.get_mut(gate)
        .unwrap()
        .connections
        .push(Connection::new("OnFullyOpen", "counter", "Add").with_parameter("1"));

    let zone = named(&w, "zone");
    kerosene_game::triggers::update_touch(&mut w, zone, true, None);
    run(&mut w, 3.0);

    assert_eq!(w.get(gate).unwrap().origin.z, 120.0, "the door should have opened");
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);
}

#[test]
fn lighting_entities_load_without_complaint() {
    // They are compile-time only, and a map is full of them.
    let mut w = world_from(
        r#"
entity { "classname" "light" "origin" "0 0 128" "_light" "255 255 255 200" }
entity { "classname" "light_environment" "pitch" "-45" "_light" "255 255 255 300" }
entity { "classname" "info_player_start" "origin" "0 0 16" }
"#,
    );
    assert_eq!(w.len(), 3);
    run(&mut w, 1.0);
    assert_eq!(w.len(), 3, "none of them should do anything at runtime");
}

#[test]
fn a_brush_entity_can_be_switched_off() {
    let mut w = world_from(
        r#"entity { "classname" "func_brush" "targetname" "wall" "model" "*3" }"#,
    );
    let wall = named(&w, "wall");
    assert!(kerosene_game::doors::brush_enabled(&w, wall));
    w.accept_input(wall, &InputEvent::new("Disable"));
    assert!(!kerosene_game::doors::brush_enabled(&w, wall));
    w.accept_input(wall, &InputEvent::new("Toggle"));
    assert!(kerosene_game::doors::brush_enabled(&w, wall));
}

#[test]
fn setting_a_field_directly_still_works_for_engine_code() {
    let mut w = EntityWorld::new(kerosene_game::registry());
    let id = w.spawn("math_counter");
    w.get_mut(id).unwrap().fields.set("value", Value::Float(7.0));
    assert_eq!(field(&w, id, "value"), 7.0);
}

#[test]
fn a_door_takes_the_time_its_speed_implies() {
    // The trap: deriving the step from the requested think interval rather
    // than the elapsed time makes a mover run slow, by an amount that varies
    // with the tick rate.
    let mut w = world_from(DOOR_MAP);
    let gate = named(&w, "gate");
    w.accept_input(gate, &InputEvent::new("Open"));

    // 120 units of travel at 100 units/s.
    run(&mut w, 1.15);
    assert!(
        w.get(gate).unwrap().origin.z < 120.0,
        "should not be there yet at 1.15s"
    );
    run(&mut w, 0.15);
    assert_eq!(w.get(gate).unwrap().origin.z, 120.0, "should arrive by about 1.2s");
}

// ---- logic_branch: the alternative ---------------------------------------

/// A branch wired to two counters, so which side fired is a number to read.
const BRANCH_MAP: &str = r#"
entity
{
    "classname" "logic_branch"
    "targetname" "gate_locked"
    "initialvalue" "0"
    connections
    {
        "OnTrue"  "yes,Add,1,0,-1"
        "OnFalse" "no,Add,1,0,-1"
    }
}
entity { "classname" "math_counter" "targetname" "yes" "startvalue" "0" }
entity { "classname" "math_counter" "targetname" "no"  "startvalue" "0" }
"#;

fn branch_world() -> EntityWorld {
    let mut w = world_from(BRANCH_MAP);
    run(&mut w, 0.1);
    w
}

/// Fire an input at the branch.
fn send(w: &mut EntityWorld, input: &str, parameter: &str) {
    w.queue_input(
        kerosene_entity::Target::Named("gate_locked".into()),
        input,
        parameter,
        0.0,
        None,
        None,
    );
    run(w, 0.1);
}

fn test(w: &mut EntityWorld) { send(w, "Test", "") }

fn counts(w: &EntityWorld) -> (f32, f32) {
    (field(w, named(w, "yes"), "value"), field(w, named(w, "no"), "value"))
}

#[test]
fn a_branch_fires_one_side_and_never_both() {
    // The whole point of it: everything else in the game fires a list, this
    // one chooses.
    let mut w = branch_world();
    test(&mut w);
    run(&mut w, 0.1);

    assert_eq!(counts(&w), (0.0, 1.0), "it started false, so the false side fired");
}

#[test]
fn setting_a_value_and_testing_it_fires_the_other_side() {
    let mut w = branch_world();
    send(&mut w, "SetValueTest", "1");
    run(&mut w, 0.1);

    assert_eq!(counts(&w), (1.0, 0.0));
}

#[test]
fn setting_without_testing_fires_nothing_until_asked() {
    // Which is what makes it a memory rather than a relay: a door can record
    // that it is locked long before anything asks.
    let mut w = branch_world();
    send(&mut w, "SetValue", "1");
    run(&mut w, 0.1);
    assert_eq!(counts(&w), (0.0, 0.0), "nothing fired yet");

    test(&mut w);
    run(&mut w, 0.1);
    assert_eq!(counts(&w), (1.0, 0.0), "and it remembered");
}

#[test]
fn toggling_flips_which_side_will_fire() {
    let mut w = branch_world();
    for _ in 0..2 {
        send(&mut w, "ToggleTest", "");
        run(&mut w, 0.1);
    }
    // False -> true fires OnTrue, then true -> false fires OnFalse.
    assert_eq!(counts(&w), (1.0, 1.0));
}

#[test]
fn a_branch_that_starts_true_says_so() {
    let mut w = world_from(&BRANCH_MAP.replace("\"initialvalue\" \"0\"", "\"initialvalue\" \"1\""));
    run(&mut w, 0.1);
    test(&mut w);
    run(&mut w, 0.1);

    assert_eq!(counts(&w), (1.0, 0.0));
}

#[test]
fn a_parameter_of_zero_is_false_rather_than_merely_present() {
    // `SetValue 0` has to mean false. Treating any parameter as "yes" would
    // make the input impossible to use from anything that computes a number.
    let mut w = branch_world();
    send(&mut w, "SetValueTest", "0");
    run(&mut w, 0.1);
    assert_eq!(counts(&w), (0.0, 1.0));
}

#[test]
fn setting_it_with_nothing_attached_means_yes() {
    // Firing `SetValue` from a button with no parameter reads as "make it
    // so", and the alternative -- silently meaning no -- would be a trap.
    let mut w = branch_world();
    send(&mut w, "SetValueTest", "");
    run(&mut w, 0.1);
    assert_eq!(counts(&w), (1.0, 0.0));
}

// ---- buttons -------------------------------------------------------------

const BUTTON_MAP: &str = r#"
entity
{
    "classname" "func_button"
    "targetname" "switch"
    "model" "*1"
    "movedir" "0 0 -1"
    "speed" "40"
    "lip" "4"
    "wait" "1"
    "model_mins" "0 0 0"
    "model_maxs" "16 16 12"
    connections { "OnPressed" "gate,Open,,0,-1" "OnPressed" "count,Add,1,0,-1" }
}
entity
{
    "classname" "math_counter"
    "targetname" "count"
    "startvalue" "0"
}
entity
{
    "classname" "func_door"
    "targetname" "gate"
    "model" "*2"
    "movedir" "0 0 1"
    "speed" "100"
    "lip" "8"
    "wait" "-1"
    "model_mins" "0 0 0"
    "model_maxs" "16 96 128"
}
"#;

fn press(w: &mut EntityWorld, id: EntityId) {
    w.accept_input(id, &InputEvent::new("Use"));
}

#[test]
fn a_button_travels_by_its_own_size_like_a_door_does() {
    // The same mover, so the same rule: 12 deep less a 4-unit lip.
    let w = world_from(BUTTON_MAP);
    let switch = named(&w, "switch");
    assert_eq!(field(&w, switch, "travel"), 8.0);
}

#[test]
fn pressing_a_button_fires_as_it_starts_moving_not_when_it_arrives() {
    // The whole reason OnPressed exists separately from OnIn: a designer wires
    // the door to OnPressed and it starts opening as the button goes in.
    let mut w = world_from(BUTTON_MAP);
    let switch = named(&w, "switch");
    let gate = named(&w, "gate");

    // The button takes 8/40 = 0.2s to go in; the door takes 120/100 = 1.2s to
    // open. Halfway through the button's travel the door must already be
    // moving, which is the claim OnPressed exists to make.
    press(&mut w, switch);
    run(&mut w, 0.1);

    assert!(field(&w, gate, "progress") > 0.0, "the door should already be opening");
    assert!(field(&w, switch, "progress") < 1.0, "while the button is still travelling in");
}

#[test]
fn a_button_pops_back_out_after_its_wait() {
    let mut w = world_from(BUTTON_MAP);
    let switch = named(&w, "switch");

    press(&mut w, switch);
    run(&mut w, 0.5);
    assert_eq!(field(&w, switch, "progress"), 1.0, "8 units at 40 ku/s is in by now");

    // 1 second of wait, then 0.2s to travel back out.
    run(&mut w, 1.5);
    assert_eq!(field(&w, switch, "progress"), 0.0, "and back out again by itself");
}

#[test]
fn a_button_held_in_stays_in() {
    let mut w = world_from(BUTTON_MAP);
    let switch = named(&w, "switch");
    if let Some(e) = w.get_mut(switch) { e.fields.set("wait", Value::Float(-1.0)); }

    press(&mut w, switch);
    run(&mut w, 3.0);

    assert_eq!(field(&w, switch, "progress"), 1.0, "a negative wait means stay put");
}

#[test]
fn pressing_a_button_that_is_already_in_does_nothing() {
    let mut w = world_from(BUTTON_MAP);
    let switch = named(&w, "switch");
    if let Some(e) = w.get_mut(switch) { e.fields.set("wait", Value::Float(-1.0)); }

    press(&mut w, switch);
    run(&mut w, 0.5);
    press(&mut w, switch);
    run(&mut w, 0.5);

    assert_eq!(
        field(&w, named(&w, "count"), "value"), 1.0,
        "a second press while it is already in must not fire OnPressed again"
    );
}

#[test]
fn a_locked_button_says_so_instead_of_pressing() {
    let mut w = world_from(BUTTON_MAP);
    let switch = named(&w, "switch");
    let gate = named(&w, "gate");
    w.get_mut(switch).unwrap().connections.push(Connection::new("OnUseLocked", "gate", "Close"));
    w.accept_input(switch, &InputEvent::new("Lock"));

    press(&mut w, switch);
    run(&mut w, 0.5);

    assert_eq!(field(&w, switch, "progress"), 0.0, "a locked button does not move");
    assert_eq!(field(&w, gate, "progress"), 0.0, "and does not fire what it is wired to");
    assert_eq!(field(&w, named(&w, "count"), "value"), 0.0, "OnPressed must not fire either");
}

#[test]
fn a_button_and_a_door_fire_different_words_for_the_same_movement() {
    // The state machine is shared; the vocabulary is not. A designer wiring a
    // button should never see OnFullyOpen.
    let mut w = world_from(BUTTON_MAP);
    let switch = named(&w, "switch");

    // The button reaches the end of its travel in this time and announces it
    // as OnIn. Anything listening for OnFullyOpen must hear nothing.
    w.get_mut(switch).unwrap().connections.push(Connection::new("OnFullyOpen", "count", "Add"));
    press(&mut w, switch);
    run(&mut w, 0.5);

    assert_eq!(
        field(&w, named(&w, "count"), "value"), 1.0,
        "only OnPressed should have counted: a button must not fire a door's outputs"
    );
}

// ---- the use key ---------------------------------------------------------

#[test]
fn using_a_door_toggles_it() {
    // What the player's use key sends. Without this a door can only be opened
    // by something a designer wired up in advance.
    let mut w = world_from(DOOR_MAP);
    let gate = named(&w, "gate");

    w.accept_input(gate, &InputEvent::new("Use"));
    run(&mut w, 2.0);
    assert_eq!(field(&w, gate, "progress"), 1.0);

    w.accept_input(gate, &InputEvent::new("Use"));
    run(&mut w, 2.0);
    assert_eq!(field(&w, gate, "progress"), 0.0, "and again, to close it");
}

#[test]
fn using_a_locked_door_reports_it_rather_than_opening() {
    let mut w = world_from(DOOR_MAP);
    let gate = named(&w, "gate");
    w.accept_input(gate, &InputEvent::new("Lock"));

    w.accept_input(gate, &InputEvent::new("Use"));
    run(&mut w, 2.0);

    assert_eq!(field(&w, gate, "progress"), 0.0);
}

// ---- rotating brushes ----------------------------------------------------

const FAN_MAP: &str = r#"
entity
{
    "classname" "func_rotating"
    "targetname" "fan"
    "model" "*1"
    "maxspeed" "90"
    "spawnflags" "1"
    "model_mins" "0 0 0"
    "model_maxs" "64 64 8"
}
entity
{
    "classname" "func_rotating"
    "targetname" "stopped"
    "model" "*2"
    "maxspeed" "90"
    "model_mins" "0 0 0"
    "model_maxs" "64 64 8"
}
"#;

fn yaw(w: &EntityWorld, id: EntityId) -> f32 {
    w.get(id).map(|e| e.angles.yaw).unwrap_or(0.0)
}

#[test]
fn a_rotating_brush_flagged_on_starts_turning_by_itself() {
    let mut w = world_from(FAN_MAP);
    let fan = named(&w, "fan");
    run(&mut w, 1.0);

    // 90 degrees a second for a second, less up to one think's worth of lag:
    // movers run on their own cadence, quantised to the tick rate, so the
    // last step before the second is up lands a little short of it.
    assert!((yaw(&w, fan) - 90.0).abs() < 8.0, "turned to {}", yaw(&w, fan));
}

#[test]
fn a_rotating_brush_without_the_flag_stays_still_until_told() {
    let mut w = world_from(FAN_MAP);
    let stopped = named(&w, "stopped");
    run(&mut w, 1.0);
    assert_eq!(yaw(&w, stopped), 0.0);

    w.accept_input(stopped, &InputEvent::new("Start"));
    run(&mut w, 1.0);
    assert!((yaw(&w, stopped) - 90.0).abs() < 8.0, "turned to {}", yaw(&w, stopped));
}

#[test]
fn stopping_a_rotating_brush_leaves_it_where_it_was() {
    let mut w = world_from(FAN_MAP);
    let fan = named(&w, "fan");
    run(&mut w, 1.0);
    w.accept_input(fan, &InputEvent::new("Stop"));
    let stopped_at = yaw(&w, fan);

    run(&mut w, 2.0);
    assert_eq!(yaw(&w, fan), stopped_at, "a stopped fan must not creep");
}

#[test]
fn restarting_does_not_jump_through_the_time_it_was_stopped() {
    // The failure this catches: `last_move` left alone while stopped, so the
    // first think after restarting integrates the whole pause at once and the
    // fan teleports to a new angle.
    let mut w = world_from(FAN_MAP);
    let fan = named(&w, "fan");
    w.accept_input(fan, &InputEvent::new("Stop"));
    let stopped_at = yaw(&w, fan);
    run(&mut w, 5.0);

    w.accept_input(fan, &InputEvent::new("Start"));
    run(&mut w, TICK * 2.0);

    let moved = (yaw(&w, fan) - stopped_at).abs();
    assert!(moved < 10.0, "jumped {moved} degrees on the first think after restarting");
}

#[test]
fn reversing_turns_the_other_way() {
    let mut w = world_from(FAN_MAP);
    let fan = named(&w, "fan");
    run(&mut w, 1.0);
    let forward = yaw(&w, fan);

    w.accept_input(fan, &InputEvent::new("Reverse"));
    run(&mut w, 1.0);

    assert!(yaw(&w, fan) < forward, "should have come back, {} then {}", forward, yaw(&w, fan));
}

#[test]
fn the_axis_is_chosen_by_spawnflag() {
    let source = r#"
entity { "classname" "func_rotating" "targetname" "roll" "model" "*1" "maxspeed" "90"
         "spawnflags" "3" "model_mins" "0 0 0" "model_maxs" "64 64 8" }
entity { "classname" "func_rotating" "targetname" "pitch" "model" "*2" "maxspeed" "90"
         "spawnflags" "5" "model_mins" "0 0 0" "model_maxs" "64 64 8" }
"#;
    let mut w = world_from(source);
    run(&mut w, 1.0);

    let rolling = w.get(named(&w, "roll")).unwrap().angles;
    let pitching = w.get(named(&w, "pitch")).unwrap().angles;

    assert!(rolling.roll.abs() > 80.0 && rolling.yaw == 0.0, "{rolling:?}");
    assert!(pitching.pitch.abs() > 80.0 && pitching.yaw == 0.0, "{pitching:?}");
}

#[test]
fn a_fan_left_running_keeps_its_angle_in_range() {
    // Angles that only ever grow lose precision, and after long enough a
    // float degree count stops being able to represent small steps at all.
    let mut w = world_from(FAN_MAP);
    let fan = named(&w, "fan");
    run(&mut w, 60.0);

    let angles = w.get(fan).unwrap().angles;
    assert!(angles.yaw >= -180.0 && angles.yaw < 180.0, "yaw ran away to {}", angles.yaw);
}

// ---- sounds --------------------------------------------------------------

const SOUND_MAP: &str = r#"
entity
{
    "classname" "ambient_generic"
    "targetname" "hum"
    "sound" "ambient/room_tone"
    "spawnflags" "1"
    "origin" "0 0 0"
}
entity
{
    "classname" "point_sound"
    "targetname" "chime"
    "sound" "ui/click"
    "origin" "64 0 0"
    connections { "OnPlay" "plays,Add,1,0,-1" }
}
entity
{
    "classname" "math_counter"
    "targetname" "plays"
    "startvalue" "0"
}
entity
{
    "classname" "point_sound"
    "targetname" "legacy"
    "message" "ui/click"
    "origin" "128 0 0"
}
"#;

fn requests_of(w: &mut EntityWorld) -> Vec<(String, String)> {
    w.take_requests().into_iter().map(|r| (r.kind.to_string(), r.payload)).collect()
}

#[test]
fn a_one_shot_sound_does_not_play_until_it_is_fired() {
    // The whole difference from an ambience: it is an event, and an event
    // that happened at map load is not an event anyone asked for.
    let mut w = world_from(SOUND_MAP);
    let _ = requests_of(&mut w);
    run(&mut w, 1.0);
    assert!(requests_of(&mut w).is_empty(), "nothing should have played on its own");

    let chime = named(&w, "chime");
    w.accept_input(chime, &InputEvent::new("Play"));
    let played = requests_of(&mut w);
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].1, "ui/click");
}

#[test]
fn a_one_shot_sound_does_not_loop() {
    // The engine reads this field to decide, and it defaults to looping --
    // which is right for an ambience and wrong for a chime that would then
    // never stop.
    let w = world_from(SOUND_MAP);
    let chime = named(&w, "chime");
    assert!(!w.get(chime).unwrap().fields.bool("looping", true));
}

#[test]
fn an_ambience_starts_with_the_map_unless_told_not_to() {
    let mut w = world_from(
        r#"entity { "classname" "ambient_generic" "targetname" "hum" "sound" "ambient/room_tone" }"#,
    );
    assert_eq!(requests_of(&mut w).len(), 1, "an ambience is a bed and beds start");
}

#[test]
fn the_source_spelling_of_the_sound_key_still_works() {
    // `message` is what Source calls it, for a reason that made sense in 1998.
    // Anyone who wrote it, or copied a Source example, should not be left
    // wondering why nothing plays.
    let mut w = world_from(SOUND_MAP);
    let _ = requests_of(&mut w);
    let legacy = named(&w, "legacy");
    w.accept_input(legacy, &InputEvent::new("Play"));

    let played = requests_of(&mut w);
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].1, "ui/click");
}

#[test]
fn a_sound_entity_naming_nothing_says_so_rather_than_playing_silence() {
    let mut w = world_from(r#"entity { "classname" "point_sound" "targetname" "quiet" }"#);
    let quiet = named(&w, "quiet");
    assert!(!w.accept_input(quiet, &InputEvent::new("Play")), "it should report failure");
    assert!(requests_of(&mut w).is_empty());
}

#[test]
fn firing_a_one_shot_twice_plays_it_twice() {
    // An ambience is a switch and refuses a second start; an event is not.
    let mut w = world_from(SOUND_MAP);
    let _ = requests_of(&mut w);
    let chime = named(&w, "chime");
    w.accept_input(chime, &InputEvent::new("Play"));
    w.accept_input(chime, &InputEvent::new("Play"));
    assert_eq!(requests_of(&mut w).len(), 2);
}

#[test]
fn a_one_shot_announces_itself_so_something_can_follow_it() {
    // A door that plays its own noise wants to know when the noise started;
    // so does anything sequencing one sound after another.
    let mut w = world_from(SOUND_MAP);
    let chime = named(&w, "chime");
    w.accept_input(chime, &InputEvent::new("Play"));
    run(&mut w, TICK * 2.0);

    assert_eq!(field(&w, named(&w, "plays"), "value"), 1.0);
}

// ---- physics props and the spawner ---------------------------------------

#[test]
fn a_spawner_drops_a_batch_when_triggered() {
    let mut w = world_from(r#"
entity {
    "classname" "prop_dynamic_spawner"
    "targetname" "dropper"
    "model" "props/cube"
    "spawncount" "3"
}
"#);
    let dropper = named(&w, "dropper");
    w.accept_input(dropper, &InputEvent::new("Trigger"));

    let props = w.find_by_class("prop_physics");
    assert_eq!(props.len(), 3, "one batch of three props");
    for id in &props {
        assert_eq!(w.get(*id).unwrap().fields.text("model"), Some("props/cube"));
    }
}

#[test]
fn a_spawner_can_spawn_on_map_start() {
    let w = world_from(r#"
entity {
    "classname" "prop_dynamic_spawner"
    "model" "props/cube"
    "spawnflags" "1"
}
"#);
    assert_eq!(w.find_by_class("prop_physics").len(), 1, "spawned once at map start");
}

#[test]
fn a_spawner_respects_its_total_limit() {
    let mut w = world_from(r#"
entity {
    "classname" "prop_dynamic_spawner"
    "targetname" "dropper"
    "model" "props/cube"
    "spawncount" "5"
    "maxprops" "3"
}
"#);
    let dropper = named(&w, "dropper");
    w.accept_input(dropper, &InputEvent::new("Trigger"));
    w.accept_input(dropper, &InputEvent::new("Trigger"));
    assert_eq!(w.find_by_class("prop_physics").len(), 3, "capped at maxprops");
}

#[test]
fn breaking_a_prop_removes_it() {
    let mut w = world_from(r#"
entity { "classname" "prop_physics" "targetname" "box" "model" "props/cube" }
"#);
    let box_id = named(&w, "box");
    assert!(w.accept_input(box_id, &InputEvent::new("Break")));
    run(&mut w, TICK);
    assert!(!w.exists(box_id), "Break removes the prop");
}
