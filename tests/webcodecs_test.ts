function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

async function assertThrows(
  run: () => unknown,
  name: string,
): Promise<void> {
  try {
    await run();
  } catch (error) {
    assert(error instanceof DOMException, String(error));
    assert(error.name === name, error.name);
    return;
  }
  throw new Error(`${name} was not thrown`);
}

function assertTypeError(run: () => unknown, message: string): void {
  try {
    run();
  } catch (error) {
    assert(error instanceof TypeError, `${message}: ${String(error)}`);
    return;
  }
  throw new Error(`${message} was not thrown`);
}

function assertRangeError(run: () => unknown, message: string): void {
  try {
    run();
  } catch (error) {
    assert(error instanceof RangeError, `${message}: ${String(error)}`);
    return;
  }
  throw new Error(`${message} was not thrown`);
}

function assertNear(actual: number, expected: number, message: string): void {
  assert(Math.abs(actual - expected) < 1e-5, `${message}: ${actual}`);
}

const fixtures = new URL("./fixtures/webcodecs/", import.meta.url);

async function bytes(name: string): Promise<Uint8Array> {
  const response = await fetch(new URL(name, fixtures));
  return new Uint8Array(await response.arrayBuffer());
}

function framesOf(blob: Uint8Array): Uint8Array[] {
  const frames: Uint8Array[] = [];
  let offset = 0;
  const view = new DataView(blob.buffer, blob.byteOffset, blob.byteLength);
  while (offset + 4 <= blob.byteLength) {
    const length = view.getUint32(offset);
    offset += 4;
    frames.push(blob.subarray(offset, offset + length));
    offset += length;
  }
  return frames;
}

/**
 * H.264 decodes only in the platform hardware decoder. A Windows or Linux
 * machine without one, such as a VM or WSL without VA-API, closes the
 * decoder and reports `NotSupportedError`. The picture tests are ignored
 * there. macOS always has VideoToolbox, so the probe must stay configured.
 */
async function probeHardwareAvc(): Promise<boolean> {
  const decoder = new VideoDecoder({ output: () => {}, error: () => {} });
  try {
    decoder.configure({
      codec: "avc1.64000B",
      description: await bytes("avc1.avcC"),
      codedWidth: 64,
      codedHeight: 64,
    });
    await Promise.resolve();
    if (decoder.state === "configured") {
      return true;
    }
    if (Yuzuko.build.os === "darwin") {
      throw new Error("VideoToolbox rejected the H.264 probe");
    }
    return false;
  } finally {
    if (decoder.state !== "closed") {
      decoder.close();
    }
  }
}

const hardwareAvc = await probeHardwareAvc();

async function golden(): Promise<ImageData> {
  const blob = new Blob([await bytes("pattern.png")], { type: "image/png" });
  const bitmap = await createImageBitmap(blob);
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const context = canvas.getContext("2d");
  assert(context, "golden context");
  context.drawImage(bitmap, 0, 0);
  return context.getImageData(0, 0, bitmap.width, bitmap.height);
}

function sample(
  data: Uint8ClampedArray,
  x: number,
  y: number,
  width: number,
): number[] {
  const index = (y * width + x) * 4;
  return [data[index], data[index + 1], data[index + 2], data[index + 3]];
}

function near(
  actual: number[],
  expected: number[],
  tolerance: number,
  label: string,
): void {
  for (let channel = 0; channel < 4; channel += 1) {
    const delta = Math.abs(actual[channel] - expected[channel]);
    assert(delta <= tolerance, `${label} channel ${channel} delta ${delta}`);
  }
}

function comparePicture(
  actual: ImageData,
  expected: ImageData,
  tolerance: number,
): void {
  assert(actual.width === expected.width, `width ${actual.width}`);
  assert(actual.height === expected.height, `height ${actual.height}`);
  const points = [
    [8, 16],
    [16, 32],
    [24, 48],
    [40, 16],
    [48, 32],
    [56, 48],
  ];
  for (const [x, y] of points) {
    near(
      sample(actual.data, x, y, actual.width),
      sample(expected.data, x, y, expected.width),
      tolerance,
      `${x},${y}`,
    );
  }
}

async function drawFrame(frame: VideoFrame): Promise<ImageData> {
  const canvas = new OffscreenCanvas(frame.displayWidth, frame.displayHeight);
  const context = canvas.getContext("2d");
  assert(context, "2d");
  context.drawImage(frame, 0, 0);
  return context.getImageData(0, 0, canvas.width, canvas.height);
}

function decodeVideo(
  codec: string,
  description: Uint8Array,
  data: Uint8Array,
  options: Record<string, unknown> = {},
): Promise<VideoFrame[]> {
  const frames: VideoFrame[] = [];
  const decoder = new VideoDecoder({
    output: (frame) => {
      frames.push(frame);
    },
    error: () => {},
  });
  decoder.configure({
    codec,
    description,
    codedWidth: 64,
    codedHeight: 64,
    ...options,
  });
  decoder.decode(
    new EncodedVideoChunk({ type: "key", timestamp: 0, data }),
  );
  assert(frames.length === 0, "output during decode");
  return decoder.flush().then(() => {
    decoder.close();
    return frames;
  });
}

function correlate(samples: Float32Array, rate: number): number {
  const frequency = 440;
  const window = 1024;
  let best = 0;
  for (let lag = 0; lag + window <= samples.length; lag += 256) {
    let sine = 0;
    let cosine = 0;
    let energy = 0;
    for (let index = 0; index < window; index += 1) {
      const sample = samples[lag + index];
      const angle = (2 * Math.PI * frequency * (lag + index)) / rate;
      sine += sample * Math.sin(angle);
      cosine += sample * Math.cos(angle);
      energy += sample * sample;
    }
    const scale = Math.sqrt((energy * window) / 2);
    const score = scale > 0 ? Math.hypot(sine, cosine) / scale : 0;
    if (score > best) {
      best = score;
    }
  }
  return best;
}

async function decodeAudio(
  codec: string,
  description: Uint8Array,
  packets: Uint8Array,
  sampleRate: number,
  numberOfChannels: number,
): Promise<Float32Array> {
  const parts: AudioData[] = [];
  let failed = "";
  const decoder = new AudioDecoder({
    output: (data) => {
      parts.push(data);
    },
    error: (error) => {
      failed = error instanceof DOMException ? error.name : String(error);
    },
  });
  decoder.configure({
    codec,
    description,
    sampleRate,
    numberOfChannels,
  });
  let timestamp = 0;
  for (const packet of framesOf(packets)) {
    decoder.decode(
      new EncodedAudioChunk({ type: "key", timestamp, data: packet }),
    );
    timestamp += 20_000;
  }
  assert(parts.length === 0, "output during decode");
  await decoder.flush();
  assert(failed === "", failed);
  decoder.close();
  const plane: number[] = [];
  for (const data of parts) {
    const copy = new Float32Array(data.numberOfFrames);
    data.copyTo(copy, { planeIndex: 0 });
    const buffer = new AudioBuffer({
      length: data.numberOfFrames,
      numberOfChannels: 1,
      sampleRate: data.sampleRate,
    });
    buffer.copyToChannel(copy, 0);
    const stored = buffer.getChannelData(0);
    assert(copy.length > 0, "empty audio");
    assert(stored.length === copy.length, "copied length");
    assert(stored[0] === copy[0], "copied sample");
    const offline = new OfflineAudioContext(1, copy.length, data.sampleRate);
    const source = offline.createBufferSource();
    source.buffer = buffer;
    source.connect(offline.destination);
    source.start();
    const rendered = await offline.startRendering();
    const played = rendered.getChannelData(0);
    assert(played.length === copy.length, `rendered ${played.length}`);
    for (let index = 0; index < copy.length; index += 1) {
      const delta = Math.abs(played[index] - copy[index]);
      assert(delta < 1e-5, `played ${index} ${played[index]} vs ${copy[index]}`);
    }
    for (const sample of copy) {
      plane.push(sample);
    }
    data.close();
    await assertThrows(() => data.copyTo(copy, { planeIndex: 0 }), "InvalidStateError");
  }
  return Float32Array.from(plane);
}

