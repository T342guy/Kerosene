// SPDX-License-Identifier: MPL-2.0
//! The editing tools: select, block, shape, entity, texture.
//!
//! Each tool is a small state machine over a drag. Keeping them here, away
//! from the UI, means "what does dragging in the block tool do" is a question
//! with a testable answer rather than something you find out by clicking.

use crate::document::Document;
use crate::viewport::{Viewport, ray_box};
use kerosene_math::{Aabb, Vec3, Winding};
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToolKind {
    /// Click to select, drag to move, shift-click to add.
    #[default]
    Select,
    /// Drag out a box brush.
    Block,
    /// Drag out something that is not a box: a wedge, a cylinder, an arch.
    Shape,
    /// Click to place a point entity.
    Entity,
    /// Click a face to apply the current material.
    Texture,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Select => "select",
            ToolKind::Block => "block",
            ToolKind::Shape => "shape",
            ToolKind::Entity => "entity",
            ToolKind::Texture => "texture",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            ToolKind::Select => "1",
            ToolKind::Block => "2",
            ToolKind::Shape => "5",
            ToolKind::Entity => "3",
            ToolKind::Texture => "4",
        }
    }

    /// Every tool, in the order the toolbar lists them -- which is the order
    /// their shortcuts run in, so the list reads 1, 2, 3, 4, 5 down the side
    /// rather than sending the eye hunting for the number it wants.
    pub fn all() -> [ToolKind; 5] {
        [ToolKind::Select, ToolKind::Block, ToolKind::Entity, ToolKind::Texture, ToolKind::Shape]
    }

    /// Whether this tool draws a box out and turns it into geometry.
    pub fn draws_a_box(self) -> bool {
        matches!(self, ToolKind::Block | ToolKind::Shape)
    }
}

/// How the texture tool responds to a click.
///
/// The original behaviour -- apply on every click -- is fine for painting a
/// whole room at once and maddening for picking one face at a time, because
/// every stray click repaints something. These three cover the two ends and
/// the middle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum TextureMode {
    /// Click only selects faces. Applying is explicit, via the face panel.
    Selection,
    /// Click selects; double-click applies. The middle ground.
    #[default]
    ApplyDoubleClick,
    /// Click selects and applies -- the original behaviour.
    AlwaysApply,
}

impl TextureMode {
    pub fn label(self) -> &'static str {
        match self {
            TextureMode::Selection => "select only",
            TextureMode::ApplyDoubleClick => "apply on double-click",
            TextureMode::AlwaysApply => "always apply",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            TextureMode::Selection => "A click selects; nothing is applied. Use the face panel's \"apply current\" to paint.",
            TextureMode::ApplyDoubleClick => "A click selects; a double-click applies the current material.",
            TextureMode::AlwaysApply => "A click selects and applies the current material.",
        }
    }

    pub fn all() -> [TextureMode; 3] {
        [TextureMode::Selection, TextureMode::ApplyDoubleClick, TextureMode::AlwaysApply]
    }

    pub fn next(self) -> TextureMode {
        match self {
            TextureMode::Selection => TextureMode::ApplyDoubleClick,
            TextureMode::ApplyDoubleClick => TextureMode::AlwaysApply,
            TextureMode::AlwaysApply => TextureMode::Selection,
        }
    }
}

/// What the texture tool selects when it clicks.
///
/// A face, or the whole brush. The two are orthogonal to [`TextureMode`]:
/// that decides *when* material is applied, this decides *what* is selected.
/// Some tools only make sense for one of these -- the face editor edits a
/// face -- and a tool may ignore the distinction when it does not apply.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum TextureTarget {
    /// Click selects the single face under the pointer.
    #[default]
    SingleFace,
    /// Click selects the whole brush -- or the entity that owns it.
    WholeBrush,
}

impl TextureTarget {
    pub fn label(self) -> &'static str {
        match self {
            TextureTarget::SingleFace => "single face",
            TextureTarget::WholeBrush => "whole brush",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            TextureTarget::SingleFace => "A click selects the one face under the pointer.",
            TextureTarget::WholeBrush => {
                "A click selects the whole brush -- or the entity that owns it, for a door."
            }
        }
    }

    pub fn all() -> [TextureTarget; 2] {
        [TextureTarget::SingleFace, TextureTarget::WholeBrush]
    }
}

/// A drag in progress.
#[derive(Clone, Copy, Debug)]
pub struct Drag {
    pub start: Vec3,
    pub current: Vec3,
    /// Whether the drag has moved far enough to count as a drag rather than a
    /// click. Without a threshold, every click nudges what it selects.
    pub is_dragging: bool,
    /// The resize grip this drag took hold of, if it started on one.
    ///
    /// Decided at the press and then fixed: working it out afresh each frame
    /// would let a drag change its mind about what it was doing as the
    /// selection moved under the pointer.
    pub grip: Option<Handle>,
    /// The selection's extent when the drag began.
    ///
    /// A resize is a ratio against the size it started at, so the size it
    /// started at has to be remembered. Reading it live would compound every
    /// frame's scaling into the next -- a drag of a few pixels would grow the
    /// brush without limit.
    pub from: Option<Aabb>,
}

