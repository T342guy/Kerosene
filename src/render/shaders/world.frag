// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#version 450

layout(location = 0) in vec2 in_uv;
layout(location = 1) in vec3 in_normal;

// Fragment samplers live in set 2, fragment uniforms in set 3.
layout(set = 2, binding = 0) uniform sampler2D base_texture;

layout(set = 3, binding = 0) uniform Shading {
    vec4 tint;
    vec4 key_direction;   // xyz direction, w unused
} shading;

layout(location = 0) out vec4 out_colour;

void main() {
    vec3 albedo = texture(base_texture, in_uv).rgb * shading.tint.rgb;

    // Placeholder shading until Radiance bakes lightmaps. A single key
    // direction plus a floor of ambient, purely so that surfaces at different
    // angles are distinguishable while the geometry is being debugged. It is
    // deliberately flat and deliberately obvious: nobody should mistake this
    // for lighting.
    vec3 normal = normalize(in_normal);
    float key = max(dot(normal, normalize(shading.key_direction.xyz)), 0.0);
    float ambient = 0.55;
    out_colour = vec4(albedo * (ambient + 0.45 * key), 1.0);
}
