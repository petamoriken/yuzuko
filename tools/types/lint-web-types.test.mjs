import assert from "node:assert/strict";
import test from "node:test";
import { allowFromJson, compareTexts } from "./compare.mjs";

const allow = allowFromJson({
  skipTypes: ["Window"],
  skipOverloads: [{ member: "GPUCommandEncoder.copyBufferToBuffer", params: 3 }],
  groups: [
    {
      members: ["DataTransfer.items", "GPURenderBundleEncoder.pushDebugGroup"],
    },
  ],
});

function paths(yuzuko, web) {
  return compareTexts(yuzuko, web, allow).findings.map((finding) => `${finding.kind} ${finding.path}`);
}

test("reports a member that @types/web adds", () => {
  const found = paths("interface GPUBuffer { label: string; }", "interface GPUBuffer { label: string; mapState: string; }");
  assert.deepEqual(found, ["missing GPUBuffer.mapState"]);
});

test("ignores a member only yuzuko declares", () => {
  const found = paths("interface GPUBuffer { label: string; flip: boolean; }", "interface GPUBuffer { label: string; }");
  assert.deepEqual(found, []);
});

test("follows AllowSharedBufferSource and keeps the wider yuzuko spelling", () => {
  const buffers = "type BufferSource = ArrayBufferView<ArrayBuffer> | ArrayBuffer; type AllowSharedBufferSource = ArrayBufferLike | ArrayBufferView<ArrayBufferLike>;";
  const behind = paths(
    `${buffers} interface TextDecoder { decode(input?: BufferSource): string; }`,
    `${buffers} interface TextDecoder { decode(input?: AllowSharedBufferSource): string; }`,
  );
  assert.deepEqual(behind, ["overload TextDecoder.decode"]);
  const ahead = paths(
    `${buffers} interface AudioDataInit { data: AllowSharedBufferSource; }`,
    `${buffers} interface AudioDataInit { data: BufferSource; }`,
  );
  assert.deepEqual(ahead, []);
});

test("follows a TypeScript 7 typed-array argument and keeps a more specific one", () => {
  const behind = paths(
    "interface ReadableStreamBYOBRequest { view: Uint8Array | null; }",
    "interface ReadableStreamBYOBRequest { view: Uint8Array<ArrayBuffer> | null; }",
  );
  assert.deepEqual(behind, ["type ReadableStreamBYOBRequest.view"]);
  const ahead = paths(
    "interface WaveShaperOptions { curve?: Float32Array<ArrayBufferLike>; }",
    "interface WaveShaperOptions { curve?: Float32Array; }",
  );
  assert.deepEqual(ahead, []);
});

test("treats ArrayBuffer as more specific than ArrayBufferLike", () => {
  const found = paths(
    "interface ReadableByteStreamController { enqueue(chunk: ArrayBufferView<ArrayBufferLike>): void; }",
    "interface ReadableByteStreamController { enqueue(chunk: ArrayBufferView<ArrayBuffer>): void; }",
  );
  assert.deepEqual(found, ["overload ReadableByteStreamController.enqueue"]);
});

test("follows an Exclude constraint", () => {
  const found = paths(
    "type BufferSource = ArrayBufferView<ArrayBuffer> | ArrayBuffer; interface Crypto { getRandomValues<T extends ArrayBufferView>(array: T): T; }",
    "type BufferSource = ArrayBufferView<ArrayBuffer> | ArrayBuffer; interface Crypto { getRandomValues<T extends Exclude<BufferSource, ArrayBuffer>>(array: T): T; }",
  );
  assert.deepEqual(found, ["overload Crypto.getRandomValues"]);
});

test("keeps a string literal and an extra union member", () => {
  assert.deepEqual(
    paths('interface TextEncoder { readonly encoding: "utf-8"; }', "interface TextEncoder { readonly encoding: string; }"),
    [],
  );
  assert.deepEqual(
    paths(
      'type GPUStorageTextureAccess = "read-only" | "write-only" | "atomic";',
      'type GPUStorageTextureAccess = "read-only" | "write-only";',
    ),
    [],
  );
});

test("reports a string-literal member @types/web adds", () => {
  const found = paths(
    'type GPUFeatureName = "subgroups";',
    'type GPUFeatureName = "subgroups" | "texture-component-swizzle";',
  );
  assert.deepEqual(found, ["type GPUFeatureName"]);
  assert.match(compareTexts('type GPUFeatureName = "subgroups";', 'type GPUFeatureName = "subgroups" | "texture-component-swizzle";', allow).findings[0].note, /texture-component-swizzle/);
});

