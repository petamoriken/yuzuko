self.onmessage = (event: MessageEvent) => {
  let scripts = true;
  try {
    importScripts("data:text/javascript,void 0");
  } catch {
    scripts = false;
  }
  self.postMessage({
    kind: "from-source",
    n: Number(event.data) + 1,
    scripts,
  });
};
