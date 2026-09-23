// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
// Model shading.
//
// Brush geometry carries baked lightmaps; models do not. A prop has to look
// lit on its own, so it gets a fixed key light plus a little ambient -- enough
// to read as a solid object sitting in the lit world, without pretending to
// be a lightmapped surface. Output is linear HDR, like the world's.

struct Camera {
    view_proj: mat4x4<f32>,
    position: vec4<f32>,
    // x: unused (exposure is the tone-map pass's), y: time, z: lightmap
    // enable, w: fullbright
    params: vec4<f32>,
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

// The same material layout the world uses -- one pipeline layout serves both,
// so this is not optional even where a map goes unread.
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

// Where this model instance has got to. The identity for a model that has not
// moved; a physics prop's pose every frame. Bound with a dynamic offset, the
// same buffer the brush models use.
struct Model {
    transform: mat4x4<f32>,
    // x: the probe this model reflects, or NO_PROBE.
    probe: vec4<u32>,
};
@group(2) @binding(0) var<uniform> model: Model;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) world_position: vec3<f32>,
    @location(3) @interpolate(flat) probe: u32,
};

@vertex
fn vs_model(input: VertexIn) -> VertexOut {
    var out: VertexOut;
    let world = (model.transform * vec4<f32>(input.position, 1.0)).xyz;
    out.clip_position = camera.view_proj * vec4<f32>(world, 1.0);
    out.uv = input.uv;
    // Rotated, not carried through: w = 0 drops the translation.
    out.normal = normalize((model.transform * vec4<f32>(input.normal, 0.0)).xyz);
    out.world_position = world;
    // Read here, where the model uniform is visible, and handed on.
    out.probe = model.probe.x;
    return out;
}

// A static prop drawn instanced: every copy of one model in a single draw,
// each with its own transform and probe from the instance buffer instead of
// the per-draw model uniform. Everything after the vertex stage is shared.
struct InstanceIn {
    @location(3) transform_0: vec4<f32>,
    @location(4) transform_1: vec4<f32>,
    @location(5) transform_2: vec4<f32>,
    @location(6) transform_3: vec4<f32>,
    @location(7) probe: u32,
};

@vertex
fn vs_model_instanced(input: VertexIn, instance: InstanceIn) -> VertexOut {
    let transform = mat4x4<f32>(
        instance.transform_0,
        instance.transform_1,
        instance.transform_2,
        instance.transform_3,
    );
    var out: VertexOut;
    let world = (transform * vec4<f32>(input.position, 1.0)).xyz;
    out.clip_position = camera.view_proj * vec4<f32>(world, 1.0);
    out.uv = input.uv;
    out.normal = normalize((transform * vec4<f32>(input.normal, 0.0)).xyz);
    out.world_position = world;
    out.probe = instance.probe;
    return out;
}

// The shading normal for a prop.
//
// `.keromdl` carries no tangents -- brush faces get theirs from the texture
// projection, and a mesh has no equivalent to read -- so the basis is
// reconstructed from how world position and UV change across the triangle.
// That is exactly the definition of a tangent, just measured per-fragment
// instead of stored per-vertex: it costs four derivatives and needs no change
// to the model format or to anything that writes one.
//
// The cost is that it is flat across a triangle and undefined where the UVs
// are degenerate, which is why brush geometry does not use it. For a prop it
// is the difference between a normal map working and there being nowhere to
// put one.
fn shading_normal(input: VertexOut) -> vec3<f32> {
    let n = normalize(input.normal);
    if (!has_map(MAP_NORMAL) || normal_strength() <= 0.0) {
        return n;
    }

    let dp1 = dpdx(input.world_position);
    let dp2 = dpdy(input.world_position);
    let duv1 = dpdx(input.uv);
    let duv2 = dpdy(input.uv);

    // Solving the 2x2 for the u direction. A zero determinant means the UVs
    // do not vary here -- a degenerate mapping -- and there is no basis to be
    // had.
    let det = duv1.x * duv2.y - duv2.x * duv1.y;
    if (abs(det) < 1e-12) {
        return n;
    }
    let t_raw = (dp1 * duv2.y - dp2 * duv1.y) / det;
    if (dot(t_raw, t_raw) < 1e-12) {
        return n;
    }

    let t = normalize(t_raw - n * dot(n, t_raw));
    let b = cross(n, t);

    let sampled = textureSample(normal_texture, base_sampler, input.uv).xyz * 2.0 - 1.0;
    let tilted = vec3<f32>(sampled.xy * normal_strength(), max(sampled.z, 1e-4));
    let world = normalize(t * tilted.x + b * tilted.y + n * tilted.z);

    if (dot(world, n) <= 0.0) {
        return n;
    }
    return world;
}

