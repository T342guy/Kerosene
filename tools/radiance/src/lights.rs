// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Light entities, read out of the compiled entity lump.
//!
//! Lighting is authored as entities, the same as everything else -- a `light`
//! is a point entity with a `_light` key, and the compiler picks it up from
//! the map. That means a level designer changes the lighting by moving things
//! in the editor, not by editing a separate file.

use kerosene_bsp::Bsp;
use kerosene_kv::{FromKvValue, KeyValues, Vec3Value};
use kerosene_math::{Angles, Vec3};

/// Distance at which a light's `_linear_attn` and `_quadratic_attn` are
/// normalised to 1. Matches Source: a light of brightness 200 with the default
/// quadratic falloff delivers 200 units of light at 100 inches away.
const ATTN_REFERENCE: f32 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightKind {
    /// Radiates in every direction.
    Point,
    /// A cone. `cone` is the outer half-angle in degrees, `inner` the angle
    /// inside which the light is at full strength.
    Spot { direction: Vec3, cone: f32, inner: f32, exponent: f32 },
    /// The sun: parallel rays from infinitely far away, only reaching surfaces
    /// with a clear line to the sky.
    Sun { direction: Vec3 },
}

#[derive(Clone, Debug)]
pub struct Light {
    pub kind: LightKind,
    pub origin: Vec3,
    /// Linear colour, already scaled by brightness.
    pub intensity: Vec3,
    /// `(constant, linear, quadratic)` falloff terms.
    pub attenuation: (f32, f32, f32),
    /// Beyond this distance the light contributes nothing worth tracing for.
    pub range: f32,
}

/// Everything the lighting compile needs from the entity lump.
pub struct LightSet {
    pub lights: Vec<Light>,
    /// Flat ambient added everywhere, from `light_environment`.
    pub ambient: Vec3,
    /// Colour the sky itself renders and emits.
    pub sky_color: Vec3,
    pub has_sun: bool,
}

impl LightSet {
    pub fn from_bsp(bsp: &Bsp) -> LightSet {
        let kv = match bsp.entities_kv() {
            Ok(kv) => kv,
            Err(e) => {
                log::warn!("entity lump did not parse ({e}); compiling with no lights");
                return LightSet {
                    lights: Vec::new(),
                    ambient: Vec3::ZERO,
                    sky_color: Vec3::ZERO,
                    has_sun: false,
                };
            }
        };
        Self::from_kv(&kv)
    }

    pub fn from_kv(kv: &KeyValues) -> LightSet {
        let mut lights = Vec::new();
        let mut ambient = Vec3::ZERO;
        let mut sky_color = Vec3::ZERO;
        let mut has_sun = false;

        for e in kv.blocks("entity") {
            let class = e.get("classname").unwrap_or("");
            let origin = e.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);

            match class {
                "light" => {
                    if let Some(l) = point_light(e, origin, LightKind::Point) {
                        lights.push(l);
                    }
                }
                "light_spot" => {
                    let angles = spot_angles(e);
                    let kind = LightKind::Spot {
                        direction: angles.forward(),
                        cone: e.get_or("_cone", 45.0f32),
                        inner: e.get_or("_inner_cone", 30.0f32),
                        exponent: e.get_or("_exponent", 1.0f32),
                    };
                    if let Some(l) = point_light(e, origin, kind) { lights.push(l); }
                }
                "light_environment" => {
                    has_sun = true;
                    let angles = spot_angles(e);
                    // The entity's angles point the way the sun *shines*, so
                    // rays travel along `forward` and surfaces are lit from
                    // the opposite direction.
                    let kind = LightKind::Sun { direction: angles.forward() };
                    if let Some((color, brightness)) = light_value(e, "_light") {
                        sky_color = color * brightness;
                        lights.push(Light {
                            kind,
                            origin,
                            intensity: color * brightness,
                            // The sun does not fall off with distance.
                            attenuation: (1.0, 0.0, 0.0),
                            range: f32::INFINITY,
                        });
                    }
                    if let Some((color, brightness)) = light_value(e, "_ambient") {
                        ambient += color * brightness;
                    }
                }
                _ => {}
            }
        }

