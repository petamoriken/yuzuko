function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function gesture(window: Yuzuko.Window): void {
  Yuzuko.__test.inject(window, [
    { kind: "focus" },
    { kind: "pointer", type: "pointerdown", x: 4, y: 6, button: 0 },
    { kind: "pointer", type: "pointerup", x: 4, y: 6, button: 0 },
  ]);
}

function bytesEqual(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) {
    return false;
  }
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return false;
    }
  }
  return true;
}

const PNG = Uint8Array.from(
  atob(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  ),
  (char) => char.charCodeAt(0),
);

Yuzuko.test("clipboard round-trips text and PNG from a focused user action", async () => {
  const window = new Yuzuko.Window({ width: 120, height: 80, title: "clip" });
  const other = new Yuzuko.Window({ width: 120, height: 80, title: "other" });
  const events: string[] = [];
  const onCopy = (event: Event) => events.push(event.type);
  const onCut = (event: Event) => events.push(event.type);
  const onPaste = (event: Event) => events.push(event.type);
  window.addEventListener("copy", onCopy);
  window.addEventListener("cut", onCut);
  window.addEventListener("paste", onPaste);
  try {
    assert(navigator.clipboard instanceof Clipboard, "clipboard");
    assert(ClipboardItem.supports("text/plain"), "supports text");
    assert(ClipboardItem.supports("image/png"), "supports png");
    assert(!ClipboardItem.supports("text/html"), "rejects html");

    Yuzuko.__test.inject(window, [{ kind: "focus" }]);
    let name = "";
    try {
      await navigator.clipboard.writeText("without a gesture");
    } catch (error) {
      name = error instanceof DOMException ? error.name : "";
    }
    assert(name === "NotAllowedError", `no gesture: ${name}`);

    gesture(window);
    Yuzuko.__test.inject(window, [{ kind: "blur" }]);
    Yuzuko.__test.inject(other, [{ kind: "focus" }]);
    Yuzuko.__test.inject(window, [
      { kind: "pointer", type: "pointerdown", x: 1, y: 1, button: 0 },
    ]);
    name = "";
    try {
      await navigator.clipboard.readText();
    } catch (error) {
      name = error instanceof DOMException ? error.name : "";
    }
    assert(name === "NotAllowedError", `unfocused: ${name}`);

    gesture(window);
    await navigator.clipboard.writeText("hello clipboard");
    const text = await navigator.clipboard.readText();
    assert(text === "hello clipboard", text);

    const blob = new Blob([PNG], { type: "image/png" });
    const item = new ClipboardItem({
      "text/plain": "png-label",
      "image/png": blob,
    });
    assert(item.types.includes("text/plain"), "item text");
    assert(item.types.includes("image/png"), "item png");
    gesture(window);
    await navigator.clipboard.write([item]);
    const items = await navigator.clipboard.read();
    assert(items.length === 1, "one item");
    const label = await (await items[0].getType("text/plain")).text();
    assert(label === "png-label", label);
    const png = new Uint8Array(
      await (await items[0].getType("image/png")).arrayBuffer(),
    );
    assert(bytesEqual(png, PNG), "png bytes");

    gesture(window);
    Yuzuko.__test.inject(window, [
      { kind: "key", type: "keydown", key: "c", code: "KeyC", metaKey: true },
      { kind: "key", type: "keydown", key: "x", code: "KeyX", metaKey: true },
      { kind: "key", type: "keydown", key: "v", code: "KeyV", metaKey: true },
    ]);
    assert(events.join(",") === "copy,cut,paste", events.join(","));
  } finally {
    window.close();
    other.close();
  }
});
