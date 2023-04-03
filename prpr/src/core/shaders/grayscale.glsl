# version 100
// Adapted from https://www.shadertoy.com/view/lsdXDH
precision mediump float;

varying lowp vec2 uv;
uniform sampler2D screenTexture;

uniform float factor; // %1.0% 0..1

void main() {
  vec3 color = texture2D(screenTexture, uv).xyz;
  vec3 lum = vec3(0.299, 0.587, 0.114);
  vec3 gray = vec3(dot(lum, color));
  gl_FragColor = vec4(mix(color, gray, factor), 1.0);
}
