#version 100
// Adapted from https://godotshaders.com/shader/screen-noise-effect-shader/
precision highp float;

varying lowp vec2 uv;
uniform sampler2D screenTexture;

uniform float seed; // %81.0%
uniform float power; // %0.03% 0..1

vec2 random(vec2 pos) {
  return fract(sin(vec2(dot(pos, vec2(12.9898,78.233)), dot(pos, vec2(-148.998,-65.233)))) * 43758.5453);
}

void main() {
  vec2 new_uv = uv + (random(uv + vec2(seed, 0.0)) - vec2(0.5, 0.5)) * power;
  gl_FragColor = texture2D(screenTexture, new_uv);
}
