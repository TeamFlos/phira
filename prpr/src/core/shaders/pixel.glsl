#version 100
// Adapted from https://godotshaders.com/shader/pixelate-2/
precision mediump float;

varying lowp vec2 uv;
uniform vec2 screenSize;
uniform sampler2D screenTexture;

uniform float size; // %10.0%

void main() {
  vec2 factor = screenSize / size;
  float x = floor(uv.x * factor.x + 0.5) / factor.x;
  float y = floor(uv.y * factor.y + 0.5) / factor.y;
  gl_FragColor = texture2D(screenTexture, vec2(x, y));
}
