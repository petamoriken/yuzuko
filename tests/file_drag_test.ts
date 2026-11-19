function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function read<T>(value: T): T {
  return value;
}

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

function throwsTypeError(run: () => void): boolean {
  try {
    run();
  } catch (error) {
    return error instanceof TypeError;
  }
  return false;
}

function separator(): string {
  return Yuzuko.build.os === "windows" ? "\\" : "/";
}

function join(dir: string, name: string): string {
  return `${dir}${separator()}${name}`;
}

function baseName(path: string): string {
  const parts = path.split(separator());
  return parts[parts.length - 1] ?? "";
}

function openWindow(title: string): Yuzuko.Window {
  return new Yuzuko.Window({ width: 80, height: 60, title });
}

type DragName = "dragenter" | "dragover" | "dragleave" | "drop";

type DragRecord = {
  type: string;
  where: "surface" | "window";
  event: DragEvent;
  phase: number;
};

function watchDrag(window: Yuzuko.Window, names: readonly DragName[]): DragRecord[] {
  const seen: DragRecord[] = [];
  const surface = window.surface;
  const take = (where: "surface" | "window") => (event: DragEvent) => {
    seen.push({
      type: event.type,
      where,
      event,
      phase: event.eventPhase,
    });
  };
  for (const name of names) {
    surface.addEventListener(name, take("surface"));
    window.addEventListener(name, take("window"));
  }
  return seen;
}

function expectHostDrag(event: DragEvent, surface: Yuzuko.WindowSurface, cancelable: boolean) {
  assert(event.target === surface, `${event.type} target`);
  assert(event.bubbles === true, `${event.type} bubbles`);
  assert(event.cancelable === cancelable, `${event.type} cancelable`);
  assert(event.composed === false, `${event.type} composed`);
  assert(event.isTrusted === true, `${event.type} trusted`);
  assert(event.clientX === 0 && event.clientY === 0, `${event.type} client`);
  assert(event.screenX === 0 && event.screenY === 0, `${event.type} screen`);
  assert(event.button === 0, `${event.type} button`);
  assert(event.buttons === 0, `${event.type} buttons`);
  assert(event.which === 1, `${event.type} which`);
  assert(event.view === null, `${event.type} view`);
  assert(event.relatedTarget === null, `${event.type} related`);
  assert(event.detail === 0, `${event.type} detail`);
  assert(event.ctrlKey === false && event.altKey === false, `${event.type} modifiers`);
  assert(event.shiftKey === false && event.metaKey === false, `${event.type} more modifiers`);
}

function expectProtected(transfer: DataTransfer, count: number) {
  assert(transfer.effectAllowed === "all", transfer.effectAllowed);
  assert(transfer.dropEffect === "none", transfer.dropEffect);
  assert(Object.isFrozen(transfer.types), "frozen types");
  assert(transfer.types.join(",") === (count === 0 ? "" : "Files"), transfer.types.join(","));
  assert(transfer.items.length === count, `items ${transfer.items.length}`);
  assert(transfer.files.length === 0, "protected files");
  const files = transfer.files;
  assert(transfer.files !== files, "new FileList");
  const items = transfer.items;
  assert(transfer.items === items, "live items");
  assert(transfer.getData("text/plain") === "", "protected getData");
  assert([...transfer.items].length === count, "item iterator");
  transfer.setData("text/plain", "nope");
  assert(transfer.getData("text/plain") === "", "protected setData");
  assert(transfer.types.join(",") === (count === 0 ? "" : "Files"), "types unchanged");
  const added = transfer.items.add(new File(["z"], "z.txt"));
  assert(added === null, "protected add");
  // @ts-expect-error add requires data.
  const missing = thrown(() => transfer.items.add());
  assert(errorMessage(missing) === "DataTransferItemList.add requires data", errorMessage(missing));
  transfer.effectAllowed = "copy";
  assert(
    read<DataTransfer["effectAllowed"]>(transfer.effectAllowed) === "all",
    "protected effectAllowed",
  );
  transfer.dropEffect = "copy";
  assert(transfer.dropEffect === "copy", transfer.dropEffect);
}

