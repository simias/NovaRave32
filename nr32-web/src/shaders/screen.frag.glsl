#version 300 es
precision mediump float;
in vec2 v_tex_coord;
out vec4 fragColor;

uniform sampler2D u_screen_texture;

void main() {
  vec3 color = texture(u_screen_texture, v_tex_coord).rgb;

  float gamma = 1.0 / 2.2;
  fragColor = vec4(pow(color, vec3(gamma)), 1.0);
}
