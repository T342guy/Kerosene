// SPDX-License-Identifier: MPL-2.0
//! Turning a compiled map into geometry a GPU can draw.
//!
//! A `.kerobsp` stores faces as rings of shared edges, which is right for
//! collision and visibility and wrong for drawing. This turns them into
//! triangles with everything a shader needs, and groups them so the world can
//! be drawn in a handful of draw calls rather than one per face.
//!
//! Two coordinate conversions happen here:
//!
//! * **Winding order.** Faces are stored clockwise as seen from the front,
//!   which is the brush-file convention. GPUs treat counter-clockwise as
//!   front-facing, so the triangle fan is emitted in reverse.
//! * **Texture coordinates.** `texinfo` produces coordinates in *texels*;
//!   shaders want them normalised by the texture's size.

use crate::lightmap::LightmapAtlas;
use bytemuck::{Pod, Zeroable};
use kerosene_bsp::{Bsp, surf};
use kerosene_math::{Aabb, Pose, Vec3};

/// One vertex of world geometry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct WorldVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Material coordinates, normalised.
    pub uv: [f32; 2],
    /// Lightmap atlas coordinates, normalised.
    pub lightmap_uv: [f32; 2],
}

/// One face's triangles.
#[derive(Clone, Debug)]
pub struct Surface {
    pub face: usize,
    pub first_index: u32,
    pub index_count: u32,
    /// Index into [`WorldMesh::materials`].
    pub material: u32,
    pub bounds: Aabb,
    /// Surface flags from the face's texinfo.
    pub flags: u32,
    /// Whether this surface has a lightmap patch in the atlas.
    pub lit: bool,
}

impl Surface {
    pub fn is_sky(&self) -> bool { self.flags & surf::SKY != 0 }
    pub fn is_translucent(&self) -> bool { self.flags & surf::TRANS != 0 }
}

/// Surfaces sharing one material, drawn together.
#[derive(Clone, Debug)]
pub struct Batch {
    pub material: u32,
    pub surfaces: Vec<u32>,
    /// Set when every surface in the batch is contiguous in the index buffer,
    /// so the whole batch is one draw call rather than one per surface.
    pub contiguous_range: Option<(u32, u32)>,
}

/// Everything needed to draw a map.
#[derive(Default)]
pub struct WorldMesh {
    pub vertices: Vec<WorldVertex>,
    pub indices: Vec<u32>,
    pub surfaces: Vec<Surface>,
    /// Material names, in the order batches reference them.
    pub materials: Vec<String>,
    /// Surface indices per BSP leaf, for PVS culling.
    pub leaf_surfaces: Vec<Vec<u32>>,
    /// Surface indices per brush model, index 0 being the world.
    ///
    /// Brush entities -- doors, moving platforms, anything tied to a class --
    /// are compiled as their own models, and their leaves are not in the
    /// world's PVS. They are therefore invisible to a leaf walk, which is how
    /// every brush entity in every map came to be built into the mesh and
    /// never drawn: the door in the sample map was simply not there.
    pub model_surfaces: Vec<Vec<u32>>,
    /// Each model's extent, in the space it was compiled in.
    pub model_bounds: Vec<Aabb>,
    pub batches: Vec<Batch>,
}

impl WorldMesh {
    pub fn triangle_count(&self) -> usize { self.indices.len() / 3 }

