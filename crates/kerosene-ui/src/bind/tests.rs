use super::*;

#[test]
fn dependencies_are_dotted_names_outside_strings_and_calls() {
    assert_eq!(
        dependencies(r#"weapon.ammo < 5 && fmt(player.health) == "low.value" "#),
        vec!["weapon.ammo", "player.health"]
    );
    assert_eq!(dependencies("weapon.name.to_upper()"), vec!["weapon.name"]);
    assert_eq!(dependencies("if true { 1.5 } else { x }"), vec!["x"]);
}

#[test]
fn a_single_expression_keeps_its_type() {
    let engine = rhai::Engine::new();
    let t = Template::parse("{1 + 2}", &engine).unwrap().unwrap();
    let (v, e) = t.eval(&engine, &mut rhai::Scope::new());
    assert!(e.is_none());
    assert_eq!(v.as_int().unwrap(), 3);
}

#[test]
fn mixed_text_becomes_a_string() {
    let engine = rhai::Engine::new();
    let t = Template::parse("{{literal}} {2.0 * 3.0} left", &engine)
        .unwrap()
        .unwrap();
    let (v, _) = t.eval(&engine, &mut rhai::Scope::new());
    assert_eq!(v.cast::<String>(), "{literal} 6 left");
}

#[test]
fn plain_text_is_not_a_template() {
    assert!(
        Template::parse("hello", &rhai::Engine::new())
            .unwrap()
            .is_none()
    );
}

#[test]
fn nested_braces_are_one_expression() {
    let engine = rhai::Engine::new();
    let t = Template::parse("{ #{a: 1}.a }", &engine).unwrap().unwrap();
    assert_eq!(
        t.eval(&engine, &mut rhai::Scope::new()).0.as_int().unwrap(),
        1
    );
}

#[test]
fn single_quotes_are_strings() {
    assert_eq!(
        single_quotes_to_double("command('slot1')"),
        r#"command("slot1")"#
    );
    assert_eq!(
        single_quotes_to_double(r#"x('say "hi"') + "it's""#),
        r#"x("say \"hi\"") + "it's""#
    );
}
