function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function assertClose(actual: number, expected: number, message: string): void {
  assert(Math.abs(actual - expected) < 1e-5, `${message}: ${actual}`);
}

function assertTypeError(run: () => unknown, message: string): void {
  try {
    run();
  } catch (error) {
    assert(error instanceof TypeError, `${message}: ${String(error)}`);
    return;
  }
  throw new Error(`${message} did not throw`);
}

async function assertThrows(
  run: () => unknown,
  name: string,
): Promise<void> {
  try {
    await run();
  } catch (error) {
    assert(error instanceof DOMException, String(error));
    assert(error.name === name, error.name);
    return;
  }
  throw new Error(`${name} was not thrown`);
}

const tones = new URL("./fixtures/audio/", import.meta.url);

async function decodeTone(
  name: string,
  sampleRate: number,
): Promise<AudioBuffer> {
  const bytes = await (await fetch(new URL(name, tones))).arrayBuffer();
  const context = new AudioContext();
  try {
    const buffer = await context.decodeAudioData(bytes);
    assert(
      buffer.sampleRate === sampleRate,
      `${name} rate ${buffer.sampleRate}`,
    );
    assert(buffer.numberOfChannels >= 1, `${name} channels`);
    const channel = buffer.getChannelData(0);
    assert(channel.length > 0, `${name} length`);
    let peak = 0;
    for (let index = 0; index < channel.length; index += 1) {
      peak = Math.max(peak, Math.abs(channel[index]));
    }
    assert(peak > 0.05, `${name} peak ${peak}`);
    return buffer;
  } finally {
    await context.close();
  }
}

function sineSample(
  frequency: number,
  sampleRate: number,
  frame: number,
): number {
  return Math.sin((2 * Math.PI * frequency * frame) / sampleRate);
}

Yuzuko.test("decodeAudioData reads WAV, FLAC, MP3, Vorbis, and Opus", async () => {
  const wav = await decodeTone("tone.wav", 44100);
  assert(wav.getChannelData(0).length > 0, "wav samples");
  await decodeTone("tone.flac", 44100);
  await decodeTone("tone.mp3", 44100);
  await decodeTone("tone.ogg", 44100);
  const opus = await decodeTone("tone.opus", 48000);
  assert(
    opus.numberOfChannels === 1,
    `opus channels ${opus.numberOfChannels}`,
  );
  const channel = opus.getChannelData(0);
  // 4800 source frames, minus the 120-sample Opus pre-skip.
  assert(channel.length === 4680, `opus length ${channel.length}`);
  for (let frame = 0; frame < channel.length; frame += 1) {
    const expected = sineSample(440, 48000, frame) * 0.5;
    assert(
      Math.abs(channel[frame] - expected) < 0.02,
      `opus sample ${frame}: ${channel[frame]} expected ${expected}`,
    );
  }
});

Yuzuko.test("gain scales a buffer played into an offline context", async () => {
  const context = new OfflineAudioContext(1, 128, 48000);
  const buffer = context.createBuffer(1, 128, 48000);
  buffer.getChannelData(0).fill(1);
  const source = context.createBufferSource();
  source.buffer = buffer;
  const gain = context.createGain();
  gain.gain.value = 0.5;
  source.connect(gain).connect(context.destination);
  source.start();
  const rendered = await context.startRendering();
  const channel = rendered.getChannelData(0);
  assert(channel.length === 128, "rendered length");
  for (let index = 0; index < channel.length; index += 1) {
    assertClose(channel[index], 0.5, `gain sample ${index}`);
  }
});

Yuzuko.test("an oscillator render is periodic and not silent", async () => {
  const context = new OfflineAudioContext(1, 200, 48000);
  const oscillator = new OscillatorNode(context, {
    type: "sine",
    frequency: 480,
  });
  oscillator.connect(context.destination);
  oscillator.start();
  const rendered = await context.startRendering();
  const channel = rendered.getChannelData(0);
  let peak = 0;
  for (const sample of channel) {
    peak = Math.max(peak, Math.abs(sample));
  }
  assert(peak > 0.5, `oscillator peak ${peak}`);
  for (let index = 0; index < 100; index += 1) {
    assertClose(channel[index], channel[index + 100], `period ${index}`);
  }
});

Yuzuko.test("renderSizeHint sets the quantum and baseLatency", async () => {
  const plain = new AudioContext();
  const large = new AudioContext({ renderSizeHint: 4096 });
  const hardware = new AudioContext({ renderSizeHint: "hardware" });
  const exact = new AudioContext({ sampleRate: 48000, renderSizeHint: 13 });
  const one = new OfflineAudioContext({
    length: 8,
    sampleRate: 48000,
    renderSizeHint: 1,
  });
  const offlineHardware = new OfflineAudioContext({
    length: 8,
    sampleRate: 44100,
    renderSizeHint: "hardware",
  });
  const legacy = new OfflineAudioContext(1, 1, 48000);
  try {
    assert(plain.renderQuantumSize === 128, "default quantum");
    assert(large.renderQuantumSize === 4096, "hint 4096");
    assert(large.baseLatency > plain.baseLatency, "baseLatency grows");
    const hardwareMax = Math.floor(6 * hardware.sampleRate);
    assert(
      hardware.renderQuantumSize >= 1 &&
        hardware.renderQuantumSize <= hardwareMax,
      `hardware quantum ${hardware.renderQuantumSize}`,
    );
    assert(exact.renderQuantumSize === 13, "non power of two");
    assert(one.renderQuantumSize === 1, "quantum 1");
    assert(offlineHardware.renderQuantumSize === 128, "offline hardware");
    assert(legacy.renderQuantumSize === 128, "three argument constructor");
    await assertThrows(
      () =>
        new OfflineAudioContext({
          length: 1000,
          sampleRate: 3000,
          renderSizeHint: 18001,
        }),
      "NotSupportedError",
    );
    await assertThrows(
      () => new AudioContext({ sampleRate: 48000, renderSizeHint: 0 }),
      "NotSupportedError",
    );
  } finally {
    await plain.close();
    await large.close();
    await hardware.close();
    await exact.close();
  }
});

