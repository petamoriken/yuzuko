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
    // @ts-expect-error File events carry paths.
    let paths: readonly string[] | undefined = event.paths;
    const copied = paths?.slice();
    events.push({
      type: event.type,
      ...(copied === undefined ? {} : { paths: copied }),
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
    const hover = events[0];
    const cancel = events[1];
    const drop = events[2];
    if (hover === undefined || cancel === undefined || drop === undefined) {
      throw new Error(`count ${events.length}`);
    }
    assert(hover.type === "filehover", hover.type);
    assert(hover.paths?.length === 2, "hover paths");
    assert(hover.paths?.[0] === absolute, "hover absolute");
    assert(
      hover.paths?.[1]?.endsWith(`${separator}rel.txt`) === true,
      hover.paths?.[1] ?? "",
    );
    assert(hover.trusted && hover.target, "hover target");
    assert(!hover.bubbles && !hover.cancelable, "hover flags");
    assert(cancel.type === "filehovercancel", cancel.type);
    assert(cancel.paths === undefined, "cancel paths");
    assert(cancel.trusted && cancel.target, "cancel target");
    assert(drop.type === "filedrop", drop.type);
    assert(drop.paths?.length === 2, "drop paths");
    assert(drop.paths?.[0] === absolute, "drop absolute");
    assert(
      drop.paths?.[1]?.endsWith(`${separator}b.txt`) === true,
      drop.paths?.[1] ?? "",
    );
    assert(drop.trusted && drop.target, "drop target");
    assert(!drop.bubbles && !drop.cancelable, "drop flags");
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
