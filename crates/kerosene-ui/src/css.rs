// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! A subset of CSS, parsed.
//!
//! Enough of the language that someone who has written a web page can style
//! a HUD without a manual: rules with selectors and specificity, comments,
//! `@keyframes` and `@font-face`. What is left out is left out on purpose --
//! media queries, `calc()`, custom properties, attribute selectors -- because
//! each is a real piece of engineering and none is what a game HUD is missing.
//!
//! Declarations are kept as text here and read into a
//! [`Style`](crate::style::Style) when the cascade applies them, so this file
//! knows nothing about what any property means. That keeps the grammar in one
//! place and the vocabulary in another.
//!
//! A mistake costs one rule, never the sheet: the parser logs a warning,
//! skips to the next `}` and carries on, the way a browser does. A typo in
//! one selector should not blank the whole HUD.

use std::collections::BTreeMap;

/// One `name: value` pair.
#[derive(Clone, PartialEq, Debug)]
pub struct Decl {
    pub name: String,
    pub value: String,
}

/// State a selector can ask about with a pseudo-class.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pseudo {
    Hover,
    Active,
    Focus,
    Disabled,
    Checked,
    FirstChild,
    LastChild,
}

impl Pseudo {
    fn parse(name: &str) -> Option<Pseudo> {
        Some(match name {
            "hover" => Pseudo::Hover,
            "active" => Pseudo::Active,
            "focus" => Pseudo::Focus,
            "disabled" => Pseudo::Disabled,
            "checked" | "selected" => Pseudo::Checked,
            "first-child" => Pseudo::FirstChild,
            "last-child" => Pseudo::LastChild,
            _ => return None,
        })
    }
}

/// `Label.big#title:hover` -- one element's worth of conditions.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Compound {
    /// Element name, compared case-insensitively. `None` for `*` or omitted.
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub pseudo: Vec<Pseudo>,
}

/// How a compound relates to the one after it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Combinator {
    /// Whitespace: any ancestor.
    Descendant,
    /// `>`: the parent.
    Child,
}

/// A full selector, stored rightmost-first: matching starts at the element
/// and walks outward, which is how every engine matches and why.
#[derive(Clone, PartialEq, Debug)]
pub struct Selector {
    /// The subject: what the rule styles.
    pub subject: Compound,
    /// Everything to its left, nearest first, each with the combinator that
    /// joins it to the compound on its right.
    pub ancestors: Vec<(Combinator, Compound)>,
}

impl Selector {
    /// CSS specificity, packed as `ids << 16 | classes << 8 | tags`.
    pub fn specificity(&self) -> u32 {
        let mut ids = 0;
        let mut classes = 0;
        let mut tags = 0;
        for c in std::iter::once(&self.subject).chain(self.ancestors.iter().map(|(_, c)| c)) {
            ids += u32::from(c.id.is_some());
            classes += (c.classes.len() + c.pseudo.len()) as u32;
            tags += u32::from(c.tag.is_some());
        }
        (ids.min(255) << 16) | (classes.min(255) << 8) | tags.min(255)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Rule {
    pub selector: Selector,
    pub specificity: u32,
    pub decls: Vec<Decl>,
}

/// `@keyframes`: declarations at points through an animation.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Keyframes {
    /// `(fraction 0..=1, declarations)`, sorted by fraction.
    pub stops: Vec<(f32, Vec<Decl>)>,
}

/// `@font-face`: a family name bound to a font file.
#[derive(Clone, PartialEq, Debug)]
pub struct FontFace {
    pub family: String,
    pub src: String,
    pub bold: bool,
}

#[derive(Clone, Default, Debug)]
pub struct StyleSheet {
    /// Where it came from, for messages.
    pub name: String,
    pub rules: Vec<Rule>,
    pub keyframes: BTreeMap<String, Keyframes>,
    pub font_faces: Vec<FontFace>,
    /// Everything the parser skipped, and why.
    pub warnings: Vec<String>,
}

impl StyleSheet {
    pub fn parse(name: &str, source: &str) -> StyleSheet {
        let mut sheet = StyleSheet {
            name: name.to_string(),
            ..Default::default()
        };
        let text = strip_comments(source);
        let mut rest = text.as_str();
        loop {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            let Some(open) = rest.find('{') else {
                sheet.warn(format!("stray text at end: {:?}", clip(rest)));
                break;
            };
            let prelude = rest[..open].trim();
            let Some(close) = matching_brace(rest, open) else {
                sheet.warn(format!("unclosed block after {:?}", clip(prelude)));
                break;
            };
            let body = &rest[open + 1..close];
            rest = &rest[close + 1..];

            if let Some(at) = prelude.strip_prefix('@') {
                sheet.at_rule(at, body);
            } else {
                sheet.style_rule(prelude, body);
            }
        }
        sheet
    }

    fn warn(&mut self, message: String) {
        self.warnings.push(format!("{}: {message}", self.name));
    }

    fn style_rule(&mut self, prelude: &str, body: &str) {
        let decls = parse_declarations(body);
        for part in prelude.split(',') {
            match parse_selector(part) {
                Some(selector) => self.rules.push(Rule {
                    specificity: selector.specificity(),
                    selector,
                    decls: decls.clone(),
                }),
                None => self.warn(format!("cannot read selector {:?}", part.trim())),
            }
        }
    }

