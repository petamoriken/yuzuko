function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

Yuzuko.test("EventSource reconnects after the retry delay", async () => {
  const server = Yuzuko.listenHttp({ hostname: "127.0.0.1", port: 0 });
  const source = new EventSource(`http://127.0.0.1:${server.addr.port}/`);
  try {
    const stream = { "content-type": "text/event-stream" };
    const received: string[] = [];
    let errors = 0;
    source.onerror = () => {
      errors += 1;
    };
    const second = new Promise<void>((resolve) => {
      source.onmessage = (event) => {
        received.push(event.data);
        if (received.length === 2) {
          resolve();
        }
      };
    });
    const first = await server.accept();
    await first.respond("retry: 20\nid: 1\ndata: one\n\n", 200, stream);
    const again = await server.accept();
    await again.respond("data: two\n\n", 200, stream);
    await second;
    assert(received.join() === "one,two", received.join());
    assert(errors >= 1, `errors ${errors}`);
  } finally {
    source.close();
    server.close();
  }
});

Yuzuko.test("EventSource decodes invalid UTF-8 with replacement characters", async () => {
  const server = Yuzuko.listenHttp({ hostname: "127.0.0.1", port: 0 });
  const source = new EventSource(`http://127.0.0.1:${server.addr.port}/`);
  try {
    const message = new Promise<string>((resolve) => {
      source.onmessage = (event) => resolve(event.data);
    });
    const request = await server.accept();
    const body = Uint8Array.from([
      ...new TextEncoder().encode("data: a"),
      0xff,
      ...new TextEncoder().encode("b\n\n"),
    ]);
    await request.respond(body, 200, { "content-type": "text/event-stream" });
    const data = await message;
    assert(data === "a\uFFFDb", JSON.stringify(data));
  } finally {
    source.close();
    server.close();
  }
});
