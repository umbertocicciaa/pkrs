#version 100
// pkrs/tint: multiplies the sprite by `tint`, blended in by `strength` (day/night cycles,
// palette moods). Alpha is kept.
precision mediump float;

varying mediump vec2 uv;
varying lowp vec4 color;

uniform sampler2D Texture;
uniform vec4 tint;
uniform float strength;

void main() {
    vec4 base = texture2D(Texture, uv) * color;
    vec3 tinted = mix(base.rgb, base.rgb * tint.rgb, clamp(strength, 0.0, 1.0));
    gl_FragColor = vec4(tinted, base.a);
}
