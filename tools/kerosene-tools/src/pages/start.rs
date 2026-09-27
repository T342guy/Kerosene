// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The start page: which project to work on.
//!
//! Shown when the toolset finds no content tree, and whenever someone asks
//! to switch. It lists the projects opened lately and offers the three ways
//! to get a new one -- open a folder, start a game, make a project of a
//! folder that has content but no project file -- which until now were a
//! path on the command line, `kerosene-tools new` and `kerosene-tools init`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use egui::{Align, Align2, FontId, Layout, Sense, Ui, Vec2};
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets::{self, Kind as ButtonKind};

use super::home::short;
use crate::recent::Recent;
use crate::toolset::Action;

/// The content tree a path the person typed or picked means: a project
/// file, a folder with one in it, a content tree, or a folder with a
/// content tree called `content` in it.
pub fn resolve(path: &Path) -> Result<PathBuf> {
    let path = expand_home(path);
    if path.is_file() {
        if !kerosene_vfs::ext::is(&path, kerosene_vfs::project::EXTENSION) {
            bail!("{} is not a project file", path.display());
        }
        return Ok(kerosene_vfs::Project::read(&path)?.content);
    }
    if !path.is_dir() {
        bail!("{} is not there", path.display());
    }
    if let Some(file) = kerosene_vfs::project::in_directory(&path) {
        return Ok(kerosene_vfs::Project::read(&file)
            .with_context(|| format!("reading {}", file.display()))?
            .content);
    }
    if kerosene_vfs::root::is_content_root(&path) {
        return Ok(path);
    }
    let inner = path.join("content");
    if kerosene_vfs::root::is_content_root(&inner) {
        return Ok(inner);
    }
    bail!(
        "{} is not a Kerosene project: it has no .{} file, and no maps/ and materials/ folders",
        path.display(),
        kerosene_vfs::project::EXTENSION
    )
}

/// `~/games` as the shell would read it.
fn expand_home(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match (text.strip_prefix("~"), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            PathBuf::from(home).join(rest.trim_start_matches(['/', '\\']))
        }
        _ => path.to_path_buf(),
    }
}

/// Which form is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    Open = 0,
    New = 1,
    Init = 2,
}

/// The page's state between frames.
pub struct StartPage {
    form: Form,
    open_path: String,
    new_name: String,
    new_location: String,
    new_game: bool,
    init_path: String,
    init_name: String,
    /// What the last form did: a success to report or an error to show.
    message: Option<Result<String, String>>,
}

impl Default for StartPage {
    fn default() -> StartPage {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|h| PathBuf::from(h).join("games").display().to_string())
            .unwrap_or_default();
        StartPage {
            form: Form::Open,
            open_path: String::new(),
            new_name: String::new(),
            new_location: home,
            new_game: true,
            init_path: String::new(),
            init_name: String::new(),
            message: None,
        }
    }
}