// ---- reflectance ------------------------------------------------------------
//
// The same model as world.wgsl, and `crates/kerosene-render/src/brdf.rs` is
// the CPU reference for both. A prop has a real light direction -- its key
// light -- so it gets the full GGX lobe as well as the even-environment term.

const PI: f32 = 3.14159265;
const DIELECTRIC_F0: f32 = 0.04;
const MIN_ROUGHNESS: f32 = 0.045;

fn f0_for(albedo: vec3<f32>, metalness: f32) -> vec3<f32> {
    return mix(vec3<f32>(DIELECTRIC_F0), albedo, metalness);
}

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

fn env_brdf(f0: vec3<f32>, roughness: f32, n_dot_v: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = clamp(roughness, 0.0, 1.0) * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * max(n_dot_v, 0.0))) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
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

// The fixed lighting a prop gets: a key light high and to the player's left,
// and an even ambient. The key's irradiance is written so a white Lambert
// surface facing it reads KEY_DIFFUSE -- the numbers this shader used before
// it had a BRDF, so props did not change brightness when it got one.
const KEY_DIFFUSE: f32 = 0.6;
const AMBIENT: f32 = 0.45;

@fragment
fn fs_model(input: VertexOut) -> @location(0) vec4<f32> {
    let albedo = textureSample(base_texture, base_sampler, input.uv);

    if (camera.params.w > 0.5) {
        // r_fullbright: show the material with no lighting at all.
        return vec4<f32>(albedo.rgb, 1.0);
    }

    let n = shading_normal(input);
    let key = normalize(vec3<f32>(0.35, 0.45, 0.85));
    var occlusion = 1.0;
    if (has_map(MAP_AO)) {
        occlusion = textureSample(ao_texture, base_sampler, input.uv).r;
    }
    let light = vec3<f32>((max(dot(n, key), 0.0) * KEY_DIFFUSE + AMBIENT) * occlusion);

    let metal = clamp(material.metalness * textureSample(metalness_texture, base_sampler, input.uv).r, 0.0, 1.0);
    var color = albedo.rgb * (1.0 - metal) * light;

    let roughness = textureSample(roughness_texture, base_sampler, input.uv).r * material.roughness_factor;
    let view = normalize(camera.position.xyz - input.world_position);
    let f0 = f0_for(albedo.rgb, metal);

    // As on the world, a dielectric with nothing said about roughness stays
    // matte and a metal always reflects.
    if ((has_map(MAP_ROUGHNESS) || metal > 0.0 || material.roughness_factor < 1.0) && specular_strength() > 0.0) {
        // Lambert is albedo / pi times irradiance, so a key that reads as
        // KEY_DIFFUSE on white has irradiance KEY_DIFFUSE * pi.
        let direct = specular_ggx(n, view, key, roughness, f0) * (KEY_DIFFUSE * PI);
        // The even ambient, or what the nearest probe actually sees.
        var environment = vec3<f32>(AMBIENT);
        if (input.probe != NO_PROBE) {
            environment = probe_radiance(input.probe, reflect(-view, n), roughness);
        }
        let ambient = env_brdf(f0, roughness, max(dot(n, view), 1e-4)) * environment;
        color = color + (direct + ambient) * occlusion * specular_strength();
    }

    color = color + dynamic_lighting(
        input.clip_position.xy,
        input.world_position,
        normalize(input.normal),
        n,
        view,
        albedo.rgb * (1.0 - metal),
        roughness,
        f0 * specular_strength(),
    );

    if (has_map(MAP_EMISSIVE)) {
        let emissive = textureSample(emissive_texture, base_sampler, input.uv).rgb;
        color = color + emissive * material.emissive_strength;
    }

    // Linear HDR out; the tone-map pass does the rest.
    return vec4<f32>(color, albedo.a);
}
