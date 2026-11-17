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

let held = null;
let sawPortError = false;

self.onmessageerror = (event) => {
  self.postMessage({ kind: "global-messageerror", ...detail(event) });
};

self.onerror = () => {
  self.postMessage({ kind: "worker-error" });
};

self.onmessage = (event) => {
  const data = event.data;
  if (data && data.kind === "echo-module") {
    self.postMessage(new WebAssembly.Module(BYTES));
    return;
  }
  if (data && data.kind === "hold-port") {
    held = data.port;
    sawPortError = false;
    held.onmessageerror = (messageError) => {
      sawPortError = true;
      self.postMessage({ kind: "port-messageerror", ...detail(messageError) });
    };
    self.postMessage({
      kind: "holding",
      handler: held.onmessageerror !== null,
    });
    return;
  }
  if (data && data.kind === "release-port") {
    self.postMessage({ kind: "before-start", seen: sawPortError });
    held.start();
    return;
  }
  if (data && data.kind === "ping") {
    self.postMessage({ kind: "pong" });
    return;
  }
  if (typeof MessagePort !== "undefined" && data instanceof MessagePort) {
    let seen = false;
    data.onmessageerror = (messageError) => {
      seen = true;
      self.postMessage({ kind: "queued-messageerror", ...detail(messageError) });
    };
    self.postMessage({ kind: "queued-before-start", seen });
    data.start();
    return;
  }
  self.postMessage({ kind: "message" });
};
