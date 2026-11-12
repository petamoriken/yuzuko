function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

const HOSTNAME = 'The hostname must be an IP address or "localhost".';
const PORT_BIND = "The port must be an integer from 0 through 65535.";
const CLOSED = "The socket is closed.";
const DATA = "The data must be a string or a BufferSource.";
const DETACHED = "The buffer is detached.";
const BUSY_ACCEPT = "An accept is already waiting.";
const BUSY_RESPOND = "A response is already waiting.";
const ALREADY = "The response was already sent.";
const STATUS = "The status must be an integer from 100 through 599.";
const UNAVAILABLE = "The address is not available.";
const REQUEST = "The request could not be read.";

type Server = Yuzuko.HttpServer;

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
    // The server is already closed.
  }
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

Yuzuko.test("an HTTP server answers one request with a body", async () => {
  const open: Server[] = [];
  const inflight: Promise<unknown>[] = [];
  let worker: Worker | undefined;
  try {
    await deadline((async () => {
      assert(typeof listenHttp === "undefined", "global listenHttp");
      assert(typeof HttpServer === "undefined", "global HttpServer");
      assert(typeof HttpRequest === "undefined", "global HttpRequest");
      assert(Yuzuko.listenHttp.length === 0, "listenHttp length");
      const server = Yuzuko.listenHttp({ hostname: "127.0.0.1", port: 0 });
      open.push(server);
      assert(server.accept.length === 0, "accept length");
      const firstAddr = server.addr;
      const secondAddr = server.addr;
      assert(firstAddr !== secondAddr, "addr is a new object");
      assert(firstAddr.hostname === "127.0.0.1", `host ${firstAddr.hostname}`);
      assert(firstAddr.port > 0, `port ${firstAddr.port}`);

      const pending = server.accept();
      inflight.push(pending);
      const fetching = fetch(`http://127.0.0.1:${server.addr.port}/hello?x=1`);
      inflight.push(fetching);
      const request = await pending;
      assert(request.method === "GET", request.method);
      assert(request.url === "/hello?x=1", request.url);
      assert(request.body.byteLength === 0, `get body ${request.body.byteLength}`);
      assert(request.respond.length === 1, "respond length");
      await request.respond("pong");
      const response = await fetching;
      assert(response.status === 200, `status ${response.status}`);
      assert(response.statusText === "OK", `status text ${response.statusText}`);
      assert(
        response.headers.get("content-type") === "text/plain;charset=UTF-8",
        `type ${response.headers.get("content-type")}`,
      );
      assert(await response.text() === "pong", "body");

      const posted = server.accept();
      inflight.push(posted);
      const payload = new Uint8Array([9, 8, 7, 6]);
      const posting = fetch(`http://127.0.0.1:${server.addr.port}/echo`, {
        method: "POST",
        body: payload,
      });
      inflight.push(posting);
      const postedRequest = await posted;
      assert(postedRequest.method === "POST", postedRequest.method);
      assert(postedRequest.url === "/echo", postedRequest.url);
      const firstBody = postedRequest.body;
      const secondBody = postedRequest.body;
      assert(firstBody !== secondBody, "body is a new view");
      assert(sameBytes(firstBody, payload), `posted ${firstBody}`);
      assert(sameBytes(secondBody, payload), "second body");
      await postedRequest.respond(payload, 201);
      const postedResponse = await posting;
      assert(postedResponse.status === 201, `post status ${postedResponse.status}`);
      assert(postedResponse.statusText === "Created", postedResponse.statusText);
      assert(
        sameBytes(new Uint8Array(await postedResponse.arrayBuffer()), [9, 8, 7, 6]),
        "post body",
      );

      const viewed = server.accept();
      inflight.push(viewed);
      const source = new Uint8Array([0, 0, 0, 4, 5, 6]);
      const viewing = fetch(`http://127.0.0.1:${server.addr.port}/view`, {
        method: "POST",
        body: new Uint8Array(source.buffer, 3),
      });
      inflight.push(viewing);
      const viewedRequest = await viewed;
      assert(sameBytes(viewedRequest.body, [4, 5, 6]), `view ${viewedRequest.body}`);
      const reply = new DataView(source.buffer, 3);
      await viewedRequest.respond(reply);
      const viewedResponse = await viewing;
      assert(sameBytes(new Uint8Array(await viewedResponse.arrayBuffer()), [4, 5, 6]), "view reply");

      const local = Yuzuko.listenHttp({ hostname: "LOCALHOST" });
      open.push(local);
      assert(local.addr.hostname === "127.0.0.1" && local.addr.port > 0, "localhost");
      const omitted = Yuzuko.listenHttp();
      const absent = Yuzuko.listenHttp(null);
      const explicit = Yuzuko.listenHttp({ port: undefined });
      open.push(omitted, absent, explicit);
      assert(omitted.addr.hostname === "127.0.0.1" && omitted.addr.port > 0, "omitted");
      assert(absent.addr.port > 0, "null options");
      assert(explicit.addr.port > 0, "undefined port");

      worker = new Worker(new URL("./workers/http.js", import.meta.url), { type: "module" });
      const inbox: Array<{ port?: number; ok?: boolean; error?: string }> = [];
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
            reject(new Error("http worker timed out"));
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
      const fromWorker = await fetch(`http://127.0.0.1:${ready.port}/worker`);
      assert(await fromWorker.text() === "from-worker", "worker body");
      const done = await nextMessage();
      assert(done.ok === true, `worker ${done.error}`);
    })(), "exchange");
  } finally {
    for (const server of open) {
      closeQuiet(server);
    }
    await Promise.all(inflight.map((promise) => promise.then(() => undefined, () => undefined)));
    worker?.terminate();
  }
});

