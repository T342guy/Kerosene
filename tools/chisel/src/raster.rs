// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
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
use crate::textures::{Texture, TextureCache};
use std::sync::Arc;
use kerosene_math::{Basis, Vec3};

/// An RGBA image, ready to hand to egui.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[u8; 4]>,
}

impl Image {
    /// A blank image. Public so a second rasteriser -- the model previewer
    /// -- can produce the same kind of picture without a copy of this.
    pub fn new(width: usize, height: usize, fill: [u8; 4]) -> Image {
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

/// The darkest a face gets.
///
/// High, on purpose. The shading here exists to separate three faces meeting
/// at a corner, not to light a scene -- and the moment textures went on, a
/// realistic falloff made every north-facing wall too dark to read the
/// texture off. An editor preview that looks moody is one you cannot work in.
const AMBIENT: f32 = 0.62;

/// Flat shading for a face, in `AMBIENT..=1.0`.
fn shading_for(normal: Vec3) -> f32 {
    let facing = (normal.dot(LIGHT.normalize()) * 0.5 + 0.5).clamp(0.0, 1.0);
    AMBIENT + (1.0 - AMBIENT) * facing
}

/// How solid a face is drawn, given its material.
///
/// Tool brushes are see-through, because that is what they are: a trigger
/// volume is a region, not a wall, and one drawn opaque hides the room it is
/// meant to be sitting in. Drawing them solid made a level with triggers in it
/// impossible to work in -- which is the same reason Hammer draws them this
/// way.
///
/// `nodraw` is the exception among tool materials: it *is* a wall, it is just
/// one nobody sees, so it stays solid.
pub fn opacity_for(material: &str) -> f32 {
    let lower = material.to_ascii_lowercase();
    let Some(tool) = lower.strip_prefix("tools/") else { return 1.0 };
    match tool {
        // Solid geometry that happens not to be drawn.
        "nodraw" | "invisible" | "skybox" | "sky" => 1.0,
        // Everything else is a volume: clip, trigger, hint, skip, water.
        _ => 0.45,
    }
}

/// How the 3D pane draws faces.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Shading {
    /// The material's own texture, shaded by facing.
    #[default]
    Textured,
    /// Flat colours from the material's average, shaded by facing. What the
    /// pane looked like before textures, and still the fastest way to read
    /// shape when a texture is busy.
    Flat,
    /// Untextured grey, so geometry is all that is visible. For hunting a
    /// leak or a brush in the wrong place.
    Shaded,
    /// Each face coloured by its walkmap rule -- allow, deny, avoid, always --
    /// so a designer can read where NPCs may go without compiling.
    Walkmap,
}

impl Shading {
    pub fn label(self) -> &'static str {
        match self {
            Shading::Textured => "textured",
            Shading::Flat => "flat colour",
            Shading::Shaded => "shaded only",
            Shading::Walkmap => "walkmap",
        }
    }

    pub fn all() -> [Shading; 4] {
        [Shading::Textured, Shading::Flat, Shading::Shaded, Shading::Walkmap]
    }
}

/// Everything the rasteriser needs beyond the document.
///
/// Textures arrive through a closure rather than a cache and a filesystem,
/// so this file knows nothing about where content lives -- and so a test can
/// hand it a two-by-two checkerboard without building a content tree first.
pub struct Settings<'a> {
    pub shading: Shading,
    /// Material name to texture. `None` draws as [`Shading::Shaded`].
    pub resolve: Option<&'a mut dyn FnMut(&str) -> Option<Arc<Texture>>>,
}

impl Settings<'_> {
    /// Untextured grey. What the pane looked like before textures existed.
    pub fn shaded_only() -> Settings<'static> {
        Settings { shading: Shading::Shaded, resolve: None }
    }
}

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
    render_with(document, eye, basis, fov, width, height, &mut Settings::shaded_only())
}

