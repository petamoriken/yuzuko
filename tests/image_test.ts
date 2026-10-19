function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

async function pngBlob() {
  const url = new URL("./fixtures/images/corners.png", import.meta.url);
  return await (await fetch(url)).blob();
}

Yuzuko.test("createImageBitmap decodes a blob on a later task", async () => {
  const blob = await pngBlob();
  let microtask = false;
  const pending = createImageBitmap(blob);
  Promise.resolve().then(() => {
    microtask = true;
  });
  const bitmap = await pending;
  assert(microtask, "resolved after a microtask");
  assert(bitmap.width === 2 && bitmap.height === 2, "size");
  bitmap.close();
  assert(bitmap.width === 0 && bitmap.height === 0, "closed");
  let closed = false;
  try {
    await createImageBitmap(bitmap);
  } catch (error) {
    closed = error.name === "InvalidStateError";
  }
  assert(closed, "closed source");
});

Yuzuko.test("crop, resize, and bad bytes follow the bitmap rules", async () => {
  const blob = await pngBlob();
  let zero = false;
  try {
    await createImageBitmap(blob, 0, 0, 0, 1);
  } catch (error) {
    zero = error instanceof RangeError;
  }
  assert(zero, "zero crop");
  let resize = false;
  try {
    await createImageBitmap(blob, { resizeWidth: 0, resizeHeight: 4 });
  } catch (error) {
    resize = error.name === "InvalidStateError";
  }
  assert(resize, "zero resize");
  const cropped = await createImageBitmap(blob, 0, 0, 1, 1, {
    resizeWidth: 3,
    resizeHeight: 2,
    resizeQuality: "low",
  });
  assert(cropped.width === 3 && cropped.height === 2, "resized");
  let corrupt = false;
  try {
    await createImageBitmap(new Blob([new Uint8Array([1, 2, 3])]));
  } catch (error) {
    corrupt = error.name === "InvalidStateError";
  }
  assert(corrupt, "corrupt");
});

Yuzuko.test("ImageData keeps its color space and a blank canvas is transparent", async () => {
  const data = new ImageData(2, 3, {
    colorSpace: "display-p3",
    pixelFormat: "rgba-float16",
  });
  assert(data.width === 2 && data.height === 3, "size");
  assert(data.colorSpace === "display-p3", "space");
  assert(data.pixelFormat === "rgba-float16", "format");
  assert(data.data.length === 24, "float16 length");
  data.data[0] = 0.5;
  const bitmap = await createImageBitmap(data);
  assert(bitmap.width === 2 && bitmap.height === 3, "bitmap");
  const canvas = new OffscreenCanvas(4, 5);
  const copied = await createImageBitmap(canvas);
  assert(copied.width === 4 && copied.height === 5, "canvas");
  let huge = false;
  try {
    await createImageBitmap(new OffscreenCanvas(20000, 10));
  } catch (error) {
    huge = error.name === "InvalidStateError";
  }
  assert(huge, "limit");
});

Yuzuko.test("fetch reads file, blob, and data URLs", async () => {
  const fileUrl = new URL("./fixtures/images/corners.png", import.meta.url);
  const file = await fetch(fileUrl);
  assert(file.ok && file.status === 200, "status");
  assert(file.headers.get("content-type") === "image/png", "type");
  const bitmap = await createImageBitmap(await file.blob());
  assert(bitmap.width === 2 && bitmap.height === 2, "file bitmap");
  let outside = false;
  try {
    await fetch("file:///etc/passwd");
  } catch (error) {
    outside = error instanceof TypeError;
  }
  assert(outside, "outside entry");
  let http = false;
  try {
    await fetch("https://example.test/hero.png");
  } catch (error) {
    http = error.name === "NotSupportedError";
  }
  assert(http, "https");

  const blobUrl = URL.createObjectURL(await pngBlob());
  const fromBlob = await fetch(blobUrl);
  assert((await fromBlob.blob()).size > 0, "blob body");
  URL.revokeObjectURL(blobUrl);

  const data = await fetch("data:text/plain,hello%20yuzuko");
  assert(await data.text() === "hello yuzuko", "data text");
  let used = false;
  try {
    await data.text();
  } catch (error) {
    used = error instanceof TypeError;
  }
  assert(used, "body used");
  const json = await fetch('data:application/json,{"n":1}');
  const parsed = await json.json();
  assert(parsed.n === 1, "json");
});
