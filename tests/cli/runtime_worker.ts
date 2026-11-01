self.onmessage = () => {
  postMessage(
    [
      typeof AbortController,
      typeof fetch,
      typeof WebSocket,
      typeof Yuzuko.readFile,
      typeof Yuzuko.Window,
      typeof OffscreenCanvas,
      typeof requestAnimationFrame,
      typeof requestIdleCallback,
    ].join(" "),
  );
};
