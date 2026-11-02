function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

interface Seen {
  at: string;
  phase: number;
  trusted: boolean;
  target: EventTarget | null;
  bubbles: boolean;
  cancelable: boolean;
}

Yuzuko.test("assistive actions bubble through the authored tree", () => {
  const window = new Yuzuko.Window({
    width: 180,
    height: 80,
    title: "input",
  });
  try {
    const root = window.attachAccessibleRoot();
    const child = new Yuzuko.AccessibleNode();
    child.role = "button";
    root.appendChild(child);

    const seen: Seen[] = [];
    const record = (at: string) => (event: Event) => {
      seen.push({
        at,
        phase: event.eventPhase,
        trusted: event.isTrusted,
        target: event.target,
        bubbles: event.bubbles,
        cancelable: event.cancelable,
      });
    };
    child.addEventListener("accessibleclick", record("child"));
    root.addEventListener("accessibleclick", record("root"));
    Yuzuko.__test.inject(window, [
      { kind: "accessible", type: "click", node: child },
    ]);
    assert(seen.length === 2, `click events ${seen.length}`);
    assert(seen[0].at === "child", "target first");
    assert(seen[0].phase === Event.AT_TARGET, "target phase");
    assert(seen[1].at === "root", "then the root");
    assert(seen[1].phase === Event.BUBBLING_PHASE, "bubble phase");
    assert(seen[0].trusted === true, "trusted");
    assert(seen[0].target === child, "event target");
    assert(seen[0].bubbles === true, "bubbles");
    assert(seen[0].cancelable === false, "not cancelable");

    child.dispatchEvent(new Event("accessibleclick", { bubbles: true }));
    assert(seen.length === 3, "dispatchEvent reaches the target");
    assert(seen[2].at === "child", "user event stays on the target");
    assert(seen[2].trusted === false, "user event is not trusted");

    const kinds = [
      "contextmenu",
      "decrement",
      "increment",
      "scrollintoview",
    ] as const;
    for (const kind of kinds) {
      let hit = false;
      const name = `accessible${kind}`;
      const listener = () => {
        hit = true;
      };
      child.addEventListener(name, listener);
      Yuzuko.__test.inject(window, [
        { kind: "accessible", type: kind, node: child },
      ]);
      child.removeEventListener(name, listener);
      assert(hit, kind);
    }

    child.role = "slider";
    child.valueNow = 1;
    let detail: unknown = "unset";
    child.addEventListener("accessiblesetvalue", (event) => {
      detail = event.detail;
    });
    Yuzuko.__test.inject(window, [
      { kind: "accessible", type: "setvalue", node: child, value: 4 },
    ]);
    assert(detail === 4, `detail ${String(detail)}`);
    assert(child.valueNow === 1, "setvalue does not change valueNow");
    Yuzuko.__test.inject(window, [
      { kind: "accessible", type: "setvalue", node: child },
    ]);
    assert(detail === null, "missing number");
    Yuzuko.__test.inject(window, [
      { kind: "accessible", type: "setvalue", node: child, value: Infinity },
    ]);
    assert(detail === null, "non-finite number");
    assert(child.valueNow === 1, "valueNow stays");

    child.focusable = true;
    let accessible = false;
    let os = false;
    child.addEventListener("accessiblefocus", () => {
      accessible = true;
    });
    window.addEventListener("focus", () => {
      os = true;
    });
    window.addEventListener("blur", () => {
      os = true;
    });
    Yuzuko.__test.inject(window, [
      { kind: "accessible", type: "focus", node: child },
    ]);
    assert(accessible, "accessiblefocus");
    assert(os === false, "no window focus or blur");

    let thrown: unknown;
    try {
      Yuzuko.__test.inject(window, [
        { kind: "accessible", type: "nope", node: {} },
      ]);
    } catch (error) {
      thrown = error;
    }
    assert(thrown instanceof TypeError, "unknown type");
    assert(
      (thrown as Error).message === "The value is not a valid token.",
      (thrown as Error).message,
    );
    thrown = undefined;
    try {
      Yuzuko.__test.inject(window, [
        { kind: "accessible", type: "click" },
      ]);
    } catch (error) {
      thrown = error;
    }
    assert(thrown instanceof TypeError, "missing node");
    assert(
      (thrown as Error).message === "The node must be an AccessibleNode.",
      (thrown as Error).message,
    );
  } finally {
    window.close();
  }
});
