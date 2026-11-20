function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function filePath(url: string): string {
  const pathname = decodeURIComponent(new URL(url).pathname);
  if (Yuzuko.build.os !== "windows") {
    return pathname;
  }
  const bare = pathname.startsWith("/") ? pathname.slice(1) : pathname;
  return bare.replaceAll("/", "\\");
}

// `path.dirname`: no trailing separator, except a root such as `/` or `C:\`.
function directoryName(path: string): string {
  const separator = Yuzuko.build.os === "windows" ? "\\" : "/";
  const index = path.lastIndexOf(separator);
  if (index < 0) {
    return path;
  }
  const root = separator === "\\" ? index <= 2 : index === 0;
  return path.slice(0, root ? index + 1 : index);
}

function thrown(run: () => void): { name: string; message: string } {
  try {
    run();
  } catch (error) {
    // @ts-expect-error A thrown value is unknown.
    const name: string = error.name;
    // @ts-expect-error A thrown value is unknown.
    const message: string = error.message;
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

Yuzuko.test("import.meta resolve, filename, and dirname", () => {
  const keys = ["url", "filename", "dirname", "resolve"];
  for (const key of keys) {
    const descriptor = Object.getOwnPropertyDescriptor(import.meta, key);
    assert(
      descriptor !== undefined &&
        descriptor.writable === true &&
        descriptor.enumerable === true &&
        descriptor.configurable === true,
      key,
    );
  }

  const filename = filePath(import.meta.url);
  assert(import.meta.filename === filename, import.meta.filename);
  assert(import.meta.dirname === directoryName(filename), import.meta.dirname);
  assert(import.meta.url.endsWith("/import_meta_test.ts"), import.meta.url);
  assert(
    Object.hasOwn(import.meta, "wasmInstances") === false,
    "wasmInstances",
  );

  const resolve = import.meta.resolve;
  assert(typeof resolve === "function", "function");
  assert(resolve.name === "resolve", resolve.name);
  assert(resolve.length === 1, String(resolve.length));
  assert(Object.getPrototypeOf(resolve) === Function.prototype, "prototype");

  const selfUrl = import.meta.resolve("./import_meta_test.ts");
  assert(selfUrl === import.meta.url, selfUrl);
  assert(import.meta.resolve(import.meta.url) === import.meta.url, "file url");
  const aliased = import.meta.resolve("./import_meta_test.js");
  assert(aliased === import.meta.url, aliased);
  const { resolve: extracted } = import.meta;
  assert(extracted("./import_meta_test.ts") === import.meta.url, "extracted");
  assert(
    extracted.call(null, "./import_meta_test.ts") === import.meta.url,
    "this",
  );
  assert(
    import.meta.resolve("./import_meta_test.ts", undefined) === import.meta.url,
    "undefined parent",
  );

  const workerFile = new URL("./workers/import_meta.js", import.meta.url).href;
  const wake = import.meta.resolve("./wake_lock.js", workerFile);
  assert(
    new URL(wake).pathname.endsWith("/workers/wake_lock.js"),
    wake,
  );
  const direct = import.meta.resolve("./workers/wake_lock.js");
  assert(new URL(direct).pathname.endsWith("/workers/wake_lock.js"), direct);

  const missingFile = new URL("./missing-import-meta.ts", import.meta.url).href;
  const normalized = import.meta.resolve(missingFile);
  assert(
    new URL(normalized).pathname.endsWith("/missing-import-meta.ts"),
    normalized,
  );

  let reads = 0;
  const specifier = {
    toString() {
      reads += 1;
      return "./import_meta_test.ts";
    },
  };
  // @ts-expect-error The specifier is converted with ToString.
  const viaToString = import.meta.resolve(specifier);
  assert(reads === 1, String(reads));
  assert(viaToString === import.meta.url, viaToString);

  let parentReads = 0;
  const parent = {
    toString() {
      parentReads += 1;
      return import.meta.url;
    },
  };
  const symbol = thrown(() => {
    // @ts-expect-error A symbol cannot be a specifier.
    import.meta.resolve(Symbol("x"), parent);
  });
  assert(symbol.name === "TypeError", symbol.name);
  assert(parentReads === 0, "parent was read");

  const custom = {
    toString() {
      throw new RangeError("custom");
    },
  };
  const customError = thrown(() => {
    // @ts-expect-error The specifier is converted with ToString.
    import.meta.resolve(custom);
  });
  assert(customError.name === "RangeError", customError.name);
  assert(customError.message === "custom", customError.message);

  const absent = thrown(() => {
    // @ts-expect-error resolve requires a specifier.
    import.meta.resolve();
  });
  assert(absent.name === "TypeError", absent.message);

  const relative = thrown(() => {
    import.meta.resolve("./missing-import-meta.ts");
  });
  assert(relative.name === "TypeError", relative.name);
  assert(
    relative.message.includes('failed to resolve "./missing-import-meta.ts"'),
    relative.message,
  );

  const bare = thrown(() => {
    import.meta.resolve("./wake_lock.js");
  });
  assert(bare.name === "TypeError", bare.message);

  const node = thrown(() => {
    import.meta.resolve("node:fs");
  });
  assert(
    node.message.includes("node: imports are not supported (node:fs)"),
    node.message,
  );

  const remote = thrown(() => {
    import.meta.resolve("https://example.com/");
  });
  assert(
    remote.message.includes('failed to resolve "https://example.com/"'),
    remote.message,
  );

  const remoteParent = thrown(() => {
    import.meta.resolve("./import_meta_test.ts", "https://example.com/a.js");
  });
  assert(
    remoteParent.message.includes("only file URLs can be imported"),
    remoteParent.message,
  );

  let constructed = false;
  const construct = thrown(() => {
    // @ts-expect-error import.meta.resolve is not a constructor.
    new resolve("./import_meta_test.ts");
    constructed = true;
  });
  assert(constructed === false, "constructed");
  assert(construct.name === "TypeError", construct.name);
});

Yuzuko.test("module workers expose import.meta paths", async () => {
  const worker = new Worker(
    new URL("./workers/import_meta.js", import.meta.url),
    { type: "module" },
  );
  const pending = nextMessage(worker);
  worker.postMessage(null);
  // @ts-expect-error The module worker reports its import.meta fields.
  const reply: {
    url: string;
    filename: string;
    dirname: string;
    resolved: string;
    aliased: string;
    length: number;
    name: string;
  } = await pending;
  worker.terminate();

  assert(reply.url.endsWith("/workers/import_meta.js"), reply.url);
  assert(reply.filename === filePath(reply.url), reply.filename);
  assert(reply.dirname === directoryName(reply.filename), reply.dirname);
  assert(reply.resolved === reply.url, reply.resolved);
  assert(
    new URL(reply.aliased).pathname.endsWith("/workers/wake_lock.js"),
    reply.aliased,
  );
  assert(reply.length === 1, String(reply.length));
  assert(reply.name === "resolve", reply.name);
});

Yuzuko.test("import.meta.url keeps a specifier fragment", async () => {
  const plain = await import("./import_meta_fragment.ts");
  const hashed = await import("./import_meta_fragment.ts#1");
  assert(plain.meta !== hashed.meta, `${plain.meta} vs ${hashed.meta}`);
  assert(plain.meta.endsWith("/import_meta_fragment.ts"), plain.meta);
  assert(hashed.meta.endsWith("/import_meta_fragment.ts#1"), hashed.meta);
  assert(plain.file === hashed.file, hashed.file);
  assert(plain.file.includes("#") === false, plain.file);
  const resolved = import.meta.resolve("./import_meta_fragment.ts#1");
  assert(resolved === hashed.meta, resolved);
});
