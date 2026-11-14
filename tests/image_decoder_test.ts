function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

async function bytes(name) {
  const url = new URL(`./fixtures/images/${name}`, import.meta.url);
  return new Uint8Array(await (await fetch(url)).arrayBuffer());
}

async function bitmapPixels(source) {
  const bitmap = await createImageBitmap(source);
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const ctx = canvas.getContext("2d");
  ctx.drawImage(bitmap, 0, 0);
  const data = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data;
  bitmap.close();
  return [...data];
}

Yuzuko.test("isTypeSupported knows the still-image decoders", async () => {
  assert(await ImageDecoder.isTypeSupported("image/png"), "png");
  assert(await ImageDecoder.isTypeSupported("image/jpeg"), "jpeg");
  assert(await ImageDecoder.isTypeSupported("image/webp"), "webp");
  assert(await ImageDecoder.isTypeSupported("image/jxl"), "jxl");
  assert(
    await ImageDecoder.isTypeSupported("image/png;charset=utf-8"),
    "parameters",
  );
  assert(await ImageDecoder.isTypeSupported("image/avif"), "avif");
  assert(await ImageDecoder.isTypeSupported("image/gif"), "gif");
  assert(
    await ImageDecoder.isTypeSupported("image/gif;charset=utf-8"),
    "gif parameters",
  );
  assert(await ImageDecoder.isTypeSupported("image/bmp"), "bmp");
  assert(
    await ImageDecoder.isTypeSupported("image/bmp;charset=utf-8"),
    "bmp parameters",
  );
});

async function rejectionName(value: Promise<unknown>): Promise<string> {
  try {
    await value;
  } catch (error) {
    return error instanceof TypeError ? "TypeError" : String(error);
  }
  return "resolved";
}

Yuzuko.test("ImageDecoder promise members reject a foreign receiver", async () => {
  const completed = ImageDecoder.prototype.completed;
  assert(completed instanceof Promise, "completed is a promise");
  assert(await rejectionName(completed) === "TypeError", "prototype completed");
  const ready = ImageTrackList.prototype.ready;
  assert(ready instanceof Promise, "ready is a promise");
  assert(await rejectionName(ready) === "TypeError", "prototype ready");
  const decode = ImageDecoder.prototype.decode.call({});
  assert(decode instanceof Promise, "foreign decode returns a promise");
  assert(await rejectionName(decode) === "TypeError", "foreign decode");
  assert(ImageDecoder.isTypeSupported.length === 1, "isTypeSupported length");
  const missing = ImageDecoder.isTypeSupported();
  assert(missing instanceof Promise, "missing type returns a promise");
  assert(await rejectionName(missing) === "TypeError", "missing type");

  const encoded = await bytes("corners.png");
  const decoder = new ImageDecoder({ type: "image/png", data: encoded });
  await decoder.completed;
  await decoder.tracks.ready;
  const result = await decoder.decode();
  result.image.close();
  decoder.close();
});

