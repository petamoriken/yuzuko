function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function pixel(ctx, x, y) {
  const data = ctx.getImageData(x, y, 1, 1).data;
  return [data[0], data[1], data[2], data[3]];
}

Yuzuko.test("fillRect paints an exact sRGB pixel", () => {
  const canvas = new OffscreenCanvas(8, 4);
  const ctx = canvas.getContext("2d");
  assert(ctx !== null, "context");
  assert(canvas.getContext("2d") === ctx, "cached");
  assert(canvas.getContext("webgpu") === null, "webgpu later");
  assert(canvas.getContext("bitmaprenderer") === null, "different kind");
  ctx.fillStyle = "#00ff00";
  ctx.fillRect(0, 0, 8, 4);
  assert(pixel(ctx, 3, 1).join() === "0,255,0,255", "green");
  ctx.clearRect(0, 0, 8, 4);
  assert(pixel(ctx, 3, 1).join() === "0,0,0,0", "cleared");
});

Yuzuko.test("save, restore, and translate affect later draws", () => {
  const ctx = new OffscreenCanvas(6, 6).getContext("2d");
  ctx.fillStyle = "#ff0000";
  ctx.save();
  ctx.translate(2, 2);
  ctx.fillRect(0, 0, 2, 2);
  ctx.restore();
  ctx.fillStyle = "#0000ff";
  ctx.fillRect(0, 0, 1, 1);
  assert(pixel(ctx, 0, 0).join() === "0,0,255,255", "restored style");
  assert(pixel(ctx, 3, 3).join() === "255,0,0,255", "translated");
  assert(pixel(ctx, 1, 1).join() === "0,0,0,0", "outside");
});

Yuzuko.test("a path fill and even-odd hole", () => {
  const ctx = new OffscreenCanvas(10, 10).getContext("2d");
  ctx.fillStyle = "rgb(0, 0, 255)";
  ctx.beginPath();
  ctx.rect(1, 1, 8, 8);
  ctx.rect(3, 3, 4, 4);
  ctx.fill("evenodd");
  assert(pixel(ctx, 2, 2).join() === "0,0,255,255", "outer");
  assert(pixel(ctx, 5, 5).join() === "0,0,0,0", "hole");
  assert(ctx.isPointInPath(2, 2, "evenodd") === true, "hit");
  assert(ctx.isPointInPath(5, 5, "evenodd") === false, "miss");
});

Yuzuko.test("putImageData round-trips float16 Display P3", () => {
  const canvas = new OffscreenCanvas(1, 1);
  const ctx = canvas.getContext("2d", {
    colorSpace: "display-p3",
    colorType: "float16",
  });
  const source = new Float16Array([1.5, -0.25, 0.5, 0.75]);
  const data = new ImageData(source, 1, 1, {
    colorSpace: "display-p3",
    pixelFormat: "rgba-float16",
  });
  ctx.putImageData(data, 0, 0);
  const out = ctx.getImageData(0, 0, 1, 1, {
    colorSpace: "display-p3",
    pixelFormat: "rgba-float16",
  });
  assert(out.colorSpace === "display-p3", "space");
  assert(out.pixelFormat === "rgba-float16", "format");
  for (let i = 0; i < 4; i++) {
    const delta = Math.abs(out.data[i] - source[i]);
    assert(delta <= 0.002, `channel ${i} delta ${delta}`);
  }
});

Yuzuko.test("display-p3 path color is stored as sRGB unorm8", () => {
  const ctx = new OffscreenCanvas(4, 4).getContext("2d");
  ctx.fillStyle = "color(display-p3 1 0 0)";
  ctx.fillRect(0, 0, 4, 4);
  const [r, g, b, a] = pixel(ctx, 2, 2);
  assert(a === 255, "opaque");
  assert(r > 240 && g < 20 && b < 40, `srgb ${r},${g},${b}`);
});

Yuzuko.test("named fonts and other composites are rejected", () => {
  const ctx = new OffscreenCanvas(2, 2).getContext("2d");
  let named = false;
  try {
    ctx.font = "16px Arial";
  } catch (error) {
    named = error.name === "NotSupportedError";
  }
  assert(named, "named font");
  assert(ctx.font.includes("sans-serif"), "font unchanged");
  let composite = false;
  try {
    ctx.globalCompositeOperation = "destination-over";
  } catch (error) {
    composite = error.name === "NotSupportedError";
  }
  assert(composite, "composite");
  let shadow = false;
  try {
    ctx.shadowBlur = 4;
  } catch (error) {
    shadow = error.name === "NotSupportedError";
  }
  assert(shadow, "shadow");
  assert(ctx.shadowBlur === 0, "shadow unchanged");
});

