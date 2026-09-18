// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Compiling: the settings dialog and what happens when a compile ends.

use super::*;
use kerosene_ui::output::{Level, Line};
use kerosene_ui::theme::{self, colors, icons};
use kerosene_ui::widgets;

impl ChiselApp {
    /// Pick up whatever the compile left behind.
    ///
    /// Chiefly the leak trace: Cleave writes one beside the map when the world
    /// is not sealed, and it is only worth writing if something draws it. A
    /// clean compile clears the last one, so a fixed leak stops being shown.
    pub(super) fn after_compile(&mut self) {
        let Some(job) = &self.compile else { return };
        let failed = job.failed;
        let map = job.output().map(|p| p.to_path_buf()).or_else(|| {
            self.document
                .path
                .clone()
                .map(|p| p.with_extension("kerobsp"))
        });

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
        self.model_previews.clear();
        self.materials = scan_materials(&self.content_root);
        self.models = scan_models(&self.content_root);

        self.status = if failed {
            "compile failed".into()
        } else if let Some(at) = self.leak.origin() {
            format!(
                "compiled, but the map LEAKS -- follow the red line from {} {} {}",
                kerosene_math::format_float(at.x),
                kerosene_math::format_float(at.y),
                kerosene_math::format_float(at.z),
            )
        } else {
            "compile finished".into()
        };
    }

