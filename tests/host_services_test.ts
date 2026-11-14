function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function text(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function throws(run: () => void): unknown {
  try {
    run();
  } catch (error) {
    return error;
  }
  throw new Error("did not throw");
}

function decode(bytes: Uint8Array): string {
  return new TextDecoder().decode(bytes);
}

const RECORD_TYPE =
  'The record type must be "A", "AAAA", "CNAME", "NS", "PTR", "MX", "TXT", "SRV", "SOA", "CAA", or "NAPTR".';

function sleepCommand(): { command: string; args: string[] } {
  if (Yuzuko.build.os === "windows") {
    return { command: "ping.exe", args: ["-n", "30", "127.0.0.1"] };
  }
  return { command: "/bin/sleep", args: ["30"] };
}

function envCommand(): { command: string; args: string[] } {
  if (Yuzuko.build.os === "windows") {
    return { command: "cmd.exe", args: ["/d", "/c", "set"] };
  }
  return { command: "/usr/bin/env", args: [] };
}

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

async function readAll(conn: Yuzuko.TcpConn): Promise<Uint8Array> {
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const buffer = new Uint8Array(4096);
    const count = await conn.read(buffer);
    if (count === null) {
      break;
    }
    if (count === 0) {
      throw new Error("empty tcp read");
    }
    chunks.push(buffer.slice(0, count));
    total += count;
  }
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return bytes;
}

Yuzuko.test("SIGHUP is caught on Unix and rejected on Windows", async () => {
  if (Yuzuko.build.os === "windows") {
    const error = throws(() => Yuzuko.addSignalListener("SIGHUP", () => {}));
    assert(error instanceof Yuzuko.errors.NotSupported, `type ${text(error)}`);
    assert(text(error) === "Windows cannot catch this signal.", text(error));
    return;
  }
  let finish: () => void = () => {};
  const done = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const listener = () => {
    finish();
  };
  Yuzuko.addSignalListener("SIGHUP", listener);
  try {
    Yuzuko.raiseSignal("SIGHUP");
    await Promise.race([
      done,
      new Promise<void>((_, reject) => {
        setTimeout(() => reject(new Error("SIGHUP timed out")), 5000);
      }),
    ]);
  } finally {
    Yuzuko.removeSignalListener("SIGHUP", listener);
  }
});

Yuzuko.test("SIGKILL and an unknown name cannot be caught", () => {
  const add = Yuzuko.addSignalListener as (
    signal: string,
    listener: () => void,
  ) => void;
  const kill = throws(() => add("SIGKILL", () => {}));
  assert(kill instanceof TypeError, `kill type ${text(kill)}`);
  assert(text(kill) === "The signal cannot be caught.", text(kill));
  const stop = throws(() => add("SIGSTOP", () => {}));
  assert(text(stop) === "The signal cannot be caught.", text(stop));
  const unknown = throws(() => add("NO_SUCH_SIGNAL", () => {}));
  assert(unknown instanceof TypeError, `unknown type ${text(unknown)}`);
  assert(text(unknown) === "The signal is not supported.", text(unknown));
});

Yuzuko.test("clearEnv starts from an empty environment", async () => {
  const launched = envCommand();
  const child = Yuzuko.spawn(launched.command, {
    args: launched.args,
    clearEnv: true,
    env: { FOO: "1" },
    stderr: "null",
  });
  const output = await child.output();
  const stdout = decode(output.stdout).replaceAll("\r\n", "\n").trim();
  assert(output.code === 0, `code ${String(output.code)} stdout ${stdout}`);
  assert(stdout === "FOO=1", `env ${JSON.stringify(stdout)}`);
});