Yuzuko.test("AudioWorklet process uses the render quantum", async () => {
  const context = new AudioContext({ renderSizeHint: 256 });
  try {
    const script = new URL("./fixtures/audio/probe-processor.js", import.meta.url);
    await context.audioWorklet.addModule(script.href);
    let node!: AudioWorkletNode;
    const ctor = await new Promise<Record<string, number>>((resolve) => {
      node = new AudioWorkletNode(context, "probe");
      node.port.onmessage = (event) => {
        if (event.data.type === "ctor") {
          resolve(event.data);
        }
      };
    });
    assert(context.renderQuantumSize === 256, "context quantum");
    assert(ctor.renderQuantumSize === 256, "worklet quantum");
    assert(ctor.sampleRate === context.sampleRate, "worklet rate");
    assert(ctor.currentFrame === 0, "worklet frame");
    assert(ctor.currentTime === 0, "worklet time");
    const gain = node.parameters.get("gain");
    assert(gain instanceof AudioParam, "parameter");
    assertClose(gain.value, 0.25, "parameter value");

    const pong = await new Promise<{ echo: { hello: string } }>((resolve) => {
      node.port.onmessage = (event) => {
        if (event.data.type === "pong") {
          resolve(event.data);
        }
      };
      node.port.postMessage({ hello: "worklet" });
    });
    assert(pong.echo.hello === "worklet", "port round trip");

    const processed = await new Promise<{
      length: number;
      gainLength: number;
      gain0: number;
    }>((resolve) => {
      node.port.onmessage = (event) => {
        if (event.data.type === "process") {
          resolve(event.data);
        }
      };
      const source = new ConstantSourceNode(context);
      source.connect(node).connect(context.destination);
      source.start();
    });
    assert(processed.length === 256, `process length ${processed.length}`);
    assert(processed.gainLength === 256, "parameter block");
    assertClose(processed.gain0, 0.25, "parameter sample");
  } finally {
    await context.close();
  }
});

Yuzuko.test("realtime currentTime advances until close", async () => {
  const context = new AudioContext();
  try {
    assert(context.currentTime === 0, "clock starts at 0");
    const source = new ConstantSourceNode(context, { offset: 0 });
    source.connect(context.destination);
    source.start();
    await new Promise((resolve) => setTimeout(resolve, 50));
    const first = context.currentTime;
    assert(first > 0, `currentTime after start ${first}`);
    const latency = context.outputLatency;
    assert(
      latency === 0 || latency === 0.1,
      `outputLatency ${latency}`,
    );
    await new Promise((resolve) => setTimeout(resolve, 200));
    const second = context.currentTime;
    assert(second - first > 0.05, `clock delta ${second - first}`);
  } finally {
    await context.close();
  }
});

Yuzuko.test("suspend freezes the realtime clock until resume", async () => {
  const context = new AudioContext();
  try {
    const source = new ConstantSourceNode(context, { offset: 0 });
    source.connect(context.destination);
    await context.suspend();
    source.start();
    assert(context.currentTime === 0, "suspended start stays at 0");
    await new Promise((resolve) => setTimeout(resolve, 80));
    assert(context.currentTime === 0, "stays 0 while suspended");
    await context.resume();
    await new Promise((resolve) => setTimeout(resolve, 50));
    const started = context.currentTime;
    assert(started > 0, `resumed ${started}`);
    await context.suspend();
    const frozen = context.currentTime;
    await new Promise((resolve) => setTimeout(resolve, 150));
    assert(
      context.currentTime === frozen,
      `frozen ${context.currentTime} ${frozen}`,
    );
    await context.resume();
    await new Promise((resolve) => setTimeout(resolve, 150));
    assert(context.currentTime > frozen, `continued ${context.currentTime}`);
  } finally {
    await context.close();
  }
});

Yuzuko.test("offline suspend stops on a quantum boundary", async () => {
  const renderSize = 441;
  const sampleRate = 48000;
  const context = new OfflineAudioContext({
    length: renderSize * 2,
    sampleRate,
    renderSizeHint: renderSize,
  });
  const source = new ConstantSourceNode(context, { offset: 0.25 });
  source.connect(context.destination);
  source.start();
  const suspendPromise = context.suspend(renderSize / sampleRate);
  const renderingPromise = context.startRendering();
  await suspendPromise;
  assert(
    context.currentTime === renderSize / sampleRate,
    `currentTime ${context.currentTime}`,
  );
  context.resume();
  const rendered = await renderingPromise;
  assert(rendered.length === renderSize * 2, "rendered length");
  const channel = rendered.getChannelData(0);
  assertClose(channel[0], 0.25, "first sample");
  assertClose(channel[channel.length - 1], 0.25, "last sample");

  const mid = new OfflineAudioContext(1, 44100, 44100);
  const tone = new ConstantSourceNode(mid, { offset: 1 });
  tone.connect(mid.destination);
  tone.start();
  const paused = mid.suspend(0.25);
  const rendering = mid.startRendering();
  await paused;
  assert(mid.currentTime === 11136 / 44100, `rounded ${mid.currentTime}`);
  mid.resume();
  await rendering;
});

Yuzuko.test("copyToChannel keeps samples outside the written range", () => {
  const context = new OfflineAudioContext(1, 16, 48000);
  const buffer = context.createBuffer(1, 16, 48000);
  const channel = buffer.getChannelData(0);
  channel.fill(-1);
  const source = new Float32Array(10);
  for (let index = 0; index < source.length; index += 1) {
    source[index] = index + 1;
  }
  buffer.copyToChannel(source, 0);
  const head = buffer.getChannelData(0);
  for (let index = 0; index < 10; index += 1) {
    assertClose(head[index], index + 1, `head ${index}`);
  }
  for (let index = 10; index < 16; index += 1) {
    assertClose(head[index], -1, `tail ${index}`);
  }

  const offset = context.createBuffer(1, 16, 48000);
  offset.getChannelData(0).fill(-1);
  offset.copyToChannel(source, 0, 5);
  const placed = offset.getChannelData(0);
  for (let index = 0; index < 5; index += 1) {
    assertClose(placed[index], -1, `prefix ${index}`);
  }
  for (let index = 0; index < 10; index += 1) {
    assertClose(placed[index + 5], index + 1, `offset ${index}`);
  }
  assertClose(placed[15], -1, "offset tail");
  buffer.copyToChannel(source, 0, -1);
  offset.copyFromChannel(source, 0, -1);
});

