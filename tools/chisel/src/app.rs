// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The editor's user interface.
//!
//! Hammer's layout, because it is the right one for the job: a strip of tool
//! icons down the left, a toolbar row under the menu, a tabbed inspector on
//! the right, a status bar along the bottom, and four viewports filling the
//! middle -- 3D, top, front and side.
//!
//! Everything here turns a gesture into a call on [`Document`] and draws the
//! result. The decisions all live in the modules it calls.

use crate::compile::{CompileJob, CompileMessage, CompileSettings, Quality, available_tools};
use crate::document::Document;
use crate::inspector::{self, PropertyRow};
use crate::raster::Shading;
use crate::textures::TextureCache;
use crate::tools::{TextureMode, TextureTarget, Tool, ToolKind};
use crate::viewport::Viewport;
use crate::{classes, draw, files, raster};
use egui::{Context, Key, Modifiers, RichText};
use kerosene_entity::{ClassKind, KeyKind, Schema};
use kerosene_map::{Connection, WalkmapRule};
use kerosene_math::Vec3;
use std::path::PathBuf;

mod browser;
mod compile;
mod dialogs;
mod menu;
mod panel;
mod properties;
mod status;
mod toolbar;
mod viewports;
mod widgets;

use widgets::*;

/// Entity classes offered if the built-in schema ever fails to load.
///
/// The built-in set is compiled into the game crate, so this should be
/// unreachable; it is a last resort so the entity tool is not simply broken
/// if the embedded schema is somehow invalid.
const FALLBACK_CLASSES: &[&str] = &["info_player_start", "light", "logic_relay"];

/// How fast the 3D camera flies by default, in kerosene units per second.
///
/// A shade above a player's running speed, so moving through a level in the
/// editor feels like the pace it will be played at. Shift doubles it, Alt
/// halves it, and the wheel changes it while flying.
const DEFAULT_FLY_SPEED: f32 = 384.0;

/// The width of the bars between panes, in points.
const SPLITTER: f32 = 5.0;

/// How many pixels of texture a material swatch is built from.
///
/// Larger than any swatch is drawn, so one is always scaled down. egui filters
/// on the way down and not on the way up, and a swatch scaled up from a mip
/// smaller than itself is a blur.
const THUMBNAIL: u32 = 128;

/// Materials offered in the material picker when the content tree cannot be
/// scanned.
const FALLBACK_MATERIALS: &[&str] = &[
    "dev/grid",
    "dev/wall",
    "dev/door",
    "tools/nodraw",
    "tools/clip",
    "tools/trigger",
    "tools/hint",
    "tools/skip",
    "tools/skybox",
];

/// The egui key a tool's advertised shortcut stands for.
///
/// Here rather than on `ToolKind` so the tool definitions stay free of the UI
/// toolkit: what a tool *is* should not depend on what draws it.
fn shortcut_key(shortcut: &str) -> Option<Key> {
    Some(match shortcut {
        "1" => Key::Num1,
        "2" => Key::Num2,
        "3" => Key::Num3,
        "4" => Key::Num4,
        "5" => Key::Num5,
        "6" => Key::Num6,
        _ => return None,
    })
}

/// What the asset browser is being used to pick.
#[derive(Clone, Debug, PartialEq)]
pub enum Browsing {
    /// The material new brushes wear, and what a click paints onto the
    /// selection.
    Material,
    /// A model. Carries the property row the pick should go back to, and the
    /// value already there so the browser can show what is chosen. `None` is
    /// a browse with nowhere to put the answer -- opened from the menu, to
    /// see what a project has in it.
    Model { row: Option<usize>, current: String },
}

/// A file operation waiting for a name.
///
/// Naming a map is a decision, so it gets a field to type into rather than a
/// menu item with the answer baked in. "save as maps/untitled.keromap" was
/// the whole of the editor's file handling, and it is not a way to name
/// anything: the second map you make overwrites the first.
pub struct NamePrompt {
    pub kind: PromptKind,
    /// What has been typed. A bare name means a map in this project; see
    /// [`crate::files::resolve`].
    pub name: String,
    /// Why the last attempt did not go through.
    pub error: Option<String>,
    /// Set on the frame it opens, so the field can take the keyboard.
    fresh: bool,
}

/// What a [`NamePrompt`] will do with the name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromptKind {
    /// Write the map under a new name, leaving any existing file alone.
    SaveAs,
    /// Move the map, and the artefacts compiled from it, to a new name.
    Rename,
}

impl PromptKind {
    pub fn title(self) -> &'static str {
        match self {
            PromptKind::SaveAs => "save as",
            PromptKind::Rename => "rename",
        }
    }

    pub fn verb(self) -> &'static str {
        match self {
            PromptKind::SaveAs => "save",
            PromptKind::Rename => "rename",
        }
    }
}

/// What someone chose when told a change would be lost.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Decision {
    Save,
    Discard,
    Cancel,
}

/// Something that would discard unsaved work, held until it is confirmed.
#[derive(Clone, Debug, PartialEq)]
pub enum Discarding {
    /// Start again on an empty map.
    New,
    /// Open a map from disk.
    Open(PathBuf),
    /// Close the editor.
    Quit,
}

/// The inspector's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InspectorTab {
    /// What is selected: an entity's keys, a brush's type, a face's texture.
    #[default]
    Object,
    /// The current tool's settings: entity classes, shape sliders.
    Tool,
    /// The material browser, docked.
    Materials,
}

impl InspectorTab {
    fn from_index(index: usize) -> InspectorTab {
        match index {
            1 => InspectorTab::Tool,
            2 => InspectorTab::Materials,
            _ => InspectorTab::Object,
        }
    }
}

pub struct ChiselApp {
    pub document: Document,
    pub tool: Tool,
    pub viewports: [Viewport; 4],
    /// Which pane the pointer last acted in.
    pub active: usize,
    /// Show one pane full size instead of four.
    pub maximised: Option<usize>,
    pub compile: Option<CompileJob>,
    pub compile_settings: CompileSettings,
    pub show_compile: bool,
    pub show_tools_check: bool,
    pub status: String,
    pub materials: Vec<String>,
    pub models: Vec<String>,
    pub content_root: PathBuf,
    /// The game's entity class definitions, read from the content tree.
    pub schema: Schema,
    /// Textures for the 3D pane and the material browser.
    pub textures: TextureCache,
    /// Thumbnails handed to egui, one per material, built on demand.
    thumbnails: std::collections::HashMap<String, egui::TextureHandle>,
    /// The content tree, for reading textures the way the engine does.
    pub vfs: kerosene_vfs::Vfs,
    /// How the 3D panes draw.
    pub shading: Shading,
    /// Substring filter on the entity class list.
    pub entity_filter: String,
    /// Which of the inspector's tabs is showing.
    pub inspector_tab: InspectorTab,
    /// The selection as the inspector last saw it -- solids, entities, faces
    /// -- so it can notice a fresh selection and show the Object tab.
    inspector_seen: (usize, usize, usize),
    /// The tool as the inspector last saw it, for the same reason.
    inspector_tool: ToolKind,
    /// Where the pointer is in the world, and in which pane, when it is over
    /// a flat view. The status bar shows it.
    pub pointer_world: Option<(usize, Vec3)>,
    /// Where the content tree came from, for the status bar to show. An
    /// editor with no content is nearly useless, so how it decided is worth
    /// having in front of you rather than in a log nobody reads.
    pub content_note: String,
    /// The route out of a leaking map, from the last compile.
    pub leak: crate::leak::LeakTrace,
    /// Where the four panes divide, as fractions of the area. Dragged.
    pub split: egui::Vec2,
    /// How fast the 3D camera flies, in kerosene units per second.
    pub fly_speed: f32,
    /// The rasterised 3D panes, kept until something they depend on moves.
    previews: [Option<Preview>; 4],
    /// The asset browser, and what it is picking for.
    pub browsing: Option<Browsing>,
    /// What has been typed into the browser's search box.
    pub browse_filter: String,
    /// How big the browser draws its swatches, in points.
    pub browse_size: f32,
    /// Rendered model previews, one per model, built on demand.
    model_previews: std::collections::HashMap<String, egui::TextureHandle>,
    /// A save-as or rename waiting for a name.
    pub prompt: Option<NamePrompt>,
    /// Something that would throw away unsaved work, waiting to be confirmed.
    pub discarding: Option<Discarding>,
    /// Set once a close has been agreed to; the host polls it.
    quit: bool,
    /// In-progress property edits, held until the field is done with.
    ///
    /// Committing on every keystroke would push a whole undo snapshot per
    /// character typed, so a field is edited in a buffer and written back when
    /// it loses focus or the selection moves on.
    properties: Option<PropertyEdit>,
    /// The Hammer-style "Object Properties" popup, opened by right-clicking an
    /// object. It is a separate buffer from the docked inspector because it
    /// also edits the world: a plain brush has no brush entity to hang settings
    /// on, but it can still carry keyvalues like `playercollision`.
    property_window: Option<PropertyWindow>,
}

