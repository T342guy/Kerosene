// SPDX-License-Identifier: LGPL-3.0-or-later
//! On-disk lump structures for `.voidbsp`.
//!
//! Every struct here is `#[repr(C)]` and padding-free so it can be
//! reinterpreted straight from a mapped byte slice by `bytemuck` -- loading a
//! map should be a read and a cast, not a parse. Field orders are chosen to
//! keep natural alignment without implicit padding; the `size_and_alignment`
//! test enforces that, because a silently-inserted pad byte would shift every
//! subsequent record and corrupt the map in ways that look like geometry bugs.

use bytemuck::{Pod, Zeroable};
use void_math::{Aabb, Plane, Vec3};

/// What a volume is made of. Traces test against a *mask* of these.
///
/// The values follow Source's, and the reason they are flags rather than an
/// enum is that one brush can be several things at once: water that also
/// blocks bullets, a grate that blocks players but not sight.
pub mod contents {
    pub const EMPTY: u32 = 0;
    /// Blocks everything. The default for a world brush.
    pub const SOLID: u32 = 1 << 0;
    /// Transparent but solid, like glass.
    pub const WINDOW: u32 = 1 << 1;
    /// Blocks movement and bullets but not sight, like a grate.
    pub const GRATE: u32 = 1 << 3;
    pub const SLIME: u32 = 1 << 4;
    pub const WATER: u32 = 1 << 5;
    /// Blocks light during the lighting compile even if not solid.
    pub const OPAQUE: u32 = 1 << 7;
    /// Belongs to a moving brush entity rather than the world.
    pub const MOVEABLE: u32 = 1 << 14;
    /// Blocks players only -- invisible walls.
    pub const PLAYER_CLIP: u32 = 1 << 16;
    /// Blocks AI only.
    pub const MONSTER_CLIP: u32 = 1 << 17;
    /// A trigger volume: not solid, but traces can find it.
    pub const TRIGGER: u32 = 1 << 18;
    /// A ladder volume: not solid, but a player standing in it climbs.
    pub const LADDER: u32 = 1 << 19;
    /// Detail geometry, which does not split the world tree. See
    /// [`crate::Bsp`] docs for why that matters.
    pub const DETAIL: u32 = 1 << 27;
    /// Any transparency at all; makes the renderer sort it late.
    pub const TRANSLUCENT: u32 = 1 << 28;

    /// Everything a walking player collides with.
    pub const MASK_PLAYER_SOLID: u32 = SOLID | MOVEABLE | PLAYER_CLIP | WINDOW | GRATE;
    /// Everything a bullet stops on.
    pub const MASK_SHOT: u32 = SOLID | MOVEABLE | WINDOW | GRATE;
    /// Everything that blocks line of sight.
    pub const MASK_OPAQUE: u32 = SOLID | MOVEABLE | OPAQUE;
    /// Solid world only.
    pub const MASK_SOLID: u32 = SOLID | MOVEABLE | WINDOW | GRATE;
    /// Water and slime.
    pub const MASK_WATER: u32 = WATER | SLIME;
    /// Everything that changes how a player moves without blocking them.
    ///
    /// Not solid, so it never appears in a movement trace -- these are found
    /// by asking what is at a point, which is why they need a mask of their
    /// own rather than riding along with [`MASK_PLAYER_SOLID`].
    pub const MASK_VOLUMES: u32 = WATER | SLIME | LADDER;
}

/// How a surface behaves for rendering and compiling.
pub mod surf {
    /// Emits light during the lighting compile.
    pub const LIGHT: u32 = 1 << 0;
    /// Draws the skybox and lets light through from the sun.
    pub const SKY: u32 = 1 << 2;
    /// Scrolling/warping surface, like water.
    pub const WARP: u32 = 1 << 3;
    pub const TRANS: u32 = 1 << 4;
    /// Never renders. Faces marked this are dropped before the face lump.
    pub const NODRAW: u32 = 1 << 7;
    /// A hint surface: forces a BSP split along its plane, then vanishes.
    pub const HINT: u32 = 1 << 8;
    /// Removed entirely at compile time -- the other faces of a hint brush.
    pub const SKIP: u32 = 1 << 9;
    /// Takes no lightmap.
    pub const NOLIGHT: u32 = 1 << 10;
    /// A trigger surface.
    pub const TRIGGER: u32 = 1 << 6;
}

