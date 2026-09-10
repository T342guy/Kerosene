// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "chisel/document.hpp"
#include "math/aabb.hpp"
#include "math/angles.hpp"
#include "math/mat.hpp"
#include "math/vec.hpp"

#include <optional>
#include <string_view>

namespace kero::chisel {

using math::Angles;
using math::Mat4;
using math::Vec2;
using math::Vec3;
using math::Vec3d;

/// Which way a viewport looks.
///
/// Three orthographic and one perspective, the arrangement every brush editor
/// has used since Quake -- not out of nostalgia but because building on a grid
/// needs a view with no foreshortening, and three of them are what pin a point
/// in space. The 3D view is for judging the result, not for placing anything.
enum class ViewKind : u8 {
    Top,     ///< Looking down. X right, Y up.
    Front,   ///< Looking north. X right, Z up.
    Side,    ///< Looking west. Y right, Z up.
    Perspective,
};

[[nodiscard]] std::string_view name_of(ViewKind kind);
[[nodiscard]] bool is_orthographic(ViewKind kind);

/// The world axes a viewport's screen axes correspond to.
struct ViewAxes {
    Vec3 right;
    Vec3 up;
    Vec3 forward;  ///< Into the screen.
};

[[nodiscard]] ViewAxes axes_of(ViewKind kind);

/// A ray through the world, for picking.
struct Ray {
    Vec3d origin;
    Vec3d direction;  ///< Unit length.
};

/// What a ray hit.
struct Hit {
    bool valid = false;
    i32 solid = 0;
    /// Which side of that solid the ray entered through.
    usize face = 0;
    f64 distance = 0.0;
    Vec3d point;

    explicit operator bool() const { return valid; }
};

/// One pane of the editor.
///
/// Holds where it is looking and how big it is; the panel sets the size from
/// whatever room ImGui gave it, every frame.
struct Viewport {
    ViewKind kind = ViewKind::Top;

    /// The world point at the middle of an orthographic view.
    Vec3 centre;
    /// Pixels per kerosene unit. At 1.0 a 128 ku room is 128 pixels across.
    f32 zoom = 0.5f;

    /// The 3D camera.
    Vec3 eye{-256.0f, -256.0f, 192.0f};
    Angles angles{25.0f, 45.0f, 0.0f};
    f32 fov = 90.0f;

    f32 width = 1.0f;
    f32 height = 1.0f;

    [[nodiscard]] ViewAxes axes() const { return axes_of(kind); }

    /// Clip-space transform, for drawing.
    [[nodiscard]] Mat4 view_projection() const;

    /// The world point under a pixel.
    ///
    /// For an orthographic view the depth is unconstrained -- a pixel names a
    /// line, not a point -- so the answer lies on the plane through `centre`.
    /// That is what makes drawing on a grid work: the depth you get is the
    /// depth you were looking at.
    [[nodiscard]] Vec3 world_from_screen(Vec2 pixel) const;

    /// Where a world point lands, in pixels from the viewport's top left.
    [[nodiscard]] Vec2 screen_from_world(Vec3 world) const;

    /// The pick ray through a pixel. Parallel to the view axis in an
    /// orthographic view, and through the eye in the 3D one.
    [[nodiscard]] Ray ray_from_screen(Vec2 pixel) const;

    /// How many kerosene units a pixel covers, for hit tolerances that feel the
    /// same at every zoom.
    [[nodiscard]] f32 units_per_pixel() const;

    /// Moves the view to look at a box -- what "frame the selection" does.
    void frame(const math::Aabb& bounds);

    /// Pans an orthographic view by a pixel delta, or moves the 3D camera.
    void pan(Vec2 pixels);

    /// Zooms about a pixel, so the world point under the cursor stays put.
    void zoom_at(Vec2 pixel, f32 factor);
};

/// Where a ray enters a brush, if it does.
///
/// A brush is the intersection of its sides' half-spaces, so this is the slab
/// method: clip the ray's parameter range by each plane in turn and see whether
/// anything survives. Exact, cheap, and it falls out of the same property that
/// makes the format worth having -- no triangulation, and the entering plane is
/// the face that was hit, which is what the editor needs to highlight.
[[nodiscard]] Hit pick_solid(const map::Solid& solid, const Ray& ray);

/// The nearest brush a ray hits.
[[nodiscard]] Hit pick(const Document& document, const Ray& ray);

/// The nearest point entity within `radius` world units of a ray.
///
/// Point entities have no geometry, so they are picked as spheres. The radius
/// comes from the viewport's scale so the target stays the same size on screen
/// however far zoomed out -- an entity you cannot click is an entity you cannot
/// edit.
[[nodiscard]] std::optional<i32> pick_entity(const Document& document, const Ray& ray,
                                             f64 radius);

/// Snaps to a grid. Zero or less leaves the value alone.
[[nodiscard]] f64 snap_to_grid(f64 value, f64 grid);
[[nodiscard]] Vec3d snap_to_grid(const Vec3d& value, f64 grid);

}  // namespace kero::chisel
