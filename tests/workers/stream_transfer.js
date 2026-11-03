self.onmessage = (event) => {
  const data = event.data;
  const run = async () => {
    if (data.kind === "read") {
      const reader = data.stream.getReader();
      const first = await reader.read();
      let restDone = first.done === true;
      if (!restDone) {
        const second = await reader.read();
        restDone = second.done === true;
      }
      self.postMessage({
        kind: "read",
        value: first.value,
        done: first.done === true,
        restDone,
        stream: data.stream instanceof ReadableStream,
      });
      return;
    }
    if (data.kind === "write") {
      const writer = data.stream.getWriter();
      await writer.write("from-worker");
      await writer.close();
      self.postMessage({ kind: "wrote" });
      return;
    }
    if (data.kind === "send-readable") {
      const stream = new ReadableStream({
        start(controller) {
          controller.enqueue("from-worker");
          controller.close();
        },
      });
      self.postMessage({ kind: "readable", stream }, [stream]);
    }
  };
  run().catch((error) => {
    self.postMessage({
      kind: "error",
      name: error && error.name ? error.name : "Error",
      message: error && error.message ? String(error.message) : String(error),
    });
  });
};
