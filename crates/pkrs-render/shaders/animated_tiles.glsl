#version 100
// pkrs/animated_tiles: samples the current animation frame. The engine resolves `frame`
// from `time_ticks`, `frames` and `ticks_per_frame` every frame; frames sit `frame_offset`
// pixels apart in the sheet. `tint` multiplies the result (white by default).
precision mediump float;

varying mediump vec2 uv;
varying lowp vec4 color;

uniform sampler2D Texture;
uniform float frame;
uniform vec2 frame_offset;
uniform vec2 texture_size;
uniform vec4 tint;

void main() {
    vec2 shifted = uv + frame * frame_offset / max(texture_size, vec2(1.0));
    gl_FragColor = texture2D(Texture, shifted) * color * tint;
}