Yuzuko.test("start acquires the buffer and detaches the previous view", async () => {
  const context = new OfflineAudioContext(1, 128, 8000);
  const buffer = context.createBuffer(1, 128, 8000);
  const channel = buffer.getChannelData(0);
  channel.fill(1);
  const source = context.createBufferSource();
  source.buffer = buffer;
  assert(source.playbackRate.automationRate === "k-rate", "playback k-rate");
  assert(source.detune.automationRate === "k-rate", "detune k-rate");
  await assertThrows(() => {
    source.playbackRate.automationRate = "a-rate";
  }, "InvalidStateError");
  await assertThrows(() => {
    source.detune.automationRate = "a-rate";
  }, "InvalidStateError");
  source.start();
  assert(channel.buffer.byteLength === 0, "previous view detached");
  const again = buffer.getChannelData(0);
  assert(again !== channel, "getChannelData returns a new view");
  for (let index = 0; index < again.length; index += 1) {
    assertClose(again[index], 1, `acquired sample ${index}`);
  }
  source.connect(context.destination);
  const rendered = await context.startRendering();
  const played = rendered.getChannelData(0);
  for (let index = 0; index < played.length; index += 1) {
    assertClose(played[index], 1, `played sample ${index}`);
  }

  const later = new OfflineAudioContext(1, 128, 8000);
  const held = later.createBuffer(1, 128, 8000);
  const before = held.getChannelData(0);
  before.fill(1);
  const scheduled = later.createBufferSource();
  scheduled.buffer = null;
  scheduled.start();
  scheduled.buffer = held;
  assert(before.buffer.byteLength === 0, "setter detaches the previous view");
  const readback = held.getChannelData(0);
  assertClose(readback[0], 1, "setter keeps the acquired sample");
  scheduled.connect(later.destination);
  const second = await later.startRendering();
  assertClose(second.getChannelData(0)[0], 1, "setter playback");
});

Yuzuko.test("a constant source fires ended when it is not connected", async () => {
  const sampleRate = 44100;
  const numberOfFrames = 32;
  const context = new OfflineAudioContext(1, 16 * numberOfFrames, sampleRate);
  const source = new ConstantSourceNode(context);
  let fired = false;
  const ended = new Promise<void>((resolve, reject) => {
    source.onended = () => {
      fired = true;
      resolve();
    };
    setTimeout(() => reject(new Error("ended did not fire")), 1000);
  });
  source.start();
  source.stop(numberOfFrames / context.sampleRate);
  const rendering = context.startRendering();
  assert(!fired, "ended waits for a task");
  await ended;
  assert(fired, "onended");
  await rendering;
});

Yuzuko.test(
  "interfaces without a constructor throw Illegal constructor",
  () => {
    const names = [
      "AudioParam",
      "AudioParamMap",
      "AudioNode",
      "AudioDestinationNode",
      "AudioScheduledSourceNode",
      "BaseAudioContext",
      "AudioWorklet",
    ];
    for (const name of names) {
      const ctor = (globalThis as Record<string, unknown>)[name];
      assert(typeof ctor === "function", name);
      const construct = ctor as new () => unknown;
      const call = ctor as () => unknown;
      for (const run of [() => new construct(), () => call()]) {
        try {
          run();
        } catch (error) {
          assert(error instanceof TypeError, `${name} ${String(error)}`);
          assert(
            error.message.includes("Illegal constructor"),
            `${name} ${error.message}`,
          );
          continue;
        }
        throw new Error(`${name} constructed`);
      }
    }
  },
);

Yuzuko.test("OffscreenCanvas still paints beside AudioContext", () => {
  const context = new AudioContext();
  const canvas = new OffscreenCanvas(4, 4);
  const drawing = canvas.getContext("2d");
  assert(drawing, "2d context");
  drawing.fillStyle = "#ff0000";
  drawing.fillRect(0, 0, 1, 1);
  const pixel = drawing.getImageData(0, 0, 1, 1).data;
  assert(pixel[0] === 255, `red ${pixel[0]}`);
  assert(pixel[3] === 255, `alpha ${pixel[3]}`);
  context.close();
});

function assertName(run: () => unknown, name: string): void {
  try {
    run();
  } catch (error) {
    const caught = error as { name?: string };
    assert(caught.name === name, `${name} was ${caught.name}`);
    return;
  }
  throw new Error(`${name} was not thrown`);
}

async function renderGain(schedule: (gain: AudioParam) => void): Promise<Float32Array> {
  const context = new OfflineAudioContext(1, 128, 48000);
  const source = new ConstantSourceNode(context, { offset: 1 });
  const gain = context.createGain();
  schedule(gain.gain);
  source.connect(gain).connect(context.destination);
  source.start();
  const rendered = await context.startRendering();
  return rendered.getChannelData(0);
}

Yuzuko.test("AudioParam automation changes the rendered gain", async () => {
  const linear = await renderGain((gain) => {
    gain.setValueAtTime(0, 0).linearRampToValueAtTime(1, 128 / 48000);
  });
  assertClose(linear[0], 0, "linear start");
  assert(linear[127] > 0.9, `linear end ${linear[127]}`);

  const exponential = await renderGain((gain) => {
    gain.setValueAtTime(0.25, 0);
    gain.exponentialRampToValueAtTime(1, 128 / 48000);
  });
  assertClose(exponential[0], 0.25, "exponential start");
  assert(exponential[127] > 0.9, `exponential end ${exponential[127]}`);

  const target = await renderGain((gain) => {
    gain.setValueAtTime(0, 0);
    gain.setTargetAtTime(1, 0, 128 / 48000);
  });
  assertClose(target[0], 0, "target start");
  assertClose(target[127], 1 - Math.exp(-127 / 128), "target end");

  const curve = await renderGain((gain) => {
    gain.setValueCurveAtTime(new Float32Array([0, 1]), 0, 128 / 48000);
  });
  assertClose(curve[0], 0, "curve start");
  assert(curve[127] > 0.9, `curve end ${curve[127]}`);

  const cancelled = await renderGain((gain) => {
    gain.setValueAtTime(0, 0);
    gain.cancelScheduledValues(0);
  });
  assertClose(cancelled[0], 1, "cancel restores the intrinsic value");
  assertClose(cancelled[127], 1, "cancel holds the intrinsic value");
});

Yuzuko.test("cancelAndHoldAtTime keeps the partial ramp", async () => {
  const context = new OfflineAudioContext(1, 256, 48000);
  const source = new ConstantSourceNode(context, { offset: 1 });
  const gain = context.createGain();
  gain.gain
    .setValueAtTime(0, 0)
    .linearRampToValueAtTime(1, 256 / 48000)
    .cancelAndHoldAtTime(128 / 48000);
  source.connect(gain).connect(context.destination);
  source.start();
  const rendered = await context.startRendering();
  const channel = rendered.getChannelData(0);
  assertClose(channel[64], 0.25, "partial ramp");
  assertClose(channel[200], 0.5, "held ramp");
});

Yuzuko.test("a buffer source fires ended after the buffer plays", async () => {
  const context = new OfflineAudioContext(1, 32, 44100);
  const buffer = context.createBuffer(1, 32, 44100);
  buffer.getChannelData(0).fill(1);
  const source = context.createBufferSource();
  source.buffer = buffer;
  source.connect(context.destination);
  let fired = false;
  const ended = new Promise<void>((resolve) => {
    source.onended = () => {
      fired = true;
      resolve();
    };
  });
  source.start(0);
  const rendering = context.startRendering();
  assert(!fired, "ended waits for a task");
  await ended;
  await rendering;
});

