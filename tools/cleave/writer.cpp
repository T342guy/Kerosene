// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "bsp/file.hpp"
#include "bsp/format.hpp"
#include "cleave/cleave.hpp"
#include "core/log.hpp"

#include <cstring>
#include <unordered_map>

namespace kero::cleave {
namespace {

KERO_LOG_CATEGORY(log, "cleave");

using Tol = math::Tolerance<f64>;

/// Accumulates the lumps and lays them out.
class Builder {
public:
    /// Deduplicates vertices, so a corner shared by three faces is stored once.
    ///
    /// Welding is not only about size. Two faces that agree a corner is at the
    /// same *index* cannot disagree about where it is, which is what removes
    /// the hairline cracks that appear along shared edges when each face
    /// carries its own copy of a coordinate.
    u32 add_vertex(const Vec3d& point) {
        const Key key{quantise(point.x), quantise(point.y), quantise(point.z)};
        const auto found = vertex_lookup_.find(key);
        if (found != vertex_lookup_.end()) {
            return found->second;
        }

        const auto index = static_cast<u32>(vertices_.size());
        vertices_.push_back(bsp::DiskVertex{{static_cast<f32>(point.x),
                                             static_cast<f32>(point.y),
                                             static_cast<f32>(point.z)}});
        vertex_lookup_.emplace(key, index);
        return index;
    }

    u32 add_material(const std::string& name) {
        const auto found = material_offsets_.find(name);
        if (found != material_offsets_.end()) {
            return found->second;
        }
        const auto offset = static_cast<u32>(material_text_.size());
        material_text_.insert(material_text_.end(), name.begin(), name.end());
        material_text_.push_back('\0');
        material_offsets_.emplace(name, offset);
        return offset;
    }

    /// TexInfo entries are deduplicated too: a level textured with one material
    /// on a hundred aligned walls needs one entry, not a hundred.
    u32 add_texinfo(const Side& side) {
        bsp::DiskTexInfo info{};
        const auto axis_to = [](const map::TextureAxis& axis, f32* out) {
            // The scale is folded into the axis, so the shader does one dot
            // product per coordinate rather than a divide as well.
            const f64 inverse = 1.0 / axis.scale;
            out[0] = static_cast<f32>(axis.axis.x * inverse);
            out[1] = static_cast<f32>(axis.axis.y * inverse);
            out[2] = static_cast<f32>(axis.axis.z * inverse);
            out[3] = static_cast<f32>(axis.shift);
        };
        axis_to(side.uaxis, info.u_axis);
        axis_to(side.vaxis, info.v_axis);
        info.flags = static_cast<u32>(side.kind.flags);
        info.material_offset = add_material(side.material);
        info.lightmap_scale = side.lightmap_scale;

        for (usize i = 0; i < texinfo_.size(); ++i) {
            if (std::memcmp(&texinfo_[i], &info, sizeof(info)) == 0) {
                return static_cast<u32>(i);
            }
        }
        texinfo_.push_back(info);
        return static_cast<u32>(texinfo_.size() - 1);
    }

    std::vector<bsp::DiskVertex> vertices_;
    std::vector<u32> face_vertices_;
    std::vector<bsp::DiskFace> faces_;
    std::vector<bsp::DiskNode> nodes_;
    std::vector<bsp::DiskLeaf> leaves_;
    std::vector<u32> leaf_faces_;
    std::vector<u32> leaf_brushes_;
    std::vector<bsp::DiskBrush> brushes_;
    std::vector<bsp::DiskBrushSide> brush_sides_;
    std::vector<bsp::DiskTexInfo> texinfo_;
    std::vector<char> material_text_;
    std::vector<bsp::DiskModel> models_;

private:
    using Key = std::array<i64, 3>;
    struct KeyHash {
        usize operator()(const Key& key) const {
            // A cheap mix; the vertex count is in the tens of thousands, so
            // collisions cost a memcmp and nothing more.
            usize hash = 1469598103934665603ull;
            for (i64 part : key) {
                hash = (hash ^ static_cast<usize>(part)) * 1099511628211ull;
            }
            return hash;
        }
    };

    /// Quantised to the welding tolerance, so two coordinates a rounding error
    /// apart hash to the same bucket and become one vertex.
    static i64 quantise(f64 value) {
        return static_cast<i64>(std::llround(value / Tol::kPointOnPlane));
    }

