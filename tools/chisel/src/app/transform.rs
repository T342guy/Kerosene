// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Cutting and turning the selection: the clip tool's Enter, carve,
//! hollow and its dialog, the transform dialog, flips, the quick 90-degree
//! turn, and align to grid.

use super::*;
use kerosene_math::Quat;
use kerosene_ui::theme::{self, colors};
use kerosene_ui::widgets;

/// What the transform dialog does with its three numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TransformMode {
    #[default]
    Rotate,
    Scale,
    Move,
}

impl TransformMode {
    fn label(self) -> &'static str {
        match self {
            TransformMode::Rotate => "rotate",
            TransformMode::Scale => "scale",
            TransformMode::Move => "move",
        }
    }
}

/// The transform dialog's state, kept between uses so the last numbers are
/// there to be reused.
#[derive(Clone, Debug, PartialEq)]
pub struct TransformDialog {
    pub mode: TransformMode,
    /// Degrees per axis, a factor per axis, or units per axis.
    pub value: [f32; 3],
    /// Turn and scale about the world origin rather than the selection.
    pub about_origin: bool,
}

impl Default for TransformDialog {
    fn default() -> Self {
        TransformDialog {
            mode: TransformMode::Rotate,
            value: [0.0, 0.0, 0.0],
            about_origin: false,
        }
    }
}

impl ChiselApp {
    /// Enter with the clip tool: cut along the laid line.
    pub(super) fn apply_clip(&mut self) {
        if self.tool.kind != ToolKind::Clip {
            return;
        }
        let Some(line) = self.tool.clip_line else {
            self.status = "drag a line across the selection first".into();
            return;
        };
        let Some(plane) = line.plane() else {
            self.status = "that line is too short to cut along".into();
            return;
        };
        if self.document.selection.is_empty() {
            self.status = "select what to cut first".into();
            return;
        }
        let mode = self.tool.clip_mode;
        let pieces = self.document.clip_selection(plane, mode);
        self.tool.clip_line = None;
        self.status = format!("cut: {pieces} pieces ({})", mode.label());
    }

    pub(super) fn carve(&mut self) {
        let n = self.document.carve_selection();
        self.status = if n == 0 {
            "select world brushes to carve with; they are taken out of whatever they overlap".into()
        } else {
            format!("carved {n} brushes")
        };
    }

    pub(super) fn convert_to_mesh(&mut self) {
        let n = self.document.convert_selection_to_meshes();
        self.status = if n == 0 {
            "select world brushes to turn into meshes".into()
        } else {
            format!(
                "{n} brushes are now meshes: detail, so they no longer seal the map or block visibility"
            )
        };
    }

    pub(super) fn flip(&mut self, horizontal: bool) {
        if self.document.selection.is_empty() {
            self.status = "select something to flip".into();
            return;
        }
        // Along the pane's horizontal or vertical axis; from a 3D pane,
        // horizontal is X and vertical is Z.
        let kind = self.viewports[self.active].kind;
        let axis = if kind.is_2d() {
            let (h, v, _) = kind.axes();
            if horizontal { h } else { v }
        } else if horizontal {
            0
        } else {
            2
        };
        self.document.flip_selection(axis);
        self.status = format!("flipped along {}", draw::axis_name(axis));
    }

    /// R: a quarter turn about the axis the active pane looks along.
    pub(super) fn rotate_90(&mut self) {
        let Some(centre) = self.document.selection_centre() else {
            self.status = "select something to rotate".into();
            return;
        };
        let kind = self.viewports[self.active].kind;
        let axis = if kind.is_2d() { kind.axes().2 } else { 2 };
        let mut unit = Vec3::ZERO;
        unit[axis] = 1.0;
        let pivot = self.document.grid.snap_point(centre);
        self.document
            .rotate_selection(pivot, Quat::from_axis_angle(unit, 90f32.to_radians()));
        self.status = format!("rotated 90 degrees about {}", draw::axis_name(axis));
    }

    pub(super) fn align_to_grid(&mut self) {
        let delta = self.document.align_selection_to_grid();
        self.status = if delta == Vec3::ZERO {
            "already on the grid".into()
        } else {
            format!(
                "aligned: moved {} {} {}",
                kerosene_math::format_float(delta.x),
                kerosene_math::format_float(delta.y),
                kerosene_math::format_float(delta.z),
            )
        };
    }

