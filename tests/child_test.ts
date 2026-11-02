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
