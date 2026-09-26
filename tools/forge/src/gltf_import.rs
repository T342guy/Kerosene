// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Reading glTF 2.0 (`.gltf` and `.glb`) into a `.keromdl`.
//!
//! glTF is what every current DCC tool exports with a skeleton and its
//! animations intact, which OBJ cannot carry at all. Forge takes:
//!
//! * every triangle mesh in the default scene, a model mesh per primitive,
//!   named by its material;
//! * the first skin, as the model's bones -- ordered parents first, as the
//!   format requires, with each bone's rest pose taken from the skin's
//!   inverse bind matrices, which is the pose the mesh was actually skinned
//!   in (a node's own transform need not be);
//! * every animation, resampled at [`SAMPLE_RATE`] into the dense keys the
//!   engine plays without evaluating curves. Translation and rotation are
//!   kept; scale is not, and is warned about.
//!
//! Axes and units convert on the way in: glTF is Y-up, faces +Z and is in
//! metres; Kerosene is Z-up, faces +X and is in inches. The conversion is a
//! rotation and a uniform scale, so triangle winding survives unchanged and
//! bone transforms convert by conjugation.

use anyhow::{Context, Result, bail};
use gltf::animation::Interpolation;
use gltf::animation::util::ReadOutputs;
use kerosene_asset::{Animation, Bone, BoneKey, Mesh, Model, Vertex};
use kerosene_math::{Mat3, Mat4, Quat, Vec3};
use std::collections::HashMap;
use std::path::Path;

/// Frames per second animations are resampled at.
pub const SAMPLE_RATE: f32 = 30.0;

/// Options for an import.
pub struct ImportOptions<'a> {
    /// Uniform scale on top of the metres-to-inches conversion.
    pub scale: f32,
    pub default_material: &'a str,
    pub renames: &'a HashMap<&'a str, &'a str>,
    /// Names of animations that play once and hold, rather than loop.
    pub once: &'a [String],
}

/// glTF space to Kerosene's: (x, y, z) -> (z, x, y), scaled.
fn axes() -> Mat3 {
    Mat3::from_cols(Vec3::Y, Vec3::Z, Vec3::X)
}

/// A glTF-space transform, as the same transform in Kerosene space:
/// conjugated by the axis change, with its translation scaled.
fn convert(t: Mat4, scale: f32) -> (Vec3, Quat) {
    let (_, rotation, translation) = t.to_scale_rotation_translation();
    let c = Quat::from_mat3(&axes());
    (
        (axes() * translation) * scale,
        (c * rotation * c.inverse()).normalize(),
    )
}

