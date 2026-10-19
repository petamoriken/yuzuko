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
  setTimeout = () => {
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
    globalThis.event = 1;
    if (globalThis.event !== 1) {
      throw new Error("event");
    }
  } finally {
    Object.defineProperty = defineProperty;
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
  globalThis.addEventListener("yz-global", () => {
    seen = true;
  });
  if (!globalThis.dispatchEvent(new Event("yz-global"))) {
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
