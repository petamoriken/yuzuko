Yuzuko.test("structuredClone copies Blob, File, and DOMException", async () => {
  const blob = new Blob(["hi"], { type: "Text/Plain" });
  const blobCopy = structuredClone(blob);
  if (blobCopy === blob) {
    throw new Error("blob identity");
  }
  if (!(blobCopy instanceof Blob) || blobCopy instanceof File) {
    throw new Error("blob type");
  }
  if (blobCopy.type !== "text/plain" || blobCopy.size !== 2) {
    throw new Error("blob meta " + blobCopy.type + " " + blobCopy.size);
  }
  if (await blobCopy.text() !== "hi") {
    throw new Error("blob text");
  }

  const empty = structuredClone(new Blob());
  if (empty.size !== 0 || await empty.text() !== "") {
    throw new Error("empty blob");
  }

  const file = new File(["ab"], "note.txt", {
    type: "text/plain",
    lastModified: 1000,
  });
  const fileCopy = structuredClone(file);
  if (!(fileCopy instanceof File) || !(fileCopy instanceof Blob)) {
    throw new Error("file type");
  }
  if (fileCopy === file || fileCopy.name !== "note.txt") {
    throw new Error("file name");
  }
  if (fileCopy.lastModified !== 1000 || fileCopy.type !== "text/plain") {
    throw new Error("file meta");
  }
  if (await fileCopy.text() !== "ab") {
    throw new Error("file text");
  }

  const shared = structuredClone({ left: blob, right: blob });
  if (shared.left !== shared.right || shared.left === blob) {
    throw new Error("shared blob");
  }
  if (await shared.left.text() !== "hi") {
    throw new Error("shared text");
  }

  const bytes = new Uint8Array([1, 2]);
  const bytesCopy = structuredClone(bytes);
  if (bytesCopy[0] !== 1 || bytesCopy.buffer === bytes.buffer) {
    throw new Error("bytes");
  }

  const error = new DOMException("missing", "NotFoundError");
  const errorCopy = structuredClone(error);
  if (!(errorCopy instanceof DOMException) || errorCopy === error) {
    throw new Error("exception identity");
  }
  if (
    errorCopy.message !== "missing"
    || errorCopy.name !== "NotFoundError"
    || errorCopy.code !== 8
  ) {
    throw new Error(
      errorCopy.name + " " + errorCopy.message + " " + errorCopy.code,
    );
  }
  if (typeof errorCopy.stack !== "string" || errorCopy.stack.length === 0) {
    throw new Error("exception stack");
  }

  let name = "";
  try {
    structuredClone(new URL("https://example.test/"));
  } catch (caught) {
    name = caught.name;
  }
  if (name !== "DataCloneError") {
    throw new Error("url " + name);
  }
  name = "";
  try {
    structuredClone(new URLSearchParams("a=1"));
  } catch (caught) {
    name = caught.name;
  }
  if (name !== "DataCloneError") {
    throw new Error("params " + name);
  }
  name = "";
  try {
    structuredClone(globalThis);
  } catch (caught) {
    name = caught.name;
  }
  if (name !== "DataCloneError") {
    throw new Error("global " + name);
  }
});

function cloneError(run: () => unknown): string {
  try {
    run();
    return "";
  } catch (caught) {
    return caught.name;
  }
}

