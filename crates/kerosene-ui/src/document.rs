// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! A document: one loaded layout, alive.
//!
//! Loading turns a [`Markup`] tree into panels, wires up bindings and
//! handlers, and runs the layout's scripts. After that a document is driven
//! one frame at a time by [`Document::update`], which does the same steps in
//! the same order every frame:
//!
//! 1. evaluate the bindings whose store keys changed,
//! 2. deliver events to `on:` handlers and the `on_event` hook,
//! 3. run due timers and queued input handlers,
//! 4. apply what the scripts asked for,
//! 5. cascade styles for panels whose classes or state changed,
//! 6. advance transitions and animations,
//! 7. lay out (taffy caches everything that did not change),
//! 8. rebuild the display list.
//!
//! Only step 8 happens unconditionally, and it is a walk over a few hundred
//! panels at most.

use crate::bind::{self, Template};
use crate::css::{self, Combinator, Compound, Decl, Pseudo, StyleSheet};
use crate::draw::{self, DisplayList, DrawItem, Quad, TextureRef};
use crate::markup::{self, Element, Markup};
use crate::script::{DocOp, PanelRef, Script};
use crate::store::{Event, UiStore, Value};
use crate::style::{AnimProp, Blend, Dim, Fit, Style, Timing};
use crate::text::{Fonts, TextLayout, TextParams};
use crate::{Images, Loader, LogLevel, UiAction, UiKey};
use std::collections::{BTreeMap, BTreeSet};

/// The stylesheet every document starts from, before its own.
pub const DEFAULT_CSS: &str = include_str!("default.kerocss");

/// Height of the reference screen UI pixels are measured against.
pub const REFERENCE_HEIGHT: f32 = 1080.0;

pub type NodeId = usize;

/// What sort of element a panel is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Panel,
    Label,
    Image,
    Button,
    TextEntry,
    Slider,
    Toggle,
    ProgressBar,
    Repeat,
}

impl Kind {
    fn from_name(name: &str) -> Option<Kind> {
        Some(match name.to_ascii_lowercase().as_str() {
            "panel" | "div" => Kind::Panel,
            "label" | "text" => Kind::Label,
            "image" | "img" => Kind::Image,
            "button" => Kind::Button,
            "textentry" | "input" => Kind::TextEntry,
            "slider" => Kind::Slider,
            "toggle" | "checkbox" => Kind::Toggle,
            "progressbar" | "progress" => Kind::ProgressBar,
            "repeat" => Kind::Repeat,
            _ => return None,
        })
    }

    /// Takes clicks and keyboard focus.
    fn focusable(self) -> bool {
        matches!(
            self,
            Kind::Button | Kind::TextEntry | Kind::Slider | Kind::Toggle
        )
    }
}

/// Pointer and focus state, for pseudo-classes.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct State {
    hover: bool,
    active: bool,
    focus: bool,
}

/// A transition in flight: one property easing from where it was.
#[derive(Clone, Debug)]
struct Running {
    prop: AnimProp,
    start: Box<Style>,
    elapsed: f32,
    duration: f32,
    timing: Timing,
}

/// Parts a composite element generated for itself, for CSS to style.
#[derive(Clone, Copy, Default, Debug)]
struct Parts {
    fill: Option<NodeId>,
    thumb: Option<NodeId>,
    label: Option<NodeId>,
}

pub(crate) struct Node {
    kind: Kind,
    /// Lower-cased element name, for type selectors.
    tag: String,
    id: Option<String>,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    alive: bool,

    static_classes: Vec<String>,
    /// From an interpolated `class="..."`.
    attr_classes: BTreeSet<String>,
    /// From `class:x=` bindings and scripts.
    toggled_classes: BTreeSet<String>,
    attrs: BTreeMap<String, String>,
    inline: Vec<Decl>,
    /// `style:prop=` bindings and `set_style`, by property.
    bound_style: BTreeMap<String, String>,
    handlers: BTreeMap<String, String>,
    /// Variables a `<Repeat>` gave this subtree.
    locals: Vec<(String, rhai::Dynamic)>,
    /// A `<Repeat>`'s template.
    template: Vec<Element>,
    parts: Parts,

    state: State,
    /// Style the cascade computed.
    style: Style,
    /// Style being shown: `style` plus whatever is animating.
    shown: Style,
    styled: bool,
    transitions: Vec<Running>,
    anim_time: f32,
    dirty: bool,

    taffy: taffy::NodeId,
    /// Layout box in UI pixels, absolute.
    rect: [f32; 4],
    /// Padding plus border, UI pixels: left, top, right, bottom.
    inner: [f32; 4],
    /// Where it was last drawn, for hit testing: physical rect, the inverse
    /// of its transform, and the clip it was under.
    hit: Option<([f32; 4], [f32; 6], Option<draw::ClipRect>)>,
    text_cache: Option<(String, f32, f32, TextLayout)>,
}

impl Node {
    fn has_class(&self, class: &str) -> bool {
        self.static_classes.iter().any(|c| c == class)
            || self.attr_classes.contains(class)
            || self.toggled_classes.contains(class)
    }

    fn flag(&self, name: &str) -> bool {
        self.attrs
            .get(name)
            .is_some_and(|v| !matches!(v.as_str(), "" | "false" | "0"))
    }

    fn disabled(&self) -> bool {
        self.flag("disabled")
    }

    fn checked(&self) -> bool {
        self.flag("checked")
    }

    fn number(&self, name: &str, default: f32) -> f32 {
        self.attrs
            .get(name)
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(default)
    }

    /// Text this panel draws itself, if it is the kind that does.
    fn display_text(&self) -> Option<String> {
        let text = match self.kind {
            Kind::Label => self.attrs.get("text").cloned().unwrap_or_default(),
            Kind::TextEntry => match self.attrs.get("value") {
                Some(v) if !v.is_empty() => {
                    if self.flag("password") {
                        "*".repeat(v.chars().count())
                    } else {
                        v.clone()
                    }
                }
                _ => self.attrs.get("placeholder").cloned().unwrap_or_default(),
            },
            _ => return None,
        };
        Some(if self.shown.uppercase {
            text.to_uppercase()
        } else {
            text
        })
    }

