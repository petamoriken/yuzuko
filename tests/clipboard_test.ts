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

Yuzuko.test("clipboardchange reports a focused write", async () => {
  const window = new Yuzuko.Window({
    width: 120,
    height: 80,
    title: "change",
  });
  const seen: ClipboardChangeEvent[] = [];
  let attribute = 0;
  let attributeThis: Clipboard | undefined;
  const onChange = (event: Event) => {
    seen.push(event as ClipboardChangeEvent);
  };
  navigator.clipboard.addEventListener("clipboardchange", onChange);
  navigator.clipboard.onclipboardchange = function (this: Clipboard) {
    attribute += 1;
    attributeThis = this;
  };
  try {
    assert(
      navigator.clipboard.onclipboardchange instanceof Function,
      "handler",
    );
    assert(typeof ClipboardChangeEvent === "function", "constructor");

    Yuzuko.__test.inject(window, [{ kind: "focus" }]);
    seen.length = 0;
    attribute = 0;

    let name = "";
    try {
      await navigator.clipboard.writeText("without a gesture");
    } catch (error) {
      name = error instanceof DOMException ? error.name : "";
    }
    assert(name === "NotAllowedError", `no gesture: ${name}`);
    assert(seen.length === 0, "a rejected write does not fire");

    Yuzuko.__test.inject(window, [{ kind: "blur" }, { kind: "focus" }]);
    assert(seen.length === 0, "focus and blur do not fire");

    gesture(window);
    await navigator.clipboard.writeText("changed");
    assert(seen.length === 1, `once: ${seen.length}`);
    assert(attribute === 1, `attribute: ${attribute}`);
    assert(attributeThis === navigator.clipboard, "handler this");
    const event = seen[0];
    assert(event instanceof ClipboardChangeEvent, "instance");
    assert(event instanceof Event, "event");
    assert(event.type === "clipboardchange", event.type);
    assert(event.isTrusted, "trusted");
    assert(event.target === navigator.clipboard, "target");
    assert(!event.bubbles, "bubbles");
    assert(!event.cancelable, "cancelable");
    assert(Object.isFrozen(event.types), "frozen");
    assert(event.types.join(",") === "text/plain", event.types.join(","));
    assert(event.types !== event.types, "new array");
    assert(Object.isFrozen(event.types), "frozen again");
    assert(typeof event.changeId === "bigint", typeof event.changeId);
    assert(event.changeId > 0n, `changeId ${event.changeId}`);
    const firstId = event.changeId;

    await new Promise((resolve) => {
      setTimeout(resolve, 0);
    });
    assert(seen.length === 1, `after poll ${seen.length}`);

    const blob = new Blob([PNG], { type: "image/png" });
    gesture(window);
    await navigator.clipboard.write([
      new ClipboardItem({
        "image/png": blob,
        "text/plain": "both",
      }),
    ]);
    assert(seen.length === 2, `second: ${seen.length}`);
    const both = seen[1];
    assert(both.types.length === 2, `${both.types.length}`);
    assert(both.types[0] === "text/plain", both.types.join(","));
    assert(both.types[1] === "image/png", both.types.join(","));
    assert(!both.types.includes("text/html"), "html stays off a write");
    assert(typeof both.changeId === "bigint", "second id");
    if (both.changeId !== firstId) {
      assert(both.changeId > firstId, `later ${both.changeId}`);
    }
    assert(attribute === 2, `attribute after both: ${attribute}`);

    navigator.clipboard.onclipboardchange = null;
    assert(navigator.clipboard.onclipboardchange === null, "cleared");
    gesture(window);
    await navigator.clipboard.writeText("after clear");
    assert(seen.length === 3, "listener remains");
    assert(attribute === 2, "attribute stayed cleared");

    navigator.clipboard.removeEventListener("clipboardchange", onChange);
    gesture(window);
    await navigator.clipboard.writeText("removed");
    assert(seen.length === 3, "removed");

    navigator.clipboard.addEventListener("clipboardchange", onChange);
    gesture(window);
    await navigator.clipboard.writeText("restored");
    assert(seen.length === 4, "re-added");

    const empty = new ClipboardChangeEvent("clipboardchange");
    assert(empty.types.length === 0, "default types");
    assert(Object.isFrozen(empty.types), "default frozen");
    assert(empty.types !== empty.types, "default new array");
    assert(empty.changeId === 0n, `default id ${empty.changeId}`);
    assert(!empty.isTrusted, "constructed events are not trusted");
    assert(!empty.bubbles, "constructed bubbles");
    assert(!empty.cancelable, "constructed cancelable");

    const custom = new ClipboardChangeEvent("clipboardchange", {
      types: ["custom/type", "text/plain"],
      changeId: 4n,
    });
    assert(
      custom.types.join(",") === "custom/type,text/plain",
      custom.types.join(","),
    );
    assert(custom.changeId === 4n, `4n ${custom.changeId}`);
    assert(!custom.isTrusted, "custom trusted");

    const wide = new ClipboardChangeEvent("clipboardchange", {
      changeId: 2n ** 127n,
    });
    assert(wide.changeId === 2n ** 127n, "127-bit id");
    const negative = new ClipboardChangeEvent("clipboardchange", {
      changeId: -3n,
    });
    assert(negative.changeId === -3n, `negative ${negative.changeId}`);
    const boxed = new ClipboardChangeEvent("clipboardchange", {
      changeId: Object(7n),
    });
    assert(boxed.changeId === 7n, `boxed ${boxed.changeId}`);

    name = "";
    try {
      new ClipboardChangeEvent("clipboardchange", { types: "text/plain" });
    } catch (error) {
      name = error instanceof Error ? error.name : "";
    }
    assert(name === "TypeError", `string types: ${name}`);

    let message = "";
    name = "";
    try {
      new ClipboardChangeEvent("clipboardchange", {
        changeId: 4 as unknown as bigint,
      });
    } catch (error) {
      name = error instanceof Error ? error.name : "";
      message = error instanceof Error ? error.message : "";
    }
    assert(name === "TypeError", `number id: ${name} ${message}`);

    name = "";
    message = "";
    try {
      new ClipboardChangeEvent("clipboardchange", { changeId: 2n ** 128n });
    } catch (error) {
      name = error instanceof Error ? error.name : "";
      message = error instanceof Error ? error.message : "";
    }
    assert(name === "TypeError", `wide id: ${name}`);
    assert(
      message === "ClipboardChangeEvent changeId is out of range.",
      message,
    );
  } finally {
    navigator.clipboard.onclipboardchange = null;
    navigator.clipboard.removeEventListener("clipboardchange", onChange);
    window.close();
  }
});