Yuzuko.test("a ramp stays when setValueAtTime shares its end time", async () => {
  const context = new OfflineAudioContext(1, 256, 48000);
  const source = new ConstantSourceNode(context, { offset: 1 });
  const gain = context.createGain();
  const end = 128 / 48000;
  gain.gain.setValueAtTime(0, 0);
  gain.gain.linearRampToValueAtTime(1, end);
  gain.gain.setValueAtTime(0, end);
  source.connect(gain).connect(context.destination);
  source.start();
  const rendered = await context.startRendering();
  const channel = rendered.getChannelData(0);
  assertClose(channel[0], 0, "ramp start");
  assertClose(channel[127], 127 / 128, "sample before the shared time");
  assertClose(channel[128], 0, "setValue at the ramp end");
  assertClose(channel[255], 0, "held setValue");
});

Yuzuko.test("an AudioParam input sums output 0 into the parameter", async () => {
  const context = new OfflineAudioContext(1, 128, 48000);
  const gain = context.createGain();
  gain.gain.value = 0;
  const audio = new ConstantSourceNode(context, { offset: 1 });
  const control = new ConstantSourceNode(context, { offset: 0.25 });
  const connected = control.connect(gain.gain);
  assert(connected === undefined, "connect returns undefined");
  audio.connect(gain).connect(context.destination);
  audio.start();
  control.start();
  const rendered = await context.startRendering();
  const channel = rendered.getChannelData(0);
  assertClose(channel[0], 0.25, "param input");
  assertClose(channel[127], 0.25, "param input end");
});

Yuzuko.test("automation methods reject an incomplete schedule", () => {
  const context = new OfflineAudioContext(1, 8, 48000);
  const gain = context.createGain().gain;
  assertName(
    () => gain.linearRampToValueAtTime(1, 1),
    "InvalidStateError",
  );
  gain.setValueAtTime(1, 0);
  assertName(
    () => gain.exponentialRampToValueAtTime(0, 1),
    "RangeError",
  );
  assertName(() => gain.setTargetAtTime(1, 0, 0), "RangeError");
  assertName(
    () => gain.setValueCurveAtTime(new Float32Array([1]), 0, 1),
    "InvalidStateError",
  );
});

Yuzuko.test("audioOutputDevices lists sink names", async () => {
  assert(Yuzuko.audioOutputDevices.length === 0, "length");
  const names = Yuzuko.audioOutputDevices();
  assert(Array.isArray(names), "array");
  const seen = new Set<string>();
  for (const name of names) {
    assert(typeof name === "string" && name.length > 0, `name ${name}`);
    assert(!seen.has(name), `duplicate ${name}`);
    seen.add(name);
    const context = new AudioContext({ sinkId: name });
    try {
      assert(context.sinkId === name, `accepted ${name}`);
    } finally {
      await context.close();
    }
  }
  assert(Yuzuko.audioOutputDevices() !== names, "fresh array");
  const worker = new Worker(
    new URL("./workers/audio_outputs.js", import.meta.url).href,
  );
  const probe = await new Promise<{ fn: boolean; names: string[] }>(
    (resolve, reject) => {
      worker.onmessage = (event: MessageEvent) => {
        resolve(event.data as { fn: boolean; names: string[] });
      };
      worker.onerror = (event: ErrorEvent) => {
        reject(new Error(event.message || "worker error"));
      };
    },
  );
  worker.terminate();
  assert(probe.fn, "worker function");
  assert(probe.names.join("\0") === names.join("\0"), "worker names");
});

Yuzuko.test("sinkId names an output device", async () => {
  const context = new AudioContext({ sinkId: "" });
  try {
    assert(context.sinkId === "", "default sink");
  } finally {
    await context.close();
  }
  await assertThrows(
    () => new AudioContext({ sinkId: "yuzuko-missing-audio-sink" }),
    "NotFoundError",
  );
});

function sinkType(sink: unknown): string {
  if (typeof sink === "string") {
    return sink;
  }
  if (sink !== null && typeof sink === "object" && "type" in sink) {
    return String((sink as { type: unknown }).type);
  }
  throw new Error(`sink ${String(sink)}`);
}

async function assertRejected(
  promise: Promise<unknown>,
  name: string,
): Promise<void> {
  try {
    await promise;
  } catch (error) {
    const caught = error as { name?: string };
    assert(caught.name === name, `${name} was ${caught.name}`);
    if (name !== "TypeError") {
      assert(error instanceof DOMException, String(error));
    }
    return;
  }
  throw new Error(`${name} was not thrown`);
}

Yuzuko.test("setSinkId selects the default device or no device", async () => {
  const context = new AudioContext({ sinkId: "" });
  try {
    const selected = context.setSinkId("");
    assert(selected instanceof Promise, "returns a promise");
    await selected;
    assert(context.sinkId === "", "default sink");
    await context.setSinkId({ type: "none" });
    assert(sinkType(context.sinkId) === "none", "none type");
    await context.setSinkId({ type: "none" });
    assert(sinkType(context.sinkId) === "none", "same none");
    await assertRejected(
      context.setSinkId("yuzuko-missing-audio-sink"),
      "NotFoundError",
    );
    assert(sinkType(context.sinkId) === "none", "unknown keeps the sink");
    await assertRejected(
      context.setSinkId({ type: "something_else" } as AudioSinkOptions),
      "TypeError",
    );
    assert(sinkType(context.sinkId) === "none", "bad type keeps the sink");
  } finally {
    await context.close();
  }
  await assertRejected(context.setSinkId(""), "InvalidStateError");
  const silent = new AudioContext({ sinkId: { type: "none" } });
  try {
    assert(sinkType(silent.sinkId) === "none", "constructed none");
  } finally {
    await silent.close();
  }
  assertName(
    () => new AudioContext({
      sinkId: { type: "something_else" } as AudioSinkOptions,
    }),
    "TypeError",
  );
  await assertThrows(
    () => new AudioContext({ sinkId: "yuzuko-missing-audio-sink" }),
    "NotFoundError",
  );
});

function constant(context: BaseAudioContext, offset: number): ConstantSourceNode {
  return new ConstantSourceNode(context, { offset });
}

