function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

const ARITY = "Not enough arguments";
const SIGNAL = "The signal must be a string.";
const NAME = 'The signal must be "SIGINT" or "SIGTERM".';
const LISTENER = "The listener must be a function.";
const NOT_CAUGHT = "The signal is not caught.";

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

type SignalMessage = { ready?: boolean; hit?: boolean; error?: string };

Yuzuko.test("SIGINT and SIGTERM listeners run and the process stays alive", async () => {
  assert(typeof addSignalListener === "undefined", "global addSignalListener");
  assert(typeof removeSignalListener === "undefined", "global removeSignalListener");
  assert(typeof raiseSignal === "undefined", "global raiseSignal");
  assert(Yuzuko.addSignalListener.length === 2, "add length");
  assert(Yuzuko.removeSignalListener.length === 2, "remove length");
  assert(Yuzuko.raiseSignal.length === 1, "raise length");

  const seen: string[] = [];
  const onInt = () => {
    seen.push("int");
  };
  const onIntAgain = () => {
    seen.push("int-2");
  };
  let finishInt: () => void = () => {};
  const intDone = new Promise<void>((resolve) => {
    finishInt = resolve;
  });
  const markInt = () => {
    seen.push("int-done");
    finishInt();
  };
  const onTerm = () => {
    seen.push("term");
  };
  let finishTerm: () => void = () => {};
  const termDone = new Promise<void>((resolve) => {
    finishTerm = resolve;
  });
  const markTerm = () => {
    seen.push("term-done");
    finishTerm();
  };
  const removed = () => {
    seen.push("removed");
  };

  let worker: Worker | undefined;
  try {
    Yuzuko.addSignalListener("SIGINT", onInt);
    Yuzuko.addSignalListener("SIGINT", onInt);
    Yuzuko.addSignalListener("SIGINT", onIntAgain);
    Yuzuko.addSignalListener("SIGINT", markInt);

    worker = new Worker(new URL("./workers/signal.js", import.meta.url), {
      type: "module",
    });
    const inbox: SignalMessage[] = [];
    let waiting: ((value: SignalMessage) => void) | undefined;
    let failing: ((error: Error) => void) | undefined;
    worker.onmessage = (event) => {
      const value = event.data as SignalMessage;
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
      return new Promise<SignalMessage>((resolve, reject) => {
        const timer = setTimeout(() => {
          waiting = undefined;
          failing = undefined;
          reject(new Error("signal worker timed out"));
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
    Yuzuko.raiseSignal("SIGINT");
    await intDone;
    assert(seen.join(",") === "int,int-2,int-done", `seen ${seen.join(",")}`);
    const hit = await nextMessage();
    assert(hit.hit === true, `worker hit ${JSON.stringify(hit)}`);

    Yuzuko.addSignalListener("SIGTERM", removed);
    Yuzuko.addSignalListener("SIGTERM", onTerm);
    Yuzuko.addSignalListener("SIGTERM", markTerm);
    Yuzuko.removeSignalListener("SIGTERM", removed);
    Yuzuko.removeSignalListener("SIGTERM", () => {});
    Yuzuko.raiseSignal("SIGTERM");
    await termDone;
    assert(
      seen.join(",") === "int,int-2,int-done,term,term-done",
      `term ${seen.join(",")}`,
    );
  } finally {
    Yuzuko.removeSignalListener("SIGINT", onInt);
    Yuzuko.removeSignalListener("SIGINT", onIntAgain);
    Yuzuko.removeSignalListener("SIGINT", markInt);
    Yuzuko.removeSignalListener("SIGTERM", removed);
    Yuzuko.removeSignalListener("SIGTERM", onTerm);
    Yuzuko.removeSignalListener("SIGTERM", markTerm);
    worker?.terminate();
  }
});

Yuzuko.test("a bad signal does not terminate the process", () => {
  const add = Yuzuko.addSignalListener as (signal?: unknown, listener?: unknown) => void;
  const remove = Yuzuko.removeSignalListener as (
    signal?: unknown,
    listener?: unknown,
  ) => void;
  const raise = Yuzuko.raiseSignal as (signal?: unknown) => void;

  assert(text(throws(() => add())) === ARITY, "add arity");
  assert(text(throws(() => remove("SIGINT"))) === ARITY, "remove arity");
  assert(text(throws(() => raise())) === ARITY, "raise arity");

  const missing = throws(() => add(1, () => {}));
  assert(missing instanceof TypeError, "number type");
  assert(text(missing) === SIGNAL, text(missing));

  let hooked = false;
  const evil = {
    toString() {
      hooked = true;
      return "SIGINT";
    },
  };
  const hook = throws(() => add(evil, () => {}));
  assert(hook instanceof TypeError, "hook type");
  assert(text(hook) === SIGNAL, text(hook));
  assert(hooked === false, "toString ran");

  const boxed = throws(() => add(Object("SIGINT"), () => {}));
  assert(boxed instanceof TypeError, "boxed type");
  assert(text(boxed) === SIGNAL, text(boxed));

  const name = throws(() => add("SIGHUP", () => {}));
  assert(name instanceof TypeError, "name type");
  assert(text(name) === NAME, text(name));

  const listener = throws(() => add("SIGINT", "no"));
  assert(listener instanceof TypeError, "listener type");
  assert(text(listener) === LISTENER, text(listener));

  remove("SIGTERM", () => {});

  const open = throws(() => raise("SIGINT"));
  assert(open instanceof Error, "open type");
  assert(!(open instanceof TypeError), "open is TypeError");
  assert(text(open) === NOT_CAUGHT, text(open));
});
