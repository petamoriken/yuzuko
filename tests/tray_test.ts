function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function notSupported(body: () => void, label: string) {
  let name = "";
  try {
    body();
  } catch (error) {
    name = error instanceof DOMException ? error.name : String(error);
  }
  assert(name === "NotSupportedError", `${label}: ${name}`);
}

const PNG = Uint8Array.from(
  atob(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  ),
  (char) => char.charCodeAt(0),
);

Yuzuko.test("tray and dock use methods and their own menuclick", () => {
  assert(typeof Yuzuko.Tray === "function", "Tray");
  assert(Yuzuko.dock instanceof EventTarget, "dock is an EventTarget");
  assert(typeof Yuzuko.dock.visible === "undefined", "no dock.visible");
  assert(typeof globalThis.Tray === "undefined", "Tray stays off globalThis");

  const tray = new Yuzuko.Tray();
  const window = new Yuzuko.Window({ width: 80, height: 60, title: "tray" });
  let trayAction = "";
  let clicks = 0;
  let dockAction = "";
  const onTray = (event: Event) => {
    trayAction = (event as Event & { action?: string }).action ?? "";
  };
  const onClick = () => {
    clicks += 1;
  };
  const onDock = (event: Event) => {
    dockAction = (event as Event & { action?: string }).action ?? "";
  };
  tray.addEventListener("menuclick", onTray);
  tray.addEventListener("click", onClick);
  Yuzuko.dock.addEventListener("menuclick", onDock);
  const macos = Yuzuko.build.os === "darwin";
  const linux = Yuzuko.build.os === "linux";
  try {
    tray.setIcon(PNG);
    if (macos) {
      tray.setIcon(PNG, { template: true });
    } else {
      notSupported(() => tray.setIcon(PNG, { template: true }), "template");
    }
    if (linux) {
      notSupported(() => tray.setTooltip("Yuzuko"), "tooltip");
    } else {
      tray.setTooltip("Yuzuko");
      tray.setTooltip(null);
    }
    const bounds = tray.bounds;
    assert(
      bounds === null ||
        (typeof bounds.x === "number" && typeof bounds.width === "number"),
      "bounds",
    );
    tray.setMenu(
      new Yuzuko.Menu([{ label: "Open", action: "tray.open" }]),
    );
    const dockMenu = new Yuzuko.Menu([{ label: "Quit", action: "dock.quit" }]);
    if (macos) {
      Yuzuko.dock.setMenu(dockMenu);
      Yuzuko.dock.setBadge("3");
      Yuzuko.dock.setBadge(null);
      Yuzuko.dock.bounce();
      Yuzuko.dock.setVisible(false);
      Yuzuko.dock.setVisible(true);
    } else {
      // There is no Dock. `bounce` needs an OS window, and this one is
      // headless under `yuzuko test`.
      notSupported(() => Yuzuko.dock.setMenu(dockMenu), "dock menu");
      notSupported(() => Yuzuko.dock.setBadge("3"), "badge");
      notSupported(() => Yuzuko.dock.setVisible(false), "visible");
      notSupported(() => Yuzuko.dock.bounce(), "bounce");
    }

    Yuzuko.__test.inject(window, [{ kind: "menu", action: "tray.open" }]);
    assert(trayAction === "tray.open", trayAction);
    if (macos) {
      Yuzuko.__test.inject(window, [{ kind: "menu", action: "dock.quit" }]);
      assert(dockAction === "dock.quit", dockAction);
    }
    Yuzuko.__test.inject(window, [{ kind: "trayclick" }]);
    assert(clicks === 1, `clicks ${clicks}`);

    let name = "";
    try {
      tray.setIcon(new Uint8Array([1, 2, 3]));
    } catch (error) {
      name = error instanceof Error ? error.name : "";
    }
    assert(name === "TypeError", `bad png: ${name}`);
  } finally {
    tray.close();
    window.close();
    tray.removeEventListener("menuclick", onTray);
    tray.removeEventListener("click", onClick);
    Yuzuko.dock.removeEventListener("menuclick", onDock);
  }

  let closed = "";
  try {
    tray.setTooltip("after");
  } catch (error) {
    closed = error instanceof DOMException ? error.name : "";
  }
  assert(closed === "InvalidStateError", `closed tray: ${closed}`);
});
