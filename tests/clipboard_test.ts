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
    assert(ClipboardItem.supports("text/html"), "supports html");

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
    const readItem = items[0];
    if (readItem === undefined) {
      throw new Error("clipboard item");
    }
    const label = await (await readItem.getType("text/plain")).text();
    assert(label === "png-label", label);
    const png = new Uint8Array(
      await (await readItem.getType("image/png")).arrayBuffer(),
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
    // @ts-expect-error The clipboardchange listener receives a ClipboardChangeEvent.
    const change: ClipboardChangeEvent = event;
    seen.push(change);
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
    let changesOnce: number = seen.length;
    assert(changesOnce === 1, `once: ${seen.length}`);
    let writesOnce: number = attribute;
    assert(writesOnce === 1, `attribute: ${attribute}`);
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
    let changesAfterPoll: number = seen.length;
    assert(changesAfterPoll === 1, `after poll ${seen.length}`);

    const blob = new Blob([PNG], { type: "image/png" });
    gesture(window);
    await navigator.clipboard.write([
      new ClipboardItem({
        "image/png": blob,
        "text/plain": "both",
      }),
    ]);
    let changesTwice: number = seen.length;
    assert(changesTwice === 2, `second: ${seen.length}`);
    const both = seen[1];
    if (both === undefined) {
      throw new Error("second change");
    }
    assert(both.types.length === 2, `${both.types.length}`);
    assert(both.types[0] === "text/plain", both.types.join(","));
    assert(both.types[1] === "image/png", both.types.join(","));
    assert(!both.types.includes("text/html"), "html stays off a write");
    assert(typeof both.changeId === "bigint", "second id");
    if (both.changeId !== firstId) {
      assert(both.changeId > firstId, `later ${both.changeId}`);
    }
    let writesTwice: number = attribute;
    assert(writesTwice === 2, `attribute after both: ${attribute}`);

    navigator.clipboard.onclipboardchange = null;
    assert(navigator.clipboard.onclipboardchange === null, "cleared");
    gesture(window);
    await navigator.clipboard.writeText("after clear");
    let changesRemain: number = seen.length;
    assert(changesRemain === 3, "listener remains");
    let writesCleared: number = attribute;
    assert(writesCleared === 2, "attribute stayed cleared");

    navigator.clipboard.removeEventListener("clipboardchange", onChange);
    gesture(window);
    await navigator.clipboard.writeText("removed");
    let changesRemoved: number = seen.length;
    assert(changesRemoved === 3, "removed");

    navigator.clipboard.addEventListener("clipboardchange", onChange);
    gesture(window);
    await navigator.clipboard.writeText("restored");
    let changesRestored: number = seen.length;
    assert(changesRestored === 4, "re-added");

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
      new ClipboardChangeEvent("clipboardchange", {
        // @ts-expect-error types must be an array.
        types: "text/plain",
      });
    } catch (error) {
      name = error instanceof Error ? error.name : "";
    }
    assert(name === "TypeError", `string types: ${name}`);

    let message = "";
    name = "";
    try {
      new ClipboardChangeEvent("clipboardchange", {
        // @ts-expect-error changeId must be a bigint.
        changeId: 4,
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

Yuzuko.test("DataTransfer stores strings and keeps a stable types array", () => {
  assert(DataTransfer.length === 0, "ctor length");
  assert(DataTransfer.prototype.getData.length === 1, "getData length");
  assert(DataTransfer.prototype.setData.length === 2, "setData length");
  assert(DataTransfer.prototype.clearData.length === 0, "clearData length");

  const transfer = new DataTransfer();
  assert(transfer.dropEffect === "none", transfer.dropEffect);
  assert(transfer.effectAllowed === "none", transfer.effectAllowed);
  assert(Object.isFrozen(transfer.types), "frozen");
  assert(transfer.types.length === 0, "empty types");
  const empty = transfer.types;
  assert(transfer.types === empty, "stable empty");
  transfer.clearData();
  assert(transfer.types === empty, "clear of nothing");

  // @ts-expect-error Invalid drop effects are ignored.
  transfer.dropEffect = "COPY";
  // @ts-expect-error Invalid effectAllowed values are ignored.
  transfer.effectAllowed = "Copy";
  assert(transfer.dropEffect === "none", "invalid drop");
  assert(transfer.effectAllowed === "none", "invalid allowed");
  transfer.dropEffect = "move";
  transfer.effectAllowed = "copyLink";
  assert(transfer.dropEffect === "move", transfer.dropEffect);
  assert(transfer.effectAllowed === "copyLink", transfer.effectAllowed);
  transfer.effectAllowed = "uninitialized";
  assert(transfer.effectAllowed === "uninitialized", transfer.effectAllowed);

  transfer.setData("Text/Plain", "plain");
  transfer.setData("text", "alias");
  transfer.setData("TEXT/Ä", "wide");
  assert(transfer.getData("text/plain") === "alias", "text alias");
  assert(transfer.getData("TEXT") === "alias", "TEXT alias");
  assert(transfer.getData("text/Ä") === "wide", "non-ascii case");
  assert(transfer.getData("text/ä") === "", "ascii lowercase only");
  let types = transfer.types;
  assert(types.join(",") === "text/plain,text/Ä", types.join(","));
  transfer.setData("text/html", "<b>h</b>");
  transfer.setData("text/plain", "replaced");
  assert(transfer.types !== types, "replace allocates");
  assert(
    transfer.types.join(",") === "text/Ä,text/html,text/plain",
    transfer.types.join(","),
  );
  types = transfer.types;
  transfer.setData("text/plain", "replaced");
  assert(transfer.types !== types, "same value still allocates");
  assert(transfer.getData("text/plain") === "replaced", "replaced");

  const uri =
    "# comment\r\n\r\nhttps://a.example/1\r\nhttps://b.example/2\n";
  transfer.setData("URL", uri);
  assert(transfer.getData("url") === "https://a.example/1", "first url");
  assert(transfer.getData("text/uri-list") === uri, "raw uri list");
  assert(transfer.getData("text/uri-list;charset=utf-8") === "", "parameter");
  assert(transfer.getData(" url ") === "", "unstripped url");
  transfer.setData("text/uri-list", "# only\n\n");
  assert(transfer.getData("url") === "", "no url");
  transfer.clearData("url");
  assert(transfer.getData("text/uri-list") === "", "cleared url");
  assert(!transfer.types.includes("text/uri-list"), "url type gone");

  const kept = transfer.types;
  transfer.clearData("text/uri-list");
  assert(transfer.types === kept, "missing clearData");
  // @ts-expect-error null is not a format.
  transfer.clearData(null);
  assert(transfer.getData("text/plain") === "replaced", "null format");
  transfer.clearData();
  assert(transfer.types.length === 0, "cleared");
  assert(transfer.getData("text/plain") === "", "cleared plain");

  let name = "";
  try {
    // @ts-expect-error format is required.
    transfer.getData();
  } catch (error) {
    name = error instanceof TypeError ? error.message : "";
  }
  assert(name === "Not enough arguments", name);
});

Yuzuko.test("ClipboardEvent stores a DataTransfer", () => {
  assert(new ClipboardEvent("copy").clipboardData === null, "omitted");
  assert(
    new ClipboardEvent("copy", { clipboardData: null }).clipboardData === null,
    "null",
  );
  assert(
    new ClipboardEvent(
      "copy",
      // @ts-expect-error A null init is ignored.
      null,
    ).clipboardData === null,
    "non-object",
  );
  const transfer = new DataTransfer();
  transfer.setData("text/plain", "stored");
  const event = new ClipboardEvent("paste", { clipboardData: transfer });
  assert(event.clipboardData === transfer, "same object");
  assert(event.clipboardData?.getData("text") === "stored", "round trip");

  let name = "";
  let message = "";
  try {
    new ClipboardEvent("copy", {
      // @ts-expect-error clipboardData must be a DataTransfer.
      clipboardData: {},
    });
  } catch (error) {
    name = error instanceof TypeError ? error.name : "";
    message = error instanceof Error ? error.message : "";
  }
  assert(name === "TypeError", name);
  assert(message === "clipboardData is not a DataTransfer", message);

  message = "";
  try {
    const init: ClipboardEventInit = {};
    Object.defineProperty(init, "clipboardData", {
      get() {
        throw new TypeError("boom");
      },
    });
    new ClipboardEvent("copy", init);
  } catch (error) {
    message = error instanceof Error ? error.message : "";
  }
  assert(message === "boom", message);
});

Yuzuko.test("host copy, cut, and paste use clipboardData", async () => {
  const window = new Yuzuko.Window({
    width: 120,
    height: 80,
    title: "transfer",
  });
  const changes: string[][] = [];
  const onChange = (event: Event) => {
    // @ts-expect-error The clipboardchange listener receives a ClipboardChangeEvent.
    const change: ClipboardChangeEvent = event;
    changes.push([...change.types]);
  };
  navigator.clipboard.addEventListener("clipboardchange", onChange);
  try {
    gesture(window);
    await navigator.clipboard.writeText("kept");
    const afterWrite = changes.length;

    let started = "";
    let startedTypes = "";
    const onCopy = (event: Event) => {
      // @ts-expect-error The copy listener receives a ClipboardEvent.
      const copyEvent: ClipboardEvent = event;
      const data = copyEvent.clipboardData;
      assert(data instanceof DataTransfer, "copy data");
      started = data?.getData("text/plain") ?? "missing";
      startedTypes = data?.types.join(",") ?? "missing";
      data?.setData("text/plain", "from copy");
      data?.setData("text/html", "<b>from copy</b>");
      event.preventDefault();
    };
    window.addEventListener("copy", onCopy);
    Yuzuko.__test.inject(window, [
      { kind: "key", type: "keydown", key: "c", code: "KeyC", metaKey: true },
    ]);
    window.removeEventListener("copy", onCopy);
    assert(started === "", started);
    assert(startedTypes === "", startedTypes);
    gesture(window);
    assert((await navigator.clipboard.readText()) === "from copy", "published");
    assert(changes.length === afterWrite + 1, "change fired");
    const lastChange = changes[changes.length - 1];
    if (lastChange === undefined) {
      throw new Error("change fired");
    }
    assert(lastChange.join(",") === "text/plain", "text only");

    const onPlain = (event: Event) => {
      // @ts-expect-error The copy listener receives a ClipboardEvent.
      const copyEvent: ClipboardEvent = event;
      const data = copyEvent.clipboardData;
      data?.setData("text/plain", "ignored");
    };
    window.addEventListener("copy", onPlain);
    Yuzuko.__test.inject(window, [
      { kind: "key", type: "keydown", key: "c", code: "KeyC", metaKey: true },
    ]);
    window.removeEventListener("copy", onPlain);
    gesture(window);
    assert((await navigator.clipboard.readText()) === "from copy", "uncancelled");
    assert(changes.length === afterWrite + 1, "no change without cancel");

    const onHtml = (event: Event) => {
      // @ts-expect-error The copy listener receives a ClipboardEvent.
      const copyEvent: ClipboardEvent = event;
      copyEvent.clipboardData?.setData("text/html", "<i>x</i>");
      event.preventDefault();
    };
    window.addEventListener("copy", onHtml);
    Yuzuko.__test.inject(window, [
      { kind: "key", type: "keydown", key: "c", code: "KeyC", metaKey: true },
    ]);
    window.removeEventListener("copy", onHtml);
    gesture(window);
    assert((await navigator.clipboard.readText()) === "from copy", "html stays");

    const onCut = (event: Event) => {
      // @ts-expect-error The cut listener receives a ClipboardEvent.
      const cutEvent: ClipboardEvent = event;
      cutEvent.clipboardData?.setData("text/plain", "");
      event.preventDefault();
    };
    window.addEventListener("cut", onCut);
    Yuzuko.__test.inject(window, [
      { kind: "key", type: "keydown", key: "x", code: "KeyX", metaKey: true },
    ]);
    window.removeEventListener("cut", onCut);
    gesture(window);
    assert((await navigator.clipboard.readText()) === "", "empty cut");

    gesture(window);
    await navigator.clipboard.writeText("paste-me");
    let pasted = "unset";
    let pasteTypes = "unset";
    let mutated = "unset";
    let allowed = "unset";
    let dropped = "unset";
    const onPaste = (event: Event) => {
      // @ts-expect-error The paste listener receives a ClipboardEvent.
      const pasteEvent: ClipboardEvent = event;
      const data = pasteEvent.clipboardData;
      assert(data instanceof DataTransfer, "paste data");
      pasted = data?.getData("text") ?? "missing";
      pasteTypes = data?.types.join(",") ?? "missing";
      data?.setData("text/plain", "mutated");
      data?.clearData();
      if (data) {
        data.effectAllowed = "copy";
        data.dropEffect = "copy";
        allowed = data.effectAllowed;
        dropped = data.dropEffect;
      }
      mutated = data?.getData("text/plain") ?? "missing";
    };
    window.addEventListener("paste", onPaste);
    Yuzuko.__test.inject(window, [
      { kind: "key", type: "keydown", key: "v", code: "KeyV", metaKey: true },
    ]);
    window.removeEventListener("paste", onPaste);
    assert(pasted === "paste-me", pasted);
    assert(pasteTypes === "text/plain", pasteTypes);
    assert(mutated === "paste-me", mutated);
    assert(allowed === "none", allowed);
    assert(dropped === "copy", dropped);
    gesture(window);
    assert((await navigator.clipboard.readText()) === "paste-me", "paste read");

    const synthetic = new DataTransfer();
    synthetic.setData("text/plain", "synthetic");
    const syntheticEvent = new ClipboardEvent("copy", {
      bubbles: true,
      cancelable: true,
      clipboardData: synthetic,
    });
    const onSynthetic = (event: Event) => event.preventDefault();
    window.addEventListener("copy", onSynthetic);
    window.dispatchEvent(syntheticEvent);
    window.removeEventListener("copy", onSynthetic);
    gesture(window);
    assert(
      (await navigator.clipboard.readText()) === "paste-me",
      "synthetic copy",
    );

    const blob = new Blob([PNG], { type: "image/png" });
    gesture(window);
    await navigator.clipboard.write([
      new ClipboardItem({ "image/png": blob }),
    ]);
    let pngTypes = "unset";
    const onPngPaste = (event: Event) => {
      // @ts-expect-error The paste listener receives a ClipboardEvent.
      const pasteEvent: ClipboardEvent = event;
      pngTypes = pasteEvent.clipboardData?.types.join(",") ?? "missing";
    };
    window.addEventListener("paste", onPngPaste);
    Yuzuko.__test.inject(window, [
      { kind: "key", type: "keydown", key: "v", code: "KeyV", metaKey: true },
    ]);
    window.removeEventListener("paste", onPngPaste);
    assert(pngTypes === "", pngTypes);
  } finally {
    navigator.clipboard.removeEventListener("clipboardchange", onChange);
    window.close();
  }
});

Yuzuko.test("ClipboardItem keeps custom types", async () => {
  const plain = new Blob(["hello"], { type: "text/plain" });
  const custom = new Blob(["this should work"], { type: "application/abc" });
  const web = new Blob(["web custom format should work"], {
    type: "web application/abc",
  });
  new ClipboardItem({ "text/plain": plain });
  new ClipboardItem({ "application/abc": custom });
  new ClipboardItem({ "web text/plain": plain });
  new ClipboardItem({ "web application/abc": web });
  const item = new ClipboardItem({
    "text/plain": plain,
    "application/abc": custom,
    "web application/abc": web,
  });
  assert(
    item.types.join(",") === "text/plain,application/abc,web application/abc",
    item.types.join(","),
  );
  assert((await (await item.getType("text/plain")).text()) === "hello", "plain");
  const webOut = await item.getType("web application/abc");
  assert(webOut.type.includes("web application/abc"), webOut.type);
  assert(
    (await webOut.text()) === "web custom format should work",
    "web bytes",
  );

  const strings = new ClipboardItem({
    "text/plain": "abc",
    "application/abc": "xxx",
    "web application/abc": "yyy",
  });
  const stringBlob = await strings.getType("web application/abc");
  assert(stringBlob.type === "web application/abc", stringBlob.type);
  assert((await stringBlob.text()) === "yyy", "string web");
  const plainString = await strings.getType("text/plain");
  assert(plainString.type === "text/plain", plainString.type);
  assert((await plainString.text()) === "abc", "string plain");
  const unsupported = await strings.getType("application/abc");
  assert(unsupported.type === "application/abc", unsupported.type);
  assert((await unsupported.text()) === "xxx", "string custom");

  const parameterized = new ClipboardItem({ "text/plain;foo=1": custom });
  const parameterBlob = await parameterized.getType("text/plain;foo=1");
  assert(parameterBlob.type.includes("application/abc"), parameterBlob.type);
  const webParameter = new ClipboardItem({ "web text/plain;foo=1": custom });
  const webParameterBlob = await webParameter.getType("web text/plain;foo=1");
  assert(webParameterBlob.type.includes("application/abc"), webParameterBlob.type);

  let invalid = "";
  try {
    new ClipboardItem({ "not a/real type": custom });
  } catch (error) {
    invalid = error instanceof TypeError ? "TypeError" : "";
  }
  assert(invalid === "TypeError", invalid);
  invalid = "";
  try {
    new ClipboardItem({ "web notarealtype": web });
  } catch (error) {
    invalid = error instanceof TypeError ? "TypeError" : "";
  }
  assert(invalid === "TypeError", "bad web");

  assert(ClipboardItem.supports("text/uri-list"), "uri");
  assert(ClipboardItem.supports("image/svg+xml"), "svg");
  assert(ClipboardItem.supports("web foo/bar"), "web");
  assert(ClipboardItem.supports("web text/html"), "web html");
  assert(!ClipboardItem.supports("text/plain;foo=1"), "parameters");
  assert(!ClipboardItem.supports("web foo/bar;foo=1"), "web parameters");
  assert(!ClipboardItem.supports("foo/bar"), "unknown");
  assert(!ClipboardItem.supports("web "), "empty web");
  assert(!ClipboardItem.supports("weB text/html"), "case");

  const window = new Yuzuko.Window({ width: 80, height: 60, title: "item" });
  try {
    gesture(window);
    let name = "";
    try {
      await navigator.clipboard.write([
        new ClipboardItem({ "application/abc": custom }),
      ]);
    } catch (error) {
      name = error instanceof DOMException ? error.name : "";
    }
    assert(name === "NotSupportedError", `custom write: ${name}`);

    gesture(window);
    await navigator.clipboard.write([
      new ClipboardItem({
        "text/plain": "kept",
        "application/abc": "ignored",
      }),
    ]);
    assert((await navigator.clipboard.readText()) === "kept", "mixed write");
  } finally {
    window.close();
  }
});

Yuzuko.test("a dedicated worker has no DataTransfer", async () => {
  const worker = new Worker(
    new URL("./workers/clipboard.js", import.meta.url).href,
  );
  const probe = await new Promise<{
    clipboard: boolean;
    Clipboard: boolean;
    DataTransfer: boolean;
    ClipboardEvent: boolean;
  }>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => {
      const value: {
        clipboard: boolean;
        Clipboard: boolean;
        DataTransfer: boolean;
        ClipboardEvent: boolean;
      } = event.data;
      resolve(value);
    };
    worker.onerror = (event: ErrorEvent) => {
      reject(new Error(event.message || "worker error"));
    };
  });
  worker.terminate();
  assert(probe.clipboard === false, "worker clipboard");
  assert(probe.Clipboard === false, "worker Clipboard");
  assert(probe.DataTransfer === false, "worker DataTransfer");
  assert(probe.ClipboardEvent === false, "worker ClipboardEvent");
});
