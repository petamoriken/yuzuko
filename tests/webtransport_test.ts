function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function assertThrows(run: () => unknown, name: string): void {
  try {
    run();
  } catch (error) {
    assert(error instanceof Error, String(error));
    assert((error as Error).name === name, (error as Error).name);
    return;
  }
  throw new Error(`${name} was not thrown`);
}

Yuzuko.test("datagram bytes mode accepts a BYOB reader", () => {
  const transport = new WebTransport("https://example.com", {
    datagramsReadableType: "bytes",
  });
  try {
    const reader = transport.datagrams.readable.getReader({ mode: "byob" });
    reader.releaseLock();
    const defaults = transport.datagrams.readable.getReader();
    defaults.releaseLock();
    assertThrows(
      () =>
        transport.incomingUnidirectionalStreams.getReader({ mode: "byob" }),
      "TypeError",
    );
    assertThrows(
      () =>
        transport.incomingBidirectionalStreams.getReader({ mode: "byob" }),
      "TypeError",
    );
  } finally {
    transport.close();
  }
});

Yuzuko.test("omitted datagramsReadableType rejects a BYOB reader", () => {
  const transport = new WebTransport("https://example.com");
  try {
    const reader = transport.datagrams.readable.getReader();
    reader.releaseLock();
    assertThrows(
      () => transport.datagrams.readable.getReader({ mode: "byob" }),
      "TypeError",
    );
  } finally {
    transport.close();
  }
});

Yuzuko.test("datagramsReadableType other than bytes throws TypeError", () => {
  assertThrows(
    () =>
      new WebTransport("https://example.com", {
        datagramsReadableType: "chunks",
      }),
    "TypeError",
  );
});
