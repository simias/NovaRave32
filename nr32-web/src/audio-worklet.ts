/// <reference lib="webworker" />

class NoRa32AudioWorklet extends AudioWorkletProcessor {
  process(inputs: Float32Array[][], outputs: Float32Array[][]) {
    void inputs;

    const output = outputs[0];
    const left = output[0];
    const right = output[1];

    for (let i = 0; i < left.length; i++) {
      left[i] = (i % 255) / 255;
      right[i] = (i % 255) / 255;
    }

    return true;
  }
}

registerProcessor('nora32-audio', NoRa32AudioWorklet);
