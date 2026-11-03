import defer * as ns from "./value.ts";
import { events } from "./log.ts";

if (events.includes("value")) {
  throw new Error("evaluated early");
}
if (ns.value !== 1) {
  throw new Error("value " + ns.value);
}
if (!events.includes("value")) {
  throw new Error("not evaluated");
}
