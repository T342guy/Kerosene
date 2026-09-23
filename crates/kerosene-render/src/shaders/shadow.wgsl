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
