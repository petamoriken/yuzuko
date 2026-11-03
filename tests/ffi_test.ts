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
    assert(add(-1, 1) === 0, `neg ${add(-1, 1)}`);
    assert(
      add(2147483647, 1) === -2147483648,
      `wrap ${add(2147483647, 1)}`,
    );
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
  const three = throws(() =>
    open(MISSING, {
      nope: { parameters: ["i32", "i32", "i32"], result: "i32" },
    }),
  );
  assert(isError(three), `three type ${text(three)}`);
  assert(text(three) === OPEN_FAIL, text(three));
  const four = throws(() =>
    open(MISSING, {
      nope: { parameters: ["i32", "i32", "i32", "i32"], result: "i32" },
    }),
  );
  assert(isError(four), `four type ${text(four)}`);
  assert(text(four) === OPEN_FAIL, text(four));
  const pointerResult = throws(() =>
    open(MISSING, { nope: { result: "pointer" } }),
  );
  assert(isError(pointerResult), `pointer result ${text(pointerResult)}`);
  assert(text(pointerResult) === OPEN_FAIL, text(pointerResult));
  const cstringResult = throws(() =>
    open(MISSING, { nope: { result: "cstring" } }),
  );
  assert(isError(cstringResult), `cstring result ${text(cstringResult)}`);
  assert(text(cstringResult) === OPEN_FAIL, text(cstringResult));
  const floatResult = throws(() => open(MISSING, { nope: { result: "f64" } }));
  assert(isError(floatResult), `float result ${text(floatResult)}`);
  assert(text(floatResult) === OPEN_FAIL, text(floatResult));
  const wide = ["i32", "i32", "i32", "i32", "i32"];
  let wideRead = false;
  Object.defineProperty(wide, "0", {
    get() {
      wideRead = true;
      return "i32";
    },
  });
  assert(
    text(
      throws(() => open(MISSING, { nope: { parameters: wide, result: "i32" } })),
    ) === UNSUPPORTED,
    "five",
  );
  assert(wideRead === false, "five read an element");
  const counted = ["i32", "i32", "i32", "i32"];
  const paramBoom = new Error("param");
  Object.defineProperty(counted, "0", {
    get() {
      throw paramBoom;
    },
  });
  assert(
    throws(() => open(MISSING, { nope: { parameters: counted, result: "i32" } })) ===
      paramBoom,
    "four reads an element",
  );
  assert(
    text(
      throws(() =>
        open(MISSING, {
          nope: { parameters: ["pointer", "f64"], result: "void" },
        }),
      ),
    ) === UNSUPPORTED,
    "pointer float",
  );
  assert(
    text(
      throws(() =>
        open(MISSING, { nope: { parameters: ["f64"], result: "pointer" } }),
      ),
    ) === UNSUPPORTED,
    "float pointer",
  );
  assert(
    text(throws(() => open(MISSING, { nope: { result: "buffer" } }))) ===
      UNSUPPORTED,
    "buffer result",
  );
  const mixedInteger = throws(() =>
    open(MISSING, {
      nope: {
        parameters: ["i32", "pointer", "buffer", "cstring"],
        result: "void",
      },
    }),
  );
  assert(isError(mixedInteger), `mixed integer ${text(mixedInteger)}`);
  assert(text(mixedInteger) === OPEN_FAIL, text(mixedInteger));
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

const WIDE = `#if defined(_WIN32)
#define YUZUKO_EXPORT __declspec(dllexport)
#else
#define YUZUKO_EXPORT
#endif

YUZUKO_EXPORT int yuzuko_fill(unsigned char *bytes, int length) {
  if (bytes == 0 || length < 4) {
    return -1;
  }
  bytes[0] = 1;
  bytes[1] = 2;
  bytes[2] = 3;
  bytes[3] = 4;
  return 4;
}

YUZUKO_EXPORT int yuzuko_length(const char *text) {
  if (text == 0) {
    return -1;
  }
  int length = 0;
  while (text[length] != 0) {
    length += 1;
  }
  return length;
}

YUZUKO_EXPORT int yuzuko_length_at(const char *text) {
  return yuzuko_length(text);
}

YUZUKO_EXPORT const char *yuzuko_word(void) {
  return "yuzuko";
}

YUZUKO_EXPORT const char *yuzuko_none(void) {
  return 0;
}

YUZUKO_EXPORT const char *yuzuko_bad_utf8(void) {
  return "a\\xff" "b";
}

YUZUKO_EXPORT void *yuzuko_identity(void *value) {
  return value;
}

static unsigned char yuzuko_slot[4];

YUZUKO_EXPORT void *yuzuko_store_ptr(void) {
  return yuzuko_slot;
}

YUZUKO_EXPORT int yuzuko_sum3(int a, int b, int c) {
  return a + b + c;
}

YUZUKO_EXPORT int yuzuko_sum4(int a, int b, int c, int d) {
  return a + b + c + d;
}

YUZUKO_EXPORT const char *yuzuko_echo(const char *text) {
  return text;
}
`;

