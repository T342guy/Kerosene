// SPDX-License-Identifier: LGPL-3.0-or-later
//! The editor's user interface.
//!
//! Hammer's layout, because it is the right one for the job: a toolbar down
//! the left, an inspector on the right, a status bar along the bottom, and
//! four viewports filling the middle -- 3D, top, front and side.
//!
//! Everything here turns a gesture into a call on [`Document`] and draws the
//! result. The decisions all live in the modules it calls.

use crate::compile::{CompileJob, CompileMessage, CompileSettings, Quality, available_tools};
use crate::document::Document;
use crate::inspector::{self, PropertyRow};
use crate::tools::{Tool, ToolKind};
use crate::viewport::Viewport;
use crate::raster::Shading;
use crate::textures::TextureCache;
use crate::{classes, draw, files, raster};
use egui::{Context, Key, Modifiers, RichText};
use std::path::PathBuf;
use void_entity::{ClassKind, KeyKind, Schema};
use void_map::Connection;
use void_math::Vec3;

/// Entity classes offered when the content tree has no definitions in it.
///
/// A bare minimum so the entity tool is not simply broken without content;
/// the real list comes from the game's `.voiddef`.
const FALLBACK_CLASSES: &[&str] = &["info_player_start", "light", "logic_relay"];

/// How fast the 3D camera flies by default, in void units per second.
///
/// A shade above a player's running speed, so moving through a level in the
/// editor feels like the pace it will be played at. Shift doubles it, Alt
/// halves it, and the wheel changes it while flying.
const DEFAULT_FLY_SPEED: f32 = 384.0;

/// The width of the bars between panes, in points.
const SPLITTER: f32 = 5.0;

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

/// A file operation waiting for a name.
///
/// Naming a map is a decision, so it gets a field to type into rather than a
/// menu item with the answer baked in. "save as maps/untitled.voidmap" was
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
enum Decision { Save, Discard, Cancel }

/// Something that would discard unsaved work, held until it is confirmed.
#[derive(Clone, Debug, PartialEq)]
pub enum Discarding {
    /// Start again on an empty map.
    New,
    /// Open a map from disk.
    Open(PathBuf),
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
    pub vfs: void_vfs::Vfs,
    /// How the 3D panes draw.
    pub shading: Shading,
    /// Substring filter on the material browser.
    pub material_filter: String,
    /// Where the content tree came from, for the status bar to show. An
    /// editor with no content is nearly useless, so how it decided is worth
    /// having in front of you rather than in a log nobody reads.
    pub content_note: String,
    /// The route out of a leaking map, from the last compile.
    pub leak: crate::leak::LeakTrace,
    /// Where the four panes divide, as fractions of the area. Dragged.
    pub split: egui::Vec2,
    /// How fast the 3D camera flies, in void units per second.
    pub fly_speed: f32,
    /// The rasterised 3D panes, kept until something they depend on moves.
    previews: [Option<Preview>; 4],
    /// A save-as or rename waiting for a name.
    pub prompt: Option<NamePrompt>,
    /// Something that would throw away unsaved work, waiting to be confirmed.
    pub discarding: Option<Discarding>,
    /// In-progress property edits, held until the field is done with.
    ///
    /// Committing on every keystroke would push a whole undo snapshot per
    /// character typed, so a field is edited in a buffer and written back when
    /// it loses focus or the selection moves on.
    properties: Option<PropertyEdit>,
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

impl ChiselApp {
    pub fn new(content_root: PathBuf) -> ChiselApp {
        let materials = scan_materials(&content_root);
        let models = scan_models(&content_root);
        let loaded = classes::load(&content_root);
        let status = loaded.summary();
        let mut vfs = void_vfs::Vfs::new();
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
            material_filter: String::new(),
            content_note: String::new(),
            leak: crate::leak::LeakTrace::default(),
            split: egui::vec2(0.5, 0.5),
            fly_speed: DEFAULT_FLY_SPEED,
            previews: [const { None }; 4],
            prompt: None,
            discarding: None,
            properties: None,
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
        names.into_iter().filter(|n| *n != "worldspawn").map(str::to_string).collect()
    }

    /// Classes that brushes can be tied to.
    pub fn brush_classes(&self) -> Vec<String> {
        let names = self.schema.names_of_kind(ClassKind::Brush);
        if names.is_empty() {
            return vec!["func_detail".into(), "func_brush".into(), "trigger_multiple".into()];
        }
        names.into_iter().filter(|n| *n != "worldspawn").map(str::to_string).collect()
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
    pub fn save(&mut self, path: Option<PathBuf>) {
        self.commit_properties();
        if path.is_none() && self.document.path.is_none() {
            self.begin_prompt(PromptKind::SaveAs);
            return;
        }
        match self.document.save(path) {
            Ok(path) => self.status = format!("saved {}", files::label(&path, &self.content_root)),
            Err(e) => self.status = format!("could not save: {e}"),
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
        self.prompt = Some(NamePrompt { kind, name, error: None, fresh: true });
    }

    /// Act on the name that was typed, or say why it will not do.
    ///
    /// Returns whether the prompt is finished with.
    pub fn confirm_prompt(&mut self) -> bool {
        let Some(prompt) = &self.prompt else { return true };
        let (kind, typed) = (prompt.kind, prompt.name.clone());

        let target = match files::resolve(&typed, &self.content_root) {
            Ok(target) => target,
            Err(e) => {
                if let Some(prompt) = &mut self.prompt { prompt.error = Some(e) }
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
                if let Some(prompt) = &mut self.prompt { prompt.error = Some(e) }
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
            return Ok(format!("{} is already its name", files::label(&target, &self.content_root)));
        }
        if target.exists() {
            return Err(format!("{} already exists", files::label(&target, &self.content_root)));
        }
        if !from.exists() {
            return self.save_as(target);
        }

        let moved = files::move_map(&from, &target)
            .map_err(|e| format!("could not rename: {e}"))?;
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
        }
    }

    /// The name field, and the "this will lose work" question.
    fn file_windows(&mut self, ctx: &Context) {
        if let Some(prompt) = &mut self.prompt {
            let (kind, mut cancel, mut confirm) = (prompt.kind, false, false);
            let fresh = std::mem::take(&mut prompt.fresh);
            let mut name = std::mem::take(&mut prompt.name);
            let error = prompt.error.clone();

            // A modal rather than a floating window: it dims what is behind
            // it and swallows the clicks, so nobody draws half a brush into a
            // map that is mid-way through being renamed.
            let modal = egui::Modal::new(egui::Id::new("chisel-name-prompt"))
                .show(ctx, |ui| {
                    ui.set_min_width(420.0);
                    ui.heading(kind.title());
                    ui.add_space(2.0);
                    ui.label(RichText::new("name").size(11.0).weak());
                    let mut output = egui::TextEdit::singleline(&mut name)
                        .desired_width(f32::INFINITY)
                        .hint_text("arena")
                        .show(ui);
                    let field = &output.response;
                    if fresh {
                        field.request_focus();
                        // And with the whole name selected, so typing
                        // replaces it. The field is filled in with the
                        // current name because that is usually what is being
                        // changed -- which makes "delete it first" the most
                        // common thing the dialog asks of anyone.
                        let all = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(name.chars().count()),
                        );
                        output.state.cursor.set_char_range(Some(all));
                        output.state.clone().store(ui.ctx(), output.response.id);
                    }
                    if output.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        confirm = true;
                    }

                    // What the name is about to mean, before it means it.
                    match files::resolve(&name, &self.content_root) {
                        Ok(path) => {
                            let label = files::label(&path, &self.content_root);
                            let exists = path.exists();
                            let note = if exists && kind == PromptKind::SaveAs {
                                RichText::new(format!("{label}  -- overwrites the map already there"))
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(220, 170, 90))
                            } else {
                                RichText::new(label).size(11.0).weak()
                            };
                            ui.label(note);
                        }
                        Err(e) => {
                            ui.label(RichText::new(e).size(11.0).color(egui::Color32::from_rgb(220, 110, 110)));
                        }
                    }
                    if let Some(error) = &error {
                        ui.label(RichText::new(error).color(egui::Color32::from_rgb(220, 110, 110)));
                    }

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button(kind.verb()).clicked() { confirm = true }
                        if ui.button("cancel").clicked() { cancel = true }
                    });
                });

