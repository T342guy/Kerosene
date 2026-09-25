// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Bindings: markup that watches the store.
//!
//! This is the HTMX idea brought to a HUD. Rather than a script that listens
//! for `weapon_changed` and then finds the crosshair and then swaps its class,
//! the crosshair *says* what it depends on:
//!
//! ```xml
//! <Panel class="crosshair xh-{weapon.active}" class:firing="{weapon.firing}"/>
//! <Label text="{weapon.ammo} / {weapon.reserve}" class:low="{weapon.ammo < 5}"/>
//! ```
//!
//! Anything in braces is a Rhai expression, compiled once when the layout
//! loads. Store keys are variables -- `player.health` is property access on
//! the `player` map -- so the whole expression language is available: maths,
//! comparisons, `if` expressions, string building.
//!
//! Each expression records the store keys it reads, found by scanning its
//! source for dotted names. A binding is evaluated again only when one of
//! those keys changes, so a HUD with a hundred bindings costs nothing on a
//! frame where only the clock moved. The scan is conservative: a name it
//! mistakes for a key only causes an extra evaluation, never a missed one.

use crate::store::key_affects;

/// One piece of a template: literal text, or an expression in braces.
#[derive(Clone, Debug)]
pub enum Piece {
    Lit(String),
    Code { source: String, ast: rhai::AST },
}

/// Text with `{expressions}` in it.
#[derive(Clone, Debug)]
pub struct Template {
    pub pieces: Vec<Piece>,
    /// Store keys the expressions read.
    pub deps: Vec<String>,
}

impl Template {
    /// Parse a template; `Ok(None)` if the text has no braces, so a plain
    /// attribute stays a plain attribute. `{{` and `}}` are literal braces.
    pub fn parse(text: &str, engine: &rhai::Engine) -> Result<Option<Template>, String> {
        if !text.contains('{') {
            return Ok(None);
        }
        let mut pieces = Vec::new();
        let mut deps = Vec::new();
        let mut lit = String::new();
        let bytes = text.as_bytes();
        let mut i = 0;
        let mut any_code = false;
        while i < text.len() {
            let c = bytes[i];
            if c == b'{' && bytes.get(i + 1) == Some(&b'{') {
                lit.push('{');
                i += 2;
                continue;
            }
            if c == b'}' && bytes.get(i + 1) == Some(&b'}') {
                lit.push('}');
                i += 2;
                continue;
            }
            if c != b'{' {
                let ch = text[i..].chars().next().unwrap_or(' ');
                lit.push(ch);
                i += ch.len_utf8();
                continue;
            }
            let end =
                matching_close(text, i).ok_or_else(|| format!("unclosed `{{` in {text:?}"))?;
            let source = text[i + 1..end].trim().to_string();
            if !lit.is_empty() {
                pieces.push(Piece::Lit(std::mem::take(&mut lit)));
            }
            let ast = engine
                .compile_expression(single_quotes_to_double(&source))
                .map_err(|e| format!("{{{source}}}: {e}"))?;
            for d in dependencies(&source) {
                if !deps.contains(&d) {
                    deps.push(d);
                }
            }
            pieces.push(Piece::Code { source, ast });
            any_code = true;
            i = end + 1;
        }
        if !lit.is_empty() {
            pieces.push(Piece::Lit(lit));
        }
        Ok(any_code.then_some(Template { pieces, deps }))
    }

    /// Whether a change to `key` means evaluating this again.
    pub fn depends_on(&self, key: &str) -> bool {
        self.deps.iter().any(|d| key_affects(key, d))
    }

