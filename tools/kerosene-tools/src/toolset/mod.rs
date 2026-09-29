// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The unified toolset window.
//!
//! One application, one window: a home page, an asset browser, the world
//! editor, the model viewer, the sound editor, a build page and an archive
//! page, switched from a sidebar down the left edge. Across the top, a bar
//! with where you are, a box that searches everything, and whatever job is
//! running; along the bottom, one output panel every job logs into. Each
//! tool is the tool it used to be, so a designer goes from drawing brushes
//! to building the project to checking the archive without leaving the
//! window -- and `ctrl-P` finds any map, model, sound or command without
//! knowing which tab it lives on.
//!
//! The stages also remain available as headless subcommands, so a build server
//! or a script can still drive them without a screen.

mod commands;
mod sidebar;
mod topbar;

use kerosene_rhi::wgpu;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chisel::ChiselApp;
use kerosene_toolui::output::{OutputPanel, Source};
use kerosene_toolui::palette::Palette;
use kerosene_toolui::theme::{colors, icons};
use kerosene_toolui::widgets;
use timbre::gui::Timbre;

use crate::job::Job;
use crate::pages::archive::ArchivePage;
use crate::pages::assets::{AssetsPage, Category, Index};
use crate::pages::build::BuildPage;
use crate::pages::home::{HomePage, Jobs, ProjectInfo};
use crate::pages::start::StartPage;
use crate::recent::Recent;

/// Which tool the window is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Tab {
    /// Where the window opens: what the project is and what is in it.
    #[default]
    Home,
    /// Every file in the content tree.
    Assets,
    /// The world editor.
    Editor,
    /// The model viewer: Loupe, on its own tab rather than its own window,
    /// the same way Timbre lives inside `Sound` rather than launching apart.
    Models,
    Sound,
    Build,
    Archive,
}

/// What each tab is called, drawn as, and switched to with.
struct TabInfo {
    name: &'static str,
    glyph: &'static str,
    key: egui::Key,
    shortcut: &'static str,
    /// What the palette says about it.
    blurb: &'static str,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Home,
        Tab::Assets,
        Tab::Editor,
        Tab::Models,
        Tab::Sound,
        Tab::Build,
        Tab::Archive,
    ];

    fn info(self) -> TabInfo {
        let (name, glyph, key, shortcut, blurb) = match self {
            Tab::Home => (
                "Home",
                icons::HOUSE,
                egui::Key::Num1,
                "ctrl-1",
                "the project, its content and what needs building",
            ),
            Tab::Assets => (
                "Assets",
                icons::FOLDERS,
                egui::Key::Num2,
                "ctrl-2",
                "every file in the content tree",
            ),
            Tab::Editor => (
                "Editor",
                icons::CUBE,
                egui::Key::Num3,
                "ctrl-3",
                "the world editor",
            ),
            Tab::Models => (
                "Models",
                icons::CUBE_FOCUS,
                egui::Key::Num4,
                "ctrl-4",
                "the model viewer",
            ),
            Tab::Sound => (
                "Sound",
                icons::SPEAKER_HIGH,
                egui::Key::Num5,
                "ctrl-5",
                "the sound editor",
            ),
            Tab::Build => (
                "Build",
                icons::HAMMER,
                egui::Key::Num6,
                "ctrl-6",
                "compile the project's content",
            ),
            Tab::Archive => (
                "Archive",
                icons::ARCHIVE,
                egui::Key::Num7,
                "ctrl-7",
                "pack and inspect the game's archive",
            ),
        };
        TabInfo {
            name,
            glyph,
            key,
            shortcut,
            blurb,
        }
    }

    pub fn name(self) -> &'static str {
        self.info().name
    }

    pub fn glyph(self) -> &'static str {
        self.info().glyph
    }

    pub fn shortcut(self) -> &'static str {
        self.info().shortcut
    }
}

/// Which background job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Build,
    Archive,
    Play,
}

