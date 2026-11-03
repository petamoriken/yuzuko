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
