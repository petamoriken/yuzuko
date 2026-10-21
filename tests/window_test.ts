function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

Yuzuko.test("OffscreenCanvas is a buffer and Window has a surface", () => {
  assert(typeof requestAnimationFrame === "undefined", "global rAF");
  assert(typeof Window === "undefined", "global Window");
  assert(typeof WindowSurface === "undefined", "global WindowSurface");
  assert(Yuzuko.Window.name === "Window", "constructor name");
  assert(Yuzuko.WindowSurface.name === "WindowSurface", "surface name");
  let illegal = false;
  try {
    new Yuzuko.WindowSurface();
  } catch (error) {
    illegal = error instanceof TypeError;
  }
  assert(illegal, "illegal surface");
  const buffer = new OffscreenCanvas(30, 40);
  assert(buffer instanceof EventTarget, "buffer target");
  assert(buffer.width === 30 && buffer.height === 40, "buffer size");
  buffer.width = 12;
  assert(buffer.width === 12, "assign width");
  assert(buffer.getContext("2d") !== null, "2d");
  assert(buffer.getContext("webgpu") === null, "webgpu");
  assert(buffer.getContext("bitmaprenderer") === null, "bitmap after 2d");
  const blob = buffer.convertToBlob();
  assert(typeof blob.then === "function", "convertToBlob");

  const window = new Yuzuko.Window({
    title: "tools",
    width: 200,
    height: 100,
  });
  try {
    assert(!(window instanceof OffscreenCanvas), "window is not a canvas");
    assert(typeof window.getContext === "undefined", "no window context");
    assert(window.surface instanceof Yuzuko.WindowSurface, "surface class");
    assert(!(window.surface instanceof OffscreenCanvas), "surface is not OffscreenCanvas");
    assert(window.resizable === false, "resizable default");
    assert(window.surface.objectFit === "scale-down", "objectFit default");
    assert(window.surface.imageRendering === "auto", "imageRendering default");
    window.surface.imageRendering = "pixelated";
    assert(window.surface.imageRendering === "pixelated", "pixelated");
    window.surface.imageRendering = "optimizeSpeed";
    assert(window.surface.imageRendering === "pixelated", "optimizeSpeed");
    window.surface.imageRendering = "crisp-edges";
    assert(window.surface.imageRendering === "crisp-edges", "crisp-edges");
    window.surface.imageRendering = "high-quality";
    assert(window.surface.imageRendering === "crisp-edges", "imageRendering ignored");
    window.surface.imageRendering = "Pixelated";
    assert(window.surface.imageRendering === "pixelated", "imageRendering case");
    window.surface.objectFit = "contain";
    assert(window.surface.objectFit === "contain", "objectFit contain");
    window.surface.objectFit = "stretch";
    assert(window.surface.objectFit === "contain", "objectFit ignored");
    window.surface.objectFit = "Cover";
    assert(window.surface.objectFit === "cover", "objectFit case");
    assert(window.surface.title === "tools", "title");
    assert(typeof window.title === "undefined", "no window title");
    assert(window.clientWidth === 200 && window.clientHeight === 100, "client");
    assert(
      window.surface.width === Math.round(window.clientWidth * window.devicePixelRatio)
        && window.surface.height === Math.round(window.clientHeight * window.devicePixelRatio),
      "physical bitmap",
    );
    const ctx = window.surface.getContext("2d");
    assert(ctx !== null, "surface 2d");
    assert(window.devicePixelRatio === 1, "scale");
    assert(window.cursor === "default", "cursor");
    window.cursor = "pointer";
    assert(window.cursor === "pointer", "pointer cursor");
    window.cursor = "not-a-cursor";
    assert(window.cursor === "pointer", "unknown cursor");
    window.cursor = "Crosshair";
    assert(window.cursor === "crosshair", "cursor case");
    window.resizeTo(180, 90);
    assert(window.clientWidth === 180 && window.clientHeight === 90, "resizeTo");
    assert(
      window.surface.width === 200 && window.surface.height === 100,
      "surface ignores resize",
    );
    const flexible = new Yuzuko.Window({
      width: 20,
      height: 20,
      resizable: true,
    });
    assert(flexible.resizable === true, "resizable");
    flexible.resizable = false;
    assert(flexible.resizable === false, "resizable off");
    flexible.resizable = true;
    assert(flexible.resizable === true, "resizable on");
    flexible.close();
  } finally {
    window.close();
  }
});

