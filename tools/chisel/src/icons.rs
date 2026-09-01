// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! What an entity looks like in a 2D pane.
//!
//! Every point entity used to be the same small square with its classname
//! written beside it, which means a room full of them is a wall of text you
//! have to read to navigate. Reading is the wrong operation: what you want
//! from a top-down view is to see at a glance that *those three are lights and
//! that one is the player start*, and shape does that where a label cannot.
//!
//! The shapes are drawn rather than loaded. An icon set is a set of files to
//! ship, scale, theme and keep in step with the class list; a dozen lines of
//! geometry is none of those things, and stays sharp at every zoom.

use egui::{Color32, Pos2, Stroke, Vec2};

/// The families of thing a map is made of.
///
/// Grouped by what a designer is looking for, not by how the game implements
/// them: hunting for "the lights" is a thing people do, and hunting for "the
/// classes with an `_light` key" is not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Light,
    Player,
    Sound,
    Logic,
    Script,
    Message,
    Prop,
    Target,
    /// Anything with no family of its own.
    Other,
}

impl Kind {
    /// Which family a class belongs to.
    ///
    /// Matched on prefixes, so a game that adds `light_dynamic` or
    /// `logic_case` gets the right icon without this list being updated --
    /// and a class nobody anticipated still gets a shape rather than nothing.
    pub fn of(classname: &str) -> Kind {
        let name = classname.to_ascii_lowercase();
        if name.starts_with("light") { return Kind::Light }
        if name.starts_with("ambient_") || name.contains("sound") { return Kind::Sound }
        if name.contains("script") { return Kind::Script }
        if name.starts_with("logic_") || name.starts_with("math_") { return Kind::Logic }
        if name.starts_with("point_message") || name.contains("message") { return Kind::Message }
        if name.starts_with("prop_") { return Kind::Prop }
        match name.as_str() {
            "info_player_start" => Kind::Player,
            "info_target" => Kind::Target,
            _ => Kind::Other,
        }
    }

    /// The colour this family is drawn in when it is not selected.
    ///
    /// Chosen to be distinguishable from each other and from the brush
    /// outlines, and to mean something: lights are lamp-coloured, logic is
    /// the colour of nothing in the world because it is not in the world.
    pub fn colour(self) -> Color32 {
        match self {
            Kind::Light => Color32::from_rgb(255, 214, 120),
            Kind::Player => Color32::from_rgb(120, 200, 255),
            Kind::Sound => Color32::from_rgb(120, 230, 220),
            Kind::Logic => Color32::from_rgb(200, 160, 255),
            Kind::Script => Color32::from_rgb(160, 220, 160),
            Kind::Message => Color32::from_rgb(230, 190, 150),
            Kind::Prop => Color32::from_rgb(160, 210, 140),
            Kind::Target => Color32::from_rgb(220, 150, 180),
            Kind::Other => Color32::from_rgb(150, 165, 190),
        }
    }

    /// A one-word name for the family, for a tooltip or a filter.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Light => "light",
            Kind::Player => "player",
            Kind::Sound => "sound",
            Kind::Logic => "logic",
            Kind::Script => "script",
            Kind::Message => "message",
            Kind::Prop => "prop",
            Kind::Target => "target",
            Kind::Other => "entity",
        }
    }

    /// Every family, for a legend.
    pub fn all() -> [Kind; 9] {
        [
            Kind::Light, Kind::Player, Kind::Sound, Kind::Logic, Kind::Script,
            Kind::Message, Kind::Prop, Kind::Target, Kind::Other,
        ]
    }
}

