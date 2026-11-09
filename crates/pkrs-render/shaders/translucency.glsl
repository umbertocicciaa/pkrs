#version 100
// pkrs/translucency: draws the sprite with its alpha scaled by `alpha` (water surfaces,
// ghosts, glass).
precision mediump float;

varying mediump vec2 uv;
varying lowp vec4 color;

uniform sampler2D Texture;
uniform float alpha;

void main() {
    vec4 base = texture2D(Texture, uv) * color;
    gl_FragColor = vec4(base.rgb, base.a * clamp(alpha, 0.0, 1.0));
}
