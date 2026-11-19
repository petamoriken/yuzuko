function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function throws(run: () => unknown, name: string): void {
  try {
    run();
  } catch (error) {
    const actual =
      error instanceof DOMException
        ? error.name
        : error instanceof TypeError
          ? "TypeError"
          : String(error);
    assert(actual === name, `${name} was ${actual}`);
    return;
  }
  throw new Error(`${name} was not thrown`);
}

function reset(): void {
  performance.clearMarks();
  performance.clearMeasures();
}

Yuzuko.test("user timing records marks and measures", () => {
  reset();
  const constructed = new PerformanceMark("ctor");
  assert(performance.getEntriesByName("ctor").length === 0, "constructor");
  assert(constructed instanceof PerformanceEntry, "instanceof entry");
  assert(constructed.id === 1, "constructor id");
  assert(constructed.navigationId === 0, "navigation id");
  const detail = { n: 1 };
  const cloned = performance.mark("detail", { detail, startTime: 1 });
  detail.n = 2;
  assert(cloned.detail.n === 1, "detail is a copy");
  assert(cloned.id === constructed.id + 1, "ids increase");
  assert(cloned.toJSON().id === cloned.id, "toJSON id");
  assert(cloned.toJSON().navigationId === 0, "toJSON navigationId");
  reset();
  performance.mark("m1", { startTime: 5 });
  performance.measure("s", { start: 5, duration: 0 });
  performance.mark("m2", { startTime: 5 });
  assert(
    performance.getEntries().map((entry) => entry.name).join(",") === "m1,m2,s",
    "equal startTime keeps mark order",
  );
  const marked = performance.mark("navigationStart");
  assert(marked.entryType === "mark", "timing name is a mark");
  throws(
    () => performance.measure("span", "navigationStart"),
    "TypeError",
  );
  const named = performance.measure("navigationStart");
  assert(named.entryType === "measure", "measure name");
  throws(() => performance.mark("negative", { startTime: -1 }), "TypeError");
  throws(() => performance.measure("missing", "absent-mark"), "SyntaxError");
  throws(
    () => performance.measure("bad", { start: 1, duration: -1 }),
    "TypeError",
  );
  performance.mark("late", { startTime: 10 });
  performance.mark("early", { startTime: 4 });
  const negative = performance.measure("back", "late", "early");
  assert(negative.duration < 0, "negative duration");
  performance.clearMarks();
  assert(performance.getEntriesByType("mark").length === 0, "marks cleared");
  assert(
    performance.getEntriesByName("s").length === 1,
    "measures remain",
  );
  performance.clearMeasures();
  assert(performance.getEntries().length === 0, "measures cleared");
  assert(performance.getEntriesByType("resource").length === 0, "no resource");
});

Yuzuko.test("PerformanceObserver reports marks on a later task", async () => {
  reset();
  const types = PerformanceObserver.supportedEntryTypes;
  assert(Object.isFrozen(types), "frozen supported types");
  assert(types.join(",") === "mark,measure", "supported types");
  assert(PerformanceObserver.supportedEntryTypes === types, "same array");
  let seen = false;
  const observer = new PerformanceObserver((list) => {
    seen = list.getEntries().some((entry) => entry.name === "later");
  });
  observer.observe({ type: "mark" });
  performance.mark("early");
  const buffered = await new Promise<string[]>((resolve) => {
    const previous = new PerformanceObserver((list) => {
      previous.disconnect();
      resolve(list.getEntries().map((entry) => entry.name));
    });
    previous.observe({ type: "mark", buffered: true });
  });
  assert(buffered.includes("early"), "buffered mark");
  performance.mark("later");
  await Promise.resolve();
  assert(!seen, "callback waits for a task");
  await new Promise<void>((resolve) => {
    setTimeout(resolve, 0);
  });
  assert(seen, "callback ran");
  const mode = new PerformanceObserver(() => {});
  mode.observe({ entryTypes: ["mark"] });
  mode.disconnect();
  throws(
    () => mode.observe({ type: "mark" }),
    "InvalidModificationError",
  );
  throws(
    () => mode.observe({ entryTypes: ["mark"], buffered: true }),
    "TypeError",
  );
  const taken = new PerformanceObserver(() => {
    throw new Error("takeRecords should empty the buffer");
  });
  taken.observe({ type: "measure" });
  performance.measure("taken");
  const records = taken.takeRecords();
  assert(records.length === 1, "takeRecords");
  const takenEntry = records[0];
  assert(takenEntry !== undefined, "taken entry");
  assert(takenEntry.name === "taken", "taken name");
  await new Promise<void>((resolve) => {
    setTimeout(resolve, 0);
  });
  observer.disconnect();
  mode.disconnect();
  taken.disconnect();
});
