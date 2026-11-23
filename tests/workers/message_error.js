const BYTES = new Uint8Array([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);

function detail(event) {
  return {
    data: event.data,
    type: event.type,
    trusted: event.isTrusted,
    bubbles: event.bubbles,
    cancelable: event.cancelable,
    messageEvent: event instanceof MessageEvent,
  };
}

function moduleFacts(value) {
  let instantiated = false;
  if (value instanceof WebAssembly.Module) {
    instantiated = new WebAssembly.Instance(value) instanceof WebAssembly.Instance;
  }
  return {
    module: value instanceof WebAssembly.Module,
    instantiated,
  };
}

let held = null;
let sawPortError = false;

self.onmessageerror = (event) => {
  self.postMessage({ kind: "global-messageerror", ...detail(event) });
};

self.onerror = () => {
  self.postMessage({ kind: "worker-error" });
};

self.onmessage = async (event) => {
  const data = event.data;
  if (data && data.kind === "echo-module") {
    self.postMessage(new WebAssembly.Module(BYTES));
    return;
  }
  if (data && data.kind === "same-ref") {
    self.postMessage({
      kind: "same-ref",
      modules:
        data.a instanceof WebAssembly.Module &&
        data.b instanceof WebAssembly.Module &&
        data.c instanceof WebAssembly.Module,
      ab: data.a === data.b,
      ac: data.a !== data.c,
      instantiated: new WebAssembly.Instance(data.a) instanceof WebAssembly.Instance,
    });
    return;
  }
  if (data && data.kind === "hold-port") {
    held = data.port;
    sawPortError = false;
    held.onmessageerror = () => {
      sawPortError = true;
      self.postMessage({ kind: "port-messageerror" });
    };
    self.postMessage({
      kind: "holding",
      handler: held.onmessageerror !== null,
    });
    return;
  }
  if (data && data.kind === "release-port") {
    self.postMessage({ kind: "before-start", seen: sawPortError });
    held.onmessage = (message) => {
      self.postMessage({
        kind: "port-message",
        ...moduleFacts(message.data),
        type: message.type,
        trusted: message.isTrusted,
        bubbles: message.bubbles,
        cancelable: message.cancelable,
        messageEvent: message instanceof MessageEvent,
      });
    };
    return;
  }
  if (data && data.kind === "ping") {
    self.postMessage({ kind: "pong" });
    return;
  }
  if (data && data.kind === "stream-module") {
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(new WebAssembly.Module(BYTES));
        controller.close();
      },
    });
    self.postMessage({ kind: "stream", stream }, [stream]);
    return;
  }
  if (data && data.kind === "read-stream") {
    try {
      const reader = data.stream.getReader();
      const first = await reader.read();
      const second = await reader.read();
      self.postMessage({
        kind: "read-stream",
        module: first.value instanceof WebAssembly.Module,
        done: first.done === false && second.done === true,
      });
    } catch (error) {
      self.postMessage({
        kind: "read-stream",
        module: false,
        done: false,
        value: `${error && error.name}: ${error && error.message}`,
      });
    }
    return;
  }
  if (typeof MessagePort !== "undefined" && data instanceof MessagePort) {
    let seen = false;
    data.onmessageerror = () => {
      seen = true;
      self.postMessage({ kind: "queued-messageerror" });
    };
    self.postMessage({ kind: "queued-before-start", seen });
    data.onmessage = (message) => {
      self.postMessage({
        kind: "queued-message",
        ...moduleFacts(message.data),
        type: message.type,
        trusted: message.isTrusted,
        bubbles: message.bubbles,
        cancelable: message.cancelable,
        messageEvent: message instanceof MessageEvent,
      });
    };
    return;
  }
  if (data instanceof WebAssembly.Module) {
    self.postMessage({
      kind: "global-module",
      ...detail(event),
      ...moduleFacts(data),
    });
    return;
  }
  self.postMessage({ kind: "message" });
};
