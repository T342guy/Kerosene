// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! What an entity looks like in the viewports, beyond a marker.
//!
//! A prop is its model; a spot light is a cone pointing where it shines; a
//! sound is the sphere it can be heard in. Hammer calls these helpers, and
//! they are what makes a level readable without selecting everything in it
//! one by one to look at its keys.
//!
//! This file turns an entity into [`Helper`]s: plain geometry in world
//! space, with no idea how it will be drawn. The 3D pane fills and strokes
//! them; the 2D panes stroke their outlines.

use egui::Color32;
use kerosene_entity::schema::{ClassSpec, HelperKind, HelperSpec, KeyKind};
use kerosene_math::{Pose, Vec3};

/// One thing to draw for an entity.
#[derive(Clone, Debug, PartialEq)]
pub enum Helper {
    /// A model, posed in the world.
    Model {
        /// The entity it stands for: a point entity whose model drew is not
        /// given a marker box as well.
        owner: Option<u32>,
        path: String,
        pose: Pose,
        selected: bool,
        opacity: f32,
    },
    /// Line segments.
    Lines {
        segments: Vec<[Vec3; 2]>,
        color: Color32,
        /// Drawn through walls as well as in front of them.
        xray: bool,
    },
    /// A filled volume, usually translucent.
    Fill {
        triangles: Vec<[Vec3; 3]>,
        color: Color32,
    },
}

/// A model path as the content tree names it: `props/crate`, from any of
/// `props/crate`, `models/props/crate` or `models/props/crate.kmdl`.
pub fn model_name(path: &str) -> &str {
    let path = path.trim();
    let path = path.strip_prefix("models/").unwrap_or(path);
    path.strip_suffix(".kmdl").unwrap_or(path)
}

/// The model an entity shows: its own `model` key, or the class's default
/// for it -- what the game would spawn it with.
pub fn entity_model(entity: &kerosene_map::Entity, spec: Option<&ClassSpec>) -> Option<String> {
    let own = entity.get("model").filter(|m| !m.trim().is_empty());
    let default = || {
        spec?
            .key("model")
            .filter(|k| k.kind == KeyKind::Model)
            .map(|k| k.default.as_str())
            .filter(|d| !d.trim().is_empty())
    };
    own.or_else(default).map(|m| model_name(m).to_string())
}

/// Which helpers the panes draw beyond models.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HelperMode {
    /// Cones, radii and target lines for what is selected. Hammer's
    /// default, and the right one: a level with every light's sphere drawn
    /// at once is a level you cannot see.
    #[default]
    Selected,
    /// For every entity.
    All,
    /// Models only.
    None,
}

impl HelperMode {
    pub fn label(self) -> &'static str {
        match self {
            HelperMode::Selected => "helpers: selected",
            HelperMode::All => "helpers: all",
            HelperMode::None => "helpers: off",
        }
    }

    pub fn next(self) -> HelperMode {
        match self {
            HelperMode::Selected => HelperMode::All,
            HelperMode::All => HelperMode::None,
            HelperMode::None => HelperMode::Selected,
        }
    }
}

/// Where the entities with a name are, for target lines.
pub type Targets<'a> = &'a dyn Fn(&str) -> Vec<Vec3>;

