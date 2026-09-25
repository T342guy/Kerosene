// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The UI system: every document the game has up, by layer.
//!
//! A **layer** is a named slot holding at most one document -- `hud`,
//! `overlay`, `menu`, or anything a game invents. Showing a layer loads its
//! file (or keeps the one already loaded, with its state); hiding it keeps the
//! document so showing it again is instant. Layers draw in `z` order, which a
//! layout sets on its `<root>`, and the topmost visible layer marked
//! `interactive` gets the pointer and keyboard; nothing below it does, which
//! is what makes a menu modal without any code saying so.
//!
//! A **world panel** is a document drawn into a texture in the level rather
//! than onto the screen -- a terminal, a keypad, a scoreboard on a wall. It is
//! the same kind of document with its own size, fed input by the engine when
//! the player aims at it.
//!
//! Hot reload lives here too: every second the files each document read are
//! read again and compared, and a document whose sources changed is loaded
//! fresh. Edit a stylesheet with the game running and the HUD changes a
//! second later.

use crate::document::{Document, Frame};
use crate::draw::DisplayList;
use crate::store::{UiStore, Value};
use crate::text::Fonts;
use crate::{Images, Loader, LogLevel, UiAction, UiInput};
use std::collections::{BTreeMap, BTreeSet};

/// How often hot reload looks at the files, in seconds.
pub const RELOAD_INTERVAL: f32 = 1.0;

struct Layer {
    name: String,
    path: String,
    doc: Option<Document>,
    visible: bool,
}

/// A document drawn into the world.
struct WorldPanel {
    path: String,
    size: (u32, u32),
    doc: Option<Document>,
    /// Bumped whenever the panel draws something different.
    revision: u64,
    pointer: Option<(f32, f32)>,
}

pub struct UiSystem {
    pub fonts: Fonts,
    pub images: Images,
    layers: Vec<Layer>,
    panels: BTreeMap<String, WorldPanel>,
    /// Outline every panel and report what is under the pointer.
    pub debug: bool,
    pub hot_reload: bool,
    reload_timer: f32,
    pointer: (f32, f32),
    /// Actions input produced, handed out by the next update.
    pending_actions: Vec<UiAction>,
    messages: Vec<(LogLevel, String)>,
    combined: DisplayList,
}

impl Default for UiSystem {
    fn default() -> Self {
        UiSystem::new()
    }
}

impl UiSystem {
    pub fn new() -> UiSystem {
        UiSystem {
            fonts: Fonts::new(),
            images: Images::default(),
            layers: Vec::new(),
            panels: BTreeMap::new(),
            debug: false,
            hot_reload: true,
            reload_timer: 0.0,
            pointer: (0.0, 0.0),
            pending_actions: Vec::new(),
            messages: Vec::new(),
            combined: DisplayList::default(),
        }
    }

    // ---- layers ------------------------------------------------------------

    /// Show a layer with a layout, loading it unless that file is already
    /// what the layer holds.
    pub fn show(&mut self, layer: &str, path: &str, loader: &dyn Loader) -> Result<(), String> {
        let index = match self.layers.iter().position(|l| l.name == layer) {
            Some(i) => i,
            None => {
                self.layers.push(Layer {
                    name: layer.to_string(),
                    path: String::new(),
                    doc: None,
                    visible: false,
                });
                self.layers.len() - 1
            }
        };
        let needs_load = self.layers[index].path != path || self.layers[index].doc.is_none();
        if needs_load {
            self.layers[index].path = path.to_string();
            let doc = Document::load(path, loader, &mut self.fonts);
            match doc {
                Ok(doc) => self.layers[index].doc = Some(doc),
                Err(e) => {
                    self.layers[index].doc = None;
                    self.layers[index].visible = false;
                    return Err(e);
                }
            }
        }
        self.layers[index].visible = true;
        self.sort_layers();
        Ok(())
    }