Yuzuko.test("a file cancel with no hover has an empty dragleave", () => {
  const window = openWindow("drag-empty");
  const seen = watchDrag(window, ["dragleave"]);
  let custom = 0;
  window.addEventListener("filehovercancel", () => {
    custom += 1;
  });
  window.surface.addEventListener("filehovercancel", () => {
    custom += 1;
  });
  try {
    Yuzuko.__test.inject(window, [{ kind: "filehovercancel" }]);
    assert(seen.length === 2, `count ${seen.length}`);
    const surface = seen[0];
    const bubbled = seen[1];
    assert(surface !== undefined && bubbled !== undefined, "records");
    assert(surface.where === "surface" && surface.type === "dragleave", surface.type);
    assert(bubbled.where === "window" && bubbled.event === surface.event, "same event");
    expectHostDrag(surface.event, window.surface, false);
    const transfer = surface.event.dataTransfer;
    assert(transfer instanceof DataTransfer, "dataTransfer");
    expectProtected(transfer, 0);
    assert(custom === 0, "no filehovercancel");
  } finally {
    window.close();
  }
});

Yuzuko.test("a file hover dispatches protected drag events on the surface", async () => {
  const window = openWindow("drag-hover");
  const surface = window.surface;
  const absolute = Yuzuko.build.os === "windows"
    ? "C:\\yuzuko-drag\\a.txt"
    : "/yuzuko-drag/a.txt";
  const seen = watchDrag(window, ["dragenter", "dragover", "dragleave", "drop"]);
  let custom = 0;
  const onCustom = () => {
    custom += 1;
  };
  window.addEventListener("filehover", onCustom);
  window.addEventListener("filehovercancel", onCustom);
  surface.addEventListener("filehover", onCustom);
  let yuzukoSaw = false;
  const onYuzuko = () => {
    yuzukoSaw = true;
  };
  Yuzuko.addEventListener("dragenter", onYuzuko);
  Yuzuko.addEventListener("filehover", onYuzuko);
  try {
    Yuzuko.__test.inject(window, [
      { kind: "filehover", paths: [absolute, "rel.txt"] },
    ]);
    assert(seen.length === 4, `drag count ${seen.length}`);
    const enterSurface = seen[0];
    const enterWindow = seen[1];
    const overSurface = seen[2];
    const overWindow = seen[3];
    assert(
      enterSurface !== undefined
        && enterWindow !== undefined
        && overSurface !== undefined
        && overWindow !== undefined,
      "records",
    );
    assert(enterSurface.where === "surface" && enterSurface.type === "dragenter", "enter surface");
    assert(enterWindow.where === "window" && enterWindow.event === enterSurface.event, "enter window");
    assert(enterSurface.phase === 2, `enter target phase ${enterSurface.phase}`);
    assert(enterWindow.phase === 3, `enter bubble phase ${enterWindow.phase}`);
    assert(overSurface.where === "surface" && overSurface.type === "dragover", "over surface");
    assert(overWindow.where === "window" && overWindow.event === overSurface.event, "over window");
    expectHostDrag(enterSurface.event, surface, true);
    expectHostDrag(overSurface.event, surface, true);
    const enter = enterSurface.event.dataTransfer;
    const over = overSurface.event.dataTransfer;
    assert(enter instanceof DataTransfer && over instanceof DataTransfer, "transfers");
    assert(enter !== over, "separate stores");
    expectProtected(enter, 2);
    expectProtected(over, 2);
    const first = enter.items[0];
    const second = enter.items[1];
    assert(first !== undefined && second !== undefined, "two items");
    assert(first.kind === "file" && second.kind === "file", first.kind);
    assert(first.type === "text/plain" && second.type === "text/plain", first.type);
    assert(first.getAsFile() === null, "protected file");
    let called = false;
    first.getAsString(() => {
      called = true;
    });
    await new Promise<void>((resolve) => {
      setTimeout(resolve, 0);
    });
    assert(read<boolean>(called) === false, "protected callback");
    assert(custom === 0, "no custom file event");
    assert(yuzukoSaw === false, "drag stays off Yuzuko");
  } finally {
    Yuzuko.removeEventListener("dragenter", onYuzuko);
    Yuzuko.removeEventListener("filehover", onYuzuko);
    window.close();
  }
});