Yuzuko.test("delay of zero is identity and ten samples land the impulse", async () => {
  const identity = new OfflineAudioContext(1, 128, 48000);
  const impulse = identity.createBuffer(1, 128, 48000);
  impulse.getChannelData(0)[0] = 1;
  const source = identity.createBufferSource();
  source.buffer = impulse;
  const delay = new DelayNode(identity, { delayTime: 0, maxDelayTime: 1 });
  source.connect(delay).connect(identity.destination);
  source.start();
  const same = await identity.startRendering();
  assertClose(same.getChannelData(0)[0], 1, "delay zero");
  assertClose(same.getChannelData(0)[1], 0, "delay zero tail");

  const context = new OfflineAudioContext(1, 128, 48000);
  const shifted = context.createBuffer(1, 128, 48000);
  shifted.getChannelData(0)[0] = 1;
  const shiftedSource = context.createBufferSource();
  shiftedSource.buffer = shifted;
  const ten = context.createDelay(1);
  ten.delayTime.value = 10 / 48000;
  shiftedSource.connect(ten).connect(context.destination);
  shiftedSource.start();
  const rendered = await context.startRendering();
  const channel = rendered.getChannelData(0);
  assertClose(channel[9], 0, "sample before the delay");
  assertClose(channel[10], 1, "sample 10");
  await assertThrows(
    () => new DelayNode(context, { maxDelayTime: 0 }),
    "NotSupportedError",
  );
  await assertThrows(() => context.createDelay(180), "NotSupportedError");
});

Yuzuko.test("stereo pan places a mono source", async () => {
  async function pan(value: number): Promise<Float32Array[]> {
    const context = new OfflineAudioContext(2, 128, 48000);
    const source = constant(context, 1);
    const panner = new StereoPannerNode(context, { pan: value });
    source.connect(panner).connect(context.destination);
    source.start();
    const rendered = await context.startRendering();
    return [rendered.getChannelData(0), rendered.getChannelData(1)];
  }
  const hardLeft = await pan(-1);
  assertClose(hardLeft[0][0], 1, "hard left");
  assertClose(hardLeft[1][0], 0, "hard left right");
  const center = await pan(0);
  const gain = Math.SQRT1_2;
  assertClose(center[0][64], gain, "center left");
  assertClose(center[1][64], gain, "center right");
});

Yuzuko.test("splitter and merger route and downmix channels", async () => {
  const context = new OfflineAudioContext(2, 128, 48000);
  const buffer = context.createBuffer(2, 128, 48000);
  buffer.getChannelData(0).fill(0.25);
  buffer.getChannelData(1).fill(0.75);
  const source = context.createBufferSource();
  source.buffer = buffer;
  const splitter = new ChannelSplitterNode(context, { numberOfOutputs: 2 });
  const merger = new ChannelMergerNode(context, { numberOfInputs: 2 });
  source.connect(splitter);
  splitter.connect(merger, 0, 0);
  splitter.connect(merger, 1, 1);
  merger.connect(context.destination);
  source.start();
  const routed = await context.startRendering();
  assertClose(routed.getChannelData(0)[0], 0.25, "splitter left");
  assertClose(routed.getChannelData(1)[0], 0.75, "splitter right");

  const down = new OfflineAudioContext(1, 128, 48000);
  const stereo = down.createBuffer(2, 128, 48000);
  stereo.getChannelData(0).fill(0.25);
  stereo.getChannelData(1).fill(0.75);
  const downSource = down.createBufferSource();
  downSource.buffer = stereo;
  const one = new ChannelMergerNode(down, { numberOfInputs: 1 });
  downSource.connect(one).connect(down.destination);
  downSource.start();
  const mixed = await down.startRendering();
  assertClose(mixed.getChannelData(0)[0], 0.5, "merger average");

  const wide = new OfflineAudioContext(1, 128, 48000);
  const triple = wide.createBuffer(3, 128, 48000);
  triple.getChannelData(0).fill(1);
  triple.getChannelData(1).fill(2);
  triple.getChannelData(2).fill(3);
  const wideSource = wide.createBufferSource();
  wideSource.buffer = triple;
  const merged = new ChannelMergerNode(wide, { numberOfInputs: 1 });
  wideSource.connect(merged).connect(wide.destination);
  wideSource.start();
  const first = await wide.startRendering();
  assertClose(first.getChannelData(0)[0], 1, "merger keeps the first channel");

  const locked = new OfflineAudioContext(1, 8, 48000);
  await assertThrows(() => {
    new ChannelMergerNode(locked).channelCount = 2;
  }, "InvalidStateError");
  await assertThrows(
    () => new ChannelSplitterNode(locked, { numberOfOutputs: 0 }),
    "IndexSizeError",
  );
  await assertThrows(
    () => new ChannelSplitterNode(locked, { numberOfOutputs: 33 }),
    "IndexSizeError",
  );
});

Yuzuko.test("waveshaper maps the curve", async () => {
  const context = new OfflineAudioContext(1, 128, 48000);
  const buffer = context.createBuffer(1, 128, 48000);
  const samples = buffer.getChannelData(0);
  samples[0] = -1;
  samples[1] = 0;
  samples[2] = 1;
  const source = context.createBufferSource();
  source.buffer = buffer;
  const shaper = new WaveShaperNode(context, { curve: new Float32Array([0, 1]) });
  source.connect(shaper).connect(context.destination);
  source.start();
  const rendered = await context.startRendering();
  const channel = rendered.getChannelData(0);
  assertClose(channel[0], 0, "curve start");
  assertClose(channel[1], 0.5, "curve middle");
  assertClose(channel[2], 1, "curve end");
  await assertThrows(() => {
    shaper.curve = new Float32Array([1]);
  }, "InvalidStateError");
  shaper.oversample = "invalid" as OverSampleType;
  assert(shaper.oversample === "none", "bad oversample ignored");
  assertName(
    () =>
      new WaveShaperNode(context, {
        oversample: "invalid" as OverSampleType,
      }),
    "TypeError",
  );
});

Yuzuko.test("biquad frequency zero and the lowpass response", async () => {
  async function filtered(type: BiquadFilterType): Promise<number> {
    const context = new OfflineAudioContext(1, 128, 48000);
    const source = constant(context, 0.5);
    const filter = new BiquadFilterNode(context, { type, frequency: 0 });
    source.connect(filter).connect(context.destination);
    source.start();
    const rendered = await context.startRendering();
    return rendered.getChannelData(0)[64];
  }
  assertClose(await filtered("lowpass"), 0, "lowpass dc");
  assertClose(await filtered("highpass"), 0.5, "highpass dc");

  const context = new OfflineAudioContext(1, 8, 48000);
  const filter = context.createBiquadFilter();
  assert(filter.context === context, "node context");
  assert(context.destination.context === context, "destination context");
  filter.type = "foobar" as BiquadFilterType;
  assert(filter.type === "lowpass", "bad type ignored");
  assertName(
    () => new BiquadFilterNode(context, { type: "foobar" as BiquadFilterType }),
    "TypeError",
  );
  assertName(
    () =>
      new PeriodicWave(context, {
        real: new Float32Array(512),
        imag: new Float32Array(4),
      }),
    "IndexSizeError",
  );
  const cosine = new PeriodicWave(context, {
    real: new Float32Array([0, 1]),
  });
  const sine = new PeriodicWave(context, {
    imag: new Float32Array([0, 1]),
  });
  assert(cosine instanceof PeriodicWave, "omitted imag");
  assert(sine instanceof PeriodicWave, "omitted real");
  const frequency = new Float32Array([0, 24000]);
  const magnitude = new Float32Array(2);
  const phase = new Float32Array(2);
  filter.getFrequencyResponse(frequency, magnitude, phase);
  assertClose(magnitude[0], 1, "response at 0 Hz");
  assert(Math.abs(magnitude[1]) < 1e-3, `nyquist ${magnitude[1]}`);
});

