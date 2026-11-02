const listener = Yuzuko.listenTcp({ hostname: "127.0.0.1", port: 0 });
let conn;
try {
  postMessage({ port: listener.addr.port });
  conn = await listener.accept();
  const out = new Uint8Array(3);
  let offset = 0;
  while (offset < out.length) {
    const n = await conn.read(new Uint8Array(out.buffer, offset));
    if (n === null || n === 0) {
      throw new Error(`short read ${n} at ${offset}`);
    }
    offset += n;
  }
  postMessage({
    hostname: conn.remoteAddr.hostname,
    port: conn.remoteAddr.port,
    bytes: Array.from(out),
  });
} catch (error) {
  postMessage({
    error: error instanceof Error ? `${error.name}:${error.message}` : String(error),
  });
} finally {
  try {
    conn?.close();
  } catch {
    // The connection is already closed.
  }
  try {
    listener.close();
  } catch {
    // The listener is already closed.
  }
}
