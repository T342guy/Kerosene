// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Baking lightmaps.
//!
//! Every lit face carries a small grid of *luxels*, each holding the light
//! arriving at one patch of surface. Baking one means finding where that luxel
//! sits in the world, then asking every light whether it can see it.
//!
//! Two details do most of the work of making the result look right:
//!
//! * **Samples land on the surface, not inside it.** A luxel grid covers the
//!   face's bounding rectangle in texture space, so grid points near the edge
//!   of an angled face fall inside neighbouring geometry. Left alone they bake
//!   black and produce a dark rim around every surface. Nudging them toward
//!   the face centre until they clear solid fixes it.
//!
//! * **Shadow rays start slightly off the surface.** Starting exactly on the
//!   face means the first thing the ray hits is the face itself, and every
//!   surface bakes fully shadowed.

use crate::lights::LightSet;
use kerosene_bsp::{Bsp, ColorRgbExp32, contents, surf};
use kerosene_math::{Mat3, Vec3};
use rayon::prelude::*;

#[derive(Clone, Debug)]
pub struct BakeOptions {
    /// Samples per luxel per axis. 1 is fast and hard-edged; 2 or 3 softens
    /// the stair-stepping along shadow boundaries.
    pub supersample: u32,
    /// How many times light bounces off surfaces.
    pub bounces: u32,
    /// Multiplier on everything, for exposure.
    pub scale: f32,
    /// Multiplier on the flat ambient term.
    pub ambient_scale: f32,
}

impl Default for BakeOptions {
    fn default() -> Self {
        BakeOptions {
            supersample: 2,
            bounces: 1,
            scale: 1.0,
            ambient_scale: 1.0,
        }
    }
}

#[derive(Default, Debug)]
pub struct BakeStats {
    pub faces_lit: usize,
    pub faces_unlit: usize,
    pub luxels: usize,
    pub luxels_rescued: usize,
    pub bounce_patches: usize,
}

/// How far off the surface a shadow ray begins, in inches.
///
/// Large enough to clear the face itself after the plane arithmetic, small
/// enough not to peek over the lip of a step.
const SURFACE_OFFSET: f32 = 0.5;

/// Bake lighting into a map's lighting lump.
pub fn bake(bsp: &mut Bsp, lights: &LightSet, options: &BakeOptions) -> BakeStats {
    let mut stats = BakeStats::default();

    // Lay out the lighting lump first so each face knows where its samples go.
    let mut offset = 0usize;
    let mut jobs: Vec<(usize, usize, usize)> = Vec::new(); // face, offset, count
    for i in 0..bsp.faces.len() {
        let f = &bsp.faces[i];
        let (w, h) = (f.lightmap_size[0] as usize, f.lightmap_size[1] as usize);
        let ti = bsp
            .texinfo
            .get(f.texinfo as usize)
            .copied()
            .unwrap_or_default();
        if w == 0 || h == 0 || ti.flags & (surf::NOLIGHT | surf::SKY | surf::NODRAW) != 0 {
            bsp.faces[i].lightmap_offset = -1;
            stats.faces_unlit += 1;
            continue;
        }
        bsp.faces[i].lightmap_offset = offset as i32;
        jobs.push((i, offset, w * h));
        offset += w * h;
        stats.faces_lit += 1;
    }
    stats.luxels = offset;

    if offset == 0 {
        bsp.lighting.clear();
        return stats;
    }

    // ---- direct lighting ----
    let progress = Progress::new("direct", jobs.len());
    let direct: Vec<(usize, FaceLight)> = jobs
        .par_iter()
        .map(|&(face, _, _)| {
            let lit = light_face(bsp, face, lights, options, Pass::Direct);
            progress.tick();
            (face, lit)
        })
        .collect();

    let mut direct_samples: Vec<Vec3> = vec![Vec3::ZERO; offset];
    for (face, lit) in &direct {
        let start = bsp.faces[*face].lightmap_offset as usize;
        direct_samples[start..start + lit.luxels.len()].copy_from_slice(&lit.luxels);
        stats.luxels_rescued += lit.rescued;
    }
    let mut samples = direct_samples.clone();

    // ---- bounced lighting ----
    // Each lit face becomes an area emitter of its own average brightness
    // times its material's reflectivity, and every luxel gathers from them.
    // One bounce is what turns a room lit by a single lamp from a hard pool of
    // light into something that reads as an interior.
    for bounce in 0..options.bounces {
        let patches = build_patches(bsp, &samples, &jobs);
        if patches.is_empty() {
            break;
        }
        stats.bounce_patches = patches.len();

        let label = format!("bounce {}/{}", bounce + 1, options.bounces);
        let progress = Progress::new(&label, jobs.len());
        let bounced: Vec<(usize, FaceLight)> = jobs
            .par_iter()
            .map(|&(face, _, _)| {
                let lit = light_face(bsp, face, lights, options, Pass::Bounce(&patches));
                progress.tick();
                (face, lit)
            })
            .collect();

        // Direct plus this bounce's gather -- from patches that already hold
        // the previous bounce, which is how light gets around two corners.
        for (face, lit) in &bounced {
            let start = bsp.faces[*face].lightmap_offset as usize;
            for (i, v) in lit.luxels.iter().enumerate() {
                samples[start + i] = direct_samples[start + i] + *v;
            }
        }
    }

    bsp.lighting = samples
        .iter()
        .map(|c| ColorRgbExp32::from_linear(*c * options.scale))
        .collect();

    stats
}