Yuzuko.test("IIR halves a constant and rejects coefficients", async () => {
  const context = new OfflineAudioContext(1, 128, 48000);
  const source = constant(context, 1);
  const filter = new IIRFilterNode(context, {
    feedforward: [0.5],
    feedback: [1],
  });
  source.connect(filter).connect(context.destination);
  source.start();
  const rendered = await context.startRendering();
  assertClose(rendered.getChannelData(0)[0], 0.5, "iir half");
  assertClose(rendered.getChannelData(0)[32], 0.5, "iir holds");
  await assertThrows(
    () => new IIRFilterNode(context, { feedforward: [], feedback: [1] }),
    "NotSupportedError",
  );
  await assertThrows(
    () =>
      context.createIIRFilter(new Array(21).fill(1), [1]),
    "NotSupportedError",
  );
  await assertThrows(
    () => new IIRFilterNode(context, { feedforward: [0], feedback: [1] }),
    "InvalidStateError",
  );
  await assertThrows(
    () => new IIRFilterNode(context, { feedforward: [1], feedback: [0] }),
    "InvalidStateError",
  );
  assertName(
    () =>
      new IIRFilterNode(context, { feedback: [1] } as IIRFilterOptions),
    "TypeError",
  );
  assertName(
    () =>
      new IIRFilterNode(context, { feedforward: [1] } as IIRFilterOptions),
    "TypeError",
  );
});

Yuzuko.test("the compressor identity, hard knee, and automatic makeup", async () => {
  const identity = new OfflineAudioContext(1, 128, 48000);
  const quiet = constant(identity, 0.5);
  const flat = new DynamicsCompressorNode(identity, {
    threshold: 0,
    knee: 0,
    ratio: 1,
    makeupGain: 0,
  });
  quiet.connect(flat).connect(identity.destination);
  quiet.start();
  const plain = await identity.startRendering();
  assertClose(plain.getChannelData(0)[64], 0.5, "ratio one");

  const context = new OfflineAudioContext(1, 128, 48000);
  const source = constant(context, 1);
  const compressor = new DynamicsCompressorNode(context, {
    threshold: -24,
    knee: 0,
    ratio: 20,
    attack: 0,
    release: 0,
    makeupGain: 0,
  });
  source.connect(compressor).connect(context.destination);
  source.start();
  const rendered = await context.startRendering();
  const channel = rendered.getChannelData(0);
  const threshold = 10 ** (-24 / 20);
  const expected = threshold + (1 - threshold) / 20;
  assertClose(channel[0], expected, "hard knee start");
  assertClose(channel[127], expected, "hard knee end");

  const automatic = new DynamicsCompressorNode(context);
  assert(automatic.threshold.automationRate === "k-rate", "k-rate");
  assert(
    automatic.attack.value === Math.fround(0.003),
    `attack ${automatic.attack.value}`,
  );
  await assertThrows(() => {
    automatic.makeupGain.value = 1;
  }, "InvalidStateError");
  await assertThrows(
    () => automatic.makeupGain.setValueAtTime(1, 0),
    "InvalidStateError",
  );
  await assertThrows(() => {
    automatic.threshold.automationRate = "a-rate";
  }, "InvalidStateError");
});

Yuzuko.test("convolution reproduces a short impulse response", async () => {
  const context = new OfflineAudioContext(1, 128, 48000);
  const impulseResponse = context.createBuffer(1, 3, 48000);
  const kernel = impulseResponse.getChannelData(0);
  kernel[0] = 1;
  kernel[1] = 0;
  kernel[2] = 0.5;
  const convolver = new ConvolverNode(context, {
    buffer: impulseResponse,
    disableNormalization: true,
  });
  assert(convolver.buffer === impulseResponse, "buffer identity");
  const impulse = context.createBuffer(1, 128, 48000);
  impulse.getChannelData(0)[0] = 1;
  const source = context.createBufferSource();
  source.buffer = impulse;
  source.connect(convolver).connect(context.destination);
  source.start();
  const rendered = await context.startRendering();
  const channel = rendered.getChannelData(0);
  assertClose(channel[0], 1, "impulse");
  assertClose(channel[1], 0, "gap");
  assertClose(channel[2], 0.5, "tail");
  const wide = context.createBuffer(3, 4, 48000);
  await assertThrows(() => {
    convolver.buffer = wide;
  }, "NotSupportedError");
  assert(convolver.buffer === impulseResponse, "rejected buffer stays");
});

Yuzuko.test("an analyser reads a side chain", async () => {
  const context = new OfflineAudioContext(1, 128, 48000);
  const source = constant(context, 0.5);
  const analyser = new AnalyserNode(context);
  source.connect(context.destination);
  source.connect(analyser);
  source.start();
  await context.startRendering();
  assert(analyser.frequencyBinCount === 1024, "bins");
  const time = new Float32Array(analyser.fftSize);
  analyser.getFloatTimeDomainData(time);
  assertClose(time[0], 0, "unfilled prefix");
  assertClose(time[analyser.fftSize - 1], 0.5, "newest sample");
  assertClose(time[analyser.fftSize - 128], 0.5, "quantum start");
  const bytes = new Uint8Array(analyser.fftSize);
  analyser.getByteTimeDomainData(bytes);
  assert(bytes[0] === 128, `silence byte ${bytes[0]}`);
  await assertThrows(() => {
    analyser.fftSize = 30;
  }, "IndexSizeError");
  await assertThrows(() => {
    analyser.minDecibels = analyser.maxDecibels;
  }, "IndexSizeError");
  await assertThrows(() => {
    analyser.smoothingTimeConstant = 1.5;
  }, "IndexSizeError");
});

