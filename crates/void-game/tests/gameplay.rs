//! Behaviour tests for the game's entity classes.
//!
//! These drive the classes through the same entity world the engine uses, so
//! they exercise the real I/O routing rather than calling handlers directly.

use void_entity::{Connection, EntityId, EntityWorld, InputEvent, Value};
use void_kv::KeyValues;
use void_math::Vec3;

const TICK: f32 = 1.0 / 64.0;

fn world_from(src: &str) -> EntityWorld {
    let mut w = EntityWorld::new(void_game::registry());
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
        void_game::triggers::update_touch(&mut w, zone, true, None);
        w.run(TICK);
    }
    assert_eq!(field(&w, counter, "value"), 1.0);

    for _ in 0..20 {
        void_game::triggers::update_touch(&mut w, zone, false, None);
        w.run(TICK);
    }
    assert_eq!(field(&w, counter, "value"), 0.0);
}

#[test]
fn a_trigger_once_removes_itself_after_firing() {
    let src = TRIGGER_MAP.replace("trigger_multiple", "trigger_once");
    let mut w = world_from(&src);
    let zone = named(&w, "zone");
    void_game::triggers::update_touch(&mut w, zone, true, None);
    w.run(TICK);
    assert!(!w.exists(zone), "a trigger_once should be gone after it fires");
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);
}

#[test]
fn a_disabled_trigger_does_not_fire() {
    let mut w = world_from(TRIGGER_MAP);
    let zone = named(&w, "zone");
    w.accept_input(zone, &InputEvent::new("Disable"));
    void_game::triggers::update_touch(&mut w, zone, true, None);
    w.run(TICK);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 0.0);

    w.accept_input(zone, &InputEvent::new("Enable"));
    void_game::triggers::update_touch(&mut w, zone, true, None);
    w.run(TICK);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);
}

#[test]
fn disabling_an_occupied_trigger_releases_it() {
    // Otherwise the trigger believes it is occupied forever and never fires
    // OnStartTouch again.
    let mut w = world_from(TRIGGER_MAP);
    let zone = named(&w, "zone");
    void_game::triggers::update_touch(&mut w, zone, true, None);
    w.run(TICK);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 1.0);

    w.accept_input(zone, &InputEvent::new("Disable"));
    w.run(TICK);
    assert_eq!(field(&w, named(&w, "counter"), "value"), 0.0, "OnEndTouch should have fired");

    w.accept_input(zone, &InputEvent::new("Enable"));
    void_game::triggers::update_touch(&mut w, zone, true, None);
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
    void_game::triggers::update_touch(&mut w, zone, true, None);
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
    assert!(void_game::doors::brush_enabled(&w, wall));
    w.accept_input(wall, &InputEvent::new("Disable"));
    assert!(!void_game::doors::brush_enabled(&w, wall));
    w.accept_input(wall, &InputEvent::new("Toggle"));
    assert!(void_game::doors::brush_enabled(&w, wall));
}

#[test]
fn setting_a_field_directly_still_works_for_engine_code() {
    let mut w = EntityWorld::new(void_game::registry());
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
