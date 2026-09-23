// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! `.keromdl` -- compiled models, the MDL analogue.
//!
//! Brush geometry handles walls and floors; models handle everything a brush
//! cannot describe -- crates, machinery, characters. Forge compiles source
//! meshes into this, and the engine loads it without further processing.
//!
//! One model holds several *meshes*, each with its own material, because a
//! single object routinely uses more than one: a crate is wood on five sides
//! and metal on the corners. Splitting by material at compile time means the
//! renderer can issue one draw call per mesh instead of sorting at runtime.
//!
//! Vertices carry four bone influences whether or not the model is skinned.
//! The cost is 8 bytes a vertex on static props; the benefit is one vertex
//! layout, one shader path, and no branch in the hot loop.
//!
//! A skinned model carries its animations too (version 2): each clip is its
//! bones' local transforms resampled at a fixed rate, so playing one is two
//! lookups and an interpolation per bone, with no curve evaluation and no
//! knowledge of the tool it was authored in. A bone's rest transform is its
//! bind pose: the pose the mesh was skinned in, which is what the renderer
//! inverts to move the mesh with the bones.

use bytemuck::{Pod, Zeroable};
use kerosene_math::{Aabb, Vec3};
use thiserror::Error;

const MAGIC: [u8; 4] = *b"KRMD";
/// Version 2 added animations. Version 1 files -- every static prop written
/// before them -- still load, as models with none.
const VERSION: u32 = 2;
const HEADER_SIZE: usize = 64;

/// Bones per vertex. Four is the usual compromise: enough for a shoulder or a
/// hip to deform smoothly, few enough to keep the vertex small.
pub const MAX_BONE_INFLUENCES: usize = 4;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("not a .keromdl file (bad magic)")]
    BadMagic,
    #[error("version {found}; this build reads version {expected}")]
    BadVersion { found: u32, expected: u32 },
    #[error("truncated: needs {needed} bytes, has {available}")]
    Truncated { needed: usize, available: usize },
    #[error("mesh {mesh} indexes vertices past the end of the vertex array")]
    BadIndex { mesh: usize },
    #[error("bone {bone} has parent {parent}, which is not before it")]
    BadBoneOrder { bone: usize, parent: i32 },
    #[error(
        "animation {animation} has {found} keys; {frames} frames of {bones} bones need {expected}"
    )]
    BadAnimation {
        animation: usize,
        found: usize,
        frames: usize,
        bones: usize,
        expected: usize,
    },
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub bone_indices: [u8; 4],
    /// Normalised to 255 across the four influences.
    pub bone_weights: [u8; 4],
}

