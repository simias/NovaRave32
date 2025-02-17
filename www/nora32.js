import init, { NoRa32 } from "./pkg/novarave32.js";

async function start() {
    redirectConsole("console");

    await init();

    let rom = await fetchROM("./pkg/ROM.BIN");

    let nora32 = new NoRa32();

    nora32.load_rom(rom);

    nora32.run_frame();
}
start();

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