Yuzuko.test("two surfaces keep independent pointer, key, and IME sequences", () => {
  const left = new Yuzuko.Window({ width: 200, height: 100, title: "left" });
  const right = new Yuzuko.Window({ width: 220, height: 120, title: "right" });
  const leftEvents = [];
  const rightEvents = [];
  record(left, leftEvents);
  record(right, rightEvents);
  try {
    left.setImeAllowed(true);
    Yuzuko.__test.inject(left, [
      { kind: "pointer", type: "pointermove", x: 4, y: 6 },
      { kind: "pointer", type: "pointermove", x: 8, y: 9 },
      { kind: "pointer", type: "pointerdown", x: 8, y: 9, button: 0 },
      { kind: "pointer", type: "pointerup", x: 8, y: 9, button: 0 },
      { kind: "pointer", type: "pointerdown", x: 8, y: 9, button: 0 },
      { kind: "pointer", type: "pointerup", x: 8, y: 9, button: 0 },
      { kind: "key", type: "keydown", key: "a", code: "KeyA", text: "a" },
      { kind: "key", type: "keyup", key: "a", code: "KeyA" },
      { kind: "ime", action: "enabled" },
      { kind: "ime", action: "preedit", data: "ka" },
      { kind: "key", type: "keydown", key: "a", code: "KeyA" },
      { kind: "ime", action: "commit", data: "か" },
    ]);
    assert(rightEvents.length === 0, "right stayed quiet");
    assert(describe(leftEvents) === [
      "pointermove:1:mouse:8,9:-1:4,3:0",
      "mousemove:1:mouse:8,9:0:4,3:0",
      "pointerdown:1:mouse:8,9:0:0,0:0.5",
      "mousedown:1:mouse:8,9:0:0,0:0.5",
      "pointerup:1:mouse:8,9:0:0,0:0",
      "mouseup:1:mouse:8,9:0:0,0:0",
      "click:1:mouse:8,9:0:0,0:0",
      "pointerdown:1:mouse:8,9:0:0,0:0.5",
      "mousedown:1:mouse:8,9:0:0,0:0.5",
      "pointerup:1:mouse:8,9:0:0,0:0",
      "mouseup:1:mouse:8,9:0:0,0:0",
      "click:1:mouse:8,9:0:0,0:0",
      "dblclick:1:mouse:8,9:0:0,0:0",
      "keydown:a:KeyA:false",
      "beforeinput:insertText:a:false",
      "keyup:a:KeyA:false",
      "compositionstart:",
      "compositionupdate:ka",
      "beforeinput:insertCompositionText:ka:true",
      "keydown:a:KeyA:true",
      "compositionend:か",
      "beforeinput:insertText:か:false",
    ].join("\n"), describe(leftEvents));
    const move = leftEvents[0];
    assert(move.clientX === 8 && move.clientY === 9, "client origin");
    assert(move.offsetX === 8 && move.offsetY === 9, "offset origin");
    assert(move.pageX === 8 && move.pageY === 9, "page origin");
    assert(move.getCoalescedEvents().length === 2, "coalesced");
    assert(move.getCoalescedEvents()[1].clientX === 8, "last coalesced");
    assert(move.getCoalescedEvents()[1].clientY === 9, "last coalesced y");
    assert(leftEvents[6].detail === 1, "click detail");
    assert(leftEvents[12].detail === 2, "dblclick detail");

    left.close();
    let closed = false;
    try {
      Yuzuko.__test.inject(left, [{ kind: "pointer", type: "pointermove", x: 1, y: 1 }]);
    } catch (error) {
      closed = error.name === "InvalidStateError";
    }
    assert(closed, "closed inject");
    Yuzuko.__test.inject(right, [
      { kind: "pointer", type: "pointerdown", x: 3, y: 4, button: 2 },
      { kind: "pointer", type: "pointerup", x: 3, y: 4, button: 2 },
    ]);
    assert(describe(rightEvents) === [
      "pointerdown:1:mouse:3,4:2:0,0:0.5",
      "mousedown:1:mouse:3,4:2:0,0:0.5",
      "pointerup:1:mouse:3,4:2:0,0:0",
      "mouseup:1:mouse:3,4:2:0,0:0",
      "auxclick:1:mouse:3,4:2:0,0:0",
      "contextmenu:1:mouse:3,4:2:0,0:0",
    ].join("\n"), describe(rightEvents));
    assert(leftEvents.length === 22, "left did not grow");
  } finally {
    left.close();
    right.close();
  }
});

