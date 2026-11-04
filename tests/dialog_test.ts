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

Yuzuko.test("file picker options are checked before the dialog", () => {
  const win = new Yuzuko.Window({ title: "picker", width: 80, height: 60 });
  assert(win.pickFiles.length === 0, "pickFiles length");
  assert(win.pickSaveFile.length === 0, "pickSaveFile length");

  let error = thrown(() => {
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
    win.pickSaveFile({ suggestedName: "" });
  });
  assert(
    errorMessage(error) === "suggestedName must be a file name.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickSaveFile({ suggestedName: "a/b.txt" });
  });
  assert(
    errorMessage(error) === "suggestedName must be a file name.",
    errorMessage(error),
  );

  error = thrown(() => {
    win.pickSaveFile({
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
    win.pickSaveFile({
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
    win.pickSaveFile({ suggestedName: "note.txt" });
  });
  assert(errorName(error) === "NotSupportedError", errorMessage(error));

  error = thrown(() => {
    win.pickSaveFile({ suggestedName: "." });
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
    win.pickSaveFile({ suggestedName: "a\\b" });
  });
  assert(
    errorMessage(error) === "suggestedName must be a file name.",
    errorMessage(error),
  );
});
