function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

const COMMAND = "The command must be a string.";
const EMPTY = "The command must not be empty.";
const OPTIONS = "Options must be an object.";
const ARGS = "The arguments must be an array of strings.";
const NUL = "The command and arguments must not contain a null character.";
const SPAWN_FAIL = "The child process could not be started.";
const ILLEGAL = "Illegal constructor";
const INVOCATION = "illegal invocation";
const NOT_COLLECTED = "The child output is not collected.";
const STDIN_MODE =
  'The standard input must be "null", "inherit", or "piped".';
const STREAM_MODE =
  'The standard stream must be "null", "inherit", "piped", or "collect".';
const STDIN_NOT_PIPED = "The standard input is not piped.";
const STDOUT_NOT_PIPED = "The standard output is not piped.";
const STDERR_NOT_PIPED = "The standard error is not piped.";
const STDIN_CLOSED = "The standard input is closed.";
const DATA = "The data must be a BufferSource.";
const DETACHED = "The buffer is detached.";
const ENV_TYPE = "The environment must be an object.";
const NAME_EMPTY = "The name must not be empty.";
const NAME_RESERVED = "The name must not contain `=` or NUL.";
const VALUE_TYPE = "The value must be a string or null.";
const VALUE_NUL = "The value must not contain NUL.";
const PATH_TYPE = "The path must be a string or a file URL.";
const PATH_SCHEME = "The URL scheme must be file.";
const PATH_NUL = "The path must not contain NUL.";
const DIR_MISSING = "The directory was not found.";
const DIR_KIND = "The path is not a directory.";

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

async function rejects(promise: Promise<unknown>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error("did not reject");
}

function decode(bytes: Uint8Array): string {
  return new TextDecoder().decode(bytes);
}

function shell(script: string): { command: string; args: string[] } {
  if (Yuzuko.build.os === "windows") {
    return { command: "cmd.exe", args: ["/d", "/c", script] };
  }
  return { command: "/bin/sh", args: ["-c", script] };
}

function exitCommand(): { command: string; args: string[] } {
  if (Yuzuko.build.os === "windows") {
    return { command: "cmd.exe", args: ["/d", "/c", "exit 0"] };
  }
  return { command: "/bin/sh", args: ["-c", "exit 0"] };
}

function directoryCommand(): { command: string; args: string[] } {
  if (Yuzuko.build.os === "windows") {
    return { command: "cmd.exe", args: ["/d", "/c", "cd"] };
  }
  return { command: "/bin/pwd", args: [] };
}

function stdinCommand(): { command: string; args: string[] } {
  if (Yuzuko.build.os === "windows") {
    return {
      command: "cmd.exe",
      args: ["/v:on", "/d", "/c", "set /p LINE=& echo !LINE!-ok"],
    };
  }
  return {
    command: "/bin/sh",
    args: ["-c", 'read line; printf %s "$line-ok"'],
  };
}

function join(dir: string, name: string): string {
  const sep = Yuzuko.build.os === "windows" ? "\\" : "/";
  const base = dir.endsWith(sep) ? dir.slice(0, -1) : dir;
  return base + sep + name;
}

function samePath(actual: string, expected: string): boolean {
  const normalize = (path: string) =>
    path.trim().replaceAll("\\", "/").replace(/\/+$/, "");
  const left = normalize(actual);
  const right = normalize(expected);
  if (Yuzuko.build.os === "windows") {
    return left.toLowerCase() === right.toLowerCase();
  }
  return left === right;
}

function fileUrl(path: string): URL {
  const separator = Yuzuko.build.os === "windows" ? "\\" : "/";
  const parts = path.split(separator).filter((part) => part.length > 0);
  let encoded = parts.map((part) => encodeURIComponent(part)).join("/");
  if (Yuzuko.build.os === "windows") {
    encoded = encoded.replace(/^([A-Za-z])%3A/, "$1:");
  }
  return new URL(`file:///${encoded}`);
}

