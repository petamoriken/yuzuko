function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function record(event) {
  return {
    type: event.type,
    bubbles: event.bubbles,
    cancelable: event.cancelable,
    trusted: event.isTrusted,
    progress: event instanceof ProgressEvent,
    lengthComputable: event.lengthComputable,
    loaded: event.loaded,
    total: event.total,
  };
}

function readText(blob, listen) {
  return new Promise((resolve) => {
    const reader = new FileReader();
    const events = [];
    const note = (event) => {
      events.push({ ...record(event), target: event.target === reader });
    };
    reader.onloadstart = note;
    reader.onprogress = note;
    reader.onload = note;
    reader.onabort = note;
    reader.onerror = note;
    reader.onloadend = (event) => {
      note(event);
      resolve({ reader, events });
    };
    if (listen) {
      listen(reader, note);
    }
    reader.readAsText(blob);
  });
}

Yuzuko.test("FileReader and XHR progress events", async () => {
  const empty = await readText(new Blob([]));
  assert(
    empty.events.map((event) => event.type).join() === "loadstart,load,loadend",
    empty.events.map((event) => event.type).join(),
  );
  for (const event of empty.events) {
    assert(event.progress, event.type);
    assert(event.bubbles === false, "bubbles");
    assert(event.cancelable === false, "cancelable");
    assert(event.trusted === true, "trusted");
    assert(event.target === true, "target");
    assert(event.loaded === 0 && event.total === 0, "empty counts");
    assert(event.lengthComputable === false, "empty computable");
  }
  assert(empty.reader.result === "", "empty text");

  const text = await readText(new Blob(["ab"]));
  assert(
    text.events.map((event) => event.type).join() ===
      "loadstart,progress,load,loadend",
    text.events.map((event) => event.type).join(),
  );
  assert(
    text.events[0].loaded === 0 &&
      text.events[0].total === 2 &&
      text.events[0].lengthComputable === true,
    "loadstart counts",
  );
  for (const event of text.events.slice(1)) {
    assert(
      event.loaded === 2 &&
        event.total === 2 &&
        event.lengthComputable === true,
      event.type,
    );
  }
  assert(text.reader.result === "ab", text.reader.result);

  const early = await readText(new Blob(["ab"]), (reader, note) => {
    reader.onloadstart = (event) => {
      note(event);
      reader.abort();
    };
  });
  assert(
    early.events.map((event) => event.type).join() === "loadstart,abort,loadend",
    early.events.map((event) => event.type).join(),
  );
  for (const event of early.events) {
    assert(event.loaded === 0 && event.total === 2, event.type);
    assert(event.lengthComputable === true, event.type);
    assert(event.progress, event.type);
  }
  assert(early.reader.readyState === early.reader.DONE, "done");
  assert(early.reader.result === null, "aborted result");
  assert(early.reader.error.name === "AbortError", early.reader.error.name);

  const mid = await readText(new Blob(["ab"]), (reader, note) => {
    reader.onprogress = (event) => {
      note(event);
      reader.abort();
    };
  });
  assert(
    mid.events.map((event) => event.type).join() ===
      "loadstart,progress,abort,loadend",
    mid.events.map((event) => event.type).join(),
  );
  assert(mid.events[1].loaded === 2, "progress loaded");
  assert(mid.events[2].loaded === 2 && mid.events[3].loaded === 2, "abort loaded");

  const scripted = new ProgressEvent("load", {
    lengthComputable: true,
    loaded: 1,
    total: 4,
  });
  assert(scripted.isTrusted === false, "scripted trust");
  assert(scripted.bubbles === false && scripted.cancelable === false, "scripted flags");
  assert(scripted.lengthComputable === true, "scripted computable");
  assert(scripted.loaded === 1 && scripted.total === 4, "scripted counts");

  const xhr = new XMLHttpRequest();
  const loaded = await new Promise((resolve) => {
    xhr.open("GET", "data:text/plain,hi");
    xhr.onload = (event) => {
      resolve({
        type: event.type,
        lengthComputable: event.lengthComputable,
        loaded: event.loaded,
        total: event.total,
        target: event.target === xhr,
        current: event.currentTarget === xhr,
        phase: event.eventPhase,
        trusted: event.isTrusted,
        progress: event instanceof ProgressEvent,
        text: xhr.responseText,
        event,
      });
    };
    xhr.send();
  });
  assert(loaded.type === "load" && loaded.progress, "xhr load");
  assert(loaded.lengthComputable === true, "xhr computable");
  assert(loaded.loaded === 2 && loaded.total === 2, "xhr counts");
  assert(loaded.target && loaded.current && loaded.phase === 2, "xhr dispatch");
  assert(loaded.trusted === true, "xhr trust");
  assert(loaded.text === "hi", loaded.text);
  assert(loaded.event.currentTarget === null, "current after");
  assert(loaded.event.target === xhr, "target stays");
  assert(loaded.event.eventPhase === 0, "phase after");

  const bad = await new Promise((resolve) => {
    const request = new XMLHttpRequest();
    request.open("GET", "data:text/plain");
    request.onerror = (event) => resolve(event);
    request.onload = () => resolve(null);
    request.send();
  });
  assert(bad instanceof ProgressEvent && bad.type === "error", "xhr error");
  assert(
    bad.lengthComputable === false && bad.loaded === 0 && bad.total === 0,
    "xhr error counts",
  );

  const blank = await new Promise((resolve) => {
    const request = new XMLHttpRequest();
    request.open("GET", "data:text/plain,");
    request.onload = (event) => resolve(event);
    request.onerror = () => resolve(null);
    request.send();
  });
  assert(blank instanceof ProgressEvent && blank.type === "load", "empty load");
  assert(
    blank.lengthComputable === false && blank.loaded === 0 && blank.total === 0,
    "empty xhr counts",
  );
});