Yuzuko.test("structuredClone copies ImageData and transfers its buffer", () => {
  const pixels = new Uint8ClampedArray([1, 2, 3, 4, 5, 6, 7, 8]);
  const image = new ImageData(pixels, 2, 1);
  const copy = structuredClone(image);
  if (copy === image || !(copy instanceof ImageData)) {
    throw new Error("image identity");
  }
  if (copy.width !== 2 || copy.height !== 1 || copy.colorSpace !== "srgb") {
    throw new Error("image meta");
  }
  if (copy.pixelFormat !== "rgba-unorm8" || copy.data.buffer === image.data.buffer) {
    throw new Error("image buffer");
  }
  if (copy.data[0] !== 1 || copy.data[7] !== 8 || image.data[0] !== 1) {
    throw new Error("image pixels");
  }

  const shared = structuredClone({ left: image, right: image });
  if (shared.left !== shared.right || shared.left === image) {
    throw new Error("shared image");
  }
  if (shared.left.data[4] !== 5) {
    throw new Error("shared pixel");
  }

  if (cloneError(() => structuredClone(image, { transfer: [image] })) !== "DataCloneError") {
    throw new Error("image transfer");
  }
  if (image.data[0] !== 1 || image.data.buffer.byteLength === 0) {
    throw new Error("image stayed");
  }

  const source = new Float16Array([1.5, -0.25, 0.5, 0.75]);
  const wide = new ImageData(source, 1, 1, {
    colorSpace: "display-p3",
    pixelFormat: "rgba-float16",
  });
  const wideCopy = structuredClone(wide);
  if (wideCopy.colorSpace !== "display-p3" || wideCopy.pixelFormat !== "rgba-float16") {
    throw new Error("wide meta");
  }
  if (wideCopy.data.buffer === wide.data.buffer) {
    throw new Error("wide buffer");
  }
  for (let index = 0; index < 4; index += 1) {
    if (wideCopy.data[index] !== wide.data[index]) {
      throw new Error("wide pixel " + index);
    }
  }

  const held = image.data.buffer;
  const moved = structuredClone(
    { image, buffer: image.data.buffer },
    { transfer: [image.data.buffer] },
  );
  if (held.byteLength !== 0) {
    throw new Error("source buffer");
  }
  if (moved.image.data.buffer !== moved.buffer) {
    throw new Error("shared transferred buffer");
  }
  if (moved.image.width !== 2 || moved.image.data[0] !== 1 || moved.image.data[7] !== 8) {
    throw new Error("transferred pixels");
  }
});

Yuzuko.test("structuredClone copies and transfers ImageBitmap", async () => {
  const bitmap = await createImageBitmap(
    new ImageData(new Uint8ClampedArray([9, 8, 7, 255]), 1, 1),
  );
  const cloned = structuredClone(bitmap);
  if (cloned === bitmap || cloned.width !== 1 || cloned.height !== 1) {
    throw new Error("bitmap copy");
  }
  if (bitmap.width !== 1) {
    throw new Error("bitmap source");
  }
  const moved = structuredClone(bitmap, { transfer: [bitmap] });
  if (moved.width !== 1 || moved.height !== 1 || bitmap.width !== 0 || bitmap.height !== 0) {
    throw new Error("bitmap transfer");
  }

  const closed = await createImageBitmap(
    new ImageData(new Uint8ClampedArray([1, 2, 3, 4]), 1, 1),
  );
  closed.close();
  if (cloneError(() => structuredClone(closed)) !== "DataCloneError") {
    throw new Error("closed bitmap");
  }

  const once = await createImageBitmap(
    new ImageData(new Uint8ClampedArray([1, 2, 3, 4]), 1, 1),
  );
  if (
    cloneError(() => structuredClone(once, { transfer: [once, once] }))
      !== "DataCloneError"
    || once.width !== 1
  ) {
    throw new Error("duplicate bitmap");
  }

  const extra = await createImageBitmap(
    new ImageData(new Uint8ClampedArray([1, 2, 3, 4]), 1, 1),
  );
  structuredClone(0, { transfer: [extra] });
  if (extra.width !== 0 || extra.height !== 0) {
    throw new Error("unreferenced bitmap");
  }

  const kept = await createImageBitmap(
    new ImageData(new Uint8ClampedArray([1, 2, 3, 4]), 1, 1),
  );
  if (
    cloneError(() => structuredClone(
      { bitmap: kept, bad: globalThis },
      { transfer: [kept] },
    )) !== "DataCloneError"
    || kept.width !== 1
  ) {
    throw new Error("failed bitmap transfer");
  }
});

