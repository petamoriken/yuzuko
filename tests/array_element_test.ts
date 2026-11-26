// Host-made arrays define their elements. A setter on Array.prototype must
// not receive them, and the element must not be a hole.

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function withPoisonedIndex<T>(run: () => T): { result: T; leaked: unknown[] } {
  const leaked: unknown[] = [];
  Object.defineProperty(Array.prototype, "0", {
    configurable: true,
    get() {
      return undefined;
    },
    set(value) {
      leaked.push(value);
    },
  });
  try {
    return { result: run(), leaked };
  } finally {
    delete (Array.prototype as unknown as Record<string, unknown>)["0"];
  }
}

function ownFirst(array: ArrayLike<unknown>, label: string): unknown {
  assert(Object.hasOwn(array, 0), `${label} element 0 is a hole`);
  return array[0];
}

Yuzuko.test("host arrays define elements past an Array.prototype setter", () => {
  const ctx = new OffscreenCanvas(1, 1).getContext("2d")!;
  ctx.setLineDash([5, 3]);
  const dash = withPoisonedIndex(() => ctx.getLineDash());
  assert(dash.leaked.length === 0, "getLineDash leaked");
  assert(ownFirst(dash.result, "getLineDash") === 5, "getLineDash value");

  const target = new EventTarget();
  let path: EventTarget[] = [];
  target.addEventListener("x", (event) => {
    path = withPoisonedIndex(() => event.composedPath()).result;
  });
  target.dispatchEvent(new Event("x"));
  assert(ownFirst(path, "composedPath") === target, "composedPath value");

  const entries = withPoisonedIndex(() => Yuzuko.readDirSync("."));
  assert(entries.leaked.length === 0, "readDir leaked");
  assert(typeof (ownFirst(entries.result, "readDir") as { name: string }).name === "string", "readDir value");

  const channel = new MessageChannel();
  const ports = withPoisonedIndex(() =>
    new MessageEvent("message", { ports: [channel.port1] }).ports
  );
  assert(ports.leaked.length === 0, "ports leaked");
  assert(ownFirst(ports.result, "ports") === channel.port1, "ports value");
  channel.port1.close();
  channel.port2.close();

  const pairs = withPoisonedIndex(() => [...new Headers({ a: "1" })]);
  assert(pairs.leaked.length === 0, "headers leaked");
  assert(ownFirst(pairs.result[0]!, "headers pair") === "a", "headers value");
});
