declare const WebKitCSSMatrix: unknown;
declare const requestAnimationFrame: unknown;

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function script(name: string): string {
  return new URL(`./workers/${name}`, import.meta.url).href;
}

function thrown(run: () => void): { name: string; message: string } {
  try {
    run();
  } catch (error) {
    const name = error instanceof Error ? error.name : "";
    const message = error instanceof Error ? error.message : "";
    return { name, message };
  }
  throw new Error("expected a throw");
}

function nextMessage(worker: Worker): Promise<unknown> {
  return new Promise((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => resolve(event.data);
    worker.onerror = (event: ErrorEvent) => {
      reject(new Error(event.message || "worker error"));
    };
  });
}

Yuzuko.test("dedicated workers stay off the window global", async () => {
  assert(String(new DOMMatrix()) === "matrix(1, 0, 0, 1, 0, 0)", "window matrix");
  assert(
    // @ts-expect-error fonts is absent on the main global.
    globalThis.fonts === undefined,
    "no globalThis.fonts",
  );
  assert(Yuzuko.fonts instanceof FontFaceSet, "Yuzuko.fonts");
  assert(typeof WebKitCSSMatrix === "undefined", "no WebKitCSSMatrix");

  const worker = new Worker(script("classic.js"));
  const pending = nextMessage(worker);
  worker.postMessage({ kind: "ping" });
  // @ts-expect-error The classic worker replies with this record.
  const reply: {
    selfIsGlobal: boolean;
    dedicated: boolean;
    workerScope: boolean;
    hasWindow: boolean;
    hasGlobalWindow: boolean;
    hasDocument: boolean;
    hasSharedWorker: boolean;
    fontsSame: boolean;
    fontsSet: boolean;
    queryLocalFonts: boolean;
    fontData: boolean;
    matrix: string;
    readOnly: string;
    css: boolean;
    empty: boolean;
    nullCss: boolean;
    setMatrix: boolean;
    webkit: boolean;
    imported: number;
    encoded: string;
    performanceNow: boolean;
  } = await pending;
  assert(reply.selfIsGlobal, "self");
  assert(reply.dedicated && reply.workerScope, "scope");
  assert(reply.hasWindow === false, "no Yuzuko.Window");
  assert(reply.hasGlobalWindow === false, "no global window");
  assert(reply.hasDocument === false, "no document");
  assert(reply.hasSharedWorker === false, "no SharedWorker");
  assert(reply.fontsSame && reply.fontsSet, "self.fonts");
  assert(reply.queryLocalFonts && reply.fontData, "queryLocalFonts");
  assert(reply.matrix === "[object DOMMatrix]", reply.matrix);
  assert(reply.readOnly === "[object DOMMatrixReadOnly]", reply.readOnly);
  assert(reply.css && reply.empty && reply.nullCss, "css constructor");
  assert(reply.setMatrix === false, "setMatrixValue");
  assert(reply.webkit === false, "webkit");
  assert(reply.imported === 7, "importScripts");
  assert(reply.encoded === btoa("hi"), "btoa");
  assert(reply.performanceNow, "performance.now");
  worker.terminate();
});

Yuzuko.test("module workers, nested workers, and errors", async () => {
  const moduleWorker = new Worker(script("module.js"), { type: "module" });
  const modulePending = nextMessage(moduleWorker);
  moduleWorker.postMessage({ kind: "module", n: 4 });
  // @ts-expect-error The module worker replies with kind and n.
  const moduleReply: { kind: string; n: number } = await modulePending;
  assert(moduleReply.kind === "module" && moduleReply.n === 5, "module");
  moduleWorker.terminate();

  const classic = new Worker(script("classic.js"));
  const nestedPending = nextMessage(classic);
  classic.postMessage({ kind: "nest", url: script("child.js") });
  // @ts-expect-error The nested worker replies with kind and text.
  const nested: { kind: string; text: string } = await nestedPending;
  assert(nested.kind === "nested" && nested.text === "child-ok", nested.text);
  classic.terminate();

  const failing = new Worker(script("throw.js"));
  const message = await new Promise<string>((resolve, reject) => {
    failing.onerror = (event: ErrorEvent) => resolve(event.message || "");
    setTimeout(() => reject(new Error("error event timed out")), 5000);
  });
  assert(message.includes("worker-boom"), message);
  failing.terminate();
});