test("matches number aliases, readonly arrays, iterables, promise void, and hex", () => {
  const yuzuko = `
    type GPUSize64 = number;
    type GPUColor = number[] | GPUColorDict;
    interface GPUColorDict { r: number; }
    interface GPUBuffer { readonly usage: number; }
    interface GPUQueue { submit(buffers: GPUCommandBuffer[]): void; writeTexture(size: GPUColor): void; }
    interface GPUCommandBuffer {}
    interface WebTransport { ready: Promise<void>; }
    declare const GPUMapMode: { readonly READ: 0x0001; };
  `;
  const web = `
    type GPUSize64 = number;
    interface GPUBuffer { readonly usage: GPUSize64; }
    interface GPUQueue { submit(buffers: Iterable<GPUCommandBuffer>): void; writeTexture(size: Iterable<number>): void; }
    interface GPUCommandBuffer {}
    interface WebTransport { ready: Promise<undefined>; }
    declare namespace GPUMapMode { const READ: 0x1; }
  `;
  assert.deepEqual(paths(yuzuko, web), []);
});

test("matches ReadonlyArray and union order", () => {
  assert.deepEqual(
    paths(
      "interface ClipboardItem { readonly types: readonly string[]; }",
      "interface ClipboardItem { readonly types: ReadonlyArray<string>; }",
    ),
    [],
  );
  assert.deepEqual(
    paths('type EndingType = "transparent" | "opaque";', 'type EndingType = "opaque" | "transparent";'),
    [],
  );
});

test("keeps a specific algorithm union", () => {
  const found = paths(
    "interface Algorithm { name: string; } type AlgorithmIdentifier = Algorithm | string; interface RsaParams { name: string; } interface SubtleCrypto { importKey(algorithm: RsaParams): void; }",
    "interface Algorithm { name: string; } type AlgorithmIdentifier = Algorithm | string; interface RsaParams { name: string; } interface SubtleCrypto { importKey(algorithm: AlgorithmIdentifier): void; }",
  );
  assert.deepEqual(found, []);
});

test("skips Window, allowlisted members, and the short copy overload", () => {
  const yuzuko = `
    interface Window extends EventTarget {}
    interface EventTarget {}
    interface DataTransfer { dropEffect: string; }
    interface GPURenderBundleEncoder { finish(): void; }
    interface GPUCommandEncoder { copyBufferToBuffer(source: GPUBuffer, sourceOffset: number, destination: GPUBuffer, destinationOffset: number, size: number): void; }
    interface GPUBuffer {}
  `;
  const web = `
    interface Window extends EventTarget { document: Document; }
    interface EventTarget {}
    interface Document {}
    interface DataTransfer { dropEffect: string; items: DataTransferItemList; }
    interface DataTransferItemList {}
    interface GPURenderBundleEncoder { finish(): void; pushDebugGroup(label: string): void; }
    interface GPUCommandEncoder {
      copyBufferToBuffer(source: GPUBuffer, destination: GPUBuffer, size?: number): void;
      copyBufferToBuffer(source: GPUBuffer, sourceOffset: number, destination: GPUBuffer, destinationOffset: number, size?: number): void;
    }
    interface GPUBuffer {}
  `;
  assert.deepEqual(paths(yuzuko, web), []);
});

test("matches a class to an interface and a var", () => {
  const found = paths(
    "declare class EventTarget { constructor(); addEventListener(type: string): void; }",
    "interface EventTarget { addEventListener(type: string): void; } declare var EventTarget: { prototype: EventTarget; new(): EventTarget; };",
  );
  assert.deepEqual(found, []);
});

test("does not report a DOM-only interface", () => {
  assert.deepEqual(paths("interface GPU { requestAdapter(): void; }", "interface GPU { requestAdapter(): void; } interface HTMLElement {}"), []);
});

test("ignores an extra forEach thisArg", () => {
  const found = paths(
    "interface Headers { forEach(callback: (value: string) => void): void; }",
    "interface Headers { forEach(callback: (value: string) => void, thisArg?: any): void; }",
  );
  assert.deepEqual(found, []);
});

test("drops an undeclared union member and reports a declared one", () => {
  const quiet = paths(
    "interface ImageBitmap {} interface OffscreenCanvas {} type CanvasImageSource = ImageBitmap | OffscreenCanvas;",
    "interface ImageBitmap {} interface OffscreenCanvas {} interface HTMLImageElement {} type CanvasImageSource = HTMLImageElement | ImageBitmap | OffscreenCanvas;",
  );
  assert.deepEqual(quiet, []);
  const flagged = paths(
    "interface MessagePort {} interface ServiceWorker {} type MessageEventSource = MessagePort | null;",
    "interface MessagePort {} interface ServiceWorker {} type MessageEventSource = MessagePort | ServiceWorker | null;",
  );
  assert.deepEqual(flagged, ["type MessageEventSource"]);
});

test("keeps Float32Array generics and an Iterable of numbers", () => {
  assert.deepEqual(
    paths(
      "interface AudioContext { createPeriodicWave(real: Float32Array<ArrayBufferLike>, imag: Float32Array<ArrayBufferLike>): void; }",
      "interface AudioContext { createPeriodicWave(real: number[] | Float32Array, imag: Iterable<number>): void; }",
    ),
    [],
  );
  assert.deepEqual(
    paths(
      "interface AudioParam { setValueCurveAtTime(values: Iterable<number>): void; }",
      "interface AudioParam { setValueCurveAtTime(values: number[] | Float32Array): void; }",
    ),
    [],
  );
  assert.deepEqual(
    paths(
      "interface ReadableStreamBYOBRequest { view: Uint8Array | null; }",
      "interface ReadableStreamBYOBRequest { view: Uint8Array<ArrayBuffer> | null; }",
    ),
    ["type ReadableStreamBYOBRequest.view"],
  );
});