Yuzuko.test("decode a PNG buffer into a VideoFrame", async () => {
  const encoded = await bytes("corners.png");
  const direct = await bitmapPixels(new Blob([encoded], { type: "image/png" }));
  const decoder = new ImageDecoder({ type: "image/png", data: encoded });
  assert(decoder.complete, "buffer is complete");
  assert(decoder.type === "image/png", "type");
  await decoder.completed;
  await decoder.tracks.ready;
  assert(decoder.tracks.length === 1, "one track");
  assert(decoder.tracks.selectedIndex === 0, "selected");
  assert(decoder.tracks[0].animated === false, "still");
  assert(decoder.tracks[0].frameCount === 1, "one frame");
  assert(decoder.tracks[0].repetitionCount === 0, "no loop");
  const result = await decoder.decode();
  assert(result.complete, "frame complete");
  assert(result.image.displayWidth === 2, "width");
  assert(result.image.displayHeight === 2, "height");
  assert(result.image.format === "RGBA", "format");
  const space = result.image.colorSpace;
  assert(space instanceof VideoColorSpace, "color space");
  assert(space.primaries === "bt709", space.primaries);
  assert(space.transfer === "iec61966-2-1", space.transfer);
  assert(space.matrix === "rgb", space.matrix);
  assert(space.fullRange === true, "full range");
  const json = space.toJSON();
  assert(json.primaries === "bt709" && json.fullRange === true, "toJSON");
  assert(result.image.colorSpace !== space, "fresh color space");
  const decoded = await bitmapPixels(result.image);
  assert(decoded.join() === direct.join(), "pixels match createImageBitmap");
  result.image.close();
  assert(result.image.format === null, "closed format");
  assert(result.image.colorSpace.primaries === null, "closed primaries");
  assert(result.image.colorSpace.transfer === null, "closed transfer");
  assert(result.image.colorSpace.matrix === null, "closed matrix");
  assert(result.image.colorSpace.fullRange === null, "closed range");
  assert(result.image.displayWidth === 0, "closed display");
  const again = await decoder.decode();
  assert(again.image.displayWidth === 2, "second frame");
  again.image.close();
  decoder.close();
  let closed = false;
  try {
    await decoder.decode();
  } catch (error) {
    closed = error.name === "InvalidStateError";
  }
  assert(closed, "closed decode");
});

Yuzuko.test("VideoColorSpace keeps the WebCodecs enums", () => {
  const space = new VideoColorSpace({
    primaries: "smpte432",
    transfer: "iec61966-2-1",
    matrix: "rgb",
    fullRange: true,
  });
  assert(space.primaries === "smpte432", space.primaries);
  assert(space.transfer === "iec61966-2-1", space.transfer);
  assert(space.matrix === "rgb", space.matrix);
  assert(space.fullRange === true, "range");
  const empty = new VideoColorSpace();
  assert(empty.primaries === null, "primaries");
  assert(empty.transfer === null, "transfer");
  assert(empty.matrix === null, "matrix");
  assert(empty.fullRange === null, "fullRange");
  assert(empty.toJSON().primaries === null, "json null");
  let invalid = false;
  try {
    new VideoColorSpace({ primaries: "nope" });
  } catch (error) {
    invalid = error instanceof TypeError;
  }
  assert(invalid, "bad primaries");
});

Yuzuko.test("a ReadableStream of JPEG bytes decodes", async () => {
  const encoded = await bytes("red.jpg");
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue(encoded.subarray(0, 10));
      controller.enqueue(encoded.subarray(10));
      controller.close();
    },
  });
  const decoder = new ImageDecoder({ type: "image/jpeg", data: stream });
  assert(decoder.complete === false, "stream is open");
  const result = await decoder.decode();
  assert(decoder.complete, "stream ended");
  assert(result.image.displayWidth > 0, "width");
  assert(result.image.displayHeight > 0, "height");
  result.image.close();
  decoder.close();
});

Yuzuko.test("WebP and JPEG XL decode", async () => {
  for (const [name, type] of [
    ["corners.webp", "image/webp"],
    ["corners.jxl", "image/jxl"],
  ]) {
    const encoded = await bytes(name);
    const decoder = new ImageDecoder({ type, data: encoded });
    const result = await decoder.decode();
    assert(result.image.displayWidth === 2, name);
    result.image.close();
    decoder.close();
  }
});

function pushU16(out, value) {
  out.push(value & 0xff, (value >> 8) & 0xff);
}

function pushU32(out, value) {
  out.push(
    value & 0xff,
    (value >> 8) & 0xff,
    (value >> 16) & 0xff,
    (value >>> 24) & 0xff,
  );
}