    fn at_rule(&mut self, at: &str, body: &str) {
        let (keyword, name) = at.split_once(char::is_whitespace).unwrap_or((at, ""));
        match keyword {
            "keyframes" => {
                let name = name.trim().to_string();
                let mut frames = Keyframes::default();
                let mut rest = body;
                loop {
                    rest = rest.trim_start();
                    let Some(open) = rest.find('{') else { break };
                    let Some(close) = matching_brace(rest, open) else {
                        break;
                    };
                    let decls = parse_declarations(&rest[open + 1..close]);
                    for stop in rest[..open].split(',') {
                        let at = match stop.trim() {
                            "from" => Some(0.0),
                            "to" => Some(1.0),
                            s => s
                                .strip_suffix('%')
                                .and_then(|p| p.trim().parse::<f32>().ok())
                                .map(|p| p / 100.0),
                        };
                        match at {
                            Some(at) => frames.stops.push((at.clamp(0.0, 1.0), decls.clone())),
                            None => self.warn(format!("bad keyframe {:?} in {name}", stop.trim())),
                        }
                    }
                    rest = &rest[close + 1..];
                }
                frames.stops.sort_by(|a, b| a.0.total_cmp(&b.0));
                self.keyframes.insert(name, frames);
            }
            "font-face" => {
                let mut family = None;
                let mut src = None;
                let mut bold = false;
                for d in parse_declarations(body) {
                    match d.name.as_str() {
                        "font-family" => family = Some(unquote(&d.value).to_string()),
                        "src" => src = Some(url_or_text(&d.value).to_string()),
                        "font-weight" => bold = is_bold(&d.value),
                        _ => {}
                    }
                }
                match (family, src) {
                    (Some(family), Some(src)) => {
                        self.font_faces.push(FontFace { family, src, bold })
                    }
                    _ => self.warn("@font-face needs font-family and src".to_string()),
                }
            }
            other => self.warn(format!("@{other} is not supported")),
        }
    }
}

/// `bold`, `bolder` or a weight of 600 and up.
pub fn is_bold(value: &str) -> bool {
    let v = value.trim();
    v == "bold" || v == "bolder" || v.parse::<u32>().is_ok_and(|w| w >= 600)
}

/// Parse `a: b; c: d` into declarations. `!important` is accepted and
/// ignored: specificity is the only tiebreak here.
pub fn parse_declarations(body: &str) -> Vec<Decl> {
    let mut out = Vec::new();
    for part in split_top_level(body, ';') {
        let Some((name, value)) = part.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().trim_end_matches("!important").trim();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        out.push(Decl {
            name,
            value: value.to_string(),
        });
    }
    out
}

/// Split on `sep`, ignoring separators inside parentheses or quotes, so
/// `rgba(0, 0, 0, 1)` and `url("a;b")` survive.
pub fn split_top_level(text: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, c) if c == sep && depth <= 0 => {
                out.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

pub fn parse_selector(text: &str) -> Option<Selector> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // Tokenise into compounds and combinators. `a>b`, `a > b` and `a  b` all
    // have to come out right.
    let spaced = text.replace('>', " > ");
    let mut compounds: Vec<Compound> = Vec::new();
    let mut combinators: Vec<Combinator> = Vec::new();
    let mut pending = Combinator::Descendant;
    for token in spaced.split_whitespace() {
        if token == ">" {
            if compounds.is_empty() {
                return None;
            }
            pending = Combinator::Child;
            continue;
        }
        let compound = parse_compound(token)?;
        if !compounds.is_empty() {
            combinators.push(pending);
        }
        pending = Combinator::Descendant;
        compounds.push(compound);
    }
    let subject = compounds.pop()?;
    let mut ancestors = Vec::new();
    while let Some(c) = compounds.pop() {
        ancestors.push((combinators.pop()?, c));
    }
    Some(Selector { subject, ancestors })
}

fn parse_compound(token: &str) -> Option<Compound> {
    let mut compound = Compound::default();
    let mut chars = token.char_indices().peekable();
    // A leading run of name characters is the tag.
    let tag_end = token.find(['.', '#', ':']).unwrap_or(token.len());
    let tag = &token[..tag_end];
    if !tag.is_empty() && tag != "*" {
        if !tag.chars().all(is_name_char) {
            return None;
        }
        compound.tag = Some(tag.to_ascii_lowercase());
    }
    while let Some(&(i, _)) = chars.peek() {
        if i >= tag_end {
            break;
        }
        chars.next();
    }
    while let Some((_, marker)) = chars.next() {
        let mut name = String::new();
        while let Some(&(_, c)) = chars.peek() {
            if !is_name_char(c) {
                break;
            }
            name.push(c);
            chars.next();
        }
        if name.is_empty() {
            return None;
        }
        match marker {
            '.' => compound.classes.push(name),
            '#' => compound.id = Some(name),
            ':' => compound.pseudo.push(Pseudo::parse(&name)?),
            _ => return None,
        }
    }
    Some(compound)
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// The index of the `}` that closes the `{` at `open`.
fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for (i, c) in text[open..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
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

fn clip(text: &str) -> &str {
    let end = text.char_indices().nth(40).map_or(text.len(), |(i, _)| i);
    &text[..end]
}

/// Strip one layer of matching quotes.
pub fn unquote(text: &str) -> &str {
    let t = text.trim();
    for q in ['"', '\''] {
        if let Some(inner) = t.strip_prefix(q).and_then(|t| t.strip_suffix(q)) {
            return inner;
        }
    }
    t
}

/// `url("x")` gives `x`; anything else is returned unquoted.
pub fn url_or_text(text: &str) -> &str {
    let t = text.trim();
    match t.strip_prefix("url(").and_then(|t| t.strip_suffix(')')) {
        Some(inner) => unquote(inner),
        None => unquote(t),
    }
}

#[cfg(test)]
mod tests;
