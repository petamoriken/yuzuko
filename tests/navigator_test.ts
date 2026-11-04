function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function positive(value: unknown, message: string): asserts value is number {
  assert(typeof value === "number", message);
  assert(Number.isInteger(value), `${message} integer`);
  assert(value >= 1, `${message} positive`);
}

Yuzuko.test("navigator.hardwareConcurrency is a positive integer", async () => {
  positive(navigator.hardwareConcurrency, "main");
  assert(typeof navigator.getGamepads === "function", "replaced navigator");
  const worker = new Worker(
    new URL("./workers/hardware_concurrency.js", import.meta.url).href,
  );
  const workerCount = await new Promise<unknown>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => resolve(event.data);
    worker.onerror = (event: ErrorEvent) => {
      reject(new Error(event.message || "worker error"));
    };
  });
  worker.terminate();
  positive(workerCount, "worker");
  assert(workerCount === navigator.hardwareConcurrency, "same count");
});