async function takeAudioData(): Promise<AudioData> {
  const parts: AudioData[] = [];
  let failed = "";
  const decoder = new AudioDecoder({
    output: (data) => {
      parts.push(data);
    },
    error: (error) => {
      failed = error instanceof DOMException ? error.name : String(error);
    },
  });
  decoder.configure({
    codec: "mp4a.40.2",
    description: await bytes("aac.asc"),
    sampleRate: 48000,
    numberOfChannels: 1,
  });
  let timestamp = 0;
  for (const packet of framesOf(await bytes("aac.frames"))) {
    decoder.decode(
      new EncodedAudioChunk({ type: "key", timestamp, data: packet }),
    );
    timestamp += 20_000;
  }
  assert(parts.length === 0, "output during decode");
  await decoder.flush();
  assert(failed === "", failed);
  decoder.close();
  assert(parts.length > 0, "no audio");
  const first = parts[0];
  assert(first.numberOfFrames > 0, "empty audio");
  for (const extra of parts.slice(1)) {
    extra.close();
  }
  return first;
}

async function takeFrame(): Promise<VideoFrame> {
  const url = new URL("./fixtures/images/corners.png", import.meta.url);
  const encoded = new Uint8Array(await (await fetch(url)).arrayBuffer());
  const decoder = new ImageDecoder({ type: "image/png", data: encoded });
  const result = await decoder.decode();
  decoder.close();
  return result.image;
}

function assertTone(samples: Float32Array, rate: number, label: string): void {
  let peak = 0;
  for (const sample of samples) {
    peak = Math.max(peak, Math.abs(sample));
  }
  assert(peak > 0.05, `${label} peak ${peak}`);
  const correlation = correlate(samples, rate);
  assert(
    correlation > 0.7,
    `${label} correlation ${correlation} peak ${peak}`,
  );
}

Yuzuko.test("WebCodecs constructors exist and encoders do not", () => {
  assert(typeof VideoDecoder === "function", "VideoDecoder");
  assert(typeof EncodedVideoChunk === "function", "EncodedVideoChunk");
  assert(typeof VideoFrame === "function", "VideoFrame");
  assert(typeof AudioDecoder === "function", "AudioDecoder");
  assert(typeof EncodedAudioChunk === "function", "AudioData chunk");
  assert(typeof AudioData === "function", "AudioData");
  assert(typeof VideoEncoder === "undefined", "VideoEncoder");
  assert(typeof AudioEncoder === "undefined", "AudioEncoder");
  assert(typeof ImageDecoder === "function", "ImageDecoder");
  assert(typeof HTMLVideoElement === "undefined", "HTMLVideoElement");
  let name = "";
  try {
    new VideoFrame();
  } catch (error) {
    name = error instanceof TypeError ? error.name : "";
  }
  assert(name === "TypeError", name);
  name = "";
  try {
    new AudioData();
  } catch (error) {
    name = error instanceof TypeError ? error.name : "";
  }
  assert(name === "TypeError", "AudioData " + name);
});

Yuzuko.test("VideoDecoder dispatches a dequeue event", () => {
  const decoder = new VideoDecoder({ output: () => {}, error: () => {} });
  assert(decoder instanceof EventTarget, "EventTarget");
  assert(decoder.ondequeue === null, "ondequeue starts null");
  decoder.configure({
    codec: "av01.0.04M.08",
    hardwareAcceleration: "prefer-software",
  });
  const handled: Event[] = [];
  const listened: Event[] = [];
  decoder.ondequeue = (event) => {
    assert(event instanceof Event, "handler event");
    assert(event.type === "dequeue", event.type);
    assert(event.target === decoder, "target");
    assert(event.currentTarget === decoder, "currentTarget");
    assert(event.eventPhase === Event.AT_TARGET, "phase");
    assert(event.isTrusted === true, "trusted");
    assert(event.bubbles === false, "bubbles");
    assert(event.cancelable === false, "cancelable");
    assert(decoder.decodeQueueSize === 0, "size while handling");
    handled.push(event);
  };
  decoder.addEventListener("dequeue", (event) => {
    listened.push(event);
  });
  const chunk = new EncodedVideoChunk({
    type: "key",
    timestamp: 0,
    data: new Uint8Array([1, 2, 3, 4]),
  });
  decoder.decode(chunk);
  assert(decoder.decodeQueueSize === 1, "queued");
  decoder.reset();
  assert(decoder.decodeQueueSize === 0, "cleared");
  assert(handled.length === 1, "one handler call");
  assert(listened.length === 1 && listened[0] === handled[0], "same event");
  decoder.ondequeue = null;
  assert(decoder.ondequeue === null, "cleared handler");
  decoder.configure({
    codec: "av01.0.04M.08",
    hardwareAcceleration: "prefer-software",
  });
  decoder.decode(chunk);
  decoder.reset();
  assert(handled.length === 1, "removed handler stays quiet");
  assert(listened.length === 2, "listener still runs");
  decoder.close();
});

Yuzuko.test("an unconfigured decoder rejects decode and flush", async () => {
  const decoder = new VideoDecoder({ output: () => {}, error: () => {} });
  assert(decoder.state === "unconfigured", decoder.state);
  const chunk = new EncodedVideoChunk({
    type: "key",
    timestamp: 0,
    data: new Uint8Array([1, 2, 3, 4]),
  });
  await assertThrows(() => decoder.decode(chunk), "InvalidStateError");
  await assertThrows(() => decoder.flush(), "InvalidStateError");
  const foreign = VideoDecoder.prototype.flush.call({});
  assert(foreign instanceof Promise, "foreign flush returns a promise");
  let foreignName = "";
  try {
    await foreign;
  } catch (error) {
    foreignName = error instanceof TypeError ? "TypeError" : String(error);
  }
  assert(foreignName === "TypeError", foreignName);
  const foreignAudio = AudioDecoder.prototype.flush.call({});
  assert(foreignAudio instanceof Promise, "foreign audio flush returns a promise");
  foreignName = "";
  try {
    await foreignAudio;
  } catch (error) {
    foreignName = error instanceof TypeError ? "TypeError" : String(error);
  }
  assert(foreignName === "TypeError", foreignName);
  const audio = new AudioDecoder({ output: () => {}, error: () => {} });
  const packet = new EncodedAudioChunk({
    type: "key",
    timestamp: 0,
    data: new Uint8Array([1]),
  });
  await assertThrows(() => audio.decode(packet), "InvalidStateError");
  await assertThrows(() => audio.flush(), "InvalidStateError");
  audio.close();
  await assertThrows(() => audio.flush(), "InvalidStateError");
  decoder.close();
  await assertThrows(() => decoder.configure({ codec: "avc1" }), "InvalidStateError");
});

Yuzuko.test("unsupported codecs do not decode", async () => {
  const picture = await golden();
  assert(picture.width === 64, "golden width");
  for (const codec of [
    "hev1.1.6.L93.B0",
    "mp4v.20.9",
    "zzzz",
    "vp8",
    "vp08.00.10.08",
    "vp09.00.10.08",
  ]) {
    const frames: VideoFrame[] = [];
    let seen = "";
    const decoder = new VideoDecoder({
      output: (frame) => {
        frames.push(frame);
      },
      error: (error) => {
        seen = error instanceof DOMException ? error.name : "";
      },
    });
    decoder.configure({
      codec,
      codedWidth: 64,
      codedHeight: 64,
      description: new Uint8Array([1, 2, 3, 4]),
    });
    assert(decoder.state === "closed", codec);
    await assertThrows(() => decoder.flush(), "NotSupportedError");
    assert(seen === "NotSupportedError", `${codec} ${seen}`);
    assert(frames.length === 0, codec);
  }
  for (const codec of ["flac", "vorbis"]) {
    let seen = "";
    const audio = new AudioDecoder({
      output: () => {},
      error: (error) => {
        seen = error instanceof DOMException ? error.name : "";
      },
    });
    audio.configure({
      codec,
      sampleRate: 48000,
      numberOfChannels: 1,
    });
    assert(audio.state === "closed", codec);
    await assertThrows(() => audio.flush(), "NotSupportedError");
    assert(seen === "NotSupportedError", `${codec} ${seen}`);
  }
  const opus = new AudioDecoder({ output: () => {}, error: () => {} });
  opus.configure({
    codec: "opus",
    sampleRate: 48000,
    numberOfChannels: 1,
  });
  assert(opus.state === "configured", opus.state);
  opus.close();
});

Yuzuko.test("a rejected avc1 config yields no VideoFrame", async () => {
  const frames: VideoFrame[] = [];
  let seen = "";
  const decoder = new VideoDecoder({
    output: (frame) => {
      frames.push(frame);
    },
    error: (error) => {
      seen = error instanceof DOMException ? error.name : "";
    },
  });
  const bad = await bytes("avc1-bad.avcC");
  decoder.configure({
    codec: "avc1.42E01E",
    codedWidth: 64,
    codedHeight: 64,
    description: bad,
  });
  assert(decoder.state === "closed", decoder.state);
  await assertThrows(() => decoder.flush(), "NotSupportedError");
  assert(seen === "NotSupportedError", seen);
  assert(frames.length === 0, "hardware reject produced a frame");
});

