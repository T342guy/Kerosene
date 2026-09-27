// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The card that comes up when the pointer rests on something in a pane.
//!
//! A level is full of things that look alike: a dozen marker boxes, forty
//! grey brushes. Selecting each to read what it is costs a click and loses
//! the selection you had. Hammer 5 answers with a card under the pointer --
//! what it is, what it is called, what it is wired to -- and so does this.

use super::*;
use kerosene_toolui::theme::{self, colors};

/// What the pointer is over in a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hovered {
    Entity(u32),
    /// One face of a brush, in a 3D pane.
    Face {
        solid: u32,
        side: u32,
    },
    /// A brush, in a 2D pane, which cannot say which face.
    Solid(u32),
    Mesh(u32),
}

/// How many keys a card lists before it says "and N more".
const CARD_KEYS: usize = 6;

/// Keys every entity has, which a card would only be repeating.
const UNREMARKABLE: [&str; 5] = ["classname", "targetname", "origin", "angles", "spawnflags"];

impl ChiselApp {
    /// The card for whatever is under the pointer.
    pub(super) fn hover_card(&self, ui: &mut egui::Ui, hovered: Hovered) {
        ui.set_max_width(300.0);
        match hovered {
            Hovered::Entity(id) => self.entity_card(ui, id),
            Hovered::Face { solid, side } => self.brush_card(ui, solid, Some(side)),
            Hovered::Solid(solid) => self.brush_card(ui, solid, None),
            Hovered::Mesh(id) => {
                let faces = self
                    .document
                    .visible_meshes()
                    .find(|m| m.id == id)
                    .map_or(0, |m| m.faces.len());
                ui.label(RichText::new(format!("mesh {id}")).monospace().strong());
                ui.label(theme::caption(format!(
                    "{faces} faces -- detail geometry, never part of the tree"
                )));
            }
        }
    }

    fn entity_card(&self, ui: &mut egui::Ui, id: u32) {
        let Some(entity) = self.document.find_entity(id) else {
            return;
        };
        let classname = entity.classname();
        let spec = self.schema.get(classname);
        ui.horizontal(|ui| {
            let kind = crate::icons::Kind::of(classname);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            crate::icons::draw(ui.painter(), rect.center(), 5.5, kind, kind.colour());
            ui.label(RichText::new(classname).monospace().strong());
            if let Some(name) = entity.targetname().filter(|n| !n.trim().is_empty()) {
                ui.label(RichText::new(name).monospace().color(colors::ACCENT));
            }
        });
        if let Some(help) = spec
            .map(|s| first_sentence(&s.help))
            .filter(|h| !h.is_empty())
        {
            ui.label(theme::caption(help));
        }

        let keys: Vec<(String, &str)> = entity
            .properties
            .iter()
            .filter(|(k, v)| {
                !v.trim().is_empty() && !UNREMARKABLE.iter().any(|u| u.eq_ignore_ascii_case(k))
            })
            .map(|(k, v)| {
                let label = spec
                    .and_then(|s| s.key(k))
                    .map(|k| k.label.clone())
                    .filter(|l| !l.is_empty())
                    .unwrap_or_else(|| k.clone());
                (label, v.as_str())
            })
            .collect();
        if !keys.is_empty() {
            ui.add_space(2.0);
            egui::Grid::new(("hover-keys", id))
                .num_columns(2)
                .spacing([10.0, 1.0])
                .show(ui, |ui| {
                    for (label, value) in keys.iter().take(CARD_KEYS) {
                        ui.label(theme::caption(label));
                        let value = if value.chars().count() > 36 {
                            format!("{}...", value.chars().take(34).collect::<String>())
                        } else {
                            value.to_string()
                        };
                        ui.label(theme::mono(value).color(colors::TEXT));
                        ui.end_row();
                    }
                });
            if keys.len() > CARD_KEYS {
                ui.label(theme::caption(format!(
                    "and {} more",
                    keys.len() - CARD_KEYS
                )));
            }
        }

        // The wiring, counted both ways, with anything broken called out:
        // the thing a designer most wants to know about an entity they are
        // not looking at the properties of.
        let entities = &self.document.map.entities;
        let outputs = entity.connections.len();
        let inputs = crate::wiring::inputs_to(entities, entity).len();
        let broken = entity
            .connections
            .iter()
            .filter(|c| crate::wiring::validate(entity, c, entities, &self.schema).is_broken())
            .count();
        if outputs + inputs > 0 {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(theme::caption(format!(
                    "{outputs} output{}  -  {inputs} input{}",
                    plural(outputs),
                    plural(inputs)
                )));
                if broken > 0 {
                    ui.label(
                        RichText::new(format!("{broken} broken"))
                            .size(11.0)
                            .color(colors::ERR),
                    );
                }
            });
        }
        ui.label(theme::caption("double-click for its properties").weak());
    }

    fn brush_card(&self, ui: &mut egui::Ui, solid_id: u32, side: Option<u32>) {
        let Some((owner, solid)) = self
            .document
            .map
            .all_solids()
            .find(|(_, s)| s.id == solid_id)
        else {
            return;
        };
        ui.horizontal(|ui| {
            if owner.classname() == "worldspawn" {
                ui.label(
                    RichText::new(format!("brush {solid_id}"))
                        .monospace()
                        .strong(),
                );
            } else {
                ui.label(RichText::new(owner.classname()).monospace().strong());
                if let Some(name) = owner.targetname().filter(|n| !n.trim().is_empty()) {
                    ui.label(RichText::new(name).monospace().color(colors::ACCENT));
                }
                ui.label(theme::caption(format!("brush {solid_id}")));
            }
        });
        let size = solid.bounds().size();
        ui.label(theme::caption(format!(
            "{} x {} x {}",
            kerosene_math::units::length_short(size.x),
            kerosene_math::units::length_short(size.y),
            kerosene_math::units::length_short(size.z),
        )));
        if let Some(side) = side.and_then(|id| solid.sides.iter().find(|s| s.id == id)) {
            ui.horizontal(|ui| {
                ui.label(theme::caption("face"));
                ui.label(theme::mono(&side.material).color(colors::TEXT));
            });
            if let Some(problem) = self.textures.problem(&side.material) {
                ui.label(RichText::new(problem).size(11.0).color(colors::WARN));
            }
        }
    }
}

/// A help text's first sentence: a card is not the place for the rest.
fn first_sentence(text: &str) -> String {
    let text = text.trim();
    match text.find(". ") {
        Some(at) => text[..=at].to_string(),
        None => text.to_string(),
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_shows_the_first_sentence_of_the_help() {
        assert_eq!(first_sentence("A door. It slides open. Mostly."), "A door.");
        assert_eq!(first_sentence("No full stop"), "No full stop");
        assert_eq!(first_sentence("Ends here."), "Ends here.");
    }
}
