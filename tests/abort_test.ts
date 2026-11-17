Yuzuko.test("AbortSignal.any follows sources without retaining them", () => {
  const empty = AbortSignal.any([]);
  if (empty.aborted) {
    throw new Error("empty");
  }

  const controller = new AbortController();
  const followed = AbortSignal.any([controller.signal]);
  if (followed === controller.signal || followed.aborted) {
    throw new Error("fresh");
  }
  if (followed.reason !== undefined) {
    throw new Error("reason before abort");
  }
  let saw = false;
  followed.onabort = (event) => {
    if (event.target !== followed) {
      throw new Error("target");
    }
    saw = true;
  };
  controller.abort("because");
  if (!saw || !followed.aborted || followed.reason !== "because") {
    throw new Error("follow");
  }

  const first = new AbortController();
  const second = new AbortController();
  first.abort("one");
  second.abort("two");
  const already = AbortSignal.any([
    first.signal,
    second.signal,
    first.signal,
  ]);
  if (!already.aborted || already.reason !== "one") {
    throw new Error("first reason");
  }

  const repeated = new AbortController();
  const once = AbortSignal.any([repeated.signal, repeated.signal]);
  repeated.abort("same");
  if (!once.aborted || once.reason !== "same") {
    throw new Error("duplicate");
  }

  const controllers = [0, 1, 2].map(() => new AbortController());
  const firstController = controllers[0];
  const secondController = controllers[1];
  const thirdController = controllers[2];
  if (!firstController || !secondController || !thirdController) {
    throw new Error("controllers");
  }
  const outer = AbortSignal.any([
    AbortSignal.any([firstController.signal, secondController.signal]),
    thirdController.signal,
  ]);
  secondController.abort();
  if (!outer.aborted || outer.reason.name !== "AbortError") {
    throw new Error("composed");
  }
});

Yuzuko.test("AbortSignal.any aborts dependents after the source event", () => {
  const controller = new AbortController();
  const signals = [];
  signals.push(controller.signal);
  signals.push(AbortSignal.any([controller.signal]));
  signals.push(AbortSignal.any([controller.signal]));
  const rootSignal = signals[0];
  const nestedSignal = signals[1];
  if (!rootSignal || !nestedSignal) {
    throw new Error("signals");
  }
  signals.push(AbortSignal.any([rootSignal]));
  signals.push(AbortSignal.any([nestedSignal]));
  let result = "";
  for (let index = 0; index < signals.length; index++) {
    const signal = signals[index];
    if (!signal) {
      throw new Error("signal");
    }
    signal.addEventListener("abort", () => {
      result += index;
    });
  }
  controller.abort("ordered");
  if (result !== "01234") {
    throw new Error(result);
  }
  const ordered = signals[4];
  if (!ordered || ordered.reason !== "ordered") {
    throw new Error("ordered reason");
  }

  const root = new AbortController();
  const inner = AbortSignal.any([root.signal]);
  const nested = AbortSignal.any([inner]);
  root.signal.addEventListener("abort", () => {
    const during = AbortSignal.any([nested]);
    if (!during.aborted || !inner.aborted || !nested.aborted) {
      throw new Error("during abort");
    }
    result = "during";
  });
  root.abort();
  // @ts-expect-error The earlier check narrowed result to "01234".
  if (result !== "during") {
    throw new Error("during listener");
  }
});