Yuzuko.test("clearEnv, ids, and an aborted signal are rejected before spawn", () => {
  const spawn = Yuzuko.spawn as (
    command: string,
    options?: unknown,
  ) => Yuzuko.Child;
  const clear = throws(() =>
    spawn("yuzuko-not-a-program", { clearEnv: "yes" }),
  );
  assert(clear instanceof TypeError, `clearEnv type ${text(clear)}`);
  assert(text(clear) === "clearEnv must be a boolean.", text(clear));

  const uid = throws(() => spawn("yuzuko-not-a-program", { uid: -1 }));
  assert(
    text(uid) === "The user id must be an integer from 0 through 4294967295.",
    text(uid),
  );
  const fraction = throws(() => spawn("yuzuko-not-a-program", { uid: 1.5 }));
  assert(
    text(fraction) ===
      "The user id must be an integer from 0 through 4294967295.",
    text(fraction),
  );
  const gid = throws(() => spawn("yuzuko-not-a-program", { gid: -1 }));
  assert(
    text(gid) ===
      "The group id must be an integer from 0 through 4294967295.",
    text(gid),
  );

  if (Yuzuko.build.os === "windows") {
    const present = throws(() => spawn("yuzuko-not-a-program", { uid: 1 }));
    assert(
      present instanceof Yuzuko.errors.NotSupported,
      `uid type ${text(present)}`,
    );
    assert(
      text(present) === "Windows cannot set a user or group id.",
      text(present),
    );
  }

  const controller = new AbortController();
  controller.abort();
  const aborted = throws(() =>
    spawn("yuzuko-not-a-program", { signal: controller.signal }),
  );
  assert(aborted instanceof DOMException, `abort type ${text(aborted)}`);
  assert(aborted.name === "AbortError", `abort name ${aborted.name}`);
  assert(
    text(aborted) === "The signal has already been aborted.",
    text(aborted),
  );

  const bad = throws(() => spawn("yuzuko-not-a-program", { signal: {} }));
  assert(bad instanceof TypeError, `signal type ${text(bad)}`);
  assert(text(bad) === "The signal must be an AbortSignal.", text(bad));
});

Yuzuko.test("abort and kill(signal) terminate the child", async () => {
  const launched = sleepCommand();
  const controller = new AbortController();
  const abortedChild = Yuzuko.spawn(launched.command, {
    args: launched.args,
    signal: controller.signal,
    stdout: "null",
    stderr: "null",
  });
  controller.abort();
  const aborted = await abortedChild.status();
  if (Yuzuko.build.os === "windows") {
    assert(aborted.signal === null, `abort signal ${String(aborted.signal)}`);
    assert(aborted.code === 1, `abort code ${String(aborted.code)}`);
  } else {
    assert(aborted.code === null, `abort code ${String(aborted.code)}`);
    assert(aborted.signal === 9, `abort signal ${String(aborted.signal)}`);
  }

  const child = Yuzuko.spawn(launched.command, {
    args: launched.args,
    stdout: "null",
    stderr: "null",
  });
  try {
    if (Yuzuko.build.os === "windows") {
      const denied = throws(() => child.kill("SIGINT"));
      assert(
        denied instanceof Yuzuko.errors.NotSupported,
        `sigint type ${text(denied)}`,
      );
      assert(
        text(denied) === "Windows cannot signal a child with this signal.",
        text(denied),
      );
      child.kill("SIGTERM");
      const status = await child.status();
      assert(status.signal === null, `term signal ${String(status.signal)}`);
      assert(status.code === 1, `term code ${String(status.code)}`);
    } else {
      child.kill("SIGTERM");
      const status = await child.status();
      assert(status.code === null, `term code ${String(status.code)}`);
      assert(status.signal === 15, `term signal ${String(status.signal)}`);
    }
  } finally {
    child.kill();
  }
});