const ARGUMENT = "The argument could not be converted.";
const DATA = "The data must be a BufferSource.";
const DETACHED = "The buffer is detached.";
const STRING_TYPE = "The string must be a string.";
const STRING_NUL = "The string must not contain a null character.";

Yuzuko.test("pointers, buffers, and C strings cross the call", async () => {
  const dir = Yuzuko.makeTempDirSync({ prefix: "yuzuko-ffi-wide-" });
  const source = join(dir, "wide.c");
  const library = join(dir, libraryName());
  Yuzuko.writeTextFileSync(source, WIDE);
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

  const lib = Yuzuko.dlopen(library, {
    yuzuko_fill: { parameters: ["buffer", "i32"], result: "i32" },
    yuzuko_length: { parameters: ["cstring"], result: "i32" },
    yuzuko_length_at: { parameters: ["pointer"], result: "i32" },
    yuzuko_word: { result: "cstring" },
    yuzuko_none: { result: "cstring" },
    yuzuko_bad_utf8: { result: "cstring" },
    yuzuko_identity: { parameters: ["pointer"], result: "pointer" },
    yuzuko_store_ptr: { result: "pointer" },
    yuzuko_sum3: { parameters: ["i32", "i32", "i32"], result: "i32" },
    yuzuko_sum4: { parameters: ["i32", "i32", "i32", "i32"], result: "i32" },
    yuzuko_echo: { parameters: ["cstring"], result: "cstring" },
  });
  let open = true;
  try {
    const fill = lib.symbols.yuzuko_fill;
    const length = lib.symbols.yuzuko_length;
    const lengthAt = lib.symbols.yuzuko_length_at;
    const word = lib.symbols.yuzuko_word;
    const none = lib.symbols.yuzuko_none;
    const badUtf8 = lib.symbols.yuzuko_bad_utf8;
    const identity = lib.symbols.yuzuko_identity;
    const store = lib.symbols.yuzuko_store_ptr;
    const sum3 = lib.symbols.yuzuko_sum3;
    const sum4 = lib.symbols.yuzuko_sum4;
    const echo = lib.symbols.yuzuko_echo;
    assert(fill.length === 2, "fill length");
    assert(sum3.length === 3, "sum3 length");
    assert(sum4.length === 4, "sum4 length");
    assert(word.length === 0, "word length");
    assert(identity.length === 1, "identity length");

    const raw = new Uint8Array(8);
    const view = raw.subarray(2);
    assert(fill(view, view.byteLength) === 4, "fill view");
    assert(raw[0] === 0 && raw[1] === 0, "fill left the prefix");
    assert(
      raw[2] === 1 && raw[3] === 2 && raw[4] === 3 && raw[5] === 4,
      `fill bytes ${raw.join(",")}`,
    );
    const short = new Uint8Array(4);
    assert(fill(short, 3) === -1, "fill short");
    assert(short[0] === 0 && short[1] === 0, "fill short wrote");

    const buffer = new ArrayBuffer(4);
    assert(fill(buffer, 4) === 4, "fill buffer");
    const bufferBytes = new Uint8Array(buffer);
    assert(
      bufferBytes[0] === 1 &&
        bufferBytes[1] === 2 &&
        bufferBytes[2] === 3 &&
        bufferBytes[3] === 4,
      "fill buffer bytes",
    );
    const shared = new SharedArrayBuffer(4);
    assert(fill(shared, 4) === 4, "fill shared");
    const sharedBytes = new Uint8Array(shared);
    assert(sharedBytes[0] === 1 && sharedBytes[3] === 4, "fill shared bytes");
    const empty = new Uint8Array(0);
    assert(fill(empty, 0) === -1, "fill empty");

    assert(length("yuzuko") === 6, "length");
    assert(length("") === 0, "empty string");
    assert(echo("hello") === "hello", "echo");
    assert(echo("") === "", "echo empty");
    assert(word() === "yuzuko", `word ${String(word())}`);
    assert(none() === null, `none ${String(none())}`);
    assert(badUtf8() === "a\uFFFDb", `utf8 ${badUtf8()}`);
    assert(sum3(20, 21, 1) === 42, "sum3");
    assert(sum4(1, 2, 3, 4) === 10, "sum4");
    assert(sum4(-1, 1, 0, 0) === 0, "sum4 sign");

    assert(identity(null) === 0n, "null pointer");
    assert(identity(0n) === 0n, "zero bigint");
    assert(identity(1) === 1n, "number one");
    assert(identity(0) === 0n, "number zero");
    assert(identity(-0) === 0n, "negative zero");
    assert(identity(4294967296) === 4294967296n, "high bit");
    assert(
      identity(Number.MAX_SAFE_INTEGER) === BigInt(Number.MAX_SAFE_INTEGER),
      "max safe",
    );
    const slot = store();
    assert(typeof slot === "bigint" && slot !== 0n, `slot ${String(slot)}`);
    assert(store() === slot, "stable slot");
    assert(identity(slot) === slot, "identity slot");
    assert(lengthAt(null) === -1, "length at null");
    assert(lengthAt(0n) === -1, "length at zero");
    assert(lengthAt(slot) === 0, "length at slot");
    let extra = false;
    const ignored = {
      valueOf() {
        extra = true;
        throw new Error("extra");
      },
    };
    assert(identity(1n, ignored as unknown as null) === 1n, "extra pointer");
    assert(extra === false, "extra pointer converted");

    assert(text(throws(() => identity(undefined))) === ARGUMENT, "undefined");
    assert(text(throws(() => identity(Object(1n)))) === ARGUMENT, "boxed bigint");
    assert(text(throws(() => identity(Object(1)))) === ARGUMENT, "boxed number");
    assert(text(throws(() => identity(-1))) === ARGUMENT, "negative");
    assert(text(throws(() => identity(1.5))) === ARGUMENT, "fraction");
    assert(text(throws(() => identity(2n ** 64n))) === ARGUMENT, "too big");
    assert(text(throws(() => identity(-1n))) === ARGUMENT, "negative bigint");

    let stringHook = false;
    const evil = {
      toString() {
        stringHook = true;
        return "hi";
      },
    };
    const stringError = throws(() => length(evil as unknown as string));
    assert(stringError instanceof TypeError, "cstring type");
    assert(text(stringError) === STRING_TYPE, text(stringError));
    assert(stringHook === false, "cstring toString");
    assert(text(throws(() => length(1 as unknown as string))) === STRING_TYPE, "cstring number");
    assert(
      text(throws(() => length("a\0b"))) === STRING_NUL,
      "cstring nul",
    );
    assert(text(throws(() => fill(null as unknown as Uint8Array, 4))) === DATA, "null buffer");

    const detachedBuffer = new ArrayBuffer(4);
    const detachedView = new Uint8Array(detachedBuffer);
    structuredClone(detachedView, { transfer: [detachedBuffer] });
    const detached = throws(() => fill(detachedView, 4));
    assert(detached instanceof TypeError, "detached type");
    assert(text(detached) === DETACHED, text(detached));

    const live = new ArrayBuffer(4);
    const liveView = new Uint8Array(live);
    const detaching = {
      valueOf() {
        structuredClone(liveView, { transfer: [live] });
        return 4;
      },
    };
    const detachedDuring = throws(() => fill(liveView, detaching as unknown as number));
    assert(detachedDuring instanceof TypeError, "detach during type");
    assert(text(detachedDuring) === DETACHED, text(detachedDuring));

    const fresh = new Uint8Array(4);
    const closing = {
      valueOf() {
        lib.close();
        open = false;
        return 4;
      },
    };
    const closed = throws(() => fill(fresh, closing as unknown as number));
    assert(isError(closed), `closed type ${text(closed)}`);
    assert(text(closed) === CLOSED, text(closed));
    const after = throws(() => word());
    assert(isError(after), `after close ${text(after)}`);
    assert(text(after) === CLOSED, text(after));
  } finally {
    if (open) {
      open = false;
      try {
        lib.close();
      } catch {
        // The assertion failure may already have closed it.
      }
    }
  }
});
