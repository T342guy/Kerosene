// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "cleave/brush.hpp"

#include "core/jobs.hpp"
#include "core/log.hpp"
#include "math/units.hpp"

#include <algorithm>
#include <cmath>
#include <format>

namespace kero::cleave {
namespace {

KERO_LOG_CATEGORY(log, "cleave");

using Tol = math::Tolerance<f64>;

/// The canonical facing of a plane: the one whose normal points along the
/// positive direction of its first significant axis.
///
/// Needed so that a wall and the brush on the other side of it, which store the
/// plane facing opposite ways, still land in the same bucket and the same pair.
bool is_canonical(const Planed& plane) {
    for (usize axis = 0; axis < 3; ++axis) {
        if (std::abs(plane.normal[axis]) > Tol::kNormalLength) {
            return plane.normal[axis] > 0.0;
        }
    }
    return true;
}

}  // namespace

i64 PlaneSet::bucket_of(const Planed& plane) const {
    const Planed canonical = is_canonical(plane) ? plane : plane.flipped();
    return static_cast<i64>(std::llround(canonical.distance / Tol::kPlaneDistance));
}

std::optional<u32> PlaneSet::find(const Planed& plane) const {
    const i64 centre = bucket_of(plane);
    // A plane equal within tolerance can round into either neighbouring bucket,
    // so all three are searched. Checking only the exact bucket silently
    // duplicates planes, which is the failure this class exists to prevent.
    for (i64 offset = -1; offset <= 1; ++offset) {
        const auto found = buckets_.find(centre + offset);
        if (found == buckets_.end()) {
            continue;
        }
        for (u32 index : found->second) {
            if (planes_[index].equivalent(plane)) {
                return index;
            }
            if (planes_[index].equivalent(plane.flipped())) {
                return flip(index);
            }
        }
    }
    return std::nullopt;
}

u32 PlaneSet::add(const Planed& plane) {
    if (const std::optional<u32> existing = find(plane)) {
        return *existing;
    }

    // Added as a pair, canonical first, so index ^ 1 is always the opposite
    // facing and no lookup is needed to flip a plane.
    Planed canonical = is_canonical(plane) ? plane : plane.flipped();
    canonical.snap();

    const auto index = static_cast<u32>(planes_.size());
    planes_.push_back(canonical);
    planes_.push_back(canonical.flipped());
    buckets_[bucket_of(canonical)].push_back(index);

    return canonical.equivalent(plane) ? index : flip(index);
}

bool Brush::seals() const {
    return any(contents & bsp::Contents::Solid) && !detail;
}

bool Brush::contains(const Vec3d& point) const {
    for (const Side& side : sides) {
        // Uses the winding's own plane rather than the plane set, so this is
        // usable before the brush is registered.
        Planed plane;
        if (!side.winding.plane(plane)) {
            continue;
        }
        if (plane.distance_to(point) > Tol::kPointOnPlane) {
            return false;
        }
    }
    return true;
}

std::vector<u32> Brush::plane_indices() const {
    std::vector<u32> indices;
    indices.reserve(sides.size());
    for (const Side& side : sides) {
        if (!side.bevel) {
            indices.push_back(side.plane);
        }
    }
    return indices;
}

Aabbd World::bounds() const {
    Aabbd box;
    for (const Brush& brush : brushes) {
        box.add(brush.bounds);
    }
    return box;
}

namespace {

/// Builds one brush's faces by clipping each side's base winding by every other
/// side's half-space. A side left with nothing does not bound the solid --
/// which is ordinary, not an error: a designer who drags a face past its
/// opposite number produces one, and the right answer is to drop it.
bool build_brush_windings(Brush& brush, const PlaneSet& planes, std::string& problem) {
    Aabbd bounds;
    usize bounding_sides = 0;

    for (Side& side : brush.sides) {
        std::optional<Windingd> winding = Windingd::from_plane(planes[side.plane]);

        for (const Side& other : brush.sides) {
            if (&other == &side || !winding) {
                continue;
            }
            // Clip to the *back* of the other side: a brush is the intersection
            // of its sides' back half-spaces, because the normals face outward.
            winding = winding->clipped(planes[other.plane].flipped());
        }

        if (!winding) {
            continue;
        }
        winding->snap();
        if (!winding->valid()) {
            continue;
        }

        side.winding = std::move(*winding);
        ++bounding_sides;
        for (const Vec3d& point : side.winding.points()) {
            bounds.add(point);
        }
    }

    if (bounding_sides < 4) {
        problem = std::format(
            "the {} sides do not enclose a volume; {} of them bound anything. "
            "A face was probably dragged past the one opposite it",
            brush.sides.size(), bounding_sides);
        return false;
    }

    constexpr f64 kExtent = static_cast<f64>(units::kWorldExtent);
    for (usize axis = 0; axis < 3; ++axis) {
        if (bounds.mins[axis] < -kExtent || bounds.maxs[axis] > kExtent) {
            problem = std::format(
                "extends beyond the world, which is +/-{} ku on each axis",
                units::kWorldExtent);
            return false;
        }
    }

    brush.bounds = bounds;
    // Sides that bound nothing are dropped now, so nothing downstream has to
    // keep checking for an empty winding.
    std::erase_if(brush.sides, [](const Side& side) { return side.winding.empty(); });
    return true;
}

Side side_from_map(const map::Side& source, PlaneSet& planes) {
    Side side;
    side.plane = planes.add(source.plane);
    side.material = source.material;
    side.kind = bsp::classify_material(source.material);
    side.uaxis = source.uaxis;
    side.vaxis = source.vaxis;
    side.lightmap_scale = source.lightmap_scale;
    side.smoothing_groups = source.smoothing_groups;
    side.map_id = source.id;
    return side;
}

/// A brush's contents is the union of its sides' -- but a brush all of whose
/// sides are `tools/skip` is not a brush at all, and one mixing a tool material
/// with an ordinary one takes the tool's meaning, because that is what the
/// designer was reaching for.
bsp::Contents contents_of(const Brush& brush) {
    bsp::Contents contents = bsp::Contents::Empty;
    bool saw_tool = false;

    for (const Side& side : brush.sides) {
        if (bsp::is_tool_material(side.material)) {
            saw_tool = true;
            contents |= side.kind.contents;
        }
    }
    if (saw_tool) {
        return contents;
    }
    return bsp::Contents::Solid;
}

}  // namespace

World build_world(const map::Map& map, std::vector<BrushProblem>& problems) {
    World world;

    struct Source {
        const map::Entity* entity;
        usize entity_index;
        const map::Solid* solid;
    };

    std::vector<Source> sources;
    world.entities.push_back(&map.world);
    for (const map::Solid& solid : map.world.solids) {
        sources.push_back(Source{&map.world, 0, &solid});
    }
    for (const map::Entity& entity : map.entities) {
        const usize index = world.entities.size();
        world.entities.push_back(&entity);
        for (const map::Solid& solid : entity.solids) {
            sources.push_back(Source{&entity, index, &solid});
        }
    }

    // The plane set is shared and ordered, so registering planes stays serial;
    // it is a hash insert per side and nowhere near the cost of the clipping.
    // The clipping itself is where the time goes, and that is parallel below.
    std::vector<Brush> candidates;
    candidates.reserve(sources.size());

    for (const Source& source : sources) {
        Brush brush;
        brush.map_id = source.solid->id;
        brush.entity = source.entity_index;
        brush.detail = source.entity->classname == "func_detail";
        brush.sides.reserve(source.solid->sides.size());
        for (const map::Side& side : source.solid->sides) {
            brush.sides.push_back(side_from_map(side, world.planes));
        }
        brush.contents = contents_of(brush);
        if (brush.detail) {
            brush.contents |= bsp::Contents::Detail;
        }
        candidates.push_back(std::move(brush));
    }

    std::vector<std::string> failures(candidates.size());
    jobs().parallel_for(0, candidates.size(), [&](usize i) {
        if (!build_brush_windings(candidates[i], world.planes, failures[i])) {
            // Left non-empty to mark the brush as rejected.
            if (failures[i].empty()) {
                failures[i] = "could not be compiled";
            }
        }
    });

    for (usize i = 0; i < candidates.size(); ++i) {
        if (!failures[i].empty()) {
            problems.push_back(BrushProblem{candidates[i].map_id, candidates[i].entity,
                                            failures[i]});
            continue;
        }
        // A brush made entirely of tools/skip contributes nothing at all.
        if (candidates[i].contents == bsp::Contents::Empty &&
            !any(candidates[i].contents & bsp::Contents::Trigger)) {
            bool all_skip = true;
            for (const Side& side : candidates[i].sides) {
                if (!any(side.kind.flags & bsp::SurfaceFlags::Skip)) {
                    all_skip = false;
                }
            }
            if (all_skip) {
                continue;
            }
        }
        world.brushes.push_back(std::move(candidates[i]));
    }

    KERO_INFO(log, "{} brushes, {} planes, {} entities",
              world.brushes.size(), world.planes.size() / 2, world.entities.size());
    return world;
}

void add_bevel_planes(World& world) {
    // Serial: it is a handful of hash inserts per brush, and the plane set is
    // shared and order-dependent, so parallelising it would cost more in
    // synchronisation than the whole pass takes.
    for (Brush& brush : world.brushes) {
        // Only the axial planes are needed, and only where the brush does not
        // already have one facing that way. A box gains nothing; a wedge gains
        // the three or four it was missing.
        for (usize axis = 0; axis < 3; ++axis) {
            for (i32 direction = -1; direction <= 1; direction += 2) {
                Vec3d normal;
                normal[axis] = static_cast<f64>(direction);

                bool already_present = false;
                for (const Side& side : brush.sides) {
                    if (math::nearly_equal(dot(world.planes[side.plane].normal, normal),
                                           1.0, 1e-9)) {
                        already_present = true;
                        break;
                    }
                }
                if (already_present) {
                    continue;
                }

                const f64 distance =
                    direction > 0 ? brush.bounds.maxs[axis] : -brush.bounds.mins[axis];

                Side bevel;
                bevel.plane = world.planes.add(Planed(normal, distance));
                bevel.bevel = true;
                bevel.material = "tools/nodraw";
                bevel.kind = bsp::classify_material(bevel.material);
                brush.sides.push_back(std::move(bevel));
            }
        }
    }
}

namespace {

/// Removes from `fragments` everything the cutter `brush` hides.
///
/// Three things can happen to a face fragment, and telling them apart is the
/// whole of CSG:
///
///   * **Buried.** It lies inside the cutter's volume. Gone.
///   * **Back to back.** It lies on a cutter face pointing the *opposite* way --
///     two brushes pushed flush against each other. The surface is interior to
///     the union, so it is removed from *both* brushes, not kept once.
///   * **Shared.** It lies on a cutter face pointing the *same* way -- two
///     brushes whose tops are level, say. That surface really is exposed, so
///     exactly one of the two must keep it; `inclusive` says which, decided by
///     brush order so the answer never depends on iteration order.
///
/// The facing test is an index comparison, because the plane set stores planes
/// in facing pairs: same index means same facing, `index ^ 1` means opposite.
void subtract_brush(std::vector<Windingd>& fragments, u32 face_plane, const Brush& brush,
                    const PlaneSet& planes, bool inclusive) {
    // Does the cutter have a face on our plane at all?
    std::optional<u32> shared;
    for (const Side& side : brush.sides) {
        if (side.bevel) {
            continue;
        }
        if ((side.plane & ~1u) == (face_plane & ~1u)) {
            shared = side.plane;
            break;
        }
    }
    const bool same_facing = shared.has_value() && *shared == face_plane;

    std::vector<Windingd> kept;
    kept.reserve(fragments.size());

    for (const Windingd& fragment : fragments) {
        std::vector<Windingd> remaining{fragment};

        for (const Side& side : brush.sides) {
            if (side.bevel || remaining.empty()) {
                continue;
            }
            // A plane our own fragment lies on cannot cut it: every point is on
            // it, and splitting yields neither half.
            if (shared && side.plane == *shared) {
                continue;
            }

            std::vector<Windingd> still_inside;
            for (const Windingd& piece : remaining) {
                std::optional<Windingd> front;
                std::optional<Windingd> back;
                piece.split(planes[side.plane], front, back);
                if (front) {
                    kept.push_back(std::move(*front));  // In front: outside the cutter.
                }
                if (back) {
                    still_inside.push_back(std::move(*back));
                }
            }
            remaining = std::move(still_inside);
        }

        // What survived every cut is the part the cutter covers: buried in its
        // volume when we are not coplanar with it, and the shared surface when
        // we are. Only a same-facing surface can be kept, and only by the brush
        // that wins the tie-break.
        if (same_facing && !inclusive) {
            for (Windingd& piece : remaining) {
                kept.push_back(std::move(piece));
            }
        }
    }

    fragments = std::move(kept);
}

}  // namespace

void chop_brushes(World& world) {
    const usize count = world.brushes.size();

    // Nothing is mutated that another job reads: each job writes only its own
    // brush's `visible` lists and reads every brush's planes and bounds.
    jobs().parallel_for(0, count, [&](usize index) {
        Brush& brush = world.brushes[index];

        for (Side& side : brush.sides) {
            if (side.bevel) {
                continue;
            }
            // A face that is never drawn does not need its visible surface
            // computed, and skipping it saves the majority of the work on a
            // level with much nodraw in it.
            if (!side.kind.visible()) {
                continue;
            }
            side.visible.assign({side.winding});
        }

        for (usize other = 0; other < count; ++other) {
            if (other == index) {
                continue;
            }
            const Brush& cutter = world.brushes[other];

            // Only solid brushes hide surfaces. A trigger volume overlapping a
            // wall must not delete the wall.
            if (!any(cutter.contents & bsp::Contents::SolidMask)) {
                continue;
            }
            // A brush in one entity does not cut a brush in another: a door
            // sliding through a wall would otherwise erase the wall's face at
            // compile time, and the hole would still be there when it opened.
            if (cutter.entity != brush.entity) {
                continue;
            }
            if (!brush.bounds.intersects(cutter.bounds)) {
                continue;
            }

            // The tie-break for two brushes sharing a wall: the lower-indexed
            // one keeps the coplanar face. Deterministic, so two runs of the
            // compiler produce the same level.
            const bool inclusive = other < index;

            for (Side& side : brush.sides) {
                if (!side.visible.empty()) {
                    subtract_brush(side.visible, side.plane, cutter, world.planes,
                                   inclusive);
                }
            }
        }
    });

    usize faces = 0;
    for (const Brush& brush : world.brushes) {
        for (const Side& side : brush.sides) {
            faces += side.visible.size();
        }
    }
    KERO_INFO(log, "CSG left {} visible face fragments", faces);
}

}  // namespace kero::cleave
