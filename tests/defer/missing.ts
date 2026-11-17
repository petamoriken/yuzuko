// @ts-expect-error The missing module is the assertion under test.
import defer * as ns from "./not_a_module.ts";

void ns;
