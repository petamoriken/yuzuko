import { events } from "./log.ts";
import { n } from "./tla_dep.ts";

events.push("uses_tla");

export const doubled = n * 2;
