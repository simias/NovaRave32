/// <reference lib="webworker" />

import { SampleFifo } from './shared-fifo.ts';

class NoRa32AudioWorklet extends AudioWorkletProcessor {
  sampleFifo: SampleFifo;
  lastWriteIndex: number;
  canAskForSamples: boolean;

  constructor(options: { processorOptions: NoRa32Options }) {
    super();

    const opts = options.processorOptions;

    this.sampleFifo = new SampleFifo(opts.fifoBuffer, opts.indexBuffer, opts.capacity);

    this.lastWriteIndex = this.sampleFifo.writeIndex();
    this.canAskForSamples = true;
  }

  process(inputs: Float32Array[][], outputs: Float32Array[][]) {
    void inputs;

    const output = outputs[0];
    const left = output[0];
    const right = output[1];

    const wants = left.length;
    const avail = this.sampleFifo.availableSamples();
    let have = Math.trunc(avail / 2);

    const ri = this.sampleFifo.readIndex();
    const capacity = this.sampleFifo.capacity;

    let p = 0;
    for (let i = 0; i < wants; i++) {
      if (have > 0) {
        left[i] = this.sampleFifo.fifo[(ri + p) % capacity] / 32768.0;
        right[i] = this.sampleFifo.fifo[(ri + p + 1) % capacity] / 32768.0;

        p += 2;
        have -= 1;
      } else {
        left[i] = 0;
        right[i] = 0;
      }
    }

    this.sampleFifo.consume(avail - have * 2);

    const wi = this.sampleFifo.writeIndex();

    if (wi != this.lastWriteIndex) {
      // We have received new samples in the interim
      this.lastWriteIndex = wi;
      this.canAskForSamples = true;
    }

    if (this.canAskForSamples && have < 44100 / 20) {
      this.port.postMessage('need_samples');
      this.canAskForSamples = false;
    }

    return true;
  }
}

registerProcessor('nora32-audio', NoRa32AudioWorklet);

type NoRa32Options = {
  fifoBuffer: SharedArrayBuffer;
  indexBuffer: SharedArrayBuffer;
  capacity: number;
};