Yuzuko.test("structuredClone copies encoded chunks and rejects a transfer", () => {
  const audio = new EncodedAudioChunk({
    type: "key",
    timestamp: 20,
    data: new Uint8Array([1, 2, 3]),
  });
  const audioCopy = structuredClone(audio);
  if (
    audioCopy === audio
    || audioCopy.type !== "key"
    || audioCopy.timestamp !== 20
    || audioCopy.byteLength !== 3
  ) {
    throw new Error("audio chunk");
  }
  if (
    cloneError(() => structuredClone(audio, { transfer: [audio] }))
      !== "DataCloneError"
    || audio.byteLength !== 3
  ) {
    throw new Error("audio chunk transfer");
  }

  const video = new EncodedVideoChunk({
    type: "delta",
    timestamp: 40,
    data: new Uint8Array([9]),
  });
  const videoCopy = structuredClone(video);
  if (
    videoCopy === video
    || videoCopy.type !== "delta"
    || videoCopy.timestamp !== 40
    || videoCopy.byteLength !== 1
  ) {
    throw new Error("video chunk");
  }
  if (cloneError(() => structuredClone(video, { transfer: [video] })) !== "DataCloneError") {
    throw new Error("video chunk transfer");
  }
});

Yuzuko.test("structuredClone transfers a port and a canvas", async () => {
  const channel = new MessageChannel();
  const copy = structuredClone(channel.port2, { transfer: [channel.port2] });
  const received = new Promise((resolve) => {
    channel.port1.onmessage = (event) => resolve(event.data);
  });
  copy.postMessage("ohai");
  if (await received !== "ohai") {
    throw new Error("port message");
  }
  if (
    cloneError(() => structuredClone(channel.port2, { transfer: [channel.port2] }))
      !== "DataCloneError"
  ) {
    throw new Error("second port");
  }

  const spare = new MessageChannel();
  structuredClone(0, { transfer: [spare.port1] });
  const ping = new Promise((resolve) => {
    spare.port2.onmessage = (event) => resolve(event.data);
  });
  spare.port1.postMessage("open");
  if (await ping !== "open") {
    throw new Error("unreferenced port");
  }

  const plain = new OffscreenCanvas(3, 3);
  if (cloneError(() => structuredClone(plain)) !== "DataCloneError" || plain.width !== 3) {
    throw new Error("canvas copy");
  }
  const canvas = new OffscreenCanvas(10, 10);
  const moved = structuredClone(canvas, { transfer: [canvas] });
  if (moved.width !== 10 || moved.height !== 10 || canvas.width !== 0) {
    throw new Error("canvas transfer");
  }
  const idle = new OffscreenCanvas(2, 2);
  structuredClone(0, { transfer: [idle] });
  if (idle.width !== 2 || idle.getContext("2d") === null) {
    throw new Error("unreferenced canvas");
  }
});

Yuzuko.test("a transfer list rejects values that cannot move", async () => {
  const blob = new Blob(["ab"]);
  if (cloneError(() => structuredClone(blob, { transfer: [blob] })) !== "DataCloneError") {
    throw new Error("blob transfer");
  }
  if (await blob.text() !== "ab") {
    throw new Error("blob stayed");
  }

  const view = new Uint8Array([4, 5]);
  if (cloneError(() => structuredClone(view, { transfer: [view] })) !== "DataCloneError") {
    throw new Error("view transfer");
  }
  if (view[0] !== 4) {
    throw new Error("view stayed");
  }
  const buffer = new Uint8Array([7, 8]).buffer;
  const bufferCopy = structuredClone(buffer, { transfer: [buffer] });
  if (buffer.byteLength !== 0 || bufferCopy.byteLength !== 2) {
    throw new Error("buffer transfer");
  }

  const owned = await createImageBitmap(
    new ImageData(new Uint8ClampedArray([1, 2, 3, 4]), 1, 1),
  );
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue(owned, { transfer: [owned] });
    },
    type: "owning",
  });
  if (owned.width !== 0) {
    throw new Error("owning source");
  }
  const chunk = await stream.getReader().read();
  if (chunk.value.width !== 1 || chunk.value.height !== 1) {
    throw new Error("owning chunk");
  }

  const bytes = new Uint8Array([1, 2, 3]);
  if (cloneError(() => {
    new ReadableStream({
      start(controller) {
        controller.enqueue(bytes, { transfer: [bytes] });
      },
      type: "owning",
    });
  }) !== "DataCloneError" || bytes[0] !== 1) {
    throw new Error("owning view");
  }

  const payload = new Blob(["x"]);
  if (cloneError(() => {
    new ReadableStream({
      start(controller) {
        controller.enqueue(payload, { transfer: [payload] });
      },
      type: "owning",
    });
  }) !== "DataCloneError" || await payload.text() !== "x") {
    throw new Error("owning blob");
  }
});

