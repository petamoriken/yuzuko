import defer from "./defer/default_value.ts";
import { events } from "./defer/log.ts";
import defer * as valueNs from "./defer/value.ts";
import defer * as hidden from "./defer/then_export.ts";
import defer * as marker from "./defer/marker.ts";
import defer * as plainNs from "./defer/plain.js";
import defer * as thrown from "./defer/throws.ts";
import defer * as uses from "./defer/uses_tla.ts";
import defer * as direct from "./defer/tla_dep.ts";
import defer * as data from "./defer/data.json" with { type: "json" };
import defer * as bad from "./defer/invalid.json" with { type: "json" };
import defer * as wasm from "./wasm/add.wasm";
import defer * as side from "./defer/calls_side.wasm";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function count(name: string): number {
  return events.filter((event) => event === name).length;
}

function namespaceInstance(namespace: object): unknown {
  const webAssembly = WebAssembly as unknown as {
    namespaceInstance(value: object): unknown;
  };
  return webAssembly.namespaceInstance(namespace);
}

function throws(fn: () => unknown): unknown {
  try {
    fn();
  } catch (error) {
    return error;
  }
  return undefined;
}

Yuzuko.test("deferred modules stay unevaluated until observation", () => {
  assert(events.includes("default"), "default import evaluated at load");
  assert(defer === 5, "default binding");
  assert(count("default") === 1, "default once");
  assert(events.includes("tla"), "top-level await dependency evaluated at load");
  assert(!events.includes("value"), "value deferred");
  assert(!events.includes("then"), "then export deferred");
  assert(!events.includes("marker"), "marker deferred");
  assert(!events.includes("plain"), "javascript deferred");
  assert(!events.includes("uses_tla"), "sync importer deferred");

  assert(valueNs[Symbol.toStringTag] === "Deferred Module", "tag");
  assert(
    Object.prototype.toString.call(valueNs) === "[object Deferred Module]",
    "toString",
  );
  assert(valueNs.then === undefined, "missing then");
  assert(!events.includes("value"), "tag and then did not evaluate");

  assert(valueNs.value === 1, "value");
  assert(count("value") === 1, "value once");
  assert(valueNs[Symbol.toStringTag] === "Deferred Module", "tag after evaluation");
  assert(
    Object.prototype.toString.call(valueNs) === "[object Deferred Module]",
    "toString after evaluation",
  );
});

Yuzuko.test("an exported then stays unevaluated until another key", () => {
  const early = throws(() => hidden.then);
  assert(early instanceof ReferenceError, String(early));
  assert(!events.includes("then"), "get then did not evaluate");
  assert("then" in hidden, "then is a binding");
  assert(!events.includes("then"), "has then did not evaluate");
  assert(hidden.other === 2, "other");
  assert(count("then") === 1, "other evaluated once");
  assert(hidden.then === 4, "then export");
  assert(count("then") === 1, "then read did not evaluate again");
});

Yuzuko.test("has and own keys evaluate a deferred module", () => {
  assert(!events.includes("plain"), "plain before has");
  assert("plain" in plainNs, "has");
  assert(events.includes("plain"), "has evaluated");
  assert(plainNs.plain === 9, "plain export");

  assert(!events.includes("marker"), "marker before keys");
  assert(Object.keys(marker).includes("kept"), "keys");
  assert(events.includes("marker"), "keys evaluated");
  assert(marker.kept === true, "kept");
});

Yuzuko.test("an evaluation error is cached on the deferred namespace", () => {
  const first = throws(() => thrown.missing);
  const second = throws(() => thrown.missing);
  assert(first instanceof Error, "threw");
  assert(String(first).includes("defer boom"), String(first));
  assert(first === second, "cached error");
});

Yuzuko.test("top-level await runs before the deferred importer", () => {
  assert(events.includes("tla"), "dependency already evaluated");
  assert(!events.includes("uses_tla"), "importer still deferred");
  assert(direct.n === 2, "deferred view of the evaluated dependency");
  assert(count("tla") === 1, "dependency once");
  assert(uses.doubled === 4, "observation");
  assert(count("uses_tla") === 1, "importer once");
  assert(count("tla") === 1, "dependency stayed evaluated");
});

Yuzuko.test("import.defer fulfills with the deferred namespace", async () => {
  const first = await import.defer("./defer/dyn.ts");
  assert(!events.includes("dyn"), "dynamic defer waits");
  assert(first.value === 3, "dyn value");
  assert(count("dyn") === 1, "dyn once");
  const second = await import.defer("./defer/dyn.ts");
  assert(second === first, "repeated import.defer");
  assert(count("dyn") === 1, "dyn still once");
  const shared = await import.defer("./defer/value.ts");
  assert(shared === valueNs, "same namespace as the static defer import");
  assert(count("value") === 1, "shared module stayed evaluated");
});

Yuzuko.test("import.defer rejects when top-level await throws", async () => {
  const caught = await import.defer("./defer/tla_throws.ts").then(
    () => undefined,
    (error: unknown) => error,
  );
  assert(caught instanceof Error, "rejected");
  assert(String(caught).includes("tla boom"), String(caught));
});

Yuzuko.test("deferred JSON is parsed on observation", async () => {
  assert(data[Symbol.toStringTag] === "Deferred Module", "json tag");
  assert(data.default.ok === true, "json value");
  const dynamic = await import.defer("./defer/data.json", {
    with: { type: "json" },
  });
  assert(dynamic === data, "same json namespace");
  const first = throws(() => bad.default);
  const second = throws(() => bad.default);
  assert(first instanceof SyntaxError, String(first));
  assert(first === second, "json error cached");
});

Yuzuko.test("deferred Wasm instantiates on observation", () => {
  const plain = throws(() => namespaceInstance({}));
  assert(plain instanceof TypeError, "plain object");
  const early = throws(() => namespaceInstance(wasm));
  assert(early instanceof TypeError, "before instantiation");
  assert(wasm.add(2, 3) === 5, "add");
  assert(wasm.answer === 7, "answer");
  const instance = namespaceInstance(wasm);
  assert(instance instanceof WebAssembly.Instance, "namespace instance");
  const again = throws(() => namespaceInstance({}));
  assert(again instanceof TypeError, "plain object after refresh");

  const first = throws(() => side.run);
  const second = throws(() => side.run);
  assert(String(first).includes("side effect ran"), String(first));
  assert(first === second, "wasm evaluation error cached");
});
