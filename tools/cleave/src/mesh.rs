// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Compiling meshes.
//!
//! A mesh (see [`kerosene_map::mesh`]) is detail: it never enters the tree,
//! so it never splits a leaf or blocks visibility. It becomes two things.
//!
//! * **Faces.** Each face -- cut into flat convex pieces if it is not one --
//!   is filed down the tree into the leaves that can see it, exactly as a
//!   detail brush's faces are, and from there on is an ordinary face: lit by
//!   Radiance, drawn by the renderer, walked on by the walkmap.
//!
//! * **Collision slabs.** Each piece is also extruded backwards into a thin
//!   convex brush: the piece itself for a front, a back plane
//!   [`SLAB_THICKNESS`] behind it, and a side through each edge. Slabs are
//!   detail brushes, so the player's traces, bullets, the rigid-body
//!   simulation and Radiance's shadow rays all see the mesh through the same
//!   brush code they already use -- no second collision system. Quake 3
//!   collided with its curved patches the same way.
//!
//! A slab never cuts another brush's faces in CSG: it is collision, not
//! geometry, and a mesh floor laid over a brush floor must not erase it.

use crate::brush::{BrushWork, SideWork, Warning};
use crate::material;
use kerosene_map::{Mesh, Side, Solid, polygon_normal};
use kerosene_math::{Plane, PlaneSet, Vec3, Winding};

/// How far behind its face a collision slab reaches.
///
/// Thick enough that a player moving at speed cannot step through it in one
/// tick, thin enough that a slab behind a wall panel stays inside the wall.
pub const SLAB_THICKNESS: f32 = 8.0;

/// One drawable piece of a mesh face, ready to be filed into the tree.
#[derive(Clone, Debug)]
pub struct MeshFaceWork {
    pub winding: Winding,
    /// Index into the compile's [`PlaneSet`].
    pub plane: u32,
    /// Material, texture axes and surface flags, in the form the texinfo
    /// builder takes for a brush face.
    pub side: SideWork,
    pub section: u16,
    /// The mesh it came from, for messages.
    pub mesh_id: u32,
}

/// Everything a set of meshes compiles to.
#[derive(Default)]
pub struct MeshWork {
    pub faces: Vec<MeshFaceWork>,
    pub slabs: Vec<BrushWork>,
}

/// Compile the world's meshes. `section_of` places each mesh in a streamed
/// section the way a brush would be placed.
pub fn compile_meshes(
    meshes: &[Mesh],
    planes: &mut PlaneSet,
    warnings: &mut Vec<Warning>,
    mut section_of: impl FnMut(&Solid, &mut Vec<Warning>) -> u16,
) -> MeshWork {
    let mut out = MeshWork::default();
    for mesh in meshes {
        if let Err(e) = mesh.validate() {
            warnings.push(Warning {
                brush_id: mesh.id,
                message: format!("mesh {e}; skipped"),
            });
            continue;
        }
        // The mesh's visgroups and keys, on a stand-in solid, decide its
        // section by the same rules as a brush, and grow the section's bounds
        // to hold it. Padded a unit, so a flat mesh is still a box.
        let b = mesh.bounds();
        let padded = kerosene_math::Aabb::new(b.min - Vec3::ONE, b.max + Vec3::ONE);
        let mut placeholder = Solid::cube(padded, "tools/nodraw");
        placeholder.id = mesh.id;
        placeholder.editor = mesh.editor.clone();
        if let Some(section) = mesh.get("section") {
            placeholder.set("section", section);
        }
        let section = section_of(&placeholder, warnings);

        for face in &mesh.faces {
            let flags = material::flags_for(&face.material);
            for piece in mesh.face_pieces(face) {
                let normal = polygon_normal(&piece);
                if normal == Vec3::ZERO {
                    continue;
                }
                let plane = Plane::new(normal, normal.dot(piece[0]));
                let side = SideWork {
                    plane: planes.insert(plane),
                    material: face.material.clone(),
                    winding: None,
                    surface: flags.surface,
                    emits_face: flags.emits_face,
                    uaxis: face.uaxis,
                    vaxis: face.vaxis,
                    lightmap_scale: face.lightmap_scale,
                    smoothing_groups: 0,
                    walkmap: face.walkmap,
                    map_side_id: face.id,
                    fragments: Vec::new(),
                    generated: false,
                    used_as_node: false,
                };
                if side.emits_face {
                    out.faces.push(MeshFaceWork {
                        winding: Winding::new(piece.clone()),
                        plane: side.plane,
                        side: side.clone(),
                        section,
                        mesh_id: mesh.id,
                    });
                }
                if flags.contents != 0
                    && let Some(slab) = slab(mesh.id, face.id, &piece, plane, face, section, planes)
                {
                    out.slabs.push(slab);
                }
            }
        }
    }
    out
}