/// Something the person asked for, from a page, the sidebar, the top bar or
/// the palette. Carried out after the frame is drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Goto(Tab),
    OpenMap(PathBuf),
    NewMap,
    /// A model, by the name a schema's `model` key holds.
    OpenModel(String),
    OpenSound(PathBuf),
    /// The Assets tab, on one category or on everything.
    BrowseAssets(Option<Category>),
    RescanAssets,
    Build {
        /// Skip full visibility and lighting, whatever the Build tab says.
        fast: bool,
    },
    Clean,
    Pack,
    Verify,
    Play,
    Cancel(JobKind),
    /// Open another project, by its content tree.
    SwitchProject(PathBuf),
    /// Take a project off the recent list.
    ForgetProject(PathBuf),
    ShowStart,
    HideStart,
    ToggleOutput,
    ShowOutput,
    OpenPalette,
    /// Show a file or folder in the system's file manager.
    Reveal(PathBuf),
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
    /// The `.kdef` text of a game's own classes, shown in the editor
    /// after the stock ones. What a game hands over when it re-hosts the
    /// toolset; empty for the stock one.
    pub schema: Vec<&'static str>,
    /// Which binary is the game, when the caller knows: a game that
    /// re-hosts the toolset names its own package. `None` asks the project
    /// file, and falls back to the stock runtime.
    pub runtime: Option<kerosene_vfs::toolchain::Runtime>,
}

/// The jobs whose logs the output panel shows, in the order it lists them.
const SOURCES: [&str; 4] = ["Compile", "Build", "Archive", "Play"];
const SOURCE_COMPILE: usize = 0;
const SOURCE_BUILD: usize = 1;
const SOURCE_ARCHIVE: usize = 2;
const SOURCE_PLAY: usize = 3;

/// What closes the editor's map, held while the author is asked whether to
/// save it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AfterClose {
    /// Open another project.
    Switch(PathBuf),
    /// Open another map.
    OpenMap(PathBuf),
    /// Start a new map from the starter room.
    NewMap,
}

/// The whole toolset in one window.
pub struct Toolset {
    tab: Tab,
    /// What the project is, for every page that says so.
    info: ProjectInfo,
    /// Whether a content tree was found at all. Without one, the start
    /// page is all there is to show.
    found: bool,
    /// Every file in the content tree, walked when the tree may have changed.
    index: Index,
    showing_start: bool,
    start: StartPage,
    home: HomePage,
    assets: AssetsPage,
    editor: ChiselApp,
    models: loupe::LoupeApp,
    sound: Option<Timbre>,
    sound_note: String,
    build: BuildPage,
    archive: ArchivePage,
    play: Option<Job>,
    output: OutputPanel,
    palette: Palette,
    recent: Recent,
    /// What to do once the editor's unsaved map is dealt with.
    pending_close: Option<AfterClose>,
    /// Whether each output source was running last frame, so the panel can
    /// come up the moment one starts rather than being asked for.
    was_running: [bool; 4],
    /// What the launch said about the game, kept for opening another
    /// project the same way.
    schema: Vec<&'static str>,
    runtime: Option<kerosene_vfs::toolchain::Runtime>,
    layout_file: Option<PathBuf>,
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
        let mut editor = ChiselApp::with_schema(root.clone(), &launch.schema);
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
        let opened_map = launch.map.is_some();
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