function delay() {
  return new Promise((resolve) => {
    setTimeout(resolve, 0);
  });
}

function withTimeout(promise, message) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error(message));
    }, 2000);
  });
  return Promise.race([promise, timeout]).finally(() => {
    clearTimeout(timer);
  });
}

function cloneFailure(run) {
  try {
    run();
    return { name: "", message: "" };
  } catch (caught) {
    return {
      name: caught && caught.name ? caught.name : "",
      message: caught && caught.message ? String(caught.message) : "",
    };
  }
}

async function rejection(promise) {
  try {
    return { value: await promise, name: "", message: "" };
  } catch (caught) {
    return {
      value: caught,
      name: caught && caught.name ? caught.name : "",
      message: caught && caught.message ? String(caught.message) : "",
    };
  }
}

function turn() {
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => {
      resolve();
    };
    channel.port2.postMessage(null);
  });
}

async function until(ready, message) {
  // A zero-delay timer is polled before a queued port job, so spinning on
  // setTimeout never delivers the stream message this wait observes.
  for (let attempt = 0; attempt < 8; attempt += 1) {
    if (ready()) {
      return;
    }
    await turn();
  }
  if (!ready()) {
    throw new Error(message);
  }
}

function streamWorker() {
  return new Worker(
    new URL("./workers/stream_transfer.js", import.meta.url).href,
  );
}

function messageQueue(worker) {
  const items = [];
  const resolvers = [];
  const rejecters = [];
  let failure;
  worker.onmessage = (event) => {
    const resolve = resolvers.shift();
    if (resolve) {
      resolve(event.data);
    } else {
      items.push(event.data);
    }
  };
  worker.onerror = (event) => {
    if (failure) {
      return;
    }
    failure = new Error(event.message || "worker error");
    while (rejecters.length > 0) {
      rejecters.shift()(failure);
    }
  };
  return {
    take() {
      if (items.length > 0) {
        return Promise.resolve(items.shift());
      }
      if (failure) {
        return Promise.reject(failure);
      }
      return new Promise((resolve, reject) => {
        resolvers.push(resolve);
        rejecters.push(reject);
      });
    },
  };
}

function assertReply(reply, kind) {
  if (!reply || reply.kind === "error") {
    throw new Error(reply && reply.message ? reply.message : "worker failed");
  }
  if (reply.kind !== kind) {
    throw new Error("worker reply " + String(reply && reply.kind));
  }
}

function chunkStream(value) {
  return new ReadableStream({
    start(controller) {
      controller.enqueue(value);
      controller.close();
    },
  });
}

Yuzuko.test("structuredClone transfers a readable stream", async () => {
  const readable = chunkStream("chunk");
  const moved = structuredClone(readable, { transfer: [readable] });
  if (!readable.locked) {
    throw new Error("original locked");
  }
  if (moved === readable || !(moved instanceof ReadableStream)) {
    throw new Error("identity");
  }
  if (Object.getPrototypeOf(moved) !== ReadableStream.prototype) {
    throw new Error("prototype");
  }
  if (
    cloneError(() => structuredClone(readable, { transfer: [readable] }))
      !== "DataCloneError"
  ) {
    throw new Error("second transfer");
  }
  const reader = moved.getReader();
  const first = await reader.read();
  if (first.done || first.value !== "chunk") {
    throw new Error("chunk");
  }
  const second = await reader.read();
  if (second.done !== true) {
    throw new Error("done");
  }

  const wrappedSource = chunkStream("wrapped");
  const wrapped = structuredClone(
    { readable: wrappedSource },
    { transfer: [wrappedSource] },
  );
  const wrappedChunk = await wrapped.readable.getReader().read();
  if (wrappedChunk.value !== "wrapped") {
    throw new Error("wrapped");
  }

  const sharedSource = chunkStream("shared");
  const shared = structuredClone(
    { left: sharedSource, right: sharedSource },
    { transfer: [sharedSource] },
  );
  if (shared.left !== shared.right) {
    throw new Error("shared identity");
  }
  const sharedChunk = await shared.left.getReader().read();
  if (sharedChunk.value !== "shared") {
    throw new Error("shared chunk");
  }

});