pub fn import(path: &Path, options: &ImportOptions) -> Result<Model> {
    let (doc, buffers, _) =
        gltf::import(path).with_context(|| format!("reading {}", path.display()))?;
    let scale = options.scale * kerosene_math::units::KU_PER_METRE;
    let c = Mat4::from_mat3(axes());

    // ---- the node tree ----
    let node_count = doc.nodes().count();
    let mut parent: Vec<Option<usize>> = vec![None; node_count];
    for node in doc.nodes() {
        for child in node.children() {
            parent[child.index()] = Some(node.index());
        }
    }
    let local: Vec<Mat4> = doc
        .nodes()
        .map(|n| Mat4::from_cols_array_2d(&n.transform().matrix()))
        .collect();
    let world: Vec<Mat4> = (0..node_count)
        .map(|mut n| {
            let mut m = local[n];
            while let Some(p) = parent[n] {
                m = local[p] * m;
                n = p;
            }
            m
        })
        .collect();

    // ---- the skeleton ----
    let skins: Vec<_> = doc.skins().collect();
    if skins.len() > 1 {
        println!("  warning: {} skins; only the first is used", skins.len());
    }
    let mut model = Model::new();
    // Node index -> bone index, for joints of the skin.
    let mut bone_of: HashMap<usize, usize> = HashMap::new();
    // Bone order, parents first, as node indices.
    let mut joint_nodes: Vec<usize> = Vec::new();
    // The node whose mesh the skin deforms: its space is the model's.
    let skinned_node = doc
        .nodes()
        .find(|n| n.skin().is_some() && n.mesh().is_some())
        .map(|n| n.index());
    let mesh_space = skinned_node
        .map(|n| world[n].inverse())
        .unwrap_or(Mat4::IDENTITY);

    if let Some(skin) = skins.first() {
        let joints: Vec<usize> = skin.joints().map(|j| j.index()).collect();
        if joints.len() > 255 {
            bail!("the skin has {} joints; a .keromdl holds 255", joints.len());
        }
        let reader = skin.reader(|b| Some(&buffers[b.index()]));
        let inverse_bind: Vec<Mat4> = match reader.read_inverse_bind_matrices() {
            Some(m) => m.map(|m| Mat4::from_cols_array_2d(&m)).collect(),
            None => vec![Mat4::IDENTITY; joints.len()],
        };
        let bind: HashMap<usize, Mat4> = joints
            .iter()
            .zip(&inverse_bind)
            .map(|(&n, ibm)| (n, ibm.inverse()))
            .collect();

        // Each joint's nearest ancestor that is also a joint.
        let joint_parent = |n: usize| {
            let mut p = parent[n];
            while let Some(q) = p {
                if bind.contains_key(&q) {
                    return Some(q);
                }
                p = parent[q];
            }
            None
        };
        // Parents first: visit each joint after its joint parent.
        fn visit(n: usize, joint_parent: &dyn Fn(usize) -> Option<usize>, order: &mut Vec<usize>) {
            if order.contains(&n) {
                return;
            }
            if let Some(p) = joint_parent(n) {
                visit(p, joint_parent, order);
            }
            order.push(n);
        }
        for &j in &joints {
            visit(j, &joint_parent, &mut joint_nodes);
        }
        for (bone, &node) in joint_nodes.iter().enumerate() {
            bone_of.insert(node, bone);
        }

        for &node in &joint_nodes {
            let parent_bone = joint_parent(node);
            let rest = match parent_bone {
                Some(p) => bind[&p].inverse() * bind[&node],
                None => bind[&node],
            };
            let (position, rotation) = convert(rest, scale);
            let name = doc
                .nodes()
                .nth(node)
                .and_then(|n| n.name().map(str::to_string))
                .unwrap_or_else(|| format!("bone{node}"));
            let name_offset = model.intern(&name);
            model.bones.push(Bone {
                parent: parent_bone.map_or(-1, |p| bone_of[&p] as i32),
                name_offset,
                position: position.to_array(),
                rotation: rotation.to_array(),
            });
        }
    }

    // ---- the meshes ----
    let mut scale_warned = false;
    for node in doc.nodes() {
        let Some(mesh) = node.mesh() else { continue };
        let skinned = node.skin().is_some() && !model.bones.is_empty();
        // A skinned mesh lives in its own node's space, which is the model's;
        // anything else is placed where the scene puts it, relative to that.
        let place = if skinned {
            Mat4::IDENTITY
        } else {
            mesh_space * world[node.index()]
        };
        let to_model = Mat4::from_scale(Vec3::splat(scale)) * c * place;
        let normal_matrix = Mat3::from_mat4(c * place).inverse().transpose();

        // The joint indices a primitive's JOINTS_0 refers to are the skin's
        // order; the model's bones are parents-first.
        let skin_to_bone: Vec<u8> = node
            .skin()
            .map(|s| {
                s.joints()
                    .map(|j| bone_of.get(&j.index()).copied().unwrap_or(0) as u8)
                    .collect()
            })
            .unwrap_or_default();

        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                println!("  warning: skipped a primitive that is not triangles");
                continue;
            }
            let reader = primitive.reader(|b| Some(&buffers[b.index()]));
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let positions: Vec<Vec3> = positions.map(Vec3::from_array).collect();
            let normals: Vec<Vec3> = reader
                .read_normals()
                .map(|n| n.map(Vec3::from_array).collect())
                .unwrap_or_default();
            let uvs: Vec<[f32; 2]> = reader
                .read_tex_coords(0)
                .map(|t| t.into_f32().collect())
                .unwrap_or_default();
            let joints: Vec<[u16; 4]> = reader
                .read_joints(0)
                .map(|j| j.into_u16().collect())
                .unwrap_or_default();
            let weights: Vec<[f32; 4]> = reader
                .read_weights(0)
                .map(|w| w.into_f32().collect())
                .unwrap_or_default();
            let indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };

            let base = model.vertices.len() as u32;
            for (i, p) in positions.iter().enumerate() {
                let position = to_model.transform_point3(*p);
                let normal = normals
                    .get(i)
                    .map(|n| (normal_matrix * *n).normalize_or_zero())
                    .unwrap_or(Vec3::Z);
                let uv = uvs.get(i).copied().unwrap_or([0.0, 0.0]);
                let mut vertex = Vertex::rigid(position, normal, uv);
                if skinned && let (Some(j), Some(w)) = (joints.get(i), weights.get(i)) {
                    let (indices, weights) = quantise_weights(*j, *w, &skin_to_bone);
                    vertex.bone_indices = indices;
                    vertex.bone_weights = weights;
                }
                model.vertices.push(vertex);
            }
            let first_index = model.indices.len() as u32;
            model.indices.extend(indices.iter().map(|&i| base + i));

            let material = primitive.material().name().unwrap_or("default");
            let material =
                options
                    .renames
                    .get(material)
                    .copied()
                    .unwrap_or(if material == "default" {
                        options.default_material
                    } else {
                        material
                    });
            let material_offset = model.intern(material);
            model.meshes.push(Mesh {
                first_index,
                index_count: indices.len() as u32,
                material_offset,
                flags: 0,
            });
        }
    }
    if model.meshes.is_empty() {
        bail!("{} has no triangle meshes", path.display());
    }

    // ---- the animations ----
    for animation in doc.animations() {
        if model.bones.is_empty() {
            println!("  warning: animations but no skin; animations skipped");
            break;
        }
        let name = animation
            .name()
            .map(str::to_string)
            .unwrap_or_else(|| format!("anim{}", animation.index()));
        // Each joint's channels: translation and rotation samples.
        let mut tracks: HashMap<usize, Tracks> = HashMap::new();
        let mut duration = 0.0f32;
        for channel in animation.channels() {
            let node = channel.target().node().index();
            if !bone_of.contains_key(&node) {
                continue;
            }
            let reader = channel.reader(|b| Some(&buffers[b.index()]));
            let Some(times) = reader.read_inputs() else {
                continue;
            };
            let times: Vec<f32> = times.collect();
            duration = duration.max(times.last().copied().unwrap_or(0.0));
            let interpolation = channel.sampler().interpolation();
            let entry = tracks.entry(node).or_default();
            match reader.read_outputs() {
                Some(ReadOutputs::Translations(v)) => {
                    entry.0 = Some(Track::new(
                        times,
                        v.map(Vec3::from_array).collect(),
                        interpolation,
                    ));
                }
                Some(ReadOutputs::Rotations(r)) => {
                    let values = r.into_f32().map(Quat::from_array).collect();
                    entry.1 = Some(Track::new(times, values, interpolation));
                }
                Some(ReadOutputs::Scales(_)) if !scale_warned => {
                    println!(
                        "  warning: bone scale is animated; a .keromdl keeps only translation and rotation"
                    );
                    scale_warned = true;
                }
                _ => {}
            }
        }

        let frame_count = ((duration * SAMPLE_RATE).round() as u32 + 1).max(1);
        let mut keys = Vec::with_capacity(frame_count as usize * joint_nodes.len());
        for frame in 0..frame_count {
            let t = frame as f32 / SAMPLE_RATE;
            for &node in &joint_nodes {
                let (_, rest_r, rest_t) = local[node].to_scale_rotation_translation();
                let (translation, rotation) = match tracks.get(&node) {
                    Some((tt, rt)) => (
                        tt.as_ref().map_or(rest_t, |tr| tr.sample(t)),
                        rt.as_ref().map_or(rest_r, |tr| tr.sample(t)),
                    ),
                    None => (rest_t, rest_r),
                };
                let mut m = Mat4::from_rotation_translation(rotation, translation);
                // A root bone's parent is not a bone: fold in the nodes above
                // it, relative to the mesh, so its keys are in model space as
                // its rest pose is.
                let is_root = model.bones[bone_of[&node]].parent < 0;
                if is_root {
                    let above = parent[node].map_or(Mat4::IDENTITY, |p| world[p]);
                    m = mesh_space * above * m;
                }
                let (translation, rotation) = convert(m, scale);
                keys.push(BoneKey {
                    translation: translation.to_array(),
                    rotation: rotation.to_array(),
                });
            }
        }
        let looping = !options.once.iter().any(|o| o.eq_ignore_ascii_case(&name));
        model.animations.push(Animation {
            name,
            fps: SAMPLE_RATE,
            frame_count,
            looping,
            keys,
        });
    }

    model.recompute_bounds();
    Ok(model)
}

