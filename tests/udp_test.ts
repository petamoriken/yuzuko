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
const TOO_LARGE = "The datagram is too large.";
const FAMILY = "The address family does not match the socket.";
const BUSY = "A receive is already waiting.";
const UNAVAILABLE = "The address is not available.";

type Socket = Yuzuko.Datagram;

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

function closeQuiet(socket: Socket | undefined): void {
  if (!socket) {
    return;
  }
  try {
    socket.close();
  } catch {
    // The socket is already closed.
  }
}

Yuzuko.test("local datagrams exchange bytes and the source address", async () => {
  const open: Socket[] = [];
  const inflight: Promise<unknown>[] = [];
  let worker: Worker | undefined;
  try {
    assert(typeof listenDatagram === "undefined", "global listenDatagram");
    assert(typeof Datagram === "undefined", "global Datagram");
    assert(Yuzuko.listenDatagram.length === 0, "listen length");
    assert(Yuzuko.Datagram.prototype.send.length === 1, "send length");
    assert(Yuzuko.Datagram.prototype.receive.length === 0, "receive length");
    assert(Yuzuko.Datagram.prototype.close.length === 0, "close length");

    const left = Yuzuko.listenDatagram({ hostname: "127.0.0.1", port: 0 });
    const right = Yuzuko.listenDatagram({ hostname: "localhost", port: 0 });
    open.push(left, right);
    assert(left instanceof Yuzuko.Datagram, "instanceof");
    assert(left.addr.hostname === "127.0.0.1", `left host ${left.addr.hostname}`);
    assert(right.addr.hostname === "127.0.0.1", `right host ${right.addr.hostname}`);
    assert(left.addr.port > 0 && left.addr.port <= 65535, `left port ${left.addr.port}`);
    assert(right.addr.port > 0 && right.addr.port !== left.addr.port, "ports");
    const again = left.addr;
    assert(again !== left.addr, "addr is a new object");
    assert(again.hostname === "127.0.0.1" && again.port === left.addr.port, "stable addr");

    const payload = new Uint8Array(1000);
    for (let index = 0; index < payload.length; index += 1) {
      payload[index] = index & 0xff;
    }
    const waiting = left.receive();
    inflight.push(waiting);
    const sent = await right.send(payload, {
      hostname: "localhost",
      port: left.addr.port,
    });
    assert(sent === 1000, `sent ${sent}`);
    const message = await waiting;
    assert(message.data instanceof Uint8Array, "data type");
    assert(sameBytes(message.data, payload), `payload ${message.data.length}`);
    assert(message.hostname === "127.0.0.1", `source host ${message.hostname}`);
    assert(message.port === right.addr.port, `source port ${message.port}`);

    const raw = new Uint8Array([0, 9, 8, 7, 0]).buffer;
    const view = new DataView(raw, 1, 3);
    const back = right.receive();
    inflight.push(back);
    const viewSent = await left.send(view, right.addr);
    assert(viewSent === 3, `view sent ${viewSent}`);
    const viewed = await back;
    assert(sameBytes(viewed.data, [9, 8, 7]), `view ${Array.from(viewed.data)}`);
    assert(viewed.port === left.addr.port, `view source ${viewed.port}`);

    const emptyWait = left.receive();
    inflight.push(emptyWait);
    assert(await right.send(new Uint8Array(0), left.addr) === 0, "empty send");
    const empty = await emptyWait;
    assert(empty.data.length === 0, `empty ${empty.data.length}`);

    const shared = new SharedArrayBuffer(4);
    new Uint8Array(shared).set([4, 5, 6, 7]);
    const sharedWait = left.receive();
    inflight.push(sharedWait);
    assert(await right.send(shared, left.addr) === 4, "shared send");
    const sharedMessage = await sharedWait;
    assert(sameBytes(sharedMessage.data, [4, 5, 6, 7]), "shared bytes");

    const omitted = Yuzuko.listenDatagram();
    const absent = Yuzuko.listenDatagram(null);
    const explicit = Yuzuko.listenDatagram({ port: undefined });
    open.push(omitted, absent, explicit);
    assert(omitted.addr.hostname === "127.0.0.1" && omitted.addr.port > 0, "omitted");
    assert(absent.addr.hostname === "127.0.0.1" && absent.addr.port > 0, "null options");
    assert(explicit.addr.port > 0, "undefined port");

    worker = new Worker(new URL("./workers/udp.js", import.meta.url), {
      type: "module",
    });
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
          reject(new Error("datagram timed out"));
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
    const local = Yuzuko.listenDatagram({ hostname: "127.0.0.1", port: 0 });
    open.push(local);
    const workerPayload = new Uint8Array([9, 8, 7]);
    const workerSent = await local.send(workerPayload, {
      hostname: "127.0.0.1",
      port: ready.port,
    });
    assert(workerSent === 3, `worker sent ${workerSent}`);
    const fromWorker = await nextMessage();
    assert(fromWorker.error === undefined, `worker ${fromWorker.error}`);
    assert(fromWorker.hostname === "127.0.0.1", `worker host ${fromWorker.hostname}`);
    assert(fromWorker.port === local.addr.port, `worker source ${fromWorker.port}`);
    assert(
      Array.isArray(fromWorker.bytes) && fromWorker.bytes.join(",") === "9,8,7",
      `worker bytes ${fromWorker.bytes}`,
    );
  } finally {
    for (const socket of open) {
      closeQuiet(socket);
    }
    await Promise.all(inflight.map((promise) => promise.then(() => undefined, () => undefined)));
    worker?.terminate();
  }
});

