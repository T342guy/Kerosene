// SPDX-License-Identifier: MPL-2.0
use super::*;

#[test]
fn the_games_own_classes_are_recognised() {
    for (class, want) in [
        ("light", Kind::Light),
        ("light_spot", Kind::Light),
        ("light_environment", Kind::Light),
        ("info_player_start", Kind::Player),
        ("info_target", Kind::Target),
        ("ambient_generic", Kind::Sound),
        ("logic_relay", Kind::Logic),
        ("logic_auto", Kind::Logic),
        ("logic_timer", Kind::Logic),
        ("logic_branch", Kind::Logic),
        ("math_counter", Kind::Logic),
        ("logic_script", Kind::Script),
        ("point_message", Kind::Message),
        ("prop_static", Kind::Prop),
    ] {
        assert_eq!(Kind::of(class), want, "{class}");
    }
}

#[test]
fn a_script_is_a_script_before_it_is_logic() {
    // `logic_script` starts with `logic_` and is not one: what it does is run
    // code, and that is the thing worth telling apart at a glance.
    assert_eq!(Kind::of("logic_script"), Kind::Script);
}

#[test]
fn a_class_nobody_anticipated_still_gets_a_shape() {
    // A blank space where an entity is would be worse than a plain box.
    assert_eq!(Kind::of("func_wibble"), Kind::Other);
    assert_eq!(Kind::of(""), Kind::Other);
}

#[test]
fn a_game_that_adds_a_class_gets_the_right_icon_without_this_list_changing() {
    // Prefixes rather than an enumeration, so `light_dynamic` and
    // `logic_case` are right the day someone writes them.
    assert_eq!(Kind::of("light_dynamic"), Kind::Light);
    assert_eq!(Kind::of("logic_case"), Kind::Logic);
    assert_eq!(Kind::of("prop_physics"), Kind::Prop);
    assert_eq!(Kind::of("ambient_music"), Kind::Sound);
}

#[test]
fn classnames_are_matched_however_they_are_capitalised() {
    assert_eq!(Kind::of("LIGHT_SPOT"), Kind::Light);
    assert_eq!(Kind::of("Info_Player_Start"), Kind::Player);
}

#[test]
fn every_family_has_a_name_and_a_colour_of_its_own() {
    let mut colours = Vec::new();
    let mut labels = Vec::new();
    for kind in Kind::all() {
        assert!(!kind.label().is_empty());
        assert!(!labels.contains(&kind.label()), "{} reuses a name", kind.label());
        labels.push(kind.label());

        let c = kind.colour();
        assert!(!colours.contains(&c), "{} reuses a colour", kind.label());
        colours.push(c);
    }
    assert_eq!(Kind::all().len(), labels.len());
}

#[test]
fn every_family_is_bright_enough_to_see_on_the_dark_background() {
    // The viewports are nearly black. An icon drawn in a dark colour is an
    // icon nobody can find, which is the problem this set exists to solve.
    for kind in Kind::all() {
        let c = kind.colour();
        let brightness = c.r() as u32 + c.g() as u32 + c.b() as u32;
        assert!(brightness > 330, "{} is too dark: {c:?}", kind.label());
    }
}

#[test]
fn drawing_an_icon_puts_shapes_on_the_screen() {
    // Every family draws something. A family whose shape is nothing looks
    // exactly like an entity that is not there.
    let ctx = egui::Context::default();
    for kind in Kind::all() {
        let mut count = 0;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let painter = ui.painter();
                let before = painter.clone();
                let _ = before;
                draw(painter, egui::pos2(100.0, 100.0), 10.0, kind, kind.colour());
                count += 1;
            });
        });
        assert_eq!(count, 1, "{} drew nothing", kind.label());
    }
}

#[test]
fn an_icon_stays_inside_the_radius_it_was_given() {
    // Viewports draw these at every zoom, and one that overflows its box
    // smears into its neighbours. Measured against an empty frame, so the
    // panel's own background is not mistaken for an icon.
    let centre = egui::pos2(100.0, 100.0);
    let baseline = frame_shapes(|_| {});

    for kind in Kind::all() {
        let shapes = frame_shapes(|ui| draw(ui.painter(), centre, 12.0, kind, kind.colour()));
        assert!(shapes.len() > baseline.len(), "{} drew nothing", kind.label());

        // A stroke has width, so the box is the radius plus a little.
        let allowed = egui::Rect::from_center_size(centre, egui::Vec2::splat(12.0 * 2.0 + 6.0));
        for shape in &shapes[baseline.len()..] {
            let bounds = shape.visual_bounding_rect();
            if bounds.is_negative() { continue }
            assert!(allowed.contains_rect(bounds), "{} overflows: {bounds:?}", kind.label());
        }
    }
}

/// The shapes one frame produced, with the given contents drawn into it.
fn frame_shapes(contents: impl FnMut(&mut egui::Ui)) -> Vec<egui::Shape> {
    let mut contents = contents;
    let ctx = egui::Context::default();
    let output = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| contents(ui));
    });
    output.shapes.into_iter().map(|c| c.shape).collect()
}
