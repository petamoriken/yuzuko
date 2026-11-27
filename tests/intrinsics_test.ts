function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

// Each test replaces a global for its own body and restores it after.
function replacing<T>(name: string, value: unknown, body: () => T): T {
  const realm = globalThis as unknown as Record<string, unknown>;
  const original = Object.getOwnPropertyDescriptor(realm, name);
  Object.defineProperty(realm, name, {
    value,
    writable: true,
    configurable: true,
  });
  try {
    return body();
  } finally {
    if (original) {
      Object.defineProperty(realm, name, original);
    }
  }
}

Yuzuko.test("Response.json uses the built-in JSON.stringify", async () => {
  const response = replacing(
    "JSON",
    { stringify: () => '"forged"' },
    () => Response.json({ a: 1 }),
  );
  assert((await response.text()) === '{"a":1}', "body");
});

Yuzuko.test("Request and pipeTo use the built-in AbortSignal", async () => {
  const controller = new AbortController();
  const request = replacing("AbortSignal", undefined, () =>
    new Request("https://example.test/", { signal: controller.signal }),
  );
  assert(request.signal instanceof AbortSignal, "request signal");
  const piped = replacing("AbortSignal", undefined, () =>
    new ReadableStream().pipeTo(new WritableStream(), {
      signal: controller.signal,
    }),
  );
  controller.abort();
  let name = "";
  try {
    await piped;
  } catch (error) {
    name = (error as Error).name;
  }
  assert(name === "AbortError", name);
});

Yuzuko.test("a WritableStream controller gets a real AbortSignal", async () => {
  let signal: unknown;
  replacing("AbortController", function Forged() {
    return { signal: { forged: true }, abort() {} };
  }, () =>
    new WritableStream({
      start(controller) {
        signal = controller.signal;
      },
    }),
  );
  assert(signal instanceof AbortSignal, "controller.signal");
});

Yuzuko.test("getRandomValues throws the built-in QuotaExceededError", () => {
  let thrown: unknown;
  replacing("QuotaExceededError", function Forged() {
    return { forged: true };
  }, () => {
    try {
      crypto.getRandomValues(new Uint8Array(65537));
    } catch (error) {
      thrown = error;
    }
  });
  assert(thrown instanceof DOMException, String(thrown));
  assert((thrown as DOMException).name === "QuotaExceededError", "name");
});

Yuzuko.test("Cache.put builds its Request with the built-in", async () => {
  const cache = await caches.open("intrinsics");
  await replacing("Request", function Forged() {
    throw new Error("forged Request");
  }, () => cache.put("https://example.test/a", new Response("stored")));
  const matched = await cache.match("https://example.test/a");
  assert((await matched?.text()) === "stored", "stored body");
});
