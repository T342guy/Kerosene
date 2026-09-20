// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Loupe: a model viewer.
//!
//! A model is referenced in a map, or in a class's schema, by a bare path --
//! `props/crate_wood` -- and until now the only way to find out what one
//! actually looks like was to place it, compile the map, and go and look.
//! This is Half-Life Model Viewer's answer to the same problem, for
//! `.keromdl`: pick a model from the content tree, spin it, and read off its
//! meshes, materials and bones without touching a map at all.
//!
//! It draws through [`chisel::preview`], the same small rasteriser the asset
//! browser's thumbnails and the placement ghost already use, rather than a
//! second copy of it -- so a model looks the same here as it does everywhere
//! else in the toolset.

use chisel::textures::TextureCache;
use egui::Color32;
use kerosene_asset::Model;
use kerosene_ui::theme::{self, colors, icons};
use kerosene_ui::widgets;
use kerosene_vfs::Vfs;
use std::path::PathBuf;

/// How the orbit camera starts: a three-quarter view, the angle a product
/// shot is taken from and the one that reads a shape fastest.
const DEFAULT_YAW: f32 = 35.0;
const DEFAULT_PITCH: f32 = -20.0;

const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 6.0;

pub struct LoupeApp {
    root: PathBuf,
    vfs: Vfs,
    /// Every `.keromdl` under `models/`, relative and without the extension --
    /// the same names a schema's `model` key holds.
    names: Vec<String>,
    filter: String,
    selected: Option<String>,
    model: Option<Model>,
    /// Why loading the selected model failed, if it did. Shown instead of a
    /// silently empty viewer, so a typo'd path or a stale file says so.
    load_error: Option<String>,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    render: Option<Rendered>,
    /// Textures the selected model's materials resolve to, loaded through the
    /// same VFS and compiled `.kerotex` path the 3D pane and asset browser
    /// use -- so a model looks the same here as it would placed in a level.
    textures: TextureCache,
}

/// The last frame drawn, and what it was drawn from -- so orbiting only
/// re-rasterises when the orbit, or the model, actually changed.
struct Rendered {
    key: (String, usize, i32, i32, i32, usize),
    texture: egui::TextureHandle,
}

impl LoupeApp {
    /// Open against a content tree. Never fails: an empty or missing
    /// `models/` directory is a viewer with nothing in its list yet, not a
    /// reason to refuse to open, the same tolerance Chisel's own asset
    /// browser has for an incomplete project.
    pub fn open(root: PathBuf) -> LoupeApp {
        let mut vfs = Vfs::new();
        vfs.add_directory(&root, "GAME");
        let names = chisel::app::scan_models(&root);
        LoupeApp {
            root,
            vfs,
            names,
            filter: String::new(),
            selected: None,
            model: None,
            load_error: None,
            yaw: DEFAULT_YAW,
            pitch: DEFAULT_PITCH,
            zoom: 1.0,
            render: None,
            textures: TextureCache::new(),
        }
    }

    /// Re-scan `models/` -- the toolset calls this when its project tab
    /// notices the content tree changed under it.
    pub fn refresh(&mut self) {
        self.names = chisel::app::scan_models(&self.root);
        self.textures.clear();
        self.render = None;
    }

    fn select(&mut self, name: &str) {
        self.selected = Some(name.to_string());
        self.load_error = None;
        self.render = None;
        self.model = match self
            .vfs
            .read(&format!("models/{name}.keromdl"))
            .map_err(|e| e.to_string())
            .and_then(|bytes| Model::from_bytes(&bytes).map_err(|e| e.to_string()))
        {
            Ok(model) => Some(model),
            Err(e) => {
                self.load_error = Some(e);
                None
            }
        };
    }

    fn model_list(&mut self, ui: &mut egui::Ui) {
        widgets::section(ui, "models", |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::icon(icons::MAGNIFYING_GLASS).color(colors::TEXT_MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .desired_width(f32::INFINITY)
                        .hint_text("filter models"),
                );
                // A model Forge just compiled has no other way to appear
                // here: nothing tells this list the content tree changed.
                if widgets::icon_button(ui, icons::ARROW_CLOCKWISE, "rescan models/").clicked() {
                    self.refresh();
                }
            });
            ui.add_space(2.0);

