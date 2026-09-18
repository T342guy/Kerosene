// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The unified toolset window.
//!
//! One application, one window: a project page, the world editor, the sound
//! editor, a build form and an archive form, switched with an activity bar
//! of icons down the left edge, and one output panel along the bottom that
//! every job logs into. Each tool is the tool it used to be, so a designer
//! goes from drawing brushes to building the project to checking the archive
//! without leaving the window.
//!
//! The stages also remain available as headless subcommands, so a build server
//! or a script can still drive them without a screen.

use anyhow::Result;
use chisel::ChiselApp;
use kerosene_ui::App as _;
use kerosene_ui::output::{OutputPanel, Source};
use kerosene_ui::theme::{self, colors, icons};
use kerosene_ui::widgets;
use std::path::PathBuf;
use timbre::gui::Timbre;

use crate::panels::{ArchivePanel, BuildPanel};
use crate::project::{ProjectAction, ProjectPanel};

/// Which tool the window is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    /// Where the window opens: what the project is and what is in it.
    #[default]
    Project,
    /// The world editor.
    Editor,
    Sound,
    Build,
    Archive,
}

impl Tab {
    const ALL: [Tab; 5] = [
        Tab::Project,
        Tab::Editor,
        Tab::Sound,
        Tab::Build,
        Tab::Archive,
    ];

    fn name(self) -> &'static str {
        match self {
            Tab::Project => "Project",
            Tab::Editor => "Editor",
            Tab::Sound => "Sound",
            Tab::Build => "Build",
            Tab::Archive => "Archive",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Tab::Project => icons::HOUSE,
            Tab::Editor => icons::CUBE,
            Tab::Sound => icons::SPEAKER_HIGH,
            Tab::Build => icons::HAMMER,
            Tab::Archive => icons::ARCHIVE,
        }
    }

    fn shortcut(self) -> &'static str {
        match self {
            Tab::Project => "ctrl-1",
            Tab::Editor => "ctrl-2",
            Tab::Sound => "ctrl-3",
            Tab::Build => "ctrl-4",
            Tab::Archive => "ctrl-5",
        }
    }

    fn key(self) -> egui::Key {
        match self {
            Tab::Project => egui::Key::Num1,
            Tab::Editor => egui::Key::Num2,
            Tab::Sound => egui::Key::Num3,
            Tab::Build => egui::Key::Num4,
            Tab::Archive => egui::Key::Num5,
        }
    }
}

/// How the toolset was asked to start.
#[derive(Clone, Debug, Default)]
pub struct Launch {
    /// Which tab to open on.
    pub tab: Tab,
    /// The content tree, when named on the command line.
    pub content: Option<PathBuf>,
    /// A map to open in the editor, when named on the command line.
    pub map: Option<PathBuf>,
    /// Which binary is the game, when the caller knows: a game that
    /// re-hosts the toolset names its own package. `None` asks the project
    /// file, and falls back to the stock runtime.
    pub runtime: Option<kerosene_vfs::toolchain::Runtime>,
}

/// The jobs whose logs the output panel shows, in the order it lists them.
const SOURCES: [&str; 3] = ["Compile", "Build", "Archive"];
const SOURCE_COMPILE: usize = 0;
const SOURCE_BUILD: usize = 1;
const SOURCE_ARCHIVE: usize = 2;

/// The whole toolset in one window.
pub struct Toolset {
    tab: Tab,
    project: ProjectPanel,
    editor: ChiselApp,
    sound: Option<Timbre>,
    sound_note: String,
    build: BuildPanel,
    archive: ArchivePanel,
    output: OutputPanel,
    /// Whether each job was running last frame, so the panel can come up
    /// the moment one starts rather than being asked for.
    was_running: [bool; 3],
}