/// Draw a document, with textures if there are any.
pub fn render_with(
    document: &Document,
    eye: Vec3,
    basis: Basis,
    fov: f32,
    width: usize,
    height: usize,
    settings: &mut Settings<'_>,
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
    let half_y = (kerosene_render::vertical_fov(fov, aspect) * 0.5).tan().max(1e-4);
    let half_x = half_y * aspect;

    let project = |camera: Vec3| -> [f32; 3] {
        let inv_z = 1.0 / camera.z;
        [
            ((camera.x / (camera.z * half_x)) * 0.5 + 0.5) * width as f32,
            (0.5 - (camera.y / (camera.z * half_y)) * 0.5) * height as f32,
            inv_z,
        ]
    };

    let mut faces = draw::visible_faces(document, eye, basis);
    // A see-through face has to be drawn over what is behind it, so the solid
    // world goes down first and the volumes on top. Within each group the
    // depth buffer still decides; this only orders the two passes.
    faces.sort_by(|a, b| {
        let (a_solid, b_solid) = (opacity_for(&a.material) >= 1.0, opacity_for(&b.material) >= 1.0);
        b_solid.cmp(&a_solid).then(b.depth.total_cmp(&a.depth))
    });

    for (index, face) in faces.into_iter().enumerate() {
        let shade = shading_for(face.normal);
        let opacity = opacity_for(&face.material);

        // A selected face is tinted rather than replaced, so what it is
        // textured with is still readable while it is being worked on.
        let tint = if face.face_selected || face.selected {
            Some(colors::SELECTED)
        } else {
            None
        };

        let resolved = match (&mut settings.resolve, settings.shading) {
            (Some(resolve), Shading::Textured | Shading::Flat) => resolve(&face.material),
            _ => None,
        };
        // The average, or a colour derived from the name when there is no
        // texture -- so a material that has not been compiled yet is a wrong
        // colour rather than a black hole, and two of them are two colours.
        // The walkmap view ignores materials entirely: it colours by rule.
        let flat = match settings.shading {
            Shading::Walkmap => {
                let c = colors::walkmap(face.walkmap);
                [c.r(), c.g(), c.b()]
            }
            _ => match (&resolved, settings.shading) {
                (Some(texture), _) => texture.average,
                (None, Shading::Textured | Shading::Flat) => {
                    TextureCache::fallback_colour(&face.material)
                }
                _ => [colors::BRUSH.r(), colors::BRUSH.g(), colors::BRUSH.b()],
            },
        };
        // Flat mode wants the average, not the pixels.
        let texture = (settings.shading == Shading::Textured).then_some(resolved).flatten();

        let vertices: Vec<Vertex> = face
            .polygon
            .iter()
            .map(|v| {
                let screen = project(v.position);
                let (w, h) = match &texture {
                    Some(t) => (t.width() as f32, t.height() as f32),
                    None => (1.0, 1.0),
                };
                Vertex { screen, uv: (v.texel.0 / w, v.texel.1 / h) }
            })
            .collect();

        let surface = Surface { texture: texture.clone(), flat, shade, tint, opacity };
        // A convex polygon fans from any vertex.
        for i in 1..vertices.len().saturating_sub(1) {
            triangle(
                &mut image,
                &mut depth,
                &mut face_at,
                [vertices[0], vertices[i], vertices[i + 1]],
                &surface,
                index as u32 + 1,
            );
        }
    }

    outline(&mut image, &face_at);
    markers(document, &mut image, &mut depth, eye, basis, project);
    image
}

/// A vertex ready to rasterise.
#[derive(Clone, Copy, Debug)]
struct Vertex {
    /// `[x, y, 1/z]` in pixels.
    screen: [f32; 3],
    /// Normalised texture coordinate, before the perspective divide.
    uv: (f32, f32),
}

/// What to fill a triangle with.
struct Surface {
    texture: Option<Arc<crate::textures::Texture>>,
    /// Used when there is no texture, and as the tint's base.
    flat: [u8; 3],
    /// Flat shading from the face normal.
    shade: f32,
    /// Mixed in over the top, for a selection.
    tint: Option<egui::Color32>,
    /// 1.0 for world geometry, less for a tool volume.
    opacity: f32,
}

impl Surface {
    fn is_opaque(&self) -> bool { self.opacity >= 1.0 }

    fn finish(&self, colour: [u8; 3]) -> [u8; 4] {
        let mut out = [
            (colour[0] as f32 * self.shade) as u8,
            (colour[1] as f32 * self.shade) as u8,
            (colour[2] as f32 * self.shade) as u8,
            255,
        ];
        if let Some(tint) = self.tint {
            for c in 0..3 {
                let target = [tint.r(), tint.g(), tint.b()][c] as f32 * self.shade;
                out[c] = (out[c] as f32 * 0.45 + target * 0.55) as u8;
            }
        }
        out
    }
}