        LightSet { lights, ambient, sky_color, has_sun }
    }

    pub fn is_empty(&self) -> bool {
        self.lights.is_empty() && self.ambient == Vec3::ZERO
    }
}

fn vec3(s: &str) -> Option<Vec3> {
    Vec3Value::from_kv(s).ok().map(|v| Vec3::from_array(v.to_array()))
}

/// Read a `"r g b brightness"` key into a linear colour and a brightness.
///
/// The four-number spelling is Source's, and the brightness is separate from
/// the colour so that a designer can tint a light without changing how bright
/// it is. A three-number value means brightness 200, the editor default.
fn light_value(e: &KeyValues, key: &str) -> Option<(Vec3, f32)> {
    let raw = e.get(key)?;
    let nums: Vec<f32> = raw.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    match nums.len() {
        3 => Some((Vec3::new(nums[0], nums[1], nums[2]) / 255.0, 200.0)),
        4 => Some((Vec3::new(nums[0], nums[1], nums[2]) / 255.0, nums[3])),
        _ => None,
    }
}

fn point_light(e: &KeyValues, origin: Vec3, kind: LightKind) -> Option<Light> {
    let (color, brightness) = light_value(e, "_light")?;
    let attenuation = (
        e.get_or("_constant_attn", 0.0f32),
        e.get_or("_linear_attn", 0.0f32),
        // Quadratic by default: real light falls off with the square of
        // distance, and anything else looks wrong in a room.
        e.get_or("_quadratic_attn", 1.0f32),
    );
    let intensity = color * brightness;
    Some(Light {
        range: cutoff_range(intensity, attenuation),
        kind,
        origin,
        intensity,
        attenuation,
    })
}

/// Distance past which a light is too dim to bother tracing to.
///
/// Without a cutoff every luxel fires a shadow ray at every light in the map,
/// and the compile time is the product of the two. Solving the falloff for
/// "one part in 512 of a fully bright surface" is cheap and cuts most of them.
fn cutoff_range(intensity: Vec3, (c, l, q): (f32, f32, f32)) -> f32 {
    let peak = intensity.max_element();
    if peak <= 0.0 { return 0.0; }
    let threshold = 1.0 / 512.0;

    if q > 0.0 {
        // peak * ref^2 / (q * d^2) = threshold
        (peak * ATTN_REFERENCE * ATTN_REFERENCE / (q * threshold)).sqrt()
    } else if l > 0.0 {
        peak * ATTN_REFERENCE / (l * threshold)
    } else if c > 0.0 {
        // No distance falloff at all; only the constant term limits it.
        if peak / c > threshold { f32::INFINITY } else { 0.0 }
    } else {
        f32::INFINITY
    }
}

/// A light's aim, from `angles` and the `pitch` key.
///
/// `pitch` overrides the pitch component of `angles` when present. That
/// redundancy is Source's, and it exists because the editor writes `pitch`
/// separately for lights so it can be dragged in a 2D view.
fn spot_angles(e: &KeyValues) -> Angles {
    let a = e.get("angles").and_then(vec3).unwrap_or(Vec3::ZERO);
    let mut angles = Angles::new(a.x, a.y, a.z);
    if let Some(pitch) = e.get("pitch").and_then(|p| p.trim().parse::<f32>().ok()) {
        // The key is stored as an upward-positive angle, the opposite of the
        // engine's pitch convention.
        angles.pitch = -pitch;
    }
    angles
}

