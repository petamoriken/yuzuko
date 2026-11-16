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

Yuzuko.test("inherited initMouseEvent leaves subclass fields", () => {
  assert(
    Object.prototype.hasOwnProperty.call(MouseEvent.prototype, "initMouseEvent"),
    "mouse own",
  );
  assert(
    !Object.prototype.hasOwnProperty.call(DragEvent.prototype, "initMouseEvent"),
    "drag own",
  );
  assert(
    !Object.prototype.hasOwnProperty.call(
      PointerEvent.prototype,
      "initMouseEvent",
    ),
    "pointer own",
  );
  assert(
    !Object.prototype.hasOwnProperty.call(WheelEvent.prototype, "initMouseEvent"),
    "wheel own",
  );

  const transfer = new DataTransfer();
  const drag = new DragEvent("dragstart", { dataTransfer: transfer });
  drag.initMouseEvent(
    "dragover",
    true,
    true,
    null,
    0,
    0,
    0,
    0,
    0,
    false,
    true,
    false,
    true,
    0,
    null,
  );
  assert(drag.type === "dragover", "drag type");
  assert(drag.altKey === true, "alt");
  assert(drag.shiftKey === false, "shift");
  assert(drag.metaKey === true, "meta");
  assert(drag.dataTransfer === transfer, "transfer");

  const pointer = new PointerEvent("pointerdown", {
    pointerId: 9,
    movementX: 3,
    buttons: 2,
    screenX: 4,
    clientX: 4,
  });
  pointer.initMouseEvent(
    "pointermove",
    false,
    false,
    null,
    0,
    1.5,
    0,
    1.5,
    0,
    false,
    false,
    false,
    false,
    0,
    null,
  );
  assert(pointer.pointerId === 9, "pointerId");
  assert(pointer.movementX === 3, "movement");
  assert(pointer.buttons === 2, "buttons");
  assert(pointer.screenX === 1.5, "fraction screen");
  assert(pointer.clientX === 1.5, "fraction client");
  assert(pointer.layerX === 1, "floored layer");
  pointer.initMouseEvent(
    "click",
    false,
    false,
    null,
    0,
    1.5,
    0,
    1.5,
    0,
    false,
    false,
    false,
    false,
    0,
    null,
  );
  assert(pointer.screenX === 1, "click screen");
  assert(pointer.clientX === 1, "click client");
  assert(pointer.layerX === 1, "click layer");
  assert(pointer.pointerId === 9, "pointerId kept");

  const wheel = new WheelEvent("wheel", { deltaY: 4, clientX: 1.9 });
  wheel.initMouseEvent("wheel", false, false, null, 0, 0, 0, 1.9, 0);
  assert(wheel.deltaY === 4, "delta");
  assert(wheel.clientX === 1, "wheel client");
});

Yuzuko.test("initUIEvent writes the UI fields only", () => {
  assert(UIEvent.prototype.initUIEvent.length === 1, "length");
  const fresh = new UIEvent("a", { detail: 5, which: 8, composed: true });
  assert(throwsTypeError(() => fresh.initUIEvent()), "arity");
  assert(fresh.type === "a", "arity type");
  assert(throwsTypeError(() => fresh.initUIEvent("b", true, true, {}, 9)), "view");
  assert(fresh.type === "a", "view type");
  assert(fresh.detail === 5, "view detail");

  fresh.initUIEvent("z");
  assert(fresh.type === "z", "type");
  assert(fresh.bubbles === false, "bubbles");
  assert(fresh.cancelable === false, "cancelable");
  assert(fresh.view === null, "view null");
  assert(fresh.detail === 0, "detail");
  assert(fresh.which === 8, "which");
  assert(fresh.composed === true, "composed");
  assert(fresh.isTrusted === false, "trusted");

  fresh.initUIEvent("b", true, true, globalThis, 3);
  assert(fresh.view === globalThis, "global view");
  assert(fresh.detail === 3, "set detail");
  fresh.initUIEvent("c", false, false, null, 1);
  assert(fresh.view === null, "cleared view");

  const target = new EventTarget();
  const event = new UIEvent("a", {
    bubbles: true,
    cancelable: true,
    detail: 4,
  });
  target.addEventListener("a", () => {
    assert(
      throwsTypeError(() => event.initUIEvent("b", true, true, {}, 9)),
      "dispatch view",
    );
    assert(event.type === "a", "dispatch type");
    assert(event.detail === 4, "dispatch detail");
    event.stopPropagation();
    event.preventDefault();
    event.initUIEvent("b", false, false, null, 1);
    assert(event.type === "a", "noop type");
    assert(event.detail === 4, "noop detail");
  });
  target.dispatchEvent(event);
  assert(event.cancelBubble === true, "stopped");
  assert(event.defaultPrevented === true, "prevented");
  assert(event.target === target, "target");
  event.initUIEvent("b", false, false, null, 1);
  assert(event.type === "b", "after type");
  assert(event.detail === 1, "after detail");
  assert(event.cancelBubble === true, "stop kept");
  assert(event.defaultPrevented === true, "prevent kept");
  assert(event.target === target, "target kept");
  assert(event.isTrusted === false, "trust kept");
});

