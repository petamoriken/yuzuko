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