Yuzuko.test("preventDefault on keydown skips text and close is cancelable", () => {
  const window = new Yuzuko.Window({ width: 40, height: 40 });
  const events = [];
  record(window, events);
  window.addEventListener("keydown", (event) => {
    if (event.key === "b") {
      event.preventDefault();
    }
  });
  try {
    Yuzuko.__test.inject(window, [
      { kind: "key", type: "keydown", key: "b", code: "KeyB", text: "b" },
    ]);
    assert(describe(events) === "keydown:b:KeyB:false", describe(events));
    let prevented = false;
    window.addEventListener("close", (event) => {
      event.preventDefault();
      prevented = true;
    }, { once: true });
    window.close();
    assert(prevented, "close fired");
    Yuzuko.__test.inject(window, [
      { kind: "focus" },
    ]);
    assert(events[events.length - 1].type === "focus", "still open");
  } finally {
    window.close();
  }
});

Yuzuko.test("animation frames run once per frame and pointer lock resolves", async () => {
  const window = new Yuzuko.Window({ width: 64, height: 32 });
  let phase = 0;
  let locked = 0;
  window.addEventListener("pointerlockchange", () => {
    locked += 1;
  });
  try {
    await window.requestPointerLock();
    assert(locked === 1, "locked");
    await window.exitPointerLock();
    assert(locked === 2, "unlocked");
    await window.requestFullscreen();
    await window.exitFullscreen();
    await new Promise((resolve) => {
      window.requestAnimationFrame(() => {
        assert(phase === 0, "first frame");
        phase = 1;
        window.requestAnimationFrame(() => {
          assert(phase === 1, "second frame");
          phase = 2;
          window.close();
          resolve();
        });
        assert(phase === 1, "inner waits");
      });
    });
    assert(phase === 2, "finished");
  } finally {
    window.close();
  }
});

Yuzuko.test("animation frames keep a display cadence", async () => {
  const window = new Yuzuko.Window({ width: 32, height: 32 });
  try {
    const gaps = await new Promise((resolve) => {
      const samples = [];
      let last = 0;
      let left = 8;
      window.requestAnimationFrame(function frame(now) {
        if (last !== 0) {
          samples.push(now - last);
        }
        last = now;
        left -= 1;
        if (left === 0) {
          resolve(samples);
          return;
        }
        window.requestAnimationFrame(frame);
      });
    });
    const average = gaps.reduce((sum, gap) => sum + gap, 0) / gaps.length;
    assert(average < 40, "average frame " + average.toFixed(1));
    assert(gaps.every((gap) => gap < 80), "frame gap");
  } finally {
    window.close();
  }
});

