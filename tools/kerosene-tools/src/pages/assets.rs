// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The Assets tab: every file in the content tree, what it is, and whether
//! its compiled form is up to date.
//!
//! The editor lists materials and the model viewer lists models, but nothing
//! answered "what is in this project" -- or the question that comes just
//! before a broken build, "which of these did I change since it was last
//! compiled". The [`Index`] answers both, from one walk of the tree, and the
//! project page and the command palette read the same index rather than
//! each walking the disk for themselves.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align, Align2, FontId, Layout, Rect, Sense, Ui, Vec2};
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets::{self, Kind as ButtonKind, Tone};
use kerosene_vfs::ext;

use crate::toolset::Action;

/// What a file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Map,
    CompiledMap,
    Material,
    CompiledMaterial,
    Texture,
    Image,
    Model,
    Mesh,
    Sound,
    AudioSource,
    Script,
    Ui,
    Config,
    Archive,
    /// What a map compile leaves beside a map: portals, leak traces, walk
    /// graphs, build stamps.
    Artefact,
    Other,
}

impl Kind {
    /// What a file with this extension, at this path under the content
    /// root, is.
    pub fn of(relative: &str) -> Kind {
        let ext = relative
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            e if e == ext::MAP => Kind::Map,
            e if e == ext::BSP => Kind::CompiledMap,
            e if e == ext::MATERIAL => Kind::Material,
            e if e == ext::MATERIAL_COMPILED => Kind::CompiledMaterial,
            e if e == ext::TEXTURE => Kind::Texture,
            e if e == ext::MODEL => Kind::Model,
            e if e == ext::AUDIO => Kind::Sound,
            e if e == ext::SCRIPT => Kind::Script,
            e if e == ext::UI_LAYOUT || e == ext::UI_STYLE => Kind::Ui,
            e if e == ext::CONFIG
                || e == ext::PROJECT
                || e == ext::CLASSES
                || e == ext::SOUNDSCRIPT =>
            {
                Kind::Config
            }
            e if e == ext::ARCHIVE => Kind::Archive,
            e if e == ext::PORTALS || e == ext::LEAK || e == ext::WALK || e == ext::BUILD_STAMP => {
                Kind::Artefact
            }
            "rhai" => Kind::Script,
            "png" | "jpg" | "jpeg" | "tga" | "bmp" => Kind::Image,
            "obj" | "gltf" | "glb" => Kind::Mesh,
            "wav" | "flac" | "mp3" | "ogg" => Kind::AudioSource,
            "ttf" | "otf" => Kind::Ui,
            _ => Kind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Map => "Map",
            Kind::CompiledMap => "Compiled map",
            Kind::Material => "Material",
            Kind::CompiledMaterial => "Compiled material",
            Kind::Texture => "Texture",
            Kind::Image => "Image",
            Kind::Model => "Model",
            Kind::Mesh => "Mesh",
            Kind::Sound => "Compiled sound",
            Kind::AudioSource => "Sound",
            Kind::Script => "Script",
            Kind::Ui => "UI",
            Kind::Config => "Config",
            Kind::Archive => "Archive",
            Kind::Artefact => "Build output",
            Kind::Other => "Other",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Kind::Map => icons::MAP_TRIFOLD,
            Kind::CompiledMap => icons::CUBE,
            Kind::Material | Kind::CompiledMaterial => icons::PAINT_BUCKET,
            Kind::Texture | Kind::Image => icons::IMAGE,
            Kind::Model | Kind::Mesh => icons::PACKAGE,
            Kind::Sound | Kind::AudioSource => icons::SPEAKER_HIGH,
            Kind::Script => icons::SCROLL,
            Kind::Ui => icons::LAYOUT,
            Kind::Config => icons::GEAR_SIX,
            Kind::Archive => icons::ARCHIVE,
            Kind::Artefact => icons::GEAR,
            Kind::Other => icons::FILE,
        }
    }

    pub fn category(self) -> Category {
        match self {
            Kind::Map | Kind::CompiledMap => Category::Maps,
            Kind::Material | Kind::CompiledMaterial => Category::Materials,
            Kind::Texture | Kind::Image => Category::Textures,
            Kind::Model | Kind::Mesh => Category::Models,
            Kind::Sound | Kind::AudioSource => Category::Sounds,
            Kind::Script => Category::Scripts,
            Kind::Ui => Category::Ui,
            Kind::Config | Kind::Archive | Kind::Artefact | Kind::Other => Category::Other,
        }
    }
}

