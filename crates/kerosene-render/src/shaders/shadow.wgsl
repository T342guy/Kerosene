// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
// Shadow maps: depth from a light's point of view, and nothing else.
//
// One entry point serves world geometry and studio models alike: both put
// their position at location 0, and the vertex buffer layout -- which the
// pipeline sets, not the shader -- skips whatever comes after it. There is no
// fragment stage; the rasteriser writes depth on its own.

struct ShadowView {
    view_proj: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> shadow_view: ShadowView;

// The same per-model uniform the scene draws with, at the same dynamic
// offset, so a door casts its shadow from where it has swung to.
struct Model {
    transform: mat4x4<f32>,
    probe: vec4<u32>,
};
@group(1) @binding(0) var<uniform> model: Model;

@vertex
fn vs_shadow(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return shadow_view.view_proj * (model.transform * vec4<f32>(position, 1.0));
}

// The bone palette: one matrix per bone, bind pose to posed. Slot 0 is all
// identities, which is what a static model binds.
struct Bones {
    palette: array<mat4x4<f32>, 128>,
};
@group(2) @binding(0) var<uniform> bones: Bones;

// An animated studio model: skinned, then placed.
@vertex
fn vs_shadow_skinned(
    @location(0) position: vec3<f32>,
    @location(8) joints: vec4<u32>,
    @location(9) weights: vec4<f32>,
) -> @builtin(position) vec4<f32> {
    let skin = bones.palette[joints.x] * weights.x
        + bones.palette[joints.y] * weights.y
        + bones.palette[joints.z] * weights.z
        + bones.palette[joints.w] * weights.w;
    return shadow_view.view_proj * (model.transform * skin * vec4<f32>(position, 1.0));
}

// Static props, instanced: the transform comes from the instance buffer.
@vertex
fn vs_shadow_instanced(
    @location(0) position: vec3<f32>,
    @location(3) transform_0: vec4<f32>,
    @location(4) transform_1: vec4<f32>,
    @location(5) transform_2: vec4<f32>,
    @location(6) transform_3: vec4<f32>,
) -> @builtin(position) vec4<f32> {
    let transform = mat4x4<f32>(transform_0, transform_1, transform_2, transform_3);
    return shadow_view.view_proj * (transform * vec4<f32>(position, 1.0));
}
