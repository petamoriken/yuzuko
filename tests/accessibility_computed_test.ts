function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function assertThrows(
  run: () => unknown,
  kind: "type" | "dom",
  name: string,
  message: string,
): void {
  let thrown: unknown;
  try {
    run();
  } catch (error) {
    thrown = error;
  }
  if (thrown === undefined) {
    throw new Error(`expected throw: ${message}`);
  }
  if (kind === "dom") {
    assert(thrown instanceof DOMException, `not DOMException: ${message}`);
    const exception = thrown as DOMException;
    assert(exception.name === name, `${exception.name} !== ${name}`);
  } else {
    assert(thrown instanceof TypeError, `not TypeError: ${message}`);
  }
  const error = thrown as Error;
  assert(error.message === message, error.message);
}

async function assertRejects(
  pending: Promise<unknown>,
  kind: "type" | "dom",
  name: string,
  message: string,
): Promise<void> {
  try {
    await pending;
  } catch (error) {
    if (kind === "dom") {
      assert(error instanceof DOMException, `not DOMException: ${message}`);
      const exception = error as DOMException;
      assert(exception.name === name, `${exception.name} !== ${name}`);
    } else {
      assert(error instanceof TypeError, `not TypeError: ${message}`);
    }
    const reported = error as Error;
    assert(reported.message === message, reported.message);
    return;
  }
  throw new Error(`expected rejection: ${message}`);
}