Yuzuko.test("initKeyboardEvent writes the key and the four modifiers", () => {
  assert(KeyboardEvent.prototype.initKeyboardEvent.length === 1, "length");
  const event = new KeyboardEvent("keydown", {
    key: "A",
    code: "KeyA",
    location: 2,
    repeat: true,
    isComposing: true,
    ctrlKey: true,
    altKey: true,
    shiftKey: true,
    metaKey: true,
    modifierCapsLock: true,
    charCode: 65,
    keyCode: 65,
    which: 65,
    detail: 3,
  });
  assert(throwsTypeError(() => event.initKeyboardEvent()), "arity");
  assert(event.key === "A", "arity key");
  assert(
    throwsTypeError(() =>
      event.initKeyboardEvent("keyup", true, true, {}, "b"),
    ),
    "view",
  );
  assert(event.key === "A", "view key");

  event.initKeyboardEvent(
    "keyup",
    true,
    false,
    null,
    "b",
    1,
    false,
    true,
    false,
    true,
  );
  assert(event.type === "keyup", "type");
  assert(event.bubbles === true, "bubbles");
  assert(event.cancelable === false, "cancelable");
  assert(event.key === "b", "key");
  assert(event.location === 1, "location");
  assert(event.ctrlKey === false, "ctrl");
  assert(event.altKey === true, "alt");
  assert(event.shiftKey === false, "shift");
  assert(event.metaKey === true, "meta");
  assert(event.code === "KeyA", "code");
  assert(event.repeat === true, "repeat");
  assert(event.isComposing === true, "composing");
  assert(event.charCode === 65, "char");
  assert(event.keyCode === 65, "keyCode");
  assert(event.which === 65, "which");
  assert(event.getModifierState("CapsLock") === true, "caps");
  assert(event.detail === 3, "detail");

  event.initKeyboardEvent("keydown");
  assert(event.key === "", "default key");
  assert(event.location === 0, "default location");
  assert(event.ctrlKey === false, "default ctrl");
  assert(event.altKey === false, "default alt");
  assert(event.shiftKey === false, "default shift");
  assert(event.metaKey === false, "default meta");
  assert(event.getModifierState("CapsLock") === true, "caps kept");
  assert(event.code === "KeyA", "code kept");

  const target = new EventTarget();
  target.addEventListener("keydown", () => {
    event.initKeyboardEvent("keyup", true, true, null, "c", 2);
    assert(event.key === "", "dispatch key");
    assert(event.type === "keydown", "dispatch type");
  });
  target.dispatchEvent(event);
});

Yuzuko.test("initCompositionEvent writes data and leaves detail", () => {
  assert(CompositionEvent.prototype.initCompositionEvent.length === 1, "length");
  const event = new CompositionEvent("compositionupdate", {
    data: "ab",
    detail: 4,
  });
  assert(throwsTypeError(() => event.initCompositionEvent()), "arity");
  assert(event.data === "ab", "arity data");
  assert(
    throwsTypeError(() => event.initCompositionEvent("compositionstart", true, true, {})),
    "view",
  );
  assert(event.data === "ab", "view data");
  event.initCompositionEvent("compositionstart");
  assert(event.type === "compositionstart", "type");
  assert(event.data === "", "default data");
  assert(event.detail === 4, "detail");
  assert(event.bubbles === false, "bubbles");
  event.initCompositionEvent("compositionupdate", true, true, null, "xy");
  assert(event.data === "xy", "data");
  assert(event.bubbles === true, "set bubbles");
  assert(event.detail === 4, "detail kept");

  const target = new EventTarget();
  target.addEventListener("compositionupdate", () => {
    event.initCompositionEvent("compositionend", false, false, null, "no");
    assert(event.data === "xy", "dispatch data");
  });
  target.dispatchEvent(event);
});

