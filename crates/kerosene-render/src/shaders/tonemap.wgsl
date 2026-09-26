// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
// The last step of a frame: HDR scene colour to the display.
//
// Everything before this works in linear light with no ceiling -- a lightmap
// can hold ten times "full", a highlight can be brighter than the surface
// under it -- and the swapchain holds 0..1. This is the one place that range
// is folded down, so it happens once, after everything has been added up,
// rather than per surface where two bright things drawn over each other each
// got compressed on their own and the sum was wrong.
//
// Exposure lives here for the same reason. It is a property of the camera,
// not of any surface, and applying it once means a lightmap, an emissive and
// a sky all move together when it changes.

struct ToneMap {
    exposure: f32,
    // 0: none (clamp), 1: Reinhard, 2: ACES.
    curve: u32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var<uniform> tonemap: ToneMap;

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
};

// One triangle that covers the screen: cheaper than a quad, since a quad's
// diagonal is a seam the GPU shades twice.
@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> VertexOut {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOut;
    out.clip_position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    return out;
}

// Narkowicz's fit of the ACES reference rendering transform. The 0.6 pre-scale
// is his: the fit maps scene white to display white rather than to ACES's
// darker mid-grey, which is what someone used to a Reinhard curve expects to
// see at `mat_exposure 1`. A filmic toe keeps shadows dense and a shoulder
// rolls highlights off instead of clipping them, which is most of what
// "looks like a modern engine" means for a tone curve.
fn aces(x: vec3<f32>) -> vec3<f32> {
    let v = x * 0.6;
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((v * (a * v + b)) / (v * (c * v + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn reinhard(x: vec3<f32>) -> vec3<f32> {
    return x / (x + vec3<f32>(1.0));
}

@fragment
fn fs_tonemap(input: VertexOut) -> @location(0) vec4<f32> {
    // Same size as the target, so a load rather than a filtered sample: there
    // is nothing between texels to filter.
    let hdr = textureLoad(scene, vec2<i32>(input.clip_position.xy), 0).rgb;
    let exposed = max(hdr, vec3<f32>(0.0)) * tonemap.exposure;

    var color: vec3<f32>;
    switch tonemap.curve {
        case 0u: {
            color = clamp(exposed, vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 1u: {
            color = reinhard(exposed);
        }
        default: {
            color = aces(exposed);
        }
    }
    // No gamma step: the swapchain is sRGB and the hardware encodes on write.
    return vec4<f32>(color, 1.0);
}