Yuzuko.test("the computed tree is the published snapshot", async () => {
  assert(globalThis.ComputedAccessibleNode === undefined, "global constructor");
  assert(typeof Yuzuko.ComputedAccessibleNode === "function", "constructor");
  assert(
    Yuzuko.ComputedAccessibleNode.name === "ComputedAccessibleNode",
    "name",
  );
  assertThrows(
    () => new Yuzuko.ComputedAccessibleNode(),
    "type",
    "",
    "Illegal constructor",
  );
  let bare: unknown;
  try {
    bare = Function("return ComputedAccessibleNode")();
  } catch (error) {
    bare = error;
  }
  assert(bare instanceof ReferenceError, "bare name");
  assert(
    !(Yuzuko.ComputedAccessibleNode.prototype instanceof EventTarget),
    "prototype is not an EventTarget",
  );

  const method = Yuzuko.Window.prototype.getComputedAccessibleNode;
  assert(method.length === 1, "length");
  assert(method.name === "getComputedAccessibleNode", "method name");
  const sample = new Yuzuko.AccessibleNode();
  assertThrows(() => method.call({}), "type", "", "illegal invocation");
  assertThrows(
    () => method.call({}, sample),
    "type",
    "",
    "illegal invocation",
  );
  await assertRejects(
    Yuzuko.ComputedAccessibleNode.prototype.ensureUpToDate.call({}),
    "type",
    "",
    "illegal invocation",
  );

  const window = new Yuzuko.Window({
    width: 200,
    height: 100,
    title: "Palette",
  });
  const other = new Yuzuko.Window({ width: 40, height: 20, title: "Other" });
  const closed = new Yuzuko.Window({ width: 30, height: 20, title: "Closed" });
  try {
    closed.close();
    assertThrows(
      () => closed.getComputedAccessibleNode(),
      "type",
      "",
      "Not enough arguments",
    );
    assertThrows(
      () => closed.getComputedAccessibleNode(1 as unknown as Yuzuko.AccessibleNode),
      "type",
      "",
      "The node must be an AccessibleNode.",
    );
    assertThrows(
      () => closed.getComputedAccessibleNode(sample),
      "dom",
      "InvalidStateError",
      "The window is closed.",
    );
    assertThrows(
      () => window.getComputedAccessibleNode(),
      "type",
      "",
      "Not enough arguments",
    );
    assertThrows(
      () => window.getComputedAccessibleNode({} as Yuzuko.AccessibleNode),
      "type",
      "",
      "The node must be an AccessibleNode.",
    );

    const otherRoot = other.surface.attachAccessibleRoot();
    assert(
      (await window.getComputedAccessibleNode(sample)) === null,
      "detached",
    );
    assert(
      (await window.getComputedAccessibleNode(otherRoot)) === null,
      "other window",
    );

    const root = window.surface.attachAccessibleRoot();
    root.role = "button";
    let seen = false;
    const pending = window.getComputedAccessibleNode(root);
    assert(pending instanceof Promise, "promise");
    pending.then(() => {
      seen = true;
    });
    root.role = "link";
    assert(seen === false, "snapshot waits for a task");
    const computed = await pending;
    assert(seen === true, "task settled");
    assert(computed !== null, "author root");
    assert(computed instanceof Yuzuko.ComputedAccessibleNode, "instanceof");
    assert(!(computed instanceof EventTarget), "not an EventTarget");
    assert(!(computed instanceof Yuzuko.AccessibleNode), "not authored");
    assert(computed.role === "link", "write before the task is included");
    assert(computed.ensureUpToDate.length === 0, "ensureUpToDate length");
    const again = await window.getComputedAccessibleNode(root, "extra");
    assert(again === computed, "identity");

    const chrome = computed.parent;
    assert(chrome !== null, "chrome parent");
    assert(chrome instanceof Yuzuko.ComputedAccessibleNode, "chrome type");
    assert(!(chrome instanceof Yuzuko.AccessibleNode), "chrome is synthetic");
    assert(chrome.role === "window", "chrome role");
    assert(chrome.name === "Palette", "chrome name");
    assert(chrome.parent === null, "chrome parent");
    assert(chrome.offsetParent === null, "chrome offset parent");
    assert(chrome.firstChild === computed, "chrome first child");
    assert(chrome.lastChild === computed, "chrome last child");
    assert(chrome.previousSibling === null, "chrome previous");
    assert(chrome.nextSibling === null, "chrome next");
    assert(chrome.offsetLeft === "0px", "chrome left");
    assert(chrome.offsetTop === "0px", "chrome top");
    assert(window.clientWidth === 200, `clientWidth ${window.clientWidth}`);
    assert(window.clientHeight === 100, `clientHeight ${window.clientHeight}`);
    assert(
      chrome.offsetWidth === `${window.clientWidth}px`,
      chrome.offsetWidth ?? "missing width",
    );
    assert(
      chrome.offsetHeight === `${window.clientHeight}px`,
      chrome.offsetHeight ?? "missing height",
    );
    assert(chrome.focusable === false, "chrome focusable");
    assert(!("children" in computed), "no children list");
    assert(!("atomic" in computed), "no atomic");
    assert(!("autocomplete" in computed), "no autocomplete");
    assert(!("keyShortcuts" in computed), "no keyShortcuts");
    assert(!("placeholder" in computed), "no placeholder");
    assert(!("label" in computed), "no label");
    assert(computed.firstChild === null, "no children yet");

    root.role = "button";
    assert(computed.role === "link", "stale role");
    assert((await computed.ensureUpToDate()) === undefined, "ensure resolves");
    assert(computed.role === "button", "refreshed role");

    window.surface.title = "";
    await computed.ensureUpToDate();
    assert(chrome.name === "", "empty title");
    window.surface.title = "Palette";

    const directory = new Yuzuko.AccessibleNode();
    const image = new Yuzuko.AccessibleNode();
    const blank = new Yuzuko.AccessibleNode();
    const none = new Yuzuko.AccessibleNode();
    const presentation = new Yuzuko.AccessibleNode();
    const generic = new Yuzuko.AccessibleNode();
    const text = new Yuzuko.AccessibleNode();
    const plain = new Yuzuko.AccessibleNode();
    const cleared = new Yuzuko.AccessibleNode();
    const box = new Yuzuko.AccessibleNode();
    const button = new Yuzuko.AccessibleNode();
    const row = new Yuzuko.AccessibleNode();
    const rowZero = new Yuzuko.AccessibleNode();
    const rowNeg = new Yuzuko.AccessibleNode();
    const counted = new Yuzuko.AccessibleNode();
    const flags = new Yuzuko.AccessibleNode();
    const placed = new Yuzuko.AccessibleNode();
    directory.role = "directory";
    image.role = "img";
    blank.role = "";
    none.role = "none";
    presentation.role = "presentation";
    generic.role = "generic";
    root.role = "hammer";
    root.name = "";
    root.description = "desc";
    root.roleDescription = "role";
    root.valueText = "";
    text.role = "textbox";
    text.multiline = true;
    plain.role = "textbox";
    plain.multiline = false;
    cleared.role = "textbox";
    cleared.multiline = null;
    box.role = "checkbox";
    box.checked = "mixed";
    box.pressed = "true";
    button.role = "button";
    button.pressed = "true";
    button.checked = "mixed";
    row.role = "row";
    row.rowIndex = 1;
    row.colIndex = 2;
    row.level = 3;
    row.posInSet = 4;
    row.rowCount = -3;
    rowZero.role = "row";
    rowZero.rowIndex = 0;
    rowZero.colCount = 0;
    rowNeg.role = "row";
    rowNeg.rowIndex = -1;
    rowNeg.colCount = -1;
    counted.role = "table";
    counted.colSpan = 0;
    counted.rowSpan = 2;
    counted.setSize = 5;
    flags.role = "group";
    flags.hidden = false;
    flags.disabled = false;
    flags.expanded = false;
    flags.selected = false;
    flags.busy = true;
    flags.modal = true;
    flags.required = true;
    flags.readOnly = true;
    flags.multiselectable = true;
    flags.focusable = true;
    flags.live = "polite";
    flags.orientation = "horizontal";
    flags.valueNow = 1.5;
    flags.valueMin = 0;
    flags.valueMax = 4;
    flags.checked = "false";
    placed.role = "group";
    placed.offsetLeft = 12;
    placed.hidden = true;
    placed.multiline = true;
    for (const node of [
      directory,
      image,
      blank,
      none,
      presentation,
      generic,
      text,
      plain,
      cleared,
      box,
      button,
      row,
      rowZero,
      rowNeg,
      counted,
      flags,
      placed,
    ]) {
      root.appendChild(node);
    }
    placed.offsetParent = root;
    assertThrows(
      () => {
        placed.offsetParent = placed;
      },
      "dom",
      "HierarchyRequestError",
      "The offset parent is not an ancestor.",
    );
    assert(placed.offsetParent === root, "rejected offset parent is ignored");

    await computed.ensureUpToDate();
    assert(computed.role === "unknown", "unknown role");
    assert(computed.name === "", "empty name");
    assert(computed.description === "desc", "description");
    assert(computed.roleDescription === "role", "role description");
    assert(computed.valueText === "", "empty value text");
    const first = computed.firstChild;
    const last = computed.lastChild;
    assert(first !== null && last !== null, "children");
    assert(first.parent === computed, "child parent");
    assert(last.parent === computed, "last parent");
    assert(first.previousSibling === null, "first previous");
    assert(last.nextSibling === null, "last next");
    assert(first.nextSibling !== null, "first next");
    assert(last.previousSibling !== null, "last previous");
    const directoryNode = await window.getComputedAccessibleNode(directory);
    assert(directoryNode === first, "first identity");
    assert(directoryNode.role === "list", "directory");
    assert(
      (await window.getComputedAccessibleNode(image))?.role === "image",
      "img",
    );
    assert((await window.getComputedAccessibleNode(blank))?.role === "generic", "empty role");
    assert((await window.getComputedAccessibleNode(none))?.role === "generic", "none");
    assert(
      (await window.getComputedAccessibleNode(presentation))?.role === "generic",
      "presentation",
    );
    assert(
      (await window.getComputedAccessibleNode(generic))?.role === "generic",
      "generic",
    );
    const textNode = await window.getComputedAccessibleNode(text);
    assert(textNode?.role === "textbox" && textNode.multiline === true, "multiline");
    assert((await window.getComputedAccessibleNode(plain))?.multiline === null, "multiline false");
    assert(
      (await window.getComputedAccessibleNode(cleared))?.multiline === null,
      "multiline cleared",
    );
    const boxNode = await window.getComputedAccessibleNode(box);
    assert(boxNode?.checked === "mixed" && boxNode.pressed === null, "checkbox");
    const buttonNode = await window.getComputedAccessibleNode(button);
    assert(
      buttonNode?.pressed === "true" && buttonNode.checked === null,
      "button",
    );
    const rowNode = await window.getComputedAccessibleNode(row);
    assert(rowNode?.rowIndex === 0, "rowIndex 1");
    assert(rowNode?.colIndex === 1, "colIndex 2");
    assert(rowNode?.level === 2, "level 3");
    assert(rowNode?.posInSet === 3, "posInSet 4");
    assert(rowNode?.rowCount === null, "negative rowCount");
    const zeroNode = await window.getComputedAccessibleNode(rowZero);
    assert(zeroNode?.rowIndex === 0 && zeroNode.colCount === 0, "zero index and count");
    const negNode = await window.getComputedAccessibleNode(rowNeg);
    assert(negNode?.rowIndex === null && negNode.colCount === null, "negative index");
    const countNode = await window.getComputedAccessibleNode(counted);
    assert(countNode?.colSpan === 0, "colSpan zero");
    assert(countNode?.rowSpan === 2, "rowSpan");
    assert(countNode?.setSize === 5, "setSize");
    const flagNode = await window.getComputedAccessibleNode(flags);
    assert(flagNode?.hidden === null, "hidden false");
    assert(flagNode?.disabled === null, "disabled false");
    assert(flagNode?.expanded === false, "expanded false");
    assert(flagNode?.selected === false, "selected false");
    assert(flagNode?.busy === true, "busy");
    assert(flagNode?.modal === true, "modal");
    assert(flagNode?.required === true, "required");
    assert(flagNode?.readOnly === true, "readOnly");
    assert(flagNode?.multiselectable === true, "multiselectable");
    assert(flagNode?.focusable === true, "focusable");
    assert(flagNode?.live === "polite", "live");
    assert(flagNode?.orientation === "horizontal", "orientation");
    assert(flagNode?.valueNow === 1.5, "valueNow");
    assert(flagNode?.valueMin === 0, "valueMin");
    assert(flagNode?.valueMax === 4, "valueMax");
    assert(flagNode?.checked === "false" && flagNode.pressed === null, "group checked");
    assert(directoryNode.focusable === false, "focusable default");
    const placedNode = await window.getComputedAccessibleNode(placed);
    assert(placedNode === last, "last identity");
    assert(placedNode.hidden === true, "hidden stays in the tree");
    assert(placedNode.multiline === null, "multiline ignored");
    assert(placedNode.offsetLeft === "12px", "offsetLeft");
    assert(placedNode.offsetTop === "0px", "missing offsetTop");
    assert(placedNode.offsetWidth === "0px", "missing offsetWidth");
    assert(placedNode.offsetHeight === "0px", "missing offsetHeight");
    assert(placedNode.offsetParent === computed, "offset parent");

    root.removeChild(directory);
    assert(directoryNode.role === "list", "removed node stays stale");
    await directoryNode.ensureUpToDate();
    assert(directoryNode.role === null, "removed role");
    assert(directoryNode.name === null, "removed name");
    assert(directoryNode.checked === null, "removed checked");
    assert(directoryNode.expanded === null, "removed expanded");
    assert(directoryNode.focusable === false, "removed focusable");
    assert(directoryNode.parent === null, "removed parent");
    assert(directoryNode.nextSibling === null, "removed sibling");
    assert(directoryNode.offsetLeft === null, "removed offset");
    assert(
      (await window.getComputedAccessibleNode(directory)) === null,
      "removed resolves null",
    );
    assert(computed.firstChild?.role === "image", "next child");

    const role = computed.role;
    const closing = window.getComputedAccessibleNode(root);
    window.close();
    await assertRejects(
      closing,
      "dom",
      "InvalidStateError",
      "The window is closed.",
    );
    assert(computed.role === role, "close keeps the snapshot");
    await assertRejects(
      computed.ensureUpToDate(),
      "dom",
      "InvalidStateError",
      "The window is closed.",
    );
    assert(computed.role === role, "rejected refresh keeps the snapshot");
    assertThrows(
      () => window.getComputedAccessibleNode(),
      "type",
      "",
      "Not enough arguments",
    );
    assertThrows(
      () => window.getComputedAccessibleNode(root),
      "dom",
      "InvalidStateError",
      "The window is closed.",
    );
  } finally {
    window.close();
    other.close();
    closed.close();
  }
});