impl Vertex {
    pub fn rigid(position: Vec3, normal: Vec3, uv: [f32; 2]) -> Vertex {
        Vertex {
            position: position.to_array(),
            normal: normal.to_array(),
            uv,
            bone_indices: [0; 4],
            // Fully bound to bone 0, which for a static model is the identity.
            bone_weights: [255, 0, 0, 0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Mesh {
    pub first_index: u32,
    pub index_count: u32,
    /// Offset into the string table.
    pub material_offset: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Bone {
    /// Index of the parent bone, or -1 for a root.
    pub parent: i32,
    pub name_offset: u32,
    /// Rest position, relative to the parent.
    pub position: [f32; 3],
    /// Rest rotation as a quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
}

/// One bone's local transform in one frame of an animation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BoneKey {
    /// Relative to the parent bone, like [`Bone::position`].
    pub translation: [f32; 3],
    /// Quaternion `[x, y, z, w]`, relative to the parent.
    pub rotation: [f32; 4],
}

/// An animation clip.
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub name: String,
    /// Frames per second the keys were sampled at.
    pub fps: f32,
    pub frame_count: u32,
    /// Whether it plays round again when it ends, or holds its last frame.
    pub looping: bool,
    /// `frame_count` frames of one [`BoneKey`] per bone, frame after frame.
    pub keys: Vec<BoneKey>,
}

impl Animation {
    /// Length in seconds, from the first frame to the last.
    pub fn duration(&self) -> f32 {
        if self.frame_count < 2 || self.fps <= 0.0 {
            0.0
        } else {
            (self.frame_count - 1) as f32 / self.fps
        }
    }

    /// The keys of one frame, one per bone.
    pub fn frame(&self, frame: usize, bone_count: usize) -> &[BoneKey] {
        let start = frame.min(self.frame_count.saturating_sub(1) as usize) * bone_count;
        self.keys.get(start..start + bone_count).unwrap_or(&[])
    }
}

/// An animation's record in the file; its keys follow every record.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RawAnimation {
    name_offset: u32,
    frame_count: u32,
    fps: f32,
    /// Bit 0: loops.
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RawHeader {
    magic: [u8; 4],
    version: u32,
    vertex_count: u32,
    index_count: u32,
    mesh_count: u32,
    bone_count: u32,
    string_bytes: u32,
    flags: u32,
    mins: [f32; 3],
    maxs: [f32; 3],
    /// Zero in version 1, where it was reserved.
    animation_count: u32,
    _reserved: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub meshes: Vec<Mesh>,
    pub bones: Vec<Bone>,
    /// NUL-separated names, indexed by the offsets above.
    pub strings: Vec<u8>,
    pub bounds: Aabb,
    /// Animation clips, for a skinned model.
    pub animations: Vec<Animation>,
}

impl Model {
    pub fn new() -> Self {
        Model {
            bounds: Aabb::EMPTY,
            ..Default::default()
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
    pub fn is_skinned(&self) -> bool {
        self.bones.len() > 1
    }

    /// Intern a name, reusing an existing entry.
    pub fn intern(&mut self, name: &str) -> u32 {
        let needle = name.as_bytes();
        let mut offset = 0usize;
        while offset < self.strings.len() {
            let existing = read_string(&self.strings, offset);
            if existing.as_bytes() == needle {
                return offset as u32;
            }
            offset += existing.len() + 1;
        }
        let at = self.strings.len() as u32;
        self.strings.extend_from_slice(needle);
        self.strings.push(0);
        at
    }

    pub fn string_at(&self, offset: u32) -> &str {
        read_string(&self.strings, offset as usize)
    }

    pub fn mesh_material(&self, mesh: usize) -> &str {
        self.meshes
            .get(mesh)
            .map_or("", |m| self.string_at(m.material_offset))
    }

    pub fn bone_name(&self, bone: usize) -> &str {
        self.bones
            .get(bone)
            .map_or("", |b| self.string_at(b.name_offset))
    }

    /// Every material this model draws with, for content packing.
    pub fn materials(&self) -> Vec<&str> {
        let mut out: Vec<&str> = (0..self.meshes.len())
            .map(|i| self.mesh_material(i))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Recompute the bounding box from the vertices.
    pub fn recompute_bounds(&mut self) {
        let mut b = Aabb::EMPTY;
        for v in &self.vertices {
            b.add_point(Vec3::from_array(v.position));
        }
        self.bounds = b;
    }

    /// Check every index and bone reference points at something real.
    pub fn validate(&self) -> Result<(), ModelError> {
        for (i, mesh) in self.meshes.iter().enumerate() {
            let end = mesh.first_index as usize + mesh.index_count as usize;
            if end > self.indices.len() {
                return Err(ModelError::BadIndex { mesh: i });
            }
            for &index in &self.indices[mesh.first_index as usize..end] {
                if index as usize >= self.vertices.len() {
                    return Err(ModelError::BadIndex { mesh: i });
                }
            }
        }
        // Parents must come first so a single forward pass can build world
        // transforms without recursion or sorting.
        for (i, bone) in self.bones.iter().enumerate() {
            if bone.parent >= 0 && bone.parent as usize >= i {
                return Err(ModelError::BadBoneOrder {
                    bone: i,
                    parent: bone.parent,
                });
            }
        }
        for (i, animation) in self.animations.iter().enumerate() {
            let expected = animation.frame_count as usize * self.bones.len();
            if animation.keys.len() != expected {
                return Err(ModelError::BadAnimation {
                    animation: i,
                    found: animation.keys.len(),
                    frames: animation.frame_count as usize,
                    bones: self.bones.len(),
                    expected,
                });
            }
        }
        Ok(())
    }

    /// The index of the animation called `name`, ignoring case.
    pub fn animation_index(&self, name: &str) -> Option<usize> {
        self.animations
            .iter()
            .position(|a| a.name.eq_ignore_ascii_case(name))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        // Animation names go in the string table with everything else, so
        // they are interned on a copy rather than on `self`.
        let mut strings = self.strings.clone();
        let mut records = Vec::with_capacity(self.animations.len());
        for animation in &self.animations {
            let name_offset = intern_into(&mut strings, &animation.name);
            records.push(RawAnimation {
                name_offset,
                frame_count: animation.frame_count,
                fps: animation.fps,
                flags: animation.looping as u32,
            });
        }

        let header = RawHeader {
            magic: MAGIC,
            version: VERSION,
            vertex_count: self.vertices.len() as u32,
            index_count: self.indices.len() as u32,
            mesh_count: self.meshes.len() as u32,
            bone_count: self.bones.len() as u32,
            string_bytes: strings.len() as u32,
            flags: 0,
            mins: self.bounds.min.to_array(),
            maxs: self.bounds.max.to_array(),
            animation_count: self.animations.len() as u32,
            _reserved: 0,
        };

        let mut out = Vec::new();
        out.extend_from_slice(bytemuck::bytes_of(&header));
        debug_assert_eq!(out.len(), HEADER_SIZE);
        out.extend_from_slice(bytemuck::cast_slice(&self.vertices));
        out.extend_from_slice(bytemuck::cast_slice(&self.indices));
        out.extend_from_slice(bytemuck::cast_slice(&self.meshes));
        out.extend_from_slice(bytemuck::cast_slice(&self.bones));
        out.extend_from_slice(&strings);
        // Records, then every clip's keys, so the records can be read as one
        // array. Keys are four-byte fields, so no padding is needed after the
        // string table for a copying reader like this one.
        out.extend_from_slice(bytemuck::cast_slice(&records));
        for animation in &self.animations {
            out.extend_from_slice(bytemuck::cast_slice(&animation.keys));
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Model, ModelError> {
        if bytes.len() < HEADER_SIZE {
            return Err(ModelError::Truncated {
                needed: HEADER_SIZE,
                available: bytes.len(),
            });
        }
        // Read unaligned: the caller's slice makes no alignment promise.
        let header: RawHeader = bytemuck::pod_read_unaligned(&bytes[..HEADER_SIZE]);
        if header.magic != MAGIC {
            return Err(ModelError::BadMagic);
        }
        if !(1..=VERSION).contains(&header.version) {
            return Err(ModelError::BadVersion {
                found: header.version,
                expected: VERSION,
            });
        }

        let mut offset = HEADER_SIZE;
        let vertices: Vec<Vertex> = read_array(bytes, &mut offset, header.vertex_count as usize)?;
        let indices: Vec<u32> = read_array(bytes, &mut offset, header.index_count as usize)?;
        let meshes: Vec<Mesh> = read_array(bytes, &mut offset, header.mesh_count as usize)?;
        let bones: Vec<Bone> = read_array(bytes, &mut offset, header.bone_count as usize)?;

        let string_end = offset + header.string_bytes as usize;
        if string_end > bytes.len() {
            return Err(ModelError::Truncated {
                needed: string_end,
                available: bytes.len(),
            });
        }
        let strings = bytes[offset..string_end].to_vec();
        offset = string_end;

        let animation_count = if header.version >= 2 {
            header.animation_count as usize
        } else {
            0
        };
        let records: Vec<RawAnimation> = read_array(bytes, &mut offset, animation_count)?;
        let mut animations = Vec::with_capacity(records.len());
        for record in &records {
            let count = record.frame_count as usize * bones.len();
            let keys: Vec<BoneKey> = read_array(bytes, &mut offset, count)?;
            animations.push(Animation {
                name: read_string(&strings, record.name_offset as usize).to_string(),
                fps: record.fps,
                frame_count: record.frame_count,
                looping: record.flags & 1 != 0,
                keys,
            });
        }

        let model = Model {
            vertices,
            indices,
            meshes,
            bones,
            strings,
            bounds: Aabb::new(Vec3::from_array(header.mins), Vec3::from_array(header.maxs)),
            animations,
        };
        model.validate()?;
        Ok(model)
    }
}

fn read_array<T: Pod>(
    bytes: &[u8],
    offset: &mut usize,
    count: usize,
) -> Result<Vec<T>, ModelError> {
    let size = count * std::mem::size_of::<T>();
    let end = *offset + size;
    if end > bytes.len() {
        return Err(ModelError::Truncated {
            needed: end,
            available: bytes.len(),
        });
    }
    let mut out: Vec<T> = vec![T::zeroed(); count];
    bytemuck::cast_slice_mut::<T, u8>(&mut out).copy_from_slice(&bytes[*offset..end]);
    *offset = end;
    Ok(out)
}

/// Intern a name into a NUL-separated table, reusing an existing entry.
fn intern_into(strings: &mut Vec<u8>, name: &str) -> u32 {
    let mut offset = 0usize;
    while offset < strings.len() {
        let existing = read_string(strings, offset);
        if existing == name {
            return offset as u32;
        }
        offset += existing.len() + 1;
    }
    let at = strings.len() as u32;
    strings.extend_from_slice(name.as_bytes());
    strings.push(0);
    at
}

fn read_string(buf: &[u8], offset: usize) -> &str {
    if offset >= buf.len() {
        return "";
    }
    let rest = &buf[offset..];
    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    std::str::from_utf8(&rest[..end]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube as two meshes with different materials.
    fn crate_model() -> Model {
        let mut m = Model::new();
        let wood = m.intern("props/crate_wood");
        let metal = m.intern("props/crate_metal");

        // Two quads is enough to exercise the structure.
        for (i, z) in [0.0f32, 32.0].iter().enumerate() {
            let base = m.vertices.len() as u32;
            for (x, y) in [(0.0, 0.0), (32.0, 0.0), (32.0, 32.0), (0.0, 32.0)] {
                m.vertices.push(Vertex::rigid(
                    Vec3::new(x, y, *z),
                    Vec3::Z,
                    [x / 32.0, y / 32.0],
                ));
            }
            let first_index = m.indices.len() as u32;
            m.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            m.meshes.push(Mesh {
                first_index,
                index_count: 6,
                material_offset: if i == 0 { wood } else { metal },
                flags: 0,
            });
        }
        m.recompute_bounds();
        m
    }

    #[test]
    fn a_model_round_trips() {
        let m = crate_model();
        let bytes = m.to_bytes();
        let back = Model::from_bytes(&bytes).unwrap();
        assert_eq!(back.vertices.len(), m.vertices.len());
        assert_eq!(back.triangle_count(), 4);
        assert_eq!(back.meshes.len(), 2);
        assert_eq!(back.mesh_material(0), "props/crate_wood");
        assert_eq!(back.mesh_material(1), "props/crate_metal");
        assert_eq!(back.to_bytes(), bytes);
    }

    #[test]
    fn bounds_cover_every_vertex() {
        let m = crate_model();
        assert_eq!(m.bounds.min, Vec3::ZERO);
        assert_eq!(m.bounds.max, Vec3::new(32.0, 32.0, 32.0));
        let back = Model::from_bytes(&m.to_bytes()).unwrap();
        assert_eq!(back.bounds, m.bounds);
    }

    #[test]
    fn materials_are_listed_once_each() {
        assert_eq!(
            crate_model().materials(),
            vec!["props/crate_metal", "props/crate_wood"]
        );
    }

    #[test]
    fn interning_reuses_names_and_does_not_match_prefixes() {
        let mut m = Model::new();
        let a = m.intern("props/crate");
        let b = m.intern("props/crate_lid");
        let c = m.intern("props/crate");
        assert_eq!(a, c);
        assert_ne!(a, b);
        assert_eq!(m.string_at(b), "props/crate_lid");
    }

    #[test]
    fn a_static_model_is_fully_weighted_to_its_root() {
        let m = crate_model();
        for v in &m.vertices {
            assert_eq!(v.bone_weights, [255, 0, 0, 0]);
        }
        assert!(!m.is_skinned());
    }

    #[test]
    fn out_of_range_indices_are_rejected() {
        let mut m = crate_model();
        m.indices[0] = 9999;
        assert!(matches!(
            m.validate(),
            Err(ModelError::BadIndex { mesh: 0 })
        ));
        // And a file carrying them fails to load rather than crashing later.
        assert!(Model::from_bytes(&m.to_bytes()).is_err());
    }

    #[test]
    fn a_mesh_range_past_the_index_array_is_rejected() {
        let mut m = crate_model();
        m.meshes[0].index_count = 999;
        assert!(matches!(
            m.validate(),
            Err(ModelError::BadIndex { mesh: 0 })
        ));
    }

    #[test]
    fn bones_must_be_listed_parents_first() {
        let mut m = crate_model();
        let root = m.intern("root");
        let child = m.intern("child");
        // Child listed before its parent.
        m.bones.push(Bone {
            parent: 1,
            name_offset: child,
            rotation: [0.0, 0.0, 0.0, 1.0],
            ..Default::default()
        });
        m.bones.push(Bone {
            parent: -1,
            name_offset: root,
            rotation: [0.0, 0.0, 0.0, 1.0],
            ..Default::default()
        });
        assert!(matches!(
            m.validate(),
            Err(ModelError::BadBoneOrder { bone: 0, .. })
        ));

        m.bones.swap(0, 1);
        m.bones[1].parent = 0;
        assert!(m.validate().is_ok());
        assert_eq!(m.bone_name(0), "root");
        assert!(m.is_skinned());
    }

    #[test]
    fn garbage_and_truncation_are_rejected() {
        assert!(matches!(
            Model::from_bytes(&[0u8; 128]),
            Err(ModelError::BadMagic)
        ));
        assert!(matches!(
            Model::from_bytes(b"VM"),
            Err(ModelError::Truncated { .. })
        ));

        let bytes = crate_model().to_bytes();
        assert!(matches!(
            Model::from_bytes(&bytes[..HEADER_SIZE + 8]),
            Err(ModelError::Truncated { .. })
        ));
    }

    #[test]
    fn an_empty_model_is_valid() {
        let m = Model::new();
        let back = Model::from_bytes(&m.to_bytes()).unwrap();
        assert_eq!(back.triangle_count(), 0);
        assert!(back.materials().is_empty());
    }

    /// Two bones, and a one-second walk that swings the child.
    fn skinned_model() -> Model {
        let mut m = crate_model();
        let root = m.intern("root");
        let arm = m.intern("arm");
        m.bones.push(Bone {
            parent: -1,
            name_offset: root,
            rotation: [0.0, 0.0, 0.0, 1.0],
            ..Default::default()
        });
        m.bones.push(Bone {
            parent: 0,
            name_offset: arm,
            position: [0.0, 0.0, 16.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        });
        let mut keys = Vec::new();
        for frame in 0..4 {
            keys.push(BoneKey {
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
            });
            let angle = frame as f32 * 0.3;
            keys.push(BoneKey {
                translation: [0.0, 0.0, 16.0],
                rotation: [0.0, (angle / 2.0).sin(), 0.0, (angle / 2.0).cos()],
            });
        }
        m.animations.push(Animation {
            name: "walk".into(),
            fps: 3.0,
            frame_count: 4,
            looping: true,
            keys,
        });
        m
    }

    #[test]
    fn animations_round_trip_and_are_found_by_name() {
        let m = skinned_model();
        let bytes = m.to_bytes();
        let back = Model::from_bytes(&bytes).unwrap();
        assert_eq!(back.animations, m.animations);
        assert_eq!(back.animation_index("WALK"), Some(0));
        assert_eq!(back.animation_index("run"), None);
        assert!((back.animations[0].duration() - 1.0).abs() < 1e-6);
        assert_eq!(back.animations[0].frame(2, 2)[1], m.animations[0].keys[5]);
        // The name went into the file's string table, not the model's.
        assert_eq!(back.to_bytes(), bytes);
    }

    #[test]
    fn a_version_1_model_still_loads_with_no_animations() {
        // Every prop compiled before animations existed.
        let mut bytes = crate_model().to_bytes();
        bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
        let back = Model::from_bytes(&bytes).unwrap();
        assert!(back.animations.is_empty());
        assert_eq!(back.meshes.len(), 2);
    }

    #[test]
    fn an_animation_short_of_keys_is_rejected() {
        let mut m = skinned_model();
        m.animations[0].keys.pop();
        assert!(matches!(
            m.validate(),
            Err(ModelError::BadAnimation { animation: 0, .. })
        ));
    }

    #[test]
    fn vertex_layout_has_no_padding() {
        // The renderer uploads these straight to the GPU with a fixed stride.
        assert_eq!(std::mem::size_of::<Vertex>(), 40);
        assert_eq!(std::mem::size_of::<Mesh>(), 16);
        assert_eq!(std::mem::size_of::<Bone>(), 36);
        assert_eq!(std::mem::size_of::<BoneKey>(), 28);
        assert_eq!(std::mem::size_of::<RawHeader>(), HEADER_SIZE);
    }
}