        let info = ProjectInfo::new(root.clone(), project, kerosene_vfs::root::describe(&found));
        let archive = ArchivePage::new(root.clone(), info.archive.clone());
        Ok(Toolset {
            tab: launch.tab,
            showing_start: found.is_none() && !opened_map,
            found: found.is_some(),
            index: if found.is_some() {
                Index::scan(&root)
            } else {
                Index::default()
            },
            info,
            start: StartPage::default(),
            home: HomePage::default(),
            assets: AssetsPage::default(),
            editor,
            models: loupe::LoupeApp::open(root.clone()),
            sound,
            sound_note,
            build: BuildPage::new(root),
            archive,
            play: None,
            output: OutputPanel::default(),
            palette: Palette::default(),
            recent: Recent::default(),
            pending_close: None,
            was_running: [false; 4],
            schema: launch.schema,
            runtime: launch.runtime,
            layout_file: None,
        })
    }

    /// Remember projects in this person's recent list, from now on.
    pub fn use_recent(&mut self, recent: Recent) {
        self.recent = recent;
        if self.found {
            self.recent.add(&self.info.content, &self.info.name);
        }
    }

    /// Keep the editor's layout in this file.
    pub fn set_layout_file(&mut self, file: PathBuf) {
        self.editor.set_layout_file(file.clone());
        self.layout_file = Some(file);
    }

    /// Which tab is showing.
    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// The command palette, for a host that opens it pre-filled.
    pub fn palette_mut(&mut self) -> &mut Palette {
        &mut self.palette
    }

    /// Whether each output source is running, in [`SOURCES`] order.
    fn running(&self) -> [bool; 4] {
        [
            self.editor.compiling(),
            self.build.running(),
            self.archive.running(),
            self.play.as_ref().is_some_and(Job::running),
        ]
    }

    fn poll_jobs(&mut self) {
        for job in [&mut self.build.job, &mut self.archive.job, &mut self.play]
            .into_iter()
            .flatten()
        {
            job.poll();
        }
    }

    /// Walk the content tree again: a job, a save or a switch of tab may
    /// have changed what is there.
    fn rescan(&mut self) {
        if self.found {
            self.index = Index::scan(&self.info.content);
        }
    }

    /// The panel along the bottom that every job logs into.
    fn output_panel(&mut self, ctx: &egui::Context) {
        let running = self.running();
        fn lines(job: &Option<Job>) -> Vec<kerosene_toolui::output::Line<'_>> {
            job.as_ref().map(Job::lines).unwrap_or_default()
        }
        let outcome = |job: &Option<Job>| job.as_ref().and_then(Job::outcome);
        let sources = [
            Source {
                name: SOURCES[SOURCE_COMPILE],
                lines: self.editor.output_lines(),
                running: running[SOURCE_COMPILE],
                failed: self.editor.compile_failed(),
            },
            Source {
                name: SOURCES[SOURCE_BUILD],
                lines: lines(&self.build.job),
                running: running[SOURCE_BUILD],
                failed: outcome(&self.build.job),
            },
            Source {
                name: SOURCES[SOURCE_ARCHIVE],
                lines: lines(&self.archive.job),
                running: running[SOURCE_ARCHIVE],
                failed: outcome(&self.archive.job),
            },
            Source {
                name: SOURCES[SOURCE_PLAY],
                lines: lines(&self.play),
                running: running[SOURCE_PLAY],
                failed: outcome(&self.play),
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
            Some(SOURCE_PLAY) => {
                if let Some(job) = &mut self.play {
                    job.log.clear();
                }
            }
            _ => {}
        }
    }

    /// The toolset's own keys: the palette, switching tabs and the output
    /// panel. They run before the tab's keys, and only with ctrl held, so
    /// the editor's unmodified digits still pick its tools.
    fn shortcuts(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if self.palette.is_open() {
            return;
        }
        // The palette opens even from a text field: it is how a person who
        // is typing gets somewhere else.
        let palette = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::P)
                | i.consume_key(egui::Modifiers::CTRL, egui::Key::P)
                | i.consume_key(egui::Modifiers::CTRL, egui::Key::K)
        });
        if palette {
            actions.push(Action::OpenPalette);
            return;
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        ctx.input_mut(|i| {
            for tab in Tab::ALL {
                if i.consume_key(egui::Modifiers::CTRL, tab.info().key) {
                    actions.push(Action::Goto(tab));
                }
            }
            if i.consume_key(egui::Modifiers::CTRL, egui::Key::Backtick) {
                actions.push(Action::ToggleOutput);
            }
        });
    }

    /// Do what a button, the sidebar or the palette would: the one way
    /// in for a host that drives the window itself, and for tests.
    pub fn act(&mut self, action: Action) {
        match action {
            Action::Goto(tab) => self.goto(tab),
            Action::OpenMap(path) => self.close_map_then(AfterClose::OpenMap(path)),
            Action::NewMap => self.close_map_then(AfterClose::NewMap),
            Action::OpenModel(name) => {
                self.models.show_model(&name);
                self.goto(Tab::Models);
            }
            Action::OpenSound(path) => {
                if let Some(sound) = &mut self.sound {
                    sound.show_sound(&path);
                }
                self.goto(Tab::Sound);
            }
            Action::BrowseAssets(category) => {
                self.assets.category = category;
                self.goto(Tab::Assets);
            }
            Action::RescanAssets => self.rescan(),
            Action::Build { fast } => {
                self.build.start(fast);
                self.output.show_source(SOURCE_BUILD);
            }
            Action::Clean => {
                self.build.clean();
                self.output.show_source(SOURCE_BUILD);
            }
            Action::Pack => {
                self.archive.pack();
                self.output.show_source(SOURCE_ARCHIVE);
            }
            Action::Verify => {
                self.archive.verify();
                self.output.show_source(SOURCE_ARCHIVE);
            }
            Action::Play => {
                if !self.play.as_ref().is_some_and(Job::running) {
                    let args = [
                        "--content".to_string(),
                        self.info.content.display().to_string(),
                    ];
                    self.play = Some(Job::start("Play", "play", &args));
                    self.output.show_source(SOURCE_PLAY);
                }
            }
            Action::Cancel(kind) => {
                let job = match kind {
                    JobKind::Build => &mut self.build.job,
                    JobKind::Archive => &mut self.archive.job,
                    JobKind::Play => &mut self.play,
                };
                if let Some(job) = job {
                    job.cancel();
                }
            }
            Action::SwitchProject(content) => self.close_map_then(AfterClose::Switch(content)),
            Action::ForgetProject(content) => self.recent.remove(&content),
            Action::ShowStart => self.showing_start = true,
            Action::HideStart => self.showing_start = !self.found,
            Action::ToggleOutput => self.output.open = !self.output.open,
            Action::ShowOutput => self.output.open = true,
            Action::OpenPalette => self.palette.open(),
            Action::Reveal(path) => reveal(&path),
        }
    }

    fn goto(&mut self, tab: Tab) {
        if matches!(tab, Tab::Home | Tab::Assets) && tab != self.tab {
            self.rescan();
        }
        self.tab = tab;
        if self.found {
            self.showing_start = false;
        }
    }

    /// Open another project in place of this one. The editor's map must
    /// already be saved or given up.
    fn switch_to(&mut self, content: PathBuf) {
        let launch = Launch {
            tab: Tab::Home,
            content: Some(content),
            map: None,
            schema: self.schema.clone(),
            runtime: self.runtime.clone(),
        };
        let mut next = match Toolset::open(launch) {
            Ok(next) => next,
            Err(e) => {
                log::error!("could not open the project: {e:#}");
                return;
            }
        };
        // Jobs belong to the project they were started on.
        for job in [&mut self.build.job, &mut self.archive.job, &mut self.play]
            .into_iter()
            .flatten()
        {
            job.cancel();
        }
        next.editor.gpu_target = self.editor.gpu_target;
        if let Some(file) = self.layout_file.take() {
            next.set_layout_file(file);
        }
        next.output.open = self.output.open;
        next.output.height = self.output.height;
        next.use_recent(std::mem::take(&mut self.recent));
        *self = next;
    }

    /// Do `next`, which closes the editor's map, once the map is safe to
    /// close: now if it has no unsaved changes, after asking if it has.
    fn close_map_then(&mut self, next: AfterClose) {
        if self.editor.document.is_modified() {
            self.goto(Tab::Editor);
            self.pending_close = Some(next);
        } else {
            self.finish_close(next);
        }
    }

    fn finish_close(&mut self, next: AfterClose) {
        match next {
            AfterClose::Switch(content) => self.switch_to(content),
            AfterClose::OpenMap(path) => {
                self.editor.open(path);
                self.goto(Tab::Editor);
            }
            AfterClose::NewMap => {
                self.editor.document = chisel::app::starter_document();
                self.goto(Tab::Editor);
            }
        }
    }

    /// The question asked before closing an unsaved map.
    fn close_dialog(&mut self, ctx: &egui::Context) {
        let Some(next) = self.pending_close.clone() else {
            return;
        };
        let title = self.editor.document.title();
        let can_save = self.editor.document.path.is_some();
        let (what, verb) = match &next {
            AfterClose::Switch(_) => ("Opening another project", "switch"),
            AfterClose::OpenMap(_) => ("Opening another map", "open"),
            AfterClose::NewMap => ("Starting a new map", "start"),
        };
        let mut choice = None;
        let modal = widgets::dialog(
            ctx,
            "close-map",
            "Save the map first?",
            380.0,
            |ui| {
                ui.label(format!(
                    "{title} has changes that are not saved. {what} closes it."
                ));
            },
            |ui| {
                if ui
                    .add_enabled(can_save, egui::Button::new(format!("Save and {verb}")))
                    .clicked()
                {
                    choice = Some(true);
                }
                if ui.button(format!("Discard and {verb}")).clicked() {
                    choice = Some(false);
                }
                if ui.button("Cancel").clicked() {
                    self.pending_close = None;
                }
            },
        );
        if modal.should_close() {
            self.pending_close = None;
        }
        match choice {
            Some(true) if self.editor.save(None) => {
                self.pending_close = None;
                self.finish_close(next);
            }
            Some(true) => {}
            Some(false) => {
                self.pending_close = None;
                self.finish_close(next);
            }
            None => {}
        }
    }

    /// What the Sound tab shows when there is no sound editor.
    fn sound_missing(&self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors::BG_APP))
            .show(ctx, |ui| {
                ui.add_space(80.0);
                widgets::empty_state(
                    ui,
                    icons::SPEAKER_SLASH,
                    "The sound editor is not available",
                    &self.sound_note,
                );
            });
    }
}

