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

async function rejectionName(run: () => Promise<unknown>): Promise<string> {
  try {
    await run();
    return "";
  } catch (error) {
    if (error instanceof DOMException || error instanceof TypeError) {
      return error.name;
    }
    return "";
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
    assert(
      pad.vibrationActuator.effects === pad.vibrationActuator.effects,
      "same effects",
    );
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
  assert(!("pulse" in pad.vibrationActuator), "pulse");
  assert(await pad.vibrationActuator.reset() === "complete", "reset");
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

Yuzuko.test("dual-rumble completes, preempts, and resets", async () => {
  disconnectAll();
  Yuzuko.__test.gamepad({ type: "connect", id: "Rumble", vibration: true });
  const pad = connected()[0];
  assert(pad !== undefined, "pad");
  const actuator = pad.vibrationActuator;
  try {
    assert(actuator.effects.length === 1, "effects");
    assert(actuator.effects[0] === "dual-rumble", String(actuator.effects[0]));
    assert(Object.isFrozen(actuator.effects), "frozen");
    assert(actuator.effects === actuator.effects, "same effects");

    assert(
      (await rejectionName(() =>
        actuator.playEffect("dual-rumble", { strongMagnitude: 2 }),
      )) === "TypeError",
      "magnitude",
    );
    assert(
      (await rejectionName(() => actuator.playEffect("nope"))) === "TypeError",
      "type",
    );
    assert(
      (await rejectionName(() =>
        actuator.playEffect("trigger-rumble", { leftTrigger: 2 }),
      )) === "TypeError",
      "trigger magnitude",
    );
    assert(
      (await rejectionName(() => actuator.playEffect("trigger-rumble"))) ===
        "NotSupportedError",
      "trigger",
    );
    assert(!("pulse" in actuator), "pulse");

    const kept = actuator.playEffect("dual-rumble", {
      duration: 80,
      strongMagnitude: 1,
      weakMagnitude: 0,
    });
    assert(
      (await rejectionName(() =>
        actuator.playEffect("dual-rumble", { weakMagnitude: 1.2 }),
      )) === "TypeError",
      "invalid parameters do not preempt",
    );
    assert((await kept) === "complete", "kept");

    const replaced = actuator.playEffect("dual-rumble", {
      duration: 250,
      strongMagnitude: 0.5,
      weakMagnitude: 0.25,
    });
    const next = actuator.playEffect("dual-rumble", {
      duration: 30,
      strongMagnitude: 0.1,
      weakMagnitude: 0.1,
    });
    assert((await replaced) === "preempted", "preempted");
    assert((await next) === "complete", "replacement");

    const playing = actuator.playEffect("dual-rumble", {
      duration: 400,
      strongMagnitude: 1,
      weakMagnitude: 1,
    });
    assert((await actuator.reset()) === "complete", "reset");
    assert((await playing) === "preempted", "reset preempts");

    const started = performance.now();
    assert(
      (await actuator.playEffect("dual-rumble", {
        duration: 20,
        startDelay: 40,
        strongMagnitude: 0.3,
        weakMagnitude: 0.3,
      })) === "complete",
      "delayed",
    );
    const elapsed = performance.now() - started;
    assert(elapsed >= 50, "startDelay " + elapsed);

    assert(
      (await actuator.playEffect("dual-rumble", {
        duration: 0,
        strongMagnitude: 1,
      })) === "complete",
      "zero duration",
    );

    const hanging = actuator.playEffect("dual-rumble", {
      duration: 500,
      strongMagnitude: 1,
      weakMagnitude: 1,
    });
    Yuzuko.__test.gamepad({ type: "disconnect", index: pad.index });
    assert((await hanging) === "preempted", "disconnect preempts");
    assert(
      (await rejectionName(() => actuator.playEffect("dual-rumble"))) ===
        "InvalidStateError",
      "disconnected",
    );
  } finally {
    disconnectAll();
  }
});