            let filter = self.filter.to_ascii_lowercase();
            let names: Vec<String> = self
                .names
                .iter()
                .filter(|n| filter.is_empty() || n.to_ascii_lowercase().contains(&filter))
                .cloned()
                .collect();
            if names.is_empty() {
                ui.label(theme::caption(if self.names.is_empty() {
                    "no .keromdl files found under models/"
                } else {
                    "nothing matches"
                }));
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for name in names {
                        let selected = self.selected.as_deref() == Some(&name);
                        if ui
                            .selectable_label(selected, theme::mono(&name))
                            .clicked()
                        {
                            self.select(&name);
                        }
                    }
                });
        });
    }

    /// The rasterised orbit view, rendered again only when the orbit or the
    /// model itself changed -- the same caching Chisel's 3D pane uses, and
    /// for the same reason: a software rasteriser is cheap, not free.
    fn viewer(&mut self, ui: &mut egui::Ui) {
        let Some(model) = &self.model else {
            ui.centered_and_justified(|ui| {
                ui.label(theme::caption(match &self.load_error {
                    Some(e) => e.as_str(),
                    None => "choose a model from the list",
                }));
            });
            return;
        };

        let available = ui.available_size();
        let size = (available.x.min(available.y).max(1.0) as usize).min(1024);
        let key = (
            self.selected.clone().unwrap_or_default(),
            size,
            (self.yaw * 10.0) as i32,
            (self.pitch * 10.0) as i32,
            (self.zoom * 100.0) as i32,
            // A texture arriving after a failed load changes the picture.
            self.textures.len(),
        );

        let stale = self.render.as_ref().is_none_or(|r| r.key != key);
        if stale {
            let vfs = &self.vfs;
            let cache = &mut self.textures;
            let mut resolve = move |material: &str| cache.get(vfs, material);
            let image = chisel::preview::model_zoomed(
                model,
                size,
                self.yaw,
                self.pitch,
                self.zoom,
                Some(&mut resolve),
            );
            let pixels: Vec<Color32> = image
                .pixels
                .iter()
                .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                .collect();
            let color_image = egui::ColorImage {
                size: [image.width, image.height],
                pixels,
                source_size: egui::vec2(image.width as f32, image.height as f32),
            };
            match &mut self.render {
                Some(rendered) => {
                    rendered.texture.set(color_image, egui::TextureOptions::LINEAR);
                    rendered.key = key;
                }
                None => {
                    let texture =
                        ui.ctx()
                            .load_texture("loupe-view", color_image, egui::TextureOptions::LINEAR);
                    self.render = Some(Rendered { key, texture });
                }
            }
        }

        let Some(rendered) = &self.render else { return };
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(size as f32, size as f32),
            egui::Sense::click_and_drag(),
        );
        ui.painter().image(
            rendered.texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        if response.dragged() {
            let delta = response.drag_delta();
            self.yaw -= delta.x * 0.4;
            self.pitch = (self.pitch - delta.y * 0.4).clamp(-89.0, 89.0);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(MIN_ZOOM, MAX_ZOOM);
            }
        }
    }

    /// Mesh, material and bone lists, and the note that this format has
    /// nothing to animate: `.keromdl` bones are a rest pose only.
    fn info_panel(&self, ui: &mut egui::Ui) {
        let Some(model) = &self.model else { return };

        widgets::section(ui, "meshes", |ui| {
            if model.meshes.is_empty() {
                ui.label(theme::caption("none"));
            }
            for (i, mesh) in model.meshes.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(theme::mono(model.mesh_material(i)));
                    ui.label(theme::caption(format!("{} tris", mesh.index_count / 3)));
                });
            }
        });

        widgets::section(ui, "bones", |ui| {
            if model.bones.len() <= 1 {
                ui.label(theme::caption("none (a static prop)"));
            }
            for (i, bone) in model.bones.iter().enumerate() {
                let parent = if bone.parent < 0 {
                    "root".to_string()
                } else {
                    model.bone_name(bone.parent as usize).to_string()
                };
                ui.label(format!("{}  <- {parent}", model.bone_name(i)));
            }
        });

        widgets::section(ui, "animation", |ui| {
            // Not a limitation of this viewer: `.keromdl` itself carries only
            // a rest pose, no keyframe tracks. Saying so plainly beats a
            // panel that just sits there empty and lets you wonder why.
            ui.label(theme::caption(
                "`.keromdl` has no animation data to play -- only a rest pose. \
                 Playback would need a model-format change, not a viewer change.",
            ));
        });

        if let Err(e) = model.validate() {
            ui.add_space(4.0);
            ui.label(theme::warn(format!("this model does not validate: {e}")));
        }
    }
}

impl kerosene_ui::App for LoupeApp {
    fn window_title(&self) -> String {
        match &self.selected {
            Some(name) => format!("{name} -- Loupe"),
            None => "Loupe -- Kerosene model viewer".into(),
        }
    }

    fn ui(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("loupe-models")
            .resizable(true)
            .default_width(220.0)
            .width_range(160.0..=420.0)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_PANEL)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ctx, |ui| self.model_list(ui));

        egui::SidePanel::right("loupe-info")
            .resizable(true)
            .default_width(260.0)
            .width_range(200.0..=480.0)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_PANEL)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.info_panel(ui));
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors::BG_APP))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if self.model.is_some() {
                        ui.label(theme::caption(
                            "drag to orbit, scroll to zoom, R resets the view",
                        ));
                    }
                });
                if ctx.input(|i| i.key_pressed(egui::Key::R)) {
                    self.yaw = DEFAULT_YAW;
                    self.pitch = DEFAULT_PITCH;
                    self.zoom = 1.0;
                }
                self.viewer(ui);
            });
    }
}

#[cfg(test)]
mod tests;
