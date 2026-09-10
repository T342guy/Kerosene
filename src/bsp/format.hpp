// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "bsp/surface.hpp"
#include "core/types.hpp"

#include <array>

/// `.kbsp` -- the compiled map.
///
/// A lumped binary file: a header of (offset, length) pairs, then the lumps.
/// The shape is Quake's and Source's, and it is worth copying because of what it
/// makes possible rather than what it looks like. Lumps mean the three compile
/// stages can each add to a file the previous one wrote -- Cleave emits the
/// geometry, Umbra fills in visibility, Radiance fills in lighting -- without
/// any of them knowing the others' formats. That is what lets a stage be
/// re-run, replaced, or skipped: an unvised, unlit map still loads and plays,
/// it just draws everything and looks flat.
///
/// Everything here is fixed-width, little-endian, and free of padding by
/// construction, so a lump is a memory image of an array. The file is *not*
/// compatible with anything else that ends in .bsp, and the magic is checked to
/// make sure it is never mistaken for one.
///
/// Versioning is strict: a mismatch is an error with a "recompile the map"
/// message, never a best-effort read. Compiled output is regenerable from the
/// .kmap beside it, so there is nothing to be gained by tolerating a format
/// this build does not understand.
namespace kero::bsp {

/// "KBSP" as a little-endian u32.
inline constexpr u32 kMagic = 0x5053424Bu;
inline constexpr u32 kVersion = 1;

enum class LumpId : u32 {
    Entities,      ///< KeyValues text, exactly as .kmap wrote it.
    Planes,
    Vertices,
    FaceVertices,  ///< Index array; a face names a range in it.
    Faces,
    Nodes,
    Leaves,
    LeafFaces,     ///< Index array.
    LeafBrushes,   ///< Index array.
    Brushes,
    BrushSides,
    TexInfo,
    Materials,     ///< NUL-separated names; TexInfo indexes into it by offset.
    Visibility,    ///< Written by Umbra. Empty means "everything is visible".
    Lighting,      ///< Written by Radiance. Empty means "fullbright".
    Models,        ///< Submodels: the world is 0, each brush entity gets one.
    Count,
};

inline constexpr usize kLumpCount = static_cast<usize>(LumpId::Count);

struct Lump {
    u32 offset = 0;
    u32 length = 0;
};

struct Header {
    u32 magic = kMagic;
    u32 version = kVersion;
    std::array<Lump, kLumpCount> lumps{};
};

#pragma pack(push, 1)

struct DiskPlane {
    f32 normal[3];
    f32 distance;
    u32 type;  ///< math::PlaneType, so the runtime need not re-derive it.
};

struct DiskVertex {
    f32 position[3];
};

/// A face: a convex polygon, on a plane, with a material and a lightmap.
struct DiskFace {
    u32 plane;
    u32 flipped;        ///< 1 when the face faces opposite its plane's normal.
    u32 first_vertex;   ///< Into FaceVertices.
    u32 vertex_count;
    u32 texinfo;
    i32 lightmap_offset;   ///< Byte offset into Lighting; -1 when unlit.
    u16 lightmap_size[2];  ///< Luxels across and down.
    u32 surface_flags;     ///< SurfaceFlags, copied so the renderer need not look it up.
};

/// An interior node of the BSP tree.
///
/// A child is a node index when non-negative and the leaf `-(child + 1)` when
/// negative -- Quake's encoding, kept because it lets a node's two children be
/// one array with no discriminant and no indirection, which is what makes a
/// tree descent tight.
struct DiskNode {
    u32 plane;
    i32 children[2];
    f32 mins[3];
    f32 maxs[3];
    u32 first_face;
    u32 face_count;
};

struct DiskLeaf {
    i32 cluster;   ///< Index into the PVS; -1 for a leaf that sees nothing.
    i32 area;      ///< Reserved for area portals.
    u32 contents;  ///< Contents, the union of the leaf's brushes'.
    f32 mins[3];
    f32 maxs[3];
    u32 first_leaf_face;
    u32 leaf_face_count;
    u32 first_leaf_brush;
    u32 leaf_brush_count;
};

/// A brush, kept in the file alongside the faces derived from it.
///
/// The faces are what is drawn; the brush is what is collided with. Keeping
/// both is what lets the runtime trace an *arbitrary* box against the level:
/// the brush's planes, plus the bevel planes Cleave adds, are exactly the
/// Minkowski sum's supporting planes for any box size. Quake and Source
/// precompute a handful of fixed hull sizes instead and snap every entity to
/// the nearest one.
struct DiskBrush {
    u32 first_side;
    u32 side_count;
    u32 contents;
};

struct DiskBrushSide {
    u32 plane;
    u32 texinfo;
    /// 1 for a plane Cleave added to make box traces exact, rather than one the
    /// designer drew. Bevels are collision-only and never produce a face.
    u32 bevel;
};

/// How a material is projected onto a face.
struct DiskTexInfo {
    f32 u_axis[4];  ///< xyz direction, w shift. World position dotted with this.
    f32 v_axis[4];
    u32 flags;             ///< SurfaceFlags.
    u32 material_offset;   ///< Byte offset into the Materials lump.
    f32 lightmap_scale;    ///< Kerosene units per luxel.
};

/// A submodel. Model 0 is the world; each brush entity gets one, and its
/// entity's `model` key holds "*<index>".
struct DiskModel {
    f32 mins[3];
    f32 maxs[3];
    f32 origin[3];
    i32 head_node;
    u32 first_face;
    u32 face_count;
};

#pragma pack(pop)

// The file is a memory image of these, so a compiler that decided to pad one
// would change the format silently. Checked here rather than discovered later
// on a machine with different alignment rules.
static_assert(sizeof(DiskPlane) == 20);
static_assert(sizeof(DiskVertex) == 12);
static_assert(sizeof(DiskFace) == 32);
static_assert(sizeof(DiskNode) == 44);
static_assert(sizeof(DiskLeaf) == 52);
static_assert(sizeof(DiskBrush) == 12);
static_assert(sizeof(DiskBrushSide) == 12);
static_assert(sizeof(DiskTexInfo) == 44);
static_assert(sizeof(DiskModel) == 48);

/// The visibility lump's header. One offset per cluster, into the same lump.
///
/// Two vectors per cluster, not one: what a cluster can *see* and what can
/// *hear* it. The audible set is a looser flood that ignores line of sight, so
/// a sound around a corner still reaches the player. Source keeps this
/// distinction and it is the reason audio does not cut out when you step behind
/// a pillar.
struct DiskVisHeader {
    u32 cluster_count;
    // Followed by cluster_count pairs of u32 offsets, then the compressed
    // vectors themselves.
};

inline constexpr usize kVisPvs = 0;
inline constexpr usize kVisPas = 1;

}  // namespace kero::bsp
