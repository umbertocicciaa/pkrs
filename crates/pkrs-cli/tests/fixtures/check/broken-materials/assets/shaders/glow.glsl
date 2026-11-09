#version 330
precision mediump float;

varying mediump vec2 uv;

uniform sampler2D Texture;
uniform sampler2D Noise;
uniform float strength;
uniform vec4 color;
uniform float time_ticks;

void main() {
    gl_FragColor = texture2D(Texture, uv) * color * strength;
}