impl Drag {
    pub fn bounds(&self) -> Aabb {
        Aabb::new(self.start.min(self.current), self.start.max(self.current))
    }

    pub fn delta(&self) -> Vec3 { self.current - self.start }

    /// Whether this drag is resizing rather than moving.
    pub fn is_resize(&self) -> bool { self.grip.is_some() }
}

/// Pixels a pointer must travel before a click becomes a drag.
pub const DRAG_THRESHOLD: f32 = 3.0;

/// The active tool and its drag state.
#[derive(Default)]
pub struct Tool {
    pub kind: ToolKind,
    pub drag: Option<Drag>,
    /// Where the drag started on screen, for the click-versus-drag test.
    drag_origin_px: (f32, f32),
    /// Class placed by the entity tool.
    pub entity_class: String,
    /// What the shape tool draws.
    pub shape: crate::shapes::Shape,
    /// How many sides it has, how far round it goes, how thick its wall is.
    pub shape_options: crate::shapes::Options,
    /// How the texture tool responds to a click.
    pub texture_mode: TextureMode,
    /// What the texture tool selects: one face, or the whole brush.
    pub texture_target: TextureTarget,
}

impl Tool {
    pub fn new() -> Tool {
        Tool {
            entity_class: "info_player_start".to_string(),
            shape_options: crate::shapes::Options::default(),
            texture_mode: TextureMode::default(),
            texture_target: TextureTarget::default(),
            ..Default::default()
        }
    }

    pub fn set_kind(&mut self, kind: ToolKind) {
        // Switching tools mid-drag would apply the new tool to the old drag.
        self.drag = None;
        self.kind = kind;
    }

    /// Pointer pressed in a viewport.
    pub fn press(&mut self, document: &Document, viewport: &Viewport, x: f32, y: f32) {
        let depth = default_depth(document, viewport);
        let world = document.grid.snap_point(viewport.screen_to_world(x, y, depth));
        self.drag_origin_px = (x, y);

        // Pressing on one of the selection's grips resizes it; pressing
        // anywhere else does what it always did. Only the select tool, and
        // only in a 2D pane: a resize needs two axes on screen and a third
        // that holds still, which is exactly what an orthographic view is.
        let (grip, from) = match (self.kind, document.selection_bounds()) {
            (ToolKind::Select, Some(bounds)) if viewport.kind.is_2d() => {
                (handle_at(bounds, viewport, x, y), Some(bounds))
            }
            _ => (None, None),
        };

        self.drag = Some(Drag {
            start: world,
            current: world,
            is_dragging: false,
            grip,
            from: grip.and(from),
        });
    }

    /// Pointer moved while held.
    pub fn drag_to(&mut self, document: &Document, viewport: &Viewport, x: f32, y: f32) {
        let Some(drag) = &mut self.drag else { return };
        let depth = drag.start[viewport.kind.axes().2];
        drag.current = document.grid.snap_point(viewport.screen_to_world(x, y, depth));

        let moved = ((x - self.drag_origin_px.0).powi(2) + (y - self.drag_origin_px.1).powi(2)).sqrt();
        if moved > DRAG_THRESHOLD { drag.is_dragging = true; }
    }

    /// Pointer released. Returns what the caller should do.
    pub fn release(&mut self, add_to_selection: bool) -> Option<ToolAction> {
        let drag = self.drag.take()?;
        Some(match self.kind {
            ToolKind::Block => {
                if !drag.is_dragging { return None; }
                ToolAction::CreateBlock(drag.bounds())
            }
            ToolKind::Shape => {
                if !drag.is_dragging { return None; }
                ToolAction::CreateShape {
                    bounds: drag.bounds(),
                    shape: self.shape,
                    options: self.shape_options,
                }
            }
            ToolKind::Entity => ToolAction::CreateEntity(self.entity_class.clone(), drag.start),
            ToolKind::Texture => ToolAction::ApplyMaterialAt(drag.start),
            ToolKind::Select => match (drag.grip, drag.from) {
                (Some(grip), Some(from)) if drag.is_dragging => {
                    ToolAction::Resize { from, grip, to: drag.current }
                }
                _ if drag.is_dragging => ToolAction::Move(drag.delta()),
                // A click on a grip with no drag behind it is a click, and
                // clicking is how you select something else.
                _ => ToolAction::PickAt(drag.start, add_to_selection),
            },
        })
    }

    pub fn cancel(&mut self) { self.drag = None; }
}

/// What a finished drag asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolAction {
    CreateBlock(Aabb),
    /// Fill a box with something that is not a box.
    ///
    /// The shape and its settings ride along rather than being read back off
    /// the tool, so the action is a complete description of what to do -- and
    /// so a test can ask for an arch without building a Tool to hold the
    /// request.
    CreateShape {
        bounds: Aabb,
        shape: crate::shapes::Shape,
        options: crate::shapes::Options,
    },
    /// Resize the selection by dragging one of its grips somewhere.
    ///
    /// Carries where the selection started rather than a finished scale
    /// factor, so the ratio is worked out once, against the size the drag
    /// began at, by the code that has the viewport to work it out in.
    Resize { from: Aabb, grip: Handle, to: Vec3 },
    CreateEntity(String, Vec3),
    ApplyMaterialAt(Vec3),
    Move(Vec3),
    /// Select whatever is at this point; `true` adds to the selection.
    PickAt(Vec3, bool),
}

