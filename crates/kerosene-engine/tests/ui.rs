// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The game UI against a real, compiled map: the store, the HUD, decals and
//! world panels, headless.

mod common;

use cleave::{CompileOptions, compile};
use kerosene_engine::engine::{Engine, EngineConfig, report_unhandled, take_console_requests};
use kerosene_engine::input::InputState;
use kerosene_map::{Connection, Entity, Map, Solid};
use kerosene_math::{Aabb, Angles, Vec3};
use kerosene_ui::Value;
use std::path::PathBuf;

const TICK: f32 = 1.0 / 64.0;

/// A closed room 512 long with a door at the far end, a keypad panel on the
/// way that opens it, and a decal on the floor.
fn room() -> Map {
    let mut map = Map::new();
    let t = 16.0;
    let (len, wide, tall) = (512.0f32, 128.0f32, 128.0f32);
    for slab in [
        Aabb::new(Vec3::new(-t, -t, -t), Vec3::new(len + t, wide + t, 0.0)),
        Aabb::new(
            Vec3::new(-t, -t, tall),
            Vec3::new(len + t, wide + t, tall + t),
        ),
        Aabb::new(Vec3::new(-t, -t, 0.0), Vec3::new(0.0, wide + t, tall)),
        Aabb::new(Vec3::new(len, -t, 0.0), Vec3::new(len + t, wide + t, tall)),
        Aabb::new(Vec3::new(0.0, -t, 0.0), Vec3::new(len, 0.0, tall)),
        Aabb::new(Vec3::new(0.0, wide, 0.0), Vec3::new(len, wide + t, tall)),
    ] {
        map.add_world_solid(Solid::cube(slab, "dev/grid"));
    }

    let bounds = Aabb::new(Vec3::new(400.0, 0.0, 0.0), Vec3::new(416.0, wide, tall));
    let id = map.next_id();
    let solid_id = map.next_id();
    let side_ids: Vec<u32> = (0..6).map(|_| map.next_id()).collect();
    let mut solid = Solid::cube(bounds, "dev/grid");
    solid.id = solid_id;
    for (s, sid) in solid.sides.iter_mut().zip(side_ids) {
        s.id = sid;
    }
    let mut door = Entity::new(id, "func_door");
    door.set("targetname", "gate");
    door.set("movedir", "0 0 1");
    door.set("speed", "400");
    door.set("wait", "-1");
    door.solids.push(solid);
    map.entities.push(door);

    // Facing back down the room at the player, within reach of the spawn.
    let id = map.next_id();
    let mut panel = Entity::new(id, "point_worldpanel");
    panel.set("targetname", "keypad");
    panel.set_origin(Vec3::new(112.0, 64.0, 64.0));
    panel.set("angles", "0 180 0");
    panel.set("layout", "ui/keypad.keroui");
    panel.set("width", "64");
    panel.set("height", "64");
    panel.set("resolution", "256");
    panel.set("interactive", "1");
    panel.connect(Connection::new("OnUnlock", "gate", "Open"));
    map.entities.push(panel);

    let id = map.next_id();
    let mut decal = Entity::new(id, "infodecal");
    decal.set_origin(Vec3::new(64.0, 64.0, 4.0));
    decal.set("texture", "decals/crack");
    decal.set("size", "24");
    map.entities.push(decal);

    let id = map.next_id();
    let mut spawn = Entity::new(id, "info_player_start");
    spawn.set_origin(Vec3::new(32.0, 64.0, 8.0));
    map.entities.push(spawn);
    map
}

const KEYPAD: &str = r#"<root>
    <style>
        #unlock { width: 100%; height: 100%; }
    </style>
    <Button id="unlock" text="Open" onactivate="emit('OnUnlock', '1234')"/>
</root>"#;

const HUD: &str = r#"<root>
    <Label id="health" text="{player.health}" class:hurt="{player.health < 50}"/>
    <Label id="objective" text="{objective.text}"/>
</root>"#;