/// A plane as stored in the file.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct BspPlane {
    pub normal: [f32; 3],
    pub dist: f32,
    /// Cached [`void_math::PlaneKind`], so traces can take an axial fast path
    /// without re-deriving it per query.
    pub kind: u32,
}

impl BspPlane {
    pub fn to_plane(&self) -> Plane {
        Plane::new(Vec3::from_array(self.normal), self.dist)
    }
    pub fn from_plane(p: &Plane) -> Self {
        BspPlane { normal: p.normal.to_array(), dist: p.dist, kind: p.kind() as u32 }
    }
}

/// An edge between two vertices, shared by the faces that meet along it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Edge {
    pub v: [u32; 2],
}

/// A renderable polygon.
///
/// Vertices are reached indirectly: `first_surfedge .. + num_surfedges` indexes
/// the surfedge lump, each entry of which is a *signed* index into the edge
/// lump -- negative meaning "walk this edge backwards". The indirection exists
/// so two faces meeting at an edge share one edge record and, crucially, one
/// pair of vertex positions, which keeps their seam watertight no matter how
/// the float arithmetic rounds.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Face {
    pub plane: u32,
    /// 0 if the face faces the same way as its plane, 1 if reversed.
    pub side: u8,
    /// Whether this face sits on a node (rather than hanging in a leaf).
    pub on_node: u8,
    pub _pad: [u8; 2],
    pub first_surfedge: u32,
    pub num_surfedges: u32,
    pub texinfo: u32,
    /// Index into a displacement lump, or -1. Reserved.
    pub dispinfo: i32,
    /// Byte offset into the lighting lump, or -1 for an unlit face.
    pub lightmap_offset: i32,
    /// Lightmap origin in luxel space.
    pub lightmap_mins: [i32; 2],
    /// Lightmap dimensions in luxels.
    pub lightmap_size: [u32; 2],
    /// Up to four light styles blended on this face; 255 means unused.
    /// Style 0 is the baked static light.
    pub light_styles: [u8; 4],
    pub area: f32,
}

/// An interior node of the BSP tree.
///
/// `children[0]` is the front side, `children[1]` the back. A non-negative
/// value indexes [`Bsp::nodes`](crate::Bsp::nodes); a negative value `c`
/// indexes leaf `-(c + 1)`, which is how the two arrays share one index space.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Node {
    pub plane: u32,
    pub children: [i32; 2],
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub first_face: u32,
    pub num_faces: u16,
    pub area: i16,
}

impl Node {
    pub fn bounds(&self) -> Aabb {
        Aabb::new(
            Vec3::new(self.mins[0] as f32, self.mins[1] as f32, self.mins[2] as f32),
            Vec3::new(self.maxs[0] as f32, self.maxs[1] as f32, self.maxs[2] as f32),
        )
    }
}

/// Index of a leaf child, encoded the way [`Node::children`] stores it.
#[inline]
pub const fn encode_leaf(leaf: usize) -> i32 { -((leaf as i32) + 1) }

/// Decode a [`Node::children`] entry into either a node or a leaf index.
#[inline]
pub const fn decode_child(child: i32) -> Child {
    if child < 0 { Child::Leaf((-child - 1) as usize) } else { Child::Node(child as usize) }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Child {
    Node(usize),
    Leaf(usize),
}

/// A convex region of space.
///
/// Leaves are what the tree actually partitions the world into. A leaf's
/// `cluster` is its visibility identity: several leaves can share a cluster,
/// and the PVS is computed between clusters rather than leaves to keep the
/// bit-vectors small. `-1` means the leaf is solid and has no visibility.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Leaf {
    pub contents: u32,
    pub first_leafface: u32,
    pub first_leafbrush: u32,
    pub num_leaffaces: u16,
    pub num_leafbrushes: u16,
    pub cluster: i16,
    pub area: i16,
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
}

impl Leaf {
    pub fn is_solid(&self) -> bool { self.contents & contents::SOLID != 0 }
    pub fn has_vis(&self) -> bool { self.cluster >= 0 }
    pub fn bounds(&self) -> Aabb {
        Aabb::new(
            Vec3::new(self.mins[0] as f32, self.mins[1] as f32, self.mins[2] as f32),
            Vec3::new(self.maxs[0] as f32, self.maxs[1] as f32, self.maxs[2] as f32),
        )
    }
}