    /// Hide a layer, keeping its document for next time.
    pub fn hide(&mut self, layer: &str) {
        if let Some(l) = self.layers.iter_mut().find(|l| l.name == layer) {
            l.visible = false;
        }
    }

    /// Forget a layer entirely.
    pub fn remove(&mut self, layer: &str) {
        self.layers.retain(|l| l.name != layer);
    }

    pub fn is_visible(&self, layer: &str) -> bool {
        self.layers
            .iter()
            .any(|l| l.name == layer && l.visible && l.doc.is_some())
    }

    /// `(layer, file, visible)` for each layer.
    pub fn layers(&self) -> impl Iterator<Item = (&str, &str, bool)> {
        self.layers
            .iter()
            .map(|l| (l.name.as_str(), l.path.as_str(), l.visible))
    }

    /// A layer's document, for tests and tools.
    pub fn document(&self, layer: &str) -> Option<&Document> {
        self.layers.iter().find(|l| l.name == layer)?.doc.as_ref()
    }

    fn sort_layers(&mut self) {
        self.layers
            .sort_by_key(|l| l.doc.as_ref().map_or(0, |d| d.z));
    }

    /// Whether a visible layer wants the pointer and keyboard -- a menu is
    /// up, so the mouse should be free.
    pub fn wants_input(&self) -> bool {
        self.layers
            .iter()
            .any(|l| l.visible && l.doc.as_ref().is_some_and(|d| d.interactive))
    }

    // ---- world panels ----------------------------------------------------------

    /// Put a layout on a world panel of `size` pixels, loading it unless it
    /// is already there.
    pub fn set_panel(
        &mut self,
        name: &str,
        path: &str,
        size: (u32, u32),
        loader: &dyn Loader,
    ) -> Result<(), String> {
        if let Some(p) = self.panels.get_mut(name)
            && p.path == path
            && p.doc.is_some()
        {
            p.size = size;
            return Ok(());
        }
        let doc = Document::load(path, loader, &mut self.fonts)?;
        self.panels.insert(
            name.to_string(),
            WorldPanel {
                path: path.to_string(),
                size,
                doc: Some(doc),
                revision: 0,
                pointer: None,
            },
        );
        Ok(())
    }

    pub fn remove_panel(&mut self, name: &str) {
        self.panels.remove(name);
    }

    pub fn clear_panels(&mut self) {
        self.panels.clear();
    }

    /// Names of every world panel.
    pub fn panels(&self) -> impl Iterator<Item = &str> {
        self.panels.keys().map(String::as_str)
    }

    /// What a panel draws, and a number that changes whenever that does, so
    /// the renderer can skip redrawing a texture that would come out the
    /// same.
    pub fn panel_display(&self, name: &str) -> Option<(&DisplayList, u64)> {
        let p = self.panels.get(name)?;
        Some((p.doc.as_ref()?.display_list(), p.revision))
    }

    pub fn panel_document(&self, name: &str) -> Option<&Document> {
        self.panels.get(name)?.doc.as_ref()
    }

    /// Input for a world panel, in its own pixels. `PointerMove` with the
    /// pointer off the panel should be sent as [`UiSystem::panel_leave`].
    pub fn panel_input(&mut self, name: &str, input: UiInput) -> bool {
        let Some(p) = self.panels.get_mut(name) else {
            return false;
        };
        let Some(doc) = p.doc.as_mut() else {
            return false;
        };
        let actions = &mut self.pending_actions;
        match input {
            UiInput::PointerMove { x, y } => {
                p.pointer = Some((x, y));
                doc.pointer_move(x, y, actions)
            }
            UiInput::PointerButton { down } => match p.pointer {
                Some((x, y)) => doc.pointer_button(down, x, y, actions),
                None => false,
            },
            UiInput::Key(key) => doc.key(key, actions),
            UiInput::Text(text) => doc.text(&text, actions),
        }
    }

