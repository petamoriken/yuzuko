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

Yuzuko.test({ name: "an H.264 frame matches the golden and then closes", ignore: !hardwareAvc, fn: async () => {
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
} });

Yuzuko.test({ name: "WebGPU readback matches the H.264 golden", ignore: !hardwareAvc, fn: async () => {
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
} });

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

Yuzuko.test({ name: "a VideoFrame cannot be transferred", ignore: !hardwareAvc, fn: async () => {
  const frames = await decodeVideo(
    "avc1.64000B",
    await bytes("avc1.avcC"),
    await bytes("avc1.idr"),
  );
  const frame = frames[0];
  let cloned = "";
  try {
    structuredClone(frame);
  } catch (error) {
    cloned = error instanceof DOMException ? error.name : "";
  }
  assert(cloned === "DataCloneError", cloned);
  const worker = new Worker(new URL("./fixtures/webcodecs/recv.js", import.meta.url));
  let transfer = "";
  try {
    worker.postMessage(frame);
  } catch (error) {
    transfer = error instanceof DOMException ? error.name : "";
  }
  worker.terminate();
  assert(transfer === "DataCloneError", transfer);
  frame.close();
} });

Yuzuko.test({ name: "prefer-software H.264 still decodes in hardware", ignore: !hardwareAvc, fn: async () => {
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
} });