/// Draw a class's icon centred on a point.
///
/// `radius` is half the icon's size in pixels. Everything is drawn from that,
/// so the same call works for a 5-pixel marker in a zoomed-out viewport and a
/// 12-pixel one in a menu.
pub fn draw(painter: &egui::Painter, at: Pos2, radius: f32, kind: Kind, colour: Color32) {
    let stroke = Stroke::new((radius * 0.22).clamp(1.0, 2.0), colour);
    let r = radius;

    match kind {
        // A lamp: a filled centre with rays coming off it.
        Kind::Light => {
            painter.circle_filled(at, r * 0.45, colour);
            for i in 0..8 {
                let a = std::f32::consts::TAU * i as f32 / 8.0;
                let (s, c) = a.sin_cos();
                let dir = Vec2::new(c, s);
                painter.line_segment([at + dir * (r * 0.65), at + dir * r], stroke);
            }
        }
        // A person: a head and shoulders.
        Kind::Player => {
            painter.circle_stroke(at - Vec2::new(0.0, r * 0.45), r * 0.35, stroke);
            painter.line_segment([at - Vec2::new(r * 0.6, -r * 0.8), at + Vec2::new(0.0, -r * 0.1)], stroke);
            painter.line_segment([at + Vec2::new(r * 0.6, r * 0.8), at + Vec2::new(0.0, -r * 0.1)], stroke);
            painter.line_segment([at - Vec2::new(r * 0.6, -r * 0.8), at + Vec2::new(r * 0.6, r * 0.8)], stroke);
        }
        // A speaker: a cone with sound coming out of it.
        Kind::Sound => {
            let cone = [
                at + Vec2::new(-r * 0.7, -r * 0.35),
                at + Vec2::new(-r * 0.2, -r * 0.35),
                at + Vec2::new(r * 0.15, -r * 0.8),
                at + Vec2::new(r * 0.15, r * 0.8),
                at + Vec2::new(-r * 0.2, r * 0.35),
                at + Vec2::new(-r * 0.7, r * 0.35),
            ];
            painter.add(egui::Shape::closed_line(cone.to_vec(), stroke));
            for (i, scale) in [0.45f32, 0.8].iter().enumerate() {
                let x = r * (0.4 + i as f32 * 0.3);
                painter.line_segment(
                    [at + Vec2::new(x, -r * scale * 0.6), at + Vec2::new(x, r * scale * 0.6)],
                    stroke,
                );
            }
        }
        // A diamond: a decision, standing on its point.
        Kind::Logic => {
            let d = [
                at + Vec2::new(0.0, -r),
                at + Vec2::new(r, 0.0),
                at + Vec2::new(0.0, r),
                at + Vec2::new(-r, 0.0),
            ];
            painter.add(egui::Shape::closed_line(d.to_vec(), stroke));
        }
        // Angle brackets: code.
        Kind::Script => {
            for side in [-1.0f32, 1.0] {
                painter.add(egui::Shape::line(
                    vec![
                        at + Vec2::new(side * r * 0.2, -r * 0.7),
                        at + Vec2::new(side * r * 0.85, 0.0),
                        at + Vec2::new(side * r * 0.2, r * 0.7),
                    ],
                    stroke,
                ));
            }
        }
        // A speech bubble.
        Kind::Message => {
            painter.rect_stroke(
                egui::Rect::from_center_size(at - Vec2::new(0.0, r * 0.2), Vec2::new(r * 1.8, r * 1.1)),
                r * 0.25,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.add(egui::Shape::line(
                vec![
                    at + Vec2::new(-r * 0.35, r * 0.35),
                    at + Vec2::new(-r * 0.45, r),
                    at + Vec2::new(0.1 * r, r * 0.35),
                ],
                stroke,
            ));
        }
        // A box in perspective: a thing that stands in the world.
        Kind::Prop => {
            let s = r * 0.62;
            let back = Vec2::new(r * 0.35, -r * 0.35);
            let front = egui::Rect::from_center_size(at + Vec2::new(-r * 0.15, r * 0.15), Vec2::splat(s * 2.0));
            painter.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Middle);
            painter.line_segment([front.left_top(), front.left_top() + back], stroke);
            painter.line_segment([front.right_top(), front.right_top() + back], stroke);
            painter.line_segment([front.right_bottom(), front.right_bottom() + back], stroke);
            painter.add(egui::Shape::line(
                vec![front.left_top() + back, front.right_top() + back, front.right_bottom() + back],
                stroke,
            ));
        }
        // A crosshair: somewhere to aim at.
        Kind::Target => {
            painter.circle_stroke(at, r * 0.55, stroke);
            for dir in [Vec2::X, Vec2::Y] {
                painter.line_segment([at - dir * r, at - dir * (r * 0.75)], stroke);
                painter.line_segment([at + dir * (r * 0.75), at + dir * r], stroke);
            }
        }
        // A plain box, for anything with no family of its own.
        Kind::Other => {
            painter.rect_stroke(
                egui::Rect::from_center_size(at, Vec2::splat(r * 1.6)),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
    }
}

#[cfg(test)]
mod tests;