Yuzuko.test("HTTP arguments and close use the server", async () => {
  const open: Server[] = [];
  const inflight: Promise<unknown>[] = [];
  try {
    await deadline((async () => {
      let constructed = "";
      try {
        new Yuzuko.HttpServer();
      } catch (error) {
        constructed = error instanceof TypeError ? error.message : "other";
      }
      assert(constructed === "Illegal constructor", constructed);
      constructed = "";
      try {
        new Yuzuko.HttpRequest();
      } catch (error) {
        constructed = error instanceof TypeError ? error.message : "other";
      }
      assert(constructed === "Illegal constructor", constructed);

      let options = "";
      try {
        Yuzuko.listenHttp(1 as unknown as Yuzuko.ListenHttpOptions);
      } catch (error) {
        options = error instanceof TypeError ? error.message : "other";
      }
      assert(options === "Options must be an object.", options);

      let host = "";
      try {
        Yuzuko.listenHttp({ hostname: "example.com", port: 0 });
      } catch (error) {
        host = error instanceof TypeError ? error.message : "other";
      }
      assert(host === HOSTNAME, host);

      let brackets = "";
      try {
        Yuzuko.listenHttp({ hostname: "[::1]", port: 0 });
      } catch (error) {
        brackets = error instanceof TypeError ? error.message : "other";
      }
      assert(brackets === HOSTNAME, brackets);

      let boom = "";
      try {
        Yuzuko.listenHttp({
          hostname: {
            toString() {
              throw new Error("boom");
            },
          },
        } as unknown as Yuzuko.ListenHttpOptions);
      } catch (error) {
        boom = error instanceof Error ? error.message : "other";
      }
      assert(boom === "boom", boom);

      const converted = Yuzuko.listenHttp({
        hostname: { toString: () => "127.0.0.1" } as unknown as string,
        port: { valueOf: () => 0 } as unknown as number,
      });
      open.push(converted);
      assert(converted.addr.hostname === "127.0.0.1" && converted.addr.port > 0, "converted");

      for (const port of [-1, 1.5, 65536, Number.NaN, Number.POSITIVE_INFINITY]) {
        let message = "";
        try {
          Yuzuko.listenHttp({ port });
        } catch (error) {
          message = error instanceof TypeError ? error.message : "other";
        }
        assert(message === PORT_BIND, `${port} ${message}`);
      }

      let refusedBind = "";
      try {
        Yuzuko.listenHttp({ hostname: "192.0.2.1", port: 0 });
      } catch (error) {
        refusedBind = error instanceof Yuzuko.errors.AddrNotAvailable ? error.message : "type";
      }
      assert(refusedBind === UNAVAILABLE, refusedBind);

      const live = Yuzuko.listenHttp({ hostname: "127.0.0.1", port: 0 });
      open.push(live);
      let invoked = "";
      try {
        Yuzuko.HttpServer.prototype.accept.call({});
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
        busy = error instanceof Yuzuko.errors.Busy ? error.message : "other";
      }
      assert(busy === BUSY_ACCEPT, busy);
      const fetching = fetch(`http://127.0.0.1:${live.addr.port}/busy`);
      inflight.push(fetching);
      const request = await pending;
      await request.respond("once");
      assert(await (await fetching).text() === "once", "busy request");

      let returned: unknown = "unset";
      let missing = "";
      try {
        returned = request.respond();
      } catch (error) {
        missing = error instanceof TypeError ? error.message : "other";
      }
      assert(!(returned instanceof Promise), "short respond returned a promise");
      assert(missing === "Not enough arguments", missing);

      returned = "unset";
      let data = "";
      try {
        returned = request.respond(1 as unknown as string);
      } catch (error) {
        data = error instanceof TypeError ? error.message : "other";
      }
      assert(!(returned instanceof Promise), "number respond returned a promise");
      assert(data === DATA, data);

      const detachedBuffer = new ArrayBuffer(4);
      const detachedView = new Uint8Array(detachedBuffer);
      structuredClone(detachedView, { transfer: [detachedBuffer] });
      returned = "unset";
      let detached = "";
      try {
        returned = request.respond(detachedView);
      } catch (error) {
        detached = error instanceof TypeError ? error.message : "other";
      }
      assert(!(returned instanceof Promise), "detached respond returned a promise");
      assert(detached === DETACHED, detached);

      for (const status of [99, 600, 200.5, Number.NaN]) {
        let message = "";
        returned = "unset";
        try {
          returned = request.respond("no", status);
        } catch (error) {
          message = error instanceof TypeError ? error.message : "other";
        }
        assert(!(returned instanceof Promise), `status ${status} returned a promise`);
        assert(message === STATUS, `${status} ${message}`);
      }

      let again = "";
      returned = "unset";
      try {
        returned = request.respond("again");
      } catch (error) {
        again = error instanceof Error && !(error instanceof TypeError) ? error.message : "type";
      }
      assert(!(returned instanceof Promise), "second respond returned a promise");
      assert(again === ALREADY, again);

      const waitingAccept = live.accept();
      inflight.push(waitingAccept);
      const secondFetch = fetch(`http://127.0.0.1:${live.addr.port}/hold`);
      inflight.push(secondFetch.then(() => undefined, () => undefined));
      const held = await waitingAccept;
      const writing = held.respond("held");
      inflight.push(writing);
      let busyRespond = "";
      try {
        await held.respond("other");
      } catch (error) {
        busyRespond = error instanceof Yuzuko.errors.Busy ? error.message : "other";
      }
      assert(busyRespond === BUSY_RESPOND, busyRespond);
      await writing;
      assert(await (await secondFetch).text() === "held", "held body");

      const blocked = live.accept();
      inflight.push(blocked);
      live.close();
      let acceptClosed = "";
      try {
        await blocked;
      } catch (error) {
        acceptClosed = error instanceof Yuzuko.errors.BadResource ? error.message : "other";
      }
      assert(acceptClosed === CLOSED, acceptClosed);
      let second = "";
      try {
        live.close();
      } catch (error) {
        second = error instanceof Yuzuko.errors.BadResource ? error.message : "type";
      }
      assert(second === CLOSED, second);
    })(), "errors");
  } finally {
    for (const server of open) {
      closeQuiet(server);
    }
    await Promise.all(inflight.map((promise) => promise.then(() => undefined, () => undefined)));
  }
});

Yuzuko.test("a malformed request rejects Http", async () => {
  const server = Yuzuko.listenHttp({ hostname: "127.0.0.1", port: 0 });
  const pending = server.accept();
  let conn: Yuzuko.TcpConn | undefined;
  try {
    await deadline((async () => {
      conn = await Yuzuko.connectTcp({
        hostname: "127.0.0.1",
        port: server.addr.port,
      });
      await conn.write(new TextEncoder().encode("not http\r\n\r\n"));
      let name = "";
      let message = "";
      try {
        await pending;
      } catch (error) {
        name = error instanceof Yuzuko.errors.Http ? error.name : "other";
        message = error instanceof Yuzuko.errors.Http ? error.message : "other";
      }
      assert(name === "Http", name);
      assert(message === REQUEST, message);
    })(), "malformed");
  } finally {
    closeQuiet(conn);
    closeQuiet(server);
    await pending.then(() => undefined, () => undefined);
  }
});
