Yuzuko.test("events survive prototype pollution", () => {
  Array.prototype.push = () => {
    throw new Error("polluted push");
  };
  WeakMap.prototype.get = () => {
    throw new Error("polluted get");
  };
  const target = new EventTarget();
  let seen = false;
  target.addEventListener("go", () => {
    seen = true;
  });
  target.dispatchEvent(new Event("go"));
  if (!seen) {
    throw new Error("listener did not run");
  }
});

Yuzuko.test("abort timeout uses the original setTimeout", async () => {
  const realSetTimeout = setTimeout;
  globalThis.setTimeout = () => {
    throw new Error("polluted setTimeout");
  };
  const signal = AbortSignal.timeout(20);
  await new Promise((resolve) => realSetTimeout(resolve, 80));
  if (!signal.aborted) {
    throw new Error("signal did not abort");
  }
  if (signal.reason.name !== "TimeoutError") {
    throw new Error(String(signal.reason));
  }
});

Yuzuko.test("wasm compile ignores polluted promise and function methods", () => {
  const bytes = new Uint8Array([
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
  ]);
  const reflectApply = Reflect.apply;
  const then = Promise.prototype.then;
  const call = Function.prototype.call;
  const apply = Function.prototype.apply;
  const arrayBuffer = Response.prototype.arrayBuffer;
  Promise.prototype.then = () => {
    throw new Error("polluted then");
  };
  Function.prototype.call = () => {
    throw new Error("polluted call");
  };
  Function.prototype.apply = () => {
    throw new Error("polluted apply");
  };
  Response.prototype.arrayBuffer = () => {
    throw new Error("polluted arrayBuffer");
  };
  let compiled;
  let streamed;
  try {
    compiled = WebAssembly.compile(bytes);
    const response = new Response(bytes, {
      headers: { "content-type": "application/wasm" },
    });
    streamed = WebAssembly.compileStreaming(response);
  } finally {
    Promise.prototype.then = then;
    Function.prototype.call = call;
    Function.prototype.apply = apply;
    Response.prototype.arrayBuffer = arrayBuffer;
  }
  return reflectApply(then, compiled, [(module) => {
    if (!(module instanceof WebAssembly.Module)) {
      throw new Error("compile");
    }
    return reflectApply(then, streamed, [(streamedModule) => {
      if (!(streamedModule instanceof WebAssembly.Module)) {
        throw new Error("compileStreaming");
      }
    }]);
  }]);
});

Yuzuko.test("event assignment ignores a later defineProperty", () => {
  const defineProperty = Object.defineProperty;
  Object.defineProperty = () => {
    throw new Error("polluted defineProperty");
  };
  try {
    // @ts-expect-error event is not a number.
    globalThis.event = 1;
    // @ts-expect-error event is not a number.
    if (globalThis.event !== 1) {
      throw new Error("event");
    }
  } finally {
    Object.defineProperty = defineProperty;
    // @ts-expect-error event is not an optional property.
    delete globalThis.event;
  }
});

Yuzuko.test("ReadableStream.from ignores polluted promise methods", () => {
  const reflectApply = Reflect.apply;
  const resolve = Promise.resolve;
  const reject = Promise.reject;
  const then = Promise.prototype.then;
  const call = Function.prototype.call;
  Promise.resolve = () => {
    throw new Error("polluted resolve");
  };
  Promise.reject = () => {
    throw new Error("polluted reject");
  };
  Promise.prototype.then = () => {
    throw new Error("polluted then");
  };
  Function.prototype.call = () => {
    throw new Error("polluted call");
  };
  let read;
  try {
    read = ReadableStream.from(["a"]).getReader().read();
  } finally {
    Promise.resolve = resolve;
    Promise.reject = reject;
    Promise.prototype.then = then;
    Function.prototype.call = call;
  }
  return reflectApply(then, read, [(chunk) => {
    if (chunk.done || chunk.value !== "a") {
      throw new Error("chunk");
    }
  }]);
});

