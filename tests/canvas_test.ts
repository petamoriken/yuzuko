function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function pixel(
  ctx: OffscreenCanvasRenderingContext2D,
  x: number,
  y: number,
): [number, number, number, number] {
  const data = ctx.getImageData(x, y, 1, 1).data;
  return [data[0]!, data[1]!, data[2]!, data[3]!];
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
  const ctx = new OffscreenCanvas(6, 6).getContext("2d")!;
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
  const ctx = new OffscreenCanvas(10, 10).getContext("2d")!;
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
  })!;
  const source = new Float16Array([1.5, -0.25, 0.5, 0.75]);
  // @ts-expect-error Float16Array is not an ImageDataArray.
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
    const delta = Math.abs(out.data[i]! - source[i]!);
    assert(delta <= 0.002, `channel ${i} delta ${delta}`);
  }
});

Yuzuko.test("getImageData settings getters can draw on the same context", () => {
  const ctx = new OffscreenCanvas(2, 1).getContext("2d")!;
  const settings = {
    get colorSpace(): PredefinedColorSpace {
      ctx.fillStyle = "rgb(255, 0, 0)";
      ctx.fillRect(0, 0, 1, 1);
      return "srgb";
    },
  };
  const image = ctx.getImageData(0, 0, 2, 1, settings);
  assert(image.data[0] === 255 && image.data[3] === 255, "drawn pixel");
  assert(image.data[7] === 0, "blank pixel");
});

Yuzuko.test("toneMapping is stored and does not change a fill", () => {
  const plain = new OffscreenCanvas(1, 1).getContext("2d")!;
  assert(plain.getContextAttributes().toneMapping.mode === "standard", "default");
  const empty = new OffscreenCanvas(1, 1).getContext("2d", { toneMapping: {} })!;
  assert(empty.getContextAttributes().toneMapping.mode === "standard", "empty");
  const standard = new OffscreenCanvas(1, 1).getContext("2d", {
    toneMapping: { mode: "standard" },
  })!;
  assert(standard.getContextAttributes().toneMapping.mode === "standard", "standard");
  const extended = new OffscreenCanvas(1, 1).getContext("2d", {
    toneMapping: { mode: "extended" },
    colorType: "float16",
  })!;
  assert(extended.getContextAttributes().toneMapping.mode === "extended", "extended");
  extended.fillStyle = "#00ff00";
  extended.fillRect(0, 0, 1, 1);
  assert(pixel(extended, 0, 0).join() === "0,255,0,255", "paint unchanged");
  let bad = false;
  try {
    new OffscreenCanvas(1, 1).getContext("2d", { toneMapping: { mode: "hdr" } });
  } catch (error) {
    bad = error instanceof Error && error.name === "TypeError";
  }
  assert(bad, "invalid mode");
  let object = false;
  try {
    new OffscreenCanvas(1, 1).getContext("2d", { toneMapping: null });
  } catch (error) {
    object = error instanceof Error && error.name === "TypeError";
  }
  assert(object, "null toneMapping");
});

Yuzuko.test("display-p3 path color is stored as sRGB unorm8", () => {
  const ctx = new OffscreenCanvas(4, 4).getContext("2d")!;
  ctx.fillStyle = "color(display-p3 1 0 0)";
  ctx.fillRect(0, 0, 4, 4);
  const [r, g, b, a] = pixel(ctx, 2, 2);
  assert(a === 255, "opaque");
  assert(r > 240 && g < 20 && b < 40, `srgb ${r},${g},${b}`);
});

