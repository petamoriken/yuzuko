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
 * machine without one, such as a VM or WSL without VA-API, rejects the valid
 * fixture config with `NotSupportedError`, and the picture tests are ignored
 * there. macOS always has VideoToolbox, so the probe must succeed.
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
    return true;
  } catch (error) {
    const unsupported = error instanceof DOMException &&
      error.name === "NotSupportedError";
    if (!unsupported || Yuzuko.build.os === "darwin") {
      throw error;
    }
    return false;
  } finally {
    decoder.close();
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
    "av01.0.04M.08",
  ]) {
    const frames: VideoFrame[] = [];
    const decoder = new VideoDecoder({
      output: (frame) => {
        frames.push(frame);
      },
      error: () => {},
    });
    await assertThrows(
      () =>
        decoder.configure({
          codec,
          codedWidth: 64,
          codedHeight: 64,
          description: new Uint8Array([1, 2, 3, 4]),
        }),
      "NotSupportedError",
    );
    assert(frames.length === 0, codec);
    decoder.close();
  }
  for (const codec of ["flac", "opus", "vorbis"]) {
    const audio = new AudioDecoder({ output: () => {}, error: () => {} });
    await assertThrows(
      () =>
        audio.configure({
          codec,
          sampleRate: 48000,
          numberOfChannels: 1,
        }),
      "NotSupportedError",
    );
    audio.close();
  }
});

Yuzuko.test("a rejected avc1 config yields no VideoFrame", async () => {
  const frames: VideoFrame[] = [];
  const decoder = new VideoDecoder({
    output: (frame) => {
      frames.push(frame);
    },
    error: () => {},
  });
  const bad = await bytes("avc1-bad.avcC");
  await assertThrows(
    () =>
      decoder.configure({
        codec: "avc1.42E01E",
        codedWidth: 64,
        codedHeight: 64,
        description: bad,
      }),
    "NotSupportedError",
  );
  assert(frames.length === 0, "hardware reject produced a frame");
  decoder.close();
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
  await assertThrows(() => audio.numberOfFrames, "InvalidStateError");

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

  await assertThrows(() => new VideoFrame(new Uint8Array(8), {
    format: "I420P10",
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