Yuzuko.test("workers run while the owner is busy", () => {
  const worker = new Worker(script("parallel.js"));
  const shared = new Uint8Array(new SharedArrayBuffer(16));
  worker.postMessage(shared);
  const end = performance.now() + 5000;
  while (shared[0] !== 1 && performance.now() < end) {
    // The worker writes the same buffer on its own thread.
  }
  assert(shared[0] === 1, "parallel");
  const later = new Uint8Array(new SharedArrayBuffer(16));
  worker.terminate();
  worker.postMessage(later);
  const stop = performance.now() + 200;
  while (performance.now() < stop) {
    // terminate drops later messages
  }
  assert(later[0] === 0, "terminated");
});

Yuzuko.test("postMessage transfers buffers, ports, and canvases", async () => {
  const worker = new Worker(script("classic.js"));

  const bytes = new Uint8Array([4, 5, 6]).buffer;
  const copied = nextMessage(worker);
  worker.postMessage(bytes, [bytes]);
  // @ts-expect-error The worker replies with the copied bytes.
  const view: number[] = await copied;
  assert(bytes.byteLength === 0, "detached");
  assert(view.join() === "4,5,6", view.join());

  const channel = new MessageChannel();
  const portReply = new Promise<string>((resolve) => {
    channel.port2.onmessage = (event: MessageEvent) => resolve(String(event.data));
  });
  worker.postMessage(channel.port1, [channel.port1]);
  assert(await portReply === "port-ok", "port");

  const canvas = new OffscreenCanvas(4, 4);
  const context = canvas.getContext("2d");
  assert(context, "2d");
  context.fillStyle = "#ff0000";
  context.fillRect(0, 0, 4, 4);
  const canvasReply = nextMessage(worker);
  worker.postMessage(canvas, [canvas]);
  assert(canvas.width === 0, "sender width");
  let detached = "";
  try {
    canvas.getContext("2d");
  } catch (error) {
    detached = error instanceof DOMException ? error.name : "";
  }
  assert(detached === "InvalidStateError", detached);
  // @ts-expect-error The worker replies with the painted canvas.
  const painted: { width: number; pixel: number[] } = await canvasReply;
  assert(painted.width === 4, String(painted.width));
  assert(painted.pixel.join() === "255,0,0,255", painted.pixel.join());

  const growable = new ArrayBuffer(4, { maxByteLength: 16 });
  new Uint8Array(growable).set([1, 2, 3, 4]);
  const slice = new Uint8Array(growable, 1, 2);
  const described = nextMessage(worker);
  worker.postMessage({ kind: "buffer", buffer: growable, view: slice }, [growable]);
  // @ts-expect-error The worker replies with the moved buffer.
  const info: {
    kind: string;
    byteLength: number;
    resized: number;
    maxByteLength: number;
    resizable: boolean;
    detached: boolean;
    bytes: number[];
    offset: number;
    viewLength: number;
    viewBytes: number[];
  } = await described;
  assert(growable.detached === true, "sender detached");
  assert(info.kind === "buffer", info.kind);
  assert(info.byteLength === 4, String(info.byteLength));
  assert(info.resized === 16, String(info.resized));
  assert(info.maxByteLength === 16, String(info.maxByteLength));
  assert(info.resizable === true, "resizable");
  assert(info.detached === false, "receiver attached");
  assert(info.bytes.join() === "1,2,3,4", info.bytes.join());
  assert(info.offset === 1 && info.viewLength === 2, "view range");
  assert(info.viewBytes.join() === "2,3", info.viewBytes.join());

  const empty = new ArrayBuffer(0);
  const emptyReply = nextMessage(worker);
  worker.postMessage(
    { kind: "buffer", buffer: empty, view: new Uint8Array(empty) },
    [empty],
  );
  // @ts-expect-error The worker replies with the empty buffer.
  const emptyInfo: { byteLength: number; detached: boolean; resizable: boolean } =
    await emptyReply;
  assert(empty.detached === true, "empty sender");
  assert(emptyInfo.byteLength === 0 && emptyInfo.detached === false, "empty receiver");
  assert(emptyInfo.resizable === false, "empty fixed");

  const returned = nextMessage(worker);
  worker.postMessage({ kind: "return-buffer" });
  const sentBack: unknown = await returned;
  assert(sentBack instanceof ArrayBuffer, "returned type");
  assert(sentBack.resizable === true && sentBack.maxByteLength === 16, "returned size");
  assert(sentBack.byteLength === 4, "returned length");
  assert(new Uint8Array(sentBack).join() === "1,2,3,4", "returned bytes");

  const viaView = new Uint8Array([7, 8, 9]);
  const viaReply = nextMessage(worker);
  // @ts-expect-error A view in a postMessage transfer list moves its buffer.
  worker.postMessage(viaView.buffer, [viaView]);
  // @ts-expect-error The worker replies with the copied bytes.
  const viaBytes: number[] = await viaReply;
  assert(viaView.buffer.detached === true, "view list detaches");
  assert(viaBytes.join() === "7,8,9", viaBytes.join());

  const stay = new Uint8Array([4, 5, 6]);
  const failure = thrown(() => {
    worker.postMessage({ bad: globalThis, buffer: stay.buffer }, [stay.buffer]);
  });
  assert(failure.name === "DataCloneError", failure.name);
  assert(stay.buffer.detached === false, "failed transfer stays");
  assert(stay[0] === 4 && stay.buffer.byteLength === 3, "failed bytes");
  worker.terminate();
});

