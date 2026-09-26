// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
// World surface shading.
//
// Diffuse lighting is entirely baked: the lightmap atlas already holds the
// result of every light, bounce and shadow that Radiance computed. The
// fragment shader's job is to combine it with the material, not to light
// anything -- and not to tone-map either: it writes linear HDR, and the
// tone-map pass folds the whole frame into display range at once. That is the whole bargain of a BSP engine -- expensive
// lighting, computed once, at build time.
//
// What the material still gets a say in is everything the bake could not know:
// which way the surface actually faces at texel scale (the normal map), how
// tight its highlight is (roughness), what it shadows itself (occlusion), and
// what it emits regardless of any of that (emissive), and whether it is metal.
// Those are per-texel and view-dependent, so they belong here rather than in
// the atlas.

struct Camera {
    view_proj: mat4x4<f32>,
    position: vec4<f32>,
    // x: unused (exposure is the tone-map pass's), y: time, z: lightmap
    // enable, w: fullbright
    params: vec4<f32>,
    // Colour the sky renders, from light_environment.
    sky_color: vec4<f32>,
    // x: normal-map scale (r_bumpmap), y: specular scale (r_specular)
    render: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var lightmap_texture: texture_2d<f32>;
@group(0) @binding(2) var lightmap_sampler: sampler;
@group(0) @binding(3) var probe_texture: texture_2d_array<f32>;
@group(0) @binding(4) var probe_sampler: sampler;

// Dynamic lights. `crates/kerosene-render/src/lights.rs` packs these and is
// the reference for everything the light loop below does.
struct GpuLight {
    // xyz position, w range.
    position_range: vec4<f32>,
    // rgb intensity on the lightmap's scale, w 1 for a spot.
    color_kind: vec4<f32>,
    // xyz spot direction, w cos(outer half-angle).
    direction_outer: vec4<f32>,
    // x cos(inner half-angle), y cone exponent, z constant, w linear falloff.
    cone_attn: vec4<f32>,
    // x quadratic falloff, y first shadow layer or -1.
    attn_shadow: vec4<f32>,
};
struct DynamicLights {
    lights: array<GpuLight, 32>,
    shadow_matrices: array<mat4x4<f32>, 16>,
    // x light count.
    count: vec4<u32>,
    // xy target size, z near, w cluster far.
    screen: vec4<f32>,
    // xyz camera forward.
    forward: vec4<f32>,
};
@group(0) @binding(5) var<uniform> dynamic_lights: DynamicLights;
// A bit per light for each of the 16 x 9 x 24 clusters, four to a vec4.
struct Clusters {
    masks: array<vec4<u32>, 864>,
};
@group(0) @binding(6) var<uniform> clusters: Clusters;
@group(0) @binding(7) var shadow_texture: texture_depth_2d_array;
@group(0) @binding(8) var shadow_sampler: sampler_comparison;

// A material is one sampler, a word saying which of its maps are real, and
// six textures. The absent ones are bound to a neutral 1x1 texel, so every
// material fills the same layout and there is one pipeline rather than a
// variant per combination of maps.
@group(1) @binding(0) var base_sampler: sampler;

struct MaterialParams {
    // Bit 0 base, 1 normal, 2 roughness, 3 emissive, 4 occlusion, 5 metalness.
    present: u32,
    emissive_strength: f32,
    normal_strength: f32,
    specular_strength: f32,
    // `$metalness`; scales the metalness map, whose neutral stand-in is white.
    metalness: f32,
    // `$roughnessfactor`; scales the roughness map the same way.
    roughness_factor: f32,
    _pad0: f32,
    _pad1: f32,
};
@group(1) @binding(1) var<uniform> material: MaterialParams;

@group(1) @binding(2) var base_texture: texture_2d<f32>;
@group(1) @binding(3) var normal_texture: texture_2d<f32>;
@group(1) @binding(4) var roughness_texture: texture_2d<f32>;
@group(1) @binding(5) var emissive_texture: texture_2d<f32>;
@group(1) @binding(6) var ao_texture: texture_2d<f32>;
@group(1) @binding(7) var metalness_texture: texture_2d<f32>;

const MAP_NORMAL: u32 = 1u;
const MAP_ROUGHNESS: u32 = 2u;
const MAP_EMISSIVE: u32 = 3u;
const MAP_AO: u32 = 4u;

fn has_map(slot: u32) -> bool {
    return (material.present & (1u << slot)) != 0u;
}

// The material's own strength, scaled by the console's. A material may say its
// bumps are subtle; `r_bumpmap 0` says show none at all, and `r_bumpmap 2` says
// show me what is in there.
fn normal_strength() -> f32 {
    return material.normal_strength * camera.render.x;
}

fn specular_strength() -> f32 {
    return material.specular_strength * camera.render.y;
}

// Where this brush model has got to since it was compiled. The identity for
// the world; a door's displacement while it opens, or a rotating brush's turn.
// Bound with a dynamic offset, so it changes between draws inside one pass.
struct Model {
    transform: mat4x4<f32>,
};
@group(2) @binding(0) var<uniform> model: Model;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) lightmap_uv: vec2<f32>,
    // xyz the tangent, w the bitangent's handedness.
    @location(4) tangent: vec4<f32>,
    // The probe this face reflects, or NO_PROBE. The same for every vertex
    // of a face, so flat is exact rather than an approximation.
    @location(5) probe: u32,
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) lightmap_uv: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) world_position: vec3<f32>,
    @location(4) tangent: vec4<f32>,
    @location(5) @interpolate(flat) probe: u32,
};

