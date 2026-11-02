class CloneProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.port.onmessage = (event) => {
      const data = event.data;
      if (!data) {
        return;
      }
      try {
        if (data.kind === "audio") {
          const sample = new Float32Array(data.value.numberOfFrames);
          data.value.copyTo(sample, { planeIndex: 0 });
          this.port.postMessage({
            kind: "audio",
            frames: data.value.numberOfFrames,
            channels: data.value.numberOfChannels,
            rate: data.value.sampleRate,
            sample: sample[0],
            hasAudioData: typeof AudioData !== "undefined",
          });
          return;
        }
        if (data.kind === "frame") {
          this.port.postMessage({
            kind: "frame",
            width: data.value.displayWidth,
            height: data.value.displayHeight,
            hasVideoFrame: typeof VideoFrame !== "undefined",
          });
        }
      } catch (error) {
        this.port.postMessage({
          kind: "error",
          name: error && error.name,
          message: String(error),
        });
      }
    };
    this.port.postMessage({ kind: "ready" });
  }

  process() {
    return false;
  }
}

registerProcessor("clone", CloneProcessor);