/// One of the eight grips around a selection, for resizing it.
///
/// Named by which way it lies from the centre in the view's own two axes:
/// `(-1, -1)` is the corner nearest the origin of the pane, `(1, 0)` the
/// middle of the right edge. `(0, 0)` is not a handle -- that is the inside
/// of the box, which means move.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle {
    pub h: i8,
    pub v: i8,
}

/// How close, in pixels, the pointer must be to grab a handle.
///
/// Generous, because the alternative is a resize that only starts on about
/// one attempt in three, and because missing a handle costs nothing: the
/// press falls through to a move, which is undoable and obvious.
pub const HANDLE_GRAB: f32 = 7.0;

/// How big the drawn grips are, in pixels.
pub const HANDLE_SIZE: f32 = 8.0;

impl Handle {
    /// The eight grips, corners and edge midpoints.
    pub fn all() -> impl Iterator<Item = Handle> {
        [-1i8, 0, 1]
            .into_iter()
            .flat_map(|h| [-1i8, 0, 1].into_iter().map(move |v| Handle { h, v }))
            .filter(|grip| !(grip.h == 0 && grip.v == 0))
    }

    /// Where this grip sits in the world, given what is selected.
    pub fn world_position(self, bounds: Aabb, viewport: &Viewport) -> Vec3 {
        let (h, v, _) = viewport.kind.axes();
        let mut at = bounds.center();
        at[h] = pick(bounds.min[h], bounds.max[h], self.h);
        at[v] = pick(bounds.min[v], bounds.max[v], self.v);
        at
    }

    /// The grip opposite this one -- the corner a drag pivots about.
    ///
    /// Dragging the right edge must hold the left edge still. Anything else
    /// and the brush walks across the level while you resize it.
    pub fn opposite(self) -> Handle {
        Handle { h: -self.h, v: -self.v }
    }

    /// A short description, for the status bar.
    pub fn label(self) -> &'static str {
        match (self.h, self.v) {
            (0, _) | (_, 0) => "edge",
            _ => "corner",
        }
    }
}

fn pick(min: f32, max: f32, side: i8) -> f32 {
    match side {
        -1 => min,
        1 => max,
        _ => (min + max) * 0.5,
    }
}

/// The grip nearest a screen point, if the pointer is close enough to one.
///
/// Corners are tested before edges, because at small zoom levels a corner
/// grip and two edge grips overlap and the corner is the one that does what
/// you meant.
pub fn handle_at(bounds: Aabb, viewport: &Viewport, x: f32, y: f32) -> Option<Handle> {
    let mut best: Option<(f32, Handle)> = None;
    for grip in Handle::all() {
        let (hx, hy) = viewport.world_to_screen(grip.world_position(bounds, viewport));
        let distance = ((hx - x).powi(2) + (hy - y).powi(2)).sqrt();
        if distance > HANDLE_GRAB { continue }

        // A corner beats an edge at equal distance; otherwise nearest wins.
        let corner = grip.h != 0 && grip.v != 0;
        let ranked = if corner { distance - 0.001 } else { distance };
        if best.is_none_or(|(previous, _)| ranked < previous) {
            best = Some((ranked, grip));
        }
    }
    best.map(|(_, grip)| grip)
}

/// The scale a resize drag asks for, and the point it pivots about.
///
/// Returns `None` when the drag would collapse the selection to nothing.
/// Nothing is not a size: a brush of zero thickness still exists, still
/// compiles, and is invisible in every view -- so the drag is refused at one
/// grid square rather than allowed to produce one.
pub fn resize_factor(
    bounds: Aabb,
    viewport: &Viewport,
    grip: Handle,
    to: Vec3,
    minimum: f32,
) -> Option<(Vec3, Vec3)> {
    let (h, v, _) = viewport.kind.axes();
    let anchor = grip.opposite().world_position(bounds, viewport);
    let mut factor = Vec3::ONE;

    for (axis, side) in [(h, grip.h), (v, grip.v)] {
        if side == 0 { continue }
        let was = pick(bounds.min[axis], bounds.max[axis], side) - anchor[axis];
        if was.abs() < f32::EPSILON {
            // The selection is already flat on this axis, so there is no
            // ratio to scale by. Leaving it alone beats dividing by zero.
            continue;
        }
        let now = to[axis] - anchor[axis];
        // Dragged past the anchor, or squeezed below the smallest useful
        // size: either way the answer is the minimum, on the side it started.
        // The factor therefore stays positive, so the selection can be made
        // very small but never pulled inside out -- and an inverted brush is
        // not a small brush, it is a hole in the world that compiles.
        let size = if now.signum() == was.signum() {
            now.abs().max(minimum)
        } else {
            minimum
        };
        factor[axis] = size / was.abs();
    }

    (factor != Vec3::ONE).then_some((anchor, factor))
}

