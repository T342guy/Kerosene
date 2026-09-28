// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Projected decals: bullet holes, scorch marks, sprays, placed signs.
//!
//! Source's approach, and the one that suits a forward renderer with baked
//! light: when a decal is placed, the world triangles inside its box are
//! found and clipped to it on the CPU, and the pieces become a small mesh of
//! their own. Each clipped vertex keeps the lightmap coordinate of the
//! surface it was cut from, so a decal is lit by exactly the bake the wall
//! under it is -- a bullet hole in a shadow is in shadow -- and it is drawn by
//! the world's own shader, dynamic lights and all, with the decal material's
//! texture in place of the wall's.
//!
//! The work happens once, at placement. Drawing a hundred decals is a draw
//! call per material and section; nothing is re-projected per frame.
//!
//! What is not covered: decals on moving brush models and props, which would
//! have to follow their pose. Only the static world takes them.

use crate::mesh::{WorldMesh, WorldVertex};
use kerosene_math::{Aabb, Vec3};

/// How far a decal is lifted off the surface it lies on, in world units:
/// enough to win the depth test without a bias, too little to see.
pub const LIFT: f32 = 0.08;

/// Where a decal goes and how big it is.
#[derive(Clone, PartialEq, Debug)]
pub struct DecalSpec {
    pub material: String,
    /// A point on the surface.
    pub origin: Vec3,
    /// The surface's outward normal there; the decal projects along it.
    pub normal: Vec3,
    pub width: f32,
    pub height: f32,
    /// Degrees about the normal.
    pub rotation: f32,
    /// How far in front of and behind `origin` surfaces are taken, so a decal
    /// on a corner wraps it but one on a thin wall does not come through the
    /// other side.
    pub depth: f32,
}

impl DecalSpec {
    pub fn new(material: &str, origin: Vec3, normal: Vec3, size: f32) -> DecalSpec {
        DecalSpec {
            material: material.to_string(),
            origin,
            normal,
            width: size,
            height: size,
            rotation: 0.0,
            depth: (size * 0.5).clamp(2.0, 16.0),
        }
    }

    /// The decal's axes: right, up, and the normal it projects along.
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let n = self.normal.normalize_or_zero();
        let n = if n == Vec3::ZERO { Vec3::Z } else { n };
        // "Up" on a wall is world up; on a floor or ceiling there is no such
        // thing, and world +Y stands in.
        let reference = if n.z.abs() > 0.9 { Vec3::Y } else { Vec3::Z };
        let right = reference.cross(n).normalize();
        let up = n.cross(right);
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        (right * cos + up * sin, up * cos - right * sin, n)
    }

    fn bounds(&self) -> Aabb {
        let (r, u, n) = self.basis();
        let (hw, hh, d) = (self.width * 0.5, self.height * 0.5, self.depth);
        let mut points = Vec::with_capacity(8);
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                for sz in [-1.0, 1.0] {
                    points.push(self.origin + r * hw * sx + u * hh * sy + n * d * sz);
                }
            }
        }
        Aabb::from_points(&points)
    }
}

fn overlaps(a: &Aabb, b: &Aabb) -> bool {
    a.min.x <= b.max.x
        && a.max.x >= b.min.x
        && a.min.y <= b.max.y
        && a.max.y >= b.min.y
        && a.min.z <= b.max.z
        && a.max.z >= b.min.z
}

fn lerp_vertex(a: &WorldVertex, b: &WorldVertex, t: f32) -> WorldVertex {
    let mix3 = |x: [f32; 3], y: [f32; 3]| {
        [
            x[0] + (y[0] - x[0]) * t,
            x[1] + (y[1] - x[1]) * t,
            x[2] + (y[2] - x[2]) * t,
        ]
    };
    let mix2 = |x: [f32; 2], y: [f32; 2]| [x[0] + (y[0] - x[0]) * t, x[1] + (y[1] - x[1]) * t];
    WorldVertex {
        position: mix3(a.position, b.position),
        normal: mix3(a.normal, b.normal),
        uv: mix2(a.uv, b.uv),
        lightmap_uv: mix2(a.lightmap_uv, b.lightmap_uv),
        tangent: {
            let t3 = mix3(
                [a.tangent[0], a.tangent[1], a.tangent[2]],
                [b.tangent[0], b.tangent[1], b.tangent[2]],
            );
            [t3[0], t3[1], t3[2], a.tangent[3]]
        },
        probe: a.probe,
    }
}

