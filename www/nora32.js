import init, { NoRa32 } from "./pkg/novarave32.js";

redirectConsole("nora32-console");

const wasm = await init();

const canvas = document.getElementById('nora32-screen');
canvas.style['image-rendering'] = 'pixelated';
const gl = canvas.getContext('webgl2', { antialias: false });
const f32_buffer = gl.createBuffer();
const u8_buffer = gl.createBuffer();

async function start() {

    const vshader = compileShader(gl, gl.VERTEX_SHADER, noRaVertexShader);
    const fshader = compileShader(gl, gl.FRAGMENT_SHADER, noRaFragmentShader);

    const program = gl.createProgram();
    gl.attachShader(program, vshader);
    gl.attachShader(program, fshader);
    gl.linkProgram(program);

    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
        console.error("Program linking failed:", gl.getProgramInfoLog(program));
    }
    gl.useProgram(program);


    const positionLocation = gl.getAttribLocation(program, "a_position");
    const colorLocation = gl.getAttribLocation(program, "a_color");

    gl.enableVertexAttribArray(positionLocation);
    gl.enableVertexAttribArray(colorLocation);

    gl.bindBuffer(gl.ARRAY_BUFFER, f32_buffer);
    const stride = Float32Array.BYTES_PER_ELEMENT * 4;
    gl.vertexAttribPointer(positionLocation, 4, gl.FLOAT, false, stride, 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, u8_buffer);
    gl.vertexAttribIPointer(colorLocation, 4, gl.UNSIGNED_BYTE, 4, 0);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.clearColor(0.0, 0.0, 0.0, 1.0);


    let rom = await fetchROM("./pkg/ROM.BIN");

    let nora32 = new NoRa32();

    nora32.load_rom(rom);

    nora32.run_frame();
}

window.drawTriangles3D = function (f32_ptr, u8_ptr, count) {
    console.log(`DRAW TRIANGLES called with ${count} vertices`);

    let f32data = new Float32Array(wasm.memory.buffer, f32_ptr, count * 4);
    let u8data = new Uint8Array(wasm.memory.buffer, u8_ptr, count * 4);

    gl.bindBuffer(gl.ARRAY_BUFFER, f32_buffer);
    gl.bufferData(gl.ARRAY_BUFFER, f32data, gl.DYNAMIC_DRAW);

    gl.bindBuffer(gl.ARRAY_BUFFER, u8_buffer);
    gl.bufferData(gl.ARRAY_BUFFER, u8data, gl.DYNAMIC_DRAW);

    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, count);
}

async function fetchROM(url) {
    const response = await fetch(url);
    const romData = new Uint8Array(await response.arrayBuffer());
    return romData;
}

function redirectConsole(elem_id) {
    const elem = document.getElementById(elem_id);

    if (!elem) {
        return;
    }

    elem.textContent = "";

    function logToPage(type, ...args) {
        const msg = args.map(arg => (typeof arg === "object" ? JSON.stringify(arg, null, 2) : arg)).join(" ");
        elem.textContent += `[${type}] ${msg}\n`;
    }

    ['log', 'info', 'warn', 'error', 'debug'].forEach(m => {
        const oldMethod = console[m];

        const label = m.toUpperCase();

        console[m] = function (...args) {
            oldMethod.apply(console, args);
            logToPage(label, ...args);
        }
    });
}

function compileShader(gl, type, source) {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);

    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
        console.error("Shader compilation failed:", gl.getShaderInfoLog(shader));
        gl.deleteShader(shader);
        return null;
    }

    return shader;
}

const noRaVertexShader = `#version 300 es
    in vec4 a_position;
    in uvec4 a_color;

    out vec4 v_color;

    void main() {
        gl_Position = a_position;
        v_color = vec4(a_color) / 255.;
    }
`;

const noRaFragmentShader = `#version 300 es
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
`;

start();
