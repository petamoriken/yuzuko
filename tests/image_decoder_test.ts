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
  assert(!(await ImageDecoder.isTypeSupported("image/avif")), "avif");
  assert(!(await ImageDecoder.isTypeSupported("image/gif")), "gif");
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
  const decoded = await bitmapPixels(result.image);
  assert(decoded.join() === direct.join(), "pixels match createImageBitmap");
  result.image.close();
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
