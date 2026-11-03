import { bump } from "./counter.ts";

bump();
throw new Error("evaluated");