/// Everything drawn for one entity.
///
/// `origin` is where it stands -- a brush entity's is the middle of its
/// brushes, which its own `origin` key does not say. `extras` is whether to
/// draw more than its model: the caller decides from [`HelperMode`].
pub fn for_entity(
    entity: &kerosene_map::Entity,
    spec: Option<&ClassSpec>,
    origin: Vec3,
    selected: bool,
    extras: bool,
    targets: Targets<'_>,
) -> Vec<Helper> {
    let read = Reader { entity, spec };
    let mut out = Vec::new();
    let declared: &[HelperSpec] = spec.map_or(&[], |s| s.helpers.as_slice());

    // The model: declared, or implied by a model key -- a game whose schema
    // predates helpers still gets its props drawn.
    let model_key = declared
        .iter()
        .find(|h| h.kind == HelperKind::Model)
        .map(|h| h.param("key").unwrap_or("model"));
    let model = match model_key {
        Some(key) if key != "model" => read
            .text(key)
            .filter(|m| !m.trim().is_empty())
            .map(|m| model_name(&m).to_string()),
        _ => entity_model(entity, spec),
    };
    if entity.solids.is_empty()
        && let Some(path) = model
    {
        out.push(Helper::Model {
            owner: Some(entity.id),
            path,
            pose: Pose::new(origin, entity.angles()),
            selected,
            opacity: 1.0,
        });
    }
    if !extras {
        return out;
    }

    let colour = if selected {
        crate::draw::colors::SELECTED
    } else {
        crate::icons::Kind::of(entity.classname()).colour()
    };
    let mut lined = Vec::new();
    for helper in declared {
        match helper.kind {
            HelperKind::Model => {}
            HelperKind::Direction => {
                let length = read.number_param(helper, "length", 48.0);
                let basis = entity.angles().vectors();
                out.push(arrow(origin, basis, length, colour));
            }
            HelperKind::LightRadius => {
                let light = read.light();
                let (bright, reach) = light.radii();
                let tint = light.colour.gamma_multiply(0.8);
                if bright > 0.0 {
                    out.push(sphere(origin, bright, tint));
                }
                if reach > bright {
                    out.push(sphere(origin, reach, tint.gamma_multiply(0.45)));
                }
            }
            HelperKind::LightCone => {
                let light = read.light();
                let outer = read.number("_cone", 0.0);
                if outer <= 0.0 {
                    continue;
                }
                let inner = read.number("_inner_cone", 0.0).clamp(0.0, outer);
                let length = light.radii().0.clamp(64.0, 1024.0);
                out.extend(cone(origin, read.aim(), outer, inner, length, light.colour));
            }
            HelperKind::Sphere => {
                let radius = read.number_param(helper, "radius", 0.0);
                if radius > 0.0 {
                    let tint = helper
                        .param("color")
                        .and_then(parse_colour)
                        .unwrap_or(colour);
                    out.push(sphere(origin, radius, tint));
                }
            }
            HelperKind::Frustum => {
                let fov = read.number_param(helper, "fov", 90.0);
                let length = read.number_param(helper, "length", 256.0);
                out.push(frustum(
                    origin,
                    entity.angles().vectors(),
                    fov,
                    length,
                    colour,
                ));
            }
            HelperKind::Line => {
                let key = helper.param("key").unwrap_or("target");
                lined.push(key.to_ascii_lowercase());
                out.extend(target_lines(origin, read.text(key), targets, colour));
            }
            HelperKind::Rect => {
                let width = read.number_param(helper, "width", 32.0);
                let height = read.number_param(helper, "height", 32.0);
                out.extend(rect(
                    origin,
                    entity.angles().vectors(),
                    width,
                    height,
                    colour,
                ));
            }
            // A helper kind newer than this editor draws nothing. Needed
            // across the crate boundary, where the kind is non-exhaustive;
            // unreachable where the bundle puts both in one crate.
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    // Every key that names another entity is a line to it, declared or not:
    // that is what the key means, and a schema should not have to say so.
    if let Some(spec) = spec {
        for key in &spec.keys {
            if key.kind == KeyKind::TargetDestination
                && !lined.contains(&key.name.to_ascii_lowercase())
            {
                out.extend(target_lines(origin, read.text(&key.name), targets, colour));
            }
        }
    }
    out
}

/// Keys read from an entity, falling back to the class's defaults.
struct Reader<'a> {
    entity: &'a kerosene_map::Entity,
    spec: Option<&'a ClassSpec>,
}

impl Reader<'_> {
    fn text(&self, key: &str) -> Option<String> {
        self.entity
            .get(key)
            .filter(|v| !v.trim().is_empty())
            .map(str::to_string)
            .or_else(|| {
                self.spec?
                    .key(key)
                    .map(|k| k.default.clone())
                    .filter(|d| !d.trim().is_empty())
            })
    }

    fn number(&self, key: &str, fallback: f32) -> f32 {
        self.text(key)
            .and_then(|v| v.split_whitespace().next()?.parse().ok())
            .unwrap_or(fallback)
    }

    /// A parameter that is a number, or the name of a key holding one.
    fn number_param(&self, helper: &HelperSpec, name: &str, fallback: f32) -> f32 {
        match helper.param(name) {
            Some(value) => match value.trim().parse::<f32>() {
                Ok(n) => n,
                Err(_) => self.number(value, fallback),
            },
            None => fallback,
        }
    }

    /// Where a light points: its angles, with the `pitch` key overriding
    /// the pitch as Radiance reads it (stored upward-positive).
    fn aim(&self) -> kerosene_math::Basis {
        let mut angles = self.entity.angles();
        let pitch = self.number("pitch", 0.0);
        if pitch != 0.0 {
            angles.pitch = -pitch;
        }
        angles.vectors()
    }

    fn light(&self) -> Light {
        let numbers: Vec<f32> = self
            .text("_light")
            .unwrap_or_else(|| "255 255 255 200".into())
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        let (rgb, brightness) = match numbers.as_slice() {
            [r, g, b, v, ..] => ([*r, *g, *b], *v),
            [r, g, b] => ([*r, *g, *b], 200.0),
            _ => ([255.0; 3], 200.0),
        };
        let byte = |v: f32| v.clamp(0.0, 255.0) as u8;
        Light {
            colour: Color32::from_rgb(byte(rgb[0]), byte(rgb[1]), byte(rgb[2])),
            peak: rgb.iter().copied().fold(0.0, f32::max) / 255.0 * brightness,
            attenuation: kerosene_math::light::Attenuation {
                constant: self.number("_constant_attn", 0.0),
                linear: self.number("_linear_attn", 0.0),
                quadratic: self.number("_quadratic_attn", 1.0),
            },
            limit: self.number("distance", 0.0),
        }
    }
}