/// Depth to place new geometry at along a 2D view's hidden axis.
///
/// The current selection, if there is one, so that a brush drawn next to
/// another lands beside it rather than at the origin. That is the behaviour
/// that makes building in 2D views practical.
fn default_depth(document: &Document, viewport: &Viewport) -> f32 {
    if !viewport.kind.is_2d() { return 0.0; }
    let axis = viewport.kind.axes().2;
    match document.selection_bounds() {
        Some(bounds) => bounds.min[axis],
        None => 0.0,
    }
}

/// The solid nearest a point in a 2D view, if any.
///
/// Ties break toward the smallest brush: a small detail brush inside a large
/// room brush is the one you meant to click.
pub fn pick_solid_2d(document: &Document, point: Vec3, viewport: &Viewport) -> Option<u32> {
    let (h, v, _) = viewport.kind.axes();
    let mut best: Option<(f32, u32)> = None;

    for (_, solid) in document.map.all_solids() {
        let bounds = solid.bounds();
        // Only the two axes the view shows: the third is depth, and a 2D view
        // selects through the whole level.
        if point[h] < bounds.min[h] || point[h] > bounds.max[h] { continue; }
        if point[v] < bounds.min[v] || point[v] > bounds.max[v] { continue; }

        let area = (bounds.size()[h] * bounds.size()[v]).max(1.0);
        if best.is_none_or(|(best_area, _)| area < best_area) {
            best = Some((area, solid.id));
        }
    }
    best.map(|(_, id)| id)
}

/// The entity nearest a point in a 2D view.
pub fn pick_entity_2d(document: &Document, point: Vec3, viewport: &Viewport) -> Option<u32> {
    let (h, v, _) = viewport.kind.axes();
    // Point entities are drawn as a small box; clicking near one picks it.
    let reach = 16.0 / viewport.zoom.max(0.001);

    let mut best: Option<(f32, u32)> = None;
    for entity in document.map.entities.iter().filter(|e| e.solids.is_empty()) {
        let origin = entity.origin();
        let distance =
            ((origin[h] - point[h]).powi(2) + (origin[v] - point[v]).powi(2)).sqrt();
        if distance > reach { continue; }
        if best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, entity.id));
        }
    }
    best.map(|(_, id)| id)
}

/// The solid a 3D pick ray hits first.
pub fn pick_solid_3d(document: &Document, origin: Vec3, direction: Vec3) -> Option<u32> {
    let mut best: Option<(f32, u32)> = None;
    for (_, solid) in document.map.all_solids() {
        let Some(distance) = ray_box(origin, direction, solid.bounds()) else { continue };
        if best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, solid.id));
        }
    }
    best.map(|(_, id)| id)
}

/// The face a 3D pick ray hits first, as `(solid id, side id)`.
///
/// Against the actual face polygons rather than the brush's bounding box:
/// picking a *face* is what the texture tool does, and a box hit says nothing
/// about which of six faces you meant. Back-facing polygons are skipped, so
/// clicking a wall from inside a room picks the wall you can see and not the
/// one behind you.
pub fn pick_face_3d(
    document: &Document,
    origin: Vec3,
    direction: Vec3,
) -> Option<(u32, u32)> {
    let mut best: Option<(f32, (u32, u32))> = None;
    for (_, solid) in document.map.all_solids() {
        for (side, winding) in solid.face_windings() {
            let Some(plane) = side.plane() else { continue };
            let facing = plane.normal.dot(direction);
            // Only faces turned towards the ray, and never one it runs along.
            if facing >= -1e-6 { continue }

            let distance = -(plane.normal.dot(origin) - plane.dist) / facing;
            if distance < 0.0 { continue }
            let hit = origin + direction * distance;
            if !winding_contains(&winding, plane.normal, hit) { continue }
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, (solid.id, side.id)));
            }
        }
    }
    best.map(|(_, ids)| ids)
}