/// A brush model: model 0 is the world, models 1.. are brush entities.
///
/// A `func_door` is model 1, say; the entity lump gives its `model` key as
/// `"*1"`, and the engine moves it by moving the model rather than by
/// re-splitting the world tree.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Model {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// Rotation origin, for doors that swing rather than slide.
    pub origin: [f32; 3],
    pub head_node: i32,
    pub first_face: u32,
    pub num_faces: u32,
}

impl Model {
    pub fn bounds(&self) -> Aabb {
        Aabb::new(Vec3::from_array(self.mins), Vec3::from_array(self.maxs))
    }
}

/// A convex collision volume, kept separately from render faces.
///
/// Rendering wants triangles; collision wants half-spaces. Keeping brushes in
/// the file means a trace can test a handful of planes instead of thousands of
/// triangles, and it is why box traces against brush geometry are cheap.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Brush {
    pub first_side: u32,
    pub num_sides: u32,
    pub contents: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct BrushSide {
    pub plane: u32,
    /// Index into texinfo, or -1 for a generated bevel plane.
    pub texinfo: i32,
    /// 1 if this plane was added to make box traces correct rather than being
    /// an authored face. See the compiler's bevel pass.
    pub bevel: u32,
}

/// How a face maps to a material and to its lightmap.
///
/// Two sets of axes rather than one: `texture_vecs` at the material's own
/// resolution, `lightmap_vecs` at the much coarser lightmap resolution. They
/// are separate because a face's lightmap is packed independently of its
/// texture and typically 64x lower resolution.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct TexInfo {
    /// `[u, v]`, each `[x, y, z, offset]`.
    pub texture_vecs: [[f32; 4]; 2],
    pub lightmap_vecs: [[f32; 4]; 2],
    pub flags: u32,
    pub texdata: u32,
}

impl TexInfo {
    /// Texture coordinate of a world point, in texels.
    pub fn texcoord(&self, p: Vec3) -> (f32, f32) {
        (
            p.x * self.texture_vecs[0][0] + p.y * self.texture_vecs[0][1]
                + p.z * self.texture_vecs[0][2] + self.texture_vecs[0][3],
            p.x * self.texture_vecs[1][0] + p.y * self.texture_vecs[1][1]
                + p.z * self.texture_vecs[1][2] + self.texture_vecs[1][3],
        )
    }

    /// Lightmap coordinate of a world point, in luxels.
    pub fn lightcoord(&self, p: Vec3) -> (f32, f32) {
        (
            p.x * self.lightmap_vecs[0][0] + p.y * self.lightmap_vecs[0][1]
                + p.z * self.lightmap_vecs[0][2] + self.lightmap_vecs[0][3],
            p.x * self.lightmap_vecs[1][0] + p.y * self.lightmap_vecs[1][1]
                + p.z * self.lightmap_vecs[1][2] + self.lightmap_vecs[1][3],
        )
    }

    pub fn is_nodraw(&self) -> bool { self.flags & surf::NODRAW != 0 }
    pub fn is_sky(&self) -> bool { self.flags & surf::SKY != 0 }
}

/// A material reference plus the data the lighting compile needs about it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct TexData {
    /// Average colour, used as the bounce colour in radiosity.
    pub reflectivity: [f32; 3],
    /// Offset into the texdata string lump.
    pub name_offset: u32,
    pub width: u32,
    pub height: u32,
    pub view_width: u32,
    pub view_height: u32,
}

/// One RGB lightmap sample, with an exponent.
///
/// The exponent is what lets a baked lightmap carry values well above 1.0 --
/// a bright sky or a lamp right against a wall -- in four bytes instead of
/// twelve. Same trick Source uses, and the reason its lighting can be tone
/// mapped at runtime rather than being clipped at bake time.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct ColorRgbExp32 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub exponent: i8,
}

impl ColorRgbExp32 {
    /// Decode to linear RGB.
    pub fn to_linear(self) -> Vec3 {
        let scale = (self.exponent as f32).exp2();
        Vec3::new(self.r as f32, self.g as f32, self.b as f32) * scale
    }

