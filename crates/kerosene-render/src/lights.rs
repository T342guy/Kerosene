// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Dynamic lights: the ones that exist at draw time.
//!
//! Everything else in the world is lit by Radiance, ahead of time. A dynamic
//! light is the exception -- a flashlight, a muzzle flash, a lamp that swings
//! or turns off -- and it is lit here, every frame, on top of the lightmap.
//! Source 2 draws the same line: baked for what does not move, live for what
//! does.
//!
//! A dynamic light reads the same as a baked one with the same keys. The
//! falloff and cone are [`kerosene_math::light`], which Radiance uses too, and
//! the brightness is on the lightmap's scale (a `_light` of `255 255 255 200`
//! at a hundred units is what Radiance would have baked there).
//!
//! # Clustered shading
//!
//! Testing every light at every pixel costs lights times pixels. Instead the
//! view is cut into a grid of [`CLUSTERS_X`] by [`CLUSTERS_Y`] tiles and
//! [`CLUSTERS_Z`] depth slices, and each cluster gets a bitmask of the lights
//! whose sphere reaches it; a pixel tests only its own cluster's lights. The
//! binning is done here on the CPU -- it is a few dozen spheres against a few
//! thousand boxes -- which keeps it deterministic and available on every
//! backend, including GL, which has no compute to do it with.
//!
//! # Shadows
//!
//! A light marked to cast shadows is given layers of a depth-texture array:
//! one for a spot, six for a point light. They are handed out nearest light
//! first until [`SHADOW_LAYERS`] run out; a light that misses out still
//! lights, unshadowed.

use crate::camera::Camera;
use bytemuck::{Pod, Zeroable};
use kerosene_math::light::{Attenuation, spot_cone};
use kerosene_math::{Mat4, Vec3, Vec4};

/// The most dynamic lights drawn in one frame. The cluster masks are 32
/// bits, one per light.
pub const MAX_LIGHTS: usize = 32;

/// Cluster grid: tiles across, tiles down, and depth slices.
pub const CLUSTERS_X: u32 = 16;
pub const CLUSTERS_Y: u32 = 9;
pub const CLUSTERS_Z: u32 = 24;
pub const CLUSTER_COUNT: usize = (CLUSTERS_X * CLUSTERS_Y * CLUSTERS_Z) as usize;

/// Depth the slices run to. Slices are exponential, so near ones are thin
/// where the detail is; anything past this lands in the last one.
pub const CLUSTER_FAR: f32 = 4096.0;

/// Depth-texture layers for shadows: a point light takes six, a spot one.
pub const SHADOW_LAYERS: usize = 16;

/// Edge length of each shadow layer.
///
/// 512 keeps the whole array at 16 MiB, which fits on the integrated GPUs
/// the engine means to run on; filtering hides most of what 1024 would add.
pub const SHADOW_SIZE: u32 = 512;

/// Near plane of every shadow view. Close enough for a flashlight held
/// against a wall, far enough to keep depth precision.
pub const SHADOW_NEAR: f32 = 2.0;

/// A light fainter than this, on the lightmap's scale, is out of range. One
/// 8-bit step of a fully lit surface.
pub const LIGHT_THRESHOLD: f32 = 1.0 / 256.0;

/// No shadow for this light.
pub const NO_SHADOW: i32 = -1;

/// A light that exists at draw time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicLight {
    pub origin: Vec3,
    /// Linear colour, 0..1 per channel.
    pub color: Vec3,
    /// Source's fourth `_light` number: how bright at [`kerosene_math::light::ATTN_REFERENCE`].
    pub brightness: f32,
    pub attenuation: Attenuation,
    /// A hard limit on reach, when the light should stop short of where its
    /// falloff would take it. `None` is the falloff's own range.
    pub max_distance: Option<f32>,
    pub spot: Option<Spot>,
    /// Whether it casts shadows, if a shadow layer is free.
    pub shadows: bool,
}

