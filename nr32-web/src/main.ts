import './style.css';

import { Emulator } from './emulator.ts';
import { SampleFifo } from './shared-fifo.ts';
import { redirectConsole } from './console.ts';

import workletUrl from './audio-worklet.ts?worker&url';

async function main() {
  const consoleElem = document.querySelector<HTMLPreElement>('#nora32-console');
  if (consoleElem) {
    redirectConsole(consoleElem);
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

  const rom = await fetchROM('./cart.nr32');

  emu.loadRom(rom);

  // We have two ways of synchronizing the emulator: if audio is on we use the
  // audio worklet's FIFO level to decide when a new frame should be scheduled
  // (sync-on-audio). If we're muted we just schedule with a 30FPS interval
  // instead (sync-on-video).
  const runInterval = () => {
    return setInterval(() => emu.runFrame(), 1000 / 30);
  };

  // Browsers normally don't let you start the audio without user interaction,
  // so we always start muted with sync-on-video.
  let frameInterval: number | undefined = runInterval();

  const mute_toggle = document.querySelector<HTMLButtonElement>('#nora32-mute-toggle');

  let audioContext: AudioContext | undefined = undefined;

  if (mute_toggle) {
    let muted = true;
    mute_toggle.addEventListener('click', async () => {
      muted = !muted;
      if (muted) {
        mute_toggle.textContent = 'Unmute';
        frameInterval = runInterval();
        if (audioContext) {
          await audioContext.suspend();
          audioContext = undefined;
        }
        emu.m.on_output_audio_samples((samples_i16_ptr: number, count: number) => {
          void samples_i16_ptr;
          void count;
        });
      } else {
        mute_toggle.textContent = 'Mute';
        clearInterval(frameInterval);

        if (audioContext) {
          await audioContext.suspend();
        }
        audioContext = new AudioContext({ sampleRate: 44100 });
        await audioContext.audioWorklet.addModule(workletUrl);

        const sampleFifo = SampleFifo.with_capacity((44100 * 2) / 5);

        const audioNode = new AudioWorkletNode(audioContext, 'nora32-audio', {
          numberOfInputs: 0,
          numberOfOutputs: 1,
          outputChannelCount: [2],
          processorOptions: {
            fifoBuffer: sampleFifo.fifoBuffer,
            indexBuffer: sampleFifo.indexBuffer,
            capacity: sampleFifo.capacity,
          },
        });
        audioNode.connect(audioContext.destination);

        emu.m.on_output_audio_samples((samples_i16_ptr: number, count: number) => {
          const samples = new Int16Array(emu.wasm.memory.buffer, samples_i16_ptr, count);

          sampleFifo.loadI16Samples(samples);
        });

        audioNode.port.onmessage = (event) => {
          if (event.data === 'need_samples') {
            requestAnimationFrame(() => {
              emu.runFrame();
            });
          }
        };

        audioContext.resume();

        frameInterval = undefined;
      }
    });
  }
}

main();