Yuzuko.test({
  name: "VideoDecoder output can be assigned after decode returns",
  ignore: !hardwareAvc,
  async fn() {
    const received: VideoFrame[] = [];
    const callbacks: { output: ((frame: VideoFrame) => void) | null } = {
      output: null,
    };
    const decoder = new VideoDecoder({
      output: (frame) => {
        callbacks.output?.(frame);
      },
      error: () => {},
    });
    decoder.configure({
      codec: "avc1.64000B",
      description: await bytes("avc1.avcC"),
      codedWidth: 64,
      codedHeight: 64,
    });
    decoder.decode(
      new EncodedVideoChunk({
        type: "key",
        timestamp: 5,
        duration: 8,
        data: await bytes("avc1.idr"),
      }),
    );
    assert(received.length === 0, "output during decode");
    callbacks.output = (frame) => {
      received.push(frame);
    };
    await decoder.flush();
    assert(received.length === 1, `frames ${received.length}`);
    assert(received[0].timestamp === 5, `timestamp ${received[0].timestamp}`);
    assert(received[0].duration === 8, `duration ${received[0].duration}`);
    received[0].close();
    decoder.close();
  },
});

Yuzuko.test({
  name: "VideoDecoder error runs after decode returns",
  ignore: !hardwareAvc,
  async fn() {
    let errors = 0;
    let name = "";
    const decoder = new VideoDecoder({
      output: (frame) => {
        frame.close();
      },
      error: (error) => {
        errors += 1;
        name = error instanceof DOMException ? error.name : "";
      },
    });
    decoder.configure({
      codec: "avc1.64000B",
      description: await bytes("avc1.avcC"),
      codedWidth: 64,
      codedHeight: 64,
    });
    decoder.decode(
      new EncodedVideoChunk({
        type: "key",
        timestamp: 0,
        data: new Uint8Array(),
      }),
    );
    assert(errors === 0, "error during decode");
    assert(decoder.state === "configured", decoder.state);
    await Promise.resolve();
    assert(errors === 1, `errors ${errors}`);
    assert(name === "EncodingError", name);
    assert(decoder.state === "closed", decoder.state);
  },
});

Yuzuko.test({
  name: "an H.264 frame matches the golden and then closes",
  ignore: !hardwareAvc,
  async fn() {
    const expected = await golden();
    const frames = await decodeVideo(
      "avc1.64000B",
      await bytes("avc1.avcC"),
      await bytes("avc1.idr"),
    );
    assert(frames.length === 1, `frames ${frames.length}`);
    const frame = frames[0];
    assert(frame.format === "RGBA", frame.format);
    comparePicture(await drawFrame(frame), expected, 16);
    const bitmap = await createImageBitmap(frame);
    assert(bitmap.width === 64 && bitmap.height === 64, "bitmap");
    const canvas = new OffscreenCanvas(64, 64);
    const context = canvas.getContext("2d");
    assert(context, "bitmap context");
    context.drawImage(bitmap, 0, 0);
    comparePicture(context.getImageData(0, 0, 64, 64), expected, 16);
    frame.close();
    await assertThrows(() => context.drawImage(frame, 0, 0), "InvalidStateError");
    await assertThrows(() => createImageBitmap(frame), "InvalidStateError");
  },
});

Yuzuko.test({
  name: "WebGPU readback matches the H.264 golden",
  ignore: !hardwareAvc,
  async fn() {
    const expected = await golden();
    const frames = await decodeVideo(
      "avc1.64000B",
      await bytes("avc1.avcC"),
      await bytes("avc1.idr"),
    );
    const frame = frames[0];
    const adapter = await navigator.gpu.requestAdapter();
    assert(adapter, "adapter");
    const device = await adapter.requestDevice();
    const texture = device.createTexture({
      size: [64, 64],
      format: "rgba8unorm",
      usage: GPUTextureUsage.COPY_DST | GPUTextureUsage.COPY_SRC,
    });
    device.queue.copyExternalImageToTexture(
      { source: frame },
      { texture, colorSpace: "srgb" },
      [64, 64],
    );
    const buffer = device.createBuffer({
      size: 64 * 64 * 4,
      usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
    });
    const encoder = device.createCommandEncoder();
    encoder.copyTextureToBuffer(
      { texture },
      { buffer, bytesPerRow: 256 },
      [64, 64],
    );
    device.queue.submit([encoder.finish()]);
    await buffer.mapAsync(GPUMapMode.READ);
    const pixels = new Uint8ClampedArray(buffer.getMappedRange());
    const image = new ImageData(pixels.slice(), 64, 64);
    comparePicture(image, expected, 16);
    buffer.unmap();
    frame.close();
    const later = new OffscreenCanvas(64, 64).getContext("2d");
    assert(later, "later");
    await assertThrows(() => later.drawImage(frame, 0, 0), "InvalidStateError");
    device.destroy();
  },
});

Yuzuko.test("AudioDecoder error runs after decode returns", async () => {
  let errors = 0;
  let name = "";
  const decoder = new AudioDecoder({
    output: () => {},
    error: (error) => {
      errors += 1;
      name = error instanceof DOMException ? error.name : "";
    },
  });
  decoder.configure({
    codec: "mp4a.40.2",
    description: await bytes("aac.asc"),
    sampleRate: 48000,
    numberOfChannels: 1,
  });
  decoder.decode(
    new EncodedAudioChunk({
      type: "key",
      timestamp: 0,
      data: new Uint8Array(),
    }),
  );
  assert(errors === 0, "error during decode");
  assert(decoder.state === "configured", decoder.state);
  await Promise.resolve();
  assert(decoder.state === "closed", decoder.state);
  assert(errors === 1, `errors ${errors}`);
  assert(name === "EncodingError", name);
});

Yuzuko.test("AAC samples fill an AudioBuffer", async () => {
  const samples = await decodeAudio(
    "mp4a.40.2",
    await bytes("aac.asc"),
    await bytes("aac.frames"),
    48000,
    1,
  );
  assertTone(samples, 48000, "aac");
});

Yuzuko.test("VideoFrame and AudioData clone and transfer", async () => {
  const frame = await takeFrame();
  const copy = structuredClone(frame);
  assert(copy !== frame, "frame identity");
  assert(copy.displayWidth === 2 && copy.displayHeight === 2, "frame copy");
  assert(copy.timestamp === frame.timestamp, "frame time");
  assert(frame.displayWidth === 2, "frame stays open");
  const drawn = await drawFrame(copy);
  copy.close();

  const worker = new Worker(new URL("./workers/clone.js", import.meta.url));
  const copied = new Promise<unknown>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => resolve(event.data);
    worker.onerror = () => reject(new Error("worker error"));
  });
  worker.postMessage(frame);
  const copiedSummary = await copied as {
    kind: string;
    width: number;
    height: number;
    timestamp: number;
    pixel: number[];
  };
  assert(copiedSummary.kind === "frame", copiedSummary.kind);
  assert(copiedSummary.width === 2 && copiedSummary.height === 2, "posted frame");
  assert(copiedSummary.pixel.join() === String([
    drawn.data[0],
    drawn.data[1],
    drawn.data[2],
    drawn.data[3],
  ]), copiedSummary.pixel.join());
  assert(frame.displayWidth === 2, "post leaves the frame open");

  const transferred = new Promise<unknown>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => resolve(event.data);
    worker.onerror = () => reject(new Error("worker error"));
  });
  worker.postMessage(frame, [frame]);
  const transferredSummary = await transferred as { kind: string; width: number };
  assert(transferredSummary.kind === "frame", transferredSummary.kind);
  assert(transferredSummary.width === 2, "transferred frame");
  assert(frame.format === null, "closed format");
  assert(frame.displayWidth === 0, "closed display");
  assert(frame.codedWidth === 0, "closed coded");
  assert(frame.visibleRect === null, "closed visible");
  assert(frame.timestamp === copiedSummary.timestamp, "closed timestamp");
  await assertThrows(() => frame.clone(), "InvalidStateError");

  const audio = await takeAudioData();
  const frames = audio.numberOfFrames;
  const channels = audio.numberOfChannels;
  const audioTimestamp = audio.timestamp;
  const local = new Float32Array(frames);
  audio.copyTo(local, { planeIndex: 0 });
  const audioCopy = structuredClone(audio);
  const copiedSamples = new Float32Array(audioCopy.numberOfFrames);
  audioCopy.copyTo(copiedSamples, { planeIndex: 0 });
  assert(audioCopy !== audio, "audio identity");
  assert(audioCopy.numberOfFrames === frames, "audio frames");
  assert(audioCopy.sampleRate === audio.sampleRate, "audio rate");
  assert(copiedSamples[0] === local[0], "audio sample");
  assert(audio.numberOfFrames > 0, "audio stays open");
  audioCopy.close();

  const audioReply = new Promise<unknown>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => resolve(event.data);
    worker.onerror = () => reject(new Error("worker error"));
  });
  worker.postMessage(audio, [audio]);
  const audioSummary = await audioReply as {
    kind: string;
    frames: number;
    channels: number;
    rate: number;
    sample: number;
  };
  assert(audioSummary.kind === "audio", audioSummary.kind);
  assert(audioSummary.frames === frames, "worker frames");
  assert(audioSummary.channels === channels, "worker channels");
  assert(audioSummary.rate === 48000, String(audioSummary.rate));
  assert(audioSummary.sample === local[0], "worker sample");
  assert(audio.format === null, "closed audio format");
  assert(audio.numberOfFrames === 0, "closed audio frames");
  assert(audio.sampleRate === 0, "closed audio rate");
  assert(audio.numberOfChannels === 0, "closed audio channels");
  assert(audio.duration === 0, "closed audio duration");
  assert(audio.timestamp === audioTimestamp, "closed audio timestamp");
  await assertThrows(() => audio.clone(), "InvalidStateError");

  const chunk = new EncodedAudioChunk({
    type: "key",
    timestamp: 20,
    data: new Uint8Array([1, 2, 3]),
  });
  const chunkReply = new Promise<unknown>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => resolve(event.data);
    worker.onerror = () => reject(new Error("worker error"));
  });
  worker.postMessage(chunk);
  const chunkSummary = await chunkReply as {
    kind: string;
    type: string;
    timestamp: number;
    byteLength: number;
  };
  assert(chunkSummary.kind === "audio-chunk", chunkSummary.kind);
  assert(chunkSummary.type === "key", chunkSummary.type);
  assert(chunkSummary.timestamp === 20, String(chunkSummary.timestamp));
  assert(chunkSummary.byteLength === 3, String(chunkSummary.byteLength));
  await assertThrows(
    () => worker.postMessage(chunk, [chunk]),
    "DataCloneError",
  );
  assert(chunk.byteLength === 3, "chunk stays");
  worker.terminate();
});

