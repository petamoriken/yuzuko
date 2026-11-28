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

  const plain = new Blob(["z"], { type: "A" });
  const plainCopy = structuredClone(plain);
  if (plainCopy.type !== "a" || plainCopy.size !== 1) {
    throw new Error("plain type " + plainCopy.type);
  }
  const spaced = new File(["q"], "n", { type: " image/gif " });
  const spacedCopy = structuredClone(spaced);
  if (
    !(spacedCopy instanceof File)
    || spacedCopy.type !== " image/gif "
    || spacedCopy.name !== "n"
  ) {
    throw new Error("spaced type " + spacedCopy.type);
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
    name = caught instanceof DOMException ? caught.name : "";
  }
  if (name !== "DataCloneError") {
    throw new Error("url " + name);
  }
  name = "";
  try {
    structuredClone(new URLSearchParams("a=1"));
  } catch (caught) {
    name = caught instanceof DOMException ? caught.name : "";
  }
  if (name !== "DataCloneError") {
    throw new Error("params " + name);
  }
  name = "";
  try {
    structuredClone(globalThis);
  } catch (caught) {
    name = caught instanceof DOMException ? caught.name : "";
  }
  if (name !== "DataCloneError") {
    throw new Error("global " + name);
  }
});

Yuzuko.test("structuredClone rejects a SharedArrayBuffer", () => {
  if (crossOriginIsolated !== false) {
    throw new Error("crossOriginIsolated " + String(crossOriginIsolated));
  }
  const samples = [
    new SharedArrayBuffer(8),
    new SharedArrayBuffer(16, { maxByteLength: 1024 }),
    new Uint8Array(new SharedArrayBuffer(4)),
    { buffer: new SharedArrayBuffer(4) },
  ];
  for (const value of samples) {
    let caught: DOMException | null = null;
    try {
      structuredClone(value);
    } catch (error) {
      // @ts-expect-error structuredClone throws DOMException.
      caught = error;
    }
    if (
      !(caught instanceof DOMException)
      || caught.name !== "DataCloneError"
      || caught.code !== DOMException.DATA_CLONE_ERR
      || caught.message !== "The SharedArrayBuffer could not be cloned."
    ) {
      throw new Error(
        (caught && caught.name + " " + caught.message) || "missing",
      );
    }
  }
});

function cloneError(run: () => unknown): string {
  try {
    run();
    return "";
  } catch (caught) {
    return caught instanceof Error ? caught.name : "";
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

  if (cloneError(() => {
    // @ts-expect-error ImageData is not transferable.
    structuredClone(image, { transfer: [image] });
  }) !== "DataCloneError") {
    throw new Error("image transfer");
  }
  if (image.data[0] !== 1 || image.data.buffer.byteLength === 0) {
    throw new Error("image stayed");
  }

  const source = new Float16Array([1.5, -0.25, 0.5, 0.75]);
  // @ts-expect-error Float16Array is not an ImageDataArray.
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

function bitmapPixel(bitmap: ImageBitmap): string {
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const context = canvas.getContext("2d");
  if (context === null) {
    throw new Error("bitmap context");
  }
  context.drawImage(bitmap, 0, 0);
  const data = context.getImageData(0, 0, 1, 1).data;
  return data[0] + "," + data[1] + "," + data[2] + "," + data[3];
}

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
  let width: number = bitmap.width;
  let height: number = bitmap.height;
  if (
    moved.width !== 1
    || moved.height !== 1
    || width !== 0
    || height !== 0
    || bitmapPixel(cloned) !== "9,8,7,255"
    || bitmapPixel(moved) !== "9,8,7,255"
  ) {
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
    cloneError(() => {
      // @ts-expect-error An encoded audio chunk is not transferable.
      structuredClone(audio, { transfer: [audio] });
    })
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
  if (cloneError(() => {
    // @ts-expect-error An encoded video chunk is not transferable.
    structuredClone(video, { transfer: [video] });
  }) !== "DataCloneError") {
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
  if (cloneError(() => {
    // @ts-expect-error A blob is not transferable.
    structuredClone(blob, { transfer: [blob] });
  }) !== "DataCloneError") {
    throw new Error("blob transfer");
  }
  if (await blob.text() !== "ab") {
    throw new Error("blob stayed");
  }

  const view = new Uint8Array([4, 5]);
  if (cloneError(() => {
    // @ts-expect-error A view is not transferable.
    structuredClone(view, { transfer: [view] });
  }) !== "DataCloneError") {
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
      // @ts-expect-error An owning controller enqueues with a transfer list.
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
        // @ts-expect-error An owning controller rejects a view in the transfer list.
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
        // @ts-expect-error An owning controller rejects a blob in the transfer list.
        controller.enqueue(payload, { transfer: [payload] });
      },
      type: "owning",
    });
  }) !== "DataCloneError" || await payload.text() !== "x") {
    throw new Error("owning blob");
  }
});

