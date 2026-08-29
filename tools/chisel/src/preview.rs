// SPDX-License-Identifier: LGPL-3.0-or-later
//! Rendering a model small, so it can be picked by looking at it.
//!
//! A model is referenced in a map by a path -- `props/crate_wood` -- and the
//! editor showed exactly that: a name in a dropdown. Which is fine when you
//! wrote the model and terrible otherwise, because a name is not a shape and
//! the only way to find out what `crate_wood` looks like was to place one,
//! compile the map and go and look at it.
//!
//! This is a rasteriser for one job: a model, alone, lit from one side,
//! framed on itself in a square. It is deliberately separate from the world
//! rasteriser next door, which is about a level seen through a camera the
//! designer is flying -- a different problem with different answers.

use crate::raster::Image;
use void_asset::Model;
use void_math::{Angles, Vec3};

/// The background a preview is drawn on.
///
/// Slightly lighter than the viewports, so a preview reads as a picture of a
/// thing rather than as a hole in the panel.
pub const BACKGROUND: [u8; 4] = [30, 33, 39, 255];

/// Where the light comes from. Over the viewer's left shoulder, which is
/// where it has come from in every product shot ever taken.
const LIGHT: Vec3 = Vec3::new(-0.5, -0.6, 0.62);

/// Render a model into a square image, framed on itself.
///
/// `yaw` and `pitch` are degrees around the model, so a caller can spin it.
/// The camera distance comes from the model's own size, which is what makes
/// one call work for a doorframe and a teacup.
pub fn model(model: &Model, size: usize, yaw: f32, pitch: f32) -> Image {
    let mut image = Image::new(size, size, BACKGROUND);
    if size == 0 || model.indices.len() < 3 { return image }

    let bounds = model.bounds;
    let centre = bounds.center();
    let radius = (bounds.size().length() * 0.5).max(1.0);

    // Far enough back that the whole thing fits, with a margin so it does not
    // touch the edges of its box.
    let angles = Angles::new(pitch, yaw, 0.0);
    let basis = angles.vectors();
    let eye = centre - basis.forward * (radius * 2.6);

    let half = size as f32 * 0.5;
    // A fixed field of view; the distance above does the framing.
    let focal = half / (35.0f32.to_radians().tan());

    let mut depth = vec![f32::NEG_INFINITY; size * size];
    let light = LIGHT.normalize_or_zero();

    for triangle in model.indices.chunks_exact(3) {
        let corners: Vec<Vec3> = triangle
            .iter()
            .map(|i| Vec3::from_array(model.vertices[*i as usize].position))
            .collect();

        // A flat normal from the winding, rather than the vertex normals: a
        // preview wants the shape read clearly, and per-vertex smoothing on a
        // 60-pixel image mostly reads as mud.
        //
        // Negated because `.voidmdl` stores triangles clockwise as seen from
        // the front, the same way `.voidmap` faces are. Taking the cross
        // product at face value points every normal into the model, which
        // renders its inside: a crate came out as a shapeless lump, because
        // what you were looking at was the far wall of the inside of it.
        let normal =
            -(corners[1] - corners[0]).cross(corners[2] - corners[0]).normalize_or_zero();
        let shade = 0.35 + 0.65 * normal.dot(light).max(0.0);

        // Back-face culling in world space rather than by the sign of the
        // screen-space area: it does not depend on which way the projection
        // happens to flip handedness, so it stays right if the camera does.
        if normal.dot(eye - corners[0]) <= 0.0 { continue }

        let projected: Vec<[f32; 3]> = corners
            .iter()
            .map(|p| {
                let local = *p - eye;
                let z = local.dot(basis.forward);
                // Everything is in front: the camera was placed outside the
                // model's own bounding sphere.
                let inv = 1.0 / z.max(0.001);
                [
                    half + local.dot(basis.right) * focal * inv,
                    half - local.dot(basis.up) * focal * inv,
                    inv,
                ]
            })
            .collect();

        fill(&mut image, &mut depth, &projected, shade);
    }
    image
}

/// Fill one projected triangle, nearest-wins.
///
/// Draws either screen winding: which way round a triangle comes out depends
/// on the projection, and deciding *facing* from that would tie this to one
/// camera convention. Facing is settled before we get here.
fn fill(image: &mut Image, depth: &mut [f32], p: &[[f32; 3]], shade: f32) {
    let area = edge(p[0], p[1], [p[2][0], p[2][1]]);
    if area == 0.0 { return }
    let sign = area.signum();
    let area = area.abs();

    let min_x = p.iter().map(|v| v[0]).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
    let max_x = (p.iter().map(|v| v[0]).fold(f32::MIN, f32::max).ceil() as usize).min(image.width);
    let min_y = p.iter().map(|v| v[1]).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
    let max_y = (p.iter().map(|v| v[1]).fold(f32::MIN, f32::max).ceil() as usize).min(image.height);

    for y in min_y..max_y {
        for x in min_x..max_x {
            let at = [x as f32 + 0.5, y as f32 + 0.5];
            let w0 = edge(p[1], p[2], at) * sign;
            let w1 = edge(p[2], p[0], at) * sign;
            let w2 = edge(p[0], p[1], at) * sign;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 { continue }

            let inv_z = (w0 * p[0][2] + w1 * p[1][2] + w2 * p[2][2]) / area;
            let index = y * image.width + x;
            if inv_z <= depth[index] { continue }
            depth[index] = inv_z;

            let value = (200.0 * shade) as u8;
            image.pixels[index] = [value, value, (value as f32 * 1.06).min(255.0) as u8, 255];
        }
    }
}

fn edge(a: [f32; 3], b: [f32; 3], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

#[cfg(test)]
mod tests;
