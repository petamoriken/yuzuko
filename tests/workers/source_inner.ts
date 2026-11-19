import source mod from "../wasm/math.ts";

const AbstractModuleSource = Object.getPrototypeOf(WebAssembly.Module);

// @ts-expect-error A worker global has onmessage.
self.onmessage = () => {
  self.postMessage({
    tag: Object.prototype.toString.call(mod),
    abstract: mod instanceof AbstractModuleSource,
    // @ts-expect-error ModuleSource is not a global.
    global: typeof ModuleSource,
  });
};
