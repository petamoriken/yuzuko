function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function pixel(ctx, x, y) {
  const data = ctx.getImageData(x, y, 1, 1).data;
  return [data[0], data[1], data[2], data[3]];
}

function same(actual, expected, message) {
  assert(
    actual.join() === expected.join(),
    `${message}: ${actual} vs ${expected}`,
  );
}

function context(width, height) {
  const ctx = new OffscreenCanvas(width, height).getContext("2d");
  assert(ctx !== null, "context");
  return ctx;
}

Yuzuko.test("source-in clears pixels the source misses", () => {
  const ctx = context(8, 4);
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 8, 4);
  ctx.globalCompositeOperation = "source-in";
  ctx.fillStyle = "#0000ff";
  ctx.fillRect(0, 0, 4, 4);
  same(pixel(ctx, 1, 1), [0, 0, 255, 255], "overlap");
  same(pixel(ctx, 6, 1), [0, 0, 0, 0], "uncovered");
});

Yuzuko.test("destination-over keeps an opaque destination", () => {
  const ctx = context(8, 4);
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(2, 0, 4, 4);
  ctx.globalCompositeOperation = "destination-over";
  ctx.fillStyle = "#0000ff";
  ctx.fillRect(0, 0, 8, 4);
  same(pixel(ctx, 0, 1), [0, 0, 255, 255], "corner");
  same(pixel(ctx, 3, 1), [255, 0, 0, 255], "kept");
});

Yuzuko.test("destination-in keeps only the overlap", () => {
  const ctx = context(8, 4);
  ctx.fillStyle = "#0000ff";
  ctx.fillRect(0, 0, 8, 4);
  ctx.globalCompositeOperation = "destination-in";
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 4, 4);
  same(pixel(ctx, 1, 1), [0, 0, 255, 255], "overlap");
  same(pixel(ctx, 6, 1), [0, 0, 0, 0], "cleared");
});

Yuzuko.test("copy replaces the shape and clears the rest", () => {
  const ctx = context(8, 4);
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 8, 4);
  ctx.globalCompositeOperation = "copy";
  ctx.fillStyle = "#0000ff";
  ctx.fillRect(0, 0, 2, 4);
  same(pixel(ctx, 0, 1), [0, 0, 255, 255], "copied");
  same(pixel(ctx, 4, 1), [0, 0, 0, 0], "cleared");
});

Yuzuko.test("lighter adds premul colors and saturates", () => {
  const ctx = context(4, 4);
  ctx.fillStyle = "#00ffff";
  ctx.fillRect(0, 0, 4, 4);
  ctx.globalCompositeOperation = "lighter";
  ctx.fillStyle = "#ffff00";
  ctx.fillRect(0, 0, 4, 4);
  same(pixel(ctx, 1, 1), [255, 255, 255, 255], "white");
});

Yuzuko.test("multiply blends the source over the destination", () => {
  const ctx = context(4, 4);
  ctx.fillStyle = "#ffff00";
  ctx.fillRect(0, 0, 4, 4);
  ctx.globalCompositeOperation = "multiply";
  ctx.fillStyle = "#0000ff";
  ctx.fillRect(0, 0, 4, 4);
  same(pixel(ctx, 1, 1), [0, 0, 0, 255], "black");
});

Yuzuko.test("xor clears the overlap and keeps the rest", () => {
  const ctx = context(8, 4);
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 8, 4);
  ctx.globalCompositeOperation = "xor";
  ctx.fillStyle = "#0000ff";
  ctx.fillRect(0, 0, 4, 4);
  same(pixel(ctx, 1, 1), [0, 0, 0, 0], "overlap");
  same(pixel(ctx, 6, 1), [255, 0, 0, 255], "rest");
});

Yuzuko.test("an offset shadow is black beside the shape", () => {
  const ctx = context(8, 4);
  ctx.shadowColor = "#000000";
  ctx.shadowOffsetX = 4;
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 2, 4);
  same(pixel(ctx, 1, 1), [255, 0, 0, 255], "shape");
  same(pixel(ctx, 5, 1), [0, 0, 0, 255], "shadow");
  same(pixel(ctx, 7, 1), [0, 0, 0, 0], "outside");
});

Yuzuko.test("a shadow can fall inside a clip the shape misses", () => {
  const ctx = context(8, 4);
  ctx.fillStyle = "#00ff00";
  ctx.fillRect(0, 0, 8, 4);
  ctx.beginPath();
  ctx.rect(4, 0, 4, 4);
  ctx.clip();
  ctx.shadowColor = "#0000ff";
  ctx.shadowOffsetX = 4;
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 4, 4);
  same(pixel(ctx, 1, 1), [0, 255, 0, 255], "outside clip");
  same(pixel(ctx, 6, 1), [0, 0, 255, 255], "shadow in clip");
});

