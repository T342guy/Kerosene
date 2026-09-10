// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "cleave/brush.hpp"

#include <memory>
#include <vector>

namespace kero::cleave {

/// A portal: the opening between two leaves.
///
/// Every boundary in the tree that is not solid is a portal, and the portal
/// graph is what everything after the tree is built runs on. Leak detection is
/// a flood fill across it, Umbra's visibility is a flow along it, and the
/// leaves it connects are the clusters the PVS is expressed in. Getting the
/// portals right matters more than getting the tree balanced.
struct Portal {
    u32 plane = 0;          ///< Index into the PlaneSet, facing `front`.
    Windingd winding;

    /// The two leaves the portal joins. `front` is the one the plane's normal
    /// points into.
    struct Node* front = nullptr;
    struct Node* back = nullptr;

    /// Set during the flood fill, so a leak can be traced back to the outside.
    bool flooded = false;
};

/// A node of the BSP tree. A leaf has no plane and no children.
struct Node {
    // --- Interior nodes ---
    u32 plane = 0;
    std::unique_ptr<Node> children[2];  ///< [0] front, [1] back.

    // --- Leaves ---
    bool leaf = false;
    bsp::Contents contents = bsp::Contents::Empty;
    i32 cluster = -1;

    /// Which entity's flood fill reached this leaf. -1 if none did, which for a
    /// non-solid leaf means it is outside the level.
    i32 occupant = -1;

    Node* parent = nullptr;
    Aabbd bounds;

    /// Portals touching this node. Interior nodes hold them while the tree is
    /// being built; after portalisation they belong to leaves.
    std::vector<Portal*> portals;

    /// The brushes still relevant here. Empty on an empty leaf.
    std::vector<Brush> brushes;

    /// Face fragments that ended up in this leaf.
    std::vector<std::pair<const Side*, Windingd>> faces;

    [[nodiscard]] bool solid() const { return any(contents & bsp::Contents::Solid); }
};

/// How the tree turned out, for reporting and for the tests.
struct TreeStats {
    usize nodes = 0;
    usize leaves = 0;
    usize solid_leaves = 0;
    usize empty_leaves = 0;
    usize portals = 0;
    usize splits = 0;      ///< Brushes cut by a split plane.
    usize max_depth = 0;
};

/// How aggressively to prefer a balanced tree over an uncut one.
///
/// These are the only two things a BSP split heuristic can trade against each
/// other, and the trade is genuinely a judgement call, so it is exposed rather
/// than buried. Splitting less produces fewer faces and compiles faster;
/// balancing more produces a shallower tree and faster traces. Axial planes
/// beat both, which is why they get a large bonus: level geometry is mostly
/// axial, and an axial split usually cuts nothing at all.
struct SplitPolicy {
    /// Weight on the number of brushes a candidate plane would cut in two.
    f64 split_cost = 5.0;
    /// Weight on how lopsided the front/back division would be.
    f64 balance_cost = 1.0;
    /// Subtracted from the score of an axial candidate.
    f64 axial_bonus = 10.0;
    /// Subtracted from the score of a hint plane, which a designer placed
    /// specifically to control where the tree splits.
    f64 hint_bonus = 1e9;
};

class Tree {
public:
    Tree() = default;
    ~Tree();

    Tree(const Tree&) = delete;
    Tree& operator=(const Tree&) = delete;

    /// Builds the tree from the world's structural brushes.
    ///
    /// Detail brushes are deliberately excluded. They are solid, but a
    /// cluttered room full of crates and pipes would otherwise carve the
    /// visibility structure into hundreds of leaves that see almost the same
    /// thing -- which is slower to compute, slower to trace, and no more
    /// accurate. They are added back to the leaves they land in afterwards.
    void build(const World& world, const SplitPolicy& policy = {});

    /// Builds the portal graph: one portal for every non-solid boundary.
    ///
    /// Takes the world by reference because the six planes boxing the tree in
    /// are registered as it runs.
    void portalize(World& world);

    /// Floods outward from each entity's position and marks the leaves it can
    /// reach. Returns false if the flood escaped into the void.
    [[nodiscard]] bool flood_entities(const World& world, const map::Map& map);

    /// Discards the leaves the flood never reached, so the void outside the
    /// level does not end up in the compiled file.
    void fill_outside();

    /// Puts detail brushes and visible face fragments into the leaves they
    /// belong to, and returns how many faces were placed. Run after
    /// flood_entities, so faces facing the void are dropped with it.
    usize place_detail_and_faces(const World& world);

    /// The portal path from a leaked entity out to the void, for `.kleak`.
    [[nodiscard]] const std::vector<Vec3d>& leak_path() const { return leak_path_; }
    [[nodiscard]] const std::string& leak_entity() const { return leak_entity_; }

    [[nodiscard]] Node* root() { return root_.get(); }
    [[nodiscard]] const Node* root() const { return root_.get(); }
    [[nodiscard]] const TreeStats& stats() const { return stats_; }

    /// The leaf containing `point`.
    [[nodiscard]] Node* leaf_at(const World& world, const Vec3d& point);
    [[nodiscard]] const Node* leaf_at(const World& world, const Vec3d& point) const;

    /// Every leaf, in depth-first order.
    [[nodiscard]] std::vector<Node*> leaves();

private:
    void build_recursive(Node& node, const World& world, const SplitPolicy& policy, usize depth);
    void make_head_portals(World& world);
    void portalize_recursive(Node& node, const World& world);
    void free_portals();

    std::unique_ptr<Node> root_;
    std::vector<std::unique_ptr<Portal>> portals_;
    TreeStats stats_;
    std::vector<Vec3d> leak_path_;
    std::string leak_entity_;
};

}  // namespace kero::cleave