Yuzuko.test("a closed window requestAnimationFrame returns 0", async () => {
  const window = new Yuzuko.Window({ width: 40, height: 40 });
  const clock = new Yuzuko.Window({ width: 8, height: 8 });
  let orphan = null;
  try {
    const first = window.requestAnimationFrame(() => {});
    assert(first === 1, "first handle");
    window.cancelAnimationFrame(first);
    window.cancelAnimationFrame(0);

    let kept = 0;
    let keptHandle = 0;
    window.requestAnimationFrame(() => {
      kept += 1;
    });
    window.addEventListener("close", (event) => {
      event.preventDefault();
      keptHandle = window.requestAnimationFrame(() => {
        kept += 1;
      });
    }, { once: true });
    window.close();
    assert(keptHandle > 0, "queued during close");
    await nextFrame(window);
    assert(kept === 2, "cancelled close kept both frames");

    let dropped = 0;
    let droppedHandle = 0;
    let after = -1;
    window.addEventListener("close", () => {
      droppedHandle = window.requestAnimationFrame(() => {
        dropped += 1;
      });
    }, { once: true });
    await new Promise((resolve) => {
      window.requestAnimationFrame(() => {
        window.close();
        after = window.requestAnimationFrame(() => {
          dropped += 1;
        });
        window.cancelAnimationFrame(after);
        window.cancelAnimationFrame(0);
        resolve();
      });
    });
    assert(droppedHandle > 0, "queued before destroy");
    assert(after === 0, "returns 0 after close");
    const outside = window.requestAnimationFrame(() => {
      dropped += 1;
    });
    assert(outside === 0, "already closed");
    window.cancelAnimationFrame(1);
    let bad = false;
    try {
      window.requestAnimationFrame(1);
    } catch (error) {
      bad = error instanceof TypeError;
    }
    assert(bad, "callback type");
    let rejected = false;
    try {
      window.resizeTo(10, 10);
    } catch (error) {
      rejected = error instanceof DOMException && error.name === "InvalidStateError";
    }
    assert(rejected, "resize still throws");

    orphan = new Yuzuko.Window({ width: 8, height: 8 });
    let ran = 0;
    orphan.requestAnimationFrame(() => {
      ran += 1;
    });
    orphan.close();
    assert(orphan.requestAnimationFrame(() => {
      ran += 1;
    }) === 0, "closed from outside");
    await nextFrames(clock, 2);
    assert(dropped === 0, "destroyed queue");
    assert(ran === 0, "cleared before its frame");
  } finally {
    window.close();
    clock.close();
    if (orphan !== null) {
      orphan.close();
    }
  }
});

Yuzuko.test("matchMedia tracks size, pixel ratio, and preferences", () => {
  const window = new Yuzuko.Window({ width: 400, height: 200 });
  try {
    const wide = window.matchMedia("(min-width: 300px)");
    assert(wide.matches, "min-width");
    assert(wide.media === "(min-width: 300px)", "media text");
    assert(window.matchMedia("(width >= 400px)").matches, "width >=");
    assert(!window.matchMedia("(width > 400px)").matches, "width >");
    assert(window.matchMedia("(200px <= width <= 500px)").matches, "range");
    assert(window.matchMedia("(orientation: landscape)").matches, "landscape");
    assert(!window.matchMedia("(orientation: portrait)").matches, "portrait");
    assert(
      window.matchMedia("(min-width: 100px) and (orientation: landscape)").matches,
      "and",
    );
    assert(
      window.matchMedia("(orientation: portrait), (pointer: fine)").matches,
      "comma or",
    );
    assert(window.matchMedia("(pointer: fine)").matches, "fine pointer");
    assert(!window.matchMedia("(pointer: coarse)").matches, "not coarse");
    assert(window.matchMedia("(pointer)").matches, "boolean pointer");
    const dpr = window.devicePixelRatio;
    assert(
      window.matchMedia(`(resolution >= ${dpr}dppx)`).matches,
      "resolution",
    );
    assert(
      window.matchMedia(`(device-pixel-ratio >= ${dpr})`).matches,
      "device-pixel-ratio",
    );
    assert(window.matchMedia("(color-gamut: srgb)").matches, "srgb");
    const p3 = window.matchMedia("(color-gamut: p3)").matches;
    const rec2020 = window.matchMedia("(color-gamut: rec2020)").matches;
    assert(!rec2020 || p3, "rec2020 implies p3");
    assert(!p3 || window.matchMedia("(color-gamut: srgb)").matches, "p3 implies srgb");
    const light = window.matchMedia("(prefers-color-scheme: light)").matches;
    const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    assert(light !== dark, "one scheme");
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const motion = window.matchMedia("(prefers-reduced-motion: no-preference)").matches;
    assert(reduce !== motion, "one motion");
    const contrasts = ["no-preference", "less", "more", "custom"].filter((value) =>
      window.matchMedia(`(prefers-contrast: ${value})`).matches
    );
    assert(contrasts.length === 1, contrasts.join(","));
    assert(!window.matchMedia("((((").matches, "invalid");
    assert(wide instanceof MediaQueryList, "list type");
    let illegalList = false;
    try {
      new MediaQueryList();
    } catch (error) {
      illegalList = error instanceof TypeError;
    }
    assert(illegalList, "illegal list");
    let illegalEvent = false;
    try {
      new MediaQueryListEvent("change");
    } catch (error) {
      illegalEvent = error instanceof TypeError;
    }
    assert(illegalEvent, "illegal event");
    let changes = 0;
    let legacy = 0;
    let attribute = 0;
    const hear = () => {
      legacy += 1;
    };
    wide.addListener(hear);
    wide.addListener(hear);
    wide.onchange = () => {
      attribute += 1;
    };
    wide.addEventListener("change", (event) => {
      changes += 1;
      assert(event instanceof MediaQueryListEvent, "change event");
      assert(event.matches === false, "now narrow");
      assert(event.media === "(min-width: 300px)", "event media");
    });
    window.resizeTo(40, 80);
    assert(!wide.matches, "resized");
    assert(changes === 1, "change fired");
    assert(legacy === 1, "addListener");
    assert(attribute === 1, "onchange");
    assert(window.matchMedia("(orientation: portrait)").matches, "now portrait");
  } finally {
    window.close();
  }
});