impl Toolset {
    /// Find the content tree and open every tool against it.
    pub fn open(launch: Launch) -> Result<Toolset> {
        let found = kerosene_vfs::root::find(launch.content.as_deref(), launch.map.as_deref());
        log::info!("{}", kerosene_vfs::root::describe(&found));
        let root = found.as_ref().map(|f| f.root.clone()).unwrap_or_default();

        // Make any content directory that is missing before a tool goes
        // looking for it. Chisel scans `materials/` and Timbre needs `sound/`;
        // either one absent is a tool that opens empty and says nothing about
        // why.
        if let Some(found) = &found {
            kerosene_vfs::root::scaffold(
                &found.root,
                found.project.as_ref().and_then(|p| p.dirs.as_deref()),
            );
        }

        // The editor opens on the found tree, or a starter room when there is
        // no content at all -- same behaviour as the standalone editor had.
        let mut editor = ChiselApp::new(root.clone());
        editor.content_note = kerosene_vfs::root::describe(&found);
        editor.compile_settings.content_root = root.clone();
        // F9 runs the project's own game when it names one; the stock
        // runtime otherwise. Decided here, once, so the editor's compile
        // dialog can say which before anyone presses the key.
        let project = found.as_ref().and_then(|f| f.project.as_ref());
        editor.compile_settings.runtime = launch
            .runtime
            .clone()
            .unwrap_or_else(|| kerosene_vfs::toolchain::Runtime::for_project(project));
        if let Some(map) = launch.map {
            editor.open(map);
        } else if found.is_none() {
            editor.document = chisel::app::starter_document();
        }

        // The sound editor needs a `sound/` tree; without one it simply stays
        // closed and the tab says why, rather than refusing to open the
        // toolset at all.
        let (sound, sound_note) = match Timbre::open(&root) {
            Ok(sound) => (Some(sound), String::new()),
            Err(e) => (None, format!("could not open the sound editor: {e:#}")),
        };

        Ok(Toolset {
            tab: launch.tab,
            project: ProjectPanel::new(root.clone(), project, kerosene_vfs::root::describe(&found)),
            editor,
            sound,
            sound_note,
            build: BuildPanel::new(root.clone()),
            archive: ArchivePanel::new(root, project),
            output: OutputPanel::default(),
            was_running: [false; 3],
        })
    }

