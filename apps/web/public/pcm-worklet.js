class PrivateAiPcmProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.targetRate = 16000;
    this.ratio = sampleRate / this.targetRate;
    this.source = [];
    this.readIndex = 0;
    this.output = [];
    this.chunkSamples = 5120;
    this.port.onmessage = (event) => {
      if (event.data?.type === "flush") {
        this.emit(true);
        this.port.postMessage({ type: "flushed" });
      }
    };
  }

  process(inputs) {
    const input = inputs[0]?.[0];
    if (!input?.length) return true;
    for (let index = 0; index < input.length; index += 1) {
      this.source.push(input[index]);
    }
    while (this.readIndex + 1 < this.source.length) {
      const index = Math.floor(this.readIndex);
      const fraction = this.readIndex - index;
      const sample = this.source[index] * (1 - fraction) + this.source[index + 1] * fraction;
      this.output.push(Math.max(-1, Math.min(1, sample)));
      this.readIndex += this.ratio;
      if (this.output.length >= this.chunkSamples) this.emit(false);
    }
    const consumed = Math.floor(this.readIndex);
    if (consumed > 0) {
      this.source = this.source.slice(consumed);
      this.readIndex -= consumed;
    }
    return true;
  }

  emit(flush) {
    while (this.output.length >= this.chunkSamples || (flush && this.output.length)) {
      const size = Math.min(this.chunkSamples, this.output.length);
      const chunk = new Float32Array(this.output.splice(0, size));
      this.port.postMessage(chunk.buffer, [chunk.buffer]);
    }
  }
}

registerProcessor("private-ai-pcm-16k", PrivateAiPcmProcessor);
