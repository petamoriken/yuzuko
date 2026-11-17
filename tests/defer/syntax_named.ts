// @ts-expect-error Deferred named import is the negative test.
import defer { value } from "./value.ts";

void value;
