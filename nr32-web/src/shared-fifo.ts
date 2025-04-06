export class SampleFifo {
  fifoBuffer: SharedArrayBuffer;
  indexBuffer: SharedArrayBuffer;

  fifo: Int16Array;
  indices: Uint32Array;

  // Capacity in pairs of samples
  capacity: number;

  constructor(fifoBuffer: SharedArrayBuffer, indexBuffer: SharedArrayBuffer, capacity: number) {
    this.fifoBuffer = fifoBuffer;
    this.indexBuffer = indexBuffer;

    this.fifo = new Int16Array(fifoBuffer);
    this.indices = new Uint32Array(indexBuffer);

    this.capacity = capacity;
  }

  writeIndex(): number {
    return Atomics.load(this.indices, 0);
  }

  readIndex(): number {
    return Atomics.load(this.indices, 1);
  }

  setWriteIndex(i: number) {
    Atomics.store(this.indices, 0, i);
  }

  setReadIndex(i: number) {
    Atomics.store(this.indices, 1, i);
  }

  consume(nSamples: number) {
    nSamples = Math.min(nSamples, this.availableSamples());

    const ri = (this.readIndex() + nSamples) % this.capacity;

    this.setReadIndex(ri);
  }

  availableSamples(): number {
    const wi = this.writeIndex();
    const ri = this.readIndex();

    if (wi >= ri) {
      return wi - ri;
    } else {
      return this.capacity - (ri - wi);
    }
  }

  freeSamples(): number {
    return this.capacity - this.availableSamples();
  }

  loadI16Samples(samples: Int16Array) {
    let will_load = samples.length;
    const free = this.freeSamples();

    if (will_load > free) {
      console.warn('Audio buffer overflow');
      will_load = free;
    }

    let wi = this.writeIndex();

    for (let i = 0; i < will_load; i++) {
      this.fifo[wi] = samples[i];
      wi = (wi + 1) % this.capacity;
    }

    this.setWriteIndex(wi);
  }

  // Allocate a new FIFO for `capacity` pairs of stereo samples
  static with_capacity(capacity: number) {
    capacity = Math.round(capacity);

    const fifoBuffer = new SharedArrayBuffer(capacity * Int16Array.BYTES_PER_ELEMENT);
    const indexBuffer = new SharedArrayBuffer(2 * Uint32Array.BYTES_PER_ELEMENT);

    return new SampleFifo(fifoBuffer, indexBuffer, capacity);
  }
}
