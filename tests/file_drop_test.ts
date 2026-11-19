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

Yuzuko.test("a coalesced drop exposes both files and no custom event", () => {
  const window = new Yuzuko.Window({
    width: 80,
    height: 60,
    title: "drop",
  });
  const absolute = Yuzuko.build.os === "windows"
    ? "C:\\yuzuko-drop\\a.txt"
    : "/yuzuko-drop/a.txt";
  let custom = 0;
  const onCustom = () => {
    custom += 1;
  };
  for (const type of ["filehover", "filehovercancel", "filedrop"]) {
    window.addEventListener(type, onCustom);
    Yuzuko.addEventListener(type, onCustom);
    window.surface.addEventListener(type, onCustom);
  }
  const drops: DragEvent[] = [];
  window.addEventListener("drop", (event) => {
    drops.push(event);
  });
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
    assert(custom === 0, "no custom file event");
    assert(drops.length === 1, `drops ${drops.length}`);
    const drag = drops[0];
    assert(drag !== undefined, "drop");
    assert(drag.dataTransfer?.files.length === 2, "two files");
    assert(drag.dataTransfer?.files[0]?.name === "a.txt", "first name");
    assert(drag.dataTransfer?.files[1]?.name === "b.txt", "second name");

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
    for (const type of ["filehover", "filehovercancel", "filedrop"]) {
      window.removeEventListener(type, onCustom);
      Yuzuko.removeEventListener(type, onCustom);
      window.surface.removeEventListener(type, onCustom);
    }
    window.close();
  }
});
