// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#version 450

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec4 in_colour;

layout(set = 1, binding = 0) uniform Camera {
    mat4 view_projection;
    vec4 tint;          // Multiplied into every vertex colour.
} camera;

layout(location = 0) out vec4 out_colour;

void main() {
    out_colour = in_colour * camera.tint;
    gl_Position = camera.view_projection * vec4(in_position, 1.0);
}