/// Four joint weights as bytes that add to exactly 255, strongest first,
/// with joint indices mapped from the skin's order to the model's.
fn quantise_weights(
    joints: [u16; 4],
    weights: [f32; 4],
    skin_to_bone: &[u8],
) -> ([u8; 4], [u8; 4]) {
    let mut pairs: Vec<(u8, f32)> = joints
        .iter()
        .zip(weights)
        .map(|(&j, w)| {
            (
                skin_to_bone.get(j as usize).copied().unwrap_or(0),
                w.max(0.0),
            )
        })
        .collect();
    pairs.sort_by(|a, b| b.1.total_cmp(&a.1));
    let total: f32 = pairs.iter().map(|p| p.1).sum();
    if total <= 0.0 {
        return ([0; 4], [255, 0, 0, 0]);
    }
    let mut bytes = [0u8; 4];
    let mut used = 0u32;
    for (i, (_, w)) in pairs.iter().enumerate() {
        bytes[i] = ((w / total) * 255.0).round() as u8;
        used += bytes[i] as u32;
    }
    // Rounding error goes to the strongest influence, so the sum is exact.
    bytes[0] = (bytes[0] as i32 + 255 - used as i32).clamp(0, 255) as u8;
    let indices = [pairs[0].0, pairs[1].0, pairs[2].0, pairs[3].0];
    (indices, bytes)
}

