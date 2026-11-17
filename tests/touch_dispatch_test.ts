function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function touch(
  kind: string,
  type: string,
  fields: Record<string, unknown>,
): Record<string, unknown> {
  return {
    kind,
    type,
    pointerType: "touch",
    ...fields,
  };
}

Yuzuko.test("a host touch dispatches a trusted TouchEvent", () => {
  assert(typeof Yuzuko.__test.inject === "function", "inject");
  const window = new Yuzuko.Window({ width: 200, height: 100 });
  const order: string[] = [];
  const touches: TouchEvent[] = [];
  const bubble: string[] = [];
  const types = [
    "pointerdown",
    "touchstart",
    "mousedown",
    "pointerrawupdate",
    "pointermove",
    "touchmove",
    "mousemove",
    "pointerup",
    "touchend",
    "mouseup",
    "click",
    "pointercancel",
    "touchcancel",
  ];
  try {
    for (const type of types) {
      window.surface.addEventListener(type, (event: Event) => {
        order.push(type);
        if (!type.startsWith("touch")) {
          return;
        }
        // @ts-expect-error The touch listener receives a TouchEvent.
        const touchEvent: TouchEvent = event;
        touches.push(touchEvent);
        assert(event instanceof TouchEvent, `${type} class`);
        assert(event.isTrusted === true, `${type} trusted`);
        assert(event.target === window.surface, `${type} target`);
        assert(event.currentTarget === window.surface, `${type} current`);
        assert(event.eventPhase === Event.AT_TARGET, `${type} phase`);
        assert(event.bubbles === true, `${type} bubbles`);
        assert(event.composed === true, `${type} composed`);
        // @ts-expect-error The view is the realm global.
        assert(event.view === globalThis, `${type} view`);
        assert(event.detail === 0, `${type} detail`);
        assert(event.touches !== event.targetTouches, `${type} lists`);
        assert(event.touches.length === event.targetTouches.length, `${type} count`);
      });
      window.addEventListener(type, (event) => {
        if (!type.startsWith("touch")) {
          return;
        }
        bubble.push(type);
        assert(event.target === window.surface, `${type} bubble target`);
        assert(event.currentTarget === window, `${type} bubble current`);
        assert(event.eventPhase === Event.BUBBLING_PHASE, `${type} bubble phase`);
      });
    }
    Yuzuko.__test.inject(window, [
      touch("pointer", "pointerdown", {
        pointerId: 3,
        x: 10.5,
        y: 20.25,
        screenX: 30.5,
        screenY: 40.75,
        pressure: 0.4,
        ctrlKey: true,
      }),
      touch("pointer", "pointermove", {
        pointerId: 3,
        x: 11,
        y: 21,
        pressure: 0.2,
      }),
      touch("pointer", "pointermove", {
        pointerId: 3,
        x: 12.5,
        y: 21.25,
        screenX: 32.5,
        screenY: 41.75,
        pressure: 0.6,
        shiftKey: true,
      }),
      touch("pointer", "pointerdown", {
        pointerId: 4,
        x: 1.5,
        y: 2.5,
        pressure: 0.2,
      }),
      touch("pointer", "pointerup", {
        pointerId: 3,
        x: 13,
        y: 22,
        pressure: 0.1,
      }),
      touch("pointer", "pointercancel", {
        pointerId: 4,
        x: 1.5,
        y: 2.5,
      }),
      { kind: "pointer", type: "pointerdown", pointerType: "mouse", x: 5, y: 6 },
      { kind: "pointer", type: "pointerdown", pointerType: "pen", pointerId: 9, x: 7, y: 8 },
      touch("pointer", "pointermove", {
        pointerId: 11,
        x: 8.5,
        y: 9.5,
        pressure: 0.3,
      }),
      touch("pointer", "pointerup", {
        pointerId: 12,
        x: 9,
        y: 9,
      }),
    ]);
    assert(
      order.join(",") ===
        [
          "pointerdown",
          "touchstart",
          "mousedown",
          "pointerrawupdate",
          "pointerrawupdate",
          "pointermove",
          "touchmove",
          "mousemove",
          "pointerdown",
          "touchstart",
          "mousedown",
          "pointerup",
          "touchend",
          "mouseup",
          "click",
          "pointercancel",
          "touchcancel",
          "pointerdown",
          "mousedown",
          "pointerdown",
          "mousedown",
          "pointerrawupdate",
          "pointermove",
          "touchmove",
          "mousemove",
          "pointerup",
          "touchend",
          "mouseup",
          "click",
        ].join(","),
      order.join(","),
    );
    assert(
      bubble.join(",") ===
        "touchstart,touchmove,touchstart,touchend,touchcancel,touchmove,touchend",
      bubble.join(","),
    );

    const start = touches[0]!;
    assert(start.type === "touchstart" && start.cancelable === true, "start flags");
    assert(start.ctrlKey === true && start.shiftKey === false, "start keys");
    assert(start.altKey === false && start.metaKey === false, "start other keys");
    assert(start.touches.length === 1 && start.changedTouches.length === 1, "start lists");
    assert(start.changedTouches[0] === start.touches[0], "start identity");
    assert(start.targetTouches[0] === start.touches[0], "start target identity");
    const point = start.touches[0]!;
    assert(point instanceof Touch, "touch class");
    assert(point.target === window.surface, "touch target");
    assert(point.identifier === 3, "id");
    assert(point.clientX === 10.5 && point.clientY === 20.25, "client");
    assert(point.screenX === 30.5 && point.screenY === 40.75, "screen");
    assert(point.pageX === 10.5 && point.pageY === 20.25, "page");
    assert(point.force === 0.4, "force");
    assert(point.radiusX === 0 && point.radiusY === 0 && point.rotationAngle === 0, "radius");
    assert(point.altitudeAngle === Math.PI / 2, "altitude");
    assert(point.azimuthAngle === 0 && point.touchType === "", "azimuth");

    const move = touches[1]!;
    assert(move.type === "touchmove" && move.cancelable === true, "move flags");
    assert(move.shiftKey === true && move.ctrlKey === false, "move keys");
    assert(move.touches.length === 1 && move.changedTouches[0] === move.touches[0], "move identity");
    assert(move.touches[0]!.clientX === 12.5 && move.touches[0]!.clientY === 21.25, "move client");
    assert(move.touches[0]!.screenX === 32.5 && move.touches[0]!.screenY === 41.75, "move screen");
    assert(move.touches[0]!.force === 0.6, "move force");

    const second = touches[2]!;
    assert(second.touches.length === 2 && second.changedTouches.length === 1, "second lists");
    assert(second.touches[0]!.identifier === 3 && second.touches[0]!.clientX === 12.5, "kept");
    assert(second.touches[1]!.identifier === 4 && second.touches[1]!.clientX === 1.5, "added");
    assert(second.changedTouches[0] === second.touches[1], "second identity");
    assert(second.targetTouches[0] === second.touches[0], "second target");
    assert(second.touches.item(0) === second.touches[0], "item");

    const end = touches[3]!;
    assert(end.type === "touchend" && end.cancelable === true, "end flags");
    assert(end.touches.length === 1 && end.targetTouches.length === 1, "end active");
    assert(end.touches[0]!.identifier === 4 && end.targetTouches[0] === end.touches[0], "end rest");
    assert(end.changedTouches.length === 1 && end.changedTouches[0]!.identifier === 3, "ended");
    assert(end.changedTouches[0]!.clientX === 13 && end.changedTouches[0]!.force === 0.1, "end point");
    assert(end.changedTouches[0] !== end.touches[0], "end separate");

    const cancel = touches[4]!;
    assert(cancel.type === "touchcancel" && cancel.cancelable === false, "cancel flags");
    assert(cancel.touches.length === 0 && cancel.targetTouches.length === 0, "cancel active");
    assert(cancel.changedTouches.length === 1 && cancel.changedTouches[0]!.identifier === 4, "cancelled");

    const unknownMove = touches[5]!;
    assert(unknownMove.touches.length === 1 && unknownMove.changedTouches[0] === unknownMove.touches[0], "unknown move");
    assert(unknownMove.touches[0]!.identifier === 11 && unknownMove.touches[0]!.clientX === 8.5, "unknown id");
    assert(unknownMove.touches[0]!.force === 0.3, "unknown force");
    const unknownEnd = touches[6]!;
    assert(unknownEnd.touches.length === 1 && unknownEnd.touches[0]!.identifier === 11, "unknown remains");
    assert(unknownEnd.changedTouches.length === 1 && unknownEnd.changedTouches[0]!.identifier === 12, "unknown end");
    assert(unknownEnd.changedTouches[0]!.clientX === 9, "unknown end point");
    assert(touches.length === 7, "no mouse touch");
  } finally {
    window.close();
  }
});

