import './style.css';

import { Emulator } from './emulator.ts';
import { redirectConsole } from './console.ts';

async function main() {
  const console = document.querySelector<HTMLPreElement>('#nora32-console');
  if (console) {
    redirectConsole(console);
  }

  const canvas = document.querySelector<HTMLCanvasElement>('#nora32-screen');

  if (!canvas) {
    throw new Error('No <canvas> element!');
  }

  async function fetchROM(url: string) {
    const response = await fetch(url);
    const romData = new Uint8Array(await response.arrayBuffer());
    return romData;
  }

  const emu = await Emulator.build(canvas);

  const rom = await fetchROM('/ROM.BIN');

  emu.loadRom(rom);

  setInterval(() => emu.runFrame(), 1000 / 30);
}

main();
