use super::*;
use crate::{UiInput, UiSystem};

type Files = BTreeMap<String, String>;

fn files(list: &[(&str, &str)]) -> Files {
    list.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// A system showing `ui/t.keroui` on the `hud` layer at 1080p.
fn setup(layout: &str, extra: &[(&str, &str)]) -> (UiSystem, UiStore, Files) {
    let mut all = vec![("ui/t.keroui", layout)];
    all.extend_from_slice(extra);
    let f = files(&all);
    let mut ui = UiSystem::new();
    ui.show("hud", "ui/t.keroui", &f).unwrap();
    (ui, UiStore::new(), f)
}

fn frame(ui: &mut UiSystem, store: &mut UiStore, f: &Files) -> Vec<UiAction> {
    ui.update(1.0 / 60.0, (1920, 1080), store, f)
}

fn doc(ui: &UiSystem) -> &Document {
    ui.document("hud").unwrap()
}

#[test]
fn the_crosshair_follows_the_active_weapon() {
    let (mut ui, mut store, f) = setup(
        r#"<root><Panel id="xh" class="crosshair xh-{weapon.active}" class:firing="{weapon.firing}"/></root>"#,
        &[],
    );
    store.set("weapon.active", "pistol");
    frame(&mut ui, &mut store, &f);
    let xh = doc(&ui).find("xh").unwrap();
    assert_eq!(doc(&ui).classes(xh), vec!["crosshair", "xh-pistol"]);

    store.set("weapon.active", "shotgun");
    store.set("weapon.firing", true);
    frame(&mut ui, &mut store, &f);
    assert_eq!(
        doc(&ui).classes(xh),
        vec!["crosshair", "xh-shotgun", "firing"]
    );
}

#[test]
fn class_swaps_restyle_through_the_stylesheet() {
    let (mut ui, mut store, f) = setup(
        r#"<root><styles><include src="ui/t.kerocss"/></styles><Panel id="xh" class="xh-{weapon.active}"/></root>"#,
        &[(
            "ui/t.kerocss",
            ".xh-pistol { width: 8px } .xh-shotgun { width: 40px }",
        )],
    );
    store.set("weapon.active", "pistol");
    frame(&mut ui, &mut store, &f);
    let xh = doc(&ui).find("xh").unwrap();
    assert_eq!(doc(&ui).rect(xh)[2], 8.0);
    store.set("weapon.active", "shotgun");
    frame(&mut ui, &mut store, &f);
    assert_eq!(doc(&ui).rect(xh)[2], 40.0);
}

#[test]
fn specificity_beats_order() {
    let (mut ui, mut store, f) = setup(
        r#"<root><style>#a { width: 10px } Panel { width: 99px }</style><Panel id="a"/></root>"#,
        &[],
    );
    frame(&mut ui, &mut store, &f);
    let a = doc(&ui).find("a").unwrap();
    assert_eq!(doc(&ui).rect(a)[2], 10.0);
}

#[test]
fn flexbox_lays_out_rows() {
    let (mut ui, mut store, f) = setup(
        r#"<root><style>
            #row { flex-direction: row; width: 300px; height: 50px; gap: 10px }
            #row Panel { flex-grow: 1 }
        </style>
        <Panel id="row"><Panel id="a"/><Panel id="b"/></Panel></root>"#,
        &[],
    );
    frame(&mut ui, &mut store, &f);
    let d = doc(&ui);
    let (a, b) = (d.rect(d.find("a").unwrap()), d.rect(d.find("b").unwrap()));
    assert_eq!((a[2], b[2]), (145.0, 145.0));
    assert_eq!(b[0], 155.0);
}

#[test]
fn ui_pixels_scale_with_the_screen() {
    let (mut ui, mut store, f) = setup(
        r#"<root><Panel id="a" style="width: 100px; height: 100px; background-color: red"/></root>"#,
        &[],
    );
    ui.update(0.016, (1280, 720), &mut store, &f);
    let quad = ui.display_list().quads().next().unwrap();
    assert!((quad.rect[2] - 100.0 * 720.0 / 1080.0).abs() < 0.01);
}

#[test]
fn a_binding_reevaluates_only_when_its_keys_change() {
    let (mut ui, mut store, f) = setup(
        r#"<root><script>let calls = 0;</script><Label id="l" text="{player.health}"/></root>"#,
        &[],
    );
    store.set("player.health", 100);
    frame(&mut ui, &mut store, &f);
    let l = doc(&ui).find("l").unwrap();
    assert_eq!(doc(&ui).attr(l, "text"), Some("100"));
    store.set("time.now", 5);
    frame(&mut ui, &mut store, &f);
    store.set("player.health", 50.5);
    frame(&mut ui, &mut store, &f);
    assert_eq!(doc(&ui).attr(l, "text"), Some("50.5"));
}

#[test]
fn visible_hides_and_removes_from_layout() {
    let (mut ui, mut store, f) = setup(
        r#"<root><Panel id="a" visible="{player.alive}" style="height: 10px"/><Panel id="b" style="height: 10px"/></root>"#,
        &[],
    );
    store.set("player.alive", false);
    frame(&mut ui, &mut store, &f);
    let d = doc(&ui);
    assert_eq!(d.rect(d.find("b").unwrap())[1], 0.0);
    store.set("player.alive", true);
    frame(&mut ui, &mut store, &f);
    let d = doc(&ui);
    assert_eq!(d.rect(d.find("b").unwrap())[1], 10.0);
}

#[test]
fn events_reach_on_handlers_and_scripts() {
    let (mut ui, mut store, f) = setup(
        r#"<root>
            <scripts><include src="ui/t.keroscript"/></scripts>
            <Panel id="flash" on:player_damaged="target.trigger_class('hit')"/>
        </root>"#,
        &[(
            "ui/t.keroscript",
            r#"fn on_event(name, data) { if name == "player_damaged" { command("echo ouch " + data); } }"#,
        )],
    );
    frame(&mut ui, &mut store, &f);
    store.emit("player_damaged", "25");
    let actions = frame(&mut ui, &mut store, &f);
    // Script effects apply at the end of the frame that ran them.
    let flash = doc(&ui).find("flash").unwrap();
    assert!(doc(&ui).classes(flash).contains(&"hit".to_string()));
    assert_eq!(actions, vec![UiAction::Command("echo ouch 25".into())]);
}

#[test]
fn scripts_find_panels_and_change_them() {
    let (mut ui, mut store, f) = setup(
        r#"<root><script>
            fn on_load() { panel("title").set_text("Ready"); panel("nope").hide(); set_store("ui.loaded", true); }
        </script><Label id="title"/></root>"#,
        &[],
    );
    frame(&mut ui, &mut store, &f);
    let t = doc(&ui).find("title").unwrap();
    assert_eq!(doc(&ui).attr(t, "text"), Some("Ready"));
    assert_eq!(store.get("ui.loaded"), Some(&Value::Bool(true)));
    let messages = ui.take_messages();
    assert!(
        messages.iter().any(|(_, m)| m.contains("panel(\"nope\")")),
        "{messages:?}"
    );
}

#[test]
fn transitions_ease_between_values() {
    let (mut ui, mut store, f) = setup(
        r#"<root><style>
            #a { opacity: 1; transition: opacity 1s linear }
            #a.gone { opacity: 0 }
        </style><Panel id="a" class:gone="{x}"/></root>"#,
        &[],
    );
    store.set("x", false);
    frame(&mut ui, &mut store, &f);
    store.set("x", true);
    ui.update(0.5, (1920, 1080), &mut store, &f);
    let a = doc(&ui).find("a").unwrap();
    let mid = doc(&ui).shown_style(a).opacity;
    assert!((0.4..0.6).contains(&mid), "{mid}");
    ui.update(0.6, (1920, 1080), &mut store, &f);
    assert_eq!(doc(&ui).shown_style(a).opacity, 0.0);
}

#[test]
fn keyframe_animations_run() {
    let (mut ui, mut store, f) = setup(
        r#"<root><style>
            @keyframes fade { from { opacity: 0 } to { opacity: 1 } }
            #a { animation: fade 1s linear }
        </style><Panel id="a"/></root>"#,
        &[],
    );
    ui.update(0.25, (1920, 1080), &mut store, &f);
    let a = doc(&ui).find("a").unwrap();
    assert!((doc(&ui).shown_style(a).opacity - 0.25).abs() < 0.01);
    ui.update(2.0, (1920, 1080), &mut store, &f);
    assert_eq!(doc(&ui).shown_style(a).opacity, 1.0);
}

#[test]
fn repeat_makes_one_child_per_count() {
    let (mut ui, mut store, f) = setup(
        r#"<root><Repeat id="r" count="{slots}"><Label text="slot {index}"/></Repeat></root>"#,
        &[],
    );
    store.set("slots", 3);
    frame(&mut ui, &mut store, &f);
    let texts: Vec<String> = doc(&ui)
        .display_list()
        .quads()
        .filter(|q| q.texture == TextureRef::Glyphs)
        .map(|_| String::new())
        .collect();
    assert!(!texts.is_empty());
    let r = doc(&ui).find("r").unwrap();
    assert_eq!(doc(&ui).nodes[r].children.len(), 3);
    let last = doc(&ui).nodes[r].children[2];
    assert_eq!(doc(&ui).attr(last, "text"), Some("slot 2"));
    store.set("slots", 1);
    frame(&mut ui, &mut store, &f);
    assert_eq!(doc(&ui).nodes[r].children.len(), 1);
}

fn menu(layout: &str) -> (UiSystem, UiStore, Files) {
    let f = files(&[("ui/m.keroui", layout)]);
    let mut ui = UiSystem::new();
    ui.show("menu", "ui/m.keroui", &f).unwrap();
    let mut store = UiStore::new();
    ui.update(0.016, (1920, 1080), &mut store, &f);
    (ui, store, f)
}

fn centre(ui: &UiSystem, id: &str) -> (f32, f32) {
    let d = ui.document("menu").unwrap();
    let r = d.rect(d.find(id).unwrap());
    (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
}

#[test]
fn clicking_a_button_runs_its_handler() {
    let (mut ui, mut store, f) = menu(
        r#"<root interactive="true"><Button id="b" text="Resume" onactivate="command('resume')"/></root>"#,
    );
    assert!(ui.wants_input());
    let (x, y) = centre(&ui, "b");
    assert!(ui.input(UiInput::PointerMove { x, y }));
    ui.input(UiInput::PointerButton { down: true });
    ui.input(UiInput::PointerButton { down: false });
    let actions = ui.update(0.016, (1920, 1080), &mut store, &f);
    assert_eq!(
        actions,
        vec![UiAction::Command("resume".into())],
        "{:?}",
        ui.take_messages()
    );
}

#[test]
fn hover_applies_the_hover_style() {
    let (mut ui, mut store, f) = menu(
        r#"<root interactive="true"><style>#b:hover { background-color: red }</style><Button id="b" text="x"/></root>"#,
    );
    let (x, y) = centre(&ui, "b");
    ui.input(UiInput::PointerMove { x, y });
    // Long enough for the default button transition to finish.
    ui.update(1.0, (1920, 1080), &mut store, &f);
    let d = ui.document("menu").unwrap();
    assert_eq!(
        d.shown_style(d.find("b").unwrap()).background_color,
        crate::style::Color([1.0, 0.0, 0.0, 1.0])
    );
}

#[test]
fn a_slider_bound_to_a_cvar_writes_it_back() {
    let (mut ui, mut store, f) = menu(
        r#"<root interactive="true"><style>Slider { width: 200px }</style><Slider id="s" cvar="volume" min="0" max="1"/></root>"#,
    );
    store.set("cvar.volume", 0.5);
    ui.update(0.016, (1920, 1080), &mut store, &f);
    let d = ui.document("menu").unwrap();
    let s = d.find("s").unwrap();
    assert_eq!(d.attr(s, "value"), Some("0.5"));

    // Click at the right-hand end.
    let r = d.rect(s);
    ui.input(UiInput::PointerMove {
        x: r[0] + r[2] - 0.5,
        y: r[1] + r[3] / 2.0,
    });
    ui.input(UiInput::PointerButton { down: true });
    ui.input(UiInput::PointerButton { down: false });
    let actions = ui.update(0.016, (1920, 1080), &mut store, &f);
    assert!(
        actions.iter().any(|a| matches!(a, UiAction::SetCvar { name, value } if name == "volume" && value.starts_with("0.99") || value == "1")),
        "{actions:?}"
    );
}

#[test]
fn the_keyboard_moves_focus_and_activates() {
    let (mut ui, mut store, f) = menu(
        r#"<root interactive="true">
            <Button id="a" text="A" onactivate="command('a')"/>
            <Button id="b" text="B" onactivate="command('b')"/>
        </root>"#,
    );
    ui.input(UiInput::Key(crate::UiKey::Down));
    ui.input(UiInput::Key(crate::UiKey::Down));
    ui.input(UiInput::Key(crate::UiKey::Enter));
    let actions = ui.update(0.016, (1920, 1080), &mut store, &f);
    assert_eq!(actions, vec![UiAction::Command("b".into())]);
}

#[test]
fn text_entry_takes_typing() {
    let (mut ui, mut store, f) = menu(
        r#"<root interactive="true"><TextEntry id="t" onsubmit="emit('code', data)"/></root>"#,
    );
    ui.input(UiInput::Key(crate::UiKey::Tab));
    ui.input(UiInput::Text("1234".into()));
    ui.input(UiInput::Key(crate::UiKey::Backspace));
    ui.input(UiInput::Key(crate::UiKey::Enter));
    let actions = ui.update(0.016, (1920, 1080), &mut store, &f);
    assert_eq!(
        actions,
        vec![UiAction::Emit {
            name: "code".into(),
            data: "123".into(),
            source: "menu".into()
        }]
    );
}

#[test]
fn a_hud_never_takes_input() {
    let (mut ui, _, _) = setup(r#"<root><Button id="b" text="x"/></root>"#, &[]);
    assert!(!ui.wants_input());
    assert!(!ui.input(UiInput::PointerButton { down: true }));
}

#[test]
fn radial_fill_reaches_the_display_list() {
    let (mut ui, mut store, f) = setup(
        r#"<root><Panel id="cd" style="width: 64px; height: 64px; background-color: white" style:kero-fill="radial({ability.dash.cooldown})"/></root>"#,
        &[],
    );
    store.set("ability.dash.cooldown", 0.25);
    frame(&mut ui, &mut store, &f);
    let q = ui.display_list().quads().next().unwrap();
    assert_eq!(q.fill, (1, 0.25));
}

#[test]
fn includes_pull_in_other_layouts() {
    let (mut ui, mut store, f) = setup(
        r#"<root><Include src="ui/part.keroui"/></root>"#,
        &[(
            "ui/part.keroui",
            r#"<root><Label id="inner" text="hi"/></root>"#,
        )],
    );
    frame(&mut ui, &mut store, &f);
    assert!(doc(&ui).find("inner").is_some());
}

#[test]
fn a_repeats_children_lay_out_in_its_parent() {
    let (mut ui, mut store, f) = setup(
        r#"<root><style>#row { flex-direction: row; } .cell { width: 10px; height: 10px; }</style>
            <Panel id="row"><Panel id="first" class="cell"/><Repeat count="{n}"><Panel class="cell"/></Repeat><Panel id="last" class="cell"/></Panel>
        </root>"#,
        &[],
    );
    store.set("n", 2);
    frame(&mut ui, &mut store, &f);
    let d = doc(&ui);
    // first, two repeated, last: in a row, in document order.
    assert_eq!(d.rect(d.find("first").unwrap())[0], 0.0);
    assert_eq!(d.rect(d.find("last").unwrap())[0], 30.0);
    store.set("n", 4);
    frame(&mut ui, &mut store, &f);
    let d = doc(&ui);
    assert_eq!(d.rect(d.find("last").unwrap())[0], 50.0);
}

#[test]
fn the_platform_object_reads_published_keys_and_queues_actions() {
    let (mut ui, mut store, f) = setup(
        r#"<root>
            <script>
                fn on_event(name, data) {
                    if name == "boss_dead" && !steam.is_unlocked("ACH_BOSS") {
                        platform.unlock("ACH_BOSS");
                        platform.add_stat("kills", 1);
                    }
                }
            </script>
            <Label id="who" text="{platform.user} {platform.stats.kills}"/>
            <Label id="badge" visible="{platform.achievements.ACH_BOSS}"/>
        </root>"#,
        &[],
    );
    store.set("platform.user", "tester");
    store.set("platform.stats.kills", 3.0);
    store.set("platform.achievements.ACH_BOSS", false);
    frame(&mut ui, &mut store, &f);
    let who = doc(&ui).find("who").unwrap();
    assert_eq!(doc(&ui).attr(who, "text"), Some("tester 3"));

    store.emit("boss_dead", "");
    let actions = frame(&mut ui, &mut store, &f);
    use kerosene_platform::PlatformAction;
    assert_eq!(
        actions,
        vec![
            UiAction::Platform(PlatformAction::Unlock("ACH_BOSS".into())),
            UiAction::Platform(PlatformAction::AddStat {
                name: "kills".into(),
                delta: 1.0
            }),
        ]
    );
}