Yuzuko.test("shadowColor alone does not draw", () => {
  const ctx = context(4, 4);
  ctx.shadowColor = "#ff0000";
  ctx.globalAlpha = 0.5;
  ctx.fillStyle = "#00ff00";
  ctx.fillRect(0, 0, 4, 4);
  const sample = pixel(ctx, 1, 1);
  assert(sample[0] < 8, `red ${sample}`);
  assert(sample[1] > 240, `green ${sample}`);
  assert(sample[3] > 110 && sample[3] < 145, `alpha ${sample}`);
});

Yuzuko.test("shadowBlur spreads outside the shape", () => {
  const ctx = context(24, 24);
  ctx.shadowColor = "#0000ff";
  ctx.shadowBlur = 4;
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(10, 10, 4, 4);
  const edge = pixel(ctx, 9, 12);
  assert(edge[2] > 0 && edge[3] > 0 && edge[0] < 20, `edge ${edge}`);
  same(pixel(ctx, 0, 12), [0, 0, 0, 0], "far");
  same(pixel(ctx, 12, 12), [255, 0, 0, 255], "shape");
});

Yuzuko.test("clearRect ignores the operator and the shadow", () => {
  const ctx = context(8, 4);
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 8, 4);
  ctx.globalCompositeOperation = "copy";
  ctx.shadowColor = "#000000";
  ctx.shadowOffsetX = 3;
  ctx.shadowBlur = 2;
  ctx.clearRect(0, 0, 2, 4);
  same(pixel(ctx, 0, 1), [0, 0, 0, 0], "cleared");
  same(pixel(ctx, 5, 1), [255, 0, 0, 255], "rest");
});

Yuzuko.test("putImageData ignores source-in and the shadow", () => {
  const ctx = context(8, 4);
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 8, 4);
  ctx.globalCompositeOperation = "source-in";
  ctx.shadowColor = "#000000";
  ctx.shadowOffsetX = 4;
  const blue = new ImageData(new Uint8ClampedArray([0, 0, 255, 255]), 1, 1);
  ctx.putImageData(blue, 1, 1);
  same(pixel(ctx, 1, 1), [0, 0, 255, 255], "written");
  same(pixel(ctx, 6, 1), [255, 0, 0, 255], "rest");
});

Yuzuko.test("globalAlpha scales the shape and its shadow", () => {
  const ctx = context(12, 4);
  ctx.shadowColor = "#000000";
  ctx.shadowOffsetX = 6;
  ctx.globalAlpha = 0.5;
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 4, 4);
  const shape = pixel(ctx, 1, 1);
  const shadow = pixel(ctx, 8, 1);
  assert(shape[0] > 240 && shape[1] < 8 && shape[2] < 8, `shape ${shape}`);
  assert(shape[3] > 110 && shape[3] < 145, `shape alpha ${shape}`);
  assert(shadow[0] < 8 && shadow[1] < 8 && shadow[2] < 8, `shadow ${shadow}`);
  assert(shadow[3] > 110 && shadow[3] < 145, `shadow alpha ${shadow}`);
  same(pixel(ctx, 11, 1), [0, 0, 0, 0], "outside");
});

Yuzuko.test("scale does not scale shadowOffsetX", () => {
  const ctx = context(10, 6);
  ctx.scale(2, 2);
  ctx.shadowColor = "#000000";
  ctx.shadowOffsetX = 2;
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 2, 2);
  same(pixel(ctx, 1, 2), [255, 0, 0, 255], "shape");
  same(pixel(ctx, 5, 2), [0, 0, 0, 255], "unscaled shadow");
  same(pixel(ctx, 7, 2), [0, 0, 0, 0], "past the shadow");
});

Yuzuko.test("a clip outside the canvas does not clear source-in", () => {
  const ctx = context(8, 4);
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(0, 0, 8, 4);
  ctx.globalCompositeOperation = "source-in";
  ctx.beginPath();
  ctx.rect(20, 20, 4, 4);
  ctx.clip();
  ctx.fillStyle = "#0000ff";
  ctx.fillRect(0, 0, 8, 4);
  same(pixel(ctx, 1, 1), [255, 0, 0, 255], "unchanged");
});

Yuzuko.test("a CSS blur filter spreads ink past the rectangle", () => {
  const ctx = context(8, 8);
  ctx.filter = "blur(4px)";
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(2, 2, 2, 2);
  assert(pixel(ctx, 0, 3)[3] > 0, "outside");
  assert(pixel(ctx, 3, 3)[3] < 255, "inside");
});