@vertex
fn vs_main(input: VertexIn) -> VertexOut {
    var out: VertexOut;
    // The same transform the collision code inverts when it traces against
    // this model, which is the reason what you see and what you walk into are
    // in the same place by construction rather than by agreement.
    let world = (model.transform * vec4<f32>(input.position, 1.0)).xyz;
    out.clip_position = camera.view_proj * vec4<f32>(world, 1.0);
    out.uv = input.uv;
    out.lightmap_uv = input.lightmap_uv;
    // Rotated, not carried through: w = 0 drops the translation, which a
    // direction must not have. This is the basis a normal map is read in, and
    // the surface the specular lobe is measured against.
    out.normal = (model.transform * vec4<f32>(input.normal, 0.0)).xyz;
    // The tangent rotates with the model for the same reason the normal does.
    // Handedness rides along in w untouched: it is a sign, not a direction,
    // and a rigid transform cannot change it.
    out.tangent = vec4<f32>(
        (model.transform * vec4<f32>(input.tangent.xyz, 0.0)).xyz,
        input.tangent.w,
    );
    out.world_position = world;
    out.probe = input.probe;
    return out;
}

// A floor of ambient light so a surface with no lightmap is dim rather than
// pure black. An unlit room should look unlit, not look broken.
const MIN_AMBIENT: f32 = 0.06;

// The shading normal: the surface's own, tilted by the normal map where there
// is one.
//
// The basis comes from the texture projection rather than from the triangles
// (see `face_tangent` in mesh.rs), so tangent space and the UVs a normal map
// is sampled with are the same space by construction. Re-orthogonalising here
// is what makes that survive interpolation across a face: the rasteriser will
// happily hand a fragment a tangent that has drifted off the plane.
fn shading_normal(input: VertexOut) -> vec3<f32> {
    let n = normalize(input.normal);
    if (!has_map(MAP_NORMAL) || normal_strength() <= 0.0) {
        return n;
    }

    let t_raw = input.tangent.xyz;
    if (dot(t_raw, t_raw) < 1e-12) {
        return n;
    }
    // Gram-Schmidt: drop whatever part of the tangent has drifted along the
    // normal, keep the rest.
    let t = normalize(t_raw - n * dot(n, t_raw));
    let b = cross(n, t) * input.tangent.w;

    // Tangent-space normals are stored biased into 0..1.
    let sampled = textureSample(normal_texture, base_sampler, input.uv).xyz * 2.0 - 1.0;
    // Scaling x and y rather than lerping the result keeps the vector a
    // direction: a mix with the flat normal would shorten it towards zero
    // where the map is steep.
    let tilted = vec3<f32>(sampled.xy * normal_strength(), max(sampled.z, 1e-4));

    let world = normalize(t * tilted.x + b * tilted.y + n * tilted.z);
    // A normal map that has been resized, or is not really a normal map, can
    // produce a vector facing into the surface. Keep the geometric normal
    // rather than lighting the back of the wall.
    if (dot(world, n) <= 0.0) {
        return n;
    }
    return world;
}