Yuzuko.test("postMessage delivers transfer-list ports and rejects a bad call", async () => {
  const worker = new Worker(script("classic.js"));
  const missing = thrown(() => {
    // @ts-expect-error postMessage requires a message.
    worker.postMessage();
  });
  assert(missing.name === "TypeError", missing.name);
  assert(missing.message === "Not enough arguments", missing.message);

  const rejected = new MessageChannel();
  const nullList = thrown(() => {
    // @ts-expect-error A transfer list cannot contain null.
    worker.postMessage("null", [rejected.port1, null, rejected.port2]);
  });
  assert(nullList.name === "TypeError", nullList.name);
  assert(
    nullList.message === "The transfer list cannot contain null or undefined",
    nullList.message,
  );

  const channel = new MessageChannel();
  const extra = new MessageChannel();
  const reply = nextMessage(worker);
  worker.postMessage("ports", [channel.port1, extra.port1]);
  channel.port2.postMessage("ping");
  // @ts-expect-error The worker replies with the port count.
  const body: { kind: string; count: number; ping: string } = await reply;
  assert(body.kind === "ports", body.kind);
  assert(body.count === 2, String(body.count));
  assert(body.ping === "ping", body.ping);

  const arity = nextMessage(worker);
  worker.postMessage("no-args");
  // @ts-expect-error The worker replies with the arity check.
  const noArgs: { kind: string; thrown: boolean } = await arity;
  assert(noArgs.kind === "no-args", noArgs.kind);
  assert(noArgs.thrown, "worker postMessage()");
  worker.terminate();
});

Yuzuko.test("fonts, windows, and WebGPU stay on their thread", async () => {
  const worker = new Worker(script("module.js"), { type: "module" });
  const fontPending = nextMessage(worker);
  const fontUrl = new URL("./fonts/YuzukoTest.ttf", import.meta.url).href;
  worker.postMessage({ kind: "font", url: fontUrl });
  // @ts-expect-error The worker replies with the font measurement.
  const font: {
    same: boolean;
    width: number;
    generic: number;
    status: string;
  } = await fontPending;
  assert(font.same, "fonts");
  assert(font.status === "loaded", font.status);
  assert(Math.abs(font.width - font.generic) > 1, `${font.width} vs ${font.generic}`);
  const main = new OffscreenCanvas(40, 20).getContext("2d");
  assert(main, "main 2d");
  main.font = "20px WorkerOnly";
  assert(main.font.includes("WorkerOnly"), main.font);
  const missingWidth = main.measureText("A").width;
  main.font = "20px sans-serif";
  const genericWidth = main.measureText("A").width;
  assert(missingWidth === genericWidth, `${missingWidth} vs ${genericWidth}`);
  assert(
    // @ts-expect-error fonts is absent on the main global.
    globalThis.fonts === undefined,
    "main fonts",
  );

  const window = new Yuzuko.Window({ width: 32, height: 32, title: "worker" });
  let transfer = "";
  try {
    // @ts-expect-error A window is not transferable.
    worker.postMessage(window, [window]);
  } catch (error) {
    transfer = error instanceof DOMException ? error.name : "";
  }
  assert(transfer === "DataCloneError", transfer);
  assert(window.clientWidth === 32, "window stays");
  assert(window.surface.width === 32, "surface stays");
  window.close();

  const adapter = await navigator.gpu.requestAdapter();
  assert(adapter, "adapter");
  const device = await adapter.requestDevice();
  let clone = "";
  try {
    // @ts-expect-error A GPU device is not transferable.
    worker.postMessage(device, [device]);
  } catch (error) {
    clone = error instanceof DOMException ? error.name : "";
  }
  assert(clone === "DataCloneError", clone);
  const buffer = device.createBuffer({
    size: 4,
    usage: GPUBufferUsage.COPY_SRC,
  });
  buffer.destroy();

  const gpuPending = nextMessage(worker);
  worker.postMessage({ kind: "gpu" });
  // @ts-expect-error The worker replies with the GPU error name.
  const gpu: { name: string } = await gpuPending;
  assert(gpu.name === "InvalidStateError", gpu.name);
  worker.terminate();
});

