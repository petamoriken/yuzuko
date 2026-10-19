function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function pixel(ctx, x, y) {
  const data = ctx.getImageData(x, y, 1, 1).data;
  return [data[0], data[1], data[2], data[3]];
}

function inkColumns(canvas) {
  const ctx = canvas.getContext("2d");
  const w = canvas.width;
  const h = canvas.height;
  const data = ctx.getImageData(0, 0, w, h).data;
  let minX = w;
  let maxX = -1;
  let above = 0;
  let below = 0;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      if (data[(y * w + x) * 4 + 3] < 16) {
        continue;
      }
      minX = Math.min(minX, x);
      maxX = Math.max(maxX, x);
      if (y < 48) {
        above += 1;
      }
      if (y > 52) {
        below += 1;
      }
    }
  }
  return { minX, maxX, above, below };
}

Yuzuko.test("css filters parse and do not paint", () => {
  const ctx = new OffscreenCanvas(8, 4).getContext("2d");
  assert(ctx.filter === "none", "default");
  ctx.filter = "blur(2px)";
  assert(ctx.filter === "blur(2px)", "stored");
  ctx.filter = "blur(2)";
  assert(ctx.filter === "blur(2px)", "unitless ignored");
  ctx.filter = "inherit";
  assert(ctx.filter === "blur(2px)", "inherit ignored");
  ctx.filter = "initial";
  assert(ctx.filter === "blur(2px)", "initial ignored");
  ctx.filter = "";
  assert(ctx.filter === "blur(2px)", "empty ignored");
  ctx.filter = "blur(1px) contrast(50%)";
  assert(ctx.filter === "blur(1px) contrast(50%)", "list");
  ctx.filter = " NONE ";
  assert(ctx.filter === " NONE ", "none keyword");
  ctx.filter = "drop-shadow(4px 4px 0 red)";
  assert(ctx.filter === "drop-shadow(4px 4px 0 red)", "shadow syntax");
  ctx.save();
  ctx.filter = "opacity(50%)";
  assert(ctx.filter === "opacity(50%)", "saved value");
  ctx.restore();
  assert(ctx.filter === "drop-shadow(4px 4px 0 red)", "restored");

  ctx.filter = "blur(4px)";
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(2, 1, 2, 2);
  assert(pixel(ctx, 2, 1).join() === "255,0,0,255", "sharp");
  assert(pixel(ctx, 0, 0).join() === "0,0,0,0", "unblurred");
});

Yuzuko.test("layers group alpha and keep filters unpainted", () => {
  const ctx = new OffscreenCanvas(8, 4).getContext("2d");
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, 8, 4);
  ctx.globalAlpha = 0.5;
  ctx.filter = "blur(3px)";
  ctx.beginLayer();
  assert(ctx.globalAlpha === 1, "alpha reset");
  assert(ctx.filter === "none", "filter reset");
  ctx.fillStyle = "#0000ff";
  ctx.fillRect(0, 0, 8, 4);
  ctx.fillStyle = "#00ff00";
  ctx.fillRect(0, 0, 8, 4);
  assert(pixel(ctx, 1, 1).join() === "255,255,255,255", "open layer hidden");
  ctx.endLayer();
  assert(ctx.globalAlpha === 0.5, "alpha restored");
  assert(ctx.filter === "blur(3px)", "filter restored");
  const [r, g, b, a] = pixel(ctx, 1, 1);
  assert(a === 255, "opaque canvas");
  assert(g > 240 && r > 100 && r < 160 && b > 100 && b < 160, `grouped ${r},${g},${b}`);
  assert(pixel(ctx, 0, 0)[0] > 100, "edge stays sharp");

  ctx.globalAlpha = 0.5;
  ctx.beginLayer();
  ctx.globalAlpha = 0.25;
  ctx.beginLayer();
  assert(ctx.globalAlpha === 1, "inner reset");
  ctx.endLayer();
  assert(ctx.globalAlpha === 0.25, "inner restored");
  ctx.endLayer();
  assert(ctx.globalAlpha === 0.5, "outer restored");

  let restore = false;
  ctx.beginLayer();
  try {
    ctx.restore();
  } catch (error) {
    restore = error.name === "InvalidStateError";
  }
  assert(restore, "restore inside layer");
  ctx.endLayer();

  ctx.save();
  let ended = false;
  try {
    ctx.endLayer();
  } catch (error) {
    ended = error.name === "InvalidStateError";
  }
  assert(ended, "endLayer inside save");
  ctx.restore();

  let lone = false;
  try {
    ctx.endLayer();
  } catch (error) {
    lone = error.name === "InvalidStateError";
  }
  assert(lone, "lone endLayer");

  ctx.beginLayer();
  ctx.save();
  ctx.filter = "sepia(1)";
  ctx.restore();
  assert(ctx.filter === "none", "save inside layer");
  ctx.endLayer();

  ctx.beginLayer();
  ctx.reset();
  let afterReset = false;
  try {
    ctx.endLayer();
  } catch (error) {
    afterReset = error.name === "InvalidStateError";
  }
  assert(afterReset, "reset discards the layer");
  assert(ctx.filter === "none", "reset filter");
});

