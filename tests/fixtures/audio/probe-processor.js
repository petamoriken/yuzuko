class ProbeProcessor extends AudioWorkletProcessor {
  static get parameterDescriptors() {
    return [{
      name: "gain",
      defaultValue: 0.25,
      minValue: 0,
      maxValue: 1,
    }];
  }

  constructor() {
    super();
    this.port.onmessage = (event) => {
      this.port.postMessage({ type: "pong", echo: event.data });
    };
    this.port.postMessage({
      type: "ctor",
      renderQuantumSize,
      sampleRate,
      currentFrame,
      currentTime,
    });
  }

  process(_inputs, outputs, parameters) {
    const channel = outputs[0] && outputs[0][0];
    this.port.postMessage({
      type: "process",
      length: channel ? channel.length : 0,
      gainLength: parameters.gain.length,
      gain0: parameters.gain[0],
    });
    return false;
  }
}

registerProcessor("probe", ProbeProcessor);