Yuzuko.test("WindowSurface transfers control, not itself, to a worker", async () => {
  assert(typeof requestAnimationFrame === "undefined", "no global rAF");
  const direct = new Yuzuko.Window({ width: 8, height: 8, title: "direct" });
  assert(!(direct.surface instanceof OffscreenCanvas), "not an OffscreenCanvas");
  assert(direct.surface instanceof Yuzuko.WindowSurface, "surface");
  const context = direct.surface.getContext("2d");
  assert(context, "direct 2d");
  assert(context.canvas === direct.surface, "context canvas");
  context.fillStyle = "#00ff00";
  context.fillRect(0, 0, 1, 1);
  const green = context.getImageData(0, 0, 1, 1).data;
  assert(green[1] === 255, `green ${green[1]}`);
  let held = "";
  const probe = new Worker(script("classic.js"));
  try {
    // @ts-expect-error A window surface is not transferable.
    probe.postMessage(direct.surface, [direct.surface]);
  } catch (error) {
    held = error instanceof DOMException ? error.name : "";
  }
  assert(held === "DataCloneError", held);
  assert(direct.surface.width === 8, "surface stays");
  let denied = "";
  try {
    direct.surface.transferControlToOffscreen();
  } catch (error) {
    denied = error instanceof DOMException ? error.name : "";
  }
  assert(denied === "InvalidStateError", denied);
  direct.close();
  probe.terminate();

  const window = new Yuzuko.Window({ width: 8, height: 8, title: "transfer" });
  const offscreen = window.surface.transferControlToOffscreen();
  assert(offscreen instanceof OffscreenCanvas, "offscreen");
  assert(!(offscreen instanceof Yuzuko.WindowSurface), "offscreen is not the surface");
  let again = "";
  try {
    window.surface.getContext("2d");
  } catch (error) {
    again = error instanceof DOMException ? error.name : "";
  }
  assert(again === "InvalidStateError", again);
  let second = "";
  try {
    window.surface.transferControlToOffscreen();
  } catch (error) {
    second = error instanceof DOMException ? error.name : "";
  }
  assert(second === "InvalidStateError", second);

  const worker = new Worker(script("surface.js"));
  const painted = nextMessage(worker);
  worker.postMessage(offscreen, [offscreen]);
  assert(offscreen.width === 0, "detached offscreen");
  // @ts-expect-error The worker replies with the presented frame.
  const reply: { kind: string; pixel: number[]; width: number } = await painted;
  assert(reply.kind === "frame", reply.kind);
  assert(reply.width === 8, String(reply.width));
  assert(reply.pixel.join() === "0,0,255,255", reply.pixel.join());
  worker.terminate();
  window.close();
});

