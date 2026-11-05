import { add, answer } from "./wasm/add.wasm";
import wasmText from "./wasm/add.wasm" with { type: "text" };
import wasmBytes from "./wasm/add.wasm" with { type: "bytes" };
import { marker } from "./text_bytes/side.js";
import { marker as styled } from "./text_bytes/side.js" with { type: "css" };
import { marker as scripted } from "./text_bytes/side.js" with {
  type: "javascript",
};
import sideText from "./text_bytes/side.js" with { type: "text" };
import sideBytes from "./text_bytes/side.js" with { type: "bytes" };
import onlyText from "./text_bytes/unevaluated.js" with { type: "text" };
import { count } from "./text_bytes/counter.ts";
import note from "./text_bytes/note.txt" with { type: "text" };
import noteAgain from "./text_bytes/note.txt" with { type: "text" };
import bytes from "./text_bytes/note.txt" with { type: "bytes" };
import bytesAgain from "./text_bytes/note.txt" with { type: "bytes" };
import emptyText from "./text_bytes/empty" with { type: "text" };
import emptyBytes from "./text_bytes/empty" with { type: "bytes" };
import badText from "./text_bytes/bad.bin" with { type: "text" };
import badBytes from "./text_bytes/bad.bin" with { type: "bytes" };
import bomText from "./text_bytes/bom.txt" with { type: "text" };
import nulText from "./text_bytes/null-byte.txt" with { type: "text" };
import nulBytes from "./text_bytes/null-byte.txt" with { type: "bytes" };
import rawJson from "./text_bytes/data.json" with { type: "text" };
import parsedJson from "./text_bytes/data.json" with { type: "json" };
import jsonBytes from "./text_bytes/data.json" with { type: "bytes" };
import defer * as deferredBom from "./text_bytes/bom.txt" with { type: "text" };
import defer * as deferredEmpty from "./text_bytes/empty" with { type: "bytes" };

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function throws(fn: () => unknown): unknown {
  try {
    fn();
  } catch (error) {
    return error;
  }
  return undefined;
}

async function rejects(promise: Promise<unknown>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error("expected a rejection");
}

function bytesEqual(view: Uint8Array, expected: number[], label: string): void {
  assert(view.length === expected.length, label + " length " + view.length);
  for (let i = 0; i < expected.length; i += 1) {
    assert(view[i] === expected[i], label + " byte " + i);
  }
}

function checkImmutable(view: Uint8Array, label: string): void {
  assert(view instanceof Uint8Array, label + " view");
  assert(view.buffer instanceof ArrayBuffer, label + " buffer");
  assert(!(view.buffer instanceof SharedArrayBuffer), label + " shared");
  assert(view.buffer.immutable === true, label + " immutable");
  assert(view.buffer.resizable === false, label + " resizable");
  assert(view.buffer.detached === false, label + " detached");
  assert(view.byteLength === view.buffer.byteLength, label + " sizes");
  const resize = throws(() => view.buffer.resize(0));
  assert(resize instanceof TypeError, label + " resize " + String(resize));
  const transfer = throws(() => view.buffer.transfer());
  assert(transfer instanceof TypeError, label + " transfer " + String(transfer));
  const again = throws(() => view.buffer.transferToImmutable());
  assert(
    again instanceof TypeError,
    label + " transferToImmutable " + String(again),
  );
  assert(view.buffer.detached === false, label + " stayed attached");
  if (view.length > 0) {
    const saved = view[0];
    const write = throws(() => {
      view[0] = (saved + 1) % 256;
    });
    assert(write instanceof TypeError, label + " write " + String(write));
    assert(view[0] === saved, label + " unchanged");
  }
}

Yuzuko.test("text modules export the decoded source", () => {
  assert(note === "Hello World!\n", JSON.stringify(note));
  assert(noteAgain === note, "repeated text");
  assert(typeof note === "string", "text type");
  assert(emptyText === "", "empty text");
  assert(badText === "\uFFFD\uFFFDA", JSON.stringify(badText));
  assert(bomText === "\uFEFFhi", JSON.stringify(bomText));
  assert(bomText.charCodeAt(0) === 0xfeff, "bom kept");
  assert(nulText === "a\0b", "nul text");
  assert(nulText.length === 3, "nul length");
  assert(rawJson === "{\"ok\":true}\n", JSON.stringify(rawJson));
  assert(parsedJson.ok === true, "json still parses");
  assert(typeof wasmText === "string", "wasm text");
  assert(sideText.startsWith("import { bump }"), "side source");
  assert(onlyText.includes("evaluated"), "unevaluated source");
  assert(marker === 1 && styled === 1 && scripted === 1, "script markers");
  assert(count() === 1, "script ran once, text did not");
  assert(add(2, 3) === 5, "wasm export");
  assert(answer === 7, "wasm global");
});