Yuzuko.test("base64 ops", () => {
  if (btoa("hi") !== "aGk=") {
    throw new Error("btoa");
  }
  if (atob("aGk=") !== "hi") {
    throw new Error("atob");
  }
  let name = "";
  try {
    btoa("h€");
  } catch (error) {
    // @ts-expect-error A thrown value is unknown.
    name = error.name;
  }
  if (name !== "InvalidCharacterError") {
    throw new Error(name);
  }
});

Yuzuko.test("url parser", () => {
  const url = new URL("/a", "https://example.com/base");
  if (url.href !== "https://example.com/a") {
    throw new Error(url.href);
  }
  if (url.toString() !== url.href || String(url) !== url.href) {
    throw new Error(url.toString());
  }
  if (`${url}` !== url.href) {
    throw new Error("template");
  }
  if (Object.prototype.toString.call(url) !== "[object URL]") {
    throw new Error(Object.prototype.toString.call(url));
  }
  url.searchParams.append("q", "1");
  if (url.search !== "?q=1") {
    throw new Error(url.search);
  }
  if (url.searchParams.toString() !== "q=1") {
    throw new Error(url.searchParams.toString());
  }
  if (!URL.canParse("https://example.com")) {
    throw new Error("canParse");
  }
  if (URL.parse("::") !== null) {
    throw new Error("parse null");
  }
  const parsed = URL.parse("/a", "https://example.com/base");
  if (!parsed || parsed.href !== "https://example.com/a") {
    throw new Error("parse");
  }
});

Yuzuko.test("globalThis dispatches events", () => {
  let seen = false;
  self.addEventListener("yz-global", () => {
    seen = true;
  });
  if (!self.dispatchEvent(new Event("yz-global"))) {
    throw new Error("dispatch");
  }
  if (!seen) {
    throw new Error("global listener");
  }
  const error = new DOMException("nope", "SyntaxError");
  if (!(error instanceof Error)) {
    throw new Error("instanceof");
  }
  if (error.name !== "SyntaxError" || error.code !== 12) {
    throw new Error(`${error.name} ${error.code}`);
  }
  const file = new File(["hi"], "note.txt");
  if (!(file instanceof Blob) || file.name !== "note.txt" || file.size !== 2) {
    throw new Error("file");
  }
});

Yuzuko.test("host dictionaries ignore Object.prototype accessors", () => {
  const names = [
    "x",
    "y",
    "width",
    "height",
    "read",
    "written",
    "done",
    "value",
    "timeOrigin",
  ];
  const previous = names.map((name) =>
    Object.getOwnPropertyDescriptor(Object.prototype, name),
  );
  for (const name of names) {
    // A null prototype keeps `value` from being read off `Object.prototype`
    // once that accessor exists.
    const descriptor: PropertyDescriptor = Object.create(null);
    descriptor.configurable = true;
    descriptor.get = () => "polluted";
    Object.defineProperty(Object.prototype, name, descriptor);
  }
  try {
    const point = new DOMPoint(1, 2, 3, 4).toJSON();
    if (point.x !== 1 || point.y !== 2 || point.z !== 3 || point.w !== 4) {
      throw new Error("DOMPoint");
    }
    const rect = new DOMRect(1, 2, 3, 4).toJSON();
    if (rect.x !== 1 || rect.width !== 3 || rect.height !== 4) {
      throw new Error("DOMRect");
    }
    const into = new TextEncoder().encodeInto("ab", new Uint8Array(8));
    if (into.read !== 2 || into.written !== 2) {
      throw new Error("encodeInto");
    }
    const entry = new URLSearchParams("a=b").entries().next();
    if (entry.done !== false || entry.value[0] !== "a" || entry.value[1] !== "b") {
      throw new Error("iterator");
    }
    const origin = performance.toJSON().timeOrigin;
    if (typeof origin !== "number") {
      throw new Error("performance");
    }
    const headers = new Headers([["a", "b"]]).entries().next();
    if (headers.done !== false || headers.value[0] !== "a") {
      throw new Error("headers");
    }
  } finally {
    names.forEach((name, index) => {
      const descriptor = previous[index];
      if (descriptor) {
        Object.defineProperty(Object.prototype, name, descriptor);
      } else {
        Reflect.deleteProperty(Object.prototype, name);
      }
    });
  }
});
