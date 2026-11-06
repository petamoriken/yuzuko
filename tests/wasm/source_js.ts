import source mod from "./math.ts";
import source again from "./math.ts";
import { double } from "./math.ts";

const AbstractModuleSource = Object.getPrototypeOf(WebAssembly.Module);
if (AbstractModuleSource.name !== "AbstractModuleSource") {
  throw new Error("abstract name " + AbstractModuleSource.name);
}
if (typeof ModuleSource !== "undefined") {
  throw new Error("ModuleSource is a global");
}
if (!(mod instanceof AbstractModuleSource)) {
  throw new Error("not AbstractModuleSource");
}
if (mod instanceof WebAssembly.Module) {
  throw new Error("ModuleSource is a WebAssembly.Module");
}
if (Object.prototype.toString.call(mod) !== "[object ModuleSource]") {
  throw new Error(Object.prototype.toString.call(mod));
}
if (mod !== again) {
  throw new Error("source identity");
}
if (structuredClone(mod) !== mod) {
  throw new Error("structuredClone identity");
}
if (double(2) !== 4) {
  throw new Error("evaluation import");
}

const dyn = await import.source("./math.ts");
if (dyn !== mod) {
  throw new Error("import.source identity");
}

let evaluated = false;
try {
  await import(mod);
  evaluated = true;
} catch {
  evaluated = false;
}
if (evaluated) {
  throw new Error("import(moduleSource) resolved");
}