async function readAll(stream: ReadableStream<Uint8Array>): Promise<string> {
  const reader = stream.getReader();
  const chunks: Uint8Array[] = [];
  for (;;) {
    const step = await reader.read();
    if (step.done) {
      break;
    }
    chunks.push(step.value);
  }
  const size = chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
  const out = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return decode(out);
}

function outputCommand(): { command: string; args: string[] } {
  if (Yuzuko.build.os === "windows") {
    return {
      command: "cmd.exe",
      args: ["/d", "/c", "echo hello-child& echo err-child 1>&2& exit 3"],
    };
  }
  return {
    command: "/bin/sh",
    args: ["-c", "printf 'hello-child'; printf 'err-child' >&2; exit 3"],
  };
}

function sleepCommand(): { command: string; args: string[] } {
  if (Yuzuko.build.os === "windows") {
    return { command: "ping.exe", args: ["-n", "30", "127.0.0.1"] };
  }
  return { command: "/bin/sleep", args: ["30"] };
}

type ChildMessage = {
  ready?: boolean;
  code?: number | null;
  stdout?: string;
  error?: string;
};

Yuzuko.test("a child reports its exit code and stdout", async () => {
  assert(typeof spawn === "undefined", "global spawn");
  assert(typeof Child === "undefined", "global Child");
  assert(Yuzuko.spawn.length === 1, "spawn length");
  assert(Yuzuko.Child.length === 0, "Child length");
  const ctor = throws(() => {
    new (Yuzuko.Child as unknown as new () => unknown)();
  });
  assert(ctor instanceof TypeError, "ctor type");
  assert(text(ctor) === ILLEGAL, text(ctor));

  const launched = outputCommand();
  const child = Yuzuko.spawn(launched.command, { args: launched.args });
  assert(typeof child.pid === "number" && child.pid > 0, `pid ${child.pid}`);
  assert(child.output.length === 0, "output length");
  assert(child.kill.length === 0, "kill length");
  assert(child.status.length === 0, "status length");
  assert(child.readStdout.length === 1, "readStdout length");
  assert(child.readStderr.length === 1, "readStderr length");
  assert(child.writeStdin.length === 1, "writeStdin length");
  assert(child.closeStdin.length === 0, "closeStdin length");
  assert(child.stdin === null, "stdin stream");
  assert(child.stdout === null, "stdout stream");
  assert(child.stderr === null, "stderr stream");
  assert(child.stdinMode === "null", `stdin mode ${child.stdinMode}`);
  assert(child.stdoutMode === "collect", `stdout mode ${child.stdoutMode}`);
  assert(child.stderrMode === "collect", `stderr mode ${child.stderrMode}`);
  const first = child.output();
  const second = child.output();
  assert(first === second, "same promise");
  const output = await first;
  assert(output.code === 3, `code ${output.code}`);
  assert(output.signal === null, `signal ${String(output.signal)}`);
  assert(output.stdout instanceof Uint8Array, "stdout type");
  assert(output.stderr instanceof Uint8Array, "stderr type");
  const stdout = new TextDecoder().decode(output.stdout);
  const stderr = new TextDecoder().decode(output.stderr);
  if (Yuzuko.build.os === "windows") {
    assert(stdout.includes("hello-child"), `stdout ${stdout}`);
    assert(stderr.includes("err-child"), `stderr ${stderr}`);
  } else {
    assert(stdout === "hello-child", `stdout ${JSON.stringify(stdout)}`);
    assert(stderr === "err-child", `stderr ${JSON.stringify(stderr)}`);
  }
  const statusPromise = child.status();
  assert(statusPromise === child.status(), "same status");
  const status = await statusPromise;
  assert(status.code === output.code, `status code ${status.code}`);
  assert(
    status.signal === output.signal,
    `status signal ${String(status.signal)}`,
  );

  const worker = new Worker(new URL("./workers/spawn.js", import.meta.url), {
    type: "module",
  });
  try {
    const inbox: ChildMessage[] = [];
    let waiting: ((value: ChildMessage) => void) | undefined;
    let failing: ((error: Error) => void) | undefined;
    worker.onmessage = (event) => {
      const value = event.data as ChildMessage;
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
      return new Promise<ChildMessage>((resolve, reject) => {
        const timer = setTimeout(() => {
          waiting = undefined;
          failing = undefined;
          reject(new Error("child worker timed out"));
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
    worker.postMessage(launched);
    const reply = await nextMessage();
    assert(reply.error === undefined, `worker error ${reply.error}`);
    assert(reply.code === 3, `worker code ${reply.code}`);
    if (Yuzuko.build.os === "windows") {
      assert(
        reply.stdout?.includes("hello-child") === true,
        `worker stdout ${reply.stdout}`,
      );
    } else {
      assert(reply.stdout === "hello-child", `worker stdout ${reply.stdout}`);
    }
  } finally {
    worker.terminate();
  }
});

Yuzuko.test("kill ends the child and the process stays alive", async () => {
  const launched = sleepCommand();
  const child = Yuzuko.spawn(launched.command, { args: launched.args });
  try {
    child.kill();
    child.kill();
    const output = await child.output();
    if (Yuzuko.build.os === "windows") {
      assert(output.signal === null, `signal ${String(output.signal)}`);
      assert(output.code === 1, `code ${output.code}`);
    } else {
      assert(output.code === null, `code ${String(output.code)}`);
      assert(output.signal === 9, `signal ${String(output.signal)}`);
    }
  } finally {
    child.kill();
  }
});

Yuzuko.test("a bad command does not start a process", () => {
  const spawn = Yuzuko.spawn as (
    command?: unknown,
    options?: unknown,
  ) => Yuzuko.Child;

  assert(text(throws(() => spawn())) === "Not enough arguments", "arity");

  let hooked = false;
  const evil = {
    toString() {
      hooked = true;
      return "/bin/sh";
    },
  };
  const hook = throws(() => spawn(evil));
  assert(hook instanceof TypeError, "hook type");
  assert(text(hook) === COMMAND, text(hook));
  assert(hooked === false, "toString ran");

  const boxed = throws(() => spawn(Object("/bin/sh")));
  assert(boxed instanceof TypeError, "boxed type");
  assert(text(boxed) === COMMAND, text(boxed));

  assert(text(throws(() => spawn(""))) === EMPTY, "empty");
  assert(text(throws(() => spawn("sh", 1))) === OPTIONS, "options");
  assert(text(throws(() => spawn("sh", { args: "no" }))) === ARGS, "args");
  assert(
    text(throws(() => spawn("sh", { args: [Object("x")] }))) === ARGS,
    "boxed arg",
  );
  assert(text(throws(() => spawn("a\0b"))) === NUL, "nul");

  const missing = throws(() => spawn("/no/such/yuzuko-child-binary"));
  assert(missing instanceof Error, "missing type");
  assert(!(missing instanceof TypeError), "missing is TypeError");
  assert(text(missing) === SPAWN_FAIL, text(missing));
});

Yuzuko.test("spawn rejects bad streams, env, and cwd", () => {
  const spawn = Yuzuko.spawn as (
    command?: unknown,
    options?: unknown,
  ) => Yuzuko.Child;

  assert(
    text(throws(() => spawn("sh", { stdin: "collect" }))) === STDIN_MODE,
    "stdin collect",
  );
  assert(text(throws(() => spawn("sh", { stdin: 1 }))) === STDIN_MODE, "stdin number");
  assert(
    text(throws(() => spawn("sh", { stdout: "pipe" }))) === STREAM_MODE,
    "stdout pipe",
  );
  assert(
    text(throws(() => spawn("sh", { stdout: Object("piped") }))) === STREAM_MODE,
    "stdout boxed",
  );
  assert(
    text(throws(() => spawn("sh", { stderr: "nope" }))) === STREAM_MODE,
    "stderr nope",
  );
  assert(text(throws(() => spawn("sh", []))) === OPTIONS, "array options");

  let extra = false;
  const ignored: Record<string, unknown> = {};
  Object.defineProperty(ignored, "extra", {
    enumerable: true,
    get() {
      extra = true;
      return 1;
    },
  });
  const ignoredError = throws(() =>
    spawn("/no/such/yuzuko-child-binary", ignored),
  );
  assert(isError(ignoredError), `ignored type ${text(ignoredError)}`);
  assert(text(ignoredError) === SPAWN_FAIL, text(ignoredError));
  assert(extra === false, "unknown field was read");

  const envBoom = new Error("env");
  const envOptions: Record<string, unknown> = {};
  Object.defineProperty(envOptions, "env", {
    enumerable: true,
    get() {
      throw envBoom;
    },
  });
  assert(throws(() => spawn("sh", envOptions)) === envBoom, "env getter");

  assert(text(throws(() => spawn("sh", { env: [] }))) === ENV_TYPE, "env array");
  assert(
    text(throws(() => spawn("sh", { env: { "": "x" } }))) === NAME_EMPTY,
    "env empty",
  );
  assert(
    text(throws(() => spawn("sh", { env: { "A=B": "x" } }))) === NAME_RESERVED,
    "env equals",
  );
  assert(
    text(throws(() => spawn("sh", { env: { "A\0B": "x" } }))) === NAME_RESERVED,
    "env nul name",
  );
  let valueHook = false;
  const value = {
    toString() {
      valueHook = true;
      return "x";
    },
  };
  const valueError = throws(() => spawn("sh", { env: { A: value } }));
  assert(valueError instanceof TypeError, "env value type");
  assert(text(valueError) === VALUE_TYPE, text(valueError));
  assert(valueHook === false, "env value toString");
  assert(
    text(throws(() => spawn("sh", { env: { A: "a\0b" } }))) === VALUE_NUL,
    "env nul value",
  );

  assert(text(throws(() => spawn("sh", { cwd: 1 }))) === PATH_TYPE, "cwd number");
  assert(
    text(throws(() => spawn("sh", { cwd: "a\0b" }))) === PATH_NUL,
    "cwd nul",
  );
  const scheme = throws(() =>
    spawn("sh", { cwd: new URL("https://example.invalid/dir") }),
  );
  assert(scheme instanceof TypeError, "cwd scheme type");
  assert(text(scheme) === PATH_SCHEME, text(scheme));
  const looked = throws(() =>
    spawn("sh", { cwd: "https://example.invalid/dir" }),
  );
  assert(isError(looked), `cwd string url ${text(looked)}`);
  assert(text(looked) === DIR_MISSING, text(looked));
  const absent = throws(() => spawn("sh", { cwd: "/no/such/yuzuko-child-dir" }));
  assert(isError(absent), `cwd missing ${text(absent)}`);
  assert(text(absent) === DIR_MISSING, text(absent));
  const file = Yuzuko.makeTempFileSync({ prefix: "yuzuko-child-" });
  const kind = throws(() => spawn("sh", { cwd: file }));
  assert(isError(kind), `cwd file ${text(kind)}`);
  assert(text(kind) === DIR_KIND, text(kind));

  assert(
    text(throws(() => Yuzuko.Child.prototype.kill.call({}))) === INVOCATION,
    "kill receiver",
  );
  assert(
    text(
      throws(() =>
        Yuzuko.Child.prototype.readStdout.call({}, new Uint8Array(1)),
      ),
    ) === INVOCATION,
    "read receiver",
  );
  assert(
    text(
      throws(() =>
        Yuzuko.Child.prototype.writeStdin.call({}, new Uint8Array(1)),
      ),
    ) === INVOCATION,
    "write receiver",
  );
  assert(
    text(throws(() => Yuzuko.Child.prototype.closeStdin.call({}))) ===
      INVOCATION,
    "close receiver",
  );
  assert(
    text(throws(() => Yuzuko.Child.prototype.status.call({}))) === INVOCATION,
    "status receiver",
  );
  assert(
    text(throws(() => Yuzuko.Child.prototype.output.call({}))) === INVOCATION,
    "output receiver",
  );
  const stdinGetter = Object.getOwnPropertyDescriptor(
    Yuzuko.Child.prototype,
    "stdin",
  )?.get;
  assert(typeof stdinGetter === "function", "stdin getter");
  assert(text(throws(() => stdinGetter?.call({}))) === INVOCATION, "stdin receiver");
});

Yuzuko.test("cwd and env are applied to the child", async () => {
  Yuzuko.env.set("YUZUKO_CHILD_KEEP", "kept");
  Yuzuko.env.set("YUZUKO_CHILD_DROP", "gone");
  const saved = Yuzuko.cwd();
  const root = Yuzuko.makeTempDirSync({ prefix: "yuzuko-child-cwd-" });
  try {
    const overlayScript =
      Yuzuko.build.os === "windows"
        ? "echo %YUZUKO_CHILD_KEEP%-%YUZUKO_CHILD_EXTRA%& if defined YUZUKO_CHILD_HIDDEN (echo bad-hidden) else (echo clear-hidden)& if defined YUZUKO_CHILD_PROTO (echo bad-proto) else (echo clear-proto)"
        : 'printf %s "$YUZUKO_CHILD_KEEP-$YUZUKO_CHILD_EXTRA-[$YUZUKO_CHILD_HIDDEN]-[$YUZUKO_CHILD_PROTO]"';
    const overlay = shell(overlayScript);
    const env = Object.create({ YUZUKO_CHILD_PROTO: "proto" }) as {
      YUZUKO_CHILD_EXTRA?: string;
    };
    env.YUZUKO_CHILD_EXTRA = "extra";
    Object.defineProperty(env, "YUZUKO_CHILD_HIDDEN", {
      enumerable: false,
      value: "hidden",
    });
    const symbol = Symbol("yuzuko");
    (env as Record<symbol, string>)[symbol] = "no";
    const overChild = Yuzuko.spawn(overlay.command, {
      args: overlay.args,
      env,
    });
    const over = await overChild.output();
    const overText = decode(over.stdout);
    assert(over.code === 0, `overlay code ${over.code} ${overText}`);
    if (Yuzuko.build.os === "windows") {
      assert(overText.includes("kept-extra"), `overlay ${JSON.stringify(overText)}`);
      assert(overText.includes("clear-hidden"), `hidden ${overText}`);
      assert(overText.includes("clear-proto"), `proto ${overText}`);
    } else {
      assert(
        overText === "kept-extra-[]-[]",
        `overlay ${JSON.stringify(overText)}`,
      );
    }
    assert(Yuzuko.env.get("YUZUKO_CHILD_EXTRA") === undefined, "parent extra");

    const dropScript =
      Yuzuko.build.os === "windows"
        ? "if defined YUZUKO_CHILD_DROP (echo set) else (echo unset)"
        : 'if [ -n "${YUZUKO_CHILD_DROP+x}" ]; then printf set; else printf unset; fi';
    const drop = shell(dropScript);
    const dropChild = Yuzuko.spawn(drop.command, {
      args: drop.args,
      env: { YUZUKO_CHILD_DROP: null },
    });
    const dropped = await dropChild.output();
    const dropText = decode(dropped.stdout);
    assert(dropped.code === 0, `drop code ${dropped.code}`);
    assert(dropText.includes("unset"), `drop ${JSON.stringify(dropText)}`);
    assert(Yuzuko.env.get("YUZUKO_CHILD_DROP") === "gone", "parent drop");

    const listed = directoryCommand();
    const absolute = Yuzuko.spawn(listed.command, {
      args: listed.args,
      cwd: root,
    });
    const absoluteOutput = await absolute.output();
    assert(absoluteOutput.code === 0, `cwd code ${absoluteOutput.code}`);
    assert(
      samePath(decode(absoluteOutput.stdout), root),
      `cwd ${decode(absoluteOutput.stdout)} expected ${root}`,
    );
    const fromUrl = Yuzuko.spawn(listed.command, {
      args: listed.args,
      cwd: fileUrl(root),
    });
    const urlOutput = await fromUrl.output();
    assert(urlOutput.code === 0, `url cwd code ${urlOutput.code}`);
    assert(
      samePath(decode(urlOutput.stdout), root),
      `url cwd ${decode(urlOutput.stdout)} expected ${root}`,
    );

    const inherited = exitCommand();
    const inheritedChild = Yuzuko.spawn(inherited.command, {
      args: inherited.args,
      cwd: "",
    });
    const inheritedOutput = await inheritedChild.output();
    assert(inheritedOutput.code === 0, `empty cwd ${inheritedOutput.code}`);

    Yuzuko.chdir(root);
    Yuzuko.mkdirSync("nested");
    const relative = Yuzuko.spawn(listed.command, {
      args: listed.args,
      cwd: "nested",
    });
    const relativeOutput = await relative.output();
    const nested = join(root, "nested");
    assert(relativeOutput.code === 0, `relative code ${relativeOutput.code}`);
    assert(
      samePath(decode(relativeOutput.stdout), nested),
      `relative ${decode(relativeOutput.stdout)} expected ${nested}`,
    );
  } finally {
    Yuzuko.env.delete("YUZUKO_CHILD_KEEP");
    Yuzuko.env.delete("YUZUKO_CHILD_DROP");
    Yuzuko.env.delete("YUZUKO_CHILD_EXTRA");
    Yuzuko.env.delete("YUZUKO_CHILD_HIDDEN");
    Yuzuko.env.delete("YUZUKO_CHILD_PROTO");
    Yuzuko.chdir(saved);
  }
});

Yuzuko.test("null and inherit streams are empty", async () => {
  const launched = exitCommand();
  for (const mode of ["null", "inherit"] as const) {
    const child = Yuzuko.spawn(launched.command, {
      args: launched.args,
      stdin: mode === "inherit" ? "inherit" : "null",
      stdout: mode,
      stderr: mode,
    });
    try {
      const status = child.status();
      const output = child.output();
      assert(output === child.output(), "same output");
      const ended = await status;
      const collected = await output;
      assert(ended.code === 0, `${mode} status ${ended.code}`);
      assert(collected.code === 0, `${mode} code ${collected.code}`);
      assert(ended.signal === null, `${mode} signal ${String(ended.signal)}`);
      assert(collected.stdout.byteLength === 0, `${mode} stdout`);
      assert(collected.stderr.byteLength === 0, `${mode} stderr`);
      assert(child.stdin === null, `${mode} stdin stream`);
      assert(child.stdout === null, `${mode} stdout stream`);
      assert(child.stderr === null, `${mode} stderr stream`);
      assert(child.stdinMode === (mode === "inherit" ? "inherit" : "null"), mode);
      assert(child.stdoutMode === mode, `${mode} stdout mode`);
      assert(child.stderrMode === mode, `${mode} stderr mode`);
    } finally {
      child.kill();
    }
  }
});

Yuzuko.test("a piped standard input is collected with output()", async () => {
  const launched = stdinCommand();
  const child = Yuzuko.spawn(launched.command, {
    args: launched.args,
    stdin: "piped",
  });
  try {
    assert(child.stdinMode === "piped", "stdin mode");
    assert(child.stdoutMode === "collect", "stdout mode");
    assert(child.stdin instanceof WritableStream, "stdin stream");
    assert(child.stdin === child.stdin, "same stdin");
    assert(child.stdout === null, "stdout stream");
    const output = child.output();
    assert(output === child.output(), "same output");
    assert(await child.writeStdin(new Uint8Array(0)) === 0, "empty write");
    assert(await child.writeStdin(new SharedArrayBuffer(0)) === 0, "empty shared");
    assert(await child.writeStdin(new ArrayBuffer(0)) === 0, "empty buffer");
    const writer = child.stdin.getWriter();
    const line = new TextEncoder().encode("pipe\n");
    const holder = new Uint8Array(line.byteLength + 1);
    holder.set(line, 1);
    await writer.write(holder.subarray(1));
    await writer.close();
    const again = throws(() => child.closeStdin());
    assert(again instanceof TypeError, "second close type");
    assert(text(again) === STDIN_CLOSED, text(again));
    const collected = await output;
    const stdout = decode(collected.stdout);
    assert(collected.code === 0, `code ${collected.code} ${stdout}`);
    if (Yuzuko.build.os === "windows") {
      assert(stdout.includes("pipe-ok"), `stdout ${JSON.stringify(stdout)}`);
    } else {
      assert(stdout === "pipe-ok", `stdout ${JSON.stringify(stdout)}`);
    }
  } finally {
    try {
      child.closeStdin();
    } catch {
      // Already closed, or standard input was not piped.
    }
    child.kill();
  }

  const bad = Yuzuko.spawn(launched.command, {
    args: launched.args,
    stdin: "piped",
  });
  try {
    const writer = bad.stdin?.getWriter();
    assert(writer !== undefined, "bad writer");
    const error = await rejects(writer.write("nope"));
    assert(error instanceof TypeError, `chunk type ${text(error)}`);
    assert(text(error) === DATA, text(error));
  } finally {
    try {
      bad.closeStdin();
    } catch {
      // The stream may already have closed it.
    }
    bad.kill();
  }
});

Yuzuko.test("piped output is a stream and stderr can be read", async () => {
  const launched = shell(
    Yuzuko.build.os === "windows"
      ? "echo out& echo err-child 1>&2"
      : "printf out; printf err-child >&2",
  );
  const child = Yuzuko.spawn(launched.command, {
    args: launched.args,
    stdout: "piped",
  });
  try {
    const rejected = throws(() => child.output());
    assert(rejected instanceof TypeError, "output type");
    assert(text(rejected) === NOT_COLLECTED, text(rejected));
    assert(child.stdout instanceof ReadableStream, "stdout stream");
    assert(child.stdout === child.stdout, "same stdout");
    assert(child.stderr === null, "stderr stream");
    assert(child.stderrMode === "collect", `stderr mode ${child.stderrMode}`);
    assert(
      text(throws(() => child.readStderr(new Uint8Array(1)))) ===
        STDERR_NOT_PIPED,
      "stderr not piped",
    );
    const stdout = await readAll(child.stdout);
    const status = child.status();
    assert(status === child.status(), "same status");
    const ended = await status;
    assert(ended.code === 0, `code ${ended.code}`);
    if (Yuzuko.build.os === "windows") {
      assert(stdout.includes("out"), `stdout ${JSON.stringify(stdout)}`);
    } else {
      assert(stdout === "out", `stdout ${JSON.stringify(stdout)}`);
    }
  } finally {
    child.kill();
  }

  const errors = shell(
    Yuzuko.build.os === "windows"
      ? "echo err-only 1>&2"
      : "printf err-only >&2",
  );
  const errChild = Yuzuko.spawn(errors.command, {
    args: errors.args,
    stdout: "null",
    stderr: "piped",
  });
  try {
    const rejected = throws(() => errChild.output());
    assert(rejected instanceof TypeError, "stderr output type");
    assert(text(rejected) === NOT_COLLECTED, text(rejected));
    assert(errChild.stdout === null, "null stdout");
    assert(errChild.stderr instanceof ReadableStream, "stderr stream");
    const stderr = await readAll(errChild.stderr);
    const ended = await errChild.status();
    assert(ended.code === 0, `stderr code ${ended.code}`);
    if (Yuzuko.build.os === "windows") {
      assert(stderr.includes("err-only"), `stderr ${JSON.stringify(stderr)}`);
    } else {
      assert(stderr === "err-only", `stderr ${JSON.stringify(stderr)}`);
    }
  } finally {
    errChild.kill();
  }
});

Yuzuko.test("reads honor offsets, detachment, and the end of the pipe", async () => {
  const launched = shell(
    Yuzuko.build.os === "windows" ? "echo abcd" : "printf %s abcd",
  );
  const child = Yuzuko.spawn(launched.command, {
    args: launched.args,
    stdout: "piped",
    stderr: "null",
  });
  try {
    assert(
      text(throws(() => child.readStdout())) === "Not enough arguments",
      "read arity",
    );
    assert(
      text(throws(() => child.writeStdin())) === "Not enough arguments",
      "write arity",
    );
    assert(
      text(throws(() => child.writeStdin(new Uint8Array(1)))) === STDIN_NOT_PIPED,
      "stdin not piped",
    );
    assert(
      text(throws(() => child.closeStdin())) === STDIN_NOT_PIPED,
      "close not piped",
    );
    assert(
      text(throws(() => child.readStderr(new Uint8Array(1)))) ===
        STDERR_NOT_PIPED,
      "stderr not piped",
    );
    assert(text(throws(() => child.readStdout(1))) === DATA, "read type");

    const doomedBuffer = new ArrayBuffer(4);
    const doomed = new Uint8Array(doomedBuffer);
    structuredClone(doomed, { transfer: [doomedBuffer] });
    const detached = throws(() => child.readStdout(doomed));
    assert(detached instanceof TypeError, "detached type");
    assert(text(detached) === DETACHED, text(detached));

    const empty = new Uint8Array(0);
    assert(await child.readStdout(empty) === 0, "empty before");
    const storage = new Uint8Array(16);
    const view = storage.subarray(2);
    const first = await child.readStdout(view);
    assert(typeof first === "number" && first > 0, `first ${String(first)}`);
    assert(storage[0] === 0 && storage[1] === 0, "offset prefix");
    let got = first;
    while (got < view.byteLength) {
      const more = await child.readStdout(storage.subarray(2 + got));
      if (more === null) {
        break;
      }
      got += more;
    }
    const stdout = decode(storage.subarray(2, 2 + got));
    if (Yuzuko.build.os === "windows") {
      assert(stdout.includes("abcd"), `stdout ${JSON.stringify(stdout)}`);
    } else {
      assert(stdout === "abcd", `stdout ${JSON.stringify(stdout)}`);
    }
    assert(await child.readStdout(view) === null, "eof");
    assert(await child.readStdout(view) === null, "eof again");
    assert(await child.readStdout(empty) === 0, "empty after eof");
    const ended = await child.status();
    assert(ended.code === 0, `code ${ended.code}`);
  } finally {
    child.kill();
  }

  const sharedLaunch = shell(
    Yuzuko.build.os === "windows" ? "echo zz" : "printf %s zz",
  );
  const sharedChild = Yuzuko.spawn(sharedLaunch.command, {
    args: sharedLaunch.args,
    stdout: "piped",
    stderr: "null",
  });
  try {
    const buffer = new ArrayBuffer(1);
    const first = await sharedChild.readStdout(buffer);
    assert(first === 1, `array buffer ${String(first)}`);
    assert(new Uint8Array(buffer)[0] === 122, "array buffer byte");
    const shared = new SharedArrayBuffer(8);
    const count = await sharedChild.readStdout(shared);
    assert(typeof count === "number" && count > 0, `shared ${String(count)}`);
    assert(new Uint8Array(shared)[0] === 122, "shared byte");
    while ((await sharedChild.readStdout(buffer)) !== null) {
      // Drain the pipe so the child can exit.
    }
    const ended = await sharedChild.status();
    assert(ended.code === 0, `shared code ${ended.code}`);
  } finally {
    sharedChild.kill();
  }
});
