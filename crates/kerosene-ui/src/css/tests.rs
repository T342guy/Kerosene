use super::*;

#[test]
fn rules_selectors_and_specificity() {
    let sheet = StyleSheet::parse(
        "t",
        "/* c */ Label.big#title:hover, Panel > .x { color: red; font-size: 20px }",
    );
    assert!(sheet.warnings.is_empty(), "{:?}", sheet.warnings);
    assert_eq!(sheet.rules.len(), 2);
    let first = &sheet.rules[0].selector;
    assert_eq!(first.subject.tag.as_deref(), Some("label"));
    assert_eq!(first.subject.id.as_deref(), Some("title"));
    assert_eq!(first.subject.classes, vec!["big"]);
    assert_eq!(first.subject.pseudo, vec![Pseudo::Hover]);
    assert_eq!(sheet.rules[0].specificity, (1 << 16) | (2 << 8) | 1);
    let second = &sheet.rules[1].selector;
    assert_eq!(second.ancestors.len(), 1);
    assert_eq!(second.ancestors[0].0, Combinator::Child);
    assert_eq!(sheet.rules[1].decls[1].value, "20px");
}

#[test]
fn a_bad_rule_costs_only_itself() {
    let sheet = StyleSheet::parse("t", ".a!! { color: red } .b { color: blue }");
    assert_eq!(sheet.rules.len(), 1);
    assert_eq!(sheet.warnings.len(), 1);
}

#[test]
fn keyframes_and_font_faces() {
    let sheet = StyleSheet::parse(
        "t",
        r#"@keyframes pulse { from { opacity: 1 } 50% { opacity: 0.2 } to { opacity: 1 } }
           @font-face { font-family: "Hud"; src: url("ui/fonts/hud.ttf"); font-weight: bold }"#,
    );
    let pulse = &sheet.keyframes["pulse"];
    assert_eq!(
        pulse.stops.iter().map(|s| s.0).collect::<Vec<_>>(),
        vec![0.0, 0.5, 1.0]
    );
    assert_eq!(
        sheet.font_faces,
        vec![FontFace {
            family: "Hud".into(),
            src: "ui/fonts/hud.ttf".into(),
            bold: true
        }]
    );
}

#[test]
fn separators_inside_parentheses_are_not_split() {
    let decls = parse_declarations("background: url(\"a;b.png\"); color: rgba(0, 0, 0, 0.5)");
    assert_eq!(decls.len(), 2);
    assert_eq!(decls[1].value, "rgba(0, 0, 0, 0.5)");
}
