function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

const HOSTNAME = 'The hostname must be an IP address or "localhost".';
const PORT_BIND = "The port must be an integer from 0 through 65535.";
const PORT_DEST = "The port must be an integer from 1 through 65535.";
const CLOSED = "The socket is closed.";
const DATA = "The data must be a BufferSource.";
const DETACHED = "The buffer is detached.";
const BUSY_ACCEPT = "An accept is already waiting.";
const BUSY_READ = "A read is already waiting.";
const BIND = "The socket could not bind.";
const CONNECT = "The connection was refused.";

type Listener = Yuzuko.TcpListener;
type Conn = Yuzuko.TcpConn;

function sameBytes(left: Uint8Array, right: ArrayLike<number>): boolean {
  if (left.length !== right.length) {
    return false;
  }
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return false;
    }
  }
  return true;
}

function closeQuiet(value: { close(): void } | undefined): void {
  if (!value) {
    return;
  }
  try {
    value.close();
  } catch {
    // The socket is already closed.
  }
}

async function readExact(conn: Conn, length: number): Promise<Uint8Array> {
  const out = new Uint8Array(length);
  let offset = 0;
  while (offset < length) {
    const n = await conn.read(new Uint8Array(out.buffer, offset));
    if (n === null) {
      throw new Error(`eof at ${offset}`);
    }
    if (n === 0) {
      throw new Error(`empty read at ${offset}`);
    }
    offset += n;
  }
  return out;
}

function deadline<T>(work: Promise<T>, label: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<T>((_resolve, reject) => {
    timer = setTimeout(() => reject(new Error(`${label} timed out`)), 10000);
  });
  work.then(() => undefined, () => undefined);
  return Promise.race([work, timeout]).finally(() => {
    if (timer !== undefined) {
      clearTimeout(timer);
    }
  });
}