/// A group of kinds, as the tab's side list shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Maps,
    Materials,
    Textures,
    Models,
    Sounds,
    Scripts,
    Ui,
    Other,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Maps,
        Category::Materials,
        Category::Textures,
        Category::Models,
        Category::Sounds,
        Category::Scripts,
        Category::Ui,
        Category::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Maps => "Maps",
            Category::Materials => "Materials",
            Category::Textures => "Textures",
            Category::Models => "Models",
            Category::Sounds => "Sounds",
            Category::Scripts => "Scripts",
            Category::Ui => "UI",
            Category::Other => "Other",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Category::Maps => icons::MAP_TRIFOLD,
            Category::Materials => icons::PAINT_BUCKET,
            Category::Textures => icons::IMAGE,
            Category::Models => icons::PACKAGE,
            Category::Sounds => icons::SPEAKER_HIGH,
            Category::Scripts => icons::SCROLL,
            Category::Ui => icons::LAYOUT,
            Category::Other => icons::FOLDER,
        }
    }
}

/// Whether a source's compiled form is there and newer than it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Status {
    /// Compiled, and newer than its source.
    Compiled,
    /// Compiled, but the source has changed since.
    Stale,
    /// Never compiled.
    Missing,
    /// Not a source anything compiles.
    NotApplicable,
}

impl Status {
    pub fn badge(self) -> Option<(&'static str, Tone)> {
        match self {
            Status::Compiled => Some(("compiled", Tone::Ok)),
            Status::Stale => Some(("stale", Tone::Warn)),
            Status::Missing => Some(("not compiled", Tone::Err)),
            Status::NotApplicable => None,
        }
    }
}

/// One file in the content tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    /// Relative to the content root, with forward slashes.
    pub relative: String,
    pub kind: Kind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub status: Status,
}

impl Entry {
    /// The file's name without its directories.
    pub fn file_name(&self) -> &str {
        self.relative.rsplit('/').next().unwrap_or(&self.relative)
    }

    /// What opening this file does in the toolset, when it opens anywhere.
    pub fn open_action(&self) -> Option<Action> {
        match self.kind {
            Kind::Map => Some(Action::OpenMap(self.path.clone())),
            Kind::CompiledMap => {
                let source = self.path.with_extension(ext::MAP);
                source.is_file().then_some(Action::OpenMap(source))
            }
            Kind::Model => {
                let name = self.relative.strip_prefix("models/")?;
                let name = name.strip_suffix(&format!(".{}", ext::MODEL))?;
                Some(Action::OpenModel(name.to_string()))
            }
            Kind::AudioSource if self.relative.starts_with("sound/") => {
                Some(Action::OpenSound(self.path.clone()))
            }
            _ => None,
        }
    }
}

/// The compiled file a source becomes, when it becomes one.
fn output_of(root: &Path, relative: &str, path: &Path, kind: Kind) -> Option<PathBuf> {
    match kind {
        Kind::Map => Some(path.with_extension(ext::BSP)),
        Kind::Material => Some(path.with_extension(ext::MATERIAL_COMPILED)),
        // Forge compiles `art/<name>.obj` to `models/<name>.kmdl`, as Kiln
        // runs it; a mesh anywhere else is not built by anything.
        Kind::Mesh => kiln::model_output(root, path),
        Kind::AudioSource if relative.starts_with("sound/") => Some(timbre::output_for(path)),
        Kind::Image => {
            // Alchemy compiles `art/<name>.png` to `materials/<name>.ktex`.
            let rest = relative.strip_prefix("art/")?;
            Some(
                root.join("materials")
                    .join(rest)
                    .with_extension(ext::TEXTURE),
            )
        }
        _ => None,
    }
}

fn status_of(output: Option<&Path>, source: &Path) -> Status {
    match output {
        None => Status::NotApplicable,
        Some(output) if !output.is_file() => Status::Missing,
        Some(output) if kerosene_vfs::up_to_date(output, &[source]) => Status::Compiled,
        Some(_) => Status::Stale,
    }
}

/// Every file under a content root, walked once.
#[derive(Clone, Debug, Default)]
pub struct Index {
    pub root: PathBuf,
    /// Sorted by relative path.
    pub entries: Vec<Entry>,
}

