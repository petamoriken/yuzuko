function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

// A `let` binding is narrowed to the literal just written. The type argument keeps the wider type.
function read<T>(value: T): T {
  return value;
}

function assertThrows(
  run: () => unknown,
  kind: "type" | "dom",
  name: string,
  message: string,
): void {
  let thrown: unknown;
  try {
    run();
  } catch (error) {
    thrown = error;
  }
  if (thrown === undefined) {
    throw new Error(`expected throw: ${message}`);
  }
  if (kind === "dom") {
    assert(thrown instanceof DOMException, `not DOMException: ${message}`);
    assert(thrown.name === name, `${thrown.name} !== ${name}`);
  } else {
    assert(thrown instanceof TypeError, `not TypeError: ${message}`);
  }
  assert(thrown.message === message, thrown.message);
}

Yuzuko.test("an accessible root is one authored tree", () => {
  assert(
    // @ts-expect-error AccessibleNode is absent on the main global.
    globalThis.AccessibleNode === undefined,
    "global constructor",
  );
  assert(typeof Yuzuko.AccessibleNode === "function", "constructor");
  assert(Yuzuko.AccessibleNode.name === "AccessibleNode", "name");
  assert(Yuzuko.AccessibleNode.length === 0, "constructor length");
  let bare: unknown;
  try {
    bare = Function("return AccessibleNode")();
  } catch (error) {
    bare = error;
  }
  assert(bare instanceof ReferenceError, "bare name");

  const sample = new Yuzuko.AccessibleNode();
  assert(sample instanceof Yuzuko.AccessibleNode, "instanceof");
  assert(sample instanceof EventTarget, "EventTarget");
  assert(sample.appendChild.length === 1, "appendChild length");
  assert(sample.insertBefore.length === 1, "insertBefore length");
  assert(sample.replaceChild.length === 2, "replaceChild length");
  assert(sample.removeChild.length === 1, "removeChild length");
  assert(sample.focus.length === 0, "focus length");
  assert(sample.parent === null, "detached parent");
  assert(sample.focusable === false, "focusable default");

  const attach = Yuzuko.WindowSurface.prototype.attachAccessibleRoot;
  assert(attach.length === 0, "attach length");
  assertThrows(
    () => attach.call({}),
    "type",
    "",
    "illegal invocation",
  );
  const descriptor = Object.getOwnPropertyDescriptor(
    Yuzuko.WindowSurface.prototype,
    "accessibleRoot",
  );
  assert(descriptor?.get !== undefined, "accessibleRoot getter");
  assertThrows(
    () => descriptor.get?.call({}),
    "type",
    "",
    "illegal invocation",
  );

  const window = new Yuzuko.Window({ width: 200, height: 100, title: "tree" });
  const closed = new Yuzuko.Window({ width: 80, height: 40, title: "closed" });
  try {
    closed.close();
    assertThrows(
      () => closed.surface.attachAccessibleRoot(),
      "dom",
      "InvalidStateError",
      "The window is closed.",
    );
    assertThrows(
      () => closed.surface.accessibleRoot,
      "dom",
      "InvalidStateError",
      "The window is closed.",
    );

    let accessibleRoot: Yuzuko.AccessibleNode | null =
      window.surface.accessibleRoot;
    assert(accessibleRoot === null, "no root yet");
    const root = window.surface.attachAccessibleRoot();
    assert(window.surface.accessibleRoot === root, "getter returns the root");
    assert(root.parent === null, "root parent");
    assertThrows(
      () => window.surface.attachAccessibleRoot(),
      "dom",
      "InvalidStateError",
      "An accessible root is already attached.",
    );

    const a = new Yuzuko.AccessibleNode();
    const b = new Yuzuko.AccessibleNode();
    const c = new Yuzuko.AccessibleNode();
    assert(root.appendChild(a) === a, "append returns the node");
    assert(root.insertBefore(b, a) === b, "insert returns the node");
    assert(root.firstChild === b, "firstChild");
    assert(root.lastChild === a, "lastChild");
    assert(b.nextSibling === a, "nextSibling");
    assert(a.previousSibling === b, "previousSibling");
    assert(a.parent === root, "parent");
    root.insertBefore(b, b);
    assert(root.firstChild === b, "insertBefore self stays put");
    assert(b.nextSibling === a, "sibling after self insert");

    assertThrows(
      () => root.insertBefore(root, root),
      "dom",
      "NotFoundError",
      "The node is not a child.",
    );
    assertThrows(
      () => root.insertBefore(root, null),
      "dom",
      "HierarchyRequestError",
      "The accessible root cannot be moved.",
    );
    assertThrows(
      () => a.appendChild(root),
      "dom",
      "HierarchyRequestError",
      "The accessible root cannot be moved.",
    );
    assertThrows(
      () => a.appendChild(a),
      "dom",
      "HierarchyRequestError",
      "The node is an ancestor.",
    );
    root.appendChild(c);
    b.appendChild(c);
    assert(c.parent === b, "move detaches the previous parent");
    assert(root.lastChild === a, "moved node left its old parent");
    assertThrows(
      () => c.appendChild(b),
      "dom",
      "HierarchyRequestError",
      "The node is an ancestor.",
    );
    assert(b.replaceChild(c, c) === c, "replace with itself");
    assert(c.parent === b, "replace with itself keeps the child");
    const d = new Yuzuko.AccessibleNode();
    assert(b.replaceChild(d, c) === c, "replace returns the removed child");
    assert(c.parent === null, "replaced child is detached");
    assert(d.parent === b, "replacement is attached");
    assert(b.removeChild(d) === d, "remove returns the child");
    assert(d.parent === null, "removed child is detached");
    assertThrows(
      () => b.removeChild(d),
      "dom",
      "NotFoundError",
      "The node is not a child.",
    );
    assertThrows(
      () => {
        // @ts-expect-error A number is not an AccessibleNode.
        return root.appendChild(1);
      },
      "type",
      "",
      "The node must be an AccessibleNode.",
    );
    assertThrows(
      () => {
        // @ts-expect-error The node argument is required.
        return root.appendChild();
      },
      "type",
      "",
      "Not enough arguments",
    );

    assertThrows(
      () => a.focus(),
      "dom",
      "InvalidStateError",
      "The node is not focusable.",
    );
    a.focusable = true;
    const loose = new Yuzuko.AccessibleNode();
    loose.focusable = true;
    assertThrows(
      () => loose.focus(),
      "dom",
      "InvalidStateError",
      "The node is not connected.",
    );
    let saw = false;
    const mark = () => {
      saw = true;
    };
    a.addEventListener("accessiblefocus", mark);
    window.addEventListener("focus", mark);
    window.addEventListener("blur", mark);
    a.focus();
    assert(saw === false, "script focus dispatches no focus event");
    a.removeEventListener("accessiblefocus", mark);
    window.removeEventListener("focus", mark);
    window.removeEventListener("blur", mark);

    window.close();
    assertThrows(
      () => window.surface.accessibleRoot,
      "dom",
      "InvalidStateError",
      "The window is closed.",
    );
    assertThrows(
      () => window.surface.attachAccessibleRoot(),
      "dom",
      "InvalidStateError",
      "The window is closed.",
    );
    b.appendChild(d);
    assert(d.parent === b, "edits still work after close");
    assertThrows(
      () => a.focus(),
      "dom",
      "InvalidStateError",
      "The window is closed.",
    );
  } finally {
    window.close();
    closed.close();
  }
});