    /// The pointer left a world panel.
    pub fn panel_leave(&mut self, name: &str) {
        if let Some(p) = self.panels.get_mut(name)
            && p.pointer.take().is_some()
            && let Some(doc) = p.doc.as_mut()
        {
            doc.pointer_move(-1.0, -1.0, &mut self.pending_actions);
            doc.pointer_button(false, -1.0, -1.0, &mut self.pending_actions);
        }
    }

    // ---- input ----------------------------------------------------------------

    /// Screen input. `true` if a layer took it; the caller should then not
    /// also treat it as a game key.
    pub fn input(&mut self, input: UiInput) -> bool {
        let actions = &mut self.pending_actions;
        // The topmost visible interactive layer, and only it.
        let Some(doc) = self
            .layers
            .iter_mut()
            .rev()
            .filter(|l| l.visible)
            .find_map(|l| l.doc.as_mut().filter(|d| d.interactive))
        else {
            if let UiInput::PointerMove { x, y } = input {
                self.pointer = (x, y);
            }
            return false;
        };
        match input {
            UiInput::PointerMove { x, y } => {
                self.pointer = (x, y);
                doc.pointer_move(x, y, actions);
                // A modal layer owns the whole screen.
                true
            }
            UiInput::PointerButton { down } => {
                doc.pointer_button(down, self.pointer.0, self.pointer.1, actions);
                true
            }
            UiInput::Key(key) => doc.key(key, actions),
            UiInput::Text(text) => doc.text(&text, actions),
        }
    }

    /// What is under the pointer, for `ui_debug`.
    pub fn describe_pointer(&self) -> Option<String> {
        self.layers
            .iter()
            .rev()
            .filter(|l| l.visible)
            .find_map(|l| l.doc.as_ref()?.describe_at(self.pointer.0, self.pointer.1))
    }

    // ---- frame ----------------------------------------------------------------

    /// Run every visible document and world panel for a frame. Returns what
    /// the documents asked the engine to do.
    pub fn update(
        &mut self,
        dt: f32,
        viewport: (u32, u32),
        store: &mut UiStore,
        loader: &dyn Loader,
    ) -> Vec<UiAction> {
        if self.hot_reload {
            self.reload_timer += dt;
            if self.reload_timer >= RELOAD_INTERVAL {
                self.reload_timer = 0.0;
                self.reload_changed(loader);
            }
        }

        let events = store.take_events();
        let mut actions = std::mem::take(&mut self.pending_actions);
        let mut writes: Vec<(String, Value)> = Vec::new();

        let debug = self.debug;
        for layer in &mut self.layers {
            // Hidden layers still hear events, so a menu opened later shows
            // what happened while it was closed; they just skip the frame.
            let Some(doc) = layer.doc.as_mut() else {
                continue;
            };
            if !layer.visible {
                continue;
            }
            doc.debug = debug;
            let mut own = Vec::new();
            doc.update(&mut Frame {
                dt,
                viewport,
                store,
                events: &events,
                fonts: &mut self.fonts,
                images: &mut self.images,
                loader,
                actions: &mut own,
                store_writes: &mut writes,
            });
            tag_source(&mut own, &layer.name);
            actions.extend(own);
            self.messages.append(&mut doc.messages);
        }
        for (name, panel) in &mut self.panels {
            let Some(doc) = panel.doc.as_mut() else {
                continue;
            };
            let before = doc.display_list().clone();
            let mut own = Vec::new();
            doc.update(&mut Frame {
                dt,
                viewport: panel.size,
                store,
                events: &events,
                fonts: &mut self.fonts,
                images: &mut self.images,
                loader,
                actions: &mut own,
                store_writes: &mut writes,
            });
            if *doc.display_list() != before {
                panel.revision += 1;
            }
            tag_source(&mut own, &format!("panel:{name}"));
            actions.extend(own);
            self.messages.append(&mut doc.messages);
        }

        for (key, value) in writes {
            store.set(&key, value);
        }
        let mut out = Vec::new();
        for action in actions {
            match action {
                UiAction::ShowLayer { layer, path } => {
                    if let Err(e) = self.show(&layer, &path, loader) {
                        self.messages.push((LogLevel::Error, e));
                    }
                }
                UiAction::HideLayer(layer) => self.hide(&layer),
                UiAction::Emit { name, data, source } => {
                    // Every document hears it next frame, and the engine now.
                    store.emit(&name, data.clone());
                    out.push(UiAction::Emit { name, data, source });
                }
                other => out.push(other),
            }
        }

        let mut combined = DisplayList {
            items: Vec::new(),
            size: viewport,
        };
        for layer in &self.layers {
            if let (true, Some(doc)) = (layer.visible, &layer.doc) {
                combined.extend(doc.display_list());
            }
        }
        self.combined = combined;
        out
    }

