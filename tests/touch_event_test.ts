function assert(condition: unknown, message: string): void {
  if (!condition) {
    throw new Error(message);
  }
}

// A `let` binding is narrowed to the literal just written. The type argument keeps the wider type.
function read<T>(value: T): T {
  return value;
}

function throwsTypeError(run: () => void): boolean {
  try {
    run();
  } catch (error) {
    return error instanceof TypeError;
  }
  return false;
}

Yuzuko.test("a synthetic TouchEvent is not cancelable", () => {
  for (const type of ["touchstart", "touchmove", "touchend", "touchcancel"]) {
    const event = new TouchEvent(type);
    assert(event instanceof TouchEvent, type);
    assert(event instanceof UIEvent, `${type} ui`);
    assert(event.cancelable === false, `${type} cancelable`);
    assert(event.bubbles === false, `${type} bubbles`);
    assert(event.touches instanceof TouchList, `${type} touches`);
    assert(event.touches.length === 0, `${type} length`);
    assert(event.targetTouches.length === 0, `${type} target`);
    assert(event.changedTouches.length === 0, `${type} changed`);
    assert(event.touches.item(0) === null, `${type} item`);
    assert(event.touches[0] === undefined, `${type} index`);
    assert(event.altKey === false && event.ctrlKey === false, `${type} keys`);
    let prevented: boolean | null = null;
    const listener = (dispatched: Event) => {
      dispatched.preventDefault();
      prevented = dispatched.defaultPrevented;
    };
    self.addEventListener(type, listener);
    try {
      assert(self.dispatchEvent(event) === true, `${type} dispatch`);
      assert(prevented === false, `${type} default`);
    } finally {
      self.removeEventListener(type, listener);
    }
  }
});

Yuzuko.test("Touch and TouchList keep the constructed contacts", () => {
  const target = new EventTarget();
  const touch = new Touch({
    identifier: 4,
    target,
    clientX: 10.5,
    clientY: 3,
    force: 0.25,
    radiusX: 2,
  });
  assert(touch instanceof Touch, "touch");
  assert(touch.identifier === 4, "id");
  assert(touch.target === target, "target");
  assert(touch.clientX === 10.5, "clientX");
  assert(touch.clientY === 3, "clientY");
  assert(touch.screenX === 0 && touch.pageX === 0, "origin");
  assert(touch.force === 0.25, "force");
  assert(touch.radiusX === 2 && touch.radiusY === 0, "radius");
  assert(touch.rotationAngle === 0, "angle");
  const iterable = {
    *[Symbol.iterator]() {
      yield touch;
    },
  };
  const event = new TouchEvent("touchstart", {
    cancelable: true,
    bubbles: true,
    // @ts-expect-error A custom iterator is a touch sequence.
    touches: iterable,
    changedTouches: [touch],
    altKey: true,
    // @ts-expect-error The view is the realm global.
    view: globalThis,
    detail: 2,
  });
  assert(event.cancelable === true, "cancelable");
  assert(event.bubbles === true, "bubbles");
  // @ts-expect-error The view is the realm global.
  assert(event.view === globalThis, "view");
  assert(event.detail === 2, "detail");
  assert(event.altKey === true, "alt");
  assert(event.shiftKey === false && event.metaKey === false, "other keys");
  assert(event.touches.length === 1, "length");
  assert(event.touches[0] === touch, "index");
  assert(event.touches.item(0) === touch, "item");
  assert(event.touches.item(1) === null, "item past");
  assert(event.touches[1] === undefined, "index past");
  assert(Object.keys(event.touches).join() === "0", "keys");
  assert(event.targetTouches.length === 0, "targetTouches");
  assert(event.changedTouches[0] === touch, "changed");
  assert(
    throwsTypeError(() => {
      event.touches[0] = touch;
    }),
    "assign",
  );
  assert(event.touches[0] === touch, "unchanged");
  assert(
    throwsTypeError(() => {
      delete event.touches[0];
    }),
    "delete",
  );
  assert(event.touches.length === 1, "still there");
  let prevented: boolean = false;
  self.addEventListener("touchstart", (dispatched: Event) => {
    dispatched.preventDefault();
    prevented = dispatched.defaultPrevented;
  });
  assert(self.dispatchEvent(event) === false, "canceled");
  let settled: boolean = read<boolean>(prevented);
  assert(settled === true, "defaultPrevented");
});

Yuzuko.test("Touch and TouchList reject the wrong values", () => {
  assert(
    throwsTypeError(() => {
      // @ts-expect-error Touch requires an init dictionary.
      new Touch();
    }),
    "no init",
  );
  assert(
    throwsTypeError(() => {
      // @ts-expect-error target is required.
      new Touch({ identifier: 1 });
    }),
    "missing target",
  );
  assert(
    throwsTypeError(() => {
      new Touch({
        identifier: 1,
        // @ts-expect-error A plain object is not an EventTarget.
        target: {},
      });
    }),
    "plain target",
  );
  assert(
    throwsTypeError(() => {
      new TouchList();
    }),
    "illegal list",
  );
  assert(
    throwsTypeError(() => {
      new TouchEvent("touchstart", {
        // @ts-expect-error An EventTarget is not a Touch.
        touches: [new EventTarget()],
      });
    }),
    "not a touch",
  );
  assert(
    throwsTypeError(() => {
      new TouchEvent("touchstart", {
        // @ts-expect-error null is not a touch sequence.
        touches: null,
      });
    }),
    "null sequence",
  );
  const touch = new Touch({ identifier: 1, target: new EventTarget() });
  let threw = false;
  try {
    new TouchEvent("touchstart", {
      // @ts-expect-error The iterator throws while the sequence is read.
      touches: {
        [Symbol.iterator]() {
          return {
            next() {
              throw new RangeError("stop");
            },
          };
        },
      },
    });
  } catch (error) {
    threw = error instanceof RangeError && error.message === "stop";
  }
  assert(threw, "iterator throw");
  assert(touch.identifier === 1, "kept");
});
