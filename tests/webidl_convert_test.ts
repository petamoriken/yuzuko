function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function detach(): ArrayBuffer {
  const buffer = new ArrayBuffer(4);
  structuredClone(buffer, { transfer: [buffer] });
  return buffer;
}

async function capture(run: () => unknown): Promise<unknown> {
  try {
    await run();
  } catch (error) {
    return error;
  }
  return undefined;
}

function assertType(error: unknown, message: string, label: string): void {
  assert(error instanceof TypeError, `${label}: ${String(error)}`);
  assert(error.message === message, `${label}: ${error.message}`);
}

const DETACHED = "The buffer is detached.";
const OUT_OF_RANGE = "The provided value is out of range";
const data = new Uint8Array([1]);

Yuzuko.test("dictionary conversion rethrows and accepts null", () => {
  const getter = new Error("timestamp getter");
  const init: EncodedVideoChunkInit = {
    type: "key",
    get timestamp(): number {
      throw getter;
    },
    data,
  };
  let caught: unknown;
  try {
    new EncodedVideoChunk(init);
  } catch (error) {
    caught = error;
  }
  assert(caught === getter, "timestamp getter");

  const valueOf = new Error("valueOf threw");
  const badNumber = {
    valueOf(): number {
      throw valueOf;
    },
  };
  let converted: unknown;
  try {
    new EncodedVideoChunk({
      type: "key",
      // @ts-expect-error valueOf runs at the WebIDL boundary.
      timestamp: badNumber,
      data,
    });
  } catch (error) {
    converted = error;
  }
  assert(converted === valueOf, "timestamp valueOf");

  const stringified = new Error("toString threw");
  let named: unknown;
  try {
    new EncodedVideoChunk({
      // @ts-expect-error ToString runs at the WebIDL boundary.
      type: {
        toString(): string {
          throw stringified;
        },
      },
      timestamp: 0,
      data,
    });
  } catch (error) {
    named = error;
  }
  assert(named === stringified, "type toString");

  const chunk = new EncodedVideoChunk({
    type: "key",
    timestamp: -0.9,
    // @ts-expect-error null converts with ToNumber.
    duration: null,
    data,
  });
  assert(chunk.timestamp === 0, `timestamp ${chunk.timestamp}`);
  assert(chunk.duration === 0, `duration ${chunk.duration}`);

  const truncated = new EncodedVideoChunk({
    type: "key",
    timestamp: 0,
    duration: -0.9,
    data,
  });
  assert(truncated.duration === 0, `fraction ${truncated.duration}`);
  const limit = new EncodedVideoChunk({
    type: "key",
    timestamp: Number.MAX_SAFE_INTEGER,
    duration: Number.MAX_SAFE_INTEGER,
    data,
  });
  assert(limit.timestamp === Number.MAX_SAFE_INTEGER, "max timestamp");
  assert(limit.duration === Number.MAX_SAFE_INTEGER, "max duration");
});

Yuzuko.test("encoded chunk ranges reject after truncation", () => {
  const cases: Array<{ run: () => void; message: string }> = [
    {
      run: () => new EncodedVideoChunk({ type: "key", timestamp: NaN, data }),
      message: "The timestamp is out of range",
    },
    {
      run: () =>
        new EncodedVideoChunk({ type: "key", timestamp: 2 ** 53, data }),
      message: "The timestamp is out of range",
    },
    {
      run: () =>
        new EncodedVideoChunk({
          type: "key",
          timestamp: 0,
          duration: -1,
          data,
        }),
      message: "duration is out of range",
    },
    {
      run: () =>
        new EncodedVideoChunk({
          type: "key",
          timestamp: 0,
          duration: NaN,
          data,
        }),
      message: "duration is out of range",
    },
    {
      run: () =>
        new EncodedVideoChunk({
          type: "key",
          timestamp: 0,
          duration: 2 ** 53,
          data,
        }),
      message: "duration is out of range",
    },
  ];
  for (const item of cases) {
    let error: unknown;
    try {
      item.run();
    } catch (caught) {
      error = caught;
    }
    assertType(error, item.message, item.message);
  }
});