impl StartPage {
    /// Draw the page. `current` is the project open behind it, if one is,
    /// so the page can offer the way back.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        recent: &Recent,
        current: Option<&str>,
    ) -> Option<Action> {
        let mut action = None;
        widgets::page(ctx, 980.0, |ui| {
            hero(ui, current, &mut action);
            ui.add_space(theme::SPACE_XL);
            ui.columns(2, |columns| {
                if let Some(a) = recent_list(&mut columns[0], recent) {
                    action = Some(a);
                }
                if let Some(a) = self.forms(&mut columns[1]) {
                    action = Some(a);
                }
            });
        });
        action
    }

    fn forms(&mut self, ui: &mut Ui) -> Option<Action> {
        let mut action = None;
        widgets::card(ui, |ui| {
            // The columns this sits in stretch what they hold; a form's
            // buttons should be the size of their labels.
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                self.form_body(ui, &mut action)
            });
        });
        action
    }

    fn form_body(&mut self, ui: &mut Ui, action: &mut Option<Action>) {
        {
            let mut selected = self.form as usize;
            if widgets::tab_bar(
                ui,
                &[
                    (icons::FOLDER_OPEN, "Open"),
                    (icons::SPARKLE, "New game"),
                    (icons::FOLDER_PLUS, "Make a project"),
                ],
                &mut selected,
            ) {
                self.message = None;
            }
            self.form = match selected {
                1 => Form::New,
                2 => Form::Init,
                _ => Form::Open,
            };
            ui.add_space(theme::SPACE_MD);
            *action = match self.form {
                Form::Open => self.open_form(ui),
                Form::New => self.new_form(ui),
                Form::Init => self.init_form(ui),
            };
            if let Some(message) = &self.message {
                ui.add_space(theme::SPACE_SM);
                match message {
                    Ok(text) => ui.label(theme::ok(format!("{}  {text}", icons::CHECK_CIRCLE))),
                    Err(text) => ui.label(theme::err(format!("{}  {text}", icons::WARNING))),
                };
            }
        }
    }

    fn open_form(&mut self, ui: &mut Ui) -> Option<Action> {
        ui.label(theme::caption(
            "A project folder, its project file, or a content folder.",
        ));
        ui.add_space(theme::SPACE_XS);
        let field = path_field(ui, &mut self.open_path, "~/games/my-game");
        ui.add_space(theme::SPACE_SM);
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clicked =
            widgets::button(ui, ButtonKind::Primary, icons::FOLDER_OPEN, "Open project").clicked();
        if !(clicked || enter) || self.open_path.trim().is_empty() {
            return None;
        }
        match resolve(Path::new(self.open_path.trim())) {
            Ok(content) => {
                self.message = None;
                Some(Action::SwitchProject(content))
            }
            Err(e) => {
                self.message = Some(Err(format!("{e:#}")));
                None
            }
        }
    }

    fn new_form(&mut self, ui: &mut Ui) -> Option<Action> {
        labelled(ui, "Name", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text("Orbital Drift")
                    .margin(egui::Margin::symmetric(8, 6))
                    .desired_width(f32::INFINITY),
            );
        });
        labelled(ui, "Location", |ui| {
            path_field(ui, &mut self.new_location, "~/games");
        });
        labelled(ui, "Kind", |ui| {
            ui.radio_value(&mut self.new_game, true, "A game")
                .on_hover_text("A Cargo package with its own Rust code, a project and a map.");
            ui.radio_value(&mut self.new_game, false, "Content only")
                .on_hover_text("A project and a map on the stock runtime: no Rust, no build.");
        });
        let folder = self.new_folder();
        ui.add_space(theme::SPACE_XS);
        ui.label(theme::caption(match &folder {
            Some(folder) => format!("Creates {}", short(folder)),
            None => "Give the game a name.".to_string(),
        }));
        ui.add_space(theme::SPACE_SM);
        let create = ui
            .add_enabled_ui(folder.is_some(), |ui| {
                widgets::button(ui, ButtonKind::Primary, icons::SPARKLE, "Create")
            })
            .inner
            .clicked();
        let folder = folder?;
        if !create {
            return None;
        }
        match crate::new::create(&folder, self.new_name.trim(), !self.new_game) {
            Ok(content) => {
                self.message = Some(Ok(if self.new_game {
                    "Made. Run `cargo play` in the folder to build the game itself.".to_string()
                } else {
                    "Made.".to_string()
                }));
                Some(Action::SwitchProject(content))
            }
            Err(e) => {
                self.message = Some(Err(format!("{e:#}")));
                None
            }
        }
    }

    /// The folder a new game would be made in.
    fn new_folder(&self) -> Option<PathBuf> {
        let name = self.new_name.trim();
        if name.is_empty() || self.new_location.trim().is_empty() {
            return None;
        }
        let names = crate::new::Names::from_title(name);
        Some(expand_home(Path::new(self.new_location.trim())).join(names.package))
    }

    fn init_form(&mut self, ui: &mut Ui) -> Option<Action> {
        ui.label(theme::caption(
            "Writes a project file and the content folders a project needs. \
             Anything already there is left alone.",
        ));
        ui.add_space(theme::SPACE_XS);
        labelled(ui, "Folder", |ui| {
            path_field(ui, &mut self.init_path, "~/games/existing");
        });
        labelled(ui, "Name", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.init_name)
                    .hint_text("the folder's name")
                    .margin(egui::Margin::symmetric(8, 6))
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(theme::SPACE_SM);
        let ready = !self.init_path.trim().is_empty();
        let make = ui
            .add_enabled_ui(ready, |ui| {
                widgets::button(ui, ButtonKind::Primary, icons::FOLDER_PLUS, "Make project")
            })
            .inner
            .clicked();
        if !make {
            return None;
        }
        let dir = expand_home(Path::new(self.init_path.trim()));
        let name = Some(self.init_name.trim()).filter(|n| !n.is_empty());
        match crate::init::init_project(&dir, name, "content") {
            Ok(made) => {
                self.message = Some(Ok(if made.existed {
                    format!("{} was already a project.", short(&made.project))
                } else {
                    format!("Wrote {}.", short(&made.project))
                }));
                Some(Action::SwitchProject(made.content))
            }
            Err(e) => {
                self.message = Some(Err(format!("{e:#}")));
                None
            }
        }
    }
}

