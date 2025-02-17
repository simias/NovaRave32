import init, { NoRa32 } from "./pkg/novarave32.js";

redirectConsole("nora32-console");

const wasm = await init();

const canvas = document.getElementById('nora32-screen');
const gl = canvas.getContext('webgl', { antialias: false });

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

    const buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);

    const positionLocation = gl.getAttribLocation(program, "a_position");
    const colorLocation = gl.getAttribLocation(program, "a_color");

    gl.enableVertexAttribArray(positionLocation);
    gl.enableVertexAttribArray(colorLocation);

    const stride = Float32Array.BYTES_PER_ELEMENT * (3 + 4);
    gl.vertexAttribPointer(positionLocation, 3, gl.FLOAT, false, stride, 0);
    gl.vertexAttribPointer(colorLocation, 4, gl.FLOAT, false, stride, 3 * Float32Array.BYTES_PER_ELEMENT);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.clearColor(0.0, 0.0, 0.0, 1.0);


    let rom = await fetchROM("./pkg/ROM.BIN");

    let nora32 = new NoRa32();

    nora32.load_rom(rom);

    nora32.run_frame();
}

window.drawTriangles3D = function (ptr, count) {
    console.log(`DRAW TRIANGLES called with ${count} vertices`);

    let data = new Float32Array(wasm.memory.buffer, ptr, count);

    gl.bufferData(gl.ARRAY_BUFFER, data, gl.DYNAMIC_DRAW);

    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, count / 7);
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

    ['log', 'info', 'warn', 'error'].forEach(m => {
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

const noRaVertexShader = `
    attribute vec3 a_position;
    attribute vec4 a_color;

    varying vec4 v_color;

    void main() {
        gl_Position = vec4(a_position, 1.0);
        v_color = a_color;
    }
`;

const noRaFragmentShader = `
    precision mediump float;
    varying vec4 v_color;

    void main() {
        gl_FragColor = v_color;
    }
`;

start();