Yuzuko.test("a stream transfer list rejects copies, duplicates, and locks", async () => {
  const omitted = chunkStream("stay");
  const omittedFailure = cloneFailure(() => structuredClone(omitted));
  if (
    omittedFailure.name !== "DataCloneError"
    || omittedFailure.message !== "The object could not be cloned."
    || omitted.locked
  ) {
    throw new Error("omitted " + omittedFailure.message);
  }
  if ((await omitted.getReader().read()).value !== "stay") {
    throw new Error("omitted chunk");
  }

  const unreachable = chunkStream("open");
  structuredClone(0, { transfer: [unreachable] });
  if (unreachable.locked) {
    throw new Error("unreachable locked");
  }
  if ((await unreachable.getReader().read()).value !== "open") {
    throw new Error("unreachable chunk");
  }

  const duplicate = new ReadableStream();
  const duplicateFailure = cloneFailure(() => {
    structuredClone(duplicate, { transfer: [duplicate, duplicate] });
  });
  if (
    duplicateFailure.name !== "DataCloneError"
    || duplicateFailure.message !== "The object is already in the transfer list"
    || duplicate.locked
  ) {
    throw new Error("duplicate " + duplicateFailure.message);
  }

  let controller;
  const locked = new ReadableStream({
    start(control) {
      controller = control;
      control.enqueue("held");
    },
  });
  const reader = locked.getReader();
  const lockedFailure = cloneFailure(() => {
    structuredClone(locked, { transfer: [locked] });
  });
  if (lockedFailure.name !== "DataCloneError") {
    throw new Error("locked " + lockedFailure.name);
  }
  if ((await reader.read()).value !== "held") {
    throw new Error("locked reader");
  }
  controller.close();
});

Yuzuko.test("transferred readables keep the base prototype and the source state", async () => {
  class SubRead extends ReadableStream {}
  const subclass = new SubRead({
    start(controller) {
      controller.enqueue("sub");
      controller.close();
    },
  });
  const movedSubclass = structuredClone(subclass, { transfer: [subclass] });
  if (
    Object.getPrototypeOf(movedSubclass) !== ReadableStream.prototype
    || !(movedSubclass instanceof ReadableStream)
  ) {
    throw new Error("subclass");
  }
  if ((await movedSubclass.getReader().read()).value !== "sub") {
    throw new Error("subclass chunk");
  }

  let closedController;
  const closed = new ReadableStream({
    start(controller) {
      closedController = controller;
    },
  });
  closedController.close();
  const movedClosed = structuredClone(closed, { transfer: [closed] });
  if ((await movedClosed.getReader().read()).done !== true) {
    throw new Error("closed");
  }

  let erroredController;
  const errored = new ReadableStream({
    start(controller) {
      erroredController = controller;
    },
  });
  erroredController.error("foo");
  const movedErrored = structuredClone(errored, { transfer: [errored] });
  const erroredRead = await rejection(movedErrored.getReader().read());
  if (erroredRead.value !== "foo") {
    throw new Error("errored " + String(erroredRead.value));
  }

  const reason = new DOMException("nope", "NotFoundError");
  let exceptionController;
  const exceptional = new ReadableStream({
    start(controller) {
      exceptionController = controller;
    },
  });
  exceptionController.error(reason);
  const movedException = structuredClone(exceptional, { transfer: [exceptional] });
  const exceptionRead = await rejection(movedException.getReader().read());
  if (
    exceptionRead.name !== "NotFoundError"
    || exceptionRead.message !== "nope"
  ) {
    throw new Error("exception " + exceptionRead.name + " " + exceptionRead.message);
  }

  let disturbedController;
  const disturbed = new ReadableStream({
    start(controller) {
      disturbedController = controller;
      controller.enqueue("one");
      controller.enqueue("two");
    },
  });
  const firstReader = disturbed.getReader();
  if ((await firstReader.read()).value !== "one") {
    throw new Error("disturbed first");
  }
  firstReader.releaseLock();
  const movedDisturbed = structuredClone(disturbed, { transfer: [disturbed] });
  if ((await movedDisturbed.getReader().read()).value !== "two") {
    throw new Error("disturbed second");
  }
});

