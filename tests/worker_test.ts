function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function script(name: string): string {
  return new URL(`./workers/${name}`, import.meta.url).href;
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
  assert(globalThis.fonts === undefined, "no globalThis.fonts");
  assert(Yuzuko.fonts instanceof FontFaceSet, "Yuzuko.fonts");
  assert(typeof WebKitCSSMatrix === "undefined", "no WebKitCSSMatrix");

  const worker = new Worker(script("classic.js"));
  const pending = nextMessage(worker);
  worker.postMessage({ kind: "ping" });
  const reply = await pending as {
    selfIsGlobal: boolean;
    dedicated: boolean;
    workerScope: boolean;
    hasWindow: boolean;
    hasDocument: boolean;
    hasSharedWorker: boolean;
    fontsSame: boolean;
    fontsSet: boolean;
    matrix: string;
    readOnly: string;
    css: boolean;
    empty: boolean;
    nullCss: boolean;
    setMatrix: boolean;
    webkit: boolean;
    imported: number;
    encoded: string;
  };
  assert(reply.selfIsGlobal, "self");
  assert(reply.dedicated && reply.workerScope, "scope");
  assert(reply.hasWindow === false, "no Yuzuko.Window");
  assert(reply.hasDocument === false, "no document");
  assert(reply.hasSharedWorker === false, "no SharedWorker");
  assert(reply.fontsSame && reply.fontsSet, "self.fonts");
  assert(reply.matrix === "[object DOMMatrix]", reply.matrix);
  assert(reply.readOnly === "[object DOMMatrixReadOnly]", reply.readOnly);
  assert(reply.css && reply.empty && reply.nullCss, "css constructor");
  assert(reply.setMatrix === false, "setMatrixValue");
  assert(reply.webkit === false, "webkit");
  assert(reply.imported === 7, "importScripts");
  assert(reply.encoded === btoa("hi"), "btoa");
  worker.terminate();
});

Yuzuko.test("module workers, nested workers, and errors", async () => {
  const moduleWorker = new Worker(script("module.js"), { type: "module" });
  const modulePending = nextMessage(moduleWorker);
  moduleWorker.postMessage({ kind: "module", n: 4 });
  const moduleReply = await modulePending as { kind: string; n: number };
  assert(moduleReply.kind === "module" && moduleReply.n === 5, "module");
  moduleWorker.terminate();

  const classic = new Worker(script("classic.js"));
  const nestedPending = nextMessage(classic);
  classic.postMessage({ kind: "nest", url: script("child.js") });
  const nested = await nestedPending as { kind: string; text: string };
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
  const view = await copied as number[];
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
  const painted = await canvasReply as { width: number; pixel: number[] };
  assert(painted.width === 4, String(painted.width));
  assert(painted.pixel.join() === "255,0,0,255", painted.pixel.join());
  worker.terminate();
});

Yuzuko.test("fonts, windows, and WebGPU stay on their thread", async () => {
  const worker = new Worker(script("module.js"), { type: "module" });
  const fontPending = nextMessage(worker);
  const fontUrl = new URL("./fonts/YuzukoTest.ttf", import.meta.url).href;
  worker.postMessage({ kind: "font", url: fontUrl });
  const font = await fontPending as {
    same: boolean;
    width: number;
    generic: number;
    status: string;
  };
  assert(font.same, "fonts");
  assert(font.status === "loaded", font.status);
  assert(Math.abs(font.width - font.generic) > 1, `${font.width} vs ${font.generic}`);
  const main = new OffscreenCanvas(40, 20).getContext("2d");
  assert(main, "main 2d");
  let missing = "";
  try {
    main.font = "20px WorkerOnly";
  } catch (error) {
    missing = error instanceof DOMException ? error.name : "";
  }
  assert(missing === "NotSupportedError", missing);
  assert(globalThis.fonts === undefined, "main fonts");

  const window = new Yuzuko.Window({ width: 32, height: 32, title: "worker" });
  let transfer = "";
  try {
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
  const gpu = await gpuPending as { name: string };
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
  const reply = await painted as { kind: string; pixel: number[]; width: number };
  assert(reply.kind === "frame", reply.kind);
  assert(reply.width === 8, String(reply.width));
  assert(reply.pixel.join() === "0,0,255,255", reply.pixel.join());
  worker.terminate();
  window.close();
});
