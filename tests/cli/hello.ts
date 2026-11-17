import { greeting } from "./greeting.ts";
// @ts-expect-error Type-only import of a module that is not on disk.
import type { Missing } from "./missing.ts";

export type { Missing };

console.log(greeting);