Yuzuko.test("visibilityState tracks occlusion, not focus", () => {
  const window = new Yuzuko.Window({ width: 80, height: 40 });
  try {
    const surface = window.surface;
    assert(surface.visibilityState === "visible", "starts visible");
    assert(new OffscreenCanvas(1, 1).visibilityState === undefined, "buffer");
    const states = [];
    surface.addEventListener("visibilitychange", (event) => {
      assert(event.target === surface, "target");
      assert(event.isTrusted === true, "trusted");
      states.push(surface.visibilityState);
    });
    Yuzuko.__test.inject(window, [{ kind: "blur" }]);
    assert(surface.visibilityState === "visible", "blur stays visible");
    assert(states.length === 0, "blur is quiet");
    Yuzuko.__test.inject(window, [{ kind: "visibility", state: "hidden" }]);
    assert(surface.visibilityState === "hidden", "occluded");
    Yuzuko.__test.inject(window, [{ kind: "visibility", state: "hidden" }]);
    assert(states.join(",") === "hidden", "one hide");
    Yuzuko.__test.inject(window, [{ kind: "visibility", state: "visible" }]);
    assert(surface.visibilityState === "visible", "shown");
    assert(states.join(",") === "hidden,visible", states.join(","));
    let rejected = false;
    try {
      Yuzuko.__test.inject(window, [{ kind: "visibility", state: "prerender" }]);
    } catch (error) {
      rejected = error instanceof TypeError;
    }
    assert(rejected, "bad state");
    assert(surface.visibilityState === "visible", "unchanged");
    window.close();
    assert(surface.visibilityState === "hidden", "closed");
    assert(states.join(",") === "hidden,visible,hidden", states.join(","));
  } finally {
    window.close();
  }
});

Yuzuko.test("focus and blur are FocusEvents on the window", () => {
  const window = new Yuzuko.Window({ width: 80, height: 40 });
  try {
    const seen = [];
    const onSurface = [];
    window.surface.addEventListener("focus", () => {
      onSurface.push("focus");
    });
    window.surface.addEventListener("blur", () => {
      onSurface.push("blur");
    });
    window.addEventListener("focus", (event) => {
      assert(event.target === window, "focus target");
      assert(event instanceof FocusEvent, "focus event");
      assert(event.relatedTarget === null, "focus related");
      assert(event.bubbles === false, "focus bubbles");
      seen.push("focus");
    });
    window.addEventListener("blur", (event) => {
      assert(event.target === window, "blur target");
      assert(event instanceof FocusEvent, "blur event");
      assert(event.relatedTarget === null, "blur related");
      seen.push("blur");
    });
    Yuzuko.__test.inject(window, [
      { kind: "focus" },
      { kind: "blur" },
      { kind: "focus" },
    ]);
    assert(seen.join(",") === "focus,blur,focus", seen.join(","));
    assert(onSurface.length === 0, "not the surface");
  } finally {
    window.close();
  }
});