    /// The hollow dialog: one number.
    pub(super) fn hollow_window(&mut self, ctx: &Context) {
        if !self.show_hollow {
            return;
        }
        let mut go = false;
        let mut close = false;
        let modal = widgets::dialog(
            ctx,
            "chisel-hollow",
            "Hollow",
            320.0,
            |ui| {
                ui.label(theme::caption(
                    "Each selected brush becomes walls this thick, mitred at the corners. \
                     A negative thickness builds the walls outward around it.",
                ));
                ui.horizontal(|ui| {
                    ui.label(theme::caption("thickness"));
                    let r = ui.add(
                        egui::DragValue::new(&mut self.hollow_thickness)
                            .speed(1.0)
                            .suffix(" ku"),
                    );
                    r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
                })
                .inner
            },
            |ui| {
                if ui.button("cancel").clicked() {
                    close = true;
                }
                if widgets::primary_button(ui, RichText::new("hollow").color(colors::ON_ACCENT))
                    .clicked()
                {
                    go = true;
                }
            },
        );
        go |= modal.inner.0;
        if modal.should_close() {
            close = true;
        }
        if go {
            let n = self.document.hollow_selection(self.hollow_thickness);
            self.status = if n == 0 {
                "select a brush to hollow".into()
            } else {
                format!("hollowed into {n} walls")
            };
            close = true;
        }
        if close {
            self.show_hollow = false;
        }
    }

    /// The transform dialog: rotate, scale or move by numbers.
    pub(super) fn transform_window(&mut self, ctx: &Context) {
        if !self.show_transform {
            return;
        }
        let mut go = false;
        let mut close = false;
        let modal = widgets::dialog(
            ctx,
            "chisel-transform",
            "Transform",
            360.0,
            |ui| {
                ui.horizontal(|ui| {
                    for mode in [
                        TransformMode::Rotate,
                        TransformMode::Scale,
                        TransformMode::Move,
                    ] {
                        if ui
                            .selectable_label(self.transform.mode == mode, mode.label())
                            .clicked()
                        {
                            self.transform.mode = mode;
                            self.transform.value = match mode {
                                TransformMode::Scale => [1.0, 1.0, 1.0],
                                _ => [0.0, 0.0, 0.0],
                            };
                        }
                    }
                });
                ui.add_space(6.0);
                let (names, suffix, speed) = match self.transform.mode {
                    TransformMode::Rotate => (["x", "y", "z"], " deg", 1.0),
                    TransformMode::Scale => (["x", "y", "z"], " x", 0.01),
                    TransformMode::Move => (["x", "y", "z"], " ku", 1.0),
                };
                let entered = ui
                    .horizontal(|ui| {
                        let mut entered = false;
                        for (i, name) in names.iter().enumerate() {
                            let r = ui.add(
                                egui::DragValue::new(&mut self.transform.value[i])
                                    .speed(speed)
                                    .prefix(format!("{name} "))
                                    .suffix(suffix),
                            );
                            entered |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        }
                        entered
                    })
                    .inner;
                if self.transform.mode != TransformMode::Move {
                    ui.checkbox(&mut self.transform.about_origin, "about the world origin")
                        .on_hover_text("Otherwise about the centre of the selection.");
                }
                ui.label(theme::caption(match self.transform.mode {
                    TransformMode::Rotate => "Degrees about each axis, applied x then y then z.",
                    TransformMode::Scale => "A factor per axis; 2 doubles, 0.5 halves.",
                    TransformMode::Move => "Kerosene units along each axis.",
                }));
                entered
            },
            |ui| {
                if ui.button("cancel").clicked() {
                    close = true;
                }
                if widgets::primary_button(ui, RichText::new("apply").color(colors::ON_ACCENT))
                    .clicked()
                {
                    go = true;
                }
            },
        );
        go |= modal.inner.0;
        if modal.should_close() {
            close = true;
        }
        if go {
            self.apply_transform();
            close = true;
        }
        if close {
            self.show_transform = false;
        }
    }

    pub(super) fn apply_transform(&mut self) {
        let Some(centre) = self.document.selection_centre() else {
            self.status = "select something to transform".into();
            return;
        };
        let pivot = if self.transform.about_origin {
            Vec3::ZERO
        } else {
            centre
        };
        let v = self.transform.value;
        match self.transform.mode {
            TransformMode::Rotate => {
                let q = Quat::from_rotation_z(v[2].to_radians())
                    * Quat::from_rotation_y(v[1].to_radians())
                    * Quat::from_rotation_x(v[0].to_radians());
                self.document.rotate_selection(pivot, q);
                self.status = format!("rotated {} {} {}", v[0], v[1], v[2]);
            }
            TransformMode::Scale => {
                let factor = Vec3::new(v[0], v[1], v[2]);
                self.document.scale_selection(pivot, factor);
                self.status = format!("scaled by {} {} {}", v[0], v[1], v[2]);
            }
            TransformMode::Move => {
                let delta = Vec3::new(v[0], v[1], v[2]);
                let snap = std::mem::replace(&mut self.document.grid.snap, false);
                self.document.move_selection(delta);
                self.document.grid.snap = snap;
                self.status = format!("moved {} {} {}", v[0], v[1], v[2]);
            }
        }
    }
}