test("matches a callback interface, an iterator alias, and Array<T>", () => {
  assert.deepEqual(
    paths(
      "interface DOMException {} interface AudioDecoderInit { error: (error: DOMException) => void; }",
      "interface DOMException {} interface WebCodecsErrorCallback { (error: DOMException): void; } interface AudioDecoderInit { error: WebCodecsErrorCallback; }",
    ),
    [],
  );
  assert.deepEqual(
    paths(
      "interface URLSearchParams { entries(): IterableIterator<[string, string]>; }",
      "interface URLSearchParamsIterator<T> { next(): T; } interface URLSearchParams { entries(): URLSearchParamsIterator<[string, string]>; }",
    ),
    [],
  );
  assert.deepEqual(
    paths(
      "interface JsonWebKey { oth?: Array<{ d?: string; r?: string; t?: string }>; }",
      "interface RsaOtherPrimesInfo { d?: string; r?: string; t?: string; } interface JsonWebKey { oth?: RsaOtherPrimesInfo[]; }",
    ),
    [],
  );
});

test("matches an alias to an interface with the same fields", () => {
  assert.deepEqual(
    paths(
      "type GPUTexelCopyBufferLayout = { offset?: number; };",
      "interface GPUTexelCopyBufferLayout { offset?: number; }",
    ),
    [],
  );
});

test("ignores an event-map listener and still reports a new parameter", () => {
  const event = "interface Event {} interface EventListener {} interface EventListenerOptions {}";
  assert.deepEqual(
    paths(
      `${event} interface AbortSignal { removeEventListener(type: string, callback: EventListener | null, options?: EventListenerOptions | boolean): void; }`,
      `${event} interface AbortSignalEventMap {} interface AbortSignal { removeEventListener<K extends keyof AbortSignalEventMap>(type: K, listener: (this: AbortSignal, ev: AbortSignalEventMap[K]) => any, options?: boolean | EventListenerOptions): void; }`,
    ),
    [],
  );
  assert.deepEqual(
    paths(
      `${event} interface EventSource { addEventListener(type: string, callback: EventListener | null, options?: boolean): void; }`,
      `${event} interface EventSource { addEventListener(type: string, listener: (this: EventSource, event: Event) => any, options?: boolean): void; addEventListener(type: string, listener: EventListener, options?: boolean, extra: boolean): void; }`,
    ),
    ["overload EventSource.addEventListener"],
  );
});

test("flattens an extended interface", () => {
  const found = paths(
    "interface EventTarget { addEventListener(type: string): void; } interface GPUDevice extends EventTarget { label: string; }",
    "interface GPUDevice { label: string; addEventListener(type: string): void; }",
  );
  assert.deepEqual(found, []);
});

test("reads a type parameter default", () => {
  assert.deepEqual(
    paths("interface Box<T = string> { value: T; }", "interface Box<T = number> { value: T; }"),
    ["type Box type parameter 1"],
  );
});

test("rejects a file that does not parse", () => {
  assert.throws(
    () => compareTexts("interface Broken { foo: }", "interface Broken { foo: string; }"),
    /yuzuko\.d\.ts:\d+: Type expected\./,
  );
});

test("treats optional undefined and split export overloads as covered", () => {
  assert.deepEqual(
    paths(
      "declare function clearTimeout(id?: number): void;",
      "declare function clearTimeout(id: number | undefined): void;",
    ),
    [],
  );
  const keys = 'type KeyFormat = "jwk" | "raw" | "pkcs8"; interface CryptoKey {} interface JsonWebKey {}';
  assert.deepEqual(
    paths(
      `${keys} interface SubtleCrypto { exportKey(format: "jwk", key: CryptoKey): Promise<JsonWebKey>; exportKey(format: Exclude<KeyFormat, "jwk">, key: CryptoKey): Promise<ArrayBuffer>; }`,
      `${keys} interface SubtleCrypto { exportKey(format: KeyFormat, key: CryptoKey): Promise<ArrayBuffer | JsonWebKey>; }`,
    ),
    [],
  );
  assert.deepEqual(
    paths(
      'interface Algorithm { name: string; } type AlgorithmIdentifier = Algorithm | string; interface SubtleCrypto { generateKey(algorithm: AlgorithmIdentifier): Promise<CryptoKey | CryptoKeyPair>; }',
      'interface Algorithm { name: string; } type AlgorithmIdentifier = Algorithm | string; interface SubtleCrypto { generateKey(algorithm: "Ed25519" | { name: "Ed25519" }): Promise<CryptoKeyPair>; }',
    ),
    [],
  );
});