/// A rendered 3D pane and the state it was rendered from.
struct Preview {
    texture: egui::TextureHandle,
    key: u64,
}

struct PropertyEdit {
    entity: u32,
    rows: Vec<PropertyRow>,
    /// The entity's outputs, buffered for the same reason the rows are.
    ///
    /// Rebuilding these from the document every frame is what made the output
    /// fields impossible to type into: each keystroke landed in a temporary
    /// that was thrown away and re-cloned before the next frame drew, so the
    /// caret moved and the text never changed.
    connections: Vec<Connection>,
    dirty: bool,
    /// The document revision these rows were read from, so an undo or an edit
    /// made elsewhere refreshes them instead of being overwritten by a stale
    /// buffer.
    revision: u64,
}

/// The popup form of the property editor: a grid of key/value rows, almost
/// exactly Hammer's "Object Properties" dialog. Every key the class reads is
/// shown whether or not it is set, custom keys are renamed in place, and a new
/// key can be added to any brush or entity from the row at the bottom.
struct PropertyWindow {
    /// The entity being edited. For a world brush this is `worldspawn`.
    entity: u32,
    rows: Vec<PropertyRow>,
    dirty: bool,
    /// Document revision the rows were read from, for the same reason the
    /// docked inspector tracks one.
    revision: u64,
    /// The entity's outputs, buffered for the same reason the rows are.
    connections: Vec<Connection>,
    /// The two halves of the add-a-key row at the bottom of the grid.
    new_key: String,
    new_value: String,
    /// Narrowest value field drawn last frame. Layout only, and only so a
    /// test can see a collapse that no assertion about the map would notice.
    #[cfg(test)]
    narrowest_value: f32,
}