Yuzuko.test("local TCP sockets exchange bytes", async () => {
  const open: Array<{ close(): void }> = [];
  const inflight: Promise<unknown>[] = [];
  let worker: Worker | undefined;
  try {
    await deadline((async () => {
      assert(typeof listenTcp === "undefined", "global listenTcp");
      assert(typeof connectTcp === "undefined", "global connectTcp");
      assert(typeof TcpListener === "undefined", "global TcpListener");
      assert(typeof TcpConn === "undefined", "global TcpConn");
      assert(Yuzuko.listenTcp.length === 0, "listen length");
      assert(Yuzuko.connectTcp.length === 1, "connect length");
      assert(Yuzuko.TcpListener.prototype.accept.length === 0, "accept length");
      assert(Yuzuko.TcpListener.prototype.close.length === 0, "listener close length");
      assert(Yuzuko.TcpConn.prototype.read.length === 1, "read length");
      assert(Yuzuko.TcpConn.prototype.write.length === 1, "write length");
      assert(Yuzuko.TcpConn.prototype.close.length === 0, "conn close length");

      const listener = Yuzuko.listenTcp({ hostname: "127.0.0.1", port: 0 });
      open.push(listener);
      assert(listener instanceof Yuzuko.TcpListener, "instanceof listener");
      assert(listener.addr.hostname === "127.0.0.1", `host ${listener.addr.hostname}`);
      assert(listener.addr.port > 0, `port ${listener.addr.port}`);
      const again = listener.addr;
      assert(again !== listener.addr, "addr is a new object");
      assert(again.port === listener.addr.port, "stable port");

      const accepted = listener.accept();
      inflight.push(accepted);
      const client = await Yuzuko.connectTcp({
        hostname: "localhost",
        port: listener.addr.port,
      });
      open.push(client);
      const server = await accepted;
      open.push(server);
      assert(server instanceof Yuzuko.TcpConn, "instanceof conn");
      assert(client.remoteAddr.hostname === "127.0.0.1", `client peer ${client.remoteAddr.hostname}`);
      assert(client.remoteAddr.port === listener.addr.port, `client peer port ${client.remoteAddr.port}`);
      assert(server.remoteAddr.hostname === "127.0.0.1", `server peer ${server.remoteAddr.hostname}`);
      assert(server.remoteAddr.port === client.localAddr.port, `server peer port ${server.remoteAddr.port}`);
      assert(server.localAddr !== server.remoteAddr, "addresses are new objects");
      assert(await client.write(new Uint8Array(0)) === 0, "empty write");
      assert(await server.read(new Uint8Array(0)) === 0, "empty read");

      const payload = new Uint8Array(1000);
      for (let index = 0; index < payload.length; index += 1) {
        payload[index] = index & 0xff;
      }
      const written = await client.write(payload);
      assert(written === 1000, `written ${written}`);
      const got = await readExact(server, 1000);
      assert(sameBytes(got, payload), `payload ${got.length}`);

      const raw = new Uint8Array([0, 9, 8, 7, 0]).buffer;
      const view = new DataView(raw, 1, 3);
      assert(await server.write(view) === 3, "view write");
      const viewed = await readExact(client, 3);
      assert(sameBytes(viewed, [9, 8, 7]), `view ${Array.from(viewed)}`);

      const shared = new SharedArrayBuffer(4);
      new Uint8Array(shared).set([4, 5, 6, 7]);
      assert(await client.write(shared) === 4, "shared write");
      const sharedGot = await readExact(server, 4);
      assert(sameBytes(sharedGot, [4, 5, 6, 7]), "shared bytes");

      client.close();
      const end = await server.read(new Uint8Array(8));
      assert(end === null, `eof ${end}`);

      const omitted = Yuzuko.listenTcp();
      const absent = Yuzuko.listenTcp(null);
      const explicit = Yuzuko.listenTcp({ port: undefined });
      open.push(omitted, absent, explicit);
      assert(omitted.addr.hostname === "127.0.0.1" && omitted.addr.port > 0, "omitted");
      assert(absent.addr.hostname === "127.0.0.1" && absent.addr.port > 0, "null options");
      assert(explicit.addr.port > 0, "undefined port");

      worker = new Worker(new URL("./workers/tcp.js", import.meta.url), { type: "module" });
      const inbox: Array<{ port?: number; hostname?: string; bytes?: number[]; error?: string }> = [];
      let waitingMessage: ((value: (typeof inbox)[number]) => void) | undefined;
      let failMessage: ((error: Error) => void) | undefined;
      worker.onmessage = (event) => {
        const value = event.data as (typeof inbox)[number];
        if (waitingMessage) {
          const resolve = waitingMessage;
          waitingMessage = undefined;
          resolve(value);
        } else {
          inbox.push(value);
        }
      };
      worker.onerror = (event) => {
        const error = new Error(String(event.message || "worker error"));
        if (failMessage) {
          failMessage(error);
        }
      };
      const nextMessage = () => {
        const queued = inbox.shift();
        if (queued) {
          return Promise.resolve(queued);
        }
        return new Promise<(typeof inbox)[number]>((resolve, reject) => {
          const timer = setTimeout(() => {
            waitingMessage = undefined;
            failMessage = undefined;
            reject(new Error("tcp worker timed out"));
          }, 10000);
          waitingMessage = (value) => {
            clearTimeout(timer);
            failMessage = undefined;
            resolve(value);
          };
          failMessage = (error) => {
            clearTimeout(timer);
            waitingMessage = undefined;
            reject(error);
          };
        });
      };
      const ready = await nextMessage();
      assert(ready.error === undefined, `worker bind ${ready.error}`);
      assert(typeof ready.port === "number" && ready.port > 0, `worker port ${ready.port}`);
      const local = await Yuzuko.connectTcp({ hostname: "127.0.0.1", port: ready.port });
      open.push(local);
      assert(await local.write(new Uint8Array([9, 8, 7])) === 3, "worker write");
      const fromWorker = await nextMessage();
      assert(fromWorker.error === undefined, `worker ${fromWorker.error}`);
      assert(fromWorker.hostname === "127.0.0.1", `worker host ${fromWorker.hostname}`);
      assert(fromWorker.port === local.localAddr.port, `worker source ${fromWorker.port}`);
      assert(
        Array.isArray(fromWorker.bytes) && fromWorker.bytes.join(",") === "9,8,7",
        `worker bytes ${fromWorker.bytes}`,
      );
    })(), "exchange");
  } finally {
    for (const socket of open) {
      closeQuiet(socket);
    }
    await Promise.all(inflight.map((promise) => promise.then(() => undefined, () => undefined)));
    worker?.terminate();
  }
});

