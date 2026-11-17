// @ts-expect-error The missing module is the assertion under test.
import { missing } from "./does-not-exist.ts";

export const value = missing;
