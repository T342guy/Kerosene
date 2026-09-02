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
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var lightmap_texture: texture_2d<f32>;
@group(0) @binding(2) var lightmap_sampler: sampler;

@group(1) @binding(0) var base_texture: texture_2d<f32>;
@group(1) @binding(1) var base_sampler: sampler;

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

@fragment
fn fs_model(input: VertexOut) -> @location(0) vec4<f32> {
    let albedo = textureSample(base_texture, base_sampler, input.uv);

    if (camera.params.w > 0.5) {
        // r_fullbright: show the material with no lighting at all.
        return vec4<f32>(albedo.rgb, 1.0);
    }

    // A fixed key light high and to the player's left, plus ambient. Enough
    // for a prop to show its shape; nothing dynamic samples it.
    let n = normalize(input.normal);
    let key = normalize(vec3<f32>(0.35, 0.45, 0.85));
    let light = max(dot(n, key), 0.0) * 0.6 + 0.45;

    var color = albedo.rgb * light * camera.params.x;
    color = color / (color + vec3<f32>(1.0));
    color = pow(color, vec3<f32>(1.0 / 2.2));

    return vec4<f32>(color, albedo.a);
}
