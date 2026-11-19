function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function read(target: object, name: string): unknown {
  // @ts-expect-error Handler names are looked up as strings.
  return target[name];
}

function write(target: object, name: string, value: unknown): void {
  // @ts-expect-error Handler names are assigned as strings.
  target[name] = value;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "";
}

function identical(left: unknown, right: unknown): boolean {
  return left === right;
}

const WINDOW_ONLY = [
  "close",
  "resize",
  "focus",
  "blur",
  "menuclick",
  "keydown",
  "keyup",
  "wheel",
  "compositionstart",
  "compositionupdate",
  "compositionend",
  "pointerlockchange",
  "pointerlockerror",
];

const SURFACE_ONLY = [
  "visibilitychange",
  "beforeinput",
  "copy",
  "cut",
  "paste",
];

const ACCESSIBLE = [
  "accessibleclick",
  "accessiblecontextmenu",
  "accessibledecrement",
  "accessiblefocus",
  "accessibleincrement",
  "accessiblescrollintoview",
  "accessiblesetvalue",
];

const SHARED = [
  "dragenter",
  "dragover",
  "dragleave",
  "drop",
  "pointerdown",
  "pointerup",
  "pointermove",
  "pointerrawupdate",
  "pointercancel",
  "pointerover",
  "pointerout",
  "pointerenter",
  "pointerleave",
  "gotpointercapture",
  "lostpointercapture",
  "touchstart",
  "touchmove",
  "touchend",
  "touchcancel",
  "mousedown",
  "mouseup",
  "mousemove",
  "mouseover",
  "mouseout",
  "mouseenter",
  "mouseleave",
  "click",
  "dblclick",
  "auxclick",
  "contextmenu",
];

function expectHandler(prototype: object, event: string) {
  const name = `on${event}`;
  const descriptor = Object.getOwnPropertyDescriptor(prototype, name);
  assert(descriptor !== undefined, name);
  assert(descriptor.enumerable === true, `${name} enumerable`);
  assert(descriptor.configurable === true, `${name} configurable`);
  assert(typeof descriptor.get === "function", `${name} get`);
  assert(typeof descriptor.set === "function", `${name} set`);
  assert(descriptor.get.name === `get ${name}`, descriptor.get.name);
  assert(descriptor.set.name === `set ${name}`, descriptor.set.name);
  assert(descriptor.get.length === 0, `${name} get length`);
  assert(descriptor.set.length === 1, `${name} set length`);
}

function expectOwn(target: object, event: string) {
  const name = `on${event}`;
  const descriptor = Object.getOwnPropertyDescriptor(target, name);
  assert(descriptor !== undefined, name);
  assert(descriptor.enumerable === true, `${name} enumerable`);
  assert(descriptor.configurable === true, `${name} configurable`);
  assert(typeof descriptor.get === "function", `${name} get`);
  assert(typeof descriptor.set === "function", `${name} set`);
  assert(descriptor.get.name === `get ${name}`, descriptor.get.name);
  assert(descriptor.set.name === `set ${name}`, descriptor.set.name);
  const prototype: unknown = Object.getPrototypeOf(target);
  assert(prototype !== null, `${name} prototype`);
  assert(
    Object.getOwnPropertyDescriptor(prototype, name) === undefined,
    `${name} stays off the prototype`,
  );
  assert(read(target, name) === null, `${name} null`);
}