/// What a light's keys add up to.
struct Light {
    colour: Color32,
    peak: f32,
    attenuation: kerosene_math::light::Attenuation,
    /// A `distance` cap, when the class has one. Zero is none.
    limit: f32,
}

/// How bright a surface has to be lit to count as "fully lit" for the
/// inner sphere: 255, in the units a light's brightness is written in --
/// Radiance divides by 255 on the way into the lightmap, so a light of
/// brightness 200 is just short of full at the 100-unit reference distance.
pub const FULLY_LIT: f32 = 255.0;
/// And for the outer sphere: about 3% of full, where a light stops being
/// something anyone would notice.
pub const STILL_LIT: f32 = 8.0;

impl Light {
    /// Where it lights a surface fully, and where it has all but faded -- the two spheres Hammer draws for a light's 50%
    /// and 0% distances, from the falloff Radiance and the renderer share.
    fn radii(&self) -> (f32, f32) {
        let cap = |r: f32| {
            if self.limit > 0.0 {
                r.min(self.limit)
            } else {
                r
            }
        };
        (
            cap(self.attenuation.range(self.peak, FULLY_LIT)),
            cap(self.attenuation.range(self.peak, STILL_LIT)),
        )
    }
}

fn parse_colour(text: &str) -> Option<Color32> {
    let n: Vec<u8> = text
        .split_whitespace()
        .filter_map(|t| t.parse::<f32>().ok())
        .map(|v| v.clamp(0.0, 255.0) as u8)
        .collect();
    match n.as_slice() {
        [r, g, b, ..] => Some(Color32::from_rgb(*r, *g, *b)),
        _ => None,
    }
}

