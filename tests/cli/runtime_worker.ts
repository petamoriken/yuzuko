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
      typeof Yuzuko.args,
      typeof Yuzuko.env,
      typeof Yuzuko.cwd,
      typeof Yuzuko.chdir,
      typeof Yuzuko.exit,
    ].join(" "),
  );
};
