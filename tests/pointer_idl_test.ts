function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function throwsTypeError(run) {
  try {
    run();
  } catch (error) {
    return error instanceof TypeError;
  }
  return false;
}

function constant(holder, name, value) {
  const desc = Object.getOwnPropertyDescriptor(holder, name);
  assert(desc !== undefined, `${name} own`);
  assert(desc.value === value, `${name} value`);
  assert(desc.writable === false, `${name} writable`);
  assert(desc.enumerable === true, `${name} enumerable`);
  assert(desc.configurable === false, `${name} configurable`);
  assert(desc.get === undefined && desc.set === undefined, `${name} data`);
}

Yuzuko.test("mouse event constructors have length 1", () => {
  assert(MouseEvent.length === 1, "mouse");
  assert(WheelEvent.length === 1, "wheel");
  assert(PointerEvent.length === 1, "pointer");
  assert(DragEvent.length === 1, "drag");
  assert(MouseEvent.prototype.initMouseEvent.length === 1, "init");
  assert(throwsTypeError(() => new MouseEvent()), "mouse arity");
  assert(
    throwsTypeError(() => new MouseEvent("mousedown").initMouseEvent()),
    "init arity",
  );
});

Yuzuko.test("wheel and key constants are fixed data properties", () => {
  const cases = [
    [WheelEvent, "DOM_DELTA_PIXEL", 0],
    [WheelEvent, "DOM_DELTA_LINE", 1],
    [WheelEvent, "DOM_DELTA_PAGE", 2],
    [KeyboardEvent, "DOM_KEY_LOCATION_STANDARD", 0],
    [KeyboardEvent, "DOM_KEY_LOCATION_LEFT", 1],
    [KeyboardEvent, "DOM_KEY_LOCATION_RIGHT", 2],
    [KeyboardEvent, "DOM_KEY_LOCATION_NUMPAD", 3],
  ];
  for (const [constructor, name, value] of cases) {
    constant(constructor, name, value);
    constant(constructor.prototype, name, value);
  }
  assert(
    throwsTypeError(() => {
      delete WheelEvent.DOM_DELTA_PIXEL;
    }),
    "delete",
  );
  assert(WheelEvent.DOM_DELTA_PIXEL === 0, "kept");
});

Yuzuko.test("layer coordinates follow the floored client point", () => {
  const mouse = new MouseEvent("mousedown", {
    clientX: 5.9,
    clientY: -1.2,
    screenX: 5.9,
  });
  assert(mouse.clientX === 5, "client");
  assert(mouse.offsetX === 5, "offset");
  assert(mouse.layerX === 5, "layerX");
  assert(mouse.layerY === -2, "layerY");
  assert(mouse.screenX === 5, "screen");

  const down = new PointerEvent("pointerdown", { clientX: 0.3, screenX: 0.3 });
  assert(down.clientX === 0.3, "fraction client");
  assert(down.screenX === 0.3, "fraction screen");
  assert(down.offsetX === 0.3, "fraction offset");
  assert(down.layerX === 0, "integer layer");

  const click = new PointerEvent("click", { screenX: 0.1, clientX: 0.1 });
  assert(click.screenX === 0, "click screen");
  assert(click.clientX === 0, "click client");
  assert(click.layerX === 0, "click layer");
});