Yuzuko.test("pointer events propagate from the window to its surface", () => {
  const window = new Yuzuko.Window({ width: 80, height: 40 });
  try {
    const order = [];
    const capture = (event) => {
      order.push("capture");
      assert(event.eventPhase === Event.CAPTURING_PHASE, "capture phase");
      assert(event.currentTarget === window, "capture current");
      assert(event.target === window.surface, "target");
      const path = event.composedPath();
      assert(path[0] === window.surface && path[1] === window, "path");
    };
    const atSurface = (event) => {
      order.push("surface");
      assert(event.eventPhase === Event.AT_TARGET, "target phase");
      assert(event.currentTarget === window.surface, "surface current");
    };
    const bubble = (event) => {
      order.push("bubble");
      assert(event.eventPhase === Event.BUBBLING_PHASE, "bubble phase");
      assert(event.currentTarget === window, "bubble current");
    };
    window.addEventListener("pointerdown", capture, true);
    window.surface.addEventListener("pointerdown", atSurface);
    window.addEventListener("pointerdown", bubble);
    Yuzuko.__test.inject(window, [
      { kind: "pointer", type: "pointerdown", x: 3, y: 4, button: 0 },
    ]);
    assert(order.join(",") === "capture,surface,bubble", order.join(","));

    window.removeEventListener("pointerdown", capture, true);
    window.surface.removeEventListener("pointerdown", atSurface);
    window.removeEventListener("pointerdown", bubble);

    const stopped = [];
    const stopCapture = (event) => {
      stopped.push("capture");
      event.stopPropagation();
    };
    const missed = () => stopped.push("later");
    window.addEventListener("pointerdown", stopCapture, true);
    window.surface.addEventListener("pointerdown", missed);
    window.addEventListener("pointerdown", missed);
    Yuzuko.__test.inject(window, [
      { kind: "pointer", type: "pointerdown", x: 3, y: 4, button: 0 },
    ]);
    assert(stopped.join(",") === "capture", stopped.join(","));

    window.removeEventListener("pointerdown", stopCapture, true);
    window.surface.removeEventListener("pointerdown", missed);
    window.removeEventListener("pointerdown", missed);

    const immediate = [];
    const stopNow = (event) => {
      immediate.push("first");
      event.stopImmediatePropagation();
    };
    const second = () => immediate.push("second");
    window.addEventListener("pointerdown", stopNow, true);
    window.addEventListener("pointerdown", second, true);
    window.surface.addEventListener("pointerdown", second);
    Yuzuko.__test.inject(window, [
      { kind: "pointer", type: "pointerdown", x: 3, y: 4, button: 0 },
    ]);
    assert(immediate.join(",") === "first", immediate.join(","));

    window.removeEventListener("pointerdown", stopNow, true);
    window.removeEventListener("pointerdown", second, true);
    window.surface.removeEventListener("pointerdown", second);

    const fromSurface = [];
    const stopAtSurface = (event) => {
      fromSurface.push("surface");
      event.stopPropagation();
    };
    const alsoSurface = () => fromSurface.push("surface-next");
    const after = () => fromSurface.push("bubble");
    window.addEventListener("pointerdown", () => fromSurface.push("capture"), true);
    window.surface.addEventListener("pointerdown", stopAtSurface);
    window.surface.addEventListener("pointerdown", alsoSurface);
    window.addEventListener("pointerdown", after);
    Yuzuko.__test.inject(window, [
      { kind: "pointer", type: "pointerdown", x: 3, y: 4, button: 0 },
      { kind: "pointer", type: "pointerup", x: 3, y: 4, button: 0 },
    ]);
    assert(
      fromSurface.join(",") === "capture,surface,surface-next",
      fromSurface.join(","),
    );

    const mouse = [];
    window.surface.addEventListener("mousedown", () => mouse.push("mousedown"));
    window.surface.addEventListener("mouseup", () => mouse.push("mouseup"));
    window.surface.addEventListener("click", (event) => {
      mouse.push("click");
      assert(event.target === window.surface, "click target");
    });
    Yuzuko.__test.inject(window, [
      { kind: "pointer", type: "pointerdown", x: 5, y: 6, button: 0 },
      { kind: "pointer", type: "pointerup", x: 5, y: 6, button: 0 },
    ]);
    assert(mouse.join(",") === "mousedown,mouseup,click", mouse.join(","));

    let keys = 0;
    window.surface.addEventListener("keydown", () => {
      keys += 1;
    });
    Yuzuko.__test.inject(window, [
      { kind: "key", type: "keydown", key: "a", code: "KeyA" },
    ]);
    assert(keys === 0, "keys stay on the window");
  } finally {
    window.close();
  }
});

