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
  typeof Yuzuko.listenDatagram,
  typeof Yuzuko.Datagram,
  typeof Yuzuko.listenTcp,
  typeof Yuzuko.connectTcp,
  typeof Yuzuko.TcpListener,
  typeof Yuzuko.TcpConn,
  typeof Yuzuko.listenHttp,
  typeof Yuzuko.HttpServer,
  typeof Yuzuko.HttpRequest,
  typeof Yuzuko.addSignalListener,
  typeof Yuzuko.removeSignalListener,
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
  typeof Yuzuko.args,
  typeof Yuzuko.env,
  typeof Yuzuko.cwd,
  typeof Yuzuko.chdir,
  typeof Yuzuko.exit,
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
