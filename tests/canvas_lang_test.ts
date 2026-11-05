function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

// Noto Sans CJK SC subset of U+9AA8 (骨), the same face as
// https://github.com/linebender/parley/pull/756 (issue 748). OpenType `locl`
// keeps three outlines for these tags: zh and zh-Hans stay on the default
// simplified glyph, zh-Hant and zh-TW share the traditional glyph, and zh-HK
// uses the Hong Kong glyph. Each character is drawn at the same x so identical
// glyphs produce identical pixels.
const WIDTH = 64;
const ROW = 48;
const LANGS = ["zh", "zh-Hans", "zh-Hant", "zh-TW", "zh-HK"] as const;

function rowBytes(data: Uint8ClampedArray, index: number): Uint8ClampedArray {
  const stride = ROW * WIDTH * 4;
  return data.subarray(index * stride, (index + 1) * stride);
}

function samePixels(actual: Uint8ClampedArray, expected: Uint8ClampedArray): boolean {
  if (actual.length !== expected.length) {
    return false;
  }
  for (let index = 0; index < actual.length; index += 1) {
    if (actual[index] !== expected[index]) {
      return false;
    }
  }
  return true;
}

function assertSame(
  actual: Uint8ClampedArray,
  expected: Uint8ClampedArray,
  label: string,
): void {
  assert(
    actual.length === expected.length,
    `${label} length ${actual.length} vs ${expected.length}`,
  );
  for (let index = 0; index < actual.length; index += 4) {
    if (
      actual[index] !== expected[index] ||
      actual[index + 1] !== expected[index + 1] ||
      actual[index + 2] !== expected[index + 2] ||
      actual[index + 3] !== expected[index + 3]
    ) {
      const pixel = index / 4;
      const x = pixel % WIDTH;
      const y = Math.floor(pixel / WIDTH);
      throw new Error(
        `${label} at ${x},${y}: ${actual[index]},${actual[index + 1]},${actual[index + 2]},${actual[index + 3]} vs ${expected[index]},${expected[index + 1]},${expected[index + 2]},${expected[index + 3]}`,
      );
    }
  }
}

function assertInk(data: Uint8ClampedArray, label: string): void {
  for (let index = 0; index < data.length; index += 4) {
    if (
      data[index] !== 255 ||
      data[index + 1] !== 255 ||
      data[index + 2] !== 255 ||
      data[index + 3] !== 255
    ) {
      return;
    }
  }
  throw new Error(`${label} has no ink`);
}

async function loadSubset(): Promise<void> {
  const bytes = await (
    await fetch(new URL("./fonts/NotoSansCJKsc-Regular-subset.otf", import.meta.url))
  ).arrayBuffer();
  const face = new FontFace("NotoSansCJKsc", bytes);
  const loaded = await face.load();
  assert(loaded.status === "loaded", loaded.status);
  Yuzuko.fonts.add(face);
}

function draw(): OffscreenCanvasRenderingContext2D {
  const canvas = new OffscreenCanvas(WIDTH, ROW * LANGS.length);
  const context = canvas.getContext("2d");
  assert(context, "2d");
  context.font = "32px NotoSansCJKsc";
  context.textBaseline = "alphabetic";
  context.fillStyle = "#ffffff";
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.fillStyle = "#000000";
  for (let index = 0; index < LANGS.length; index += 1) {
    context.lang = LANGS[index];
    context.fillText("骨", 8, index * ROW + 36);
  }
  return context;
}

async function goldenPixels(): Promise<Uint8ClampedArray> {
  const blob = await (
    await fetch(new URL("./fixtures/canvas/noto-cjk-lang.png", import.meta.url))
  ).blob();
  const bitmap = await createImageBitmap(blob);
  assert(bitmap.width === WIDTH && bitmap.height === ROW * LANGS.length, "golden size");
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const context = canvas.getContext("2d");
  assert(context, "golden 2d");
  context.imageSmoothingEnabled = false;
  context.drawImage(bitmap, 0, 0);
  const data = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
  bitmap.close();
  return data;
}

Yuzuko.test("OffscreenCanvas lang selects Noto Sans CJK locl glyphs", async () => {
  assert(Yuzuko.canvasRenderer === "cpu", Yuzuko.canvasRenderer);
  await loadSubset();
  const drawn = draw().getImageData(0, 0, WIDTH, ROW * LANGS.length).data;
  for (let index = 0; index < LANGS.length; index += 1) {
    assertInk(rowBytes(drawn, index), LANGS[index]);
  }
  assertSame(rowBytes(drawn, 0), rowBytes(drawn, 1), "zh and zh-Hans");
  assertSame(rowBytes(drawn, 2), rowBytes(drawn, 3), "zh-Hant and zh-TW");
  assert(!samePixels(rowBytes(drawn, 0), rowBytes(drawn, 2)), "zh matches zh-Hant");
  assert(!samePixels(rowBytes(drawn, 0), rowBytes(drawn, 4)), "zh matches zh-HK");
  assert(!samePixels(rowBytes(drawn, 2), rowBytes(drawn, 4)), "zh-Hant matches zh-HK");
  assertSame(drawn, await goldenPixels(), "noto-cjk-lang.png");
});