Yuzuko.test("a non-cloneable chunk errors both sides of a transferred readable", async () => {
  let controller;
  const readable = new ReadableStream({
    start(control) {
      controller = control;
    },
  });
  const moved = structuredClone(readable, { transfer: [readable] });
  const reader = moved.getReader();
  controller.enqueue(new ReadableStream());
  const readFailure = await rejection(reader.read());
  if (readFailure.name !== "DataCloneError") {
    throw new Error("read " + readFailure.name);
  }
  let enqueueName = "";
  try {
    controller.enqueue("later");
  } catch (caught) {
    enqueueName = caught && caught.name ? caught.name : "";
  }
  if (enqueueName !== "TypeError") {
    throw new Error("enqueue " + enqueueName);
  }
});

Yuzuko.test("chunk getters run when a transferred readable posts the chunk", async () => {
  let flag = false;
  let controller;
  const readable = new ReadableStream({
    start(control) {
      controller = control;
    },
  });
  const moved = structuredClone(readable, { transfer: [readable] });
  const reader = moved.getReader();
  controller.enqueue({
    get getter() {
      flag = true;
      return "a";
    },
  });
  await delay();
  if (flag) {
    throw new Error("getter ran early");
  }
  const chunk = await reader.read();
  if (!flag || !chunk.value || chunk.value.getter !== "a") {
    throw new Error("getter " + flag);
  }
});

Yuzuko.test("transferring a readable counts one extra chunk of backpressure", async () => {
  const idle = [];
  const quiet = new ReadableStream({
    pull() {
      idle.push("pull");
    },
  }, { highWaterMark: 0 });
  await delay();
  if (idle.length !== 0) {
    throw new Error("early pull " + idle.length);
  }
  structuredClone(quiet, { transfer: [quiet] });
  await delay();
  if (idle.length !== 1 || idle[0] !== "pull") {
    throw new Error("pulls " + idle.join(","));
  }

  let pulls = 0;
  const filling = new ReadableStream({
    pull(controller) {
      pulls += 1;
      controller.enqueue(pulls);
    },
  }, { highWaterMark: 2 });
  structuredClone(filling, { transfer: [filling] });
  await delay();
  if (pulls !== 3) {
    throw new Error("high water pulls " + pulls);
  }
});

Yuzuko.test("a transferred byte stream arrives as a default stream", async () => {
  let pulls = 0;
  let controller;
  const readable = new ReadableStream({
    type: "bytes",
    start(control) {
      controller = control;
      control.enqueue(new Uint8Array(1024));
      control.enqueue(new Uint8Array(1024));
    },
    pull() {
      pulls += 1;
      controller.enqueue(new Uint8Array(1024));
    },
  }, { highWaterMark: 512 });
  await delay();
  if (pulls !== 0) {
    throw new Error("setup pulls " + pulls);
  }
  const moved = structuredClone(readable, { transfer: [readable] });
  await delay();
  if (pulls !== 0) {
    throw new Error("transfer pulls " + pulls);
  }
  let byobName = "";
  let byobMessage = "";
  try {
    moved.getReader({ mode: "byob" });
  } catch (caught) {
    byobName = caught && caught.name ? caught.name : "";
    byobMessage = caught && caught.message ? String(caught.message) : "";
  }
  if (byobName !== "TypeError" || !byobMessage.includes("BYOB")) {
    throw new Error("byob " + byobName + " " + byobMessage);
  }
  const chunk = await moved.getReader().read();
  if (
    pulls !== 1
    || !(chunk.value instanceof Uint8Array)
    || chunk.value.byteLength !== 1024
  ) {
    throw new Error("byte chunk " + pulls + " " + (chunk.value && chunk.value.byteLength));
  }
});

Yuzuko.test("a transferred writable starts with one chunk of queue space", () => {
  const original = new WritableStream(
    {},
    new ByteLengthQueuingStrategy({ highWaterMark: 65536 }),
  );
  const moved = structuredClone(original, { transfer: [original] });
  const writer = moved.getWriter();
  if (writer.desiredSize !== 1) {
    throw new Error("desiredSize " + writer.desiredSize);
  }
});

