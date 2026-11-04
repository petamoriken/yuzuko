function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

Yuzuko.test("dialogs belong to the window and are absent when headless", () => {
  assert(typeof globalThis.alert === "undefined", "no global alert");
  const win = new Yuzuko.Window({ title: "dialogs", width: 80, height: 60 });
  let text = "";
  const value = {
    toString() {
      text = "stringified";
      return "hello";
    },
  };
  let name = "";
  try {
    win.alert(value);
  } catch (error) {
    name = error instanceof DOMException ? error.name : "";
  }
  assert(text === "stringified", text);
  assert(name === "NotSupportedError", `alert: ${name}`);

  name = "";
  try {
    win.confirm("Discard?");
  } catch (error) {
    name = error instanceof DOMException ? error.name : "";
  }
  assert(name === "NotSupportedError", `confirm: ${name}`);

  name = "";
  try {
    win.prompt("Name:", "Ada");
  } catch (error) {
    name = error instanceof DOMException ? error.name : "";
  }
  assert(name === "NotSupportedError", `prompt: ${name}`);

  win.close();
  name = "";
  try {
    win.alert("closed");
  } catch (error) {
    name = error instanceof DOMException ? error.name : "";
  }
  assert(name === "InvalidStateError", `closed: ${name}`);
});

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

