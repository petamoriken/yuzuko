Yuzuko.test("process arguments are empty", () => {
  if (JSON.stringify(Yuzuko.args) !== "[]") {
    throw new Error(JSON.stringify(Yuzuko.args));
  }
  if (typeof Yuzuko.cwd !== "function") {
    throw new Error("cwd");
  }
  if (typeof Yuzuko.chdir !== "function") {
    throw new Error("chdir");
  }
  if (typeof Yuzuko.exit !== "function") {
    throw new Error("exit");
  }
  if (typeof Yuzuko.env.get !== "function") {
    throw new Error("env");
  }
  if (Yuzuko.cwd().length === 0) {
    throw new Error("empty cwd");
  }
});