Yuzuko.test("postMessage clones ImageData and transfers ImageBitmap", async () => {
  const worker = new Worker(script("clone.js"));
  const image = new ImageData(new Uint8ClampedArray([4, 5, 6, 7]), 1, 1);
  const imageReply = nextMessage(worker);
  worker.postMessage(image);
  // @ts-expect-error The worker replies with the cloned image.
  const imageSummary: {
    kind: string;
    width: number;
    height: number;
    colorSpace: string;
    pixelFormat: string;
    pixel: number;
  } = await imageReply;
  assert(imageSummary.kind === "image", imageSummary.kind);
  assert(imageSummary.width === 1 && imageSummary.height === 1, "image size");
  assert(imageSummary.colorSpace === "srgb", imageSummary.colorSpace);
  assert(imageSummary.pixelFormat === "rgba-unorm8", imageSummary.pixelFormat);
  assert(imageSummary.pixel === 4, String(imageSummary.pixel));
  assert(image.data[0] === 4, "image source");

  const bitmap = await createImageBitmap(image);
  const bitmapReply = nextMessage(worker);
  worker.postMessage(bitmap, [bitmap]);
  assert(bitmap.width === 0 && bitmap.height === 0, "bitmap detached");
  // @ts-expect-error The worker replies with the transferred bitmap.
  const bitmapSummary: {
    kind: string;
    width: number;
    height: number;
  } = await bitmapReply;
  assert(bitmapSummary.kind === "bitmap", bitmapSummary.kind);
  assert(bitmapSummary.width === 1 && bitmapSummary.height === 1, "bitmap size");

  const kept = await createImageBitmap(
    new ImageData(new Uint8ClampedArray([1, 2, 3, 4]), 1, 1),
  );
  const keptReply = nextMessage(worker);
  worker.postMessage(kept);
  // @ts-expect-error The worker replies with the copied bitmap.
  const keptSummary: { kind: string; width: number } = await keptReply;
  assert(keptSummary.kind === "bitmap", keptSummary.kind);
  assert(kept.width === 1 && keptSummary.width === 1, "bitmap copy");
  worker.terminate();
});

function workerMessages(worker: Worker): { next(): Promise<Record<string, unknown>> } {
  const queue: Record<string, unknown>[] = [];
  let wait: ((value: Record<string, unknown>) => void) | null = null;
  worker.addEventListener("message", (event: Event) => {
    // @ts-expect-error A message event carries the posted value.
    const raw: unknown = event.data;
    if (raw === null || typeof raw !== "object" || !("kind" in raw)) return;
    const kind = raw.kind;
    if (typeof kind !== "string") return;
    const data = raw as Record<string, unknown>;
    if (wait) {
      const resolve = wait;
      wait = null;
      resolve(data);
    } else {
      queue.push(data);
    }
  });
  return {
    next() {
      const queued = queue.shift();
      if (queued) return Promise.resolve(queued);
      return new Promise((resolve) => {
        wait = resolve;
      });
    },
  };
}

function wasmModule(): WebAssembly.Module {
  return new WebAssembly.Module(
    new Uint8Array([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]),
  );
}

function assertMessage(event: Record<string, unknown>, label: string) {
  assert(event["module"] === true, `${label} module`);
  assert(event["instantiated"] === true, `${label} instance`);
  assert(event["type"] === "message", `${label} type`);
  assert(event["trusted"] === true, `${label} trust`);
  assert(event["bubbles"] === false && event["cancelable"] === false, `${label} flags`);
  assert(event["messageEvent"] === true, `${label} event`);
}

function transferName(run: () => void): string {
  try {
    run();
  } catch (error) {
    return error instanceof DOMException ? `${error.name}: ${error.message}` : "";
  }
  return "";
}