    std::unordered_map<Key, u32, KeyHash> vertex_lookup_;
    std::unordered_map<std::string, u32> material_offsets_;
};

/// Emits the tree depth-first, returning the child encoding: a node index when
/// non-negative, and -(leaf + 1) for a leaf.
i32 emit_node(Builder& builder, const World& world, Node& node) {
    if (node.leaf) {
        bsp::DiskLeaf leaf{};
        leaf.cluster = node.cluster;
        leaf.area = 0;
        leaf.contents = static_cast<u32>(node.contents);
        for (usize axis = 0; axis < 3; ++axis) {
            leaf.mins[axis] = static_cast<f32>(node.bounds.mins[axis]);
            leaf.maxs[axis] = static_cast<f32>(node.bounds.maxs[axis]);
        }

        leaf.first_leaf_face = static_cast<u32>(builder.leaf_faces_.size());
        for (const auto& [side, winding] : node.faces) {
            bsp::DiskFace face{};
            face.plane = side->plane & ~1u;
            face.flipped = (side->plane & 1u) != 0 ? 1u : 0u;
            face.texinfo = builder.add_texinfo(*side);
            face.surface_flags = static_cast<u32>(side->kind.flags);
            face.lightmap_offset = -1;   // Radiance fills these in.
            face.lightmap_size[0] = 0;
            face.lightmap_size[1] = 0;
            face.first_vertex = static_cast<u32>(builder.face_vertices_.size());
            for (const Vec3d& point : winding.points()) {
                builder.face_vertices_.push_back(builder.add_vertex(point));
            }
            face.vertex_count =
                static_cast<u32>(builder.face_vertices_.size() - face.first_vertex);

            builder.leaf_faces_.push_back(static_cast<u32>(builder.faces_.size()));
            builder.faces_.push_back(face);
        }
        leaf.leaf_face_count =
            static_cast<u32>(builder.leaf_faces_.size() - leaf.first_leaf_face);

        leaf.first_leaf_brush = static_cast<u32>(builder.leaf_brushes_.size());
        for (const Brush& brush : node.brushes) {
            bsp::DiskBrush disk{};
            disk.contents = static_cast<u32>(brush.contents);
            disk.first_side = static_cast<u32>(builder.brush_sides_.size());
            for (const Side& side : brush.sides) {
                bsp::DiskBrushSide disk_side{};
                disk_side.plane = side.plane;
                disk_side.texinfo = builder.add_texinfo(side);
                disk_side.bevel = side.bevel ? 1u : 0u;
                builder.brush_sides_.push_back(disk_side);
            }
            disk.side_count =
                static_cast<u32>(builder.brush_sides_.size() - disk.first_side);

            builder.leaf_brushes_.push_back(static_cast<u32>(builder.brushes_.size()));
            builder.brushes_.push_back(disk);
        }
        leaf.leaf_brush_count =
            static_cast<u32>(builder.leaf_brushes_.size() - leaf.first_leaf_brush);

        builder.leaves_.push_back(leaf);
        return -static_cast<i32>(builder.leaves_.size());
    }

    // The node is reserved before its children are emitted, so its index is
    // stable while they fill in theirs.
    const auto index = static_cast<u32>(builder.nodes_.size());
    builder.nodes_.push_back(bsp::DiskNode{});

    const i32 front = emit_node(builder, world, *node.children[0]);
    const i32 back = emit_node(builder, world, *node.children[1]);

    bsp::DiskNode& disk = builder.nodes_[index];
    disk.plane = node.plane & ~1u;
    disk.children[0] = front;
    disk.children[1] = back;
    for (usize axis = 0; axis < 3; ++axis) {
        disk.mins[axis] = static_cast<f32>(node.bounds.mins[axis]);
        disk.maxs[axis] = static_cast<f32>(node.bounds.maxs[axis]);
    }
    disk.first_face = 0;
    disk.face_count = 0;

    // A node's plane is stored canonically, so a child index that was derived
    // against the flipped facing would descend the wrong way. Swapped here
    // rather than at every descent in the runtime.
    if ((node.plane & 1u) != 0) {
        std::swap(disk.children[0], disk.children[1]);
    }
    return static_cast<i32>(index);
}

}  // namespace

std::vector<std::byte> serialise(const map::Map& map, const World& world, Tree& tree,
                                 Stats& stats) {
    Builder builder;

    // Model 0 is the world. Brush entities get their own submodels once they
    // are compiled separately; for now each is a box in the world's tree.
    bsp::DiskModel world_model{};
    const Aabbd bounds = world.bounds();
    for (usize axis = 0; axis < 3; ++axis) {
        world_model.mins[axis] = static_cast<f32>(bounds.mins[axis]);
        world_model.maxs[axis] = static_cast<f32>(bounds.maxs[axis]);
        world_model.origin[axis] = 0.0f;
    }
    world_model.head_node = 0;
    world_model.first_face = 0;

    if (tree.root() != nullptr) {
        (void)emit_node(builder, world, *tree.root());
    }
    world_model.face_count = static_cast<u32>(builder.faces_.size());
    builder.models_.push_back(world_model);

    // The entity lump is the map's own text, so a compiled level carries
    // everything an entity class might read -- including keys this build does
    // not implement. That is what lets the game code evolve without recompiling
    // every map.
    kv::Document entity_document;
    const kv::Document source = map::to_document(map);
    for (const kv::Block& block : source.blocks) {
        if (block.name == "entity") {
            kv::Block stripped = block;
            // The brushes are already compiled into the tree; carrying their
            // text as well would double the size of the lump for nothing.
            std::erase_if(stripped.children,
                          [](const kv::Block& child) { return child.name == "solid"; });
            entity_document.blocks.push_back(std::move(stripped));
        } else if (block.name == "world") {
            kv::Block stripped = block;
            std::erase_if(stripped.children,
                          [](const kv::Block& child) { return child.name == "solid"; });
            entity_document.blocks.push_back(std::move(stripped));
        }
    }
    const std::string entity_text = entity_document.to_string();

    std::vector<bsp::DiskPlane> planes;
    planes.reserve(world.planes.size() / 2);
    // Only the canonical facing of each pair is stored; a runtime that wants
    // the other one negates, which is cheaper than reading it.
    for (usize i = 0; i < world.planes.size(); i += 2) {
        const Planed& plane = world.planes[static_cast<u32>(i)];
        planes.push_back(bsp::DiskPlane{{static_cast<f32>(plane.normal.x),
                                         static_cast<f32>(plane.normal.y),
                                         static_cast<f32>(plane.normal.z)},
                                        static_cast<f32>(plane.distance),
                                        static_cast<u32>(plane.type)});
    }
    // Plane indices in the lumps above are pair indices; halved here so they
    // address the stored array.
    for (bsp::DiskFace& face : builder.faces_) {
        face.plane /= 2;
    }
    for (bsp::DiskNode& node : builder.nodes_) {
        node.plane /= 2;
    }
    for (bsp::DiskBrushSide& side : builder.brush_sides_) {
        // A brush side keeps its facing: the low bit says whether the outward
        // normal is the stored plane's or its negation.
        side.plane = ((side.plane / 2) << 1) | (side.plane & 1u);
    }

    bsp::File file;
    file.set_lump(bsp::LumpId::Entities,
                  std::vector<std::byte>(
                      reinterpret_cast<const std::byte*>(entity_text.data()),
                      reinterpret_cast<const std::byte*>(entity_text.data() +
                                                         entity_text.size())));
    file.set_lump(bsp::LumpId::Planes, planes);
    file.set_lump(bsp::LumpId::Vertices, builder.vertices_);
    file.set_lump(bsp::LumpId::FaceVertices, builder.face_vertices_);
    file.set_lump(bsp::LumpId::Faces, builder.faces_);
    file.set_lump(bsp::LumpId::Nodes, builder.nodes_);
    file.set_lump(bsp::LumpId::Leaves, builder.leaves_);
    file.set_lump(bsp::LumpId::LeafFaces, builder.leaf_faces_);
    file.set_lump(bsp::LumpId::LeafBrushes, builder.leaf_brushes_);
    file.set_lump(bsp::LumpId::Brushes, builder.brushes_);
    file.set_lump(bsp::LumpId::BrushSides, builder.brush_sides_);
    file.set_lump(bsp::LumpId::TexInfo, builder.texinfo_);
    file.set_lump(bsp::LumpId::Materials, builder.material_text_);
    file.set_lump(bsp::LumpId::Models, builder.models_);
    // Visibility and Lighting are left empty for Umbra and Radiance. Empty
    // means "everything is visible" and "fullbright", which is exactly what an
    // uncompiled level should look like -- you should be able to walk a level
    // thirty seconds after drawing it.

    const std::vector<std::byte> out = file.to_bytes();

    // Overwritten with what actually reached the file: welding vertices and
    // dropping the flipped half of each plane pair both change the counts, and
    // the file's numbers are the ones worth reporting.
    stats.planes = planes.size();
    stats.faces = builder.faces_.size();
    stats.vertices = builder.vertices_.size();

    KERO_INFO(log, "{} faces, {} vertices, {} planes, {} texinfos, {} bytes",
              builder.faces_.size(), builder.vertices_.size(), planes.size(),
              builder.texinfo_.size(), out.size());
    return out;
}

}  // namespace kero::cleave
