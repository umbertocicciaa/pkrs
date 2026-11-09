#version 100
// Pulses the sprite's opacity around `opacity` with the engine's `time_ticks`.
precision mediump float;

varying mediump vec2 uv;
varying lowp vec4 color;

uniform sampler2D Texture;
uniform float time_ticks;
uniform float opacity;

void main() {
    vec4 base = texture2D(Texture, uv) * color;
    float pulse = 0.15 * sin(time_ticks / 20.0);
    gl_FragColor = vec4(base.rgb, base.a * clamp(opacity + pulse, 0.0, 1.0));
}