Yuzuko.test("window geometry follows the frame and the client", () => {
  const plain = new Yuzuko.Window({ width: 200, height: 100 });
  try {
    assert(plain.outerX === 0 && plain.outerY === 0, "default origin");
    assert(plain.screenX === 0 && plain.screenY === 0, "default client");
    assert(
      plain.screenLeft === plain.screenX && plain.screenTop === plain.screenY,
      "aliases",
    );
    assert(plain.innerX === plain.screenX && plain.innerY === plain.screenY, "inner");
    assert(plain.visible === true, "visible");
    assert(plain.alwaysOnTop === false, "alwaysOnTop default");
    assert(plain.decorations === true, "decorations default");
    const bare = new Yuzuko.Window({
      width: 20,
      height: 20,
      decorations: false,
      transparent: true,
    });
    try {
      assert(bare.decorations === false, "no decorations");
      assert(bare.transparent === true, "transparent");
    } finally {
      bare.close();
    }
    assert(plain.active === true, "active default");
    assert(plain.maximized === false, "maximized default");
    assert(plain.transparent === false, "opaque default");
    assert(plain.minWidth === undefined && plain.maxHeight === undefined, "no limits");
    assert(typeof plain.reload === "undefined", "no reload");
  } finally {
    plain.close();
  }

  const window = new Yuzuko.Window({
    title: "placed",
    width: 200,
    height: 100,
    x: 40,
    y: -15,
    alwaysOnTop: true,
    active: false,
  });
  const other = new Yuzuko.Window({ width: 50, height: 40, x: 0, y: 0 });
  try {
    assert(window.outerX === 40 && window.outerY === -15, "placed");
    assert(window.screenX === 40 && window.screenY === -15, "headless client");
    assert(window.active === false, "inactive");
    assert(window.alwaysOnTop === true, "alwaysOnTop");
    window.alwaysOnTop = false;
    assert(window.alwaysOnTop === false, "alwaysOnTop off");
    window.moveTo(10, 20);
    assert(window.outerX === 10 && window.outerY === 20, "moveTo");
    assert(window.screenX === 10 && window.innerY === 20, "moveTo client");
    window.moveBy(5, -4);
    assert(window.outerX === 15 && window.outerY === 16, "moveBy");
    window.resizeBy(8, -10);
    assert(window.clientWidth === 208 && window.clientHeight === 90, "resizeBy");
    assert(window.surface.width === 200 && window.surface.height === 100, "bitmap stays");
    window.surface.title = "renamed";
    assert(window.surface.title === "renamed", "surface title");
    window.hide();
    assert(window.visible === false, "hide");
    assert(window.surface.visibilityState === "visible", "occlusion stays");
    window.show();
    assert(window.visible === true, "show");
    const seen = [];
    other.addEventListener("focus", () => seen.push("other-focus"));
    other.addEventListener("blur", () => seen.push("other-blur"));
    window.addEventListener("focus", () => seen.push("focus"));
    other.focus();
    window.focus();
    window.focus();
    assert(seen.join(",") === "other-focus,other-blur,focus", seen.join(","));
    let bad = false;
    try {
      window.moveTo(Number.NaN, 1);
    } catch (error) {
      bad = error instanceof TypeError;
    }
    assert(bad, "nan position");
    let shrunk = false;
    try {
      window.resizeBy(-1000, 0);
    } catch (error) {
      shrunk = error instanceof TypeError;
    }
    assert(shrunk, "negative size");
    const limited = new Yuzuko.Window({
      width: 50,
      height: 20,
      visible: false,
      maximized: true,
      minWidth: 80,
      minHeight: 40,
      maxWidth: 400,
      maxHeight: 300,
    });
    try {
      assert(limited.visible === false, "starts hidden");
      assert(limited.maximized === true, "maximized");
      assert(limited.clientWidth === 80 && limited.clientHeight === 40, "opened inside min");
      assert(limited.minWidth === 80 && limited.minHeight === 40, "min");
      assert(limited.maxWidth === 400 && limited.maxHeight === 300, "max");
      assert(limited.minHeight !== undefined && limited.maxWidth !== undefined, "limits set");
      limited.resizeTo(10, 500);
      assert(limited.clientWidth === 80 && limited.clientHeight === 300, "clamped");
      limited.resizeBy(1000, -1000);
      assert(limited.clientWidth === 400 && limited.clientHeight === 40, "clamped by");
      limited.show();
      assert(limited.visible === true, "shown");
    } finally {
      limited.close();
    }
    const wide = new Yuzuko.Window({ width: 100, height: 80, minWidth: 60 });
    try {
      assert(wide.minWidth === 60 && wide.minHeight === undefined, "one minimum");
      wide.resizeTo(10, 10);
      assert(wide.clientWidth === 60 && wide.clientHeight === 10, "width floor");
    } finally {
      wide.close();
    }
    let inverted = false;
    try {
      new Yuzuko.Window({ width: 50, height: 50, minWidth: 80, maxWidth: 40 });
    } catch (error) {
      inverted = error instanceof TypeError;
    }
    assert(inverted, "minimum above maximum");
  } finally {
    window.close();
    other.close();
  }
  let closed = false;
  try {
    window.moveTo(1, 1);
  } catch (error) {
    closed = error instanceof DOMException && error.name === "InvalidStateError";
  }
  assert(closed, "closed move");
  assert(window.screenX === 0 && window.outerY === 0, "closed origin");
  assert(window.visible === false, "closed visible");
});

