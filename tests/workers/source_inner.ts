import source mod from "../wasm/math.ts";

const AbstractModuleSource = Object.getPrototypeOf(WebAssembly.Module);

self.onmessage = () => {
  self.postMessage({
    tag: Object.prototype.toString.call(mod),
    abstract: mod instanceof AbstractModuleSource,
    global: typeof ModuleSource,
  });
};
