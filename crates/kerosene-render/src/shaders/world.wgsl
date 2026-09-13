// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
// World surface shading.
//
// Diffuse lighting is entirely baked: the lightmap atlas already holds the
// result of every light, bounce and shadow that Radiance computed. The
// fragment shader's job is to combine it with the material and tone-map, not
// to light anything. That is the whole bargain of a BSP engine -- expensive
// lighting, computed once, at build time.
//
// What the material still gets a say in is everything the bake could not know:
// which way the surface actually faces at texel scale (the normal map), how
// tight its highlight is (roughness), what it shadows itself (occlusion), and
// what it emits regardless of any of that (emissive). Those are per-texel and
// view-dependent, so they belong here rather than in the atlas.

struct Camera {
    view_proj: mat4x4<f32>,
    position: vec4<f32>,
    // x: exposure, y: time, z: lightmap enable, w: fullbright
    params: vec4<f32>,
    // Colour the sky renders, from light_environment.
    sky_color: vec4<f32>,
    // x: normal-map scale (r_bumpmap), y: specular scale (r_specular)
    render: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var lightmap_texture: texture_2d<f32>;
@group(0) @binding(2) var lightmap_sampler: sampler;

// A material is one sampler, a word saying which of its maps are real, and
// five textures. The absent ones are bound to a neutral 1x1 texel, so every
// material fills the same layout and there is one pipeline rather than a
// variant per combination of maps.
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
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) lightmap_uv: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) world_position: vec3<f32>,
    @location(4) tangent: vec4<f32>,
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
    return out;
}

// Lightmaps are stored tone-mapped into 0..1, so a surface lit to "full"
// reads as 0.5-ish. Scaling back up here restores the range without needing
// a floating-point atlas.
const LIGHTMAP_SCALE: f32 = 2.0;

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

// A highlight the baked lighting cannot give us.
//
// The lightmap is diffuse: Radiance integrated light arriving at the surface,
// with no idea where the viewer would eventually stand. So specular is added
// here, from the one direction we do know something about -- the surface's own
// normal against the view -- rather than from lights that no longer exist by
// the time anything is drawn. It is a cheap Blinn-Phong lobe steered by
// roughness, not a physically-based one; what it buys is that a smooth surface
// reads as smooth instead of reading as a matte surface with a smooth texture.
fn specular(n: vec3<f32>, view: vec3<f32>, light: vec3<f32>, roughness: f32) -> vec3<f32> {
    if (specular_strength() <= 0.0) {
        return vec3<f32>(0.0);
    }
    // Treat the light as arriving along the normal -- the best guess available
    // once lighting is baked -- so the half vector is between the normal and
    // the eye.
    let h = normalize(n + view);
    let gloss = clamp(1.0 - roughness, 0.0, 1.0);
    // 2..2048, so "smooth" is a tight highlight and "rough" is barely a lobe.
    let power = exp2(1.0 + gloss * gloss * 10.0);
    let lobe = pow(max(dot(n, h), 0.0), power);

    // Schlick, at a dielectric's 4% reflectance: no material says otherwise
    // yet, and a metalness map is the next thing to add here.
    let f = 0.04 + 0.96 * pow(1.0 - max(dot(n, view), 0.0), 5.0);

    return light * lobe * f * gloss * specular_strength();
}

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

    // Occlusion darkens what the surface shadows itself, which the lightmap
    // cannot: its luxels are far too coarse to see into a crevice.
    if (has_map(MAP_AO)) {
        let ao = textureSample(ao_texture, base_sampler, input.uv).r;
        light = light * ao;
    }

    var color = albedo.rgb * light * camera.params.x;

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
    // does something on a wall with no roughness map -- the alternative being
    // that it does nothing at all and looks broken.
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

    // A surface with no roughness map is fully rough, so it gets no highlight
    // at all and renders exactly as it did before any of this existed.
    if (has_map(MAP_ROUGHNESS)) {
        let roughness = textureSample(roughness_texture, base_sampler, input.uv).r;
        color = color + specular(n, view, light * camera.params.x, roughness);
    }

    // Emissive is added after lighting and before tone-mapping: it is light
    // leaving the surface, so it should bloom out the same way a lit highlight
    // does rather than being pasted on at full strength afterwards.
    if (has_map(MAP_EMISSIVE)) {
        let emissive = textureSample(emissive_texture, base_sampler, input.uv).rgb;
        color = color + emissive * material.emissive_strength * camera.params.x;
    }

    // Reinhard, so a bright highlight keeps its shape instead of clipping to
    // a flat white blob.
    color = color / (color + vec3<f32>(1.0));
    // No gamma step here: the swapchain is an sRGB format, so the hardware
    // encodes on write. Doing it here as well encoded twice and washed the
    // whole image out -- survivable while albedo was also being sampled
    // wrong, because the two errors pulled in opposite directions, but not
    // once the textures started being decoded correctly.

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
    // Straight through, and correct: the texture is sampled through an sRGB
    // view so this is linear, and the sRGB swapchain encodes it on write.
    let albedo = textureSample(base_texture, base_sampler, input.uv);
    return vec4<f32>(albedo.rgb, albedo.a);
}