Yuzuko.test("a module posted across agents is delivered", async () => {
  const module = wasmModule();
  const copied = structuredClone(module);
  assert(copied instanceof WebAssembly.Module, "structuredClone");
  assert(copied !== module, "structuredClone identity");
  assert(new WebAssembly.Instance(copied) instanceof WebAssembly.Instance, "copy instance");

  const local = new MessageChannel();
  const localResult = new Promise<unknown>((resolve) => {
    local.port2.onmessage = (event: MessageEvent) => resolve(event.data);
    local.port2.onmessageerror = () => resolve("messageerror");
  });
  local.port1.postMessage(module);
  const localModule = await localResult;
  assert(localModule instanceof WebAssembly.Module, "same agent");
  assert(localModule !== module, "same agent identity");
  local.port2.onmessageerror = () => {};
  // @ts-expect-error A number is not a messageerror handler.
  local.port2.onmessageerror = 1;
  assert(local.port2.onmessageerror === null, "port handler cleared");
  assert(
    transferName(() => {
      // @ts-expect-error A WebAssembly.Module is not transferable.
      local.port1.postMessage(module, [module]);
    }) === "DataCloneError: The value cannot be transferred",
    "port transfer",
  );
  assert(
    transferName(() => {
      // @ts-expect-error A WebAssembly.Module is not transferable.
      structuredClone(module, { transfer: [module] });
    }) === "DataCloneError: The value cannot be transferred",
    "clone transfer",
  );
  assert(new WebAssembly.Instance(module) instanceof WebAssembly.Instance, "source remains");

  const worker = new Worker(script("message_error.js"));
  const incoming = workerMessages(worker);
  worker.postMessage({ kind: "ping" });
  assert((await incoming.next())["kind"] === "pong", "ready");

  let parentError = false;
  let parentMessageError = false;
  worker.onerror = () => {
    parentError = true;
  };
  worker.onmessageerror = () => {
    parentMessageError = true;
  };
  worker.postMessage(module);
  const globalMessage = await incoming.next();
  assert(globalMessage["kind"] === "global-module", String(globalMessage["kind"]));
  assert(globalMessage["data"] instanceof WebAssembly.Module, "worker data");
  assert(globalMessage["data"] !== module, "worker identity");
  assertMessage(globalMessage, "worker global");
  assert(parentError === false, "parent error");
  assert(parentMessageError === false, "parent messageerror");

  const echoed = new Promise<WebAssembly.Module>((resolve, reject) => {
    const onMessage = (event: Event) => {
      // @ts-expect-error A message event carries the posted module.
      const data: unknown = event.data;
      if (!(data instanceof WebAssembly.Module)) return;
      worker.removeEventListener("message", onMessage);
      resolve(data);
    };
    worker.addEventListener("message", onMessage);
    worker.onmessageerror = () => {
      worker.removeEventListener("message", onMessage);
      reject(new Error("echo messageerror"));
    };
  });
  worker.postMessage({ kind: "echo-module" });
  const echoedModule = await echoed;
  assert(echoedModule !== module, "owner identity");
  assert(new WebAssembly.Instance(echoedModule) instanceof WebAssembly.Instance, "owner instance");
  assert(parentError === false, "owner error");

  worker.onmessageerror = () => {};
  // @ts-expect-error A number is not a messageerror handler.
  worker.onmessageerror = 1;
  assert(worker.onmessageerror === null, "worker handler cleared");

  const again = wasmModule();
  worker.postMessage({ kind: "same-ref", a: module, b: module, c: again });
  const same = await incoming.next();
  assert(same["kind"] === "same-ref", String(same["kind"]));
  assert(same["modules"] === true && same["ab"] === true && same["ac"] === true, "same ref");
  assert(same["instantiated"] === true, "same ref instance");

  const channel = new MessageChannel();
  worker.postMessage({ kind: "hold-port", port: channel.port1 }, [channel.port1]);
  const holding = await incoming.next();
  assert(holding["kind"] === "holding" && holding["handler"] === true, "holding");
  channel.port2.postMessage(module);
  worker.postMessage({ kind: "release-port" });
  const before = await incoming.next();
  assert(
    before["kind"] === "before-start" && before["seen"] === false,
    "not started",
  );
  const portMessage = await incoming.next();
  assert(portMessage["kind"] === "port-message", String(portMessage["kind"]));
  assertMessage(portMessage, "port");

  const queued = new MessageChannel();
  queued.port2.postMessage(wasmModule());
  worker.postMessage(queued.port1, [queued.port1]);
  const queuedBefore = await incoming.next();
  assert(
    queuedBefore["kind"] === "queued-before-start" && queuedBefore["seen"] === false,
    "queued quiet",
  );
  const queuedMessage = await incoming.next();
  assert(queuedMessage["kind"] === "queued-message", String(queuedMessage["kind"]));
  assertMessage(queuedMessage, "queued");

  worker.postMessage({ kind: "stream-module" });
  const streamed = await incoming.next();
  assert(streamed["kind"] === "stream", String(streamed["kind"]));
  const stream = streamed["stream"];
  assert(stream instanceof ReadableStream, "stream");
  const reader = stream.getReader();
  const first = await reader.read();
  assert(first.done === false && first.value instanceof WebAssembly.Module, "stream module");
  assert((await reader.read()).done === true, "stream end");

  const outbound = new ReadableStream({
    start(controller) {
      controller.enqueue(wasmModule());
      controller.close();
    },
  });
  worker.postMessage({ kind: "read-stream", stream: outbound }, [outbound]);
  const read = await incoming.next();
  assert(read["kind"] === "read-stream", String(read["kind"]));
  assert(read["module"] === true && read["done"] === true, String(read["value"]));

  parentMessageError = false;
  worker.onmessageerror = () => {
    parentMessageError = true;
  };
  assert(
    transferName(() => {
      // @ts-expect-error A WebAssembly.Module is not transferable.
      worker.postMessage(module, [module]);
    }) === "DataCloneError: The value cannot be transferred",
    "worker transfer",
  );
  let cloneName = "";
  try {
    worker.postMessage(() => {});
  } catch (error) {
    cloneName = error instanceof DOMException ? error.name : "";
  }
  assert(cloneName === "DataCloneError", cloneName);
  worker.postMessage({ kind: "ping" });
  const pong = await incoming.next();
  assert(pong["kind"] === "pong", String(pong["kind"]));
  assert(parentMessageError === false, "function messageerror");
  assert(parentError === false, "function error");
  worker.terminate();
});

