Yuzuko.test("microtask runs before a timer", async () => {
  const order: string[] = [];
  queueMicrotask(() => {
    order.push("micro");
  });
  await new Promise<void>((resolve) => {
    setTimeout(() => {
      order.push("timer");
      resolve();
    }, 0);
  });
  if (order.join(",") !== "micro,timer") {
    throw new Error(order.join(","));
  }
});