    /// The icons down the left edge.
    fn activity_bar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("activity")
            .exact_width(50.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_HEADER)
                    .inner_margin(egui::Margin::symmetric(7, 8)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;

                // The mark at the top: a K, in the accent, so the window is
                // recognisable in a taskbar of grey rectangles.
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("K")
                            .size(22.0)
                            .strong()
                            .color(colors::ACCENT),
                    )
                    .on_hover_text("Kerosene toolset");
                });
                ui.add_space(6.0);

                let running = [
                    false,
                    self.editor.compiling(),
                    self.sound
                        .as_ref()
                        .is_some_and(|s| s.wants_continuous_redraw()),
                    self.build.running(),
                    self.archive.running(),
                ];
                for (tab, busy) in Tab::ALL.into_iter().zip(running) {
                    let response = widgets::tool_button(
                        ui,
                        tab.glyph(),
                        tab.name(),
                        Some(tab.shortcut()),
                        self.tab == tab,
                    );
                    if busy {
                        // A dot in the corner: something is happening there.
                        let at = response.rect.right_top() + egui::vec2(-6.0, 6.0);
                        ui.painter().circle_filled(at, 3.0, colors::ACCENT);
                    }
                    if response.clicked() {
                        self.tab = tab;
                    }
                }

                // The bottom: the project, and the output panel's switch.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(2.0);
                    widgets::icon_toggle(
                        ui,
                        icons::TERMINAL_WINDOW,
                        "the output panel  (ctrl-`)",
                        &mut self.output.open,
                    );
                    ui.add_space(4.0);
                    let name = self.project.name();
                    let initial: String = name.chars().take(2).collect();
                    ui.label(
                        egui::RichText::new(initial.to_uppercase())
                            .size(11.0)
                            .strong()
                            .color(colors::TEXT_MUTED),
                    )
                    .on_hover_text(format!("{name}\n{}", self.project.content_root().display()));
                });
            });
    }

    /// The panel along the bottom that every job logs into.
    fn output_panel(&mut self, ctx: &egui::Context) {
        // Bring it up when a job starts, on that job.
        let running = [
            self.editor.compiling(),
            self.build.running(),
            self.archive.running(),
        ];
        for (index, (now, before)) in running.iter().zip(self.was_running).enumerate() {
            if *now && !before {
                self.output.show_source(index);
            }
        }
        self.was_running = running;

        let sources = [
            Source {
                name: SOURCES[SOURCE_COMPILE],
                lines: self.editor.output_lines(),
                running: running[SOURCE_COMPILE],
                failed: self.editor.compile_failed(),
            },
            Source {
                name: SOURCES[SOURCE_BUILD],
                lines: self
                    .build
                    .job
                    .as_ref()
                    .map(|j| j.lines())
                    .unwrap_or_default(),
                running: running[SOURCE_BUILD],
                failed: self.build.job.as_ref().and_then(|j| j.outcome()),
            },
            Source {
                name: SOURCES[SOURCE_ARCHIVE],
                lines: self
                    .archive
                    .job
                    .as_ref()
                    .map(|j| j.lines())
                    .unwrap_or_default(),
                running: running[SOURCE_ARCHIVE],
                failed: self.archive.job.as_ref().and_then(|j| j.outcome()),
            },
        ];
        let action = self.output.ui(ctx, &sources);
        match action.clear {
            Some(SOURCE_COMPILE) => {
                if !self.editor.compiling() {
                    self.editor.compile = None;
                }
            }
            Some(SOURCE_BUILD) => {
                if let Some(job) = &mut self.build.job {
                    job.log.clear();
                }
            }
            Some(SOURCE_ARCHIVE) => {
                if let Some(job) = &mut self.archive.job {
                    job.log.clear();
                }
            }
            _ => {}
        }
    }

    /// The toolset's own keys: switching tabs and the output panel. They
    /// run before the tab's keys, and only with ctrl held, so the editor's
    /// unmodified digits still pick its tools.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        ctx.input_mut(|i| {
            for tab in Tab::ALL {
                if i.consume_key(egui::Modifiers::CTRL, tab.key()) {
                    self.tab = tab;
                }
            }
            if i.consume_key(egui::Modifiers::CTRL, egui::Key::Backtick) {
                self.output.open = !self.output.open;
            }
        });
    }

    fn act(&mut self, action: ProjectAction) {
        match action {
            ProjectAction::OpenMap(path) => {
                self.editor.open(path);
                self.tab = Tab::Editor;
            }
            ProjectAction::NewMap => {
                if !self.editor.document.is_modified() {
                    self.editor.document = chisel::app::starter_document();
                }
                self.tab = Tab::Editor;
            }
            ProjectAction::Build => {
                self.build.start();
                self.tab = Tab::Build;
            }
            ProjectAction::Pack => {
                self.archive.pack();
                self.tab = Tab::Archive;
            }
        }
    }
}

impl kerosene_ui::App for Toolset {
    fn ui(&mut self, ctx: &egui::Context) {
        self.shortcuts(ctx);
        self.activity_bar(ctx);
        let before = self.was_running;
        self.output_panel(ctx);

        // A finished job changes what is on disk, which the project page
        // counts.
        let finished = before.iter().any(|r| *r) && !self.was_running.iter().any(|r| *r);
        if finished {
            self.project.refresh();
        }

        match self.tab {
            Tab::Project => {
                let building = self.build.running();
                let packing = self.archive.running();
                if let Some(action) = self.project.ui(ctx, building, packing) {
                    self.act(action);
                }
            }
            Tab::Editor => self.editor.ui(ctx),
            Tab::Sound => match &mut self.sound {
                Some(sound) => sound.ui(ctx),
                None => {
                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::new()
                                .fill(colors::BG_APP)
                                .inner_margin(egui::Margin::same(24)),
                        )
                        .show(ctx, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    theme::icon(icons::SPEAKER_SLASH)
                                        .size(22.0)
                                        .color(colors::WARN),
                                );
                                ui.label(theme::heading("Sound"));
                            });
                            ui.add_space(8.0);
                            ui.label(theme::warn(&self.sound_note));
                        });
                }
            },
            Tab::Build => self.build.ui(ctx),
            Tab::Archive => self.archive.ui(ctx),
        }
    }

    fn window_title(&self) -> String {
        match self.tab {
            Tab::Project => format!("{} -- Kerosene toolset", self.project.name()),
            Tab::Editor => self.editor.window_title(),
            Tab::Sound => self
                .sound
                .as_ref()
                .map(|s| s.window_title())
                .unwrap_or_else(|| "Sound -- Kerosene toolset".into()),
            Tab::Build => "Build -- Kerosene toolset".into(),
            Tab::Archive => "Archive -- Kerosene toolset".into(),
        }
    }

    fn wants_continuous_redraw(&self) -> bool {
        // A running job keeps the output panel and the activity bar's dot
        // moving whichever tab is up.
        self.build.running()
            || self.archive.running()
            || match self.tab {
                Tab::Sound => self
                    .sound
                    .as_ref()
                    .is_some_and(|s| s.wants_continuous_redraw()),
                _ => false,
            }
    }

    fn close_requested(&mut self) -> bool {
        // The editor is the one tool holding work that is not on disk; its
        // question is shown on its own tab.
        if self.editor.request_close() {
            return true;
        }
        self.tab = Tab::Editor;
        false
    }

    fn wants_to_quit(&self) -> bool {
        self.editor.wants_to_quit()
    }
}