Yuzuko.test("cancelling a touch event skips the mouse events", () => {
  const window = new Yuzuko.Window({ width: 200, height: 100 });
  const order: string[] = [];
  try {
    for (const type of [
      "pointerdown",
      "gotpointercapture",
      "touchstart",
      "mousedown",
      "pointerrawupdate",
      "pointermove",
      "touchmove",
      "mousemove",
      "pointerup",
      "touchend",
      "mouseup",
      "click",
      "lostpointercapture",
    ]) {
      window.surface.addEventListener(type, (event) => {
        order.push(event.type);
      });
    }
    window.surface.addEventListener("touchstart", (event: Event) => {
      // @ts-expect-error The touch listener receives a TouchEvent.
      const touchEvent: TouchEvent = event;
      if (touchEvent.changedTouches[0]!.identifier === 5) {
        event.preventDefault();
      }
    });
    window.surface.addEventListener("touchmove", (event: Event) => {
      // @ts-expect-error The touch listener receives a TouchEvent.
      const touchEvent: TouchEvent = event;
      if (touchEvent.changedTouches[0]!.identifier === 6) {
        event.preventDefault();
      }
    });
    window.surface.addEventListener("pointerdown", (event: Event) => {
      // @ts-expect-error The pointer listener receives a PointerEvent.
      const point: PointerEvent = event;
      if (point.pointerId === 8) {
        window.setPointerCapture(8);
      }
    });
    window.surface.addEventListener("touchend", (event: Event) => {
      // @ts-expect-error The touch listener receives a TouchEvent.
      const touchEvent: TouchEvent = event;
      if (touchEvent.changedTouches[0]!.identifier === 8) {
        event.preventDefault();
      }
    });
    Yuzuko.__test.inject(window, [
      touch("pointer", "pointerdown", { pointerId: 5, x: 1, y: 1 }),
      touch("pointer", "pointerup", { pointerId: 5, x: 1, y: 1 }),
      touch("pointer", "pointerdown", { pointerId: 6, x: 2, y: 2 }),
      touch("pointer", "pointermove", { pointerId: 6, x: 3, y: 4, pressure: 0.5 }),
      touch("pointer", "pointerdown", { pointerId: 8, x: 4, y: 4 }),
      touch("pointer", "pointerup", { pointerId: 8, x: 4, y: 4 }),
    ]);
    assert(
      order.join(",") ===
        [
          "pointerdown",
          "touchstart",
          "pointerup",
          "touchend",
          "mouseup",
          "click",
          "pointerdown",
          "touchstart",
          "mousedown",
          "pointerrawupdate",
          "pointermove",
          "touchmove",
          "pointerdown",
          "gotpointercapture",
          "touchstart",
          "mousedown",
          "pointerup",
          "touchend",
          "lostpointercapture",
        ].join(","),
      order.join(","),
    );
  } finally {
    window.close();
  }
});
