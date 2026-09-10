// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "chisel/viewport.hpp"

#include "math/units.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

namespace kero::chisel {
namespace {

/// How far back an orthographic pick ray starts.
///
/// Far enough to be outside anything in the world, so the ray enters every
/// brush it will hit rather than starting inside one -- which would report the
/// far face, or nothing.
constexpr f64 kOrthoRayStart = static_cast<f64>(units::kWorldExtent) * 2.0;

constexpr f32 kMinZoom = 0.01f;   // A whole 16384 ku level in 164 pixels.
constexpr f32 kMaxZoom = 64.0f;   // A quarter of a unit per pixel.

}  // namespace

std::string_view name_of(ViewKind kind) {
    switch (kind) {
        case ViewKind::Top:         return "Top (x/y)";
        case ViewKind::Front:       return "Front (x/z)";
        case ViewKind::Side:        return "Side (y/z)";
        case ViewKind::Perspective: return "3D";
    }
    return "?";
}

bool is_orthographic(ViewKind kind) { return kind != ViewKind::Perspective; }

ViewAxes axes_of(ViewKind kind) {
    switch (kind) {
        case ViewKind::Top:
            // Looking down. North is up the screen, which is how a map reads.
            return ViewAxes{Vec3(1, 0, 0), Vec3(0, 1, 0), Vec3(0, 0, -1)};
        case ViewKind::Front:
            // Looking north, from the south. Up is up.
            return ViewAxes{Vec3(1, 0, 0), Vec3(0, 0, 1), Vec3(0, 1, 0)};
        case ViewKind::Side:
            // Looking west, from the east.
            return ViewAxes{Vec3(0, 1, 0), Vec3(0, 0, 1), Vec3(-1, 0, 0)};
        case ViewKind::Perspective:
            break;
    }
    return ViewAxes{Vec3(1, 0, 0), Vec3(0, 0, 1), Vec3(0, 1, 0)};
}

f32 Viewport::units_per_pixel() const {
    if (is_orthographic(kind)) {
        return 1.0f / std::max(zoom, kMinZoom);
    }
    // In perspective there is no single answer; this is the scale at a
    // plausible working distance, which is what a pick tolerance wants.
    return 2.0f * std::tan(math::to_radians(fov) * 0.5f) * 128.0f /
           std::max(height, 1.0f);
}

Mat4 Viewport::view_projection() const {
    if (kind == ViewKind::Perspective) {
        const f32 aspect = height > 0.0f ? width / height : 1.0f;
        return Mat4::perspective(fov, aspect, 1.0f, units::kWorldExtent * 4.0f) *
               Mat4::view_from_angles(eye, angles);
    }

    const ViewAxes view = axes();
    const f32 half_width = width * 0.5f / std::max(zoom, kMinZoom);
    const f32 half_height = height * 0.5f / std::max(zoom, kMinZoom);

    // An orthographic projection built by hand rather than through Mat4: the
    // depth range has to span the whole world in both directions, because an
    // orthographic view has no near plane worth speaking of and clipping the
    // level in half would be a strange way to find out.
    constexpr f32 kDepth = units::kWorldExtent * 4.0f;

    Mat4 projection = Mat4::identity();
    projection.m[0][0] = 1.0f / half_width;
    projection.m[1][1] = 1.0f / half_height;
    // Depth into [0, 1], matching the perspective projection so one depth
    // buffer setup serves both.
    projection.m[2][2] = -0.5f / kDepth;
    projection.m[3][2] = 0.5f;

    Mat4 look = Mat4::identity();
    look.m[0][0] = view.right.x;
    look.m[1][0] = view.right.y;
    look.m[2][0] = view.right.z;
    look.m[0][1] = view.up.x;
    look.m[1][1] = view.up.y;
    look.m[2][1] = view.up.z;
    look.m[0][2] = view.forward.x;
    look.m[1][2] = view.forward.y;
    look.m[2][2] = view.forward.z;
    look.m[3][0] = -dot(view.right, centre);
    look.m[3][1] = -dot(view.up, centre);
    look.m[3][2] = -dot(view.forward, centre);

    return projection * look;
}

Vec3 Viewport::world_from_screen(Vec2 pixel) const {
    const ViewAxes view = axes();
    const f32 scale = 1.0f / std::max(zoom, kMinZoom);

    // Screen Y runs down, world up runs up.
    const f32 across = (pixel.x - width * 0.5f) * scale;
    const f32 down = (pixel.y - height * 0.5f) * scale;

    return centre + view.right * across - view.up * down;
}

Vec2 Viewport::screen_from_world(Vec3 world) const {
    const ViewAxes view = axes();
    const Vec3 offset = world - centre;
    return Vec2(width * 0.5f + dot(offset, view.right) * zoom,
                height * 0.5f - dot(offset, view.up) * zoom);
}

Ray Viewport::ray_from_screen(Vec2 pixel) const {
    if (kind == ViewKind::Perspective) {
        Vec3 forward;
        Vec3 right;
        Vec3 up;
        math::angle_vectors(angles, &forward, &right, &up);
        // angle_vectors' right is the one the camera basis wants; see
        // Mat4::view_from_angles.
        const f32 aspect = height > 0.0f ? width / height : 1.0f;
        const f32 tangent = std::tan(math::to_radians(fov) * 0.5f);

        const f32 across = (pixel.x / std::max(width, 1.0f) * 2.0f - 1.0f) * tangent *
                           aspect;
        const f32 down = (pixel.y / std::max(height, 1.0f) * 2.0f - 1.0f) * tangent;

        Vec3 direction = forward + right * -across + up * -down;
        direction.normalize();
        return Ray{Vec3d(eye), Vec3d(direction)};
    }

    const ViewAxes view = axes();
    const Vec3 on_plane = world_from_screen(pixel);
    // Started well behind everything, so the ray enters each brush rather than
    // beginning inside one.
    const Vec3d direction(view.forward);
    return Ray{Vec3d(on_plane) - direction * kOrthoRayStart, direction};
}

void Viewport::frame(const math::Aabb& bounds) {
    if (bounds.empty()) {
        return;
    }

    const Vec3 middle = bounds.centre();
    if (kind == ViewKind::Perspective) {
        const Vec3 size = bounds.size();
        const f32 extent = std::max({size.x, size.y, size.z, 32.0f});
        // Far enough back that the box fits the vertical field of view, with
        // room to spare so it does not sit exactly on the edges.
        const f32 distance = extent / std::tan(math::to_radians(fov) * 0.5f) * 0.75f;
        Vec3 forward;
        math::angle_vectors(angles, &forward, nullptr, nullptr);
        eye = middle - forward * distance;
        return;
    }

    const ViewAxes view = axes();
    centre = middle;

    const Vec3 size = bounds.size();
    const f32 across = std::abs(dot(size, view.right));
    const f32 up = std::abs(dot(size, view.up));
    if (across <= 0.0f && up <= 0.0f) {
        return;
    }

    // A tenth of margin, so the thing framed is not flush against the edges.
    const f32 fit_x = across > 0.0f ? width / (across * 1.1f) : kMaxZoom;
    const f32 fit_y = up > 0.0f ? height / (up * 1.1f) : kMaxZoom;
    zoom = std::clamp(std::min(fit_x, fit_y), kMinZoom, kMaxZoom);
}

void Viewport::pan(Vec2 pixels) {
    const ViewAxes view = axes();
    if (kind == ViewKind::Perspective) {
        Vec3 forward;
        Vec3 right;
        Vec3 up;
        math::angle_vectors(angles, &forward, &right, &up);
        eye += right * pixels.x + up * pixels.y;
        return;
    }

    const f32 scale = 1.0f / std::max(zoom, kMinZoom);
    centre -= view.right * (pixels.x * scale);
    centre += view.up * (pixels.y * scale);
}

void Viewport::zoom_at(Vec2 pixel, f32 factor) {
    if (kind == ViewKind::Perspective) {
        Vec3 forward;
        math::angle_vectors(angles, &forward, nullptr, nullptr);
        eye += forward * ((factor - 1.0f) * 128.0f);
        return;
    }

    // The world point under the cursor before and after must be the same one,
    // so zooming goes where you are looking rather than to the middle. Without
    // this you spend the whole time zooming and then panning back.
    const Vec3 before = world_from_screen(pixel);
    zoom = std::clamp(zoom * factor, kMinZoom, kMaxZoom);
    const Vec3 after = world_from_screen(pixel);
    centre += before - after;
}

// ---------------------------------------------------------------------------
// Picking
// ---------------------------------------------------------------------------

Hit pick_solid(const map::Solid& solid, const Ray& ray) {
    Hit hit;
    if (solid.sides.size() < 4) {
        return hit;
    }

    // The slab method against the brush's own half-spaces. A brush *is* the
    // intersection of them, so this needs no triangulation and no winding --
    // and the plane the ray last entered through is the face that was hit,
    // which is what the editor wants to highlight.
    f64 enter = -std::numeric_limits<f64>::max();
    f64 leave = std::numeric_limits<f64>::max();
    usize entering_face = 0;

    for (usize i = 0; i < solid.sides.size(); ++i) {
        const math::Planed& plane = solid.sides[i].plane;
        const f64 slope = dot(plane.normal, ray.direction);
        const f64 distance = dot(plane.normal, ray.origin) - plane.distance;

        if (std::abs(slope) < 1e-9) {
            // Parallel to this face. Outside it means outside the brush.
            if (distance > 0.0) {
                return hit;
            }
            continue;
        }

        const f64 t = -distance / slope;
        if (slope < 0.0) {
            if (t > enter) {
                enter = t;
                entering_face = i;
            }
        } else {
            leave = std::min(leave, t);
        }

        if (enter > leave) {
            return hit;
        }
    }

    if (enter > leave || leave < 0.0) {
        return hit;
    }

    hit.valid = true;
    hit.solid = solid.id;
    hit.face = entering_face;
    // A ray starting inside the brush enters at zero rather than behind itself.
    hit.distance = std::max(enter, 0.0);
    hit.point = ray.origin + ray.direction * hit.distance;
    return hit;
}

Hit pick(const Document& document, const Ray& ray) {
    Hit best;
    best.distance = std::numeric_limits<f64>::max();

    for (const Document::SolidRef& ref : document.all_solids()) {
        const Hit hit = pick_solid(*ref.solid, ray);
        if (hit.valid && hit.distance < best.distance) {
            best = hit;
        }
    }

    if (!best.valid) {
        return Hit{};
    }
    return best;
}

std::optional<i32> pick_entity(const Document& document, const Ray& ray, f64 radius) {
    std::optional<i32> best;
    f64 best_distance = std::numeric_limits<f64>::max();

    for (const map::Entity& entity : document.map().entities) {
        if (entity.is_brush_entity()) {
            continue;  // Picked by its brushes, like any other geometry.
        }
        const std::optional<Vec3d> origin = entity.origin();
        if (!origin) {
            continue;
        }

        // Closest approach of the ray to the point.
        const Vec3d to_entity = *origin - ray.origin;
        const f64 along = dot(to_entity, ray.direction);
        if (along < 0.0) {
            continue;  // Behind us.
        }
        const Vec3d nearest = ray.origin + ray.direction * along;
        if (distance_squared(nearest, *origin) > radius * radius) {
            continue;
        }

        if (along < best_distance) {
            best_distance = along;
            best = entity.id;
        }
    }

    return best;
}

f64 snap_to_grid(f64 value, f64 grid) {
    if (grid <= 0.0) {
        return value;
    }
    return std::round(value / grid) * grid;
}

Vec3d snap_to_grid(const Vec3d& value, f64 grid) {
    return Vec3d(snap_to_grid(value.x, grid), snap_to_grid(value.y, grid),
                 snap_to_grid(value.z, grid));
}

}  // namespace kero::chisel
