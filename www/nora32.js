import init, { NoRa32 } from "./pkg/novarave32.js";

redirectConsole("nora32-console");

const wasm = await init();

const canvas = document.getElementById('nora32-screen');
canvas.style['image-rendering'] = 'pixelated';
const gl = canvas.getContext('webgl2', { antialias: false });
const i16_buffer = gl.createBuffer();
const u8_buffer = gl.createBuffer();
const quad_vbo = gl.createBuffer();

const quad_vert = new Float32Array([
    -1., -1.,  0., 0., // Bottom-left (x, y, u, v)
    1., -1.,  1., 0., // Bottom-right
    -1.,  1.,  0., 1., // Top-left
    1.,  1.,  1., 1.  // Top-right
]);

gl.bindBuffer(gl.ARRAY_BUFFER, quad_vbo);
gl.bufferData(gl.ARRAY_BUFFER, quad_vert, gl.STATIC_DRAW);

let projectionLocation = undefined;

const swidth = canvas.width;
const sheight = canvas.height;

// FBO used for off-screen rendering
const fbo = gl.createFramebuffer();

// Framebuffer texture
const fbt = gl.createTexture();
gl.bindTexture(gl.TEXTURE_2D, fbt);
gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, swidth, sheight, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAX_FILTER, gl.NEAREST);

const fbdepth = gl.createRenderbuffer();
gl.bindRenderbuffer(gl.RENDERBUFFER, fbdepth);
gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT16, swidth, sheight);

const noRaProgram = gl.createProgram();
const screenProgram = gl.createProgram();

const noRaVao = gl.createVertexArray();
const screenVao = gl.createVertexArray();

const nora32 = new NoRa32();


function compileShaders(prog, vs, fs) {
    const vshader = compileShader(gl, gl.VERTEX_SHADER, vs);
    const fshader = compileShader(gl, gl.FRAGMENT_SHADER, fs);

    gl.attachShader(prog, vshader);
    gl.attachShader(prog, fshader);
    gl.linkProgram(prog);

    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
        console.error("Program linking failed:", gl.getProgramInfoLog(prog));
    }
}

async function startAudio() {
    console.log("start audio");
    const audioCtx = new AudioContext({ sampleRate: 44100 });

    if (audioCtx.state === "suspended") {
        await audioCtx.resume();
    }

    await audioCtx.audioWorklet.addModule("nora32audio.js");

    const audioNode = new AudioWorkletNode(audioCtx, "nora32-audio-processor", {
        outputChannelCount: [2]});

    audioNode.port.onmessage = (event) => {
        if (event.data === "need_samples") {
            nora32.run_frame();
        }
    };

    window.outputAudioSamples = function (samples_i16, count) {
        let samples = new Int16Array(wasm.memory.buffer, samples_i16, count);

        audioNode.port.postMessage(samples);
    }

    // Start audio. This is what we use to synchronize the emulation progress
    audioNode.connect(audioCtx.destination);
}

