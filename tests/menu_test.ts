function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

Yuzuko.test({
  name: "Linux has no menu bar to install",
  ignore: Yuzuko.build.os !== "linux",
  fn() {
    const menu = new Yuzuko.Menu([
      { label: "File", submenu: [{ label: "Save", action: "file.save" }] },
    ]);
    let name = "";
    try {
      Yuzuko.setMenu(menu);
    } catch (error) {
      name = error instanceof DOMException ? error.name : "";
    }
    assert(name === "NotSupportedError", `setMenu: ${name}`);
  },
});

Yuzuko.test({
  name: "menuclick carries the selected action",
  ignore: Yuzuko.build.os === "linux",
  fn() {
    assert(Yuzuko instanceof EventTarget, "Yuzuko is an EventTarget");
    assert(globalThis.Menu === undefined, "Menu stays off globalThis");
    assert(typeof Yuzuko.setMenu === "function", "setMenu");
    assert(typeof document === "undefined", "document stays absent");

    const menu = new Yuzuko.Menu([
      {
        label: "File",
        submenu: [
          { label: "Save", action: "file.save" },
          {
            label: "Export",
            submenu: [{ label: "PNG", action: "file.export.png" }],
          },
        ],
      },
    ]);
    Yuzuko.setMenu(menu);

    let action = "";
    let trusted = false;
    const onClick = (event: Event) => {
      const record = event as Event & { action?: string };
      action = record.action ?? "";
      trusted = event.isTrusted;
    };
    Yuzuko.addEventListener("menuclick", onClick);
    const window = new Yuzuko.Window({ width: 80, height: 60, title: "menu" });
    try {
      Yuzuko.__test.inject(window, [{ kind: "menu", action: "file.save" }]);
      assert(action === "file.save", action);
      assert(trusted, "menuclick is trusted");
      Yuzuko.__test.inject(window, [
        { kind: "menu", action: "file.export.png" },
      ]);
      assert(action === "file.export.png", action);

      let name = "";
      try {
        Yuzuko.__test.inject(window, [{ kind: "menu", action: "missing" }]);
      } catch (error) {
        name = error instanceof Error ? error.name : "";
      }
      assert(name === "TypeError", `missing action: ${name}`);
    } finally {
      window.close();
      Yuzuko.removeEventListener("menuclick", onClick);
    }
  },
});