/// The logo, the name, and the way back to the open project.
fn hero(ui: &mut Ui, current: Option<&str>, action: &mut Option<Action>) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(14), colors::ACCENT);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            icons::FIRE,
            FontId::proportional(36.0),
            colors::ON_ACCENT,
        );
        ui.add_space(theme::SPACE_SM);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Kerosene").size(30.0).strong());
            ui.label(theme::subtitle("Open a project, or start a new one."));
        });
        if let Some(current) = current {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::button(
                    ui,
                    ButtonKind::Secondary,
                    icons::ARROW_LEFT,
                    &format!("Back to {current}"),
                )
                .clicked()
                {
                    *action = Some(Action::HideStart);
                }
            });
        }
    });
}

fn recent_list(ui: &mut Ui, recent: &Recent) -> Option<Action> {
    let mut action = None;
    widgets::titled_card(
        ui,
        "recent projects",
        |_| {},
        |ui| {
            if recent.entries.is_empty() {
                widgets::empty_state(
                    ui,
                    icons::CLOCK_COUNTER_CLOCKWISE,
                    "No recent projects",
                    "Projects you open are listed here.",
                );
                return;
            }
            ui.spacing_mut().item_spacing.y = 2.0;
            for entry in &recent.entries {
                let exists = entry.exists();
                let detail = if exists {
                    format!(
                        "{} · {}",
                        short(&entry.content),
                        widgets::human_age(entry.age())
                    )
                } else {
                    format!("{} · missing", short(&entry.content))
                };
                let glyph = if exists {
                    icons::FOLDER_SIMPLE
                } else {
                    icons::FOLDER_DASHED
                };
                let mut forget = false;
                let row = widgets::list_row(ui, glyph, &entry.name, &detail, false, |ui| {
                    if widgets::icon_button(ui, icons::X, "Remove from the list").clicked() {
                        forget = true;
                    }
                });
                if forget {
                    action = Some(Action::ForgetProject(entry.content.clone()));
                } else if row.clicked() && exists {
                    action = Some(Action::SwitchProject(entry.content.clone()));
                }
            }
        },
    );
    action
}

fn labelled(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(70.0, 24.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.0),
            colors::TEXT_MUTED,
        );
        add(ui);
    });
    ui.add_space(theme::SPACE_XS);
}

fn path_field(ui: &mut Ui, text: &mut String, hint: &str) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .font(egui::TextStyle::Monospace)
            .margin(egui::Margin::symmetric(8, 6))
            .desired_width(f32::INFINITY),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kerosene-start-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_folder_a_project_file_or_a_content_tree_all_resolve() {
        let dir = scratch("resolve");
        let made = crate::init::init_project(&dir.join("game"), Some("Game"), "content").unwrap();
        let content = made.content.clone();
        assert_eq!(resolve(&dir.join("game")).unwrap(), content);
        assert_eq!(resolve(&made.project).unwrap(), content);
        assert_eq!(resolve(&content).unwrap(), content);
        assert!(resolve(&dir.join("nothing")).is_err());
        std::fs::create_dir_all(dir.join("plain")).unwrap();
        let err = resolve(&dir.join("plain")).unwrap_err().to_string();
        assert!(err.contains("not a Kerosene project"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_new_form_names_the_folder_after_the_game() {
        let page = StartPage {
            new_name: "Orbital Drift".into(),
            new_location: "/games".into(),
            ..Default::default()
        };
        assert_eq!(
            page.new_folder(),
            Some(PathBuf::from("/games/orbital-drift"))
        );
        let unnamed = StartPage {
            new_name: "  ".into(),
            ..Default::default()
        };
        assert_eq!(unnamed.new_folder(), None);
    }

    #[test]
    fn a_content_only_project_is_made_and_opens() {
        let dir = scratch("new");
        let content = crate::new::create(&dir.join("drift"), "Drift", true).unwrap();
        assert!(content.join("maps/drift_start.kmap").is_file());
        assert_eq!(resolve(&dir.join("drift")).unwrap(), content);
        assert!(
            crate::new::create(&dir.join("drift"), "Drift", true).is_err(),
            "a folder with files in it is not overwritten"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_page_draws_every_form() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut recent = Recent::default();
        recent.add(Path::new("/nowhere/content"), "Gone");
        for form in [Form::Open, Form::New, Form::Init] {
            let mut page = StartPage {
                form,
                ..Default::default()
            };
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                assert_eq!(page.ui(ctx, &recent, Some("Current")), None);
            });
            assert!(!output.shapes.is_empty());
        }
    }
}
