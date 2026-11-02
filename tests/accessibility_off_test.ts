function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

Yuzuko.test("accessible inject is absent without the feature", () => {
  const window = new Yuzuko.Window({ width: 80, height: 40 });
  try {
    let thrown: unknown;
    try {
      Yuzuko.__test.inject(window, [
        { kind: "accessible", type: "click", node: {} },
      ]);
    } catch (error) {
      thrown = error;
    }
    assert(thrown instanceof TypeError, "type");
    assert(
      (thrown as Error).message === "unknown inject kind accessible",
      (thrown as Error).message,
    );
  } finally {
    window.close();
  }
});