Yuzuko.test("initCustomEvent stores detail by identity", () => {
  assert(CustomEvent.prototype.initCustomEvent.length === 1, "length");
  const original = { n: 1 };
  const event = new CustomEvent("a", { detail: original });
  assert(throwsTypeError(() => event.initCustomEvent()), "arity");
  assert(event.detail === original, "arity detail");
  event.initCustomEvent("b");
  assert(event.type === "b", "type");
  assert(event.detail === null, "omitted");
  assert(event.bubbles === false, "bubbles");
  event.initCustomEvent("c", false, false, undefined);
  assert(event.detail === null, "undefined");
  const box = { n: 2 };
  event.initCustomEvent("d", true, true, box);
  assert(event.detail === box, "identity");
  assert(event.bubbles === true, "set bubbles");
  event.initCustomEvent("e", false, false, null);
  assert(event.detail === null, "null");

  const target = new EventTarget();
  const kept = { n: 3 };
  const dispatched = new CustomEvent("a", { detail: kept });
  target.addEventListener("a", () => {
    dispatched.initCustomEvent("b", true, true, { n: 4 });
    assert(dispatched.detail === kept, "dispatch detail");
    assert(dispatched.type === "a", "dispatch type");
  });
  target.dispatchEvent(dispatched);
});

Yuzuko.test("initMessageEvent stores a port and a frozen port list", async () => {
  assert(MessageEvent.prototype.initMessageEvent.length === 1, "length");
  const constructed = new MessageEvent("m");
  assert(constructed.data === undefined, "constructed data");
  assert(constructed.source === null, "constructed source");
  assert(throwsTypeError(() => constructed.initMessageEvent()), "arity");
  assert(constructed.data === undefined, "arity data");

  constructed.initMessageEvent("message");
  assert(constructed.data === null, "default data");
  assert(constructed.origin === "", "origin");
  assert(constructed.lastEventId === "", "id");
  assert(constructed.source === null, "default source");
  assert(constructed.ports.length === 0, "empty ports");
  const firstPorts = constructed.ports;
  assert(Object.isFrozen(firstPorts), "frozen");
  assert(constructed.ports === firstPorts, "same ports");

  const channel = new MessageChannel();
  constructed.initMessageEvent(
    "message",
    false,
    false,
    "data",
    "https://example.test",
    "id",
    channel.port1,
    [channel.port2],
  );
  assert(constructed.data === "data", "data");
  assert(constructed.origin === "https://example.test", "set origin");
  assert(constructed.lastEventId === "id", "set id");
  assert(constructed.source === channel.port1, "source");
  assert(constructed.ports.length === 1, "one port");
  assert(constructed.ports[0] === channel.port2, "listed");
  assert(constructed.ports !== firstPorts, "replaced");
  const frozen = constructed.ports;
  assert(Object.isFrozen(frozen), "replaced frozen");
  assert(constructed.ports === frozen, "stable");

  constructed.initMessageEvent("message", false, false, 1, "", "", null);
  assert(constructed.source === null, "null source");
  constructed.initMessageEvent("message", false, false, 1, "", "", undefined);
  assert(constructed.source === null, "undefined source");

  constructed.initMessageEvent("x", false, false, 1, "", "", globalThis);
  assert(constructed.source === globalThis, "global source");
  assert(constructed.source === self && !("window" in globalThis), "self");
  assert(constructed.type === "x", "global type");
  assert(constructed.data === 1, "global data");
  constructed.initMessageEvent("message", false, false, 1, "", "", self);
  assert(constructed.source === self, "self source");
  assert(
    throwsTypeError(() =>
      constructed.initMessageEvent(
        "x",
        false,
        false,
        1,
        "",
        "",
        new Proxy(globalThis, {}),
      ),
    ),
    "proxy source",
  );
  assert(constructed.source === self, "proxy kept");
  assert(
    throwsTypeError(() =>
      constructed.initMessageEvent(
        "x",
        false,
        false,
        1,
        "",
        "",
        Object.create(globalThis),
      ),
    ),
    "prototype source",
  );
  assert(constructed.type === "message", "prototype type");
  assert(
    throwsTypeError(() =>
      constructed.initMessageEvent("x", false, false, 1, "", "", {}),
    ),
    "object source",
  );
  assert(
    throwsTypeError(() =>
      constructed.initMessageEvent("x", false, false, 1, "", "", 1),
    ),
    "primitive source",
  );
  assert(constructed.type === "message", "primitive type");

  const listed = new Set([channel.port1]);
  constructed.initMessageEvent("m", false, false, null, "", "", null, listed);
  assert(constructed.ports.length === 1, "set length");
  assert(constructed.ports[0] === channel.port1, "set port");
  function* ports() {
    yield channel.port2;
  }
  constructed.initMessageEvent("m", false, false, null, "", "", null, ports());
  assert(constructed.ports[0] === channel.port2, "generator");
  assert(
    throwsTypeError(() =>
      constructed.initMessageEvent("m", false, false, null, "", "", null, null),
    ),
    "null ports",
  );
  assert(constructed.ports[0] === channel.port2, "null ports kept");
  assert(
    throwsTypeError(() =>
      constructed.initMessageEvent("m", false, false, null, "", "", null, [
        channel.port1,
        {},
      ]),
    ),
    "bad element",
  );
  assert(constructed.ports[0] === channel.port2, "bad element kept");

  const target = new EventTarget();
  const held = new MessageEvent("m", { data: "keep", origin: "o" });
  target.addEventListener("m", () => {
    assert(
      throwsTypeError(() =>
        held.initMessageEvent("n", true, true, "x", "p", "q", {}),
      ),
      "dispatch source",
    );
    assert(held.type === "m", "throw type");
    assert(held.data === "keep", "throw data");
    assert(held.origin === "o", "throw origin");
    held.initMessageEvent("n", true, true, "x");
    assert(held.type === "m", "noop type");
    assert(held.data === "keep", "noop data");
  });
  target.dispatchEvent(held);

  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("message")), 1000);
    addEventListener(
      "message",
      (event) => {
        clearTimeout(timer);
        try {
          assert(event.source === null, "posted source");
          assert(event.data === "legacy-init-source", "posted data");
          resolve();
        } catch (error) {
          reject(error);
        }
      },
      { once: true },
    );
    postMessage("legacy-init-source", "*");
  });

  const worker = new Worker(
    new URL("./workers/message_source.js", import.meta.url).href,
  );
  const workerReply = await new Promise<{
    name: string;
    stored: boolean;
    source: boolean;
    port: boolean;
    dedicated: boolean;
  }>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("worker source")), 1000);
    worker.onmessage = (event) => {
      clearTimeout(timer);
      resolve(event.data);
    };
    worker.onerror = (event) => {
      clearTimeout(timer);
      reject(new Error(event.message || "worker error"));
    };
    worker.postMessage("go");
  });
  worker.terminate();
  assert(workerReply.name === "TypeError", JSON.stringify(workerReply));
  assert(workerReply.stored === false, "worker stored");
  assert(workerReply.port === true, "worker port");
});

Yuzuko.test("omitted stream chunks are undefined", async () => {
  const written = [];
  const writable = new WritableStream({
    write(chunk) {
      written.push(chunk);
    },
  });
  const writer = writable.getWriter();
  await writer.write();
  await writer.write(7);
  await writer.close();
  assert(written.length === 2, "written length");
  assert(written[0] === undefined, "omitted write");
  assert(written[1] === 7, "chunk write");

  let controller;
  const transformed = new TransformStream({
    start(next) {
      controller = next;
    },
  });
  const reader = transformed.readable.getReader();
  const pending = reader.read();
  controller.enqueue();
  controller.enqueue("chunk");
  const first = await pending;
  const second = await reader.read();
  assert(first.done === false, "first open");
  assert(first.value === undefined, "omitted enqueue");
  assert(second.done === false, "second open");
  assert(second.value === "chunk", "enqueued chunk");
});
