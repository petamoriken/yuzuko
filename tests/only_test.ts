Yuzuko.test("skipped", () => {
  throw new Error("should not run");
});

Yuzuko.test.only("focused", () => {});
