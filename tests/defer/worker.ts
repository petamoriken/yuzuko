import { events } from "./log.ts";
import defer * as ns from "./value.ts";

declare var onmessage: ((event: MessageEvent) => void) | null;

onmessage = () => {
  const before = events.includes("value");
  const value = ns.value;
  const after = events.includes("value");
  self.postMessage(`${before}:${after}:${value}`);
};
