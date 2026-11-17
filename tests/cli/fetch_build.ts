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
console.log(
  names
    .map((name) => {
      // @ts-expect-error The global object has no string index.
      return typeof globalThis[name];
    })
    .join(" "),
);