/// How many segments a circle is drawn with.
const CIRCLE: usize = 32;

fn circle(centre: Vec3, a: Vec3, b: Vec3, radius: f32) -> Vec<Vec3> {
    (0..CIRCLE)
        .map(|i| {
            let t = i as f32 / CIRCLE as f32 * std::f32::consts::TAU;
            centre + (a * t.cos() + b * t.sin()) * radius
        })
        .collect()
}

fn loop_segments(points: &[Vec3]) -> impl Iterator<Item = [Vec3; 2]> + '_ {
    (0..points.len()).map(move |i| [points[i], points[(i + 1) % points.len()]])
}

/// Three great circles.
pub fn sphere(centre: Vec3, radius: f32, color: Color32) -> Helper {
    let mut segments = Vec::new();
    for (a, b) in [(Vec3::X, Vec3::Y), (Vec3::X, Vec3::Z), (Vec3::Y, Vec3::Z)] {
        segments.extend(loop_segments(&circle(centre, a, b, radius)));
    }
    Helper::Lines {
        segments,
        color,
        xray: false,
    }
}

/// An arrow from `origin` along `basis.forward`.
pub fn arrow(origin: Vec3, basis: kerosene_math::Basis, length: f32, color: Color32) -> Helper {
    let tip = origin + basis.forward * length;
    let head = length.min(64.0) * 0.25;
    let mut segments = vec![[origin, tip]];
    for side in [basis.right, -basis.right, basis.up, -basis.up] {
        segments.push([tip, tip - basis.forward * head + side * head * 0.5]);
    }
    Helper::Lines {
        segments,
        color,
        xray: false,
    }
}

/// A light's cone: a translucent outer cone, its rim, and the inner rim.
///
/// `outer` and `inner` are half-angles in degrees, as `_cone` is written.
pub fn cone(
    apex: Vec3,
    basis: kerosene_math::Basis,
    outer: f32,
    inner: f32,
    length: f32,
    colour: Color32,
) -> Vec<Helper> {
    let outer = outer.clamp(1.0, 89.0);
    let centre = apex + basis.forward * length;
    let rim = circle(
        centre,
        basis.right,
        basis.up,
        length * outer.to_radians().tan(),
    );
    let mut triangles = Vec::with_capacity(CIRCLE * 2);
    for [a, b] in loop_segments(&rim) {
        triangles.push([apex, a, b]);
        triangles.push([centre, b, a]);
    }
    let mut segments: Vec<[Vec3; 2]> = loop_segments(&rim).collect();
    for i in (0..CIRCLE).step_by(CIRCLE / 4) {
        segments.push([apex, rim[i]]);
    }
    let mut out = vec![
        Helper::Fill {
            triangles,
            color: Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), 36),
        },
        Helper::Lines {
            segments,
            color: colour,
            xray: false,
        },
    ];
    if inner > 0.0 && inner < outer {
        let inner_rim = circle(
            centre,
            basis.right,
            basis.up,
            length * inner.to_radians().tan(),
        );
        out.push(Helper::Lines {
            segments: loop_segments(&inner_rim).collect(),
            color: colour.gamma_multiply(0.6),
            xray: false,
        });
    }
    out
}

/// A camera's view: a pyramid with a 4:3 far rectangle.
pub fn frustum(
    origin: Vec3,
    basis: kerosene_math::Basis,
    fov: f32,
    length: f32,
    color: Color32,
) -> Helper {
    let half_x = length * (fov.clamp(1.0, 170.0) * 0.5).to_radians().tan();
    let half_y = half_x * 0.75;
    let centre = origin + basis.forward * length;
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .map(|(x, y)| centre + basis.right * (x * half_x) + basis.up * (y * half_y));
    let mut segments: Vec<[Vec3; 2]> = loop_segments(&corners).collect();
    segments.extend(corners.iter().map(|c| [origin, *c]));
    Helper::Lines {
        segments,
        color,
        xray: false,
    }
}