Yuzuko.test("XMLHttpRequest is an EventTarget", async () => {
  const xhr = new XMLHttpRequest();
  assert(xhr instanceof EventTarget, "instance");
  assert(XMLHttpRequest.prototype instanceof EventTarget, "prototype");
  assert(typeof xhr.addEventListener === "function", "addEventListener");
  assert(typeof xhr.removeEventListener === "function", "removeEventListener");
  assert(typeof xhr.dispatchEvent === "function", "dispatchEvent");

  xhr.onload = () => {};
  xhr.onload = 1;
  assert(xhr.onload === null, "non-function handler");

  const seen = [];
  xhr.addEventListener("ping", (event) => {
    seen.push("a");
    event.stopPropagation();
    assert(event.target === xhr, "scripted target");
    assert(event.currentTarget === xhr, "scripted current");
  });
  xhr.addEventListener("ping", () => seen.push("b"));
  assert(xhr.dispatchEvent(new Event("ping")) === true, "dispatch");
  assert(seen.join() === "a,b", seen.join());

  const order = [];
  xhr.open("GET", "data:text/plain,hi");
  xhr.send();
  xhr.addEventListener("load", (event) => {
    order.push("listener");
    assert(event.currentTarget === xhr, "listener current");
    assert(event.target === xhr, "listener target");
    assert(event.eventPhase === 2, "listener phase");
    assert(event.isTrusted === true, "listener trust");
    assert(event.bubbles === false && event.cancelable === false, "listener flags");
  });
  xhr.onload = (event) => {
    order.push("attr");
    assert(event.loaded === 2 && event.total === 2, "listener counts");
  };
  await new Promise((resolve) => xhr.addEventListener("load", resolve));
  assert(order.join() === "listener,attr", order.join());
  assert(xhr.responseText === "hi", xhr.responseText);

  const removed = new XMLHttpRequest();
  removed.open("GET", "data:text/plain,hi");
  let ran = false;
  const listener = () => {
    ran = true;
  };
  removed.addEventListener("load", listener);
  removed.removeEventListener("load", listener);
  removed.send();
  await new Promise((resolve) => {
    removed.onload = resolve;
  });
  assert(ran === false, "removed listener");

  const failed = new XMLHttpRequest();
  failed.open("GET", "data:text/plain");
  const error = await new Promise((resolve) => {
    failed.addEventListener("error", resolve);
    failed.send();
  });
  assert(error instanceof ProgressEvent && error.type === "error", "error listener");
  assert(
    error.lengthComputable === false && error.loaded === 0 && error.total === 0,
    "error listener counts",
  );
  assert(error.target === failed, "error target");
});