            if let Some(prompt) = &mut self.prompt {
                prompt.name = name;
            }
            // Escape, or a click on the dimmed background.
            if modal.should_close() { cancel = true }
            if cancel {
                self.prompt = None;
            } else if confirm {
                self.confirm_prompt();
            }
        }

        if let Some(what) = self.discarding.clone() {
            let mut decided = None;
            let modal = egui::Modal::new(egui::Id::new("chisel-unsaved"))
                .show(ctx, |ui| {
                    ui.set_min_width(360.0);
                    ui.heading("unsaved changes");
                    ui.add_space(2.0);
                    ui.label(format!("{} has changes that have not been saved.", self.document.title()));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("save first").clicked() { decided = Some(Decision::Save) }
                        if ui.button("discard them").clicked() { decided = Some(Decision::Discard) }
                        if ui.button("cancel").clicked() { decided = Some(Decision::Cancel) }
                    });
                });
            // Escape and a click outside both mean "no", which is the answer
            // that keeps the work.
            if modal.should_close() { decided = Some(Decision::Cancel) }

            match decided {
                Some(Decision::Save) => {
                    self.discarding = None;
                    let had_path = self.document.path.is_some();
                    self.save(None);
                    // With no path the save turned into a name prompt, and
                    // going ahead now would throw away the work it is asking
                    // where to put.
                    if had_path { self.discard_now(what) }
                }
                Some(Decision::Discard) => {
                    self.discarding = None;
                    self.discard_now(what);
                }
                Some(Decision::Cancel) => self.discarding = None,
                None => {}
            }
        }
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
        self.inspector(ctx);
        self.status_bar(ctx);
        self.compile_window(ctx);
        self.file_windows(ctx);
        self.viewports_panel(ctx);
    }

    /// Pick up whatever the compile left behind.
    ///
    /// Chiefly the leak trace: Cleave writes one beside the map when the world
    /// is not sealed, and it is only worth writing if something draws it. A
    /// clean compile clears the last one, so a fixed leak stops being shown.
    fn after_compile(&mut self) {
        let Some(job) = &self.compile else { return };
        let failed = job.failed;
        let map = job
            .output()
            .map(|p| p.to_path_buf())
            .or_else(|| self.document.path.clone().map(|p| p.with_extension("voidbsp")));

        // Cleared first, and unconditionally. A trace is about one compile,
        // and keeping the last one around because this compile wrote none is
        // how a fixed map goes on reporting a leak.
        self.leak = crate::leak::LeakTrace::default();
        if let Some(trace) = map.as_deref().and_then(crate::leak::LeakTrace::beside) {
            self.leak = trace;
        }

        // Alchemy may have just produced textures that failed to load when
        // the editor started. Without this the pane keeps drawing the flat
        // fallback until someone restarts, which looks exactly like the
        // compile having done nothing.
        self.textures.clear();
        self.thumbnails.clear();
        self.materials = scan_materials(&self.content_root);

        self.status = if failed {
            "compile failed".into()
        } else if let Some(at) = self.leak.origin() {
            format!(
                "compiled, but the map LEAKS -- follow the red line from {} {} {}",
                void_math::format_float(at.x),
                void_math::format_float(at.y),
                void_math::format_float(at.z),
            )
        } else {
            "compile finished".into()
        };
    }

    fn shortcuts(&mut self, ctx: &Context) {
        // A modal takes the keyboard whole. Otherwise ctrl-S while the "save
        // as" field is open would save the map under the name the dialog is
        // asking you to replace, and escape would both close the dialog and
        // clear the selection behind it.
        if self.prompt.is_some() || self.discarding.is_some() { return }

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
            Delete,
            Cancel,
            Tool(ToolKind),
            Finer,
            Coarser,
            Compile,
        }

        let mut actions = Vec::new();
        ctx.input_mut(|i| {
            let ctrl = Modifiers::COMMAND;

            // Chorded shortcuts stay live while typing: ctrl-S must save
            // whatever the focus is.
            if i.consume_key(ctrl, Key::Z) && !typing { actions.push(Action::Undo) }
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
            if i.consume_key(ctrl | Modifiers::SHIFT, Key::S) { actions.push(Action::SaveAs) }
            if i.consume_key(ctrl, Key::S) { actions.push(Action::Save) }

            if typing { return }

            if i.consume_key(Modifiers::NONE, Key::Delete)
                || i.consume_key(Modifiers::NONE, Key::Backspace)
            {
                actions.push(Action::Delete)
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) { actions.push(Action::Cancel) }

            // Tool shortcuts, as Hammer numbers them.
            for (index, kind) in ToolKind::all().into_iter().enumerate() {
                let key = match index {
                    0 => Key::Num1,
                    1 => Key::Num2,
                    2 => Key::Num3,
                    _ => Key::Num4,
                };
                if i.consume_key(Modifiers::NONE, key) { actions.push(Action::Tool(kind)) }
            }

            // The grid keys every brush editor has used for thirty years.
            if i.consume_key(Modifiers::NONE, Key::OpenBracket) { actions.push(Action::Finer) }
            if i.consume_key(Modifiers::NONE, Key::CloseBracket) { actions.push(Action::Coarser) }
            if i.consume_key(Modifiers::NONE, Key::F9) { actions.push(Action::Compile) }
        });

        for action in actions {
            match action {
                // Anything half-typed becomes its own undo step first, so
                // ctrl-Z takes back the edit rather than the one before it.
                Action::Undo => {
                    self.commit_properties();
                    if let Some(label) = self.document.undo() {
                        self.status = format!("undid {label}");
                    }
                }
                Action::Redo => {
                    self.commit_properties();
                    if let Some(label) = self.document.redo() {
                        self.status = format!("redid {label}");
                    }
                }
                Action::Save => self.save(None),
                Action::SaveAs => self.begin_prompt(PromptKind::SaveAs),
                Action::Delete => {
                    let n = self.document.delete_selection();
                    if n > 0 { self.status = format!("deleted {n}") }
                }
                Action::Cancel => {
                    self.tool.cancel();
                    self.document.selection.clear();
                }
                Action::Tool(kind) => self.tool.set_kind(kind),
                Action::Finer => self.document.grid.finer(),
                Action::Coarser => self.document.grid.coarser(),
                Action::Compile => self.compile_now(Quality::Fast),
            }
        }
    }

    fn menu_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("file", |ui| {
                    if ui.button("new").clicked() {
                        self.discard_or_ask(Discarding::New);
                        ui.close();
                    }

                    // The maps in this project, by name. A file browser would
                    // be the general answer; this is the one that is right
                    // almost every time, and it is one click.
                    let maps = files::maps_in(&self.content_root);
                    ui.menu_button("open", |ui| {
                        if maps.is_empty() {
                            ui.label(RichText::new("no maps in this project yet").size(11.0).weak());
                        }
                        for map in &maps {
                            let name = files::label(map, &self.content_root);
                            let name = name.strip_prefix("maps/").unwrap_or(&name);
                            if ui.button(name).clicked() {
                                self.discard_or_ask(Discarding::Open(map.clone()));
                                ui.close();
                            }
                        }
                    });

                    ui.separator();
                    if ui.button("save            ctrl-S").clicked() {
                        self.save(None);
                        ui.close();
                    }
                    if ui.button("save as...      ctrl-shift-S").clicked() {
                        self.begin_prompt(PromptKind::SaveAs);
                        ui.close();
                    }
                    if ui.button("rename...").clicked() {
                        self.begin_prompt(PromptKind::Rename);
                        ui.close();
                    }

                    ui.separator();
                    let where_it_is = match self.document.path.as_deref() {
                        Some(path) => files::label(path, &self.content_root),
                        None => "not saved anywhere yet".to_string(),
                    };
                    ui.label(RichText::new(where_it_is).size(11.0).weak());
                });

                ui.menu_button("edit", |ui| {
                    let undo = self.document.undo_label().map(str::to_string);
                    let label = undo.map_or("undo".to_string(), |l| format!("undo {l}"));
                    if ui.add_enabled(self.document.undo_depth() > 0, egui::Button::new(label)).clicked() {
                        self.document.undo();
                        ui.close();
                    }
                    if ui.add_enabled(self.document.redo_depth() > 0, egui::Button::new("redo")).clicked() {
                        self.document.redo();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("delete").clicked() { self.document.delete_selection(); ui.close(); }
                });

                ui.menu_button("map", |ui| {
                    if ui.button("compile (fast)  F9").clicked() {
                        self.compile_now(Quality::Fast);
                        ui.close();
                    }
                    if ui.button("compile (full)").clicked() {
                        self.compile_now(Quality::Full);
                        ui.close();
                    }
                    ui.separator();
                    if !self.leak.is_empty() && ui.button("clear the leak trace").clicked() {
                        self.leak = crate::leak::LeakTrace::default();
                        self.status = "leak trace cleared".into();
                        ui.close();
                    }
                    if ui.button("check for problems").clicked() {
                        let problems = self.document.problems();
                        self.status = if problems.is_empty() {
                            "no problems found".into()
                        } else {
                            format!("{} problems: {}", problems.len(), problems[0])
                        };
                        ui.close();
                    }
                    if ui.button("check tools are installed").clicked() {
                        self.show_tools_check = true;
                        ui.close();
                    }
                });

                ui.menu_button("view", |ui| {
                    ui.checkbox(&mut self.document.grid.visible, "show grid");
                    ui.checkbox(&mut self.document.grid.snap, "snap to grid");

                    ui.separator();
                    ui.label(RichText::new("3D panes").size(11.0).weak());
                    for mode in Shading::all() {
                        if ui.radio_value(&mut self.shading, mode, mode.label()).clicked() {
                            self.status = format!("3D panes: {}", mode.label());
                        }
                    }
                    if ui.button("reload textures").clicked() {
                        self.textures.clear();
                        self.thumbnails.clear();
                        self.materials = scan_materials(&self.content_root);
                        self.status = "textures reloaded".into();
                        ui.close();
                    }

                    ui.separator();
                    if ui.button("frame everything").clicked() { self.frame_all(); ui.close(); }
                    if self.maximised.is_some() && ui.button("show four panes").clicked() {
                        self.maximised = None;
                        ui.close();
                    }
                });

                ui.separator();
                ui.label(RichText::new(self.document.title()).monospace());
            });
        });
    }

    fn toolbar(&mut self, ctx: &Context) {
        egui::SidePanel::left("tools").exact_width(120.0).show(ctx, |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("tools").strong());
            for kind in ToolKind::all() {
                let selected = self.tool.kind == kind;
                let label = format!("{}  [{}]", kind.label(), kind.shortcut());
                if ui.selectable_label(selected, label).clicked() {
                    self.tool.set_kind(kind);
                }
            }

            ui.separator();
            ui.label(RichText::new("grid").strong());
            ui.horizontal(|ui| {
                if ui.small_button("[").clicked() { self.document.grid.finer(); }
                ui.label(RichText::new(void_math::units::length_short(self.document.grid.size)).monospace())
                    .on_hover_text(void_math::units::length(self.document.grid.size));
                if ui.small_button("]").clicked() { self.document.grid.coarser(); }
            });

            ui.separator();
            self.material_browser(ui);

            if self.tool.kind == ToolKind::Entity {
                ui.separator();
                ui.label(RichText::new("entity").strong());
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    for class in self.point_classes() {
                        let selected = self.tool.entity_class == class;
                        let help =
                            self.schema.get(&class).map(|s| s.help.clone()).unwrap_or_default();
                        let item = ui.selectable_label(selected, &class);
                        let item = if help.is_empty() { item } else { item.on_hover_text(help) };
                        if item.clicked() {
                            self.tool.entity_class = class;
                        }
                    }
                });
            }
        });
    }

    // ---- the property inspector -----------------------------------------

    /// Write pending property edits into the document, as one undo step.
    /// The material picker: a grid of what the textures actually look like.
    ///
    /// A list of names is only usable by someone who already knows what every
    /// name looks like, which is nobody on their first level. The thumbnails
    /// are the same pixels the 3D pane draws with, so picking one is picking
    /// what you can see.
    fn material_browser(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("material").strong());
            if !self.material_filter.is_empty() && ui.small_button("x").clicked() {
                self.material_filter.clear();
            }
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.material_filter)
                .desired_width(f32::INFINITY)
                .hint_text("filter"),
        );

        let current = self.document.current_material.clone();
        let filter = self.material_filter.to_ascii_lowercase();
        let materials: Vec<String> = self
            .materials
            .iter()
            .filter(|m| filter.is_empty() || m.to_ascii_lowercase().contains(&filter))
            .cloned()
            .collect();

        // What is on the selection, so the picker shows where you already are.
        ui.label(
            RichText::new(&current)
                .monospace()
                .size(10.0)
                .color(draw::colors::SELECTED),
        );

        const CELL: f32 = 48.0;
        egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, false]).show(
            ui,
            |ui| {
                let columns = ((ui.available_width() + 4.0) / (CELL + 6.0)).floor().max(1.0) as usize;
                let mut picked = None;
                egui::Grid::new("materials").spacing([4.0, 4.0]).show(ui, |ui| {
                    for (index, material) in materials.iter().enumerate() {
                        let handle = self.thumbnail(ui.ctx(), material);
                        let selected = *material == current;
                        let image = egui::Image::new(&handle)
                            .fit_to_exact_size(egui::vec2(CELL, CELL))
                            .corner_radius(2.0);
                        let response = ui
                            .add(egui::ImageButton::new(image).selected(selected))
                            .on_hover_text(match self.textures.problem(material) {
                                Some(problem) => format!("{material}\n\n{problem}"),
                                None => material.clone(),
                            });
                        if response.clicked() { picked = Some(material.clone()); }
                        if index % columns == columns - 1 { ui.end_row(); }
                    }
                });
                if let Some(material) = picked {
                    self.document.current_material = material.clone();
                    if !self.document.selection.is_empty() {
                        let faces = self.document.apply_material();
                        self.status = format!("{material} on {faces} faces");
                    } else {
                        self.status = material;
                    }
                }
            },
        );
    }

    /// An egui texture for a material, built once.
    ///
    /// From a mid mip rather than the top one: a 48-pixel thumbnail of a
    /// 256-pixel texture is going to be scaled down anyway, and scaling down
    /// something already filtered looks like the texture rather than like
    /// aliasing.
    fn thumbnail(&mut self, ctx: &Context, material: &str) -> egui::TextureHandle {
        if let Some(handle) = self.thumbnails.get(material) { return handle.clone() }

        let image = match self.textures.get(&self.vfs, material) {
            Some(texture) => {
                let level = texture.level(texture.mips.len().saturating_sub(2));
                egui::ColorImage {
                    size: [level.width as usize, level.height as usize],
                    pixels: level
                        .pixels
                        .iter()
                        .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                        .collect(),
                    source_size: egui::vec2(level.width as f32, level.height as f32),
                }
            }
            None => {
                // A flat swatch of the fallback colour, so a material with no
                // texture behind it is still a distinct thing to click.
                let [r, g, b] = TextureCache::fallback_colour(material);
                egui::ColorImage {
                    size: [2, 2],
                    pixels: vec![egui::Color32::from_rgb(r, g, b); 4],
                    source_size: egui::vec2(2.0, 2.0),
                }
            }
        };

        let handle = ctx.load_texture(
            format!("mat-{material}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.thumbnails.insert(material.to_string(), handle.clone());
        handle
    }

    fn commit_properties(&mut self) {
        let Some(edit) = self.properties.as_mut() else { return };
        if !edit.dirty { return }
        edit.dirty = false;
        let (id, rows, connections) = (edit.entity, edit.rows.clone(), edit.connections.clone());
        self.document.apply("edit properties", |doc| {
            if let Some(entity) = doc.find_entity_mut(id) {
                inspector::apply(entity, &rows);
                entity.connections = connections;
            }
        });
        let revision = self.document.revision();
        if let Some(edit) = self.properties.as_mut() { edit.revision = revision; }
    }

    /// Point the edit buffer at whatever is selected now.
    fn sync_properties(&mut self, id: Option<u32>) {
        let revision = self.document.revision();
        match self.properties.as_ref() {
            // Same entity, and nothing has changed underneath a buffer that is
            // not mid-edit: leave it alone. Rebuilding every frame would throw
            // away what is being typed.
            Some(edit) if Some(edit.entity) == id && (edit.dirty || edit.revision == revision) => {
                return;
            }
            None if id.is_none() => return,
            _ => {}
        }
        // Moving on commits what was in flight; an edit is not lost by
        // clicking somewhere else, which is what a person expects.
        self.commit_properties();
        let revision = self.document.revision();
        self.properties = id.and_then(|id| {
            let entity = self.document.find_entity(id)?;
            let spec = self.schema.get(entity.classname());
            Some(PropertyEdit {
                entity: id,
                rows: inspector::rows(spec, entity),
                connections: entity.connections.clone(),
                dirty: false,
                revision,
            })
        });
    }

    fn inspector(&mut self, ctx: &Context) {
        let selected: Vec<u32> = self.document.selection.entities.iter().copied().collect();
        self.sync_properties(selected.first().copied());

        egui::SidePanel::right("inspector").exact_width(320.0).show(ctx, |ui| {
            ui.add_space(4.0);

            // A face selection is what you are looking at when you have one,
            // so it comes first.
            if self.document.selected_face_count() > 0 {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    self.face_panel(ui);
                });
                return;
            }

            let Some(&id) = selected.first() else {
                self.no_entity_selected(ui);
                return;
            };

            let Some(entity) = self.document.find_entity(id) else { return };
            let classname = entity.classname().to_string();
            let is_brush_entity = entity.is_brush_entity();
            let spec = self.schema.get(&classname).cloned();

            ui.horizontal(|ui| {
                ui.label(RichText::new(&classname).monospace().strong());
                if spec.is_none() {
                    ui.label(RichText::new("(no definition)").color(egui::Color32::from_rgb(220, 160, 90)))
                        .on_hover_text(
                            "No .voiddef describes this class, so only the keys it \
                             already carries can be shown.",
                        );
                }
            });
            if let Some(help) = spec.as_ref().map(|s| s.help.as_str()).filter(|h| !h.is_empty()) {
                ui.label(RichText::new(help).size(11.0).weak());
            }
            if selected.len() > 1 {
                ui.label(
                    RichText::new(format!("{} entities selected -- editing the first", selected.len()))
                        .size(11.0)
                        .weak(),
                );
            }
            ui.separator();

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                self.property_rows(ui);
                ui.add_space(6.0);
                self.outputs_section(ui, id, spec.as_ref());

                if is_brush_entity {
                    ui.add_space(6.0);
                    ui.separator();
                    if ui.button("move brushes back to world").clicked() {
                        let n = self.document.untie_to_world();
                        self.status = format!("moved {n} brushes to the world");
                    }
                }
            });
        });
    }

    fn no_entity_selected(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("properties").strong());
        ui.label(match self.document.selection.solids.len() {
            0 => "nothing selected".to_string(),
            1 => "1 brush selected".to_string(),
            n => format!("{n} brushes selected"),
        });

        if self.document.selection.solids.is_empty() { return }
        ui.separator();
        ui.label("tie to entity");
        for class in self.brush_classes() {
            let help = self.schema.get(&class).map(|s| s.help.clone()).unwrap_or_default();
            let button = ui.button(&class);
            let button = if help.is_empty() { button } else { button.on_hover_text(help) };
            if button.clicked() {
                self.document.tie_to_entity(&class);
                self.status = format!("tied to {class}");
            }
        }
    }

    /// One widget per key the class defines, typed by the schema.
    fn property_rows(&mut self, ui: &mut egui::Ui) {
        let Some(edit) = self.properties.as_mut() else { return };
        if edit.rows.is_empty() {
            ui.label(RichText::new("this class has no settings").weak());
            return;
        }

        let materials = &self.materials;
        let models = &self.models;
        let mut commit = false;

        for (index, row) in edit.rows.iter_mut().enumerate() {
            let response = property_widget(ui, index, row, materials, models);
            if response.changed {
                edit.dirty = true;
            }
            // Discrete widgets are done the moment they change; text and
            // number fields are done when they are left.
            if response.finished {
                commit = true;
            }
        }

        ui.add_space(4.0);
        if ui.small_button("+ add a key the game does not define").clicked() {
            edit.rows.push(PropertyRow {
                key: format!("key{}", edit.rows.len()),
                label: format!("key{}", edit.rows.len()),
                kind: KeyKind::String,
                help: String::new(),
                choices: Vec::new(),
                default: String::new(),
                value: Some(String::new()),
                described: false,
            });
            edit.dirty = true;
            commit = true;
        }

        if commit { self.commit_properties(); }
    }

    /// The output wiring: which of this entity's outputs fires what, where.
    fn outputs_section(&mut self, ui: &mut egui::Ui, _id: u32, spec: Option<&void_entity::ClassSpec>) {
        ui.separator();
        ui.label(RichText::new("outputs").strong());

        let outputs: Vec<String> = spec
            .map(|s| s.outputs.iter().map(|o| o.name.clone()).collect())
            .unwrap_or_default();
        let targets = inspector::target_names(&self.document);
        // Worked out before the buffer is borrowed, because the answer depends
        // on the whole map rather than on this entity.
        let inputs_for: Vec<Vec<String>> = self
            .properties
            .as_ref()
            .map(|edit| {
                edit.connections
                    .iter()
                    .map(|c| inspector::inputs_for_target(&self.schema, &self.document, &c.target))
                    .collect()
            })
            .unwrap_or_default();

        let Some(edit) = self.properties.as_mut() else { return };
        if edit.connections.is_empty() {
            ui.label(RichText::new("nothing wired up").weak().size(11.0));
        }

        let mut remove = None;
        let mut commit = false;
        for (index, connection) in edit.connections.iter_mut().enumerate() {
            let empty = Vec::new();
            let inputs = inputs_for.get(index).unwrap_or(&empty);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let r = combo_or_text(ui, ("out", index), &mut connection.output, &outputs, 150.0);
                    edit.dirty |= r.changed;
                    commit |= r.finished;
                    if ui.small_button("x").on_hover_text("remove this output").clicked() {
                        remove = Some(index);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("to").size(11.0).weak());
                    let r = combo_or_text(ui, ("tgt", index), &mut connection.target, &targets, 190.0);
                    edit.dirty |= r.changed;
                    commit |= r.finished;
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("fire").size(11.0).weak());
                    let r = combo_or_text(ui, ("in", index), &mut connection.input, inputs, 180.0);
                    edit.dirty |= r.changed;
                    commit |= r.finished;
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("after").size(11.0).weak());
                    let r = ui.add(
                        egui::DragValue::new(&mut connection.delay)
                            .speed(0.05)
                            .range(0.0..=600.0)
                            .suffix(" s"),
                    );
                    edit.dirty |= r.changed();
                    commit |= r.drag_stopped() || r.lost_focus();

                    ui.label(RichText::new("param").size(11.0).weak());
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut connection.parameter).desired_width(90.0),
                    );
                    edit.dirty |= r.changed();
                    commit |= r.lost_focus();
                });
                let mut once = !connection.is_unlimited();
                if ui.checkbox(&mut once, RichText::new("only once").size(11.0)).changed() {
                    connection.times_to_fire = if once { 1 } else { -1 };
                    edit.dirty = true;
                    commit = true;
                }
            });
        }

        if let Some(index) = remove {
            edit.connections.remove(index);
            edit.dirty = true;
            commit = true;
        }

        if ui.button("+ add output").clicked() {
            let output = outputs.first().cloned().unwrap_or_else(|| "OnTrigger".to_string());
            let target = targets.first().cloned().unwrap_or_default();
            edit.connections.push(Connection::new(&output, &target, "Trigger"));
            edit.dirty = true;
            commit = true;
        }

        if commit { self.commit_properties(); }
    }

    fn status_bar(&mut self, ctx: &Context) {
        use void_math::units;

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.status).monospace().size(11.0));

                // Where the map lives, always visible. "Did that save?" is
                // not a question an editor should make anyone guess at, and
                // an unnamed map is worth saying outright rather than showing
                // as `untitled.voidmap` as though it were a file.
                ui.separator();
                let (file, colour) = match self.document.path.as_deref() {
                    Some(path) => {
                        let name = files::label(path, &self.content_root);
                        if self.document.is_modified() {
                            (format!("{name} *"), Some(egui::Color32::from_rgb(240, 200, 90)))
                        } else {
                            (name, None)
                        }
                    }
                    None => ("not saved".to_string(), Some(egui::Color32::from_rgb(240, 200, 90))),
                };
                let label = RichText::new(file).monospace().size(11.0);
                let label = match colour { Some(c) => label.color(c), None => label };
                ui.label(label).on_hover_text(match self.document.path.as_deref() {
                    Some(path) => format!(
                        "{}\n\nctrl-S saves. file -> rename... moves it, and takes anything \
                         compiled from it along.",
                        path.display()
                    ),
                    None => "This map has never been saved. ctrl-S will ask for a name."
                        .to_string(),
                });

                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "{} brushes  {} entities",
                        self.document.map.solid_count(),
                        self.document.map.entities.len(),
                    ))
                    .monospace()
                    .size(11.0),
                );
                ui.separator();
                ui.label(RichText::new(format!("grid {}", units::length_short(self.document.grid.size))).monospace().size(11.0))
                    .on_hover_text(format!(
                        "One grid square is {}.\nDistances in VoidEngine are void units: \
                         1 vu is one inch, a player is {} tall and runs at {}.",
                        units::length(self.document.grid.size),
                        units::length(units::PLAYER_HEIGHT),
                        units::speed(units::PLAYER_SPEED),
                    ));
                ui.separator();
                ui.label(RichText::new(self.viewports[self.active].kind.label()).monospace().size(11.0));

                // An editor with no content is nearly useless, and the way it
                // used to fail was silent: three hard-coded class names, flat
                // colours, and no clue why.
                ui.separator();
                let missing = self.textures.problem_count();
                let (text, colour) = if self.schema.is_empty() {
                    ("no entity classes".to_string(), Some(draw::colors::LEAK))
                } else if missing > 0 {
                    (
                        format!("{missing} materials unbuilt"),
                        Some(egui::Color32::from_rgb(240, 200, 90)),
                    )
                } else {
                    (format!("{} classes, {} materials", self.schema.len(), self.materials.len()), None)
                };
                let label = RichText::new(text).monospace().size(11.0);
                let label = match colour {
                    Some(c) => label.color(c),
                    None => label,
                };
                ui.label(label).on_hover_text(if missing > 0 {
                    format!(
                        "{}\n\n{missing} materials have no compiled texture behind them. \
                         Chisel builds the textures on startup and again before every \
                         compile, so these are ones that would not build -- check the log. \
                         view -> reload textures picks up a build done outside.",
                        self.content_note
                    )
                } else {
                    self.content_note.clone()
                });

                if let Some(bounds) = self.document.selection_bounds() {
                    let size = bounds.size();
                    let centre = bounds.center();
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "selection {} x {} x {} vu",
                            void_math::format_float(size.x),
                            void_math::format_float(size.y),
                            void_math::format_float(size.z),
                        ))
                        .monospace()
                        .size(11.0),
                    )
                    .on_hover_text(format!(
                        "{}\nheight {}\ncentred at {} {} {} vu",
                        units::size(size.x, size.y, size.z),
                        units::in_players(size.z),
                        void_math::format_float(centre.x),
                        void_math::format_float(centre.y),
                        void_math::format_float(centre.z),
                    ));
                }

                if !self.viewports[self.active].kind.is_2d() {
                    ui.separator();
                    ui.label(
                        RichText::new(format!("fly {}", units::length_short(self.fly_speed) + "/s"))
                            .monospace()
                            .size(11.0),
                    )
                    .on_hover_text("WASD to fly, Q and E for down and up, Shift to hurry, Alt to creep. Ctrl-wheel changes the speed.");
                }
            });
        });
    }

    fn compile_window(&mut self, ctx: &Context) {
        if self.show_tools_check {
            let mut open = true;
            egui::Window::new("tools").open(&mut open).show(ctx, |ui| {
                ui.label("Chisel runs the compilers as separate programs.");
                ui.separator();
                for (name, found) in available_tools() {
                    ui.label(
                        RichText::new(format!("{} {name}", if found { "found  " } else { "missing" }))
                            .monospace()
                            .color(if found { egui::Color32::LIGHT_GREEN } else { egui::Color32::LIGHT_RED }),
                    );
                }
                ui.separator();
                ui.label("Build them with: cargo build -p cleave -p umbra -p radiance -p void-runtime");
            });
            self.show_tools_check = open;
        }

        if !self.show_compile { return; }
        let mut open = true;
        // Collected inside the window and acted on after it, so the closure
        // does not need a second mutable borrow of the app.
        let mut start: Option<Option<Quality>> = None;
        egui::Window::new("compile")
            .open(&mut open)
            .default_size([560.0, 380.0])
            .show(ctx, |ui| {
                let settings = &mut self.compile_settings;
                ui.horizontal(|ui| {
                    ui.checkbox(&mut settings.run_vis, "visibility");
                    ui.checkbox(&mut settings.fast_vis, "fast");
                    ui.checkbox(&mut settings.run_lighting, "lighting");
                    ui.checkbox(&mut settings.run_after, "run after");
                });
                ui.horizontal(|ui| {
                    ui.add(egui::Slider::new(&mut settings.samples, 1..=4).text("samples"));
                    ui.add(egui::Slider::new(&mut settings.bounces, 0..=4).text("bounces"));
                });
                ui.checkbox(&mut settings.run_materials, "compile new materials first")
                    .on_hover_text(
                        "Runs Alchemy over the art tree, skipping anything already \
                         compiled. Without it, a material that has never been through \
                         Alchemy loads as the missing-material checkerboard.",
                    );
                ui.checkbox(&mut settings.ignore_leaks, "build even if the map leaks")
                    .on_hover_text(
                        "A map that leaks has no sealed inside, so visibility is near \
                         useless and light bleeds through walls. Cleave normally \
                         refuses to build one. With this it builds anyway and writes a \
                         .voidleak trace beside the map showing the way out.",
                    );

                ui.separator();
                ui.horizontal(|ui| {
                    let running = self.compile.as_ref().is_some_and(|j| !j.finished);
                    if ui
                        .add_enabled(!running, egui::Button::new("compile"))
                        .on_hover_text("Compile with exactly the settings above.")
                        .clicked()
                    {
                        start = Some(None);
                    }
                    if ui.add_enabled(!running, egui::Button::new("fast")).clicked() {
                        start = Some(Some(Quality::Fast));
                    }
                    if ui.add_enabled(!running, egui::Button::new("full")).clicked() {
                        start = Some(Some(Quality::Full));
                    }
                    if running { ui.spinner(); }
                    if let Some(path) =
                        self.compile.as_ref().filter(|j| j.finished && !j.failed).and_then(|j| j.output())
                    {
                        ui.label(RichText::new(format!("built {}", path.display())).size(11.0).weak());
                    }
                });
                ui.separator();

                if let Some(job) = &self.compile {
                    egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                        for message in &job.log {
                            let (text, color) = match message {
                                CompileMessage::Stage(s) => {
                                    (format!("--- {s} ---"), egui::Color32::LIGHT_BLUE)
                                }
                                CompileMessage::Line(l) => (l.clone(), egui::Color32::GRAY),
                                CompileMessage::Failed(e) => (format!("failed: {e}"), egui::Color32::LIGHT_RED),
                                CompileMessage::Finished(p) => {
                                    (format!("done: {}", p.display()), egui::Color32::LIGHT_GREEN)
                                }
                            };
                            ui.label(RichText::new(text).monospace().size(11.0).color(color));
                        }
                    });
                } else {
                    ui.label("nothing has been compiled yet");
                }
            });
        self.show_compile = open;
        match start {
            Some(Some(quality)) => self.compile_now(quality),
            Some(None) => {
                let settings = self.compile_settings.clone();
                self.start_compile(settings);
            }
            None => {}
        }
    }

    /// Compile at a quality preset, keeping every other setting the compile
    /// window is showing.
    ///
    /// The presets used to build a whole fresh `CompileSettings`, which threw
    /// away the leak checkbox on the way to the compiler -- so ticking it did
    /// nothing at all.
    fn compile_now(&mut self, quality: Quality) {
        self.compile_settings.set_quality(quality);
        let settings = self.compile_settings.clone();
        self.start_compile(settings);
    }

    fn start_compile(&mut self, settings: CompileSettings) {
        self.commit_properties();
        // The compilers read files, so the map has to be on disk first --
        // and compiling something other than what was saved would be a
        // genuinely confusing bug to chase.
        // A map with no name is named before it is compiled, rather than
        // being saved as `untitled` behind your back. The name is not
        // cosmetic: it is what `void +map <name>` loads, so a compile that
        // picks one for you is a compile whose output you have to go looking
        // for.
        let Some(path) = self.document.path.clone() else {
            self.begin_prompt(PromptKind::SaveAs);
            self.status = "name the map before compiling it".into();
            return;
        };
        if let Err(e) = self.document.save(Some(path.clone())) {
            self.status = format!("could not save before compiling: {e}");
            return;
        }

        let problems = self.document.problems();
        if !problems.is_empty() {
            self.status = format!("{} problems must be fixed first: {}", problems.len(), problems[0]);
            self.show_compile = true;
            return;
        }

        let mut settings = settings;
        settings.content_root = self.content_root.clone();
        self.compile_settings = settings.clone();
        self.compile = Some(CompileJob::start(&path, settings));
        self.show_compile = true;
        self.status = format!("compiling {}", path.display());
    }

    fn viewports_panel(&mut self, ctx: &Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_rect_before_wrap();

            if let Some(index) = self.maximised {
                self.viewport_ui(ui, index, available);
                return;
            }

            // Panes divide at a draggable fraction rather than at the middle.
            // Half of laying out a level is looking at one view closely and
            // the others only for reference.
            self.split.x = self.split.x.clamp(0.1, 0.9);
            self.split.y = self.split.y.clamp(0.1, 0.9);
            let cut = egui::pos2(
                available.min.x + available.width() * self.split.x,
                available.min.y + available.height() * self.split.y,
            );
            let half = SPLITTER * 0.5;
            let (l, r) = (available.min.x, available.max.x);
            let (t, b) = (available.min.y, available.max.y);
            let rects = [
                egui::Rect::from_min_max(egui::pos2(l, t), egui::pos2(cut.x - half, cut.y - half)),
                egui::Rect::from_min_max(egui::pos2(cut.x + half, t), egui::pos2(r, cut.y - half)),
                egui::Rect::from_min_max(egui::pos2(l, cut.y + half), egui::pos2(cut.x - half, b)),
                egui::Rect::from_min_max(egui::pos2(cut.x + half, cut.y + half), egui::pos2(r, b)),
            ];
            for (index, rect) in rects.into_iter().enumerate() {
                self.viewport_ui(ui, index, rect);
            }

            // Registered after the panes so they take the pointer first: a
            // splitter a viewport can steal the drag from is one that only
            // works some of the time.
            let vertical =
                egui::Rect::from_min_max(egui::pos2(cut.x - half, t), egui::pos2(cut.x + half, b));
            let horizontal =
                egui::Rect::from_min_max(egui::pos2(l, cut.y - half), egui::pos2(r, cut.y + half));
            for (bar, axis) in [(vertical, 0usize), (horizontal, 1usize)] {
                let response = ui.interact(bar, ui.id().with(("splitter", axis)), egui::Sense::drag());
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(if axis == 0 {
                        egui::CursorIcon::ResizeHorizontal
                    } else {
                        egui::CursorIcon::ResizeVertical
                    });
                }
                if response.dragged() {
                    let (delta, extent) = if axis == 0 {
                        (response.drag_delta().x, available.width())
                    } else {
                        (response.drag_delta().y, available.height())
                    };
                    self.split[axis] = (self.split[axis] + delta / extent.max(1.0)).clamp(0.1, 0.9);
                }
                let lit = response.hovered() || response.dragged();
                ui.painter().rect_filled(
                    bar,
                    0.0,
                    if lit { draw::colors::SELECTED } else { draw::colors::GRID_MAJOR },
                );
            }
        });
    }

    fn viewport_ui(&mut self, ui: &mut egui::Ui, index: usize, rect: egui::Rect) {
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        self.viewports[index].size = (rect.width(), rect.height());

        let painter = ui.painter_at(rect);
        let kind = self.viewports[index].kind;

        if kind.is_2d() {
            draw::draw_2d(&painter, rect, &self.viewports[index], &self.document, &self.tool, &self.leak);
        } else {
            self.draw_preview(ui, &painter, index, rect);
        }

        // The pane's label is a menu: any pane can show any view. Six flat
        // views exist, and a layout that could only ever reach three of them
        // was the reason to add the other three.
        let header =
            egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 3.0), egui::vec2(112.0, 18.0));
        let mut chosen = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(header), |ui| {
            ui.style_mut().visuals.override_text_color = Some(draw::colors::TEXT);
            egui::ComboBox::from_id_salt(("view", index))
                .selected_text(RichText::new(kind.label()).monospace().size(11.0))
                .width(104.0)
                .show_ui(ui, |ui| {
                    for option in crate::viewport::ViewportKind::all() {
                        if ui.selectable_label(option == kind, option.label()).clicked() {
                            chosen = Some(option);
                        }
                    }
                });
        });
        if let Some(option) = chosen {
            self.viewports[index].set_kind(option);
            self.status = format!("pane {} shows {}", index + 1, option.label());
        }

        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, if self.active == index {
                draw::colors::SELECTED
            } else {
                draw::colors::GRID_MAJOR
            }),
            egui::StrokeKind::Inside,
        );

        if response.hovered() { self.active = index; }
        self.viewport_input(index, rect, &response, ui);
    }


    /// The face editor: how the texture sits on the selected faces.
    ///
    /// The arithmetic is in [`crate::faces`] and tested there; this is the
    /// dial in front of it. Every control acts on the whole selection at once,
    /// as one undo step.
    fn face_panel(&mut self, ui: &mut egui::Ui) {
        use crate::faces::{self, Justify};

        let specs = self.document.selected_face_specs();
        let Some(first) = specs.first().cloned() else { return };
        let count = specs.len();

        ui.horizontal(|ui| {
            ui.label(RichText::new("face").strong());
            ui.label(
                RichText::new(if count == 1 {
                    "1 face".to_string()
                } else {
                    format!("{count} faces")
                })
                .size(11.0)
                .weak(),
            );
        });

        // Which material, and the texture behind it.
        let materials: std::collections::BTreeSet<&str> =
            specs.iter().map(|f| f.side.material.as_str()).collect();
        let shown = match materials.len() {
            1 => first.side.material.clone(),
            n => format!("{n} different materials"),
        };
        ui.label(RichText::new(&shown).monospace().size(11.0));

        let size = self
            .textures
            .get(&self.vfs, &first.side.material)
            .map(|t| (t.width(), t.height()))
            // The content may not be built. 256 is what the dev set is, and a
            // fit against the wrong size is better than no fit at all.
            .unwrap_or((256, 256));
        ui.label(
            RichText::new(format!("{} x {} texels", size.0, size.1)).size(10.0).weak(),
        );

        ui.horizontal(|ui| {
            let current = self.document.current_material.clone();
            if ui
                .button("apply current")
                .on_hover_text(format!("Put {current} on {count} face(s)"))
                .clicked()
            {
                let applied = self.document.apply_material();
                self.status = format!("{current} on {applied} faces");
            }
            if ui.button("pick up").on_hover_text("Take this face's material").clicked() {
                self.document.current_material = first.side.material.clone();
                self.status = format!("picked up {}", first.side.material);
            }
        });

        ui.separator();

        // A value shared by every selected face, or None when they differ.
        // Showing one face's number for six is how you overwrite five of them
        // by accident.
        let shared = |get: fn(&crate::document::FaceSpec) -> f32| -> Option<f32> {
            let first = get(&specs[0]);
            specs.iter().all(|f| (get(f) - first).abs() < 1e-4).then_some(first)
        };

        let mut edit: Option<(&'static str, FaceEdit)> = None;

        ui.label(RichText::new("scale (units per texel)").size(11.0).weak());
        ui.horizontal(|ui| {
            if let Some(value) = number(ui, "u", shared(|f| f.side.uaxis.scale), 0.01) {
                edit = Some(("scale", FaceEdit::ScaleU(value)));
            }
            if let Some(value) = number(ui, "v", shared(|f| f.side.vaxis.scale), 0.01) {
                edit = Some(("scale", FaceEdit::ScaleV(value)));
            }
        });

        ui.label(RichText::new("shift (texels)").size(11.0).weak());
        ui.horizontal(|ui| {
            if let Some(value) = number(ui, "u", shared(|f| f.side.uaxis.offset), 1.0) {
                edit = Some(("shift", FaceEdit::ShiftU(value)));
            }
            if let Some(value) = number(ui, "v", shared(|f| f.side.vaxis.offset), 1.0) {
                edit = Some(("shift", FaceEdit::ShiftV(value)));
            }
        });

        ui.horizontal(|ui| {
            ui.label(RichText::new("rotate").size(11.0).weak());
            for degrees in [-90.0f32, -15.0, 15.0, 90.0] {
                if ui.small_button(format!("{degrees:+.0}")).clicked() {
                    edit = Some(("rotate", FaceEdit::Rotate(degrees)));
                }
            }
        });

        ui.separator();
        ui.label(RichText::new("alignment").size(11.0).weak());
        ui.horizontal(|ui| {
            if ui
                .button("world")
                .on_hover_text(
                    "The default projection. Adjacent faces of a wall share a \
                     continuous texture.",
                )
                .clicked()
            {
                edit = Some(("align to world", FaceEdit::AlignWorld));
            }
            if ui
                .button("face")
                .on_hover_text(
                    "Axes in the face's own plane. A texture on a slope stops \
                     being foreshortened, at the cost of no longer lining up \
                     with its neighbours.",
                )
                .clicked()
            {
                edit = Some(("align to face", FaceEdit::AlignFace));
            }
            if ui
                .button("fit")
                .on_hover_text("Scale so the texture spans the face exactly once")
                .clicked()
            {
                edit = Some(("fit", FaceEdit::Justify(Justify::Fit)));
            }
        });

        ui.label(RichText::new("justify").size(11.0).weak());
        ui.horizontal(|ui| {
            for how in [
                Justify::Left,
                Justify::Right,
                Justify::Top,
                Justify::Bottom,
                Justify::Centre,
            ] {
                if ui.small_button(how.label()).clicked() {
                    edit = Some(("justify", FaceEdit::Justify(how)));
                }
            }
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("lightmap").size(11.0).weak());
            if let Some(value) = number(ui, "vu/luxel", shared(|f| f.side.lightmap_scale), 0.5) {
                edit = Some(("lightmap scale", FaceEdit::Lightmap(value)));
            }
        });

        ui.separator();
        if ui.button("clear face selection").clicked() {
            self.document.selection.faces.clear();
        }

        if let Some((label, what)) = edit {
            let changed = self.document.edit_faces(label, move |side, plane, winding| {
                match what {
                    FaceEdit::ScaleU(v) => faces::set_scale(side, v, side.vaxis.scale),
                    FaceEdit::ScaleV(v) => faces::set_scale(side, side.uaxis.scale, v),
                    FaceEdit::ShiftU(v) => faces::set_shift(side, v, side.vaxis.offset),
                    FaceEdit::ShiftV(v) => faces::set_shift(side, side.uaxis.offset, v),
                    FaceEdit::Rotate(d) => faces::rotate_by(side, plane, winding, d),
                    FaceEdit::AlignWorld => faces::align_to_world(side, plane),
                    FaceEdit::AlignFace => faces::align_to_face(side, plane),
                    FaceEdit::Justify(how) => faces::justify(side, winding, how, size),
                    FaceEdit::Lightmap(v) => side.lightmap_scale = v.clamp(1.0, 128.0),
                }
            });
            self.status = format!("{label} on {changed} faces");
        }
    }

    /// A click in a 3D pane.
    ///
    /// What it picks depends on the tool, because "the thing under the
    /// pointer" means different things: the select tool wants the brush or the
    /// entity that owns it, and the texture tool wants the one face you are
    /// looking at. Picking a brush when someone meant a face is how a whole
    /// room ends up wearing the same texture.
    fn pick_in_3d(&mut self, index: usize, x: f32, y: f32, ui: &egui::Ui) {
        let (origin, direction) = self.viewports[index].pick_ray(x, y);
        let (add, sample) = ui.input(|i| (i.modifiers.shift, i.modifiers.ctrl));

        if self.tool.kind == ToolKind::Texture {
            let Some((solid, side)) = crate::tools::pick_face_3d(&self.document, origin, direction)
            else {
                if !add { self.document.selection.faces.clear(); }
                return;
            };

            // Ctrl samples: the eyedropper every texture tool has, and worth
            // having now that the browser shows what you picked up.
            if sample {
                if let Some(material) = self
                    .document
                    .find_solid(solid)
                    .and_then(|s| s.sides.iter().find(|s| s.id == side))
                    .map(|s| s.material.clone())
                {
                    self.status = format!("picked up {material}");
                    self.document.current_material = material;
                }
                return;
            }

            if !add { self.document.selection.clear(); }
            self.document.selection.faces.insert((solid, side));
            if !add {
                // A plain click with the texture tool applies, which is what
                // the tool is named after. Shift only selects, for building up
                // a set of faces to edit together.
                let material = self.document.current_material.clone();
                self.document.apply_material();
                self.status = format!("{material} on 1 face");
            }
            return;
        }

        if !add { self.document.selection.clear(); }
        if let Some(id) = crate::tools::pick_solid_3d(&self.document, origin, direction) {
            // Clicking a brush that belongs to an entity selects the entity:
            // that is the thing a designer thinks of as the door. Same rule
            // the 2D views follow.
            let owner = self
                .document
                .map
                .all_solids()
                .find(|(_, s)| s.id == id)
                .map(|(e, _)| (e.id, e.is_brush_entity() && e.classname() != "worldspawn"));
            match owner {
                Some((entity, true)) => { self.document.selection.entities.insert(entity); }
                _ => { self.document.selection.solids.insert(id); }
            }
        }
    }

    /// Fly the 3D camera with the keyboard.
    ///
    /// WASD along the view, Q and E straight up and down, Shift to hurry and
    /// Alt to creep. Movement is per second rather than per frame, so it does
    /// not depend on how fast the pane happens to be redrawing.
    ///
    /// Only the pane under the pointer moves, and only while nothing is being
    /// typed into -- otherwise naming an entity `wasd_door` would fly the
    /// camera across the level.
    fn fly(&mut self, index: usize, response: &egui::Response, ui: &egui::Ui) {
        if ui.ctx().wants_keyboard_input() { return }
        if !(response.hovered() || response.dragged()) { return }

        let (forward, side, up, fast, slow, dt) = ui.input(|i| {
            let held = |k: Key| i.key_down(k);
            (
                (held(Key::W) as i32 - held(Key::S) as i32) as f32,
                (held(Key::D) as i32 - held(Key::A) as i32) as f32,
                (held(Key::E) as i32 - held(Key::Q) as i32) as f32,
                i.modifiers.shift,
                i.modifiers.alt,
                // Clamped: a frame that took a second (a compile finishing, a
                // window being dragged) must not teleport the camera.
                i.stable_dt.min(0.1),
            )
        });
        if forward == 0.0 && side == 0.0 && up == 0.0 { return }

        let speed = self.fly_speed * if fast { 2.5 } else { 1.0 } * if slow { 0.25 } else { 1.0 };
        let viewport = &mut self.viewports[index];
        viewport.eye += viewport.fly_step(forward, side, up, speed * dt);

        // Held keys produce no events, so without this the view moves one
        // frame and stops until the pointer twitches.
        ui.ctx().request_repaint();
    }

    /// Draw a 3D pane, rasterising it again only if it would look different.
    ///
    /// A software rasteriser is cheap but not free, and an editor spends most
    /// of its frames showing exactly what it showed last frame. Hashing what
    /// the image depends on turns a still view into a texture blit.
    fn draw_preview(&mut self, ui: &egui::Ui, painter: &egui::Painter, index: usize, rect: egui::Rect) {
        use std::hash::{Hash, Hasher};

        // Render at device resolution so the pane is not soft on a high-DPI
        // screen, but cap it: past a point this is work nobody can see.
        const MAX_EDGE: f32 = 1920.0;
        let scale = ui.ctx().pixels_per_point();
        let width = (rect.width() * scale).round().clamp(1.0, MAX_EDGE) as usize;
        let height = (rect.height() * scale).round().clamp(1.0, MAX_EDGE) as usize;

        let viewport = &self.viewports[index];
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.document.revision().hash(&mut hasher);
        // The selection changes the colours but not the map, so it is not
        // covered by the revision.
        let mut selected: Vec<u32> = self.document.selection.solids.iter().copied().collect();
        selected.extend(self.document.selection.entities.iter().copied());
        selected.sort_unstable();
        selected.hash(&mut hasher);
        for f in [
            viewport.eye.x, viewport.eye.y, viewport.eye.z,
            viewport.angles.pitch, viewport.angles.yaw, viewport.angles.roll,
            viewport.fov,
        ] {
            f.to_bits().hash(&mut hasher);
        }
        (width, height).hash(&mut hasher);
        (self.shading as u8).hash(&mut hasher);
        // A texture arriving after a failed load changes the picture.
        self.textures.len().hash(&mut hasher);
        let key = hasher.finish();

        let stale = self.previews[index].as_ref().is_none_or(|p| p.key != key);
        if stale {
            let viewport = viewport.clone();
            let vfs = &self.vfs;
            let cache = &mut self.textures;
            let mut resolve = move |material: &str| cache.get(vfs, material);
            let mut settings =
                raster::Settings { shading: self.shading, resolve: Some(&mut resolve) };
            let image = raster::render_with(
                &self.document,
                viewport.eye,
                viewport.angles.vectors(),
                viewport.fov,
                width,
                height,
                &mut settings,
            );
            let pixels: Vec<egui::Color32> = image
                .pixels
                .iter()
                .map(|p| egui::Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3]))
                .collect();
            let color_image = egui::ColorImage {
                size: [image.width, image.height],
                pixels,
                source_size: egui::vec2(image.width as f32, image.height as f32),
            };
            match self.previews[index].as_mut() {
                Some(preview) => {
                    preview.texture.set(color_image, egui::TextureOptions::LINEAR);
                    preview.key = key;
                }
                None => {
                    let texture = ui.ctx().load_texture(
                        format!("chisel-3d-{index}"),
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.previews[index] = Some(Preview { texture, key });
                }
            }
        }

        if let Some(preview) = &self.previews[index] {
            painter.image(
                preview.texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }

        self.draw_move_ghost(painter, index, rect);
        self.draw_leak_3d(painter, index, rect);
    }

    /// The leak trace, over the 3D image.
    ///
    /// Not depth-tested on purpose: the whole point is to follow it *through*
    /// the wall it escapes by.
    fn draw_leak_3d(&self, painter: &egui::Painter, index: usize, rect: egui::Rect) {
        if self.leak.is_empty() { return }
        let viewport = &self.viewports[index];
        let basis = viewport.angles.vectors();
        let aspect = rect.width() / rect.height().max(1.0);
        let half_y = (void_render::vertical_fov(viewport.fov, aspect) * 0.5).tan().max(1e-4);
        let half_x = half_y * aspect;

        let camera = draw::to_camera_space(&self.leak.points, viewport.eye, basis);
        let stroke = egui::Stroke::new(2.0, draw::colors::LEAK);
        for pair in camera.windows(2) {
            // Clip the segment to the near plane rather than dropping it: the
            // camera is usually inside the room the leak starts in.
            let (mut a, mut b) = (pair[0], pair[1]);
            if a.z < draw::NEAR && b.z < draw::NEAR { continue }
            if a.z < draw::NEAR {
                a = a + (b - a) * ((draw::NEAR - a.z) / (b.z - a.z));
            } else if b.z < draw::NEAR {
                b = b + (a - b) * ((draw::NEAR - b.z) / (a.z - b.z));
            }
            let project = |c: Vec3| {
                egui::pos2(
                    rect.center().x + (c.x / (c.z * half_x)) * rect.width() * 0.5,
                    rect.center().y - (c.y / (c.z * half_y)) * rect.height() * 0.5,
                )
            };
            painter.line_segment([project(a), project(b)], stroke);
        }
    }

    /// Outline where a dragged selection will land, over the 3D image.
    ///
    /// Stroked on top rather than rasterised into the pane, for two reasons:
    /// the cached image does not have to be thrown away on every mouse move,
    /// and a ghost that is hidden by the wall you are dragging something
    /// behind is a ghost that is no use.
    fn draw_move_ghost(&self, painter: &egui::Painter, index: usize, rect: egui::Rect) {
        let Some(drag) = &self.tool.drag else { return };
        if self.tool.kind != ToolKind::Select || !drag.is_dragging { return }

        let viewport = &self.viewports[index];
        let basis = viewport.angles.vectors();
        let aspect = rect.width() / rect.height().max(1.0);
        let half_y = (void_render::vertical_fov(viewport.fov, aspect) * 0.5).tan().max(1e-4);
        let half_x = half_y * aspect;
        let project = |camera: Vec3| -> egui::Pos2 {
            egui::pos2(
                rect.center().x + (camera.x / (camera.z * half_x)) * rect.width() * 0.5,
                rect.center().y - (camera.y / (camera.z * half_y)) * rect.height() * 0.5,
            )
        };

        let stroke = egui::Stroke::new(1.5, draw::colors::TOOL_PREVIEW);
        for polygon in draw::ghost_outline(&self.document, drag.delta()) {
            let camera = draw::to_camera_space(&polygon, viewport.eye, basis);
            // An entity marker is a line segment, not a loop; clipping a loop
            // is the wrong operation for it.
            let clipped = if polygon.len() > 2 {
                draw::clip_near_positions(&camera, draw::NEAR)
            } else if camera.iter().all(|p| p.z >= draw::NEAR) {
                camera
            } else {
                continue;
            };
            if clipped.len() < 2 { continue }
            let points: Vec<egui::Pos2> = clipped.iter().map(|p| project(*p)).collect();
            let last = if points.len() > 2 { points.len() } else { points.len() - 1 };
            for i in 0..last {
                painter.line_segment([points[i], points[(i + 1) % points.len()]], stroke);
            }
        }
    }

    fn viewport_input(
        &mut self,
        index: usize,
        rect: egui::Rect,
        response: &egui::Response,
        ui: &egui::Ui,
    ) {
        let local = |pos: egui::Pos2| (pos.x - rect.min.x, pos.y - rect.min.y);
        let kind = self.viewports[index].kind;

        // Scroll zooms a 2D pane and moves the 3D camera forward.
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if kind.is_2d() {
                    if let Some(pos) = response.hover_pos() {
                        let (x, y) = local(pos);
                        self.viewports[index].zoom_at(1.0 + scroll * 0.002, x, y);
                    }
                } else if ui.input(|i| i.modifiers.ctrl) {
                    // Ctrl-wheel sets how fast the camera flies, the way it
                    // does in every 3D application.
                    self.fly_speed = (self.fly_speed * (1.0 + scroll * 0.004)).clamp(16.0, 8192.0);
                    self.status = format!("fly speed {}", void_math::units::speed(self.fly_speed));
                } else if ui.input(|i| i.modifiers.ctrl) {
                    // Ctrl-wheel sets how fast the camera flies, as it does in
                    // every other 3D application.
                    self.fly_speed = (self.fly_speed * (1.0 + scroll * 0.004)).clamp(16.0, 8192.0);
                    self.status = format!("fly speed {}", void_math::units::speed(self.fly_speed));
                } else {
                    let forward = self.viewports[index].angles.forward();
                    self.viewports[index].eye += forward * scroll * 2.0;
                }
            }
        }

        // Middle-drag pans; right-drag looks around in 3D.
        if response.dragged_by(egui::PointerButton::Middle) {
            let delta = response.drag_delta();
            if kind.is_2d() {
                self.viewports[index].pan(delta.x, delta.y);
            } else {
                let viewport = &mut self.viewports[index];
                let basis = viewport.angles.vectors();
                viewport.eye += basis.right * -delta.x + basis.up * delta.y;
            }
        }
        if response.dragged_by(egui::PointerButton::Secondary) && !kind.is_2d() {
            let delta = response.drag_delta();
            let viewport = &mut self.viewports[index];
            viewport.angles.yaw -= delta.x * 0.25;
            viewport.angles.pitch += delta.y * 0.25;
            viewport.angles = viewport.angles.clamped_view();
        }

        if !kind.is_2d() {
            self.fly(index, response, ui);
        }

        if !kind.is_2d() {
            self.fly(index, response, ui);
        }

        if !kind.is_2d() {
            // The 3D pane picks but does not drag geometry: a drag there has
            // no unambiguous depth, and the orthographic views do have one.
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let (x, y) = local(pos);
                    self.pick_in_3d(index, x, y, ui);
                }
            }
            return;
        }

        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(pos) = response.interact_pointer_pos() {
                let (x, y) = local(pos);
                self.tool.press(&self.document, &self.viewports[index], x, y);
            }
        }
        if response.dragged_by(egui::PointerButton::Primary) {
            if let Some(pos) = response.interact_pointer_pos() {
                let (x, y) = local(pos);
                self.tool.drag_to(&self.document, &self.viewports[index], x, y);
            }
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) || response.clicked() {
            if self.tool.drag.is_none() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let (x, y) = local(pos);
                    self.tool.press(&self.document, &self.viewports[index], x, y);
                }
            }
            let add = ui.input(|i| i.modifiers.shift);
            if let Some(action) = self.tool.release(add) {
                let viewport = self.viewports[index].clone();
                draw::apply_action(&mut self.document, &viewport, action);
            }
        }
    }
}

