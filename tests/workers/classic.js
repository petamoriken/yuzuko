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
  if (typeof MessagePort !== "undefined" && data instanceof MessagePort) {
    data.postMessage("port-ok");
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
    hasDocument: typeof document !== "undefined",
    hasSharedWorker: typeof SharedWorker !== "undefined",
    fontsSame: self.fonts === globalThis.fonts,
    fontsSet: self.fonts instanceof FontFaceSet,
    matrix: String(new DOMMatrix()),
    readOnly: String(new DOMMatrixReadOnly()),
    css: throwsType(() => new DOMMatrix("matrix(1, 0, 0, 1, 0, 0)")),
    empty: throwsType(() => new DOMMatrix("")),
    nullCss: throwsType(() => new DOMMatrix(null)),
    setMatrix: "setMatrixValue" in DOMMatrix.prototype,
    webkit: typeof WebKitCSSMatrix !== "undefined",
    imported: self.workerLib,
    encoded: btoa("hi"),
  });
};