/// A spot light's aim and cone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    /// Unit direction it shines along.
    pub direction: Vec3,
    /// Half-angles in degrees: full inside `inner`, nothing past `outer`.
    pub outer: f32,
    pub inner: f32,
    pub exponent: f32,
}

impl DynamicLight {
    /// A white point light of brightness 200, the editor's default.
    pub fn point(origin: Vec3) -> DynamicLight {
        DynamicLight {
            origin,
            color: Vec3::ONE,
            brightness: 200.0,
            attenuation: Attenuation::default(),
            max_distance: None,
            spot: None,
            shadows: false,
        }
    }

    /// Peak intensity on the lightmap's scale: `color * brightness / 255`,
    /// which is what Radiance's luxels hold before the atlas divides by 255.
    pub fn intensity(&self) -> Vec3 {
        self.color * (self.brightness / 255.0)
    }

    /// How far it reaches before it is too faint to see.
    pub fn range(&self) -> f32 {
        let natural = self
            .attenuation
            .range(self.intensity().max_element(), LIGHT_THRESHOLD);
        let limited = match self.max_distance {
            Some(d) if d > 0.0 => natural.min(d),
            _ => natural,
        };
        // A light with no falloff at all still needs a sphere to bin.
        limited.min(kerosene_math::MAX_MAP_RANGE)
    }

    /// Light arriving at `point` on the lightmap's scale, before the Lambert
    /// term, and the unit direction toward the light. `None` when it does not
    /// reach. The CPU reference for the shaders' `dynamic_light`.
    pub fn arriving(&self, point: Vec3) -> Option<(Vec3, Vec3)> {
        let delta = self.origin - point;
        let dist = delta.length();
        let range = self.range();
        if dist >= range {
            return None;
        }
        let to_light = delta / dist.max(1e-6);
        let mut scale = self.attenuation.falloff(dist) * range_window(dist, range);
        if let Some(spot) = self.spot {
            scale *= spot_cone(
                (-to_light).dot(spot.direction),
                spot.outer,
                spot.inner,
                spot.exponent,
            );
        }
        (scale > 0.0).then(|| (self.intensity() * scale, to_light))
    }

    /// How many shadow layers it needs.
    pub fn shadow_layers(&self) -> usize {
        if self.spot.is_some() { 1 } else { 6 }
    }

    /// The view-projection of each of its shadow layers, in layer order. A
    /// point light's six are +X, -X, +Y, -Y, +Z, -Z, which is the order the
    /// shaders choose a face in.
    pub fn shadow_views(&self) -> Vec<Mat4> {
        let far = self.range().max(SHADOW_NEAR * 2.0);
        match self.spot {
            Some(spot) => {
                let fov = (spot.outer * 2.0).clamp(1.0, 170.0).to_radians();
                let proj = Mat4::perspective_rh(fov, 1.0, SHADOW_NEAR, far);
                vec![proj * look_along(self.origin, spot.direction)]
            }
            None => {
                let proj = Mat4::perspective_rh(90f32.to_radians(), 1.0, SHADOW_NEAR, far);
                [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z]
                    .into_iter()
                    .map(|dir| proj * look_along(self.origin, dir))
                    .collect()
            }
        }
    }
}

/// A smooth fade over the last part of a light's range, so the edge of its
/// sphere is not a visible line. `(1 - (d/r)^4)^2`, which is 1 for most of
/// the range and eases to 0 at it.
pub fn range_window(dist: f32, range: f32) -> f32 {
    if range <= 0.0 {
        return 0.0;
    }
    let x = (dist / range).clamp(0.0, 1.0);
    let w = 1.0 - x * x * x * x;
    w * w
}

fn look_along(eye: Vec3, dir: Vec3) -> Mat4 {
    let up = if dir.z.abs() > 0.99 { Vec3::X } else { Vec3::Z };
    Mat4::look_to_rh(eye, dir, up)
}

// ---- what the GPU gets --------------------------------------------------------