Yuzuko.test("layer filters validate and clips do not leak", () => {
  const ctx = new OffscreenCanvas(8, 4).getContext("2d");
  ctx.beginLayer({ filter: "blur(4px)" });
  ctx.fillStyle = "#00ff00";
  ctx.fillRect(2, 1, 2, 2);
  ctx.endLayer();
  assert(pixel(ctx, 2, 1).join() === "0,255,0,255", "layer not blurred");
  assert(pixel(ctx, 0, 0).join() === "0,0,0,0", "outside clear");

  ctx.beginLayer();
  ctx.endLayer();
  ctx.beginLayer(null);
  ctx.endLayer();
  ctx.beginLayer({});
  ctx.endLayer();
  ctx.beginLayer({ filter: "" });
  ctx.endLayer();

  let bad = false;
  try {
    ctx.beginLayer({ filter: "blur(4)" });
  } catch (error) {
    bad = error.name === "TypeError";
  }
  assert(bad, "bad filter");
  let closed = false;
  try {
    ctx.endLayer();
  } catch (error) {
    closed = error.name === "InvalidStateError";
  }
  assert(closed, "layer was not opened");

  let object = false;
  try {
    ctx.beginLayer({ filter: { name: "gaussianBlur", stdDeviation: 4 } });
  } catch (error) {
    object = error.name === "TypeError";
  }
  assert(object, "no CanvasFilter");

  let text = false;
  try {
    ctx.beginLayer("blur(1px)");
  } catch (error) {
    text = error.name === "TypeError";
  }
  assert(text, "options must be a dictionary");

  ctx.globalAlpha = 1;
  ctx.beginLayer();
  ctx.beginPath();
  ctx.rect(0, 0, 1, 1);
  ctx.clip();
  ctx.endLayer();
  ctx.fillStyle = "#ff0000";
  ctx.fillRect(2, 0, 2, 2);
  assert(pixel(ctx, 3, 1)[0] === 255, "inner clip did not leak");

  const clipped = new OffscreenCanvas(8, 4);
  const ctx2 = clipped.getContext("2d");
  ctx2.beginPath();
  ctx2.rect(0, 0, 2, 4);
  ctx2.clip();
  ctx2.beginLayer();
  ctx2.fillStyle = "#0000ff";
  ctx2.fillRect(0, 0, 8, 4);
  ctx2.endLayer();
  assert(pixel(ctx2, 1, 1)[2] === 255, "inside clip");
  assert(pixel(ctx2, 4, 1)[3] === 0, "parent clip");

  const moved = new OffscreenCanvas(8, 4);
  const ctx3 = moved.getContext("2d");
  ctx3.translate(3, 0);
  ctx3.beginLayer();
  ctx3.fillStyle = "#ff0000";
  ctx3.fillRect(0, 0, 2, 2);
  ctx3.endLayer();
  assert(pixel(ctx3, 3, 0)[0] === 255, "translated");
  assert(pixel(ctx3, 0, 0)[3] === 0, "origin empty");

  const replaced = new OffscreenCanvas(8, 4);
  const ctx4 = replaced.getContext("2d");
  ctx4.translate(3, 0);
  ctx4.beginLayer();
  ctx4.setTransform(1, 0, 0, 1, 0, 0);
  ctx4.fillStyle = "#0000ff";
  ctx4.fillRect(0, 0, 2, 2);
  ctx4.endLayer();
  ctx4.fillStyle = "#00ff00";
  ctx4.fillRect(0, 0, 2, 2);
  assert(pixel(ctx4, 0, 0)[2] === 255, "setTransform is absolute");
  assert(pixel(ctx4, 3, 0)[1] === 255, "transform restored");
});

