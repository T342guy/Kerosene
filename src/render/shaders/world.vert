// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#version 450

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec2 in_uv;
layout(location = 2) in vec3 in_normal;

// SDL_GPU's SPIR-V convention: vertex uniform buffers live in set 1.
layout(set = 1, binding = 0) uniform Camera {
    mat4 view_projection;
} camera;

layout(location = 0) out vec2 out_uv;
layout(location = 1) out vec3 out_normal;

void main() {
    out_uv = in_uv;
    out_normal = in_normal;
    gl_Position = camera.view_projection * vec4(in_position, 1.0);
}
