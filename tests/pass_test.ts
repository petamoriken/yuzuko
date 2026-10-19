Yuzuko.test("addition", () => {
  if (1 + 1 !== 2) {
    throw new Error("bad sum");
  }
});

Yuzuko.test("step", (t) => {
  t.step("inner", () => {
    if (typeof Yuzuko.build.os !== "string") {
      throw new Error("missing os");
    }
  });
});
