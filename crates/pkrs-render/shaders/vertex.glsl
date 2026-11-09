#version 100
// Engine vertex stage shared by every material (§5.6). Material fragment shaders receive
// `uv` (texture coordinates) and `color` (the sprite tint, 0..1) and sample `Texture`.
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying mediump vec2 uv;
varying lowp vec4 color;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}
