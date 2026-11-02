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

  const readable = new ReadableStream();
  if (
    cloneError(() => structuredClone(readable, { transfer: [readable] }))
      !== "DataCloneError"
  ) {
    throw new Error("stream transfer");
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
