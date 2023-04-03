#version 100
// Adapted from https://godotshaders.com/shader/glitch-effect-shader/
precision highp float;

varying lowp vec2 uv;
uniform sampler2D screenTexture;
uniform float time;

uniform float power; // %0.03%
uniform float rate; // %0.6% 0..1
uniform float speed; // %5.0%
uniform float blockCount; // %30.5%
uniform float colorRate; // %0.01% 0..1

float my_trunc(float x) {
  return x < 0.0? -floor(-x): floor(x);
}

float random(float seed) {
  return fract(543.2543 * sin(dot(vec2(seed, seed), vec2(3525.46, -54.3415))));
}

void main() {
  float enable_shift = float(random(my_trunc(time * speed)) < rate);

  vec2 fixed_uv = uv;
  fixed_uv.x += (random((my_trunc(uv.y * blockCount) / blockCount) + time) - 0.5) * power * enable_shift;

  vec4 pixel_color = texture2D(screenTexture, fixed_uv);
  pixel_color.r = mix(
    pixel_color.r,
    texture2D(screenTexture, fixed_uv + vec2(colorRate, 0.0)).r,
    enable_shift
  );
  pixel_color.b = mix(
    pixel_color.b,
    texture2D(screenTexture, fixed_uv + vec2(-colorRate, 0.0)).b,
    enable_shift
  );
  gl_FragColor = pixel_color;
}