Yuzuko.test("VideoFrameInit uses modulo and buffer init uses EnforceRange", () => {
  const canvas = new OffscreenCanvas(1, 1);
  const imageInit: VideoFrameInit = {
    timestamp: NaN,
    // @ts-expect-error null converts with ToNumber.
    duration: null,
  };
  const fromImage = new VideoFrame(canvas, imageInit);
  assert(fromImage.timestamp === 0, `image timestamp ${fromImage.timestamp}`);
  assert(fromImage.duration === 0, `image duration ${fromImage.duration}`);
  fromImage.close();

  const wrapped = new VideoFrame(canvas, { timestamp: 0, duration: -1 });
  assert(wrapped.duration === 2 ** 64, `modulo duration ${wrapped.duration}`);
  wrapped.close();

  const rgba = new Uint8Array([1, 2, 3, 255]);
  const bufferFrame = new VideoFrame(rgba, {
    format: "RGBA",
    codedWidth: 1.9,
    codedHeight: 1,
    timestamp: -0.9,
    // @ts-expect-error null converts with ToNumber.
    duration: null,
  });
  assert(bufferFrame.codedWidth === 1, `codedWidth ${bufferFrame.codedWidth}`);
  assert(bufferFrame.timestamp === 0, `buffer timestamp ${bufferFrame.timestamp}`);
  assert(bufferFrame.duration === 0, `buffer duration ${bufferFrame.duration}`);
  bufferFrame.close();

  let error: unknown;
  try {
    new VideoFrame(rgba, {
      format: "RGBA",
      codedWidth: 1,
      codedHeight: 1,
      timestamp: NaN,
    });
  } catch (caught) {
    error = caught;
  }
  assertType(error, "The timestamp is out of range", "buffer timestamp");
});

Yuzuko.test("a detached buffer throws TypeError", async () => {
  const decoded = await capture(() => new TextDecoder().decode(detach()));
  assertType(decoded, DETACHED, "TextDecoder");

  const viewBuffer = new ArrayBuffer(4);
  const view = new Uint8Array(viewBuffer);
  structuredClone(view, { transfer: [viewBuffer] });
  const decodedView = await capture(() => new TextDecoder().decode(view));
  assertType(decodedView, DETACHED, "TextDecoder view");

  const digested = await capture(() => crypto.subtle.digest("SHA-256", detach()));
  assertType(digested, DETACHED, "digest");

  const posted = await capture(
    () => new Request("http://127.0.0.1/", { method: "POST", body: detach() }),
  );
  assertType(posted, DETACHED, "Request body");
});

Yuzuko.test("OscillatorNode.start accepts null", () => {
  const context = new OfflineAudioContext(1, 128, 48000);
  const oscillator = new OscillatorNode(context);
  oscillator.connect(context.destination);
  // @ts-expect-error null converts with ToNumber.
  oscillator.start(null);
});

async function adapterOrSkip() {
  const adapter = await navigator.gpu.requestAdapter();
  if (!adapter) {
    console.log("webgpu-skipped");
    return null;
  }
  console.log("webgpu-ran");
  return adapter;
}

Yuzuko.test("WebGPU sizes are EnforceRange", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const usage = GPUBufferUsage.COPY_DST;
  const bytes = new Uint8Array(4);

  let usageError: unknown;
  try {
    device.createBuffer({ size: 4, usage: -1 });
  } catch (error) {
    usageError = error;
  }
  assertType(usageError, OUT_OF_RANGE, "usage");

  let zero: unknown;
  try {
    device.createBuffer({ size: -0.9, usage });
  } catch (error) {
    zero = error;
  }
  assert(zero instanceof RangeError, `zero size ${String(zero)}`);
  assert(zero.message === "GPUBuffer size must be non-zero", zero.message);

  let absent: unknown;
  try {
    device.createBuffer({
      // @ts-expect-error null converts with ToNumber.
      size: null,
      usage,
    });
  } catch (error) {
    absent = error;
  }
  assert(absent instanceof RangeError, `null size ${String(absent)}`);

  const buffer = device.createBuffer({ size: 4.9, usage });
  assert(buffer.size === 4, `size ${buffer.size}`);
  device.queue.writeBuffer(buffer, -0.9, bytes);
  device.queue.writeBuffer(
    buffer,
    0,
    bytes,
    undefined,
    // @ts-expect-error null converts with ToNumber.
    null,
  );

  const offsets: Array<{ offset: number; message: string; name: string }> = [
    { offset: NaN, message: OUT_OF_RANGE, name: "TypeError" },
    { offset: -1, message: OUT_OF_RANGE, name: "TypeError" },
    { offset: 2 ** 53, message: OUT_OF_RANGE, name: "TypeError" },
    {
      offset: Number.MAX_SAFE_INTEGER,
      message: "The buffer offset is out of range",
      name: "OperationError",
    },
  ];
  for (const item of offsets) {
    let error: unknown;
    try {
      device.queue.writeBuffer(buffer, item.offset, bytes);
    } catch (caught) {
      error = caught;
    }
    assert(error instanceof Error, `${item.offset}: ${String(error)}`);
    assert(error.name === item.name, `${item.offset}: ${error.name}`);
    assert(error.message === item.message, `${item.offset}: ${error.message}`);
  }

  buffer.destroy();
  device.destroy();
});
