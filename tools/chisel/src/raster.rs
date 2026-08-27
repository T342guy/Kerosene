// SPDX-License-Identifier: LGPL-3.0-or-later
//! A small software rasteriser for the 3D pane.
//!
//! The 3D view used to be drawn with egui's painter: polygons sorted back to
//! front and filled in that order. Painter's algorithm has no correct sort for
//! general geometry, and a level is exactly the case that breaks it -- a long
//! floor and a short wall have no ordering that is right for every pixel, so
//! walls show through each other from some angles and not others. Sorting by
//! average depth fails one way, sorting by farthest vertex fails another, and
//! neither is a bug that can be fixed by choosing a better key.
//!
//! It also produced spikes and stray lines: a polygon clipped exactly on the
//! near plane can carry duplicate vertices, and a vertex sitting at the near
//! plane far off to one side projects hundreds of thousands of pixels away.
//! Anti-aliased stroke tessellation does something spectacular with both.
//!
//! So the pane is rasterised here instead, with a depth buffer, and handed to
//! egui as an image. Occlusion becomes a per-pixel comparison, which is right
//! by construction; degenerate triangles have zero area and are skipped; and
//! every coordinate is clamped to the raster, so nothing can escape the pane.
//! A level editor's 3D pane is a few hundred polygons over a couple of hundred
//! thousand pixels, and the result is cached until something moves.

use crate::document::Document;
use crate::draw::{self, colors};
use void_math::{Basis, Vec3};

/// An RGBA image, ready to hand to egui.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[u8; 4]>,
}

impl Image {
    fn new(width: usize, height: usize, fill: [u8; 4]) -> Image {
        Image { width, height, pixels: vec![fill; width * height] }
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        self.pixels[y * self.width + x]
    }

    /// How many pixels are not the background colour.
    pub fn covered(&self) -> usize {
        let background = background_rgba();
        self.pixels.iter().filter(|p| **p != background).count()
    }
}

fn background_rgba() -> [u8; 4] {
    let c = colors::BACKGROUND;
    [c.r(), c.g(), c.b(), 255]
}

/// The direction the flat shading comes from. Not physical, just enough for
/// three faces meeting at a corner to be three different shades.
const LIGHT: Vec3 = Vec3::new(0.4, 0.3, 0.87);

/// Draw a document from a viewpoint.
///
/// `fov` is the horizontal field of view in degrees, as the viewport stores it.
pub fn render(
    document: &Document,
    eye: Vec3,
    basis: Basis,
    fov: f32,
    width: usize,
    height: usize,
) -> Image {
    let mut image = Image::new(width.max(1), height.max(1), background_rgba());
    if width == 0 || height == 0 { return image }

    // 1/z, not z: it interpolates linearly in screen space, which is what
    // makes a depth buffer correct across a perspective projection. Zero is
    // infinitely far away, so an untouched pixel loses to everything.
    let mut depth = vec![0.0f32; image.pixels.len()];
    // Which face won each pixel, offset by one. Outlines fall out of this for
    // free, and they are then occluded correctly because the buffer already is.
    let mut face_at = vec![0u32; image.pixels.len()];

    let aspect = width as f32 / height as f32;
    let half_y = (void_render::vertical_fov(fov, aspect) * 0.5).tan().max(1e-4);
    let half_x = half_y * aspect;

    let project = |camera: Vec3| -> [f32; 3] {
        let inv_z = 1.0 / camera.z;
        [
            ((camera.x / (camera.z * half_x)) * 0.5 + 0.5) * width as f32,
            (0.5 - (camera.y / (camera.z * half_y)) * 0.5) * height as f32,
            inv_z,
        ]
    };

    for (index, face) in draw::visible_faces(document, eye, basis).into_iter().enumerate() {
        let shade = (face.normal.dot(LIGHT.normalize()) * 0.5 + 0.5).clamp(0.25, 1.0);
        let base = if face.selected { colors::SELECTED } else { colors::BRUSH };
        let color = [
            (base.r() as f32 * shade) as u8,
            (base.g() as f32 * shade) as u8,
            (base.b() as f32 * shade) as u8,
            255,
        ];

        let screen: Vec<[f32; 3]> = face.polygon.iter().map(|p| project(*p)).collect();
        // A convex polygon fans from any vertex.
        for i in 1..screen.len().saturating_sub(1) {
            triangle(
                &mut image,
                &mut depth,
                &mut face_at,
                [screen[0], screen[i], screen[i + 1]],
                color,
                index as u32 + 1,
            );
        }
    }

    outline(&mut image, &face_at);
    markers(document, &mut image, &mut depth, eye, basis, project);
    image
}