/// A line every tenth of the way through a pass, from whichever thread
/// crosses the mark. Lighting is the longest stage of a build, and a build
/// that prints nothing for minutes looks hung.
struct Progress {
    label: String,
    total: usize,
    done: std::sync::atomic::AtomicUsize,
    started: std::time::Instant,
}

impl Progress {
    fn new(label: &str, total: usize) -> Progress {
        Progress {
            label: label.to_string(),
            total,
            done: std::sync::atomic::AtomicUsize::new(0),
            started: std::time::Instant::now(),
        }
    }

    fn tick(&self) {
        let done = self.done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let step = (self.total / 10).max(1);
        if self.total >= 200 && done.is_multiple_of(step) && done < self.total {
            println!(
                "  {:<10} {}% ({done}/{} faces, {:.0}s)",
                self.label,
                done * 100 / self.total,
                self.total,
                self.started.elapsed().as_secs_f32()
            );
        }
    }
}

/// An area light standing in for a lit surface, for the bounce pass.
struct Patch {
    center: Vec3,
    normal: Vec3,
    area: f32,
    /// Light this surface re-emits.
    radiance: Vec3,
}

fn build_patches(bsp: &Bsp, samples: &[Vec3], jobs: &[(usize, usize, usize)]) -> Vec<Patch> {
    let mut out = Vec::new();
    for &(face_index, offset, count) in jobs {
        if count == 0 {
            continue;
        }
        let face = &bsp.faces[face_index];
        let average: Vec3 = samples[offset..offset + count]
            .iter()
            .copied()
            .sum::<Vec3>()
            / count as f32;
        if average.max_element() < 1.0 {
            continue;
        }

        let reflectivity = bsp
            .texinfo
            .get(face.texinfo as usize)
            .and_then(|ti| bsp.texdata.get(ti.texdata as usize))
            .map(|td| Vec3::from_array(td.reflectivity))
            .unwrap_or(Vec3::splat(0.5));

        let verts = bsp.face_vertices(face_index);
        if verts.len() < 3 {
            continue;
        }
        let center = verts.iter().copied().sum::<Vec3>() / verts.len() as f32;
        let Some(plane) = bsp.face_plane(face_index) else {
            continue;
        };

        out.push(Patch {
            center,
            normal: plane.normal,
            area: face.area.max(1.0),
            radiance: average * reflectivity,
        });
    }
    out
}

/// Which light a pass gathers.
///
/// Direct light is gathered once; every bounce pass gathers only what the
/// patches re-emit and adds it to the direct result it was handed. Gathering
/// both on every bounce re-traced every shadow ray per bounce, which was
/// most of what `--bounces 3` cost.
#[derive(Clone, Copy)]
enum Pass<'a> {
    Direct,
    Bounce(&'a [Patch]),
}

