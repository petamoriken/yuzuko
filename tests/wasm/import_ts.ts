import { quad } from "./use_math.wasm";

if (quad(3) !== 12) {
  throw new Error("quad returned " + quad(3));
}
