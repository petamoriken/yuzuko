const socket = Yuzuko.listenDatagram({ hostname: "127.0.0.1", port: 0 });
try {
  postMessage({ port: socket.addr.port });
  const message = await socket.receive();
  postMessage({
    hostname: message.hostname,
    port: message.port,
    bytes: Array.from(message.data),
  });
} catch (error) {
  postMessage({
    error: error instanceof Error ? `${error.name}:${error.message}` : String(error),
  });
} finally {
  try {
    socket.close();
  } catch {
    // The socket is already closed.
  }
}
