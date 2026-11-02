self.onmessage = () => {
  postMessage(
    [
      typeof AbortController,
      typeof fetch,
      typeof WebSocket,
      typeof Yuzuko.readFile,
      typeof Yuzuko.listenDatagram,
      typeof Yuzuko.Datagram,
      typeof Yuzuko.listenTcp,
      typeof Yuzuko.connectTcp,
      typeof Yuzuko.TcpListener,
      typeof Yuzuko.TcpConn,
      typeof Yuzuko.listenHttp,
      typeof Yuzuko.HttpServer,
      typeof Yuzuko.HttpRequest,
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