Yuzuko.test("a transferred writable resolves a write when the chunk is posted", async () => {
  let resolveWrite;
  let markEntered;
  const entered = new Promise((resolve) => {
    markEntered = resolve;
  });
  const original = new WritableStream({
    write() {
      markEntered();
      return new Promise((resolve) => {
        resolveWrite = resolve;
      });
    },
  });
  const moved = structuredClone(original, { transfer: [original] });
  const writer = moved.getWriter();
  await writer.write("a");
  if (writer.desiredSize !== 1) {
    throw new Error("desiredSize " + writer.desiredSize);
  }
  await entered;
  let settled = false;
  const second = writer.write("b").then(
    () => {
      settled = true;
    },
    () => {
      settled = true;
    },
  );
  await delay();
  if (settled) {
    throw new Error("second write settled");
  }
  resolveWrite();
  await second;
  if (!settled) {
    throw new Error("second write stayed pending");
  }
});

Yuzuko.test("abort and a failed write cross a transferred writable", async () => {
  let reason = "missing";
  let markAbort;
  const aborted = new Promise((resolve) => {
    markAbort = resolve;
  });
  const original = new WritableStream({
    abort(value) {
      reason = value;
      markAbort();
    },
  });
  const moved = structuredClone(original, { transfer: [original] });
  const pending = moved.abort("p");
  pending.then(() => {}, () => {});
  await aborted;
  if (reason !== "p") {
    throw new Error("abort " + String(reason));
  }
  await pending;

  const reasons = [];
  const sink = new WritableStream({
    abort(value) {
      reasons.push(value);
    },
  });
  const transferred = structuredClone(sink, { transfer: [sink] });
  const writer = transferred.getWriter();
  const closed = rejection(writer.closed);
  const writeFailure = await rejection(writer.write(new WritableStream()));
  if (writeFailure.name !== "DataCloneError") {
    throw new Error("write " + writeFailure.name);
  }
  const closedFailure = await closed;
  if (closedFailure.name !== "DataCloneError") {
    throw new Error("closed " + closedFailure.name);
  }
  await until(() => reasons.length > 0, "original abort missing");
  if (!reasons[0] || reasons[0].name !== "DataCloneError") {
    throw new Error("original abort");
  }
});

Yuzuko.test("canceling a transferred readable does not wait for the source", async () => {
  let cancelComplete = false;
  let cancelReason = "missing";
  const readable = new ReadableStream({
    cancel(reason) {
      cancelReason = reason;
      return delay().then(() => {
        cancelComplete = true;
      });
    },
  });
  const moved = structuredClone(readable, { transfer: [readable] });
  await moved.cancel("r");
  if (cancelComplete) {
    throw new Error("cancel waited");
  }
  await until(() => cancelComplete, "cancel did not finish");
  if (cancelReason !== "r") {
    throw new Error("cancel reason " + String(cancelReason));
  }
});

Yuzuko.test("structuredClone transfers a transform stream", async () => {
  const original = new TransformStream();
  const moved = structuredClone(original, { transfer: [original] });
  if (!original.readable.locked || !original.writable.locked) {
    throw new Error("original sides");
  }
  if (Object.getPrototypeOf(moved) !== TransformStream.prototype) {
    throw new Error("transform prototype");
  }
  if (Object.getPrototypeOf(moved.readable) !== ReadableStream.prototype) {
    throw new Error("readable prototype");
  }
  if (Object.getPrototypeOf(moved.writable) !== WritableStream.prototype) {
    throw new Error("writable prototype");
  }
  const reader = moved.readable.getReader();
  const writer = moved.writable.getWriter();
  const writing = Promise.all([writer.write("hi"), writer.close()]);
  writing.then(() => {}, () => {});
  const first = await reader.read();
  if (first.done || first.value !== "hi") {
    throw new Error("transform chunk");
  }
  const second = await reader.read();
  if (second.done !== true) {
    throw new Error("transform done");
  }
  await writing;

  const readableLocked = new TransformStream();
  readableLocked.readable.getReader();
  const readableFailure = cloneFailure(() => {
    structuredClone(readableLocked, { transfer: [readableLocked] });
  });
  if (readableFailure.name !== "DataCloneError" || readableLocked.writable.locked) {
    throw new Error("locked readable");
  }

  const writableLocked = new TransformStream();
  writableLocked.writable.getWriter();
  const writableFailure = cloneFailure(() => {
    structuredClone(writableLocked, { transfer: [writableLocked] });
  });
  if (writableFailure.name !== "DataCloneError" || writableLocked.readable.locked) {
    throw new Error("locked writable");
  }
});