Yuzuko.test("equalpower panning follows the listener", async () => {
  async function render(
    place: (panner: PannerNode, listener: AudioListener) => void,
  ): Promise<Float32Array[]> {
    const context = new OfflineAudioContext(2, 128, 48000);
    const source = constant(context, 1);
    const panner = new PannerNode(context);
    place(panner, context.listener);
    source.connect(panner).connect(context.destination);
    source.start();
    const rendered = await context.startRendering();
    return [rendered.getChannelData(0), rendered.getChannelData(1)];
  }
  const right = await render((panner) => {
    panner.positionX.value = 1;
  });
  assertClose(right[0][64], 0, "right left");
  assertClose(right[1][64], 1, "right right");
  const center = await render((panner, listener) => {
    panner.positionX.value = 1;
    listener.setPosition(1, 0, 0);
  });
  const gain = Math.SQRT1_2;
  assertClose(center[0][32], gain, "same point left");
  assertClose(center[1][32], gain, "same point right");
  const left = await render((_panner, listener) => {
    listener.setPosition(1, 0, 0);
  });
  assertClose(left[0][32], 1, "listener left");
  assertClose(left[1][32], 0, "listener right");

  const context = new OfflineAudioContext(1, 8, 48000);
  assert(context.listener === context.listener, "listener identity");
  assertClose(context.listener.forwardZ.value, -1, "forward");
  assertName(() => new AudioListener(), "TypeError");
  const constructed = new PannerNode(context, { panningModel: "HRTF" });
  assert(constructed.panningModel === "HRTF", "constructed HRTF");
  const panner = new PannerNode(context);
  assert(panner.panningModel === "equalpower", "default model");
  panner.panningModel = "HRTF";
  assert(panner.panningModel === "HRTF", "assigned HRTF");
  panner.panningModel = "equalpower";
  assert(panner.panningModel === "equalpower", "restored model");
  assertName(() => {
    panner.panningModel = "room";
  }, "TypeError");
  assert(panner.panningModel === "equalpower", "invalid model stays");
  assertName(() => {
    panner.refDistance = -1;
  }, "RangeError");
  await assertThrows(() => {
    panner.coneOuterGain = -1;
  }, "InvalidStateError");
  await assertThrows(
    () => new PannerNode(context, { coneOuterGain: 1.1 }),
    "InvalidStateError",
  );
  assert(panner.coneOuterGain === 0, "cone stays 0");
  panner.channelCountMode = "foobar" as ChannelCountMode;
  assert(panner.channelCountMode === "clamped-max", "bad mode ignored");
  for (const node of [
    panner,
    new StereoPannerNode(context),
    new ConvolverNode(context),
    new DynamicsCompressorNode(context),
  ]) {
    await assertThrows(() => {
      node.channelCount = 3;
    }, "NotSupportedError");
  }
});

Yuzuko.test("HRTF delays one ear of a side click", async () => {
  const context = new OfflineAudioContext(2, 256, 48000);
  const impulse = context.createBuffer(1, 256, 48000);
  impulse.getChannelData(0)[127] = 1;
  const source = context.createBufferSource();
  source.buffer = impulse;
  const panner = new PannerNode(context, { panningModel: "HRTF" });
  panner.positionX.value = 1;
  source.connect(panner).connect(context.destination);
  source.start();
  const rendered = await context.startRendering();
  const peakAt = (channel: Float32Array): number => {
    let index = 0;
    let peak = 0;
    for (let frame = 0; frame < channel.length; frame += 1) {
      const value = Math.abs(channel[frame]);
      if (value > peak) {
        peak = value;
        index = frame;
      }
    }
    assert(peak > 0.05, `peak ${peak}`);
    return index;
  };
  const left = peakAt(rendered.getChannelData(0));
  const right = peakAt(rendered.getChannelData(1));
  assert(left !== right, `peaks ${left} and ${right}`);
});

async function takeAudioData(): Promise<AudioData> {
  const root = new URL("./fixtures/webcodecs/", import.meta.url);
  const load = async (name: string) =>
    new Uint8Array(await (await fetch(new URL(name, root))).arrayBuffer());
  const packets = await load("aac.frames");
  const parts: AudioData[] = [];
  let failed = "";
  const decoder = new AudioDecoder({
    output: (data) => {
      parts.push(data);
    },
    error: (error) => {
      failed = error instanceof DOMException ? error.name : String(error);
    },
  });
  decoder.configure({
    codec: "mp4a.40.2",
    description: await load("aac.asc"),
    sampleRate: 48000,
    numberOfChannels: 1,
  });
  const view = new DataView(
    packets.buffer,
    packets.byteOffset,
    packets.byteLength,
  );
  let offset = 0;
  let timestamp = 0;
  while (offset + 4 <= packets.byteLength) {
    const length = view.getUint32(offset);
    offset += 4;
    decoder.decode(
      new EncodedAudioChunk({
        type: "key",
        timestamp,
        data: packets.subarray(offset, offset + length),
      }),
    );
    offset += length;
    timestamp += 20_000;
  }
  await decoder.flush();
  assert(failed === "", failed);
  decoder.close();
  assert(parts.length > 0, "no audio");
  const first = parts[0];
  assert(first.numberOfFrames > 0, "empty audio");
  for (const extra of parts.slice(1)) {
    extra.close();
  }
  return first;
}

async function takeFrame(): Promise<VideoFrame> {
  const url = new URL("./fixtures/images/corners.png", import.meta.url);
  const encoded = new Uint8Array(await (await fetch(url)).arrayBuffer());
  const decoder = new ImageDecoder({ type: "image/png", data: encoded });
  const result = await decoder.decode();
  decoder.close();
  return result.image;
}