Yuzuko.test("a 24-bit bottom-up BMP is red at the origin", async () => {
  const file = [];
  file.push(0x42, 0x4d);
  const header = 14 + 40;
  const stride = 8;
  pushU32(file, header + stride * 2);
  pushU16(file, 0);
  pushU16(file, 0);
  pushU32(file, header);
  pushU32(file, 40);
  pushU32(file, 2);
  pushU32(file, 2);
  pushU16(file, 1);
  pushU16(file, 24);
  pushU32(file, 0);
  pushU32(file, 0);
  pushU32(file, 0);
  pushU32(file, 0);
  pushU32(file, 0);
  pushU32(file, 0);
  // Bottom row is stored first. The origin is the last row: BGR red, then white.
  file.push(255, 0, 0, 0, 255, 0, 0, 0);
  file.push(0, 0, 255, 255, 255, 255, 0, 0);
  const encoded = new Uint8Array(file);
  const decoder = new ImageDecoder({ type: "image/bmp", data: encoded });
  const result = await decoder.decode();
  assert(result.image.displayWidth === 2, "width");
  assert(result.image.displayHeight === 2, "height");
  const pixels = await bitmapPixels(result.image);
  assert(
    pixels[0] === 255 &&
      pixels[1] === 0 &&
      pixels[2] === 0 &&
      pixels[3] === 255,
    `origin ${pixels.slice(0, 4)}`,
  );
  result.image.close();
  decoder.close();
});

Yuzuko.test("AVIF decodes as one still frame", async () => {
  const encoded = await bytes("red.avif");
  const bitmap = await bitmapPixels(
    new Blob([encoded], { type: "image/avif" }),
  );
  const decoder = new ImageDecoder({ type: "image/avif", data: encoded });
  await decoder.completed;
  await decoder.tracks.ready;
  assert(decoder.tracks.length === 1, "one track");
  assert(decoder.tracks[0].animated === false, "still");
  assert(decoder.tracks[0].frameCount === 1, "one frame");
  assert(decoder.tracks[0].repetitionCount === 0, "no loop");
  const result = await decoder.decode();
  assert(result.complete, "complete");
  assert(result.image.displayWidth === 16, "width");
  assert(result.image.displayHeight === 16, "height");
  assert(result.image.format === "I420", result.image.format);
  assert(result.image.timestamp === 0, "timestamp");
  assert(result.image.duration === null, "duration");
  const decoded = await bitmapPixels(result.image);
  const index = (8 * 16 + 8) * 4;
  assert(
    decoded[index] > 180 &&
      decoded[index + 1] < 80 &&
      decoded[index + 2] < 80 &&
      decoded[index + 3] === 255,
    `pixel ${decoded[index]},${decoded[index + 1]},${decoded[index + 2]}`,
  );
  assert(decoded.join() === bitmap.join(), "matches createImageBitmap");
  result.image.close();
  decoder.close();
});

function samePixels(actual, expected, message) {
  assert(actual.length === expected.length, `${message} length`);
  for (let index = 0; index < actual.length; index += 1) {
    if (actual[index] !== expected[index]) {
      assert(false, `${message} at ${index}: ${actual[index]} vs ${expected[index]}`);
    }
  }
}

function corner(pixels, x, y, width) {
  const index = (y * width + x) * 4;
  return pixels.slice(index, index + 4);
}

function assertCorner(pixel, kind, name) {
  const [red, green, blue, alpha] = pixel;
  assert(alpha === 255, `${name} ${kind} alpha ${alpha}`);
  if (kind === "yellow") {
    assert(red >= 250 && green >= 250 && blue <= 8, `${name} yellow ${pixel}`);
  } else if (kind === "red") {
    assert(red >= 250 && green <= 20 && blue <= 8, `${name} red ${pixel}`);
  } else if (kind === "blue") {
    assert(blue >= 240 && red <= 16 && green <= 16, `${name} blue ${pixel}`);
  } else {
    assert(green >= 240 && red <= 16 && blue <= 16, `${name} green ${pixel}`);
  }
}

async function drawnPixels(source) {
  const bitmap = await createImageBitmap(source);
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const context = canvas.getContext("2d");
  assert(context, "context");
  context.drawImage(bitmap, 0, 0);
  const pixels = [...context.getImageData(0, 0, bitmap.width, bitmap.height).data];
  return { bitmap, pixels };
}

