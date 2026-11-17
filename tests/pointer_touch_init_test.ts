function assert(condition: unknown, message: string): void {
  if (!condition) {
    throw new Error(message);
  }
}

Yuzuko.test("PointerEvent stores altitude, azimuth, and event lists", () => {
  const coalesced = new PointerEvent("pointermove", { clientX: 1.5 });
  const predicted = new PointerEvent("pointermove", { clientX: 2.5 });
  const event = new PointerEvent("pointermove", {
    altitudeAngle: 0.4,
    azimuthAngle: 1.2,
    coalescedEvents: [coalesced],
    predictedEvents: [predicted],
  });
  assert(event.altitudeAngle === 0.4, "altitude");
  assert(event.azimuthAngle === 1.2, "azimuth");
  const coalescedList = event.getCoalescedEvents();
  const predictedList = event.getPredictedEvents();
  assert(coalescedList.length === 1 && coalescedList[0] === coalesced, "coalesced");
  assert(predictedList.length === 1 && predictedList[0] === predicted, "predicted");
  const empty = new PointerEvent("pointerdown");
  assert(empty.getCoalescedEvents().length === 0, "empty coalesced");
  assert(empty.getPredictedEvents().length === 0, "empty predicted");
});

Yuzuko.test("Touch stores altitude, azimuth, and touchType", () => {
  const target = new EventTarget();
  const touch = new Touch({
    identifier: 7,
    target,
    altitudeAngle: 0.3,
    azimuthAngle: 1.1,
    touchType: "stylus",
  });
  assert(touch.altitudeAngle === 0.3, "altitude");
  assert(touch.azimuthAngle === 1.1, "azimuth");
  assert(touch.touchType === "stylus", "type");
  assert(touch.identifier === 7 && touch.target === target, "identity");
});

Yuzuko.test("an omitted Touch altitude, azimuth, and touchType use the defaults", () => {
  const target = new EventTarget();
  const omitted = new Touch({ identifier: 1, target });
  assert(omitted.altitudeAngle === Math.PI / 2, "altitude");
  assert(omitted.azimuthAngle === 0, "azimuth");
  assert(omitted.touchType === "", "type");
  // @ts-expect-error Explicit undefined uses the omitted defaults.
  const explicit = new Touch({
    identifier: 2,
    target,
    altitudeAngle: undefined,
    azimuthAngle: undefined,
    touchType: undefined,
  });
  assert(explicit.altitudeAngle === Math.PI / 2, "undefined altitude");
  assert(explicit.azimuthAngle === 0, "undefined azimuth");
  assert(explicit.touchType === "", "undefined type");
});