Yuzuko.test("initMouseEvent writes the legacy argument order", () => {
  const target = new EventTarget();
  const event = new MouseEvent("mousedown", {
    buttons: 1,
    movementX: 4,
    movementY: 6,
    clientX: 9,
  });
  event.initMouseEvent(
    "mouseup",
    true,
    true,
    globalThis,
    2,
    5.9,
    1,
    3.2,
    4,
    true,
    true,
    false,
    true,
    2,
    target,
  );
  assert(event.type === "mouseup", "type");
  assert(event.bubbles === true && event.cancelable === true, "flags");
  assert(event.view === globalThis, "view");
  assert(event.detail === 2, "detail");
  assert(event.screenX === 5 && event.screenY === 1, "screen");
  assert(event.clientX === 3 && event.clientY === 4, "client");
  assert(event.layerX === 3 && event.offsetX === 3, "layer");
  assert(event.ctrlKey === true, "ctrl");
  assert(event.altKey === true, "alt");
  assert(event.shiftKey === false, "shift");
  assert(event.metaKey === true, "meta");
  assert(event.button === 2, "button");
  assert(event.relatedTarget === target, "related");
  assert(event.buttons === 1, "buttons");
  assert(event.movementX === 4 && event.movementY === 6, "movement");
  assert(event.isTrusted === false, "trusted");

  const bare = new MouseEvent("mousedown", {
    clientX: 8,
    ctrlKey: true,
    button: 1,
    relatedTarget: target,
  });
  bare.initMouseEvent("click");
  assert(bare.type === "click", "type only");
  assert(bare.bubbles === false && bare.cancelable === false, "defaults");
  assert(bare.view === null && bare.relatedTarget === null, "nulls");
  assert(bare.detail === 0 && bare.clientX === 0 && bare.button === 0, "zeros");
  assert(bare.ctrlKey === false && bare.altKey === false, "keys");

  const live = new MouseEvent("mousedown", { clientX: 4 });
  let during = false;
  const listener = (dispatched) => {
    dispatched.initMouseEvent(
      "mouseup",
      true,
      true,
      globalThis,
      7,
      9,
      9,
      9,
      9,
      true,
      true,
      true,
      true,
      1,
      target,
    );
    during = dispatched.type === "mousedown" && dispatched.clientX === 4;
  };
  addEventListener("mousedown", listener);
  try {
    assert(dispatchEvent(live) === true, "dispatch");
  } finally {
    removeEventListener("mousedown", listener);
  }
  assert(during, "during");
  assert(live.type === "mousedown" && live.clientX === 4, "after");

  let converted = false;
  const bad = {
    valueOf() {
      throw new TypeError("convert");
    },
  };
  const converting = (dispatched) => {
    converted = throwsTypeError(() => {
      dispatched.initMouseEvent("mouseup", false, false, null, bad);
    });
    assert(dispatched.type === "mousedown", "type after throw");
  };
  addEventListener("mousedown", converting);
  try {
    dispatchEvent(live);
  } finally {
    removeEventListener("mousedown", converting);
  }
  assert(converted, "convert");
  assert(live.detail === 0, "detail stayed");

  assert(
    throwsTypeError(() => live.initMouseEvent("x", false, false, {})),
    "view",
  );
  assert(
    throwsTypeError(() =>
      live.initMouseEvent(
        "x",
        false,
        false,
        null,
        0,
        0,
        0,
        0,
        0,
        false,
        false,
        false,
        false,
        0,
        1,
      ),
    ),
    "related",
  );
});

Yuzuko.test("wheel momentum follows the dictionary", () => {
  assert(new WheelEvent("wheel").momentum === false, "default");
  assert(new WheelEvent("wheel", { momentum: true }).momentum === true, "set");
  assert(new WheelEvent("wheel", { momentum: false }).momentum === false, "clear");
});

Yuzuko.test("navigator reports touch points", () => {
  const value = navigator.maxTouchPoints;
  assert(typeof value === "number", "number");
  assert(Number.isInteger(value), "integer");
  assert(value >= 0 && value <= 2147483647, "range");
  assert(Object.hasOwn(navigator, "maxTouchPoints") === false, "inherited");
});

Yuzuko.test("drag events floor and store a data transfer", () => {
  const event = new DragEvent("dragstart", { clientX: 2.8, screenX: 1.1 });
  assert(event instanceof DragEvent, "instance");
  assert(event instanceof MouseEvent, "mouse");
  assert(event.clientX === 2 && event.screenX === 1, "floor");
  assert(event.layerX === 2, "layer");
  assert(event.dataTransfer === null, "null");
  assert(new DragEvent("drop", { dataTransfer: null }).dataTransfer === null, "explicit");
  const store = new DataTransfer();
  const withStore = new DragEvent("drop", { dataTransfer: store });
  assert(withStore.dataTransfer === store, "same");
  assert(
    throwsTypeError(() => new DragEvent("drop", { dataTransfer: {} })),
    "reject",
  );
});
