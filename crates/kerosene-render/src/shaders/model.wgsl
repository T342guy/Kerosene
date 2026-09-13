// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
// Model shading.
//
// Brush geometry carries baked lightmaps; models do not. A prop has to look
// lit on its own, so it gets a fixed key light plus a little ambient -- enough
// to read as a solid object sitting in the lit world, without pretending to
// be a lightmapped surface.

struct Camera {
    view_proj: mat4x4<f32>,
    position: vec4<f32>,
    // x: exposure, y: time, z: lightmap enable, w: fullbright
    params: vec4<f32>,
    sky_color: vec4<f32>,
    // x: normal-map scale (r_bumpmap), y: specular scale (r_specular)
    render: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var lightmap_texture: texture_2d<f32>;
@group(0) @binding(2) var lightmap_sampler: sampler;

// The same material layout the world uses -- one pipeline layout serves both,
// so this is not optional even where a map goes unread.
@group(1) @binding(0) var base_sampler: sampler;

struct MaterialParams {
    // Bit 0 base, 1 normal, 2 roughness, 3 emissive, 4 occlusion.
    present: u32,
    emissive_strength: f32,
    normal_strength: f32,
    specular_strength: f32,
};
@group(1) @binding(1) var<uniform> material: MaterialParams;

@group(1) @binding(2) var base_texture: texture_2d<f32>;
@group(1) @binding(3) var normal_texture: texture_2d<f32>;
@group(1) @binding(4) var roughness_texture: texture_2d<f32>;
@group(1) @binding(5) var emissive_texture: texture_2d<f32>;
@group(1) @binding(6) var ao_texture: texture_2d<f32>;

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

@fragment
fn fs_model(input: VertexOut) -> @location(0) vec4<f32> {
    let albedo = textureSample(base_texture, base_sampler, input.uv);

    if (camera.params.w > 0.5) {
        // r_fullbright: show the material with no lighting at all.
        return vec4<f32>(albedo.rgb, 1.0);
    }

    // A fixed key light high and to the player's left, plus ambient. Enough
    // for a prop to show its shape; nothing dynamic samples it.
    let n = shading_normal(input);
    let key = normalize(vec3<f32>(0.35, 0.45, 0.85));
    var light = vec3<f32>(max(dot(n, key), 0.0) * 0.6 + 0.45);

    if (has_map(MAP_AO)) {
        light = light * textureSample(ao_texture, base_sampler, input.uv).r;
    }

    var color = albedo.rgb * light * camera.params.x;

    // A prop has a real light direction to reflect, unlike a lightmapped wall,
    // so its highlight comes from the key light rather than from a guess.
    if (has_map(MAP_ROUGHNESS) && specular_strength() > 0.0) {
        let roughness = textureSample(roughness_texture, base_sampler, input.uv).r;
        let view = normalize(camera.position.xyz - input.world_position);
        let h = normalize(key + view);
        let gloss = clamp(1.0 - roughness, 0.0, 1.0);
        let power = exp2(1.0 + gloss * gloss * 10.0);
        let lobe = pow(max(dot(n, h), 0.0), power);
        let f = 0.04 + 0.96 * pow(1.0 - max(dot(n, view), 0.0), 5.0);
        color = color + light * lobe * f * gloss * specular_strength() * camera.params.x;
    }

    if (has_map(MAP_EMISSIVE)) {
        let emissive = textureSample(emissive_texture, base_sampler, input.uv).rgb;
        color = color + emissive * material.emissive_strength * camera.params.x;
    }

    color = color / (color + vec3<f32>(1.0));
    // No gamma step here: the swapchain is an sRGB format, so the hardware
    // encodes on write. Doing it here as well encoded twice and washed the
    // whole image out -- survivable while albedo was also being sampled
    // wrong, because the two errors pulled in opposite directions, but not
    // once the textures started being decoded correctly.

    return vec4<f32>(color, albedo.a);
}
