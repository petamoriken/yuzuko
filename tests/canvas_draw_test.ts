function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function pixel(ctx, x, y) {
  const data = ctx.getImageData(x, y, 1, 1).data;
  return [data[0], data[1], data[2], data[3]];
}

function pixels(ctx, w, h) {
  return Array.from(ctx.getImageData(0, 0, w, h).data).join(",");
}

function checkerboard() {
  const source = new OffscreenCanvas(4, 4);
  const src = source.getContext("2d");
  for (let y = 0; y < 4; y++) {
    for (let x = 0; x < 4; x++) {
      src.fillStyle = (x + y) % 2 === 0 ? "#ff0000" : "#0000ff";
      src.fillRect(x, y, 1, 1);
    }
  }
  return source;
}

async function scaled(quality, dw, dh) {
  const bitmap = await createImageBitmap(checkerboard());
  const canvas = new OffscreenCanvas(dw, dh);
  const ctx = canvas.getContext("2d");
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = quality;
  ctx.drawImage(bitmap, 0, 0, dw, dh);
  return pixels(ctx, dw, dh);
}

Yuzuko.test("modern colors convert to srgb and paint", () => {
  const ctx = new OffscreenCanvas(2, 2).getContext("2d");
  ctx.fillStyle = "color-mix(in srgb, red, blue)";
  assert(ctx.fillStyle === "color(srgb 0.5 0 0.5)", "color-mix in srgb");
  ctx.fillStyle = "color-mix(in srgb, red, color(srgb 1 0 0))";
  assert(ctx.fillStyle === "color(srgb 1 0 0)", "color-mix with color()");
  ctx.fillStyle = "hwb(0 0% 0%)";
  assert(ctx.fillStyle === "color(srgb 1 0 0)", "hwb red");
  ctx.fillRect(0, 0, 1, 1);
  assert(pixel(ctx, 0, 0).join() === "255,0,0,255", "hwb paints red");

  ctx.fillStyle = "lab(100 0 0)";
  ctx.fillRect(0, 0, 1, 1);
  const white = pixel(ctx, 0, 0);
  assert(white[0] > 250 && white[1] > 250 && white[2] > 250, "lab white");

  ctx.fillStyle = "oklch(0.6 0.2 30)";
  const before = ctx.fillStyle;
  ctx.fillRect(1, 0, 1, 1);
  const mixed = pixel(ctx, 1, 0);
  assert(mixed[0] + mixed[1] + mixed[2] > 0, "oklch paints");
  assert(before.startsWith("color(srgb "), "oklch serializes as srgb");

  ctx.fillStyle = "color(display-p3 1 0 0)";
  assert(ctx.fillStyle === "color(display-p3 1 0 0)", "display-p3 round-trips");
  ctx.fillStyle = "#010203";
  ctx.fillStyle = "color-mix(in xyz, red, blue)";
  assert(ctx.fillStyle === "#010203", "xyz is ignored");

  ctx.filter = "none";
  ctx.filter = "drop-shadow(1px 1px 0px hwb(0 0% 0%))";
  assert(
    ctx.filter === "drop-shadow(1px 1px 0px hwb(0 0% 0%))",
    "a filter color parses",
  );
  ctx.filter = "none";
  ctx.filter = "blur(calc(1px + 1px))";
  assert(ctx.filter === "none", "a filter length calc stays unsupported");
});

Yuzuko.test("imageSmoothingQuality changes a scaled draw", async () => {
  const low = await scaled("low", 16, 16);
  const medium = await scaled("medium", 16, 16);
  const high = await scaled("high", 16, 16);
  assert(low !== medium, "low and medium differ");
  assert(medium !== high, "medium and high differ");
  assert(low !== high, "low and high differ");

  const nativeLow = await scaled("low", 4, 4);
  const nativeMedium = await scaled("medium", 4, 4);
  const nativeHigh = await scaled("high", 4, 4);
  assert(nativeLow === nativeMedium, "1:1 low matches medium");
  assert(nativeLow === nativeHigh, "1:1 low matches high");

  const ctx = new OffscreenCanvas(4, 4).getContext("2d");
  ctx.imageSmoothingQuality = "high";
  assert(ctx.imageSmoothingQuality === "high", "quality round-trips");
  ctx.imageSmoothingQuality = "nope";
  assert(ctx.imageSmoothingQuality === "high", "unknown quality is ignored");
});

Yuzuko.test("measureText reports glyph ink", () => {
  const ctx = new OffscreenCanvas(8, 8).getContext("2d");
  ctx.font = "80px sans-serif";
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  const letter = ctx.measureText("A");
  const space = ctx.measureText(" ");
  const descender = ctx.measureText("g");
  assert(letter.width > 0, "width");
  assert(space.actualBoundingBoxAscent === 0, "a space has no ink ascent");
  assert(space.actualBoundingBoxDescent === 0, "a space has no ink descent");
  assert(letter.actualBoundingBoxAscent > 0, "a letter has ink");
  assert(
    letter.actualBoundingBoxLeft !== 0 ||
      Math.abs(letter.actualBoundingBoxRight - letter.width) > 0.5,
    "ink is not the advance",
  );
  assert(
    descender.actualBoundingBoxDescent > letter.actualBoundingBoxDescent,
    "a descender extends below",
  );
  assert(letter.alphabeticBaseline === 0, "alphabetic baseline");
  assert(letter.hangingBaseline < 0, "hanging is above alphabetic");
  assert(letter.ideographicBaseline > 0, "ideographic is below alphabetic");
  assert(letter.fontBoundingBoxAscent > letter.actualBoundingBoxAscent, "font box");

  ctx.textAlign = "center";
  const centered = ctx.measureText("A");
  assert(
    centered.actualBoundingBoxLeft >
      letter.actualBoundingBoxLeft + letter.width * 0.3,
    "center shifts the ink left of the anchor",
  );

  ctx.textBaseline = "top";
  const top = ctx.measureText("A");
  assert(top.alphabeticBaseline > 0, "alphabetic is below top");
  assert(
    top.fontBoundingBoxAscent < letter.fontBoundingBoxAscent,
    "top baseline reduces font ascent",
  );
  assert(typeof ctx.fillTextCluster === "undefined", "no fillTextCluster");
  assert(typeof TextCluster === "undefined", "no TextCluster");
});

Yuzuko.test("CanvasFilter stays absent", () => {
  const ctx = new OffscreenCanvas(4, 4).getContext("2d");
  assert(typeof CanvasFilter === "undefined", "the class was removed");
  let threw = false;
  try {
    ctx.beginLayer({ filter: { name: "gaussianBlur", stdDeviation: 4 } });
  } catch (error) {
    threw = error.name === "TypeError";
  }
  assert(threw, "a filter dictionary throws");
  let closed = false;
  try {
    ctx.endLayer();
  } catch (error) {
    closed = error.name === "InvalidStateError";
  }
  assert(closed, "the dictionary did not open a layer");
});