/// Whether a point on a face's plane is inside the face.
///
/// The winding is convex, so the point is inside when it falls on the same
/// side of every edge. Which side that is depends on the winding order, and
/// this project's is clockwise seen from the front -- so rather than encode
/// that here and be wrong the day it changes, the first edge that says
/// anything decides and the rest must agree.
///
/// The epsilon lets a click landing exactly on an edge count for one of the
/// two faces rather than for neither.
fn winding_contains(winding: &Winding, normal: Vec3, point: Vec3) -> bool {
    let n = winding.points.len();
    if n < 3 { return false }

    let mut sign = 0.0f32;
    for i in 0..n {
        let a = winding.points[i];
        let b = winding.points[(i + 1) % n];
        let side = (b - a).cross(point - a).dot(normal);
        if side.abs() <= 0.05 { continue }
        if sign == 0.0 {
            sign = side.signum();
        } else if side.signum() != sign {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::ViewportKind;

    fn setup() -> (Document, Viewport) {
        let mut document = Document::new();
        document.grid.size = 16.0;
        let viewport = Viewport { size: (800.0, 600.0), zoom: 1.0, ..Viewport::new(ViewportKind::Top) };
        (document, viewport)
    }

    #[test]
    fn dragging_the_block_tool_asks_for_a_brush() {
        let (document, viewport) = setup();
        let mut tool = Tool { kind: ToolKind::Block, ..Tool::new() };
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.drag_to(&document, &viewport, 500.0, 200.0);

        match tool.release(false) {
            Some(ToolAction::CreateBlock(bounds)) => {
                assert!(bounds.size().x > 0.0 && bounds.size().y > 0.0);
            }
            other => panic!("expected a block, got {other:?}"),
        }
    }

    #[test]
    fn a_click_with_the_block_tool_creates_nothing() {
        // Otherwise every stray click leaves a one-grid-unit brush behind.
        let (document, viewport) = setup();
        let mut tool = Tool { kind: ToolKind::Block, ..Tool::new() };
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.drag_to(&document, &viewport, 401.0, 300.0);
        assert_eq!(tool.release(false), None);
    }

    #[test]
    fn a_click_with_the_select_tool_picks_rather_than_moving() {
        let (document, viewport) = setup();
        let mut tool = Tool::new();
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.drag_to(&document, &viewport, 401.0, 301.0);
        assert!(matches!(tool.release(false), Some(ToolAction::PickAt(_, false))));
    }

    #[test]
    fn dragging_with_the_select_tool_moves() {
        let (document, viewport) = setup();
        let mut tool = Tool::new();
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.drag_to(&document, &viewport, 464.0, 300.0);
        match tool.release(false) {
            Some(ToolAction::Move(delta)) => assert_eq!(delta.x, 64.0),
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn shift_click_adds_to_the_selection() {
        let (document, viewport) = setup();
        let mut tool = Tool::new();
        tool.press(&document, &viewport, 400.0, 300.0);
        assert!(matches!(tool.release(true), Some(ToolAction::PickAt(_, true))));
    }

    #[test]
    fn switching_tools_abandons_the_drag_in_progress() {
        let (document, viewport) = setup();
        let mut tool = Tool { kind: ToolKind::Block, ..Tool::new() };
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.set_kind(ToolKind::Select);
        assert!(tool.drag.is_none());
        assert_eq!(tool.release(false), None);
    }

    #[test]
    fn new_geometry_lands_beside_the_selection_not_at_the_origin() {
        let (mut document, viewport) = setup();
        document.create_block(Vec3::new(0.0, 0.0, 128.0), Vec3::new(64.0, 64.0, 192.0));

        let mut tool = Tool { kind: ToolKind::Block, ..Tool::new() };
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.drag_to(&document, &viewport, 500.0, 200.0);
        match tool.release(false) {
            // The top view hides Z; the new brush should sit at the selected
            // brush's Z rather than back at zero.
            Some(ToolAction::CreateBlock(bounds)) => assert_eq!(bounds.min.z, 128.0),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn picking_in_2d_reaches_through_the_depth_axis() {
        // A top view must select a brush at any height, not only at z = 0.
        let (mut document, viewport) = setup();
        let id = document.create_block(Vec3::new(0.0, 0.0, 500.0), Vec3::new(64.0, 64.0, 564.0));
        let hit = pick_solid_2d(&document, Vec3::new(32.0, 32.0, 0.0), &viewport);
        assert_eq!(hit, Some(id));
    }

    #[test]
    fn picking_prefers_the_smaller_brush() {
        // A detail brush inside a room brush is the one you meant to click.
        let (mut document, viewport) = setup();
        document.create_block(Vec3::new(-256.0, -256.0, 0.0), Vec3::new(256.0, 256.0, 128.0));
        let small = document.create_block(Vec3::new(0.0, 0.0, 0.0), Vec3::new(32.0, 32.0, 32.0));
        assert_eq!(pick_solid_2d(&document, Vec3::new(16.0, 16.0, 0.0), &viewport), Some(small));
    }

    #[test]
    fn clicking_empty_space_picks_nothing() {
        let (mut document, viewport) = setup();
        document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        assert_eq!(pick_solid_2d(&document, Vec3::new(1000.0, 1000.0, 0.0), &viewport), None);
    }

    #[test]
    fn point_entities_can_be_picked_near_their_origin() {
        let (mut document, viewport) = setup();
        let id = document.create_entity("light", Vec3::new(100.0, 100.0, 0.0));
        assert_eq!(pick_entity_2d(&document, Vec3::new(104.0, 104.0, 0.0), &viewport), Some(id));
        assert_eq!(pick_entity_2d(&document, Vec3::new(400.0, 400.0, 0.0), &viewport), None);
    }

    #[test]
    fn the_3d_ray_picks_the_nearest_brush() {
        let mut document = Document::new();
        let near = document.create_block(Vec3::new(100.0, -32.0, -32.0), Vec3::new(164.0, 32.0, 32.0));
        document.create_block(Vec3::new(400.0, -32.0, -32.0), Vec3::new(464.0, 32.0, 32.0));
        assert_eq!(pick_solid_3d(&document, Vec3::ZERO, Vec3::X), Some(near));
        assert_eq!(pick_solid_3d(&document, Vec3::ZERO, -Vec3::X), None);
    }

    #[test]
    fn every_tool_has_a_label_and_a_shortcut() {
        for kind in ToolKind::all() {
            assert!(!kind.label().is_empty());
            assert!(!kind.shortcut().is_empty());
        }
    }

    // ---- resize grips ------------------------------------------------------

    /// A document with one 128-unit box selected, and a top view of it.
    fn with_a_selected_box() -> (Document, Viewport, Aabb) {
        let (mut document, viewport) = setup();
        let bounds = Aabb::new(Vec3::ZERO, Vec3::splat(128.0));
        let id = document.create_block(bounds.min, bounds.max);
        document.selection.clear();
        document.selection.solids.insert(id);
        (document, viewport, bounds)
    }

    #[test]
    fn every_grip_is_a_side_or_a_corner_and_never_the_middle() {
        let grips: Vec<Handle> = Handle::all().collect();
        assert_eq!(grips.len(), 8);
        assert!(!grips.contains(&Handle { h: 0, v: 0 }), "the middle is a move, not a resize");
        assert_eq!(grips.iter().filter(|g| g.h != 0 && g.v != 0).count(), 4, "four corners");
        assert_eq!(grips.iter().filter(|g| g.h == 0 || g.v == 0).count(), 4, "four edges");
    }

    #[test]
    fn a_grip_sits_where_the_selection_does() {
        let (_, viewport, bounds) = with_a_selected_box();
        // Top view: horizontal is x, vertical is y.
        let corner = Handle { h: -1, v: -1 }.world_position(bounds, &viewport);
        assert_eq!(corner.x, 0.0);
        assert_eq!(corner.y, 0.0);

        let right_edge = Handle { h: 1, v: 0 }.world_position(bounds, &viewport);
        assert_eq!(right_edge.x, 128.0);
        assert_eq!(right_edge.y, 64.0, "the middle of the edge, not a corner");
    }

    #[test]
    fn every_grip_has_an_opposite_that_is_the_one_across_from_it() {
        for grip in Handle::all() {
            assert_eq!(grip.opposite().opposite(), grip);
            assert_ne!(grip.opposite(), grip, "nothing is its own anchor");
        }
    }

    #[test]
    fn pressing_on_a_grip_takes_hold_of_it() {
        let (document, viewport, bounds) = with_a_selected_box();
        let mut tool = Tool::new();
        let (x, y) = viewport.world_to_screen(Handle { h: 1, v: 1 }.world_position(bounds, &viewport));

        tool.press(&document, &viewport, x, y);
        assert_eq!(tool.drag.unwrap().grip, Some(Handle { h: 1, v: 1 }));
        assert!(tool.drag.unwrap().is_resize());
    }

    #[test]
    fn pressing_in_the_middle_of_the_selection_is_still_a_move() {
        let (document, viewport, bounds) = with_a_selected_box();
        let mut tool = Tool::new();
        let (x, y) = viewport.world_to_screen(bounds.center());

        tool.press(&document, &viewport, x, y);
        assert_eq!(tool.drag.unwrap().grip, None);
    }

    #[test]
    fn only_the_select_tool_has_grips() {
        // Dragging a corner with the block tool draws a brush there, which is
        // what the block tool is for.
        let (document, viewport, bounds) = with_a_selected_box();
        let mut tool = Tool { kind: ToolKind::Block, ..Tool::new() };
        let (x, y) = viewport.world_to_screen(Handle { h: 1, v: 1 }.world_position(bounds, &viewport));

        tool.press(&document, &viewport, x, y);
        assert_eq!(tool.drag.unwrap().grip, None);
    }

    #[test]
    fn dragging_a_grip_asks_for_a_resize() {
        let (document, viewport, bounds) = with_a_selected_box();
        let mut tool = Tool::new();
        let (x, y) = viewport.world_to_screen(Handle { h: 1, v: 1 }.world_position(bounds, &viewport));

        tool.press(&document, &viewport, x, y);
        tool.drag_to(&document, &viewport, x + 128.0, y - 128.0);

        match tool.release(false) {
            Some(ToolAction::Resize { from, grip, .. }) => {
                assert_eq!(from, bounds);
                assert_eq!(grip, Handle { h: 1, v: 1 });
            }
            other => panic!("expected a resize, got {other:?}"),
        }
    }

    #[test]
    fn a_click_on_a_grip_selects_rather_than_resizing_by_nothing() {
        let (document, viewport, bounds) = with_a_selected_box();
        let mut tool = Tool::new();
        let (x, y) = viewport.world_to_screen(Handle { h: 1, v: 1 }.world_position(bounds, &viewport));

        tool.press(&document, &viewport, x, y);
        assert!(matches!(tool.release(false), Some(ToolAction::PickAt(..))));
    }

    #[test]
    fn dragging_a_corner_scales_both_axes_about_the_far_corner() {
        let (_, viewport, bounds) = with_a_selected_box();
        let grip = Handle { h: 1, v: 1 };
        let to = Vec3::new(256.0, 256.0, 0.0);

        let (anchor, factor) = resize_factor(bounds, &viewport, grip, to, 16.0).unwrap();
        assert_eq!(anchor, Vec3::new(0.0, 0.0, 64.0), "the opposite corner holds still");
        assert_eq!(factor.x, 2.0);
        assert_eq!(factor.y, 2.0);
        assert_eq!(factor.z, 1.0, "the axis the view cannot see is untouched");
    }

    #[test]
    fn dragging_an_edge_scales_only_that_axis() {
        let (_, viewport, bounds) = with_a_selected_box();
        let grip = Handle { h: 1, v: 0 };
        let to = Vec3::new(64.0, 999.0, 0.0);

        let (_, factor) = resize_factor(bounds, &viewport, grip, to, 16.0).unwrap();
        assert_eq!(factor.x, 0.5);
        assert_eq!(factor.y, 1.0, "an edge grip does not touch the other axis");
    }

    #[test]
    fn dragging_a_grip_past_the_far_side_stops_at_the_smallest_size() {
        // Otherwise the brush turns inside out on the way through, which
        // compiles and produces a hole in the world.
        let (_, viewport, bounds) = with_a_selected_box();
        let grip = Handle { h: 1, v: 1 };
        let to = Vec3::new(-500.0, -500.0, 0.0);

        let (anchor, factor) = resize_factor(bounds, &viewport, grip, to, 16.0).unwrap();
        assert!(factor.x > 0.0 && factor.y > 0.0, "never inverted: {factor:?}");
        let size = bounds.size() * factor;
        assert_eq!(size.x, 16.0, "clamped to one grid square");
        assert_eq!(size.y, 16.0);
        assert_eq!(anchor, Vec3::new(0.0, 0.0, 64.0));
    }

    #[test]
    fn a_resize_that_changes_nothing_asks_for_nothing() {
        let (_, viewport, bounds) = with_a_selected_box();
        let grip = Handle { h: 1, v: 1 };
        let to = Handle { h: 1, v: 1 }.world_position(bounds, &viewport);
        assert!(resize_factor(bounds, &viewport, grip, to, 16.0).is_none());
    }

    #[test]
    fn a_selection_already_flat_on_an_axis_is_left_flat_rather_than_divided_by_zero() {
        let (_, viewport, _) = with_a_selected_box();
        let flat = Aabb::new(Vec3::ZERO, Vec3::new(0.0, 128.0, 128.0));
        let grip = Handle { h: 1, v: 1 };

        let (_, factor) = resize_factor(flat, &viewport, grip, Vec3::new(64.0, 256.0, 0.0), 16.0).unwrap();
        assert!(factor.x.is_finite(), "{factor:?}");
        assert_eq!(factor.x, 1.0);
        assert_eq!(factor.y, 2.0);
    }

    #[test]
    fn the_grip_under_the_pointer_is_the_one_you_get() {
        let (_, viewport, bounds) = with_a_selected_box();
        for grip in Handle::all() {
            let (x, y) = viewport.world_to_screen(grip.world_position(bounds, &viewport));
            assert_eq!(handle_at(bounds, &viewport, x, y), Some(grip), "{grip:?}");
        }
        let (cx, cy) = viewport.world_to_screen(bounds.center());
        assert_eq!(handle_at(bounds, &viewport, cx, cy), None, "the middle is not a grip");
    }

    #[test]
    fn a_corner_wins_over_the_edges_that_meet_at_it() {
        // At a low zoom the three grips overlap, and the corner is the one
        // anybody clicking there meant.
        let (_, viewport, bounds) = with_a_selected_box();
        let tiny = Viewport { zoom: 0.02, ..viewport };
        let corner = Handle { h: 1, v: 1 };
        let (x, y) = tiny.world_to_screen(corner.world_position(bounds, &tiny));

        assert_eq!(handle_at(bounds, &tiny, x, y), Some(corner));
    }

    #[test]
    fn dragging_the_shape_tool_asks_for_the_shape_it_is_set_to() {
        let (document, viewport) = setup();
        let mut tool = Tool {
            kind: ToolKind::Shape,
            shape: crate::shapes::Shape::Arch,
            ..Tool::new()
        };
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.drag_to(&document, &viewport, 600.0, 100.0);

        match tool.release(false) {
            Some(ToolAction::CreateShape { bounds, shape, options }) => {
                assert_eq!(shape, crate::shapes::Shape::Arch);
                assert!(bounds.size().x > 0.0);
                assert_eq!(options, crate::shapes::Options::default());
            }
            other => panic!("expected a shape, got {other:?}"),
        }
    }

    #[test]
    fn a_click_with_the_shape_tool_creates_nothing() {
        let (document, viewport) = setup();
        let mut tool = Tool { kind: ToolKind::Shape, ..Tool::new() };
        tool.press(&document, &viewport, 400.0, 300.0);
        assert_eq!(tool.release(false), None);
    }

    #[test]
    fn every_tool_advertises_a_distinct_shortcut() {
        // They are also what the keyboard handler binds, so a duplicate would
        // make one of the two tools unreachable.
        let mut seen = std::collections::HashSet::new();
        for kind in ToolKind::all() {
            assert!(seen.insert(kind.shortcut()), "{} reuses a shortcut", kind.label());
        }
    }

    #[test]
    fn the_texture_modes_cycle_and_are_labelled() {
        let mut seen = std::collections::HashSet::new();
        let mut mode = TextureMode::default();
        for _ in 0..TextureMode::all().len() {
            assert!(seen.insert(mode), "a mode appeared twice in the cycle");
            assert!(!mode.label().is_empty());
            assert!(!mode.describe().is_empty());
            mode = mode.next();
        }
        assert_eq!(mode, TextureMode::default(), "three modes, back to the start");
    }

    #[test]
    fn the_texture_targets_are_labelled() {
        let mut seen = std::collections::HashSet::new();
        for target in TextureTarget::all() {
            assert!(seen.insert(target), "a target appeared twice");
            assert!(!target.label().is_empty());
            assert!(!target.describe().is_empty());
        }
    }

}

#[cfg(test)]
mod face_picking_tests {
    use super::*;
    use kerosene_map::Solid;
    use kerosene_math::Aabb;

    fn room() -> (Document, u32) {
        let mut document = Document::new();
        document.map.world.solids.clear();
        let id = document.map.next_id();
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(128.0)), "dev/grid");
        solid.id = id;
        document.map.world.solids.push(solid);
        (document, id)
    }

    #[test]
    fn a_ray_picks_the_face_it_points_at_not_just_the_brush() {
        // The texture tool needs a face; a bounding-box hit says nothing about
        // which of six faces you meant.
        let (document, id) = room();
        let hit = pick_face_3d(&document, Vec3::new(-100.0, 64.0, 64.0), Vec3::X)
            .expect("the ray hits the cube");
        assert_eq!(hit.0, id);

        let side = document
            .find_solid(id)
            .unwrap()
            .sides
            .iter()
            .find(|s| s.id == hit.1)
            .expect("the side exists");
        assert!(
            side.plane().unwrap().normal.x < -0.9,
            "picked the wrong face: {:?}",
            side.plane().unwrap().normal
        );
    }

    #[test]
    fn the_nearest_face_wins() {
        let (mut document, first) = room();
        let id = document.map.next_id();
        let mut nearer = Solid::cube(
            Aabb::new(Vec3::new(-200.0, 0.0, 0.0), Vec3::new(-150.0, 128.0, 128.0)),
            "dev/wall",
        );
        nearer.id = id;
        document.map.world.solids.push(nearer);

        let hit = pick_face_3d(&document, Vec3::new(-400.0, 64.0, 64.0), Vec3::X).unwrap();
        assert_eq!(hit.0, id, "the far brush was picked over the near one");
        let _ = first;
    }

    #[test]
    fn a_ray_pointing_away_hits_nothing() {
        let (document, _) = room();
        assert!(pick_face_3d(&document, Vec3::new(-100.0, 64.0, 64.0), -Vec3::X).is_none());
    }

    #[test]
    fn a_ray_missing_the_face_hits_nothing_even_on_its_plane() {
        // On the plane of the -X face, but past the end of it.
        let (document, _) = room();
        assert!(pick_face_3d(&document, Vec3::new(-100.0, 900.0, 64.0), Vec3::X).is_none());
    }

    #[test]
    fn standing_in_a_room_picks_the_wall_you_are_looking_at() {
        // The face whose normal points back at the ray, not the one behind
        // the camera that the ray would also cross.
        let document = crate::app::starter_document();
        let hit = pick_face_3d(&document, Vec3::new(256.0, 256.0, 64.0), Vec3::X)
            .expect("a sealed room has a wall in every direction");

        let solid = document.find_solid(hit.0).expect("the brush exists");
        let side = solid.sides.iter().find(|s| s.id == hit.1).expect("the side exists");
        let normal = side.plane().unwrap().normal;
        assert!(normal.x < -0.9, "picked a face turned away from the camera: {normal:?}");
    }

    #[test]
    fn a_camera_buried_in_solid_rock_picks_nothing() {
        // Every face of the brush around it is either behind it or turned
        // away. Reporting a hit would mean texturing a face you cannot see.
        let (document, _) = room();
        assert!(pick_face_3d(&document, Vec3::splat(64.0), Vec3::X).is_none());
    }

    #[test]
    fn every_face_of_a_cube_is_reachable() {
        let (document, id) = room();
        let mut picked = std::collections::HashSet::new();
        for (from, direction) in [
            (Vec3::new(-200.0, 64.0, 64.0), Vec3::X),
            (Vec3::new(300.0, 64.0, 64.0), -Vec3::X),
            (Vec3::new(64.0, -200.0, 64.0), Vec3::Y),
            (Vec3::new(64.0, 300.0, 64.0), -Vec3::Y),
            (Vec3::new(64.0, 64.0, -200.0), Vec3::Z),
            (Vec3::new(64.0, 64.0, 300.0), -Vec3::Z),
        ] {
            let hit = pick_face_3d(&document, from, direction).expect("hits");
            assert_eq!(hit.0, id);
            picked.insert(hit.1);
        }
        assert_eq!(picked.len(), 6, "some faces are unreachable: {picked:?}");
    }

}