Yuzuko.test("XMLHttpRequest dispatches loadstart, progress, and loadend", async () => {
  async function record(url: string) {
    const xhr = new XMLHttpRequest();
    const events: ProgressEvent[] = [];
    for (const type of ["loadstart", "progress", "load", "error", "loadend"]) {
      xhr.addEventListener(type, (event) => {
        events.push(event as ProgressEvent);
      });
    }
    xhr.open("GET", url);
    xhr.send();
    await new Promise((resolve) => xhr.addEventListener("loadend", resolve));
    return events;
  }

  function counts(event: ProgressEvent, loaded: number, total: number, computable: boolean) {
    assert(event instanceof ProgressEvent, event.type);
    assert(event.isTrusted === true, `${event.type} trust`);
    assert(event.bubbles === false && event.cancelable === false, `${event.type} flags`);
    assert(event.loaded === loaded && event.total === total, `${event.type} counts`);
    assert(event.lengthComputable === computable, `${event.type} computable`);
  }

  const hi = await record("data:text/plain,hi");
  assert(
    hi.map((event) => event.type).join() === "loadstart,progress,load,loadend",
    hi.map((event) => event.type).join(),
  );
  counts(hi[0], 0, 2, true);
  counts(hi[1], 2, 2, true);
  counts(hi[2], 2, 2, true);
  counts(hi[3], 2, 2, true);

  const empty = await record("data:text/plain,");
  assert(
    empty.map((event) => event.type).join() === "loadstart,load,loadend",
    empty.map((event) => event.type).join(),
  );
  for (const event of empty) counts(event, 0, 0, false);

  const bad = await record("data:text/plain");
  assert(
    bad.map((event) => event.type).join() === "loadstart,error,loadend",
    bad.map((event) => event.type).join(),
  );
  for (const event of bad) counts(event, 0, 0, false);

  const late = new XMLHttpRequest();
  const lateTypes: string[] = [];
  late.open("GET", "data:text/plain,hi");
  late.send();
  late.addEventListener("loadstart", () => lateTypes.push("loadstart"));
  await new Promise((resolve) => {
    late.addEventListener("loadend", () => {
      lateTypes.push("loadend");
      resolve(undefined);
    });
  });
  assert(lateTypes.join() === "loadstart,loadend", lateTypes.join());
  assert(late.onabort === null, "onabort");
});

function wait(ms: number) {
  const start = performance.now();
  while (performance.now() - start < ms) {}
}