Yuzuko.test("TextEncoderStream itself cannot transfer", async () => {
  const encoder = new TextEncoderStream();
  const failure = cloneFailure(() => {
    structuredClone(encoder, { transfer: [encoder] });
  });
  if (
    failure.name !== "DataCloneError"
    || failure.message !== "The value cannot be transferred"
  ) {
    throw new Error("encoder " + failure.name + " " + failure.message);
  }
  const moved = structuredClone(encoder.readable, {
    transfer: [encoder.readable],
  });
  if (!encoder.readable.locked || !(moved instanceof ReadableStream)) {
    throw new Error("encoder readable");
  }
  const writer = encoder.writable.getWriter();
  const reader = moved.getReader();
  const pending = writer.write("a");
  pending.then(() => {}, () => {});
  const chunk = await reader.read();
  writer.close().then(() => {}, () => {});
  if (!(chunk.value instanceof Uint8Array) || chunk.value[0] !== 97) {
    throw new Error("encoded");
  }
  await pending;
});

Yuzuko.test("MessagePort.postMessage transfers a readable stream", async () => {
  const channel = new MessageChannel();
  const readable = chunkStream("port");
  const received = new Promise((resolve) => {
    channel.port2.onmessage = (event) => resolve(event.data);
  });
  channel.port1.postMessage(readable, [readable]);
  const data = await received;
  if (!(data instanceof ReadableStream) || data === readable) {
    throw new Error("port stream");
  }
  const reader = data.getReader();
  const first = await reader.read();
  if (first.value !== "port" || first.done) {
    throw new Error("port chunk");
  }
  if ((await reader.read()).done !== true) {
    throw new Error("port done");
  }
});

Yuzuko.test("a worker reads a transferred readable", async () => {
  const worker = streamWorker();
  const queue = messageQueue(worker);
  try {
    const readable = chunkStream("main");
    worker.postMessage({ kind: "read", stream: readable }, [readable]);
    if (!readable.locked) {
      throw new Error("not locked");
    }
    const reply = await withTimeout(queue.take(), "worker read timed out");
    assertReply(reply, "read");
    if (reply.value !== "main" || reply.restDone !== true || reply.stream !== true) {
      throw new Error("worker read " + JSON.stringify(reply));
    }
  } finally {
    worker.terminate();
  }
});

Yuzuko.test("a worker writes a transferred writable", async () => {
  const worker = streamWorker();
  const queue = messageQueue(worker);
  try {
    let closed = false;
    let resolveSink;
    const sink = new Promise((resolve) => {
      resolveSink = resolve;
    });
    const writable = new WritableStream({
      write(chunk) {
        resolveSink(chunk);
      },
      close() {
        closed = true;
      },
    });
    worker.postMessage({ kind: "write", stream: writable }, [writable]);
    const chunk = await withTimeout(sink, "worker write timed out");
    if (chunk !== "from-worker") {
      throw new Error("sink " + String(chunk));
    }
    const wrote = await withTimeout(queue.take(), "worker wrote timed out");
    assertReply(wrote, "wrote");
    await until(() => closed, "writable did not close");
  } finally {
    worker.terminate();
  }
});

Yuzuko.test("a worker transfers a readable back to its owner", async () => {
  const worker = streamWorker();
  const queue = messageQueue(worker);
  try {
    worker.postMessage({ kind: "send-readable" });
    const reply = await withTimeout(queue.take(), "worker stream timed out");
    assertReply(reply, "readable");
    if (!(reply.stream instanceof ReadableStream)) {
      throw new Error("worker stream");
    }
    const reader = reply.stream.getReader();
    const first = await withTimeout(reader.read(), "worker chunk timed out");
    if (first.value !== "from-worker" || first.done) {
      throw new Error("worker chunk");
    }
    const rest = await withTimeout(reader.read(), "worker done timed out");
    if (rest.done !== true) {
      throw new Error("worker done");
    }
  } finally {
    worker.terminate();
  }
});