impl kerosene_toolui::App for Toolset {
    fn ui(&mut self, ctx: &egui::Context) {
        self.poll_jobs();
        let mut actions = Vec::new();
        self.shortcuts(ctx, &mut actions);
        self.sidebar(ctx, &mut actions);
        self.topbar(ctx, &mut actions);

        // Bring the output panel up when a job starts, on that job.
        let running = self.running();
        for (index, (now, before)) in running.iter().zip(self.was_running).enumerate() {
            if *now && !before {
                self.output.show_source(index);
            }
        }
        // A finished job changes what is on disk, which the pages count.
        let finished = running
            .iter()
            .zip(self.was_running)
            .any(|(now, before)| before && !now);
        self.was_running = running;
        if finished {
            self.rescan();
            self.models.refresh();
        }
        self.output_panel(ctx);

        let action = if self.showing_start {
            let current = self.found.then_some(self.info.name.as_str());
            self.start.ui(ctx, &self.recent, current)
        } else {
            match self.tab {
                Tab::Home => {
                    let jobs = Jobs {
                        build: self.build.job.as_ref(),
                        archive: self.archive.job.as_ref(),
                        play: self.play.as_ref(),
                    };
                    self.home.ui(ctx, &self.info, &self.index, &jobs)
                }
                Tab::Assets => self.assets.ui(ctx, &self.index),
                Tab::Editor => {
                    self.editor.ui(ctx);
                    None
                }
                Tab::Models => {
                    self.models.ui(ctx);
                    None
                }
                Tab::Sound => {
                    match &mut self.sound {
                        Some(sound) => sound.ui(ctx),
                        None => self.sound_missing(ctx),
                    }
                    None
                }
                Tab::Build => self.build.ui(ctx),
                Tab::Archive => self.archive.ui(ctx, &self.index),
            }
        };
        actions.extend(action);

        let commands = self.commands();
        if let Some(id) = self.palette.ui(ctx, &commands) {
            actions.extend(commands::action_for(&id));
        }
        self.close_dialog(ctx);

        for action in actions {
            self.act(action);
        }
    }

