import source mod from "./with_import.wasm";

if (!(mod instanceof WebAssembly.Module)) {
  throw new Error("import source did not compile a WebAssembly.Module");
}
