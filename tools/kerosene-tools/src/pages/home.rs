// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The Home tab: where the toolset opens, and what it found.
//!
//! An editor that opens straight onto an empty map answers none of the
//! questions a person has on arriving -- which project is this, where is
//! its content, how much of it is there, what needs building, what was I
//! working on. This page answers them, and puts the four things people
//! come to do -- make a map, build, pack, play -- one click away.

use std::path::{Path, PathBuf};

use egui::{Align, Align2, FontId, Layout, Sense, Ui, Vec2};
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets::{self, Kind as ButtonKind, Tone};

use super::assets::{Category, Index, Kind, Status};
use crate::job::Job;
use crate::toolset::Action;

/// What the page knows about the project, taken once when it is opened.
#[derive(Clone, Debug, Default)]
pub struct ProjectInfo {
    pub name: String,
    pub content: PathBuf,
    /// The project file, when one names the tree.
    pub file: Option<PathBuf>,
    pub start_map: Option<String>,
    /// The Cargo package that is the game, when there is one.
    pub game: Option<String>,
    /// How the content was found, in the search's own words.
    pub found_note: String,
    /// Where Kiln writes this project's archive.
    pub archive: PathBuf,
}

impl ProjectInfo {
    pub fn new(
        content: PathBuf,
        project: Option<&kerosene_vfs::Project>,
        found_note: String,
    ) -> ProjectInfo {
        let name = match project {
            Some(p) => p.name.clone(),
            None => content
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "no project".to_string()),
        };
        ProjectInfo {
            name,
            archive: kiln::archive_path(&content, project),
            file: project.map(|p| p.path.clone()),
            start_map: project.and_then(|p| p.start_map.clone()),
            game: project.and_then(|p| p.game.clone()),
            found_note,
            content,
        }
    }
}

/// What the page is told about the jobs each frame.
pub struct Jobs<'a> {
    pub build: Option<&'a Job>,
    pub archive: Option<&'a Job>,
    pub play: Option<&'a Job>,
}

impl Jobs<'_> {
    fn building(&self) -> bool {
        self.build.is_some_and(Job::running)
    }

    fn packing(&self) -> bool {
        self.archive.is_some_and(Job::running)
    }

    fn playing(&self) -> bool {
        self.play.is_some_and(Job::running)
    }
}

/// The page's state between frames.
#[derive(Default)]
pub struct HomePage {
    map_filter: String,
}

impl HomePage {
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        project: &ProjectInfo,
        index: &Index,
        jobs: &Jobs<'_>,
    ) -> Option<Action> {
        let mut action = None;
        widgets::page(ctx, 980.0, |ui| {
            let subtitle = match &project.file {
                Some(file) => file.display().to_string(),
                None => "No project file -- the content tree was inferred".to_string(),
            };
            widgets::page_header(ui, icons::HOUSE, &project.name, &subtitle, |ui| {
                if widgets::button(ui, ButtonKind::Secondary, icons::SWAP, "Switch project")
                    .clicked()
                {
                    action = Some(Action::ShowStart);
                }
                if widgets::button(ui, ButtonKind::Ghost, icons::FOLDER_OPEN, "")
                    .on_hover_text("Show the content folder")
                    .clicked()
                {
                    action = Some(Action::Reveal(project.content.clone()));
                }
            });

            if let Some(a) = quick_actions(ui, jobs, index) {
                action = Some(a);
            }
            ui.add_space(theme::SPACE_LG);

            if let Some(a) = stats(ui, index) {
                action = Some(a);
            }
            ui.add_space(theme::SPACE_LG);

            ui.columns(2, |columns| {
                if let Some(a) = self.maps(&mut columns[0], project, index) {
                    action = Some(a);
                }
                about(&mut columns[1], project);
                columns[1].add_space(theme::SPACE_MD);
                if let Some(a) = activity(&mut columns[1], jobs) {
                    action = Some(a);
                }
            });
        });
        action
    }

    fn maps(&mut self, ui: &mut Ui, project: &ProjectInfo, index: &Index) -> Option<Action> {
        let mut action = None;
        let mut new_map = false;
        let maps: Vec<_> = index.of_kind(Kind::Map).collect();
        widgets::titled_card(
            ui,
            &format!("maps · {}", maps.len()),
            |ui| {
                if widgets::icon_button(ui, icons::PLUS, "New map").clicked() {
                    new_map = true;
                }
            },
            |ui| {
                if maps.is_empty() {
                    widgets::empty_state(
                        ui,
                        icons::MAP_TRIFOLD,
                        "No maps yet",
                        "New map opens the editor on a starter room.",
                    );
                    return;
                }
                if maps.len() > 6 {
                    widgets::search_field(
                        ui,
                        &mut self.map_filter,
                        "Filter maps",
                        ui.available_width(),
                    );
                    ui.add_space(theme::SPACE_XS);
                }
                let needle = self.map_filter.to_lowercase();
                ui.spacing_mut().item_spacing.y = 2.0;
                let start = project.start_map.as_deref();
                for map in maps
                    .iter()
                    .filter(|m| needle.is_empty() || m.relative.to_lowercase().contains(&needle))
                {
                    let name = map.relative.strip_prefix("maps/").unwrap_or(&map.relative);
                    let name = name
                        .strip_suffix(&format!(".{}", kerosene_vfs::ext::MAP))
                        .unwrap_or(name);
                    let detail = map
                        .modified
                        .and_then(|m| m.elapsed().ok())
                        .map(|age| format!("edited {}", widgets::human_age(age)))
                        .unwrap_or_default();
                    let row =
                        widgets::list_row(ui, icons::MAP_TRIFOLD, name, &detail, false, |ui| {
                            if let Some((label, tone)) = map.status.badge() {
                                widgets::badge(ui, label, tone);
                            }
                            if start == Some(name) {
                                widgets::badge(ui, "start", Tone::Accent)
                                    .on_hover_text("The map the game starts on");
                            }
                        });
                    if row.on_hover_text("Open in the editor").clicked() {
                        action = Some(Action::OpenMap(map.path.clone()));
                    }
                }
            },
        );
        if new_map {
            action = Some(Action::NewMap);
        }
        action
    }
}

