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

Yuzuko.test("text metrics stay the classic getters", () => {
  const canvas = new OffscreenCanvas(80, 64);
  const ctx = canvas.getContext("2d");
  ctx.font = "20px sans-serif";
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  const metrics = ctx.measureText("Ab");
  assert(metrics.width > 0, "width");
  assert(typeof metrics.actualBoundingBoxAscent === "number", "ascent");
  assert(typeof metrics.fontBoundingBoxAscent === "number", "font ascent");
  assert(typeof metrics.emHeightAscent === "number", "em ascent");
  assert(typeof metrics.alphabeticBaseline === "number", "alphabetic");
  assert(typeof metrics.getSelectionRects === "undefined", "no selection rects");
  assert(typeof metrics.getActualBoundingBox === "undefined", "no range box");
  assert(typeof metrics.getTextClusters === "undefined", "no clusters");
  assert(typeof metrics.getIndexFromOffset === "undefined", "no caret index");
  assert(typeof ctx.fillTextCluster === "undefined", "no fillTextCluster");
  assert(typeof ctx.strokeTextCluster === "undefined", "no strokeTextCluster");
  assert(typeof TextCluster === "undefined", "no TextCluster");
  assert(typeof CanvasFilter === "undefined", "CanvasFilter absent");

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
});