Yuzuko.test("XMLHttpRequest dispatches readystatechange, abort, and timeout", async () => {
  const fresh = new XMLHttpRequest();
  assert(fresh.readyState === 0, "unsent");
  assert(fresh.timeout === 0, "timeout starts at 0");
  assert(fresh.onabort === null && fresh.ontimeout === null, "handlers start null");
  assert(fresh.onreadystatechange === null, "readystatechange starts null");
  assert(XMLHttpRequest.UNSENT === 0 && fresh.UNSENT === 0, "UNSENT");
  assert(XMLHttpRequest.OPENED === 1 && fresh.OPENED === 1, "OPENED");
  assert(XMLHttpRequest.HEADERS_RECEIVED === 2 && fresh.HEADERS_RECEIVED === 2, "HEADERS_RECEIVED");
  assert(XMLHttpRequest.LOADING === 3 && fresh.LOADING === 3, "LOADING");
  assert(XMLHttpRequest.DONE === 4 && fresh.DONE === 4, "DONE");
  fresh.onabort = () => {};
  fresh.onabort = 1;
  assert(fresh.onabort === null, "non-function onabort");

  const opened: number[] = [];
  fresh.onreadystatechange = () => opened.push(fresh.readyState);
  fresh.open("GET", "data:text/plain,hi");
  assert(opened.join() === "1", opened.join());
  fresh.open("GET", "data:text/plain,yo");
  assert(opened.join() === "1", "second open");
  const quiet: string[] = [];
  fresh.addEventListener("abort", () => quiet.push("abort"));
  fresh.abort();
  assert(quiet.length === 0 && fresh.readyState === 1, "abort before send");

  const aborted: string[] = [];
  const request = new XMLHttpRequest();
  let during = -1;
  request.addEventListener("readystatechange", () => {
    aborted.push(`ready:${request.readyState}`);
  });
  request.onabort = function (event) {
    aborted.push("onabort");
    assert(this === request, "onabort this");
    assert(event instanceof ProgressEvent && event.type === "abort", "abort event");
    assert(event.isTrusted === true && event.bubbles === false && event.cancelable === false, "abort flags");
    assert(event.loaded === 0 && event.total === 0 && event.lengthComputable === false, "abort counts");
    during = request.readyState;
    assert(request.responseText === "" && request.status === 0, "abort response");
  };
  request.addEventListener("loadstart", () => aborted.push("loadstart"));
  request.addEventListener("progress", () => aborted.push("progress"));
  request.addEventListener("load", () => aborted.push("load"));
  request.addEventListener("error", () => aborted.push("error"));
  request.addEventListener("timeout", () => aborted.push("timeout"));
  request.addEventListener("loadend", () => aborted.push("loadend"));
  request.open("GET", "data:text/plain,hi");
  request.send();
  assert(request.readyState === 1 && request.responseText === "", "hidden until done");
  request.abort();
  assert(during === 4, "readyState during abort");
  assert(
    aborted.join() === "ready:1,ready:4,onabort,loadend",
    aborted.join(),
  );
  assert(request.readyState === 0, "unsent after abort");
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert(aborted.join() === "ready:1,ready:4,onabort,loadend", "aborted task dropped");

  const loaded: string[] = [];
  const kept = new XMLHttpRequest();
  kept.timeout = 1000;
  kept.onreadystatechange = () => {
    loaded.push(`ready:${kept.readyState}:${kept.responseText}`);
  };
  kept.open("GET", "data:text/plain,hi");
  kept.addEventListener("loadstart", () => loaded.push(`loadstart:${kept.responseText}`));
  kept.addEventListener("load", () => loaded.push("load"));
  kept.addEventListener("timeout", () => loaded.push("timeout"));
  kept.send();
  await new Promise((resolve) => kept.addEventListener("loadend", resolve));
  assert(
    loaded.join() === "ready:1:,ready:4:hi,loadstart:hi,load",
    loaded.join(),
  );
  assert(kept.readyState === 4 && kept.responseText === "hi", "loaded text");

  const timed: string[] = [];
  const slow = new XMLHttpRequest();
  slow.onreadystatechange = () => timed.push(`ready:${slow.readyState}`);
  slow.open("GET", "data:text/plain,hi");
  slow.ontimeout = function (event) {
    timed.push("ontimeout");
    assert(this === slow, "ontimeout this");
    assert(event instanceof ProgressEvent && event.type === "timeout", "timeout event");
    assert(event.isTrusted === true && event.bubbles === false && event.cancelable === false, "timeout flags");
    assert(event.loaded === 0 && event.total === 0 && event.lengthComputable === false, "timeout counts");
    assert(slow.readyState === 4 && slow.responseText === "", "timeout state");
  };
  slow.addEventListener("loadstart", () => timed.push("loadstart"));
  slow.addEventListener("load", () => timed.push("load"));
  slow.addEventListener("error", () => timed.push("error"));
  slow.addEventListener("abort", () => timed.push("abort"));
  slow.addEventListener("loadend", () => timed.push("loadend"));
  slow.send();
  wait(20);
  slow.timeout = 1;
  assert(slow.readyState === 4, "done after timeout");
  assert(
    timed.join() === "ready:1,ready:4,ontimeout,loadend",
    timed.join(),
  );
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert(timed.join() === "ready:1,ready:4,ontimeout,loadend", "timeout task dropped");
  slow.abort();
  assert(slow.readyState === 0, "abort after done");
  assert(timed.join() === "ready:1,ready:4,ontimeout,loadend", "no event from done");

  const replaced: string[] = [];
  const again = new XMLHttpRequest();
  again.open("GET", "data:text/plain,hi");
  again.send();
  again.addEventListener("load", () => replaced.push("load"));
  again.open("GET", "data:text/plain,yo");
  again.send();
  await new Promise((resolve) => again.addEventListener("loadend", resolve));
  assert(again.responseText === "yo", again.responseText);
  assert(replaced.join() === "load", replaced.join());

  let threw = false;
  try {
    fresh.timeout = Symbol() as unknown as number;
  } catch (error) {
    threw = error instanceof TypeError;
  }
  assert(threw, "timeout symbol");
});

