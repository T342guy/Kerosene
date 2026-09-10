// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "map/map.hpp"

#include <charconv>
#include <format>

namespace kero::map {
namespace {

kv::Error error_at(const kv::Document& document, kv::Location where, std::string message) {
    return kv::Error{std::move(message), where, document.filename};
}

/// Numbers are written back the way a person would type them: no trailing
/// zeroes, no exponent for ordinary level coordinates. A map that round-trips
/// through the editor should produce a diff of what changed, not of how the
/// formatter feels about 128.
std::string number(f64 value) {
    std::string text = std::format("{:g}", value);
    return text;
}

std::string points_to_string(const std::array<Vec3d, 3>& points) {
    return std::format("({} {} {}) ({} {} {}) ({} {} {})",
                       number(points[0].x), number(points[0].y), number(points[0].z),
                       number(points[1].x), number(points[1].y), number(points[1].z),
                       number(points[2].x), number(points[2].y), number(points[2].z));
}

/// "(a b c) (d e f) (g h i)" -- the three points that define a brush side.
bool parse_plane_points(std::string_view text, std::array<Vec3d, 3>& out) {
    usize index = 0;
    usize position = 0;

    while (index < 9 && position < text.size()) {
        while (position < text.size() && (text[position] == ' ' || text[position] == '\t' ||
                                          text[position] == '(' || text[position] == ')')) {
            ++position;
        }
        const usize start = position;
        while (position < text.size() && text[position] != ' ' && text[position] != '\t' &&
               text[position] != '(' && text[position] != ')') {
            ++position;
        }
        if (start == position) {
            break;
        }

        f64 value{};
        const char* begin = text.data() + start;
        const char* end = text.data() + position;
        const auto [stop, code] = std::from_chars(begin, end, value);
        if (code != std::errc{} || stop != end) {
            return false;
        }
        out[index / 3][index % 3] = value;
        ++index;
    }

    return index == 9;
}

/// "[x y z shift] scale"
bool parse_texture_axis(std::string_view text, TextureAxis& out) {
    const usize open = text.find('[');
    const usize close = text.find(']');
    if (open == std::string_view::npos || close == std::string_view::npos || close < open) {
        return false;
    }

    const std::string_view inside = text.substr(open + 1, close - open - 1);
    f64 values[4]{};
    usize index = 0;
    usize position = 0;

    while (index < 4 && position < inside.size()) {
        while (position < inside.size() && (inside[position] == ' ' || inside[position] == '\t')) {
            ++position;
        }
        const usize start = position;
        while (position < inside.size() && inside[position] != ' ' && inside[position] != '\t') {
            ++position;
        }
        if (start == position) {
            break;
        }
        const char* begin = inside.data() + start;
        const char* end = inside.data() + position;
        const auto [stop, code] = std::from_chars(begin, end, values[index]);
        if (code != std::errc{} || stop != end) {
            return false;
        }
        ++index;
    }
    if (index != 4) {
        return false;
    }

    out.axis = Vec3d(values[0], values[1], values[2]);
    out.shift = values[3];

    std::string_view rest = text.substr(close + 1);
    while (!rest.empty() && (rest.front() == ' ' || rest.front() == '\t')) {
        rest.remove_prefix(1);
    }
    while (!rest.empty() && (rest.back() == ' ' || rest.back() == '\t')) {
        rest.remove_suffix(1);
    }
    if (rest.empty()) {
        return false;
    }

    f64 scale{};
    const auto [stop, code] = std::from_chars(rest.data(), rest.data() + rest.size(), scale);
    if (code != std::errc{} || stop != rest.data() + rest.size()) {
        return false;
    }
    // A zero scale divides by zero when the face is textured. Rejecting it here
    // means the compiler and the renderer never have to consider the case.
    if (scale == 0.0) {
        return false;
    }
    out.scale = scale;
    return true;
}

std::expected<Side, kv::Error> side_from_block(const kv::Document& document,
                                               const kv::Block& block) {
    Side side;
    side.id = block.get_i32("id").value_or(0);

    const std::string* plane_text = block.find("plane");
    if (plane_text == nullptr) {
        return std::unexpected(error_at(document, block.where, "side has no 'plane' key"));
    }
    if (!parse_plane_points(*plane_text, side.plane_points)) {
        return std::unexpected(error_at(
            document, block.location_of("plane").value_or(block.where),
            std::format("'plane' should be three points like "
                        "\"(0 0 0) (0 16 0) (16 0 0)\", found \"{}\"", *plane_text)));
    }
    if (!Planed::from_points(side.plane_points[0], side.plane_points[1],
                             side.plane_points[2], side.plane)) {
        // A real condition in hand-edited maps, and the only place it can be
        // reported against the side that caused it.
        return std::unexpected(error_at(
            document, block.location_of("plane").value_or(block.where),
            "the three points of 'plane' are collinear or coincident, so they "
            "do not define a plane"));
    }

    side.material = block.get("material", "dev/grid");

    for (const auto& [key, axis] : {std::pair{"uaxis", &side.uaxis},
                                    std::pair{"vaxis", &side.vaxis}}) {
        if (const std::string* text = block.find(key)) {
            if (!parse_texture_axis(*text, *axis)) {
                return std::unexpected(error_at(
                    document, block.location_of(key).value_or(block.where),
                    std::format("'{}' should look like \"[1 0 0 0] 0.25\", found \"{}\"",
                                key, *text)));
            }
        }
    }

    side.rotation = block.get_f64("rotation").value_or(0.0);
    side.lightmap_scale = block.get_f32("lightmapscale").value_or(8.0f);
    side.smoothing_groups = block.get_i32("smoothing_groups").value_or(0);
    return side;
}

std::expected<Solid, kv::Error> solid_from_block(const kv::Document& document,
                                                 const kv::Block& block) {
    Solid solid;
    solid.id = block.get_i32("id").value_or(0);

    for (const kv::Block* side_block : block.children_named("side")) {
        auto side = side_from_block(document, *side_block);
        if (!side) {
            return std::unexpected(side.error());
        }
        solid.sides.push_back(std::move(*side));
    }

    if (!solid.valid()) {
        // Fewer than four planes cannot bound a volume, whatever they are.
        return std::unexpected(error_at(
            document, block.where,
            std::format("solid has {} sides; a brush needs at least 4", solid.sides.size())));
    }
    return solid;
}

Connection connection_from_pair(const kv::Pair& pair) {
    // "target,input,parameter,delay,times_to_fire" -- Source's wire format,
    // kept because it is compact and because a comma cannot appear in any of
    // the fields that precede the numeric ones.
    Connection connection;
    connection.output = pair.key;

    std::vector<std::string_view> fields;
    std::string_view rest = pair.value;
    while (fields.size() < 5) {
        const usize comma = rest.find(',');
        if (comma == std::string_view::npos) {
            fields.push_back(rest);
            break;
        }
        fields.push_back(rest.substr(0, comma));
        rest = rest.substr(comma + 1);
    }

    auto field = [&fields](usize index) -> std::string_view {
        return index < fields.size() ? fields[index] : std::string_view{};
    };

    connection.target = std::string(field(0));
    connection.input = std::string(field(1));
    connection.parameter = std::string(field(2));

    const std::string_view delay = field(3);
    if (!delay.empty()) {
        f32 value{};
        if (std::from_chars(delay.data(), delay.data() + delay.size(), value).ec == std::errc{}) {
            connection.delay = value;
        }
    }
    const std::string_view times = field(4);
    if (!times.empty()) {
        i32 value{};
        if (std::from_chars(times.data(), times.data() + times.size(), value).ec == std::errc{}) {
            connection.times_to_fire = value;
        }
    }
    return connection;
}

std::expected<Entity, kv::Error> entity_from_block(const kv::Document& document,
                                                   const kv::Block& block) {
    Entity entity;
    entity.id = block.get_i32("id").value_or(0);
    entity.classname = block.get("classname");
    entity.properties = block.pairs;

    for (const kv::Block* solid_block : block.children_named("solid")) {
        auto solid = solid_from_block(document, *solid_block);
        if (!solid) {
            return std::unexpected(solid.error());
        }
        entity.solids.push_back(std::move(*solid));
    }

    if (const kv::Block* connections = block.first_child("connections")) {
        for (const kv::Pair& pair : connections->pairs) {
            entity.connections.push_back(connection_from_pair(pair));
        }
    }

    return entity;
}

kv::Block side_to_block(const Side& side) {
    kv::Block block;
    block.name = "side";
    block.set("id", std::to_string(side.id));
    block.set("plane", points_to_string(side.plane_points));
    block.set("material", side.material);
    block.set("uaxis", side.uaxis.to_string());
    block.set("vaxis", side.vaxis.to_string());
    block.set("rotation", number(side.rotation));
    block.set("lightmapscale", number(static_cast<f64>(side.lightmap_scale)));
    block.set("smoothing_groups", std::to_string(side.smoothing_groups));
    return block;
}

kv::Block solid_to_block(const Solid& solid) {
    kv::Block block;
    block.name = "solid";
    block.set("id", std::to_string(solid.id));
    for (const Side& side : solid.sides) {
        block.children.push_back(side_to_block(side));
    }
    return block;
}

kv::Block entity_to_block(const Entity& entity, std::string_view block_name) {
    kv::Block block;
    block.name = std::string(block_name);

    // The properties are written back as they were read, so a key this build
    // does not understand survives an edit-and-save rather than being dropped.
    block.pairs = entity.properties;
    if (block.find("id") == nullptr) {
        block.pairs.insert(block.pairs.begin(), kv::Pair{"id", std::to_string(entity.id), {}});
    }

    for (const Solid& solid : entity.solids) {
        block.children.push_back(solid_to_block(solid));
    }

    if (!entity.connections.empty()) {
        kv::Block connections;
        connections.name = "connections";
        for (const Connection& connection : entity.connections) {
            connections.pairs.push_back(kv::Pair{
                connection.output,
                std::format("{},{},{},{},{}", connection.target, connection.input,
                            connection.parameter, number(static_cast<f64>(connection.delay)),
                            connection.times_to_fire),
                {}});
        }
        block.children.push_back(std::move(connections));
    }

    return block;
}

}  // namespace

std::string TextureAxis::to_string() const {
    return std::format("[{} {} {} {}] {}", number(axis.x), number(axis.y), number(axis.z),
                       number(shift), number(scale));
}

std::string_view Entity::get(std::string_view key, std::string_view fallback) const {
    for (const kv::Pair& pair : properties) {
        if (pair.key == key) {
            return pair.value;
        }
    }
    return fallback;
}

std::optional<Vec3d> Entity::origin() const {
    const std::string_view text = get("origin");
    if (text.empty()) {
        return std::nullopt;
    }

    kv::Block block;
    block.set("origin", text);
    const std::optional<std::array<f64, 3>> values = block.get_vec3("origin");
    if (!values) {
        return std::nullopt;
    }
    return Vec3d((*values)[0], (*values)[1], (*values)[2]);
}

void Entity::set(std::string_view key, std::string_view value) {
    for (kv::Pair& pair : properties) {
        if (pair.key == key) {
            pair.value = std::string(value);
            return;
        }
    }
    properties.push_back(kv::Pair{std::string(key), std::string(value), {}});
    if (key == "classname") {
        classname = std::string(value);
    }
}

std::vector<Face> faces_of(const Solid& solid) {
    std::vector<Face> faces;
    faces.reserve(solid.sides.size());

    for (usize i = 0; i < solid.sides.size(); ++i) {
        // Start from a polygon certainly larger than the world and cut it down.
        // Building the final polygon directly would mean computing
        // intersections that may be near-parallel; clipping has no ordering to
        // get wrong and no such intersection to compute.
        std::optional<math::Windingd> winding =
            math::Windingd::from_plane(solid.sides[i].plane);

        for (usize j = 0; j < solid.sides.size() && winding; ++j) {
            if (j != i) {
                // Clipped to the *back* of the other side: a brush is the
                // intersection of its sides' back half-spaces, because the
                // normals face outward.
                winding = winding->clipped(solid.sides[j].plane.flipped());
            }
        }

        if (winding && winding->valid()) {
            faces.push_back(Face{i, std::move(*winding)});
        }
    }

    return faces;
}

math::Aabbd bounds_of(const Solid& solid) {
    math::Aabbd box;
    for (const Face& face : faces_of(solid)) {
        for (const Vec3d& point : face.winding.points()) {
            box.add(point);
        }
    }
    return box;
}

bool encloses_volume(const Solid& solid) {
    // Four bounding faces is the minimum for a closed volume -- a tetrahedron.
    // Fewer sides may be *present* and still bound nothing, which is why this
    // counts what survived rather than what was authored.
    return faces_of(solid).size() >= 4;
}

usize Map::brush_count() const {
    usize count = world.solids.size();
    for (const Entity& entity : entities) {
        count += entity.solids.size();
    }
    return count;
}

usize Map::side_count() const {
    usize count = 0;
    for (const Solid& solid : world.solids) {
        count += solid.sides.size();
    }
    for (const Entity& entity : entities) {
        for (const Solid& solid : entity.solids) {
            count += solid.sides.size();
        }
    }
    return count;
}

std::vector<const Entity*> Map::by_classname(std::string_view classname) const {
    std::vector<const Entity*> found;
    if (world.classname == classname) {
        found.push_back(&world);
    }
    for (const Entity& entity : entities) {
        if (entity.classname == classname) {
            found.push_back(&entity);
        }
    }
    return found;
}

std::expected<Map, kv::Error> from_document(const kv::Document& document) {
    Map map;

    if (const kv::Block* version = document.first("versioninfo")) {
        map.editor_version = version->get_i32("editorversion").value_or(100);
        map.format_version = version->get_i32("formatversion").value_or(1);
    }

    const kv::Block* world_block = document.first("world");
    if (world_block == nullptr) {
        return std::unexpected(kv::Error{
            "no 'world' block; every map has one, even an empty one",
            kv::Location{}, document.filename});
    }

    auto world = entity_from_block(document, *world_block);
    if (!world) {
        return std::unexpected(world.error());
    }
    map.world = std::move(*world);
    if (map.world.classname.empty()) {
        map.world.classname = "worldspawn";
    }

    for (const kv::Block* entity_block : document.all("entity")) {
        auto entity = entity_from_block(document, *entity_block);
        if (!entity) {
            return std::unexpected(entity.error());
        }
        if (entity->classname.empty()) {
            return std::unexpected(error_at(document, entity_block->where,
                                            "entity has no 'classname'"));
        }
        map.entities.push_back(std::move(*entity));
    }

    return map;
}

kv::Document to_document(const Map& map) {
    kv::Document document;

    kv::Block version;
    version.name = "versioninfo";
    version.set("editorversion", std::to_string(map.editor_version));
    version.set("formatversion", std::to_string(map.format_version));
    document.blocks.push_back(std::move(version));

    document.blocks.push_back(entity_to_block(map.world, "world"));
    for (const Entity& entity : map.entities) {
        document.blocks.push_back(entity_to_block(entity, "entity"));
    }

    return document;
}

std::expected<Map, kv::Error> load(const std::string& path) {
    auto document = kv::parse_file(path);
    if (!document) {
        return std::unexpected(document.error());
    }
    return from_document(*document);
}

std::expected<void, kv::Error> save(const Map& map, const std::string& path) {
    return kv::write_file(to_document(map), path);
}

}  // namespace kero::map