/// One light, as the shaders read it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct GpuLight {
    /// xyz position, w range.
    pub position_range: [f32; 4],
    /// rgb intensity on the lightmap's scale, w: 1 for a spot, 0 a point.
    pub color_kind: [f32; 4],
    /// xyz spot direction, w cosine of the outer half-angle.
    pub direction_outer: [f32; 4],
    /// x cosine of the inner half-angle, y cone exponent, z constant and
    /// w linear attenuation.
    pub cone_attn: [f32; 4],
    /// x quadratic attenuation, y first shadow layer or -1, zw unused.
    pub attn_shadow: [f32; 4],
}

/// The per-frame light data, one uniform buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct LightsUniform {
    pub lights: [GpuLight; MAX_LIGHTS],
    pub shadow_matrices: [[[f32; 4]; 4]; SHADOW_LAYERS],
    /// x light count, yzw unused.
    pub count: [u32; 4],
    /// xy target size in pixels, z near, w [`CLUSTER_FAR`].
    pub screen: [f32; 4],
    /// xyz camera forward, w unused. With the camera's position (in the
    /// camera uniform) this gives a fragment's view depth.
    pub forward: [f32; 4],
}

impl Default for LightsUniform {
    fn default() -> Self {
        LightsUniform {
            lights: [GpuLight::default(); MAX_LIGHTS],
            shadow_matrices: [[[0.0; 4]; 4]; SHADOW_LAYERS],
            count: [0; 4],
            screen: [1.0, 1.0, 1.0, CLUSTER_FAR],
            forward: [1.0, 0.0, 0.0, 0.0],
        }
    }
}

/// One bit per light for every cluster, four clusters to a `vec4<u32>`
/// because a uniform array's elements are 16 bytes apart.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ClusterMasks {
    pub masks: [[u32; 4]; CLUSTER_COUNT / 4],
}

impl Default for ClusterMasks {
    fn default() -> Self {
        ClusterMasks {
            masks: [[0; 4]; CLUSTER_COUNT / 4],
        }
    }
}

impl ClusterMasks {
    pub fn get(&self, cluster: usize) -> u32 {
        self.masks[cluster / 4][cluster % 4]
    }
    fn set_bit(&mut self, cluster: usize, light: usize) {
        self.masks[cluster / 4][cluster % 4] |= 1 << light;
    }
}

/// Everything the shaders need about this frame's dynamic lights, and the
/// shadow views the host has to render before the scene.
pub struct LightFrame {
    pub uniform: LightsUniform,
    pub clusters: ClusterMasks,
    /// Per shadow layer in use: its view-projection, and the index into
    /// the lights passed to [`LightFrame::build`] it belongs to.
    pub shadow_views: Vec<(Mat4, usize)>,
    /// Which of the lights passed in made the cut, in upload order.
    pub drawn: Vec<usize>,
}

impl LightFrame {
    /// Choose, pack and bin this frame's lights.
    ///
    /// Lights whose sphere is outside the view are dropped; of the rest, the
    /// nearest [`MAX_LIGHTS`] are kept, and shadow layers go to the nearest
    /// shadow casters first. `width` and `height` are the target's.
    pub fn build(lights: &[DynamicLight], camera: &Camera, width: u32, height: u32) -> LightFrame {
        let frustum = camera.frustum();
        let mut candidates: Vec<(f32, usize)> = lights
            .iter()
            .enumerate()
            .filter(|(_, l)| l.intensity().max_element() > 0.0)
            .filter(|(_, l)| {
                let r = Vec3::splat(l.range());
                frustum.intersects_box(l.origin - r, l.origin + r)
            })
            .map(|(i, l)| (l.origin.distance_squared(camera.position), i))
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        candidates.truncate(MAX_LIGHTS);

        let mut frame = LightFrame {
            uniform: LightsUniform {
                screen: [
                    width.max(1) as f32,
                    height.max(1) as f32,
                    camera.near,
                    CLUSTER_FAR,
                ],
                forward: camera.forward().extend(0.0).to_array(),
                ..Default::default()
            },
            clusters: ClusterMasks::default(),
            shadow_views: Vec::new(),
            drawn: Vec::new(),
        };

        let view_proj = camera.view_projection();
        for (slot, &(_, index)) in candidates.iter().enumerate() {
            let light = &lights[index];
            let mut shadow = NO_SHADOW;
            if light.shadows && frame.shadow_views.len() + light.shadow_layers() <= SHADOW_LAYERS {
                shadow = frame.shadow_views.len() as i32;
                for view in light.shadow_views() {
                    let layer = frame.shadow_views.len();
                    frame.uniform.shadow_matrices[layer] = view.to_cols_array_2d();
                    frame.shadow_views.push((view, index));
                }
            }
            frame.uniform.lights[slot] = gpu_light(light, shadow);
            frame.drawn.push(index);
            bin_light(&mut frame.clusters, slot, light, camera, view_proj);
        }
        frame.uniform.count[0] = frame.drawn.len() as u32;
        frame
    }
}