Yuzuko.test("lang is part of the canvas state and shapes text", () => {
  const ctx = new OffscreenCanvas(64, 32).getContext("2d");
  assert(ctx.lang === "inherit", "default");
  const unknown = ctx.measureText("骨").width;
  ctx.lang = "ja";
  assert(ctx.lang === "ja", "ja");
  ctx.save();
  ctx.lang = "zh-Hans";
  assert(ctx.lang === "zh-Hans", "hans");
  ctx.restore();
  assert(ctx.lang === "ja", "restored");
  ctx.lang = "";
  assert(ctx.lang === "", "empty");
  assert(ctx.measureText("骨").width === unknown, "empty is unknown");
  ctx.lang = "ja";
  const ja = ctx.measureText("骨").width;
  ctx.lang = "zh-Hans";
  const hans = ctx.measureText("骨").width;
  assert(ja > 0 && hans > 0, "localized widths");
  ctx.lang = "not a tag";
  assert(ctx.lang === "not a tag", "stored");
  assert(ctx.measureText("骨").width === unknown, "invalid is unknown");
  ctx.reset();
  assert(ctx.lang === "inherit", "reset");
});

Yuzuko.test("text draws and CJK has a width", () => {
  const ctx = new OffscreenCanvas(64, 32).getContext("2d");
  ctx.font = "20px sans-serif";
  const latin = ctx.measureText("A").width;
  const cjk = ctx.measureText("字").width;
  assert(latin > 0, "latin width");
  assert(cjk > 0, "cjk width");
  ctx.fillStyle = "#000000";
  ctx.fillText("A", 4, 24);
  const sample = ctx.getImageData(0, 0, 64, 32).data;
  let ink = false;
  for (let i = 3; i < sample.length; i += 4) {
    if (sample[i] > 0) {
      ink = true;
      break;
    }
  }
  assert(ink, "glyph ink");
});

Yuzuko.test("drawImage copies an ImageBitmap", async () => {
  const source = new OffscreenCanvas(2, 2);
  const src = source.getContext("2d");
  src.fillStyle = "#ff00ff";
  src.fillRect(0, 0, 2, 2);
  const bitmap = await createImageBitmap(source);
  const ctx = new OffscreenCanvas(4, 4).getContext("2d");
  ctx.drawImage(bitmap, 1, 1);
  assert(pixel(ctx, 1, 1).join() === "255,0,255,255", "copied");
  assert(pixel(ctx, 0, 0).join() === "0,0,0,0", "untouched");
});

Yuzuko.test("convertToBlob encodes png, jpeg, and webp", async () => {
  const canvas = new OffscreenCanvas(2, 2);
  const ctx = canvas.getContext("2d");
  ctx.fillStyle = "#112233";
  ctx.fillRect(0, 0, 2, 2);
  const png = new Uint8Array(await (await canvas.convertToBlob()).arrayBuffer());
  assert(png[0] === 0x89 && png[1] === 0x50, "png");
  const jpeg = new Uint8Array(
    await (await canvas.convertToBlob({ type: "image/jpeg", quality: 0.8 })).arrayBuffer(),
  );
  assert(jpeg[0] === 0xff && jpeg[1] === 0xd8, "jpeg");
  const webp = new Uint8Array(
    await (await canvas.convertToBlob({ type: "image/webp" })).arrayBuffer(),
  );
  assert(
    webp[0] === 0x52 && webp[1] === 0x49 && webp[2] === 0x46 && webp[3] === 0x46,
    "webp",
  );
});

Yuzuko.test("bitmaprenderer transfers and detaches", async () => {
  const painted = new OffscreenCanvas(3, 1);
  const pixels = painted.getContext("2d");
  pixels.fillStyle = "#00ff00";
  pixels.fillRect(0, 0, 3, 1);
  const bitmap = await createImageBitmap(painted);
  const canvas = new OffscreenCanvas(1, 1);
  const ctx = canvas.getContext("bitmaprenderer");
  ctx.transferFromImageBitmap(bitmap);
  assert(bitmap.width === 0, "detached");
  assert(canvas.width === 3 && canvas.height === 1, "resized");
  const copy = await createImageBitmap(canvas);
  const view = new OffscreenCanvas(3, 1).getContext("2d");
  view.drawImage(copy, 0, 0);
  assert(pixel(view, 1, 0).join() === "0,255,0,255", "transferred");
});

Yuzuko.test("canvasRenderer is cpu or gpu", () => {
  assert(
    Yuzuko.canvasRenderer === "cpu" || Yuzuko.canvasRenderer === "gpu",
    Yuzuko.canvasRenderer,
  );
});