Yuzuko.test("leaving a file hover dispatches dragleave", () => {
  const window = openWindow("drag-leave");
  const seen = watchDrag(window, ["dragenter", "dragover", "dragleave"]);
  let custom = 0;
  window.addEventListener("filehovercancel", () => {
    custom += 1;
  });
  try {
    const note = Yuzuko.build.os === "windows"
      ? "C:\\yuzuko-drag\\note.txt"
      : "/yuzuko-drag/note.txt";
    Yuzuko.__test.inject(window, [
      { kind: "filehover", paths: [note] },
      { kind: "filehovercancel" },
    ]);
    assert(seen.length === 6, `count ${seen.length}`);
    const leaveSurface = seen[4];
    const leaveWindow = seen[5];
    assert(leaveSurface !== undefined && leaveWindow !== undefined, "leave");
    assert(leaveSurface.where === "surface" && leaveSurface.type === "dragleave", leaveSurface.type);
    assert(leaveWindow.event === leaveSurface.event, "same leave");
    expectHostDrag(leaveSurface.event, window.surface, false);
    const transfer = leaveSurface.event.dataTransfer;
    assert(transfer instanceof DataTransfer, "leave transfer");
    expectProtected(transfer, 1);
    const item = transfer.items[0];
    assert(item !== undefined && item.type === "text/plain", item?.type ?? "");
    assert(custom === 0, "no filehovercancel");
  } finally {
    window.close();
  }
});

Yuzuko.test("a file drop exposes the files without a read grant", async () => {
  const root = Yuzuko.makeTempDirSync({ prefix: "yuzuko-drag-" });
  const note = join(root, "note.txt");
  const missing = join(root, "missing.txt");
  try {
    Yuzuko.writeTextFileSync(note, "hello");
    const window = openWindow("drag-drop");
    const seen = watchDrag(window, ["drop"]);
    window.addEventListener("drop", (event) => {
      event.preventDefault();
    });
    let pathEvent = false;
    const onCustom = () => {
      pathEvent = true;
    };
    window.addEventListener("filehover", onCustom);
    window.addEventListener("filehovercancel", onCustom);
    window.addEventListener("filedrop", onCustom);
    try {
      Yuzuko.__test.inject(window, [
        { kind: "filedrop", paths: [note, missing, root] },
      ]);
      assert(seen.length === 2, `drop count ${seen.length}`);
      const surfaceDrop = seen[0];
      const windowDrop = seen[1];
      assert(surfaceDrop !== undefined && windowDrop !== undefined, "drop records");
      assert(surfaceDrop.where === "surface" && windowDrop.event === surfaceDrop.event, "drop path");
      assert(surfaceDrop.event.defaultPrevented === true, "preventDefault");
      expectHostDrag(surfaceDrop.event, window.surface, true);
      const transfer = surfaceDrop.event.dataTransfer;
      assert(transfer instanceof DataTransfer, "drop transfer");
      assert(transfer.effectAllowed === "all", transfer.effectAllowed);
      transfer.effectAllowed = "copy";
      assert(
        read<DataTransfer["effectAllowed"]>(transfer.effectAllowed) === "all",
        "read-only effectAllowed",
      );
      assert(transfer.types.join(",") === "Files", transfer.types.join(","));
      assert(transfer.files.length === 3, `files ${transfer.files.length}`);
      assert(transfer.items.length === 3, `items ${transfer.items.length}`);
      const kept = transfer.types;
      transfer.setData("text/plain", "nope");
      assert(transfer.types === kept, "read-only setData");
      assert(transfer.items.add(new File(["z"], "z.txt")) === null, "read-only add");
      const file = transfer.files[0];
      const item = transfer.items[0];
      assert(file !== undefined && item !== undefined, "first file");
      assert(item.getAsFile() === file, "same File");
      assert(file.name === "note.txt", file.name);
      assert(file.type === "text/plain", file.type);
      assert(file.webkitRelativePath === "", "relative path");
      assert(file.size === 5, `${file.size}`);
      assert(await file.text() === "hello", "text");
      const absent = transfer.files[1];
      assert(absent !== undefined, "missing file");
      assert(absent.name === "missing.txt", absent.name);
      assert(absent.type === "text/plain", absent.type);
      assert(absent.size === 0, "missing size");
      assert(await absent.text() === "", "missing text");
      const directory = transfer.files[2];
      assert(directory !== undefined, "directory");
      assert(directory.name === baseName(root), directory.name);
      assert(directory.type === "", directory.type);
      assert(directory.size === 0, "directory size");
      assert(await directory.text() === "", "directory text");
      assert(pathEvent === false, "no custom file event");
      const hostRead = thrown(() => {
        Yuzuko.readTextFileSync(note);
      });
      assert(hostRead instanceof Error && !(hostRead instanceof DOMException), "host read");
      assert(errorName(hostRead) === "NotCapable", errorName(hostRead));
      Yuzuko.permissions.revokeSync({ name: "read", path: note });
      assert(await file.text() === "hello", "revoke leaves the file readable");
      assert(await file.slice().text() === "hello", "slice");
    } finally {
      window.close();
    }
  } finally {
    Yuzuko.removeSync(root, { recursive: true });
  }
});