    /// The compile log, as the toolset's output panel wants it.
    ///
    /// The log used to be in the compile window, which meant the window
    /// covered the map while it compiled and the map was gone when the window
    /// closed. The toolset draws every job's log in one panel at the bottom;
    /// this is what it reads.
    pub fn output_lines(&self) -> Vec<Line<'_>> {
        let Some(job) = &self.compile else {
            return Vec::new();
        };
        job.log
            .iter()
            .map(|message| match message {
                CompileMessage::Stage(s) => Line::new(Level::Stage, format!("--- {s} ---")),
                CompileMessage::Line(l) => Line::classified(l),
                CompileMessage::Failed(e) => Line::new(Level::Error, format!("failed: {e}")),
                CompileMessage::Finished(p) => {
                    Line::new(Level::Ok, format!("done: {}", p.display()))
                }
            })
            .collect()
    }

    /// Whether a compile is running now.
    pub fn compiling(&self) -> bool {
        self.compile.as_ref().is_some_and(|j| !j.finished)
    }

    /// `Some(true)` when the last compile failed, `Some(false)` when it
    /// finished, `None` when there has not been one.
    pub fn compile_failed(&self) -> Option<bool> {
        self.compile
            .as_ref()
            .filter(|j| j.finished)
            .map(|j| j.failed)
    }

    pub(super) fn compile_window(&mut self, ctx: &Context) {
        if self.show_tools_check {
            let mut close = false;
            let modal = widgets::dialog(
                ctx,
                "chisel-tools-check",
                "Tools",
                380.0,
                |ui| {
                    ui.label(theme::caption(
                        "Chisel runs the compilers as subcommands of this same toolset.",
                    ));
                    ui.add_space(4.0);
                    for (name, found) in available_tools() {
                        ui.horizontal(|ui| {
                            let (glyph, colour) = if found {
                                (icons::CHECK_CIRCLE, colors::OK)
                            } else {
                                (icons::X_CIRCLE, colors::ERR)
                            };
                            ui.label(theme::icon(glyph).color(colour));
                            ui.label(theme::mono(name).color(colour));
                        });
                    }
                    ui.add_space(4.0);
                    ui.label(theme::caption(
                        "Build them with: cargo build -p kerosene-tools -p kerosene",
                    ));
                },
                |ui| {
                    if ui.button("close").clicked() {
                        close = true;
                    }
                },
            );
            if close || modal.should_close() {
                self.show_tools_check = false;
            }
        }

        if !self.show_compile {
            return;
        }
        // Collected inside the dialog and acted on after it, so the closure
        // does not need a second mutable borrow of the app.
        let mut start: Option<Option<Quality>> = None;
        let mut close = false;
        let running = self.compiling();
        let last = self
            .compile
            .as_ref()
            .filter(|j| j.finished && !j.failed)
            .and_then(|j| j.output())
            .map(|p| p.display().to_string());
        let settings = &mut self.compile_settings;
        let modal = widgets::dialog(
            ctx,
            "chisel-compile",
            "Compile",
            460.0,
            |ui| {
                widgets::section(ui, "stages", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut settings.run_vis, "visibility");
                        ui.checkbox(&mut settings.fast_vis, "fast");
                        ui.checkbox(&mut settings.run_acoustics, "acoustics")
                            .on_hover_text(
                                "Resonance: work out how each room sounds from its shape \
                                 and materials, for the engine's reverb. Fast applies here \
                                 too, with fewer rays per room.",
                            );
                        ui.checkbox(&mut settings.run_lighting, "lighting");
                        ui.checkbox(&mut settings.run_after, "run after");
                    });
                });
                widgets::section(ui, "lighting", |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Slider::new(&mut settings.samples, 1..=4).text("samples"));
                        ui.add(egui::Slider::new(&mut settings.bounces, 0..=4).text("bounces"));
                    });
                });
                widgets::section(ui, "before and after", |ui| {
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
                             .keroleak trace beside the map showing the way out.",
                        );
                });
                if running {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(12.0).color(colors::ACCENT));
                        ui.label(theme::caption("compiling -- the log is in the output panel"));
                    });
                } else if let Some(path) = &last {
                    ui.label(theme::caption(format!("last built {path}")));
                }
            },
            |ui| {
                if widgets::primary_button(
                    ui,
                    RichText::new(format!("{}  compile", icons::PLAY)).color(colors::ON_ACCENT),
                )
                .on_hover_text("Compile with exactly the settings above.")
                .clicked()
                {
                    start = Some(None);
                }
                if ui
                    .add_enabled(!running, egui::Button::new("full"))
                    .on_hover_text("Full visibility and lighting, keeping every other choice.")
                    .clicked()
                {
                    start = Some(Some(Quality::Full));
                }
                if ui
                    .add_enabled(!running, egui::Button::new("fast"))
                    .on_hover_text("Quick visibility and lighting, keeping every other choice.")
                    .clicked()
                {
                    start = Some(Some(Quality::Fast));
                }
                if ui.button("close").clicked() {
                    close = true;
                }
            },
        );
        if close || modal.should_close() {
            self.show_compile = false;
        }
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
    pub(super) fn compile_now(&mut self, quality: Quality) {
        self.compile_settings.set_quality(quality);
        let settings = self.compile_settings.clone();
        self.start_compile(settings);
    }

    pub(super) fn start_compile(&mut self, settings: CompileSettings) {
        self.commit_properties();
        self.commit_property_window();
        // One at a time: a second pipeline over the same files would race
        // the first for the `.kerobsp`, and the first's log would vanish
        // with its channel.
        if self.compile.as_ref().is_some_and(|job| !job.finished) {
            self.status = "already compiling; wait for it to finish".into();
            return;
        }
        // The compilers read files, so the map has to be on disk first --
        // and compiling something other than what was saved would be a
        // genuinely confusing bug to chase.
        // A map with no name is named before it is compiled, rather than
        // being saved as `untitled` behind your back. The name is not
        // cosmetic: it is what `kerosene +map <name>` loads, so a compile that
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
            self.status = format!(
                "{} problems must be fixed first: {}",
                problems.len(),
                problems[0]
            );
            return;
        }

        let mut settings = settings;
        settings.content_root = self.content_root.clone();
        self.compile_settings = settings.clone();
        self.compile = Some(CompileJob::start(&path, settings));
        // The settings dialog has done its job; the log is in the output
        // panel, where it does not cover the map.
        self.show_compile = false;
        self.status = format!("compiling {}", path.display());
    }
}
