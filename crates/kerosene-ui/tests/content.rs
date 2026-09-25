// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The layouts the repository ships, loaded and run.
//!
//! Every file under `content/ui` is loaded, given the values a running game
//! publishes, driven for a few frames and poked -- and must produce no
//! warning at all. A typo in a property name or a binding is otherwise
//! invisible until someone looks at the console in the running game.

use kerosene_ui::{Loader, LogLevel, UiAction, UiInput, UiKey, UiStore, UiSystem};
use std::path::PathBuf;

struct Content(PathBuf);

impl Loader for Content {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(path)).ok()
    }
}

fn content() -> Content {
    Content(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content"))
}

/// What the engine and the stock game publish.
fn store() -> UiStore {
    let mut s = UiStore::new();
    s.set("player.health", 100.0);
    s.set("player.alive", true);
    s.set("player.speed", 250.0);
    s.set("map.name", "kero_start");
    s.set("weapon.active", "pistol");
    s.set("weapon.slot", 1.0);
    s.set("weapon.ammo", 12.0);
    s.set("weapon.clip", 12.0);
    s.set("weapon.reserve", 48.0);
    s.set("weapon.reloading", false);
    s.set("weapon.reload_progress", 0.0);
    s.set("weapon.firing", false);
    s.set("weapon.count", 3.0);
    for (i, name) in ["pistol", "shotgun", "rifle"].iter().enumerate() {
        s.set(&format!("weapons.{i}.name"), *name);
        s.set(&format!("weapons.{i}.slot"), (i + 1) as f64);
        s.set(&format!("weapons.{i}.active"), i == 0);
    }
    s.set("ability.dash.ready", true);
    s.set("ability.dash.charge", 1.0);
    s.set("ability.dash.remaining", 0.0);
    s.set("cvar.volume", 1);
    s.set("cvar.cl_fov", 90);
    s.set("cvar.sensitivity", 3);
    s.set("cvar.m_invert", 0);
    s.set("cvar.ui_debug", 0);
    s
}

fn problems(ui: &mut UiSystem) -> Vec<String> {
    ui.take_messages()
        .into_iter()
        .filter(|(level, _)| *level != LogLevel::Info)
        .map(|(_, m)| m)
        .collect()
}

#[test]
fn the_hud_and_its_overlay_run_clean() {
    let files = content();
    let mut ui = UiSystem::new();
    let mut s = store();
    ui.show("hud", "ui/hud.keroui", &files).unwrap();
    ui.update(0.016, (1920, 1080), &mut s, &files);
    assert!(ui.is_visible("overlay"), "the HUD puts the overlay up");

    // A fight: shooting, switching, running dry, dashing, getting hurt.
    s.set("weapon.firing", true);
    s.set("weapon.active", "shotgun");
    s.set("weapon.ammo", 0.0);
    s.set("ability.dash.ready", false);
    s.set("ability.dash.charge", 0.3);
    s.set("ability.dash.remaining", 2.1);
    s.set("player.health", 20.0);
    s.set("objective.text", "Open the shutter");
    for event in [
        "weapon_empty",
        "weapon_reload",
        "ability_used",
        "ability_ready",
        "player_damaged",
        "map_loaded",
    ] {
        s.emit(event, "x");
    }
    for _ in 0..30 {
        ui.update(0.05, (1920, 1080), &mut s, &files);
    }
    ui.update(0.016, (1280, 720), &mut s, &files);
    assert_eq!(problems(&mut ui), Vec::<String>::new());

    let hud = ui.document("hud").unwrap();
    let xh = hud.find("crosshair").unwrap();
    assert!(hud.classes(xh).contains(&"xh-shotgun".to_string()));
    assert!(
        hud.classes(hud.find("health").unwrap())
            .contains(&"low".to_string())
    );
    assert!(!ui.display_list().is_empty());
}

#[test]
fn the_pause_menu_runs_clean_and_its_controls_work() {
    let files = content();
    let mut ui = UiSystem::new();
    let mut s = store();
    ui.show("menu", "ui/menus/pause.keroui", &files).unwrap();
    ui.update(0.016, (1920, 1080), &mut s, &files);
    assert!(ui.wants_input());

    // Down to Options and in.
    ui.input(UiInput::Key(UiKey::Down));
    ui.input(UiInput::Key(UiKey::Down));
    ui.input(UiInput::Key(UiKey::Enter));
    ui.update(0.016, (1920, 1080), &mut s, &files);
    ui.update(0.016, (1920, 1080), &mut s, &files);
    let menu = ui.document("menu").unwrap();
    let focused = menu
        .focused()
        .and_then(|f| menu.attr(f, "text"))
        .map(str::to_string);
    assert_eq!(
        s.get("ui.page").map(|v| v.to_string()),
        Some("options".into()),
        "focus on {focused:?}; {:?}",
        ui.take_messages()
    );

    // The first control is the volume slider; nudge it.
    ui.input(UiInput::Key(UiKey::Tab));
    ui.input(UiInput::Key(UiKey::Left));
    let actions = ui.update(0.016, (1920, 1080), &mut s, &files);
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, UiAction::SetCvar { name, .. } if name == "volume")),
        "{actions:?}"
    );
    for _ in 0..10 {
        ui.update(0.05, (1920, 1080), &mut s, &files);
    }
    assert_eq!(problems(&mut ui), Vec::<String>::new());
}

#[test]
fn the_keypad_checks_its_code() {
    let files = content();
    let mut ui = UiSystem::new();
    let mut s = store();
    ui.set_panel("keypad", "ui/panels/keypad.keroui", (320, 448), &files)
        .unwrap();
    ui.update(0.016, (1920, 1080), &mut s, &files);
    assert_eq!(
        s.get("keypad.shown").map(|v| v.to_string()),
        Some("----".into())
    );

    let doc = ui.panel_document("keypad").unwrap();
    let centre = |label: &str| {
        // The button whose label reads `label`.
        let id = doc.find_by_attr("text", label).unwrap();
        let r = doc.rect(id);
        (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
    };
    let presses: Vec<(f32, f32)> = ["1", "2", "3", "4", "OK"]
        .iter()
        .map(|k| centre(k))
        .collect();
    let mut emitted = Vec::new();
    for (x, y) in presses {
        ui.panel_input("keypad", UiInput::PointerMove { x, y });
        ui.panel_input("keypad", UiInput::PointerButton { down: true });
        ui.panel_input("keypad", UiInput::PointerButton { down: false });
        emitted.extend(ui.update(0.016, (1920, 1080), &mut s, &files));
    }
    assert!(
        emitted
            .iter()
            .any(|a| matches!(a, UiAction::Emit { name, data, .. } if name == "OnUnlock" && data == "1234")),
        "{emitted:?} / {:?}",
        problems(&mut ui)
    );
    assert_eq!(
        s.get("keypad.state").map(|v| v.to_string()),
        Some("ok".into())
    );
    // It clears itself a moment later.
    for _ in 0..40 {
        ui.update(0.05, (1920, 1080), &mut s, &files);
    }
    assert_eq!(
        s.get("keypad.shown").map(|v| v.to_string()),
        Some("----".into())
    );
    assert_eq!(problems(&mut ui), Vec::<String>::new());
}

#[test]
fn the_status_screen_runs_clean() {
    let files = content();
    let mut ui = UiSystem::new();
    let mut s = store();
    ui.set_panel("status", "ui/panels/status.keroui", (640, 360), &files)
        .unwrap();
    for _ in 0..5 {
        ui.update(0.05, (1920, 1080), &mut s, &files);
    }
    assert_eq!(problems(&mut ui), Vec::<String>::new());
}