/// Fill one triangle, depth-testing every pixel.
fn triangle(
    image: &mut Image,
    depth: &mut [f32],
    face_at: &mut [u32],
    mut v: [[f32; 3]; 3],
    color: [u8; 4],
    face: u32,
) {
    if v.iter().any(|p| !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite()) { return }

    let mut area = edge(v[0], v[1], [v[2][0], v[2][1]]);
    if area == 0.0 { return }
    // Both windings arrive here -- clipping does not preserve one -- so flip
    // rather than cull. Facing was already decided in world space.
    if area < 0.0 {
        v.swap(1, 2);
        area = -area;
    }

    let (w, h) = (image.width as f32, image.height as f32);
    let min_x = v.iter().fold(f32::MAX, |a, p| a.min(p[0])).floor().max(0.0) as usize;
    let max_x = (v.iter().fold(f32::MIN, |a, p| a.max(p[0])).ceil().min(w - 1.0)).max(0.0) as usize;
    let min_y = v.iter().fold(f32::MAX, |a, p| a.min(p[1])).floor().max(0.0) as usize;
    let max_y = (v.iter().fold(f32::MIN, |a, p| a.max(p[1])).ceil().min(h - 1.0)).max(0.0) as usize;
    if min_x > max_x || min_y > max_y { return }

    let inv_area = 1.0 / area;
    // Two triangles sharing an edge must not both miss a pixel that sits on
    // it. Rounding differs between the two evaluations of the same edge, so a
    // strict test leaves the occasional one-pixel hole -- a pinprick of
    // background in the middle of a solid wall. Erring the other way is free:
    // a pixel covered twice is settled by the depth test.
    let eps = (area * 1e-5).max(1e-3);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let w0 = edge(v[1], v[2], p);
            let w1 = edge(v[2], v[0], p);
            let w2 = edge(v[0], v[1], p);
            if w0 < -eps || w1 < -eps || w2 < -eps { continue }

            // 1/z is linear in screen space, so this interpolation is exact.
            let inv_z = (w0 * v[0][2] + w1 * v[1][2] + w2 * v[2][2]) * inv_area;
            let at = y * image.width + x;
            if inv_z <= depth[at] { continue }
            depth[at] = inv_z;
            face_at[at] = face;
            image.pixels[at] = color;
        }
    }
}

fn edge(a: [f32; 3], b: [f32; 3], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// Darken the pixel where one face gives way to another.
///
/// Reading the edges back out of the face buffer rather than drawing them
/// means they are occluded by whatever occluded the face -- an outline can
/// never show through a wall, which is the other half of the bug this file
/// replaced.
fn outline(image: &mut Image, face_at: &[u32]) {
    let (w, h) = (image.width, image.height);
    for y in 0..h {
        for x in 0..w {
            let at = y * w + x;
            let here = face_at[at];
            // Only darken geometry: an edge against the background would draw
            // a halo outside the shape.
            if here == 0 { continue }
            let right = if x + 1 < w { face_at[at + 1] } else { here };
            let below = if y + 1 < h { face_at[at + w] } else { here };
            if here == right && here == below { continue }
            // Safe to do in place: the decision reads `face_at`, which this
            // does not touch.
            let p = &mut image.pixels[at];
            for c in 0..3 {
                p[c] = (p[c] as f32 * 0.55) as u8;
            }
        }
    }
}

/// Point entities, as small markers that respect the depth buffer.
fn markers(
    document: &Document,
    image: &mut Image,
    depth: &mut [f32],
    eye: Vec3,
    basis: Basis,
    project: impl Fn(Vec3) -> [f32; 3],
) {
    const RADIUS: i64 = 4;
    for entity in document.map.entities.iter().filter(|e| e.solids.is_empty()) {
        let camera = draw::to_camera_space(&[entity.origin()], eye, basis);
        if camera[0].z < draw::NEAR { continue }
        let p = project(camera[0]);
        if !p[0].is_finite() || !p[1].is_finite() { continue }

        let selected = document.selection.entities.contains(&entity.id);
        let c = if selected { colors::SELECTED } else { colors::ENTITY };
        let color = [c.r(), c.g(), c.b(), 255];

        let (cx, cy) = (p[0] as i64, p[1] as i64);
        for dy in -RADIUS..=RADIUS {
            for dx in -RADIUS..=RADIUS {
                // A hollow square: a filled one hides what it is marking.
                if dx.abs() != RADIUS && dy.abs() != RADIUS { continue }
                let (x, y) = (cx + dx, cy + dy);
                if x < 0 || y < 0 || x >= image.width as i64 || y >= image.height as i64 { continue }
                let at = y as usize * image.width + x as usize;
                if p[2] < depth[at] { continue }
                depth[at] = p[2];
                image.pixels[at] = color;
            }
        }
    }
}

#[cfg(test)]
mod tests;
