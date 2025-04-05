#version 300 es
precision mediump float;

in vec4 v_color;
out vec4 fragColor;

const float ditherMatrix[16] = float[16](
-0.5,   0.,   -0.375, 0.125,
0.25,  -0.25,  0.375, -0.125,
-0.375, 0.125, -0.5, 0.,
0.375, -0.125, 0.25, -0.25);

/* Artificially reduce color component resolution to 5 bits (to simulate
 * RGB555) and add dithering */
vec4 dither(vec4 color, vec2 fragCoord) {
    int xbias = int(mod(fragCoord.x, 4.));
    int ybias = int(mod(fragCoord.y, 4.));

    float bias = ditherMatrix[xbias + ybias *4];

    return vec4(
        round(color.r * 32. + bias) / 32.,
        round(color.g * 32. + bias) / 32.,
        round(color.b * 32. + bias) / 32.,
        color.a
        );
}

void main() {
    fragColor = dither(v_color, gl_FragCoord.xy);
}
