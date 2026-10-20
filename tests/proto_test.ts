function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

interface ProtoFacts {
  descriptor: boolean;
  inherited: boolean;
  readUndefined: boolean;
  assignPrototype: boolean;
  assignOwn: boolean;
  literalNull: boolean;
  literalOwn: boolean;
  writePrototype: boolean;
  writeOwn: boolean;
  clean: boolean;
  reflectPrototype: boolean;
  reflectOwn: boolean;
  setPrototypeOf: boolean;
  created: boolean;
  toString: boolean;
}

function protoFacts(): ProtoFacts {
  const assigned: { __proto__?: unknown } = {};
  Object.assign(assigned, JSON.parse('{"__proto__": null}'));
  const literal: { __proto__?: unknown } = { __proto__: null };
  const written: { __proto__?: { marker?: boolean } } = {};
  written.__proto__ = { marker: true };
  const reflected: { __proto__?: unknown } = {};
  Reflect.set(reflected, "__proto__", null);
  const replaced = {};
  Object.setPrototypeOf(replaced, null);
  return {
    descriptor: Object.getOwnPropertyDescriptor(Object.prototype, "__proto__") === undefined,
    inherited: "__proto__" in Object.prototype,
    readUndefined: ({} as { __proto__?: unknown }).__proto__ === undefined,
    assignPrototype: Object.getPrototypeOf(assigned) === Object.prototype,
    assignOwn: Object.hasOwn(assigned, "__proto__") && assigned.__proto__ === null,
    literalNull: Object.getPrototypeOf(literal) === null,
    literalOwn: Object.hasOwn(literal, "__proto__"),
    writePrototype: Object.getPrototypeOf(written) === Object.prototype,
    writeOwn: Object.hasOwn(written, "__proto__") && written.__proto__?.marker === true,
    clean: ({} as { marker?: boolean }).marker === undefined,
    reflectPrototype: Object.getPrototypeOf(reflected) === Object.prototype,
    reflectOwn: Object.hasOwn(reflected, "__proto__") && reflected.__proto__ === null,
    setPrototypeOf: Object.getPrototypeOf(replaced) === null,
    created: Object.getPrototypeOf(Object.create(null)) === null,
    toString: typeof Object.prototype.toString === "function",
  };
}

function assertFacts(facts: ProtoFacts, where: string): void {
  assert(facts.descriptor, `${where} descriptor`);
  assert(facts.inherited === false, `${where} inherited`);
  assert(facts.readUndefined, `${where} read`);
  assert(facts.assignPrototype, `${where} assign prototype`);
  assert(facts.assignOwn, `${where} assign own`);
  assert(facts.literalNull, `${where} literal`);
  assert(facts.literalOwn === false, `${where} literal own`);
  assert(facts.writePrototype, `${where} write prototype`);
  assert(facts.writeOwn, `${where} write own`);
  assert(facts.clean, `${where} clean`);
  assert(facts.reflectPrototype, `${where} reflect prototype`);
  assert(facts.reflectOwn, `${where} reflect own`);
  assert(facts.setPrototypeOf, `${where} setPrototypeOf`);
  assert(facts.created, `${where} create`);
  assert(facts.toString, `${where} toString`);
}

function script(name: string): string {
  return new URL(`./workers/${name}`, import.meta.url).href;
}

Yuzuko.test("Object.prototype.__proto__ is absent on the main realm and in a worker", async () => {
  assertFacts(protoFacts(), "main");

  const worker = new Worker(script("proto.js"));
  const facts = await new Promise<ProtoFacts>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => resolve(event.data);
    worker.onerror = (event: ErrorEvent) => {
      reject(new Error(event.message || "worker error"));
    };
  });
  assertFacts(facts, "worker");
  worker.terminate();
});

Yuzuko.test("Object.prototype.__proto__ is absent in an AudioWorklet", async () => {
  const context = new AudioContext();
  try {
    const source = new URL("./fixtures/audio/proto-processor.js", import.meta.url);
    await context.audioWorklet.addModule(source.href);
    const facts = await new Promise<ProtoFacts>((resolve) => {
      const node = new AudioWorkletNode(context, "proto");
      node.port.onmessage = (event: MessageEvent) => resolve(event.data);
    });
    assertFacts(facts, "worklet");
  } finally {
    await context.close();
  }
});