/// Open the toolset window.
pub fn run_gui(launch: Launch) -> Result<()> {
    let toolset = Toolset::open(launch)?;
    kerosene_ui::run("Kerosene toolset", (1600, 950), toolset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kerosene_ui::App;

    /// A toolset over an empty directory: no project, no content.
    fn toolset_in(name: &str) -> (Toolset, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("kerosene-toolset-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let toolset = Toolset::open(Launch {
            tab: Tab::Project,
            content: Some(root.clone()),
            ..Default::default()
        })
        .unwrap();
        (toolset, root)
    }

    #[test]
    fn a_project_naming_a_game_package_is_what_f9_launches() {
        let (_, root) = toolset_in("game-key");
        std::fs::write(
            root.join("mine.keroproj"),
            "project { \"name\" \"Mine\" \"content\" \".\" \"game\" \"my-game\" }",
        )
        .unwrap();
        // Found through the map, the way a double-clicked map is: an
        // explicit --content is taken at its word and reads no project.
        let map = root.join("maps").join("mine.keromap");
        std::fs::create_dir_all(map.parent().unwrap()).unwrap();
        std::fs::write(&map, chisel::app::starter_document().map.to_text()).unwrap();
        let toolset = Toolset::open(Launch {
            tab: Tab::Editor,
            map: Some(map),
            ..Default::default()
        })
        .unwrap();
        match &toolset.editor.compile_settings.runtime {
            kerosene_vfs::toolchain::Runtime::Package { name, .. } => assert_eq!(name, "my-game"),
            other => panic!("expected the project's package, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_tab_draws_a_frame_without_a_content_tree() {
        let (mut toolset, root) = toolset_in("tabs");
        let ctx = egui::Context::default();
        kerosene_ui::theme::install(&ctx);
        for tab in Tab::ALL {
            toolset.tab = tab;
            toolset.output.open = true;
            let output = ctx.run(egui::RawInput::default(), |ctx| toolset.ui(ctx));
            assert!(!output.shapes.is_empty(), "{tab:?}");
            assert!(!toolset.window_title().is_empty());
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_window_opens_on_the_project_page() {
        let (toolset, root) = toolset_in("default-tab");
        assert_eq!(toolset.tab, Tab::Project);
        assert_eq!(Tab::default(), Tab::Project);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ctrl_and_a_digit_switch_tabs() {
        let (mut toolset, root) = toolset_in("keys");
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::Num4,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        });
        let _ = ctx.run(input, |ctx| toolset.ui(ctx));
        assert_eq!(toolset.tab, Tab::Build);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opening_a_map_from_the_project_page_goes_to_the_editor() {
        let (mut toolset, root) = toolset_in("open-map");
        let map = root.join("maps").join("arena.keromap");
        std::fs::create_dir_all(map.parent().unwrap()).unwrap();
        toolset.editor.document = chisel::app::starter_document();
        assert!(toolset.editor.save(Some(map.clone())));
        toolset.act(ProjectAction::OpenMap(map.clone()));
        assert_eq!(toolset.tab, Tab::Editor);
        assert_eq!(toolset.editor.document.path.as_deref(), Some(map.as_path()));
        let _ = std::fs::remove_dir_all(&root);
    }
}