Yuzuko.test("canvas state stores fonts, composites, and shadows", () => {
  const ctx = new OffscreenCanvas(2, 2).getContext("2d")!;
  ctx.font = "16px Arial";
  assert(ctx.font.includes("Arial"), ctx.font);
  const unloaded = ctx.measureText("A").width;
  ctx.font = "16px sans-serif";
  const generic = ctx.measureText("A").width;
  assert(unloaded === generic, `${unloaded} vs ${generic}`);
  ctx.font = "16px Missing, sans-serif";
  assert(ctx.font.includes("sans-serif"), ctx.font);
  ctx.font = "not a font";
  assert(ctx.font.includes("sans-serif"), "invalid font ignored");

  ctx.globalCompositeOperation = "destination-over";
  assert(ctx.globalCompositeOperation === "destination-over", "composite");
  // @ts-expect-error An unknown composite operation is ignored.
  ctx.globalCompositeOperation = "nope";
  assert(ctx.globalCompositeOperation === "destination-over", "ignored");
  ctx.save();
  ctx.globalCompositeOperation = "xor";
  ctx.shadowBlur = 4;
  ctx.restore();
  let restoredComposite: string = ctx.globalCompositeOperation;
  assert(restoredComposite === "destination-over", "restored");
  assert(ctx.shadowBlur === 0, "shadow restored");

  ctx.shadowBlur = 4;
  assert(ctx.shadowBlur === 4, "blur");
  ctx.shadowBlur = -1;
  assert(ctx.shadowBlur === 4, "negative blur");
  ctx.shadowOffsetX = -2;
  ctx.shadowOffsetY = 3;
  assert(ctx.shadowOffsetX === -2 && ctx.shadowOffsetY === 3, "offset");
  ctx.shadowColor = "lime";
  assert(ctx.shadowColor === "#00ff00", ctx.shadowColor);
  ctx.shadowColor = "bogus";
  assert(ctx.shadowColor === "#00ff00", "color ignored");
  ctx.globalCompositeOperation = "xor";
  ctx.beginLayer();
  let layerComposite: string = ctx.globalCompositeOperation;
  assert(layerComposite === "source-over", "layer composite");
  let layerBlur: number = ctx.shadowBlur;
  let layerColor: string = ctx.shadowColor;
  assert(layerBlur === 0, "layer blur");
  assert(layerColor === "rgba(0, 0, 0, 0)", "layer color");
  ctx.endLayer();
  assert(ctx.globalCompositeOperation === "xor", "layer restored");
  assert(ctx.shadowBlur === 4, "layer blur restored");
  ctx.reset();
  let resetBlur: number = ctx.shadowBlur;
  let resetColor: string = ctx.shadowColor;
  assert(resetBlur === 0, "reset blur");
  assert(resetColor === "rgba(0, 0, 0, 0)", ctx.shadowColor);
  let resetComposite: string = ctx.globalCompositeOperation;
  assert(resetComposite === "source-over", "reset composite");
});

Yuzuko.test("lang is part of the canvas state and shapes text", () => {
  const ctx = new OffscreenCanvas(64, 32).getContext("2d")!;
  assert(ctx.lang === "inherit", "default");
  const unknown = ctx.measureText("骨").width;
  ctx.lang = "ja";
  assert(ctx.lang === "ja", "ja");
  ctx.save();
  ctx.lang = "zh-Hans";
  assert(ctx.lang === "zh-Hans", "hans");
  ctx.restore();
  let restoredLang: string = ctx.lang;
  assert(restoredLang === "ja", "restored");
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
  let resetLang: string = ctx.lang;
  assert(resetLang === "inherit", "reset");
});

Yuzuko.test("text draws and CJK has a width", () => {
  const ctx = new OffscreenCanvas(64, 32).getContext("2d")!;
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
    if (sample[i]! > 0) {
      ink = true;
      break;
    }
  }
  assert(ink, "glyph ink");
});

Yuzuko.test("drawImage copies an ImageBitmap", async () => {
  const source = new OffscreenCanvas(2, 2);
  const src = source.getContext("2d")!;
  src.fillStyle = "#ff00ff";
  src.fillRect(0, 0, 2, 2);
  const bitmap = await createImageBitmap(source);
  const ctx = new OffscreenCanvas(4, 4).getContext("2d")!;
  ctx.drawImage(bitmap, 1, 1);
  assert(pixel(ctx, 1, 1).join() === "255,0,255,255", "copied");
  assert(pixel(ctx, 0, 0).join() === "0,0,0,0", "untouched");
});

