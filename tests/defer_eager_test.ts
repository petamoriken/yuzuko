import * as eager from "./defer/value.ts";
import defer * as deferred from "./defer/value.ts";
import { events } from "./defer/log.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function count(name: string): number {
  return events.filter((event) => event === name).length;
}

Yuzuko.test("an eager import evaluates once and keeps a distinct namespace", () => {
  assert(count("value") === 1, "eager evaluation");
  assert(eager !== deferred, "distinct namespaces");
  assert(eager.value === 1, "eager value");
  assert(deferred.value === 1, "deferred value");
  assert(count("value") === 1, "still once");
});
