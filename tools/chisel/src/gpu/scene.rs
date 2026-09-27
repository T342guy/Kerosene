// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! What the GPU 3D pane draws, as plain vertex lists.
//!
//! Everything here is CPU work with no device in sight, so "what does the
//! pane show" is a question the tests can ask without a GPU: which faces
//! went into which batch, what colour a selected brush is, whether a prop
//! came out as its model or as a marker box. The renderer next door only
//! uploads what this produces.
//!
//! The scene is in world space and does not depend on the camera. Flying
//! around changes one uniform; only an edit, a selection or a shading
//! change builds a new scene.

use crate::document::Document;
use crate::draw::{self, colors};
use crate::helpers::Helper;
use crate::raster::{Shading, opacity_for, shading_for};
use crate::textures::{Texture, TextureCache};
use bytemuck::{Pod, Zeroable};
use egui::Color32;
use kerosene_asset::Model;
use kerosene_math::{Aabb, Vec3};
use std::collections::HashMap;
use std::sync::Arc;

/// One corner of a filled triangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    /// World-space normal. Zero means "not shaded": helper volumes glow
    /// evenly instead of catching the light.
    pub normal: [f32; 3],
    /// Normalised texture coordinate.
    pub uv: [f32; 2],
    /// Multiplies the texture, linear RGB and opacity.
    pub color: [f32; 4],
    /// Mixed over the result by its alpha: how a selection is shown
    /// without hiding what the face is textured with.
    pub tint: [f32; 4],
}

/// One end of a line segment.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct LineVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

/// A run of triangles that share a texture and a blend mode.
#[derive(Clone, Debug)]
pub struct Batch {
    /// The material name the texture came from, `None` for untextured.
    pub texture: Option<String>,
    /// Vertex range in [`Scene::triangles`].
    pub first: u32,
    pub count: u32,
    /// Drawn after everything solid, blended, without writing depth.
    pub translucent: bool,
    /// Whether back faces are drawn. Brush faces are wound consistently
    /// and culled; a model's author may not have been so careful.
    pub two_sided: bool,
}

/// Everything the 3D pane draws, in world space.
#[derive(Default)]
pub struct Scene {
    pub triangles: Vec<Vertex>,
    pub batches: Vec<Batch>,
    /// Depth-tested lines: brush edges, entity boxes, helpers.
    pub lines: Vec<LineVertex>,
    /// Lines drawn through everything, faintly: the selection, so a brush
    /// selected behind a wall can still be found.
    pub xray: Vec<LineVertex>,
    /// The textures the batches name, for the renderer to upload.
    pub textures: HashMap<String, Arc<Texture>>,
}

impl Scene {
    /// How many triangles, for tests and the status bar.
    pub fn triangle_count(&self) -> usize {
        self.triangles.len() / 3
    }
}

/// Looks a material name up.
pub type Textures<'a> = &'a mut dyn FnMut(&str) -> Option<Arc<Texture>>;
/// Looks a model path up.
pub type Models<'a> = &'a mut dyn FnMut(&str) -> Option<Arc<Model>>;

/// What the scene needs besides the document.
pub struct Options<'a> {
    pub shading: Shading,
    pub textures: Option<Textures<'a>>,
    pub models: Option<Models<'a>>,
}

/// Half the size of the box a point entity without a model is drawn as.
pub const MARKER_HALF: f32 = 8.0;

