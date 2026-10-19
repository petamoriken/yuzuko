function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function connected(): Gamepad[] {
  return navigator.getGamepads().filter((pad): pad is Gamepad => pad !== null);
}

function disconnectAll(): void {
  const pads = navigator.getGamepads();
  for (let index = pads.length - 1; index >= 0; index -= 1) {
    if (pads[index]) {
      Yuzuko.__test.gamepad({ type: "disconnect", index });
    }
  }
}

Yuzuko.test("gamepads connect, update, and disconnect on globalThis", () => {
  assert(typeof navigator.getGamepads === "function", "getGamepads");
  assert(navigator instanceof Navigator, "instanceof Navigator");
  assert(
    typeof navigator.userAgent === "string" &&
      navigator.userAgent.startsWith("Yuzuko/"),
    "userAgent",
  );
  disconnectAll();
  assert(navigator.getGamepads().length === 0, "empty");

  const events: string[] = [];
  const onConnect = (event: Event) => {
    assert(event instanceof GamepadEvent, "event class");
    const gamepad = (event as GamepadEvent).gamepad;
    events.push(
      `connect:${gamepad.index}:${gamepad.id}:${gamepad.connected}:${gamepad.mapping}`,
    );
    assert(
      navigator.getGamepads()[gamepad.index] === gamepad,
      "same object during connect",
    );
  };
  addEventListener("gamepadconnected", onConnect);
  const disconnected: string[] = [];
  const onDisconnect = (event: Event) => {
    const gamepad = (event as GamepadEvent).gamepad;
    disconnected.push(
      `${gamepad.index}:${gamepad.connected}:${navigator.getGamepads()[gamepad.index] === gamepad}`,
    );
  };
  addEventListener("gamepaddisconnected", onDisconnect);
  let viaHandler = 0;
  globalThis.ongamepadconnected = () => {
    viaHandler += 1;
  };
  try {
    Yuzuko.__test.gamepad({
      type: "connect",
      id: "Pad A",
      axes: [0.25, -1, 0, 0.5],
      buttons: [{ pressed: true, touched: true, value: 1 }],
    });
    assert(events.length === 1, "one connect");
    assert(events[0] === "connect:0:Pad A:true:standard", events[0]);
    assert(viaHandler === 1, "ongamepadconnected");
    const pad = navigator.getGamepads()[0];
    assert(pad !== null, "slot 0");
    assert(pad.buttons.length === 17, "standard buttons");
    assert(pad.buttons[0].pressed && pad.buttons[0].touched, "pressed");
    assert(pad.buttons[0].value === 1, "value");
    assert(pad.buttons[1].pressed === false, "other button");
    assert(pad.axes[0] === 0.25 && pad.axes[1] === -1, "axes");
    assert(pad.axes.length === 4, "axis count");
    assert(Object.isFrozen(pad.axes) && Object.isFrozen(pad.buttons), "frozen");
    assert(pad.touches.length === 0, "touches");
    assert(pad.hand === "" && pad.pose === null, "extensions");
    assert(pad.vibrationActuator.effects.length === 0, "effects");
    assert(pad.vibrationActuator === pad.vibrationActuator, "same actuator");

    const before = pad.timestamp;
    Yuzuko.__test.gamepad({ type: "update", index: 0, axes: [1, 0, 0, 0] });
    assert(events.length === 1, "update does not fire");
    assert(pad.axes[0] === 1, "updated axis");
    assert(pad.timestamp >= before, "timestamp");

    Yuzuko.__test.gamepad({ type: "connect", id: "Pad B", mapping: "" });
    assert(navigator.getGamepads()[1]?.id === "Pad B", "second id");
    assert(navigator.getGamepads()[1]?.mapping === "", "unmapped");
    assert(navigator.getGamepads()[1]?.buttons.length === 0, "no buttons");

    Yuzuko.__test.gamepad({ type: "disconnect", index: 0 });
    assert(disconnected[0] === "0:false:true", disconnected[0]);
    assert(navigator.getGamepads()[0] === null, "slot cleared");
    assert(navigator.getGamepads()[1]?.id === "Pad B", "second remains");
    assert(pad.connected === false, "saved object");

    Yuzuko.__test.gamepad({ type: "connect", id: "Pad C" });
    assert(navigator.getGamepads()[0]?.id === "Pad C", "reused index");
    assert(navigator.getGamepads()[0] !== pad, "new object");
  } finally {
    globalThis.ongamepadconnected = null;
    removeEventListener("gamepadconnected", onConnect);
    removeEventListener("gamepaddisconnected", onDisconnect);
    disconnectAll();
  }
});

Yuzuko.test("haptic effects reject and GamepadEvent can be constructed", async () => {
  disconnectAll();
  Yuzuko.__test.gamepad({ type: "connect", id: "Haptic" });
  const pad = connected()[0];
  assert(pad !== undefined, "pad");
  let name = "";
  try {
    await pad.vibrationActuator.playEffect("dual-rumble");
  } catch (error) {
    name = error instanceof DOMException ? error.name : "";
  }
  assert(name === "NotSupportedError", name);
  await pad.vibrationActuator.reset();
  const event = new GamepadEvent("gamepadconnected", { gamepad: pad });
  assert(event.type === "gamepadconnected", "type");
  assert(event.gamepad === pad, "init gamepad");
  assert(event instanceof Event, "inherits Event");
  assert(new GamepadEvent("gamepadconnected").gamepad === null, "default null");
  let illegal = false;
  try {
    new Gamepad();
  } catch (error) {
    illegal = error instanceof TypeError;
  }
  assert(illegal, "Gamepad is not constructable");
  Yuzuko.__test.gamepad({ type: "disconnect", index: pad.index });
  let disconnectedName = "";
  try {
    await pad.vibrationActuator.playEffect("dual-rumble");
  } catch (error) {
    disconnectedName = error instanceof DOMException ? error.name : "";
  }
  assert(disconnectedName === "InvalidStateError", disconnectedName);
  disconnectAll();
});