// ---- reflectance ------------------------------------------------------------
//
// GGX, height-correlated Smith and Schlick: the model Source 2 and every other
// current engine settled on. `crates/kerosene-render/src/brdf.rs` is the CPU
// reference for each of these, with the tests; change both or neither.

const PI: f32 = 3.14159265;
const DIELECTRIC_F0: f32 = 0.04;

const MIN_ROUGHNESS: f32 = 0.045;

fn d_ggx(n_dot_h: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn v_smith(n_dot_v: f32, n_dot_l: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let gv = n_dot_l * sqrt(n_dot_v * n_dot_v * (1.0 - a2) + a2);
    let gl = n_dot_v * sqrt(n_dot_l * n_dot_l * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 1e-5);
}

fn f_schlick(f0: vec3<f32>, v_dot_h: f32) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - clamp(v_dot_h, 0.0, 1.0), 5.0);
}

// Specular toward `v` from light along `l`, per unit of irradiance on a
// surface facing it.
fn specular_ggx(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32, f0: vec3<f32>) -> vec3<f32> {
    let n_dot_l = dot(n, l);
    let n_dot_v = dot(n, v);
    if (n_dot_l <= 0.0 || n_dot_v <= 0.0) {
        return vec3<f32>(0.0);
    }
    let h = normalize(v + l);
    let r = clamp(roughness, MIN_ROUGHNESS, 1.0);
    let alpha = r * r;
    return f_schlick(f0, dot(v, h)) * (d_ggx(max(dot(n, h), 0.0), alpha) * v_smith(n_dot_v, n_dot_l, alpha) * n_dot_l);
}

// Specular colour at normal incidence: 4% grey for anything that is not
// metal, the base colour for anything that is.
fn f0_for(albedo: vec3<f32>, metalness: f32) -> vec3<f32> {
    return mix(vec3<f32>(DIELECTRIC_F0), albedo, metalness);
}

// What a surface reflects of an environment equally bright in every
// direction, as a fraction of that brightness. Karis's fit to the split-sum
// integral.
//
// This is the honest specular for a lightmapped surface. The lightmap says how
// much light arrived, not from where; pretending it came from one direction
// invents a highlight that nothing cast, and one that slides across a floor
// as the player walks. "Evenly from everywhere" invents nothing -- a smooth
// floor still brightens toward grazing, as a real one does -- and it is the
// term a cubemap probe multiplies once there is one to say what the
// environment actually looks like.
fn env_brdf(f0: vec3<f32>, roughness: f32, n_dot_v: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = clamp(roughness, 0.0, 1.0) * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * max(n_dot_v, 0.0))) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

// ---- cubemap probes ---------------------------------------------------------
//
// Six layers a probe in one 2D array, faces picked here rather than by the
// hardware -- see `crates/kerosene-render/src/probes.rs` for why. The face
// table is `kerosene_bsp::cubemaps::face_basis`; a test there checks the Rust
// side round-trips, and this must stay a copy of it.

const NO_PROBE: u32 = 0xffffffffu;

// (s, t, face) for a direction, s and t in 0..1.
fn probe_uv(dir: vec3<f32>) -> vec3<f32> {
    let a = abs(dir);
    var face = 0u;
    var major = vec3<f32>(1.0, 0.0, 0.0);
    var s_axis = vec3<f32>(0.0, 1.0, 0.0);
    var t_axis = vec3<f32>(0.0, 0.0, 1.0);
    if (a.x >= a.y && a.x >= a.z) {
        if (dir.x < 0.0) {
            face = 1u;
            major = vec3<f32>(-1.0, 0.0, 0.0);
            s_axis = vec3<f32>(0.0, -1.0, 0.0);
        }
    } else if (a.y >= a.z) {
        if (dir.y >= 0.0) {
            face = 2u;
            major = vec3<f32>(0.0, 1.0, 0.0);
            s_axis = vec3<f32>(-1.0, 0.0, 0.0);
        } else {
            face = 3u;
            major = vec3<f32>(0.0, -1.0, 0.0);
            s_axis = vec3<f32>(1.0, 0.0, 0.0);
        }
    } else {
        s_axis = vec3<f32>(1.0, 0.0, 0.0);
        if (dir.z >= 0.0) {
            face = 4u;
            major = vec3<f32>(0.0, 0.0, 1.0);
            t_axis = vec3<f32>(0.0, 1.0, 0.0);
        } else {
            face = 5u;
            major = vec3<f32>(0.0, 0.0, -1.0);
            t_axis = vec3<f32>(0.0, -1.0, 0.0);
        }
    }
    let m = max(dot(dir, major), 1e-6);
    return vec3<f32>((dot(dir, s_axis) / m + 1.0) * 0.5, (dot(dir, t_axis) / m + 1.0) * 0.5, f32(face));
}