    /// Slider/progress position, 0..=1.
    fn fraction(&self) -> f32 {
        let min = self.number("min", 0.0);
        let max = self.number("max", 1.0);
        let v = self.number("value", min);
        if max > min {
            ((v - min) / (max - min)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// What a binding writes to.
#[derive(Clone, PartialEq, Debug)]
enum Target {
    Attr(String),
    Class(String),
    ClassList,
    Style(String),
    InlineStyle,
    /// A `<Repeat>`'s `count`.
    Count,
}

struct Binding {
    node: NodeId,
    target: Target,
    template: Template,
}

/// Work queued by input for the next update, when the store is available.
enum Pending {
    Handler { node: NodeId, attr: &'static str },
}

/// One live layout.
pub struct Document {
    pub path: String,
    nodes: Vec<Node>,
    root: NodeId,
    taffy: taffy::TaffyTree<NodeId>,
    sheets: Vec<StyleSheet>,
    bindings: Vec<Binding>,
    /// Panels made since the last update, whose bindings have never run.
    fresh: BTreeSet<NodeId>,
    script: Script,
    pending: Vec<Pending>,
    timers: Vec<(f32, String)>,
    /// Every file this document read, with a hash of what it held.
    pub sources: Vec<(String, u64)>,
    /// Takes the pointer and keyboard while shown.
    pub interactive: bool,
    /// Draw order among layers.
    pub z: i32,
    reference_height: f32,
    viewport: (u32, u32),
    scale: f32,
    seen_generation: Option<u64>,
    loaded: bool,
    time: f32,
    hovered: Option<NodeId>,
    pressed: Option<NodeId>,
    focused: Option<NodeId>,
    /// Draw order of the panels drawn last frame, for hit testing.
    draw_order: Vec<NodeId>,
    atlas_generation: u64,
    display: DisplayList,
    /// Problems worth one console line each; drained by the owner.
    pub messages: Vec<(LogLevel, String)>,
    reported: BTreeSet<String>,
    pub debug: bool,
    /// Store keys the document reads, for the engine to publish.
    deps: BTreeSet<String>,
}

/// A stable hash of a file's contents, to notice when it changes.
pub fn content_hash(bytes: &[u8]) -> u64 {
    // FNV-1a: no dependency, stable across runs, and plenty for "did this
    // file change".
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl Document {
    /// Load a layout and everything it includes.
    pub fn load(path: &str, loader: &dyn Loader, fonts: &mut Fonts) -> Result<Document, String> {
        let mut sources = Vec::new();
        let read = |p: &str, sources: &mut Vec<(String, u64)>| -> Result<String, String> {
            let bytes = loader.read(p).ok_or_else(|| format!("{p}: not found"))?;
            sources.push((p.to_string(), content_hash(&bytes)));
            String::from_utf8(bytes).map_err(|_| format!("{p}: not UTF-8"))
        };
        let text = read(path, &mut sources)?;
        let markup = markup::parse(path, &text)?;

        let mut doc = Document {
            path: path.to_string(),
            nodes: Vec::new(),
            root: 0,
            taffy: taffy::TaffyTree::new(),
            sheets: vec![StyleSheet::parse("(default)", DEFAULT_CSS)],
            bindings: Vec::new(),
            fresh: BTreeSet::new(),
            script: Script::new(),
            pending: Vec::new(),
            timers: Vec::new(),
            sources: Vec::new(),
            interactive: markup.root_attr("interactive").is_some_and(|v| v == "true"),
            z: markup
                .root_attr("z")
                .and_then(|z| z.parse().ok())
                .unwrap_or(0),
            reference_height: markup
                .root_attr("reference-height")
                .and_then(|h| h.parse().ok())
                .filter(|h: &f32| *h > 0.0)
                .unwrap_or(REFERENCE_HEIGHT),
            viewport: (0, 0),
            scale: 1.0,
            seen_generation: None,
            loaded: false,
            time: 0.0,
            hovered: None,
            pressed: None,
            focused: None,
            draw_order: Vec::new(),
            atlas_generation: u64::MAX,
            display: DisplayList::default(),
            messages: Vec::new(),
            reported: BTreeSet::new(),
            debug: false,
            deps: BTreeSet::new(),
        };

        for file in &markup.style_files {
            match read(file, &mut sources) {
                Ok(text) => doc.add_sheet(file, &text, loader, fonts, &mut sources),
                Err(e) => doc.warn(e),
            }
        }
        for (i, text) in markup.inline_styles.iter().enumerate() {
            doc.add_sheet(
                &format!("{path} <style> {}", i + 1),
                text,
                loader,
                fonts,
                &mut sources,
            );
        }
        doc.sources = sources;

        doc.instantiate_root(&markup, loader)?;

        for file in &markup.script_files {
            match loader.read(file) {
                Some(bytes) => {
                    doc.sources.push((file.clone(), content_hash(&bytes)));
                    let text = String::from_utf8_lossy(&bytes);
                    if let Err(e) = doc.script.load(file, &text) {
                        doc.error(e);
                    }
                }
                None => doc.warn(format!("{file}: not found")),
            }
        }
        for (i, text) in markup.inline_scripts.iter().enumerate() {
            if let Err(e) = doc.script.load(&format!("{path} <script> {}", i + 1), text) {
                doc.error(e);
            }
        }
        doc.refresh_ids();
        Ok(doc)
    }

    fn add_sheet(
        &mut self,
        name: &str,
        text: &str,
        loader: &dyn Loader,
        fonts: &mut Fonts,
        sources: &mut Vec<(String, u64)>,
    ) {
        let sheet = StyleSheet::parse(name, text);
        for w in &sheet.warnings {
            self.warn(w.clone());
        }
        for face in &sheet.font_faces {
            match loader.read(&face.src) {
                Some(bytes) => {
                    sources.push((face.src.clone(), content_hash(&bytes)));
                    if let Err(e) = fonts.add(&face.family, face.bold, bytes) {
                        self.warn(e);
                    }
                }
                None => self.warn(format!("{}: font not found", face.src)),
            }
        }
        self.sheets.push(sheet);
    }

    fn warn(&mut self, message: String) {
        self.report(LogLevel::Warn, message);
    }

    fn error(&mut self, message: String) {
        self.report(LogLevel::Error, message);
    }

    /// Queue a message, once: a binding failing every frame should say so
    /// once, not sixty times a second.
    fn report(&mut self, level: LogLevel, message: String) {
        if self.reported.insert(message.clone()) {
            self.messages
                .push((level, format!("{}: {message}", self.path)));
        }
    }

    // ---- building ----------------------------------------------------------

    fn new_node(&mut self, kind: Kind, tag: &str, parent: Option<NodeId>) -> NodeId {
        let id = self.nodes.len();
        let taffy = self
            .taffy
            .new_leaf_with_context(taffy::Style::default(), id)
            .expect("taffy node");
        self.nodes.push(Node {
            kind,
            tag: tag.to_ascii_lowercase(),
            id: None,
            parent,
            children: Vec::new(),
            alive: true,
            static_classes: Vec::new(),
            attr_classes: BTreeSet::new(),
            toggled_classes: BTreeSet::new(),
            attrs: BTreeMap::new(),
            inline: Vec::new(),
            bound_style: BTreeMap::new(),
            handlers: BTreeMap::new(),
            locals: parent
                .map(|p| self.nodes[p].locals.clone())
                .unwrap_or_default(),
            template: Vec::new(),
            parts: Parts::default(),
            state: State::default(),
            style: Style::default(),
            shown: Style::default(),
            styled: false,
            transitions: Vec::new(),
            anim_time: 0.0,
            dirty: true,
            taffy,
            rect: [0.0; 4],
            inner: [0.0; 4],
            hit: None,
            text_cache: None,
        });
        if let Some(p) = parent {
            self.nodes[p].children.push(id);
            // A `<Repeat>` is not a box: what it makes is laid out in its
            // parent, as if written there, so `#slots { flex-direction: row }`
            // arranges the slots a Repeat inside it made.
            let layout_parent = self.layout_parent(p);
            let _ = self.taffy.add_child(self.nodes[layout_parent].taffy, taffy);
        }
        id
    }

    /// The panel whose box a child of `node` is laid out in: `node`, or the
    /// nearest ancestor that is not a `<Repeat>`.
    fn layout_parent(&self, mut node: NodeId) -> NodeId {
        while self.nodes[node].kind == Kind::Repeat {
            match self.nodes[node].parent {
                Some(p) => node = p,
                None => break,
            }
        }
        node
    }

    /// Put a box's layout children back in document order, with every
    /// `<Repeat>` replaced by what it made.
    fn sync_layout_children(&mut self, node: NodeId) {
        fn flatten(nodes: &[Node], node: NodeId, out: &mut Vec<taffy::NodeId>) {
            for &c in &nodes[node].children {
                if !nodes[c].alive {
                    continue;
                }
                if nodes[c].kind == Kind::Repeat {
                    flatten(nodes, c, out);
                } else {
                    out.push(nodes[c].taffy);
                }
            }
        }
        let mut children = Vec::new();
        flatten(&self.nodes, node, &mut children);
        let _ = self.taffy.set_children(self.nodes[node].taffy, &children);
    }

    fn instantiate_root(&mut self, markup: &Markup, loader: &dyn Loader) -> Result<(), String> {
        let root = self.new_node(Kind::Panel, "root", None);
        self.root = root;
        self.nodes[root].static_classes.push("root".to_string());
        for el in &markup.body {
            self.instantiate(el, root, loader, 0);
        }
        Ok(())
    }

    fn instantiate(&mut self, el: &Element, parent: NodeId, loader: &dyn Loader, depth: u32) {
        if depth > 32 {
            self.error(format!(
                "line {}: elements nested too deep (an include loop?)",
                el.line
            ));
            return;
        }
        if el.name.eq_ignore_ascii_case("include") {
            let Some(src) = el.attr("src") else {
                self.warn(format!("line {}: <Include> without src", el.line));
                return;
            };
            let Some(bytes) = loader.read(src) else {
                self.warn(format!("{src}: not found"));
                return;
            };
            self.sources.push((src.to_string(), content_hash(&bytes)));
            match markup::parse(src, &String::from_utf8_lossy(&bytes)) {
                Ok(inner) => {
                    for child in &inner.body {
                        self.instantiate(child, parent, loader, depth + 1);
                    }
                }
                Err(e) => self.error(e),
            }
            return;
        }
        let kind = Kind::from_name(&el.name).unwrap_or_else(|| {
            self.warn(format!(
                "line {}: unknown element <{}>, treated as a Panel",
                el.line, el.name
            ));
            Kind::Panel
        });
        let node = self.new_node(kind, &el.name, Some(parent));

        if matches!(kind, Kind::Label) && !el.text.is_empty() {
            self.set_attr_or_bind(node, "text", &el.text, el.line);
        }
        for (name, value) in &el.attrs {
            self.attribute(node, name, value, el.line);
        }

        match kind {
            Kind::Repeat => {
                self.nodes[node].template = el.children.clone();
                self.rebuild_repeat(node, loader);
                return;
            }
            Kind::Button
                if !el.text.is_empty()
                    || self.nodes[node].attrs.contains_key("text")
                    || self.has_binding(node, "text") =>
            {
                self.make_label_part(node, &el.text, el.line);
            }
            Kind::Toggle => {
                let bx = self.new_node(Kind::Panel, "panel", Some(node));
                self.nodes[bx].static_classes.push("toggle-box".into());
                let knob = self.new_node(Kind::Panel, "panel", Some(bx));
                self.nodes[knob].static_classes.push("toggle-knob".into());
                self.make_label_part(node, &el.text, el.line);
            }
            Kind::Slider => {
                let track = self.new_node(Kind::Panel, "panel", Some(node));
                self.nodes[track].static_classes.push("slider-track".into());
                let fill = self.new_node(Kind::Panel, "panel", Some(track));
                self.nodes[fill].static_classes.push("slider-fill".into());
                let thumb = self.new_node(Kind::Panel, "panel", Some(node));
                self.nodes[thumb].static_classes.push("slider-thumb".into());
                self.nodes[node].parts.fill = Some(fill);
                self.nodes[node].parts.thumb = Some(thumb);
                self.sync_parts(node);
            }
            Kind::ProgressBar => {
                let fill = self.new_node(Kind::Panel, "panel", Some(node));
                self.nodes[fill].static_classes.push("progress-fill".into());
                self.nodes[node].parts.fill = Some(fill);
                self.sync_parts(node);
            }
            _ => {}
        }
        for child in &el.children {
            self.instantiate(child, node, loader, depth + 1);
        }
    }

    fn has_binding(&self, node: NodeId, attr: &str) -> bool {
        self.bindings
            .iter()
            .any(|b| b.node == node && b.target == Target::Attr(attr.to_string()))
    }

    /// A `<Button text="...">` or `<Toggle>` gets a Label child to hold its
    /// text, so the text is styled like any other label (`Button Label`).
    fn make_label_part(&mut self, node: NodeId, inner_text: &str, line: u32) {
        let label = self.new_node(Kind::Label, "label", Some(node));
        self.nodes[node].parts.label = Some(label);
        // The label follows the owner's `text`, bound or not.
        let text = self.nodes[node].attrs.get("text").cloned();
        match text {
            Some(t) => {
                self.nodes[label].attrs.insert("text".into(), t);
            }
            None if !inner_text.is_empty() => {
                self.set_attr_or_bind(label, "text", inner_text, line)
            }
            None => {}
        }
        for b in 0..self.bindings.len() {
            if self.bindings[b].node == node
                && self.bindings[b].target == Target::Attr("text".into())
            {
                let template = self.bindings[b].template.clone();
                self.bindings.push(Binding {
                    node: label,
                    target: Target::Attr("text".into()),
                    template,
                });
            }
        }
    }

    fn set_attr_or_bind(&mut self, node: NodeId, name: &str, value: &str, line: u32) {
        self.bind_or(node, Target::Attr(name.to_string()), value, line, |doc| {
            doc.nodes[node]
                .attrs
                .insert(name.to_string(), value.to_string());
        });
    }

    /// Bind `value` to `target` if it has `{...}` in it, else run `plain`.
    fn bind_or(
        &mut self,
        node: NodeId,
        target: Target,
        value: &str,
        line: u32,
        plain: impl FnOnce(&mut Document),
    ) {
        match Template::parse(value, &self.script.engine) {
            Ok(Some(template)) => {
                for d in &template.deps {
                    self.deps.insert(d.clone());
                }
                self.bindings.push(Binding {
                    node,
                    target,
                    template,
                });
            }
            Ok(None) => plain(self),
            Err(e) => self.error(format!("line {line}: {e}")),
        }
    }

    fn attribute(&mut self, node: NodeId, name: &str, value: &str, line: u32) {
        if let Some(event) = name.strip_prefix("on:") {
            self.nodes[node]
                .handlers
                .insert(format!("event:{event}"), value.to_string());
            return;
        }
        if let Some(class) = name.strip_prefix("class:") {
            let class = class.to_string();
            self.bind_or(node, Target::Class(class.clone()), value, line, |doc| {
                if bind::truthy(&rhai::Dynamic::from(Value::parse(value).truthy())) {
                    doc.nodes[node].toggled_classes.insert(class);
                }
            });
            return;
        }
        if let Some(prop) = name.strip_prefix("style:") {
            // XML names cannot start with `-`, so `style:kero-fill` stands
            // for `-kero-fill`.
            let prop = match prop.strip_prefix("kero-") {
                Some(rest) => format!("-kero-{rest}"),
                None => prop.to_string(),
            };
            self.bind_or(node, Target::Style(prop.clone()), value, line, |doc| {
                doc.nodes[node].bound_style.insert(prop, value.to_string());
            });
            return;
        }
        if name.starts_with("on") {
            self.nodes[node]
                .handlers
                .insert(name.to_string(), value.to_string());
            return;
        }
        match name {
            "id" => self.nodes[node].id = Some(value.to_string()),
            "class" => self.bind_or(node, Target::ClassList, value, line, |doc| {
                doc.nodes[node].static_classes =
                    value.split_whitespace().map(str::to_string).collect();
            }),
            "style" => self.bind_or(node, Target::InlineStyle, value, line, |doc| {
                doc.nodes[node].inline = css::parse_declarations(value);
            }),
            "count" if self.nodes[node].kind == Kind::Repeat => {
                self.bind_or(node, Target::Count, value, line, |doc| {
                    doc.nodes[node]
                        .attrs
                        .insert("count".into(), value.to_string());
                })
            }
            "cvar" => {
                // Two-way: the control shows the convar and writes it back.
                self.nodes[node]
                    .attrs
                    .insert("cvar".into(), value.to_string());
                let attr = if self.nodes[node].kind == Kind::Toggle {
                    "checked"
                } else {
                    "value"
                };
                let expr = format!("{{cvar.{value}}}");
                self.set_attr_or_bind(node, attr, &expr, line);
            }
            _ => self.set_attr_or_bind(node, name, value, line),
        }
    }

    fn rebuild_repeat(&mut self, node: NodeId, loader: &dyn Loader) {
        let old: Vec<NodeId> = std::mem::take(&mut self.nodes[node].children);
        for child in old {
            self.kill(child);
        }
        let count = self.nodes[node].number("count", 0.0).clamp(0.0, 1024.0) as i64;
        let var = self.nodes[node]
            .attrs
            .get("as")
            .cloned()
            .unwrap_or_else(|| "index".to_string());
        let template = self.nodes[node].template.clone();
        let base = self.nodes[node].locals.clone();
        for i in 0..count {
            // Children pick their locals up from the parent as they are
            // made, so set this pass's before instantiating.
            let mut locals = base.clone();
            locals.push((var.clone(), i.into()));
            self.nodes[node].locals = locals;
            for el in &template {
                self.instantiate(el, node, loader, 1);
            }
        }
        self.nodes[node].locals = base;
        self.nodes[node].dirty = true;
        let layout_parent = self.layout_parent(node);
        self.sync_layout_children(layout_parent);
        self.refresh_ids();
        // New bindings have never run.
        for n in 0..self.nodes.len() {
            if self.nodes[n].alive && self.is_under(n, node) {
                self.fresh.insert(n);
            }
        }
    }

    fn is_under(&self, mut node: NodeId, ancestor: NodeId) -> bool {
        while let Some(p) = self.nodes[node].parent {
            if p == ancestor {
                return true;
            }
            node = p;
        }
        false
    }

    fn kill(&mut self, node: NodeId) {
        let children = std::mem::take(&mut self.nodes[node].children);
        for c in children {
            self.kill(c);
        }
        self.nodes[node].alive = false;
        let _ = self.taffy.remove(self.nodes[node].taffy);
        self.bindings.retain(|b| b.node != node);
        for slot in [&mut self.hovered, &mut self.pressed, &mut self.focused] {
            if *slot == Some(node) {
                *slot = None;
            }
        }
    }

    fn refresh_ids(&mut self) {
        let mut sh = self.script.shared.borrow_mut();
        sh.ids.clear();
        for (i, n) in self.nodes.iter().enumerate() {
            if let (true, Some(id)) = (n.alive, &n.id) {
                sh.ids.entry(id.clone()).or_insert(i);
            }
        }
    }

    /// Keep a slider or progress bar's generated parts where its value says.
    fn sync_parts(&mut self, node: NodeId) {
        let f = self.nodes[node].fraction();
        let pct = format!("{}%", f * 100.0);
        if let Some(fill) = self.nodes[node].parts.fill {
            self.nodes[fill]
                .bound_style
                .insert("width".into(), pct.clone());
            self.nodes[fill].dirty = true;
        }
        if let Some(thumb) = self.nodes[node].parts.thumb {
            self.nodes[thumb].bound_style.insert("left".into(), pct);
            self.nodes[thumb].dirty = true;
        }
    }

    // ---- per frame ----------------------------------------------------------

    /// Store keys this document's bindings read.
    pub fn dependencies(&self) -> impl Iterator<Item = &str> {
        self.deps.iter().map(String::as_str)
    }

    /// Run one frame. `viewport` is the target in physical pixels.
    pub fn update(&mut self, ctx: &mut Frame<'_>) {
        self.time += ctx.dt;
        if self.viewport != ctx.viewport {
            self.viewport = ctx.viewport;
            self.scale = ctx.viewport.1.max(1) as f32 / self.reference_height;
            for n in &mut self.nodes {
                // `vw`/`vh` resolve at cascade, so a resize restyles.
                n.dirty = true;
                n.text_cache = None;
            }
        }

        {
            let mut sh = self.script.shared.borrow_mut();
            sh.time = f64::from(self.time);
            if self.seen_generation != Some(ctx.store.generation()) {
                sh.store = ctx
                    .store
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect();
            }
        }

        // 1. Bindings.
        let changed: Option<Vec<String>> = match self.seen_generation {
            None => None,
            Some(g) => Some(
                ctx.store
                    .changed_since(g)
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            ),
        };
        self.seen_generation = Some(ctx.store.generation());
        let due: Vec<usize> = match &changed {
            None => (0..self.bindings.len()).collect(),
            Some(keys) if keys.is_empty() => Vec::new(),
            Some(keys) => (0..self.bindings.len())
                .filter(|&b| keys.iter().any(|k| self.bindings[b].template.depends_on(k)))
                .collect(),
        };
        let mut due = due;
        // A `<Repeat>` whose count changed makes panels with bindings that
        // have never run; run them in the same frame, a few levels deep.
        for _ in 0..8 {
            if !self.fresh.is_empty() {
                let fresh = std::mem::take(&mut self.fresh);
                due.extend(
                    (0..self.bindings.len()).filter(|&b| fresh.contains(&self.bindings[b].node)),
                );
            }
            due.sort_unstable();
            due.dedup();
            if due.is_empty() {
                break;
            }
            self.run_bindings(&due, ctx);
            due.clear();
        }

        // Hooks see the same store the bindings did.
        if !self.loaded {
            self.loaded = true;
            self.call_hook("on_load", &[], ctx.store);
        }

        // 2. Events.
        for event in ctx.events {
            self.deliver(event, ctx.store);
        }

        // 3. Timers and input.
        let mut fired = Vec::new();
        self.timers.retain_mut(|(left, function)| {
            *left -= ctx.dt;
            if *left <= 0.0 {
                fired.push(std::mem::take(function));
                false
            } else {
                true
            }
        });
        for function in fired {
            self.call_hook(&function, &[], ctx.store);
        }
        for pending in std::mem::take(&mut self.pending) {
            match pending {
                Pending::Handler { node, attr } => self.run_node_handler(node, attr, "", ctx.store),
            }
        }

        // 4. What scripts asked for.
        self.apply_script_effects(ctx);

        // 5..8.
        self.restyle(ctx.viewport);
        let layout_changed = self.animate(ctx.dt);
        self.layout(ctx.fonts, layout_changed);
        self.build_display(ctx.fonts, ctx.images);
    }

    fn scope_roots(&self, store: &UiStore) -> Vec<(String, rhai::Dynamic)> {
        store.to_scope_maps().into_iter().collect()
    }

    fn run_bindings(&mut self, due: &[usize], ctx: &mut Frame<'_>) {
        let roots = self.scope_roots(ctx.store);
        let mark = self.script.scope.len();
        for (name, value) in &roots {
            self.script.scope.push_dynamic(name.clone(), value.clone());
        }
        // A key nobody has published yet reads as nothing rather than as an
        // error: every root a binding names exists, as an empty map if need
        // be, so `{objective.text}` is `()` until a script sets it and
        // `{ui.page != 'main'}` is true rather than a failure.
        let missing: BTreeSet<String> = self
            .deps
            .iter()
            .filter_map(|d| d.split('.').next())
            .filter(|root| !roots.iter().any(|(n, _)| n == root))
            .map(str::to_string)
            .collect();
        for root in missing {
            self.script
                .scope
                .push_dynamic(root, rhai::Map::new().into());
        }
        let mut results = Vec::with_capacity(due.len());
        for &b in due {
            let binding = &self.bindings[b];
            let node = binding.node;
            if !self.nodes[node].alive {
                continue;
            }
            let local_mark = self.script.scope.len();
            for (name, value) in &self.nodes[node].locals {
                self.script.scope.push_dynamic(name.clone(), value.clone());
            }
            let (value, error) = binding
                .template
                .eval(&self.script.engine, &mut self.script.scope);
            self.script.scope.rewind(local_mark);
            results.push((node, binding.target.clone(), value, error));
        }
        self.script.scope.rewind(mark);

        // Applied after every evaluation: a `<Repeat>` rebuilding itself
        // changes the binding list, so nothing may index it from here on.
        for (node, target, value, error) in results {
            if let Some(e) = error {
                // An unpublished key is the normal state of a HUD before the
                // game has said anything; only real errors are worth a line.
                if !e.contains("not found") && !e.contains("Unknown property") {
                    self.warn(e);
                }
            }
            if self.nodes[node].alive {
                self.apply_binding(node, &target, &value, ctx.loader);
            }
        }
    }

    fn apply_binding(
        &mut self,
        node: NodeId,
        target: &Target,
        value: &rhai::Dynamic,
        loader: &dyn Loader,
    ) {
        match target {
            Target::Attr(name) => {
                let text = bind::display(value);
                self.set_attr(node, name, text);
            }
            Target::Class(class) => {
                let on = bind::truthy(value);
                let n = &mut self.nodes[node];
                let changed = if on {
                    n.toggled_classes.insert(class.clone())
                } else {
                    n.toggled_classes.remove(class)
                };
                if changed {
                    n.dirty = true;
                }
            }
            Target::ClassList => {
                let classes: BTreeSet<String> = bind::display(value)
                    .split_whitespace()
                    .map(str::to_string)
                    .collect();
                let n = &mut self.nodes[node];
                if n.attr_classes != classes {
                    n.attr_classes = classes;
                    n.dirty = true;
                }
            }
            Target::Style(prop) => {
                let text = bind::display(value);
                let n = &mut self.nodes[node];
                if n.bound_style.get(prop) != Some(&text) {
                    n.bound_style.insert(prop.clone(), text);
                    n.dirty = true;
                }
            }
            Target::InlineStyle => {
                let decls = css::parse_declarations(&bind::display(value));
                let n = &mut self.nodes[node];
                if n.inline != decls {
                    n.inline = decls;
                    n.dirty = true;
                }
            }
            Target::Count => {
                let text = bind::display(value);
                if self.nodes[node].attrs.get("count") != Some(&text) {
                    self.nodes[node].attrs.insert("count".into(), text);
                    self.rebuild_repeat(node, loader);
                }
            }
        }
    }

    fn set_attr(&mut self, node: NodeId, name: &str, value: String) {
        let n = &mut self.nodes[node];
        if n.attrs.get(name) == Some(&value) {
            return;
        }
        n.attrs.insert(name.to_string(), value.clone());
        match name {
            "text" | "value" | "placeholder" => {
                n.text_cache = None;
                let _ = self.taffy.mark_dirty(n.taffy);
                if name == "text"
                    && let Some(label) = n.parts.label
                {
                    self.set_attr(label, "text", value);
                }
            }
            // State that selectors read.
            "visible" | "disabled" | "checked" | "src" => n.dirty = true,
            _ => {}
        }
        if matches!(name, "value" | "min" | "max") {
            self.sync_parts(node);
        }
    }

    fn call_hook(&mut self, name: &str, args: &[rhai::Dynamic], store: &UiStore) {
        let _ = store;
        if let Err(e) = self.script.call(name, args) {
            self.error(e);
        }
    }

    fn deliver(&mut self, event: &Event, store: &UiStore) {
        let key = format!("event:{}", event.name);
        let listeners: Vec<NodeId> = (0..self.nodes.len())
            .filter(|&n| self.nodes[n].alive && self.nodes[n].handlers.contains_key(&key))
            .collect();
        for node in listeners {
            let code = self.nodes[node].handlers[&key].clone();
            self.run_code(node, &code, &event.data, store);
        }
        self.call_hook(
            "on_event",
            &[event.name.clone().into(), event.data.clone().into()],
            store,
        );
    }

    fn run_node_handler(&mut self, node: NodeId, attr: &'static str, data: &str, store: &UiStore) {
        if !self.nodes[node].alive {
            return;
        }
        if let Some(code) = self.nodes[node].handlers.get(attr).cloned() {
            let data = if data.is_empty() {
                self.nodes[node]
                    .attrs
                    .get("value")
                    .cloned()
                    .unwrap_or_default()
            } else {
                data.to_string()
            };
            self.run_code(node, &code, &data, store);
        }
    }

    fn run_code(&mut self, node: NodeId, code: &str, data: &str, store: &UiStore) {
        let mut vars: Vec<(&'static str, rhai::Dynamic)> = Vec::new();
        let roots = self.scope_roots(store);
        let mark = self.script.scope.len();
        for (name, value) in roots {
            self.script.scope.push_dynamic(name, value);
        }
        for (name, value) in self.nodes[node].locals.clone() {
            self.script.scope.push_dynamic(name, value);
        }
        vars.push(("target", rhai::Dynamic::from(PanelRef { node: Some(node) })));
        vars.push(("data", data.to_string().into()));
        let result = self.script.run_handler(code, vars);
        self.script.scope.rewind(mark);
        if let Err(e) = result {
            self.error(e);
        }
    }

    fn apply_script_effects(&mut self, ctx: &mut Frame<'_>) {
        let (ops, actions, writes, missing) = {
            let mut sh = self.script.shared.borrow_mut();
            (
                std::mem::take(&mut sh.ops),
                std::mem::take(&mut sh.actions),
                std::mem::take(&mut sh.store_writes),
                std::mem::take(&mut sh.missing),
            )
        };
        for id in missing {
            self.warn(format!("panel(\"{id}\"): no element has that id"));
        }
        for op in ops {
            match op {
                DocOp::SetClass { node, class, on } => {
                    let n = &mut self.nodes[node];
                    if if on {
                        n.toggled_classes.insert(class)
                    } else {
                        n.toggled_classes.remove(&class)
                    } {
                        n.dirty = true;
                    }
                }
                DocOp::ToggleClass { node, class } => {
                    let n = &mut self.nodes[node];
                    if !n.toggled_classes.remove(&class) {
                        n.toggled_classes.insert(class);
                    }
                    n.dirty = true;
                }
                DocOp::TriggerClass { node, class } => {
                    let n = &mut self.nodes[node];
                    n.toggled_classes.insert(class);
                    n.dirty = true;
                    // Restart even if the class was already there: that is
                    // the difference from add_class.
                    self.restart_animations(node);
                }
                DocOp::SetAttr { node, name, value } => self.set_attr(node, &name, value),
                DocOp::SetStyle { node, prop, value } => {
                    let n = &mut self.nodes[node];
                    n.bound_style.insert(prop, value);
                    n.dirty = true;
                }
                DocOp::Focus { node } => self.set_focus(Some(node)),
                DocOp::Schedule { seconds, function } => {
                    if self.timers.len() < 256 {
                        self.timers.push((seconds.max(0.0), function));
                    }
                }
            }
        }
        for (key, value) in writes {
            ctx.store_writes.push((key, value));
        }
        for action in actions {
            if let UiAction::Log(level, text) = action {
                self.messages
                    .push((level, format!("{}: {text}", self.path)));
            } else {
                ctx.actions.push(action);
            }
        }
    }

    fn restart_animations(&mut self, node: NodeId) {
        let children = self.nodes[node].children.clone();
        self.nodes[node].anim_time = 0.0;
        for c in children {
            self.restart_animations(c);
        }
    }

    // ---- style ----------------------------------------------------------------

    fn restyle(&mut self, viewport: (u32, u32)) {
        let ui_viewport = (
            viewport.0 as f32 / self.scale,
            viewport.1 as f32 / self.scale,
        );
        // Topmost dirty panels restyle their whole subtree: descendant
        // selectors and inheritance both flow down.
        let mut roots = Vec::new();
        for n in 0..self.nodes.len() {
            if !self.nodes[n].alive || !self.nodes[n].dirty {
                continue;
            }
            let mut p = self.nodes[n].parent;
            let mut covered = false;
            while let Some(pp) = p {
                if self.nodes[pp].dirty {
                    covered = true;
                    break;
                }
                p = self.nodes[pp].parent;
            }
            if !covered {
                roots.push(n);
            }
        }
        for n in roots {
            let parent = match self.nodes[n].parent {
                Some(p) => self.nodes[p].style.clone(),
                None => Style::default(),
            };
            self.cascade(n, &parent, ui_viewport);
        }
    }

    fn cascade(&mut self, node: NodeId, parent: &Style, viewport: (f32, f32)) {
        let mut style = Style::inherit(parent);
        let mut matched: Vec<(u32, usize, usize, usize)> = Vec::new();
        let mut order = 0;
        for (s, sheet) in self.sheets.iter().enumerate() {
            for (r, rule) in sheet.rules.iter().enumerate() {
                if selector_matches(
                    &self.nodes,
                    node,
                    &rule.selector.subject,
                    &rule.selector.ancestors,
                ) {
                    matched.push((rule.specificity, order, s, r));
                }
                order += 1;
            }
        }
        matched.sort_unstable_by_key(|m| (m.0, m.1));
        let mut errors = Vec::new();
        for (_, _, s, r) in matched {
            style.apply_all(&self.sheets[s].rules[r].decls, viewport, &mut errors);
        }
        let n = &self.nodes[node];
        style.apply_all(&n.inline, viewport, &mut errors);
        for (prop, value) in &n.bound_style {
            if !style.apply(prop, value, viewport) {
                errors.push(format!("{prop}: {value}"));
            }
        }
        // Attributes that are really style.
        if n.attrs
            .get("visible")
            .is_some_and(|v| matches!(v.as_str(), "false" | "0" | ""))
        {
            style.display = false;
        }
        if let Some(src) = n.attrs.get("src").filter(|s| !s.is_empty())
            && n.kind == Kind::Image
        {
            style.background_image = Some(src.clone());
        }
        if n.kind == Kind::Image
            && !n.bound_style.contains_key("background-size")
            && style.background_fit == Fit::Fill
        {
            // Images keep their shape unless told otherwise.
            style.background_fit = Fit::Contain;
        }
        for e in errors {
            self.warn(format!("cannot read `{e}`"));
        }

        self.retarget(node, style);
        self.nodes[node].dirty = false;
        let children = self.nodes[node].children.clone();
        let style = self.nodes[node].style.clone();
        for c in children {
            if self.nodes[c].alive {
                self.cascade(c, &style, viewport);
            }
        }
    }

    /// Adopt a newly computed style, starting transitions for what changed.
    fn retarget(&mut self, node: NodeId, new: Style) {
        let n = &mut self.nodes[node];
        if !n.styled {
            n.styled = true;
            n.shown = new.clone();
            n.style = new;
            let _ = self.taffy.set_style(n.taffy, to_taffy(&n.shown, n.kind));
            return;
        }
        if n.style == new {
            return;
        }
        for prop in AnimProp::ALL {
            if !prop.differs(&n.style, &new) {
                continue;
            }
            let spec = new
                .transitions
                .iter()
                .rev()
                .find(|t| t.prop.is_none_or(|p| p == prop))
                .filter(|t| t.duration > 0.0);
            n.transitions.retain(|t| t.prop != prop);
            if let Some(spec) = spec {
                n.transitions.push(Running {
                    prop,
                    start: Box::new(n.shown.clone()),
                    elapsed: -spec.delay,
                    duration: spec.duration,
                    timing: spec.timing,
                });
            }
        }
        let restart = match (&n.style.animation, &new.animation) {
            (Some(a), Some(b)) => a.name != b.name,
            (None, Some(_)) => true,
            _ => false,
        };
        if restart {
            n.anim_time = 0.0;
        }
        n.style = new;
    }

    /// Advance transitions and keyframe animations. Returns whether anything
    /// that affects layout moved.
    fn animate(&mut self, dt: f32) -> bool {
        let mut layout_changed = false;
        for id in 0..self.nodes.len() {
            let n = &mut self.nodes[id];
            if !n.alive {
                continue;
            }
            let mut shown = n.style.clone();
            if let Some(spec) = &n.style.animation {
                n.anim_time += dt;
                if let Some(frames) = find_keyframes(&self.sheets, &spec.name)
                    && frames.stops.len() >= 2
                    && spec.duration > 0.0
                {
                    let t = (n.anim_time - spec.delay).max(0.0) / spec.duration;
                    let iterations = spec.iterations.unwrap_or(f32::INFINITY);
                    let (cycle, mut p) = if t >= iterations {
                        (iterations.ceil() - 1.0, 1.0)
                    } else {
                        (t.floor(), t.fract())
                    };
                    if spec.alternate && (cycle as i64) % 2 == 1 {
                        p = 1.0 - p;
                    }
                    let p = spec.timing.apply(p);
                    let b = frames
                        .stops
                        .iter()
                        .position(|s| s.0 >= p)
                        .unwrap_or(frames.stops.len() - 1)
                        .max(1);
                    let (a_at, a_decls) = &frames.stops[b - 1];
                    let (b_at, b_decls) = &frames.stops[b];
                    let span = (b_at - a_at).max(1e-6);
                    let local = ((p - a_at) / span).clamp(0.0, 1.0);
                    let viewport = (0.0, 0.0);
                    let mut from = n.style.clone();
                    let mut to = n.style.clone();
                    let mut ignored = Vec::new();
                    from.apply_all(a_decls, viewport, &mut ignored);
                    to.apply_all(b_decls, viewport, &mut ignored);
                    for prop in AnimProp::ALL {
                        prop.blend(&mut shown, &from, &to, local);
                    }
                }
            }
            if !n.transitions.is_empty() {
                let base = shown.clone();
                n.transitions.retain_mut(|t| {
                    t.elapsed += dt;
                    let k = t.timing.apply((t.elapsed / t.duration).clamp(0.0, 1.0));
                    t.prop.blend(&mut shown, &t.start, &base, k);
                    t.elapsed < t.duration
                });
            }
            if shown != n.shown {
                if AnimProp::ALL
                    .iter()
                    .any(|p| p.affects_layout() && p.differs(&shown, &n.shown))
                    || layout_differs(&shown, &n.shown)
                {
                    let _ = self.taffy.set_style(n.taffy, to_taffy(&shown, n.kind));
                    layout_changed = true;
                }
                if shown.font_size != n.shown.font_size
                    || shown.font_family != n.shown.font_family
                    || shown.bold != n.shown.bold
                    || shown.uppercase != n.shown.uppercase
                    || shown.letter_spacing != n.shown.letter_spacing
                    || shown.line_height != n.shown.line_height
                    || shown.wrap != n.shown.wrap
                    || shown.text_align != n.shown.text_align
                {
                    n.text_cache = None;
                    let _ = self.taffy.mark_dirty(n.taffy);
                    layout_changed = true;
                }
                n.shown = shown;
            }
        }
        layout_changed
    }

    // ---- layout ---------------------------------------------------------------

    fn layout(&mut self, fonts: &Fonts, _changed: bool) {
        let w = self.viewport.0 as f32 / self.scale;
        let h = self.viewport.1 as f32 / self.scale;
        let root = self.nodes[self.root].taffy;
        let mut root_style = to_taffy(&self.nodes[self.root].shown, Kind::Panel);
        root_style.size = taffy::Size {
            width: taffy::Dimension::length(w),
            height: taffy::Dimension::length(h),
        };
        root_style.position = taffy::Position::Relative;
        let _ = self.taffy.set_style(root, root_style);

        let nodes = &self.nodes;
        let _ = self.taffy.compute_layout_with_measure(
            root,
            taffy::Size {
                width: taffy::AvailableSpace::Definite(w),
                height: taffy::AvailableSpace::Definite(h),
            },
            |known, available, _id, context, _style| {
                let Some(&mut id) = context else {
                    return taffy::Size::ZERO;
                };
                let n = &nodes[id];
                let Some(text) = n.display_text() else {
                    return taffy::Size::ZERO;
                };
                if let (Some(w), Some(h)) = (known.width, known.height) {
                    return taffy::Size {
                        width: w,
                        height: h,
                    };
                }
                let max = known.width.or(match available.width {
                    taffy::AvailableSpace::Definite(w) => Some(w),
                    taffy::AvailableSpace::MinContent => Some(0.0),
                    taffy::AvailableSpace::MaxContent => None,
                });
                let (mw, mh) = fonts.measure(&text, text_params(&n.shown), max);
                // Glyph advances are fractional; round up so the text is
                // never a hair wider than the box it measured for.
                taffy::Size {
                    width: known.width.unwrap_or(mw.ceil()),
                    height: known.height.unwrap_or(mh.ceil()),
                }
            },
        );
        self.read_layout(self.root, 0.0, 0.0);
    }

    fn read_layout(&mut self, node: NodeId, px: f32, py: f32) {
        if self.nodes[node].kind == Kind::Repeat {
            // No box of its own: its children are positioned in the parent's.
            self.nodes[node].rect = [px, py, 0.0, 0.0];
            for c in self.nodes[node].children.clone() {
                if self.nodes[c].alive {
                    self.read_layout(c, px, py);
                }
            }
            return;
        }
        let Ok(l) = self.taffy.layout(self.nodes[node].taffy) else {
            return;
        };
        let (x, y) = (px + l.location.x, py + l.location.y);
        let inner = [
            l.padding.left + l.border.left,
            l.padding.top + l.border.top,
            l.padding.right + l.border.right,
            l.padding.bottom + l.border.bottom,
        ];
        let size = (l.size.width, l.size.height);
        let n = &mut self.nodes[node];
        n.rect = [x, y, size.0, size.1];
        n.inner = inner;
        for c in n.children.clone() {
            if self.nodes[c].alive {
                self.read_layout(c, x, y);
            }
        }
    }

    // ---- drawing --------------------------------------------------------------

    /// What this document drew last update.
    pub fn display_list(&self) -> &DisplayList {
        &self.display
    }

    fn build_display(&mut self, fonts: &mut Fonts, images: &mut Images) {
        let mut list = DisplayList {
            items: Vec::new(),
            size: self.viewport,
        };
        self.draw_order.clear();
        if fonts.atlas.generation != self.atlas_generation {
            self.atlas_generation = fonts.atlas.generation;
            for n in &mut self.nodes {
                n.text_cache = None;
            }
        }
        for n in &mut self.nodes {
            n.hit = None;
        }
        let mut clip = None;
        self.draw_node(
            self.root,
            1.0,
            draw::IDENTITY,
            &mut clip,
            &mut list,
            fonts,
            images,
        );
        if clip.is_some() {
            list.items.push(DrawItem::Clip(None));
        }
        self.display = list;
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_node(
        &mut self,
        node: NodeId,
        parent_opacity: f32,
        parent_transform: [f32; 6],
        clip: &mut Option<draw::ClipRect>,
        list: &mut DisplayList,
        fonts: &mut Fonts,
        images: &mut Images,
    ) {
        let s = self.scale;
        let (style, rect, inner, kind) = {
            let n = &self.nodes[node];
            if !n.alive || !n.shown.display || !n.shown.visible {
                return;
            }
            (n.shown.clone(), n.rect, n.inner, n.kind)
        };
        let opacity = parent_opacity * style.opacity;
        if opacity <= 0.001 {
            return;
        }
        let phys = [rect[0] * s, rect[1] * s, rect[2] * s, rect[3] * s];

        let transform = if style.transform.is_identity() {
            parent_transform
        } else {
            let t = style.transform;
            let tx = t.translate.0.resolve(rect[2]).unwrap_or(0.0) * s;
            let ty = t.translate.1.resolve(rect[3]).unwrap_or(0.0) * s;
            let (cx, cy) = (phys[0] + phys[2] * 0.5, phys[1] + phys[3] * 0.5);
            let (sin, cos) = t.rotate.to_radians().sin_cos();
            // Translate to the centre, rotate and scale, translate back, then
            // the offset: CSS's default transform-origin.
            let local = [
                cos * t.scale.0,
                -sin * t.scale.1,
                cx + tx - (cos * t.scale.0 * cx - sin * t.scale.1 * cy),
                sin * t.scale.0,
                cos * t.scale.1,
                cy + ty - (sin * t.scale.0 * cx + cos * t.scale.1 * cy),
            ];
            draw::compose(parent_transform, local)
        };

        let radius = if style.border_radius < 0.0 {
            -style.border_radius * phys[2].min(phys[3])
        } else {
            style.border_radius * s
        };
        let fill = style.fill.encode();
        let additive = style.blend == Blend::Additive;
        let premul = |c: crate::style::Color| [c.0[0], c.0[1], c.0[2], c.0[3] * opacity];

        if let Some((blur, color)) = style.box_shadow {
            let b = blur * s;
            list.items.push(DrawItem::Quad(Quad {
                rect: [
                    phys[0] - b,
                    phys[1] - b,
                    phys[2] + 2.0 * b,
                    phys[3] + 2.0 * b,
                ],
                color: premul(color),
                color2: premul(color),
                radius: radius + b,
                softness: b.max(1.0),
                transform,
                additive,
                ..Default::default()
            }));
        }

        if style.background_color.is_visible()
            || style.gradient.is_some()
            || style.border_width > 0.0
        {
            let (color2, gradient) = match style.gradient {
                Some((c, true)) => (premul(c), 1),
                Some((c, false)) => (premul(c), 2),
                None => (premul(style.background_color), 0),
            };
            list.items.push(DrawItem::Quad(Quad {
                rect: phys,
                color: premul(style.background_color),
                color2,
                gradient,
                radius,
                border_width: style.border_width * s,
                border_color: premul(style.border_color),
                fill,
                transform,
                additive,
                ..Default::default()
            }));
        }

        if let Some(src) = &style.background_image {
            let id = images.intern(src);
            let (mut r, mut uv) = (phys, [0.0, 0.0, 1.0, 1.0]);
            if let Some((iw, ih)) = images.size(id)
                && iw > 0
                && ih > 0
            {
                let image_aspect = iw as f32 / ih as f32;
                let box_aspect = phys[2] / phys[3].max(1e-3);
                match style.background_fit {
                    Fit::Fill => {}
                    Fit::Contain => {
                        if image_aspect > box_aspect {
                            let h = phys[2] / image_aspect;
                            r = [phys[0], phys[1] + (phys[3] - h) * 0.5, phys[2], h];
                        } else {
                            let w = phys[3] * image_aspect;
                            r = [phys[0] + (phys[2] - w) * 0.5, phys[1], w, phys[3]];
                        }
                    }
                    Fit::Cover => {
                        if image_aspect > box_aspect {
                            let used = box_aspect / image_aspect;
                            uv = [(1.0 - used) * 0.5, 0.0, (1.0 + used) * 0.5, 1.0];
                        } else {
                            let used = image_aspect / box_aspect;
                            uv = [0.0, (1.0 - used) * 0.5, 1.0, (1.0 + used) * 0.5];
                        }
                    }
                }
            }
            let tint = premul(style.tint);
            list.items.push(DrawItem::Quad(Quad {
                rect: r,
                uv,
                texture: TextureRef::Image(id),
                color: tint,
                color2: tint,
                radius,
                fill,
                transform,
                additive,
                ..Default::default()
            }));
        }

        if let Some(text) = self.nodes[node].display_text()
            && !text.is_empty()
        {
            let content_w = (rect[2] - inner[0] - inner[2]).max(0.0);
            let content_h = (rect[3] - inner[1] - inner[3]).max(0.0);
            let cached = match &self.nodes[node].text_cache {
                Some((t, w, sc, layout)) if *t == text && *w == content_w && *sc == s => {
                    Some(layout.clone())
                }
                _ => None,
            };
            let layout = match cached {
                Some(l) => l,
                None => {
                    let l = fonts.layout(&text, text_params(&style), Some(content_w));
                    self.nodes[node].text_cache = Some((text.clone(), content_w, s, l.clone()));
                    l
                }
            };
            let dy = match style.vertical_align {
                crate::style::Align::Center => (content_h - layout.height) * 0.5,
                crate::style::Align::End => content_h - layout.height,
                _ => 0.0,
            };
            let ox = rect[0] + inner[0];
            let oy = rect[1] + inner[1] + dy;
            let placeholder = kind == Kind::TextEntry
                && self.nodes[node]
                    .attrs
                    .get("value")
                    .is_none_or(|v| v.is_empty());
            let ink = if placeholder {
                style.color.with_alpha(style.color.a() * 0.45)
            } else {
                style.color
            };
            let px_size = layout.size * s;
            let strike = if style.bold && !fonts.is_bold(layout.face) {
                (px_size * 0.035).max(1.0)
            } else {
                0.0
            };
            let mut emit = |dx: f32, dy: f32, color: [f32; 4], list: &mut DisplayList| {
                for g in &layout.glyphs {
                    let Some(slot) = fonts.glyph(layout.face, g.id, px_size) else {
                        continue;
                    };
                    // Snap the pen to whole pixels: glyphs rasterised at
                    // one sub-pixel offset and drawn at another go soft.
                    let x = ((ox + g.x) * s + dx).round() + slot.offset.0;
                    let y = ((oy + g.baseline) * s + dy).round() + slot.offset.1;
                    list.items.push(DrawItem::Quad(Quad {
                        rect: [x, y, slot.size.0, slot.size.1],
                        uv: slot.uv,
                        texture: TextureRef::Glyphs,
                        color,
                        color2: color,
                        transform,
                        additive,
                        ..Default::default()
                    }));
                }
            };
            // Bold asked for and no bold face loaded: overstrike, a pixel or
            // so to the right. A real bold face (`@font-face` with
            // `font-weight: bold`) is better, and is used when there is one.
            if let Some(shadow) = style.text_shadow {
                emit(
                    shadow.offset.0 * s,
                    shadow.offset.1 * s,
                    premul(shadow.color),
                    list,
                );
                if strike > 0.0 {
                    emit(
                        shadow.offset.0 * s + strike,
                        shadow.offset.1 * s,
                        premul(shadow.color),
                        list,
                    );
                }
            }
            emit(0.0, 0.0, premul(ink), list);
            if strike > 0.0 {
                emit(strike, 0.0, premul(ink), list);
            }

            // A caret after the text, blinking, while typing.
            if kind == Kind::TextEntry
                && self.focused == Some(node)
                && (self.time * 2.0).fract() < 0.6
            {
                let end = if placeholder {
                    0.0
                } else {
                    layout.glyphs.last().map_or(0.0, |_| layout.width)
                };
                list.items.push(DrawItem::Quad(Quad {
                    rect: [
                        (ox + end) * s + 1.0,
                        oy * s,
                        (1.5 * s).max(1.0),
                        layout.size * style.line_height * s,
                    ],
                    color: premul(style.color),
                    color2: premul(style.color),
                    transform,
                    ..Default::default()
                }));
            }
        }

        if self.debug {
            let c = [1.0, 0.2, 0.8, 0.8];
            list.items.push(DrawItem::Quad(Quad {
                rect: phys,
                color: [0.0; 4],
                color2: [0.0; 4],
                border_width: 1.0,
                border_color: c,
                transform,
                ..Default::default()
            }));
        }

        if let Some(inverse) = draw::invert(transform)
            && style.pointer_events
        {
            self.nodes[node].hit = Some((phys, inverse, *clip));
            self.draw_order.push(node);
        }

        let saved_clip = *clip;
        if style.overflow_hidden {
            let corners = [
                draw::apply(transform, phys[0], phys[1]),
                draw::apply(transform, phys[0] + phys[2], phys[1]),
                draw::apply(transform, phys[0], phys[1] + phys[3]),
                draw::apply(transform, phys[0] + phys[2], phys[1] + phys[3]),
            ];
            let x0 = corners
                .iter()
                .map(|c| c.0)
                .fold(f32::MAX, f32::min)
                .max(0.0);
            let y0 = corners
                .iter()
                .map(|c| c.1)
                .fold(f32::MAX, f32::min)
                .max(0.0);
            let x1 = corners.iter().map(|c| c.0).fold(f32::MIN, f32::max);
            let y1 = corners.iter().map(|c| c.1).fold(f32::MIN, f32::max);
            let mut r = [x0.floor(), y0.floor(), x1.ceil(), y1.ceil()];
            if let Some(p) = saved_clip {
                r = [
                    r[0].max(p[0] as f32),
                    r[1].max(p[1] as f32),
                    r[2].min((p[0] + p[2]) as f32),
                    r[3].min((p[1] + p[3]) as f32),
                ];
            }
            let new = [
                r[0] as u32,
                r[1] as u32,
                (r[2] - r[0]).max(0.0) as u32,
                (r[3] - r[1]).max(0.0) as u32,
            ];
            *clip = Some(new);
            list.items.push(DrawItem::Clip(Some(new)));
        }

        let mut children: Vec<NodeId> = self.nodes[node].children.clone();
        children.sort_by_key(|&c| self.nodes[c].shown.z_index);
        for c in children {
            self.draw_node(c, opacity, transform, clip, list, fonts, images);
        }

        if style.overflow_hidden {
            *clip = saved_clip;
            list.items.push(DrawItem::Clip(saved_clip));
        }
    }

    // ---- input ------------------------------------------------------------------

    /// The topmost interactive panel under a point, physical pixels.
    fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        for &node in self.draw_order.iter().rev() {
            let n = &self.nodes[node];
            let interactive =
                n.kind.focusable() || n.handlers.keys().any(|k| !k.starts_with("event:"));
            if !interactive || !n.alive {
                continue;
            }
            let Some((rect, inverse, clip)) = n.hit else {
                continue;
            };
            if let Some(c) = clip
                && (x < c[0] as f32
                    || y < c[1] as f32
                    || x >= (c[0] + c[2]) as f32
                    || y >= (c[1] + c[3]) as f32)
            {
                continue;
            }
            let (lx, ly) = draw::apply(inverse, x, y);
            if lx >= rect[0] && ly >= rect[1] && lx < rect[0] + rect[2] && ly < rect[1] + rect[3] {
                return Some(node);
            }
        }
        None
    }

    /// Whether any panel is under the point that would take a click.
    pub fn is_over_interactive(&self, x: f32, y: f32) -> bool {
        self.hit_test(x, y).is_some()
    }

    fn set_state(&mut self, node: Option<NodeId>, set: impl Fn(&mut State, bool)) {
        // Hover and active apply up the tree, as in CSS: hovering a button's
        // label hovers the button.
        let mut chain = BTreeSet::new();
        let mut p = node;
        while let Some(n) = p {
            chain.insert(n);
            p = self.nodes[n].parent;
        }
        for (i, n) in self.nodes.iter_mut().enumerate() {
            let before = n.state;
            set(&mut n.state, chain.contains(&i));
            if n.state != before {
                n.dirty = true;
            }
        }
    }

    fn set_focus(&mut self, node: Option<NodeId>) {
        if self.focused == node {
            return;
        }
        if let Some(old) = self.focused {
            self.nodes[old].state.focus = false;
            self.nodes[old].dirty = true;
            self.pending.push(Pending::Handler {
                node: old,
                attr: "onblur",
            });
        }
        self.focused = node;
        if let Some(new) = node {
            self.nodes[new].state.focus = true;
            self.nodes[new].dirty = true;
            self.pending.push(Pending::Handler {
                node: new,
                attr: "onfocus",
            });
        }
    }

    /// The pointer moved to `(x, y)`, physical pixels.
    pub fn pointer_move(&mut self, x: f32, y: f32, actions: &mut Vec<UiAction>) -> bool {
        let hit = self.hit_test(x, y);
        if hit != self.hovered {
            if let Some(old) = self.hovered {
                self.pending.push(Pending::Handler {
                    node: old,
                    attr: "onmouseout",
                });
            }
            if let Some(new) = hit {
                self.pending.push(Pending::Handler {
                    node: new,
                    attr: "onmouseover",
                });
            }
            self.hovered = hit;
            self.set_state(hit, |s, on| s.hover = on);
        }
        if let Some(p) = self.pressed
            && self.nodes[p].kind == Kind::Slider
        {
            self.drag_slider(p, x, actions);
        }
        hit.is_some()
    }

    /// A pointer button went down or up.
    pub fn pointer_button(
        &mut self,
        down: bool,
        x: f32,
        y: f32,
        actions: &mut Vec<UiAction>,
    ) -> bool {
        let hit = self.hit_test(x, y);
        if down {
            self.pressed = hit.filter(|&n| !self.nodes[n].disabled());
            self.set_state(self.pressed, |s, on| s.active = on);
            let focus = hit.filter(|&n| self.nodes[n].kind.focusable());
            self.set_focus(focus);
            if let Some(p) = self.pressed
                && self.nodes[p].kind == Kind::Slider
            {
                self.drag_slider(p, x, actions);
            }
            return hit.is_some();
        }
        let pressed = self.pressed.take();
        self.set_state(None, |s, _| s.active = false);
        if let (Some(p), Some(h)) = (pressed, hit)
            && p == h
        {
            self.activate(p, actions);
        }
        hit.is_some() || pressed.is_some()
    }

    fn drag_slider(&mut self, node: NodeId, x: f32, actions: &mut Vec<UiAction>) {
        let Some((rect, inverse, _)) = self.nodes[node].hit else {
            return;
        };
        let (lx, _) = draw::apply(inverse, x, rect[1]);
        let f = ((lx - rect[0]) / rect[2].max(1.0)).clamp(0.0, 1.0);
        let n = &self.nodes[node];
        let (min, max) = (n.number("min", 0.0), n.number("max", 1.0));
        let step = n.number("step", 0.0);
        let mut v = min + (max - min) * f;
        if step > 0.0 {
            v = min + ((v - min) / step).round() * step;
        }
        self.change_value(node, format_number(v), actions);
    }

    fn change_value(&mut self, node: NodeId, value: String, actions: &mut Vec<UiAction>) {
        let attr = if self.nodes[node].kind == Kind::Toggle {
            "checked"
        } else {
            "value"
        };
        if self.nodes[node].attrs.get(attr) == Some(&value) {
            return;
        }
        self.set_attr(node, attr, value.clone());
        if let Some(cvar) = self.nodes[node].attrs.get("cvar").cloned() {
            let value = match value.as_str() {
                "true" => "1".to_string(),
                "false" => "0".to_string(),
                _ => value,
            };
            actions.push(UiAction::SetCvar { name: cvar, value });
        }
        self.pending.push(Pending::Handler {
            node,
            attr: "onchange",
        });
    }

    fn activate(&mut self, node: NodeId, actions: &mut Vec<UiAction>) {
        if self.nodes[node].disabled() {
            return;
        }
        if self.nodes[node].kind == Kind::Toggle {
            let now = !self.nodes[node].checked();
            self.change_value(node, now.to_string(), actions);
        }
        if let Some(sound) = self.nodes[node].attrs.get("sound").cloned() {
            actions.push(UiAction::PlaySound(sound));
        }
        self.pending.push(Pending::Handler {
            node,
            attr: "onactivate",
        });
    }

    fn focusables(&self) -> Vec<NodeId> {
        self.draw_order
            .iter()
            .copied()
            .filter(|&n| self.nodes[n].kind.focusable() && !self.nodes[n].disabled())
            .collect()
    }

    /// A key, for focus navigation and text entry. `false` if nothing here
    /// wanted it.
    pub fn key(&mut self, key: UiKey, actions: &mut Vec<UiAction>) -> bool {
        let focusables = self.focusables();
        let step_focus = |doc: &mut Document, dir: isize| {
            if focusables.is_empty() {
                return;
            }
            let at = doc
                .focused
                .and_then(|f| focusables.iter().position(|&n| n == f));
            let next = match at {
                Some(i) => (i as isize + dir).rem_euclid(focusables.len() as isize) as usize,
                None if dir > 0 => 0,
                None => focusables.len() - 1,
            };
            let node = focusables[next];
            doc.set_focus(Some(node));
            doc.hovered = Some(node);
            doc.set_state(Some(node), |s, on| s.hover = on);
        };
        let focused = self.focused.filter(|&f| self.nodes[f].alive);
        let kind = focused.map(|f| self.nodes[f].kind);
        match key {
            UiKey::Tab | UiKey::Down => step_focus(self, 1),
            UiKey::BackTab | UiKey::Up => step_focus(self, -1),
            UiKey::Enter | UiKey::Space => {
                let Some(f) = focused else { return false };
                if kind == Some(Kind::TextEntry) {
                    if key == UiKey::Space {
                        return self.text(" ", actions);
                    }
                    self.pending.push(Pending::Handler {
                        node: f,
                        attr: "onsubmit",
                    });
                } else {
                    self.activate(f, actions);
                }
            }
            UiKey::Left | UiKey::Right => {
                let Some(f) = focused else { return false };
                if kind != Some(Kind::Slider) {
                    return kind == Some(Kind::TextEntry);
                }
                let n = &self.nodes[f];
                let (min, max) = (n.number("min", 0.0), n.number("max", 1.0));
                let step = match n.number("step", 0.0) {
                    s if s > 0.0 => s,
                    _ => (max - min) / 20.0,
                };
                let dir = if key == UiKey::Right { 1.0 } else { -1.0 };
                let v = (n.number("value", min) + step * dir).clamp(min.min(max), max.max(min));
                self.change_value(f, format_number(v), actions);
            }
            UiKey::Backspace => {
                let Some(f) = focused.filter(|_| kind == Some(Kind::TextEntry)) else {
                    return false;
                };
                let mut v = self.nodes[f]
                    .attrs
                    .get("value")
                    .cloned()
                    .unwrap_or_default();
                v.pop();
                self.change_value(f, v, actions);
            }
            UiKey::Escape => return false,
        }
        true
    }

    /// Typed text, for a focused text entry.
    pub fn text(&mut self, text: &str, actions: &mut Vec<UiAction>) -> bool {
        let Some(f) = self
            .focused
            .filter(|&f| self.nodes[f].alive && self.nodes[f].kind == Kind::TextEntry)
        else {
            return false;
        };
        let mut v = self.nodes[f]
            .attrs
            .get("value")
            .cloned()
            .unwrap_or_default();
        let limit = self.nodes[f].number("maxlength", 256.0) as usize;
        for c in text.chars().filter(|c| !c.is_control()) {
            if v.chars().count() < limit {
                v.push(c);
            }
        }
        self.change_value(f, v, actions);
        true
    }

    /// The panel with an `id`, for tests and tools.
    pub fn find(&self, id: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .position(|n| n.alive && n.id.as_deref() == Some(id))
    }

    /// The first live panel whose attribute `name` is `value`.
    pub fn find_by_attr(&self, name: &str, value: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .position(|n| n.alive && n.attrs.get(name).is_some_and(|v| v == value))
    }

    /// A panel's layout box in UI pixels: `x, y, w, h`.
    pub fn rect(&self, node: NodeId) -> [f32; 4] {
        self.nodes[node].rect
    }

    /// A panel's current classes, for tests and `ui_debug`.
    pub fn classes(&self, node: NodeId) -> Vec<String> {
        let n = &self.nodes[node];
        let mut all: Vec<String> = n.static_classes.clone();
        all.extend(n.attr_classes.iter().cloned());
        all.extend(n.toggled_classes.iter().cloned());
        all
    }

    /// A panel's current attribute.
    pub fn attr(&self, node: NodeId, name: &str) -> Option<&str> {
        self.nodes[node].attrs.get(name).map(String::as_str)
    }

    /// A panel's style as it is being drawn.
    pub fn shown_style(&self, node: NodeId) -> &Style {
        &self.nodes[node].shown
    }

    /// Whether the panel is being drawn at all.
    pub fn is_drawn(&self, node: NodeId) -> bool {
        self.nodes[node].hit.is_some() || self.draw_order.contains(&node)
    }

    /// The id of the focused panel.
    pub fn focused(&self) -> Option<NodeId> {
        self.focused
    }

    /// One-line description of what is under a point, for `ui_debug`.
    pub fn describe_at(&self, x: f32, y: f32) -> Option<String> {
        let node = self.draw_order.iter().rev().copied().find(|&n| {
            self.nodes[n].hit.is_some_and(|(r, inv, _)| {
                let (lx, ly) = draw::apply(inv, x, y);
                lx >= r[0] && ly >= r[1] && lx < r[0] + r[2] && ly < r[1] + r[3]
            })
        })?;
        let n = &self.nodes[node];
        Some(format!(
            "<{}{}> .{} [{:.0} {:.0} {:.0}x{:.0}]",
            n.tag,
            n.id.as_ref()
                .map(|i| format!(" id={i}"))
                .unwrap_or_default(),
            self.classes(node).join("."),
            n.rect[0],
            n.rect[1],
            n.rect[2],
            n.rect[3]
        ))
    }
}

/// Everything a document needs from its owner for one update.
pub struct Frame<'a> {
    pub dt: f32,
    pub viewport: (u32, u32),
    pub store: &'a UiStore,
    pub events: &'a [Event],
    pub fonts: &'a mut Fonts,
    pub images: &'a mut Images,
    pub loader: &'a dyn Loader,
    pub actions: &'a mut Vec<UiAction>,
    pub store_writes: &'a mut Vec<(String, Value)>,
}

fn format_number(v: f32) -> String {
    let rounded = (v * 1000.0).round() / 1000.0;
    Value::Float(f64::from(rounded)).to_string()
}

fn text_params(s: &Style) -> TextParams<'_> {
    TextParams {
        family: &s.font_family,
        bold: s.bold,
        size: s.font_size,
        letter_spacing: s.letter_spacing,
        line_height: s.line_height,
        wrap: s.wrap,
        align: s.text_align,
    }
}

fn find_keyframes<'a>(sheets: &'a [StyleSheet], name: &str) -> Option<&'a css::Keyframes> {
    sheets.iter().rev().find_map(|s| s.keyframes.get(name))
}

fn compound_matches(nodes: &[Node], node: NodeId, c: &Compound) -> bool {
    let n = &nodes[node];
    if let Some(tag) = &c.tag
        && *tag != n.tag
    {
        return false;
    }
    if let Some(id) = &c.id
        && n.id.as_ref() != Some(id)
    {
        return false;
    }
    if !c.classes.iter().all(|class| n.has_class(class)) {
        return false;
    }
    c.pseudo.iter().all(|p| match p {
        Pseudo::Hover => n.state.hover,
        Pseudo::Active => n.state.active,
        Pseudo::Focus => n.state.focus,
        Pseudo::Disabled => n.disabled(),
        Pseudo::Checked => n.checked(),
        Pseudo::FirstChild => {
            n.parent
                .and_then(|p| nodes[p].children.iter().find(|&&c| nodes[c].alive).copied())
                == Some(node)
        }
        Pseudo::LastChild => {
            n.parent.and_then(|p| {
                nodes[p]
                    .children
                    .iter()
                    .rev()
                    .find(|&&c| nodes[c].alive)
                    .copied()
            }) == Some(node)
        }
    })
}

fn selector_matches(
    nodes: &[Node],
    node: NodeId,
    subject: &Compound,
    ancestors: &[(Combinator, Compound)],
) -> bool {
    if !compound_matches(nodes, node, subject) {
        return false;
    }
    let Some(((combinator, next), rest)) = ancestors.split_first() else {
        return true;
    };
    let mut p = nodes[node].parent;
    match combinator {
        Combinator::Child => p.is_some_and(|p| selector_matches(nodes, p, next, rest)),
        Combinator::Descendant => {
            while let Some(a) = p {
                if selector_matches(nodes, a, next, rest) {
                    return true;
                }
                p = nodes[a].parent;
            }
            false
        }
    }
}

fn layout_differs(a: &Style, b: &Style) -> bool {
    a.display != b.display
        || a.position != b.position
        || a.flex_direction != b.flex_direction
        || a.flex_wrap != b.flex_wrap
        || a.justify_content != b.justify_content
        || a.align_items != b.align_items
        || a.align_self != b.align_self
        || a.flex_grow != b.flex_grow
        || a.flex_shrink != b.flex_shrink
        || a.flex_basis != b.flex_basis
        || a.width != b.width
        || a.height != b.height
        || a.min_width != b.min_width
        || a.min_height != b.min_height
        || a.max_width != b.max_width
        || a.max_height != b.max_height
        || a.margin != b.margin
        || a.padding != b.padding
        || a.inset != b.inset
        || a.gap != b.gap
        || a.border_width != b.border_width
}

fn dimension(d: Dim) -> taffy::Dimension {
    match d {
        Dim::Auto => taffy::Dimension::auto(),
        Dim::Px(v) => taffy::Dimension::length(v),
        Dim::Percent(p) => taffy::Dimension::percent(p),
    }
}

fn lpa(d: Dim) -> taffy::LengthPercentageAuto {
    match d {
        Dim::Auto => taffy::LengthPercentageAuto::auto(),
        Dim::Px(v) => taffy::LengthPercentageAuto::length(v),
        Dim::Percent(p) => taffy::LengthPercentageAuto::percent(p),
    }
}

fn lp(d: Dim) -> taffy::LengthPercentage {
    match d {
        Dim::Auto => taffy::LengthPercentage::length(0.0),
        Dim::Px(v) => taffy::LengthPercentage::length(v),
        Dim::Percent(p) => taffy::LengthPercentage::percent(p),
    }
}

fn align_items(a: crate::style::Align) -> taffy::AlignItems {
    use crate::style::Align;
    match a {
        Align::Start | Align::SpaceBetween | Align::SpaceAround | Align::SpaceEvenly => {
            taffy::AlignItems::FlexStart
        }
        Align::Center => taffy::AlignItems::Center,
        Align::End => taffy::AlignItems::FlexEnd,
        Align::Stretch => taffy::AlignItems::Stretch,
    }
}

fn justify(a: crate::style::Align) -> taffy::JustifyContent {
    use crate::style::Align;
    match a {
        Align::Start => taffy::JustifyContent::FlexStart,
        Align::Center => taffy::JustifyContent::Center,
        Align::End => taffy::JustifyContent::FlexEnd,
        Align::Stretch => taffy::JustifyContent::Stretch,
        Align::SpaceBetween => taffy::JustifyContent::SpaceBetween,
        Align::SpaceAround => taffy::JustifyContent::SpaceAround,
        Align::SpaceEvenly => taffy::JustifyContent::SpaceEvenly,
    }
}

fn to_taffy(s: &Style, _kind: Kind) -> taffy::Style {
    use crate::style::{FlexDirection, Position};
    let four = |d: [Dim; 4], f: fn(Dim) -> taffy::LengthPercentage| taffy::Rect {
        left: f(d[0]),
        top: f(d[1]),
        right: f(d[2]),
        bottom: f(d[3]),
    };
    let border = taffy::LengthPercentage::length(s.border_width);
    taffy::Style {
        display: if s.display {
            taffy::Display::Flex
        } else {
            taffy::Display::None
        },
        position: match s.position {
            Position::Relative => taffy::Position::Relative,
            Position::Absolute => taffy::Position::Absolute,
        },
        flex_direction: match s.flex_direction {
            FlexDirection::Row => taffy::FlexDirection::Row,
            FlexDirection::Column => taffy::FlexDirection::Column,
            FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
            FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
        },
        flex_wrap: if s.flex_wrap {
            taffy::FlexWrap::Wrap
        } else {
            taffy::FlexWrap::NoWrap
        },
        justify_content: Some(justify(s.justify_content)),
        align_items: Some(align_items(s.align_items)),
        align_self: s.align_self.map(align_items),
        align_content: Some(taffy::AlignContent::FlexStart),
        flex_grow: s.flex_grow,
        flex_shrink: s.flex_shrink,
        flex_basis: dimension(s.flex_basis),
        size: taffy::Size {
            width: dimension(s.width),
            height: dimension(s.height),
        },
        min_size: taffy::Size {
            width: dimension(s.min_width),
            height: dimension(s.min_height),
        },
        max_size: taffy::Size {
            width: dimension(s.max_width),
            height: dimension(s.max_height),
        },
        margin: taffy::Rect {
            left: lpa(s.margin[0]),
            top: lpa(s.margin[1]),
            right: lpa(s.margin[2]),
            bottom: lpa(s.margin[3]),
        },
        padding: four(s.padding, lp),
        border: taffy::Rect {
            left: border,
            top: border,
            right: border,
            bottom: border,
        },
        inset: taffy::Rect {
            left: lpa(s.inset[0]),
            top: lpa(s.inset[1]),
            right: lpa(s.inset[2]),
            bottom: lpa(s.inset[3]),
        },
        gap: taffy::Size {
            width: lp(s.gap.0),
            height: lp(s.gap.1),
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;
