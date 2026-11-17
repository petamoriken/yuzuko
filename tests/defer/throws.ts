throw new Error("defer boom");

// @ts-expect-error The export stays for the runtime test after the throw.
export const missing = 1;