Yuzuko.test("an AudioWorklet port transfers AudioData and VideoFrame", async () => {
  const context = new AudioContext();
  try {
    const script = new URL("./fixtures/audio/clone-processor.js", import.meta.url);
    await context.audioWorklet.addModule(script.href);
    const pending: unknown[] = [];
    let wake = () => {};
    const node = new AudioWorkletNode(context, "clone");
    node.port.onmessage = (event: MessageEvent) => {
      pending.push(event.data);
      wake();
    };
    const next = () => {
      const queued = pending.shift();
      if (queued !== undefined) {
        return Promise.resolve(queued);
      }
      return new Promise((resolve) => {
        wake = () => {
          resolve(pending.shift());
        };
      });
    };
    const ready = await next() as { kind: string };
    assert(ready.kind === "ready", ready.kind);

    const audio = await takeAudioData();
    const audioTimestamp = audio.timestamp;
    const local = new Float32Array(audio.numberOfFrames);
    audio.copyTo(local, { planeIndex: 0 });
    node.port.postMessage({ kind: "audio", value: audio });
    const copied = await next() as {
      kind: string;
      frames: number;
      channels: number;
      rate: number;
      sample: number;
      hasAudioData: boolean;
      message?: string;
    };
    assert(copied.kind === "audio", copied.message || copied.kind);
    assert(copied.frames === audio.numberOfFrames, "frames");
    assert(copied.channels === audio.numberOfChannels, "channels");
    assert(copied.rate === audio.sampleRate, "rate");
    assert(copied.sample === local[0], "sample");
    assert(copied.hasAudioData === false, "worklet AudioData");
    assert(audio.numberOfFrames > 0, "source stays open");

    node.port.postMessage({ kind: "audio", value: audio }, [audio]);
    const moved = await next() as { kind: string; sample: number; message?: string };
    assert(moved.kind === "audio", moved.message || moved.kind);
    assert(moved.sample === local[0], "transferred sample");
    assert(audio.format === null, "closed format");
    assert(audio.numberOfFrames === 0, "closed frames");
    assert(audio.sampleRate === 0, "closed rate");
    assert(audio.numberOfChannels === 0, "closed channels");
    assert(audio.duration === 0, "closed duration");
    assert(audio.timestamp === audioTimestamp, "closed timestamp");

    const frame = await takeFrame();
    const width = frame.displayWidth;
    const height = frame.displayHeight;
    node.port.postMessage({ kind: "frame", value: frame }, [frame]);
    const painted = await next() as {
      kind: string;
      width: number;
      height: number;
      hasVideoFrame: boolean;
      message?: string;
    };
    assert(painted.kind === "frame", painted.message || painted.kind);
    assert(painted.width === width, "frame width");
    assert(painted.height === height, "frame height");
    assert(painted.hasVideoFrame === false, "worklet VideoFrame");
    assert(frame.format === null, "closed format");
    assert(frame.codedWidth === 0, "closed coded");
    assert(frame.displayWidth === 0, "closed display");
  } finally {
    await context.close();
  }
});

Yuzuko.test("offline rendering dispatches complete before resolving", async () => {
  const context = new OfflineAudioContext(1, 128, 48000);
  const buffer = context.createBuffer(1, 128, 48000);
  buffer.getChannelData(0).fill(0.25);
  const source = context.createBufferSource();
  source.buffer = buffer;
  source.connect(context.destination);
  source.start();
  let eventBuffer: AudioBuffer | null = null;
  let sawEvent = false;
  context.oncomplete = (event) => {
    sawEvent = true;
    eventBuffer = event.renderedBuffer;
    assert(event.isTrusted, "trusted");
    assert(event.bubbles === false, "bubbles");
  };
  const rendered = await context.startRendering();
  assert(sawEvent, "complete ran before the promise resolved");
  assert(eventBuffer === rendered, "same buffer");
  assertClose(rendered.getChannelData(0)[0], 0.25, "sample");
});

Yuzuko.test("a cycle without a DelayNode is silent", async () => {
  const context = new OfflineAudioContext(1, 512, 48000);
  const oscillator = new OscillatorNode(context);
  const feedback = new GainNode(context);
  oscillator.connect(feedback).connect(feedback).connect(context.destination);
  oscillator.start();
  const rendered = await context.startRendering();
  const channel = rendered.getChannelData(0);
  for (let index = 0; index < channel.length; index += 1) {
    assert(channel[index] === 0, `cycle sample ${index}`);
  }
});

Yuzuko.test("a DelayNode in a cycle delays by one quantum", async () => {
  const context = new OfflineAudioContext(1, 512, 48000);
  const buffer = context.createBuffer(1, 1, 48000);
  buffer.getChannelData(0)[0] = 1;
  const impulse = new AudioBufferSourceNode(context, { buffer });
  impulse.start();
  const delay = new DelayNode(context, { delayTime: 64 / 48000 });
  const feedback = new GainNode(context);
  impulse.connect(feedback).connect(delay).connect(feedback).connect(
    context.destination,
  );
  const rendered = await context.startRendering();
  const channel = rendered.getChannelData(0);
  for (let index = 0; index < channel.length; index += 1) {
    const expected = index % 128 === 0 ? 1 : 0;
    assertClose(channel[index], expected, `delay sample ${index}`);
  }
});

Yuzuko.test("AudioBufferSourceNode accepts one non-null buffer", async () => {
  const context = new AudioContext();
  try {
    const source = new AudioBufferSourceNode(context);
    source.buffer = null;
    const buffer = new AudioBuffer({
      length: 128,
      sampleRate: context.sampleRate,
    });
    source.buffer = buffer;
    await assertThrows(() => {
      source.buffer = new AudioBuffer({
        length: 128,
        sampleRate: context.sampleRate,
      });
    }, "InvalidStateError");
    source.buffer = null;
    await assertThrows(() => {
      source.buffer = buffer;
    }, "InvalidStateError");
    const played = new AudioBufferSourceNode(context);
    played.buffer = null;
    let endedAt = -1;
    played.onended = () => {
      endedAt = context.currentTime;
    };
    played.start(1);
    assert(endedAt === 0, `ended time ${endedAt}`);
  } finally {
    await context.close();
  }
});

Yuzuko.test("setPosition rejects a coordinate outside the float range", () => {
  const context = new OfflineAudioContext(1, 1, 24000);
  const panner = context.createPanner();
  const huge = 2 * 3.40282e38;
  assertTypeError(() => panner.setPosition(huge, 0, 0), "x");
  assert(panner.positionX.value === 0, "positionX");
  assertTypeError(() => panner.setPosition(0, -huge, 0), "y");
  assert(panner.positionX.value === 0, "positionX after y");
  assertTypeError(() => panner.setOrientation(-huge, 0, 0), "orientation");
  assert(panner.orientationX.value === 1, "orientationX");
  assertTypeError(() => context.listener.setPosition(huge, 0, 0), "listener");
  assertTypeError(
    () => context.listener.setOrientation(-huge, 0, 0, 0, 1, 0),
    "listener orientation",
  );
});

Yuzuko.test("createIIRFilter accepts a Float64Array", async () => {
  const context = new OfflineAudioContext(2, 256, 24000);
  const buffer = context.createBuffer(1, 1, 24000);
  buffer.getChannelData(0)[0] = 1;
  const source = context.createBufferSource();
  source.buffer = buffer;
  const normalized = context.createIIRFilter([1], [1, 0.9]);
  const scaled = context.createIIRFilter([1], new Float64Array([2, 1.8]));
  const gain = context.createGain();
  // Dividing by the first feedback coefficient scales the feedforward too.
  gain.gain.value = 2;
  const merger = context.createChannelMerger(2);
  source.connect(normalized);
  source.connect(scaled);
  normalized.connect(merger, 0, 0);
  scaled.connect(gain).connect(merger, 0, 1);
  merger.connect(context.destination);
  source.start();
  const rendered = await context.startRendering();
  const left = rendered.getChannelData(0);
  const right = rendered.getChannelData(1);
  for (let index = 0; index < left.length; index += 1) {
    assertClose(right[index], left[index], `iir sample ${index}`);
  }
});