/// Fill one triangle, depth-testing every pixel.
fn triangle(
    image: &mut Image,
    depth: &mut [f32],
    face_at: &mut [u32],
    mut v: [Vertex; 3],
    surface: &Surface,
    face: u32,
) {
    if v.iter().any(|p| p.screen.iter().any(|c| !c.is_finite())) { return }

    let mut area = edge(v[0].screen, v[1].screen, [v[2].screen[0], v[2].screen[1]]);
    if area == 0.0 { return }
    // Both windings arrive here -- clipping does not preserve one -- so flip
    // rather than cull. Facing was already decided in world space.
    if area < 0.0 {
        v.swap(1, 2);
        area = -area;
    }

    let (w, h) = (image.width as f32, image.height as f32);
    let min_x = v.iter().fold(f32::MAX, |a, p| a.min(p.screen[0])).floor().max(0.0) as usize;
    let max_x =
        (v.iter().fold(f32::MIN, |a, p| a.max(p.screen[0])).ceil().min(w - 1.0)).max(0.0) as usize;
    let min_y = v.iter().fold(f32::MAX, |a, p| a.min(p.screen[1])).floor().max(0.0) as usize;
    let max_y =
        (v.iter().fold(f32::MIN, |a, p| a.max(p.screen[1])).ceil().min(h - 1.0)).max(0.0) as usize;
    if min_x > max_x || min_y > max_y { return }

    // Which mip to read, chosen once per triangle.
    //
    // Per pixel would be more correct and much slower; per triangle is what a
    // software rasteriser can afford, and a level's faces are large and flat
    // enough that the difference is not visible. Without any mip selection a
    // wall seen edge-on shimmers, which is far more distracting than a slightly
    // soft one.
    let mip = surface.texture.as_ref().map_or(0, |texture| {
        let (tw, th) = (texture.width() as f32, texture.height() as f32);
        let texel_area = triangle_area(
            (v[0].uv.0 * tw, v[0].uv.1 * th),
            (v[1].uv.0 * tw, v[1].uv.1 * th),
            (v[2].uv.0 * tw, v[2].uv.1 * th),
        );
        // `area` is twice the screen-space area; both are, so the ratio is
        // right and the factors of two cancel.
        let ratio = texel_area / area.max(1e-6);
        if !ratio.is_finite() || ratio <= 1.0 { 0 } else { (ratio.log2() * 0.5).round().max(0.0) as usize }
    });

    // Perspective-correct interpolation: u/z and v/z are linear in screen
    // space, u and v are not. Interpolating uv directly is the classic
    // affine-texturing swim, and on a floor running away from the camera it
    // is not subtle.
    let uv_over_z: [(f32, f32); 3] = [
        (v[0].uv.0 * v[0].screen[2], v[0].uv.1 * v[0].screen[2]),
        (v[1].uv.0 * v[1].screen[2], v[1].uv.1 * v[1].screen[2]),
        (v[2].uv.0 * v[2].screen[2], v[2].uv.1 * v[2].screen[2]),
    ];

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
            let w0 = edge(v[1].screen, v[2].screen, p);
            let w1 = edge(v[2].screen, v[0].screen, p);
            let w2 = edge(v[0].screen, v[1].screen, p);
            if w0 < -eps || w1 < -eps || w2 < -eps { continue }

            // 1/z is linear in screen space, so this interpolation is exact.
            let inv_z =
                (w0 * v[0].screen[2] + w1 * v[1].screen[2] + w2 * v[2].screen[2]) * inv_area;
            let at = y * image.width + x;
            if inv_z <= depth[at] { continue }
            // A see-through face does not take the depth buffer: two of them
            // overlapping should both show, and something further away behind
            // one of them must not be erased by it.
            if surface.is_opaque() {
                depth[at] = inv_z;
                face_at[at] = face;
            }

            let colour = match &surface.texture {
                Some(texture) if inv_z.abs() > 1e-12 => {
                    let u = (w0 * uv_over_z[0].0 + w1 * uv_over_z[1].0 + w2 * uv_over_z[2].0)
                        * inv_area
                        / inv_z;
                    let vv = (w0 * uv_over_z[0].1 + w1 * uv_over_z[1].1 + w2 * uv_over_z[2].1)
                        * inv_area
                        / inv_z;
                    let texel = texture.sample(u, vv, mip);
                    [texel[0], texel[1], texel[2]]
                }
                _ => surface.flat,
            };
            let painted = surface.finish(colour);
            image.pixels[at] = if surface.is_opaque() {
                painted
            } else {
                blend(image.pixels[at], painted, surface.opacity)
            };
        }
    }
}

/// Mix a colour over what is already there.
fn blend(under: [u8; 4], over: [u8; 4], alpha: f32) -> [u8; 4] {
    let alpha = alpha.clamp(0.0, 1.0);
    let mut out = [0u8; 4];
    for c in 0..3 {
        out[c] = (under[c] as f32 + (over[c] as f32 - under[c] as f32) * alpha) as u8;
    }
    out[3] = 255;
    out
}

/// Twice the area of a triangle in whatever space its points are in.
fn triangle_area(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs()
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

        // The same colour the 2D panes give this family of entity, so a
        // light is amber in every pane and picking it out of the 3D view does
        // not mean reading a label that is not there.
        let selected = document.selection.entities.contains(&entity.id);
        let c = if selected {
            colors::SELECTED
        } else {
            crate::icons::Kind::of(entity.classname()).colour()
        };
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