Yuzuko.test({
  name: "prefer-software H.264 still decodes in hardware",
  ignore: !hardwareAvc,
  async fn() {
    const expected = await golden();
    const frames = await decodeVideo(
      "avc1.64000B",
      await bytes("avc1.avcC"),
      await bytes("avc1.idr"),
      { hardwareAcceleration: "prefer-software" },
    );
    assert(frames.length === 1, "prefer-software frame");
    comparePicture(await drawFrame(frames[0]), expected, 16);
    frames[0].close();
  },
});

function solidBitmap(pixels: number[]): ImageBitmap {
  const canvas = new OffscreenCanvas(2, 2);
  const context = canvas.getContext("2d");
  const data = new ImageData(new Uint8ClampedArray(pixels), 2, 2);
  context.putImageData(data, 0, 0);
  return canvas.transferToImageBitmap();
}

Yuzuko.test("VideoFrame metadata is empty and copyTo checks its receiver", async () => {
  const init = {
    format: "RGBA" as const,
    codedWidth: 1,
    codedHeight: 1,
    timestamp: 0,
    metadata: { rtpTimestamp: 7 },
  };
  const frame = new VideoFrame(new Uint8Array([1, 2, 3, 255]), init);
  assert(VideoFrame.prototype.metadata.length === 0, "metadata length");
  assert(VideoFrame.prototype.metadata.name === "metadata", "metadata name");
  const meta = frame.metadata();
  assert(meta !== null && typeof meta === "object" && !Array.isArray(meta), "object");
  assert(Object.keys(meta).length === 0, "empty");
  assert(frame.metadata() !== meta, "fresh object");
  assertTypeError(() => VideoFrame.prototype.metadata.call({}), "foreign metadata");
  assertTypeError(() => VideoFrame.prototype.metadata.call(null), "null metadata");
  assert(VideoFrame.prototype.copyTo.length === 1, "copyTo length");
  const foreign = VideoFrame.prototype.copyTo.call({}, new Uint8Array(4));
  assert(foreign instanceof Promise, "foreign copyTo returns a promise");
  const missing = frame.copyTo();
  assert(missing instanceof Promise, "missing destination returns a promise");
  let foreignName = "";
  let missingName = "";
  try {
    await foreign;
  } catch (error) {
    foreignName = error instanceof TypeError ? "TypeError" : String(error);
  }
  try {
    await missing;
  } catch (error) {
    missingName = error instanceof TypeError ? "TypeError" : String(error);
  }
  assert(foreignName === "TypeError", foreignName);
  assert(missingName === "TypeError", missingName);
  frame.close();
  const closed = frame.metadata();
  assert(Object.keys(closed).length === 0, "closed metadata");
  frame.close();
});