impl Index {
    /// Walk the tree. A root that is not there is an empty index.
    pub fn scan(root: &Path) -> Index {
        let mut files = Vec::new();
        walk(root, &mut files);
        let mut entries: Vec<Entry> = files
            .into_iter()
            .filter_map(|path| {
                let relative = path
                    .strip_prefix(root)
                    .ok()?
                    .to_string_lossy()
                    .replace('\\', "/");
                let meta = std::fs::metadata(&path).ok()?;
                let kind = Kind::of(&relative);
                let output = output_of(root, &relative, &path, kind);
                let status = status_of(output.as_deref(), &path);
                Some(Entry {
                    relative,
                    kind,
                    size: meta.len(),
                    modified: meta.modified().ok(),
                    status,
                    path,
                })
            })
            .collect();
        entries.sort_by(|a, b| a.relative.cmp(&b.relative));
        Index {
            root: root.to_path_buf(),
            entries,
        }
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }

    pub fn in_category(&self, category: Category) -> usize {
        self.entries
            .iter()
            .filter(|e| e.kind.category() == category)
            .count()
    }

    pub fn of_kind(&self, kind: Kind) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(move |e| e.kind == kind)
    }

    /// Sources whose compiled form is missing or older than they are.
    pub fn out_of_date(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, Status::Stale | Status::Missing))
            .count()
    }

    pub fn total_size(&self) -> u64 {
        self.entries.iter().map(|e| e.size).sum()
    }
}

fn walk(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        // Hidden files and directories are a tool's or a version control
        // system's, not the project's.
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk(&path, into);
        } else {
            into.push(path);
        }
    }
}

// ---- the tab ---------------------------------------------------------------

/// What the table is sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Column {
    Name,
    Kind,
    Size,
    Modified,
    Status,
}

/// The tab's state between frames.
pub struct AssetsPage {
    /// `None` is every category.
    pub category: Option<Category>,
    pub query: String,
    sort: Column,
    ascending: bool,
    selected: Option<PathBuf>,
}

impl Default for AssetsPage {
    fn default() -> AssetsPage {
        AssetsPage {
            category: None,
            query: String::new(),
            sort: Column::Name,
            ascending: true,
            selected: None,
        }
    }
}

const ROW: f32 = 30.0;