Yuzuko.test("convertToBlob encodes png, jpeg, and webp", async () => {
  const canvas = new OffscreenCanvas(2, 2);
  const ctx = canvas.getContext("2d")!;
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

Yuzuko.test("a 2d context is not lost", () => {
  const canvas = new OffscreenCanvas(4, 4);
  assert(canvas.oncontextlost === null, "lost handler");
  assert(canvas.oncontextrestored === null, "restored handler");
  let lost = 0;
  let restored = 0;
  canvas.oncontextlost = () => {
    lost += 1;
  };
  canvas.oncontextrestored = () => {
    restored += 1;
  };
  assert(typeof canvas.oncontextlost === "function", "lost assigned");
  assert(typeof canvas.oncontextrestored === "function", "restored assigned");
  const ctx = canvas.getContext("2d");
  assert(ctx !== null, "context");
  assert(ctx.isContextLost() === false, "not lost");
  ctx.fillRect(0, 0, 4, 4);
  assert(lost === 0 && restored === 0, "no context loss event");
});

Yuzuko.test("bitmaprenderer transfers and detaches", async () => {
  const painted = new OffscreenCanvas(3, 1);
  const pixels = painted.getContext("2d")!;
  pixels.fillStyle = "#00ff00";
  pixels.fillRect(0, 0, 3, 1);
  const bitmap = await createImageBitmap(painted);
  const canvas = new OffscreenCanvas(1, 1);
  const ctx = canvas.getContext("bitmaprenderer")!;
  ctx.transferFromImageBitmap(bitmap);
  assert(bitmap.width === 0, "detached");
  assert(canvas.width === 3 && canvas.height === 1, "resized");
  const copy = await createImageBitmap(canvas);
  const view = new OffscreenCanvas(3, 1).getContext("2d")!;
  view.drawImage(copy, 0, 0);
  assert(pixel(view, 1, 0).join() === "0,255,0,255", "transferred");
});

Yuzuko.test("canvasRenderer is cpu or gpu", () => {
  assert(
    Yuzuko.canvasRenderer === "cpu" || Yuzuko.canvasRenderer === "gpu",
    Yuzuko.canvasRenderer,
  );
});

Yuzuko.test("CSS filters change composited pixels", () => {
  const ctx = new OffscreenCanvas(4, 2).getContext("2d")!;
  ctx.fillStyle = "#ff0000";
  ctx.filter = "brightness(0)";
  ctx.fillRect(0, 0, 4, 2);
  assert(pixel(ctx, 1, 0).join() === "0,0,0,255", "brightness");

  ctx.filter = "invert(1)";
  ctx.fillRect(0, 0, 4, 2);
  assert(pixel(ctx, 1, 0).join() === "0,255,255,255", `invert ${pixel(ctx, 1, 0).join()}`);

  ctx.filter = "none";
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 4, 2);
  ctx.filter = "opacity(0)";
  ctx.fillStyle = "#00ff00";
  ctx.fillRect(0, 0, 1, 1);
  assert(pixel(ctx, 0, 0).join() === "255,0,0,255", "opacity leaves destination");

  ctx.filter = "grayscale(1)";
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 4, 2);
  const gray = pixel(ctx, 1, 1);
  assert(gray[0] === gray[1] && gray[1] === gray[2], "gray channels");
  assert(Math.abs(gray[0] - 54) <= 1, `gray luma ${gray.join()}`);
  assert(gray[3] === 255, "gray alpha");

  ctx.filter = "brightness(0) invert(1)";
  ctx.fillRect(0, 0, 4, 2);
  assert(pixel(ctx, 0, 0).join() === "255,255,255,255", "filter chain");

  ctx.filter = "url(#missing)";
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 1, 1);
  assert(pixel(ctx, 0, 0).join() === "255,255,255,255", "url draws nothing");

  const blur = new OffscreenCanvas(3, 1).getContext("2d")!;
  blur.filter = "blur(1px)";
  blur.fillStyle = "#ff0000";
  blur.fillRect(1, 0, 1, 1);
  assert(pixel(blur, 1, 0)[3] > 0, "blur keeps coverage");
  assert(pixel(blur, 0, 0)[3] > 0, "blur spreads");

  const shadow = new OffscreenCanvas(6, 3).getContext("2d")!;
  shadow.filter = "drop-shadow(2px 0 0 #00ff00)";
  shadow.fillStyle = "#ff0000";
  shadow.fillRect(0, 0, 2, 3);
  assert(pixel(shadow, 0, 1).join() === "255,0,0,255", "shadow source");
  assert(pixel(shadow, 3, 1).join() === "0,255,0,255", "shadow offset");

  const layer = new OffscreenCanvas(2, 2).getContext("2d")!;
  layer.beginLayer({ filter: "invert(1)" });
  layer.fillStyle = "#ff0000";
  layer.fillRect(0, 0, 2, 2);
  layer.endLayer();
  assert(pixel(layer, 0, 0).join() === "0,255,255,255", "layer filter");
});