Yuzuko.test("DataTransfer items and files follow the drag data store", async () => {
  const transfer = new DataTransfer();
  const other = new DataTransfer();
  assert(transfer.dropEffect === "none", transfer.dropEffect);
  assert(transfer.effectAllowed === "none", transfer.effectAllowed);
  assert(transfer.items.length === 0, "no items");
  assert(transfer.files.length === 0, "no files");
  assert(Object.isFrozen(transfer.types), "frozen");
  assert(transfer.types === transfer.types, "stable types");
  assert(transfer.types !== other.types, "distinct types");
  assert(transfer.items === transfer.items, "stable items");
  assert(transfer.items !== other.items, "distinct items");
  assert(transfer.files !== transfer.files, "fresh files");
  assert(DataTransferItemList.prototype.add.length === 1, "add length");
  assert(DataTransferItem.prototype.getAsString.length === 1, "getAsString length");

  const illegal = thrown(() => {
    new FileList();
  });
  assert(illegal instanceof TypeError, "FileList");
  assert(errorMessage(illegal) === "Illegal constructor", errorMessage(illegal));
  assert(throwsTypeError(() => {
    new DataTransferItem();
  }), "DataTransferItem");
  assert(throwsTypeError(() => {
    new DataTransferItemList();
  }), "DataTransferItemList");
  assert(!("setDragImage" in transfer), "setDragImage");
  assert(!("webkitGetAsEntry" in DataTransferItem.prototype), "webkitGetAsEntry");
  assert(
    transfer.items[Symbol.iterator] === DataTransferItemList.prototype[Symbol.iterator],
    "iterator",
  );
  const iteratorDescriptor = Object.getOwnPropertyDescriptor(
    DataTransferItemList.prototype,
    Symbol.iterator,
  );
  assert(
    iteratorDescriptor !== undefined && iteratorDescriptor.enumerable === false,
    "iterator hidden",
  );
  assert([...transfer.items].length === 0, "empty iterator");
  const illegalItems = thrown(() => {
    DataTransferItemList.prototype[Symbol.iterator].call({});
  });
  assert(errorMessage(illegalItems) === "illegal invocation", errorMessage(illegalItems));

  const file = new File(["abc"], "test.txt", { type: "text/plain" });
  const added = transfer.items.add(file);
  assert(added !== null && added.kind === "file", added?.kind ?? "");
  assert(added.type === "text/plain", added.type);
  assert(transfer.types.join(",") === "Files", transfer.types.join(","));
  transfer.setData("text/plain", "hi");
  assert(transfer.types.join(",") === "text/plain,Files", transfer.types.join(","));
  assert(transfer.files[0] === file, "stored file");
  const beforeRemove = transfer.types;
  transfer.items.remove(0);
  assert(transfer.types !== beforeRemove, "remove allocates");
  assert(transfer.types.join(",") === "text/plain", transfer.types.join(","));
  assert(transfer.files.length === 0, "file removed");
  transfer.items.add(file);
  assert(transfer.types.join(",") === "text/plain,Files", "file returns");
  const duplicateFile = transfer.items.add(file);
  assert(duplicateFile !== null, "duplicate file");
  assert(read<number>(transfer.files.length) === 2, "two files");
  assert(transfer.files[0] === file && transfer.files[1] === file, "same object");
  const spread = [...transfer.files];
  assert(spread.length === 2 && spread[0] === file, "file iterator");
  const itemSpread = [...transfer.items];
  const firstItem = transfer.items[0];
  const secondItem = transfer.items[1];
  const thirdItem = transfer.items[2];
  assert(
    itemSpread.length === 3
      && itemSpread[0] === firstItem
      && itemSpread[1] === secondItem
      && itemSpread[2] === thirdItem
      && secondItem?.getAsFile() === file
      && thirdItem?.getAsFile() === file,
    "item iterator",
  );
  transfer.clearData();
  assert(transfer.types.join(",") === "Files", transfer.types.join(","));
  assert(read<number>(transfer.files.length) === 2, "clearData leaves files");
  const stable = transfer.types;
  transfer.items.remove(50);
  assert(transfer.types === stable, "remove past the end");
  transfer.clearData("text/plain");
  assert(transfer.types === stable, "clear of nothing");
  const addedString = transfer.items.add("again", "text/plain");
  assert(addedString !== null && addedString.kind === "string", addedString?.kind ?? "");
  assert(transfer.types !== stable, "string add allocates");
  const kept = transfer.types;
  const duplicate = thrown(() => {
    transfer.items.add("again", "TEXT");
  });
  assert(duplicate instanceof DOMException, "duplicate class");
  assert(errorName(duplicate) === "NotSupportedError", errorName(duplicate));
  assert(errorMessage(duplicate) === "The type is already present", errorMessage(duplicate));
  assert(transfer.types === kept, "failed add keeps types");
  const emptyType = thrown(() => {
    transfer.items.add("", "");
  });
  assert(emptyType instanceof DOMException, "empty class");
  assert(errorName(emptyType) === "SyntaxError", errorName(emptyType));
  assert(errorMessage(emptyType) === "The type is empty", errorMessage(emptyType));
  assert(transfer.types === kept, "empty type keeps types");
  const noData = thrown(() => {
    // @ts-expect-error add requires data.
    transfer.items.add();
  });
  assert(noData instanceof TypeError, "missing data");
  assert(errorMessage(noData) === "DataTransferItemList.add requires data", errorMessage(noData));
  const noType = thrown(() => {
    // @ts-expect-error a string requires a type.
    transfer.items.add("x");
  });
  assert(errorMessage(noType) === "DataTransferItemList.add requires a type", errorMessage(noType));

  const stringItem = transfer.items[transfer.items.length - 1];
  assert(stringItem !== undefined && stringItem.kind === "string", stringItem?.kind ?? "");
  assert(stringItem.type === "text/plain", stringItem.type);
  let called = false;
  stringItem.getAsString(null);
  // @ts-expect-error undefined is not a callback.
  stringItem.getAsString(undefined);
  assert(called === false, "null callback");
  const badCallback = thrown(() => {
    // @ts-expect-error A number is not a function.
    stringItem.getAsString(1);
  });
  assert(badCallback instanceof TypeError, "callback type");
  assert(
    errorMessage(badCallback) === "DataTransferItem.getAsString requires a function",
    errorMessage(badCallback),
  );
  const text = await new Promise<string>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("getAsString timed out")), 1000);
    stringItem.getAsString((data) => {
      clearTimeout(timer);
      resolve(data);
    });
  });
  assert(text === "again", text);
  let reported = false;
  const onError = (event: Event) => {
    reported = true;
    event.preventDefault();
  };
  globalThis.addEventListener("error", onError);
  try {
    await new Promise<void>((resolve) => {
      stringItem.getAsString(() => {
        resolve();
        throw new Error("callback failed");
      });
    });
    assert(read<boolean>(reported) === true, "reported callback");
  } finally {
    globalThis.removeEventListener("error", onError);
  }

  const liveFile = transfer.items[0];
  assert(liveFile !== undefined && liveFile.getAsFile() === file, "live file");
  transfer.items.remove(0);
  assert(liveFile.kind === "" && liveFile.type === "", liveFile.kind);
  assert(liveFile.getAsFile() === null, "removed file");
  let removedCalled = false;
  liveFile.getAsString(() => {
    removedCalled = true;
  });
  await new Promise<void>((resolve) => {
    setTimeout(resolve, 0);
  });
  assert(removedCalled === false, "removed callback");

  assert(throwsTypeError(() => {
    // @ts-expect-error An index cannot be assigned.
    transfer.items[0] = null;
  }), "assign");
  assert(throwsTypeError(() => {
    delete transfer.items[0];
  }), "delete");
  assert(transfer.items[5] === undefined, "item past the end");
  assert(transfer.files.item(5) === null, "file item past the end");
  assert(transfer.files[5] === undefined, "file index past the end");
  const beforeClear = transfer.types;
  transfer.items.clear();
  assert(transfer.items.length === 0, "cleared");
  assert(transfer.types !== beforeClear, "clear allocates");
  const empty = transfer.types;
  transfer.items.clear();
  assert(transfer.types === empty, "clear of an empty list");
});
