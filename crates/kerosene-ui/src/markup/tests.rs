use super::*;

#[test]
fn comparisons_in_bindings_need_no_escaping() {
    let m = parse(
        "t",
        r#"<root><Label class:low="{ammo < 5 && !reloading}" text="a &amp; b"/></root>"#,
    )
    .unwrap();
    assert_eq!(
        m.body[0].attr("class:low"),
        Some("{ammo < 5 && !reloading}")
    );
    assert_eq!(m.body[0].attr("text"), Some("a & b"));
}

#[test]
fn script_bodies_may_compare_too() {
    let m = parse(
        "t",
        "<root><script>fn f(a, b) { if a < b && b > 0 { 1 } else { 2 } }</script><style>Label > Panel { }</style></root>",
    )
    .unwrap();
    assert_eq!(
        m.inline_scripts,
        vec!["fn f(a, b) { if a < b && b > 0 { 1 } else { 2 } }"]
    );
    assert_eq!(m.inline_styles, vec!["Label > Panel { }"]);
}

#[test]
fn binding_prefixes_are_kept() {
    let m = parse(
        "t",
        r#"<root><Panel on:weapon_changed="x()" style:width="{w}%" class:on="{b}"/></root>"#,
    )
    .unwrap();
    let names: Vec<&str> = m.body[0].attrs.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, vec!["on:weapon_changed", "style:width", "class:on"]);
}

#[test]
fn the_outer_element_must_be_root() {
    assert!(parse("t", "<Panel/>").unwrap_err().contains("<root>"));
}

#[test]
fn comments_pass_through_untouched() {
    let m = parse("t", "<root><!-- a < b && c --><Panel/></root>").unwrap();
    assert_eq!(m.body.len(), 1);
}

#[test]
fn comments_may_say_anything() {
    let m = parse(
        "t",
        "<!-- about <root> -- and more --><root><Panel/></root>",
    )
    .unwrap();
    assert_eq!(m.body.len(), 1);
}