Yuzuko.test("respond sends caller headers and rejects content-length", async () => {
  const server = Yuzuko.listenHttp({ hostname: "127.0.0.1", port: 0 });
  let client: Yuzuko.TcpConn | undefined;
  try {
    assert(Yuzuko.HttpRequest.prototype.respond.length === 1, "respond length");
    const pending = server.accept();
    client = await Yuzuko.connectTcp({
      hostname: "127.0.0.1",
      port: server.addr.port,
    });
    const requestBytes = new TextEncoder().encode(
      "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert(await client.write(requestBytes) === requestBytes.length, "write");
    const request = await pending;
    const rejected = throws(() =>
      request.respond("no", 200, { "content-length": "1" }),
    );
    assert(rejected instanceof TypeError, `header type ${text(rejected)}`);
    assert(text(rejected) === "The header cannot be set.", text(rejected));
    await request.respond("hi", undefined, {
      "X-Yuzuko": "one",
      "content-type": "text/plain",
      "Content-Type": "application/json",
    });
    const raw = decode(await readAll(client));
    assert(raw.startsWith("HTTP/1.1 200 OK\r\n"), `status ${raw}`);
    assert(
      raw.includes("Content-Type: application/json\r\n"),
      `type ${raw}`,
    );
    assert(!raw.includes("text/plain"), `replaced ${raw}`);
    assert(raw.includes("X-Yuzuko: one\r\n"), `extra ${raw}`);
    assert(raw.endsWith("\r\n\r\nhi"), `body ${raw}`);
  } finally {
    client?.close();
    server.close();
  }
});

Yuzuko.test("a connected datagram sends without an address", async () => {
  const left = Yuzuko.listenDatagram({ hostname: "127.0.0.1", port: 0 });
  const right = Yuzuko.listenDatagram({ hostname: "127.0.0.1", port: 0 });
  const lonely = Yuzuko.listenDatagram({ hostname: "127.0.0.1", port: 0 });
  try {
    assert(Yuzuko.Datagram.prototype.send.length === 1, "send length");
    const missing = throws(() => lonely.send(new Uint8Array([1])));
    assert(
      missing instanceof Yuzuko.errors.NotConnected,
      `missing type ${text(missing)}`,
    );
    assert(text(missing) === "The socket is not connected.", text(missing));
    lonely.setBroadcast(false);
    const group = throws(() => lonely.joinMulticast("192.0.2.1"));
    assert(group instanceof TypeError, `group type ${text(group)}`);
    assert(
      text(group) === "The address must be a multicast address.",
      text(group),
    );
    try {
      lonely.joinMulticast("224.0.0.1");
      lonely.leaveMulticast("224.0.0.1");
    } catch (error) {
      assert(!(error instanceof TypeError), text(error));
      assert(error instanceof Error, text(error));
    }

    await left.connect({ hostname: "127.0.0.1", port: right.addr.port });
    const waiting = right.receive();
    const payload = new Uint8Array([7, 8, 9]);
    assert(await left.send(payload) === 3, "sent");
    const message = await waiting;
    assert(sameBytes(message.data, payload), `payload ${message.data.length}`);
    assert(message.port === left.addr.port, `port ${message.port}`);
    left.disconnect();
    const again = throws(() => left.send(payload));
    assert(
      again instanceof Yuzuko.errors.NotConnected,
      `disconnect type ${text(again)}`,
    );
    assert(text(again) === "The socket is not connected.", text(again));
  } finally {
    left.close();
    right.close();
    lonely.close();
  }
});

Yuzuko.test("reusePort shares a UDP port on Unix", () => {
  const bad = throws(() =>
    Yuzuko.listenDatagram({ reusePort: "yes" as unknown as boolean }),
  );
  assert(bad instanceof TypeError, `reuse type ${text(bad)}`);
  assert(text(bad) === "reusePort must be a boolean.", text(bad));
  if (Yuzuko.build.os === "windows") {
    const denied = throws(() =>
      Yuzuko.listenDatagram({
        hostname: "127.0.0.1",
        port: 0,
        reusePort: true,
      }),
    );
    assert(
      denied instanceof Yuzuko.errors.NotSupported,
      `windows type ${text(denied)}`,
    );
    assert(
      text(denied) === "Windows cannot reuse a listening port.",
      text(denied),
    );
    return;
  }
  const first = Yuzuko.listenDatagram({
    hostname: "127.0.0.1",
    port: 0,
    reusePort: true,
  });
  try {
    const second = Yuzuko.listenDatagram({
      hostname: "127.0.0.1",
      port: first.addr.port,
      reusePort: true,
    });
    try {
      assert(second.addr.port === first.addr.port, `port ${second.addr.port}`);
    } finally {
      second.close();
    }
  } finally {
    first.close();
  }
});

Yuzuko.test("resolveDns returns typed records or a resolver error", async () => {
  const pending = Yuzuko.resolveDns("localhost.", "MX");
  assert(pending instanceof Promise, "mx is a promise");
  try {
    const rows = await pending;
    assert(Array.isArray(rows), `rows ${JSON.stringify(rows)}`);
  } catch (error) {
    assert(error instanceof Error, text(error));
    assert(!(error instanceof TypeError), text(error));
    assert(text(error) === "The host could not be resolved.", text(error));
  }
  const wat = throws(() => Yuzuko.resolveDns("localhost.", "WAT"));
  assert(wat instanceof TypeError, `wat type ${text(wat)}`);
  assert(text(wat) === RECORD_TYPE, text(wat));
});
