function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

const PATH_TYPE = "The path must be a string.";
const PATH_EMPTY = "The path must not be empty.";
const PATH_NUL = "The path must not contain a null character.";
const SYMBOLS = "The symbols must be an object.";
const NAME_EMPTY = "The symbol name must not be empty.";
const NAME_NUL = "The symbol name must not contain a null character.";
const TOO_MANY = "There are too many symbols.";
const DEFINITION = "The symbol definition must be an object.";
const UNSUPPORTED = "The function type is not supported.";
const OPEN_FAIL = "The library could not be opened.";
const CLOSED = "The library is closed.";
const ILLEGAL = "Illegal constructor";
const INVOCATION = "illegal invocation";
const MISSING = "/no/such/yuzuko-ffi-library";

const SOURCE = `#if defined(_WIN32)
#define YUZUKO_EXPORT __declspec(dllexport)
#else
#define YUZUKO_EXPORT
#endif

YUZUKO_EXPORT int yuzuko_add(int left, int right) {
  return left + right;
}

YUZUKO_EXPORT double yuzuko_scale(double value) {
  return value * 2.0;
}

static int yuzuko_note_value = 0;

YUZUKO_EXPORT void yuzuko_note(int value) {
  yuzuko_note_value = value;
}

YUZUKO_EXPORT int yuzuko_seen(void) {
  return yuzuko_note_value;
}
`;

