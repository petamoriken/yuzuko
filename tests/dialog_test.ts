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
