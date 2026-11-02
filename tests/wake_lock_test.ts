function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

const INVALID_TYPE = "The wake lock type is invalid";

Yuzuko.test("screen wake lock requests a sentinel and release ends it", async () => {
  const window = new Yuzuko.Window({ width: 64, height: 64, title: "wake" });
  const held: WakeLockSentinel[] = [];
  try {
    assert(navigator.wakeLock === navigator.wakeLock, "same wakeLock");
    assert(navigator.wakeLock instanceof WakeLock, "WakeLock");
    assert(WakeLock.prototype.request.length === 0, "request length");
    assert(WakeLockSentinel.prototype.release.length === 0, "release length");

    let illegal = "";
    try {
      new WakeLock();
    } catch (error) {
      illegal = error instanceof TypeError ? error.message : "";
    }
    assert(illegal === "Illegal constructor", `WakeLock: ${illegal}`);
    illegal = "";
    try {
      new WakeLockSentinel();
    } catch (error) {
      illegal = error instanceof TypeError ? error.message : "";
    }
    assert(illegal === "Illegal constructor", `sentinel: ${illegal}`);

    const omitted = await navigator.wakeLock.request();
    held.push(omitted);
    assert(omitted.type === "screen", omitted.type);
    assert(omitted.released === false, "fresh released");
    assert(omitted instanceof WakeLockSentinel, "sentinel");
    assert(omitted instanceof EventTarget, "EventTarget");
    assert(omitted.onrelease === null, "onrelease starts null");

    const explicit = await navigator.wakeLock.request(undefined);
    held.push(explicit);
    assert(explicit !== omitted, "distinct sentinels");
    assert(explicit.type === "screen", "undefined defaults to screen");
    assert(explicit.released === false, "second sentinel");

    const invalid: unknown[] = ["invalid", null, 123, {}, "", true, "Screen"];
    for (const value of invalid) {
      let sync = false;
      let promise: Promise<WakeLockSentinel> | undefined;
      try {
        promise = navigator.wakeLock.request(value as WakeLockType);
      } catch {
        sync = true;
      }
      assert(sync === false, `sync throw for ${String(value)}`);
      let name = "";
      let text = "";
      try {
        await promise;
      } catch (error) {
        name = error instanceof TypeError ? "TypeError" : "";
        text = error instanceof Error ? error.message : "";
      }
      assert(name === "TypeError", `type ${String(value)} ${name}`);
      assert(text === INVALID_TYPE, text);
    }

    let fromListener = 0;
    let seen: Event | null = null;
    omitted.addEventListener("release", (event: Event) => {
      fromListener += 1;
      seen = event;
      assert(omitted.released === true, "released inside listener");
    });
    let fromHandler = 0;
    omitted.onrelease = (event: Event) => {
      fromHandler += 1;
      assert(event.target === omitted, "onrelease target");
      assert(omitted.released === true, "released inside onrelease");
      assert(event.isTrusted === true, "onrelease trusted");
      assert(event.bubbles === false, "onrelease bubbles");
      assert(event.cancelable === false, "onrelease cancelable");
    };
    const pending = omitted.release();
    assert(fromListener === 1, "listener runs before release returns");
    assert(fromHandler === 1, "onrelease runs before release returns");
    assert(seen !== null, "event");
    assert(seen!.type === "release", seen!.type);
    assert(seen!.target === omitted, "target");
    assert(seen!.isTrusted === true, "trusted");
    assert(seen!.bubbles === false, "bubbles");
    assert(seen!.cancelable === false, "cancelable");
    const resolved = await pending;
    assert(resolved === undefined, "release resolves undefined");
    assert(omitted.released === true, "stays released");
    assert(explicit.released === false, "other sentinel stays held");
    const again = await omitted.release();
    assert(again === undefined, "second release");
    assert(fromListener === 1, "release fires once");
    assert(fromHandler === 1, "onrelease fires once");

    window.hide();
    assert(explicit.released === false, "hiding the window keeps the sentinel");
    const afterHide = await explicit.release();
    assert(afterHide === undefined, "release after hide");
    assert(explicit.released === true, "released after hide");
  } finally {
    for (const sentinel of held) {
      if (!sentinel.released) {
        await sentinel.release();
      }
    }
    window.close();
  }
});

Yuzuko.test("a dedicated worker has no screen wake lock", async () => {
  const worker = new Worker(
    new URL("./workers/wake_lock.js", import.meta.url).href,
  );
  const probe = await new Promise<{ wakeLock: boolean; WakeLock: boolean }>(
    (resolve, reject) => {
      worker.onmessage = (event: MessageEvent) => {
        resolve(event.data as { wakeLock: boolean; WakeLock: boolean });
      };
      worker.onerror = (event: ErrorEvent) => {
        reject(new Error(event.message || "worker error"));
      };
    },
  );
  worker.terminate();
  assert(probe.wakeLock === false, "worker navigator.wakeLock");
  assert(probe.WakeLock === false, "worker WakeLock");
});