Yuzuko.test(
  "dialog labels and file picker options are checked before the dialog",
  () => {
  assert(typeof Yuzuko.setDialogLabels === "function", "setDialogLabels");
  assert(Yuzuko.setDialogLabels.length === 0, "setDialogLabels length");

  let error = thrown(() => {
    Yuzuko.setDialogLabels(null);
  });
  assert(errorName(error) === "TypeError", errorName(error));
  assert(
    errorMessage(error) === "The dialog labels must be an object.",
    errorMessage(error),
  );

  error = thrown(() => {
    Yuzuko.setDialogLabels("OK");
  });
  assert(
    errorMessage(error) === "The dialog labels must be an object.",
    errorMessage(error),
  );

  error = thrown(() => {
    Yuzuko.setDialogLabels({ ok: 1 });
  });
  assert(errorMessage(error) === "ok must be a string.", errorMessage(error));

  error = thrown(() => {
    Yuzuko.setDialogLabels({ open: false });
  });
  assert(
    errorMessage(error) === "open must be a string.",
    errorMessage(error),
  );

  error = thrown(() => {
    Yuzuko.setDialogLabels({ select: null });
  });
  assert(
    errorMessage(error) === "select must be a string.",
    errorMessage(error),
  );

  error = thrown(() => {
    Yuzuko.setDialogLabels({ ok: "" });
  });
  assert(errorMessage(error) === "ok must be text.", errorMessage(error));

  error = thrown(() => {
    Yuzuko.setDialogLabels({ save: "" });
  });
  assert(errorMessage(error) === "save must be text.", errorMessage(error));

  error = thrown(() => {
    Yuzuko.setDialogLabels({ cancel: "a\0b" });
  });
  assert(errorMessage(error) === "cancel must be text.", errorMessage(error));

  error = thrown(() => {
    Yuzuko.setDialogLabels({
      get ok() {
        throw new TypeError("ok getter");
      },
    });
  });
  assert(error instanceof TypeError, errorName(error));
  assert(errorMessage(error) === "ok getter", errorMessage(error));

  let later = false;
  error = thrown(() => {
    Yuzuko.setDialogLabels({
      ok: "",
      get cancel() {
        later = true;
        return "Cancel";
      },
    });
  });
  assert(later === false, "later label");
  assert(errorMessage(error) === "ok must be text.", errorMessage(error));

  let seen = false;
  Yuzuko.setDialogLabels({
    get title() {
      seen = true;
      throw new TypeError("title");
    },
  });
  assert(seen === false, "unknown label");

  Yuzuko.setDialogLabels();
  Yuzuko.setDialogLabels(undefined);
  Yuzuko.setDialogLabels({});
  Yuzuko.setDialogLabels([]);
  Yuzuko.setDialogLabels({ ok: "はい", cancel: "いいえ" });
  Yuzuko.setDialogLabels({ open: "開く", save: "保存", select: "選択" });
  Yuzuko.setDialogLabels({
    ok: "OK",
    cancel: "Cancel",
    open: "Open",
    save: "Save",
    select: "Select",
  });

  const win = new Yuzuko.Window({ title: "picker", width: 80, height: 60 });
  assert(win.pickFiles.length === 0, "pickFiles length");
  assert(win.pickSaveFile === undefined, "no pickSaveFile");
  assert(win.pickDirectory === undefined, "no pickDirectory");

  error = thrown(() => {
    win.pickFiles(null);
  });
  assert(errorName(error) === "TypeError", errorName(error));
  assert(
    errorMessage(error) === "The file picker options must be an object.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ multiple: 1 });
  });
  assert(errorMessage(error) === "multiple must be a boolean.", errorMessage(error));

  error = thrown(() => {
    win.pickFiles({
      get multiple() {
        throw new TypeError("multiple getter");
      },
    });
  });
  assert(error instanceof TypeError, errorName(error));
  assert(errorMessage(error) === "multiple getter", errorMessage(error));

  error = thrown(() => {
    win.pickFiles({ filters: { extensions: ["txt"] } });
  });
  assert(
    errorMessage(error) === "filters must be an array.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({
      get filters() {
        throw new TypeError("filters getter");
      },
    });
  });
  assert(error instanceof TypeError, errorName(error));
  assert(errorMessage(error) === "filters getter", errorMessage(error));

  error = thrown(() => {
    win.pickFiles({ filters: ["txt"] });
  });
  assert(
    errorMessage(error) === "A file filter must be an object.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ filters: [{ name: "Text" }] });
  });
  assert(
    errorMessage(error) === "extensions must be an array of extensions.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ filters: [{ extensions: [] }] });
  });
  assert(
    errorMessage(error) === "extensions must be an array of extensions.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ filters: [{ extensions: "txt" }] });
  });
  assert(
    errorMessage(error) === "extensions must be an array of extensions.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ filters: [{ extensions: ["a/b"] }] });
  });
  assert(
    errorMessage(error) === "A file extension has no separator.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ filters: [{ extensions: ["a*"] }] });
  });
  assert(
    errorMessage(error) === "A file extension has no separator.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ mode: "save", suggestedName: "" });
  });
  assert(
    errorMessage(error) === "suggestedName must be a file name.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ mode: "save", suggestedName: "a/b.txt" });
  });
  assert(
    errorMessage(error) === "suggestedName must be a file name.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ mode: "save",
      suggestedName: {
        toString() {
          throw new TypeError("name hook");
        },
      },
    });
  });
  assert(error instanceof TypeError, errorName(error));
  assert(errorMessage(error) === "name hook", errorMessage(error));

  error = thrown(() => {
    win.pickFiles({ mode: "save",
      filters: [{ name: "Text\0", extensions: ["txt"] }],
    });
  });
  assert(
    errorMessage(error) === "A file filter name must be text.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({
      filters: [
        {
          name: {
            toString() {
              throw new TypeError("filter name");
            },
          },
          extensions: ["txt"],
        },
      ],
    });
  });
  assert(error instanceof TypeError, errorName(error));
  assert(errorMessage(error) === "filter name", errorMessage(error));

  error = thrown(() => {
    win.pickFiles({ filters: [{ extensions: ["*"] }] });
  });
  assert(errorName(error) === "NotSupportedError", errorName(error));
  assert(
    errorMessage(error) === "This process has no dialog.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({
      filters: [{ extensions: ["txt"] }, { extensions: ["*"] }],
    });
  });
  assert(errorName(error) === "NotSupportedError", errorName(error));

  error = thrown(() => {
    win.pickFiles({ filters: [{ extensions: ["txt", "*"] }] });
  });
  assert(errorName(error) === "NotSupportedError", errorName(error));

  const inherited = Object.create({
    filters: [{ name: "Text", extensions: ["txt", "md"] }],
  });
  error = thrown(() => {
    win.pickFiles(inherited);
  });
  assert(errorName(error) === "NotSupportedError", errorName(error));

  error = thrown(() => {
    win.pickFiles({
      multiple: false,
      filters: [{ name: "Text", extensions: ["txt", "md"] }],
    });
  });
  assert(errorName(error) === "NotSupportedError", errorName(error));

  error = thrown(() => {
    win.pickFiles({ mode: "save", suggestedName: "note.txt" });
  });
  assert(errorName(error) === "NotSupportedError", errorMessage(error));

  error = thrown(() => {
    win.pickFiles({ mode: "save", suggestedName: "." });
  });
  assert(errorName(error) === "NotSupportedError", errorMessage(error));

  error = thrown(() => {
    win.pickFiles({
      get mode() {
        throw new TypeError("mode getter");
      },
    });
  });
  assert(error instanceof TypeError, errorName(error));
  assert(errorMessage(error) === "mode getter", errorMessage(error));

  error = thrown(() => {
    win.pickFiles({ mode: "nope", multiple: 1 });
  });
  assert(
    errorMessage(error) ===
      'mode must be "open", "save", or "directory".',
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ mode: 1 });
  });
  assert(
    errorMessage(error) ===
      'mode must be "open", "save", or "directory".',
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ mode: "open", suggestedName: "note.txt" });
  });
  assert(
    errorMessage(error) === "suggestedName applies only to a save dialog.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ mode: "save", multiple: false });
  });
  assert(
    errorMessage(error) === "multiple applies only to an open dialog.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({
      mode: "directory",
      multiple: true,
      filters: [{ extensions: ["txt"] }],
    });
  });
  assert(
    errorMessage(error) === "multiple applies only to an open dialog.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({
      mode: "directory",
      filters: [{ extensions: ["txt"] }],
    });
  });
  assert(
    errorMessage(error) === "filters apply only to a file dialog.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({
      mode: "directory",
      get filters() {
        throw new TypeError("directory filters");
      },
    });
  });
  assert(error instanceof TypeError, errorName(error));
  assert(errorMessage(error) === "directory filters", errorMessage(error));

  error = thrown(() => {
    win.pickFiles({ mode: "directory", suggestedName: "docs" });
  });
  assert(
    errorMessage(error) === "suggestedName applies only to a save dialog.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({
      mode: "directory",
      get title() {
        throw new TypeError("title");
      },
    });
  });
  assert(errorName(error) === "NotSupportedError", errorName(error));
  assert(
    errorMessage(error) === "This process has no dialog.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ mode: "directory" });
  });
  assert(errorName(error) === "NotSupportedError", errorMessage(error));

  error = thrown(() => {
    win.pickFiles({ mode: "open" });
  });
  assert(errorName(error) === "NotSupportedError", errorMessage(error));

  win.close();
  error = thrown(() => {
    win.pickFiles();
  });
  assert(errorName(error) === "InvalidStateError", `closed: ${errorName(error)}`);
  assert(errorMessage(error) === "The surface is closed.", errorMessage(error));

  error = thrown(() => {
    win.pickFiles(null);
  });
  assert(errorName(error) === "TypeError", `closed null: ${errorName(error)}`);
  assert(
    errorMessage(error) === "The file picker options must be an object.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ filters: "txt" });
  });
  assert(errorName(error) === "TypeError", `closed filters: ${errorName(error)}`);
  assert(
    errorMessage(error) === "filters must be an array.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ mode: "save", suggestedName: "a\\b" });
  });
  assert(
    errorMessage(error) === "suggestedName must be a file name.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ mode: "directory" });
  });
  assert(
    errorName(error) === "InvalidStateError",
    `closed dir: ${errorName(error)}`,
  );
  assert(errorMessage(error) === "The surface is closed.", errorMessage(error));

  error = thrown(() => {
    win.pickFiles({ mode: "directory", multiple: false });
  });
  assert(
    errorMessage(error) === "multiple applies only to an open dialog.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickFiles({ mode: "nope" });
  });
  assert(
    errorMessage(error) === 'mode must be "open", "save", or "directory".',
    errorMessage(error),
  );
});