Yuzuko.test("XMLHttpRequest open and send", async () => {
  assert(XMLHttpRequest.prototype.open.length === 2, "open length");
  assert(XMLHttpRequest.prototype.send.length === 0, "send length");

  function thrown(run: () => void): string {
    try {
      run();
      return "";
    } catch (error) {
      if (error instanceof DOMException) {
        return error.name;
      }
      if (error instanceof TypeError) {
        return `TypeError:${error.message}`;
      }
      if (error instanceof Error) {
        return `Error:${error.message}`;
      }
      return "other";
    }
  }

  function load(xhr: XMLHttpRequest): Promise<void> {
    return new Promise((resolve) => xhr.addEventListener("loadend", resolve));
  }

  const fresh = new XMLHttpRequest();
  for (const method of ["TRACE", "trace", "CONNECT", "TRACK", "CoNnEcT"]) {
    assert(thrown(() => fresh.open(method, "data:text/plain,no")) === "SecurityError", method);
    assert(fresh.readyState === 0, method);
  }

  const kept = new XMLHttpRequest();
  kept.onreadystatechange = () => {
    assert(kept.readyState === 1, "open state");
  };
  kept.open("GET", "data:text/plain,hi");
  kept.onreadystatechange = null;
  for (const method of ["TRACE", "connect"]) {
    assert(thrown(() => kept.open(method, "data:text/plain,no")) === "SecurityError", method);
    assert(kept.readyState === 1, method);
  }
  assert(thrown(() => kept.open("", "data:text/plain,no")) === "SyntaxError", "empty method");
  assert(thrown(() => kept.open("GET", "http://[")) === "SyntaxError", "bad url");
  assert(
    thrown(() => kept.open("GET\u0100", "data:,")) === "TypeError:The method is invalid",
    "byte string",
  );
  assert(thrown(() => kept.open(Symbol("m") as unknown as string, "data:,")).startsWith("TypeError"), "symbol");
  assert(
    thrown(() => kept.open("POST", "data:text/plain,yo", true, {
      toString() {
        throw new Error("user");
      },
    } as unknown as string)) === "Error:user",
    "username hook",
  );
  assert(kept.readyState === 1, "kept state");
  kept.send();
  await load(kept);
  assert(kept.responseText === "hi", kept.responseText);

  const early = new XMLHttpRequest();
  assert(thrown(() => early.send(new ReadableStream())) === "InvalidStateError", "send before open");
  assert(early.readyState === 0, "still unsent");

  const rejected = new XMLHttpRequest();
  rejected.open("POST", "data:text/plain,hi");
  const bodyError = "TypeError:The provided value is not of type 'XMLHttpRequestBodyInit'.";
  assert(thrown(() => rejected.send(new ReadableStream())) === bodyError, "stream");
  assert(thrown(() => rejected.send(new URL("https://example.com/"))) === bodyError, "url body");
  const shared = thrown(() => rejected.send(new SharedArrayBuffer(4)));
  assert(shared === "TypeError:The buffer is shared", shared);
  const sharedView = thrown(() => rejected.send(new Uint8Array(new SharedArrayBuffer(4))));
  assert(sharedView === "TypeError:The buffer is shared", sharedView);
  const detached = new ArrayBuffer(4);
  const view = new Uint8Array(detached);
  structuredClone(detached, { transfer: [detached] });
  assert(thrown(() => rejected.send(detached)) === "TypeError:The buffer is detached.", "detached");
  assert(thrown(() => rejected.send(view)) === "TypeError:The buffer is detached.", "detached view");
  assert(rejected.readyState === 1, "rejected stays opened");
  rejected.send("later");
  await load(rejected);
  assert(rejected.responseText === "hi", "send after a rejected body");

  const params = new URLSearchParams("a=1");
  params.toString = () => {
    throw new Error("hijacked");
  };
  const formBody = new XMLHttpRequest();
  formBody.open("POST", "data:text/plain,hi");
  formBody.send(params);
  assert(formBody.readyState === 1, "params serializer");
  await load(formBody);
  assert(formBody.responseText === "hi", "params payload");

  const hook = new XMLHttpRequest();
  hook.open("POST", "data:text/plain,hi");
  assert(thrown(() => hook.send({
    toString() {
      throw new Error("hook");
    },
  })) === "Error:hook", "body hook");
  assert(hook.readyState === 1, "hook stays opened");
  hook.send({
    toString() {
      return "body";
    },
  });
  await load(hook);
  assert(hook.responseText === "hi", "hook payload");

  for (const body of [null, undefined]) {
    const xhr = new XMLHttpRequest();
    xhr.open("GET", "data:text/plain,hi");
    xhr.send(body);
    await load(xhr);
    assert(xhr.responseText === "hi", "empty body");
  }

  const bodies: XMLHttpRequestBodyInit[] = [
    "hello",
    new Blob(["hello"], { type: "text/plain" }),
    new Uint8Array([1, 2, 3]),
    new ArrayBuffer(4),
    new URLSearchParams("a=b"),
    new FormData(),
  ];
  for (const body of bodies) {
    const xhr = new XMLHttpRequest();
    xhr.open("POST", "data:text/plain,hi");
    xhr.send(body);
    assert(xhr.readyState === 1, "post stays opened");
    await load(xhr);
    assert(xhr.responseText === "hi", "post payload");
  }

  for (const method of ["GET", "HEAD"]) {
    const xhr = new XMLHttpRequest();
    xhr.open(method, "data:text/plain,hi");
    xhr.send(new ReadableStream());
    assert(xhr.readyState === 1, `${method} ignores the body`);
    await load(xhr);
    assert(xhr.responseText === "hi", method);
  }

  const fromUrl = new XMLHttpRequest();
  fromUrl.open("GET", new URL("data:text/plain,hi"));
  fromUrl.send();
  await load(fromUrl);
  assert(fromUrl.responseText === "hi", "url object");

  const embedded = new XMLHttpRequest();
  embedded.open("GET", "https://user:pass@example.com/a");
  assert(embedded.readyState === 1, "credential url");
  const named = new XMLHttpRequest();
  named.open("GET", "https://example.com/a", true, "user", "pass");
  assert(named.readyState === 1, "username arguments");
  const dataUser = new XMLHttpRequest();
  dataUser.open("GET", "data:text/plain,hi", true, "user", "pass");
  dataUser.send();
  await load(dataUser);
  assert(dataUser.responseText === "hi", "data userinfo");

  const relative = new XMLHttpRequest();
  relative.open("GET", "progress_test.ts");
  assert(relative.readyState === 1, "relative url");

  const twice = new XMLHttpRequest();
  twice.open("GET", "data:text/plain,hi");
  twice.send();
  assert(thrown(() => twice.send()) === "InvalidStateError", "second send");
  await load(twice);

  const sync = new XMLHttpRequest();
  const syncEvents: string[] = [];
  sync.onreadystatechange = () => {
    syncEvents.push(`ready:${sync.readyState}:${sync.responseText}`);
  };
  sync.addEventListener("loadstart", () => syncEvents.push("loadstart"));
  sync.addEventListener("progress", () => syncEvents.push("progress"));
  sync.addEventListener("load", () => syncEvents.push("load"));
  sync.addEventListener("loadend", () => syncEvents.push("loadend"));
  sync.open("GET", "data:text/plain,hi", false);
  sync.send();
  assert(sync.readyState === 4 && sync.responseText === "hi", "sync result");
  assert(syncEvents.join() === "ready:1:,ready:4:hi,load,loadend", syncEvents.join());

  const viaUndefined = new XMLHttpRequest();
  const undefinedEvents: string[] = [];
  viaUndefined.addEventListener("readystatechange", () => {
    undefinedEvents.push(`ready:${viaUndefined.readyState}:${viaUndefined.responseText}`);
  });
  viaUndefined.addEventListener("loadstart", () => undefinedEvents.push("loadstart"));
  viaUndefined.addEventListener("progress", () => undefinedEvents.push("progress"));
  viaUndefined.addEventListener("load", () => undefinedEvents.push("load"));
  viaUndefined.addEventListener("loadend", () => undefinedEvents.push("loadend"));
  viaUndefined.open("GET", "data:text/plain,hi", undefined);
  viaUndefined.send();
  assert(
    undefinedEvents.join() === "ready:1:,ready:4:hi,load,loadend",
    undefinedEvents.join(),
  );
  assert(viaUndefined.readyState === 4 && viaUndefined.responseText === "hi", "undefined is sync");

  const asyncRequest = new XMLHttpRequest();
  asyncRequest.open("GET", "data:text/plain,hi", true);
  asyncRequest.send();
  assert(asyncRequest.readyState === 1 && asyncRequest.responseText === "", "async returns opened");
  await load(asyncRequest);
  assert(asyncRequest.responseText === "hi", "async text");

  const timed = new XMLHttpRequest();
  timed.timeout = 1000;
  timed.open("GET", "data:text/plain,hi", false);
  timed.send();
  assert(timed.readyState === 4 && timed.responseText === "hi", "sync timeout is allowed");

  const nested = new XMLHttpRequest();
  const nestedEvents: string[] = [];
  nested.onreadystatechange = () => {
    nestedEvents.push(`ready:${nested.readyState}`);
    if (nested.readyState === 4) {
      nested.open("GET", "data:text/plain,yo");
    }
  };
  nested.addEventListener("load", () => nestedEvents.push("load"));
  nested.addEventListener("loadend", () => nestedEvents.push("loadend"));
  nested.open("GET", "data:text/plain,hi", false);
  nested.send();
  assert(nested.readyState === 1, "reopened");
  assert(nestedEvents.join() === "ready:1,ready:4,ready:1", nestedEvents.join());

  const aborted = new XMLHttpRequest();
  const abortedEvents: string[] = [];
  aborted.onreadystatechange = () => {
    abortedEvents.push(`ready:${aborted.readyState}:${aborted.responseText}`);
    if (aborted.readyState === 4) {
      aborted.abort();
    }
  };
  aborted.addEventListener("abort", () => abortedEvents.push("abort"));
  aborted.addEventListener("load", () => abortedEvents.push(`load:${aborted.readyState}`));
  aborted.addEventListener("loadend", () => abortedEvents.push(`loadend:${aborted.readyState}`));
  aborted.open("GET", "data:text/plain,hi", false);
  aborted.send();
  assert(aborted.readyState === 0, "abort during sync");
  assert(
    abortedEvents.join() === "ready:1:,ready:4:hi,load:0,loadend:0",
    abortedEvents.join(),
  );

  const failed = new XMLHttpRequest();
  const failedEvents: string[] = [];
  failed.addEventListener("readystatechange", () => failedEvents.push("ready"));
  failed.addEventListener("error", () => failedEvents.push("error"));
  failed.addEventListener("loadend", () => failedEvents.push("loadend"));
  failed.open("GET", "data:text/plain", false);
  assert(thrown(() => failed.send()) === "NetworkError", "sync network");
  assert(failed.readyState === 4 && failed.responseText === "", "sync network state");
  assert(failedEvents.join() === "ready", failedEvents.join());
});
