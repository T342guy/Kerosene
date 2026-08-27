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
use crate::draw;
use crate::tools::{Tool, ToolKind};
use crate::viewport::Viewport;
use egui::{Context, Key, Modifiers, RichText};
use std::path::PathBuf;
use void_map::Connection;
use void_math::Vec3;

/// Entity classes offered in the entity tool.
///
/// A real editor reads these from the game's entity definitions; this is the
/// set the sample game implements.
const ENTITY_CLASSES: &[&str] = &[
    "info_player_start",
    "light",
    "light_spot",
    "light_environment",
    "func_door",
    "func_brush",
    "func_detail",
    "trigger_multiple",
    "trigger_once",
    "logic_relay",
    "logic_auto",
    "logic_timer",
    "math_counter",
    "point_message",
];

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
    pub content_root: PathBuf,
}

impl ChiselApp {
    pub fn new(content_root: PathBuf) -> ChiselApp {
        let materials = scan_materials(&content_root);
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
            status: "ready".to_string(),
            materials,
            content_root,
        }
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
        ctx.input_mut(|i| {
            let ctrl = Modifiers::COMMAND;

            if i.consume_key(ctrl, Key::Z) {
                if let Some(label) = self.document.undo() {
                    self.status = format!("undid {label}");
                }
            }
            if i.consume_key(ctrl | Modifiers::SHIFT, Key::Z) || i.consume_key(ctrl, Key::Y) {
                if let Some(label) = self.document.redo() {
                    self.status = format!("redid {label}");
                }
            }
            if i.consume_key(ctrl, Key::S) { self.save(None); }
            if i.consume_key(Modifiers::NONE, Key::Delete) || i.consume_key(Modifiers::NONE, Key::Backspace) {
                let n = self.document.delete_selection();
                if n > 0 { self.status = format!("deleted {n}"); }
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                self.tool.cancel();
                self.document.selection.clear();
            }

            // Tool shortcuts, as Hammer numbers them.
            for (index, kind) in ToolKind::all().into_iter().enumerate() {
                let key = match index {
                    0 => Key::Num1,
                    1 => Key::Num2,
                    2 => Key::Num3,
                    _ => Key::Num4,
                };
                if i.consume_key(Modifiers::NONE, key) { self.tool.set_kind(kind); }
            }

            // The grid keys every brush editor has used for thirty years.
            if i.consume_key(Modifiers::NONE, Key::OpenBracket) { self.document.grid.finer(); }
            if i.consume_key(Modifiers::NONE, Key::CloseBracket) { self.document.grid.coarser(); }
            if i.consume_key(Modifiers::NONE, Key::F9) { self.start_compile(CompileSettings::fast()); }
        });
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
                    for class in ENTITY_CLASSES {
                        let selected = self.tool.entity_class == *class;
                        if ui.selectable_label(selected, *class).clicked() {
                            self.tool.entity_class = (*class).to_string();
                        }
                    }
                });
            }
        });
    }

    fn inspector(&mut self, ctx: &Context) {
        egui::SidePanel::right("inspector").exact_width(280.0).show(ctx, |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("properties").strong());

            let selected: Vec<u32> = self.document.selection.entities.iter().copied().collect();
            let Some(&id) = selected.first() else {
                ui.label(match self.document.selection.solids.len() {
                    0 => "nothing selected".to_string(),
                    1 => "1 brush selected".to_string(),
                    n => format!("{n} brushes selected"),
                });

                if !self.document.selection.solids.is_empty() {
                    ui.separator();
                    ui.label("tie to entity");
                    for class in ["func_door", "func_brush", "func_detail", "trigger_multiple"] {
                        if ui.button(class).clicked() {
                            self.document.tie_to_entity(class);
                            self.status = format!("tied to {class}");
                        }
                    }
                }
                return;
            };

            if selected.len() > 1 {
                ui.label(format!("{} entities selected", selected.len()));
                ui.separator();
            }

            let Some(entity) = self.document.find_entity(id) else { return };
            let classname = entity.classname().to_string();
            let mut properties: Vec<(String, String)> = entity.properties.clone();
            let connections = entity.connections.clone();
            let is_brush_entity = entity.is_brush_entity();

            ui.label(RichText::new(&classname).monospace().strong());
            ui.separator();

            let mut changed = false;
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for (key, value) in properties.iter_mut() {
                    if key == "classname" { continue; }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(key.as_str()).monospace());
                        if ui.text_edit_singleline(value).changed() { changed = true; }
                    });
                }
            });

            if ui.button("+ add property").clicked() {
                properties.push(("key".into(), "value".into()));
                changed = true;
            }

            if changed {
                self.document.apply("edit properties", |doc| {
                    if let Some(entity) = doc.find_entity_mut(id) {
                        entity.properties = properties;
                    }
                });
            }

            ui.separator();
            ui.label(RichText::new("outputs").strong());
            if connections.is_empty() {
                ui.label("none");
            }
            for connection in &connections {
                ui.label(
                    RichText::new(format!(
                        "{} -> {}:{}{}",
                        connection.output,
                        connection.target,
                        connection.input,
                        if connection.delay > 0.0 {
                            format!(" +{:.2}s", connection.delay)
                        } else {
                            String::new()
                        }
                    ))
                    .monospace()
                    .size(11.0),
                );
            }
            if ui.button("+ add output").clicked() {
                self.document.apply("add output", |doc| {
                    if let Some(entity) = doc.find_entity_mut(id) {
                        entity.connect(Connection::new("OnTrigger", "target", "Trigger"));
                    }
                });
            }

            if is_brush_entity {
                ui.separator();
                if ui.button("move brushes back to world").clicked() {
                    let n = self.document.untie_to_world();
                    self.status = format!("moved {n} brushes to the world");
                }
            }
        });
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
            draw::draw_3d(&painter, rect, &self.viewports[index], &self.document);
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
fn scan_materials(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let materials = root.join("materials");
    collect_materials(&materials, &materials, &mut out);
    out.sort();
    out.dedup();
    if out.is_empty() {
        out = FALLBACK_MATERIALS.iter().map(|s| s.to_string()).collect();
    }
    out
}

fn collect_materials(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_materials(root, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("voidmat") {
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

    #[test]
    fn the_default_layout_is_hammers_four_panes() {
        let panes = Viewport::default_layout();
        assert_eq!(panes[0].kind, ViewportKind::Perspective);
        assert_eq!(panes[1].kind, ViewportKind::Top);
        assert_eq!(panes[2].kind, ViewportKind::Front);
        assert_eq!(panes[3].kind, ViewportKind::Side);
    }
}
