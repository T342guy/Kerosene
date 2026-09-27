// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
// Chisel's 3D pane: textured faces, lines, and the blit into the window.

struct Camera {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var face_texture: texture_2d<f32>;
@group(1) @binding(1) var face_sampler: sampler;

struct FaceIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) tint: vec4<f32>,
};

struct FaceOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) tint: vec4<f32>,
};

@vertex
fn vs_face(v: FaceIn) -> FaceOut {
    var out: FaceOut;
    out.clip = camera.view_proj * vec4<f32>(v.position, 1.0);
    out.normal = v.normal;
    out.uv = v.uv;
    out.color = v.color;
    out.tint = v.tint;
    return out;
}

// The same flat shading the software rasteriser uses (`raster::shading_for`):
// enough to tell three faces at a corner apart, never so dark that a
// texture on a north wall cannot be read.
const LIGHT: vec3<f32> = vec3<f32>(0.4, 0.3, 0.87);
const AMBIENT: f32 = 0.62;

@fragment
fn fs_face(in: FaceOut) -> @location(0) vec4<f32> {
    let texel = textureSample(face_texture, face_sampler, in.uv);
    // Alpha-tested materials: a grate is holes, not a grey sheet.
    if (texel.a < 0.5) {
        discard;
    }
    let base = texel * in.color;
    var rgb = mix(base.rgb, in.tint.rgb, in.tint.a);
    // A zero normal is a helper volume, which glows evenly.
    if (dot(in.normal, in.normal) > 0.25) {
        let facing = clamp(dot(normalize(in.normal), normalize(LIGHT)) * 0.5 + 0.5, 0.0, 1.0);
        rgb = rgb * (AMBIENT + (1.0 - AMBIENT) * facing);
    }
    return vec4<f32>(rgb, base.a);
}

struct LineIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(v: LineIn) -> LineOut {
    var out: LineOut;
    out.clip = camera.view_proj * vec4<f32>(v.position, 1.0);
    out.color = v.color;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return in.color;
}

// The resolved pane, copied into egui's pass over the pane's rectangle.
@group(0) @binding(0) var pane_texture: texture_2d<f32>;
@group(0) @binding(1) var pane_sampler: sampler;

struct BlitOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_blit(@builtin(vertex_index) index: u32) -> BlitOut {
    // One triangle that covers the viewport.
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    var out: BlitOut;
    out.clip = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

@fragment
fn fs_blit(in: BlitOut) -> @location(0) vec4<f32> {
    return textureSample(pane_texture, pane_sampler, in.uv);
}
