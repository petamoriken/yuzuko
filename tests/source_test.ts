import source workerSource from "./workers/source_module.ts";
import source wasm from "./wasm/add.wasm";

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

Yuzuko.test("new Worker runs a ModuleSource as a module", async () => {
  const AbstractModuleSource = Object.getPrototypeOf(WebAssembly.Module);
  assert(workerSource instanceof AbstractModuleSource, "abstract");
  assert(
    Object.prototype.toString.call(workerSource) === "[object ModuleSource]",
    Object.prototype.toString.call(workerSource),
  );

  const worker = new Worker(workerSource);
  const pending = nextMessage(worker);
  worker.postMessage(4);
  const reply = await pending as { kind: string; n: number; scripts: boolean };
  assert(reply.kind === "from-source" && reply.n === 5, JSON.stringify(reply));
  assert(reply.scripts === false, "module worker");
  worker.terminate();

  const explicit = new Worker(workerSource, { type: "module" });
  const explicitPending = nextMessage(explicit);
  explicit.postMessage(1);
  const explicitReply = await explicitPending as { n: number };
  assert(explicitReply.n === 2, String(explicitReply.n));
  explicit.terminate();
});

Yuzuko.test("Worker rejects a classic ModuleSource and a Wasm module", () => {
  let classic = "";
  try {
    new Worker(workerSource, { type: "classic" });
  } catch (error) {
    classic = error instanceof Error ? error.message : "";
  }
  assert(classic.includes("module"), classic);

  let wasmType = "";
  try {
    new Worker(wasm);
  } catch (error) {
    wasmType = error instanceof Error ? error.message : "";
  }
  assert(wasmType.includes("WebAssembly.Module"), wasmType);
});

Yuzuko.test("a module worker can import source", async () => {
  const worker = new Worker(script("source_inner.ts"), { type: "module" });
  const pending = nextMessage(worker);
  worker.postMessage(null);
  const reply = await pending as {
    tag: string;
    abstract: boolean;
    global: string;
  };
  assert(reply.tag === "[object ModuleSource]", reply.tag);
  assert(reply.abstract, "abstract");
  assert(reply.global === "undefined", reply.global);
  worker.terminate();
});

Yuzuko.test("postMessage clones a ModuleSource into another realm", async () => {
  const echo = new Worker(script("echo.js"));
  const pending = nextMessage(echo);
  echo.postMessage(workerSource);
  const back = await pending;
  assert(back === workerSource, "clone identity");
  echo.terminate();

  const worker = new Worker(back);
  const started = nextMessage(worker);
  worker.postMessage(8);
  const reply = await started as { n: number };
  assert(reply.n === 9, String(reply.n));
  worker.terminate();
});
