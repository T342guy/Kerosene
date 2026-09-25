// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Layout files: XML in, an owned element tree out.
//!
//! A `.keroui` file is Panorama-shaped:
//!
//! ```xml
//! <root>
//!     <styles>
//!         <include src="ui/hud.kerocss"/>
//!     </styles>
//!     <scripts>
//!         <include src="ui/hud.keroscript"/>
//!     </scripts>
//!     <Panel id="hud">
//!         <Label class="health" text="{player.health}"/>
//!     </Panel>
//! </root>
//! ```
//!
//! This module only reads the file. What an element *is* is decided when a
//! [`Document`](crate::Document) instantiates it, because a `<Repeat>` will
//! instantiate the same elements many times over and the tree has to outlive
//! the XML text it came from.

/// One XML element, with everything the XML reader borrowed copied out.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Element {
    /// Element name as written (`Label`).
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    /// Direct text content, whitespace collapsed.
    pub text: String,
    /// 1-based line in the source, for messages.
    pub line: u32,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A parsed layout file.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Markup {
    /// Attributes on `<root>`: `interactive`, `z`, `reference-height`.
    pub root_attrs: Vec<(String, String)>,
    /// Stylesheet paths, in order.
    pub style_files: Vec<String>,
    /// `<style>` blocks written in the layout itself.
    pub inline_styles: Vec<String>,
    /// Script paths, in order.
    pub script_files: Vec<String>,
    /// `<script>` blocks written in the layout itself.
    pub inline_scripts: Vec<String>,
    /// The panels: everything under `<root>` that is not styles or scripts.
    pub body: Vec<Element>,
}

