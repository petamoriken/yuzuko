Yuzuko.test("installed", () => {
  if (typeof Yuzuko.__test !== "object") {
    throw new Error("Yuzuko.__test is absent");
  }
  if (typeof Yuzuko.__test.inject !== "function") {
    throw new Error("missing inject");
  }
  if (typeof Yuzuko.__test.gamepad !== "function") {
    throw new Error("missing gamepad");
  }
});
