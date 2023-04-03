#version 100
// Adapted from https://www.shadertoy.com/view/llj3Dz
precision mediump float;

varying lowp vec2 uv;
uniform vec2 screenSize;
uniform sampler2D screenTexture;

uniform float progress; // %0.2% 0..1
uniform float centerX; // %0.5% 0..1
uniform float centerY; // %0.5% 0..1
uniform float width; // %0.1%
uniform float distortion; // %0.8%
uniform float expand; // %10.0%

void main() {
  float aspect = screenSize.y / screenSize.x;

  vec2 center = vec2(centerX, centerY);
  center.y = (center.y - 0.5) * aspect + 0.5;

  vec2 tex_coord = uv;
    tex_coord.y = (tex_coord.y - 0.5) * aspect + 0.5;
  float dist = distance(tex_coord, center);

  if (progress - width <= dist && dist <= progress + width) {
    float diff = dist - progress;
    float scale_diff = 1.0 - pow(abs(diff * expand), distortion);
    float dt = diff * scale_diff;

    vec2 dir = normalize(tex_coord - center);

    tex_coord += ((dir * dt) / (progress * dist * 40.0));
    gl_FragColor = texture2D(screenTexture, vec2(tex_coord.x, (tex_coord.y - 0.5) / aspect + 0.5));

    gl_FragColor += (gl_FragColor * scale_diff) / (progress * dist * 40.0);
  } else {
    gl_FragColor = texture2D(screenTexture, vec2(tex_coord.x, (tex_coord.y - 0.5) / aspect + 0.5));
  }
}