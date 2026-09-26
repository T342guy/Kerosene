// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Baking cubemap probes.
//!
//! Runs after the lightmaps, because a probe is a picture of the *lit* world:
//! from each `env_cubemap`, one ray per texel goes out, and the texel becomes
//! whatever that ray hit -- the lightmap at the point it landed, tinted by the
//! surface's reflectivity, or the sky's colour if it got out.
//!
//! Nothing here is a renderer. A probe is small (a few thousand texels) and
//! the renderer blurs it further for rough surfaces, so one sample per texel
//! and a surface's average colour are as much detail as it can use. What
//! matters is that the brightness is right, and it is: the same luxels the
//! engine draws the walls with.

use kerosene_bsp::cubemaps::{FACES, texel_direction};
use kerosene_bsp::{Bsp, Cubemaps, Probe, contents, encode_rgb9e5, surf};
use kerosene_kv::{FromKvValue, Vec3Value};
use kerosene_math::Vec3;
use rayon::prelude::*;
use std::collections::HashMap;

/// Texels along one edge of a face, by default. 32 is what Source ships its
/// default cubemaps at, and more is wasted on a surface that blurs it anyway.
pub const DEFAULT_FACE_SIZE: u32 = 32;

/// Far enough to cross any map.
const MAX_DISTANCE: f32 = 65536.0;

/// How a probe bake went.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProbeStats {
    pub probes: usize,
    /// Probes whose origin is inside a wall. They are kept -- removing one
    /// would renumber the rest -- but they see nothing.
    pub buried: usize,
    /// Rays that hit something no face could be found for, such as a brush
    /// entity. Drawn black.
    pub unresolved: usize,
    pub texels: usize,
}

/// Every `env_cubemap` origin in the entity lump, in order.
pub fn probe_origins(bsp: &Bsp) -> Vec<Vec3> {
    let Ok(kv) = bsp.entities_kv() else {
        return Vec::new();
    };
    kv.blocks("entity")
        .filter(|e| e.get("classname") == Some("env_cubemap"))
        .filter_map(|e| e.get("origin"))
        .filter_map(|o| Vec3Value::from_kv(o).ok())
        .map(|v| Vec3::from_array(v.to_array()))
        .collect()
}

/// Bake a probe at each of `origins`. `sky` is the colour a ray that escapes
/// the map sees, on the atlas's scale (1.0 is full light).
pub fn bake(bsp: &Bsp, origins: &[Vec3], face_size: u32, sky: Vec3) -> (Cubemaps, ProbeStats) {
    let mut stats = ProbeStats {
        probes: origins.len(),
        ..Default::default()
    };
    let per_face = (face_size * face_size) as usize;
    let faces = FaceIndex::new(bsp);

    let probes: Vec<(Probe, bool, usize)> = origins
        .par_iter()
        .map(|&origin| {
            let buried = bsp.point_is_solid(origin);
            let mut unresolved = 0;
            let mut texels = Vec::with_capacity(FACES * per_face);
            for face in 0..FACES {
                for y in 0..face_size {
                    for x in 0..face_size {
                        let dir = texel_direction(face, x, y, face_size);
                        let color = if buried {
                            Vec3::ZERO
                        } else {
                            match radiance_along(bsp, &faces, origin, dir, sky) {
                                Some(c) => c,
                                None => {
                                    unresolved += 1;
                                    Vec3::ZERO
                                }
                            }
                        };
                        texels.push(encode_rgb9e5(color));
                    }
                }
            }
            (Probe { origin, texels }, buried, unresolved)
        })
        .collect();

    let mut out = Vec::with_capacity(probes.len());
    for (probe, buried, unresolved) in probes {
        stats.buried += buried as usize;
        stats.unresolved += unresolved;
        stats.texels += probe.texels.len();
        out.push(probe);
    }
    (
        Cubemaps {
            face_size,
            probes: out,
        },
        stats,
    )
}