Yuzuko.test("accessible properties and offsets round-trip", () => {
  const window = new Yuzuko.Window({
    width: 160,
    height: 90,
    title: "props",
  });
  try {
    const root = window.surface.attachAccessibleRoot();
    const mid = new Yuzuko.AccessibleNode();
    const child = new Yuzuko.AccessibleNode();
    const sibling = new Yuzuko.AccessibleNode();
    root.appendChild(mid);
    mid.appendChild(child);
    root.appendChild(sibling);
    const node = new Yuzuko.AccessibleNode();

    assert(node.role === null, "role starts null");
    node.role = "";
    assert(node.role === "", "empty role");
    node.role = "button";
    assert(node.role === "button", "role");
    node.role = null;
    assert(node.role === null, "role cleared");
    node.name = "";
    assert(node.name === "", "empty name");
    node.name = null;
    assert(node.name === null, "name cleared");

    node.checked = true;
    let checked: string | boolean | null = read<string | boolean | null>(node.checked);
    assert(checked === "true", "checked true");
    node.checked = false;
    checked = read<string | boolean | null>(node.checked);
    assert(checked === "false", "checked false");
    node.checked = "mixed";
    assert(node.checked === "mixed", "checked mixed");
    assertThrows(
      () => {
        // @ts-expect-error An invalid checked token is rejected.
        node.checked = "nope";
      },
      "type",
      "",
      "The value is not a valid token.",
    );
    assert(node.checked === "mixed", "invalid token is ignored");
    node.checked = null;
    assert(node.checked === null, "checked cleared");

    node.valueNow = 2;
    assert(node.valueNow === 2, "valueNow");
    assertThrows(
      () => {
        node.valueNow = Infinity;
      },
      "type",
      "",
      "The value must be a finite number.",
    );
    assert(node.valueNow === 2, "non-finite value is ignored");
    node.valueNow = null;
    assert(node.valueNow === null, "valueNow cleared");

    node.disabled = true;
    assert(node.disabled === true, "disabled");
    node.disabled = null;
    assert(node.disabled === null, "disabled cleared");
    node.readOnly = true;
    assert(node.readOnly === true, "readOnly");
    node.multiline = false;
    assert(node.multiline === false, "multiline");
    node.live = "polite";
    assert(node.live === "polite", "live");
    assertThrows(
      () => {
        // @ts-expect-error An invalid live token is rejected.
        node.live = "nope";
      },
      "type",
      "",
      "The value is not a valid token.",
    );
    node.orientation = "horizontal";
    assert(node.orientation === "horizontal", "orientation");
    node.rowIndex = 1;
    assert(node.rowIndex === 1, "rowIndex");
    node.rowIndex = 0;
    assert(node.rowIndex === 0, "rowIndex zero");
    node.rowIndex = null;
    assert(node.rowIndex === null, "rowIndex cleared");
    node.colSpan = 2;
    assert(node.colSpan === 2, "colSpan");
    node.level = 0;
    assert(node.level === 0, "level zero");
    node.posInSet = -1;
    assert(node.posInSet === -1, "negative posInSet is stored");
    node.focusable = true;
    assert(node.focusable === true, "focusable");

    node.offsetLeft = 30;
    let offsetLeft: string | number | null = read<string | number | null>(node.offsetLeft);
    assert(offsetLeft === "30px", "numeric offset");
    node.offsetLeft = "30px";
    assert(node.offsetLeft === "30px", "px offset");
    node.offsetLeft = "30";
    assert(node.offsetLeft === "30px", "bare offset");
    node.offsetLeft = "30.5";
    assert(node.offsetLeft === "30.5px", "fractional offset");
    assertThrows(
      () => {
        node.offsetLeft = "30em";
      },
      "type",
      "",
      "The offset must be a CSS pixel length.",
    );
    assertThrows(
      () => {
        node.offsetLeft = "30 px";
      },
      "type",
      "",
      "The offset must be a CSS pixel length.",
    );
    assert(node.offsetLeft === "30.5px", "rejected offset is ignored");
    node.offsetLeft = null;
    assert(node.offsetLeft === null, "offset cleared");

    child.offsetParent = root;
    assert(child.offsetParent === root, "offset parent");
    assertThrows(
      () => {
        child.offsetParent = child;
      },
      "dom",
      "HierarchyRequestError",
      "The offset parent is not an ancestor.",
    );
    assertThrows(
      () => {
        child.offsetParent = sibling;
      },
      "dom",
      "HierarchyRequestError",
      "The offset parent is not an ancestor.",
    );
    assertThrows(
      () => {
        // @ts-expect-error An empty object is not an AccessibleNode.
        child.offsetParent = {};
      },
      "type",
      "",
      "The offset parent must be an AccessibleNode.",
    );
    assert(child.offsetParent === root, "rejected offset parent is ignored");
    child.offsetParent = null;
    assert(child.offsetParent === null, "offset parent cleared");
  } finally {
    window.close();
  }
});

