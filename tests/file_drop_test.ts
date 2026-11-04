function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function thrown(run: () => void): unknown {
  try {
    run();
    return undefined;
  } catch (error) {
    return error;
  }
}

function errorName(error: unknown): string {
  if (error instanceof DOMException || error instanceof Error) {
    return error.name;
  }
  return "";
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "";
}

Yuzuko.test("file drop events target the window", () => {
  const window = new Yuzuko.Window({
    width: 80,
    height: 60,
    title: "drop",
  });
  const separator = Yuzuko.build.os === "windows" ? "\\" : "/";
  const absolute = Yuzuko.build.os === "windows"
    ? "C:\\yuzuko-drop\\a.txt"
    : "/yuzuko-drop/a.txt";
  const events: Array<{
    type: string;
    paths?: readonly string[];
    trusted: boolean;
    target: boolean;
    bubbles: boolean;
    cancelable: boolean;
  }> = [];
  const take = (event: Event) => {
    const record = event as Event & { paths?: readonly string[] };
    events.push({
      type: event.type,
      paths: record.paths?.slice(),
      trusted: event.isTrusted,
      target: event.target === window,
      bubbles: event.bubbles,
      cancelable: event.cancelable,
    });
  };
  window.addEventListener("filehover", take);
  window.addEventListener("filehovercancel", take);
  window.addEventListener("filedrop", take);
  let yuzukoSaw = false;
  const onYuzuko = () => {
    yuzukoSaw = true;
  };
  Yuzuko.addEventListener("filedrop", onYuzuko);
  let surfaceSaw = false;
  const onSurface = () => {
    surfaceSaw = true;
  };
  window.surface.addEventListener("filedrop", onSurface);
  try {
    let error = thrown(() => {
      Yuzuko.__test.inject(window, [{ kind: "filedrop", paths: [] }]);
    });
    assert(
      errorMessage(error) === "A file event requires paths",
      errorMessage(error),
    );
    error = thrown(() => {
      Yuzuko.__test.inject(window, [{ kind: "filehover", paths: [1] }]);
    });
    assert(
      errorMessage(error) === "A file path must be a string",
      errorMessage(error),
    );
    error = thrown(() => {
      Yuzuko.__test.inject(window, [{ kind: "filedrop", paths: [""] }]);
    });
    assert(
      errorMessage(error) === "A file path must not be empty",
      errorMessage(error),
    );

    Yuzuko.__test.inject(window, [
      { kind: "filehover", paths: [absolute, "rel.txt"] },
      { kind: "filehovercancel" },
      { kind: "filedrop", paths: [absolute] },
      { kind: "filedrop", paths: ["b.txt"] },
    ]);
    assert(events.length === 3, `count ${events.length}`);
    assert(events[0].type === "filehover", events[0].type);
    assert(events[0].paths?.length === 2, "hover paths");
    assert(events[0].paths?.[0] === absolute, "hover absolute");
    assert(
      events[0].paths?.[1]?.endsWith(`${separator}rel.txt`) === true,
      events[0].paths?.[1] ?? "",
    );
    assert(events[0].trusted && events[0].target, "hover target");
    assert(!events[0].bubbles && !events[0].cancelable, "hover flags");
    assert(events[1].type === "filehovercancel", events[1].type);
    assert(events[1].paths === undefined, "cancel paths");
    assert(events[1].trusted && events[1].target, "cancel target");
    assert(events[2].type === "filedrop", events[2].type);
    assert(events[2].paths?.length === 2, "drop paths");
    assert(events[2].paths?.[0] === absolute, "drop absolute");
    assert(
      events[2].paths?.[1]?.endsWith(`${separator}b.txt`) === true,
      events[2].paths?.[1] ?? "",
    );
    assert(events[2].trusted && events[2].target, "drop target");
    assert(!events[2].bubbles && !events[2].cancelable, "drop flags");
    assert(!yuzukoSaw, "filedrop stays off Yuzuko");
    assert(!surfaceSaw, "filedrop stays off the surface");

    window.close();
    error = thrown(() => {
      Yuzuko.__test.inject(window, [{ kind: "filehovercancel" }]);
    });
    assert(errorName(error) === "InvalidStateError", errorName(error));
    assert(
      errorMessage(error) === "The surface is closed.",
      errorMessage(error),
    );
  } finally {
    window.removeEventListener("filehover", take);
    window.removeEventListener("filehovercancel", take);
    window.removeEventListener("filedrop", take);
    Yuzuko.removeEventListener("filedrop", onYuzuko);
    window.surface.removeEventListener("filedrop", onSurface);
    window.close();
  }
});
