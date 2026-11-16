function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function alpha(ctx, x, y) {
  return ctx.getImageData(x, y, 1, 1).data[3];
}

function filled(ctx, x, y, label) {
  const value = alpha(ctx, x, y);
  assert(value >= 200, `${label} at ${x},${y} alpha ${value}`);
}

function clear(ctx, x, y, label) {
  const value = alpha(ctx, x, y);
  assert(value <= 20, `${label} at ${x},${y} alpha ${value}`);
}

function paint(radii, omitted) {
  const ctx = new OffscreenCanvas(40, 40).getContext("2d");
  ctx.beginPath();
  if (omitted) {
    ctx.roundRect(0, 0, 40, 40);
  } else {
    ctx.roundRect(0, 0, 40, 40, radii);
  }
  ctx.fill();
  return ctx;
}

function bytes(radii) {
  return paint(radii, false).getImageData(0, 0, 40, 40).data;
}

function same(left, right, label) {
  for (let i = 0; i < left.length; i++) {
    if (left[i] !== right[i]) {
      throw new Error(`${label} differs at ${i}: ${left[i]} vs ${right[i]}`);
    }
  }
}

function differs(left, right, label) {
  for (let i = 0; i < left.length; i++) {
    if (left[i] !== right[i]) {
      return;
    }
  }
  throw new Error(`${label} unexpectedly matches`);
}

function corners(ctx, shape, label) {
  const sample = [
    [2, 2],
    [37, 2],
    [37, 37],
    [2, 37],
  ];
  for (let index = 0; index < 4; index++) {
    const [x, y] = sample[index];
    if (shape[index] === 1) {
      filled(ctx, x, y, `${label} corner ${index}`);
    } else {
      clear(ctx, x, y, `${label} corner ${index}`);
    }
  }
  filled(ctx, 20, 20, `${label} center`);
}

function throws(fn, type, message) {
  try {
    fn();
  } catch (error) {
    assert(error instanceof type, `expected ${type.name}, got ${error}`);
    if (message !== undefined && !String(error.message).includes(message)) {
      throw new Error(`expected ${message}, got ${error.message}`);
    }
    return;
  }
  throw new Error(`expected ${type.name}`);
}

Yuzuko.test("roundRect radii follow DOMPointInit and sequences", () => {
  const sharp = [1, 1, 1, 1];
  const round = [0, 0, 0, 0];
  corners(paint(undefined, true), sharp, "omitted");
  corners(paint(undefined, false), sharp, "undefined");
  corners(paint(null, false), sharp, "null");
  corners(paint(0, false), sharp, "zero");
  corners(paint(16, false), round, "number");
  corners(paint("16", false), round, "string");
  corners(paint(new Number(16), false), sharp, "number object");
  corners(paint({ x: 16, y: 16 }, false), round, "dictionary");
  same(bytes({ x: 16 }), bytes({ x: 16, y: 0 }), "omitted y");
  same(bytes({ y: 16 }), bytes({ x: 0, y: 16 }), "omitted x");
  differs(bytes({ x: 16 }), bytes({ x: 16, y: 16 }), "both axes");
  corners(paint(new DOMPoint(16, 16), false), round, "DOMPoint");
  corners(paint([16], false), round, "one");
  corners(paint([0, 16], false), [1, 0, 1, 0], "two");
  corners(paint([0, 16, 0], false), [1, 0, 1, 0], "three");
  corners(paint([0, 0, 16, 0], false), [1, 1, 0, 1], "four");
  corners(paint([[25]], false), sharp, "array element");
  corners(paint([undefined], false), sharp, "undefined element");
  corners(paint([[]], false), sharp, "empty array element");
  corners(paint(new Set([0, 16]), false), [1, 0, 1, 0], "set");
  function* pairs() {
    yield 0;
    yield new DOMPoint(16, 16);
  }
  corners(paint(pairs(), false), [1, 0, 1, 0], "generator");
  function list(a, b) {
    return arguments;
  }
  corners(paint(list(0, 16), false), [1, 0, 1, 0], "arguments");

  const path = new Path2D();
  path.roundRect(0, 0, 40, 40, [[25]]);
  const ctx = new OffscreenCanvas(40, 40).getContext("2d");
  ctx.fill(path);
  corners(ctx, sharp, "Path2D zero");
  const rounded = new Path2D();
  rounded.roundRect(0, 0, 40, 40, [16]);
  const other = new OffscreenCanvas(40, 40).getContext("2d");
  other.fill(rounded);
  corners(other, round, "Path2D number");
});