    /// Everything the screen layers draw, back to front.
    pub fn display_list(&self) -> &DisplayList {
        &self.combined
    }

    /// Console lines the documents produced.
    pub fn take_messages(&mut self) -> Vec<(LogLevel, String)> {
        std::mem::take(&mut self.messages)
    }

    /// Store keys any document reads -- the engine publishes `cvar.*`
    /// from this rather than every convar it has.
    pub fn dependencies(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let docs = self
            .layers
            .iter()
            .filter_map(|l| l.doc.as_ref())
            .chain(self.panels.values().filter_map(|p| p.doc.as_ref()));
        for doc in docs {
            out.extend(doc.dependencies().map(str::to_string));
        }
        out
    }

    /// Load every document again from its file.
    pub fn reload_all(&mut self, loader: &dyn Loader) {
        for i in 0..self.layers.len() {
            self.reload_layer(i, loader);
        }
        let names: Vec<String> = self.panels.keys().cloned().collect();
        for name in names {
            self.reload_panel(&name, loader);
        }
    }

    fn reload_changed(&mut self, loader: &dyn Loader) {
        let stale = |doc: &Document| {
            doc.sources.iter().any(|(path, hash)| {
                loader.read(path).map(|b| crate::document::content_hash(&b)) != Some(*hash)
            })
        };
        for i in 0..self.layers.len() {
            if self.layers[i].doc.as_ref().is_some_and(stale) {
                self.reload_layer(i, loader);
            }
        }
        let names: Vec<String> = self
            .panels
            .iter()
            .filter(|(_, p)| p.doc.as_ref().is_some_and(stale))
            .map(|(n, _)| n.clone())
            .collect();
        for name in names {
            self.reload_panel(&name, loader);
        }
    }

    fn reload_layer(&mut self, i: usize, loader: &dyn Loader) {
        let path = self.layers[i].path.clone();
        match Document::load(&path, loader, &mut self.fonts) {
            Ok(doc) => {
                self.messages
                    .push((LogLevel::Info, format!("reloaded {path}")));
                self.layers[i].doc = Some(doc);
            }
            // Keep showing the old one: a half-typed edit should not blank
            // the HUD, only say what is wrong with it.
            Err(e) => self.messages.push((LogLevel::Error, e)),
        }
        self.sort_layers();
    }

    fn reload_panel(&mut self, name: &str, loader: &dyn Loader) {
        let Some(p) = self.panels.get_mut(name) else {
            return;
        };
        match Document::load(&p.path, loader, &mut self.fonts) {
            Ok(doc) => {
                self.messages
                    .push((LogLevel::Info, format!("reloaded {}", p.path)));
                p.doc = Some(doc);
                p.revision += 1;
            }
            Err(e) => self.messages.push((LogLevel::Error, e)),
        }
    }
}

fn tag_source(actions: &mut [UiAction], source: &str) {
    for a in actions {
        if let UiAction::Emit { source: s, .. } = a {
            *s = source.to_string();
        }
    }
}

#[cfg(test)]
mod tests;
