// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
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

use crate::raster::{self, Image, TextureResolver};
use crate::textures::TextureCache;
use kerosene_asset::Model;
use kerosene_math::{Angles, Vec3};

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
    model_zoomed(model, size, yaw, pitch, 1.0, None)
}

/// [`model`], with the camera pulled in or pushed out by `zoom`: `1.0` is
/// the same framing `model` always used, greater than `1.0` moves in.
///
/// A thumbnail never needs anything but the default framing, but an
/// interactive viewer -- Loupe -- wants a scroll wheel to do something, and
/// this is the knob it turns.
///
/// `resolve` looks a mesh's material up to a texture, exactly as the 3D
/// pane's own [`crate::raster::Settings::resolve`] does. `None` draws every
/// mesh in its material's flat average colour -- what a thumbnail wants,
/// since loading a texture for every model in a scrolling list would be a
/// lot of decoding for a picture nobody is looking at closely.
pub fn model_zoomed(
    model: &Model,
    size: usize,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    mut resolve: Option<TextureResolver<'_>>,
) -> Image {
    let mut image = Image::new(size, size, BACKGROUND);
    if size == 0 || model.indices.len() < 3 {
        return image;
    }

    let bounds = model.bounds;
    let centre = bounds.center();
    let radius = (bounds.size().length() * 0.5).max(1.0);

    // Far enough back that the whole thing fits, with a margin so it does not
    // touch the edges of its box.
    let angles = Angles::new(pitch, yaw, 0.0);
    let basis = angles.vectors();
    let eye = centre - basis.forward * (radius * 2.6 / zoom.max(0.05));

    let half = size as f32 * 0.5;
    // A fixed field of view; the distance above does the framing.
    let focal = half / (35.0f32.to_radians().tan());

    let mut depth = vec![0.0f32; size * size];
    let mut face_at = vec![0u32; size * size];
    let light = LIGHT.normalize_or_zero();

    let project = |p: Vec3| -> [f32; 3] {
        let local = p - eye;
        let z = local.dot(basis.forward);
        // Everything is in front: the camera was placed outside the model's
        // own bounding sphere.
        let inv = 1.0 / z.max(0.001);
        [
            half + local.dot(basis.right) * focal * inv,
            half - local.dot(basis.up) * focal * inv,
            inv,
        ]
    };

    // Every mesh in one flat list of `(material, first, end)` -- meshless
    // models (nothing Forge has compiled, only test fixtures) draw as one
    // untextured mesh spanning every index, rather than not drawing at all.
    let ranges: Vec<(&str, usize, usize)> = if model.meshes.is_empty() {
        vec![("", 0, model.indices.len())]
    } else {
        model
            .meshes
            .iter()
            .enumerate()
            .map(|(i, mesh)| {
                let start = mesh.first_index as usize;
                (
                    model.mesh_material(i),
                    start,
                    start + mesh.index_count as usize,
                )
            })
            .collect()
    };

    for (material, start, end) in ranges {
        let texture = resolve.as_deref_mut().and_then(|resolve| resolve(material));
        let flat = texture
            .as_ref()
            .map_or_else(|| TextureCache::fallback_colour(material), |t| t.average);

        let Some(indices) = model.indices.get(start..end) else {
            continue;
        };
        for triangle_indices in indices.as_chunks::<3>().0 {
            let corners: [Vec3; 3] = std::array::from_fn(|i| {
                Vec3::from_array(model.vertices[triangle_indices[i] as usize].position)
            });

            // A flat normal from the winding, rather than the vertex normals:
            // a preview wants the shape read clearly, and per-vertex
            // smoothing on a small image mostly reads as mud.
            //
            // `.keromdl` stores triangles counter-clockwise as seen from the
            // front -- the same convention the GPU renderer culls by -- so
            // the raw cross product already points out of the model.
            let normal = (corners[1] - corners[0])
                .cross(corners[2] - corners[0])
                .normalize_or_zero();

            // Back-face culling in world space rather than by the sign of the
            // screen-space area: it does not depend on which way the
            // projection happens to flip handedness, so it stays right if
            // the camera does.
            if normal.dot(eye - corners[0]) <= 0.0 {
                continue;
            }

            let shade = if resolve.is_some() {
                raster::shading_for(normal)
            } else {
                0.35 + 0.65 * normal.dot(light).max(0.0)
            };

            let vertices: [raster::Vertex; 3] = std::array::from_fn(|i| raster::Vertex {
                screen: project(corners[i]),
                uv: {
                    let uv = model.vertices[triangle_indices[i] as usize].uv;
                    (uv[0], uv[1])
                },
            });
            let surface = raster::Surface {
                texture: texture.clone(),
                flat,
                shade,
                tint: None,
                opacity: 1.0,
            };
            raster::triangle(&mut image, &mut depth, &mut face_at, vertices, &surface, 0);
        }
    }
    image
}

#[cfg(test)]
mod tests;