function nextFrame(window) {
  return new Promise((resolve) => {
    window.requestAnimationFrame(() => resolve());
  });
}

function nextFrames(window, count) {
  return new Promise((resolve) => {
    const step = () => {
      count -= 1;
      if (count === 0) {
        resolve();
      } else {
        window.requestAnimationFrame(step);
      }
    };
    window.requestAnimationFrame(step);
  });
}

function record(window, into) {
  const types = [
    "pointermove",
    "pointerdown",
    "pointerup",
    "pointercancel",
    "mousemove",
    "mousedown",
    "mouseup",
    "click",
    "dblclick",
    "auxclick",
    "contextmenu",
    "keydown",
    "keyup",
    "compositionstart",
    "compositionupdate",
    "compositionend",
    "beforeinput",
    "focus",
    "blur",
    "wheel",
  ];
  for (const type of types) {
    window.addEventListener(type, (event) => {
      into.push(event);
    });
  }
}

function describe(events) {
  const lines = [];
  for (const event of events) {
    if (event.type.startsWith("pointer") || event.type.startsWith("mouse") ||
      event.type === "click" || event.type === "dblclick" ||
      event.type === "auxclick" || event.type === "contextmenu") {
      lines.push([
        event.type,
        event.pointerId,
        event.pointerType,
        event.clientX + "," + event.clientY,
        event.button,
        event.movementX + "," + event.movementY,
        event.pressure,
      ].join(":"));
    } else if (event.type === "keydown" || event.type === "keyup") {
      lines.push([event.type, event.key, event.code, event.isComposing].join(":"));
    } else if (event.type.startsWith("composition")) {
      lines.push(event.type + ":" + event.data);
    } else if (event.type === "beforeinput") {
      lines.push([event.type, event.inputType, event.data, event.isComposing].join(":"));
    } else {
      lines.push(event.type);
    }
  }
  return lines.join("\n");
}