Yuzuko.test("a dedicated worker has no AccessibleNode", async () => {
  const worker = new Worker(
    new URL("./workers/accessible.js", import.meta.url),
    { type: "module" },
  );
  try {
    const kind = await new Promise<string>((resolve, reject) => {
      worker.onmessage = (event: MessageEvent) => {
        resolve(String(event.data));
      };
      worker.onerror = () => {
        reject(new Error("worker error"));
      };
    });
    assert(kind === "undefined undefined", kind);
  } finally {
    worker.terminate();
  }
});

Yuzuko.test("a collected AccessibleNode drops its weak handle", () => {
  Yuzuko.__test.collectGarbage();
  const before = Yuzuko.__test.accessibleNodeCount();
  for (let i = 0; i < 32; i++) {
    new Yuzuko.AccessibleNode().role = "button";
  }
  Yuzuko.__test.collectGarbage();
  const after = Yuzuko.__test.accessibleNodeCount();
  assert(after === before, `nodes ${before} -> ${after}`);
});

Yuzuko.test("closing a window keeps the root until it is collected", () => {
  const window = new Yuzuko.Window({ width: 40, height: 30, title: "keep" });
  try {
    const root = window.surface.attachAccessibleRoot();
    const before = Yuzuko.__test.accessibleWindowCount();
    window.close();
    Yuzuko.__test.collectGarbage();
    assert(
      Yuzuko.__test.accessibleWindowCount() === before,
      "closed root was dropped",
    );
    const child = new Yuzuko.AccessibleNode();
    root.appendChild(child);
    assert(root.firstChild === child, "edit after close");
  } finally {
    window.close();
  }
});