    /// Encode linear RGB, choosing the exponent that preserves the most
    /// precision without clipping the brightest channel.
    pub fn from_linear(c: Vec3) -> Self {
        let max = c.max_element().max(0.0);
        if max <= 0.0 { return ColorRgbExp32 { r: 0, g: 0, b: 0, exponent: 0 }; }
        // Pick e so that max/2^e lands just under 255.
        let mut exponent = (max / 255.0).log2().ceil() as i32;
        exponent = exponent.clamp(-128, 127);
        let scale = (exponent as f32).exp2();
        ColorRgbExp32 {
            r: (c.x / scale).round().clamp(0.0, 255.0) as u8,
            g: (c.y / scale).round().clamp(0.0, 255.0) as u8,
            b: (c.z / scale).round().clamp(0.0, 255.0) as u8,
            exponent: exponent as i8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Padding inside a lump record would shift every following record and
    /// corrupt the map, so the layouts are pinned here.
    #[test]
    fn size_and_alignment() {
        use std::mem::{align_of, size_of};
        assert_eq!(size_of::<BspPlane>(), 20);
        assert_eq!(size_of::<Edge>(), 8);
        assert_eq!(size_of::<Face>(), 52);
        assert_eq!(size_of::<Node>(), 32);
        assert_eq!(size_of::<Leaf>(), 32);
        assert_eq!(size_of::<Model>(), 48);
        assert_eq!(size_of::<Brush>(), 12);
        assert_eq!(size_of::<BrushSide>(), 12);
        assert_eq!(size_of::<TexInfo>(), 72);
        assert_eq!(size_of::<TexData>(), 32);
        assert_eq!(size_of::<ColorRgbExp32>(), 4);
        for a in [
            align_of::<BspPlane>(), align_of::<Face>(), align_of::<Node>(),
            align_of::<Leaf>(), align_of::<Model>(), align_of::<TexInfo>(),
        ] {
            assert_eq!(a, 4);
        }
    }

    #[test]
    fn child_encoding_round_trips() {
        for i in 0..64usize {
            assert_eq!(decode_child(encode_leaf(i)), Child::Leaf(i));
            assert_eq!(decode_child(i as i32), Child::Node(i));
        }
        // Leaf 0 must not collide with node 0.
        assert_ne!(encode_leaf(0), 0);
    }

    #[test]
    fn hdr_colour_survives_a_round_trip() {
        for v in [
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(255.0, 128.0, 0.0),
            Vec3::new(4000.0, 4000.0, 4000.0), // a bright sky, well over 1.0
            Vec3::ZERO,
        ] {
            let back = ColorRgbExp32::from_linear(v).to_linear();
            let err = (back - v).length() / v.length().max(1.0);
            assert!(err < 0.02, "{v:?} -> {back:?}");
        }
    }

    #[test]
    fn bright_values_do_not_clip() {
        // The whole point of the exponent: a value far above 255 must survive.
        let c = ColorRgbExp32::from_linear(Vec3::splat(10_000.0));
        assert!(c.to_linear().x > 9_000.0, "{:?}", c.to_linear());
    }

    #[test]
    fn texinfo_projects_like_the_map_format_does() {
        let mut ti = TexInfo::default();
        ti.texture_vecs[0] = [1.0, 0.0, 0.0, 8.0];
        ti.texture_vecs[1] = [0.0, -1.0, 0.0, 0.0];
        let (u, v) = ti.texcoord(Vec3::new(64.0, 32.0, 0.0));
        assert_eq!((u, v), (72.0, -32.0));
    }

    #[test]
    fn content_masks_compose_as_expected() {
        assert!(contents::MASK_PLAYER_SOLID & contents::PLAYER_CLIP != 0);
        assert!(contents::MASK_SHOT & contents::PLAYER_CLIP == 0, "bullets pass player clips");
        assert!(contents::MASK_OPAQUE & contents::GRATE == 0, "you can see through a grate");

        // A ladder changes how you move without ever stopping you, which is
        // the whole distinction MASK_VOLUMES exists to draw.
        let solid = contents::MASK_PLAYER_SOLID;
        let volumes = contents::MASK_VOLUMES;
        assert_eq!(solid & contents::LADDER, 0, "a ladder must never block movement");
        assert_ne!(volumes & contents::LADDER, 0, "but it must be findable at a point");
        assert_eq!(solid & volumes, 0, "the two masks are meant to be disjoint");
    }
}
