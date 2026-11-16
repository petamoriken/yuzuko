self.onmessage = () => {
  const event = new MessageEvent("m");
  const channel = new MessageChannel();
  let name = "";
  try {
    event.initMessageEvent("m", false, false, null, "", "", self);
  } catch (error) {
    name = error instanceof TypeError ? "TypeError" : String(error);
  }
  const stored = event.source === self;
  event.initMessageEvent("m", false, false, null, "", "", channel.port1);
  self.postMessage({
    name,
    stored,
    source: event.source === null,
    port: event.source === channel.port1,
    dedicated: self instanceof DedicatedWorkerGlobalScope,
  });
};
