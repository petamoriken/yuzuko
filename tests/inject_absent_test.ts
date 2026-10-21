Yuzuko.test("hidden", () => {
  if (typeof Yuzuko.__test !== "undefined") {
    throw new Error("Yuzuko.__test is installed");
  }
});
