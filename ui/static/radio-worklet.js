// Team-radio microphone capture: downsample the browser mic to 16 kHz mono
// and post Int16 frames of 20 ms to the main thread.
class RadioCapture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.step = sampleRate / 16000;
    this.pos = 0;
    this.lp = 0;
    this.last = 0;
    this.frame = new Int16Array(320);
    this.n = 0;
  }

  process(inputs) {
    const ch = inputs[0] && inputs[0][0];
    if (!ch) return true;
    // One-pole low-pass (~6 kHz) against aliasing, then linear resample.
    const a = Math.exp((-2 * Math.PI * 6000) / sampleRate);
    for (let i = 0; i < ch.length; i++) {
      this.lp = (1 - a) * ch[i] + a * this.lp;
      const cur = this.lp;
      while (this.pos <= 1) {
        const v = this.last + (cur - this.last) * this.pos;
        this.frame[this.n++] = Math.max(-1, Math.min(1, v)) * 32767;
        if (this.n === this.frame.length) {
          this.port.postMessage(this.frame.slice(0));
          this.n = 0;
        }
        this.pos += this.step;
      }
      this.pos -= 1;
      this.last = cur;
    }
    return true;
  }
}

registerProcessor('radio-capture', RadioCapture);
