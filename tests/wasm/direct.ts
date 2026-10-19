import { add, answer } from "./add.wasm";

if (add(2, 3) !== 5) {
  throw new Error("add returned " + add(2, 3));
}
if (answer !== 7) {
  throw new Error("answer was " + answer);
}