/// The four things people come to do, as large buttons.
fn quick_actions(ui: &mut Ui, jobs: &Jobs<'_>, index: &Index) -> Option<Action> {
    let mut action = None;
    let stale = index.out_of_date();
    let build_detail = if jobs.building() {
        "building now...".to_string()
    } else if stale == 0 {
        "everything is up to date".to_string()
    } else {
        format!("{stale} source(s) changed")
    };
    let cards = [
        (
            icons::FILE_PLUS,
            "New map",
            "open the editor on a starter room".to_string(),
            false,
            Action::NewMap,
        ),
        (
            icons::HAMMER,
            "Build",
            build_detail,
            jobs.building(),
            Action::Build { fast: false },
        ),
        (
            icons::PACKAGE,
            "Pack",
            if jobs.packing() {
                "packing now...".to_string()
            } else {
                "write the archive the game ships".to_string()
            },
            jobs.packing(),
            Action::Pack,
        ),
        (
            icons::PLAY,
            "Play",
            if jobs.playing() {
                "running...".to_string()
            } else {
                "build what changed, then run".to_string()
            },
            jobs.playing(),
            Action::Play,
        ),
    ];
    ui.columns(4, |columns| {
        for (column, (glyph, title, detail, busy, chosen)) in columns.iter_mut().zip(cards) {
            if action_card(column, glyph, title, &detail, busy).clicked() && !busy {
                action = Some(chosen);
            }
        }
    });
    action
}

/// A large clickable card: an icon, a title, a line of detail.
fn action_card(ui: &mut Ui, glyph: &str, title: &str, detail: &str, busy: bool) -> egui::Response {
    let size = Vec2::new(ui.available_width(), 78.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = egui::CornerRadius::same(theme::RADIUS_LARGE);
        let hovered = response.hovered() && !busy;
        painter.rect_filled(
            rect,
            radius,
            if hovered {
                colors::BG_HEADER
            } else {
                colors::BG_ELEVATED
            },
        );
        painter.rect_stroke(
            rect,
            radius,
            egui::Stroke::new(
                1.0_f32,
                if hovered {
                    colors::ACCENT.gamma_multiply(0.6)
                } else {
                    colors::BORDER
                },
            ),
            egui::StrokeKind::Inside,
        );
        let tile = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 32.0, rect.center().y),
            Vec2::splat(36.0),
        );
        painter.rect_filled(
            tile,
            egui::CornerRadius::same(theme::RADIUS + 2),
            colors::ACCENT_SOFT,
        );
        painter.text(
            tile.center(),
            Align2::CENTER_CENTER,
            if busy { icons::CIRCLE_NOTCH } else { glyph },
            FontId::proportional(18.0),
            colors::ACCENT,
        );
        painter.text(
            egui::pos2(rect.left() + 60.0, rect.center().y - 9.0),
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(14.0),
            colors::TEXT,
        );
        painter.text(
            egui::pos2(rect.left() + 60.0, rect.center().y + 10.0),
            Align2::LEFT_CENTER,
            detail,
            FontId::proportional(11.0),
            colors::TEXT_MUTED,
        );
    }
    if busy {
        ui.ctx().request_repaint();
    }
    response
}

