// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;

fn world() -> WorldView {
    let mut view = WorldView {
        time: 12.5,
        tick: 800,
        map: "atrium".into(),
        ..Default::default()
    };
    view.entities.push(
        EntityView::new(1, "func_door")
            .with_name("gate")
            .with_origin(Vec3::new(100.0, 0.0, 0.0))
            .with_field("speed", "250"),
    );
    view.entities.push(EntityView::new(2, "func_door").with_name("gate"));
    view.entities.push(EntityView::new(3, "light").with_origin(Vec3::new(0.0, 0.0, 128.0)));
    view.entities.push(EntityView::new(4, "light"));
    view.cvars.insert("sv_gravity".into(), "800".into());
    view.player = Some(EntityView::new(99, "player").with_origin(Vec3::new(5.0, 6.0, 7.0)));
    view
}

fn host() -> ScriptHost {
    let mut host = ScriptHost::new();
    host.set_view(world());
    host
}

fn run(source: &str) -> Vec<ScriptAction> {
    let mut host = host();
    host.run(source).unwrap_or_else(|e| panic!("{source}\n{e}"));
    host.take_actions()
}

// ---- the basics -----------------------------------------------------------

#[test]
fn a_script_can_be_evaluated_for_its_value() {
    let mut host = host();
    assert_eq!(host.run("2 + 3").unwrap().as_deref(), Some("5"));
    assert_eq!(host.run("let x = 1;").unwrap(), None, "a statement has no value to show");
}

#[test]
fn state_survives_between_console_lines() {
    // Typing at the console is a conversation, not a series of unrelated
    // programs: a variable set on one line is there on the next.
    let mut host = host();
    host.run("let counter = 5;").unwrap();
    assert_eq!(host.run("counter + 1").unwrap().as_deref(), Some("6"));
}

#[test]
fn a_syntax_error_names_itself_rather_than_panicking() {
    let mut host = host();
    let err = host.run("this is not rhai (((").unwrap_err();
    assert!(matches!(err, ScriptError::Compile(_)), "{err}");
}