Yuzuko.test("structuredClone moves an ArrayBuffer", () => {
  const bytes = new Uint8Array([1, 2, 3]);
  const copied = structuredClone(bytes);
  copied[0] = 0;
  if (bytes[0] !== 1 || bytes.buffer.detached || copied.buffer === bytes.buffer) {
    throw new Error("plain copy");
  }

  const raw = new Uint8Array([9, 8, 7, 6, 5]).buffer;
  const view = new DataView(raw, 1, 3);
  const movedView = structuredClone(view, { transfer: [raw] });
  if (
    raw.detached !== true
    || raw.byteLength !== 0
    || !(movedView instanceof DataView)
    || movedView.buffer === raw
    || movedView.byteOffset !== 1
    || movedView.byteLength !== 3
    || movedView.getUint8(0) !== 8
    || movedView.getUint8(2) !== 6
    || movedView.buffer.byteLength !== 5
  ) {
    throw new Error("view range");
  }

  const source = new Uint8Array([1, 2, 3, 4, 5, 6]);
  const slice = source.subarray(2, 5);
  const movedSlice = structuredClone(slice, { transfer: [source.buffer] });
  if (
    source.buffer.detached !== true
    || !(movedSlice instanceof Uint8Array)
    || movedSlice.byteOffset !== 2
    || movedSlice.length !== 3
    || movedSlice[0] !== 3
    || movedSlice[2] !== 5
    || movedSlice.buffer.byteLength !== 6
  ) {
    throw new Error("typed view");
  }

  const growable = new ArrayBuffer(4, { maxByteLength: 16 });
  new Uint8Array(growable).set([1, 2, 3, 4]);
  const movedGrowable = structuredClone(growable, { transfer: [growable] });
  if (
    growable.detached !== true
    || growable.byteLength !== 0
    || movedGrowable === growable
    || movedGrowable.resizable !== true
    || movedGrowable.maxByteLength !== 16
    || movedGrowable.byteLength !== 4
    || new Uint8Array(movedGrowable)[3] !== 4
  ) {
    throw new Error("resizable");
  }
  movedGrowable.resize(8);
  const grownLength: number = movedGrowable.byteLength;
  if (grownLength !== 8 || new Uint8Array(movedGrowable)[0] !== 1) {
    throw new Error("resize");
  }
  if (cloneError(() => growable.resize(8)) !== "TypeError") {
    throw new Error("detached resize");
  }

  const empty = new ArrayBuffer(0);
  const movedEmpty = structuredClone(empty, { transfer: [empty] });
  if (empty.detached !== true || movedEmpty === empty || movedEmpty.byteLength !== 0) {
    throw new Error("empty");
  }

  const left = new Uint8Array([1, 2]).buffer;
  const right = new Uint8Array([3]).buffer;
  const pair = structuredClone(
    { left, right, again: left },
    { transfer: [left, right] },
  );
  if (
    left.detached !== true
    || right.detached !== true
    || pair.left !== pair.again
    || pair.left === pair.right
    || new Uint8Array(pair.left)[1] !== 2
    || new Uint8Array(pair.right)[0] !== 3
  ) {
    throw new Error("pair");
  }

  const extra = new Uint8Array([9]).buffer;
  if (structuredClone(1, { transfer: [extra] }) !== 1 || extra.detached !== true) {
    throw new Error("unreferenced buffer");
  }

  const stay = new Uint8Array([7, 6, 5]);
  if (
    cloneError(() => structuredClone(globalThis, { transfer: [stay.buffer] }))
      !== "DataCloneError"
    || stay.buffer.detached
    || stay.buffer.byteLength !== 3
    || stay[0] !== 7
  ) {
    throw new Error("failed transfer");
  }
});