/// The counts, as tiles that open the asset browser on what they count.
fn stats(ui: &mut Ui, index: &Index) -> Option<Action> {
    let mut action = None;
    let compiled_maps = index.count(Kind::CompiledMap);
    let stale_maps = index
        .of_kind(Kind::Map)
        .filter(|m| m.status != Status::Compiled)
        .count();
    let tiles = [
        (
            Category::Maps,
            index.count(Kind::Map),
            if stale_maps == 0 {
                format!("{compiled_maps} compiled")
            } else {
                format!("{stale_maps} to build")
            },
        ),
        (
            Category::Materials,
            index.count(Kind::Material),
            "compiled".to_string(),
        ),
        (
            Category::Textures,
            index.count(Kind::Texture),
            format!("{} images", index.count(Kind::Image)),
        ),
        (
            Category::Models,
            index.count(Kind::Model),
            format!("{} meshes", index.count(Kind::Mesh)),
        ),
        (
            Category::Sounds,
            index.count(Kind::AudioSource),
            format!("{} compiled", index.count(Kind::Sound)),
        ),
        (
            Category::Scripts,
            index.count(Kind::Script),
            "source".to_string(),
        ),
    ];
    ui.label(theme::section_title("content"));
    ui.add_space(theme::SPACE_SM);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(theme::SPACE_MD);
        for (category, count, note) in tiles {
            if widgets::stat_tile(
                ui,
                category.glyph(),
                &category.label().to_lowercase(),
                &count.to_string(),
                &note,
            )
            .on_hover_text(format!("Browse {}", category.label().to_lowercase()))
            .clicked()
            {
                action = Some(Action::BrowseAssets(Some(category)));
            }
        }
    });
    action
}

/// Where everything is.
fn about(ui: &mut Ui, project: &ProjectInfo) {
    widgets::titled_card(
        ui,
        "project",
        |_| {},
        |ui| {
            match &project.file {
                Some(file) => widgets::fact(ui, "file", short(file)),
                None => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(theme::icon(icons::INFO).color(colors::INFO));
                        ui.label(theme::caption(format!(
                            "{}. No project file names this tree; \
                             Switch project → Make a project writes one.",
                            project.found_note
                        )));
                    });
                    ui.add_space(theme::SPACE_XS);
                }
            }
            widgets::fact(ui, "content", short(&project.content));
            if let Some(start) = &project.start_map {
                widgets::fact(ui, "starts on", start.clone());
            }
            widgets::fact(
                ui,
                "game",
                project
                    .game
                    .clone()
                    .unwrap_or_else(|| "the stock runtime".to_string()),
            );
            widgets::fact(ui, "archive", short(&project.archive));
        },
    );
}

/// The last run of each job.
fn activity(ui: &mut Ui, jobs: &Jobs<'_>) -> Option<Action> {
    let mut action = None;
    widgets::titled_card(
        ui,
        "recent activity",
        |ui| {
            if widgets::icon_button(ui, icons::TERMINAL_WINDOW, "Show the output panel").clicked() {
                action = Some(Action::ShowOutput);
            }
        },
        |ui| {
            let all = [jobs.build, jobs.archive, jobs.play];
            if all.iter().all(Option::is_none) {
                ui.label(theme::caption(
                    "Nothing has run yet. Builds, packs and play sessions show up here.",
                ));
                return;
            }
            for job in all.into_iter().flatten() {
                job_line(ui, job);
            }
        },
    );
    action
}

/// One job: its state, how long it took, and what went wrong.
pub fn job_line(ui: &mut Ui, job: &Job) {
    ui.horizontal(|ui| {
        match job.outcome() {
            None => {
                ui.add(egui::Spinner::new().size(13.0).color(colors::ACCENT));
            }
            Some(true) => {
                ui.label(theme::icon(icons::X_CIRCLE).color(colors::ERR));
            }
            Some(false) => {
                ui.label(theme::icon(icons::CHECK_CIRCLE).color(colors::OK));
            }
        }
        ui.label(egui::RichText::new(job.label()).strong());
        let state = match job.outcome() {
            None => format!("running · {}", widgets::human_elapsed(job.elapsed())),
            Some(_) if job.cancelled() => "cancelled".to_string(),
            Some(failed) => {
                let verdict = if failed { "failed" } else { "finished" };
                let ago = job
                    .finished_ago()
                    .map(widgets::human_age)
                    .unwrap_or_default();
                format!(
                    "{verdict} in {} · {ago}",
                    widgets::human_elapsed(job.elapsed())
                )
            }
        };
        ui.label(theme::caption(state));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (errors, warnings) = job.problems();
            if errors > 0 {
                widgets::badge(ui, &format!("{errors} errors"), Tone::Err);
            }
            if warnings > 0 {
                widgets::badge(ui, &format!("{warnings} warnings"), Tone::Warn);
            }
        });
    });
}

/// A path with the home directory folded to `~`, so the part that tells
/// projects apart is the part that shows.
pub fn short(path: &Path) -> String {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    if let Some(home) = home
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_draws_with_and_without_a_project() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let cases = [
            (
                ProjectInfo::new(PathBuf::from("/nowhere"), None, "inferred".into()),
                Index::default(),
            ),
            (
                ProjectInfo::new(root.clone(), None, "given".into()),
                Index::scan(&root),
            ),
        ];
        for (project, index) in cases {
            let mut page = HomePage::default();
            let jobs = Jobs {
                build: None,
                archive: None,
                play: None,
            };
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                assert_eq!(page.ui(ctx, &project, &index, &jobs), None);
            });
            assert!(!output.shapes.is_empty());
        }
    }

    #[test]
    fn a_project_without_a_file_is_named_for_its_directory() {
        let info = ProjectInfo::new(PathBuf::from("/games/arena"), None, "inferred".into());
        assert_eq!(info.name, "arena");
        assert!(info.file.is_none());
    }
}
