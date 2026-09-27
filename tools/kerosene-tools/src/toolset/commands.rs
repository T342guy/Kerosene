// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! What the command palette offers: every tab, every job, every map, model
//! and sound, and every recent project.
//!
//! The list is built afresh each frame the palette is open, from the same
//! index the Home and Assets tabs read, so it is never out of date with what
//! they show. Each command's id says what to do in a form [`action_for`] can
//! read back, which keeps the palette itself ignorant of the toolset.

use std::path::PathBuf;

use kerosene_toolui::palette::Command;
use kerosene_toolui::theme::icons;

use super::{Action, JobKind, Tab, Toolset};
use crate::pages::assets::Kind;

impl Toolset {
    /// Everything the palette can run right now.
    pub(super) fn commands(&self) -> Vec<Command> {
        if !self.palette.is_open() {
            return Vec::new();
        }
        let mut list = Vec::new();

        for tab in Tab::ALL {
            let info = tab.info();
            list.push(
                Command::new(
                    format!("tab:{}", info.name.to_lowercase()),
                    format!("Go to {}", info.name),
                    info.glyph,
                    "Go to",
                )
                .detail(info.blurb)
                .shortcut(info.shortcut),
            );
        }

        let building = self.build.running();
        let packing = self.archive.running();
        let playing = self.play.as_ref().is_some_and(crate::job::Job::running);
        let mut job = |id: &str, title: &str, glyph: &'static str, detail: &str| {
            list.push(Command::new(id, title, glyph, "Run").detail(detail));
        };
        if !building {
            job(
                "act:build",
                "Build",
                icons::HAMMER,
                "compile everything that changed, with the Build tab's settings",
            );
            job(
                "act:build-fast",
                "Build (fast)",
                icons::LIGHTNING,
                "skip full visibility and lighting",
            );
            job(
                "act:clean",
                "Clean",
                icons::BROOM,
                "delete everything the build wrote",
            );
        } else {
            job(
                "act:cancel-build",
                "Cancel build",
                icons::STOP,
                "stop the running build",
            );
        }
        if !packing {
            job(
                "act:pack",
                "Pack archive",
                icons::PACKAGE,
                "write the archive the game ships",
            );
            job(
                "act:verify",
                "Verify archive",
                icons::SEAL_CHECK,
                "read every entry back and check its hash",
            );
        } else {
            job(
                "act:cancel-archive",
                "Cancel packing",
                icons::STOP,
                "stop the archive job",
            );
        }
        if !playing {
            job(
                "act:play",
                "Play",
                icons::PLAY,
                "build what changed, then run the game",
            );
        } else {
            job(
                "act:cancel-play",
                "Stop the game",
                icons::STOP,
                "end the play session",
            );
        }
        list.push(
            Command::new("act:new-map", "New map", icons::FILE_PLUS, "Map")
                .detail("open the editor on a starter room"),
        );
        list.push(
            Command::new(
                "act:output",
                "Toggle output panel",
                icons::TERMINAL_WINDOW,
                "View",
            )
            .shortcut("ctrl-`"),
        );
        list.push(
            Command::new(
                "act:rescan",
                "Rescan content",
                icons::ARROW_CLOCKWISE,
                "View",
            )
            .detail("walk the content tree again"),
        );
        list.push(
            Command::new(
                format!("reveal:{}", self.info.content.display()),
                "Show content folder",
                icons::FOLDER_OPEN,
                "View",
            )
            .detail(self.info.content.display().to_string()),
        );
        list.push(
            Command::new("act:start", "Switch project...", icons::SWAP, "Project")
                .detail("open, create or make a project"),
        );

        for entry in &self.index.entries {
            let (group, glyph) = match entry.kind {
                Kind::Map => ("Map", icons::MAP_TRIFOLD),
                Kind::Model => ("Model", icons::PACKAGE),
                Kind::AudioSource => ("Sound", icons::SPEAKER_HIGH),
                _ => continue,
            };
            let Some(action) = entry.open_action() else {
                continue;
            };
            let id = match action {
                Action::OpenMap(path) => format!("map:{}", path.display()),
                Action::OpenModel(name) => format!("model:{name}"),
                Action::OpenSound(path) => format!("sound:{}", path.display()),
                _ => continue,
            };
            list.push(Command::new(id, entry.file_name(), glyph, group).detail(&entry.relative));
        }

        for project in &self.recent.entries {
            if project.content == self.info.content || !project.exists() {
                continue;
            }
            list.push(
                Command::new(
                    format!("project:{}", project.content.display()),
                    format!("Open {}", project.name),
                    icons::FOLDER_SIMPLE,
                    "Project",
                )
                .detail(project.content.display().to_string()),
            );
        }
        list
    }
}

/// What a command id asks for.
pub(super) fn action_for(id: &str) -> Option<Action> {
    let (kind, rest) = id.split_once(':')?;
    Some(match kind {
        "tab" => Action::Goto(
            Tab::ALL
                .into_iter()
                .find(|t| t.name().eq_ignore_ascii_case(rest))?,
        ),
        "map" => Action::OpenMap(PathBuf::from(rest)),
        "model" => Action::OpenModel(rest.to_string()),
        "sound" => Action::OpenSound(PathBuf::from(rest)),
        "project" => Action::SwitchProject(PathBuf::from(rest)),
        "reveal" => Action::Reveal(PathBuf::from(rest)),
        "act" => match rest {
            "build" => Action::Build { fast: false },
            "build-fast" => Action::Build { fast: true },
            "clean" => Action::Clean,
            "pack" => Action::Pack,
            "verify" => Action::Verify,
            "play" => Action::Play,
            "cancel-build" => Action::Cancel(JobKind::Build),
            "cancel-archive" => Action::Cancel(JobKind::Archive),
            "cancel-play" => Action::Cancel(JobKind::Play),
            "new-map" => Action::NewMap,
            "output" => Action::ToggleOutput,
            "rescan" => Action::RescanAssets,
            "start" => Action::ShowStart,
            _ => return None,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_read_back_as_the_actions_they_name() {
        assert_eq!(action_for("tab:assets"), Some(Action::Goto(Tab::Assets)));
        assert_eq!(action_for("tab:ARCHIVE"), Some(Action::Goto(Tab::Archive)));
        assert_eq!(
            action_for("act:build-fast"),
            Some(Action::Build { fast: true })
        );
        assert_eq!(
            action_for("map:/c/maps/a.kmap"),
            Some(Action::OpenMap(PathBuf::from("/c/maps/a.kmap")))
        );
        assert_eq!(
            action_for("model:props/crate"),
            Some(Action::OpenModel("props/crate".into()))
        );
        assert_eq!(action_for("tab:nowhere"), None);
        assert_eq!(action_for("nonsense"), None);
    }
}
