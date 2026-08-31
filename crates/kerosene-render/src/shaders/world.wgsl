// SPDX-License-Identifier: MPL-2.0
// World surface shading.
//
// Lighting is entirely baked: the lightmap atlas already holds the result of
// every light, bounce and shadow that Radiance computed. The fragment shader's
// job is to combine it with the material and tone-map, not to light anything.
// That is the whole bargain of a BSP engine -- expensive lighting, computed
// once, at build time.

struct Camera {
    view_proj: mat4x4<f32>,
    position: vec4<f32>,
    // x: exposure, y: time, z: lightmap enable, w: fullbright
    params: vec4<f32>,
    // Colour the sky renders, from light_environment.
    sky_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var lightmap_texture: texture_2d<f32>;
@group(0) @binding(2) var lightmap_sampler: sampler;

@group(1) @binding(0) var base_texture: texture_2d<f32>;
@group(1) @binding(1) var base_sampler: sampler;

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
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) lightmap_uv: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) world_position: vec3<f32>,
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
    // direction must not have. Nothing samples this yet -- lighting is baked
    // -- but a normal that silently pointed the wrong way would be found by
    // whoever adds bump mapping rather than by anyone here.
    out.normal = (model.transform * vec4<f32>(input.normal, 0.0)).xyz;
    out.world_position = world;
    return out;
}

// Lightmaps are stored tone-mapped into 0..1, so a surface lit to "full"
// reads as 0.5-ish. Scaling back up here restores the range without needing
// a floating-point atlas.
const LIGHTMAP_SCALE: f32 = 2.0;

// A floor of ambient light so a surface with no lightmap is dim rather than
// pure black. An unlit room should look unlit, not look broken.
const MIN_AMBIENT: f32 = 0.06;

@fragment
fn fs_world(input: VertexOut) -> @location(0) vec4<f32> {
    let albedo = textureSample(base_texture, base_sampler, input.uv);

    if (camera.params.w > 0.5) {
        // r_fullbright: show the materials with no lighting at all, which is
        // how you tell a lighting bug from a texture bug.
        return vec4<f32>(albedo.rgb, 1.0);
    }

    var light = vec3<f32>(MIN_AMBIENT);
    if (camera.params.z > 0.5) {
        let sampled = textureSample(lightmap_texture, lightmap_sampler, input.lightmap_uv).rgb;
        light = max(sampled * LIGHTMAP_SCALE, vec3<f32>(MIN_AMBIENT));
    }

    var color = albedo.rgb * light * camera.params.x;

    // Reinhard, so a bright highlight keeps its shape instead of clipping to
    // a flat white blob.
    color = color / (color + vec3<f32>(1.0));
    // Back to sRGB for display.
    color = pow(color, vec3<f32>(1.0 / 2.2));

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
    let albedo = textureSample(base_texture, base_sampler, input.uv);
    return vec4<f32>(albedo.rgb, albedo.a);
}
