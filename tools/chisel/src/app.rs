// SPDX-License-Identifier: LGPL-3.0-or-later
//! The editor's user interface.
//!
//! Hammer's layout, because it is the right one for the job: a toolbar down
//! the left, an inspector on the right, a status bar along the bottom, and
//! four viewports filling the middle -- 3D, top, front and side.
//!
//! Everything here turns a gesture into a call on [`Document`] and draws the
//! result. The decisions all live in the modules it calls.

use crate::compile::{CompileJob, CompileMessage, CompileSettings, available_tools};
use crate::document::Document;
use crate::inspector::{self, PropertyRow};
use crate::tools::{Tool, ToolKind};
use crate::viewport::Viewport;
use crate::{classes, draw, raster};
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
    /// The rasterised 3D panes, kept until something they depend on moves.
    previews: [Option<Preview>; 4],
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
            previews: [const { None }; 4],
            properties: None,
        }
    }

    /// Class names for the entity tool, from the game's definitions.
    pub fn point_classes(&self) -> Vec<String> {
        let names = self.schema.names_of_kind(ClassKind::Point);
        if names.is_empty() {
            return FALLBACK_CLASSES.iter().map(|s| s.to_string()).collect();
        }
        names.into_iter().map(str::to_string).collect()
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

    fn save(&mut self, path: Option<PathBuf>) {
        self.commit_properties();
        match self.document.save(path) {
            Ok(path) => self.status = format!("saved {}", path.display()),
            Err(e) => self.status = format!("could not save: {e}"),
        }
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
            job.poll();
            if job.finished && !self.show_compile {
                self.status = if job.failed { "compile failed".into() } else { "compile finished".into() };
            }
            ctx.request_repaint();
        }

        self.shortcuts(ctx);
        self.menu_bar(ctx);
        self.toolbar(ctx);
        self.inspector(ctx);
        self.status_bar(ctx);
        self.compile_window(ctx);
        self.viewports_panel(ctx);
    }

    fn shortcuts(&mut self, ctx: &Context) {
        // A property field has to be able to contain the characters these
        // shortcuts use. Without this guard, typing `1` into a keyvalue
        // switches tools and typing `[` changes the grid size -- which is
        // exactly the kind of thing that makes an editor feel haunted.
        let typing = ctx.wants_keyboard_input();

        enum Action {
            Undo,
            Redo,
            Save,
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
                Action::Compile => self.start_compile(CompileSettings::fast()),
            }
        }
    }

    fn menu_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("file", |ui| {
                    if ui.button("new").clicked() {
                        self.document = Document::new();
                        self.status = "new map".into();
                        ui.close();
                    }
                    if ui.button("save").clicked() { self.save(None); ui.close(); }
                    if ui.button("save as maps/untitled.voidmap").clicked() {
                        let path = self.content_root.join("maps/untitled.voidmap");
                        self.save(Some(path));
                        ui.close();
                    }
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
                        self.start_compile(CompileSettings::fast());
                        ui.close();
                    }
                    if ui.button("compile (full)").clicked() {
                        self.start_compile(CompileSettings::full());
                        ui.close();
                    }
                    ui.separator();
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
                ui.label(void_kv::format_float(self.document.grid.size));
                if ui.small_button("]").clicked() { self.document.grid.coarser(); }
            });

            ui.separator();
            ui.label(RichText::new("material").strong());
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                let materials = self.materials.clone();
                for material in &materials {
                    let selected = self.document.current_material == *material;
                    if ui.selectable_label(selected, material).clicked() {
                        self.document.current_material = material.clone();
                        if !self.document.selection.is_empty() {
                            self.document.apply_material();
                        }
                    }
                }
            });

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
    fn commit_properties(&mut self) {
        let Some(edit) = self.properties.as_mut() else { return };
        if !edit.dirty { return }
        edit.dirty = false;
        let (id, rows) = (edit.entity, edit.rows.clone());
        self.document.apply("edit properties", |doc| {
            if let Some(entity) = doc.find_entity_mut(id) {
                inspector::apply(entity, &rows);
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
    fn outputs_section(&mut self, ui: &mut egui::Ui, id: u32, spec: Option<&void_entity::ClassSpec>) {
        ui.separator();
        ui.label(RichText::new("outputs").strong());

        let connections = match self.document.find_entity(id) {
            Some(e) => e.connections.clone(),
            None => return,
        };
        let outputs: Vec<String> = spec
            .map(|s| s.outputs.iter().map(|o| o.name.clone()).collect())
            .unwrap_or_default();
        let targets = inspector::target_names(&self.document);

        if connections.is_empty() {
            ui.label(RichText::new("nothing wired up").weak().size(11.0));
        }

        let mut edited: Option<(usize, Option<Connection>)> = None;
        for (index, connection) in connections.iter().enumerate() {
            let mut c = connection.clone();
            let mut changed = false;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    changed |= combo_or_text(ui, ("out", id, index), &mut c.output, &outputs, 110.0);
                    if ui.small_button("x").on_hover_text("remove this output").clicked() {
                        edited = Some((index, None));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("to").size(11.0).weak());
                    changed |= combo_or_text(ui, ("tgt", id, index), &mut c.target, &targets, 150.0);
                });
                let inputs = inspector::inputs_for_target(&self.schema, &self.document, &c.target);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("fire").size(11.0).weak());
                    changed |= combo_or_text(ui, ("in", id, index), &mut c.input, &inputs, 150.0);
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("after").size(11.0).weak());
                    changed |= ui
                        .add(egui::DragValue::new(&mut c.delay).speed(0.05).range(0.0..=600.0).suffix(" s"))
                        .changed();
                    ui.label(RichText::new("param").size(11.0).weak());
                    changed |= ui.add(egui::TextEdit::singleline(&mut c.parameter).desired_width(80.0)).changed();
                });
                ui.horizontal(|ui| {
                    let mut once = !c.is_unlimited();
                    if ui.checkbox(&mut once, RichText::new("only once").size(11.0)).changed() {
                        c.times_to_fire = if once { 1 } else { -1 };
                        changed = true;
                    }
                });
            });
            if changed && edited.is_none() {
                edited = Some((index, Some(c)));
            }
        }

        if let Some((index, replacement)) = edited {
            self.document.apply(
                if replacement.is_some() { "edit output" } else { "remove output" },
                |doc| {
                    if let Some(entity) = doc.find_entity_mut(id) {
                        match replacement {
                            Some(c) => entity.connections[index] = c,
                            None => { entity.connections.remove(index); }
                        }
                    }
                },
            );
        }

        if ui.button("+ add output").clicked() {
            let output = outputs.first().cloned().unwrap_or_else(|| "OnTrigger".to_string());
            let target = targets.first().cloned().unwrap_or_default();
            self.document.apply("add output", |doc| {
                if let Some(entity) = doc.find_entity_mut(id) {
                    entity.connect(Connection::new(&output, &target, "Trigger"));
                }
            });
        }
    }

    fn status_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.status).monospace().size(11.0));
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "{} brushes  {} entities  grid {}  {}",
                        self.document.map.solid_count(),
                        self.document.map.entities.len(),
                        void_kv::format_float(self.document.grid.size),
                        self.viewports[self.active].kind.label(),
                    ))
                    .monospace()
                    .size(11.0),
                );
                if let Some(bounds) = self.document.selection_bounds() {
                    let size = bounds.size();
                    ui.separator();
                    ui.label(
                        RichText::new(format!("selection {:.0} x {:.0} x {:.0}", size.x, size.y, size.z))
                            .monospace()
                            .size(11.0),
                    );
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
                ui.checkbox(&mut settings.ignore_leaks, "build even if the map leaks");
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
    }

    fn start_compile(&mut self, settings: CompileSettings) {
        self.commit_properties();
        // The compilers read files, so the map has to be on disk first --
        // and compiling something other than what was saved would be a
        // genuinely confusing bug to chase.
        let path = match self.document.path.clone() {
            Some(path) => path,
            None => self.content_root.join("maps/untitled.voidmap"),
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

            let half = egui::vec2(available.width() * 0.5, available.height() * 0.5);
            let rects = [
                egui::Rect::from_min_size(available.min, half),
                egui::Rect::from_min_size(available.min + egui::vec2(half.x, 0.0), half),
                egui::Rect::from_min_size(available.min + egui::vec2(0.0, half.y), half),
                egui::Rect::from_min_size(available.min + half, half),
            ];
            for (index, rect) in rects.into_iter().enumerate() {
                self.viewport_ui(ui, index, rect.shrink(1.0));
            }
        });
    }

    fn viewport_ui(&mut self, ui: &mut egui::Ui, index: usize, rect: egui::Rect) {
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        self.viewports[index].size = (rect.width(), rect.height());

        let painter = ui.painter_at(rect);
        let kind = self.viewports[index].kind;

        if kind.is_2d() {
            draw::draw_2d(&painter, rect, &self.viewports[index], &self.document, &self.tool);
        } else {
            self.draw_preview(ui, &painter, index, rect);
        }

        painter.text(
            rect.min + egui::vec2(6.0, 4.0),
            egui::Align2::LEFT_TOP,
            kind.label(),
            egui::FontId::monospace(11.0),
            draw::colors::TEXT,
        );
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
        let key = hasher.finish();

        let stale = self.previews[index].as_ref().is_none_or(|p| p.key != key);
        if stale {
            let image = raster::render(
                &self.document,
                viewport.eye,
                viewport.angles.vectors(),
                viewport.fov,
                width,
                height,
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
            // The 3D pane picks but does not edit: dragging geometry is done
            // in the orthographic views, where a drag is unambiguous.
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let (x, y) = local(pos);
                    let (origin, direction) = self.viewports[index].pick_ray(x, y);
                    let add = ui.input(|i| i.modifiers.shift);
                    if !add { self.document.selection.clear(); }
                    if let Some(id) = crate::tools::pick_solid_3d(&self.document, origin, direction) {
                        self.document.selection.solids.insert(id);
                    }
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
                if combo_or_text(ui, id, &mut text, options, 190.0) {
                    row.value = Some(text);
                    out.changed = true;
                    out.finished = true;
                }
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
    id: impl std::hash::Hash,
    value: &mut String,
    options: &[String],
    width: f32,
) -> bool {
    let mut changed = false;
    let text_width = (width - 30.0).max(60.0);
    changed |= ui
        .add(egui::TextEdit::singleline(value).desired_width(text_width))
        .lost_focus();
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
            changed = true;
        }
    }
    changed
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
        assert_eq!(panes[3].kind, ViewportKind::Side);
    }
}
