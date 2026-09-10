// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "cleave/tree.hpp"

#include "core/log.hpp"

#include <algorithm>
#include <deque>
#include <format>
#include <unordered_map>

namespace kero::cleave {
namespace {

KERO_LOG_CATEGORY(log, "cleave");

using Tol = math::Tolerance<f64>;

/// Removes a portal from a node's list.
void detach(Node& node, Portal* portal) {
    std::erase(node.portals, portal);
}

}  // namespace

void Tree::free_portals() {
    portals_.clear();
}

void Tree::make_head_portals(World& world) {
    // Six portals boxing the whole tree in, well outside the level. They give
    // the root's leaves a boundary, which is what makes "outside the level" a
    // place the flood fill can start from rather than a special case.
    Aabbd box = world.bounds();
    box.expand(256.0);

    for (usize axis = 0; axis < 3; ++axis) {
        for (i32 direction = -1; direction <= 1; direction += 2) {
            Vec3d normal;
            // Facing inward, so the tree is in front of every head portal.
            normal[axis] = static_cast<f64>(-direction);
            const f64 distance =
                direction > 0 ? -box.maxs[axis] : box.mins[axis];

            auto portal = std::make_unique<Portal>();
            portal->plane = world.planes.add(Planed(normal, distance));
            // The default extent: large enough to cover the box, and inside
            // the range Winding::valid() will accept. Asking for more produces
            // a winding whose own corners are outside the world, which is
            // rejected as degenerate -- correctly, and silently.
            portal->winding = Windingd::from_plane(world.planes[portal->plane]);
            portal->front = root_.get();
            portal->back = nullptr;  // The void.
            root_->portals.push_back(portal.get());
            portals_.push_back(std::move(portal));
        }
    }
}

void Tree::portalize(World& world) {
    free_portals();
    if (!root_) {
        return;
    }

    make_head_portals(world);
    portalize_recursive(*root_, world);

    stats_.portals = 0;
    for (const std::unique_ptr<Portal>& portal : portals_) {
        if (portal->front != nullptr && portal->back != nullptr &&
            !portal->front->solid() && !portal->back->solid()) {
            ++stats_.portals;
        }
    }
    KERO_INFO(log, "{} portals between open leaves", stats_.portals);
}

void Tree::portalize_recursive(Node& node, const World& world) {
    if (node.leaf) {
        return;
    }

    Node& front = *node.children[0];
    Node& back = *node.children[1];
    const Planed& plane = world.planes[node.plane];

    // The new portal on this node's plane is the plane itself, cut down by
    // every portal already bounding this node. Deriving it from the node's own
    // boundary rather than from the world box is what makes it exactly the
    // opening between the two children and nothing more.
    {
        std::optional<Windingd> winding = Windingd::from_plane(plane);

        for (Portal* bounding : node.portals) {
            if (!winding) {
                break;
            }
            const Planed& clip = world.planes[bounding->plane];
            // A portal's plane faces into the node when the node is its front.
            const Planed inward = bounding->front == &node ? clip : clip.flipped();
            winding = winding->clipped(inward);
        }

        if (winding) {
            winding->snap();
            if (winding->valid()) {
                auto portal = std::make_unique<Portal>();
                portal->plane = node.plane;
                portal->winding = std::move(*winding);
                portal->front = &front;
                portal->back = &back;
                front.portals.push_back(portal.get());
                back.portals.push_back(portal.get());
                portals_.push_back(std::move(portal));
            }
        }
    }

    // Every portal that bounded this node is now split between the children.
    // Taken by value: the loop reassigns the portals' endpoints, which mutates
    // the lists it would otherwise be iterating.
    const std::vector<Portal*> inherited = node.portals;
    node.portals.clear();

    for (Portal* portal : inherited) {
        Node* other = portal->front == &node ? portal->back : portal->front;
        const bool node_is_front = portal->front == &node;

        std::optional<Windingd> in_front;
        std::optional<Windingd> behind;
        portal->winding.split(plane, in_front, behind);

        if (in_front && behind) {
            // Straddles: the portal becomes two, one per child.
            auto extra = std::make_unique<Portal>();
            extra->plane = portal->plane;
            extra->winding = std::move(*behind);
            if (node_is_front) {
                extra->front = &back;
                extra->back = other;
            } else {
                extra->front = other;
                extra->back = &back;
            }
            back.portals.push_back(extra.get());
            if (other != nullptr) {
                other->portals.push_back(extra.get());
            }

            portal->winding = std::move(*in_front);
            if (node_is_front) {
                portal->front = &front;
            } else {
                portal->back = &front;
            }
            front.portals.push_back(portal);
            portals_.push_back(std::move(extra));
            continue;
        }

        // Wholly on one side: it just moves down to that child.
        Node* target = in_front ? &front : &back;
        if (in_front) {
            portal->winding = std::move(*in_front);
        } else if (behind) {
            portal->winding = std::move(*behind);
        } else {
            // Coplanar with this node's plane, and so degenerate here.
            if (other != nullptr) {
                detach(*other, portal);
            }
            continue;
        }

        if (node_is_front) {
            portal->front = target;
        } else {
            portal->back = target;
        }
        target->portals.push_back(portal);
    }

    portalize_recursive(front, world);
    portalize_recursive(back, world);
}

bool Tree::flood_entities(const World& world, const map::Map& map) {
    leak_path_.clear();
    leak_entity_.clear();

    if (!root_) {
        return false;
    }

    struct Start {
        Vec3d position;
        std::string description;
        i32 index = 0;
    };

    std::vector<Start> starts;
    for (usize i = 0; i < map.entities.size(); ++i) {
        const map::Entity& entity = map.entities[i];
        // Brush entities are inside the level by construction; it is the point
        // entities that tell us where the playable space is.
        if (entity.is_brush_entity()) {
            continue;
        }
        const std::optional<Vec3d> origin = entity.origin();
        if (!origin) {
            continue;
        }
        std::string name = entity.classname;
        if (const std::string_view targetname = entity.get("targetname"); !targetname.empty()) {
            name += std::format(" \"{}\"", targetname);
        }
        starts.push_back(Start{*origin, std::move(name), static_cast<i32>(i)});
    }

    if (starts.empty()) {
        KERO_WARN(log,
                  "no point entities, so there is nothing to say which side of "
                  "the geometry is inside; skipping the leak check");
        return true;
    }

    // Breadth-first, so the path recorded for a leak is the shortest one out --
    // which is the one a designer can actually follow to the hole.
    std::deque<Node*> queue;
    std::unordered_map<Node*, Portal*> came_from;

    for (const Start& start : starts) {
        Node* leaf = leaf_at(world, start.position);
        if (leaf == nullptr) {
            continue;
        }
        if (leaf->solid()) {
            KERO_WARN(log, "{} at {} is inside solid geometry", start.description,
                      start.position);
            continue;
        }
        if (leaf->occupant < 0) {
            leaf->occupant = start.index;
            queue.push_back(leaf);
            came_from[leaf] = nullptr;
        }
    }

    auto describe = [&starts](i32 index) -> std::string {
        for (const Start& start : starts) {
            if (start.index == index) {
                return start.description;
            }
        }
        return "an entity";
    };

    while (!queue.empty()) {
        Node* leaf = queue.front();
        queue.pop_front();

        for (Portal* portal : leaf->portals) {
            Node* other = portal->front == leaf ? portal->back : portal->front;

            if (other == nullptr) {
                // A head portal: the flood has reached the boundary of the
                // world, so the level is not sealed.
                leak_entity_ = describe(leaf->occupant);

                // Walk the parent chain back to the entity, recording where
                // each portal was crossed. This is what turns "leaked" into a
                // line a designer can load and follow.
                leak_path_.push_back(portal->winding.centre());
                for (Node* step = leaf; step != nullptr;) {
                    leak_path_.push_back(step->bounds.centre());
                    Portal* crossing = came_from[step];
                    if (crossing == nullptr) {
                        break;
                    }
                    leak_path_.push_back(crossing->winding.centre());
                    step = crossing->front == step ? crossing->back : crossing->front;
                }
                std::ranges::reverse(leak_path_);
                return false;
            }

            if (other->solid() || other->occupant >= 0) {
                continue;
            }
            other->occupant = leaf->occupant;
            came_from[other] = portal;
            queue.push_back(other);
        }
    }

    usize reached = 0;
    for (Node* leaf : leaves()) {
        if (leaf->occupant >= 0) {
            ++reached;
        }
    }
    KERO_INFO(log, "flood reached {} of {} open leaves", reached, stats_.empty_leaves);
    return true;
}

void Tree::fill_outside() {
    // Everything the flood never reached is outside the level. Marking it solid
    // is what removes the void from the compiled file: the renderer never
    // considers it, the PVS never allocates a cluster for it, and a trace that
    // somehow gets out there stops immediately instead of falling forever.
    usize filled = 0;
    for (Node* leaf : leaves()) {
        if (!leaf->solid() && leaf->occupant < 0) {
            leaf->contents |= bsp::Contents::Solid;
            leaf->portals.clear();
            ++filled;
        }
    }

    // Portals onto a leaf that has just become solid are no longer openings.
    for (const std::unique_ptr<Portal>& portal : portals_) {
        if (portal->front == nullptr || portal->back == nullptr) {
            continue;
        }
        if (portal->front->solid() || portal->back->solid()) {
            if (portal->front != nullptr) {
                detach(*portal->front, portal.get());
            }
            if (portal->back != nullptr) {
                detach(*portal->back, portal.get());
            }
        }
    }

    stats_.portals = 0;
    for (const std::unique_ptr<Portal>& portal : portals_) {
        if (portal->front != nullptr && portal->back != nullptr &&
            !portal->front->solid() && !portal->back->solid()) {
            ++stats_.portals;
        }
    }

    if (filled > 0) {
        KERO_INFO(log, "filled {} leaves outside the level; {} portals remain",
                  filled, stats_.portals);
    }
}

usize Tree::place_detail_and_faces(const World& world) {
    // Detail brushes were kept out of the tree so they could not carve the
    // visibility structure. They still have to be collided with, so each one
    // is filed under every open leaf it touches.
    for (const Brush& brush : world.brushes) {
        if (!brush.detail) {
            continue;
        }
        for (Node* leaf : leaves()) {
            if (leaf->solid() || !leaf->bounds.intersects(brush.bounds)) {
                continue;
            }
            leaf->brushes.push_back(brush);
        }
    }

    // Every visible face fragment is pushed down the tree and split at each
    // node, so a face spanning several leaves is filed under each of them
    // rather than under whichever one happens to contain its centre.
    usize placed = 0;
    for (const Brush& brush : world.brushes) {
        for (const Side& side : brush.sides) {
            for (const Windingd& fragment : side.visible) {
                struct Item {
                    Node* node;
                    Windingd winding;
                };
                std::vector<Item> stack;
                stack.push_back(Item{root_.get(), fragment});

                while (!stack.empty()) {
                    Item item = std::move(stack.back());
                    stack.pop_back();

                    if (item.node->leaf) {
                        // A face on the boundary of a solid leaf belongs to the
                        // open leaf on the other side, and that is where the
                        // descent below has already sent it.
                        if (!item.node->solid()) {
                            item.node->faces.emplace_back(&side, std::move(item.winding));
                            ++placed;
                        }
                        continue;
                    }

                    const Planed& plane = world.planes[item.node->plane];
                    std::optional<Windingd> in_front;
                    std::optional<Windingd> behind;
                    item.winding.split(plane, in_front, behind);

                    if (!in_front && !behind) {
                        // Coplanar with this node. The face faces one way, and
                        // that is the side the open space is on.
                        Planed face_plane;
                        if (!item.winding.plane(face_plane)) {
                            continue;
                        }
                        const bool same_direction = dot(face_plane.normal, plane.normal) > 0.0;
                        stack.push_back(Item{item.node->children[same_direction ? 0 : 1].get(),
                                             std::move(item.winding)});
                        continue;
                    }
                    if (in_front) {
                        stack.push_back(Item{item.node->children[0].get(),
                                             std::move(*in_front)});
                    }
                    if (behind) {
                        stack.push_back(Item{item.node->children[1].get(),
                                             std::move(*behind)});
                    }
                }
            }
        }
    }

    KERO_INFO(log, "placed {} faces into leaves", placed);
    return placed;
}

}  // namespace kero::cleave