    /// Build the drawable form of a map.
    ///
    /// Surfaces are emitted grouped by material so that each batch occupies a
    /// contiguous run of the index buffer, which is what lets the whole world
    /// draw in as many calls as it has materials.
    pub fn build(bsp: &Bsp, atlas: &LightmapAtlas) -> WorldMesh {
        let mut mesh = WorldMesh::default();

        // Group faces by material first.
        let mut by_material: Vec<(String, Vec<usize>)> = Vec::new();
        for face_index in 0..bsp.faces.len() {
            let ti = bsp.texinfo.get(bsp.faces[face_index].texinfo as usize);
            // Nodraw faces should never have reached the file, but a
            // hand-edited map might carry one.
            if ti.is_some_and(|t| t.flags & surf::NODRAW != 0) { continue; }
            let material = bsp.face_material(face_index).to_string();
            match by_material.iter_mut().find(|(m, _)| *m == material) {
                Some((_, faces)) => faces.push(face_index),
                None => by_material.push((material, vec![face_index])),
            }
        }
        // Sorted so the batch order -- and therefore the index buffer -- is
        // identical between runs.
        by_material.sort_by(|a, b| a.0.cmp(&b.0));

        let mut face_to_surface = vec![u32::MAX; bsp.faces.len()];

        for (material_index, (material, faces)) in by_material.into_iter().enumerate() {
            mesh.materials.push(material);
            let batch_start = mesh.indices.len() as u32;
            let mut batch_surfaces = Vec::with_capacity(faces.len());

            for face_index in faces {
                let Some(surface) =
                    mesh.push_face(bsp, atlas, face_index, material_index as u32)
                else {
                    continue;
                };
                face_to_surface[face_index] = surface;
                batch_surfaces.push(surface);
            }

            let batch_end = mesh.indices.len() as u32;
            mesh.batches.push(Batch {
                material: material_index as u32,
                surfaces: batch_surfaces,
                contiguous_range: (batch_end > batch_start).then_some((batch_start, batch_end - batch_start)),
            });
        }

        // Map models onto surfaces, so the ones the PVS cannot reach can be
        // drawn by their bounds instead.
        mesh.model_surfaces = vec![Vec::new(); bsp.models.len()];
        mesh.model_bounds = vec![Aabb::EMPTY; bsp.models.len()];
        for (model_index, model) in bsp.models.iter().enumerate() {
            let first = model.first_face as usize;
            let mut bounds = Aabb::EMPTY;
            for face in first..first + model.num_faces as usize {
                let surface = face_to_surface.get(face).copied().unwrap_or(u32::MAX);
                if surface == u32::MAX { continue }
                mesh.model_surfaces[model_index].push(surface);
                let b = mesh.surfaces[surface as usize].bounds;
                bounds.add_point(b.min);
                bounds.add_point(b.max);
            }
            mesh.model_bounds[model_index] = bounds;
        }

        // Map leaves onto surfaces so the PVS can cull them.
        mesh.leaf_surfaces = vec![Vec::new(); bsp.leaves.len()];
        for (leaf_index, leaf) in bsp.leaves.iter().enumerate() {
            let first = leaf.first_leafface as usize;
            for i in first..first + leaf.num_leaffaces as usize {
                let Some(&face) = bsp.leaffaces.get(i) else { continue };
                let surface = face_to_surface.get(face as usize).copied().unwrap_or(u32::MAX);
                if surface != u32::MAX { mesh.leaf_surfaces[leaf_index].push(surface); }
            }
        }

        mesh
    }

    /// Append one face's triangles, returning its surface index.
    fn push_face(
        &mut self,
        bsp: &Bsp,
        atlas: &LightmapAtlas,
        face_index: usize,
        material: u32,
    ) -> Option<u32> {
        let face = bsp.faces.get(face_index)?;
        let ti = bsp.texinfo.get(face.texinfo as usize)?;
        let texdata = bsp.texdata.get(ti.texdata as usize)?;
        let plane = bsp.face_plane(face_index)?;

        let points = bsp.face_vertices(face_index);
        if points.len() < 3 { return None; }

        let (tex_w, tex_h) = (texdata.width.max(1) as f32, texdata.height.max(1) as f32);
        let rect = atlas.rects.get(face_index).copied().flatten();

        // Luxel coordinates are relative to the face's own grid origin.
        let (mut min_u, mut min_v) = (f32::INFINITY, f32::INFINITY);
        let (mut max_u, mut max_v) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for &p in &points {
            let (u, v) = ti.lightcoord(p);
            min_u = min_u.min(u); max_u = max_u.max(u);
            min_v = min_v.min(v); max_v = max_v.max(v);
        }

        let base = self.vertices.len() as u32;
        let mut bounds = Aabb::EMPTY;

        for &p in &points {
            bounds.add_point(p);
            let (tu, tv) = ti.texcoord(p);
            let lightmap_uv = match rect {
                Some(rect) => {
                    // Rescale the face's luxel range onto the packed patch,
                    // which may be smaller than the face asked for.
                    let span_u = (max_u - min_u).max(1e-6);
                    let span_v = (max_v - min_v).max(1e-6);
                    let (lu, lv) = ti.lightcoord(p);
                    rect.to_uv(
                        (lu - min_u) / span_u * (rect.width.saturating_sub(1)) as f32,
                        (lv - min_v) / span_v * (rect.height.saturating_sub(1)) as f32,
                    )
                }
                // No lightmap: sample the atlas's blank corner, which is
                // black, and let the shader's unlit path take over.
                None => [0.0, 0.0],
            };

            self.vertices.push(WorldVertex {
                position: p.to_array(),
                normal: plane.normal.to_array(),
                uv: [tu / tex_w, tv / tex_h],
                lightmap_uv,
            });
        }

        let first_index = self.indices.len() as u32;
        // Reversed fan: faces are stored clockwise from the front, GPUs want
        // counter-clockwise.
        for i in 2..points.len() as u32 {
            self.indices.extend([base, base + i, base + i - 1]);
        }
        let index_count = self.indices.len() as u32 - first_index;

        let surface_index = self.surfaces.len() as u32;
        self.surfaces.push(Surface {
            face: face_index,
            first_index,
            index_count,
            material,
            bounds,
            flags: ti.flags,
            lit: rect.is_some(),
        });
        Some(surface_index)
    }