Yuzuko.test("enhanced text metrics and text clusters", () => {
  const canvas = new OffscreenCanvas(80, 64);
  const ctx = canvas.getContext("2d");
  ctx.font = "20px sans-serif";
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  const text = "Ab";
  const metrics = ctx.measureText(text);
  assert(metrics.width > 0, "width");
  const rects = metrics.getSelectionRects(0, 1);
  assert(rects.length === 1, "one rect");
  assert(rects[0] instanceof DOMRectReadOnly, "rect type");
  assert(Math.abs(rects[0].x) < 1, `selection x ${rects[0].x}`);
  assert(rects[0].width > 0 && rects[0].width < metrics.width, "partial width");
  assert(rects[0].y < 0, "above baseline");
  assert(rects[0].height > 0, "line height");
  assert(metrics.getSelectionRects(0, 0).length === 0, "empty range");

  const box = metrics.getActualBoundingBox(0, 2);
  assert(box instanceof DOMRectReadOnly, "box type");
  assert(box.width > 0 && box.height > 0, "ink box");
  assert(box.y < 0, "ink above baseline");

  const clusters = metrics.getTextClusters();
  assert(clusters.length === 2, "two clusters");
  assert(clusters[0] instanceof TextCluster, "cluster type");
  assert(clusters[0].start === 0 && clusters[0].end === 1, "first span");
  assert(clusters[1].start === 1 && clusters[1].end === 2, "second span");
  assert(clusters[0].align === "left" && clusters[0].baseline === "alphabetic", "styles");
  assert(Math.abs(clusters[0].x) < 1, "first anchor");
  assert(clusters[1].x > clusters[0].x, "second anchor");
  const centered = metrics.getTextClusters({ align: "center" });
  assert(centered[0].align === "center", "option align");
  assert(centered[0].x > 0 && centered[0].x < metrics.width, "center x");
  assert(clusters[0].align === "left", "original unchanged");
  assert(metrics.getTextClusters(1, 2).length === 1, "range");
  assert(metrics.getTextClusters(1, 2)[0].start === 1, "range start");

  const accent = ctx.measureText("e\u0301").getTextClusters();
  assert(accent.length === 1 && accent[0].end === 2, "grapheme");

  assert(metrics.getIndexFromOffset(-10) === 0, "left of text");
  assert(metrics.getIndexFromOffset(metrics.width + 10) === text.length, "right of text");
  const middle = metrics.getIndexFromOffset(metrics.width / 2);
  assert(middle === 1 || middle === 2, `middle ${middle}`);

  ctx.textAlign = "start";
  ctx.direction = "rtl";
  const rtl = ctx.measureText("Ab");
  assert(rtl.getIndexFromOffset(8) === 0, `rtl right side ${rtl.getIndexFromOffset(8)}`);
  assert(
    rtl.getIndexFromOffset(-rtl.width - 8) === 2,
    `rtl left side ${rtl.getIndexFromOffset(-rtl.width - 8)}`,
  );
  ctx.direction = "ltr";
  ctx.textAlign = "left";

  ctx.textAlign = "center";
  const centeredText = ctx.measureText("Ab");
  const full = centeredText.getSelectionRects(0, 2);
  assert(full.length === 1 && full[0].x < 0, "centered origin");
  assert(full[0].x + full[0].width > 0, "centered span");
  ctx.textAlign = "left";

  ctx.fillStyle = "#000000";
  ctx.fillText("A", 8, 50);
  const placed = inkColumns(canvas);
  assert(placed.above > 10, "baseline is above the anchor");
  assert(placed.below < placed.above, "little ink below the baseline");

  const wide = new OffscreenCanvas(80, 64);
  const wideCtx = wide.getContext("2d");
  wideCtx.font = "20px sans-serif";
  wideCtx.fillStyle = "#000000";
  wideCtx.fillText("Ab", 4, 40);
  const pair = inkColumns(wide);
  const narrow = new OffscreenCanvas(80, 64);
  const narrowCtx = narrow.getContext("2d");
  narrowCtx.font = "20px sans-serif";
  narrowCtx.fillStyle = "#000000";
  narrowCtx.fillText("A", 4, 40);
  const single = inkColumns(narrow);
  assert(pair.maxX > single.maxX + 2, "letters are not stacked");

  const alone = new OffscreenCanvas(80, 64);
  const direct = alone.getContext("2d");
  direct.font = "20px sans-serif";
  direct.fillStyle = "#000000";
  const second = metrics.getTextClusters()[1];
  direct.fillTextCluster(second, 4, 40);
  const shifted = inkColumns(alone);
  const exact = new OffscreenCanvas(80, 64);
  const pinned = exact.getContext("2d");
  pinned.font = "20px sans-serif";
  pinned.fillStyle = "#000000";
  pinned.fillTextCluster(second, 4, 40, { x: 0, y: 0 });
  const overridden = inkColumns(exact);
  assert(overridden.minX < shifted.minX - 4, "x override places the cluster");
  assert(overridden.minX < 12, `pinned near the anchor ${overridden.minX}`);

  let illegal = false;
  try {
    new TextCluster();
  } catch (error) {
    illegal = error instanceof TypeError;
  }
  assert(illegal, "no constructor");
  assert(typeof CanvasFilter === "undefined", "CanvasFilter absent");
});