Yuzuko.test("postMessage and an owning stream move an ArrayBuffer", async () => {
  const channel = new MessageChannel();
  const received = new Promise<ArrayBuffer>((resolve, reject) => {
    channel.port2.onmessage = (event: MessageEvent) => {
      if (event.data instanceof ArrayBuffer) {
        resolve(event.data);
      } else {
        reject(new Error("port type"));
      }
    };
  });
  const portBuffer = new ArrayBuffer(4, { maxByteLength: 32 });
  new Uint8Array(portBuffer)[0] = 9;
  channel.port1.postMessage(portBuffer, [portBuffer]);
  const arrived = await received;
  if (
    portBuffer.detached !== true
    || arrived === portBuffer
    || arrived.resizable !== true
    || arrived.maxByteLength !== 32
    || arrived.byteLength !== 4
    || new Uint8Array(arrived)[0] !== 9
  ) {
    throw new Error("port");
  }

  const pending = nextSelfMessage();
  const selfBytes = new Uint8Array([4, 5, 6]);
  postMessage(selfBytes.subarray(1, 3), "*", [selfBytes.buffer]);
  const event = await pending;
  const posted = event.data;
  if (
    selfBytes.buffer.detached !== true
    || !(posted instanceof Uint8Array)
    || posted.byteOffset !== 1
    || posted.length !== 2
    || posted[0] !== 5
    || posted[1] !== 6
    || posted.buffer.byteLength !== 3
  ) {
    throw new Error("self message");
  }

  const listed = new Uint8Array([7, 8, 9]);
  const listedPending = nextSelfMessage();
  // @ts-expect-error A view in a postMessage transfer list moves its buffer.
  postMessage(listed.buffer, "*", [listed]);
  const listedEvent = await listedPending;
  if (
    listed.buffer.detached !== true
    || !(listedEvent.data instanceof ArrayBuffer)
    || new Uint8Array(listedEvent.data).join(",") !== "7,8,9"
  ) {
    throw new Error("view in the transfer list");
  }

  const owned = new Uint8Array([8, 7, 6, 5]).buffer;
  const stream = new ReadableStream({
    start(controller) {
      // @ts-expect-error An owning controller enqueues with a transfer list.
      controller.enqueue(owned, { transfer: [owned] });
    },
    type: "owning",
  });
  if (owned.detached !== true) {
    throw new Error("owning detached");
  }
  const chunk = await stream.getReader().read();
  if (
    !(chunk.value instanceof ArrayBuffer)
    || new Uint8Array(chunk.value).join(",") !== "8,7,6,5"
  ) {
    throw new Error("owning bytes");
  }
});

function delay(): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, 0);
  });
}

function withTimeout<T>(promise: Promise<T>, message: string): Promise<T> {
  let timer: number | undefined;
  const timeout = new Promise<T>((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error(message));
    }, 2000);
  });
  return Promise.race([promise, timeout]).finally(() => {
    clearTimeout(timer);
  });
}

function cloneFailure(run: () => void): { name: string; message: string } {
  try {
    run();
    return { name: "", message: "" };
  } catch (caught) {
    return {
      name: caught instanceof Error ? caught.name : "",
      message: caught instanceof Error ? String(caught.message) : "",
    };
  }
}

async function rejection(promise: Promise<unknown>): Promise<{
  value: unknown;
  name: string;
  message: string;
}> {
  try {
    return { value: await promise, name: "", message: "" };
  } catch (caught) {
    return {
      value: caught,
      name: caught instanceof Error ? caught.name : "",
      message: caught instanceof Error ? String(caught.message) : "",
    };
  }
}

function turn(): Promise<void> {
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => {
      resolve();
    };
    channel.port2.postMessage(null);
  });
}

