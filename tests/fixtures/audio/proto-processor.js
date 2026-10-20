class ProtoProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    const assigned = {};
    Object.assign(assigned, JSON.parse('{"__proto__": null}'));
    const literal = { __proto__: null };
    const written = {};
    written.__proto__ = { marker: true };
    const reflected = {};
    Reflect.set(reflected, "__proto__", null);
    const replaced = {};
    Object.setPrototypeOf(replaced, null);
    this.port.postMessage({
      descriptor: Object.getOwnPropertyDescriptor(Object.prototype, "__proto__") === undefined,
      inherited: "__proto__" in Object.prototype,
      readUndefined: ({}).__proto__ === undefined,
      assignPrototype: Object.getPrototypeOf(assigned) === Object.prototype,
      assignOwn: Object.hasOwn(assigned, "__proto__") && assigned.__proto__ === null,
      literalNull: Object.getPrototypeOf(literal) === null,
      literalOwn: Object.hasOwn(literal, "__proto__"),
      writePrototype: Object.getPrototypeOf(written) === Object.prototype,
      writeOwn: Object.hasOwn(written, "__proto__") && written.__proto__.marker === true,
      clean: ({}).marker === undefined,
      reflectPrototype: Object.getPrototypeOf(reflected) === Object.prototype,
      reflectOwn: Object.hasOwn(reflected, "__proto__") && reflected.__proto__ === null,
      setPrototypeOf: Object.getPrototypeOf(replaced) === null,
      created: Object.getPrototypeOf(Object.create(null)) === null,
      toString: typeof Object.prototype.toString === "function",
    });
  }

  process() {
    return false;
  }
}

registerProcessor("proto", ProtoProcessor);