impl AssetsPage {
    /// The entries the table shows: in the category, matching the query,
    /// in the chosen order.
    pub fn visible<'a>(&self, index: &'a Index) -> Vec<&'a Entry> {
        let needle = self.query.trim().to_lowercase();
        let mut rows: Vec<&Entry> = index
            .entries
            .iter()
            .filter(|e| self.category.is_none_or(|c| e.kind.category() == c))
            .filter(|e| needle.is_empty() || e.relative.to_lowercase().contains(&needle))
            .collect();
        match self.sort {
            Column::Name => {}
            Column::Kind => rows.sort_by_key(|e| e.kind.label()),
            Column::Size => rows.sort_by_key(|e| e.size),
            Column::Modified => rows.sort_by_key(|e| e.modified),
            Column::Status => rows.sort_by_key(|e| e.status),
        }
        if !self.ascending {
            rows.reverse();
        }
        rows
    }

    pub fn ui(&mut self, ctx: &egui::Context, index: &Index) -> Option<Action> {
        let mut action = None;
        egui::SidePanel::left("assets-categories")
            .exact_width(190.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(10, 16)),
            )
            .show(ctx, |ui| self.categories(ui, index));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_APP)
                    .inner_margin(egui::Margin::symmetric(20, 16)),
            )
            .show(ctx, |ui| {
                if let Some(a) = self.header(ui, index) {
                    action = Some(a);
                }
                ui.add_space(theme::SPACE_MD);
                if let Some(a) = self.table(ui, index) {
                    action = Some(a);
                }
            });
        action
    }

    fn categories(&mut self, ui: &mut Ui, index: &Index) {
        ui.label(theme::section_title("browse"));
        ui.add_space(theme::SPACE_SM);
        ui.spacing_mut().item_spacing.y = 2.0;
        let all = index.entries.len();
        if category_row(
            ui,
            icons::SQUARES_FOUR,
            "Everything",
            all,
            self.category.is_none(),
        )
        .clicked()
        {
            self.category = None;
        }
        ui.add_space(theme::SPACE_XS);
        for category in Category::ALL {
            let count = index.in_category(category);
            let selected = self.category == Some(category);
            if category_row(ui, category.glyph(), category.label(), count, selected).clicked() {
                self.category = Some(category);
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.label(theme::caption(format!(
                "{} files · {}",
                all,
                widgets::human_size(index.total_size())
            )));
            let stale = index.out_of_date();
            if stale > 0 {
                ui.label(theme::warn(format!("{stale} need building")).size(11.5));
            }
        });
    }

    fn header(&mut self, ui: &mut Ui, index: &Index) -> Option<Action> {
        let mut action = None;
        ui.horizontal(|ui| {
            let title = self.category.map_or("Everything", |c| c.label());
            ui.label(theme::heading(title));
            ui.label(theme::caption(format!(
                "{} shown",
                self.visible(index).len()
            )));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::button(ui, ButtonKind::Secondary, icons::ARROW_CLOCKWISE, "Rescan")
                    .on_hover_text("Walk the content tree again")
                    .clicked()
                {
                    action = Some(Action::RescanAssets);
                }
                if widgets::button(ui, ButtonKind::Secondary, icons::FOLDER_OPEN, "Open folder")
                    .on_hover_text(index.root.display().to_string())
                    .clicked()
                {
                    action = Some(Action::Reveal(index.root.clone()));
                }
                widgets::search_field(ui, &mut self.query, "Filter by name or path", 260.0);
            });
        });
        action
    }

    fn table(&mut self, ui: &mut Ui, index: &Index) -> Option<Action> {
        let rows = self.visible(index);
        if rows.is_empty() {
            let (title, detail) = if index.entries.is_empty() {
                (
                    "The content tree is empty",
                    "Files you add under the content folder show up here.",
                )
            } else {
                (
                    "Nothing matches",
                    "Try another category, or clear the filter.",
                )
            };
            widgets::empty_state(ui, icons::FOLDER_DASHED, title, detail);
            return None;
        }

        let mut action = None;
        widgets::card_frame()
            .inner_margin(egui::Margin::same(0))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                self.column_headers(ui);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW, rows.len(), |ui, range| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (i, entry) in rows[range.clone()].iter().enumerate() {
                            let striped = (range.start + i) % 2 == 1;
                            if let Some(a) = self.row(ui, entry, striped) {
                                action = Some(a);
                            }
                        }
                    });
            });
        action
    }

    fn columns(width: f32) -> [f32; 5] {
        // Name takes what the fixed columns leave.
        let fixed = [120.0, 80.0, 100.0, 110.0];
        let name = (width - fixed.iter().sum::<f32>() - 16.0).max(160.0);
        [name, fixed[0], fixed[1], fixed[2], fixed[3]]
    }

    fn column_headers(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::hover());
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius {
                nw: theme::RADIUS_LARGE,
                ne: theme::RADIUS_LARGE,
                sw: 0,
                se: 0,
            },
            colors::BG_HEADER,
        );
        let widths = Self::columns(width);
        let names = [
            (Column::Name, "NAME"),
            (Column::Kind, "TYPE"),
            (Column::Size, "SIZE"),
            (Column::Modified, "MODIFIED"),
            (Column::Status, "STATUS"),
        ];
        let mut x = rect.left() + 12.0;
        for ((column, name), w) in names.into_iter().zip(widths) {
            let cell = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(w, rect.height()));
            let response = ui.interact(cell, ui.id().with(name), Sense::click());
            let current = self.sort == column;
            let text = if current {
                let arrow = if self.ascending {
                    icons::CARET_UP
                } else {
                    icons::CARET_DOWN
                };
                format!("{name} {arrow}")
            } else {
                name.to_string()
            };
            ui.painter().text(
                cell.left_center(),
                Align2::LEFT_CENTER,
                text,
                FontId::proportional(10.5),
                if current || response.hovered() {
                    colors::TEXT
                } else {
                    colors::TEXT_MUTED
                },
            );
            if response.clicked() {
                if current {
                    self.ascending = !self.ascending;
                } else {
                    self.sort = column;
                    self.ascending = true;
                }
            }
            x += w;
        }
    }

    fn row(&mut self, ui: &mut Ui, entry: &Entry, striped: bool) -> Option<Action> {
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW), Sense::click());
        let selected = self.selected.as_deref() == Some(entry.path.as_path());
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            if selected {
                painter.rect_filled(rect, 0.0, colors::ACCENT_SOFT);
            } else if response.hovered() {
                painter.rect_filled(rect, 0.0, colors::HOVER);
            } else if striped {
                painter.rect_filled(rect, 0.0, colors::BG_PANEL);
            }
            let widths = Self::columns(width);
            let mut x = rect.left() + 12.0;
            let y = rect.center().y;

            painter.text(
                egui::pos2(x + 8.0, y),
                Align2::CENTER_CENTER,
                entry.kind.glyph(),
                FontId::proportional(14.0),
                if selected {
                    colors::ACCENT
                } else {
                    colors::TEXT_MUTED
                },
            );
            let name = truncated(
                ui,
                &entry.relative,
                widths[0] - 34.0,
                FontId::monospace(12.0),
                colors::TEXT,
            );
            painter.galley(
                egui::pos2(x + 26.0, y - name.size().y / 2.0),
                name,
                colors::TEXT,
            );
            x += widths[0];

            let cells = [
                entry.kind.label().to_string(),
                widgets::human_size(entry.size),
                entry
                    .modified
                    .and_then(|m| m.elapsed().ok())
                    .map(widgets::human_age)
                    .unwrap_or_default(),
            ];
            for (text, w) in cells.into_iter().zip(&widths[1..4]) {
                painter.text(
                    egui::pos2(x, y),
                    Align2::LEFT_CENTER,
                    text,
                    FontId::proportional(12.0),
                    colors::TEXT_MUTED,
                );
                x += w;
            }
            if let Some((label, tone)) = entry.status.badge() {
                let cell =
                    Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(widths[4], ROW));
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(cell)
                        .layout(Layout::left_to_right(Align::Center)),
                    |ui| {
                        widgets::badge(ui, label, tone);
                    },
                );
            }
        }

        let mut action = None;
        if response.clicked() {
            self.selected = Some(entry.path.clone());
        }
        if response.double_clicked() {
            action = entry
                .open_action()
                .or(Some(Action::Reveal(entry.path.clone())));
        }
        let open = entry.open_action();
        let response = response.on_hover_text(if open.is_some() {
            "Double-click to open. Right-click for more."
        } else {
            "Right-click for more."
        });
        response.context_menu(|ui| {
            ui.label(theme::caption(entry.file_name()));
            ui.separator();
            if let Some(open) = &open
                && ui
                    .button(format!("{}  Open", icons::ARROW_SQUARE_OUT))
                    .clicked()
            {
                action = Some(open.clone());
                ui.close();
            }
            if ui
                .button(format!("{}  Show in folder", icons::FOLDER_OPEN))
                .clicked()
            {
                action = Some(Action::Reveal(entry.path.clone()));
                ui.close();
            }
            if ui.button(format!("{}  Copy path", icons::COPY)).clicked() {
                ui.ctx().copy_text(entry.path.display().to_string());
                ui.close();
            }
            if ui
                .button(format!("{}  Copy content path", icons::LINK))
                .clicked()
            {
                ui.ctx().copy_text(entry.relative.clone());
                ui.close();
            }
        });
        action
    }
}

