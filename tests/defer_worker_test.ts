function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

Yuzuko.test("a module worker defers evaluation until observation", async () => {
  const worker = new Worker(new URL("./defer/worker.ts", import.meta.url).href, {
    type: "module",
  });
  const pending = new Promise<string>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => {
      resolve(String(event.data));
    };
    worker.onerror = (event: ErrorEvent) => {
      reject(new Error(event.message || "worker error"));
    };
  });
  worker.postMessage("go");
  const reply = await pending;
  assert(reply === "false:true:1", reply);
  worker.terminate();
});
