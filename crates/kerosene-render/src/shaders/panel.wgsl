// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
// World panels: a UI document's texture, on a quad in the level.
//
// Drawn in the HDR scene pass with the depth test, so a wall in front of a
// screen hides it. The panel is emissive -- a screen lights itself -- and its
// brightness is on the scene's linear scale, so the tone-mapper treats it like
// any other lit surface. `crates/kerosene-render/src/ui.rs` builds the quads.

struct View {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var panel_sampler: sampler;
@group(1) @binding(0) var panel_texture: texture_2d<f32>;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) brightness: f32,
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) brightness: f32,
}

@vertex
fn vs_panel(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = view.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.brightness = in.brightness;
    return out;
}

@fragment
fn fs_panel(in: VertexOut) -> @location(0) vec4<f32> {
    // The panel texture is premultiplied, as the UI pass wrote it.
    let texel = textureSample(panel_texture, panel_sampler, in.uv);
    return vec4<f32>(texel.rgb * in.brightness, texel.a);
}
