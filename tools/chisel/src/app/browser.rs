// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The asset browser: materials and models, as pictures.

use super::*;

impl ChiselApp {
    /// Write pending property edits into the document, as one undo step.
    /// The material picker: a grid of what the textures actually look like.
    ///
    /// A list of names is only usable by someone who already knows what every
    /// name looks like, which is nobody on their first level. The thumbnails
    /// are the same pixels the 3D pane draws with, so picking one is picking
    /// what you can see.
    pub(super) fn material_browser(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("material").strong());
            if ui
                .small_button("browse")
                .on_hover_text("Open the material browser, with names and room to look")
                .clicked()
            {
                self.browsing = Some(Browsing::Material);
            }
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.material_filter)
                .desired_width(f32::INFINITY)
                .hint_text("filter"),
        );

        let current = self.document.current_material.clone();
        let filter = self.material_filter.to_ascii_lowercase();
        let materials: Vec<String> = self
            .materials
            .iter()
            .filter(|m| filter.is_empty() || m.to_ascii_lowercase().contains(&filter))
            .cloned()
            .collect();

        // What is on the selection, so the picker shows where you already are.
        ui.label(
            RichText::new(&current)
                .monospace()
                .size(10.0)
                .color(draw::colors::SELECTED),
        );

        const CELL: f32 = 48.0;
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let columns = ((ui.available_width() + 4.0) / (CELL + 6.0))
                    .floor()
                    .max(1.0) as usize;
                let mut picked = None;
                egui::Grid::new("materials")
                    .spacing([4.0, 4.0])
                    .show(ui, |ui| {
                        for (index, material) in materials.iter().enumerate() {
                            let handle = self.thumbnail(ui.ctx(), material);
                            let selected = *material == current;
                            let image = egui::Image::new(&handle)
                                .fit_to_exact_size(egui::vec2(CELL, CELL))
                                .corner_radius(2.0);
                            let response = ui
                                .add(egui::ImageButton::new(image).selected(selected))
                                .on_hover_text(match self.textures.problem(material) {
                                    Some(problem) => format!("{material}\n\n{problem}"),
                                    None => material.clone(),
                                });
                            if response.clicked() {
                                picked = Some(material.clone());
                            }
                            if index % columns == columns - 1 {
                                ui.end_row();
                            }
                        }
                    });
                if let Some(material) = picked {
                    self.document.current_material = material.clone();
                    if !self.document.selection.is_empty() {
                        let faces = self.document.apply_material();
                        self.status = format!("{material} on {faces} faces");
                    } else {
                        self.status = material;
                    }
                }
            });
    }

    /// The asset browser: a window with room to look in.
    ///
    /// The picker used to be a two-column strip of unlabelled 48-pixel
    /// swatches in a 120-point panel. That is not a browser, it is a
    /// keyhole -- and with the swatches drawn from a 2x2 mip, every one of
    /// them was the same grey square. Here there is space, names, folders,
    /// and a search.
    pub(super) fn browser_window(&mut self, ctx: &Context) {
        let Some(browsing) = self.browsing.clone() else {
            return;
        };

        let (title, all): (&str, Vec<String>) = match browsing {
            Browsing::Material => ("materials", self.materials.clone()),
            Browsing::Model { .. } => ("models", self.models.clone()),
        };

        let mut open = true;
        let mut picked: Option<String> = None;
        egui::Window::new(title)
            .open(&mut open)
            .default_width(560.0)
            .default_height(520.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("search").size(11.0).weak());
                    ui.add(
                        egui::TextEdit::singleline(&mut self.browse_filter)
                            .desired_width(240.0)
                            .hint_text("any words, any order"),
                    );
                    if !self.browse_filter.is_empty() && ui.small_button("x").clicked() {
                        self.browse_filter.clear();
                    }
                    ui.add(
                        egui::Slider::new(&mut self.browse_size, 48.0..=160.0)
                            .show_value(false)
                            .text("size"),
                    );
                });
                ui.separator();

                let matching = crate::browse::filtered(&all, &self.browse_filter);
                if matching.is_empty() {
                    ui.label(
                        RichText::new(if all.is_empty() {
                            "nothing here. Has the content been built?"
                        } else {
                            "nothing matches"
                        })
                        .weak(),
                    );
                    return;
                }

                let cell = self.browse_size;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for folder in crate::browse::folders(&matching) {
                            let name = if folder.name.is_empty() {
                                "(loose)"
                            } else {
                                &folder.name
                            };
                            ui.label(
                                RichText::new(format!("{name}/  {}", folder.items.len()))
                                    .monospace()
                                    .size(11.0)
                                    .color(draw::colors::TEXT),
                            );
                            ui.separator();

                            // Wrapped by hand rather than with a Grid, so a
                            // resized window reflows instead of clipping.
                            let per_row = ((ui.available_width() + 8.0) / (cell + 12.0))
                                .floor()
                                .max(1.0) as usize;
                            for chunk in folder.items.chunks(per_row) {
                                ui.horizontal(|ui| {
                                    for item in chunk {
                                        if self.browse_cell(ui, ctx, &browsing, item, cell) {
                                            picked = Some(item.clone());
                                        }
                                    }
                                });
                            }
                            ui.add_space(8.0);
                        }
                    });
            });

        if let Some(item) = picked {
            self.apply_browsed(&browsing, &item);
            open = false;
        }
        if !open {
            self.browsing = None;
        }
    }

    /// One swatch, with its name under it. Returns whether it was clicked.
    pub(super) fn browse_cell(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        browsing: &Browsing,
        item: &str,
        cell: f32,
    ) -> bool {
        let current = match browsing {
            Browsing::Material => self.document.current_material.clone(),
            Browsing::Model { current, .. } => current.clone(),
        };

        let mut clicked = false;
        ui.vertical(|ui| {
            ui.set_width(cell);
            let handle = match browsing {
                Browsing::Material => self.thumbnail(ctx, item),
                Browsing::Model { .. } => self.model_thumbnail(ctx, item),
            };
            let image = egui::Image::new(&handle)
                .fit_to_exact_size(egui::vec2(cell, cell))
                .corner_radius(3.0);
            let response = ui
                .add(egui::ImageButton::new(image).selected(item == current))
                .on_hover_text(self.browse_hover(browsing, item));
            clicked = response.clicked();

            // The name, under it, always. Hovering twenty tooltips to find a
            // texture is the thing this replaces.
            ui.label(
                RichText::new(crate::browse::leaf(item))
                    .monospace()
                    .size(10.0)
                    .color(if item == current {
                        draw::colors::SELECTED
                    } else {
                        draw::colors::TEXT
                    }),
            );
        });
        clicked
    }

    /// What to say about an asset on hover.
    pub(super) fn browse_hover(&self, browsing: &Browsing, item: &str) -> String {
        match browsing {
            Browsing::Material => {
                let meaning = cleave::material::describe(item);
                match self.textures.problem(item) {
                    Some(problem) => format!("{item}\n{meaning}\n\n{problem}"),
                    None => format!("{item}\n{meaning}"),
                }
            }
            Browsing::Model { .. } => item.to_string(),
        }
    }

    /// Act on a picked asset.
    pub(super) fn apply_browsed(&mut self, browsing: &Browsing, item: &str) {
        match browsing {
            Browsing::Material => {
                self.document.current_material = item.to_string();
                if !self.document.selection.is_empty() {
                    let faces = self.document.apply_material();
                    self.status = format!("{item} on {faces} faces");
                } else {
                    self.status = item.to_string();
                }
            }
            Browsing::Model { row: Some(row), .. } => {
                if let Some(edit) = self.properties.as_mut()
                    && let Some(row) = edit.rows.get_mut(*row)
                {
                    row.value = Some(item.to_string());
                    edit.dirty = true;
                }
                self.commit_properties();
                self.status = format!("model {item}");
            }
            // Opened from the menu, with nothing to set: looking is the point.
            Browsing::Model { row: None, .. } => {
                self.status = format!("{item} -- place one with a prop_static");
            }
        }
    }

    /// An egui texture showing a model, rendered once.
    pub(super) fn model_thumbnail(&mut self, ctx: &Context, name: &str) -> egui::TextureHandle {
        if let Some(handle) = self.model_previews.get(name) {
            return handle.clone();
        }

        const SIZE: usize = 128;
        let image = match self.load_model(name) {
            Some(model) => crate::preview::model(&model, SIZE, 35.0, -20.0),
            // A blank rather than nothing, so a model that will not load is
            // still something to click and still says its name.
            None => crate::raster::Image::new(SIZE, SIZE, crate::preview::BACKGROUND),
        };
        let colour = egui::ColorImage {
            size: [image.width, image.height],
            pixels: image
                .pixels
                .iter()
                .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                .collect(),
            source_size: egui::vec2(image.width as f32, image.height as f32),
        };
        let handle = ctx.load_texture(format!("mdl-{name}"), colour, egui::TextureOptions::LINEAR);
        self.model_previews.insert(name.to_string(), handle.clone());
        handle
    }

    /// Read a model out of the content tree.
    pub(super) fn load_model(&self, name: &str) -> Option<kerosene_asset::Model> {
        let bytes = self.vfs.read(&format!("models/{name}.keromdl")).ok()?;
        kerosene_asset::Model::from_bytes(&bytes).ok()
    }

    /// An egui texture for a material, built once.
    ///
    /// From the smallest mip that is still bigger than the swatch, so it is
    /// scaled down rather than up and reads as the texture rather than as
    /// aliasing. Choosing by position in the mip chain instead is what made
    /// every swatch a flat smudge: two from the end of a 256-pixel texture is
    /// a 2x2 image, and no two materials looked any different.
    pub(super) fn thumbnail(&mut self, ctx: &Context, material: &str) -> egui::TextureHandle {
        if let Some(handle) = self.thumbnails.get(material) {
            return handle.clone();
        }

        let image = match self.textures.get(&self.vfs, material) {
            Some(texture) => {
                let level = texture.level_for_size(THUMBNAIL);
                egui::ColorImage {
                    size: [level.width as usize, level.height as usize],
                    pixels: level
                        .pixels
                        .iter()
                        .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                        .collect(),
                    source_size: egui::vec2(level.width as f32, level.height as f32),
                }
            }
            None => {
                // A flat swatch of the fallback colour, so a material with no
                // texture behind it is still a distinct thing to click.
                let [r, g, b] = TextureCache::fallback_colour(material);
                egui::ColorImage {
                    size: [2, 2],
                    pixels: vec![egui::Color32::from_rgb(r, g, b); 4],
                    source_size: egui::vec2(2.0, 2.0),
                }
            }
        };

        let handle = ctx.load_texture(
            format!("mat-{material}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.thumbnails.insert(material.to_string(), handle.clone());
        handle
    }
}