    /// Evaluate. A template that is exactly one expression keeps the
    /// expression's type, so `visible="{player.alive}"` is a boolean; anything
    /// else is joined into text. A failing expression reads as empty and the
    /// error is returned alongside, so one bad binding cannot stop a frame.
    pub fn eval(
        &self,
        engine: &rhai::Engine,
        scope: &mut rhai::Scope<'_>,
    ) -> (rhai::Dynamic, Option<String>) {
        if let [Piece::Code { ast, source }] = self.pieces.as_slice() {
            return match engine.eval_ast_with_scope::<rhai::Dynamic>(scope, ast) {
                Ok(v) => (v, None),
                Err(e) => (rhai::Dynamic::UNIT, Some(format!("{{{source}}}: {e}"))),
            };
        }
        let mut out = String::new();
        let mut error = None;
        for piece in &self.pieces {
            match piece {
                Piece::Lit(s) => out.push_str(s),
                Piece::Code { ast, source } => {
                    match engine.eval_ast_with_scope::<rhai::Dynamic>(scope, ast) {
                        Ok(v) => out.push_str(&display(&v)),
                        Err(e) => error = Some(format!("{{{source}}}: {e}")),
                    }
                }
            }
        }
        (out.into(), error)
    }
}

/// Rewrite `'text'` as `"text"`.
///
/// Code in markup lives inside a double-quoted XML attribute, so the natural
/// way to write a string there is with single quotes -- which Rhai reads as a
/// character literal. A UI has no use for characters and every use for
/// strings, so single-quoted runs become strings before compiling.
pub fn single_quotes_to_double(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut chars = code.chars();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == '\\' {
                    out.push(c);
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                    continue;
                }
                if c == q {
                    quote = None;
                    out.push(if q == '\'' { '"' } else { c });
                } else if q == '\'' && c == '"' {
                    out.push_str("\\\"");
                } else {
                    out.push(c);
                }
            }
            None => match c {
                '"' | '`' => {
                    quote = Some(c);
                    out.push(c);
                }
                '\'' => {
                    quote = Some(c);
                    out.push('"');
                }
                _ => out.push(c),
            },
        }
    }
    out
}

/// How a value reads in text: whole floats without `.0`, unit as nothing.
pub fn display(v: &rhai::Dynamic) -> String {
    if v.is_unit() {
        return String::new();
    }
    crate::store::Value::from_dynamic(v).to_string()
}

/// Whether a value counts as true for `visible=` and `class:x=`.
pub fn truthy(v: &rhai::Dynamic) -> bool {
    if v.is_unit() {
        return false;
    }
    crate::store::Value::from_dynamic(v).truthy()
}

/// The `}` closing the `{` at `open`, skipping braces in strings and nested
/// ones (a Rhai map literal `#{...}` inside an expression).
fn matching_close(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for (i, c) in text[open..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'' | '`') => quote = Some(c),
            (None, '{') => depth += 1,
            (None, '}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

const KEYWORDS: &[&str] = &[
    "true", "false", "if", "else", "switch", "let", "const", "fn", "return", "in", "while", "loop",
    "for", "do", "until", "break", "continue", "this", "throw", "try", "catch", "import", "export",
    "as", "global", "private", "is", "type_of",
];

/// Dotted names an expression reads, outside strings and minus function
/// names: `weapon.ammo < 5 && ammo_colour(player.health)` gives
/// `weapon.ammo` and `player.health`.
pub fn dependencies(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if matches!(c, '"' | '\'' | '`') {
            quote = Some(c);
            i += 1;
            continue;
        }
        // A name, not preceded by a dot (that would be a property of
        // something already read) or a digit run (`1.5`).
        if (c.is_alphabetic() || c == '_')
            && (i == 0 || !(chars[i - 1] == '.' || chars[i - 1].is_alphanumeric()))
        {
            let mut segments = Vec::new();
            let mut start = i;
            loop {
                let mut end = start;
                while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                    end += 1;
                }
                segments.push(chars[start..end].iter().collect::<String>());
                i = end;
                if i + 1 < chars.len()
                    && chars[i] == '.'
                    && (chars[i + 1].is_alphabetic() || chars[i + 1] == '_')
                {
                    start = i + 1;
                    continue;
                }
                break;
            }
            // `name(` is a call: the last segment is a function or method.
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && chars[j] == '(' {
                segments.pop();
            }
            if let Some(first) = segments.first()
                && !KEYWORDS.contains(&first.as_str())
            {
                let path = segments.join(".");
                if !out.contains(&path) {
                    out.push(path);
                }
            }
            continue;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests;