function text(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function throws(run: () => void): unknown {
  try {
    run();
  } catch (error) {
    return error;
  }
  throw new Error("did not throw");
}

function isError(error: unknown): error is Error {
  return error instanceof Error && !(error instanceof TypeError);
}

function join(dir: string, name: string): string {
  const sep = Yuzuko.build.os === "windows" ? "\\" : "/";
  const base = dir.endsWith(sep) ? dir.slice(0, -1) : dir;
  return base + sep + name;
}

function libraryName(): string {
  if (Yuzuko.build.os === "windows") {
    return "add.dll";
  }
  if (Yuzuko.build.os === "linux") {
    return "libadd.so";
  }
  return "libadd.dylib";
}

function compileCommand(
  source: string,
  out: string,
): { command: string; args: string[] } {
  if (Yuzuko.build.os === "windows") {
    return { command: "clang", args: ["-shared", "-o", out, source] };
  }
  if (Yuzuko.build.os === "linux") {
    return { command: "cc", args: ["-shared", "-fPIC", "-o", out, source] };
  }
  if (Yuzuko.build.os === "darwin") {
    return { command: "clang", args: ["-dynamiclib", "-o", out, source] };
  }
  throw new Error(`no C compiler for ${Yuzuko.build.os}`);
}

type FfiMessage = {
  ready?: boolean;
  value?: number;
  error?: string;
};

Yuzuko.test("a loaded function returns the sum", async () => {
  assert(typeof dlopen === "undefined", "global dlopen");
  assert(typeof Library === "undefined", "global Library");
  assert(Yuzuko.dlopen.length === 2, "dlopen length");
  assert(Yuzuko.Library.length === 0, "Library length");
  const ctor = throws(() => {
    new (Yuzuko.Library as unknown as new () => unknown)();
  });
  assert(ctor instanceof TypeError, "ctor type");
  assert(text(ctor) === ILLEGAL, text(ctor));

  const dir = Yuzuko.makeTempDirSync({ prefix: "yuzuko-ffi-" });
  const source = join(dir, "add.c");
  const library = join(dir, libraryName());
  Yuzuko.writeTextFileSync(source, SOURCE);
  const command = compileCommand(source, library);
  const compiler = Yuzuko.spawn(command.command, { args: command.args });
  let compiled: Yuzuko.ChildOutput;
  try {
    compiled = await compiler.output();
  } finally {
    compiler.kill();
  }
  const stdout = new TextDecoder().decode(compiled.stdout);
  const stderr = new TextDecoder().decode(compiled.stderr);
  assert(
    compiled.code === 0,
    `compiler ${compiled.code} stdout ${stdout} stderr ${stderr}`,
  );

  const symbols = {
    yuzuko_add: { parameters: ["i32", "i32"], result: "i32" },
    yuzuko_scale: { parameters: ["f64"], result: "f64" },
    yuzuko_note: { parameters: ["i32"], result: "void" },
    yuzuko_seen: { result: "i32" },
  } as const;
  let lib: Yuzuko.Library | undefined = Yuzuko.dlopen(library, symbols);
  let open = true;
  let worker: Worker | undefined;
  try {
    assert(lib.symbols === lib.symbols, "same symbols");
    assert(lib.close.length === 0, "close length");
    const keys = Object.keys(lib.symbols);
    assert(
      keys.join(",") === "yuzuko_add,yuzuko_scale,yuzuko_note,yuzuko_seen",
      `keys ${keys.join(",")}`,
    );
    const add = lib.symbols.yuzuko_add;
    assert(add.length === 2, `add length ${add.length}`);
    assert(add.name === "yuzuko_add", `add name ${add.name}`);
    assert(lib.symbols.yuzuko_scale.length === 1, "scale length");
    assert(lib.symbols.yuzuko_note.length === 1, "note length");
    assert(lib.symbols.yuzuko_seen.length === 0, "seen length");
    assert(add(20, 22) === 42, `sum ${add(20, 22)}`);
    assert(lib.symbols.yuzuko_scale(21) === 42, "scale");
    assert(lib.symbols.yuzuko_note(7) === undefined, "note");
    assert(lib.symbols.yuzuko_seen() === 7, `seen ${lib.symbols.yuzuko_seen()}`);

    let calls = 0;
    const box = (value: number) => ({
      valueOf() {
        calls += 1;
        return value;
      },
    });
    assert(add(box(20), box(22)) === 42, "valueOf sum");
    assert(calls === 2, `valueOf calls ${calls}`);
    let extra = false;
    const ignored = {
      valueOf() {
        extra = true;
        throw new Error("extra");
      },
    };
    assert(add(20, 22, ignored as unknown as number) === 42, "extra");
    assert(extra === false, "extra converted");
    const boom = new Error("valueOf");
    const bad = {
      valueOf() {
        throw boom;
      },
    };
    const thrown = throws(() => {
      add(bad as unknown as number, 1);
    });
    assert(thrown === boom, "valueOf throw");
    assert(add(2, 3) === 5, "still open");
    assert(
      text(throws(() => add(1))) === "Not enough arguments",
      "symbol arity",
    );

    const missing = throws(() => {
      Yuzuko.dlopen(library, {
        yuzuko_add: { parameters: ["i32", "i32"], result: "i32" },
        yuzuko_missing: { parameters: ["i32"], result: "i32" },
      });
    });
    assert(isError(missing), `missing type ${text(missing)}`);
    assert(text(missing) === OPEN_FAIL, text(missing));
    assert(add(1, 2) === 3, "original after missing");

    worker = new Worker(new URL("./workers/ffi.js", import.meta.url), {
      type: "module",
    });
    const inbox: FfiMessage[] = [];
    let waiting: ((value: FfiMessage) => void) | undefined;
    let failing: ((error: Error) => void) | undefined;
    worker.onmessage = (event) => {
      const value = event.data as FfiMessage;
      if (waiting) {
        const resolve = waiting;
        waiting = undefined;
        resolve(value);
      } else {
        inbox.push(value);
      }
    };
    worker.onerror = (event) => {
      const error = new Error(String(event.message || "worker error"));
      if (failing) {
        failing(error);
      }
    };
    const nextMessage = () => {
      const queued = inbox.shift();
      if (queued) {
        return Promise.resolve(queued);
      }
      return new Promise<FfiMessage>((resolve, reject) => {
        const timer = setTimeout(() => {
          waiting = undefined;
          failing = undefined;
          reject(new Error("ffi worker timed out"));
        }, 5000);
        waiting = (value) => {
          clearTimeout(timer);
          failing = undefined;
          resolve(value);
        };
        failing = (error) => {
          clearTimeout(timer);
          waiting = undefined;
          reject(error);
        };
      });
    };
    const ready = await nextMessage();
    assert(ready.ready === true, `worker ready ${JSON.stringify(ready)}`);
    worker.postMessage({ path: library });
    const reply = await nextMessage();
    assert(reply.error === undefined, `worker error ${reply.error}`);
    assert(reply.value === 42, `worker value ${reply.value}`);

    const call = add;
    lib.close();
    open = false;
    assert(lib.symbols === lib.symbols, "symbols after close");
    const after = throws(() => {
      call(1, 1);
    });
    assert(isError(after), `after type ${text(after)}`);
    assert(text(after) === CLOSED, text(after));
    const again = throws(() => {
      lib?.close();
    });
    assert(isError(again), `again type ${text(again)}`);
    assert(text(again) === CLOSED, text(again));
    const invoked = throws(() => {
      Yuzuko.Library.prototype.close.call({});
    });
    assert(invoked instanceof TypeError, "invocation type");
    assert(text(invoked) === INVOCATION, text(invoked));
  } finally {
    worker?.terminate();
    if (open && lib) {
      open = false;
      try {
        lib.close();
      } catch {
        // The assertion failure may already have closed it.
      }
    }
  }
});

Yuzuko.test("a bad library does not stay open", () => {
  const open = Yuzuko.dlopen as (
    path?: unknown,
    symbols?: unknown,
  ) => Yuzuko.Library;

  assert(text(throws(() => open())) === "Not enough arguments", "arity");
  assert(text(throws(() => open("lib"))) === "Not enough arguments", "one");

  let hooked = false;
  const evil = {
    toString() {
      hooked = true;
      return "/lib";
    },
  };
  const hook = throws(() => open(evil, {}));
  assert(hook instanceof TypeError, "hook type");
  assert(text(hook) === PATH_TYPE, text(hook));
  assert(hooked === false, "toString ran");

  const boxed = throws(() => open(Object("/lib"), {}));
  assert(boxed instanceof TypeError, "boxed type");
  assert(text(boxed) === PATH_TYPE, text(boxed));
  assert(text(throws(() => open("", {}))) === PATH_EMPTY, "empty");
  assert(text(throws(() => open("a\0b", {}))) === PATH_NUL, "nul");
  assert(text(throws(() => open(MISSING, undefined))) === SYMBOLS, "undefined");
  assert(text(throws(() => open(MISSING, 1))) === SYMBOLS, "number");

  const boom = new Error("getter");
  const throwing: Record<string, unknown> = {};
  Object.defineProperty(throwing, "yuzuko_add", {
    enumerable: true,
    get() {
      throw boom;
    },
  });
  assert(throws(() => open(MISSING, throwing)) === boom, "getter");

  assert(
    text(throws(() => open(MISSING, { nope: 1 }))) === DEFINITION,
    "definition",
  );
  assert(
    text(throws(() => open(MISSING, { nope: [] }))) === DEFINITION,
    "array",
  );
  assert(
    text(
      throws(() =>
        open(MISSING, {
          nope: { parameters: ["i32", "f64"], result: "i32" },
        }),
      ),
    ) === UNSUPPORTED,
    "mixed",
  );
  assert(
    text(
      throws(() =>
        open(MISSING, {
          nope: { parameters: ["i32", "i32", "i32"], result: "i32" },
        }),
      ),
    ) === UNSUPPORTED,
    "three",
  );
  assert(
    text(throws(() => open(MISSING, { nope: { result: "u32" } }))) ===
      UNSUPPORTED,
    "u32",
  );
  let resultHook = false;
  const result = throws(() =>
    open(MISSING, {
      nope: {
        result: {
          toString() {
            resultHook = true;
            return "i32";
          },
        },
      },
    }),
  );
  assert(result instanceof TypeError, "result type");
  assert(text(result) === UNSUPPORTED, text(result));
  assert(resultHook === false, "result toString");
  assert(
    text(throws(() => open(MISSING, { "": { result: "i32" } }))) === NAME_EMPTY,
    "empty name",
  );
  assert(
    text(throws(() => open(MISSING, { "a\0b": { result: "i32" } }))) ===
      NAME_NUL,
    "nul name",
  );

  const many: Record<string, { result: "void" }> = {};
  for (let index = 0; index < 257; index += 1) {
    many[`s${index}`] = { result: "void" };
  }
  assert(text(throws(() => open(MISSING, many))) === TOO_MANY, "many");

  const hidden: Record<string, unknown> = {};
  Object.defineProperty(hidden, "bad", { enumerable: false, value: 1 });
  const absent = throws(() => open(MISSING, hidden));
  assert(isError(absent), `hidden type ${text(absent)}`);
  assert(text(absent) === OPEN_FAIL, text(absent));

  const missing = throws(() =>
    open(MISSING, { yuzuko_add: { parameters: ["i32"], result: "i32" } }),
  );
  assert(isError(missing), `missing file ${text(missing)}`);
  assert(text(missing) === OPEN_FAIL, text(missing));
});