/// Sutherland-Hodgman against one plane: keep the part of a convex polygon
/// where `distance` is not positive.
fn clip(polygon: &[WorldVertex], distance: impl Fn(&WorldVertex) -> f32) -> Vec<WorldVertex> {
    let mut out = Vec::with_capacity(polygon.len() + 2);
    for i in 0..polygon.len() {
        let a = &polygon[i];
        let b = &polygon[(i + 1) % polygon.len()];
        let (da, db) = (distance(a), distance(b));
        if da <= 0.0 {
            out.push(*a);
        }
        if (da <= 0.0) != (db <= 0.0) {
            out.push(lerp_vertex(a, b, da / (da - db)));
        }
    }
    out
}

/// Cut a decal out of the world: triangles in the decal's texture space,
/// ready to draw with the decal's material. Empty if it touches nothing.
///
/// `surfaces` limits which surfaces are considered -- the world model's, so
/// doors do not take decals they would then carry away from.
pub fn build(mesh: &WorldMesh, spec: &DecalSpec, surfaces: Option<&[u32]>) -> Vec<WorldVertex> {
    let (right, up, normal) = spec.basis();
    let bounds = spec.bounds();
    let (hw, hh, depth) = (spec.width * 0.5, spec.height * 0.5, spec.depth);
    let local = |v: &WorldVertex| {
        let d = Vec3::from_array(v.position) - spec.origin;
        (d.dot(right), d.dot(up), d.dot(normal))
    };

    let all: Vec<u32>;
    let candidates = match surfaces {
        Some(list) => list,
        None => {
            all = (0..mesh.surfaces.len() as u32).collect();
            &all
        }
    };

    let mut out = Vec::new();
    for &s in candidates {
        let Some(surface) = mesh.surfaces.get(s as usize) else {
            continue;
        };
        if surface.is_sky() || !overlaps(&surface.bounds, &bounds) {
            continue;
        }
        let first = surface.first_index as usize;
        let end = (first + surface.index_count as usize).min(mesh.indices.len());
        for tri in mesh.indices[first..end].as_chunks::<3>().0 {
            let v = [
                mesh.vertices[tri[0] as usize],
                mesh.vertices[tri[1] as usize],
                mesh.vertices[tri[2] as usize],
            ];
            // Faces turned away from the projection would take a smeared,
            // stretched copy; faces nearly edge-on, likewise. The stored
            // normals say which way a face points whatever its winding.
            let face_normal = v
                .iter()
                .map(|v| Vec3::from_array(v.normal))
                .sum::<Vec3>()
                .normalize_or_zero();
            if face_normal.dot(normal) < 0.25 {
                continue;
            }
            let mut poly: Vec<WorldVertex> = v.to_vec();
            let planes: [&dyn Fn(&WorldVertex) -> f32; 6] = [
                &|v| local(v).0 - hw,
                &|v| -local(v).0 - hw,
                &|v| local(v).1 - hh,
                &|v| -local(v).1 - hh,
                &|v| local(v).2 - depth,
                &|v| -local(v).2 - depth,
            ];
            for plane in planes {
                poly = clip(&poly, plane);
                if poly.len() < 3 {
                    break;
                }
            }
            if poly.len() < 3 {
                continue;
            }
            for v in &mut poly {
                let (x, y, _) = local(v);
                v.uv = [x / spec.width + 0.5, 0.5 - y / spec.height];
                v.tangent = [right.x, right.y, right.z, 1.0];
                let lifted = Vec3::from_array(v.position) + face_normal * LIFT;
                v.position = lifted.to_array();
            }
            for i in 1..poly.len() - 1 {
                out.extend_from_slice(&[poly[0], poly[i], poly[i + 1]]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