Yuzuko.test("a canvas past the bitmap limit has no pixels", async () => {
  for (const [width, height, colorType] of [
    [65535, 1, "unorm8"],
    [20000, 1, "unorm8"],
    [20000, 1, "float16"],
    [12000, 12000, "unorm8"],
  ] as const) {
    const canvas = new OffscreenCanvas(width, height);
    const ctx = canvas.getContext("2d", { colorType });
    assert(ctx !== null, "context");
    ctx.globalCompositeOperation = "copy";
    ctx.fillRect(0, 0, 4, 1);
    let name = "";
    try {
      ctx.getImageData(0, 0, 1, 1);
    } catch (error) {
      name = (error as DOMException).name;
    }
    assert(name === "InvalidStateError", `${width} getImageData ${name}`);
    name = "";
    try {
      await canvas.convertToBlob();
    } catch (error) {
      name = (error as DOMException).name;
    }
    assert(name === "InvalidStateError", `${width} convertToBlob ${name}`);
  }
  const small = new OffscreenCanvas(2, 2).getContext("2d");
  assert(small !== null, "small context");
  small.fillRect(0, 0, 2, 2);
  assert(pixel(small, 1, 1).join() === "0,0,0,255", "a small canvas still paints");
});

Yuzuko.test("a throwing element getter propagates from DOMMatrix and setLineDash", () => {
  const thrown = new Error("element");
  const matrix = [1, 0, 0, 1, 0, 0];
  Object.defineProperty(matrix, 0, {
    get() {
      throw thrown;
    },
  });
  let caught: unknown;
  try {
    new DOMMatrix(matrix);
  } catch (error) {
    caught = error;
  }
  assert(caught === thrown, "DOMMatrix rethrows");
  const dash = [1, 2];
  Object.defineProperty(dash, 1, {
    get() {
      throw thrown;
    },
  });
  caught = undefined;
  const ctx = new OffscreenCanvas(1, 1).getContext("2d");
  assert(ctx !== null, "context");
  try {
    ctx.setLineDash(dash);
  } catch (error) {
    caught = error;
  }
  assert(caught === thrown, "setLineDash rethrows");
  const long: number[] = [];
  long.length = 2 ** 32 - 1;
  caught = undefined;
  try {
    new DOMMatrix(long);
  } catch (error) {
    caught = error;
  }
  assert(caught instanceof TypeError, "a long sequence is rejected");
});

Yuzuko.test("putImageData at an offset past the i32 range draws nothing", () => {
  for (const colorType of ["unorm8", "float16"] as const) {
    const canvas = new OffscreenCanvas(4, 4);
    const ctx = canvas.getContext("2d", { colorType });
    assert(ctx !== null, "context");
    const data = new ImageData(new Uint8ClampedArray(64).fill(255), 4, 4);
    for (const offset of [2 ** 31, -(2 ** 31), 2 ** 40, -(2 ** 40)]) {
      ctx.putImageData(data, offset, offset);
    }
    ctx.fillStyle = "#00ff00";
    ctx.fillRect(1, 1, 1, 1);
    assert(pixel(ctx, 0, 0).join() === "0,0,0,0", `${colorType} untouched`);
    assert(pixel(ctx, 1, 1).join() === "0,255,0,255", `${colorType} painted`);
  }
});

Yuzuko.test("a huge arc radius builds a bounded path", () => {
  const started = performance.now();
  const path = new Path2D();
  path.arc(0, 0, 1e300, 0, 6);
  path.arcTo(1, 0, 1, 1, 1e300);
  path.roundRect(0, 0, 1e300, 1e300, [1e300]);
  const ctx = new OffscreenCanvas(8, 8).getContext("2d")!;
  ctx.ellipse(0, 0, 1e-300, 1e300, 0, 0, 1e10);
  ctx.arc(4, 4, 1e40, 0, 6);
  ctx.fill();
  ctx.stroke(path);
  ctx.getImageData(0, 0, 1, 1);
  const elapsed = performance.now() - started;
  if (elapsed > 10_000) {
    throw new Error(`took ${elapsed} ms`);
  }
});