Yuzuko.test("VideoFrame constructs from an image, a canvas, and a buffer", async () => {
  const bitmap = solidBitmap([
    10, 20, 30, 255,
    40, 50, 60, 255,
    70, 80, 90, 255,
    100, 110, 120, 255,
  ]);
  assertTypeError(
    () => new VideoFrame(bitmap),
    "timestamp",
  );
  const frame = new VideoFrame(bitmap, { timestamp: 10, duration: 15 });
  assert(frame.format === "RGBA", frame.format ?? "null");
  assert(frame.timestamp === 10, "timestamp");
  assert(frame.duration === 15, "duration");
  assert(frame.codedWidth === 2 && frame.codedHeight === 2, "coded");
  assert(frame.displayWidth === 2 && frame.displayHeight === 2, "display");
  assert(frame.visibleRect !== null && frame.visibleRect.width === 2, "visible");
  assert(frame.codedRect !== null && frame.codedRect.height === 2, "coded rect");
  assert(frame.colorSpace.primaries === "bt709", "primaries");
  assert(frame.colorSpace.fullRange === true, "range");
  assert(frame.allocationSize() === 16, "allocation");
  const pixels = new Uint8Array(16);
  const layout = await frame.copyTo(pixels);
  assert(layout.length === 1 && layout[0].offset === 0 && layout[0].stride === 8, "layout");
  assert(pixels[0] === 10 && pixels[1] === 20 && pixels[2] === 30 && pixels[3] === 255, "pixel");
  const cloned = frame.clone();
  const clonedPixels = new Uint8Array(16);
  await cloned.copyTo(clonedPixels);
  assert(clonedPixels[4] === 40 && clonedPixels[7] === 255, "clone");
  assert(cloned.timestamp === 10 && cloned.duration === 15, "clone time");
  frame.close();
  frame.close();
  assert(frame.format === null, "closed format");
  assert(frame.codedWidth === 0 && frame.displayWidth === 0, "closed size");
  assert(frame.visibleRect === null && frame.codedRect === null, "closed rect");
  assert(frame.colorSpace.primaries === null, "closed primaries");
  assert(frame.colorSpace.matrix === null && frame.colorSpace.fullRange === null, "closed color");
  assert(frame.timestamp === 10 && frame.duration === 15, "closed time");
  await assertThrows(() => frame.clone(), "InvalidStateError");
  await assertThrows(() => frame.allocationSize(), "InvalidStateError");
  await assertThrows(() => frame.copyTo(new Uint8Array(16)), "InvalidStateError");
  cloned.close();
  bitmap.close();
  await assertThrows(
    () => new VideoFrame(bitmap, { timestamp: 1 }),
    "InvalidStateError",
  );

  const canvas = new OffscreenCanvas(2, 2);
  const context = canvas.getContext("2d");
  context.putImageData(new ImageData(new Uint8ClampedArray([
    1, 2, 3, 255,
    5, 6, 7, 255,
    9, 10, 11, 255,
    13, 14, 15, 255,
  ]), 2, 2), 0, 0);
  const opaque = new VideoFrame(canvas, {
    timestamp: 5,
    alpha: "discard",
    visibleRect: { x: 0, y: 0, width: 2, height: 1 },
    displayWidth: 4,
    displayHeight: 2,
  });
  assert(opaque.format === "RGBX", opaque.format ?? "null");
  assert(opaque.codedWidth === 2 && opaque.codedHeight === 2, "canvas coded");
  assert(opaque.visibleRect !== null && opaque.visibleRect.height === 1, "canvas visible");
  assert(opaque.displayWidth === 4 && opaque.displayHeight === 2, "canvas display");
  const row = new Uint8Array(8);
  await opaque.copyTo(row);
  assert(row[0] === 1 && row[1] === 2 && row[2] === 3 && row[3] === 255, "discard x");
  assert(row[4] === 5 && row[7] === 255, "discard x2");
  opaque.close();

  const rgba = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 255]);
  const bufferFrame = new VideoFrame(rgba, {
    format: "RGBA",
    codedWidth: 2,
    codedHeight: 1,
    timestamp: 7,
  });
  assert(bufferFrame.colorSpace.primaries === "bt709", "rgb primaries");
  assert(bufferFrame.colorSpace.transfer === "iec61966-2-1", "rgb transfer");
  assert(bufferFrame.colorSpace.matrix === "rgb", "rgb matrix");
  assert(bufferFrame.colorSpace.fullRange === true, "rgb range");
  const round = new Uint8Array(8);
  await bufferFrame.copyTo(round);
  assert(round[0] === 1 && round[3] === 4 && round[4] === 5 && round[7] === 255, "rgba round");
  const managed = new VideoFrame(new Uint8Array([50, 100, 150, 255]), {
    format: "RGBA",
    codedWidth: 1,
    codedHeight: 1,
    timestamp: 1,
  });
  const p3 = new Uint8Array(4);
  await managed.copyTo(p3, { format: "RGBA", colorSpace: "display-p3" });
  assert(p3[0] === 62 && p3[1] === 99 && p3[2] === 146 && p3[3] === 255, "display-p3");
  const srgb = new Uint8Array(4);
  await managed.copyTo(srgb, { format: "RGBA", colorSpace: "srgb" });
  assert(srgb[0] === 50 && srgb[1] === 100 && srgb[2] === 150 && srgb[3] === 255, "srgb");
  managed.close();
  const drawn = new OffscreenCanvas(2, 1).getContext("2d");
  drawn.drawImage(bufferFrame, 0, 0);
  const sample = drawn.getImageData(1, 0, 1, 1).data;
  assert(sample[0] === 5 && sample[1] === 6 && sample[2] === 7 && sample[3] === 255, "draw");
  const child = new VideoFrame(bufferFrame);
  assert(child.timestamp === 7 && child.format === "RGBA", "child");
  child.close();

  const padded = new Uint8Array(8);
  padded.fill(0xAA);
  const dot = new VideoFrame(new Uint8Array([9, 8, 7, 6]), {
    format: "RGBA",
    codedWidth: 1,
    codedHeight: 1,
    timestamp: 1,
  });
  await dot.copyTo(padded, { layout: [{ offset: 2, stride: 4 }] });
  assert(padded[0] === 0xAA && padded[1] === 0xAA, "pad head");
  assert(padded[2] === 9 && padded[5] === 6, "pad sample");
  assert(padded[6] === 0xAA && padded[7] === 0xAA, "pad tail");
  dot.close();

  const wide = new Uint8Array([
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
  ]);
  const cropped = new VideoFrame(wide, {
    format: "RGBA",
    codedWidth: 4,
    codedHeight: 1,
    timestamp: 1,
    visibleRect: { x: 2, y: 0, width: 2, height: 1 },
  });
  assert(cropped.codedWidth === 2 && cropped.visibleRect !== null && cropped.visibleRect.x === 0, "crop");
  const cropBytes = new Uint8Array(8);
  await cropped.copyTo(cropBytes);
  assert(cropBytes[0] === 9 && cropBytes[4] === 13, "crop bytes");
  cropped.close();
  bufferFrame.close();

  const i420 = new Uint8Array([
    1, 2, 3, 4, 5, 6, 7, 8,
    9, 10,
    11, 12,
  ]);
  const yuv = new VideoFrame(i420, {
    format: "I420",
    codedWidth: 4,
    codedHeight: 2,
    timestamp: 3,
  });
  assert(yuv.allocationSize() === 12, "i420 size");
  assert(yuv.colorSpace.matrix === "bt709" && yuv.colorSpace.fullRange === false, "i420 color");
  const yuvOut = new Uint8Array(12);
  const planes = await yuv.copyTo(yuvOut);
  assert(planes.length === 3, "i420 planes");
  assert(yuvOut[0] === 1 && yuvOut[8] === 9 && yuvOut[10] === 11, "i420 bytes");
  assert(yuv.allocationSize({ format: "RGBA" }) === 32, "i420 rgba size");
  assertTypeError(
    () => yuv.allocationSize({ layout: [{ offset: 0, stride: 4 }] }),
    "plane count",
  );
  assertTypeError(
    () => yuv.allocationSize({
      layout: [
        { offset: 0, stride: 1 },
        { offset: 8, stride: 2 },
        { offset: 10, stride: 2 },
      ],
    }),
    "stride",
  );
  yuv.close();

  const rgbx = new VideoFrame(new Uint8Array([
    255, 0, 0, 0,
    0, 255, 0, 0x40,
  ]), {
    format: "RGBX",
    codedWidth: 2,
    codedHeight: 1,
    timestamp: 1,
  });
  const rgbxOut = new Uint8Array(8);
  await rgbx.copyTo(rgbxOut);
  assert(rgbxOut[0] === 255 && rgbxOut[3] === 255, "rgbx opaque");
  assert(rgbxOut[4] === 0 && rgbxOut[5] === 255 && rgbxOut[7] === 255, "rgbx second");
  rgbx.close();

  const p10 = new Uint8Array([
    1, 0, 2, 0, 3, 0, 4, 0, 5, 0, 6, 0, 7, 0, 8, 0,
    9, 0, 10, 0,
    11, 0, 12, 0,
  ]);
  const tenBit = new VideoFrame(p10, {
    format: "I420P10",
    codedWidth: 4,
    codedHeight: 2,
    timestamp: 0,
  });
  assert(tenBit.format === "I420P10", tenBit.format ?? "null");
  assert(tenBit.allocationSize() === 24, "p10 size");
  const tenOut = new Uint8Array(24);
  await tenBit.copyTo(tenOut);
  assert(tenOut[0] === 1 && tenOut[1] === 0 && tenOut[16] === 9, "p10 bytes");
  tenBit.close();
  await assertThrows(() => new VideoFrame(new Uint8Array(16), {
    format: "I420AP10",
    timestamp: 0,
    codedWidth: 2,
    codedHeight: 2,
  }), "NotSupportedError");
  const again = solidBitmap([
    1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255,
  ]);
  assertTypeError(
    () => new VideoFrame(again, { timestamp: 1, displayWidth: 4 }),
    "display pair",
  );
  again.close();

  const sizes: Array<[VideoPixelFormat, number]> = [
    ["BGRA", 32],
    ["RGBX", 32],
    ["BGRX", 32],
    ["I420A", 20],
    ["I422", 16],
    ["I444", 24],
    ["NV12", 12],
  ];
  for (const [format, size] of sizes) {
    const stored = new VideoFrame(new Uint8Array(size), {
      format,
      codedWidth: 4,
      codedHeight: 2,
      timestamp: 1,
    });
    assert(stored.format === format, format);
    assert(stored.allocationSize() === size, `${format} size`);
    const copy = new Uint8Array(size);
    await stored.copyTo(copy);
    stored.close();
  }
});

