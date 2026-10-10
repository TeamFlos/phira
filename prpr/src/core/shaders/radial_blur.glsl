#version 100
// Adapted from https://godotshaders.com/shader/radical-blur-shader/
precision mediump float;

varying lowp vec2 uv;
uniform sampler2D screenTexture;

uniform float centerX; // %0.5% 0..1
uniform float centerY; // %0.5% 0..1
uniform float power; // %0.01% 0..1
uniform float sampleCount; // %6% int 1..64

void main() {
  vec2 direction = uv - vec2(centerX, centerY);
  vec3 c = vec3(0.0);
  float f = 1.0 / sampleCount;
  for (float i = 0.0; i < 64.0; ++i) {
    if (i >= sampleCount) break;
    c += texture2D(screenTexture, uv - power * direction * i).rgb * f;
  }
  gl_FragColor = vec4(c, 1.0);
}