impl Markup {
    pub fn root_attr(&self, name: &str) -> Option<&str> {
        self.root_attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Read a layout file. `name` is what errors are reported against.
pub fn parse(name: &str, source: &str) -> Result<Markup, String> {
    let source = declare_prefixes(&escape_code(source));
    let doc = roxmltree::Document::parse(&source).map_err(|e| format!("{name}: {e}"))?;
    let root = doc.root_element();
    if !root.tag_name().name().eq_ignore_ascii_case("root") {
        return Err(format!(
            "{name}: the outermost element must be <root>, not <{}>",
            root.tag_name().name()
        ));
    }
    let mut markup = Markup {
        root_attrs: root
            .attributes()
            .map(|a| (attr_name(&a), a.value().to_string()))
            .collect(),
        ..Default::default()
    };
    for child in root.children().filter(|n| n.is_element()) {
        match child.tag_name().name().to_ascii_lowercase().as_str() {
            "styles" => collect_sources(
                child,
                "style",
                &mut markup.style_files,
                &mut markup.inline_styles,
            ),
            "scripts" => collect_sources(
                child,
                "script",
                &mut markup.script_files,
                &mut markup.inline_scripts,
            ),
            // A lone <style> or <script> directly under <root> is fine too.
            "style" => markup.inline_styles.push(raw_text(child)),
            "script" => markup.inline_scripts.push(raw_text(child)),
            _ => markup.body.push(convert(&doc, child)),
        }
    }
    Ok(markup)
}

#[cfg(test)]
mod tests;

/// Escape what XML forbids but code needs.
///
/// Bindings are code, and code compares: `class:low="{ammo < 5 && !reloading}"`
/// is the obvious way to write it and is two XML errors, a `<` and an `&` in an
/// attribute. The same goes for the body of a `<script>`. Rather than make
/// every layout author write `&lt;` and `&amp;&amp;`, those two characters are
/// escaped here -- inside attribute values and inside `<script>` and `<style>`
/// -- before the XML reader sees them. Entities already written (`&lt;`) are
/// left alone, so escaping by hand still works.
fn escape_code(source: &str) -> String {
    #[derive(PartialEq)]
    enum State {
        Text,
        Tag,
        Value(char),
        Raw(String),
    }
    let mut out = String::with_capacity(source.len() + 16);
    let mut state = State::Text;
    let mut tag = String::new();
    let mut i = 0;
    let bytes = source.as_bytes();
    let starts = |i: usize, s: &str| source[i..].starts_with(s);
    let is_entity = |i: usize| {
        let rest = &source[i + 1..];
        let end = rest.find(';').unwrap_or(0);
        end > 0
            && end < 10
            && rest[..end]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '#')
    };
    while i < source.len() {
        let c = source[i..].chars().next().unwrap_or(' ');
        let len = c.len_utf8();
        match &state {
            State::Text => {
                if starts(i, "<![CDATA[") {
                    let end = source[i..].find("]]>").map_or(source.len(), |e| i + e + 3);
                    out.push_str(&source[i..end]);
                    i = end;
                    continue;
                }
                if starts(i, "<!--") {
                    // XML forbids `--` inside a comment, and prose is full of
                    // them. A comment is thrown away, so its body is made
                    // legal rather than the author made to care.
                    let body_start = i + 4;
                    let body_end = source[body_start..]
                        .find("-->")
                        .map_or(source.len(), |e| body_start + e);
                    out.push_str("<!--");
                    out.push_str(&source[body_start..body_end].replace('-', " "));
                    out.push_str("-->");
                    i = (body_end + 3).min(source.len());
                    continue;
                }
                if c == '<' {
                    state = State::Tag;
                    tag.clear();
                }
                out.push(c);
            }
            State::Tag => {
                match c {
                    '"' | '\'' => state = State::Value(c),
                    '>' => {
                        let name = tag.trim_start_matches('/').to_ascii_lowercase();
                        let closing = tag.starts_with('/');
                        let self_closing = i > 0 && bytes[i - 1] == b'/';
                        state =
                            if !closing && !self_closing && (name == "script" || name == "style") {
                                State::Raw(name)
                            } else {
                                State::Text
                            };
                    }
                    c if tag.len() < 16
                        && (c.is_alphanumeric() || c == '/' || c == '_')
                        && !tag.contains(' ') =>
                    {
                        tag.push(c)
                    }
                    _ => tag.push(' '),
                }
                out.push(c);
            }
            State::Value(q) => {
                let q = *q;
                if c == q {
                    state = State::Tag;
                    out.push(c);
                } else if c == '<' {
                    out.push_str("&lt;");
                } else if c == '&' && !is_entity(i) {
                    out.push_str("&amp;");
                } else {
                    out.push(c);
                }
            }
            State::Raw(name) => {
                let close = format!("</{name}");
                if source[i..].len() >= close.len()
                    && source[i..i + close.len()].eq_ignore_ascii_case(&close)
                {
                    state = State::Tag;
                    tag.clear();
                    out.push('<');
                } else if c == '<' {
                    out.push_str("&lt;");
                } else if c == '&' && !is_entity(i) {
                    out.push_str("&amp;");
                } else {
                    out.push(c);
                }
            }
        }
        i += len;
    }
    out
}

/// Binding attributes are written `class:x`, `style:x` and `on:x`, which
/// XML reads as namespace prefixes. Declaring the three on `<root>` -- on the
/// same line, so error positions do not move -- makes them legal without
/// every layout having to say so.
const PREFIXES: [&str; 3] = ["class", "style", "on"];
const NAMESPACE: &str = "urn:kerosene:";

fn declare_prefixes(source: &str) -> String {
    // The first `<root` that is not inside a comment.
    let mut from = 0;
    let at = loop {
        let Some(found) = source[from..].find("<root").map(|f| from + f) else {
            return source.to_string();
        };
        let comment_open = source[..found].rfind("<!--");
        let comment_close = source[..found].rfind("-->");
        match (comment_open, comment_close) {
            (Some(open), close) if close.is_none_or(|c| c < open) => from = found + 5,
            _ => break found,
        }
    };
    let insert = at + "<root".len();
    let mut declared = String::new();
    for p in PREFIXES {
        if !source.contains(&format!("xmlns:{p}=")) {
            declared.push_str(&format!(" xmlns:{p}=\"{NAMESPACE}{p}\""));
        }
    }
    format!("{}{declared}{}", &source[..insert], &source[insert..])
}

/// An attribute's name as the layout wrote it, prefix included.
fn attr_name(a: &roxmltree::Attribute<'_, '_>) -> String {
    match a.namespace().and_then(|ns| ns.strip_prefix(NAMESPACE)) {
        Some(prefix) => format!("{prefix}:{}", a.name()),
        None => a.name().to_string(),
    }
}

fn collect_sources(
    node: roxmltree::Node<'_, '_>,
    inline_tag: &str,
    files: &mut Vec<String>,
    inline: &mut Vec<String>,
) {
    for child in node.children().filter(|n| n.is_element()) {
        let tag = child.tag_name().name().to_ascii_lowercase();
        if tag == "include" {
            if let Some(src) = child.attribute("src") {
                files.push(src.to_string());
            }
        } else if tag == inline_tag {
            inline.push(raw_text(child));
        }
    }
}

/// Text inside an element, verbatim (a script's newlines matter).
fn raw_text(node: roxmltree::Node<'_, '_>) -> String {
    node.children().filter_map(|c| c.text()).collect()
}

fn convert(doc: &roxmltree::Document<'_>, node: roxmltree::Node<'_, '_>) -> Element {
    let text: String = node
        .children()
        .filter(|c| c.is_text())
        .filter_map(|c| c.text())
        .collect::<Vec<_>>()
        .join(" ");
    Element {
        name: node.tag_name().name().to_string(),
        attrs: node
            .attributes()
            .map(|a| (attr_name(&a), a.value().to_string()))
            .collect(),
        children: node
            .children()
            .filter(|c| c.is_element())
            .map(|c| convert(doc, c))
            .collect(),
        text: text.split_whitespace().collect::<Vec<_>>().join(" "),
        line: doc.text_pos_at(node.range().start).row,
    }
}
