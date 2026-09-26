// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
// The game UI: every panel, glyph and image is one of these quads.
//
// `crates/kerosene-ui/src/draw.rs` defines what a quad is and
// `crates/kerosene-render/src/ui.rs` packs it; this draws it. Each instance is
// a rectangle with a corner radius, an optional border, a colour or
// two-colour gradient, an optional texture (the glyph atlas's coverage, or an
// image), an optional progress clip, and a 2D affine applied to its corners.
//
// Colours arrive as authored -- straight-alpha sRGB, as CSS writes them -- and
// are converted to linear here, so blending happens in linear light and the
// sRGB target encodes the result. On a target that is not sRGB the output is
// encoded by hand instead (`output_srgb`).

struct Screen {
    size: vec2<f32>,
    // 1 when the target is not an sRGB format and the shader must encode.
    output_srgb: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var ui_sampler: sampler;
@group(0) @binding(2) var glyph_atlas: texture_2d<f32>;
@group(1) @binding(0) var image: texture_2d<f32>;

struct QuadIn {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) color2: vec4<f32>,
    @location(4) border_color: vec4<f32>,
    // radius, border width, softness, fill amount
    @location(5) params: vec4<f32>,
    // texture mode (0 none, 1 glyphs, 2 image), gradient, fill kind, unused
    @location(6) modes: vec4<u32>,
    @location(7) transform_x: vec3<f32>,
    @location(8) transform_y: vec3<f32>,
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    // Position within the quad, in pixels from its top-left.
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) size: vec2<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) color2: vec4<f32>,
    @location(5) border_color: vec4<f32>,
    @location(6) @interpolate(flat) params: vec4<f32>,
    @location(7) @interpolate(flat) modes: vec4<u32>,
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}

fn linear_color(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(srgb_to_linear(c.rgb), c.a);
}

@vertex
fn vs_ui(@builtin(vertex_index) vertex: u32, quad: QuadIn) -> VertexOut {
    // Two triangles: 0-1-2, 2-1-3 over the corners
    // 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right.
    var corners = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let corner = corners[vertex];
    let unit = vec2<f32>(f32(corner & 1u), f32(corner >> 1u));

    let local = unit * quad.rect.zw;
    let p = quad.rect.xy + local;
    let moved = vec2<f32>(
        dot(quad.transform_x, vec3<f32>(p, 1.0)),
        dot(quad.transform_y, vec3<f32>(p, 1.0)),
    );
    // Pixels, top-left origin, to clip space.
    let ndc = vec2<f32>(moved.x / screen.size.x * 2.0 - 1.0, 1.0 - moved.y / screen.size.y * 2.0);

    var out: VertexOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.local = local;
    out.size = quad.rect.zw;
    out.uv = mix(quad.uv.xy, quad.uv.zw, unit);
    out.color = linear_color(quad.color);
    out.color2 = linear_color(quad.color2);
    out.border_color = linear_color(quad.border_color);
    out.params = quad.params;
    out.modes = quad.modes;
    return out;
}

// Signed distance to a rounded rectangle centred on the origin.
fn rounded_box(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(half.x, half.y));
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

const TAU: f32 = 6.28318530718;

@fragment
fn fs_ui(in: VertexOut) -> @location(0) vec4<f32> {
    // Both samples first, in uniform control flow, before anything branches
    // on a per-quad value or discards.
    let glyph = textureSample(glyph_atlas, ui_sampler, in.uv).r;
    let texel = textureSample(image, ui_sampler, in.uv);

    let radius = in.params.x;
    let border = in.params.y;
    let softness = in.params.z;
    let fill_amount = in.params.w;
    let texture_mode = in.modes.x;
    let gradient = in.modes.y;
    let fill_kind = in.modes.z;

    let half = in.size * 0.5;
    let p = in.local - half;

    // The progress clip: part of the quad, by angle or by edge.
    if (fill_kind == 1u) {
        // Clockwise from twelve o'clock.
        var angle = atan2(p.x, -p.y) / TAU;
        if (angle < 0.0) {
            angle = angle + 1.0;
        }
        if (angle > fill_amount) {
            discard;
        }
    } else if (fill_kind == 2u) {
        if (in.local.x > in.size.x * fill_amount) {
            discard;
        }
    } else if (fill_kind == 3u) {
        if (in.size.y - in.local.y > in.size.y * fill_amount) {
            discard;
        }
    }

    var color = in.color;
    if (gradient == 1u) {
        color = mix(in.color, in.color2, clamp(in.local.y / max(in.size.y, 1.0), 0.0, 1.0));
    } else if (gradient == 2u) {
        color = mix(in.color, in.color2, clamp(in.local.x / max(in.size.x, 1.0), 0.0, 1.0));
    }

    var coverage = 1.0;
    if (radius > 0.0 || softness > 0.0 || border > 0.0) {
        let d = rounded_box(p, half - vec2<f32>(softness), max(radius - softness, 0.0));
        if (softness > 0.0) {
            coverage = 1.0 - smoothstep(-softness, softness, d);
        } else {
            coverage = clamp(0.5 - d, 0.0, 1.0);
        }
        if (border > 0.0) {
            // Inside the border band the border colour takes over, with a
            // pixel of blend at its inner edge.
            let t = clamp(d + border + 0.5, 0.0, 1.0);
            color = mix(color, in.border_color, t);
        }
    }

    if (texture_mode == 1u) {
        coverage = coverage * glyph;
    } else if (texture_mode == 2u) {
        color = color * texel;
    }

    let alpha = color.a * coverage;
    var rgb = color.rgb;
    if (screen.output_srgb == 1u) {
        rgb = linear_to_srgb(rgb);
    }
    // Premultiplied: the blend state is (one, one-minus-source-alpha), or
    // (one, one) for additive panels.
    return vec4<f32>(rgb * alpha, alpha);
}