Yuzuko.test("AVIF reaches ImageBitmap, drawImage, and createPattern", async () => {
  const root = new URL("./wpt/suite/webcodecs/", import.meta.url);
  const cases = [
    ["four-colors.avif", "I444"],
    ["four-colors-limited-range-420-8bpc.avif", "I420"],
    ["four-colors-limited-range-422-8bpc.avif", "I422"],
    ["four-colors-limited-range-444-8bpc.avif", "I444"],
    ["four-colors-full-range-hlg-420-10bpc.avif", "I420P10"],
    ["four-colors-full-range-hlg-444-12bpc.avif", "I444P12"],
    ["four-colors-full-range-bt2020-pq-444-10bpc.avif", "I444P10"],
  ];
  const adapter = await navigator.gpu.requestAdapter();
  let device = null;
  if (adapter) {
    device = await adapter.requestDevice();
  }
  try {
    for (const [name, format] of cases) {
      const encoded = new Uint8Array(
        await (await fetch(new URL(name, root))).arrayBuffer(),
      );
      const bare = await drawnPixels(new Blob([encoded]));
      const typed = await drawnPixels(
        new Blob([encoded], { type: "image/avif" }),
      );
      assert(bare.bitmap.width === 320 && bare.bitmap.height === 240, name);
      samePixels(typed.pixels, bare.pixels, `${name} mime`);
      assertCorner(corner(bare.pixels, 0, 0, 320), "yellow", name);
      assertCorner(corner(bare.pixels, 319, 0, 320), "red", name);
      assertCorner(corner(bare.pixels, 0, 239, 320), "blue", name);
      assertCorner(corner(bare.pixels, 319, 239, 320), "green", name);

      const patternCanvas = new OffscreenCanvas(1, 1);
      const patternContext = patternCanvas.getContext("2d");
      assert(patternContext, "pattern context");
      const pattern = patternContext.createPattern(bare.bitmap, "repeat");
      assert(pattern !== null, `${name} pattern`);
      patternContext.fillStyle = pattern;
      patternContext.fillRect(0, 0, 1, 1);
      samePixels(
        [...patternContext.getImageData(0, 0, 1, 1).data],
        bare.pixels.slice(0, 4),
        `${name} pattern pixel`,
      );

      const frame = new VideoFrame(bare.bitmap, { timestamp: 0 });
      assert(frame.format === "RGBA", `${name} ${frame.format}`);
      const fromFrame = await drawnPixels(frame);
      samePixels(fromFrame.pixels, bare.pixels, `${name} video frame`);
      fromFrame.bitmap.close();
      frame.close();

      const decoder = new ImageDecoder({ type: "image/avif", data: encoded });
      const result = await decoder.decode();
      assert(result.image.format === format, `${name} ${result.image.format}`);
      const direct = new OffscreenCanvas(320, 240);
      const directContext = direct.getContext("2d");
      assert(directContext, "direct context");
      directContext.drawImage(result.image, 0, 0);
      samePixels(
        [...directContext.getImageData(0, 0, 320, 240).data],
        bare.pixels,
        `${name} drawImage`,
      );
      const framePattern = directContext.createPattern(result.image, "repeat");
      assert(framePattern !== null, `${name} frame pattern`);
      const fromDecoder = await drawnPixels(result.image);
      samePixels(fromDecoder.pixels, bare.pixels, `${name} decoder bitmap`);
      fromDecoder.bitmap.close();

      if (device && name === "four-colors.avif") {
        await assertGpuPixel(device, bare.bitmap, bare.pixels);
        await assertGpuPixel(device, result.image, bare.pixels);
      }

      result.image.close();
      decoder.close();
      bare.bitmap.close();
      typed.bitmap.close();
    }
  } finally {
    device?.destroy();
  }
});

