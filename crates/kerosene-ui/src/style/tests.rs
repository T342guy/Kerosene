use super::*;

const VP: (f32, f32) = (1920.0, 1080.0);

#[test]
fn colours() {
    assert_eq!(Color::parse("#fff"), Some(Color::WHITE));
    assert_eq!(Color::parse("#ff000080").unwrap().0[3], 128.0 / 255.0);
    assert_eq!(
        Color::parse("rgba(255, 0, 0, 0.5)"),
        Some(Color([1.0, 0.0, 0.0, 0.5]))
    );
    assert_eq!(Color::parse("transparent"), Some(Color::TRANSPARENT));
    assert_eq!(Color::parse("nonsense"), None);
}

#[test]
fn lengths() {
    assert_eq!(Dim::parse("12px", VP), Some(Dim::Px(12.0)));
    assert_eq!(Dim::parse("50%", VP), Some(Dim::Percent(0.5)));
    assert_eq!(Dim::parse("10vh", VP), Some(Dim::Px(108.0)));
    assert_eq!(Dim::parse("auto", VP), Some(Dim::Auto));
}

#[test]
fn shorthands_expand_in_css_order() {
    let mut s = Style::default();
    assert!(s.apply("padding", "1px 2px 3px 4px", VP));
    // Stored left, top, right, bottom.
    assert_eq!(
        s.padding,
        [Dim::Px(4.0), Dim::Px(1.0), Dim::Px(2.0), Dim::Px(3.0)]
    );
    assert!(s.apply("border", "2px solid red", VP));
    assert_eq!(s.border_width, 2.0);
    assert_eq!(s.border_color, Color([1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn kerosene_extensions() {
    let mut s = Style::default();
    assert!(s.apply("-kero-fill", "radial(25%)", VP));
    assert_eq!(s.fill, Fill::Radial(0.25));
    assert!(s.apply("-kero-blend", "additive", VP));
    assert_eq!(s.blend, Blend::Additive);
}

#[test]
fn transitions_and_animations() {
    let mut s = Style::default();
    assert!(s.apply("transition", "opacity 0.2s ease-out, transform 150ms", VP));
    assert_eq!(s.transitions.len(), 2);
    assert_eq!(s.transitions[1].duration, 0.15);
    assert!(s.apply("animation", "pulse 1s ease-in-out infinite alternate", VP));
    let a = s.animation.unwrap();
    assert_eq!(
        (a.name.as_str(), a.iterations, a.alternate),
        ("pulse", None, true)
    );
}

#[test]
fn unknown_properties_are_reported() {
    let mut s = Style::default();
    assert!(!s.apply("colour", "red", VP));
    assert!(!s.apply("width", "wide", VP));
}

#[test]
fn inherited_properties_flow_to_children() {
    let mut parent = Style::default();
    parent.apply("color", "red", VP);
    parent.apply("background-color", "blue", VP);
    let child = Style::inherit(&parent);
    assert_eq!(child.color, parent.color);
    assert_eq!(child.background_color, Color::TRANSPARENT);
}
