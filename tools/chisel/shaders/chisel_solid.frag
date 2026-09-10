// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#version 450

layout(location = 0) in vec2 in_uv;
layout(location = 1) in vec3 in_normal;

layout(set = 2, binding = 0) uniform sampler2D base_texture;

layout(set = 3, binding = 0) uniform Shading {
    vec4 tint;
    vec4 key_direction;
} shading;

layout(location = 0) out vec4 out_colour;

void main() {
    vec3 albedo = texture(base_texture, in_uv).rgb * shading.tint.rgb;

    // The same flat key the engine uses until Radiance exists, so a surface
    // reads the same in the editor as it will in the game. An editor that lit
    // things differently would be teaching the wrong lesson about the level.
    vec3 normal = normalize(in_normal);
    float key = max(dot(normal, normalize(shading.key_direction.xyz)), 0.0);
    out_colour = vec4(albedo * (0.55 + 0.45 * key), shading.tint.a);
}