Yuzuko.test("event handler attributes follow the listener rules", () => {
  const window = new Yuzuko.Window({
    width: 80,
    height: 60,
    title: "handlers",
  });
  let reported = "";
  const onError = (event: Event) => {
    if (event instanceof ErrorEvent) {
      reported = event.message;
    }
    event.preventDefault();
  };
  globalThis.addEventListener("error", onError);
  try {
    for (const name of ["onfiledrop", "onfilehover", "onfilehovercancel"]) {
      assert(
        Object.getOwnPropertyDescriptor(Yuzuko.Window.prototype, name) ===
          undefined,
        name,
      );
    }
    for (const event of WINDOW_ONLY) {
      expectHandler(Yuzuko.Window.prototype, event);
      assert(read(window, `on${event}`) === null, `${event} null`);
      assert(
        Object.getOwnPropertyDescriptor(
          Yuzuko.WindowSurface.prototype,
          `on${event}`,
        ) === undefined,
        `${event} stays off the surface`,
      );
    }
    for (const event of SURFACE_ONLY) {
      expectHandler(Yuzuko.WindowSurface.prototype, event);
      assert(read(window.surface, `on${event}`) === null, `${event} null`);
      assert(
        Object.getOwnPropertyDescriptor(
          Yuzuko.Window.prototype,
          `on${event}`,
        ) === undefined,
        `${event} stays off the window`,
      );
    }
    for (const event of SHARED) {
      expectHandler(Yuzuko.Window.prototype, event);
      expectHandler(Yuzuko.WindowSurface.prototype, event);
      assert(read(window, `on${event}`) === null, `window ${event}`);
      assert(read(window.surface, `on${event}`) === null, `surface ${event}`);
    }
    assert(
      Object.getOwnPropertyDescriptor(EventTarget.prototype, "onclick") ===
        undefined,
      "EventTarget",
    );
    assert(read(new EventTarget(), "onclick") === undefined, "plain target");
    assert(typeof Yuzuko.AccessibleNode === "function", "AccessibleNode");
    const accessible = new Yuzuko.AccessibleNode();
    for (const event of ACCESSIBLE) {
      expectHandler(Yuzuko.AccessibleNode.prototype, event);
      assert(read(accessible, `on${event}`) === null, event);
    }
    assert(
      Object.getOwnPropertyDescriptor(window, "onclick") === undefined,
      "onclick is not an own property",
    );

    expectHandler(Yuzuko.Tray.prototype, "click");
    expectHandler(Yuzuko.Tray.prototype, "menuclick");
    expectOwn(Yuzuko.dock, "menuclick");
    expectOwn(Yuzuko.dock, "reopen");
    expectOwn(Yuzuko, "menuclick");

    const click = Object.getOwnPropertyDescriptor(
      Yuzuko.Window.prototype,
      "onclick",
    );
    if (click?.get === undefined || click.set === undefined) {
      throw new Error("onclick accessor");
    }
    assert(click.get.call(window) === null, "prototype get");
    let invoked = "";
    try {
      click.get.call({});
    } catch (error) {
      invoked = message(error);
    }
    assert(invoked === "illegal invocation", invoked);
    invoked = "";
    try {
      click.set.call(Yuzuko.Window.prototype, null);
    } catch (error) {
      invoked = message(error);
    }
    assert(invoked === "illegal invocation", invoked);
    invoked = "";
    try {
      read(Yuzuko.Window.prototype, "onclick");
    } catch (error) {
      invoked = message(error);
    }
    assert(invoked === "illegal invocation", `prototype read ${invoked}`);

    let calls = 0;
    const listener = () => {
      calls += 1;
    };
    window.onresize = listener;
    assert(window.onresize === listener, "stored");
    assert(
      Object.getOwnPropertyDescriptor(window, "onresize") === undefined,
      "assignment stays on the prototype",
    );
    window.removeEventListener("resize", listener);
    assert(window.onresize === listener, "attribute remains");
    window.dispatchEvent(new Event("resize"));
    assert(calls === 0, `removed ${calls}`);
    window.onresize = listener;
    window.dispatchEvent(new Event("resize"));
    assert(calls === 1, `restored ${calls}`);
    window.onresize = null;

    calls = 0;
    window.addEventListener("keyup", listener);
    window.onkeyup = listener;
    window.dispatchEvent(new KeyboardEvent("keyup"));
    assert(calls === 1, `shared ${calls}`);
    window.onkeyup = null;

    calls = 0;
    window.onauxclick = listener;
    window.addEventListener("auxclick", listener);
    window.dispatchEvent(new MouseEvent("auxclick"));
    assert(calls === 1, `assigned first ${calls}`);
    window.onauxclick = null;
    window.removeEventListener("auxclick", listener);

    let first = 0;
    let second = 0;
    window.onkeydown = () => {
      first += 1;
    };
    window.onkeydown = () => {
      second += 1;
    };
    window.dispatchEvent(new KeyboardEvent("keydown"));
    assert(first === 0 && second === 1, `${first} ${second}`);
    window.onkeydown = null;

    calls = 0;
    window.onwheel = listener;
    // @ts-expect-error A number is not a listener.
    window.onwheel = 1;
    assert(window.onwheel === null, "number");
    write(window, "onwheel", undefined);
    assert(window.onwheel === null, "undefined");
    window.onwheel = null;
    window.dispatchEvent(new WheelEvent("wheel"));
    assert(calls === 0, `cleared ${calls}`);

    calls = 0;
    window.addEventListener("compositionend", listener, true);
    window.oncompositionend = listener;
    window.oncompositionend = null;
    window.dispatchEvent(new Event("compositionend"));
    assert(calls === 1, `capture kept ${calls}`);
    window.removeEventListener("compositionend", listener, true);

    let prevented = false;
    window.oncontextmenu = (event) => {
      event.preventDefault();
      prevented = event.defaultPrevented;
    };
    window.dispatchEvent(
      new MouseEvent("contextmenu", { cancelable: true }),
    );
    assert(prevented, "handler is not passive");
    window.oncontextmenu = null;

    calls = 0;
    window.onpointerlockchange = () => {
      calls += 1;
    };
    window.dispatchEvent(new Event("pointerlockchange"));
    window.dispatchEvent(new Event("pointerlockchange"));
    assert(calls === 2, `not once ${calls}`);
    window.onpointerlockchange = null;

    window.onblur = () => {
      throw new Error("handler blew up");
    };
    window.dispatchEvent(new FocusEvent("blur"));
    assert(reported === "handler blew up", reported);
    window.onblur = null;

    let seen = "";
    Yuzuko.onmenuclick = function (event) {
      assert(identical(event.currentTarget, Yuzuko), "menu current");
      seen = event.type;
    };
    assert(Yuzuko.onmenuclick !== null, "menu stored");
    Yuzuko.dispatchEvent(new Event("menuclick"));
    assert(seen === "menuclick", seen);
    Yuzuko.onmenuclick = null;
    assert(Yuzuko.onmenuclick === null, "menu cleared");

    let reopened = false;
    Yuzuko.dock.onreopen = function (event) {
      assert(this === Yuzuko.dock, "dock this");
      reopened = event.type === "reopen";
    };
    Yuzuko.dock.dispatchEvent(new Event("reopen"));
    assert(reopened, "reopen");
    Yuzuko.dock.onreopen = null;
    Yuzuko.dock.onmenuclick = () => {
      seen = "dock";
    };
    Yuzuko.dock.dispatchEvent(new Event("menuclick"));
    assert(seen === "dock", seen);
    Yuzuko.dock.onmenuclick = null;
  } finally {
    globalThis.removeEventListener("error", onError);
    Yuzuko.onmenuclick = null;
    Yuzuko.dock.onmenuclick = null;
    Yuzuko.dock.onreopen = null;
    window.close();
  }
});

