// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;
use crate::ConVarFlags;

fn console() -> Console {
    let mut con = Console::new();
    con.register_cvar("sv_gravity", "800", ConVarFlags::NONE, "gravity");
    con.register_cvar("sv_maxspeed", "320", ConVarFlags::NONE, "speed");
    con.register_cvar("sv_cheats", "0", ConVarFlags::NONE, "cheats");
    con.register_cvar("cl_fov", "90", ConVarFlags::NONE, "fov");
    con
}

#[test]
fn it_opens_and_closes() {
    let mut ui = ConsoleUi::new();
    assert!(!ui.open);
    ui.toggle();
    assert!(ui.open);
    ui.close();
    assert!(!ui.open);
}

#[test]
fn submitting_runs_the_line_and_clears_the_input() {
    let mut con = console();
    let mut ui = ConsoleUi::new();
    ui.set_input("sv_gravity 200");
    assert_eq!(ui.submit(&mut con).as_deref(), Some("sv_gravity 200"));
    assert_eq!(con.float("sv_gravity"), 200.0);
    assert!(ui.input.is_empty());
}

#[test]
fn submitting_nothing_runs_nothing() {
    let mut con = console();
    let mut ui = ConsoleUi::new();
    ui.set_input("   ");
    assert_eq!(ui.submit(&mut con), None);
}

// ---- history --------------------------------------------------------------

#[test]
fn up_walks_back_through_what_was_run() {
    let mut con = console();
    let mut ui = ConsoleUi::new();
    for line in ["sv_gravity 100", "sv_gravity 200", "sv_gravity 300"] {
        ui.set_input(line);
        ui.submit(&mut con);
    }

    ui.history_previous(&con);
    assert_eq!(ui.input, "sv_gravity 300");
    ui.history_previous(&con);
    assert_eq!(ui.input, "sv_gravity 200");
    ui.history_previous(&con);
    assert_eq!(ui.input, "sv_gravity 100");
    // The far end holds rather than wrapping: wrapping loses your place.
    ui.history_previous(&con);
    assert_eq!(ui.input, "sv_gravity 100");
}

#[test]
fn walking_back_and_forward_returns_the_half_typed_line() {
    // The detail every console gets wrong once: you are typing something,
    // reach for history to check a value, and come back to find your line
    // gone.
    let mut con = console();
    let mut ui = ConsoleUi::new();
    ui.set_input("sv_maxspeed 500");
    ui.submit(&mut con);

    ui.set_input("half typed");
    ui.history_previous(&con);
    assert_eq!(ui.input, "sv_maxspeed 500");
    ui.history_next(&con);
    assert_eq!(ui.input, "half typed", "the draft did not come back");
}

#[test]
fn forward_from_the_present_does_nothing() {
    let con = console();
    let mut ui = ConsoleUi::new();
    ui.set_input("typing");
    ui.history_next(&con);
    assert_eq!(ui.input, "typing");
}

#[test]
fn history_on_an_empty_console_is_not_an_error() {
    let con = console();
    let mut ui = ConsoleUi::new();
    ui.history_previous(&con);
    assert_eq!(ui.input, "");
}

// ---- completion -----------------------------------------------------------

#[test]
fn one_candidate_completes_and_adds_the_space() {
    let mut con = console();
    con.register_cvar("r_novis", "0", ConVarFlags::NONE, "vis");
    let mut ui = ConsoleUi::new();
    ui.set_input("r_no");
    ui.complete(&con);
    assert_eq!(ui.input, "r_novis ");
    assert!(ui.completions().is_empty(), "nothing left to cycle through");
}

#[test]
fn several_candidates_fill_in_as_far_as_they_agree() {
    let con = console();
    let mut ui = ConsoleUi::new();
    ui.set_input("sv_");
    ui.complete(&con);
    // sv_cheats, sv_gravity, sv_maxspeed share nothing past "sv_", so the
    // first candidate is offered rather than nothing happening.
    assert_eq!(ui.completions().len(), 3, "{:?}", ui.completions());
    assert!(ui.input.starts_with("sv_"));
}

#[test]
fn a_shared_prefix_is_filled_in_before_cycling_starts() {
    let mut con = console();
    con.register_cvar("mat_exposure", "1", ConVarFlags::NONE, "");
    con.register_cvar("mat_fullbright", "0", ConVarFlags::NONE, "");
    let mut ui = ConsoleUi::new();
    ui.set_input("ma");
    ui.complete(&con);
    assert_eq!(ui.input, "mat_", "the agreed-on part is filled in first");
}

#[test]
fn pressing_tab_again_cycles_through_the_candidates() {
    let con = console();
    let mut ui = ConsoleUi::new();
    ui.set_input("sv_");
    ui.complete(&con);
    let first = ui.input.clone();
    let count = ui.completions().len();
    assert_eq!(count, 3, "{:?}", ui.completions());

    ui.complete(&con);
    assert_ne!(ui.input, first, "tab did not move on");
    // Once round the whole list and back to where it started.
    for _ in 1..count { ui.complete(&con); }
    assert_eq!(ui.input, first, "the cycle did not come back round");
}