fn gpu_light(light: &DynamicLight, shadow: i32) -> GpuLight {
    let i = light.intensity();
    let a = light.attenuation;
    let (kind, dir, outer, inner, exponent) = match light.spot {
        Some(s) => (
            1.0,
            s.direction.normalize_or_zero(),
            s.outer.to_radians().cos(),
            s.inner.to_radians().cos(),
            s.exponent,
        ),
        None => (0.0, Vec3::X, -1.0, -1.0, 1.0),
    };
    GpuLight {
        position_range: light.origin.extend(light.range()).to_array(),
        color_kind: [i.x, i.y, i.z, kind],
        direction_outer: dir.extend(outer).to_array(),
        cone_attn: [inner, exponent, a.constant, a.linear],
        attn_shadow: [a.quadratic, shadow as f32, 0.0, 0.0],
    }
}

/// Which depth slice a view depth falls in.
pub fn depth_slice(depth: f32, near: f32) -> u32 {
    if depth <= near {
        return 0;
    }
    let t = (depth / near).ln() / (CLUSTER_FAR / near).ln();
    ((t * CLUSTERS_Z as f32) as u32).min(CLUSTERS_Z - 1)
}

/// The cluster index of a tile and slice, as the shaders compute it.
pub fn cluster_index(x: u32, y: u32, z: u32) -> usize {
    ((z * CLUSTERS_Y + y) * CLUSTERS_X + x) as usize
}