Yuzuko.test("host events call the handler attribute", () => {
  assert(typeof Yuzuko.__test.inject === "function", "inject");
  const window = new Yuzuko.Window({
    width: 120,
    height: 80,
    title: "handler events",
  });
  const tray = new Yuzuko.Tray();
  try {
    let key = "";
    window.onkeydown = function (event) {
      assert(this === window, "key this");
      key = event.key;
    };
    Yuzuko.__test.inject(window, [
      { kind: "key", type: "keydown", key: "a", code: "KeyA" },
    ]);
    assert(key === "a", key);

    let focused = false;
    window.onfocus = function (event) {
      assert(this === window, "focus this");
      assert(event instanceof FocusEvent, "focus class");
      focused = true;
    };
    Yuzuko.__test.inject(window, [{ kind: "focus" }]);
    assert(focused, "focus");

    let hovered = "";
    window.surface.ondragenter = function (event) {
      assert(this === window.surface, "drag this");
      assert(event.target === window.surface, "drag target");
      assert(event.eventPhase === Event.AT_TARGET, "drag phase");
      assert(event.bubbles === true, "drag bubbles");
      assert(event instanceof DragEvent, "drag class");
      hovered = event.dataTransfer?.items[0]?.type ?? "";
    };
    Yuzuko.__test.inject(window, [
      { kind: "filehover", paths: ["rel.txt"] },
    ]);
    assert(hovered === "text/plain", hovered);

    let state = "";
    window.surface.onvisibilitychange = function () {
      assert(this === window.surface, "visibility this");
      state = window.surface.visibilityState;
    };
    Yuzuko.__test.inject(window, [{ kind: "visibility", state: "hidden" }]);
    assert(state === "hidden", state);

    const touchOrder: string[] = [];
    window.surface.ontouchstart = function (event) {
      assert(this === window.surface, "touch surface");
      assert(event.eventPhase === Event.AT_TARGET, "touch target phase");
      assert(event instanceof TouchEvent, "touch class");
      touchOrder.push("surface");
    };
    window.ontouchstart = function (event) {
      assert(this === window, "touch window");
      assert(event.eventPhase === Event.BUBBLING_PHASE, "touch bubble phase");
      touchOrder.push("window");
    };
    Yuzuko.__test.inject(window, [
      {
        kind: "pointer",
        type: "pointerdown",
        pointerType: "touch",
        pointerId: 7,
        x: 4,
        y: 5,
      },
    ]);
    assert(touchOrder.join(",") === "surface,window", touchOrder.join(","));

    const clickOrder: string[] = [];
    window.surface.onclick = (event) => {
      assert(event.eventPhase === Event.AT_TARGET, "click target");
      clickOrder.push("surface");
    };
    window.onclick = (event) => {
      assert(event.eventPhase === Event.BUBBLING_PHASE, "click bubble");
      clickOrder.push("window");
    };
    Yuzuko.__test.inject(window, [
      { kind: "pointer", type: "pointerdown", x: 3, y: 4 },
      { kind: "pointer", type: "pointerup", x: 3, y: 4 },
    ]);
    assert(clickOrder.join(",") === "surface,window", clickOrder.join(","));

    clickOrder.length = 0;
    window.surface.onclick = (event) => {
      event.stopPropagation();
      clickOrder.push("surface");
    };
    window.onclick = () => {
      clickOrder.push("window");
    };
    Yuzuko.__test.inject(window, [
      { kind: "pointer", type: "pointerdown", x: 3, y: 4 },
      { kind: "pointer", type: "pointerup", x: 3, y: 4 },
    ]);
    assert(clickOrder.join(",") === "surface", clickOrder.join(","));

    const rawOrder: string[] = [];
    window.surface.onpointerrawupdate = (event) => {
      assert(event.type === "pointerrawupdate", event.type);
      assert(event.eventPhase === Event.AT_TARGET, "raw target");
      rawOrder.push("surface");
    };
    window.onpointerrawupdate = (event) => {
      assert(event.eventPhase === Event.BUBBLING_PHASE, "raw bubble");
      rawOrder.push("window");
    };
    Yuzuko.__test.inject(window, [
      { kind: "pointer", type: "pointermove", x: 8, y: 9 },
    ]);
    assert(rawOrder.join(",") === "surface,window", rawOrder.join(","));

    let clicks = 0;
    tray.onclick = function (event) {
      assert(this === tray, "tray this");
      assert(event.type === "click", event.type);
      clicks += 1;
    };
    Yuzuko.__test.inject(window, [{ kind: "trayclick" }]);
    assert(clicks === 1, `tray ${clicks}`);

    let menus = 0;
    tray.onmenuclick = function () {
      assert(this === tray, "tray menu this");
      menus += 1;
    };
    tray.dispatchEvent(new Event("menuclick"));
    assert(menus === 1, `tray menu ${menus}`);
  } finally {
    tray.close();
    window.close();
  }
});