/// One face's luxels, and how many of them had to be rescued from inside
/// geometry (see `rescue_sample`).
struct FaceLight {
    luxels: Vec<Vec3>,
    rescued: usize,
}

impl FaceLight {
    fn black(count: usize) -> FaceLight {
        FaceLight {
            luxels: vec![Vec3::ZERO; count],
            rescued: 0,
        }
    }
}

/// Compute every luxel of one face.
fn light_face(
    bsp: &Bsp,
    face_index: usize,
    lights: &LightSet,
    options: &BakeOptions,
    pass: Pass<'_>,
) -> FaceLight {
    let face = bsp.faces[face_index];
    let (w, h) = (
        face.lightmap_size[0] as usize,
        face.lightmap_size[1] as usize,
    );
    let Some(ti) = bsp.texinfo.get(face.texinfo as usize).copied() else {
        return FaceLight::black(w * h);
    };
    let Some(plane) = bsp.face_plane(face_index) else {
        return FaceLight::black(w * h);
    };

    let verts = bsp.face_vertices(face_index);
    if verts.len() < 3 {
        return FaceLight::black(w * h);
    }
    let face_center = verts.iter().copied().sum::<Vec3>() / verts.len() as f32;

    // Invert the world-to-luxel mapping. Two rows come from the lightmap axes
    // and the third from the face's own plane, since a luxel lies on it.
    let l0 = Vec3::new(
        ti.lightmap_vecs[0][0],
        ti.lightmap_vecs[0][1],
        ti.lightmap_vecs[0][2],
    );
    let l1 = Vec3::new(
        ti.lightmap_vecs[1][0],
        ti.lightmap_vecs[1][1],
        ti.lightmap_vecs[1][2],
    );
    let basis = Mat3::from_cols(
        Vec3::new(l0.x, l1.x, plane.normal.x),
        Vec3::new(l0.y, l1.y, plane.normal.y),
        Vec3::new(l0.z, l1.z, plane.normal.z),
    );
    if basis.determinant().abs() < 1e-9 {
        // Degenerate mapping: the lightmap axes are parallel or zero.
        return FaceLight::black(w * h);
    }
    let inverse = basis.inverse();

    // The stored grid may have been clamped to a maximum size, so derive the
    // step from the face's real extent rather than assuming one luxel per unit.
    let (mut min_u, mut min_v) = (f32::INFINITY, f32::INFINITY);
    let (mut max_u, mut max_v) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for &p in &verts {
        let (u, v) = ti.lightcoord(p);
        min_u = min_u.min(u);
        max_u = max_u.max(u);
        min_v = min_v.min(v);
        max_v = max_v.max(v);
    }
    let step_u = if w > 1 {
        (max_u - min_u) / (w - 1) as f32
    } else {
        0.0
    };
    let step_v = if h > 1 {
        (max_v - min_v) / (h - 1) as f32
    } else {
        0.0
    };

    let to_world = |u: f32, v: f32| -> Vec3 {
        inverse
            * Vec3::new(
                u - ti.lightmap_vecs[0][3],
                v - ti.lightmap_vecs[1][3],
                plane.dist,
            )
    };

    let ss = options.supersample.max(1) as usize;
    let mut out = Vec::with_capacity(w * h);
    let mut rescued = 0usize;

    for y in 0..h {
        for x in 0..w {
            let mut total = Vec3::ZERO;
            let mut taken = 0u32;

            for sy in 0..ss {
                for sx in 0..ss {
                    // Jitter within the luxel's footprint so shadow edges get
                    // averaged instead of stair-stepping.
                    let fx = if ss == 1 {
                        0.0
                    } else {
                        (sx as f32 / (ss - 1) as f32) - 0.5
                    };
                    let fy = if ss == 1 {
                        0.0
                    } else {
                        (sy as f32 / (ss - 1) as f32) - 0.5
                    };
                    let u = min_u + (x as f32 + fx) * step_u;
                    let v = min_v + (y as f32 + fy) * step_v;

                    let world = to_world(u, v);
                    let Some(sample_at) = rescue_sample(bsp, world, face_center, plane.normal)
                    else {
                        continue;
                    };
                    if sample_at != world + plane.normal * SURFACE_OFFSET {
                        rescued += 1;
                    }
                    total += gather(bsp, sample_at, plane.normal, lights, options, pass);
                    taken += 1;
                }
            }

            // Every sample was buried in geometry: fall back to the face
            // centre so the luxel is merely approximate rather than black.
            if taken == 0 {
                let at = face_center + plane.normal * SURFACE_OFFSET;
                out.push(gather(bsp, at, plane.normal, lights, options, pass));
                rescued += 1;
            } else {
                out.push(total / taken as f32);
            }
        }
    }

    FaceLight {
        luxels: out,
        rescued,
    }
}