#[test]
fn typing_after_a_completion_starts_a_new_one() {
    let con = console();
    let mut ui = ConsoleUi::new();
    ui.set_input("sv_");
    ui.complete(&con);
    assert!(!ui.completions().is_empty());
    ui.set_input("cl_");
    assert!(ui.completions().is_empty(), "a stale cycle survived an edit");
}

#[test]
fn completion_leaves_arguments_alone() {
    // Completing a value would fight the person typing it.
    let con = console();
    let mut ui = ConsoleUi::new();
    ui.set_input("sv_gravity 8");
    ui.complete(&con);
    assert_eq!(ui.input, "sv_gravity 8");
}

#[test]
fn completing_nothing_matches_nothing() {
    let con = console();
    let mut ui = ConsoleUi::new();
    ui.set_input("zzz_no_such_thing");
    ui.complete(&con);
    assert_eq!(ui.input, "zzz_no_such_thing");
}

// ---- scrollback -----------------------------------------------------------

#[test]
fn the_window_is_pinned_to_the_newest_lines_by_default() {
    let ui = ConsoleUi::new();
    assert_eq!(ui.visible_range(100, 10), 90..100);
}

#[test]
fn paging_up_moves_the_window_back() {
    let mut ui = ConsoleUi::new();
    ui.scroll_up(100);
    assert_eq!(ui.visible_range(100, 10), 80..90);
    ui.scroll_down();
    assert_eq!(ui.visible_range(100, 10), 90..100);
}

#[test]
fn a_log_shorter_than_the_window_shows_all_of_it() {
    let ui = ConsoleUi::new();
    assert_eq!(ui.visible_range(3, 10), 0..3);
    assert_eq!(ui.visible_range(0, 10), 0..0);
}

#[test]
fn scrolling_cannot_walk_off_the_top() {
    let mut ui = ConsoleUi::new();
    for _ in 0..100 { ui.scroll_up(30); }
    let range = ui.visible_range(30, 10);
    assert!(range.start < range.end || range.is_empty());
    assert!(range.end <= 30);
}

#[test]
fn submitting_snaps_back_to_the_newest_line() {
    // Otherwise you run a command while scrolled back and cannot see what it
    // said.
    let mut con = console();
    let mut ui = ConsoleUi::new();
    ui.scroll_up(100);
    assert_ne!(ui.scroll, 0);
    ui.set_input("sv_gravity 1");
    ui.submit(&mut con);
    assert_eq!(ui.scroll, 0);
}

#[test]
fn the_shared_prefix_of_nothing_is_nothing() {
    assert_eq!(common_prefix(&[]), "");
    assert_eq!(common_prefix(&["only".to_string()]), "only");
    assert_eq!(common_prefix(&["abc".into(), "abd".into()]), "ab");
    assert_eq!(common_prefix(&["abc".into(), "xyz".into()]), "");
}

// ---- introducing itself ---------------------------------------------------

#[test]
fn the_console_says_what_it_is_the_first_time_it_opens() {
    // An empty box with a blinking cursor reads as "this accepts nothing".
    let mut console = Console::new();
    let mut ui = ConsoleUi::new();
    ui.greet(&mut console);

    let said: String = console.log().map(|l| l.text.clone()).collect();
    assert!(said.contains("find"), "{said}");
    assert!(said.contains("help"), "{said}");
    assert!(said.contains("cvarlist"), "{said}");
    assert!(said.contains("escape"), "it says how to leave: {said}");
}

#[test]
fn the_greeting_counts_what_is_actually_registered() {
    let mut console = Console::new();
    let builtins = console.name_count();
    assert!(builtins > 0, "a console with no builtins is a broken console");

    console.register_cvar("sv_wibble", "1", crate::ConVarFlags::NONE, "test");
    assert_eq!(console.name_count(), builtins + 1);

    let mut ui = ConsoleUi::new();
    ui.greet(&mut console);
    let said: String = console.log().map(|l| l.text.clone()).collect();
    assert!(said.contains(&(builtins + 1).to_string()), "{said}");
}

#[test]
fn the_console_introduces_itself_once_and_then_stops() {
    // After the first time it is noise between you and the output you opened
    // the console to read.
    let mut console = Console::new();
    let mut ui = ConsoleUi::new();
    ui.greet(&mut console);
    let after_first = console.log_len();

    ui.greet(&mut console);
    ui.greet(&mut console);
    assert_eq!(console.log_len(), after_first);
}

#[test]
fn a_hidden_convar_is_not_counted_among_the_things_you_can_type() {
    let mut console = Console::new();
    let before = console.name_count();
    console.register_cvar("sv_secret", "1", crate::ConVarFlags::HIDDEN, "test");
    assert_eq!(console.name_count(), before, "hidden means hidden");
}