/// A flat rectangle facing along `basis.forward`: a panel's face.
pub fn rect(
    origin: Vec3,
    basis: kerosene_math::Basis,
    width: f32,
    height: f32,
    color: Color32,
) -> Vec<Helper> {
    let (w, h) = (width * 0.5, height * 0.5);
    // Its width runs along the entity's left, as a panel's does when seen
    // from the front.
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .map(|(x, y)| origin - basis.right * (x * w) + basis.up * (y * h));
    vec![
        Helper::Fill {
            triangles: vec![
                [corners[0], corners[1], corners[2]],
                [corners[0], corners[2], corners[3]],
            ],
            color: Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 40),
        },
        Helper::Lines {
            segments: loop_segments(&corners).collect(),
            color,
            xray: false,
        },
    ]
}

/// Where an entity stands: its origin, or for a brush entity the middle of
/// its brushes -- a door's `origin` key is its pivot, not where it is.
pub fn entity_centre(entity: &kerosene_map::Entity) -> Vec3 {
    if entity.solids.is_empty() {
        return entity.origin();
    }
    let mut bounds = kerosene_math::Aabb::EMPTY;
    for solid in &entity.solids {
        bounds = bounds.union(&solid.bounds());
    }
    if bounds.is_empty() {
        entity.origin()
    } else {
        bounds.center()
    }
}

/// Every named entity's position, by lower-cased name.
pub fn named_positions(
    entities: &[kerosene_map::Entity],
) -> std::collections::HashMap<String, Vec<Vec3>> {
    let mut out: std::collections::HashMap<String, Vec<Vec3>> = Default::default();
    for entity in entities {
        if let Some(name) = entity.targetname().filter(|n| !n.trim().is_empty()) {
            out.entry(name.trim().to_ascii_lowercase())
                .or_default()
                .push(entity_centre(entity));
        }
    }
    out
}

/// The positions a target name reaches. Matched exactly, ignoring case, as
/// the engine matches it -- Source's trailing `*` is not something the
/// engine does, and a line drawn to it would be a promise nothing keeps.
/// `!self`, `!activator` and the like name nothing fixed and reach nothing.
pub fn lookup(names: &std::collections::HashMap<String, Vec<Vec3>>, target: &str) -> Vec<Vec3> {
    let target = target.trim().to_ascii_lowercase();
    if target.is_empty() || target.starts_with('!') {
        return Vec::new();
    }
    names.get(&target).cloned().unwrap_or_default()
}

/// An arrow from `from` to `to`, its head a little short of `to` so it
/// does not vanish inside the marker it points at.
pub fn wire(from: Vec3, to: Vec3, color: Color32) -> Helper {
    let along = to - from;
    let length = along.length();
    let mut segments = vec![[from, to]];
    if length > 1.0 {
        let dir = along / length;
        let tip = to - dir * length.min(12.0);
        let side = if dir.z.abs() < 0.9 {
            dir.cross(Vec3::Z)
        } else {
            dir.cross(Vec3::X)
        }
        .normalize_or_zero();
        let up = side.cross(dir);
        let head = length.min(96.0) * 0.12;
        for off in [side, -side, up, -up] {
            segments.push([tip, tip - dir * head + off * head * 0.45]);
        }
    }
    Helper::Lines {
        segments,
        color,
        xray: true,
    }
}

/// Lines from an entity to everything its key names.
fn target_lines(
    origin: Vec3,
    name: Option<String>,
    targets: Targets<'_>,
    color: Color32,
) -> Option<Helper> {
    let name = name?;
    let segments: Vec<[Vec3; 2]> = targets(name.trim())
        .into_iter()
        .map(|to| [origin, to])
        .collect();
    (!segments.is_empty()).then_some(Helper::Lines {
        segments,
        color,
        xray: true,
    })
}

#[cfg(test)]
mod tests;