async function start() {

    compileShaders(noRaProgram, noRaVertexShader, noRaFragmentShader);
    compileShaders(screenProgram, screenVertexShader, screenFragmentShader);

    {
        gl.bindVertexArray(screenVao);
        gl.useProgram(screenProgram);

        const aPosition = gl.getAttribLocation(screenProgram, "aPosition");
        const aTexCoord = gl.getAttribLocation(screenProgram, "aTexCoord");

        gl.bindBuffer(gl.ARRAY_BUFFER, quad_vbo);
        gl.vertexAttribPointer(aPosition, 2, gl.FLOAT, false, 16, 0);
        gl.enableVertexAttribArray(aPosition);
        gl.vertexAttribPointer(aTexCoord, 2, gl.FLOAT, false, 16, 8);
        gl.enableVertexAttribArray(aTexCoord);
    }

    gl.useProgram(noRaProgram);
    gl.bindVertexArray(noRaVao);

    const positionLocation = gl.getAttribLocation(noRaProgram, "a_position");
    const colorLocation = gl.getAttribLocation(noRaProgram, "a_color");
    const matrixIndex = gl.getAttribLocation(noRaProgram, "a_projection_index");

    projectionLocation = gl.getUniformLocation(noRaProgram, "u_projections");

    gl.enableVertexAttribArray(positionLocation);
    gl.enableVertexAttribArray(colorLocation);
    gl.enableVertexAttribArray(matrixIndex);

    gl.bindBuffer(gl.ARRAY_BUFFER, i16_buffer);
    gl.vertexAttribIPointer(positionLocation, 3, gl.SHORT, 0, 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, u8_buffer);
    gl.vertexAttribIPointer(colorLocation, 4, gl.UNSIGNED_BYTE, 5, 0);
    gl.vertexAttribIPointer(matrixIndex, 1, gl.UNSIGNED_BYTE, 5, 4);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.clearColor(0.0, 0.0, 0.0, 1.0);

    // Initialize screen program (used to render off-screen framebuffers to the
    // screen)


    let rom = await fetchROM("./pkg/ROM.BIN");

    nora32.load_rom(rom);

    switch_to_vbo();

    startAudio();

    // Browsers usually block playback as long as the user hasn't interacted
    // with the page
    canvas.addEventListener("click", () => {
        startAudio();
    });
}

function switch_to_vbo() {
    // Switch back to draw program
    gl.bindVertexArray(noRaVao);

    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, fbt, 0);
    gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.RENDERBUFFER, fbdepth);
    gl.useProgram(noRaProgram);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
}

// Draw to the framebuffer
window.drawTriangles3D = function (mat_f32_ptr, mat_count, i16_ptr, u8_ptr, count) {
    // console.log(`DRAW TRIANGLES called with ${count} vertices and ${mat_count} matrices (${count / 3} triangles)`);

    gl.enable(gl.DEPTH_TEST);

    let matdata = new Float32Array(wasm.memory.buffer, mat_f32_ptr, mat_count * 16);
    let f32data = new Int16Array(wasm.memory.buffer, i16_ptr, count * 3);
    let u8data = new Uint8Array(wasm.memory.buffer, u8_ptr, count * 5);

    gl.uniformMatrix4fv(projectionLocation, false, matdata);

    gl.bindBuffer(gl.ARRAY_BUFFER, i16_buffer);
    gl.bufferData(gl.ARRAY_BUFFER, f32data, gl.DYNAMIC_DRAW);

    gl.bindBuffer(gl.ARRAY_BUFFER, u8_buffer);
    gl.bufferData(gl.ARRAY_BUFFER, u8data, gl.DYNAMIC_DRAW);

    gl.drawArrays(gl.TRIANGLES, 0, count);
}

// Display framebuffer
window.displayFramebuffer = function () {
    // console.log("Display FB");
    gl.bindVertexArray(screenVao);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.useProgram(screenProgram);

    const uTexture = gl.getUniformLocation(screenProgram, "uTexture");
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, fbt);
    gl.uniform1i(uTexture, 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, quad_vbo);

    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

    switch_to_vbo();
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
        if (elem.textContent.length > 1024 * 1024) {
            elem.textContent = "";
        }
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
    in ivec3 a_position;
    in uvec4 a_color;
    in uint a_projection_index;

    out vec4 v_color;

    uniform mat4 u_projections[32];

    void main() {
        mat4 m = u_projections[a_projection_index];
        gl_Position = m * vec4(a_position, 1.);
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

// Shaders for rendering off-screen framebuffers to the screen
const screenVertexShader = `#version 300 es
precision mediump float;
layout(location = 0) in vec2 aPosition;
layout(location = 1) in vec2 aTexCoord;

out vec2 vTexCoord;

void main() {
    vTexCoord = aTexCoord;
    gl_Position = vec4(aPosition, 0.0, 1.0);
}`;

const screenFragmentShader = `#version 300 es
precision mediump float;
in vec2 vTexCoord;
out vec4 fragColor;

uniform sampler2D uTexture;

void main() {
    vec3 color = texture(uTexture, vTexCoord).rgb;

    float gamma = 1. / 2.2;
    fragColor = vec4(pow(color, vec3(gamma)), 1.0);
}`;

start();
