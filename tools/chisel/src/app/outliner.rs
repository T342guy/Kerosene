// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The outliner: every entity in the map, as a list you can search.
//!
//! The viewports answer "what is here"; the outliner answers "where is the
//! thing called `exit_door`", which no amount of flying around does. Hammer
//! 5 keeps one docked above the object properties, and this is that:
//! grouped by family, filtered by any part of a class or a name, click to
//! select and double-click to fly there.

use super::*;
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets;

/// One line in the outliner.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineRow {
    pub id: u32,
    pub classname: String,
    pub name: Option<String>,
    pub brush: bool,
    pub hidden: bool,
}

/// The entities to list, by family, filtered by `filter`: every word must
/// appear in the class or the name, in any order.
pub fn outline(document: &Document, filter: &str) -> Vec<(crate::icons::Kind, Vec<OutlineRow>)> {
    let words: Vec<String> = filter
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let mut families: Vec<(crate::icons::Kind, Vec<OutlineRow>)> = crate::icons::Kind::all()
        .into_iter()
        .map(|k| (k, Vec::new()))
        .collect();
    for entity in &document.map.entities {
        if entity.classname() == "worldspawn" {
            continue;
        }
        let name = entity
            .targetname()
            .filter(|n| !n.trim().is_empty())
            .map(str::to_string);
        let haystack = format!(
            "{} {}",
            entity.classname().to_ascii_lowercase(),
            name.as_deref().unwrap_or_default().to_ascii_lowercase()
        );
        if !words.iter().all(|w| haystack.contains(w.as_str())) {
            continue;
        }
        let kind = crate::icons::Kind::of(entity.classname());
        let row = OutlineRow {
            id: entity.id,
            classname: entity.classname().to_string(),
            name,
            brush: !entity.solids.is_empty(),
            hidden: !document.entity_visible(entity),
        };
        if let Some((_, rows)) = families.iter_mut().find(|(k, _)| *k == kind) {
            rows.push(row);
        }
    }
    for (_, rows) in &mut families {
        // Named things first, by name; then the rest by class.
        rows.sort_by(|a, b| match (&a.name, &b.name) {
            (Some(x), Some(y)) => x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase()),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.classname.cmp(&b.classname).then(a.id.cmp(&b.id)),
        });
    }
    families.retain(|(_, rows)| !rows.is_empty());
    families
}

impl ChiselApp {
    pub(super) fn outliner_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(theme::icon(icons::MAGNIFYING_GLASS).color(colors::TEXT_MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut self.outliner_filter)
                    .desired_width(ui.available_width() - 28.0)
                    .hint_text("find by class or name"),
            );
            if !self.outliner_filter.is_empty()
                && widgets::icon_button(ui, icons::X, "clear the search").clicked()
            {
                self.outliner_filter.clear();
            }
        });

        let families = outline(&self.document, &self.outliner_filter);
        if families.is_empty() {
            ui.label(theme::caption(if self.outliner_filter.is_empty() {
                "no entities yet. The entity tool (3) places them."
            } else {
                "nothing matches"
            }));
            return;
        }

        let mut clicked: Option<(u32, bool, bool)> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (kind, rows) in &families {
                    let closed = self.outliner_closed.contains(kind.label());
                    let header = ui.horizontal(|ui| {
                        let caret = if closed {
                            icons::CARET_RIGHT
                        } else {
                            icons::CARET_DOWN
                        };
                        ui.label(theme::icon(caret).color(colors::TEXT_MUTED).size(11.0));
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(14.0, 16.0), egui::Sense::hover());
                        crate::icons::draw(ui.painter(), rect.center(), 5.0, *kind, kind.colour());
                        ui.label(theme::section_title(kind.plural()));
                        ui.label(theme::caption(rows.len().to_string()));
                    });
                    if ui
                        .interact(
                            header.response.rect,
                            ui.id().with(("family", kind.label())),
                            egui::Sense::click(),
                        )
                        .clicked()
                    {
                        if closed {
                            self.outliner_closed.remove(kind.label());
                        } else {
                            self.outliner_closed.insert(kind.label());
                        }
                    }
                    if closed {
                        continue;
                    }
                    for row in rows {
                        let selected = self.document.selection.entities.contains(&row.id);
                        let text = match &row.name {
                            Some(name) => RichText::new(format!("{name}  ")).monospace().color(
                                if row.hidden {
                                    colors::TEXT_MUTED
                                } else {
                                    colors::TEXT
                                },
                            ),
                            None => RichText::new("(unnamed)  ")
                                .italics()
                                .color(colors::TEXT_MUTED),
                        };
                        let response = ui
                            .horizontal(|ui| {
                                ui.add_space(22.0);
                                let mut job = egui::text::LayoutJob::default();
                                text.append_to(
                                    &mut job,
                                    ui.style(),
                                    egui::FontSelection::Default,
                                    egui::Align::Center,
                                );
                                RichText::new(&row.classname)
                                    .monospace()
                                    .size(11.0)
                                    .color(colors::TEXT_MUTED)
                                    .append_to(
                                        &mut job,
                                        ui.style(),
                                        egui::FontSelection::Default,
                                        egui::Align::Center,
                                    );
                                ui.add(
                                    egui::Button::selectable(selected, job)
                                        .frame_when_inactive(false),
                                )
                            })
                            .inner;
                        let response = if row.hidden {
                            response.on_hover_text("hidden -- shown dim here, and not in the views")
                        } else {
                            response
                        };
                        if response.double_clicked() {
                            clicked = Some((row.id, false, true));
                        } else if response.clicked() {
                            let add = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift);
                            clicked = Some((row.id, add, false));
                        }
                    }
                    ui.add_space(4.0);
                }
            });

        if let Some((id, add, frame)) = clicked {
            if !add {
                self.document.selection.clear();
            }
            if add && self.document.selection.entities.contains(&id) {
                self.document.selection.entities.remove(&id);
            } else {
                self.document.selection.entities.insert(id);
            }
            if frame {
                self.frame_all();
                self.status = "framed the selection".into();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> Document {
        let mut document = Document::new();
        let a = document.create_entity("light", Vec3::ZERO);
        document
            .find_entity_mut(a)
            .unwrap()
            .set("targetname", "lamp_hall");
        document.create_entity("light", Vec3::X);
        let c = document.create_entity("info_player_start", Vec3::Y);
        document
            .find_entity_mut(c)
            .unwrap()
            .set("targetname", "start");
        document
    }

    #[test]
    fn entities_are_listed_by_family_with_named_ones_first() {
        let families = outline(&document(), "");
        let lights = &families
            .iter()
            .find(|(k, _)| *k == crate::icons::Kind::Light)
            .unwrap()
            .1;
        assert_eq!(lights.len(), 2);
        assert_eq!(lights[0].name.as_deref(), Some("lamp_hall"));
        assert!(families.iter().all(|(_, rows)| !rows.is_empty()));
        assert!(
            families
                .iter()
                .flat_map(|(_, r)| r)
                .all(|r| r.classname != "worldspawn"),
            "the world is not an entity you look for"
        );
    }

    #[test]
    fn every_word_of_the_search_must_match_class_or_name() {
        let names = |filter: &str| -> Vec<u32> {
            outline(&document(), filter)
                .into_iter()
                .flat_map(|(_, rows)| rows.into_iter().map(|r| r.id))
                .collect()
        };
        assert_eq!(names("hall").len(), 1);
        assert_eq!(names("LIGHT hall").len(), 1);
        assert_eq!(names("light").len(), 2);
        assert_eq!(names("light start").len(), 0);
        assert_eq!(names("start").len(), 1);
    }
}