// What a probe sees along `dir`, blurred as far as `roughness` says: the mip
// chain runs from the probe as baked down to one texel a face.
fn probe_radiance(probe: u32, dir: vec3<f32>, roughness: f32) -> vec3<f32> {
    let st = probe_uv(normalize(dir));
    let levels = f32(textureNumLevels(probe_texture));
    let lod = clamp(roughness, 0.0, 1.0) * (levels - 1.0);
    let layer = i32(probe * 6u + u32(st.z));
    return textureSampleLevel(probe_texture, probe_sampler, st.xy, layer, lod).rgb;
}

// ---- dynamic lights ---------------------------------------------------------

// The bitmask of lights that can reach this fragment's cluster.
fn cluster_mask(frag: vec2<f32>, world: vec3<f32>) -> u32 {
    let screen = dynamic_lights.screen;
    let tile = vec2<u32>(clamp(frag / screen.xy * vec2<f32>(16.0, 9.0), vec2<f32>(0.0), vec2<f32>(15.0, 8.0)));
    let depth = dot(world - camera.position.xyz, dynamic_lights.forward.xyz);
    var slice = 0u;
    if (depth > screen.z) {
        slice = min(u32(log(depth / screen.z) / log(screen.w / screen.z) * 24.0), 23u);
    }
    let index = (slice * 9u + tile.y) * 16u + tile.x;
    return clusters.masks[index / 4u][index % 4u];
}

// How much of a light gets past its shadow map: 1 fully lit, 0 in shadow.
// A point light's six faces are +X -X +Y -Y +Z -Z, picked by the major axis
// of the direction from the light, as `DynamicLight::shadow_views` lays them.
fn shadow_factor(first_layer: i32, light_pos: vec3<f32>, is_spot: bool, world: vec3<f32>, normal: vec3<f32>) -> f32 {
    if (first_layer < 0) {
        return 1.0;
    }
    var layer = first_layer;
    if (!is_spot) {
        let d = world - light_pos;
        let a = abs(d);
        if (a.x >= a.y && a.x >= a.z) {
            layer = layer + select(1, 0, d.x >= 0.0);
        } else if (a.y >= a.z) {
            layer = layer + select(3, 2, d.y >= 0.0);
        } else {
            layer = layer + select(5, 4, d.z >= 0.0);
        }
    }
    // Pushed out along the normal, further the further from the light, so a
    // surface does not compare against its own stored depth and stripe.
    let offset = normal * (0.75 + 0.004 * distance(world, light_pos));
    let clip = dynamic_lights.shadow_matrices[layer] * vec4<f32>(world + offset, 1.0);
    if (clip.w <= 0.0) {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || ndc.z > 1.0) {
        return 1.0;
    }
    // Four hardware-filtered taps: sixteen depth tests, a soft edge.
    let texel = 1.0 / 512.0;
    var lit = 0.0;
    lit += textureSampleCompareLevel(shadow_texture, shadow_sampler, uv + vec2<f32>(-0.5, -0.5) * texel, layer, ndc.z);
    lit += textureSampleCompareLevel(shadow_texture, shadow_sampler, uv + vec2<f32>(0.5, -0.5) * texel, layer, ndc.z);
    lit += textureSampleCompareLevel(shadow_texture, shadow_sampler, uv + vec2<f32>(-0.5, 0.5) * texel, layer, ndc.z);
    lit += textureSampleCompareLevel(shadow_texture, shadow_sampler, uv + vec2<f32>(0.5, 0.5) * texel, layer, ndc.z);
    return lit * 0.25;
}

