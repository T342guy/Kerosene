// SPDX-License-Identifier: LGPL-3.0-or-later
//! Grouping a flat list of asset names into something you can look through.
//!
//! Materials and models both arrive as a list of paths -- `dev/grid`,
//! `tools/clip`, `props/crate_wood` -- and both were shown as exactly that:
//! an ungrouped run of identical swatches in a 120-point column, two abreast,
//! with the name only on hover. Finding anything meant reading every tooltip.
//!
//! The paths already say how they should be grouped. Everything here is that
//! observation and nothing else, kept away from the drawing so that "which
//! folder is this in" and "does this match what I typed" are questions with
//! testable answers.

/// One folder's worth of assets.
#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    /// The path prefix, without a trailing slash. Empty for loose names.
    pub name: String,
    /// Full paths, in the order they were given.
    pub items: Vec<String>,
}

/// Group asset paths by their leading folder.
///
/// Folders come out in alphabetical order, and loose names -- ones with no
/// folder at all -- come last under an empty name, because a list that starts
/// with the odd ones out reads as though something is wrong.
pub fn folders(paths: &[String]) -> Vec<Folder> {
    let mut folders: Vec<Folder> = Vec::new();
    for path in paths {
        let name = match path.rsplit_once('/') {
            Some((folder, _)) => folder.to_string(),
            None => String::new(),
        };
        match folders.iter_mut().find(|f| f.name == name) {
            Some(folder) => folder.items.push(path.clone()),
            None => folders.push(Folder { name, items: vec![path.clone()] }),
        }
    }
    folders.sort_by(|a, b| match (a.name.is_empty(), b.name.is_empty()) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.name.cmp(&b.name),
    });
    folders
}

/// The last part of a path: what to write under a swatch.
pub fn leaf(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, leaf)| leaf)
}

/// Whether a path matches what someone typed.
///
/// Every word has to appear somewhere, in any order and any case, so `crate
/// wood` finds `props/crate_wood` and so does `wood crate`. Matching the
/// whole query as one substring instead means the order you happen to
/// remember a name in decides whether you can find it.
pub fn matches(path: &str, query: &str) -> bool {
    let path = path.to_ascii_lowercase();
    query
        .split_whitespace()
        .all(|word| path.contains(&word.to_ascii_lowercase()))
}

/// Filter a list, keeping the order.
pub fn filtered(paths: &[String], query: &str) -> Vec<String> {
    if query.trim().is_empty() { return paths.to_vec() }
    paths.iter().filter(|p| matches(p, query)).cloned().collect()
}

#[cfg(test)]
mod tests;
