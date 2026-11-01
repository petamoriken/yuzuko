const names = [
  "fetch",
  "Headers",
  "Request",
  "Response",
  "caches",
  "XMLHttpRequest",
  "EventSource",
  "FormData",
  "WebSocket",
  "WebSocketStream",
  "WebTransport",
];
console.log(names.map((name) => typeof globalThis[name]).join(" "));