    /// Surfaces visible from a viewpoint, culled by PVS and then by frustum.
    ///
    /// The two do different jobs and both matter: the PVS removes whole rooms
    /// you cannot see through any opening, and the frustum removes what is
    /// behind you. Neither subsumes the other.
    pub fn visible_surfaces(
        &self,
        bsp: &Bsp,
        eye: Vec3,
        frustum: &crate::camera::Frustum,
    ) -> Vec<u32> {
        let cluster = bsp.point_cluster(eye);
        let leaves = bsp.visible_leaves(cluster);

        let mut seen = vec![false; self.surfaces.len()];
        let mut out = Vec::new();
        for leaf in leaves {
            // A leaf's own box is a cheap early reject before touching its
            // surfaces at all.
            if let Some(l) = bsp.leaves.get(leaf) {
                let b = l.bounds();
                if !b.is_empty() && !frustum.intersects_box(b.min, b.max) { continue; }
            }
            for &surface in self.leaf_surfaces.get(leaf).into_iter().flatten() {
                let i = surface as usize;
                if seen[i] { continue; }
                let bounds = &self.surfaces[i].bounds;
                if !frustum.intersects_box(bounds.min, bounds.max) { continue; }
                seen[i] = true;
                out.push(surface);
            }
        }
        // Sorted by material so the draw loop can group them without a second
        // pass.
        out.sort_by_key(|&s| (self.surfaces[s as usize].material, s));
        out
    }

    /// Every surface, for when there is no visibility data to cull with.
    pub fn all_surfaces(&self) -> Vec<u32> {
        (0..self.surfaces.len() as u32).collect()
    }

    /// Every surface of the world model, sorted by material.
    ///
    /// What `r_novis` draws: the whole static world with nothing culled. The
    /// brush models are deliberately *not* in it -- they are drawn separately
    /// so they can each carry their own displacement, and including them here
    /// would draw every door twice, once in the wrong place.
    pub fn world_surfaces(&self) -> Vec<u32> {
        let mut out = self.model_surfaces.first().cloned().unwrap_or_default();
        out.sort_by_key(|&s| (self.surfaces[s as usize].material, s));
        out
    }

    /// Whether a brush model, moved to `offset`, could be on screen.
    ///
    /// Frustum only, deliberately. A brush entity could be PVS-tested against
    /// the leaf it currently sits in, but there are a handful of them in a
    /// map and they are the things the player is walking up to and pressing
    /// buttons on: culling one wrongly is far more expensive than drawing it.
    pub fn model_is_visible(
        &self,
        model: usize,
        pose: Pose,
        frustum: &crate::camera::Frustum,
    ) -> bool {
        let Some(bounds) = self.model_bounds.get(model) else { return false };
        if bounds.is_empty() { return false }
        if self.model_surfaces.get(model).is_none_or(Vec::is_empty) { return false }
        // The enclosing box of the turned box, not the turned box: a frustum
        // test wants an axis-aligned answer, and a rotated model's compiled
        // bounds are no longer axis aligned once it has turned.
        let world = pose.bounds_of(*bounds);
        frustum.intersects_box(world.min, world.max)
    }
}

#[cfg(test)]
mod tests;