async function assertGpuPixel(device, source, pixels) {
  const texture = device.createTexture({
    size: [1, 1],
    format: "rgba8unorm",
    usage: GPUTextureUsage.COPY_DST | GPUTextureUsage.COPY_SRC,
  });
  device.queue.copyExternalImageToTexture(
    { source },
    { texture, colorSpace: "srgb" },
    [1, 1],
  );
  const buffer = device.createBuffer({
    size: 256,
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });
  const encoder = device.createCommandEncoder();
  encoder.copyTextureToBuffer(
    { texture },
    { buffer, bytesPerRow: 256 },
    [1, 1],
  );
  device.queue.submit([encoder.finish()]);
  await buffer.mapAsync(GPUMapMode.READ);
  const copied = [...new Uint8Array(buffer.getMappedRange()).slice(0, 4)];
  buffer.unmap();
  texture.destroy();
  samePixels(copied, pixels.slice(0, 4), "gpu pixel");
}

Yuzuko.test("bad type, bad index, and a truncated buffer fail", async () => {
  const encoded = await bytes("corners.png");
  const junk = new ImageDecoder({
    type: "junk/type",
    data: encoded,
  });
  const junkCompleted = junk.completed.then(() => null, (error) => error);
  const junkReady = junk.tracks.ready.then(() => null, (error) => error);
  let unsupported = false;
  try {
    await junk.decode();
  } catch (error) {
    unsupported = error.name === "NotSupportedError";
  }
  assert(unsupported, "junk decode");
  const readyError = await junkReady;
  const completedError = await junkCompleted;
  assert(readyError.name === "NotSupportedError", "junk tracks");
  assert(completedError.name === "NotSupportedError", "junk completed");
  junk.close();

  const decoder = new ImageDecoder({ type: "image/png", data: encoded });
  await decoder.tracks.ready;
  let range = false;
  try {
    await decoder.decode({ frameIndex: 1 });
  } catch (error) {
    range = error instanceof RangeError;
  }
  assert(range, "frame index");
  decoder.tracks[0].selected = false;
  assert(decoder.tracks.selectedIndex === -1, "deselected");
  assert(decoder.tracks.selectedTrack === null, "no selected track");
  let none = false;
  try {
    await decoder.decode();
  } catch (error) {
    none = error.name === "InvalidStateError";
  }
  assert(none, "no track");
  decoder.tracks[0].selected = true;
  const selected = await decoder.decode();
  selected.image.close();
  const pending = decoder.decode();
  decoder.reset();
  let aborted = false;
  try {
    await pending;
  } catch (error) {
    aborted = error.name === "AbortError";
  }
  assert(aborted || (await decoder.decode()).image.displayWidth === 2, "reset");
  decoder.close();

  const partial = new ImageDecoder({
    type: "image/png",
    data: encoded.slice(0, 8),
  });
  const partialReady = partial.tracks.ready.then(() => null, (error) => error);
  let encoding = false;
  try {
    await partial.completed;
  } catch (error) {
    encoding = error.name === "EncodingError";
  }
  const partialTracks = await partialReady;
  assert(encoding, "truncated");
  assert(partialTracks.name === "EncodingError", "truncated tracks");
  partial.close();
});

Yuzuko.test("ImageDecoder returns every JPEG XL animation frame", async () => {
  async function open(name) {
    const url = new URL(
      `./wpt/suite/jpegxl/resources/${name}`,
      import.meta.url,
    );
    const encoded = new Uint8Array(await (await fetch(url)).arrayBuffer());
    const decoder = new ImageDecoder({ type: "image/jxl", data: encoded });
    await decoder.tracks.ready;
    return { decoder, encoded };
  }

  const once = await open("animation-1loop.jxl");
  const track = once.decoder.tracks.selectedTrack;
  assert(track.animated === true, "animated");
  assert(track.frameCount === 4, "frames");
  assert(track.repetitionCount === 0, "plays once");
  let total = 0;
  let previous = -1;
  for (let index = 0; index < 4; index += 1) {
    const result = await once.decoder.decode({ frameIndex: index });
    assert(result.image.timestamp > previous, "timestamp increases");
    previous = result.image.timestamp;
    assert(typeof result.image.duration === "number", "duration");
    total += result.image.duration;
    result.image.close();
  }
  assert(total === 2000000, "cycle duration");
  const bitmap = await bitmapPixels(
    new Blob([once.encoded], { type: "image/jxl" }),
  );
  const first = await once.decoder.decode({ frameIndex: 0 });
  const frame = await bitmapPixels(first.image);
  assert(frame.join() === bitmap.join(), "createImageBitmap uses frame 0");
  first.image.close();
  let range = false;
  try {
    await once.decoder.decode({ frameIndex: 4 });
  } catch (error) {
    range = error instanceof RangeError;
  }
  assert(range, "past the last frame");
  once.decoder.close();

  const forever = await open("animation-infiniteloop.jxl");
  assert(
    forever.decoder.tracks.selectedTrack.repetitionCount === Infinity,
    "forever",
  );
  forever.decoder.close();

  const twice = await open("animation-2loops.jxl");
  assert(twice.decoder.tracks.selectedTrack.repetitionCount === 1, "twice");
  twice.decoder.close();
});