impl ChiselApp {
    pub fn new(content_root: PathBuf) -> ChiselApp {
        let materials = scan_materials(&content_root);
        let models = scan_models(&content_root);
        let loaded = classes::load(&content_root);
        let status = loaded.summary();
        let mut vfs = kerosene_vfs::Vfs::new();
        vfs.add_directory(&content_root, "GAME");
        // Archives too, so a packed content tree previews like a loose one.
        for archive in std::fs::read_dir(&content_root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "vault"))
        {
            let _ = vfs.mount_archive(&archive, "GAME");
        }
        ChiselApp {
            document: Document::new(),
            tool: Tool::new(),
            viewports: Viewport::default_layout(),
            active: 1,
            maximised: None,
            compile: None,
            compile_settings: CompileSettings::default(),
            show_compile: false,
            show_tools_check: false,
            status,
            materials,
            models,
            content_root,
            schema: loaded.schema,
            textures: TextureCache::new(),
            thumbnails: std::collections::HashMap::new(),
            vfs,
            shading: Shading::default(),
            entity_filter: String::new(),
            inspector_tab: InspectorTab::Object,
            inspector_seen: (0, 0, 0),
            inspector_tool: ToolKind::Select,
            pointer_world: None,
            content_note: String::new(),
            leak: crate::leak::LeakTrace::default(),
            split: egui::vec2(0.5, 0.5),
            fly_speed: DEFAULT_FLY_SPEED,
            previews: [const { None }; 4],
            browsing: None,
            browse_filter: String::new(),
            browse_size: 96.0,
            model_previews: std::collections::HashMap::new(),
            prompt: None,
            discarding: None,
            quit: false,
            properties: None,
            property_window: None,
        }
    }

    /// Class names for the entity tool, from the game's definitions.
    pub fn point_classes(&self) -> Vec<String> {
        let names = self.schema.names_of_kind(ClassKind::Point);
        if names.is_empty() {
            return FALLBACK_CLASSES.iter().map(|s| s.to_string()).collect();
        }
        // `worldspawn` is `kind any` because its brushes are the world. It is
        // not something anyone places.
        names
            .into_iter()
            .filter(|n| *n != "worldspawn")
            .map(str::to_string)
            .collect()
    }

    /// Classes that brushes can be tied to.
    pub fn brush_classes(&self) -> Vec<String> {
        let names = self.schema.names_of_kind(ClassKind::Brush);
        if names.is_empty() {
            return vec![
                "func_detail".into(),
                "func_brush".into(),
                "trigger_multiple".into(),
            ];
        }
        names
            .into_iter()
            .filter(|n| *n != "worldspawn")
            .map(str::to_string)
            .collect()
    }

    pub fn open(&mut self, path: PathBuf) {
        match Document::open(path.clone()) {
            Ok(document) => {
                self.document = document;
                self.status = format!("opened {}", path.display());
                self.frame_all();
            }
            Err(e) => self.status = format!("could not open {}: {e}", path.display()),
        }
    }

    /// Save, asking for a name first if the map has never had one.
    ///
    /// A new document has no path, and the old behaviour was to put "no path
    /// to save to" in the status bar and stop. That reads as ctrl-S doing
    /// nothing at all, which is precisely what it was doing.
    ///
    /// Returns whether the map is now on disk. A save that turned into a
    /// name prompt, or failed, is `false`, and a caller about to throw the
    /// document away on the strength of it must not.
    pub fn save(&mut self, path: Option<PathBuf>) -> bool {
        self.commit_properties();
        self.commit_property_window();
        if path.is_none() && self.document.path.is_none() {
            self.begin_prompt(PromptKind::SaveAs);
            return false;
        }
        match self.document.save(path) {
            Ok(path) => {
                self.status = format!("saved {}", files::label(&path, &self.content_root));
                true
            }
            Err(e) => {
                self.status = format!("could not save: {e}");
                false
            }
        }
    }

    /// Open the name field, filled in with the map's current name.
    pub fn begin_prompt(&mut self, kind: PromptKind) {
        let name = match self.document.path.as_deref() {
            Some(path) => {
                // Without the `maps/` prefix: it is where every map goes, so
                // showing it invites deleting it, and a name typed without it
                // means the same thing anyway.
                let label = files::label(path, &self.content_root);
                label.strip_prefix("maps/").unwrap_or(&label).to_string()
            }
            None => "untitled".to_string(),
        };
        self.prompt = Some(NamePrompt {
            kind,
            name,
            error: None,
            fresh: true,
        });
    }

    /// Act on the name that was typed, or say why it will not do.
    ///
    /// Returns whether the prompt is finished with.
    pub fn confirm_prompt(&mut self) -> bool {
        let Some(prompt) = &self.prompt else {
            return true;
        };
        let (kind, typed) = (prompt.kind, prompt.name.clone());

        let target = match files::resolve(&typed, &self.content_root) {
            Ok(target) => target,
            Err(e) => {
                if let Some(prompt) = &mut self.prompt {
                    prompt.error = Some(e)
                }
                return false;
            }
        };

        let outcome = match kind {
            PromptKind::SaveAs => self.save_as(target),
            PromptKind::Rename => self.rename_to(target),
        };
        match outcome {
            Ok(said) => {
                self.status = said;
                self.prompt = None;
                true
            }
            Err(e) => {
                if let Some(prompt) = &mut self.prompt {
                    prompt.error = Some(e)
                }
                false
            }
        }
    }

    /// Write the map under a new name.
    fn save_as(&mut self, target: PathBuf) -> Result<String, String> {
        self.commit_properties();
        match self.document.save(Some(target.clone())) {
            Ok(path) => Ok(format!("saved {}", files::label(&path, &self.content_root))),
            Err(e) => Err(format!("could not save: {e}")),
        }
    }

    /// Move the map, and anything compiled from it, to a new name.
    ///
    /// A map that has never been saved has nothing on disk to move, so this
    /// is a save under the new name -- which is what someone renaming an
    /// untitled map means by it.
    fn rename_to(&mut self, target: PathBuf) -> Result<String, String> {
        self.commit_properties();
        let Some(from) = self.document.path.clone() else {
            return self.save_as(target);
        };
        if from == target {
            return Ok(format!(
                "{} is already its name",
                files::label(&target, &self.content_root)
            ));
        }
        if target.exists() {
            return Err(format!(
                "{} already exists",
                files::label(&target, &self.content_root)
            ));
        }
        if !from.exists() {
            return self.save_as(target);
        }

        let moved =
            files::move_map(&from, &target).map_err(|e| format!("could not rename: {e}"))?;
        self.document.path = Some(target.clone());

        // Written out afterwards, so the file under the new name is the map
        // as it stands rather than as it was when it was last saved.
        if self.document.is_modified()
            && let Err(e) = self.document.save(None)
        {
            return Err(format!("renamed, but could not save: {e}"));
        }

        let name = files::label(&target, &self.content_root);
        Ok(match moved.len() {
            0 => format!("renamed to {name}"),
            n => format!("renamed to {name} ({n} compiled files moved with it)"),
        })
    }

    /// Do something that would throw away unsaved work, or ask first.
    fn discard_or_ask(&mut self, what: Discarding) {
        if self.document.is_modified() {
            self.discarding = Some(what);
        } else {
            self.discard_now(what);
        }
    }

    fn discard_now(&mut self, what: Discarding) {
        match what {
            Discarding::New => {
                self.document = Document::new();
                self.status = "new map".into();
            }
            Discarding::Open(path) => self.open(path),
            Discarding::Quit => self.quit = true,
        }
    }

    /// The window is being closed. `true` if it may close now; otherwise the
    /// unsaved-changes question goes up and [`ChiselApp::wants_to_quit`]
    /// carries the answer.
    pub fn request_close(&mut self) -> bool {
        if !self.document.is_modified() {
            return true;
        }
        self.discarding = Some(Discarding::Quit);
        false
    }

    /// Whether a close the editor was asked about has been agreed to.
    pub fn wants_to_quit(&self) -> bool {
        self.quit
    }

    /// What the window should be called: the map, and whether it is saved.
    ///
    /// The title bar is the one place the name is visible without opening a
    /// menu, and the `*` is the only thing in the window that answers "have I
    /// saved this" from across the room.
    pub fn window_title(&self) -> String {
        format!("{} -- Chisel", self.document.title())
    }

    /// Frame everything, or the selection if there is one.
    fn frame_all(&mut self) {
        let bounds = draw::framing_bounds(&self.document);
        for viewport in &mut self.viewports {
            viewport.focus_on(bounds);
        }
    }

    // ---- the frame -------------------------------------------------------

    pub fn ui(&mut self, ctx: &Context) {
        if let Some(job) = &mut self.compile {
            let was_finished = job.finished;
            job.poll();
            if job.finished && !was_finished {
                self.after_compile();
            }
            ctx.request_repaint();
        }

        self.shortcuts(ctx);
        self.menu_bar(ctx);
        self.toolbar(ctx);
        self.status_bar(ctx);
        self.tool_strip(ctx);
        self.inspector(ctx);
        self.compile_window(ctx);
        self.browser_window(ctx);
        self.file_windows(ctx);
        self.property_window_ui(ctx);
        self.viewports_panel(ctx);
    }

    fn shortcuts(&mut self, ctx: &Context) {
        // A modal takes the keyboard whole. Otherwise ctrl-S while the "save
        // as" field is open would save the map under the name the dialog is
        // asking you to replace, and escape would both close the dialog and
        // clear the selection behind it.
        if self.prompt.is_some() || self.discarding.is_some() {
            return;
        }

        // A property field has to be able to contain the characters these
        // shortcuts use. Without this guard, typing `1` into a keyvalue
        // switches tools and typing `[` changes the grid size -- which is
        // exactly the kind of thing that makes an editor feel haunted.
        let typing = ctx.wants_keyboard_input();

        enum Action {
            Undo,
            Redo,
            Save,
            SaveAs,
            New,
            SelectAll,
            Duplicate,
            Browse,
            Delete,
            Cancel,
            Properties,
            Tool(ToolKind),
            Finer,
            Coarser,
            Compile,
            CycleTextureMode,
            Maximise,
        }

        let mut actions = Vec::new();
        ctx.input_mut(|i| {
            let ctrl = Modifiers::COMMAND;

            // Chorded shortcuts stay live while typing: ctrl-S must save
            // whatever the focus is.
            if i.consume_key(ctrl, Key::Z) && !typing {
                actions.push(Action::Undo)
            }
            if (i.consume_key(ctrl | Modifiers::SHIFT, Key::Z) || i.consume_key(ctrl, Key::Y))
                && !typing
            {
                actions.push(Action::Redo)
            }
            // The shifted chord is read first because it has to be. egui
            // matches modifiers loosely -- a pattern of ctrl-S also matches a
            // press of ctrl-shift-S -- so plain save would fire as well, and
            // the map would be written under its old name behind the dialog
            // asking for a new one. Consuming the shifted form first takes the
            // event out of the queue before the looser pattern sees it.
            if i.consume_key(ctrl | Modifiers::SHIFT, Key::S) {
                actions.push(Action::SaveAs)
            }
            if i.consume_key(ctrl, Key::S) {
                actions.push(Action::Save)
            }
            if i.consume_key(ctrl, Key::N) && !typing {
                actions.push(Action::New)
            }
            if i.consume_key(ctrl, Key::A) && !typing {
                actions.push(Action::SelectAll)
            }
            if i.consume_key(ctrl, Key::D) && !typing {
                actions.push(Action::Duplicate)
            }

            if typing {
                return;
            }

            if i.consume_key(Modifiers::NONE, Key::Delete)
                || i.consume_key(Modifiers::NONE, Key::Backspace)
            {
                actions.push(Action::Delete)
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                actions.push(Action::Cancel)
            }
            if i.consume_key(Modifiers::ALT, Key::Enter) && !typing {
                actions.push(Action::Properties)
            }

            // Tool shortcuts, as Hammer numbers them. Driven by the number
            // each tool advertises rather than by its position in the list,
            // so adding a tool in the middle cannot silently renumber the
            // ones after it -- which is exactly what happened when the shape
            // tool went in between block and entity.
            for kind in ToolKind::all() {
                let Some(key) = shortcut_key(kind.shortcut()) else {
                    continue;
                };
                if i.consume_key(Modifiers::NONE, key) {
                    actions.push(Action::Tool(kind))
                }
            }

            // The grid keys every brush editor has used for thirty years.
            if i.consume_key(Modifiers::NONE, Key::OpenBracket) {
                actions.push(Action::Finer)
            }
            if i.consume_key(Modifiers::NONE, Key::CloseBracket) {
                actions.push(Action::Coarser)
            }
            if i.consume_key(Modifiers::NONE, Key::F9) {
                actions.push(Action::Compile)
            }
            if i.consume_key(Modifiers::NONE, Key::M) {
                actions.push(Action::Browse)
            }
            if i.consume_key(Modifiers::NONE, Key::T) {
                actions.push(Action::CycleTextureMode)
            }
            if i.consume_key(Modifiers::SHIFT, Key::Space) {
                actions.push(Action::Maximise)
            }
        });

        for action in actions {
            match action {
                // Anything half-typed becomes its own undo step first, so
                // ctrl-Z takes back the edit rather than the one before it.
                Action::Undo => {
                    self.commit_properties();
                    self.commit_property_window();
                    if let Some(label) = self.document.undo() {
                        self.status = format!("undid {label}");
                    }
                }
                Action::Redo => {
                    self.commit_properties();
                    self.commit_property_window();
                    if let Some(label) = self.document.redo() {
                        self.status = format!("redid {label}");
                    }
                }
                Action::Save => {
                    self.save(None);
                }
                Action::SaveAs => self.begin_prompt(PromptKind::SaveAs),
                Action::New => self.discard_or_ask(Discarding::New),
                Action::SelectAll => {
                    let n = self.document.select_all();
                    self.status = format!("selected {n}");
                }
                Action::Duplicate => {
                    // Offset by one grid step so the copy is visibly a copy
                    // rather than a second brush hidden inside the first.
                    let step = self.document.grid.size;
                    let n = self
                        .document
                        .duplicate_selection(Vec3::new(step, step, 0.0));
                    if n > 0 {
                        self.status = format!("duplicated {n}; drag to place");
                    }
                }
                Action::Browse => self.browsing = Some(Browsing::Material),
                Action::Delete => {
                    let n = self.document.delete_selection();
                    if n > 0 {
                        self.status = format!("deleted {n}")
                    }
                }
                Action::Cancel => {
                    self.tool.cancel();
                    self.document.selection.clear();
                }
                Action::Properties => self.open_property_window(),
                Action::Tool(kind) => self.tool.set_kind(kind),
                Action::Finer => self.document.grid.finer(),
                Action::Coarser => self.document.grid.coarser(),
                Action::Compile => self.compile_now(Quality::Fast),
                Action::Maximise => self.toggle_maximised(),
                Action::CycleTextureMode => {
                    self.tool.texture_mode = self.tool.texture_mode.next();
                    self.status = format!("texture tool: {}", self.tool.texture_mode.label());
                }
            }
        }
    }

}
/// Models offered where a key holds one.
fn scan_models(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let models = root.join("models");
    collect_by_extension(&models, &models, "keromdl", &mut out);
    out.sort();
    out.dedup();
    out
}