/// A joint's translation and rotation tracks, either of which may be absent.
type Tracks = (Option<Track<Vec3>>, Option<Track<Quat>>);

/// One animated property of one node.
struct Track<T> {
    times: Vec<f32>,
    values: Vec<T>,
    interpolation: Interpolation,
}

trait Mix: Copy {
    fn mix(self, other: Self, t: f32) -> Self;
}
impl Mix for Vec3 {
    fn mix(self, other: Self, t: f32) -> Self {
        self.lerp(other, t)
    }
}
impl Mix for Quat {
    fn mix(self, other: Self, t: f32) -> Self {
        let other = if self.dot(other) < 0.0 { -other } else { other };
        self.slerp(other, t).normalize()
    }
}

impl<T: Mix> Track<T> {
    fn new(times: Vec<f32>, values: Vec<T>, interpolation: Interpolation) -> Track<T> {
        // Cubic splines store in-tangent, value, out-tangent per key; the
        // value is enough at the rate Forge resamples at.
        let values = if interpolation == Interpolation::CubicSpline {
            values.into_iter().skip(1).step_by(3).collect()
        } else {
            values
        };
        Track {
            times,
            values,
            interpolation,
        }
    }

    fn sample(&self, t: f32) -> T {
        let n = self.times.len().min(self.values.len());
        if n == 1 || t <= self.times[0] {
            return self.values[0];
        }
        if t >= self.times[n - 1] {
            return self.values[n - 1];
        }
        let i = self.times[..n].partition_point(|&k| k <= t) - 1;
        if self.interpolation == Interpolation::Step {
            return self.values[i];
        }
        let span = (self.times[i + 1] - self.times[i]).max(1e-6);
        self.values[i].mix(self.values[i + 1], (t - self.times[i]) / span)
    }
}

#[cfg(test)]
mod tests;