/// sRGB bytes to linear floats, alpha passed through.
pub fn linear(c: Color32) -> [f32; 4] {
    fn channel(v: u8) -> f32 {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    [
        channel(c.r()),
        channel(c.g()),
        channel(c.b()),
        c.a() as f32 / 255.0,
    ]
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// How strongly a selected face is tinted towards the selection colour.
const SELECTED_TINT: f32 = 0.45;

/// What decides a batch: texture, translucent, two-sided.
type BatchKey = (Option<String>, bool, bool);

/// Collects triangles by batch key while the scene is built.
#[derive(Default)]
struct Builder {
    groups: Vec<(BatchKey, Vec<Vertex>)>,
    index: HashMap<BatchKey, usize>,
    scene: Scene,
}

impl Builder {
    fn triangles(
        &mut self,
        texture: Option<&str>,
        translucent: bool,
        two_sided: bool,
    ) -> &mut Vec<Vertex> {
        let key = (texture.map(str::to_string), translucent, two_sided);
        let at = match self.index.get(&key) {
            Some(&at) => at,
            None => {
                self.groups.push((key.clone(), Vec::new()));
                self.index.insert(key, self.groups.len() - 1);
                self.groups.len() - 1
            }
        };
        &mut self.groups[at].1
    }

    fn line(&mut self, a: Vec3, b: Vec3, color: Color32, xray: bool) {
        let color = linear(color);
        let list = if xray {
            &mut self.scene.xray
        } else {
            &mut self.scene.lines
        };
        list.push(LineVertex {
            position: a.to_array(),
            color,
        });
        list.push(LineVertex {
            position: b.to_array(),
            color,
        });
    }

    fn finish(mut self) -> Scene {
        // Solid first, then translucent, so the renderer can draw the batch
        // list in order and get the passes right.
        self.groups
            .sort_by_key(|((_, translucent, _), _)| *translucent);
        for ((texture, translucent, two_sided), vertices) in self.groups {
            if vertices.is_empty() {
                continue;
            }
            let first = self.scene.triangles.len() as u32;
            self.scene.triangles.extend_from_slice(&vertices);
            self.scene.batches.push(Batch {
                texture,
                first,
                count: vertices.len() as u32,
                translucent,
                two_sided,
            });
        }
        self.scene
    }
}

/// A polygon wound so that it faces along `normal` counter-clockwise.
fn wound(points: &[Vec3], normal: Vec3) -> Vec<Vec3> {
    let mut points = points.to_vec();
    if points.len() >= 3 {
        let facing = (points[1] - points[0])
            .cross(points[2] - points[0])
            .dot(normal);
        if facing < 0.0 {
            points.reverse();
        }
    }
    points
}

/// Build the scene for a document.
pub fn build(document: &Document, helpers: &[Helper], options: &mut Options<'_>) -> Scene {
    let mut builder = Builder::default();

    // Brushes.
    for (entity, solid) in document.visible_solids() {
        let selected = document.selection.solids.contains(&solid.id)
            || document.selection.entities.contains(&entity.id);
        let brush_entity = entity.classname() != "worldspawn";
        for (side, winding) in solid.face_windings() {
            let Some(plane) = side.plane() else { continue };
            let face_selected = document.selection.faces.contains(&(solid.id, side.id));
            let points = wound(&winding.points, plane.normal);
            let uvs: Vec<(f32, f32)> = points.iter().map(|p| draw::texel_for(side, *p)).collect();
            face(
                &mut builder,
                &points,
                &uvs,
                plane.normal,
                &side.material,
                colors::walkmap(side.walkmap),
                selected || face_selected,
                options,
            );
            let edge = if selected {
                colors::SELECTED
            } else if brush_entity {
                colors::BRUSH_ENTITY.gamma_multiply(0.8)
            } else {
                Color32::from_rgb(12, 13, 15)
            };
            for i in 0..points.len() {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                builder.line(a, b, edge, false);
                if selected {
                    builder.line(a, b, colors::SELECTED.gamma_multiply(0.35), true);
                }
            }
        }
    }

    // Meshes, piece by piece, as Cleave compiles them.
    for mesh in document.visible_meshes() {
        let selected = document.selection.meshes.contains(&mesh.id);
        for mesh_face in &mesh.faces {
            for piece in mesh.face_pieces(mesh_face) {
                let normal = kerosene_map::polygon_normal(&piece);
                let points = wound(&piece, normal);
                let uvs: Vec<(f32, f32)> = points
                    .iter()
                    .map(|p| draw::texel_for_axes(&mesh_face.uaxis, &mesh_face.vaxis, *p))
                    .collect();
                face(
                    &mut builder,
                    &points,
                    &uvs,
                    normal,
                    &mesh_face.material,
                    colors::walkmap(mesh_face.walkmap),
                    selected,
                    options,
                );
            }
            let points = mesh.face_points(mesh_face);
            let edge = if selected {
                colors::SELECTED
            } else {
                colors::MESH.gamma_multiply(0.5)
            };
            for i in 0..points.len() {
                builder.line(points[i], points[(i + 1) % points.len()], edge, false);
            }
        }
    }

    // Helpers: models, cones, radii. A point entity whose model drew is not
    // also given a marker box.
    let mut modelled = std::collections::HashSet::new();
    for helper in helpers {
        match helper {
            Helper::Model {
                owner,
                path,
                pose,
                selected,
                opacity,
            } => {
                let Some(model) = options.models.as_mut().and_then(|m| m(path)) else {
                    continue;
                };
                model_triangles(&mut builder, &model, pose, *selected, *opacity, options);
                if let Some(owner) = owner {
                    modelled.insert(*owner);
                }
                if *selected {
                    bounds_lines(
                        &mut builder,
                        transformed_bounds(&model.bounds, pose),
                        colors::SELECTED,
                        true,
                    );
                }
            }
            Helper::Lines {
                segments,
                color,
                xray,
            } => {
                for [a, b] in segments {
                    builder.line(*a, *b, *color, *xray);
                }
            }
            Helper::Fill { triangles, color } => {
                let c = linear(*color);
                let out = builder.triangles(None, color.a() < 255, true);
                for triangle in triangles {
                    for p in triangle {
                        out.push(Vertex {
                            position: p.to_array(),
                            normal: [0.0; 3],
                            uv: [0.0; 2],
                            color: c,
                            tint: [0.0; 4],
                        });
                    }
                }
            }
        }
    }

    // Point entities with nothing better to show: a small box, coloured
    // like the family's icon in the 2D panes.
    for entity in document.visible_point_entities() {
        if modelled.contains(&entity.id) {
            continue;
        }
        let selected = document.selection.entities.contains(&entity.id);
        let colour = crate::icons::Kind::of(entity.classname()).colour();
        let bounds = Aabb::new(
            entity.origin() - Vec3::splat(MARKER_HALF),
            entity.origin() + Vec3::splat(MARKER_HALF),
        );
        box_triangles(&mut builder, bounds, colour, selected);
        bounds_lines(
            &mut builder,
            bounds,
            if selected {
                colors::SELECTED
            } else {
                colour.gamma_multiply(0.6)
            },
            false,
        );
        if selected {
            bounds_lines(
                &mut builder,
                bounds,
                colors::SELECTED.gamma_multiply(0.35),
                true,
            );
        }
    }

    let mut scene = builder.finish();
    // The textures the batches name, so the renderer has every one it needs
    // without a resolver of its own.
    if let Some(resolve) = options.textures.as_mut() {
        for batch in &scene.batches {
            if let Some(name) = &batch.texture
                && !scene.textures.contains_key(name)
                && let Some(texture) = resolve(name)
            {
                scene.textures.insert(name.clone(), texture);
            }
        }
    }
    scene
}

/// One brush or mesh face, fanned into triangles.
#[allow(clippy::too_many_arguments)]
fn face(
    builder: &mut Builder,
    points: &[Vec3],
    texels: &[(f32, f32)],
    normal: Vec3,
    material: &str,
    walkmap: Color32,
    selected: bool,
    options: &mut Options<'_>,
) {
    if points.len() < 3 {
        return;
    }
    let opacity = opacity_for(material);
    let resolved = match (&mut options.textures, options.shading) {
        (Some(resolve), Shading::Textured | Shading::Flat) => resolve(material),
        _ => None,
    };
    let base = match options.shading {
        Shading::Walkmap => walkmap,
        Shading::Textured | Shading::Flat => match &resolved {
            Some(texture) if options.shading == Shading::Flat => rgb(texture.average),
            Some(_) => Color32::WHITE,
            None => rgb(TextureCache::fallback_colour(material)),
        },
        Shading::Shaded => colors::BRUSH,
    };
    let texture = resolved.filter(|_| options.shading == Shading::Textured);
    let (w, h) = texture
        .as_ref()
        .map_or((1.0, 1.0), |t| (t.width() as f32, t.height() as f32));

    let mut color = linear(base);
    color[3] = opacity;
    let tint = if selected {
        let s = linear(colors::SELECTED);
        [s[0], s[1], s[2], SELECTED_TINT]
    } else {
        [0.0; 4]
    };

    let vertex = |i: usize| Vertex {
        position: points[i].to_array(),
        normal: normal.to_array(),
        uv: [texels[i].0 / w, texels[i].1 / h],
        color,
        tint,
    };
    let name = texture.as_ref().map(|_| material);
    let out = builder.triangles(name, opacity < 1.0, false);
    for i in 1..points.len() - 1 {
        out.extend([vertex(0), vertex(i), vertex(i + 1)]);
    }
}

/// A model's triangles, posed into the world.
fn model_triangles(
    builder: &mut Builder,
    model: &Model,
    pose: &kerosene_math::Pose,
    selected: bool,
    opacity: f32,
    options: &mut Options<'_>,
) {
    let tint = if selected {
        let s = linear(colors::SELECTED);
        [s[0], s[1], s[2], SELECTED_TINT]
    } else {
        [0.0; 4]
    };
    let rotation = pose.rotation();
    let meshes: Vec<(u32, u32, &str)> = if model.meshes.is_empty() {
        vec![(0, model.indices.len() as u32, "")]
    } else {
        (0..model.meshes.len())
            .map(|i| {
                let m = &model.meshes[i];
                (m.first_index, m.index_count, model.mesh_material(i))
            })
            .collect()
    };
    for (first, count, material) in meshes {
        let resolved = match (&mut options.textures, options.shading) {
            (Some(resolve), Shading::Textured | Shading::Flat) if !material.is_empty() => {
                resolve(material)
            }
            _ => None,
        };
        let base = match (&resolved, options.shading) {
            (Some(t), Shading::Flat) => rgb(t.average),
            (Some(_), Shading::Textured) => Color32::WHITE,
            _ => Color32::from_rgb(190, 196, 206),
        };
        let texture = resolved.filter(|_| options.shading == Shading::Textured);
        let mut color = linear(base);
        color[3] = opacity;
        let name = texture.as_ref().map(|_| material);
        let out = builder.triangles(name, opacity < 1.0, true);
        let end = (first + count).min(model.indices.len() as u32);
        for &index in &model.indices[first as usize..end as usize] {
            let Some(v) = model.vertices.get(index as usize) else {
                continue;
            };
            let position = pose.to_world(Vec3::from_array(v.position));
            let normal = (rotation * Vec3::from_array(v.normal)).normalize_or_zero();
            out.push(Vertex {
                position: position.to_array(),
                normal: normal.to_array(),
                uv: v.uv,
                color,
                tint,
            });
        }
    }
}

/// The world-space box around a posed local box.
pub fn transformed_bounds(bounds: &Aabb, pose: &kerosene_math::Pose) -> Aabb {
    let mut out = Aabb::EMPTY;
    for corner in corners(*bounds) {
        out.add_point(pose.to_world(corner));
    }
    out
}

/// A box's eight corners, indexed by bit: x is bit 0, y bit 1, z bit 2.
pub fn corners(b: Aabb) -> [Vec3; 8] {
    std::array::from_fn(|i| {
        Vec3::new(
            if i & 1 == 0 { b.min.x } else { b.max.x },
            if i & 2 == 0 { b.min.y } else { b.max.y },
            if i & 4 == 0 { b.min.z } else { b.max.z },
        )
    })
}

/// The twelve edges of a box, as pairs of corner indices.
pub const BOX_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

fn bounds_lines(builder: &mut Builder, b: Aabb, color: Color32, xray: bool) {
    let c = corners(b);
    for (i, j) in BOX_EDGES {
        builder.line(c[i], c[j], color, xray);
    }
}

fn box_triangles(builder: &mut Builder, b: Aabb, colour: Color32, selected: bool) {
    let c = corners(b);
    // Each face as four corner indices, counter-clockwise from outside.
    const FACES: [([usize; 4], Vec3); 6] = [
        ([0, 2, 3, 1], Vec3::NEG_Z),
        ([4, 5, 7, 6], Vec3::Z),
        ([0, 1, 5, 4], Vec3::NEG_Y),
        ([2, 6, 7, 3], Vec3::Y),
        ([0, 4, 6, 2], Vec3::NEG_X),
        ([1, 3, 7, 5], Vec3::X),
    ];
    let color = linear(colour);
    let tint = if selected {
        let s = linear(colors::SELECTED);
        [s[0], s[1], s[2], 0.6]
    } else {
        [0.0; 4]
    };
    let out = builder.triangles(None, false, false);
    for (quad, normal) in FACES {
        let v = |i: usize| Vertex {
            position: c[quad[i]].to_array(),
            normal: normal.to_array(),
            uv: [0.0; 2],
            color,
            tint,
        };
        out.extend([v(0), v(1), v(2), v(0), v(2), v(3)]);
    }
}

/// The shade a face gets, exactly as the software rasteriser computes it --
/// kept here so the shader and its test agree on one formula.
pub fn shade(normal: Vec3) -> f32 {
    shading_for(normal)
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod tests;
