// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
// Debug wireframe lines.
//
// The physics debug view and other overlays draw plain, camera-space lines --
// no lighting, no texture, just a colour per vertex.

struct Camera {
    view_proj: mat4x4<f32>,
    position: vec4<f32>,
    params: vec4<f32>,
    sky_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_line(input: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_position = camera.view_proj * vec4<f32>(input.position, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_line(input: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color, 1.0);
}
