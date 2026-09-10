// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "chisel/tools.hpp"

#include <algorithm>
#include <cmath>

namespace kero::chisel {

using math::Planed;

namespace {

/// Which world axis a normal most nearly points along, and which way.
struct Facing {
    usize axis = 2;
    bool positive = true;
};

[[nodiscard]] Facing facing_of(const Vec3d& normal) {
    Facing facing;
    f64 best = -1.0;
    for (usize axis = 0; axis < 3; ++axis) {
        const f64 magnitude = std::abs(normal[axis]);
        if (magnitude > best) {
            best = magnitude;
            facing.axis = axis;
            facing.positive = normal[axis] >= 0.0;
        }
    }
    return facing;
}

/// The tangent pair for a box face, ordered so that `cross(u, v)` is the
/// outward normal -- which is the winding `Planed::from_points` expects.
[[nodiscard]] std::pair<usize, usize> tangents_for(usize axis, bool positive) {
    const usize a = (axis + 1) % 3;
    const usize b = (axis + 2) % 3;
    return positive ? std::pair{a, b} : std::pair{b, a};
}

[[nodiscard]] Vec3d unit(usize axis) {
    Vec3d v;
    v[axis] = 1.0;
    return v;
}

}  // namespace

TextureAxes default_texture_axes(const Vec3d& normal, f64 scale) {
    const Facing facing = facing_of(normal);

    TextureAxes axes;
    switch (facing.axis) {
        case 0:  // Facing east or west: run U north and V down.
            axes.u.axis = Vec3d(0.0, 1.0, 0.0);
            axes.v.axis = Vec3d(0.0, 0.0, -1.0);
            break;
        case 1:  // Facing north or south: U east, V down.
            axes.u.axis = Vec3d(1.0, 0.0, 0.0);
            axes.v.axis = Vec3d(0.0, 0.0, -1.0);
            break;
        default:  // Floor or ceiling: U east, V south, so north is up.
            axes.u.axis = Vec3d(1.0, 0.0, 0.0);
            axes.v.axis = Vec3d(0.0, -1.0, 0.0);
            break;
    }
    axes.u.scale = scale;
    axes.v.scale = scale;
    return axes;
}

map::Solid make_box(const math::Aabbd& bounds, Document& document,
                    std::string_view material) {
    map::Solid solid;
    if (bounds.empty()) {
        return solid;
    }
    const Vec3d size = bounds.maxs - bounds.mins;
    if (size.x <= 0.0 || size.y <= 0.0 || size.z <= 0.0) {
        return solid;  // A click that never became a drag.
    }

    solid.id = document.allocate_id();
    solid.sides.reserve(6);

    for (usize axis = 0; axis < 3; ++axis) {
        for (const bool positive : {true, false}) {
            const auto [u, v] = tangents_for(axis, positive);

            // The corner the two tangents run away from, on the face's plane.
            Vec3d base = bounds.mins;
            base[axis] = positive ? bounds.maxs[axis] : bounds.mins[axis];

            const Vec3d along_u = unit(u) * size[u];
            const Vec3d along_v = unit(v) * size[v];

            map::Side side;
            side.id = document.allocate_id();
            // `from_points` takes the normal from cross(a - b, c - b), so `b`
            // is the corner and the other two run along the tangents.
            side.plane_points = {base + along_u, base, base + along_v};
            if (!Planed::from_points(side.plane_points[0], side.plane_points[1],
                                     side.plane_points[2], side.plane)) {
                return map::Solid{};  // Degenerate; the caller drops it.
            }
            side.material = std::string(material);
            const TextureAxes axes = default_texture_axes(side.plane.normal);
            side.uaxis = axes.u;
            side.vaxis = axes.v;
            solid.sides.push_back(std::move(side));
        }
    }

    return solid;
}

map::Solid translate(const map::Solid& solid, const Vec3d& delta) {
    map::Solid moved = solid;
    for (map::Side& side : moved.sides) {
        for (Vec3d& point : side.plane_points) {
            point += delta;
        }
        // Recomputed rather than adjusted: the plane's distance would move by
        // dot(normal, delta), but rederiving keeps this the one place the
        // convention lives.
        (void)Planed::from_points(side.plane_points[0], side.plane_points[1],
                                  side.plane_points[2], side.plane);
    }
    return moved;
}

map::Solid resize(const map::Solid& solid, const math::Aabbd& from,
                  const math::Aabbd& to) {
    if (from.empty() || to.empty()) {
        return solid;
    }

    Vec3d scale(1.0, 1.0, 1.0);
    for (usize axis = 0; axis < 3; ++axis) {
        const f64 extent = from.maxs[axis] - from.mins[axis];
        if (std::abs(extent) > 1e-9) {
            scale[axis] = (to.maxs[axis] - to.mins[axis]) / extent;
        }
    }

    map::Solid scaled = solid;
    for (map::Side& side : scaled.sides) {
        for (Vec3d& point : side.plane_points) {
            for (usize axis = 0; axis < 3; ++axis) {
                point[axis] = to.mins[axis] + (point[axis] - from.mins[axis]) * scale[axis];
            }
        }
        (void)Planed::from_points(side.plane_points[0], side.plane_points[1],
                                  side.plane_points[2], side.plane);
    }
    return scaled;
}

math::Aabbd selection_bounds(const Document& document) {
    math::Aabbd bounds;
    for (const i32 id : document.selection().solids) {
        if (const map::Solid* solid = document.find_solid(id)) {
            bounds.add(map::bounds_of(*solid));
        }
    }
    for (const i32 id : document.selection().entities) {
        const map::Entity* entity = document.find_entity(id);
        if (entity == nullptr) {
            continue;
        }
        for (const map::Solid& solid : entity->solids) {
            bounds.add(map::bounds_of(solid));
        }
        if (const std::optional<Vec3d> origin = entity->origin()) {
            bounds.add(*origin);
        }
    }
    return bounds;
}

std::array<Grip, 8> grips() {
    return {Grip{-1, -1}, Grip{0, -1}, Grip{1, -1}, Grip{-1, 0},
            Grip{1, 0},   Grip{-1, 1}, Grip{0, 1},  Grip{1, 1}};
}

namespace {

/// The world axis a screen axis runs along, and its sign. Orthographic views
/// are axis-aligned by construction, so this is exact rather than a projection.
[[nodiscard]] Facing screen_axis(const Vec3& direction) {
    return facing_of(Vec3d(static_cast<f64>(direction.x), static_cast<f64>(direction.y),
                           static_cast<f64>(direction.z)));
}

/// Moves one side of a box, keeping mins below maxs.
void move_side(math::Aabbd& bounds, usize axis, i32 pull, f64 amount) {
    if (pull == 0) {
        return;
    }
    if (pull > 0) {
        bounds.maxs[axis] = std::max(bounds.maxs[axis] + amount, bounds.mins[axis]);
    } else {
        bounds.mins[axis] = std::min(bounds.mins[axis] + amount, bounds.maxs[axis]);
    }
}

}  // namespace

Vec3d grip_position(const math::Aabbd& bounds, const ViewAxes& axes, Grip grip) {
    Vec3d position = bounds.centre();
    const Facing across = screen_axis(axes.right);
    const Facing down = screen_axis(axes.up);

    const auto pick = [&bounds](usize axis, i32 pull, bool positive) {
        // The view's axis may run the opposite way to the world's, so which end
        // of the box a grip sits on depends on both.
        const bool high = (pull > 0) == positive;
        return pull == 0 ? bounds.centre()[axis]
                         : (high ? bounds.maxs[axis] : bounds.mins[axis]);
    };

    position[across.axis] = pick(across.axis, grip.across, across.positive);
    position[down.axis] = pick(down.axis, grip.down, down.positive);
    return position;
}

math::Aabbd drag_grip(const math::Aabbd& bounds, const ViewAxes& axes, Grip grip,
                      const Vec3d& delta) {
    if (bounds.empty()) {
        return bounds;
    }

    math::Aabbd result = bounds;
    const Facing across = screen_axis(axes.right);
    const Facing down = screen_axis(axes.up);

    const auto apply = [&result, &delta](const Facing& facing, i32 pull) {
        move_side(result, facing.axis, facing.positive ? pull : -pull, delta[facing.axis]);
    };

    apply(across, grip.across);
    apply(down, grip.down);
    return result;
}

math::Aabbd block_bounds(const ViewAxes& axes, const Vec3d& from, const Vec3d& to,
                        f64 depth, f64 grid) {
    const Facing forward = screen_axis(axes.forward);

    math::Aabbd bounds;
    bounds.add(from);
    bounds.add(to);

    // The two corners lie on the same plane, so the depth axis has no extent
    // yet. Give it one, centred where the view was looking.
    const f64 middle = from[forward.axis];
    const f64 half = std::max(depth, grid > 0.0 ? grid : 1.0) * 0.5;
    bounds.mins[forward.axis] = middle - half;
    bounds.maxs[forward.axis] = middle + half;

    return snap_bounds(bounds, grid);
}

math::Aabbd snap_bounds(const math::Aabbd& bounds, f64 grid) {
    if (grid <= 0.0 || bounds.empty()) {
        return bounds;
    }
    math::Aabbd snapped;
    snapped.mins = snap_to_grid(bounds.mins, grid);
    snapped.maxs = snap_to_grid(bounds.maxs, grid);
    for (usize axis = 0; axis < 3; ++axis) {
        // A box snapped to nothing is a box the compiler will reject, so a
        // side that collapsed keeps one grid square.
        if (snapped.maxs[axis] <= snapped.mins[axis]) {
            snapped.maxs[axis] = snapped.mins[axis] + grid;
        }
    }
    return snapped;
}

}  // namespace kero::chisel