impl Light {
    /// How much light reaches `point`, and from which direction.
    ///
    /// Returns `None` when the light cannot reach at all -- out of range,
    /// outside a spot cone -- so the caller can skip the shadow ray.
    pub fn sample(&self, point: Vec3) -> Option<(Vec3, Vec3)> {
        match self.kind {
            LightKind::Sun { direction } => {
                // Parallel rays; the surface is lit from where they came from.
                Some((self.intensity, -direction))
            }
            LightKind::Point => {
                let delta = self.origin - point;
                let dist = delta.length();
                if dist > self.range { return None; }
                let falloff = self.falloff(dist);
                if falloff <= 0.0 { return None; }
                Some((self.intensity * falloff, delta / dist.max(1e-6)))
            }
            LightKind::Spot { direction, cone, inner, exponent } => {
                let delta = self.origin - point;
                let dist = delta.length();
                if dist > self.range { return None; }
                let to_point = -delta / dist.max(1e-6);

                let cos_angle = to_point.dot(direction);
                let cos_outer = cone.to_radians().cos();
                if cos_angle < cos_outer { return None; }

                let cos_inner = inner.to_radians().cos();
                let cone_scale = if cos_angle >= cos_inner {
                    1.0
                } else {
                    // Soften the edge between the inner and outer cones.
                    let t = (cos_angle - cos_outer) / (cos_inner - cos_outer).max(1e-6);
                    t.powf(exponent.max(0.01))
                };

                let falloff = self.falloff(dist) * cone_scale;
                if falloff <= 0.0 { return None; }
                Some((self.intensity * falloff, delta / dist.max(1e-6)))
            }
        }
    }

    fn falloff(&self, dist: f32) -> f32 {
        let (c, l, q) = self.attenuation;
        let d = dist.max(1.0);
        // Linear and quadratic terms are normalised at ATTN_REFERENCE inches,
        // so brightness reads as "how bright at a normal room distance".
        let denom = c + l * d / ATTN_REFERENCE + q * (d * d) / (ATTN_REFERENCE * ATTN_REFERENCE);
        if denom <= 0.0 { 0.0 } else { 1.0 / denom }
    }

    /// Where a shadow ray toward this light should end.
    pub fn shadow_target(&self, point: Vec3) -> Vec3 {
        match self.kind {
            // The sun is outside the map, so aim far enough to leave it.
            LightKind::Sun { direction } => point - direction * kerosene_math::MAX_MAP_RANGE,
            _ => self.origin,
        }
    }

