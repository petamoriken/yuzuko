importScripts("./worker_lib.js");

function throwsType(build) {
  try {
    build();
    return false;
  } catch (error) {
    return error instanceof TypeError;
  }
}

self.onmessage = (event) => {
  const data = event.data;
  if (data && data.kind === "nest") {
    const child = new Worker(data.url);
    child.onmessage = (inner) => {
      self.postMessage({ kind: "nested", text: inner.data });
    };
    child.postMessage("child");
    return;
  }
  if (data instanceof ArrayBuffer) {
    self.postMessage(Array.from(new Uint8Array(data)));
    return;
  }
  if (data && data.kind === "buffer") {
    const buffer = data.buffer;
    const view = data.view;
    const before = buffer.byteLength;
    if (buffer.resizable) {
      buffer.resize(buffer.maxByteLength);
    }
    self.postMessage({
      kind: "buffer",
      byteLength: before,
      resized: buffer.byteLength,
      maxByteLength: buffer.maxByteLength,
      resizable: buffer.resizable,
      detached: buffer.detached,
      bytes: Array.from(new Uint8Array(buffer, 0, before)),
      offset: view.byteOffset,
      viewLength: view.byteLength,
      viewBytes: Array.from(view),
    });
    return;
  }
  if (data && data.kind === "return-buffer") {
    const buffer = new ArrayBuffer(4, { maxByteLength: 16 });
    new Uint8Array(buffer).set([1, 2, 3, 4]);
    self.postMessage(buffer, [buffer]);
    return;
  }
  if (typeof MessagePort !== "undefined" && data instanceof MessagePort) {
    data.postMessage("port-ok");
    return;
  }
  if (data === "ports") {
    const count = event.ports.length;
    const port = event.ports[0];
    if (!port) {
      self.postMessage({ kind: "ports", count, ping: "" });
      return;
    }
    port.onmessage = (inner) => {
      self.postMessage({ kind: "ports", count, ping: inner.data });
    };
    return;
  }
  if (data === "no-args") {
    let thrown = false;
    try {
      self.postMessage();
    } catch (error) {
      thrown = error instanceof TypeError && error.message === "Not enough arguments";
    }
    self.postMessage({ kind: "no-args", thrown });
    return;
  }
  if (typeof OffscreenCanvas !== "undefined" && data instanceof OffscreenCanvas) {
    const context = data.getContext("2d");
    const pixel = context.getImageData(0, 0, 1, 1).data;
    self.postMessage({
      kind: "canvas",
      width: data.width,
      pixel: [pixel[0], pixel[1], pixel[2], pixel[3]],
    });
    return;
  }
  self.postMessage({
    kind: "classic",
    selfIsGlobal: self === globalThis,
    dedicated: self instanceof DedicatedWorkerGlobalScope,
    workerScope: self instanceof WorkerGlobalScope,
    hasWindow: typeof Yuzuko !== "undefined" && typeof Yuzuko.Window === "function",
    hasGlobalWindow: typeof window !== "undefined",
    hasDocument: typeof document !== "undefined",
    hasSharedWorker: typeof SharedWorker !== "undefined",
    fontsSame: self.fonts === globalThis.fonts,
    fontsSet: self.fonts instanceof FontFaceSet,
    queryLocalFonts: typeof queryLocalFonts === "function",
    fontData: typeof FontData === "function",
    matrix: String(new DOMMatrix()),
    readOnly: String(new DOMMatrixReadOnly()),
    css: throwsType(() => new DOMMatrix("matrix(1, 0, 0, 1, 0, 0)")),
    empty: throwsType(() => new DOMMatrix("")),
    nullCss: throwsType(() => new DOMMatrix(null)),
    setMatrix: "setMatrixValue" in DOMMatrix.prototype,
    webkit: typeof WebKitCSSMatrix !== "undefined",
    imported: self.workerLib,
    encoded: btoa("hi"),
    performanceNow: typeof performance.now === "function" && performance.now() >= 0,
  });
};