#[test]
fn a_runtime_error_is_returned_not_raised() {
    let mut host = host();
    let err = host.run(r#" throw "deliberate" "#).unwrap_err();
    assert!(matches!(err, ScriptError::Runtime(_)), "{err}");
}

// ---- printing -------------------------------------------------------------

#[test]
fn print_reaches_the_console() {
    let actions = run(r#" print("hello"); "#);
    assert_eq!(actions, vec![ScriptAction::Log(ScriptLevel::Print, "hello".into())]);
}

#[test]
fn warnings_and_errors_carry_their_severity() {
    let actions = run(r#" warn("careful"); error("broken"); "#);
    assert_eq!(
        actions,
        vec![
            ScriptAction::Log(ScriptLevel::Warn, "careful".into()),
            ScriptAction::Log(ScriptLevel::Error, "broken".into()),
        ]
    );
}

// ---- reading the world ----------------------------------------------------

#[test]
fn a_script_can_read_the_clock_and_the_map() {
    let mut host = host();
    assert_eq!(host.run("time()").unwrap().as_deref(), Some("12.5"));
    assert_eq!(host.run("tick()").unwrap().as_deref(), Some("800"));
    assert_eq!(host.run("map_name()").unwrap().as_deref(), Some("atrium"));
    assert_eq!(host.run("entity_count()").unwrap().as_deref(), Some("4"));
}

#[test]
fn convars_are_readable_as_text_and_as_numbers() {
    let mut host = host();
    assert_eq!(host.run(r#" cvar("sv_gravity") "#).unwrap().as_deref(), Some("800"));
    assert_eq!(host.run(r#" cvar_float("sv_gravity") "#).unwrap().as_deref(), Some("800.0"));
    // A convar nobody registered reads as empty rather than failing: a script
    // asking about an optional setting is normal.
    assert_eq!(host.run(r#" cvar("nope") "#).unwrap().as_deref(), Some(""));
    assert_eq!(host.run(r#" cvar_float("nope") "#).unwrap().as_deref(), Some("0.0"));
}

#[test]
fn entities_are_found_by_name_and_by_class() {
    let mut host = host();
    assert_eq!(host.run(r#" find_by_name("gate").classname "#).unwrap().as_deref(), Some("func_door"));
    assert_eq!(host.run(r#" find_all_by_name("gate").len "#).unwrap().as_deref(), Some("2"));
    assert_eq!(host.run(r#" find_by_class("light").len "#).unwrap().as_deref(), Some("2"));
}

#[test]
fn an_entity_that_is_not_there_is_unit_rather_than_an_error() {
    // Asking whether something exists is not a mistake, so it must not throw.
    let mut host = host();
    assert_eq!(host.run(r#" find_by_name("nothing") == () "#).unwrap().as_deref(), Some("true"));
}

#[test]
fn keyvalues_are_readable() {
    let mut host = host();
    assert_eq!(host.run(r#" find_by_name("gate").get("speed") "#).unwrap().as_deref(), Some("250"));
    assert_eq!(
        host.run(r#" find_by_name("gate").get_float("speed") "#).unwrap().as_deref(),
        Some("250.0")
    );
    assert_eq!(host.run(r#" find_by_name("gate").has("lip") "#).unwrap().as_deref(), Some("false"));
}

#[test]
fn the_player_is_reachable_when_there_is_one() {
    let mut host = host();
    assert_eq!(host.run("player().origin.x").unwrap().as_deref(), Some("5.0"));

    let mut view = world();
    view.player = None;
    host.set_view(view);
    assert_eq!(host.run("player() == ()").unwrap().as_deref(), Some("true"));
}

// ---- acting on the world --------------------------------------------------

#[test]
fn ent_fire_reaches_the_engine_as_an_action() {
    assert_eq!(
        run(r#" ent_fire("gate", "Open"); "#),
        vec![ScriptAction::FireInput {
            target: "gate".into(),
            input: "Open".into(),
            parameter: String::new(),
            delay: 0.0,
        }]
    );
}

#[test]
fn ent_fire_takes_a_parameter_and_a_delay() {
    assert_eq!(
        run(r#" ent_fire("counter", "SetValue", "3", 1.5); "#),
        vec![ScriptAction::FireInput {
            target: "counter".into(),
            input: "SetValue".into(),
            parameter: "3".into(),
            delay: 1.5,
        }]
    );
}

#[test]
fn a_negative_delay_becomes_immediate_rather_than_going_backwards() {
    let actions = run(r#" ent_fire("gate", "Open", "", -5.0); "#);
    match &actions[0] {
        ScriptAction::FireInput { delay, .. } => assert_eq!(*delay, 0.0),
        other => panic!("{other:?}"),
    }
}

#[test]
fn firing_at_an_entity_handle_uses_its_name_when_it_has_one() {
    assert_eq!(
        run(r#" find_by_name("gate").fire("Close"); "#),
        vec![ScriptAction::FireInput {
            target: "gate".into(),
            input: "Close".into(),
            parameter: String::new(),
            delay: 0.0,
        }]
    );
}

#[test]
fn firing_at_a_nameless_entity_addresses_that_one_and_no_other() {
    // Without this, acting on one of a dozen unnamed lights would address all
    // of them or none.
    let actions = run(r#" find_by_class("light")[1].fire("Kill"); "#);
    match &actions[0] {
        ScriptAction::FireInput { target, .. } => {
            assert_eq!(target, "!id:4");
            assert_eq!(parse_id_target(target), Some(4));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(parse_id_target("gate"), None);
}

#[test]
fn setting_a_keyvalue_names_the_entity_by_handle() {
    assert_eq!(
        run(r#" find_by_name("gate").set("speed", 400.0); "#),
        vec![ScriptAction::SetField { entity: 1, key: "speed".into(), value: "400".into() }]
    );
    assert_eq!(
        run(r#" find_by_name("gate").set("message", "hello"); "#),
        vec![ScriptAction::SetField { entity: 1, key: "message".into(), value: "hello".into() }]
    );
}

#[test]
fn moving_and_removing_an_entity() {
    assert_eq!(
        run(r#" find_by_name("gate").set_origin(Vector(1.0, 2.0, 3.0)); "#),
        vec![ScriptAction::SetOrigin { entity: 1, origin: Vec3::new(1.0, 2.0, 3.0) }]
    );
    assert_eq!(run(r#" find_by_name("gate").kill(); "#), vec![ScriptAction::Kill { entity: 1 }]);
}

#[test]
fn console_commands_go_through_the_console() {
    assert_eq!(
        run(r#" command("map atrium"); "#),
        vec![ScriptAction::Command("map atrium".into())]
    );
    // Setting a convar is the same path, so cheat flags are enforced in one
    // place rather than two.
    assert_eq!(
        run(r#" set_cvar("sv_gravity", "200"); "#),
        vec![ScriptAction::Command("sv_gravity \"200\"".into())]
    );
}

#[test]
fn actions_come_out_in_the_order_the_script_asked_for_them() {
    let actions = run(r#"
        ent_fire("a", "Open");
        print("between");
        ent_fire("b", "Close");
    "#);
    assert_eq!(actions.len(), 3);
    assert!(matches!(actions[0], ScriptAction::FireInput { .. }));
    assert!(matches!(actions[1], ScriptAction::Log(..)));
    assert!(matches!(actions[2], ScriptAction::FireInput { .. }));
}

#[test]
fn taking_actions_empties_the_queue() {
    let mut host = host();
    host.run(r#" print("once"); "#).unwrap();
    assert_eq!(host.take_actions().len(), 1);
    assert!(host.take_actions().is_empty());
}

// ---- vectors --------------------------------------------------------------

#[test]
fn vector_arithmetic_works_the_way_it_reads() {
    let mut host = host();
    assert_eq!(
        host.run("(Vector(1.0,2.0,3.0) + Vector(1.0,1.0,1.0)).to_string()").unwrap().as_deref(),
        Some("2 3 4")
    );
    assert_eq!(host.run("(Vector(3.0,4.0,0.0)).length()").unwrap().as_deref(), Some("5.0"));
    assert_eq!(
        host.run("distance(Vector(0.0,0.0,0.0), Vector(0.0,0.0,10.0))").unwrap().as_deref(),
        Some("10.0")
    );
}

#[test]
fn an_entitys_position_is_a_vector_a_script_can_do_maths_on() {
    let mut host = host();
    assert_eq!(
        host.run(r#" distance(find_by_name("gate").origin, player().origin) > 0.0 "#)
            .unwrap()
            .as_deref(),
        Some("true")
    );
}

// ---- loading and hooks ----------------------------------------------------

#[test]
fn a_loaded_script_defines_functions_that_stay_callable() {
    let mut host = host();
    host.load("test", r#" fn greet() { print("hi"); } "#).unwrap();
    assert!(host.has_function("greet"));
    host.call("greet", vec![]).unwrap();
    assert_eq!(host.take_actions(), vec![ScriptAction::Log(ScriptLevel::Print, "hi".into())]);
    assert_eq!(host.loaded(), ["test"]);
}

#[test]
fn a_loaded_scripts_functions_are_callable_from_the_console_too() {
    let mut host = host();
    host.load("test", r#" fn double(n) { n * 2 } "#).unwrap();
    assert_eq!(host.run("double(21)").unwrap().as_deref(), Some("42"));
}

#[test]
fn loading_a_file_again_replaces_what_it_defined() {
    // Which is what makes a reload during development a reload.
    let mut host = host();
    host.load("test", r#" fn value() { 1 } "#).unwrap();
    host.load("test", r#" fn value() { 2 } "#).unwrap();
    assert_eq!(host.run("value()").unwrap().as_deref(), Some("2"));
    assert_eq!(host.loaded().len(), 1, "the same file counted twice");
}

#[test]
fn clearing_forgets_everything_the_scripts_defined() {
    let mut host = host();
    host.load("test", r#" fn gone() { 1 } "#).unwrap();
    host.clear();
    assert!(!host.has_function("gone"));
    assert!(host.loaded().is_empty());
}

#[test]
fn calling_a_function_that_is_not_there_says_so() {
    let mut host = host();
    let err = host.call("nope", vec![]).unwrap_err();
    assert!(matches!(err, ScriptError::NoSuchFunction(_)), "{err}");
}

#[test]
fn a_missing_hook_is_normal_and_a_present_one_runs() {
    // The engine calls hooks by name every map load; most maps define none.
    let mut host = host();
    host.call_hook(hooks::MAP_START, vec![]).expect("a missing hook is not an error");

    host.load("m", &format!(r#" fn {}() {{ ent_fire("gate", "Open"); }} "#, hooks::MAP_START))
        .unwrap();
    host.call_hook(hooks::MAP_START, vec![]).unwrap();
    assert_eq!(host.take_actions().len(), 1);
}

#[test]
fn a_tick_hook_is_handed_the_tick_length() {
    let mut host = host();
    host.load("m", &format!(r#" fn {}(dt) {{ print(`${{dt}}`); }} "#, hooks::TICK)).unwrap();
    host.call_hook(hooks::TICK, vec![rhai::Dynamic::from(0.015625_f64)]).unwrap();
    assert_eq!(
        host.take_actions(),
        vec![ScriptAction::Log(ScriptLevel::Print, "0.015625".into())]
    );
}

// ---- limits ---------------------------------------------------------------

#[test]
fn an_infinite_loop_stops_the_script_not_the_game() {
    // A level's scripts are content, edited by people who make mistakes.
    let mut host = host();
    let err = host.run("let i = 0; while true { i += 1; }").unwrap_err();
    assert!(matches!(err, ScriptError::Runtime(_)), "{err}");
}

#[test]
fn a_runaway_loop_cannot_queue_unbounded_actions() {
    let mut host = host();
    // Deliberately more iterations than the cap allows.
    let _ = host.run(&format!(
        "for i in 0..{} {{ ent_fire(\"x\", \"Y\"); }}",
        MAX_ACTIONS + 500
    ));
    assert_eq!(host.take_actions().len(), MAX_ACTIONS);
}

#[test]
fn a_script_cannot_reach_the_file_system() {
    // `import` would be a way around every bound above, so there is no module
    // resolver at all.
    let mut host = host();
    assert!(host.run(r#" import "anything" as m; "#).is_err());
}

#[test]
fn unbounded_recursion_is_stopped() {
    let mut host = host();
    host.load("m", r#" fn deep(n) { deep(n + 1) } "#).unwrap();
    assert!(host.run("deep(0)").is_err());
}