/// Find the materials in a content tree.
///
/// Falls back to a built-in list when there is nothing to scan, so the editor
/// is usable before any content exists.
/// One thing the face panel can ask for.
///
/// A value rather than a closure so the panel can decide *what* to do while
/// the UI is being drawn and apply it afterwards, without holding a borrow of
/// the document across the whole panel.
#[derive(Clone, Copy, Debug)]
enum FaceEdit {
    ScaleU(f32),
    ScaleV(f32),
    ShiftU(f32),
    ShiftV(f32),
    Rotate(f32),
    AlignWorld,
    AlignFace,
    Justify(crate::faces::Justify),
    /// World units per luxel. Finer means a sharper shadow and a bigger
    /// lightmap, so it is a per-face choice rather than a map-wide one.
    Lightmap(f32),
}

/// A number that several faces may or may not agree on.
///
/// `None` means they differ, and the field shows nothing rather than one
/// face's value -- which is how you overwrite the other five by accident.
/// Returns the new value only when the edit is finished, so dragging does not
/// push an undo step per pixel.
fn number(ui: &mut egui::Ui, label: &str, shared: Option<f32>, speed: f32) -> Option<f32> {
    ui.label(RichText::new(label).size(11.0).weak());
    let mut value = shared.unwrap_or(0.0) as f64;
    let mut widget = egui::DragValue::new(&mut value).speed(speed as f64);
    if shared.is_none() {
        widget = widget.custom_formatter(|_, _| "--".to_string());
    }
    let response = ui.add(widget);
    let finished = response.drag_stopped() || response.lost_focus();
    (finished && response.changed()).then_some(value as f32)
}

