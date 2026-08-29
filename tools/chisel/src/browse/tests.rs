// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;

fn paths(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn nothing_groups_into_nothing() {
    assert!(folders(&[]).is_empty());
}

#[test]
fn assets_group_by_their_folder() {
    let list = paths(&["dev/grid", "tools/clip", "dev/wall", "props/crate"]);
    let folders = folders(&list);

    assert_eq!(folders.len(), 3);
    assert_eq!(folders[0].name, "dev");
    assert_eq!(folders[0].items, paths(&["dev/grid", "dev/wall"]));
}

#[test]
fn folders_come_out_in_a_stable_order() {
    // A list that reshuffles between frames is one you cannot point at.
    let list = paths(&["tools/clip", "dev/grid", "props/crate"]);
    let names: Vec<String> = folders(&list).into_iter().map(|f| f.name).collect();
    assert_eq!(names, vec!["dev", "props", "tools"]);
}

#[test]
fn items_keep_the_order_they_arrived_in() {
    // Which is already sorted, coming off a directory scan -- and re-sorting
    // it here would be a second opinion nobody asked for.
    let list = paths(&["dev/wall", "dev/grid"]);
    assert_eq!(folders(&list)[0].items, paths(&["dev/wall", "dev/grid"]));
}

#[test]
fn loose_names_go_last_rather_than_first() {
    // A list that opens with the odd ones out reads as though something is
    // wrong with it.
    let list = paths(&["loose", "dev/grid"]);
    let folders = folders(&list);
    assert_eq!(folders[0].name, "dev");
    assert_eq!(folders[1].name, "");
    assert_eq!(folders[1].items, paths(&["loose"]));
}

#[test]
fn a_nested_folder_keeps_its_whole_path() {
    let list = paths(&["props/furniture/chair", "props/crate"]);
    let names: Vec<String> = folders(&list).into_iter().map(|f| f.name).collect();
    assert_eq!(names, vec!["props", "props/furniture"]);
}

#[test]
fn the_leaf_is_what_goes_under_the_swatch() {
    assert_eq!(leaf("props/furniture/chair"), "chair");
    assert_eq!(leaf("loose"), "loose");
    assert_eq!(leaf(""), "");
}

#[test]
fn a_search_matches_words_in_any_order() {
    // The order you happen to remember a name in should not decide whether
    // you can find it.
    assert!(matches("props/crate_wood", "crate wood"));
    assert!(matches("props/crate_wood", "wood crate"));
    assert!(matches("props/crate_wood", "props"));
}

#[test]
fn a_search_ignores_case() {
    assert!(matches("Dev/Grid", "grid"));
    assert!(matches("dev/grid", "GRID"));
}

#[test]
fn every_word_has_to_appear() {
    assert!(!matches("props/crate_wood", "crate metal"));
}

#[test]
fn an_empty_search_keeps_everything() {
    let list = paths(&["dev/grid", "tools/clip"]);
    assert_eq!(filtered(&list, ""), list);
    assert_eq!(filtered(&list, "   "), list);
}

#[test]
fn filtering_keeps_the_order() {
    let list = paths(&["dev/wall", "dev/grid", "tools/grid_clip"]);
    assert_eq!(filtered(&list, "grid"), paths(&["dev/grid", "tools/grid_clip"]));
}

#[test]
fn a_search_that_matches_nothing_gives_nothing_rather_than_everything() {
    let list = paths(&["dev/grid"]);
    assert!(filtered(&list, "nonesuch").is_empty());
}
