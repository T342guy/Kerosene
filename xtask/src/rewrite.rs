// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Turning a crate's source into a module's.
//!
//! In the workspace, `kerosene-math` is a crate: its own items are
//! `crate::…` and other crates name it `kerosene_math::…`. In the bundle it
//! is the module `crate::__k::math`, so both spellings become that. The
//! rewrite is by token, not by parsing: an identifier that starts a path
//! (not after `::`, `.` or `$`) and is followed by `::` -- or is the whole
//! of a `use` -- is what is replaced. Everything else, strings and comments
//! included, is copied as it is; the bundle is built in CI, so a rewrite
//! that goes wrong is a compile error, not a quiet one.

use std::collections::HashMap;

/// How one file is rewritten.
pub struct Rewrite<'a> {
    /// Library names (`kerosene_math`, `chisel`) to the path that replaces
    /// them (`crate::__k::math`).
    pub crates: &'a HashMap<String, String>,
    /// What `crate::` becomes: the module's path, for a crate folded into
    /// the bundle. `None` for the bundle's root, where `crate` is `crate`.
    pub own: Option<&'a str>,
}

impl Rewrite<'_> {
    pub fn apply(&self, source: &str) -> String {
        let chars: Vec<char> = source.chars().collect();
        let mut out = String::with_capacity(source.len() + source.len() / 16);
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let starts_ident = (c.is_alphabetic() || c == '_')
                && (i == 0 || !is_ident(chars[i - 1]))
                && !follows_path(&chars, i);
            if !starts_ident {
                out.push(c);
                i += 1;
                continue;
            }
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let rest = &chars[i..];
            let path_follows = starts_with(rest, "::");
            let replacement = if word == "crate" && path_follows {
                self.own.map(str::to_string)
            } else if let Some(path) = self.crates.get(&word) {
                (path_follows || (is_used_whole(rest) && in_use(&chars, start)))
                    .then(|| path.clone())
            } else {
                None
            };
            out.push_str(replacement.as_deref().unwrap_or(&word));
        }
        out
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn starts_with(rest: &[char], text: &str) -> bool {
    let mut it = rest.iter();
    text.chars().all(|t| it.next() == Some(&t))
}

/// Whether the identifier at `i` continues a path or a field access, or is
/// a macro's `$crate`: `a::b`, `a.b`, `$crate`. Two dots are not a field
/// access: `..kerosene_vfs::x()` in a struct update, or a range's end, starts
/// a path of its own.
fn follows_path(chars: &[char], i: usize) -> bool {
    let mut j = i;
    while j > 0 && chars[j - 1] == ' ' {
        j -= 1;
    }
    let field = j >= 1 && chars[j - 1] == '.' && !(j >= 2 && chars[j - 2] == '.');
    (j >= 2 && chars[j - 1] == ':' && chars[j - 2] == ':')
        || field
        || (j >= 1 && chars[j - 1] == '$')
}

/// A crate named whole: `use chisel;`, `use kerosene_math as math;`.
fn is_used_whole(rest: &[char]) -> bool {
    let trimmed: Vec<char> = rest.iter().copied().skip_while(|c| *c == ' ').collect();
    matches!(trimmed.first(), Some(';' | ',' | '}')) || starts_with(&trimmed, "as ")
}

/// Whether the statement the word at `at` is in began with `use`.
fn in_use(chars: &[char], at: usize) -> bool {
    let line_start = chars[..at]
        .iter()
        .rposition(|c| *c == '\n')
        .map_or(0, |p| p + 1);
    let line: String = chars[line_start..at].iter().collect();
    let line = line.trim_start();
    line.starts_with("use ") || line.starts_with("pub use ") || line.starts_with("pub(crate) use ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crates() -> HashMap<String, String> {
        [
            ("kerosene_math", "crate::__k::math"),
            ("chisel", "crate::__k::chisel"),
            ("kerosene_tools", "crate::__k::tools"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
    }

    #[test]
    fn a_crates_own_paths_and_other_crates_become_module_paths() {
        let crates = crates();
        let r = Rewrite {
            crates: &crates,
            own: Some("crate::__k::bsp"),
        };
        let src = "use crate::io::read;\nuse kerosene_math::{Vec3, Aabb};\n\
                   pub(crate) fn f() -> kerosene_math::Vec3 { crate::io::g() }\n";
        assert_eq!(
            r.apply(src),
            "use crate::__k::bsp::io::read;\nuse crate::__k::math::{Vec3, Aabb};\n\
             pub(crate) fn f() -> crate::__k::math::Vec3 { crate::__k::bsp::io::g() }\n"
        );
    }

    #[test]
    fn a_crate_used_whole_is_rewritten_and_a_lookalike_is_not() {
        let crates = crates();
        let r = Rewrite {
            crates: &crates,
            own: None,
        };
        let src = "pub use chisel;\npub use kerosene_math as math;\npub use kerosene_tools::*;\n\
                   let chisel = 3; self.chisel::x; a.chisel; my_chisel::y; $crate::z; crate::w\n";
        assert_eq!(
            r.apply(src),
            "pub use crate::__k::chisel;\npub use crate::__k::math as math;\n\
             pub use crate::__k::tools::*;\n\
             let chisel = 3; self.chisel::x; a.chisel; my_chisel::y; $crate::z; crate::w\n"
        );
    }

    #[test]
    fn a_path_after_two_dots_is_rewritten() {
        // A struct update, `..kerosene_vfs::x()`, and a range's end are paths
        // of their own, not a field access like `a.kerosene_vfs`.
        let crates = crates();
        let r = Rewrite {
            crates: &crates,
            own: None,
        };
        assert_eq!(
            r.apply("S { a, ..kerosene_math::d() }; 0..kerosene_math::N; x.kerosene_math\n"),
            "S { a, ..crate::__k::math::d() }; 0..crate::__k::math::N; x.kerosene_math\n"
        );
    }
}