Yuzuko.test("AudioData constructs, copies, and closes", async () => {
  const planar = new Float32Array([
    0.25, 0.5, 0.75, 1,
    -0.25, -0.5, -0.75, -1,
  ]);
  const data = new AudioData({
    format: "f32-planar",
    sampleRate: 8000,
    numberOfFrames: 4,
    numberOfChannels: 2,
    timestamp: -10,
    data: planar,
  });
  assert(data.format === "f32-planar", data.format ?? "null");
  assert(data.sampleRate === 8000, "rate");
  assert(data.numberOfFrames === 4, "frames");
  assert(data.numberOfChannels === 2, "channels");
  assert(data.timestamp === -10, "timestamp");
  assert(data.duration === 500, "duration");
  assert(data.allocationSize({ planeIndex: 0, frameOffset: 1, frameCount: 2 }) === 8, "size");
  const window = new Float32Array([9, 9, 9, 9]);
  data.copyTo(window.subarray(1, 3), {
    planeIndex: 0,
    frameOffset: 1,
    frameCount: 2,
  });
  assert(window[0] === 9 && window[3] === 9, "untouched");
  assert(window[1] === 0.5 && window[2] === 0.75, "planar window");
  planar[1] = 9;
  const again = new Float32Array(2);
  data.copyTo(again, { planeIndex: 0, frameOffset: 1, frameCount: 2 });
  assert(again[0] === 0.5 && again[1] === 0.75, "source copy");
  const outside = new AudioData({
    format: "f32",
    sampleRate: 8000,
    numberOfFrames: 2,
    numberOfChannels: 1,
    timestamp: 0,
    data: new Float32Array([2, -2]),
  });
  const kept = new Float32Array(2);
  outside.copyTo(kept, { planeIndex: 0 });
  assert(kept[0] === 2 && kept[1] === -2, "outside unit");
  outside.close();

  const interleaved = new Int16Array([
    -1000, 1000,
    2000, 3000,
    -2000, 4000,
    500, -500,
  ]);
  const pcm = new AudioData({
    format: "s16",
    sampleRate: 44100,
    numberOfFrames: 4,
    numberOfChannels: 2,
    timestamp: 7,
    data: interleaved,
  });
  const converted = new Float32Array(2);
  pcm.copyTo(converted, {
    planeIndex: 0,
    frameOffset: 1,
    frameCount: 2,
    format: "f32-planar",
  });
  assertNear(converted[0], 2000 / 32767, "positive s16");
  assertNear(converted[1], -2000 / 32768, "negative s16");
  const channel = new Int16Array(2);
  pcm.copyTo(channel, {
    planeIndex: 1,
    frameOffset: 1,
    frameCount: 2,
    format: "s16-planar",
  });
  assert(channel[0] === 3000 && channel[1] === 4000, "s16 plane");
  interleaved[2] = 0;
  const stable = new Int16Array(2);
  pcm.copyTo(stable, {
    planeIndex: 1,
    frameOffset: 1,
    frameCount: 2,
    format: "s16-planar",
  });
  assert(stable[0] === 3000 && stable[1] === 4000, "s16 copy");
  assertRangeError(
    () => pcm.copyTo(new Float32Array(8), { planeIndex: 1 }),
    "interleaved plane",
  );
  assertRangeError(
    () => pcm.copyTo(new Int16Array(1), {
      planeIndex: 1,
      frameOffset: 1,
      frameCount: 2,
      format: "s16-planar",
    }),
    "short destination",
  );
  assertRangeError(
    () => data.copyTo(new Float32Array(4), { planeIndex: 0, frameOffset: 4 }),
    "frameOffset",
  );
  assertTypeError(
    () => data.copyTo(new Float32Array(4), {} as AudioDataCopyToOptions),
    "planeIndex",
  );

  const cloned = data.clone();
  data.close();
  data.close();
  assert(data.format === null, "closed format");
  assert(data.sampleRate === 0, "closed rate");
  assert(data.numberOfFrames === 0, "closed frames");
  assert(data.numberOfChannels === 0, "closed channels");
  assert(data.duration === 0, "closed duration");
  assert(data.timestamp === -10, "closed timestamp");
  assert(cloned.format === "f32-planar", "clone stays");
  assert(cloned.numberOfFrames === 4, "clone frames");
  await assertThrows(() => data.clone(), "InvalidStateError");
  await assertThrows(
    () => data.allocationSize({ planeIndex: 0 }),
    "InvalidStateError",
  );
  await assertThrows(
    () => data.copyTo(new Float32Array(4), { planeIndex: 0 }),
    "InvalidStateError",
  );
  cloned.close();
  pcm.close();

  const init = {
    format: "f32-planar" as AudioSampleFormat,
    sampleRate: 8000,
    numberOfFrames: 1,
    numberOfChannels: 1,
    timestamp: 0,
    data: new Float32Array([0]),
  };
  assertTypeError(() => {
    const { format: _format, ...rest } = init;
    new AudioData(rest as AudioDataInit);
  }, "format");
  assertTypeError(
    () => new AudioData({ ...init, numberOfFrames: 0 }),
    "frames",
  );
  assertTypeError(
    () => new AudioData({
      ...init,
      format: "s16",
      numberOfFrames: 4,
      numberOfChannels: 2,
      data: new Int16Array(7),
    }),
    "short data",
  );
});

async function assertRejectsType(
  run: () => Promise<unknown>,
  message: string,
): Promise<void> {
  let result: Promise<unknown>;
  try {
    result = run();
  } catch (error) {
    throw new Error(`${message} threw ${String(error)}`);
  }
  assert(result instanceof Promise, `${message} did not return a promise`);
  try {
    await result;
  } catch (error) {
    assert(error instanceof TypeError, `${message}: ${String(error)}`);
    return;
  }
  throw new Error(`${message} was not rejected`);
}

Yuzuko.test("isConfigSupported and encoded chunks", async () => {
  const avc = await VideoDecoder.isConfigSupported({
    codec: "avc1.42001E",
    codedWidth: 16,
    codedHeight: 16,
  });
  assert(avc.supported === true, "avc1");
  assert(avc.config?.codec === "avc1.42001E", "avc1 codec");
  assert(avc.config?.codedWidth === 16, "codedWidth");
  assert(avc.config?.codedHeight === 16, "codedHeight");

  const avc3 = await VideoDecoder.isConfigSupported({ codec: "avc3.42001E" });
  assert(avc3.supported === true, "avc3");
  assert(avc3.config?.codec === "avc3.42001E", "avc3 codec");

  const vp8 = await VideoDecoder.isConfigSupported({ codec: "vp8" });
  assert(vp8.supported === false, "vp8");
  assert(vp8.config?.codec === "vp8", "vp8 codec");

  const av1 = await VideoDecoder.isConfigSupported({
    codec: "av01.0.04M.08",
    codedWidth: 16,
    codedHeight: 16,
  });
  assert(av1.supported === true, "av01");
  assert(av1.config?.codec === "av01.0.04M.08", "av01 codec");
  const av1Ten = await VideoDecoder.isConfigSupported({
    codec: "av01.0.05M.10",
  });
  assert(av1Ten.supported === true, "av01 10-bit");
  const av1Profile = await VideoDecoder.isConfigSupported({
    codec: "av01.1.05M.08",
  });
  assert(av1Profile.supported === true, "av01 profile 1");
  const av1Professional = await VideoDecoder.isConfigSupported({
    codec: "av01.2.05M.12",
  });
  assert(av1Professional.supported === true, "av01 profile 2");
  const av1Mono = await VideoDecoder.isConfigSupported({
    codec: "av01.0.05M.08.1.110.01.01.01.0",
  });
  assert(av1Mono.supported === true, "av01 monochrome");
  const av1Bad = await VideoDecoder.isConfigSupported({
    codec: "av01.0.00M.08",
    description: new Uint8Array([1, 2, 3, 4]),
  });
  assert(av1Bad.supported === false, "bad av1C");

  await assertRejectsType(
    () => VideoDecoder.isConfigSupported({} as VideoDecoderConfig),
    "missing codec",
  );
  await assertRejectsType(
    () => VideoDecoder.isConfigSupported({ codec: "" }),
    "empty codec",
  );
  await assertRejectsType(
    () =>
      VideoDecoder.isConfigSupported({
        codec: "avc1.42001E",
        hardwareAcceleration: "maybe" as HardwareAcceleration,
      }),
    "hardwareAcceleration",
  );
  const software = await VideoDecoder.isConfigSupported({
    codec: "avc1.42001E",
    hardwareAcceleration: "prefer-software",
  });
  assert(software.supported === true, "prefer-software");
  await assertRejectsType(
    () =>
      VideoDecoder.isConfigSupported({
        codec: "avc1.42001E",
        codedWidth: 0,
      }),
    "codedWidth",
  );
  await assertRejectsType(
    () =>
      (VideoDecoder.isConfigSupported as () => Promise<unknown>)(),
    "video arity",
  );

  const decoder = new VideoDecoder({ output: () => {}, error: () => {} });
  assertTypeError(() => decoder.configure({ codec: "" }), "configure empty");
  assert(decoder.state === "unconfigured", decoder.state);
  decoder.close();

  const aac = await AudioDecoder.isConfigSupported({
    codec: "mp4a.40.2",
    sampleRate: 48000,
    numberOfChannels: 2,
  });
  assert(aac.supported === true, "mp4a");
  assert(aac.config?.codec === "mp4a.40.2", "mp4a codec");
  assert(aac.config?.sampleRate === 48000, "sampleRate");
  assert(aac.config?.numberOfChannels === 2, "channels");

  const opus = await AudioDecoder.isConfigSupported({
    codec: "opus",
    sampleRate: 48000,
    numberOfChannels: 2,
  });
  assert(opus.supported === true, "opus");
  assert(opus.config?.codec === "opus", "opus codec");
  const opusWide = await AudioDecoder.isConfigSupported({
    codec: "opus",
    sampleRate: 48000,
    numberOfChannels: 16,
  });
  assert(opusWide.supported === true, "opus 16ch");
  const opusFuture = await AudioDecoder.isConfigSupported({
    codec: "opus.123",
    sampleRate: 48000,
    numberOfChannels: 2,
  });
  assert(opusFuture.supported === false, "opus.123");
  const aacFuture = await AudioDecoder.isConfigSupported({
    codec: "mp4a.FF.9",
    sampleRate: 48000,
    numberOfChannels: 2,
  });
  assert(aacFuture.supported === false, "mp4a.FF.9");
  const futureAvc = await VideoDecoder.isConfigSupported({ codec: "avc1.FF000b" });
  assert(futureAvc.supported === false, "avc1.FF000b");
  const futureLevel = await VideoDecoder.isConfigSupported({
    codec: "avc1.4D401D",
  });
  assert(futureLevel.supported === false, "avc1.4D401D");
  await assertRejectsType(
    () =>
      AudioDecoder.isConfigSupported({
        codec: "mp4a.40.2",
        sampleRate: 0,
        numberOfChannels: 2,
      }),
    "sampleRate",
  );
  await assertRejectsType(
    () =>
      (AudioDecoder.isConfigSupported as () => Promise<unknown>)(),
    "audio arity",
  );

  const source = new Uint8Array([9, 8, 7, 6]);
  const video = new EncodedVideoChunk({
    type: "key",
    timestamp: 20,
    duration: 15,
    data: source,
  });
  assert(video.duration === 15, "video duration");
  const copied = new Uint8Array(4);
  video.copyTo(copied);
  assert(copied[0] === 9 && copied[3] === 6, "video bytes");
  source[0] = 1;
  const again = new Uint8Array(4);
  video.copyTo(again);
  assert(again[0] === 9, "source mutation");
  assertTypeError(() => video.copyTo(new Uint8Array(3)), "short video");
  const cloned = structuredClone(video);
  assert(cloned.duration === 15, "cloned duration");
  assert(cloned.byteLength === 4, "cloned length");
  const clonedBytes = new Uint8Array(4);
  cloned.copyTo(clonedBytes);
  assert(clonedBytes[1] === 8 && clonedBytes[0] === 9, "cloned bytes");

  const audio = new EncodedAudioChunk({
    type: "delta",
    timestamp: -5,
    data: new Uint8Array([1, 2]),
  });
  assert(audio.duration === null, "audio duration");
  assert(audio.timestamp === -5, "audio timestamp");
  const audioBytes = new Uint8Array(2);
  audio.copyTo(audioBytes);
  assert(audioBytes[0] === 1 && audioBytes[1] === 2, "audio bytes");

  const shared = new SharedArrayBuffer(4);
  const view = new Uint8Array(shared);
  video.copyTo(view);
  assert(view[0] === 9 && view[3] === 6, "shared view");
  const bare = new SharedArrayBuffer(4);
  video.copyTo(bare);
  const bareView = new Uint8Array(bare);
  assert(bareView[2] === 7, "shared buffer");
});

