// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "chisel/document.hpp"
#include "chisel/viewport.hpp"
#include "map/map.hpp"
#include "math/aabb.hpp"

#include <array>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

/// The geometry behind the editing tools.
///
/// Deliberately free functions over plain values, with no reference to ImGui, a
/// viewport texture or a mouse. The tools themselves are a few lines of state
/// on top of these; putting the arithmetic here is what makes the interesting
/// half of an editor testable without a window.
namespace kero::chisel {

/// The polyline in a `.kleak` file: the path from the entity that leaked out to
/// the hole it escaped through.
///
/// Empty when there is no such file, which is the ordinary case and not an
/// error -- Cleave deletes a stale one whenever a level compiles clean, so the
/// editor simply stops drawing it.
[[nodiscard]] std::vector<Vec3d> load_leak_path(const std::string& path);

/// The default texture axes for a plane.
///
/// Quake's base-axis table: the world axis the face most nearly faces picks a
/// pair of tangents, so a wall is textured upright and a floor is textured
/// north-up. It is a table rather than a derivation because any continuous
/// choice of tangent has to spin somewhere, and a seam that moves when you drag
/// a face is worse than one that is always in the same place.
struct TextureAxes {
    map::TextureAxis u;
    map::TextureAxis v;
};

[[nodiscard]] TextureAxes default_texture_axes(const Vec3d& normal, f64 scale = 0.5);

/// A six-sided box. Ids are taken from the document's pool.
///
/// Returns an invalid solid -- fewer than four sides -- if the box has no
/// volume, which is what a click that never became a drag produces.
[[nodiscard]] map::Solid make_box(const math::Aabbd& bounds, Document& document,
                                  std::string_view material);

/// Moves a solid by moving every plane.
///
/// This is the whole reason the format stores planes rather than vertices: a
/// box stays a box because its half-spaces stay half-spaces. There is no
/// arrangement of drags that can make a brush non-convex, so there is no
/// validation pass that has to catch one.
[[nodiscard]] map::Solid translate(const map::Solid& solid, const Vec3d& delta);

/// Maps a solid's planes from one box onto another.
///
/// The texture axes are left alone on purpose. They are world-space
/// projections, so a wall that grows shows more texture rather than the same
/// texture stretched -- which is the behaviour a person resizing a room wants
/// and the reason the axes are stored against the plane in the first place.
[[nodiscard]] map::Solid resize(const map::Solid& solid, const math::Aabbd& from,
                                const math::Aabbd& to);

/// A copy of a solid with a material on one side, or on every side.
///
/// The texture axes come with it when the face's plane faces a different way
/// than the material was last aligned for -- a material dragged onto a wall
/// should be upright on that wall, not carrying the floor's alignment across.
[[nodiscard]] map::Solid with_material(const map::Solid& solid,
                                       std::optional<usize> side,
                                       std::string_view material);

/// The bounds of everything selected. Empty when nothing is.
[[nodiscard]] math::Aabbd selection_bounds(const Document& document);

/// One of the eight handles around a selection.
///
/// Named by which way each screen axis is pulled: a corner grip pulls on both
/// and scales two axes, an edge grip pulls on one and scales one. `(0, 0)` is
/// the middle of the box, which is a move rather than a resize and so is not a
/// grip.
struct Grip {
    i32 across = 0;  ///< -1, 0 or +1 along the view's right axis.
    i32 down = 0;    ///< -1, 0 or +1 along the view's up axis.

    [[nodiscard]] friend constexpr bool operator==(const Grip&, const Grip&) = default;
};

[[nodiscard]] std::array<Grip, 8> grips();

/// Where a grip sits in world space, for drawing and hit-testing it.
[[nodiscard]] Vec3d grip_position(const math::Aabbd& bounds, const ViewAxes& axes,
                                  Grip grip);

/// The box that results from dragging a grip by a world delta.
///
/// Only the sides the grip holds move, and a side is never dragged past the one
/// opposite it -- a brush turned inside out is not a brush, and refusing the
/// last unit of a drag is a great deal kinder than accepting it.
[[nodiscard]] math::Aabbd drag_grip(const math::Aabbd& bounds, const ViewAxes& axes,
                                    Grip grip, const Vec3d& delta);

/// The box a Block drag spans.
///
/// The two corners come from the view's plane, so the pane you draw in decides
/// which way the block stands: its depth is taken along the axis that view
/// looks down, centred on the depth you were already looking at.
[[nodiscard]] math::Aabbd block_bounds(const ViewAxes& axes, const Vec3d& from,
                                       const Vec3d& to, f64 depth, f64 grid);

/// Snaps a box's moving sides to the grid. Zero or less leaves it alone.
[[nodiscard]] math::Aabbd snap_bounds(const math::Aabbd& bounds, f64 grid);

}  // namespace kero::chisel