Yuzuko.test("roundRect rejects an unusable radius list", () => {
  const ctx = new OffscreenCanvas(40, 40).getContext("2d");
  throws(
    () => ctx.roundRect(0, 0, 40, 40, -1),
    RangeError,
    "The radius is negative",
  );
  throws(
    () => ctx.roundRect(0, 0, 40, 40, { x: -1, y: 0 }),
    RangeError,
    "The radius is negative",
  );
  throws(
    () => ctx.roundRect(0, 0, 40, 40, []),
    RangeError,
    "roundRect radii must contain one to four values",
  );
  throws(
    () => ctx.roundRect(0, 0, 40, 40, [0, 0, 0, 0, 0]),
    RangeError,
    "roundRect radii must contain one to four values",
  );
  throws(
    () => new Path2D().roundRect(0, 0, 40, 40, -2),
    RangeError,
    "The radius is negative",
  );
  throws(() => ctx.roundRect(0, 0, 40, 40, 0n), TypeError);
  throws(() => ctx.roundRect(0, 0, 40, 40, { x: 0n }), TypeError);
  throws(() => ctx.roundRect(0, 0, 40, 40, [{ x: 0n }]), TypeError);
  throws(() => ctx.roundRect(0, 0, 40, 40, { z: 0n }), TypeError);
  throws(() => ctx.roundRect(NaN, 0, 40, 40, 0n), TypeError);
  throws(
    () => ctx.roundRect(0, 0, 40, 40, [-1, NaN]),
    RangeError,
    "The radius is negative",
  );
  ctx.roundRect(0, 0, 40, 40, [NaN, -1]);
  ctx.roundRect(0, 0, 40, 40, { x: NaN, y: -1 });
  ctx.roundRect(0, 0, 40, 40, { x: -1, y: NaN });
  ctx.roundRect(NaN, 0, 40, 40, []);
  ctx.roundRect(NaN, 0, 40, 40, -1);
  ctx.fill();
  clear(ctx, 20, 20, "non-finite");
  const infinite = {
    [Symbol.iterator]() {
      return { next: () => ({ done: false, value: 1 }) };
    },
  };
  throws(
    () => ctx.roundRect(0, 0, 40, 40, infinite),
    RangeError,
    "roundRect radii must contain one to four values",
  );
  throws(
    () => ctx.roundRect(0, 0, 40, 40, { [Symbol.iterator]: 1 }),
    TypeError,
    "@@iterator is not callable",
  );
  throws(
    () => ctx.roundRect(0, 0, 40, 40, {
      get w() {
        throw new TypeError("w getter");
      },
      x: 1,
      y: 1,
    }),
    TypeError,
    "w getter",
  );
  throws(() => ctx.roundRect(0, 0), TypeError, "roundRect requires a rectangle");
});

Yuzuko.test("stroke draws the current path or a Path2D", () => {
  const ctx = new OffscreenCanvas(30, 30).getContext("2d");
  ctx.lineWidth = 6;
  ctx.beginPath();
  ctx.rect(4, 4, 10, 10);
  ctx.stroke();
  filled(ctx, 8, 4, "current edge");
  ctx.clearRect(0, 0, 30, 30);
  ctx.beginPath();
  ctx.rect(4, 4, 10, 10);
  const path = new Path2D();
  path.rect(16, 16, 10, 10);
  ctx.stroke(path);
  clear(ctx, 8, 4, "current path left unstroked");
  filled(ctx, 20, 16, "path edge");
  throws(
    () => ctx.stroke({}),
    TypeError,
    "The path argument is not a Path2D",
  );
});