Yuzuko.test("AV1 decodes an 8-bit 4:2:0 MP4", async () => {
  const response = await fetch(
    new URL("./fixtures/media/red-av1.mp4", import.meta.url),
  );
  const file = new Uint8Array(await response.arrayBuffer());
  const movie = Yuzuko.demuxMp4(file);
  assert(movie.video.length === 1, "one video track");
  const track = movie.video[0];
  assert(track.codec.startsWith("av01.0."), track.codec);
  assert(track.codec.endsWith("M.08"), track.codec);
  assert(track.codedWidth === 16, "width");
  assert(track.codedHeight === 16, "height");
  assert(track.chunks.length >= 1, "chunks");

  const supported = await VideoDecoder.isConfigSupported({
    codec: track.codec,
    description: track.description,
    codedWidth: track.codedWidth,
    codedHeight: track.codedHeight,
    hardwareAcceleration: "prefer-hardware",
  });
  assert(supported.supported === true, "prefer-hardware");

  const frames: VideoFrame[] = [];
  let failed = "";
  const decoder = new VideoDecoder({
    output: (frame) => {
      frames.push(frame);
    },
    error: (error) => {
      failed = `${error.name}: ${error.message}`;
    },
  });
  decoder.configure({
    codec: track.codec,
    description: track.description,
    codedWidth: track.codedWidth,
    codedHeight: track.codedHeight,
    hardwareAcceleration: "prefer-hardware",
  });
  for (const chunk of track.chunks) {
    decoder.decode(chunk);
  }
  await decoder.flush();
  assert(failed === "", failed);
  assert(frames.length >= 1, "frames");
  const image = await drawFrame(frames[0]);
  const index = (8 * image.width + 8) * 4;
  const red = image.data[index];
  const green = image.data[index + 1];
  const blue = image.data[index + 2];
  const alpha = image.data[index + 3];
  assert(
    red > 180 && green < 80 && blue < 80 && alpha === 255,
    `pixel ${red},${green},${blue},${alpha}`,
  );
  for (const frame of frames) {
    frame.close();
  }
  decoder.close();

  let rejected = "";
  const bad = new VideoDecoder({
    output: () => {},
    error: (error) => {
      rejected = error instanceof DOMException ? error.name : "";
    },
  });
  bad.configure({
    codec: "av01.0.00M.08",
    codedWidth: 16,
    codedHeight: 16,
    description: new Uint8Array([1, 2, 3, 4]),
  });
  assert(bad.state === "closed", bad.state);
  await assertThrows(() => bad.flush(), "NotSupportedError");
  assert(rejected === "NotSupportedError", rejected);

  const empty = new VideoDecoder({ output: () => {}, error: () => {} });
  empty.configure({ codec: "av01.0.04M.08", codedWidth: 16, codedHeight: 16 });
  assert(empty.state === "configured", empty.state);
  empty.close();
});

Yuzuko.test("AV1 software output matches Chrome pixel formats", async () => {
  const cases: Array<[string, VideoPixelFormat | "I420P10" | "I420P12" | "I422P10" | "I444P10"]> = [
    ["av1-420p10.mp4", "I420P10"],
    ["av1-420p12.mp4", "I420P12"],
    ["av1-422.mp4", "I422"],
    ["av1-422p10.mp4", "I422P10"],
    ["av1-444.mp4", "I444"],
    ["av1-444p10.mp4", "I444P10"],
    ["av1-mono.mp4", "I420"],
  ];
  for (const [name, format] of cases) {
    const response = await fetch(
      new URL(`./fixtures/media/${name}`, import.meta.url),
    );
    const file = new Uint8Array(await response.arrayBuffer());
    const track = Yuzuko.demuxMp4(file).video[0];
    const supported = await VideoDecoder.isConfigSupported({
      codec: track.codec,
      description: track.description,
      codedWidth: track.codedWidth,
      codedHeight: track.codedHeight,
      hardwareAcceleration: "prefer-software",
    });
    assert(supported.supported === true, name);
    const frames: VideoFrame[] = [];
    let failed = "";
    const decoder = new VideoDecoder({
      output: (frame) => {
        frames.push(frame);
      },
      error: (error) => {
        failed = `${error.name}: ${error.message}`;
      },
    });
    decoder.configure({
      codec: track.codec,
      description: track.description,
      codedWidth: track.codedWidth,
      codedHeight: track.codedHeight,
      hardwareAcceleration: "prefer-software",
    });
    for (const chunk of track.chunks) {
      decoder.decode(chunk);
    }
    await decoder.flush();
    assert(failed === "", `${name} ${failed}`);
    assert(frames.length >= 1, name);
    assert(frames[0].format === format, `${name} ${frames[0].format}`);
    if (name === "av1-mono.mp4") {
      const bytes = new Uint8Array(frames[0].allocationSize());
      await frames[0].copyTo(bytes);
      assert(bytes[256] === 128 && bytes[bytes.length - 1] === 128, "neutral");
    }
    const image = await drawFrame(frames[0]);
    assert(image.data[3] === 255, `${name} alpha`);
    for (const frame of frames) {
      frame.close();
    }
    decoder.close();
  }
});