/// What a property widget did this frame.
struct WidgetResult {
    /// The value in the buffer moved.
    changed: bool,
    /// The edit is complete and worth an undo step -- a discrete choice was
    /// made, or a text field was left.
    finished: bool,
}

/// Draw one property, with a widget suited to what the schema says it holds.
///
/// Typing a vector into a text box works, but it is not editing: the reason to
/// know a key is a colour or an angle is to hand a person the control they
/// would have reached for.
fn property_widget(
    ui: &mut egui::Ui,
    index: usize,
    row: &mut PropertyRow,
    materials: &[String],
    models: &[String],
) -> WidgetResult {
    let mut out = WidgetResult { changed: false, finished: false };

    ui.horizontal(|ui| {
        let label = ui.add(
            egui::Label::new(
                RichText::new(&row.label)
                    .monospace()
                    .size(11.0)
                    // An unset key is drawn faintly: it is showing the game's
                    // default, not a value anyone chose.
                    .color(if row.is_set() {
                        ui.visuals().text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    }),
            )
            .truncate(),
        );
        let mut hover = String::new();
        if !row.described {
            hover.push_str("Not defined by this class.\n");
        }
        if !row.help.is_empty() {
            hover.push_str(&row.help);
            hover.push('\n');
        }
        hover.push_str(&format!("key: {}  ({})", row.key, row.kind.name()));
        if !row.default.is_empty() {
            hover.push_str(&format!("\ndefault: {}", row.default));
        }
        label.on_hover_text(hover);
    });

    ui.horizontal(|ui| {
        let id = ("prop", index, row.key.as_str());
        match row.kind {
            KeyKind::Boolean => {
                let mut on = matches!(row.text().trim(), "1" | "true" | "yes");
                if ui.checkbox(&mut on, "").changed() {
                    row.value = Some(if on { "1".into() } else { "0".into() });
                    out.changed = true;
                    out.finished = true;
                }
            }
            KeyKind::Integer => {
                let mut v: i64 = row.text().trim().parse().unwrap_or(0);
                let r = ui.add(egui::DragValue::new(&mut v).speed(1.0));
                if r.changed() {
                    row.value = Some(v.to_string());
                    out.changed = true;
                }
                out.finished |= r.drag_stopped() || r.lost_focus();
            }
            KeyKind::Float => {
                let mut v: f64 = row.text().trim().parse().unwrap_or(0.0);
                let r = ui.add(egui::DragValue::new(&mut v).speed(0.5));
                if r.changed() {
                    row.value = Some(void_kv::format_float(v as f32));
                    out.changed = true;
                }
                out.finished |= r.drag_stopped() || r.lost_focus();
            }
            KeyKind::Vector | KeyKind::Angles => {
                let mut v = inspector::parse_vec3(row.text());
                let names = if row.kind == KeyKind::Angles {
                    ["pitch", "yaw", "roll"]
                } else {
                    ["x", "y", "z"]
                };
                let mut any = false;
                for (i, name) in names.iter().enumerate() {
                    let r = ui.add(
                        egui::DragValue::new(&mut v[i]).speed(1.0).prefix(format!("{name} ")),
                    );
                    any |= r.changed();
                    out.finished |= r.drag_stopped() || r.lost_focus();
                }
                if any {
                    row.value = Some(inspector::format_vec3(v));
                    out.changed = true;
                }
            }
            KeyKind::Color => {
                let (mut rgb, mut brightness) = inspector::parse_color(row.text());
                let mut any = ui.color_edit_button_srgb(&mut rgb).changed();
                let r = ui.add(
                    egui::DragValue::new(&mut brightness).speed(5.0).range(0.0..=100000.0),
                );
                any |= r.changed();
                out.finished |= r.drag_stopped() || r.lost_focus();
                if any {
                    row.value = Some(inspector::format_color(rgb, brightness));
                    out.changed = true;
                    out.finished = true;
                }
            }
            KeyKind::Choices => {
                let mut current = row.text().to_string();
                let label = row
                    .choices
                    .iter()
                    .find(|(v, _)| *v == current)
                    .map(|(_, l)| l.clone())
                    .unwrap_or_else(|| current.clone());
                let mut picked = None;
                egui::ComboBox::from_id_salt(id).selected_text(label).width(180.0).show_ui(
                    ui,
                    |ui| {
                        for (value, label) in &row.choices {
                            if ui.selectable_label(*value == current, label).clicked() {
                                picked = Some(value.clone());
                            }
                        }
                    },
                );
                if let Some(value) = picked {
                    current = value;
                    row.value = Some(current);
                    out.changed = true;
                    out.finished = true;
                }
            }
            KeyKind::Flags => {
                // A bit field is a row of checkboxes, because that is what it
                // is. Bits the schema does not name are preserved untouched.
                let mut bits: u32 = row.text().trim().parse().unwrap_or(0);
                let mut any = false;
                ui.vertical(|ui| {
                    for (value, label) in &row.choices {
                        let Ok(bit) = value.parse::<u32>() else { continue };
                        let mut on = bits & bit != 0;
                        if ui.checkbox(&mut on, RichText::new(label).size(11.0)).changed() {
                            if on { bits |= bit } else { bits &= !bit }
                            any = true;
                        }
                    }
                });
                if any {
                    row.value = Some(bits.to_string());
                    out.changed = true;
                    out.finished = true;
                }
            }
            KeyKind::Material | KeyKind::Model => {
                let options: &[String] = if row.kind == KeyKind::Material { materials } else { models };
                let mut text = row.text().to_string();
                let r = combo_or_text(ui, id, &mut text, options, 190.0);
                if r.changed {
                    row.value = Some(text);
                    out.changed = true;
                }
                out.finished |= r.finished;
            }
            KeyKind::String | KeyKind::TargetSource | KeyKind::TargetDestination => {
                let mut text = row.text().to_string();
                let r = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .desired_width(190.0)
                        .hint_text(row.default.as_str()),
                );
                if r.changed() {
                    row.value = Some(text);
                    out.changed = true;
                }
                out.finished |= r.lost_focus();
            }
        }

        // Clearing a key is how you go back to the game's default, so it needs
        // to be reachable. Only offered when there is something to clear.
        if row.is_set() && ui.small_button("clear").on_hover_text("Remove this key").clicked() {
            row.value = None;
            out.changed = true;
            out.finished = true;
        }
    });

    out
}