Yuzuko.test("TCP arguments and close use the socket", async () => {
  const open: Array<{ close(): void }> = [];
  const inflight: Promise<unknown>[] = [];
  try {
    await deadline((async () => {
      let constructed = "";
      try {
        new Yuzuko.TcpListener();
      } catch (error) {
        constructed = error instanceof TypeError ? error.message : "other";
      }
      assert(constructed === "Illegal constructor", constructed);
      constructed = "";
      try {
        new Yuzuko.TcpConn();
      } catch (error) {
        constructed = error instanceof TypeError ? error.message : "other";
      }
      assert(constructed === "Illegal constructor", constructed);

      let options = "";
      try {
        Yuzuko.listenTcp(1 as unknown as Yuzuko.ListenTcpOptions);
      } catch (error) {
        options = error instanceof TypeError ? error.message : "other";
      }
      assert(options === "Options must be an object.", options);

      let host = "";
      try {
        Yuzuko.listenTcp({ hostname: "example.com", port: 0 });
      } catch (error) {
        host = error instanceof TypeError ? error.message : "other";
      }
      assert(host === HOSTNAME, host);

      let brackets = "";
      try {
        Yuzuko.connectTcp({ hostname: "[::1]", port: 9 });
      } catch (error) {
        brackets = error instanceof TypeError ? error.message : "other";
      }
      assert(brackets === HOSTNAME, brackets);

      let boom = "";
      try {
        Yuzuko.listenTcp({
          hostname: {
            toString() {
              throw new Error("boom");
            },
          },
        } as unknown as Yuzuko.ListenTcpOptions);
      } catch (error) {
        boom = error instanceof Error ? error.message : "other";
      }
      assert(boom === "boom", boom);

      const converted = Yuzuko.listenTcp({
        hostname: { toString: () => "127.0.0.1" } as unknown as string,
        port: { valueOf: () => 0 } as unknown as number,
      });
      open.push(converted);
      assert(converted.addr.hostname === "127.0.0.1" && converted.addr.port > 0, "converted");

      for (const port of [-1, 1.5, 65536, Number.NaN, Number.POSITIVE_INFINITY]) {
        let message = "";
        try {
          Yuzuko.listenTcp({ port });
        } catch (error) {
          message = error instanceof TypeError ? error.message : "other";
        }
        assert(message === PORT_BIND, `${port} ${message}`);
      }

      let destPort = "";
      try {
        await Yuzuko.connectTcp({ hostname: "127.0.0.1", port: 0 });
      } catch (error) {
        destPort = error instanceof TypeError ? error.message : "other";
      }
      assert(destPort === PORT_DEST, destPort);

      let missing = "";
      let returned: unknown = "unset";
      try {
        returned = Yuzuko.connectTcp();
      } catch (error) {
        missing = error instanceof TypeError ? error.message : "other";
      }
      assert(!(returned instanceof Promise), "short connect returned a promise");
      assert(missing === "Not enough arguments", missing);

      let refusedBind = "";
      try {
        Yuzuko.listenTcp({ hostname: "192.0.2.1", port: 0 });
      } catch (error) {
        refusedBind = error instanceof Error && !(error instanceof TypeError) ? error.message : "type";
      }
      assert(refusedBind === BIND, refusedBind);

      const listener = Yuzuko.listenTcp({ hostname: "127.0.0.1", port: 0 });
      open.push(listener);
      const port = listener.addr.port;
      listener.close();
      let refused = "";
      let refusedClass = false;
      try {
        await Yuzuko.connectTcp({ hostname: "127.0.0.1", port });
      } catch (error) {
        refused = error instanceof Error ? error.message : "other";
        refusedClass = error instanceof Yuzuko.errors.ConnectionRefused;
      }
      assert(refused === CONNECT, refused);
      assert(refusedClass, "refused class");

      const live = Yuzuko.listenTcp({ hostname: "127.0.0.1", port: 0 });
      open.push(live);
      let invoked = "";
      try {
        Yuzuko.TcpListener.prototype.accept.call({});
      } catch (error) {
        invoked = error instanceof TypeError ? error.message : "other";
      }
      assert(invoked === "illegal invocation", invoked);

      const pending = live.accept();
      inflight.push(pending);
      let busy = "";
      try {
        await live.accept();
      } catch (error) {
        busy = error instanceof Error ? error.message : "other";
      }
      assert(busy === BUSY_ACCEPT, busy);
      const client = await Yuzuko.connectTcp({ hostname: "127.0.0.1", port: live.addr.port });
      open.push(client);
      const server = await pending;
      open.push(server);

      returned = "unset";
      let data = "";
      try {
        returned = client.write("hi" as unknown as Uint8Array);
      } catch (error) {
        data = error instanceof TypeError ? error.message : "other";
      }
      assert(!(returned instanceof Promise), "string write returned a promise");
      assert(data === DATA, data);

      const detachedBuffer = new ArrayBuffer(4);
      const detachedView = new Uint8Array(detachedBuffer);
      structuredClone(detachedView, { transfer: [detachedBuffer] });
      returned = "unset";
      let detached = "";
      try {
        returned = client.write(detachedView);
      } catch (error) {
        detached = error instanceof TypeError ? error.message : "other";
      }
      assert(!(returned instanceof Promise), "detached write returned a promise");
      assert(detached === DETACHED, detached);

      const blocked = server.read(new Uint8Array(4));
      inflight.push(blocked);
      let busyRead = "";
      try {
        await server.read(new Uint8Array(1));
      } catch (error) {
        busyRead = error instanceof Error ? error.message : "other";
      }
      assert(busyRead === BUSY_READ, busyRead);
      server.close();
      let closedRead = "";
      try {
        await blocked;
      } catch (error) {
        closedRead = error instanceof Error ? error.message : "other";
      }
      assert(closedRead === CLOSED, closedRead);

      let second = "";
      try {
        server.close();
      } catch (error) {
        second = error instanceof Error && !(error instanceof TypeError) ? error.message : "type";
      }
      assert(second === CLOSED, second);

      let writeClosed = "";
      try {
        await server.write(new Uint8Array([1]));
      } catch (error) {
        writeClosed = error instanceof Error ? error.message : "other";
      }
      assert(writeClosed === CLOSED, writeClosed);

      const waiting = live.accept();
      inflight.push(waiting);
      live.close();
      let acceptClosed = "";
      try {
        await waiting;
      } catch (error) {
        acceptClosed = error instanceof Error ? error.message : "other";
      }
      assert(acceptClosed === CLOSED, acceptClosed);
      let listenerAgain = "";
      try {
        live.close();
      } catch (error) {
        listenerAgain = error instanceof Error && !(error instanceof TypeError) ? error.message : "type";
      }
      assert(listenerAgain === CLOSED, listenerAgain);
    })(), "errors");
  } finally {
    for (const socket of open) {
      closeQuiet(socket);
    }
    await Promise.all(inflight.map((promise) => promise.then(() => undefined, () => undefined)));
  }
});