fn setup(name: &str, script: &str) -> (Engine, PathBuf) {
    let dir = std::env::temp_dir().join(format!("kerosene-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["maps", "scripts", "ui"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    let out = compile(&room(), &CompileOptions::default()).expect("the test map compiles");
    assert!(out.leak.is_none());
    std::fs::write(dir.join("maps/uimap.kerobsp"), out.bsp.to_bytes()).unwrap();
    std::fs::write(dir.join("scripts/uimap.keroscript"), script).unwrap();
    std::fs::write(dir.join("ui/keypad.keroui"), KEYPAD).unwrap();
    std::fs::write(dir.join("ui/hud.keroui"), HUD).unwrap();
    let mut engine = common::stock(&EngineConfig {
        content_paths: vec![dir.clone()],
        ..Default::default()
    });
    engine.console.set("ui_hud", "ui/hud.keroui");
    engine.load_map("uimap").unwrap();
    (engine, dir)
}

fn frame(engine: &mut Engine) {
    engine.ui_frame(1.0 / 60.0, (1280, 720));
}

fn tick(engine: &mut Engine, input: &InputState) {
    engine.tick(TICK, input);
    engine.console.run_buffered();
    let unclaimed = take_console_requests(engine);
    report_unhandled(engine, unclaimed);
}

#[test]
fn a_map_script_publishes_to_the_hud() {
    let (mut engine, dir) = setup(
        "script",
        r#"fn on_map_start() { ui_set("objective.text", "Find the key"); ui_event("objective_changed"); }"#,
    );
    assert_eq!(
        engine.ui.store.get("objective.text"),
        Some(&Value::Str("Find the key".into()))
    );
    assert!(
        engine
            .ui
            .store
            .pending_events()
            .iter()
            .any(|e| e.name == "objective_changed")
    );

    frame(&mut engine);
    let log: Vec<String> = engine.console.log().map(|l| l.text.clone()).collect();
    let hud = engine
        .ui
        .system
        .document("hud")
        .unwrap_or_else(|| panic!("the HUD is up once a map is: {log:#?}"));
    assert_eq!(
        hud.attr(hud.find("objective").unwrap(), "text"),
        Some("Find the key")
    );
    assert_eq!(hud.attr(hud.find("health").unwrap(), "text"), Some("100"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn damage_reaches_the_hud_as_a_value_and_an_event() {
    let (mut engine, dir) = setup("damage", "");
    frame(&mut engine);
    engine.hurt_player(60.0, "test");
    assert!(
        engine
            .ui
            .store
            .pending_events()
            .iter()
            .any(|e| e.name == "player_damaged" && e.data == "60")
    );
    tick(&mut engine, &InputState::default());
    frame(&mut engine);
    let hud = engine.ui.system.document("hud").unwrap();
    let health = hud.find("health").unwrap();
    assert_eq!(hud.attr(health, "text"), Some("40"));
    assert!(hud.classes(health).contains(&"hurt".to_string()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_infodecal_lands_on_the_floor_under_it() {
    let (engine, dir) = setup("infodecal", "");
    let decals: Vec<_> = engine.ui.decals.list.iter().collect();
    assert_eq!(decals.len(), 1, "{decals:?}");
    assert_eq!(decals[0].material, "decals/crack");
    assert!(decals[0].origin.z.abs() < 0.5, "{:?}", decals[0].origin);
    assert!(decals[0].normal.z > 0.99);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_decal_command_marks_what_you_look_at_and_a_new_map_clears_them() {
    let (mut engine, dir) = setup("decal", "");
    engine.console.set("sv_cheats", "1");
    engine.console.execute("decal decals/bullet 8");
    tick(&mut engine, &InputState::default());
    let last = engine.ui.decals.list.back().unwrap();
    assert_eq!(last.material, "decals/bullet");
    // Looking down the room at the door, which is an entity; the shot
    // passes to the far wall behind it or stops at it, either way ahead.
    assert!(last.origin.x > 300.0, "{:?}", last.origin);
    engine.load_map("uimap").unwrap();
    assert_eq!(
        engine.ui.decals.list.len(),
        1,
        "only the map's own infodecal"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pressing_use_on_a_world_panel_clicks_it_and_its_output_opens_the_door() {
    let (mut engine, dir) = setup("panel", "");
    frame(&mut engine);
    assert_eq!(engine.ui.panels.len(), 1);
    assert_eq!(engine.ui.panels[0].pixels, (256, 256));

    let gate = engine.entities.find_by_name("gate")[0];
    let closed = engine.entities.get(gate).unwrap().origin.z;

    // Look at the panel's middle and press use.
    let eye = engine.player.movement.eye_position();
    let dir_to_panel = Vec3::new(112.0, 64.0, 64.0) - eye;
    let look = InputState {
        view_angles: Angles::from_direction(dir_to_panel),
        ..Default::default()
    };
    tick(&mut engine, &look);
    frame(&mut engine);
    assert!(engine.aiming_at_panel());
    tick(
        &mut engine,
        &InputState {
            use_key: true,
            ..look
        },
    );
    frame(&mut engine);
    tick(&mut engine, &look);
    // The click becomes a handler run, the handler an emit, the emit the
    // panel's OnUnlock output, and the output the door's Open input.
    frame(&mut engine);
    for _ in 0..64 {
        tick(&mut engine, &look);
    }
    let now = engine.entities.get(gate).unwrap().origin.z;
    assert!(
        now > closed + 16.0,
        "the door did not open: {closed} -> {now}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_pause_menu_needs_a_map_and_its_file() {
    let (mut engine, dir) = setup("menu", "");
    // No ui/menus/pause.keroui in this tree.
    assert!(!engine.toggle_pause_menu());
    std::fs::create_dir_all(dir.join("ui/menus")).unwrap();
    std::fs::write(
        dir.join("ui/menus/pause.keroui"),
        r#"<root interactive="true" z="100"><Button id="resume" text="Resume" onactivate="hide_layer('menu')"/></root>"#,
    )
    .unwrap();
    assert!(engine.toggle_pause_menu());
    assert!(engine.ui_wants_input());
    frame(&mut engine);
    assert_eq!(
        engine.ui.store.get("ui.menu_open"),
        Some(&Value::Bool(true))
    );
    assert!(engine.toggle_pause_menu());
    assert!(!engine.ui_wants_input());
    let _ = std::fs::remove_dir_all(dir);
}
