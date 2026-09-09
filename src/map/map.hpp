// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "kv/keyvalues.hpp"
#include "math/plane.hpp"
#include "math/vec.hpp"

#include <array>
#include <expected>
#include <optional>
#include <string>
#include <vector>

/// `.kmap` -- the editable map format.
///
/// A map is a list of brushes and entities, and a brush is a list of *planes*
/// rather than a list of vertices. That is the single most consequential
/// decision in a Source-like engine and it is worth being explicit about why:
///
///   * A brush is the intersection of its sides' half-spaces, so convexity is
///     structural. There is no way to author a concave brush and therefore no
///     validation pass that has to reject one.
///   * Planes are what CSG operates on. Storing vertices would mean deriving
///     the planes on load, and deriving them from vertices that have already
///     been rounded is how a wall ends up very slightly not being a wall.
///   * A side's texture alignment is expressed against its plane, so dragging a
///     face moves the plane and the texture follows without being recomputed.
///
/// The stored form of a plane is three points, in the order that winds them
/// counter-clockwise when seen from the front. Three points survive a text
/// round-trip exactly, which a normal-and-distance pair does not.
///
/// Coordinates are in kerosene units: one ku is two inches, Z is up. See
/// math/units.hpp.
namespace kero::map {

using math::Planed;
using math::Vec3d;

/// A texture axis: a direction in world space, a shift along it, and a scale.
///
/// Written as `[x y z shift] scale`. Two of these -- U and V -- project world
/// coordinates onto the material, which is what makes a texture stay put on a
/// wall when the wall is resized, rather than stretching with it.
struct TextureAxis {
    Vec3d axis{1, 0, 0};
    f64 shift = 0.0;
    f64 scale = 0.25;

    [[nodiscard]] std::string to_string() const;
};

/// One face of a brush.
struct Side {
    i32 id = 0;

    /// The three points, as authored. Kept alongside the derived plane so that
    /// saving reproduces the file rather than a re-derivation of it.
    std::array<Vec3d, 3> plane_points{};
    Planed plane;

    std::string material = "dev/grid";
    TextureAxis uaxis;
    TextureAxis vaxis;
    f64 rotation = 0.0;

    /// Luxels per this many kerosene units. Larger is coarser and cheaper.
    f32 lightmap_scale = 8.0f;

    /// Faces sharing a smoothing group get their vertex normals averaged, so a
    /// curve built from flat brushes lights as a curve.
    i32 smoothing_groups = 0;
};

/// A convex solid: the intersection of its sides' back half-spaces.
struct Solid {
    i32 id = 0;
    std::vector<Side> sides;

    [[nodiscard]] bool valid() const { return sides.size() >= 4; }
};

/// One wire in the entity I/O graph: "when this happens to me, do that to them".
///
/// Source's output/input system, kept as-is because it is the best idea in the
/// engine. A button's OnPressed fires a door's Open after a delay. There is no
/// scripting language, and it composes far further than it has any right to.
struct Connection {
    std::string output;      ///< The event on this entity, e.g. "OnPressed".
    std::string target;      ///< The targetname of the entity to act on.
    std::string input;       ///< The input to fire on it, e.g. "Open".
    std::string parameter;   ///< Passed to the input, if it takes one.
    f32 delay = 0.0f;        ///< Seconds to wait first.

    /// -1 means unlimited. A one-shot trigger is `1`.
    i32 times_to_fire = -1;
};

/// A point entity, a brush entity, or the world.
struct Entity {
    i32 id = 0;
    std::string classname;

    /// Every key as written, classname and origin included. Entity classes read
    /// their own fields out of here, so an unknown key on an entity this build
    /// does not implement survives a load-and-save instead of being deleted.
    std::vector<kv::Pair> properties;

    /// Present for brush entities (func_door, trigger_multiple) and for the
    /// world. Empty for point entities.
    std::vector<Solid> solids;

    std::vector<Connection> connections;

    [[nodiscard]] bool is_brush_entity() const { return !solids.empty(); }
    [[nodiscard]] std::string_view get(std::string_view key, std::string_view fallback = {}) const;
    [[nodiscard]] std::optional<Vec3d> origin() const;
    void set(std::string_view key, std::string_view value);
};

struct Map {
    i32 editor_version = 100;
    i32 format_version = 1;

    /// The worldspawn entity, holding every brush that is not part of a brush
    /// entity. Its `solids` are the level.
    Entity world;

    /// Everything else, in file order.
    std::vector<Entity> entities;

    [[nodiscard]] usize brush_count() const;
    [[nodiscard]] usize side_count() const;

    /// Every entity with this classname, world included when it matches.
    [[nodiscard]] std::vector<const Entity*> by_classname(std::string_view classname) const;
};

/// Builds a Map from an already-parsed document.
[[nodiscard]] std::expected<Map, kv::Error> from_document(const kv::Document& document);

/// Serialises a Map back to a document, ready to write.
[[nodiscard]] kv::Document to_document(const Map& map);

[[nodiscard]] std::expected<Map, kv::Error> load(const std::string& path);
[[nodiscard]] std::expected<void, kv::Error> save(const Map& map, const std::string& path);

}  // namespace kero::map
