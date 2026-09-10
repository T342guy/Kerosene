// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "cleave/tree.hpp"

#include "core/log.hpp"

#include <algorithm>
#include <deque>
#include <limits>
#include <format>

namespace kero::cleave {
namespace {

KERO_LOG_CATEGORY(log, "cleave");

using Tol = math::Tolerance<f64>;

/// Beyond which the tree stops splitting whatever the brush list says.
///
/// Not a quality knob: a pathological map -- a thousand coplanar brushes offset
/// by a rounding error -- can otherwise recurse until the stack runs out, and a
/// compiler must produce a diagnostic rather than a crash.
constexpr usize kMaxDepth = 256;

/// Restricts `brush` to one side of `plane`, returning nothing if the
/// intersection is empty.
///
/// Adding a side rather than clipping the windings is what keeps this exact: a
/// brush stays the intersection of half-spaces, so restricting it to another
/// half-space is one more plane, and the windings are re-derived from planes
/// that were never rounded.
std::optional<Brush> clip_brush(const Brush& brush, const PlaneSet& planes,
                                u32 plane_index, bool keep_front) {
    const Planed& plane = planes[plane_index];
    // The bounds settle most cases without any geometry at all.
    const Vec3d nearest = brush.bounds.support(keep_front ? -plane.normal : plane.normal);
    const Vec3d furthest = brush.bounds.support(keep_front ? plane.normal : -plane.normal);
    const f64 near_distance = plane.distance_to(nearest);
    const f64 far_distance = plane.distance_to(furthest);

    if (keep_front) {
        if (near_distance >= -Tol::kPointOnPlane) {
            return brush;  // Entirely in front.
        }
        if (far_distance <= Tol::kPointOnPlane) {
            return std::nullopt;  // Entirely behind.
        }
    } else {
        if (near_distance <= Tol::kPointOnPlane) {
            return brush;
        }
        if (far_distance >= -Tol::kPointOnPlane) {
            return std::nullopt;
        }
    }

    Brush result = brush;
    Side cut;
    // To keep the front half, the new bounding side faces backwards -- a
    // brush's sides point out of it. No plane is added: the set stores facing
    // pairs, so the opposite facing is already there under index ^ 1.
    cut.plane = keep_front ? PlaneSet::flip(plane_index) : plane_index;
    cut.bevel = true;
    cut.material = "tools/nodraw";
    cut.kind = bsp::classify_material(cut.material);
    result.sides.push_back(std::move(cut));

    // Re-derive the windings, and with them the bounds. A brush whose remaining
    // sides no longer enclose anything is dropped.
    Aabbd bounds;
    usize bounding = 0;
    for (Side& side : result.sides) {
        std::optional<Windingd> winding = Windingd::from_plane(planes[side.plane]);
        for (const Side& other : result.sides) {
            if (&other == &side || !winding) {
                continue;
            }
            winding = winding->clipped(planes[other.plane].flipped());
        }
        if (!winding || !winding->valid()) {
            side.winding = Windingd{};
            continue;
        }
        side.winding = std::move(*winding);
        ++bounding;
        for (const Vec3d& point : side.winding.points()) {
            bounds.add(point);
        }
    }

    if (bounding < 4) {
        return std::nullopt;
    }
    result.bounds = bounds;
    std::erase_if(result.sides, [](const Side& side) { return side.winding.empty(); });
    return result;
}

struct Candidate {
    u32 plane = 0;
    f64 score = 0.0;
    bool valid = false;
};

/// Picks the plane to split on, or reports that this node is a leaf.
///
/// The candidates are the planes of the brushes present that have not already
/// been split on above. Scoring trades cuts against balance, with a large bonus
/// for axial planes -- level geometry is mostly axial, and an axial split
/// usually cuts nothing -- and an overwhelming one for hint planes, which exist
/// precisely so a designer can say where the tree should divide.
Candidate select_split_plane(const std::vector<Brush>& brushes, const Aabbd& region,
                             const PlaneSet& planes, const SplitPolicy& policy) {
    Candidate best;
    best.score = std::numeric_limits<f64>::max();

    for (const Brush& brush : brushes) {
        for (const Side& side : brush.sides) {
            if (side.used_as_splitter || side.bevel) {
                continue;
            }

            const Planed& plane = planes[side.plane];

            // Skip a plane the node's own region does not straddle: splitting
            // on it would put everything in one child and recurse forever.
            //
            // The test is against the region, not against the brushes. Getting
            // that wrong is subtle and costly: with a single brush left in a
            // room -- a step, a pillar -- every one of its own faces has the
            // brush entirely on one side, so a brush-count test rejects them
            // all, the node becomes a leaf with the brush still in it, and the
            // whole room is reported solid. The region test accepts exactly the
            // planes that divide the space in front of us.
            const f64 region_near = plane.distance_to(region.support(-plane.normal));
            const f64 region_far = plane.distance_to(region.support(plane.normal));
            if (region_near >= -Tol::kPointOnPlane || region_far <= Tol::kPointOnPlane) {
                continue;
            }

            usize front = 0;
            usize back = 0;
            usize splits = 0;

            for (const Brush& other : brushes) {
                // Bounds alone answer this for the overwhelming majority of
                // pairs, which is what keeps the scan affordable.
                const f64 near_distance =
                    plane.distance_to(other.bounds.support(-plane.normal));
                const f64 far_distance =
                    plane.distance_to(other.bounds.support(plane.normal));

                if (near_distance >= -Tol::kPointOnPlane) {
                    ++front;
                } else if (far_distance <= Tol::kPointOnPlane) {
                    ++back;
                } else {
                    ++splits;
                }
            }

            const auto imbalance = static_cast<f64>(
                front > back ? front - back : back - front);
            f64 score = static_cast<f64>(splits) * policy.split_cost +
                        imbalance * policy.balance_cost;

            if (is_axial(plane.type)) {
                score -= policy.axial_bonus;
            }
            if (any(side.kind.flags & bsp::SurfaceFlags::Hint)) {
                score -= policy.hint_bonus;
            }

            if (score < best.score) {
                best.score = score;
                best.plane = side.plane;
                best.valid = true;
            }
        }
    }

    return best;
}

/// Marks every side lying on `plane` as used, in both facings, so the subtree
/// below never considers it again.
void mark_used(std::vector<Brush>& brushes, u32 plane) {
    const u32 canonical = plane & ~1u;
    for (Brush& brush : brushes) {
        for (Side& side : brush.sides) {
            if ((side.plane & ~1u) == canonical) {
                side.used_as_splitter = true;
            }
        }
    }
}

}  // namespace

Tree::~Tree() = default;

void Tree::build(const World& world, const SplitPolicy& policy) {
    root_ = std::make_unique<Node>();
    stats_ = TreeStats{};

    // Structural brushes only. Detail is added back to the leaves afterwards.
    for (const Brush& brush : world.brushes) {
        if (brush.detail) {
            continue;
        }
        if (!any(brush.contents & bsp::Contents::SolidMask)) {
            continue;
        }
        root_->brushes.push_back(brush);
    }

    root_->bounds = world.bounds();
    // Room around the level, so the head node's leaves genuinely enclose it and
    // the outside is a place the flood fill can reach.
    root_->bounds.expand(128.0);

    build_recursive(*root_, world, policy, 0);

    KERO_INFO(log, "tree: {} nodes, {} leaves ({} solid), {} brush splits, depth {}",
              stats_.nodes, stats_.leaves, stats_.solid_leaves, stats_.splits,
              stats_.max_depth);
}

void Tree::build_recursive(Node& node, const World& world, const SplitPolicy& policy,
                           usize depth) {
    stats_.max_depth = std::max(stats_.max_depth, depth);

    Candidate split = depth < kMaxDepth
                          ? select_split_plane(node.brushes, node.bounds, world.planes, policy)
                          : Candidate{};

    if (!split.valid) {
        // A leaf. Every brush still here has had all its sides used as
        // splitters above, so this region is inside all of them.
        node.leaf = true;
        ++stats_.leaves;
        for (const Brush& brush : node.brushes) {
            node.contents |= brush.contents;
        }
        if (node.solid()) {
            ++stats_.solid_leaves;
        } else {
            ++stats_.empty_leaves;
        }
        if (depth >= kMaxDepth) {
            KERO_WARN(log,
                      "stopped splitting at depth {}; the map probably has "
                      "near-coplanar brushes stacked on each other",
                      depth);
        }
        return;
    }

    node.plane = split.plane;
    ++stats_.nodes;

    const Planed& plane = world.planes[split.plane];
    mark_used(node.brushes, split.plane);

    auto front = std::make_unique<Node>();
    auto back = std::make_unique<Node>();
    front->parent = &node;
    back->parent = &node;
    front->bounds = node.bounds;
    back->bounds = node.bounds;

    // Tightening the child bounds on an axial split keeps the leaf boxes
    // meaningful, which the renderer's culling and the trace both benefit from.
    if (is_axial(plane.type)) {
        const usize axis = static_cast<usize>(plane.type);
        const f64 value = plane.normal[axis] > 0.0 ? plane.distance : -plane.distance;
        if (plane.normal[axis] > 0.0) {
            front->bounds.mins[axis] = std::max(front->bounds.mins[axis], value);
            back->bounds.maxs[axis] = std::min(back->bounds.maxs[axis], value);
        } else {
            front->bounds.maxs[axis] = std::min(front->bounds.maxs[axis], value);
            back->bounds.mins[axis] = std::max(back->bounds.mins[axis], value);
        }
    }

    for (const Brush& brush : node.brushes) {
        std::optional<Brush> in_front = clip_brush(brush, world.planes, split.plane, true);
        std::optional<Brush> behind = clip_brush(brush, world.planes, split.plane, false);
        if (in_front && behind) {
            ++stats_.splits;
        }
        if (in_front) {
            front->brushes.push_back(std::move(*in_front));
        }
        if (behind) {
            back->brushes.push_back(std::move(*behind));
        }
    }

    // Released before recursing: a deep tree holding every ancestor's brush
    // list is the difference between a compile that fits in memory and one
    // that does not.
    node.brushes.clear();
    node.brushes.shrink_to_fit();

    build_recursive(*front, world, policy, depth + 1);
    build_recursive(*back, world, policy, depth + 1);

    node.children[0] = std::move(front);
    node.children[1] = std::move(back);
}

std::vector<Node*> Tree::leaves() {
    std::vector<Node*> found;
    if (!root_) {
        return found;
    }

    std::vector<Node*> stack{root_.get()};
    while (!stack.empty()) {
        Node* node = stack.back();
        stack.pop_back();
        if (node->leaf) {
            found.push_back(node);
            continue;
        }
        stack.push_back(node->children[0].get());
        stack.push_back(node->children[1].get());
    }
    return found;
}

Node* Tree::leaf_at(const World& world, const Vec3d& point) {
    Node* node = root_.get();
    while (node != nullptr && !node->leaf) {
        const f64 distance = world.planes[node->plane].distance_to(point);
        node = node->children[distance >= 0.0 ? 0 : 1].get();
    }
    return node;
}

const Node* Tree::leaf_at(const World& world, const Vec3d& point) const {
    // Delegating keeps one descent, so the const and non-const answers can
    // never diverge.
    return const_cast<Tree*>(this)->leaf_at(world, point);
}

}  // namespace kero::cleave