// Every dynamic light reaching this fragment: diffuse on `diffuse_color`
// (the base colour with metal's share removed) plus GGX specular. Light
// values are on the lightmap's scale, so a Lambert surface facing a light
// gets `diffuse_color * intensity * n.l`, exactly as the bake would give it.
fn dynamic_lighting(
    frag: vec2<f32>,
    world: vec3<f32>,
    geometric: vec3<f32>,
    n: vec3<f32>,
    view: vec3<f32>,
    diffuse_color: vec3<f32>,
    roughness: f32,
    f0: vec3<f32>,
) -> vec3<f32> {
    var total = vec3<f32>(0.0);
    var mask = cluster_mask(frag, world);
    while (mask != 0u) {
        let i = firstTrailingBit(mask);
        mask = mask & (mask - 1u);
        if (i >= dynamic_lights.count.x) {
            continue;
        }
        let light = dynamic_lights.lights[i];

        let delta = light.position_range.xyz - world;
        let dist = length(delta);
        let range = light.position_range.w;
        if (dist >= range) {
            continue;
        }
        let l = delta / max(dist, 1e-4);
        let n_dot_l = dot(n, l);
        if (n_dot_l <= 0.0) {
            continue;
        }

        // kerosene_math::light: falloff normalised at 100 units, times a
        // window that fades the last of the range out.
        let d = max(dist, 1.0);
        let denom = light.cone_attn.z + light.cone_attn.w * d / 100.0 + light.attn_shadow.x * d * d / 10000.0;
        var scale = select(0.0, 1.0 / denom, denom > 0.0);
        let x = dist / range;
        let w = 1.0 - x * x * x * x;
        scale = scale * w * w;

        let is_spot = light.color_kind.w > 0.5;
        if (is_spot) {
            let cos_angle = dot(-l, light.direction_outer.xyz);
            let cos_outer = light.direction_outer.w;
            let cos_inner = light.cone_attn.x;
            if (cos_angle < cos_outer) {
                continue;
            }
            if (cos_angle < cos_inner) {
                let t = (cos_angle - cos_outer) / max(cos_inner - cos_outer, 1e-6);
                scale = scale * pow(t, max(light.cone_attn.y, 0.01));
            }
        }

        scale = scale * shadow_factor(i32(light.attn_shadow.y), light.position_range.xyz, is_spot, world, geometric);
        if (scale <= 0.0) {
            continue;
        }
        let radiance = light.color_kind.rgb * scale;
        // Lambert is albedo / pi times irradiance, so the specular term,
        // which is per unit irradiance, carries the pi back in.
        total = total + diffuse_color * radiance * n_dot_l + specular_ggx(n, view, l, roughness, f0) * radiance * PI;
    }
    return total;
}

fn metalness(uv: vec2<f32>) -> f32 {
    // The neutral map is white, so without one this is the scalar alone.
    return clamp(material.metalness * textureSample(metalness_texture, base_sampler, uv).r, 0.0, 1.0);
}

