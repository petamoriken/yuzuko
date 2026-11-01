const names = [
  typeof AbortController,
  typeof AbortSignal,
  typeof EventTarget,
  typeof DOMException,
  typeof URL,
  typeof TextEncoder,
  typeof Blob,
  typeof FormData,
  typeof crypto,
  typeof Worker,
  typeof setTimeout,
  typeof requestIdleCallback,
  typeof fetch,
  typeof WebSocket,
  typeof Yuzuko.readFile,
  typeof Yuzuko.Window,
  typeof PointerEvent,
  typeof OffscreenCanvas,
  typeof createImageBitmap,
  typeof DOMMatrix,
  typeof Yuzuko.canvasRenderer,
  typeof navigator.gpu,
  typeof AudioContext,
  typeof VideoDecoder,
  typeof ImageDecoder,
].join(" ");

const worker = new Worker(new URL("./runtime_worker.ts", import.meta.url), {
  type: "module",
});
try {
  const reply = await new Promise((resolve, reject) => {
    worker.onmessage = (event) => resolve(event.data);
    worker.onerror = (event) => {
      reject(new Error(String(event.message || "worker error")));
    };
    worker.postMessage("go");
  });
  console.log(names);
  console.log(reply);
} finally {
  worker.terminate();
}
