function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

Yuzuko.test("an idle timeout converts like an unsigned long", async () => {
  for (const timeout of [Infinity, -Infinity, NaN]) {
    const deadline = await new Promise<IdleDeadline>((resolve) => {
      requestIdleCallback(resolve, { timeout });
    });
    assert(deadline.didTimeout === false, `${timeout} has no timeout`);
  }
  cancelIdleCallback(Infinity);
  cancelIdleCallback(NaN);
});
