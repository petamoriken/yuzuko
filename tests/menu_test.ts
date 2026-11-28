declare const document: unknown;

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
    assert(
      // @ts-expect-error Menu is absent on the main global.
      globalThis.Menu === undefined,
      "Menu stays off globalThis",
    );
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
      // @ts-expect-error The host event carries action.
      let chosen: string | undefined = event.action;
      action = chosen ?? "";
      trusted = event.isTrusted;
    };
    Yuzuko.addEventListener("menuclick", onClick);
    const window = new Yuzuko.Window({ width: 80, height: 60, title: "menu" });
    try {
      Yuzuko.__test.inject(window, [{ kind: "menu", action: "file.save" }]);
      let saved: string = action;
      assert(saved === "file.save", action);
      assert(trusted, "menuclick is trusted");
      Yuzuko.__test.inject(window, [
        { kind: "menu", action: "file.export.png" },
      ]);
      assert(action === "file.export.png", action);

      // A replaced globalThis.Yuzuko does not take the event.
      const realm = globalThis as unknown as { Yuzuko: unknown };
      const original = realm.Yuzuko;
      const test = Yuzuko.__test;
      realm.Yuzuko = {};
      try {
        test.inject(window, [{ kind: "menu", action: "file.save" }]);
      } finally {
        realm.Yuzuko = original;
      }
      const replaced: string = action;
      assert(replaced === "file.save", `after replace: ${replaced}`);

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