Yuzuko.test("worker URLs, decoding, and parse errors", async () => {
  // @ts-expect-error The constructor requires a URL.
  const missing = thrown(() => new Worker());
  assert(missing.name === "TypeError", missing.name);

  const invalid = thrown(() => new Worker("http://invalid:123$"));
  assert(invalid.name === "SyntaxError", invalid.name);

  const named = new Worker(undefined as unknown as string);
  const namedData = await nextMessage(named);
  assert(namedData === "undefined", String(namedData));
  named.terminate();

  const empty = new Worker("");
  const emptyKind = await new Promise<string>((resolve, reject) => {
    empty.onerror = (event: Event) => resolve(event.constructor.name);
    setTimeout(() => reject(new Error("empty worker timed out")), 5000);
  });
  assert(emptyKind === "Event", emptyKind);
  empty.terminate();

  const decoded = new Worker('data:text/javascript,postMessage("%FF")');
  const character = await nextMessage(decoded);
  assert(character === "\uFFFD", JSON.stringify(character));
  decoded.terminate();

  const parsed = new Worker("data:text/javascript,function(");
  const parsedKind = await new Promise<string>((resolve, reject) => {
    parsed.onerror = (event: Event) => resolve(event.constructor.name);
    setTimeout(() => reject(new Error("parse error timed out")), 5000);
  });
  assert(parsedKind === "Event", parsedKind);
  parsed.terminate();

  const body = [
    "var name = 'missing';",
    "var first = false;",
    "try {",
    "  importScripts('data:text/javascript,self.__y=1', 'data:text/javascript,for(;)break;');",
    "} catch (error) {",
    "  name = error instanceof SyntaxError ? error.name : String(error);",
    "  first = self.__y === 1;",
    "}",
    "postMessage({ name: name, first: first });",
  ].join("\n");
  const importer = new Worker(
    `data:text/javascript,${encodeURIComponent(body)}`,
  );
  const imported = (await nextMessage(importer)) as {
    name: string;
    first: boolean;
  };
  assert(imported.name === "SyntaxError", imported.name);
  assert(imported.first === true, "first script ran");
  importer.terminate();

  const thrownBody = [
    "var thrown = 'missing';",
    "var first = false;",
    "try {",
    "  importScripts('data:text/javascript,self.__y=1', 'data:text/javascript,throw 2');",
    "} catch (error) {",
    "  thrown = error;",
    "  first = self.__y === 1;",
    "}",
    "postMessage({ thrown: thrown, first: first });",
  ].join("\n");
  const throwing = new Worker(
    `data:text/javascript,${encodeURIComponent(thrownBody)}`,
  );
  const propagated = (await nextMessage(throwing)) as {
    thrown: unknown;
    first: boolean;
  };
  assert(propagated.thrown === 2, String(propagated.thrown));
  assert(propagated.first === true, "first script ran before the throw");
  throwing.terminate();

  const nested = new Worker(
    "data:text/javascript,importScripts('data:text/javascript,function(')",
  );
  const nestedFile = await new Promise<string>((resolve, reject) => {
    nested.onerror = (event: Event) => {
      const filename =
        "filename" in event ? String((event as ErrorEvent).filename) : "";
      resolve(filename);
    };
    setTimeout(() => reject(new Error("nested import timed out")), 5000);
  });
  assert(nestedFile.includes("function("), nestedFile);
  nested.terminate();

  const late = new Worker(
    "data:text/javascript,setTimeout(() => { throw new Error('Throw in timeout'); }, 0)",
  );
  const lateMessage = await new Promise<string>((resolve, reject) => {
    late.onerror = (event: Event) => {
      const message =
        "message" in event ? String((event as ErrorEvent).message) : "";
      resolve(message);
    };
    setTimeout(() => reject(new Error("timeout error timed out")), 5000);
  });
  assert(lateMessage.includes("Throw in timeout"), lateMessage);
  late.terminate();

  const inner = "data:text/javascript,function(";
  const middle = `data:text/javascript,${encodeURIComponent(
    `importScripts(${JSON.stringify(inner)})`,
  )}`;
  const chain = new Worker(
    `data:text/javascript,${encodeURIComponent(
      `importScripts(${JSON.stringify(middle)})`,
    )}`,
  );
  const chainFile = await new Promise<string>((resolve, reject) => {
    chain.onerror = (event: Event) => {
      const filename =
        "filename" in event ? String((event as ErrorEvent).filename) : "";
      resolve(filename);
    };
    setTimeout(() => reject(new Error("nested chain timed out")), 5000);
  });
  assert(
    chainFile.includes("function(") && !chainFile.includes("importScripts"),
    chainFile,
  );
  chain.terminate();

  const closeBody = [
    "onmessage = function (event) {",
    "  if (event.data === 'close') {",
    "    close();",
    "    postMessage('after');",
    "  } else {",
    "    postMessage('pong');",
    "  }",
    "};",
  ].join("\n");
  const closer = new Worker(
    `data:text/javascript,${encodeURIComponent(closeBody)}`,
  );
  const closed: string[] = [];
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("close timed out")), 5000);
    closer.onmessage = (event: MessageEvent) => {
      closed.push(String(event.data));
      if (closed.length === 1) {
        clearTimeout(timer);
        setTimeout(resolve, 200);
      }
    };
    closer.onerror = () => reject(new Error("close worker errored"));
    closer.postMessage("close");
    closer.postMessage("ping");
  });
  assert(closed.length === 1 && closed[0] === "after", closed.join(","));
  closer.terminate();
});