fn category_row(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    count: usize,
    selected: bool,
) -> egui::Response {
    widgets::list_row(ui, glyph, label, "", selected, |ui| {
        ui.label(theme::caption(count.to_string()).color(if count == 0 {
            colors::TEXT_FAINT
        } else {
            colors::TEXT_MUTED
        }));
    })
}

/// Text cut to fit, with an ellipsis where it was cut.
fn truncated(
    ui: &Ui,
    text: &str,
    width: f32,
    font: FontId,
    colour: egui::Color32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(
        text.to_string(),
        TextFormat {
            font_id: font,
            color: colour,
            ..Default::default()
        },
    );
    job.wrap = TextWrapping::truncate_at_width(width.max(10.0));
    ui.fonts(|f| f.layout_job(job))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kerosene-assets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, relative: &str) -> PathBuf {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, relative).unwrap();
        path
    }

    /// Make `path` look older than anything written after it.
    fn age(path: &Path) {
        let old = SystemTime::now() - Duration::from_secs(3600);
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(old).unwrap();
    }

    #[test]
    fn files_are_known_by_their_extension() {
        assert_eq!(Kind::of("maps/arena.kmap"), Kind::Map);
        assert_eq!(Kind::of("maps/arena.KBSP"), Kind::CompiledMap);
        assert_eq!(Kind::of("materials/dev/grid.kmat"), Kind::Material);
        assert_eq!(
            Kind::of("materials/dev/grid.kmat_c"),
            Kind::CompiledMaterial
        );
        assert_eq!(Kind::of("art/dev/grid.png"), Kind::Image);
        assert_eq!(Kind::of("models/props/crate.kmdl"), Kind::Model);
        assert_eq!(Kind::of("sound/ui/click.wav"), Kind::AudioSource);
        assert_eq!(Kind::of("ui/hud.kui"), Kind::Ui);
        assert_eq!(Kind::of("engine.kcfg"), Kind::Config);
        assert_eq!(Kind::of("maps/arena.kprt"), Kind::Artefact);
        assert_eq!(Kind::of("README"), Kind::Other);
    }

    #[test]
    fn a_source_is_compiled_stale_or_missing() {
        let root = scratch("status");
        let fresh = write(&root, "maps/fresh.kmap");
        age(&fresh);
        write(&root, "maps/fresh.kbsp");
        let stale_bsp = write(&root, "maps/stale.kbsp");
        age(&stale_bsp);
        write(&root, "maps/stale.kmap");
        write(&root, "maps/never.kmap");
        write(&root, "sound/ui/click.wav");
        write(&root, ".git/config");

        let index = Index::scan(&root);
        let status = |name: &str| {
            index
                .entries
                .iter()
                .find(|e| e.relative == name)
                .map(|e| e.status)
        };
        assert_eq!(status("maps/fresh.kmap"), Some(Status::Compiled));
        assert_eq!(status("maps/stale.kmap"), Some(Status::Stale));
        assert_eq!(status("maps/never.kmap"), Some(Status::Missing));
        assert_eq!(status("sound/ui/click.wav"), Some(Status::Missing));
        assert_eq!(status("maps/fresh.kbsp"), Some(Status::NotApplicable));
        assert_eq!(
            status(".git/config"),
            None,
            "hidden directories are skipped"
        );
        assert_eq!(index.count(Kind::Map), 3);
        assert_eq!(index.out_of_date(), 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opening_follows_what_the_file_is() {
        let root = scratch("open");
        write(&root, "maps/a.kmap");
        write(&root, "maps/a.kbsp");
        write(&root, "models/props/crate.kmdl");
        write(&root, "sound/door/move.wav");
        write(&root, "ui/hud.kui");
        let index = Index::scan(&root);
        let open = |name: &str| {
            index
                .entries
                .iter()
                .find(|e| e.relative == name)
                .and_then(Entry::open_action)
        };
        assert_eq!(
            open("maps/a.kmap"),
            Some(Action::OpenMap(root.join("maps/a.kmap")))
        );
        assert_eq!(
            open("maps/a.kbsp"),
            Some(Action::OpenMap(root.join("maps/a.kmap")))
        );
        assert_eq!(
            open("models/props/crate.kmdl"),
            Some(Action::OpenModel("props/crate".into()))
        );
        assert_eq!(
            open("sound/door/move.wav"),
            Some(Action::OpenSound(root.join("sound/door/move.wav")))
        );
        assert_eq!(open("ui/hud.kui"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_table_filters_by_category_and_query_and_sorts() {
        let root = scratch("filter");
        write(&root, "maps/arena.kmap");
        write(&root, "maps/lobby.kmap");
        write(&root, "materials/dev/grid.kmat");
        let index = Index::scan(&root);
        let mut page = AssetsPage {
            category: Some(Category::Maps),
            ..Default::default()
        };
        assert_eq!(page.visible(&index).len(), 2);
        page.query = "LOB".into();
        assert_eq!(page.visible(&index)[0].relative, "maps/lobby.kmap");
        page.query.clear();
        page.category = None;
        page.ascending = false;
        assert_eq!(page.visible(&index)[0].relative, "materials/dev/grid.kmat");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_shipped_content_is_indexed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let index = Index::scan(&root);
        assert!(index.count(Kind::Map) > 0, "the repository ships a map");
        assert!(index.count(Kind::Material) > 0);
    }

    #[test]
    fn the_tab_draws_empty_and_full() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        for index in [Index::default(), Index::scan(&root)] {
            let mut page = AssetsPage::default();
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                assert_eq!(page.ui(ctx, &index), None);
            });
            assert!(!output.shapes.is_empty());
        }
    }
}