/// Find the materials in a content tree.
///
/// Falls back to a built-in list when there is nothing to scan, so the editor
/// is usable before any content exists.
fn scan_materials(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let materials = root.join("materials");
    collect_by_extension(&materials, &materials, "keromat", &mut out);
    out.sort();
    out.dedup();
    if out.is_empty() {
        out = FALLBACK_MATERIALS.iter().map(|s| s.to_string()).collect();
    }
    out
}

fn collect_by_extension(
    root: &std::path::Path,
    dir: &std::path::Path,
    extension: &str,
    out: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_by_extension(root, &path, extension, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(extension)
            && let Ok(relative) = path.strip_prefix(root)
        {
            let name = relative.with_extension("");
            out.push(name.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// A starter map, so a fresh editor has something to look at rather than an
/// empty void with no sense of scale.
pub fn starter_document() -> Document {
    use kerosene_math::Aabb;
    let mut document = Document::new();
    let t = 16.0;
    let (lo, hi, tall) = (0.0f32, 512.0f32, 256.0f32);
    for slab in [
        Aabb::new(
            Vec3::new(lo - t, lo - t, lo - t),
            Vec3::new(hi + t, hi + t, lo),
        ),
        Aabb::new(
            Vec3::new(lo - t, lo - t, tall),
            Vec3::new(hi + t, hi + t, tall + t),
        ),
        Aabb::new(Vec3::new(lo - t, lo - t, lo), Vec3::new(lo, hi + t, tall)),
        Aabb::new(Vec3::new(hi, lo - t, lo), Vec3::new(hi + t, hi + t, tall)),
        Aabb::new(Vec3::new(lo, lo - t, lo), Vec3::new(hi, lo, tall)),
        Aabb::new(Vec3::new(lo, hi, lo), Vec3::new(hi, hi + t, tall)),
    ] {
        document.create_block(slab.min, slab.max);
    }
    document.create_entity("info_player_start", Vec3::new(64.0, 256.0, 16.0));
    document.create_entity("light", Vec3::new(256.0, 256.0, 192.0));
    document.selection.clear();
    document.mark_clean();
    document
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::ViewportKind;

    #[test]
    fn the_starter_map_is_a_sealed_room_that_compiles() {
        let document = starter_document();
        assert!(document.problems().is_empty(), "{:?}", document.problems());
        assert_eq!(document.map.world.solids.len(), 6);
        assert!(document.map.by_classname("info_player_start").count() == 1);
        assert!(document.map.by_classname("light").count() == 1);
    }

    #[test]
    fn the_preview_cache_key_tracks_face_selection() {
        // Picking one face tints it without editing the map, so the cache key
        // must include the face selection -- otherwise the 3D pane keeps the
        // stale image until the camera moves and invalidates it by accident.
        let mut app = ChiselApp::new(std::path::PathBuf::from("/definitely/not/here"));
        let id = app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        let side = app.document.find_solid(id).unwrap().sides[0].id;

        let before = app.preview_key(0, 160, 120);
        app.document.selection.faces.insert((id, side));
        let after = app.preview_key(0, 160, 120);

        assert_ne!(
            before, after,
            "selecting a face must invalidate the 3D preview"
        );
    }

    #[test]
    fn object_properties_targets_the_world_for_a_plain_brush() {
        let mut app = app_with_shipped_content();
        app.document = starter_document();
        let id = app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        app.document.selection.clear();
        app.document.selection.solids.insert(id);

        assert_eq!(app.properties_target(), Some(app.document.map.world.id));
    }

    #[test]
    fn object_properties_opens_on_the_brush_entity_not_the_world() {
        let mut app = app_with_shipped_content();
        app.document = starter_document();
        let id = app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        app.document.selection.clear();
        app.document.selection.solids.insert(id);
        app.document.set_brush_class(Some("func_door"));

        let target = app.properties_target().unwrap();
        assert_ne!(target, app.document.map.world.id);
        assert_eq!(
            app.document.find_entity(target).unwrap().classname(),
            "func_door"
        );
    }

    #[test]
    fn a_world_brush_can_carry_an_arbitrary_keyvalue() {
        let mut app = app_with_shipped_content();
        app.document = starter_document();
        let id = app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        app.document.selection.clear();
        app.document.selection.solids.insert(id);

        app.open_property_window();
        let window = app.property_window.as_mut().unwrap();
        assert_eq!(window.entity, app.document.map.world.id);
        // Every key the world reads is revealed, set or not.
        let keys: Vec<&str> = window.rows.iter().map(|r| r.key.as_str()).collect();
        assert!(
            keys.contains(&"skyname"),
            "world keys are revealed: {keys:?}"
        );

        // And a mapper can add a key the game has no name for, the way
        // `playercollision` lands on a Source brush.
        window.rows.push(PropertyRow {
            key: "playercollision".into(),
            label: "playercollision".into(),
            kind: KeyKind::String,
            help: String::new(),
            choices: Vec::new(),
            default: String::new(),
            value: Some("1".into()),
            described: false,
        });
        window.dirty = true;
        app.commit_property_window();

        assert_eq!(app.document.map.world.get("playercollision"), Some("1"));
    }

    #[test]
    fn the_object_properties_popup_edits_outputs_as_well_as_keys() {
        // The popup used to be keyvalues only, so wiring a door from it meant
        // closing it and reaching for the docked panel. Both halves of
        // Hammer's dialog now live in the one window.
        let mut app = app_with_shipped_content();
        app.document = starter_document();
        let id = app.document.create_entity("func_door", Vec3::ZERO);
        app.document.selection.clear();
        app.document.selection.entities.insert(id);

        app.open_property_window();
        let window = app.property_window.as_mut().unwrap();
        assert_eq!(window.entity, id);
        assert!(
            window.connections.is_empty(),
            "a fresh door is wired to nothing"
        );

        window
            .connections
            .push(Connection::new("OnFullyOpen", "lift", "Trigger"));
        window.dirty = true;
        app.commit_property_window();

        let door = app.document.find_entity(id).unwrap();
        assert_eq!(door.connections.len(), 1);
        assert_eq!(door.connections[0].target, "lift");
        assert_eq!(door.connections[0].input, "Trigger");
    }

    #[test]
    fn committing_the_popup_leaves_the_keys_and_the_wiring_both_intact() {
        // The two buffers are written back in one step. Writing either one
        // through a path that rebuilt the entity would silently drop the
        // other, and a lost output is not something a mapper notices until
        // the level is running.
        let mut app = app_with_shipped_content();
        app.document = starter_document();
        let id = app.document.create_entity("func_door", Vec3::ZERO);
        app.document.selection.clear();
        app.document.selection.entities.insert(id);

        app.open_property_window();
        let window = app.property_window.as_mut().unwrap();
        window
            .rows
            .iter_mut()
            .find(|r| r.key == "speed")
            .unwrap()
            .value = Some("250".into());
        window
            .connections
            .push(Connection::new("OnFullyOpen", "lift", "Trigger"));
        window.dirty = true;
        app.commit_property_window();

        let door = app.document.find_entity(id).unwrap();
        assert_eq!(door.get("speed"), Some("250"), "the keyvalue survived");
        assert_eq!(door.connections.len(), 1, "and so did the output");

        // One undo step, not two: the popup writes both halves together.
        app.document.undo();
        let door = app.document.find_entity(id).unwrap();
        assert_eq!(door.get("speed"), None);
        assert!(door.connections.is_empty());
    }

    #[test]
    fn the_popup_refreshes_its_wiring_when_the_document_moves_under_it() {
        // An undo, or an edit made from the docked panel, must reach the
        // popup. Otherwise closing it writes a stale buffer back and quietly
        // undoes the undo.
        let mut app = app_with_shipped_content();
        app.document = starter_document();
        let id = app.document.create_entity("func_door", Vec3::ZERO);
        app.document.selection.clear();
        app.document.selection.entities.insert(id);
        app.open_property_window();

        app.document.apply("wire it from elsewhere", |doc| {
            if let Some(e) = doc.find_entity_mut(id) {
                e.connections
                    .push(Connection::new("OnFullyOpen", "lift", "Trigger"));
            }
        });
        app.sync_property_window();

        let window = app.property_window.as_ref().unwrap();
        assert_eq!(
            window.connections.len(),
            1,
            "the popup shows what the map actually says"
        );
    }

    #[test]
    fn the_popup_draws_its_wiring_alongside_the_docked_panel() {
        // Both editors can be open on the same entity at once, and they share
        // their widget-building code. If they also shared widget ids, egui
        // would collapse the two into one and typing in either would move the
        // other's caret.
        let (mut app, root) = app_in("popup-outputs");
        let id = app.document.create_entity("func_door", Vec3::ZERO);
        if let Some(e) = app.document.find_entity_mut(id) {
            e.connections
                .push(Connection::new("OnFullyOpen", "lift", "Trigger"));
        }
        app.document.selection.clear();
        app.document.selection.entities.insert(id);
        app.open_property_window();

        let output = draw_a_frame(&mut app);
        assert!(!output.shapes.is_empty());
        assert!(app.property_window.is_some(), "drawing must not dismiss it");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Raw input describing an ordinary display.
    ///
    /// The default test context claims a 10000pt screen, on which nothing ever
    /// looks too big -- so a window that would swallow a real monitor still
    /// passes. Layout rules that depend on the screen have to be measured
    /// against a screen someone might own.
    fn a_real_screen() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        }
    }

    #[test]
    fn the_popup_settles_at_a_size_instead_of_growing_every_frame() {
        // A scroll area that fills the space it is given, inside a window that
        // sizes itself to its contents, is a loop: the area claims what the
        // window offered, the window grows to fit the claim, and next frame the
        // area claims the larger space. Left alone it walks to the edge of the
        // screen a frame at a time.
        let (mut app, root) = app_in("popup-size");
        let id = app.document.create_entity("func_door", Vec3::ZERO);
        app.document.selection.clear();
        app.document.selection.entities.insert(id);
        app.open_property_window();

        // One context across every frame: the runaway is only visible when the
        // window's stored size carries from one frame to the next.
        let ctx = egui::Context::default();
        let window = egui::Id::new("Object Properties");
        let mut sizes = Vec::new();
        for _ in 0..12 {
            let _ = ctx.run(a_real_screen(), |ctx| app.ui(ctx));
            if let Some(state) = egui::AreaState::load(&ctx, window) {
                sizes.push(state.rect().size());
            }
        }

        let first = *sizes.first().expect("the popup was laid out");
        let last = *sizes.last().expect("the popup was laid out");
        assert!(
            last.y <= first.y + 1.0,
            "the popup grew taller, {} to {}, over {} frames: {sizes:?}",
            first.y,
            last.y,
            sizes.len()
        );
        assert!(
            last.x <= first.x + 1.0,
            "the popup grew wider, {} to {}, over {} frames: {sizes:?}",
            first.x,
            last.x,
            sizes.len()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Drag the popup's bottom-right corner by `by` points and report the
    /// window's height before and after.
    fn drag_the_resize_corner(app: &mut ChiselApp, by: f32) -> (f32, f32) {
        let ctx = egui::Context::default();
        let window = egui::Id::new("Object Properties");
        let height = |ctx: &egui::Context| {
            egui::AreaState::load(ctx, window)
                .expect("the popup was laid out")
                .rect()
                .height()
        };

        // Settle first: a window that is still finding its size would make any
        // measurement here meaningless.
        for _ in 0..4 {
            let _ = ctx.run(a_real_screen(), |ctx| app.ui(ctx));
        }
        let before = height(&ctx);
        let corner = egui::AreaState::load(&ctx, window).unwrap().rect().max - egui::vec2(2.0, 2.0);

        let press = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };

        let mut grab = a_real_screen();
        grab.events = vec![egui::Event::PointerMoved(corner), press(corner, true)];
        let _ = ctx.run(grab, |ctx| app.ui(ctx));

        let target = corner + egui::vec2(0.0, by);
        for _ in 0..3 {
            let mut drag = a_real_screen();
            drag.events = vec![egui::Event::PointerMoved(target)];
            let _ = ctx.run(drag, |ctx| app.ui(ctx));
        }

        let mut release = a_real_screen();
        release.events = vec![press(target, false)];
        let _ = ctx.run(release, |ctx| app.ui(ctx));
        let _ = ctx.run(a_real_screen(), |ctx| app.ui(ctx));

        (before, height(&ctx))
    }

    #[test]
    fn the_value_fields_get_a_usable_width_rather_than_collapsing() {
        // The popup's rows were an `egui::Grid`, and a grid caps each cell at
        // the width its column measured last frame. Every widget in these rows
        // shrinks to the space it is offered, so a narrow column drew narrow
        // contents, which measured narrow, which kept the column narrow. A
        // `targetname` field asking for 190pt was drawing at 48 and staying
        // there -- and nothing about the map was wrong, so only looking at it
        // would tell you.
        let (mut app, root) = app_in("popup-widths");
        let id = app.document.create_entity("func_door", Vec3::ZERO);
        app.document.selection.clear();
        app.document.selection.entities.insert(id);
        app.open_property_window();

        let ctx = egui::Context::default();
        for _ in 0..4 {
            let _ = ctx.run(a_real_screen(), |ctx| app.ui(ctx));
        }

        let narrowest = app.property_window.as_ref().unwrap().narrowest_value;
        // A checkbox is legitimately small; anything that takes typing is not.
        assert!(
            narrowest > 20.0,
            "some value field drew at {narrowest}pt, which is nothing at all"
        );

        // And the one that has to hold a name has room for one.
        let window = app.property_window.as_mut().unwrap();
        window.rows.retain(|r| r.key == "targetname");
        for _ in 0..4 {
            let _ = ctx.run(a_real_screen(), |ctx| app.ui(ctx));
        }
        let name_field = app.property_window.as_ref().unwrap().narrowest_value;
        assert!(
            name_field > 150.0,
            "the targetname field drew at {name_field}pt, too narrow to read a name in"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_popup_can_be_dragged_taller_and_shorter_again() {
        // A smoke test, and honestly no more than one: it catches a popup whose
        // corner does nothing, which is what a body laid out at a fixed size
        // would give. It did *not* catch the content-hugging version that
        // shipped and had to be reported by hand -- headless, that version
        // resizes to the point, and only a real window showed it pinned. The
        // growth test above is the one with teeth.
        // Enough keys to fill the window: a short entity hides the bug,
        // because a window already smaller than its handle allows can still be
        // dragged. It is the full one that gets pinned to its contents.
        let (mut app, root) = app_in("popup-resize");
        let id = app.document.create_entity("func_door", Vec3::ZERO);
        if let Some(e) = app.document.find_entity_mut(id) {
            for n in 0..40 {
                e.set(&format!("custom_key_{n}"), "value");
            }
        }
        app.document.selection.clear();
        app.document.selection.entities.insert(id);
        app.open_property_window();

        let (before, taller) = drag_the_resize_corner(&mut app, 120.0);
        assert!(
            taller > before + 60.0,
            "dragging down 120pt moved the bottom edge from {before} to {taller}"
        );

        let (before, shorter) = drag_the_resize_corner(&mut app, -80.0);
        assert!(
            shorter < before - 40.0,
            "dragging up 80pt moved the bottom edge from {before} to {shorter}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_class_with_more_keys_than_fit_scrolls_rather_than_covering_the_screen() {
        // The other half of the size rule: capping the scroll area is what
        // makes a long entity produce a scrollbar instead of a window taller
        // than the display it is being edited on.
        let (mut app, root) = app_in("popup-tall");
        let id = app.document.create_entity("func_door", Vec3::ZERO);
        if let Some(e) = app.document.find_entity_mut(id) {
            for n in 0..60 {
                e.set(&format!("custom_key_{n}"), "value");
            }
        }
        app.document.selection.clear();
        app.document.selection.entities.insert(id);
        app.open_property_window();

        let ctx = egui::Context::default();
        let window = egui::Id::new("Object Properties");
        let mut size = egui::Vec2::ZERO;
        for _ in 0..8 {
            let _ = ctx.run(a_real_screen(), |ctx| app.ui(ctx));
            if let Some(state) = egui::AreaState::load(&ctx, window) {
                size = state.rect().size();
            }
        }

        // Not merely "fits on the screen": a popup that fills the display edge
        // to edge hides the map it is describing, which is most of the reason
        // to look at an entity's properties in the first place.
        let screen = ctx.screen_rect().height();
        assert!(
            size.y < screen * 0.85,
            "60 keys made a {}pt popup on a {screen}pt screen, leaving nothing behind it",
            size.y
        );

        // And it is the cap doing that rather than luck: the same entity with
        // six keys instead of sixty is not ten times shorter, because past the
        // cap the extra rows go behind a scrollbar instead of into the window.
        let (mut small, small_root) = app_in("popup-short");
        let id = small.document.create_entity("func_door", Vec3::ZERO);
        if let Some(e) = small.document.find_entity_mut(id) {
            for n in 0..6 {
                e.set(&format!("custom_key_{n}"), "value");
            }
        }
        small.document.selection.clear();
        small.document.selection.entities.insert(id);
        small.open_property_window();

        let ctx = egui::Context::default();
        let mut short = egui::Vec2::ZERO;
        for _ in 0..8 {
            let _ = ctx.run(a_real_screen(), |ctx| small.ui(ctx));
            if let Some(state) = egui::AreaState::load(&ctx, window) {
                short = state.rect().size();
            }
        }
        assert!(
            size.y < short.y * 2.0,
            "ten times the keys made the popup {}pt against {}pt -- it is growing with \
             its content rather than scrolling",
            size.y,
            short.y
        );

        let _ = std::fs::remove_dir_all(&small_root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_popup_closes_when_what_it_was_editing_is_deleted() {
        let (mut app, root) = app_in("popup-deleted");
        let id = app.document.create_entity("func_door", Vec3::ZERO);
        app.document.selection.clear();
        app.document.selection.entities.insert(id);
        app.open_property_window();

        app.document.delete_selection();
        app.sync_property_window();

        assert!(app.property_window.is_none(), "it edits nothing now");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn material_scanning_falls_back_when_there_is_no_content() {
        let materials = scan_materials(std::path::Path::new("/definitely/not/here"));
        assert!(!materials.is_empty());
        assert!(materials.iter().any(|m| m.starts_with("tools/")));
    }

    #[test]
    fn material_scanning_finds_what_is_there() {
        let dir = std::env::temp_dir().join(format!("chisel-mats-{}", std::process::id()));
        let materials = dir.join("materials/dev");
        std::fs::create_dir_all(&materials).unwrap();
        std::fs::write(materials.join("grid.keromat"), "lit { }").unwrap();
        std::fs::write(materials.join("notes.txt"), "ignored").unwrap();

        let found = scan_materials(&dir);
        assert_eq!(found, vec!["dev/grid"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn app_with_shipped_content() -> ChiselApp {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        ChiselApp::new(root)
    }

    #[test]
    fn the_entity_menus_come_from_the_games_definitions() {
        let app = app_with_shipped_content();
        assert!(
            !app.schema.is_empty(),
            "the shipped definitions load: {}",
            app.status
        );

        let points = app.point_classes();
        assert!(points.iter().any(|c| c == "light_spot"));
        assert!(points.iter().any(|c| c == "math_counter"));
        assert!(
            !points.iter().any(|c| c == "func_door"),
            "a door is not placed as a point"
        );

        let brushes = app.brush_classes();
        assert!(brushes.iter().any(|c| c == "func_door"));
        assert!(brushes.iter().any(|c| c == "trigger_multiple"));
        assert!(
            !brushes.iter().any(|c| c == "light"),
            "a light is not made of brushes"
        );
        assert!(
            !brushes.iter().any(|c| c == "worldspawn"),
            "the world is not something to tie to"
        );
    }

    #[test]
    fn the_menus_still_offer_something_without_a_content_tree() {
        let app = ChiselApp::new(std::path::PathBuf::from("/definitely/not/here"));
        // The built-in schema is always present, even with no content tree.
        assert!(
            !app.schema.is_empty(),
            "the built-in schema covers a missing tree"
        );
        assert!(
            !app.point_classes().is_empty(),
            "the entity tool must not be dead"
        );
        assert!(!app.brush_classes().is_empty());
        assert!(
            app.status.contains("built in"),
            "and it says so: {}",
            app.status
        );
    }

    #[test]
    fn model_scanning_finds_compiled_models() {
        let dir = std::env::temp_dir().join(format!("chisel-models-{}", std::process::id()));
        let models = dir.join("models/props");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("crate.keromdl"), "").unwrap();
        std::fs::write(models.join("crate.obj"), "").unwrap();
        assert_eq!(scan_models(&dir), vec!["props/crate"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_layout_is_hammers_four_panes() {
        let panes = Viewport::default_layout();
        assert_eq!(panes[0].kind, ViewportKind::Perspective);
        assert_eq!(panes[1].kind, ViewportKind::Top);
        assert_eq!(panes[2].kind, ViewportKind::Front);
        assert_eq!(panes[3].kind, ViewportKind::Right);
    }

    // ---- saving, naming and renaming ------------------------------------

    /// An editor pointed at an empty scratch project.
    fn app_in(name: &str) -> (ChiselApp, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "chisel-app-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("maps")).unwrap();
        let mut app = ChiselApp::new(root.clone());
        app.document = starter_document();
        (app, root)
    }

    #[test]
    fn saving_a_map_that_has_no_name_asks_for_one_instead_of_failing() {
        let (mut app, root) = app_in("save-unnamed");
        app.save(None);

        let prompt = app
            .prompt
            .as_ref()
            .expect("ctrl-S on an unnamed map should ask for a name");
        assert_eq!(prompt.kind, PromptKind::SaveAs);
        assert!(
            app.document.path.is_none(),
            "nothing is written until it has a name"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_typed_name_saves_into_the_projects_maps_directory() {
        let (mut app, root) = app_in("save-named");
        app.save(None);
        app.prompt.as_mut().unwrap().name = "arena".into();
        assert!(app.confirm_prompt());

        let expected = root.join("maps/arena.keromap");
        assert_eq!(app.document.path.as_deref(), Some(expected.as_path()));
        assert!(expected.is_file(), "the map is on disk");
        assert!(
            !app.document.is_modified(),
            "and no longer counts as unsaved"
        );
        assert!(app.prompt.is_none());
        assert!(app.status.contains("maps/arena.keromap"), "{}", app.status);

        // And it is a map, not an empty file.
        let text = std::fs::read_to_string(&expected).unwrap();
        assert!(text.contains("worldspawn"), "{text}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_name_that_will_not_do_leaves_the_prompt_open_and_says_why() {
        let (mut app, root) = app_in("save-bad-name");
        app.save(None);
        app.prompt.as_mut().unwrap().name = "   ".into();

        assert!(!app.confirm_prompt(), "the prompt stays open");
        let prompt = app.prompt.as_ref().unwrap();
        assert!(prompt.error.is_some(), "and says what is wrong");
        assert!(app.document.path.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn saving_again_writes_the_same_file_without_asking() {
        let (mut app, root) = app_in("save-again");
        app.save(Some(root.join("maps/arena.keromap")));
        app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        assert!(app.document.is_modified());

        app.save(None);
        assert!(
            app.prompt.is_none(),
            "a map with a name is not asked about again"
        );
        assert!(!app.document.is_modified());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renaming_moves_the_map_and_what_was_compiled_from_it() {
        let (mut app, root) = app_in("rename");
        app.save(Some(root.join("maps/old.keromap")));
        std::fs::write(root.join("maps/old.kerobsp"), b"compiled").unwrap();

        app.begin_prompt(PromptKind::Rename);
        assert_eq!(
            app.prompt.as_ref().unwrap().name,
            "old.keromap",
            "filled in with the current name"
        );
        app.prompt.as_mut().unwrap().name = "new".into();
        assert!(app.confirm_prompt());

        assert_eq!(
            app.document.path.as_deref(),
            Some(root.join("maps/new.keromap").as_path())
        );
        assert!(!root.join("maps/old.keromap").exists());
        assert!(
            root.join("maps/new.kerobsp").is_file(),
            "the compiled map came too"
        );
        assert!(app.status.contains("renamed"), "{}", app.status);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renaming_over_an_existing_map_is_refused() {
        let (mut app, root) = app_in("rename-clash");
        app.save(Some(root.join("maps/old.keromap")));
        std::fs::write(root.join("maps/taken.keromap"), "someone else's work").unwrap();

        app.begin_prompt(PromptKind::Rename);
        app.prompt.as_mut().unwrap().name = "taken".into();
        assert!(!app.confirm_prompt(), "the prompt stays open");

        assert!(
            app.prompt
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("already exists")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("maps/taken.keromap")).unwrap(),
            "someone else's work",
            "and the map that was there is untouched"
        );
        assert!(root.join("maps/old.keromap").is_file());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renaming_a_map_that_was_never_saved_just_saves_it() {
        let (mut app, root) = app_in("rename-unsaved");
        app.begin_prompt(PromptKind::Rename);
        app.prompt.as_mut().unwrap().name = "first".into();
        assert!(app.confirm_prompt());

        assert!(root.join("maps/first.keromap").is_file());
        assert!(!app.document.is_modified());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renaming_carries_unsaved_changes_to_the_new_name() {
        let (mut app, root) = app_in("rename-dirty");
        app.save(Some(root.join("maps/old.keromap")));
        let before = std::fs::read_to_string(root.join("maps/old.keromap")).unwrap();
        app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));

        app.begin_prompt(PromptKind::Rename);
        app.prompt.as_mut().unwrap().name = "new".into();
        assert!(app.confirm_prompt());

        let after = std::fs::read_to_string(root.join("maps/new.keromap")).unwrap();
        assert_ne!(
            after, before,
            "the file under the new name is the map as it stands"
        );
        assert!(!app.document.is_modified());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opening_a_map_with_unsaved_changes_asks_first() {
        let (mut app, root) = app_in("discard");
        app.save(Some(root.join("maps/arena.keromap")));
        let other = root.join("maps/other.keromap");
        std::fs::copy(root.join("maps/arena.keromap"), &other).unwrap();
        app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));

        app.discard_or_ask(Discarding::Open(other.clone()));
        assert_eq!(app.discarding, Some(Discarding::Open(other.clone())));
        assert_eq!(
            app.document.path.as_deref(),
            Some(root.join("maps/arena.keromap").as_path())
        );

        // Answering the question goes through with it.
        app.discarding = None;
        app.discard_now(Discarding::Open(other.clone()));
        assert_eq!(app.document.path.as_deref(), Some(other.as_path()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_saved_map_is_replaced_without_a_question() {
        let (mut app, root) = app_in("no-question");
        app.save(Some(root.join("maps/arena.keromap")));

        app.discard_or_ask(Discarding::New);
        assert!(
            app.discarding.is_none(),
            "nothing would be lost, so nothing is asked"
        );
        assert!(app.document.path.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn compiling_an_unnamed_map_asks_for_a_name_rather_than_inventing_one() {
        let (mut app, root) = app_in("compile-unnamed");
        app.compile_now(Quality::Fast);

        assert!(app.compile.is_none(), "nothing is compiled yet");
        assert_eq!(
            app.prompt.as_ref().map(|p| p.kind),
            Some(PromptKind::SaveAs)
        );
        assert!(
            !root.join("maps/untitled.keromap").exists(),
            "and no `untitled` is left behind"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_maps_offered_to_open_are_the_ones_in_the_project() {
        let (mut app, root) = app_in("map-list");
        app.save(Some(root.join("maps/arena.keromap")));
        app.save(Some(root.join("maps/lobby.keromap")));

        let names: Vec<String> = files::maps_in(&root)
            .iter()
            .map(|p| files::label(p, &root))
            .collect();
        assert_eq!(names, vec!["maps/arena.keromap", "maps/lobby.keromap"]);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Draw one frame with no window, the way the editor would.
    ///
    /// egui does not need a renderer to produce a frame, so the whole UI can
    /// be exercised in a test. It is worth doing: a panel that panics or a
    /// borrow that conflicts only shows up when the code actually runs, and
    /// "it compiled" has never been the same thing as "it opens".
    fn draw_a_frame(app: &mut ChiselApp) -> egui::FullOutput {
        let ctx = egui::Context::default();
        ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx))
    }

    #[test]
    fn the_editor_draws_a_frame() {
        let (mut app, root) = app_in("frame");
        let output = draw_a_frame(&mut app);
        assert!(
            !output.shapes.is_empty(),
            "a frame with nothing in it is not a frame"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_name_prompt_draws_and_stays_open_until_it_is_answered() {
        let (mut app, root) = app_in("frame-prompt");
        app.begin_prompt(PromptKind::SaveAs);

        draw_a_frame(&mut app);
        assert!(app.prompt.is_some(), "drawing it must not dismiss it");
        // The field takes the keyboard on the frame it opens, and only then.
        assert!(!app.prompt.as_ref().unwrap().fresh);

        draw_a_frame(&mut app);
        assert!(app.prompt.is_some());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_unsaved_changes_question_draws() {
        let (mut app, root) = app_in("frame-discard");
        app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        app.discard_or_ask(Discarding::New);

        draw_a_frame(&mut app);
        assert_eq!(
            app.discarding,
            Some(Discarding::New),
            "asked, and still waiting"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_window_title_names_the_map_and_whether_it_is_saved() {
        let (mut app, root) = app_in("title");
        assert_eq!(app.window_title(), "untitled -- Chisel");

        app.save(Some(root.join("maps/arena.keromap")));
        assert_eq!(app.window_title(), "arena.keromap -- Chisel");

        app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        assert_eq!(app.window_title(), "arena.keromap * -- Chisel");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_modal_takes_the_keyboard_from_the_shortcuts_behind_it() {
        let (mut app, root) = app_in("modal-keys");
        app.save(Some(root.join("maps/arena.keromap")));
        app.begin_prompt(PromptKind::SaveAs);

        // Ctrl-S with the dialog open must not save under the old name --
        // that is the name the dialog is asking you to replace.
        app.status = "nothing has happened yet".into();
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: Key::S,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        });
        let _ = ctx.run(input, |ctx| app.ui(ctx));

        assert!(app.prompt.is_some(), "the dialog is still asking");
        assert_eq!(
            app.status, "nothing has happened yet",
            "the shortcut behind the modal fired"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- the brush panel -------------------------------------------------

    /// The editor with one world brush selected.
    fn app_with_a_brush(name: &str) -> (ChiselApp, PathBuf) {
        let (mut app, root) = app_in(name);
        app.document.map.world.solids.clear();
        let id = app.document.create_block(Vec3::ZERO, Vec3::splat(128.0));
        app.document.selection.clear();
        app.document.selection.solids.insert(id);
        (app, root)
    }

    #[test]
    fn a_world_brush_draws_its_panel() {
        let (mut app, root) = app_with_a_brush("panel-world");
        let output = draw_a_frame(&mut app);
        assert!(!output.shapes.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn giving_a_brush_a_type_draws_its_settings_in_the_same_panel() {
        // No mode change, no second page: the type is a setting on the brush
        // and everything else follows from it.
        let (mut app, root) = app_with_a_brush("panel-typed");
        assert!(app.document.set_brush_class(Some("trigger_multiple")));
        draw_a_frame(&mut app);

        // The buffer the panel edits is pointed at the entity that now owns
        // the brushes, without anyone having selected it by hand.
        let (id, class) = app.document.selected_brush_class().unwrap();
        assert_eq!(class, "trigger_multiple");
        assert_eq!(
            app.document.selection.entities.iter().copied().next(),
            Some(id)
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_brush_class_draws_without_falling_over() {
        // Including the ones with no settings at all, which is the case that
        // used to look identical to a door.
        let (mut app, root) = app_with_a_brush("panel-all");
        for class in app.brush_classes() {
            app.document.set_brush_class(Some(&class));
            let output = draw_a_frame(&mut app);
            assert!(!output.shapes.is_empty(), "{class}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn putting_a_brush_back_in_the_world_draws_too() {
        let (mut app, root) = app_with_a_brush("panel-untied");
        app.document.set_brush_class(Some("func_door"));
        draw_a_frame(&mut app);
        assert!(app.document.set_brush_class(None));
        draw_a_frame(&mut app);

        assert!(app.document.selected_brush_class().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_entity_that_chooses_draws_both_sides_of_the_choice() {
        let (mut app, root) = app_in("panel-branch");
        let id = app.document.create_entity("logic_branch", Vec3::ZERO);
        if let Some(e) = app.document.find_entity_mut(id) {
            e.connections
                .push(kerosene_map::Connection::new("OnTrue", "gate", "Lock"));
        }
        app.document.selection.clear();
        app.document.selection.entities.insert(id);

        let output = draw_a_frame(&mut app);
        assert!(!output.shapes.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_game_offers_an_alternative_to_wire_at_all() {
        // Everything else in the game fires a list. Without a class that
        // chooses, a map can say "when X, do Y" and has no way to say
        // "otherwise do Z" -- and no editor can offer what does not exist.
        let app = app_with_shipped_content();
        let spec = app.schema.get("logic_branch").expect("the game defines it");
        let outputs: Vec<&str> = spec.outputs.iter().map(|o| o.name.as_str()).collect();
        assert!(
            outputs.contains(&"OnTrue") && outputs.contains(&"OnFalse"),
            "{outputs:?}"
        );
        assert_eq!(crate::wiring::opposite_of("OnTrue"), Some("OnFalse"));
    }

    // ---- the frame's furniture ------------------------------------------

    #[test]
    fn every_tool_draws_its_strip_toolbar_and_inspector_tabs() {
        let (mut app, root) = app_in("frame-tools");
        for kind in ToolKind::all() {
            app.tool.set_kind(kind);
            for tab in [
                InspectorTab::Object,
                InspectorTab::Tool,
                InspectorTab::Materials,
            ] {
                app.inspector_tab = tab;
                let output = draw_a_frame(&mut app);
                assert!(!output.shapes.is_empty(), "{kind:?} on {tab:?}");
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn picking_the_entity_or_shape_tool_shows_the_tool_tab() {
        let (mut app, root) = app_in("frame-follow");
        app.inspector_tab = InspectorTab::Materials;
        draw_a_frame(&mut app);
        app.tool.set_kind(ToolKind::Entity);
        draw_a_frame(&mut app);
        assert_eq!(app.inspector_tab, InspectorTab::Tool);

        // Looking at the materials again, then selecting something, brings
        // the Object tab up -- but only on a *fresh* selection.
        app.inspector_tab = InspectorTab::Materials;
        draw_a_frame(&mut app);
        let id = app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        app.document.selection.solids.insert(id);
        draw_a_frame(&mut app);
        assert_eq!(app.inspector_tab, InspectorTab::Object);
        app.inspector_tab = InspectorTab::Tool;
        draw_a_frame(&mut app);
        assert_eq!(app.inspector_tab, InspectorTab::Tool, "left where it was put");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_pane_can_be_maximised_and_put_back() {
        let (mut app, root) = app_in("frame-maximise");
        app.active = 2;
        app.toggle_maximised();
        assert_eq!(app.maximised, Some(2));
        let output = draw_a_frame(&mut app);
        assert!(!output.shapes.is_empty());
        app.toggle_maximised();
        assert_eq!(app.maximised, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_materials_tab_applies_to_the_selection() {
        let (mut app, root) = app_with_a_brush("materials-tab");
        let material = app.materials[1].clone();
        app.apply_browsed(&Browsing::Material, &material);
        assert_eq!(app.document.current_material, material);
        let faces = app.document.map.world.solids[0].sides.iter();
        assert!(faces.clone().all(|s| s.material == material), "applied to every face");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn starting_a_compile_closes_the_settings_and_feeds_the_output_panel() {
        let (mut app, root) = app_in("compile-output");
        app.save(Some(root.join("maps/arena.keromap")));
        app.show_compile = true;
        app.compile_now(Quality::Fast);
        assert!(!app.show_compile, "the dialog is done once the compile starts");
        assert!(app.compiling() || app.compile_failed().is_some());
        // Whatever the compilers made of it, the log is readable as lines.
        for _ in 0..50 {
            if let Some(job) = &mut app.compile {
                job.poll();
                if job.finished {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _ = app.output_lines();
        app.compile = None;
        let _ = std::fs::remove_dir_all(&root);
    }
}