/// The light arriving at `origin` from `dir`. `None` when the ray stopped on
/// something no world face could be found for.
fn radiance_along(
    bsp: &Bsp,
    faces: &FaceIndex,
    origin: Vec3,
    dir: Vec3,
    sky: Vec3,
) -> Option<Vec3> {
    let trace = bsp.trace_ray(origin, origin + dir * MAX_DISTANCE, contents::MASK_OPAQUE);
    if !trace.hit() {
        // Out of the world entirely: a leak, or a probe in the void. Sky is
        // the least surprising thing to show.
        return Some(sky);
    }
    if trace.surface_flags & surf::SKY != 0 {
        return Some(sky);
    }
    let normal = trace.plane.map(|p| p.normal).unwrap_or(-dir);
    let face = faces.face_at(bsp, trace.endpos, normal)?;
    let light = luxel_at(bsp, face, trace.endpos)?;
    Some(light * reflectivity(bsp, face))
}

/// The world's faces, bucketed by the plane they lie in.
///
/// A trace reports the plane it stopped on and where; this turns that into a
/// face. The obvious route -- the faces of the leaf in front of the hit --
/// misses: Cleave files each face into the leaf that sees it, and a hit an
/// epsilon off the floor can sit in a thin neighbouring leaf with none. The
/// plane is the one thing the trace and the face are sure to agree on.
struct FaceIndex {
    by_plane: HashMap<PlaneKey, Vec<usize>>,
}

/// A plane rounded coarsely enough that the brush plane a trace reports and
/// the face plane Cleave wrote land in the same bucket, and finely enough
/// that two parallel walls a unit apart do not.
type PlaneKey = (i32, i32, i32, i32);

fn plane_key(normal: Vec3, dist: f32) -> PlaneKey {
    let q = |v: f32| (v * 1000.0).round() as i32;
    (q(normal.x), q(normal.y), q(normal.z), dist.round() as i32)
}

impl FaceIndex {
    fn new(bsp: &Bsp) -> FaceIndex {
        let mut by_plane: HashMap<PlaneKey, Vec<usize>> = HashMap::new();
        // World faces only: a brush entity is baked where it was compiled,
        // which is not where it will be when anyone looks at the reflection.
        let world = bsp.models.first().map_or(0..0, |m| {
            m.first_face as usize..(m.first_face + m.num_faces) as usize
        });
        for face in world {
            if let Some(plane) = bsp.face_plane(face) {
                by_plane
                    .entry(plane_key(plane.normal, plane.dist))
                    .or_default()
                    .push(face);
            }
        }
        FaceIndex { by_plane }
    }

    /// The face that `point`, on a surface facing `normal`, lies on.
    ///
    /// Prefers a face whose polygon contains the point; failing that, the
    /// nearest in the plane, because a ray that lands on an edge can miss
    /// both neighbours by a rounding error.
    fn face_at(&self, bsp: &Bsp, point: Vec3, normal: Vec3) -> Option<usize> {
        let dist = normal.dot(point);
        let mut nearest: Option<(f32, usize)> = None;
        // The neighbouring buckets too, for a plane that rounds the other way.
        for d in [0, -1, 1] {
            let (x, y, z, w) = plane_key(normal, dist);
            let Some(faces) = self.by_plane.get(&(x, y, z, w + d)) else {
                continue;
            };
            for &face in faces {
                let Some(plane) = bsp.face_plane(face) else {
                    continue;
                };
                if plane.normal.dot(normal) < 0.99 || plane.distance_to(point).abs() > 1.0 {
                    continue;
                }
                let verts = bsp.face_vertices(face);
                if verts.len() < 3 {
                    continue;
                }
                if contains(&verts, plane.normal, point) {
                    return Some(face);
                }
                let centre = verts.iter().copied().sum::<Vec3>() / verts.len() as f32;
                let d = centre.distance_squared(point);
                if nearest.is_none_or(|(best, _)| d < best) {
                    nearest = Some((d, face));
                }
            }
        }
        nearest.map(|(_, f)| f)
    }
}