function abandonAccessibleWindow(): void {
  const window = new Yuzuko.Window({ width: 40, height: 30, title: "drop" });
  window.surface.attachAccessibleRoot().appendChild(new Yuzuko.AccessibleNode());
  window.close();
}

// Counts taken before the window in the next test is abandoned. Windows
// keeps that window reachable until the allocating frame returns, so the
// following test is what observes the collection.
let abandonedWindows = 0;
let abandonedNodes = 0;

Yuzuko.test("a closed window stays recorded until its frame returns", () => {
  Yuzuko.__test.collectGarbage();
  Yuzuko.__test.collectGarbage();
  abandonedWindows = Yuzuko.__test.accessibleWindowCount();
  abandonedNodes = Yuzuko.__test.accessibleNodeCount();
  abandonAccessibleWindow();
  assert(
    Yuzuko.__test.accessibleWindowCount() === abandonedWindows + 1,
    `windows ${abandonedWindows} -> ${Yuzuko.__test.accessibleWindowCount()}`,
  );
  assert(
    Yuzuko.__test.accessibleNodeCount() === abandonedNodes + 2,
    `nodes ${abandonedNodes} -> ${Yuzuko.__test.accessibleNodeCount()}`,
  );
});

Yuzuko.test("a closed window is dropped after its root is collected", () => {
  Yuzuko.__test.collectGarbage();
  Yuzuko.__test.collectGarbage();
  assert(
    Yuzuko.__test.accessibleWindowCount() === abandonedWindows,
    `windows left ${Yuzuko.__test.accessibleWindowCount()}`,
  );
  assert(
    Yuzuko.__test.accessibleNodeCount() === abandonedNodes,
    `nodes left ${Yuzuko.__test.accessibleNodeCount()}`,
  );
});