/// Set this light's bit in every cluster its sphere might touch.
///
/// Conservative: the tiles come from the screen rectangle of the sphere's
/// bounding box, the slices from its nearest and farthest depth. A light
/// that is binned into a cluster it does not reach costs one wasted test per
/// pixel there; one missed would be a light that vanishes at a tile edge.
fn bin_light(
    masks: &mut ClusterMasks,
    slot: usize,
    light: &DynamicLight,
    camera: &Camera,
    vp: Mat4,
) {
    let range = light.range();
    let forward = camera.forward();
    let depth = (light.origin - camera.position).dot(forward);
    if depth + range < camera.near {
        return;
    }
    let z0 = depth_slice((depth - range).max(camera.near), camera.near);
    let z1 = depth_slice(depth + range, camera.near);

    // The screen rectangle. If any corner of the box is behind the eye the
    // projection is meaningless, and the light may cover anything.
    let (mut x0, mut y0, mut x1, mut y1) = (0, 0, CLUSTERS_X - 1, CLUSTERS_Y - 1);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut behind = false;
    for corner in 0..8 {
        let offset = Vec3::new(
            if corner & 1 == 0 { -range } else { range },
            if corner & 2 == 0 { -range } else { range },
            if corner & 4 == 0 { -range } else { range },
        );
        let clip = vp * Vec4::from((light.origin + offset, 1.0));
        if clip.w <= 1e-3 {
            behind = true;
            break;
        }
        let ndc = clip.truncate() / clip.w;
        min = min.min(ndc);
        max = max.max(ndc);
    }
    if !behind {
        if max.x < -1.0 || min.x > 1.0 || max.y < -1.0 || min.y > 1.0 {
            return;
        }
        let tile_x = |ndc: f32| {
            (((ndc * 0.5 + 0.5) * CLUSTERS_X as f32).floor() as i32).clamp(0, CLUSTERS_X as i32 - 1)
                as u32
        };
        // Screen y runs down, NDC y up.
        let tile_y = |ndc: f32| {
            (((0.5 - ndc * 0.5) * CLUSTERS_Y as f32).floor() as i32).clamp(0, CLUSTERS_Y as i32 - 1)
                as u32
        };
        x0 = tile_x(min.x);
        x1 = tile_x(max.x);
        y0 = tile_y(max.y);
        y1 = tile_y(min.y);
    }

    for z in z0..=z1 {
        for y in y0..=y1 {
            for x in x0..=x1 {
                masks.set_bit(cluster_index(x, y, z), slot);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kerosene_math::Angles;

    fn camera() -> Camera {
        Camera {
            position: Vec3::ZERO,
            angles: Angles::new(0.0, 0.0, 0.0),
            aspect: 16.0 / 9.0,
            ..Default::default()
        }
    }

    #[test]
    fn the_uniforms_match_what_the_shaders_declare() {
        assert_eq!(std::mem::size_of::<GpuLight>(), 80);
        assert_eq!(
            std::mem::size_of::<LightsUniform>(),
            80 * MAX_LIGHTS + 64 * SHADOW_LAYERS + 16 * 3
        );
        assert_eq!(std::mem::size_of::<ClusterMasks>(), CLUSTER_COUNT * 4);
        // The masks must fit the smallest uniform binding a device may offer.
        assert!(std::mem::size_of::<ClusterMasks>() <= 16 * 1024);
    }

    #[test]
    fn a_dynamic_light_reads_as_a_baked_one_would() {
        // Radiance's luxel for a white 200 at 100 units is 200; the atlas
        // divides by 255. A live light must land on the same number.
        let light = DynamicLight::point(Vec3::new(0.0, 0.0, 100.0));
        let (arriving, dir) = light.arriving(Vec3::ZERO).unwrap();
        assert!((arriving.x - 200.0 / 255.0).abs() < 0.01, "{arriving}");
        assert!((dir - Vec3::Z).length() < 1e-5);
    }

    #[test]
    fn the_range_is_where_it_fades_out() {
        let light = DynamicLight::point(Vec3::ZERO);
        let r = light.range();
        assert!(light.arriving(Vec3::new(r * 0.5, 0.0, 0.0)).is_some());
        assert!(light.arriving(Vec3::new(r * 1.01, 0.0, 0.0)).is_none());
        let limited = DynamicLight {
            max_distance: Some(64.0),
            ..light
        };
        assert_eq!(limited.range(), 64.0);
    }

    #[test]
    fn a_spot_lights_only_its_cone() {
        let light = DynamicLight {
            spot: Some(Spot {
                direction: -Vec3::Z,
                outer: 30.0,
                inner: 20.0,
                exponent: 1.0,
            }),
            ..DynamicLight::point(Vec3::new(0.0, 0.0, 100.0))
        };
        assert!(light.arriving(Vec3::ZERO).is_some(), "straight below");
        assert!(
            light.arriving(Vec3::new(200.0, 0.0, 0.0)).is_none(),
            "far off axis"
        );
    }

    #[test]
    fn slices_run_from_near_to_far_and_clamp() {
        assert_eq!(depth_slice(0.0, 3.0), 0);
        assert_eq!(depth_slice(3.0, 3.0), 0);
        assert!(depth_slice(100.0, 3.0) > depth_slice(10.0, 3.0));
        assert_eq!(depth_slice(1e9, 3.0), CLUSTERS_Z - 1);
    }

    #[test]
    fn a_light_ahead_is_binned_where_it_is_and_nowhere_else() {
        let cam = camera();
        let light = DynamicLight {
            max_distance: Some(32.0),
            ..DynamicLight::point(Vec3::new(500.0, 0.0, 0.0))
        };
        let frame = LightFrame::build(&[light], &cam, 1600, 900);
        assert_eq!(frame.drawn, vec![0]);

        // Dead centre of the screen, at its depth.
        let z = depth_slice(500.0, cam.near);
        let centre = cluster_index(CLUSTERS_X / 2, CLUSTERS_Y / 2, z);
        assert_eq!(frame.clusters.get(centre), 1);
        // A corner tile at the same depth, and the centre much nearer, miss.
        assert_eq!(frame.clusters.get(cluster_index(0, 0, z)), 0);
        assert_eq!(
            frame
                .clusters
                .get(cluster_index(CLUSTERS_X / 2, CLUSTERS_Y / 2, 0)),
            0
        );
    }

    #[test]
    fn a_light_behind_the_camera_is_not_drawn() {
        let light = DynamicLight {
            max_distance: Some(32.0),
            ..DynamicLight::point(Vec3::new(-500.0, 0.0, 0.0))
        };
        let frame = LightFrame::build(&[light], &camera(), 1600, 900);
        assert!(frame.drawn.is_empty());
        assert_eq!(frame.uniform.count[0], 0);
    }

    #[test]
    fn a_light_around_the_camera_covers_the_whole_screen_up_close() {
        let light = DynamicLight::point(Vec3::new(10.0, 0.0, 0.0));
        let frame = LightFrame::build(&[light], &camera(), 1600, 900);
        for (x, y) in [(0, 0), (CLUSTERS_X - 1, CLUSTERS_Y - 1)] {
            assert_eq!(frame.clusters.get(cluster_index(x, y, 0)), 1);
        }
    }

    #[test]
    fn the_nearest_lights_win_and_shadow_layers_run_out_in_order() {
        let cam = camera();
        let mut lights: Vec<DynamicLight> = (0..40)
            .map(|i| DynamicLight {
                shadows: true,
                max_distance: Some(16.0),
                ..DynamicLight::point(Vec3::new(100.0 + i as f32 * 10.0, 0.0, 0.0))
            })
            .collect();
        lights.reverse();
        let frame = LightFrame::build(&lights, &cam, 1600, 900);
        assert_eq!(frame.drawn.len(), MAX_LIGHTS);
        // The nearest is last in the input and first drawn.
        assert_eq!(frame.drawn[0], 39);
        // Point lights take six layers each: two fit in sixteen.
        assert_eq!(frame.shadow_views.len(), 12);
        assert_eq!(frame.uniform.lights[0].attn_shadow[1], 0.0);
        assert_eq!(frame.uniform.lights[1].attn_shadow[1], 6.0);
        assert_eq!(frame.uniform.lights[2].attn_shadow[1], NO_SHADOW as f32);
    }

    #[test]
    fn a_spot_shadow_view_looks_where_the_spot_points() {
        let light = DynamicLight {
            spot: Some(Spot {
                direction: Vec3::X,
                outer: 30.0,
                inner: 20.0,
                exponent: 1.0,
            }),
            ..DynamicLight::point(Vec3::ZERO)
        };
        let views = light.shadow_views();
        assert_eq!(views.len(), 1);
        let ahead = views[0] * Vec4::new(100.0, 0.0, 0.0, 1.0);
        let ndc = ahead.truncate() / ahead.w;
        assert!(ndc.x.abs() < 1e-3 && ndc.y.abs() < 1e-3, "{ndc}");
        assert!((0.0..=1.0).contains(&ndc.z));
    }

    #[test]
    fn each_point_shadow_face_sees_along_its_own_axis() {
        let views = DynamicLight::point(Vec3::ZERO).shadow_views();
        for (face, dir) in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z]
            .into_iter()
            .enumerate()
        {
            let clip = views[face] * (dir * 100.0).extend(1.0);
            let ndc = clip.truncate() / clip.w;
            assert!(
                clip.w > 0.0 && ndc.x.abs() < 1e-3 && ndc.y.abs() < 1e-3,
                "face {face}"
            );
        }
    }
}