@fragment
fn fs_world(input: VertexOut) -> @location(0) vec4<f32> {
    let albedo = textureSample(base_texture, base_sampler, input.uv);

    if (camera.params.w > 0.5) {
        // r_fullbright: show the materials with no lighting at all, which is
        // how you tell a lighting bug from a texture bug.
        return vec4<f32>(albedo.rgb, 1.0);
    }

    // Linear light, as Radiance baked it: 1.0 is a surface lit to full, and
    // a lamp against a wall is many times that.
    var light = vec3<f32>(MIN_AMBIENT);
    if (camera.params.z > 0.5) {
        let sampled = textureSample(lightmap_texture, lightmap_sampler, input.lightmap_uv).rgb;
        light = max(sampled, vec3<f32>(MIN_AMBIENT));
    }

    // Occlusion darkens what the surface shadows itself, which the lightmap
    // cannot: its luxels are far too coarse to see into a crevice.
    if (has_map(MAP_AO)) {
        let ao = textureSample(ao_texture, base_sampler, input.uv).r;
        light = light * ao;
    }

    let metal = metalness(input.uv);
    // Metal has no diffuse: what light it does not reflect, it absorbs.
    var color = albedo.rgb * (1.0 - metal) * light;

    let geometric = normalize(input.normal);
    let n = shading_normal(input);
    let view = normalize(camera.position.xyz - input.world_position);

    // Make the bumps visible in the diffuse term, not only in the highlight.
    //
    // This is the awkward part of normal mapping a lightmapped world: the
    // atlas holds how much light arrives, not which way it came from, so
    // there is no direction to take the real dot product against. Radiosity
    // normal maps -- three lightmaps on a tangent-space basis -- are the
    // proper fix, and are a change to Radiance and to the atlas rather than
    // to this shader.
    //
    // Until then, a fixed notional direction. It is a lie, but a consistent
    // one: every surface is lit as though the light were up and slightly to
    // one side, which is where light usually is, and it is the same lie the
    // model shader already tells for props. What it buys is that a normal map
    // does something on a wall -- the alternative being that it does nothing
    // at all and looks broken.
    if (has_map(MAP_NORMAL) && normal_strength() > 0.0) {
        let key = normalize(vec3<f32>(0.3, 0.4, 0.9));
        // Half-Lambert: light wraps past the terminator rather than clamping
        // to black, because the baked light already arrived from everywhere
        // and a hard terminator would read as a shadow that is not there.
        let bumped = dot(n, key) * 0.5 + 0.5;
        let flat = dot(geometric, key) * 0.5 + 0.5;
        // The *difference* the map makes, not the absolute term: a flat
        // normal map must leave the surface exactly as the lightmap had it.
        color = color * clamp(bumped / max(flat, 0.05), 0.5, 1.6);
    }

    // A dielectric with no roughness map and no `$roughnessfactor` is taken
    // to be fully rough and gets no specular term, so a plain albedo material
    // renders exactly as the lightmap had it. Metal always reflects: a metal
    // with no specular would be black.
    //
    // What it reflects is the face's probe, seen along the mirror direction
    // and blurred by roughness. A face with no probe reflects an even glow
    // the brightness of its own lightmap, which is right on average and
    // wrong in every particular -- the reason to place env_cubemaps.
    let roughness = textureSample(roughness_texture, base_sampler, input.uv).r * material.roughness_factor;
    let f0 = f0_for(albedo.rgb, metal);
    if ((has_map(MAP_ROUGHNESS) || metal > 0.0 || material.roughness_factor < 1.0) && specular_strength() > 0.0) {
        let n_dot_v = max(dot(n, view), 1e-4);
        var environment = light;
        if (input.probe != NO_PROBE) {
            environment = probe_radiance(input.probe, reflect(-view, n), roughness);
            if (has_map(MAP_AO)) {
                environment = environment * textureSample(ao_texture, base_sampler, input.uv).r;
            }
        }
        color = color + environment * env_brdf(f0, roughness, n_dot_v) * specular_strength();
    }

    // Dynamic lights, on top of the bake. They have a real direction, so the
    // normal map and the specular lobe both answer to them properly -- the
    // one place a world surface is lit the way a prop is.
    color = color + dynamic_lighting(
        input.clip_position.xy,
        input.world_position,
        geometric,
        n,
        view,
        albedo.rgb * (1.0 - metal),
        roughness,
        f0 * specular_strength(),
    );

    // Emissive is light leaving the surface, added in the same linear space
    // as everything else so it blooms and tone-maps along with a highlight
    // rather than being pasted on at full strength afterwards.
    if (has_map(MAP_EMISSIVE)) {
        let emissive = textureSample(emissive_texture, base_sampler, input.uv).rgb;
        color = color + emissive * material.emissive_strength;
    }

    return vec4<f32>(color, albedo.a);
}

@fragment
fn fs_sky(input: VertexOut) -> @location(0) vec4<f32> {
    // The sky is drawn on real geometry, but should look infinitely far away,
    // so it is sampled by view *direction* rather than by surface position.
    let dir = normalize(input.world_position - camera.position.xyz);
    let u = atan2(dir.y, dir.x) / (2.0 * 3.14159265) + 0.5;
    let v = clamp(0.5 - asin(clamp(dir.z, -1.0, 1.0)) / 3.14159265, 0.0, 1.0);

    let sky = textureSample(base_texture, base_sampler, vec2<f32>(u, v)).rgb;
    return vec4<f32>(sky * camera.sky_color.rgb, 1.0);
}

@fragment
fn fs_unlit(input: VertexOut) -> @location(0) vec4<f32> {
    // Straight through: the texture is sampled through an sRGB view so this
    // is linear, and the tone-map pass takes it from there like anything else.
    let albedo = textureSample(base_texture, base_sampler, input.uv);
    return vec4<f32>(albedo.rgb, albedo.a);
}