Yuzuko.test("bytes modules export an immutable Uint8Array", () => {
  bytesEqual(
    bytes,
    [72, 101, 108, 108, 111, 32, 87, 111, 114, 108, 100, 33, 10],
    "note",
  );
  assert(bytes === bytesAgain, "repeated bytes");
  checkImmutable(bytes, "note");
  assert(emptyBytes.length === 0, "empty length");
  assert(emptyBytes.buffer.byteLength === 0, "empty buffer");
  checkImmutable(emptyBytes, "empty");
  bytesEqual(badBytes, [0xff, 0xfe, 0x41], "bad");
  checkImmutable(badBytes, "bad");
  bytesEqual(nulBytes, [0x61, 0x00, 0x62], "nul");
  checkImmutable(jsonBytes, "json");
  assert(jsonBytes[0] === 0x7b, "json byte");
  checkImmutable(sideBytes, "side");
  assert(sideBytes[0] === "i".charCodeAt(0), "side byte");
  checkImmutable(wasmBytes, "wasm");
  bytesEqual(
    wasmBytes.subarray(0, 4),
    [0x00, 0x61, 0x73, 0x6d],
    "wasm magic",
  );
});

Yuzuko.test("deferred text and bytes evaluate on observation", () => {
  assert(deferredBom[Symbol.toStringTag] === "Deferred Module", "text tag");
  assert(deferredEmpty[Symbol.toStringTag] === "Deferred Module", "bytes tag");
  assert(deferredBom.default === "\uFEFFhi", "deferred text");
  assert(deferredEmpty.default instanceof Uint8Array, "deferred bytes");
  assert(deferredEmpty.default.length === 0, "deferred empty");
  assert(deferredEmpty.default.buffer.immutable === true, "deferred immutable");
  assert(deferredEmpty.default === emptyBytes, "defer shares the bytes module");
});

Yuzuko.test("the module map keeps each type", async () => {
  const lateFail = await rejects(import("./text_bytes/late.txt"));
  assert(lateFail instanceof Error, String(lateFail));
  const late = await import("./text_bytes/late.txt", { with: { type: "text" } });
  assert(late.default === "late\n", JSON.stringify(late.default));
  const lateFailAgain = await rejects(import("./text_bytes/late.txt"));
  assert(lateFailAgain instanceof Error, String(lateFailAgain));
  const lateBytes = await import("./text_bytes/late.txt", {
    with: { type: "bytes" },
  });
  assert(lateBytes.default instanceof Uint8Array, "late bytes");
  bytesEqual(lateBytes.default, [108, 97, 116, 101, 10], "late");
  const lateBytesAgain = await import("./text_bytes/late.txt", {
    with: { type: "bytes" },
  });
  assert(lateBytesAgain.default === lateBytes.default, "dynamic bytes cache");

  const bomBytes = await import("./text_bytes/bom.txt", { with: { type: "bytes" } });
  assert(bomBytes.default instanceof Uint8Array, "bom bytes");
  bytesEqual(bomBytes.default, [0xef, 0xbb, 0xbf, 0x68, 0x69], "bom");
  const bomPlain = await rejects(import("./text_bytes/bom.txt"));
  assert(bomPlain instanceof Error, String(bomPlain));
  assert(deferredBom.default === bomText, "bom text module");

  const dynamicNote = await import("./text_bytes/note.txt", {
    with: { type: "bytes" },
  });
  assert(dynamicNote.default === bytes, "static and dynamic bytes");
  const dynamicText = await import("./text_bytes/note.txt", {
    with: { type: "text" },
  });
  assert(dynamicText.default === note, "static and dynamic text");

  const jsonFail = await rejects(import("./text_bytes/data.json"));
  assert(jsonFail instanceof TypeError, String(jsonFail));
  assert(String(jsonFail).includes('type: "json"'), String(jsonFail));

  const deferredNul = await import.defer("./text_bytes/null-byte.txt", {
    with: { type: "text" },
  });
  assert(deferredNul[Symbol.toStringTag] === "Deferred Module", "nul tag");
  assert(deferredNul.default === "a\0b", "deferred nul");
});

Yuzuko.test("ordinary buffers can become immutable", () => {
  const plain = new ArrayBuffer(4);
  assert(plain.immutable === false, "plain immutable");
  const view = new Uint8Array(plain);
  view[0] = 7;
  const frozen = plain.transferToImmutable();
  assert(plain.detached === true, "source detached");
  assert(frozen.immutable === true, "frozen");
  assert(frozen.byteLength === 4, "frozen length");
  assert(new Uint8Array(frozen)[0] === 7, "frozen byte");
  const sliceSource = new Uint8Array([9, 8, 7]).buffer;
  const sliced = sliceSource.sliceToImmutable(1, 3);
  assert(sliced.immutable === true, "slice immutable");
  assert(sliced.byteLength === 2, "slice length");
  assert(new Uint8Array(sliced)[0] === 8, "slice byte");
  assert(sliceSource.detached === false, "slice source attached");
  assert(sliceSource.immutable === false, "slice source mutable");
});