    fn window_title(&self) -> String {
        if self.showing_start {
            return "Kerosene".into();
        }
        let project = &self.info.name;
        match self.tab {
            Tab::Editor => self.editor.window_title(),
            Tab::Sound => self
                .sound
                .as_ref()
                .map(|s| s.window_title())
                .unwrap_or_else(|| format!("Sound -- {project}")),
            Tab::Models => self.models.window_title(),
            tab => format!("{} -- {project} -- Kerosene", tab.name()),
        }
    }

    fn wants_continuous_redraw(&self) -> bool {
        // A running job keeps the output panel, the top bar's timer and the
        // sidebar's dot moving whichever tab is up.
        self.running().iter().any(|r| *r)
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
        self.showing_start = false;
        self.tab = Tab::Editor;
        false
    }

    fn wants_to_quit(&self) -> bool {
        self.editor.wants_to_quit()
    }

    fn gpu_ready(&mut self, target: wgpu::TextureFormat) {
        self.editor.gpu_target = Some(target);
    }
}

/// Show a file or folder in the system's file manager: the folder opened,
/// and the file selected in it where the file manager can do that.
pub fn reveal(path: &Path) {
    let folder = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    let mut command = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("explorer");
        if path.is_file() {
            c.arg(format!("/select,{}", path.display()));
        } else {
            c.arg(folder);
        }
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        if path.is_file() {
            c.arg("-R");
            c.arg(path);
        } else {
            c.arg(folder);
        }
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(folder);
        c
    };
    if let Err(e) = command.spawn() {
        log::warn!("could not show {}: {e}", path.display());
    }
}

/// Open the toolset window.
pub fn run_gui(launch: Launch) -> Result<()> {
    let mut toolset = Toolset::open(launch)?;
    // The editor's layout and the recent projects are the person's, kept
    // with their other settings -- and only here, in the real window, so no
    // test ever writes to them.
    if let Some(dir) = kerosene_vfs::user_data_dir("kerosene") {
        toolset.set_layout_file(dir.join(chisel::app::LAYOUT_FILE));
    }
    toolset.use_recent(Recent::for_user());
    kerosene_toolui::run("Kerosene", (1600, 950), toolset)
}

#[cfg(test)]
mod tests;