Yuzuko.test("a worker transfer detached during serialization throws", () => {
  const worker = new Worker(script("echo.js"));
  const buffer = new ArrayBuffer(8);
  let stolen: ArrayBuffer | undefined;
  const value = {
    buffer,
    get steal() {
      stolen = buffer.transfer();
      return 0;
    },
  };
  const error = thrown(() => worker.postMessage(value, [buffer]));
  worker.terminate();
  assert(error.name === "DataCloneError", error.name + ": " + error.message);
  assert(stolen !== undefined && stolen.byteLength === 8, "stolen buffer");
});

Yuzuko.test("a data: worker URL drops its fragment", async () => {
  const worker = new Worker("data:text/javascript,postMessage(1)#frag");
  const value = await nextMessage(worker);
  worker.terminate();
  assert(value === 1, String(value));
});

Yuzuko.test("a port message to a worker lists transferred ports", async () => {
  const body = [
    "onmessage = (event) => {",
    "  const control = event.ports[0];",
    "  control.onmessage = (inner) => {",
    "    const count = inner.ports.length;",
    "    inner.ports[0].postMessage('via ' + count);",
    "  };",
    "};",
  ].join("\n");
  const worker = new Worker(`data:text/javascript,${encodeURIComponent(body)}`);
  const control = new MessageChannel();
  worker.postMessage(null, [control.port2]);
  const reply = new MessageChannel();
  const answered = new Promise<unknown>((resolve) => {
    reply.port1.onmessage = (event) => resolve(event.data);
  });
  control.port1.postMessage(null, [reply.port2]);
  const value = await answered;
  worker.terminate();
  control.port1.close();
  reply.port1.close();
  assert(value === "via 1", String(value));
});
