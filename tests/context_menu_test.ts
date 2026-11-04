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

Yuzuko.test("a context menu clicks its window", () => {
  const window = new Yuzuko.Window({
    width: 80,
    height: 60,
    title: "context",
  });
  assert(window.setContextMenu.length === 1, "setContextMenu length");
  assert(window.showContextMenu.length === 3, "showContextMenu length");
  const menu = new Yuzuko.Menu([{ label: "Save", action: "ctx.save" }]);
  const other = new Yuzuko.Menu([{ label: "Other", action: "ctx.other" }]);
  window.setContextMenu(menu);

  let action = "";
  let trusted = false;
  let targetIsWindow = false;
  const onClick = (event: Event) => {
    const record = event as Event & { action?: string };
    action = record.action ?? "";
    trusted = event.isTrusted;
    targetIsWindow = event.target === window;
  };
  window.addEventListener("menuclick", onClick);
  let yuzukoSaw = false;
  const onYuzuko = () => {
    yuzukoSaw = true;
  };
  Yuzuko.addEventListener("menuclick", onYuzuko);
  try {
    Yuzuko.__test.inject(window, [{ kind: "menu", action: "ctx.save" }]);
    assert(action === "ctx.save", action);
    assert(trusted, "menuclick is trusted");
    assert(targetIsWindow, "menuclick targets the window");
    assert(!yuzukoSaw, "menuclick stays off Yuzuko");

    let error = thrown(() => {
      Yuzuko.__test.inject(window, [{ kind: "menu", action: "missing" }]);
    });
    assert(errorName(error) === "TypeError", `missing: ${errorName(error)}`);
    assert(
      errorMessage(error) === "The menu action is not installed",
      errorMessage(error),
    );

    error = thrown(() => {
      window.showContextMenu(menu, 1);
    });
    assert(
      errorMessage(error) === "Not enough arguments",
      errorMessage(error),
    );

    error = thrown(() => {
      window.showContextMenu(other, 4, 6);
    });
    assert(errorName(error) === "NotSupportedError", errorName(error));
    assert(
      errorMessage(error) === "This process has no context menu.",
      errorMessage(error),
    );
    action = "";
    Yuzuko.__test.inject(window, [{ kind: "menu", action: "ctx.save" }]);
    assert(action === "ctx.save", `kept: ${action}`);
    error = thrown(() => {
      Yuzuko.__test.inject(window, [{ kind: "menu", action: "ctx.other" }]);
    });
    assert(errorName(error) === "TypeError", `other: ${errorName(error)}`);

    error = thrown(() => {
      window.showContextMenu(other, Number.NaN, 1);
    });
    assert(errorName(error) === "TypeError", errorName(error));
    assert(
      errorMessage(error) === "showContextMenu requires a finite x and y.",
      errorMessage(error),
    );

    error = thrown(() => {
      window.showContextMenu({}, Number.NaN, 1);
    });
    assert(
      errorMessage(error) === "showContextMenu requires a finite x and y.",
      errorMessage(error),
    );

    error = thrown(() => {
      window.showContextMenu({}, 1, 1);
    });
    assert(errorName(error) === "TypeError", errorName(error));
    assert(
      errorMessage(error) === "showContextMenu requires a Menu.",
      errorMessage(error),
    );

    window.setContextMenu(null);
    error = thrown(() => {
      Yuzuko.__test.inject(window, [{ kind: "menu", action: "ctx.save" }]);
    });
    assert(errorName(error) === "TypeError", `cleared: ${errorName(error)}`);

    error = thrown(() => {
      window.showContextMenu(other, 1, 1);
    });
    assert(errorName(error) === "NotSupportedError", errorName(error));
    assert(
      errorMessage(error) === "This process has no context menu.",
      errorMessage(error),
    );

    window.close();
    error = thrown(() => {
      window.showContextMenu(other, Number.NaN, 0);
    });
    assert(errorName(error) === "TypeError", `closed nan: ${errorName(error)}`);

    error = thrown(() => {
      window.showContextMenu(other, 1, 1);
    });
    assert(
      errorName(error) === "InvalidStateError",
      `closed show: ${errorName(error)}`,
    );

    error = thrown(() => {
      window.showContextMenu({}, 1, 1);
    });
    assert(
      errorName(error) === "InvalidStateError",
      `closed menu: ${errorName(error)}`,
    );

    error = thrown(() => {
      window.setContextMenu(null);
    });
    assert(
      errorName(error) === "InvalidStateError",
      `closed clear: ${errorName(error)}`,
    );

    error = thrown(() => {
      window.setContextMenu({});
    });
    assert(errorName(error) === "TypeError", `closed set: ${errorName(error)}`);
    assert(
      errorMessage(error) === "setContextMenu requires a Menu or null.",
      errorMessage(error),
    );

    const second = new Yuzuko.Window({ width: 40, height: 40 });
    const kept = new Yuzuko.Menu([{ label: "Keep", action: "ctx.keep" }]);
    const alternate = new Yuzuko.Menu([
      { label: "Alt", action: "ctx.alt" },
    ]);
    second.setContextMenu(kept);
    let sawContext = false;
    second.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      sawContext = true;
      try {
        second.showContextMenu(alternate, 2, 2);
      } catch (shown) {
        if (errorName(shown) !== "NotSupportedError") {
          throw shown;
        }
      }
    });
    Yuzuko.__test.inject(second, [
      { kind: "pointer", type: "pointerup", x: 2, y: 2, button: 2 },
    ]);
    assert(sawContext, "contextmenu reached the window");
    let keptAction = "";
    second.addEventListener("menuclick", (event) => {
      const record = event as Event & { action?: string };
      keptAction = record.action ?? "";
    });
    Yuzuko.__test.inject(second, [{ kind: "menu", action: "ctx.keep" }]);
    assert(keptAction === "ctx.keep", keptAction);
    error = thrown(() => {
      Yuzuko.__test.inject(second, [{ kind: "menu", action: "ctx.alt" }]);
    });
    assert(errorName(error) === "TypeError", `alt: ${errorName(error)}`);
    second.close();
  } finally {
    window.removeEventListener("menuclick", onClick);
    Yuzuko.removeEventListener("menuclick", onYuzuko);
  }
});