    pub fn is_sun(&self) -> bool { matches!(self.kind, LightKind::Sun { .. }) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> LightSet {
        LightSet::from_kv(&KeyValues::parse(text).unwrap())
    }

    #[test]
    fn a_point_light_is_read_with_its_colour_and_brightness() {
        let set = parse(r#"entity { "classname" "light" "origin" "0 0 128" "_light" "255 128 0 300" }"#);
        assert_eq!(set.lights.len(), 1);
        let l = &set.lights[0];
        assert_eq!(l.origin, Vec3::new(0.0, 0.0, 128.0));
        assert_eq!(l.kind, LightKind::Point);
        // Colour is normalised and scaled by brightness.
        assert!((l.intensity.x - 300.0).abs() < 1e-3);
        assert!((l.intensity.y - 300.0 * 128.0 / 255.0).abs() < 1e-2);
        assert_eq!(l.intensity.z, 0.0);
    }

    #[test]
    fn a_three_number_light_gets_the_default_brightness() {
        let set = parse(r#"entity { "classname" "light" "origin" "0 0 0" "_light" "255 255 255" }"#);
        assert!((set.lights[0].intensity.x - 200.0).abs() < 1e-3);
    }

    #[test]
    fn brightness_falls_off_with_the_square_of_distance() {
        let set = parse(r#"entity { "classname" "light" "origin" "0 0 0" "_light" "255 255 255 200" }"#);
        let l = &set.lights[0];
        let near = l.sample(Vec3::new(100.0, 0.0, 0.0)).unwrap().0.x;
        let far = l.sample(Vec3::new(200.0, 0.0, 0.0)).unwrap().0.x;
        assert!((near - 200.0).abs() < 1.0, "at the reference distance, brightness reads directly: {near}");
        assert!((far / near - 0.25).abs() < 0.01, "doubling distance should quarter it: {far} vs {near}");
    }

    #[test]
    fn a_light_is_dropped_beyond_its_useful_range() {
        let set = parse(r#"entity { "classname" "light" "origin" "0 0 0" "_light" "255 255 255 200" }"#);
        let l = &set.lights[0];
        assert!(l.range.is_finite() && l.range > 100.0);
        assert!(l.sample(Vec3::new(l.range * 2.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn the_light_direction_points_at_the_light() {
        let set = parse(r#"entity { "classname" "light" "origin" "0 0 128" "_light" "255 255 255 200" }"#);
        let (_, dir) = set.lights[0].sample(Vec3::ZERO).unwrap();
        assert!((dir - Vec3::Z).length() < 1e-4, "{dir:?}");
    }

    #[test]
    fn a_spot_light_lights_inside_its_cone_and_not_outside() {
        // Aimed straight down. The `pitch` key is upward-positive, so a
        // light shining at the floor is pitch -90, not 90.
        let set = parse(
            r#"entity { "classname" "light_spot" "origin" "0 0 128" "_light" "255 255 255 200"
                        "pitch" "-90" "_cone" "45" "_inner_cone" "30" }"#,
        );
        let l = &set.lights[0];
        let LightKind::Spot { direction, .. } = l.kind else { panic!("not a spot") };
        assert!((direction - -Vec3::Z).length() < 1e-4, "should aim down, got {direction:?}");

        assert!(l.sample(Vec3::new(0.0, 0.0, 0.0)).is_some(), "directly below is lit");
        assert!(
            l.sample(Vec3::new(1000.0, 0.0, 0.0)).is_none(),
            "far off to the side is outside the cone"
        );
    }

    #[test]
    fn a_spot_softens_between_its_inner_and_outer_cones() {
        let set = parse(
            r#"entity { "classname" "light_spot" "origin" "0 0 100" "_light" "255 255 255 200"
                        "pitch" "-90" "_cone" "60" "_inner_cone" "20" }"#,
        );
        let l = &set.lights[0];
        let centre = l.sample(Vec3::ZERO).unwrap().0.x;
        // 45 degrees out: between the inner and outer cones.
        let edge = l.sample(Vec3::new(100.0, 0.0, 0.0)).unwrap().0.x;
        assert!(edge < centre, "the cone edge must be dimmer: {edge} vs {centre}");
        assert!(edge > 0.0);
    }

    #[test]
    fn light_environment_gives_a_sun_and_an_ambient() {
        let set = parse(
            r#"entity { "classname" "light_environment" "pitch" "-60" "angles" "0 45 0"
                        "_light" "255 250 240 400" "_ambient" "60 70 90 100" }"#,
        );
        assert!(set.has_sun);
        assert_eq!(set.lights.len(), 1);
        assert!(set.lights[0].is_sun());
        assert!(set.ambient.length() > 0.0);
        assert!(set.sky_color.length() > 0.0);
    }

    #[test]
    fn the_sun_does_not_fall_off_with_distance() {
        let set = parse(r#"entity { "classname" "light_environment" "pitch" "-90" "_light" "255 255 255 300" }"#);
        let l = &set.lights[0];
        let near = l.sample(Vec3::ZERO).unwrap().0;
        let far = l.sample(Vec3::new(0.0, 0.0, -5000.0)).unwrap().0;
        assert_eq!(near, far);
    }

    #[test]
    fn the_sun_lights_surfaces_from_where_its_rays_come_from() {
        // pitch -90 means shining downward from overhead.
        let set = parse(r#"entity { "classname" "light_environment" "pitch" "-90" "_light" "255 255 255 300" }"#);
        let (_, dir) = set.lights[0].sample(Vec3::ZERO).unwrap();
        assert!((dir - Vec3::Z).length() < 1e-4, "a floor should be lit from above, got {dir:?}");
    }

    #[test]
    fn a_shadow_ray_to_the_sun_aims_out_of_the_map() {
        let set = parse(r#"entity { "classname" "light_environment" "pitch" "-90" "_light" "255 255 255 300" }"#);
        let target = set.lights[0].shadow_target(Vec3::ZERO);
        assert!(target.z > kerosene_math::MAX_MAP_COORD, "must reach past the sky, got {target:?}");
    }

    #[test]
    fn a_map_with_no_lights_is_recognised() {
        let set = parse(r#"entity { "classname" "info_player_start" "origin" "0 0 0" }"#);
        assert!(set.is_empty());
    }

    #[test]
    fn a_light_without_a_light_key_is_skipped() {
        let set = parse(r#"entity { "classname" "light" "origin" "0 0 0" }"#);
        assert!(set.lights.is_empty());
    }
}
