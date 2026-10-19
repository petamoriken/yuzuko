function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function assertClose(actual: number, expected: number, message: string): void {
  assert(Math.abs(actual - expected) < 1e-5, `${message}: ${actual}`);
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

async function decodeTone(name: string): Promise<AudioBuffer> {
  const bytes = await (await fetch(new URL(name, tones))).arrayBuffer();
  const context = new AudioContext();
  try {
    const buffer = await context.decodeAudioData(bytes);
    assert(buffer.sampleRate === 44100, `${name} rate ${buffer.sampleRate}`);
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

Yuzuko.test("decodeAudioData reads WAV, FLAC, MP3, and Vorbis", async () => {
  await decodeTone("tone.wav");
  await decodeTone("tone.flac");
  await decodeTone("tone.mp3");
  await decodeTone("tone.ogg");
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