/// A combo box of known values that still accepts anything typed.
///
/// Both halves matter: the list is how a name is found, and the text box is
/// how a name that does not exist yet gets used -- wiring an output to an
/// entity you have not placed is a normal way to work.
fn combo_or_text(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + Clone,
    value: &mut String,
    options: &[String],
    width: f32,
) -> WidgetResult {
    let mut out = WidgetResult { changed: false, finished: false };
    let text_width = (width - 30.0).max(60.0);

    let response = ui.add(egui::TextEdit::singleline(value).desired_width(text_width));
    out.changed |= response.changed();
    out.finished |= response.lost_focus();

    if !options.is_empty() {
        let mut picked = None;
        egui::ComboBox::from_id_salt(id).selected_text("").width(24.0).show_ui(ui, |ui| {
            for option in options {
                if ui.selectable_label(option == value, option).clicked() {
                    picked = Some(option.clone());
                }
            }
        });
        if let Some(p) = picked {
            *value = p;
            out.changed = true;
            out.finished = true;
        }
    }
    out
}

/// Models offered where a key holds one.
fn scan_models(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let models = root.join("models");
    collect_by_extension(&models, &models, "voidmdl", &mut out);
    out.sort();
    out.dedup();
    out
}

fn scan_materials(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let materials = root.join("materials");
    collect_by_extension(&materials, &materials, "voidmat", &mut out);
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
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_by_extension(root, &path, extension, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(extension) {
            if let Ok(relative) = path.strip_prefix(root) {
                let name = relative.with_extension("");
                out.push(name.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

/// A starter map, so a fresh editor has something to look at rather than an
/// empty void with no sense of scale.
pub fn starter_document() -> Document {
    use void_math::Aabb;
    let mut document = Document::new();
    let t = 16.0;
    let (lo, hi, tall) = (0.0f32, 512.0f32, 256.0f32);
    for slab in [
        Aabb::new(Vec3::new(lo - t, lo - t, lo - t), Vec3::new(hi + t, hi + t, lo)),
        Aabb::new(Vec3::new(lo - t, lo - t, tall), Vec3::new(hi + t, hi + t, tall + t)),
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
        std::fs::write(materials.join("grid.voidmat"), "lit { }").unwrap();
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
        assert!(!app.schema.is_empty(), "the shipped definitions load: {}", app.status);

        let points = app.point_classes();
        assert!(points.iter().any(|c| c == "light_spot"));
        assert!(points.iter().any(|c| c == "math_counter"));
        assert!(!points.iter().any(|c| c == "func_door"), "a door is not placed as a point");

        let brushes = app.brush_classes();
        assert!(brushes.iter().any(|c| c == "func_door"));
        assert!(brushes.iter().any(|c| c == "trigger_multiple"));
        assert!(!brushes.iter().any(|c| c == "light"), "a light is not made of brushes");
        assert!(!brushes.iter().any(|c| c == "worldspawn"), "the world is not something to tie to");
    }

    #[test]
    fn the_menus_still_offer_something_without_a_content_tree() {
        let app = ChiselApp::new(std::path::PathBuf::from("/definitely/not/here"));
        assert!(app.schema.is_empty());
        assert!(!app.point_classes().is_empty(), "the entity tool must not be dead");
        assert!(!app.brush_classes().is_empty());
        assert!(app.status.contains("no .voiddef"), "and it says so: {}", app.status);
    }

    #[test]
    fn model_scanning_finds_compiled_models() {
        let dir = std::env::temp_dir().join(format!("chisel-models-{}", std::process::id()));
        let models = dir.join("models/props");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("crate.voidmdl"), "").unwrap();
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

        let prompt = app.prompt.as_ref().expect("ctrl-S on an unnamed map should ask for a name");
        assert_eq!(prompt.kind, PromptKind::SaveAs);
        assert!(app.document.path.is_none(), "nothing is written until it has a name");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_typed_name_saves_into_the_projects_maps_directory() {
        let (mut app, root) = app_in("save-named");
        app.save(None);
        app.prompt.as_mut().unwrap().name = "arena".into();
        assert!(app.confirm_prompt());

        let expected = root.join("maps/arena.voidmap");
        assert_eq!(app.document.path.as_deref(), Some(expected.as_path()));
        assert!(expected.is_file(), "the map is on disk");
        assert!(!app.document.is_modified(), "and no longer counts as unsaved");
        assert!(app.prompt.is_none());
        assert!(app.status.contains("maps/arena.voidmap"), "{}", app.status);

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
        app.save(Some(root.join("maps/arena.voidmap")));
        app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        assert!(app.document.is_modified());

        app.save(None);
        assert!(app.prompt.is_none(), "a map with a name is not asked about again");
        assert!(!app.document.is_modified());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renaming_moves_the_map_and_what_was_compiled_from_it() {
        let (mut app, root) = app_in("rename");
        app.save(Some(root.join("maps/old.voidmap")));
        std::fs::write(root.join("maps/old.voidbsp"), b"compiled").unwrap();

        app.begin_prompt(PromptKind::Rename);
        assert_eq!(app.prompt.as_ref().unwrap().name, "old.voidmap", "filled in with the current name");
        app.prompt.as_mut().unwrap().name = "new".into();
        assert!(app.confirm_prompt());

        assert_eq!(app.document.path.as_deref(), Some(root.join("maps/new.voidmap").as_path()));
        assert!(!root.join("maps/old.voidmap").exists());
        assert!(root.join("maps/new.voidbsp").is_file(), "the compiled map came too");
        assert!(app.status.contains("renamed"), "{}", app.status);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renaming_over_an_existing_map_is_refused() {
        let (mut app, root) = app_in("rename-clash");
        app.save(Some(root.join("maps/old.voidmap")));
        std::fs::write(root.join("maps/taken.voidmap"), "someone else's work").unwrap();

        app.begin_prompt(PromptKind::Rename);
        app.prompt.as_mut().unwrap().name = "taken".into();
        assert!(!app.confirm_prompt(), "the prompt stays open");

        assert!(app.prompt.as_ref().unwrap().error.as_ref().unwrap().contains("already exists"));
        assert_eq!(
            std::fs::read_to_string(root.join("maps/taken.voidmap")).unwrap(),
            "someone else's work",
            "and the map that was there is untouched"
        );
        assert!(root.join("maps/old.voidmap").is_file());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renaming_a_map_that_was_never_saved_just_saves_it() {
        let (mut app, root) = app_in("rename-unsaved");
        app.begin_prompt(PromptKind::Rename);
        app.prompt.as_mut().unwrap().name = "first".into();
        assert!(app.confirm_prompt());

        assert!(root.join("maps/first.voidmap").is_file());
        assert!(!app.document.is_modified());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renaming_carries_unsaved_changes_to_the_new_name() {
        let (mut app, root) = app_in("rename-dirty");
        app.save(Some(root.join("maps/old.voidmap")));
        let before = std::fs::read_to_string(root.join("maps/old.voidmap")).unwrap();
        app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));

        app.begin_prompt(PromptKind::Rename);
        app.prompt.as_mut().unwrap().name = "new".into();
        assert!(app.confirm_prompt());

        let after = std::fs::read_to_string(root.join("maps/new.voidmap")).unwrap();
        assert_ne!(after, before, "the file under the new name is the map as it stands");
        assert!(!app.document.is_modified());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opening_a_map_with_unsaved_changes_asks_first() {
        let (mut app, root) = app_in("discard");
        app.save(Some(root.join("maps/arena.voidmap")));
        let other = root.join("maps/other.voidmap");
        std::fs::copy(root.join("maps/arena.voidmap"), &other).unwrap();
        app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));

        app.discard_or_ask(Discarding::Open(other.clone()));
        assert_eq!(app.discarding, Some(Discarding::Open(other.clone())));
        assert_eq!(app.document.path.as_deref(), Some(root.join("maps/arena.voidmap").as_path()));

        // Answering the question goes through with it.
        app.discarding = None;
        app.discard_now(Discarding::Open(other.clone()));
        assert_eq!(app.document.path.as_deref(), Some(other.as_path()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_saved_map_is_replaced_without_a_question() {
        let (mut app, root) = app_in("no-question");
        app.save(Some(root.join("maps/arena.voidmap")));

        app.discard_or_ask(Discarding::New);
        assert!(app.discarding.is_none(), "nothing would be lost, so nothing is asked");
        assert!(app.document.path.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn compiling_an_unnamed_map_asks_for_a_name_rather_than_inventing_one() {
        let (mut app, root) = app_in("compile-unnamed");
        app.compile_now(Quality::Fast);

        assert!(app.compile.is_none(), "nothing is compiled yet");
        assert_eq!(app.prompt.as_ref().map(|p| p.kind), Some(PromptKind::SaveAs));
        assert!(!root.join("maps/untitled.voidmap").exists(), "and no `untitled` is left behind");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_maps_offered_to_open_are_the_ones_in_the_project() {
        let (mut app, root) = app_in("map-list");
        app.save(Some(root.join("maps/arena.voidmap")));
        app.save(Some(root.join("maps/lobby.voidmap")));

        let names: Vec<String> = files::maps_in(&root)
            .iter()
            .map(|p| files::label(p, &root))
            .collect();
        assert_eq!(names, vec!["maps/arena.voidmap", "maps/lobby.voidmap"]);

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
        assert!(!output.shapes.is_empty(), "a frame with nothing in it is not a frame");
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
        assert_eq!(app.discarding, Some(Discarding::New), "asked, and still waiting");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_window_title_names_the_map_and_whether_it_is_saved() {
        let (mut app, root) = app_in("title");
        assert_eq!(app.window_title(), "untitled -- Chisel");

        app.save(Some(root.join("maps/arena.voidmap")));
        assert_eq!(app.window_title(), "arena.voidmap -- Chisel");

        app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        assert_eq!(app.window_title(), "arena.voidmap * -- Chisel");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_modal_takes_the_keyboard_from_the_shortcuts_behind_it() {
        let (mut app, root) = app_in("modal-keys");
        app.save(Some(root.join("maps/arena.voidmap")));
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
        assert_eq!(app.status, "nothing has happened yet", "the shortcut behind the modal fired");

        let _ = std::fs::remove_dir_all(&root);
    }
}
