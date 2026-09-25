use super::*;

fn files(list: &[(&str, &str)]) -> BTreeMap<String, String> {
    list.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn a_changed_stylesheet_reloads_the_document() {
    let mut f = files(&[
        (
            "ui/h.keroui",
            r#"<root><styles><include src="ui/h.kerocss"/></styles><Panel id="a"/></root>"#,
        ),
        ("ui/h.kerocss", "#a { width: 10px }"),
    ]);
    let mut ui = UiSystem::new();
    let mut store = UiStore::new();
    ui.show("hud", "ui/h.keroui", &f).unwrap();
    ui.update(0.1, (1920, 1080), &mut store, &f);
    f.insert("ui/h.kerocss".into(), "#a { width: 20px }".into());
    ui.update(RELOAD_INTERVAL, (1920, 1080), &mut store, &f);
    ui.update(0.1, (1920, 1080), &mut store, &f);
    let d = ui.document("hud").unwrap();
    assert_eq!(d.rect(d.find("a").unwrap())[2], 20.0);
    assert!(
        ui.take_messages()
            .iter()
            .any(|(_, m)| m.contains("reloaded"))
    );
}

#[test]
fn a_broken_edit_keeps_the_old_document() {
    let mut f = files(&[("ui/h.keroui", r#"<root><Panel id="a"/></root>"#)]);
    let mut ui = UiSystem::new();
    let mut store = UiStore::new();
    ui.show("hud", "ui/h.keroui", &f).unwrap();
    f.insert("ui/h.keroui".into(), "<root><Panel".into());
    ui.update(RELOAD_INTERVAL, (1920, 1080), &mut store, &f);
    assert!(ui.document("hud").unwrap().find("a").is_some());
}

#[test]
fn layers_draw_in_z_order_and_the_top_interactive_one_gets_input() {
    let f = files(&[
        (
            "ui/menu.keroui",
            r#"<root interactive="true" z="100"><Panel style="width: 10px; height: 10px; background-color: red"/></root>"#,
        ),
        (
            "ui/hud.keroui",
            r#"<root><Panel style="width: 10px; height: 10px; background-color: blue"/></root>"#,
        ),
    ]);
    let mut ui = UiSystem::new();
    let mut store = UiStore::new();
    ui.show("menu", "ui/menu.keroui", &f).unwrap();
    ui.show("hud", "ui/hud.keroui", &f).unwrap();
    ui.update(0.1, (1920, 1080), &mut store, &f);
    let colours: Vec<[f32; 4]> = ui.display_list().quads().map(|q| q.color).collect();
    assert_eq!(colours, vec![[0.0, 0.0, 1.0, 1.0], [1.0, 0.0, 0.0, 1.0]]);
    assert!(ui.wants_input());
    ui.hide("menu");
    assert!(!ui.wants_input());
}

#[test]
fn show_layer_from_a_script() {
    let f = files(&[
        (
            "ui/a.keroui",
            r#"<root><script>fn on_load() { show_layer("menu", "ui/b.keroui"); }</script></root>"#,
        ),
        ("ui/b.keroui", r#"<root interactive="true"/>"#),
    ]);
    let mut ui = UiSystem::new();
    let mut store = UiStore::new();
    ui.show("hud", "ui/a.keroui", &f).unwrap();
    ui.update(0.1, (1920, 1080), &mut store, &f);
    assert!(ui.is_visible("menu"));
}

#[test]
fn world_panels_report_when_they_change() {
    let f = files(&[("ui/p.keroui", r#"<root><Label text="{door.code}"/></root>"#)]);
    let mut ui = UiSystem::new();
    let mut store = UiStore::new();
    ui.set_panel("keypad", "ui/p.keroui", (256, 256), &f)
        .unwrap();
    ui.update(0.1, (1920, 1080), &mut store, &f);
    let (_, r0) = ui.panel_display("keypad").unwrap();
    ui.update(0.1, (1920, 1080), &mut store, &f);
    assert_eq!(ui.panel_display("keypad").unwrap().1, r0);
    store.set("door.code", "1234");
    ui.update(0.1, (1920, 1080), &mut store, &f);
    let (list, r1) = ui.panel_display("keypad").unwrap();
    assert!(r1 > r0);
    assert_eq!(list.size, (256, 256));
}

#[test]
fn emits_are_tagged_with_where_they_came_from() {
    let f = files(&[(
        "ui/p.keroui",
        r#"<root><script>fn on_load() { emit("ready", 1); }</script></root>"#,
    )]);
    let mut ui = UiSystem::new();
    let mut store = UiStore::new();
    ui.set_panel("keypad", "ui/p.keroui", (64, 64), &f).unwrap();
    let actions = ui.update(0.1, (1920, 1080), &mut store, &f);
    assert_eq!(
        actions,
        vec![UiAction::Emit {
            name: "ready".into(),
            data: "1".into(),
            source: "panel:keypad".into()
        }]
    );
    // ...and every document hears it next frame.
    assert_eq!(store.pending_events().len(), 1);
}