/// The collision brush behind one flat convex piece.
fn slab(
    mesh_id: u32,
    face_id: u32,
    piece: &[Vec3],
    plane: Plane,
    face: &kerosene_map::MeshFace,
    section: u16,
    planes: &mut PlaneSet,
) -> Option<BrushWork> {
    let n = plane.normal;
    let centre =
        piece.iter().copied().sum::<Vec3>() / piece.len() as f32 - n * (SLAB_THICKNESS * 0.5);

    let mut solid = Solid {
        id: mesh_id,
        sides: Vec::with_capacity(piece.len() + 2),
        properties: vec![("detail".to_string(), "1".to_string())],
        editor: Default::default(),
    };
    // Every side wears the face's material, never drawn: the slab's contents
    // then follow from one material (a trigger mesh is a trigger, a clip
    // mesh a clip), and a trace that lands anywhere on it reports the
    // surface -- footsteps and impacts read `$surfaceprop` from it.
    let material = face.material.as_str();
    let mut front = Side::from_plane(face_id, plane, material);
    front.uaxis = face.uaxis;
    front.vaxis = face.vaxis;
    solid.sides.push(front);
    solid.sides.push(Side::from_plane(
        face_id,
        Plane::new(-n, -(plane.dist - SLAB_THICKNESS)),
        material,
    ));
    for i in 0..piece.len() {
        let (a, b) = (piece[i], piece[(i + 1) % piece.len()]);
        let edge = b - a;
        if edge.length_squared() < 1e-6 {
            continue;
        }
        let mut side_normal = edge.cross(n).normalize_or_zero();
        if side_normal == Vec3::ZERO {
            continue;
        }
        // Outward, away from the slab's middle.
        if side_normal.dot(a - centre) < 0.0 {
            side_normal = -side_normal;
        }
        solid.sides.push(Side::from_plane(
            face_id,
            Plane::new(side_normal, side_normal.dot(a)),
            material,
        ));
    }

    let mut ignored = Vec::new();
    let mut brush =
        BrushWork::from_solid_in_section(&solid, 0, "worldspawn", section, planes, &mut ignored)?;
    brush.from_mesh = true;
    for side in &mut brush.sides {
        side.emits_face = false;
    }
    Some(brush)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kerosene_map::MeshFace;

    fn floor(material: &str) -> Mesh {
        let mut m = Mesh::new(1);
        m.vertices = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 64.0, 0.0),
            Vec3::new(64.0, 64.0, 0.0),
            Vec3::new(64.0, 0.0, 0.0),
        ];
        let points = m.vertices.clone();
        m.faces
            .push(MeshFace::new(2, vec![0, 1, 2, 3], &points, material));
        m
    }

    fn compile(meshes: &[Mesh]) -> (MeshWork, PlaneSet, Vec<Warning>) {
        let mut planes = PlaneSet::new();
        let mut warnings = Vec::new();
        let work = compile_meshes(meshes, &mut planes, &mut warnings, |_, _| 0);
        (work, planes, warnings)
    }

    #[test]
    fn a_face_becomes_a_drawn_piece_and_a_slab_under_it() {
        let (work, planes, warnings) = compile(&[floor("dev/grid")]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(work.faces.len(), 1);
        assert_eq!(planes.get(work.faces[0].plane).normal, Vec3::Z);

        assert_eq!(work.slabs.len(), 1);
        let slab = &work.slabs[0];
        assert!(slab.is_detail() && !slab.is_structural() && slab.from_mesh);
        assert!(slab.sides.iter().all(|s| !s.emits_face), "collision only");
        // Top at the face, bottom a slab's thickness down, sides on the edges.
        assert!((slab.bounds.max.z - 0.0).abs() < 0.01);
        assert!((slab.bounds.min.z + SLAB_THICKNESS).abs() < 0.01);
        assert!((slab.bounds.min.x - 0.0).abs() < 0.01 && (slab.bounds.max.x - 64.0).abs() < 0.01);
        assert!(slab.contains_point(Vec3::new(32.0, 32.0, -1.0), &planes));
        assert!(!slab.contains_point(Vec3::new(32.0, 32.0, 1.0), &planes));
    }

    #[test]
    fn a_nodraw_mesh_collides_but_draws_nothing() {
        let (work, _, _) = compile(&[floor("tools/nodraw")]);
        assert!(work.faces.is_empty());
        assert_eq!(work.slabs.len(), 1);
    }

    #[test]
    fn a_trigger_mesh_neither_draws_nor_blocks_as_solid() {
        let (work, _, _) = compile(&[floor("tools/trigger")]);
        assert!(work.faces.is_empty());
        assert!(
            work.slabs
                .iter()
                .all(|s| s.contents & kerosene_bsp::contents::SOLID == 0)
        );
    }

    #[test]
    fn a_broken_mesh_is_skipped_with_a_warning() {
        let mut bad = floor("dev/grid");
        bad.faces[0].indices = vec![0, 1];
        let (work, _, warnings) = compile(&[bad]);
        assert!(work.faces.is_empty() && work.slabs.is_empty());
        assert_eq!(warnings.len(), 1);
    }
}
