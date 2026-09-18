// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Compiling: the settings dialog and what happens when a compile ends.

use super::*;

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

    pub(super) fn compile_window(&mut self, ctx: &Context) {
        if self.show_tools_check {
            let mut open = true;
            egui::Window::new("tools").open(&mut open).show(ctx, |ui| {
                ui.label("Chisel runs the compilers as subcommands of this same toolset.");
                ui.separator();
                for (name, found) in available_tools() {
                    ui.label(
                        RichText::new(format!("{} {name}", if found { "found  " } else { "missing" }))
                            .monospace()
                            .color(if found { egui::Color32::LIGHT_GREEN } else { egui::Color32::LIGHT_RED }),
                    );
                }
                ui.separator();
                ui.label("Build them with: cargo build -p cleave -p umbra -p radiance -p kerosene-runtime");
            });
            self.show_tools_check = open;
        }

        if !self.show_compile {
            return;
        }
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
                    ui.checkbox(&mut settings.run_acoustics, "acoustics")
                        .on_hover_text(
                            "Resonance: work out how each room sounds from its shape \
                             and materials, for the engine's reverb. Fast applies here \
                             too, with fewer rays per room.",
                        );
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
                         .keroleak trace beside the map showing the way out.",
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
                    if ui
                        .add_enabled(!running, egui::Button::new("fast"))
                        .clicked()
                    {
                        start = Some(Some(Quality::Fast));
                    }
                    if ui
                        .add_enabled(!running, egui::Button::new("full"))
                        .clicked()
                    {
                        start = Some(Some(Quality::Full));
                    }
                    if running {
                        ui.spinner();
                    }
                    if let Some(path) = self
                        .compile
                        .as_ref()
                        .filter(|j| j.finished && !j.failed)
                        .and_then(|j| j.output())
                    {
                        ui.label(
                            RichText::new(format!("built {}", path.display()))
                                .size(11.0)
                                .weak(),
                        );
                    }
                });
                ui.separator();

                if let Some(job) = &self.compile {
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for message in &job.log {
                                let (text, color) = match message {
                                    CompileMessage::Stage(s) => {
                                        (format!("--- {s} ---"), egui::Color32::LIGHT_BLUE)
                                    }
                                    CompileMessage::Line(l) => (l.clone(), egui::Color32::GRAY),
                                    CompileMessage::Failed(e) => {
                                        (format!("failed: {e}"), egui::Color32::LIGHT_RED)
                                    }
                                    CompileMessage::Finished(p) => (
                                        format!("done: {}", p.display()),
                                        egui::Color32::LIGHT_GREEN,
                                    ),
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
            self.show_compile = true;
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
}
