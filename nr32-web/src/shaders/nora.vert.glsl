#version 300 es

in ivec3 a_position;
in uvec4 a_color;
in uint a_projection_index;

out vec4 v_color;

uniform mat4 u_projections[32];

void main() {
    mat4 m = u_projections[a_projection_index];
    gl_Position = m * vec4(a_position, 1.);
    v_color = vec4(a_color) / 255.;
}