Yuzuko.test("datagram arguments and close use the socket", async () => {
  const open: Socket[] = [];
  const inflight: Promise<unknown>[] = [];
  try {
    let constructed = "";
    try {
      new Yuzuko.Datagram();
    } catch (error) {
      constructed = error instanceof TypeError ? error.message : "other";
    }
    assert(constructed === "Illegal constructor", constructed);

    let options = "";
    try {
      Yuzuko.listenDatagram(1 as unknown as Yuzuko.ListenDatagramOptions);
    } catch (error) {
      options = error instanceof TypeError ? error.message : "other";
    }
    assert(options === "Options must be an object.", options);

    let host = "";
    try {
      Yuzuko.listenDatagram({ hostname: "example.com", port: 0 });
    } catch (error) {
      host = error instanceof TypeError ? error.message : "other";
    }
    assert(host === HOSTNAME, host);

    let brackets = "";
    try {
      Yuzuko.listenDatagram({ hostname: "[::1]", port: 0 });
    } catch (error) {
      brackets = error instanceof TypeError ? error.message : "other";
    }
    assert(brackets === HOSTNAME, brackets);

    let boom = "";
    try {
      Yuzuko.listenDatagram({
        hostname: {
          toString() {
            throw new Error("boom");
          },
        },
      } as unknown as Yuzuko.ListenDatagramOptions);
    } catch (error) {
      boom = error instanceof Error ? error.message : "other";
    }
    assert(boom === "boom", boom);

    const named = {
      toString() {
        return "127.0.0.1";
      },
    };
    const converted = Yuzuko.listenDatagram({
      hostname: named as unknown as string,
      port: { valueOf: () => 0 } as unknown as number,
    });
    open.push(converted);
    assert(converted.addr.hostname === "127.0.0.1" && converted.addr.port > 0, "converted");

    for (const port of [-1, 1.5, 65536, Number.NaN, Number.POSITIVE_INFINITY]) {
      let message = "";
      try {
        Yuzuko.listenDatagram({ port });
      } catch (error) {
        message = error instanceof TypeError ? error.message : "other";
      }
      assert(message === PORT_BIND, `${port} ${message}`);
    }

    let refused = "";
    try {
      Yuzuko.listenDatagram({ hostname: "192.0.2.1", port: 0 });
    } catch (error) {
      refused = error instanceof Yuzuko.errors.AddrNotAvailable ? error.message : "type";
    }
    assert(refused === UNAVAILABLE, refused);

    const socket = Yuzuko.listenDatagram({ hostname: "127.0.0.1", port: 0 });
    open.push(socket);

    let invoked = "";
    try {
      Yuzuko.Datagram.prototype.receive.call({});
    } catch (error) {
      invoked = error instanceof TypeError ? error.message : "other";
    }
    assert(invoked === "illegal invocation", invoked);

    let arity = "";
    let returned: unknown = "unset";
    try {
      returned = socket.send(new Uint8Array([1]));
    } catch (error) {
      arity = error instanceof TypeError ? error.message : "other";
    }
    assert(!(returned instanceof Promise), "short send returned a promise");
    assert(arity === "Not enough arguments", arity);

    returned = "unset";
    let data = "";
    try {
      returned = socket.send("hi" as unknown as Uint8Array, socket.addr);
    } catch (error) {
      data = error instanceof TypeError ? error.message : "other";
    }
    assert(!(returned instanceof Promise), "string send returned a promise");
    assert(data === DATA, data);

    const detachedBuffer = new ArrayBuffer(4);
    const detachedView = new Uint8Array(detachedBuffer);
    structuredClone(detachedView, { transfer: [detachedBuffer] });
    returned = "unset";
    let detached = "";
    try {
      returned = socket.send(detachedView, socket.addr);
    } catch (error) {
      detached = error instanceof TypeError ? error.message : "other";
    }
    assert(!(returned instanceof Promise), "detached send returned a promise");
    assert(detached === DETACHED, detached);

    returned = "unset";
    let tooBig = "";
    try {
      returned = socket.send(new Uint8Array(65508), socket.addr);
    } catch (error) {
      tooBig = error instanceof TypeError ? error.message : "other";
    }
    assert(!(returned instanceof Promise), "large send returned a promise");
    assert(tooBig === TOO_LARGE, tooBig);

    returned = "unset";
    let family = "";
    try {
      returned = socket.send(new Uint8Array([1]), { hostname: "::1", port: 9 });
    } catch (error) {
      family = error instanceof TypeError ? error.message : "other";
    }
    assert(!(returned instanceof Promise), "family send returned a promise");
    assert(family === FAMILY, family);

    let destPort = "";
    try {
      socket.send(new Uint8Array([1]), { hostname: "127.0.0.1", port: 0 });
    } catch (error) {
      destPort = error instanceof TypeError ? error.message : "other";
    }
    assert(destPort === PORT_DEST, destPort);

    const pending = socket.receive();
    inflight.push(pending);
    let busy = "";
    try {
      await socket.receive();
    } catch (error) {
      busy = error instanceof Yuzuko.errors.Busy ? error.message : "other";
    }
    assert(busy === BUSY, busy);
    const peer = Yuzuko.listenDatagram({ hostname: "127.0.0.1", port: 0 });
    open.push(peer);
    assert(await peer.send(new Uint8Array([1]), socket.addr) === 1, "release busy");
    const got = await pending;
    assert(sameBytes(got.data, [1]), "busy payload");

    const blocked = socket.receive();
    inflight.push(blocked);
    socket.close();
    let closed = "";
    try {
      await blocked;
    } catch (error) {
      closed = error instanceof Yuzuko.errors.BadResource ? error.message : "other";
    }
    assert(closed === CLOSED, closed);

    let second = "";
    try {
      socket.close();
    } catch (error) {
      second = error instanceof Yuzuko.errors.BadResource ? error.message : "type";
    }
    assert(second === CLOSED, second);

    let sendClosed = "";
    try {
      await socket.send(new Uint8Array([1]), peer.addr);
    } catch (error) {
      sendClosed = error instanceof Yuzuko.errors.BadResource ? error.message : "other";
    }
    assert(sendClosed === CLOSED, sendClosed);

    let receiveClosed = "";
    try {
      await socket.receive();
    } catch (error) {
      receiveClosed = error instanceof Yuzuko.errors.BadResource ? error.message : "other";
    }
    assert(receiveClosed === CLOSED, receiveClosed);
  } finally {
    for (const socket of open) {
      closeQuiet(socket);
    }
    await Promise.all(inflight.map((promise) => promise.then(() => undefined, () => undefined)));
  }
});
