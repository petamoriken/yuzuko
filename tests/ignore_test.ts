Yuzuko.test.ignore("later", () => {
  throw new Error("ignored");
});

Yuzuko.test("runs", () => {});
