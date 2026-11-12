const server = Yuzuko.listenHttp({ hostname: "127.0.0.1", port: 0 });
let request;
try {
  if (
    typeof Yuzuko.errors.Http !== "function" ||
    typeof Yuzuko.errors.BadResource !== "function" ||
    typeof Yuzuko.errors.NotCapable !== "function"
  ) {
    throw new Error("errors");
  }
  const pending = server.accept();
  postMessage({ port: server.addr.port });
  request = await pending;
  if (request.method !== "GET" || request.url !== "/worker") {
    throw new Error(`${request.method} ${request.url}`);
  }
  await request.respond("from-worker");
  postMessage({ ok: true });
} catch (error) {
  postMessage({
    error: error instanceof Error ? `${error.name}:${error.message}` : String(error),
  });
} finally {
  try {
    server.close();
  } catch {
    // The server is already closed.
  }
}
