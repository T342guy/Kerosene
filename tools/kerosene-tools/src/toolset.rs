// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The unified toolset window.
//!
//! One application, one window: the world editor, the sound editor, a build
//! panel and an archive panel, switched with a rail down the left edge. Each
//! of them is the tool it used to be, so a designer goes from drawing brushes
//! to building the project to checking the archive without leaving the window.
//!
//! The stages also remain available as headless subcommands, so a build server
//! or a script can still drive them without a screen.

use anyhow::Result;
use chisel::ChiselApp;
use std::path::PathBuf;
use timbre::gui::Timbre;

use crate::panels::{ArchivePanel, BuildPanel};

/// Which tool the window is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    /// The world editor, the default view.
    #[default]
    Editor,
    Sound,
    Build,
    Archive,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Editor, Tab::Sound, Tab::Build, Tab::Archive];

    fn name(self) -> &'static str {
        match self {
            Tab::Editor => "Editor",
            Tab::Sound => "Sound",
            Tab::Build => "Build",
            Tab::Archive => "Archive",
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
}

/// The whole toolset in one window.
pub struct Toolset {
    tab: Tab,
    editor: ChiselApp,
    sound: Option<Timbre>,
    sound_note: String,
    build: BuildPanel,
    archive: ArchivePanel,
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
            editor,
            sound,
            sound_note,
            build: BuildPanel::new(root.clone()),
            archive: ArchivePanel::new(root),
        })
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("toolset-sidebar")
            .resizable(false)
            .default_width(120.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.heading("Kerosene");
                ui.separator();
                for tab in Tab::ALL {
                    if ui.selectable_label(self.tab == tab, tab.name()).clicked() {
                        self.tab = tab;
                    }
                }
            });
    }
}

impl kerosene_ui::App for Toolset {
    fn ui(&mut self, ctx: &egui::Context) {
        self.sidebar(ctx);
        match self.tab {
            Tab::Editor => self.editor.ui(ctx),
            Tab::Sound => match &mut self.sound {
                Some(sound) => sound.ui(ctx),
                None => {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.heading("Sound");
                        ui.separator();
                        ui.label(&self.sound_note);
                    });
                }
            },
            Tab::Build => self.build.ui(ctx),
            Tab::Archive => self.archive.ui(ctx),
        }
    }

    fn window_title(&self) -> String {
        match self.tab {
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
        match self.tab {
            Tab::Sound => self
                .sound
                .as_ref()
                .is_some_and(|s| s.wants_continuous_redraw()),
            Tab::Build => self.build.running(),
            Tab::Archive => self.archive.running(),
            Tab::Editor => false,
        }
    }
}

/// Open the toolset window.
pub fn run_gui(launch: Launch) -> Result<()> {
    let toolset = Toolset::open(launch)?;
    kerosene_ui::run("Kerosene toolset", (1600, 950), toolset)
}