Yuzuko.test("AV1 prefers the hardware decoder when it accepts the frame", async () => {
  const response = await fetch(
    new URL("./fixtures/media/red-av1-256.mp4", import.meta.url),
  );
  const file = new Uint8Array(await response.arrayBuffer());
  const movie = Yuzuko.demuxMp4(file);
  const track = movie.video[0];
  assert(track.codedWidth === 256 && track.codedHeight === 256, "size");
  for (const hint of ["prefer-hardware", "prefer-software", "no-preference"]) {
    const frames: VideoFrame[] = [];
    let failed = "";
    const decoder = new VideoDecoder({
      output: (frame) => {
        frames.push(frame);
      },
      error: (error) => {
        failed = `${error.name}: ${error.message}`;
      },
    });
    decoder.configure({
      codec: track.codec,
      description: track.description,
      codedWidth: track.codedWidth,
      codedHeight: track.codedHeight,
      hardwareAcceleration: hint,
    });
    for (const chunk of track.chunks) {
      decoder.decode(chunk);
    }
    await decoder.flush();
    assert(failed === "", `${hint} ${failed}`);
    assert(frames.length >= 1, hint);
    const image = await drawFrame(frames[0]);
    const index = (128 * image.width + 128) * 4;
    const red = image.data[index];
    const green = image.data[index + 1];
    const blue = image.data[index + 2];
    const alpha = image.data[index + 3];
    assert(
      red > 180 && green < 80 && blue < 80 && alpha === 255,
      `${hint} pixel ${red},${green},${blue},${alpha}`,
    );
    for (const frame of frames) {
      frame.close();
    }
    decoder.close();
  }
});

Yuzuko.test("VideoFrame rotation and flip orient the draw", async () => {
  // y y r r
  // b b g g
  const bytes = new Uint8Array([
    255, 255, 0, 255, 255, 255, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
    0, 0, 255, 255, 0, 0, 255, 255, 0, 255, 0, 255, 0, 255, 0, 255,
  ]);
  const frame = new VideoFrame(bytes, {
    format: "RGBX",
    codedWidth: 4,
    codedHeight: 2,
    timestamp: 0,
    rotation: -315,
    flip: true,
  });
  assert(frame.rotation === 90, "rotation");
  assert(frame.flip === true, "flip");
  assert(frame.visibleRect?.width === 4 && frame.visibleRect.height === 2, "visible");
  assert(frame.codedWidth === 4 && frame.codedHeight === 2, "coded");
  assert(frame.displayWidth === 2 && frame.displayHeight === 4, "display");
  const coded = new Uint8Array(frame.allocationSize());
  await frame.copyTo(coded);
  assert(coded[4] === 255 && coded[5] === 255 && coded[6] === 0, "coded samples");
  const cloned = structuredClone(frame);
  assert(cloned.rotation === 90 && cloned.flip === true, "clone");
  cloned.close();

  const canvas = new OffscreenCanvas(frame.displayWidth, frame.displayHeight);
  const ctx = canvas.getContext("2d");
  assert(ctx !== null, "context");
  ctx.drawImage(frame, 0, 0);
  const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
  function near(
    x: number,
    y: number,
    expected: [number, number, number],
    message: string,
  ): void {
    const index = 4 * (canvas.width * y + x);
    for (let channel = 0; channel < 3; channel += 1) {
      const actual = data[index + channel] ?? 0;
      assert(Math.abs(actual - expected[channel]) <= 2, `${message} ${actual}`);
    }
  }
  near(0, 0, [255, 255, 0], "yellow");
  near(1, 0, [0, 0, 255], "blue");
  near(0, 3, [255, 0, 0], "red");
  near(1, 3, [0, 255, 0], "green");

  const wrapped = new VideoFrame(frame, { rotation: 90, flip: true });
  assert(wrapped.rotation === 0 && wrapped.flip === false, "composed");
  assert(wrapped.displayWidth === 4 && wrapped.displayHeight === 2, "composed display");
  wrapped.close();
  frame.close();
  assert(frame.rotation === 0 && frame.flip === false, "closed");

  const shown = new VideoFrame(bytes, {
    format: "RGBX",
    codedWidth: 4,
    codedHeight: 2,
    timestamp: 0,
    rotation: 90,
    displayWidth: 4,
    displayHeight: 2,
  });
  assert(shown.displayWidth === 4 && shown.displayHeight === 2, "explicit display");
  const bitmap = await createImageBitmap(shown);
  assert(bitmap.width === 2 && bitmap.height === 4, "oriented bitmap");
  bitmap.close();
  shown.close();

  assertTypeError(
    () =>
      new VideoFrame(bytes, {
        format: "RGBX",
        codedWidth: 4,
        codedHeight: 2,
        timestamp: 0,
        rotation: Infinity,
      }),
    "rotation",
  );
});

Yuzuko.test("AudioDecoder dispatches a dequeue event", () => {
  const decoder = new AudioDecoder({ output: () => {}, error: () => {} });
  assert(decoder instanceof EventTarget, "EventTarget");
  assert(decoder.ondequeue === null, "ondequeue starts null");
  decoder.configure({
    codec: "opus",
    sampleRate: 48000,
    numberOfChannels: 1,
  });
  const handled: Event[] = [];
  const listened: Event[] = [];
  decoder.ondequeue = (event) => {
    assert(event instanceof Event, "handler event");
    assert(event.type === "dequeue", event.type);
    assert(event.target === decoder, "target");
    assert(event.currentTarget === decoder, "currentTarget");
    assert(event.eventPhase === Event.AT_TARGET, "phase");
    assert(event.isTrusted === true, "trusted");
    assert(event.bubbles === false, "bubbles");
    assert(event.cancelable === false, "cancelable");
    assert(decoder.decodeQueueSize === 0, "size while handling");
    handled.push(event);
  };
  decoder.addEventListener("dequeue", (event) => {
    listened.push(event);
  });
  const chunk = new EncodedAudioChunk({
    type: "key",
    timestamp: 0,
    data: new Uint8Array([1, 2, 3, 4]),
  });
  decoder.decode(chunk);
  assert(decoder.decodeQueueSize === 1, "queued");
  decoder.reset();
  assert(decoder.decodeQueueSize === 0, "cleared");
  assert(handled.length === 1, "one handler call");
  assert(listened.length === 1 && listened[0] === handled[0], "same event");
  decoder.ondequeue = null;
  assert(decoder.ondequeue === null, "cleared handler");
  decoder.configure({
    codec: "opus",
    sampleRate: 48000,
    numberOfChannels: 1,
  });
  decoder.decode(chunk);
  decoder.reset();
  assert(handled.length === 1, "removed handler stays quiet");
  assert(listened.length === 2, "listener still runs");
  decoder.close();
});

Yuzuko.test("Opus pre-skip moves only a negative timestamp", async () => {
  const response = await fetch(
    new URL("./wpt/suite/webcodecs/sfx-opus.ogg", import.meta.url),
  );
  const file = new Uint8Array(await response.arrayBuffer());
  const description = file.subarray(28, 47);
  const first = file.subarray(185, 635);
  const second = file.subarray(635, 903);
  const decode = async (timestamp: number): Promise<AudioData[]> => {
    const frames: AudioData[] = [];
    let failed = "";
    const decoder = new AudioDecoder({
      output: (data) => {
        frames.push(data);
      },
      error: (error) => {
        failed = error instanceof DOMException ? error.name : String(error);
      },
    });
    decoder.configure({
      codec: "opus",
      sampleRate: 44100,
      numberOfChannels: 1,
      description,
    });
    decoder.decode(
      new EncodedAudioChunk({ type: "key", timestamp, data: first }),
    );
    decoder.decode(
      new EncodedAudioChunk({
        type: "key",
        timestamp: 20_000 + timestamp,
        data: second,
      }),
    );
    await decoder.flush();
    assert(failed === "", failed);
    decoder.close();
    return frames;
  };
  const early = await decode(-42);
  assert(early.length === 2, `frames ${early.length}`);
  assert(early[0].sampleRate === 48000, `rate ${early[0].sampleRate}`);
  assert(early[0].numberOfChannels === 1, "channels");
  assert(early[0].numberOfFrames === 960 - 312, `frames ${early[0].numberOfFrames}`);
  assert(early[0].timestamp === -42 + 6500, `timestamp ${early[0].timestamp}`);
  assert(early[1].numberOfFrames === 960, `second ${early[1].numberOfFrames}`);
  assert(early[1].timestamp === 20_000 - 42, `second time ${early[1].timestamp}`);
  for (const frame of early) {
    frame.close();
  }

  const kept = await decode(42);
  assert(kept.length === 2, `kept ${kept.length}`);
  assert(kept[0].timestamp === 42, `kept time ${kept[0].timestamp}`);
  assert(kept[0].sampleRate === 48000, "kept rate");
  assert(kept[0].numberOfFrames === 960 - 312, `kept frames ${kept[0].numberOfFrames}`);
  for (const frame of kept) {
    frame.close();
  }

  let seen = "";
  const wide = new AudioDecoder({
    output: () => {},
    error: (error) => {
      seen = error instanceof DOMException ? error.name : "";
    },
  });
  wide.configure({
    codec: "opus",
    sampleRate: 48000,
    numberOfChannels: 16,
  });
  assert(wide.state === "closed", wide.state);
  await assertThrows(() => wide.flush(), "NotSupportedError");
  assert(seen === "NotSupportedError", seen);
});