async function until(ready: () => boolean, message: string): Promise<void> {
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

interface WorkerReply {
  kind?: string;
  message?: string;
  value?: unknown;
  restDone?: boolean;
  stream?: unknown;
}

function messageQueue(worker: Worker): { take(): Promise<WorkerReply> } {
  const items: WorkerReply[] = [];
  const resolvers: Array<(value: WorkerReply) => void> = [];
  const rejecters: Array<(reason: Error) => void> = [];
  let failure: Error | undefined;
  worker.onmessage = (event) => {
    const resolve = resolvers.shift();
    const data: WorkerReply = event.data;
    if (resolve) {
      resolve(data);
    } else {
      items.push(data);
    }
  };
  worker.onerror = (event) => {
    if (failure) {
      return;
    }
    failure = new Error(event.message || "worker error");
    while (rejecters.length > 0) {
      const reject = rejecters.shift();
      if (reject) {
        reject(failure);
      }
    }
  };
  return {
    take() {
      if (items.length > 0) {
        return Promise.resolve(items.shift()!);
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

function assertReply(reply: WorkerReply, kind: string): void {
  if (!reply || reply.kind === "error") {
    throw new Error(reply && reply.message ? reply.message : "worker failed");
  }
  if (reply.kind !== kind) {
    throw new Error("worker reply " + String(reply && reply.kind));
  }
}

function chunkStream(value: unknown): ReadableStream {
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

  let controller: ReadableStreamDefaultController<string> | undefined;
  const locked = new ReadableStream<string>({
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
  controller!.close();
});

Yuzuko.test("transferred readables keep the base prototype and the source state", async () => {
  class SubRead extends ReadableStream<string> {}
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

  let closedController: ReadableStreamDefaultController<unknown> | undefined;
  const closed = new ReadableStream({
    start(controller) {
      closedController = controller;
    },
  });
  closedController!.close();
  const movedClosed = structuredClone(closed, { transfer: [closed] });
  if ((await movedClosed.getReader().read()).done !== true) {
    throw new Error("closed");
  }

  let erroredController: ReadableStreamDefaultController<unknown> | undefined;
  const errored = new ReadableStream({
    start(controller) {
      erroredController = controller;
    },
  });
  erroredController!.error("foo");
  const movedErrored = structuredClone(errored, { transfer: [errored] });
  const erroredRead = await rejection(movedErrored.getReader().read());
  if (erroredRead.value !== "foo") {
    throw new Error("errored " + String(erroredRead.value));
  }

  const reason = new DOMException("nope", "NotFoundError");
  let exceptionController: ReadableStreamDefaultController<unknown> | undefined;
  const exceptional = new ReadableStream({
    start(controller) {
      exceptionController = controller;
    },
  });
  exceptionController!.error(reason);
  const movedException = structuredClone(exceptional, { transfer: [exceptional] });
  const exceptionRead = await rejection(movedException.getReader().read());
  if (
    exceptionRead.name !== "NotFoundError"
    || exceptionRead.message !== "nope"
  ) {
    throw new Error("exception " + exceptionRead.name + " " + exceptionRead.message);
  }

  const disturbed = new ReadableStream({
    start(controller) {
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
  let controller: ReadableStreamDefaultController<unknown> | undefined;
  const readable = new ReadableStream<unknown>({
    start(control) {
      controller = control;
    },
  });
  const moved = structuredClone(readable, { transfer: [readable] });
  const reader = moved.getReader();
  controller!.enqueue(new ReadableStream());
  const readFailure = await rejection(reader.read());
  if (readFailure.name !== "DataCloneError") {
    throw new Error("read " + readFailure.name);
  }
  let enqueueName = "";
  try {
    controller!.enqueue("later");
  } catch (caught) {
    enqueueName = caught instanceof Error ? caught.name : "";
  }
  if (enqueueName !== "TypeError") {
    throw new Error("enqueue " + enqueueName);
  }
});

Yuzuko.test("chunk getters run when a transferred readable posts the chunk", async () => {
  let flag = false;
  let controller: ReadableStreamDefaultController<{ getter: string }> | undefined;
  const readable = new ReadableStream<{ getter: string }>({
    start(control) {
      controller = control;
    },
  });
  const moved = structuredClone(readable, { transfer: [readable] });
  const reader = moved.getReader();
  controller!.enqueue({
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
  const idle: string[] = [];
  const quiet = new ReadableStream({
    pull() {
      idle.push("pull");
    },
  }, { highWaterMark: 0 });
  await delay();
  const earlyPulls = idle.length;
  if (earlyPulls !== 0) {
    throw new Error("early pull " + earlyPulls);
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
  let controller: ReadableByteStreamController | undefined;
  const readable = new ReadableStream({
    type: "bytes",
    start(control) {
      controller = control;
      control.enqueue(new Uint8Array(1024));
      control.enqueue(new Uint8Array(1024));
    },
    pull() {
      pulls += 1;
      controller!.enqueue(new Uint8Array(1024));
    },
  }, { highWaterMark: 512 });
  await delay();
  const setupPulls = pulls;
  if (setupPulls !== 0) {
    throw new Error("setup pulls " + setupPulls);
  }
  const moved = structuredClone(readable, { transfer: [readable] });
  await delay();
  const transferPulls = pulls;
  if (transferPulls !== 0) {
    throw new Error("transfer pulls " + transferPulls);
  }
  let byobName = "";
  let byobMessage = "";
  try {
    moved.getReader({ mode: "byob" });
  } catch (caught) {
    byobName = caught instanceof Error ? caught.name : "";
    byobMessage = caught instanceof Error ? String(caught.message) : "";
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
  let resolveWrite: (() => void) | undefined;
  let markEntered: (() => void) | undefined;
  const entered = new Promise<void>((resolve) => {
    markEntered = resolve;
  });
  const original = new WritableStream({
    write() {
      markEntered!();
      return new Promise<void>((resolve) => {
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
  resolveWrite!();
  await second;
  if (!settled) {
    throw new Error("second write stayed pending");
  }
});

Yuzuko.test("abort and a failed write cross a transferred writable", async () => {
  let reason = "missing";
  let markAbort: (() => void) | undefined;
  const aborted = new Promise<void>((resolve) => {
    markAbort = resolve;
  });
  const original = new WritableStream({
    abort(value) {
      reason = value;
      markAbort!();
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

  const reasons: unknown[] = [];
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
  const abortReason = reasons[0];
  if (!(abortReason instanceof DOMException) || abortReason.name !== "DataCloneError") {
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
    let resolveSink: ((chunk: unknown) => void) | undefined;
    const sink = new Promise<unknown>((resolve) => {
      resolveSink = resolve;
    });
    const writable = new WritableStream({
      write(chunk: unknown) {
        resolveSink!(chunk);
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
    const stream = reply.stream;
    if (!(stream instanceof ReadableStream)) {
      throw new Error("worker stream");
    }
    const reader = stream.getReader();
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

declare function addEventListener(
  type: string,
  listener: EventListenerOrEventListenerObject | null,
  options?: boolean | AddEventListenerOptions,
): void;
declare function removeEventListener(
  type: string,
  listener: EventListenerOrEventListenerObject | null,
  options?: boolean | EventListenerOptions,
): void;

function nextSelfMessage(): Promise<MessageEvent> {
  return new Promise((resolve) => {
    addEventListener(
      "message",
      // @ts-expect-error The message listener receives a MessageEvent.
      (event) => resolve(event),
      { once: true },
    );
  });
}

Yuzuko.test("postMessage transfers a readable stream to this realm", async () => {
  const readable = chunkStream("self");
  const pending = nextSelfMessage();
  postMessage(readable, "*", [readable]);
  if (!readable.locked) {
    throw new Error("original locked");
  }
  const event = await pending;
  if (event.source !== null) {
    throw new Error("source");
  }
  if (!Array.isArray(event.ports) || event.ports.length !== 0) {
    throw new Error("ports " + event.ports.length);
  }
  if (event.data === readable || !(event.data instanceof ReadableStream)) {
    throw new Error("identity");
  }
  const reader = event.data.getReader();
  const first = await reader.read();
  if (first.value !== "self" || first.done) {
    throw new Error("chunk");
  }
  if ((await reader.read()).done !== true) {
    throw new Error("done");
  }
});

Yuzuko.test("postMessage ports lists a transferred MessagePort", async () => {
  const channel = new MessageChannel();
  const readable = chunkStream("with-port");
  const pending = nextSelfMessage();
  postMessage(
    { stream: readable, port: channel.port1 },
    { targetOrigin: "*", transfer: [readable, channel.port1] },
  );
  const event = await pending;
  if (event.ports.length !== 1 || event.ports[0] !== event.data.port) {
    throw new Error("ports");
  }
  if (Object.isFrozen(event.ports) !== true) {
    throw new Error("frozen");
  }
  if (!(event.data.stream instanceof ReadableStream)) {
    throw new Error("stream");
  }
  const spare = new MessageChannel();
  const again = nextSelfMessage();
  postMessage({}, "*", [spare.port1]);
  const second = await again;
  if (second.ports.length !== 1 || second.ports[0] !== spare.port1) {
    throw new Error("unreferenced port");
  }
  const heard = new Promise((resolve) => {
    spare.port2.onmessage = (event) => resolve(event.data);
  });
  spare.port1.postMessage("open");
  if ((await heard) !== "open") {
    throw new Error("unreferenced still open");
  }
  const reader = event.data.stream.getReader();
  const first = await reader.read();
  if (first.value !== "with-port" || first.done) {
    throw new Error("chunk");
  }
});

async function delivered(run: () => void, value: string): Promise<void> {
  const pending = nextSelfMessage();
  run();
  const event = await pending;
  if (event.data !== value) {
    throw new Error("data " + String(event.data));
  }
  if (event.source !== null) {
    throw new Error("source");
  }
  if (event.isTrusted !== true) {
    throw new Error("trusted");
  }
}

Yuzuko.test("postMessage delivers / and serializes a foreign origin", async () => {
  await delivered(() => postMessage("slash", "/"), "slash");
  console.log("postMessage / delivered");
  await delivered(() => postMessage("one"), "one");
  console.log("postMessage one argument delivered");
  await delivered(
    () => postMessage("opts", { targetOrigin: "/" }),
    "opts",
  );
  await delivered(() => postMessage("empty", {}), "empty");
  console.log("postMessage targetOrigin / delivered");
  await delivered(() => postMessage("star", "*"), "star");
  console.log("postMessage * delivered");

  const bad = chunkStream("bad");
  const syntax = cloneFailure(() => postMessage(bad, "not a url", [bad]));
  const early = cloneFailure(() => postMessage(() => {}, "not a url"));
  if (
    syntax.name !== "SyntaxError" ||
    early.name !== "SyntaxError" ||
    bad.locked
  ) {
    throw new Error(
      "syntax " + syntax.name + " " + early.name + " " + bad.locked,
    );
  }
  console.log("postMessage unparsable SyntaxError");

  const foreign = chunkStream("foreign");
  const idle = chunkStream("idle");
  let saw = false;
  const stop = () => {
    saw = true;
  };
  addEventListener("message", stop);
  postMessage(foreign, "https://example.test", [foreign]);
  postMessage("nope", "https://example.test", [idle]);
  if (!foreign.locked || idle.locked) {
    throw new Error("transfer " + foreign.locked + " " + idle.locked);
  }
  await turn();
  await turn();
  removeEventListener("message", stop);
  if (saw) {
    throw new Error("foreign delivered");
  }
  console.log("postMessage foreign origin neutered no message");

  const clone = cloneFailure(() =>
    postMessage(() => {}, "https://example.test"),
  );
  if (clone.name !== "DataCloneError") {
    throw new Error("clone " + clone.name + " " + clone.message);
  }
  console.log("postMessage uncloneable DataCloneError");
});

Yuzuko.test("a transfer detached during serialization throws DataCloneError", async () => {
  const transfersDuring = (post: (value: unknown, buffer: ArrayBuffer) => void) => {
    const buffer = new ArrayBuffer(8);
    new Uint8Array(buffer)[0] = 7;
    let stolen: ArrayBuffer | undefined;
    const value = {
      buffer,
      get steal() {
        stolen = buffer.transfer();
        return 0;
      },
    };
    try {
      post(value, buffer);
    } catch (error) {
      if (!(error instanceof DOMException) || error.name !== "DataCloneError") {
        throw new Error("unexpected " + String(error));
      }
      if (stolen === undefined || new Uint8Array(stolen)[0] !== 7) {
        throw new Error("stolen buffer");
      }
      return;
    }
    throw new Error("transfer of a detached buffer succeeded");
  };
  transfersDuring((value, buffer) => {
    structuredClone(value, { transfer: [buffer] });
  });
  const channel = new MessageChannel();
  transfersDuring((value, buffer) => {
    channel.port1.postMessage(value, [buffer]);
  });
  channel.port1.close();
  channel.port2.close();
});

Yuzuko.test("a closed MessagePort still serializes and transfers", () => {
  const channel = new MessageChannel();
  channel.port1.close();
  const buffer = new ArrayBuffer(8);
  channel.port1.postMessage(buffer, [buffer]);
  if (buffer.byteLength !== 0) {
    throw new Error("the transferred buffer stayed attached");
  }
  try {
    channel.port1.postMessage(() => {});
    throw new Error("a function was posted");
  } catch (error) {
    if (!(error instanceof DOMException) || error.name !== "DataCloneError") {
      throw error;
    }
  }
  channel.port2.close();
});

Yuzuko.test("MessagePort.postMessage lists transferred ports", async () => {
  const channel = new MessageChannel();
  const carried = new MessageChannel();
  const received = new Promise<MessageEvent>((resolve) => {
    channel.port2.onmessage = resolve;
  });
  channel.port1.postMessage({ note: 1 }, [carried.port1]);
  const event = await received;
  if (event.ports.length !== 1 || !(event.ports[0] instanceof MessagePort)) {
    throw new Error("ports " + event.ports.length);
  }
  // A port the value does not reach stays the same object in this realm,
  // as for postMessage on the main global.
  if (event.ports[0] !== carried.port1) {
    throw new Error("the listed port is another object");
  }
  const echoed = new Promise<MessageEvent>((resolve) => {
    carried.port2.onmessage = resolve;
  });
  event.ports[0].postMessage("through");
  if ((await echoed).data !== "through") {
    throw new Error("the transferred port is not entangled");
  }
  channel.port1.close();
  channel.port2.close();
  event.ports[0].close();
  carried.port2.close();
});

Yuzuko.test("DataCloneError uses the realm's own DOMException", () => {
  const realm = globalThis as unknown as { DOMException: unknown };
  const original = realm.DOMException;
  let thrown: unknown;
  try {
    realm.DOMException = function decoy() {
      throw new Error("the decoy constructor ran");
    };
    structuredClone(() => {});
  } catch (error) {
    thrown = error;
  } finally {
    realm.DOMException = original;
  }
  if (!(thrown instanceof DOMException) || thrown.name !== "DataCloneError") {
    throw new Error(`unexpected ${String(thrown)}`);
  }
});

Yuzuko.test("a received SharedArrayBuffer keeps the intrinsic prototype", async () => {
  const original = globalThis.SharedArrayBuffer;
  const channel = new MessageChannel();
  try {
    const decoy = { prototype: { decoy: true } };
    // @ts-expect-error replace the global binding with a decoy.
    globalThis.SharedArrayBuffer = decoy;
    const received = new Promise<unknown>((resolve) => {
      channel.port2.onmessage = (event) => resolve(event.data);
    });
    channel.port1.postMessage(new original(8));
    const data = await received;
    if (Object.getPrototypeOf(data) !== original.prototype) {
      throw new Error("the buffer took the replaced global's prototype");
    }
  } finally {
    globalThis.SharedArrayBuffer = original;
    channel.port1.close();
    channel.port2.close();
  }
});

Yuzuko.test("readAsText reads charset from the parsed MIME type", async () => {
  const bytes = new Uint8Array([0x68, 0x00, 0x69, 0x00]);
  const quoted = new Blob([bytes], { type: 'text/plain;x="y;charset=utf-16le"' });
  // The charset text sits inside a quoted value, so the type has no charset.
  const read = await new Promise<string>((resolve) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.readAsText(quoted);
  });
  if (read !== "h\0i\0") {
    throw new Error(JSON.stringify(read));
  }
  const declared = new Blob([bytes], { type: "text/plain;charset=utf-16le" });
  const decoded = await new Promise<string>((resolve) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.readAsText(declared);
  });
  if (decoded !== "hi") {
    throw new Error(JSON.stringify(decoded));
  }
});

Yuzuko.test("a port cannot transfer itself", () => {
  const channel = new MessageChannel();
  let name = "";
  try {
    channel.port1.postMessage("hi", [channel.port1]);
  } catch (error) {
    name = (error as Error).name;
  }
  channel.port1.close();
  channel.port2.close();
  if (name !== "DataCloneError") {
    throw new Error(name);
  }
});