/// Move a sample point out of solid geometry, if it landed there.
///
/// Grid points near the edge of an angled face fall outside it, often inside
/// the wall next door. Walking toward the face centre finds the nearest point
/// that is genuinely on the surface.
fn rescue_sample(bsp: &Bsp, world: Vec3, face_center: Vec3, normal: Vec3) -> Option<Vec3> {
    let lifted = world + normal * SURFACE_OFFSET;
    if !bsp.point_is_solid(lifted) {
        return Some(lifted);
    }

    for step in 1..=4 {
        let t = step as f32 / 5.0;
        let pulled = world.lerp(face_center, t) + normal * SURFACE_OFFSET;
        if !bsp.point_is_solid(pulled) {
            return Some(pulled);
        }
    }
    None
}

/// Light arriving at a point on a surface, for one pass.
fn gather(
    bsp: &Bsp,
    point: Vec3,
    normal: Vec3,
    lights: &LightSet,
    options: &BakeOptions,
    pass: Pass<'_>,
) -> Vec3 {
    if let Pass::Bounce(patches) = pass {
        return gather_bounce(bsp, point, normal, patches);
    }
    let mut total = lights.ambient * options.ambient_scale;

    for light in &lights.lights {
        let Some((intensity, direction)) = light.sample(point) else {
            continue;
        };

        // Lambert: a surface edge-on to a light receives none of it.
        let lambert = normal.dot(direction);
        if lambert <= 0.0 {
            continue;
        }

        let target = light.shadow_target(point);
        let trace = bsp.trace_ray(point, target, contents::MASK_OPAQUE);

        if light.is_sun() {
            // The sun only reaches surfaces with a clear path to the sky. A
            // ray that stops on anything else is in shadow; one that reaches
            // a sky surface has left the building.
            if trace.hit() && trace.surface_flags & surf::SKY == 0 {
                continue;
            }
        } else if trace.hit() {
            continue;
        }

        total += intensity * lambert;
    }

    total
}

/// Light arriving from other lit surfaces.
fn gather_bounce(bsp: &Bsp, point: Vec3, normal: Vec3, patches: &[Patch]) -> Vec3 {
    let mut total = Vec3::ZERO;

    for patch in patches {
        let delta = patch.center - point;
        let dist_sq = delta.length_squared();
        if dist_sq < 1.0 {
            continue;
        }
        let dist = dist_sq.sqrt();
        let dir = delta / dist;

        let cos_receiver = normal.dot(dir);
        if cos_receiver <= 0.0 {
            continue;
        }
        let cos_emitter = patch.normal.dot(-dir);
        if cos_emitter <= 0.0 {
            continue;
        }

        // Standard form factor between two differential patches.
        let form = cos_receiver * cos_emitter * patch.area / (std::f32::consts::PI * dist_sq);
        if form < 1e-4 {
            continue;
        }

        // Traced to just above the emitting face, not onto it: a ray that
        // ends exactly on a solid brush's plane counts as entering it, so
        // every patch used to occlude itself and no bounce ever arrived.
        let target = patch.center + patch.normal * SURFACE_OFFSET;
        if bsp.trace_ray(point, target, contents::MASK_OPAQUE).hit() {
            continue;
        }
        total += patch.radiance * form;
    }

    total
}

#[cfg(test)]
mod tests;