Yuzuko.test("a non-detachable transfer throws DataCloneError", () => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  let clone = false;
  try {
    new ImageDecoder({
      type: "image/png",
      data: new Uint8Array(memory.buffer),
      transfer: [memory.buffer],
    });
  } catch (error) {
    clone = error.name === "DataCloneError";
  }
  assert(clone, "cannot detach wasm memory");
});

Yuzuko.test("ImageDecoder desired size scales every frame", async () => {
  const encoded = await bytes("corners.png");
  const wide = new ImageDecoder({
    type: "image/png",
    data: encoded,
    desiredWidth: 4,
  });
  const wideFrame = await wide.decode();
  assert(wideFrame.image.displayWidth === 4, "width");
  assert(wideFrame.image.displayHeight === 4, "height");
  const wideBitmap = await createImageBitmap(wideFrame.image);
  assert(wideBitmap.width === 4 && wideBitmap.height === 4, "bitmap");
  wideBitmap.close();
  wideFrame.image.close();
  wide.close();

  const exact = new ImageDecoder({
    type: "image/png",
    data: encoded,
    desiredWidth: 4,
    desiredHeight: 2,
  });
  const exactFrame = await exact.decode();
  assert(exactFrame.image.displayWidth === 4, "exact width");
  assert(exactFrame.image.displayHeight === 2, "exact height");
  const exactBitmap = await createImageBitmap(exactFrame.image);
  assert(exactBitmap.width === 4 && exactBitmap.height === 2, "exact bitmap");
  exactBitmap.close();
  exactFrame.image.close();
  exact.close();

  // 2×1 PNG. desiredWidth 3 rounds the other edge up from 1.5 to 2.
  const strip = new Uint8Array([
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2,
    0, 0, 0, 1, 8, 2, 0, 0, 0, 123, 64, 232, 221, 0, 0, 0, 13, 73, 68, 65, 84,
    120, 156, 99, 248, 207, 0, 4, 255, 1, 7, 0, 1, 255, 226, 35, 158, 89, 0, 0,
    0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
  ]);
  const aspect = new ImageDecoder({
    type: "image/png",
    data: strip,
    desiredWidth: 3,
  });
  const aspectFrame = await aspect.decode();
  assert(aspectFrame.image.displayWidth === 3, "aspect width");
  assert(aspectFrame.image.displayHeight === 2, "aspect height");
  const aspectBitmap = await createImageBitmap(aspectFrame.image);
  assert(aspectBitmap.width === 3 && aspectBitmap.height === 2, "aspect bitmap");
  aspectBitmap.close();
  aspectFrame.image.close();
  aspect.close();

  let zero = false;
  try {
    new ImageDecoder({
      type: "image/png",
      data: encoded,
      desiredWidth: 0,
    });
  } catch (error) {
    zero = error instanceof TypeError;
  }
  assert(zero, "desiredWidth 0");

  const still = new ImageDecoder({
    type: "image/png",
    data: encoded,
    preferAnimation: true,
  });
  await still.tracks.ready;
  assert(still.tracks[0].animated === false, "one track");
  still.close();
});