/// Whether a convex polygon contains a point on its plane. Either winding.
fn contains(verts: &[Vec3], normal: Vec3, point: Vec3) -> bool {
    const EPSILON: f32 = 0.1;
    let (mut ahead, mut behind) = (false, false);
    for i in 0..verts.len() {
        let a = verts[i];
        let b = verts[(i + 1) % verts.len()];
        let side = (b - a).cross(point - a).dot(normal);
        let len = (b - a).length().max(1e-6);
        if side / len > EPSILON {
            ahead = true;
        } else if side / len < -EPSILON {
            behind = true;
        }
    }
    !(ahead && behind)
}

/// The baked light at `point` on `face`, on the atlas's scale.
///
/// The inverse of how `bake::light_face` lays its grid out: luxels span the
/// face's extent in lightmap space evenly, first to last.
fn luxel_at(bsp: &Bsp, face: usize, point: Vec3) -> Option<Vec3> {
    let f = bsp.faces.get(face)?;
    if f.lightmap_offset < 0 {
        return None;
    }
    let ti = bsp.texinfo.get(f.texinfo as usize)?;
    let (w, h) = (f.lightmap_size[0] as usize, f.lightmap_size[1] as usize);
    if w == 0 || h == 0 {
        return None;
    }

    let (mut min_u, mut min_v) = (f32::INFINITY, f32::INFINITY);
    let (mut max_u, mut max_v) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in bsp.face_vertices(face) {
        let (u, v) = ti.lightcoord(p);
        min_u = min_u.min(u);
        max_u = max_u.max(u);
        min_v = min_v.min(v);
        max_v = max_v.max(v);
    }
    let index = |value: f32, min: f32, max: f32, count: usize| -> usize {
        if count <= 1 || max <= min {
            return 0;
        }
        let t = (value - min) / (max - min) * (count - 1) as f32;
        (t.round().max(0.0) as usize).min(count - 1)
    };
    let (u, v) = ti.lightcoord(point);
    let x = index(u, min_u, max_u, w);
    let y = index(v, min_v, max_v, h);
    let sample = bsp.lighting.get(f.lightmap_offset as usize + y * w + x)?;
    Some(sample.to_linear() / 255.0)
}

/// How much of the light reaching a face it sends back: its texture's average
/// colour, as Cleave measured it.
fn reflectivity(bsp: &Bsp, face: usize) -> Vec3 {
    bsp.faces
        .get(face)
        .and_then(|f| bsp.texinfo.get(f.texinfo as usize))
        .and_then(|ti| bsp.texdata.get(ti.texdata as usize))
        .map(|td| Vec3::from_array(td.reflectivity))
        .unwrap_or(Vec3::splat(0.5))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_polygon_contains_its_centre_and_not_a_point_outside() {
        let square = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 64.0, 0.0),
            Vec3::new(64.0, 64.0, 0.0),
            Vec3::new(64.0, 0.0, 0.0),
        ];
        assert!(contains(&square, Vec3::Z, Vec3::new(32.0, 32.0, 0.0)));
        assert!(
            contains(&square, Vec3::Z, Vec3::new(0.0, 32.0, 0.0)),
            "edges count"
        );
        assert!(!contains(&square, Vec3::Z, Vec3::new(80.0, 32.0, 0.0)));
        // The other winding answers the same.
        let mut reversed = square;
        reversed.reverse();
        assert!(contains(&reversed, Vec3::Z, Vec3::new(32.0, 32.0, 0.0)));
        assert!(!contains(&reversed, Vec3::Z, Vec3::new(-5.0, 32.0, 0.0)));
    }

    #[test]
    fn probe_origins_come_from_env_cubemap_entities_only() {
        let mut bsp = Bsp::new();
        bsp.entities = r#"
entity { "classname" "worldspawn" }
entity { "classname" "env_cubemap" "origin" "1 2 3" }
entity { "classname" "light" "origin" "9 9 9" }
entity { "classname" "env_cubemap" "origin" "-4 5 64" }
"#
        .into();
        assert_eq!(
            probe_origins(&bsp),
            vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(-4.0, 5.0, 64.0)]
        );
    }
}
