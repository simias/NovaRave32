class NoRaAudioProcessor extends AudioWorkletProcessor {
    constructor() {
        super();

        // 0.25s max buffer
        this.buffer = new Float32Array((44100 * 2) / 4);
        this.readIndex = 0;
        this.writeIndex = 0;
        this.samplesAvailable = 0;
        this.askedForSamples = false;

        this.port.onmessage = (event) => {
            const samples = event.data;

            let nsamples = samples.length;
            // console.log("Received samples", nsamples);

            const free_space = this.buffer.length - this.samplesAvailable;

            if (free_space < samples.length) {
                console.warn("Audio overrun!");
                nsamples = free_space;
            }

            for (let i = 0; i < nsamples; i++) {
                // Convert from i16 to float
                this.buffer[this.writeIndex] = samples[i] / 32768.0;
                this.writeIndex = (this.writeIndex + 1) % this.buffer.length;
            }

            this.samplesAvailable += samples.length;
            this.askedForSamples = false;
        };

        this.port.postMessage("need_samples");
        this.askedForSamples = true;
    }

    process(inputs, outputs) {
        const output = outputs[0];
        const left = output[0];
        const right = output[1];

        let want_samples = left.length;

        if (this.samplesAvailable < want_samples * 2) {
            console.warn("Audio underrun!");
            want_samples = this.samplesAvailable / 2;
        }

        for (let i = 0; i < want_samples; i++) {
            left[i] = this.buffer[this.readIndex];
            this.readIndex = (this.readIndex + 1) % this.buffer.length;
            right[i] = this.buffer[this.readIndex];
            this.readIndex = (this.readIndex + 1) % this.buffer.length;
        }

        this.samplesAvailable -= want_samples * 2;

        // Always have 0.05s of runahead
        if (!this.askedForSamples && this.samplesAvailable < (44100 * 2 / 20)) {
            this.port.postMessage("need_samples");
            this.askedForSamples = true;
        }

        return true;
    }
}

registerProcessor("nora32-audio-processor", NoRaAudioProcessor);

