function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

const NOT_SEQUENCE = "The key code list is not a sequence";
const TOO_LONG = "The key code list is too long";
const NO_FOCUS = "No window is focused";
const BAD_CODE = "The key code is not a valid code";

async function rejection(run: () => Promise<unknown>): Promise<{
  name: string;
  message: string;
}> {
  let sync = false;
  let promise: Promise<unknown> | undefined;
  try {
    promise = run();
  } catch (error) {
    sync = true;
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`sync throw: ${message}`);
  }
  assert(sync === false, "sync");
  try {
    await promise;
    throw new Error("expected a rejection");
  } catch (error) {
    if (error instanceof Error && error.message === "expected a rejection") {
      throw error;
    }
    return {
      name: error instanceof Error ? error.name : "",
      message: error instanceof Error ? error.message : "",
    };
  }
}

Yuzuko.test("keyboard.lock reserves codes for the focused window", async () => {
  assert(navigator.keyboard === navigator.keyboard, "same keyboard");
  assert(navigator.keyboard instanceof Keyboard, "Keyboard");
  assert(navigator.keyboard instanceof EventTarget, "EventTarget");
  assert(Keyboard.prototype.lock.length === 0, "lock length");
  assert(Keyboard.prototype.unlock.length === 0, "unlock length");

  let illegal = "";
  try {
    new Keyboard();
  } catch (error) {
    illegal = error instanceof TypeError ? error.message : "";
  }
  assert(illegal === "Illegal constructor", illegal);

  let missed = await rejection(() => navigator.keyboard.lock());
  assert(missed.name === "InvalidStateError", missed.name);
  assert(missed.message === NO_FOCUS, missed.message);

  const nullList = await rejection(() => navigator.keyboard.lock(null as never));
  assert(nullList.name === "TypeError", nullList.name);
  assert(nullList.message === NOT_SEQUENCE, nullList.message);

  const unknown = await rejection(() => navigator.keyboard.lock(["Nope"]));
  assert(unknown.name === "InvalidStateError", unknown.name);
  assert(unknown.message === NO_FOCUS, unknown.message);

  const quiet = new Yuzuko.Window({
    active: false,
    width: 32,
    height: 32,
    title: "keys",
  });
  try {
    missed = await rejection(() => navigator.keyboard.lock(["KeyW"]));
    assert(missed.name === "InvalidStateError", missed.name);

    quiet.focus();
    assert((await navigator.keyboard.lock()) === undefined, "lock all");
    assert(
      (await navigator.keyboard.lock(undefined)) === undefined,
      "undefined locks all",
    );
    assert((await navigator.keyboard.lock([])) === undefined, "empty list");
    assert(
      (await navigator.keyboard.lock(["KeyW", "Escape", "Tab"])) === undefined,
      "named codes",
    );
    assert(
      (await navigator.keyboard.lock(["KeyW", "KeyW"])) === undefined,
      "duplicate",
    );
    assert(
      (await navigator.keyboard.lock(new Set(["KeyA"]))) === undefined,
      "set",
    );
    const described = [{ toString() { return "Escape"; } }];
    assert(
      (await navigator.keyboard.lock(described)) === undefined,
      "toString",
    );

    const invalid = await rejection(() =>
      navigator.keyboard.lock(["KeyW", "Nope"]),
    );
    assert(invalid.name === "InvalidAccessError", invalid.name);
    assert(invalid.message === BAD_CODE, invalid.message);
    assert(
      (await navigator.keyboard.lock(["KeyA"])) === undefined,
      "lock after invalid",
    );

    const many = Array.from({ length: 1024 }, () => "KeyW");
    assert((await navigator.keyboard.lock(many)) === undefined, "1024");
    const extra = Array.from({ length: 1025 }, () => "KeyA");
    const tooLong = await rejection(() => navigator.keyboard.lock(extra));
    assert(tooLong.name === "TypeError", tooLong.name);
    assert(tooLong.message === TOO_LONG, tooLong.message);

    function* forever(): Generator<string> {
      while (true) {
        yield "KeyW";
      }
    }
    const endless = await rejection(() => navigator.keyboard.lock(forever()));
    assert(endless.name === "TypeError", endless.name);
    assert(endless.message === TOO_LONG, endless.message);

    const letters = await rejection(() =>
      navigator.keyboard.lock("KeyW" as never),
    );
    assert(letters.name === "InvalidAccessError", letters.name);
    assert(letters.message === BAD_CODE, letters.message);

    const boom = {
      [Symbol.iterator]() {
        return {
          next() {
            return {
              done: false,
              value: {
                toString() {
                  throw new TypeError("boom");
                },
              },
            };
          },
        };
      },
    };
    const thrown = await rejection(() => navigator.keyboard.lock(boom));
    assert(thrown.name === "TypeError", thrown.name);
    assert(thrown.message === "boom", thrown.message);

    let seen = "";
    quiet.addEventListener("keydown", (event) => {
      seen = event.code;
    });
    Yuzuko.__test.inject(quiet, [
      { kind: "key", type: "keydown", key: "w", code: "KeyW" },
    ]);
    assert(seen === "KeyW", seen);

    await navigator.keyboard.lock(["KeyW"]);
    Yuzuko.__test.inject(quiet, [{ kind: "blur" }]);
    const blurred = await rejection(() => navigator.keyboard.lock(["Nope"]));
    assert(blurred.name === "InvalidStateError", blurred.name);
    assert(blurred.message === NO_FOCUS, blurred.message);

    quiet.focus();
    assert(navigator.keyboard.unlock() === undefined, "unlock");
    assert(navigator.keyboard.unlock() === undefined, "unlock again");

    const other = new Yuzuko.Window({
      active: false,
      width: 32,
      height: 32,
    });
    try {
      other.focus();
      assert((await navigator.keyboard.lock(["Tab"])) === undefined, "other");
    } finally {
      other.close();
    }
    const closed = await rejection(() => navigator.keyboard.lock());
    assert(closed.name === "InvalidStateError", closed.name);
    assert(closed.message === NO_FOCUS, closed.message);
  } finally {
    quiet.close();
  }
});

Yuzuko.test("a dedicated worker has no keyboard lock", async () => {
  const worker = new Worker(
    new URL("./workers/keyboard_lock.js", import.meta.url).href,
  );
  const probe = await new Promise<{ keyboard: boolean; Keyboard: boolean }>(
    (resolve, reject) => {
      worker.onmessage = (event: MessageEvent) => {
        resolve(event.data as { keyboard: boolean; Keyboard: boolean });
      };
      worker.onerror = (event: ErrorEvent) => {
        reject(new Error(event.message || "worker error"));
      };
    },
  );
  worker.terminate();
  assert(probe.keyboard === false, "worker navigator.keyboard");
  assert(probe.Keyboard === false, "worker Keyboard");
});
