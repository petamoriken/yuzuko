/// <reference lib="esnext" />

/**
 * API reference for the Yuzuko runtime.
 *
 * Web-standard and WinterTC APIs are declared on the global scope. Runtime
 * APIs are declared on {@link Yuzuko}. This file stands alone. It declares the
 * types this runtime has. A name that `lib.dom.d.ts` also declares is defined
 * here with this runtime's members, so a program that loads both reports
 * duplicate identifiers. The ambient declarations describe the main realm of a
 * runtime built with the default Cargo features. That build enables `fetch`,
 * `fs`, `socket`, `udp`, `tcp`, `http`, `signal`, `child`, `ffi`, `window`,
 * `accessibility`, `image`, `canvas`, `webgpu`, `audio`, and `codec`.
 * `cargo build -p yuzuko --no-default-features` is the JavaScript
 * runtime: {@link AbortController}, {@link EventTarget}, {@link DOMException},
 * {@link URL}, {@link TextEncoder}, {@link Blob}, {@link FormData}, streams,
 * `crypto`, {@link Worker}, and the timers stay. The `Yuzuko` object stays
 * on the main realm and in a dedicated worker. `Yuzuko.args`, `Yuzuko.env`,
 * `Yuzuko.cwd`, `Yuzuko.chdir`, and `Yuzuko.exit` are part of that object in
 * every build. That build omits `fetch`,
 * the file system, datagram sockets, TCP sockets, an HTTP server, signal
 * listeners, a child process, foreign functions, virtual and computed
 * accessibility nodes,
 * {@link WebSocket}, {@link Window},
 * canvas, WebGPU, Web Audio, and WebCodecs.
 * `--no-default-features --features name` selects a
 * subset. `image`, `canvas`, `webgpu`, `codec`, and `accessibility` also
 * enable `window`.
 * {@link DedicatedWorkerGlobalScope} describes a dedicated worker.
 *
 * A web-standard interface or member ends with an MDN link in the form used by
 * [TypeScript-DOM-lib-generator](https://github.com/microsoft/TypeScript-DOM-lib-generator).
 * Members that exist only on Yuzuko have no MDN page.
 *
 * @packageDocumentation
 */

/**
 * Web types this runtime uses. `lib.dom.d.ts` declares these names too.
 */
type AlgorithmIdentifier = Algorithm | string;
type AllowSharedBufferSource = ArrayBufferLike | ArrayBufferView<ArrayBufferLike>;
type AudioContextLatencyCategory = "balanced" | "interactive" | "playback";
type AudioContextState = "closed" | "interrupted" | "running" | "suspended";
type AudioSampleFormat = "f32" | "f32-planar" | "s16" | "s16-planar" | "s32" | "s32-planar" | "u8" | "u8-planar";
type AutomationRate = "a-rate" | "k-rate";
type BinaryType = "arraybuffer" | "blob";
type BodyInit = ReadableStream | XMLHttpRequestBodyInit;
type XMLHttpRequestBodyInit = Blob | BufferSource | FormData | URLSearchParams | string;
type BufferSource = ArrayBufferView<ArrayBuffer> | ArrayBuffer;
type CanvasDirection = "inherit" | "ltr" | "rtl";
type CanvasFillRule = "evenodd" | "nonzero";
type CanvasImageSource = ImageBitmap | OffscreenCanvas | VideoFrame;
type CanvasLineCap = "butt" | "round" | "square";
type CanvasLineJoin = "bevel" | "miter" | "round";
type CanvasTextAlign = "center" | "end" | "left" | "right" | "start";
type CanvasTextBaseline = "alphabetic" | "bottom" | "hanging" | "ideographic" | "middle" | "top";
type BiquadFilterType = "allpass" | "bandpass" | "highpass" | "highshelf" | "lowpass" | "lowshelf" | "notch" | "peaking";
type ChannelCountMode = "clamped-max" | "explicit" | "max";
type ChannelInterpretation = "discrete" | "speakers";
type DistanceModelType = "exponential" | "inverse" | "linear";
type OverSampleType = "2x" | "4x" | "none";
type PanningModelType = "HRTF" | "equalpower";
type CodecState = "closed" | "configured" | "unconfigured";
type ColorSpaceConversion = "default" | "none";
type DOMHighResTimeStamp = number;
interface DecodeErrorCallback {
  (error: DOMException): void;
}
interface DecodeSuccessCallback {
  (decodedData: AudioBuffer): void;
}
type EncodedAudioChunkType = "delta" | "key";
type EncodedVideoChunkType = "delta" | "key";
type EventListenerOrEventListenerObject = EventListener | EventListenerObject;
type FontDisplay = "auto" | "block" | "fallback" | "optional" | "swap";
type FontFaceLoadStatus = "error" | "loaded" | "loading" | "unloaded";
type FontFaceSetLoadStatus = "loaded" | "loading";
type FormDataEntryValue = File | string;
interface FrameRequestCallback {
  (time: DOMHighResTimeStamp): void;
}
type GPUAddressMode = "clamp-to-edge" | "mirror-repeat" | "repeat";
type GPUBindingResource = GPUSampler | GPUTexture | GPUTextureView | GPUBuffer | GPUBufferBinding | GPUExternalTexture;
type GPUBlendFactor = "constant" | "dst" | "dst-alpha" | "one" | "one-minus-constant" | "one-minus-dst" | "one-minus-dst-alpha" | "one-minus-src" | "one-minus-src-alpha" | "one-minus-src1" | "one-minus-src1-alpha" | "src" | "src-alpha" | "src-alpha-saturated" | "src1" | "src1-alpha" | "zero";
type GPUBlendOperation = "add" | "max" | "min" | "reverse-subtract" | "subtract";
type GPUBufferBindingType = "read-only-storage" | "storage" | "uniform";
type GPUBufferUsageFlags = number;
type GPUCanvasAlphaMode = "opaque" | "premultiplied";
type GPUColor = number[] | GPUColorDict;
interface GPUColorDict {
  a: number;
  b: number;
  g: number;
  r: number;
}
type GPUColorWriteFlags = number;
interface GPUComputePassEncoder extends GPUBindingCommandsMixin, GPUObjectBase {
  dispatchWorkgroups(workgroupCountX: GPUSize32, workgroupCountY?: GPUSize32, workgroupCountZ?: GPUSize32): void;
  dispatchWorkgroupsIndirect(indirectBuffer: GPUBuffer, indirectOffset: GPUSize64): void;
  end(): void;
  setPipeline(pipeline: GPUComputePipeline): void;
}

interface GPUBindingCommandsMixin {
  setBindGroup(index: GPUIndex32, bindGroup: GPUBindGroup | null, dynamicOffsets?: GPUBufferDynamicOffset[]): void;
  setBindGroup(index: GPUIndex32, bindGroup: GPUBindGroup | null, dynamicOffsetsData: Uint32Array<ArrayBufferLike>, dynamicOffsetsDataStart: GPUSize64, dynamicOffsetsDataLength: GPUSize32): void;
}
type GPUIndex32 = number;
type GPUBufferDynamicOffset = number;
type GPUSize64 = number;
type GPUSize32 = number;
interface GPUBindingCommandsMixin {
  setBindGroup(index: GPUIndex32, bindGroup: GPUBindGroup | null, dynamicOffsets?: Iterable<GPUBufferDynamicOffset>): void;
}
interface GPUObjectBase {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUObjectBase/label) */
  label: string;
}
interface GPUComputePipeline extends GPUObjectBase, GPUPipelineBase {

}

interface GPUPipelineBase {
  getBindGroupLayout(index: number): GPUBindGroupLayout;
}
type GPUCopyExternalImageSource = ImageBitmap | OffscreenCanvas | VideoFrame;
type GPUCullMode = "back" | "front" | "none";
type GPUDeviceLostReason = "destroyed" | "unknown";
interface GPUError {
  readonly message: string;
}

type GPUErrorFilter = "internal" | "out-of-memory" | "validation";
type GPUExtent3D = GPUIntegerCoordinate[] | GPUExtent3DDict;
type GPUIntegerCoordinate = number;
interface GPUExtent3DDict {
  depthOrArrayLayers?: GPUIntegerCoordinate;
  height?: GPUIntegerCoordinate;
  width: GPUIntegerCoordinate;
}
type GPUFeatureName = "bgra8unorm-storage" | "clip-distances" | "core-features-and-limits" | "depth-clip-control" | "depth32float-stencil8" | "dual-source-blending" | "float32-blendable" | "float32-filterable" | "indirect-first-instance" | "primitive-index" | "rg11b10ufloat-renderable" | "shader-f16" | "subgroups" | "texture-compression-astc" | "texture-compression-astc-sliced-3d" | "texture-compression-bc" | "texture-compression-bc-sliced-3d" | "texture-compression-etc2" | "texture-formats-tier1" | "timestamp-query";
type GPUFilterMode = "linear" | "nearest";
type GPUFrontFace = "ccw" | "cw";
type GPUIndexFormat = "uint16" | "uint32";
type GPULoadOp = "clear" | "load";
type GPUMapModeFlags = number;
type GPUMipmapFilterMode = "linear" | "nearest";
type GPUOrigin2D = GPUIntegerCoordinate[] | GPUOrigin2DDict;
interface GPUOrigin2DDict {
  x?: GPUIntegerCoordinate;
  y?: GPUIntegerCoordinate;
}
type GPUOrigin3D = GPUIntegerCoordinate[] | GPUOrigin3DDict;
interface GPUOrigin3DDict {
  x?: GPUIntegerCoordinate;
  y?: GPUIntegerCoordinate;
  z?: GPUIntegerCoordinate;
}
type GPUPowerPreference = "high-performance" | "low-power";
type GPUPrimitiveTopology = "line-list" | "line-strip" | "point-list" | "triangle-list" | "triangle-strip";
interface GPUQuerySet extends GPUObjectBase {
  readonly count: GPUSize32Out;
  readonly type: GPUQueryType;

  /**
   * Marks this set destroyed.
   *
   * A later `resolveQuerySet` of this set is a validation error.
   * `writeTimestamp` stays absent.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUQuerySet/destroy)
   */
  destroy(): void;
}

type GPUSize32Out = number;
type GPUQueryType = "occlusion" | "timestamp";
interface GPURenderBundleEncoder extends GPUBindingCommandsMixin, GPUObjectBase, GPURenderCommandsMixin {
  finish(descriptor?: GPURenderBundleDescriptor): GPURenderBundle;
}

interface GPURenderCommandsMixin {
  draw(vertexCount: GPUSize32, instanceCount?: GPUSize32, firstVertex?: GPUSize32, firstInstance?: GPUSize32): void;
  drawIndexed(indexCount: GPUSize32, instanceCount?: GPUSize32, firstIndex?: GPUSize32, baseVertex?: GPUSignedOffset32, firstInstance?: GPUSize32): void;
  drawIndexedIndirect(indirectBuffer: GPUBuffer, indirectOffset: GPUSize64): void;
  drawIndirect(indirectBuffer: GPUBuffer, indirectOffset: GPUSize64): void;
  setIndexBuffer(buffer: GPUBuffer, indexFormat: GPUIndexFormat, offset?: GPUSize64, size?: GPUSize64): void;
  setPipeline(pipeline: GPURenderPipeline): void;
  setVertexBuffer(slot: GPUIndex32, buffer: GPUBuffer | null, offset?: GPUSize64, size?: GPUSize64): void;
}
type GPUSignedOffset32 = number;
interface GPURenderBundleDescriptor {
  label?: string;
}
interface GPURenderBundle extends GPUObjectBase {}
interface GPURenderPassDepthStencilAttachment {
  depthClearValue?: number;
  depthLoadOp?: GPULoadOp;
  depthReadOnly?: boolean;
  depthStoreOp?: GPUStoreOp;
  stencilClearValue?: GPUStencilValue;
  stencilLoadOp?: GPULoadOp;
  stencilReadOnly?: boolean;
  stencilStoreOp?: GPUStoreOp;
  view: GPUTexture | GPUTextureView;
}
type GPUStencilValue = number;
interface GPURenderPassTimestampWrites {
  beginningOfPassWriteIndex?: GPUSize32;
  endOfPassWriteIndex?: GPUSize32;
  querySet: GPUQuerySet;
}
type GPUSamplerBindingType = "comparison" | "filtering" | "non-filtering";
type GPUShaderStageFlags = number;
type GPUStoreOp = "discard" | "store";
interface GPUSupportedFeatures {
  forEach(callbackfn: (value: string, key: string, parent: GPUSupportedFeatures) => void, thisArg?: any): void;
}

interface GPUSupportedFeatures extends ReadonlySet<string> {

}
interface GPUSupportedLimits {
  readonly maxBindGroups: number;
  readonly maxBindGroupsPlusVertexBuffers: number;
  readonly maxBindingsPerBindGroup: number;
  readonly maxBufferSize: number;
  readonly maxColorAttachmentBytesPerSample: number;
  readonly maxColorAttachments: number;
  readonly maxComputeInvocationsPerWorkgroup: number;
  readonly maxComputeWorkgroupSizeX: number;
  readonly maxComputeWorkgroupSizeY: number;
  readonly maxComputeWorkgroupSizeZ: number;
  readonly maxComputeWorkgroupStorageSize: number;
  readonly maxComputeWorkgroupsPerDimension: number;
  readonly maxDynamicStorageBuffersPerPipelineLayout: number;
  readonly maxDynamicUniformBuffersPerPipelineLayout: number;
  readonly maxInterStageShaderVariables: number;
  readonly maxSampledTexturesPerShaderStage: number;
  readonly maxSamplersPerShaderStage: number;
  readonly maxStorageBufferBindingSize: number;
  readonly maxStorageBuffersPerShaderStage: number;
  readonly maxStorageTexturesPerShaderStage: number;
  readonly maxTextureArrayLayers: number;
  readonly maxTextureDimension1D: number;
  readonly maxTextureDimension2D: number;
  readonly maxTextureDimension3D: number;
  readonly maxUniformBufferBindingSize: number;
  readonly maxUniformBuffersPerShaderStage: number;
  readonly maxVertexAttributes: number;
  readonly maxVertexBufferArrayStride: number;
  readonly maxVertexBuffers: number;
  readonly minStorageBufferOffsetAlignment: number;
  readonly minUniformBufferOffsetAlignment: number;
}
type GPUTextureAspect = "all" | "depth-only" | "stencil-only";
type GPUTextureDimension = "1d" | "2d" | "3d";
type GPUTextureFormat = "astc-10x10-unorm" | "astc-10x10-unorm-srgb" | "astc-10x5-unorm" | "astc-10x5-unorm-srgb" | "astc-10x6-unorm" | "astc-10x6-unorm-srgb" | "astc-10x8-unorm" | "astc-10x8-unorm-srgb" | "astc-12x10-unorm" | "astc-12x10-unorm-srgb" | "astc-12x12-unorm" | "astc-12x12-unorm-srgb" | "astc-4x4-unorm" | "astc-4x4-unorm-srgb" | "astc-5x4-unorm" | "astc-5x4-unorm-srgb" | "astc-5x5-unorm" | "astc-5x5-unorm-srgb" | "astc-6x5-unorm" | "astc-6x5-unorm-srgb" | "astc-6x6-unorm" | "astc-6x6-unorm-srgb" | "astc-8x5-unorm" | "astc-8x5-unorm-srgb" | "astc-8x6-unorm" | "astc-8x6-unorm-srgb" | "astc-8x8-unorm" | "astc-8x8-unorm-srgb" | "bc1-rgba-unorm" | "bc1-rgba-unorm-srgb" | "bc2-rgba-unorm" | "bc2-rgba-unorm-srgb" | "bc3-rgba-unorm" | "bc3-rgba-unorm-srgb" | "bc4-r-snorm" | "bc4-r-unorm" | "bc5-rg-snorm" | "bc5-rg-unorm" | "bc6h-rgb-float" | "bc6h-rgb-ufloat" | "bc7-rgba-unorm" | "bc7-rgba-unorm-srgb" | "bgra8unorm" | "bgra8unorm-srgb" | "depth16unorm" | "depth24plus" | "depth24plus-stencil8" | "depth32float" | "depth32float-stencil8" | "eac-r11snorm" | "eac-r11unorm" | "eac-rg11snorm" | "eac-rg11unorm" | "etc2-rgb8a1unorm" | "etc2-rgb8a1unorm-srgb" | "etc2-rgb8unorm" | "etc2-rgb8unorm-srgb" | "etc2-rgba8unorm" | "etc2-rgba8unorm-srgb" | "r16float" | "r16sint" | "r16snorm" | "r16uint" | "r16unorm" | "r32float" | "r32sint" | "r32uint" | "r8sint" | "r8snorm" | "r8uint" | "r8unorm" | "rg11b10ufloat" | "rg16float" | "rg16sint" | "rg16snorm" | "rg16uint" | "rg16unorm" | "rg32float" | "rg32sint" | "rg32uint" | "rg8sint" | "rg8snorm" | "rg8uint" | "rg8unorm" | "rgb10a2uint" | "rgb10a2unorm" | "rgb9e5ufloat" | "rgba16float" | "rgba16sint" | "rgba16snorm" | "rgba16uint" | "rgba16unorm" | "rgba32float" | "rgba32sint" | "rgba32uint" | "rgba8sint" | "rgba8snorm" | "rgba8uint" | "rgba8unorm" | "rgba8unorm-srgb" | "stencil8";
type GPUTextureSampleType = "depth" | "float" | "sint" | "uint" | "unfilterable-float";
type GPUTextureUsageFlags = number;
type GPUTextureViewDimension = "1d" | "2d" | "2d-array" | "3d" | "cube" | "cube-array";
type GPUVertexFormat = "float16" | "float16x2" | "float16x4" | "float32" | "float32x2" | "float32x3" | "float32x4" | "sint16" | "sint16x2" | "sint16x4" | "sint32" | "sint32x2" | "sint32x3" | "sint32x4" | "sint8" | "sint8x2" | "sint8x4" | "snorm16" | "snorm16x2" | "snorm16x4" | "snorm8" | "snorm8x2" | "snorm8x4" | "uint16" | "uint16x2" | "uint16x4" | "uint32" | "uint32x2" | "uint32x3" | "uint32x4" | "uint8" | "uint8x2" | "uint8x4" | "unorm10-10-10-2" | "unorm16" | "unorm16x2" | "unorm16x4" | "unorm8" | "unorm8x2" | "unorm8x4" | "unorm8x4-bgra";
type GPUVertexStepMode = "instance" | "vertex";
type GamepadHapticEffectType = "dual-rumble" | "trigger-rumble";
type GamepadHapticsResult = "complete" | "preempted";
type GamepadMappingType = "" | "standard" | "xr-standard";
type GlobalCompositeOperation = "color" | "color-burn" | "color-dodge" | "copy" | "darken" | "destination-atop" | "destination-in" | "destination-out" | "destination-over" | "difference" | "exclusion" | "hard-light" | "hue" | "lighten" | "lighter" | "luminosity" | "multiply" | "overlay" | "saturation" | "screen" | "soft-light" | "source-atop" | "source-in" | "source-out" | "source-over" | "xor";
type HardwareAcceleration = "no-preference" | "prefer-hardware" | "prefer-software";
type HeadersInit = [string, string][] | Record<string, string> | Headers;
interface IdleRequestCallback {
  (deadline: IdleDeadline): void;
}
type ImageBitmapSource = CanvasImageSource | Blob | ImageData;
type ImageBufferSource = AllowSharedBufferSource | ReadableStream;
type ImageDataArray = Uint8ClampedArray<ArrayBuffer>;
type ImageSmoothingQuality = "high" | "low" | "medium";
type KeyFormat = "jwk" | "pkcs8" | "raw" | "spki";
type KeyUsage = "decrypt" | "deriveBits" | "deriveKey" | "encrypt" | "sign" | "unwrapKey" | "verify" | "wrapKey";
type OnErrorEventHandler = OnErrorEventHandlerNonNull | null;
interface OnErrorEventHandlerNonNull {
  (event: Event | string, source?: string, lineno?: number, colno?: number, error?: Error): any;
}
type OscillatorType = "custom" | "sawtooth" | "sine" | "square" | "triangle";
/**
 * An omitted `real` or `imag` is a zero sequence of the other length.
 * @throws {@link DOMException} `IndexSizeError` when both `real` and `imag`
 * are present and their lengths differ, or when fewer than two coefficients
 * remain.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/PeriodicWave)
 */
interface PeriodicWave {

}
declare var PeriodicWave: {
  prototype: PeriodicWave;
  new(context: BaseAudioContext, options?: PeriodicWaveOptions): PeriodicWave;
};

type PredefinedColorSpace = "display-p3" | "srgb";
type PresentationStyle = "attachment" | "inline" | "unspecified";
interface QueuingStrategySize<T = any> {
  (chunk: T): number;
}
type ReadableStreamReadResult<T> = ReadableStreamReadValueResult<T> | ReadableStreamReadDoneResult<T>;
interface ReadableStreamReadValueResult<T> {
  done: false;
  value: T;
}
interface ReadableStreamReadDoneResult<T> {
  done: true;
  value: T | undefined;
}
type ReadableStreamType = "bytes" | "owning";
type RenderingContext = OffscreenCanvasRenderingContext2D | BitmapContext | GPUCanvasContext;
type RequestCache = "default" | "force-cache" | "no-cache" | "no-store" | "only-if-cached" | "reload";
type RequestCredentials = "include" | "omit" | "same-origin";
type RequestInfo = Request | string;
type RequestPriority = "auto" | "high" | "low";
type RequestRedirect = "error" | "follow" | "manual";
type ResponseType = "basic" | "cors" | "default" | "error" | "opaque" | "opaqueredirect";
type TimerHandler = string | Function;
/** Host values a transfer list can move. */
type Transferable = OffscreenCanvas | ImageBitmap | MessagePort | ReadableStream | WritableStream | TransformStream | AudioData | VideoFrame | ArrayBuffer;
interface TransformerStartCallback<O> {
  (controller: TransformStreamDefaultController<O>): any;
}
interface UnderlyingSinkStartCallback {
  (controller: WritableStreamDefaultController): any;
}
interface UnderlyingSourceCancelCallback {
  (reason?: any): void | PromiseLike<void>;
}
interface UnderlyingSourcePullCallback<R> {
  (controller: ReadableStreamController<R>): void | PromiseLike<void>;
}
type ReadableStreamController<T> = ReadableStreamDefaultController<T> | ReadableByteStreamController;
interface UnderlyingSourceStartCallback<R> {
  (controller: ReadableStreamController<R>): any;
}
type VideoPixelFormat = "BGRA" | "BGRX" | "I420" | "I420A" | "I422" | "I444" | "NV12" | "RGBA" | "RGBX";
type VideoColorPrimaries = "bt2020" | "bt470bg" | "bt709" | "smpte170m" | "smpte432";
type VideoMatrixCoefficients = "bt2020-ncl" | "bt470bg" | "bt709" | "rgb" | "smpte170m";
type VideoTransferCharacteristics = "bt709" | "hlg" | "iec61966-2-1" | "linear" | "pq" | "smpte170m";
type AlphaOption = "discard" | "keep";
interface WGSLLanguageFeatures {
  forEach(callbackfn: (value: string, key: string, parent: WGSLLanguageFeatures) => void, thisArg?: any): void;
}
interface WGSLLanguageFeatures extends ReadonlySet<string> {

}
interface Window extends EventTarget {}
type WorkerType = "classic" | "module";
type URLPatternInput = string | URLPatternInit;
type BlobPart = BufferSource | Blob | string;
interface UnderlyingDefaultSource<R = any> {
  cancel?: UnderlyingSourceCancelCallback;
  pull?: (controller: ReadableStreamDefaultController<R>) => void | PromiseLike<void>;
  start?: (controller: ReadableStreamDefaultController<R>) => any;
  type?: undefined;
}
interface QueuingStrategyInit {
  highWaterMark: number;
}
type CompressionFormat = "deflate" | "deflate-raw" | "gzip";
interface VideoDecoderSupport {
  config?: VideoDecoderConfig;
  supported?: boolean;
}
interface AudioDecoderSupport {
  config?: AudioDecoderConfig;
  supported?: boolean;
}
interface MediaQueryListEventInit extends EventInit {
  matches?: boolean;
  media?: string;
}
/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventListener)
 */
interface EventListener {
  (evt: Event): void;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventListenerObject)
 */
interface EventListenerObject {
  handleEvent(object: Event): void;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventListenerOptions)
 */
interface EventListenerOptions {
  capture?: boolean;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AddEventListenerOptions)
 */
interface AddEventListenerOptions extends EventListenerOptions {
  /** When true, the listener is removed after it runs once. */
  once?: boolean;

  /** When true, `preventDefault()` inside the listener has no effect. */
  passive?: boolean;

  /** Removes the listener when this signal aborts. */
  signal?: AbortSignal;
}

/**
 * Bytes accepted by {@link FontFace}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/FontFace)
 */
type BinaryData = ArrayBuffer | ArrayBufferView;

/**
 * A JSON Web Key.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/importKey)
 */
interface JsonWebKey {
  alg?: string;
  crv?: string;
  d?: string;
  dp?: string;
  dq?: string;
  e?: string;
  ext?: boolean;
  k?: string;
  key_ops?: string[];
  kty?: string;
  n?: string;
  oth?: Array<{ d?: string; r?: string; t?: string }>;
  p?: string;
  q?: string;
  qi?: string;
  use?: string;
  x?: string;
  y?: string;
}

/**
 * The Yuzuko namespace.
 *
 * `Yuzuko` is one {@link EventTarget} for the process. A menu selection
 * dispatches `menuclick` here. Web-standard objects are not constructed from
 * this namespace.
 */
declare namespace Yuzuko {
  /**
   * Options for {@link Window}.
   *
   * Lengths are CSS pixels. An omitted `width` or `height` is 1280 or 720.
   * An omitted position centers the frame on the primary monitor, or uses
   * `0, 0` when no monitor exists. Setting only one of `x` or `y` uses `0`
   * for the other.
   */
  interface WindowOptions {
    /** Initial client width in CSS pixels. @defaultValue 1280 */
    width?: number;

    /** Initial client height in CSS pixels. @defaultValue 720 */
    height?: number;

    /**
     * Initial OS window title.
     *
     * Read and write the live title on {@link WindowSurface.title}.
     *
     * @defaultValue `"yuzuko"`, or the package `name` when that config is found
     */
    title?: string;

    /**
     * Whether the OS window can be resized.
     *
     * Assigning {@link Window.resizable} updates the live window.
     *
     * @defaultValue false
     */
    resizable?: boolean;

    /** Frame origin on the X axis, in CSS pixels. Positive is to the right. */
    x?: number;

    /** Frame origin on the Y axis, in CSS pixels. Positive is downward. */
    y?: number;

    /**
     * Keep the window above normal windows.
     *
     * @defaultValue false
     */
    alwaysOnTop?: boolean;

    /**
     * Whether the OS window has a title bar and borders.
     *
     * Creation only. The live window does not accept a later change.
     *
     * @defaultValue true
     */
    decorations?: boolean;

    /**
     * Whether the window is focused when it opens.
     *
     * Creation only. `false` skips the initial focus. It does not create a
     * separate non-activating panel.
     *
     * @defaultValue true
     */
    active?: boolean;

    /**
     * Whether the OS window is shown.
     *
     * `false` creates the window hidden until {@link Window.show}.
     *
     * @defaultValue true
     */
    visible?: boolean;

    /**
     * Whether the window opens maximized.
     *
     * This is not fullscreen.
     *
     * @defaultValue false
     */
    maximized?: boolean;

    /** Minimum client width in CSS pixels. Unset when omitted. */
    minWidth?: number;

    /** Minimum client height in CSS pixels. Unset when omitted. */
    minHeight?: number;

    /**
     * Maximum client width in CSS pixels.
     *
     * Unset when omitted. When only one maximum is set, the OS limit for the
     * other edge is 100000 CSS pixels.
     */
    maxWidth?: number;

    /** Maximum client height in CSS pixels. See {@link WindowOptions.maxWidth}. */
    maxHeight?: number;

    /**
     * Ask the OS to composite the window's alpha.
     *
     * Creation only. Unpainted pixels and the area outside the fitted bitmap
     * are clear. macOS clears the window background. Windows uses the Desktop
     * Window Manager. Wayland and X11 need a compositor.
     *
     * @defaultValue false
     */
    transparent?: boolean;
  }

  /**
   * Events dispatched on a {@link Window}.
   *
   * Pointer events target {@link Window.surface} and propagate through the
   * window. Key, IME, wheel, focus, blur, resize, and close target the window
   * only. `focus` and `blur` do not bubble.
   */
  interface WindowEventMap {
    close: Event;
    resize: Event;
    focus: FocusEvent;
    blur: FocusEvent;
    keydown: KeyboardEvent;
    keyup: KeyboardEvent;
    wheel: WheelEvent;
    compositionstart: CompositionEvent;
    compositionupdate: CompositionEvent;
    compositionend: CompositionEvent;
    pointerlockchange: Event;
    pointerlockerror: Event;
    pointerdown: PointerEvent;
    pointerup: PointerEvent;
    pointermove: PointerEvent;
    pointerrawupdate: PointerEvent;
    pointercancel: PointerEvent;
    pointerover: PointerEvent;
    pointerout: PointerEvent;
    pointerenter: PointerEvent;
    pointerleave: PointerEvent;
    gotpointercapture: PointerEvent;
    lostpointercapture: PointerEvent;
    mousedown: MouseEvent;
    mouseup: MouseEvent;
    mousemove: MouseEvent;
    mouseover: MouseEvent;
    mouseout: MouseEvent;
    mouseenter: MouseEvent;
    mouseleave: MouseEvent;
    click: MouseEvent;
    dblclick: MouseEvent;
    auxclick: MouseEvent;
    contextmenu: MouseEvent;
  }

  type AccessibleTriState = "true" | "false" | "mixed";

  type AccessibleLive = "off" | "polite" | "assertive";

  type AccessibleOrientation = "horizontal" | "vertical";

  /**
   * Recognized ARIA role token.
   *
   * `generic`, `none`, and `presentation` publish a generic container.
   * Any other string is still stored and published as an unknown role.
   */
  type AccessibleRole =
    | "alert"
    | "alertdialog"
    | "application"
    | "article"
    | "banner"
    | "blockquote"
    | "button"
    | "caption"
    | "cell"
    | "checkbox"
    | "code"
    | "columnheader"
    | "combobox"
    | "complementary"
    | "contentinfo"
    | "definition"
    | "dialog"
    | "directory"
    | "document"
    | "emphasis"
    | "feed"
    | "figure"
    | "form"
    | "generic"
    | "grid"
    | "gridcell"
    | "group"
    | "heading"
    | "image"
    | "img"
    | "link"
    | "list"
    | "listbox"
    | "listitem"
    | "log"
    | "main"
    | "marquee"
    | "math"
    | "menu"
    | "menubar"
    | "menuitem"
    | "menuitemcheckbox"
    | "menuitemradio"
    | "meter"
    | "navigation"
    | "none"
    | "note"
    | "option"
    | "paragraph"
    | "presentation"
    | "progressbar"
    | "radio"
    | "radiogroup"
    | "region"
    | "row"
    | "rowgroup"
    | "rowheader"
    | "scrollbar"
    | "search"
    | "searchbox"
    | "separator"
    | "slider"
    | "spinbutton"
    | "status"
    | "strong"
    | "switch"
    | "tab"
    | "table"
    | "tablist"
    | "tabpanel"
    | "term"
    | "textbox"
    | "time"
    | "timer"
    | "toolbar"
    | "tooltip"
    | "tree"
    | "treegrid"
    | "treeitem"
    | "window";

  /**
   * Role token on a {@link ComputedAccessibleNode}.
   *
   * `"unknown"` is the published role when the authored token is not
   * recognized. It is not an {@link AccessibleRole}.
   */
  type ComputedAccessibleRole = AccessibleRole | "unknown";

  /**
   * `accessiblesetvalue` dispatched on an {@link AccessibleNode}.
   *
   * There is no constructor. The event is a {@link CustomEvent} whose
   * `detail` is the requested finite number, or `null` when the request has
   * no number. The event does not change `valueNow` or `valueText`.
   */
  interface AccessibleSetValueEvent extends CustomEvent<number | null> {}

  /** Events dispatched on an {@link AccessibleNode}. */
  interface AccessibleNodeEventMap {
    accessibleclick: Event;
    accessiblecontextmenu: Event;
    accessibledecrement: Event;
    accessiblefocus: Event;
    accessibleincrement: Event;
    accessiblescrollintoview: Event;
    accessiblesetvalue: AccessibleSetValueEvent;
  }

  /**
   * A virtual accessibility node.
   *
   * This is the Accessibility Object Model Phase 3 draft, attached to a
   * {@link WindowSurface} because there is no document. It is not on `globalThis`.
   * A dedicated worker does not have it. `parent` and the sibling getters
   * read the authored tree. The published tree is a
   * {@link ComputedAccessibleNode}.
   *
   * Assistive-technology actions bubble through that tree and are not
   * cancelable. They do not bubble to the window. There are no
   * `onaccessible*` handlers. `dispatchEvent` stays single-target.
   *
   * `role`, `name`, `description`, `roleDescription`, and `valueText` use
   * `ToString`. `null` or `undefined` clears them. `checked` and `pressed`
   * are `"true"`, `"false"`, or `"mixed"`. `disabled`, `expanded`, `hidden`,
   * `selected`, `busy`, `modal`, `required`, `readOnly`, `multiselectable`,
   * and `multiline` are booleans or `null`. `valueNow`, `valueMin`, and
   * `valueMax` are finite numbers or `null`. Counts and indexes are IDL
   * `long` values or `null`. `role` is an {@link AccessibleRole} or any
   * other string. `live` and `orientation` are the tokens below.
   * `focusable` defaults to `false`.
   *
   * Offsets are CSS pixel lengths. The getter returns `null` or a string
   * such as `"30px"`. The setter accepts `null`, `undefined`, a finite
   * number, or a string that is a bare number or a `px` length.
   */
  class AccessibleNode extends EventTarget {
    /** Creates a node that is not attached to a window. */
    constructor();

    /**
     * Appends `node` and detaches it from its previous parent.
     *
     * @throws {@link TypeError} When `node` is not an `AccessibleNode`, or
     * when the argument is missing.
     * @throws {@link DOMException} `HierarchyRequestError` when `node` is an
     * ancestor, or when it is the accessible root.
     */
    appendChild(node: AccessibleNode): AccessibleNode;

    /**
     * Inserts `node` before `child`. `null` or `undefined` appends.
     *
     * `insertBefore(node, node)` keeps `node` in place when it is already a
     * child.
     *
     * @throws {@link TypeError} When `node` is not an `AccessibleNode`, or
     * when `node` is missing.
     * @throws {@link DOMException} `NotFoundError` when `child` is not a child.
     * @throws {@link DOMException} `HierarchyRequestError` when `node` is an
     * ancestor, or when it is the accessible root.
     */
    insertBefore(
      node: AccessibleNode,
      child?: AccessibleNode | null,
    ): AccessibleNode;

    /**
     * Replaces `child` with `node` and returns `child`.
     *
     * When `node` and `child` are the same child, the tree does not change.
     *
     * @throws {@link TypeError} When either value is not an `AccessibleNode`,
     * or when an argument is missing.
     * @throws {@link DOMException} `NotFoundError` when `child` is not a child.
     * @throws {@link DOMException} `HierarchyRequestError` when `node` is an
     * ancestor, or when it is the accessible root.
     */
    replaceChild(node: AccessibleNode, child: AccessibleNode): AccessibleNode;

    /**
     * Removes `node` and returns it.
     *
     * @throws {@link TypeError} When `node` is not an `AccessibleNode`, or
     * when the argument is missing.
     * @throws {@link DOMException} `NotFoundError` when `node` is not a child.
     */
    removeChild(node: AccessibleNode): AccessibleNode;

    /**
     * Moves accessible focus to this node.
     *
     * This does not move operating-system focus and does not dispatch
     * `focus`, `blur`, or `accessiblefocus`.
     *
     * @throws {@link DOMException} `InvalidStateError` when the node is not
     * focusable, is not connected to a root, or its window is closed.
     */
    focus(): void;

    /**
     * ARIA role token. `null` when unset. `""` when set to an empty string.
     *
     * {@link AccessibleRole} is the recognized set. Any other string is
     * stored and published as an unknown role.
     */
    role: AccessibleRole | (string & {}) | null;

    /** Accessible name. There is no `label` property. */
    name: string | null;

    /** Accessible description. */
    description: string | null;

    /** Author role description. */
    roleDescription: string | null;

    /** Value text. This does not clear `valueNow`. */
    valueText: string | null;

    /**
     * Checked state.
     *
     * @throws {@link TypeError} When the value is not `true`, `false`,
     * `mixed`, `null`, or `undefined`.
     */
    checked: AccessibleTriState | boolean | null;

    /**
     * Pressed state.
     *
     * @throws {@link TypeError} When the value is not `true`, `false`,
     * `mixed`, `null`, or `undefined`.
     */
    pressed: AccessibleTriState | boolean | null;

    /** `null` clears the flag. */
    disabled: boolean | null;

    /** `null` clears the state. `false` is collapsed. */
    expanded: boolean | null;

    /** `null` clears the flag. */
    hidden: boolean | null;

    /** `null` clears the state. `false` is unselected. */
    selected: boolean | null;

    /** `null` clears the flag. */
    busy: boolean | null;

    /** `null` clears the flag. */
    modal: boolean | null;

    /** `null` clears the flag. */
    required: boolean | null;

    /** `null` clears the flag. */
    readOnly: boolean | null;

    /** `null` clears the flag. */
    multiselectable: boolean | null;

    /**
     * When `true`, a `textbox` is published as a multiline text input.
     *
     * `null` clears it.
     */
    multiline: boolean | null;

    /**
     * Current numeric value.
     *
     * @throws {@link TypeError} When the value is not finite.
     */
    valueNow: number | null;

    /**
     * Minimum numeric value.
     *
     * @throws {@link TypeError} When the value is not finite.
     */
    valueMin: number | null;

    /**
     * Maximum numeric value.
     *
     * @throws {@link TypeError} When the value is not finite.
     */
    valueMax: number | null;

    /** `null` clears the count. A negative count is stored and not published. */
    colCount: number | null;

    /** `null` clears the count. */
    rowCount: number | null;

    /** One-based column index, or `null`. `0` stays `0`. */
    colIndex: number | null;

    /** `null` clears the span. */
    colSpan: number | null;

    /** One-based row index, or `null`. `0` stays `0`. */
    rowIndex: number | null;

    /** `null` clears the span. */
    rowSpan: number | null;

    /** One-based level, or `null`. `0` stays `0`. */
    level: number | null;

    /** `null` clears the set size. */
    setSize: number | null;

    /** One-based position in the set, or `null`. `0` stays `0`. */
    posInSet: number | null;

    /**
     * Live-region politeness.
     *
     * @throws {@link TypeError} When the value is not a live token, `null`,
     * or `undefined`.
     */
    live: AccessibleLive | null;

    /**
     * Widget orientation.
     *
     * @throws {@link TypeError} When the value is not an orientation token,
     * `null`, or `undefined`.
     */
    orientation: AccessibleOrientation | null;

    /** @defaultValue false */
    focusable: boolean;

    /**
     * Horizontal offset in CSS pixels, or `null`.
     *
     * @throws {@link TypeError} When the value is not a finite number or a
     * `px` length.
     */
    offsetLeft: string | number | null;

    /**
     * Vertical offset in CSS pixels, or `null`.
     *
     * @throws {@link TypeError} When the value is not a finite number or a
     * `px` length.
     */
    offsetTop: string | number | null;

    /**
     * Width in CSS pixels, or `null`.
     *
     * @throws {@link TypeError} When the value is not a finite number or a
     * `px` length.
     */
    offsetWidth: string | number | null;

    /**
     * Height in CSS pixels, or `null`.
     *
     * @throws {@link TypeError} When the value is not a finite number or a
     * `px` length.
     */
    offsetHeight: string | number | null;

    /**
     * Ancestor the offsets are relative to, or `null` for the parent.
     *
     * @throws {@link TypeError} When the value is not an `AccessibleNode`.
     * @throws {@link DOMException} `HierarchyRequestError` when the node is
     * not a strict ancestor.
     */
    offsetParent: AccessibleNode | null;

    /** Parent in the authored tree, or `null` for a root or a disconnected node. */
    readonly parent: AccessibleNode | null;

    /** First authored child, or `null`. */
    readonly firstChild: AccessibleNode | null;

    /** Last authored child, or `null`. */
    readonly lastChild: AccessibleNode | null;

    /** Previous authored sibling, or `null`. */
    readonly previousSibling: AccessibleNode | null;

    /** Next authored sibling, or `null`. */
    readonly nextSibling: AccessibleNode | null;

    addEventListener<K extends keyof AccessibleNodeEventMap>(
      type: K,
      listener: (this: AccessibleNode, ev: AccessibleNodeEventMap[K]) => any,
      options?: boolean | AddEventListenerOptions,
    ): void;
    addEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | AddEventListenerOptions,
    ): void;
    removeEventListener<K extends keyof AccessibleNodeEventMap>(
      type: K,
      listener: (this: AccessibleNode, ev: AccessibleNodeEventMap[K]) => any,
      options?: boolean | EventListenerOptions,
    ): void;
    removeEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | EventListenerOptions,
    ): void;
  }

  /**
   * One node of the published accessibility tree.
   *
   * This is the computed view from
   * {@link Window.getComputedAccessibleNode}. There is no public constructor.
   * It is not an `EventTarget` and it is not on `globalThis`. A dedicated
   * worker does not have it. Reads keep the snapshot until `ensureUpToDate`
   * or another `getComputedAccessibleNode` for that window.
   *
   * The synthetic window node is included. It is not an `AccessibleNode`.
   * The author root's `parent` is that window node. `role` is the published
   * token, so an unknown author role is `"unknown"` and `img` is `"image"`.
   * Indexes are the published zero-based values. Offsets are local CSS pixel
   * strings.
   *
   * @throws {@link TypeError} Illegal constructor.
   */
  class ComputedAccessibleNode {
    /** @throws {@link TypeError} Illegal constructor. */
    constructor();

    /**
     * Replaces this window's snapshot and resolves `undefined`.
     *
     * A node that has left the tree keeps this object. After the snapshot,
     * its string and boolean properties are `null`, `focusable` is `false`,
     * and its links are `null`.
     *
     * @throws {@link TypeError} `illegal invocation` when `this` is not a
     * `ComputedAccessibleNode`. The promise rejects.
     * @throws {@link DOMException} `InvalidStateError` when the window is
     * closed. The promise rejects with `The window is closed.`
     */
    ensureUpToDate(): Promise<void>;

    /** Published role, or `null` when this node has left the tree. */
    readonly role: ComputedAccessibleRole | null;

    /** Published name, or `null` when it was omitted. `""` stays empty. */
    readonly name: string | null;

    /** Published description, or `null`. */
    readonly description: string | null;

    /** Published role description, or `null`. */
    readonly roleDescription: string | null;

    /** Published value text, or `null`. */
    readonly valueText: string | null;

    /** Published checked state, or `null` when pressed was published instead. */
    readonly checked: AccessibleTriState | null;

    /** Published pressed state, or `null` when checked was published instead. */
    readonly pressed: AccessibleTriState | null;

    /** `true` when disabled was published, otherwise `null`. */
    readonly disabled: true | null;

    /** Published expanded state, including `false`, or `null`. */
    readonly expanded: boolean | null;

    /** `true` when hidden was published, otherwise `null`. */
    readonly hidden: true | null;

    /** Published selected state, including `false`, or `null`. */
    readonly selected: boolean | null;

    /** `true` when busy was published, otherwise `null`. */
    readonly busy: true | null;

    /** `true` when modal was published, otherwise `null`. */
    readonly modal: true | null;

    /** `true` when required was published, otherwise `null`. */
    readonly required: true | null;

    /** `true` when read-only was published, otherwise `null`. */
    readonly readOnly: true | null;

    /** `true` when multiselectable was published, otherwise `null`. */
    readonly multiselectable: true | null;

    /**
     * `true` when this text box is published as a multiline text input.
     * Otherwise `null`.
     */
    readonly multiline: true | null;

    /** Published current value, or `null`. */
    readonly valueNow: number | null;

    /** Published minimum, or `null`. */
    readonly valueMin: number | null;

    /** Published maximum, or `null`. */
    readonly valueMax: number | null;

    /** Published column count. `0` stays `0`. A negative count is `null`. */
    readonly colCount: number | null;

    /** Published row count. `0` stays `0`. A negative count is `null`. */
    readonly rowCount: number | null;

    /** Published zero-based column index. An authored `1` is `0`. */
    readonly colIndex: number | null;

    /** Published column span. `0` stays `0`. A negative span is `null`. */
    readonly colSpan: number | null;

    /** Published zero-based row index. An authored `1` is `0`. */
    readonly rowIndex: number | null;

    /** Published row span. `0` stays `0`. A negative span is `null`. */
    readonly rowSpan: number | null;

    /** Published zero-based level. An authored `1` is `0`. */
    readonly level: number | null;

    /** Published set size. `0` stays `0`. A negative size is `null`. */
    readonly setSize: number | null;

    /** Published zero-based position. An authored `1` is `0`. */
    readonly posInSet: number | null;

    /** Published live token, or `null`. */
    readonly live: AccessibleLive | null;

    /** Published orientation, or `null`. */
    readonly orientation: AccessibleOrientation | null;

    /**
     * Published focusable state.
     *
     * @defaultValue false
     */
    readonly focusable: boolean;

    /** Local CSS pixel offset, such as `"0px"`, or `null` outside the tree. */
    readonly offsetLeft: string | null;

    /** Local CSS pixel offset, such as `"0px"`, or `null` outside the tree. */
    readonly offsetTop: string | null;

    /** Local CSS pixel length, such as `"0px"`, or `null` outside the tree. */
    readonly offsetWidth: string | null;

    /** Local CSS pixel length, such as `"0px"`, or `null` outside the tree. */
    readonly offsetHeight: string | null;

    /** Computed offset parent when that node is a strict ancestor, or `null`. */
    readonly offsetParent: ComputedAccessibleNode | null;

    /** Parent in the published tree, or `null`. */
    readonly parent: ComputedAccessibleNode | null;

    /** First published child, or `null`. */
    readonly firstChild: ComputedAccessibleNode | null;

    /** Last published child, or `null`. */
    readonly lastChild: ComputedAccessibleNode | null;

    /** Previous published sibling, or `null`. */
    readonly previousSibling: ComputedAccessibleNode | null;

    /** Next published sibling, or `null`. */
    readonly nextSibling: ComputedAccessibleNode | null;
  }

  /**
   * One native window.
   *
   * Startup does not open a window. Each `new Yuzuko.Window()` opens one.
   * `YUZUKO_HEADLESS=1` and `yuzuko test` skip OS window creation. A headless
   * window still runs its drawing buffer and animation frames at 60 Hz. A
   * dedicated worker has no `Window`.
   * The window cannot be transferred.
   *
   * @example
   * ```ts
   * const win = new Yuzuko.Window({ title: "yuzuko", width: 1280, height: 720 });
   * const ctx = win.surface.getContext("2d")!;
   * ctx.fillStyle = "black";
   * ctx.fillRect(0, 0, win.clientWidth, win.clientHeight);
   * ```
   */
  class Window extends EventTarget {
    /**
     * Opens a window.
     *
     * @param options - Size, title, and frame flags.
     * @throws {@link TypeError} When `options` is not an object, a length or
     * position is not finite, or a minimum is greater than the matching maximum.
     */
    constructor(options?: WindowOptions);

    /** The bitmap presented in this window. It does not follow later resizes. */
    readonly surface: WindowSurface;

    /**
     * Whether the OS window can be resized.
     *
     * @defaultValue false
     */
    resizable: boolean;

    /** Client width in CSS pixels. */
    readonly clientWidth: number;

    /** Client height in CSS pixels. */
    readonly clientHeight: number;

    /** This window's scale factor. */
    readonly devicePixelRatio: number;

    /** Client origin on the X axis, in CSS pixels. Same value as `screenLeft` and `innerX`. */
    readonly screenX: number;

    /** Client origin on the Y axis, in CSS pixels. Same value as `screenTop` and `innerY`. */
    readonly screenY: number;

    /** Alias of {@link Window.screenX}. */
    readonly screenLeft: number;

    /** Alias of {@link Window.screenY}. */
    readonly screenTop: number;

    /** Client origin on the X axis. Same value as {@link Window.screenX}. */
    readonly innerX: number;

    /** Client origin on the Y axis. Same value as {@link Window.screenY}. */
    readonly innerY: number;

    /** Frame origin on the X axis, including the title bar, in CSS pixels. */
    readonly outerX: number;

    /** Frame origin on the Y axis, including the title bar, in CSS pixels. */
    readonly outerY: number;

    /**
     * Whether the OS window is shown.
     *
     * This does not by itself change {@link WindowSurface.visibilityState}.
     */
    readonly visible: boolean;

    /**
     * Keep the window above normal windows.
     *
     * @defaultValue false
     */
    alwaysOnTop: boolean;

    /**
     * Whether the OS window has a title bar and borders.
     *
     * The value is fixed by the constructor.
     *
     * @defaultValue true
     */
    readonly decorations: boolean;

    /**
     * Whether the window took focus when it opened.
     *
     * The value is fixed by the constructor.
     *
     * @defaultValue true
     */
    readonly active: boolean;

    /**
     * Whether the window was created maximized.
     *
     * @defaultValue false
     */
    readonly maximized: boolean;

    /**
     * Whether the window composites alpha.
     *
     * The value is fixed by the constructor.
     *
     * @defaultValue false
     */
    readonly transparent: boolean;

    /** Minimum client width in CSS pixels, or `undefined` when unset. */
    readonly minWidth: number | undefined;

    /** Minimum client height in CSS pixels, or `undefined` when unset. */
    readonly minHeight: number | undefined;

    /** Maximum client width in CSS pixels, or `undefined` when unset. */
    readonly maxWidth: number | undefined;

    /** Maximum client height in CSS pixels, or `undefined` when unset. */
    readonly maxHeight: number | undefined;

    /**
     * Cursor shown over this window.
     *
     * `"none"` hides it. Any other value is a winit cursor name:
     * `"default"`, `"context-menu"`, `"help"`, `"pointer"`, `"progress"`,
     * `"wait"`, `"cell"`, `"crosshair"`, `"text"`, `"vertical-text"`,
     * `"alias"`, `"copy"`, `"move"`, `"no-drop"`, `"not-allowed"`, `"grab"`,
     * `"grabbing"`, `"e-resize"`, `"n-resize"`, `"ne-resize"`, `"nw-resize"`,
     * `"s-resize"`, `"se-resize"`, `"sw-resize"`, `"w-resize"`, `"ew-resize"`,
     * `"ns-resize"`, `"nesw-resize"`, `"nwse-resize"`, `"col-resize"`,
     * `"row-resize"`, `"all-scroll"`, `"zoom-in"`, or `"zoom-out"`.
     *
     * Matching is case-insensitive. An unrecognized name is ignored. The
     * getter returns the canonical keyword.
     *
     * @defaultValue `"default"`
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    cursor: string;

    /**
     * Runs `callback` before this window's next rendering update.
     *
     * A Canvas 2D or bitmaprenderer change is presented on the next
     * event-loop turn without this callback. Use this callback to draw on a
     * later frame.
     *
     * The period is the refresh rate of the monitor this window is on. On
     * macOS that is the screen's maximum, so a variable-refresh display runs
     * at that maximum while a frame is scheduled. Linux and Windows use the
     * monitor's current mode. Moving the window adopts the new monitor on the
     * next frame. A headless window, or a rate that cannot be read, runs at
     * 60 Hz. A rate below 10 Hz or above 1000 Hz is treated as 60 Hz.
     *
     * The main global has no `requestAnimationFrame`. Frames are not throttled
     * while the surface is hidden. After the window is closed this returns `0`
     * and does not queue `callback`. `0` is not a live handle. During `close`,
     * and during the `visibilitychange` that follows a close that proceeds,
     * the window is still open, so this still queues a callback. That callback
     * is dropped when the window is destroyed.
     *
     * @param callback - Receives the same timestamp as `performance.now()`.
     * @returns A positive handle for {@link Window.cancelAnimationFrame}, or
     * `0` when the window is already closed.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/requestAnimationFrame)
     */
    requestAnimationFrame(callback: FrameRequestCallback): number;

    /**
     * Cancels a callback scheduled on this window.
     *
     * An unknown handle does nothing. That includes `0` and a handle whose
     * callback was already dropped because the window closed.
     *
     * @param handle - The value returned by {@link Window.requestAnimationFrame}.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/cancelAnimationFrame)
     */
    cancelAnimationFrame(handle: number): void;

    /**
     * Sets the client size in CSS pixels.
     *
     * The size is clamped to the minimum and maximum. `surface` is not resized.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    resizeTo(width: number, height: number): void;

    /**
     * Adds CSS pixels to the client size, with the same clamp as {@link Window.resizeTo}.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    resizeBy(x: number, y: number): void;

    /**
     * Moves the frame origin to `x`, `y` in CSS pixels.
     *
     * Positive `y` is downward.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    moveTo(x: number, y: number): void;

    /**
     * Moves the frame origin by `x`, `y` CSS pixels.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    moveBy(x: number, y: number): void;

    /**
     * Shows the OS window.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    show(): void;

    /**
     * Hides the OS window.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    hide(): void;

    /**
     * Focuses this window.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    focus(): void;

    /**
     * Dispatches a cancelable `close` event.
     *
     * If `preventDefault()` is not called, only this window is destroyed.
     */
    close(): void;

    /**
     * Returns the published accessibility node for `node`.
     *
     * The promise settles on a queued task, so a write made after this call
     * and before the task is included. The same published node returns the
     * same object while that object is reachable. A detached node, or a node
     * attached to another window, resolves `null`.
     *
     * @param node - An authored {@link AccessibleNode}.
     * @throws {@link TypeError} When `this` is not a `Window`, `node` is
     * missing, or `node` is not an `AccessibleNode`. These throw before a
     * promise is returned.
     * @throws {@link DOMException} `InvalidStateError` when the window is
     * closed. This throw happens before a promise is returned, and after the
     * argument checks. If the window closes before the queued task runs, the
     * promise rejects with the same exception.
     */
    getComputedAccessibleNode(
      node: AccessibleNode,
    ): Promise<ComputedAccessibleNode | null>;

    /**
     * A `MediaQueryList` for this window's CSS size, `devicePixelRatio`, and
     * display preferences.
     *
     * The query understands `screen` and `all`, `not` / `and` / `or`, comma
     * lists, `min-` and `max-` prefixes, and level 4 ranges for `width`,
     * `height`, `resolution`, and `device-pixel-ratio`. Also recognized:
     * `orientation`, `color-gamut` (`srgb`, `p3`, `rec2020`),
     * `prefers-color-scheme`, `prefers-reduced-motion`, `prefers-contrast`,
     * and `pointer`. `pointer` is `fine`. `hover`, `any-pointer`, and
     * `scripting` do not match. An unknown feature or a broken query does not
     * match.
     *
     * On macOS, `color-gamut: p3` matches when the main display can represent
     * Display P3. Other platforms report `srgb`. `rec2020` does not match.
     * `prefers-color-scheme` follows the window theme. On macOS,
     * `prefers-reduced-motion: reduce` follows the Reduce Motion setting, and
     * `prefers-contrast` is `more` when Increase Contrast is on. On Windows,
     * `prefers-reduced-motion: reduce` matches when Animation effects is off,
     * `prefers-contrast` is `more` while a contrast theme is on, and the
     * scheme before a window reports its theme is the apps mode. `less` and
     * `custom` do not match. Linux reports `no-preference` for those two
     * features and uses the winit theme when a window exists.
     *
     * A dedicated worker has no `matchMedia`.
     *
     * @param query - The media query text. `media` on the result is this string.
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/matchMedia)
     */
    matchMedia(query: string): MediaQueryList;

    /**
     * Shows a native modal dialog with OK.
     *
     * `message` is converted with `ToString`. There is no global `alert` and
     * no `<dialog>`. Linux uses GTK 3 and does not parent the dialog to the
     * winit window.
     *
     * @throws {@link DOMException} `NotSupportedError` when the process is
     * headless, including `YUZUKO_HEADLESS=1` and `yuzuko test`.
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    alert(message?: any): void;

    /**
     * Shows a native modal dialog with OK and Cancel.
     *
     * @returns `true` when the user chooses OK.
     * @throws {@link DOMException} `NotSupportedError` when the process is
     * headless, including `YUZUKO_HEADLESS=1` and `yuzuko test`.
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    confirm(message?: any): boolean;

    /**
     * Shows a native modal dialog with a text field, OK, and Cancel.
     *
     * @param message - Converted with `ToString`.
     * @param defaultValue - The initial field text. An omitted value is `""`.
     * @returns The entered string, `""` when the field is cleared and accepted,
     * or `null` when cancelled.
     * @throws {@link DOMException} `NotSupportedError` when the process is
     * headless, including `YUZUKO_HEADLESS=1` and `yuzuko test`.
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    prompt(message?: any, defaultValue?: string): string | null;

    /**
     * Enables or disables IME for this window.
     *
     * IME is off by default. Turning it off while a composition is open
     * dispatches `compositionend`.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    setImeAllowed(allowed: boolean): void;

    /**
     * Sets the IME candidate rectangle, in CSS pixels relative to the client origin.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    setCompositionCaret(
      x: number,
      y: number,
      width: number,
      height: number,
    ): void;

    /**
     * Grabs the cursor for this window and hides it.
     *
     * Success dispatches `pointerlockchange` on this window. Failure dispatches
     * `pointerlockerror` and rejects.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed
     * or the OS rejects the grab. The rejection name is `InvalidStateError`.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Element/requestPointerLock)
     */
    requestPointerLock(): Promise<void>;

    /**
     * Releases the pointer grab.
     *
     * A locked pointer dispatches `pointerlockchange` on this window.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Document/exitPointerLock)
     */
    exitPointerLock(): Promise<void>;

    /**
     * Makes this window fullscreen.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed
     * or the OS rejects fullscreen.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Element/requestFullscreen)
     */
    requestFullscreen(): Promise<void>;

    /**
     * Leaves fullscreen.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Document/exitFullscreen)
     */
    exitFullscreen(): Promise<void>;

    /**
     * Captures `pointerId` for this window.
     *
     * A new capture dispatches `gotpointercapture` on the surface, and the
     * event propagates through this window.
     *
     * @throws {@link DOMException} `NotFoundError` when the pointer is not active.
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Element/setPointerCapture)
     */
    setPointerCapture(pointerId: number): void;

    /**
     * Releases a pointer capture and dispatches `lostpointercapture`.
     *
     * @throws {@link DOMException} `NotFoundError` when the pointer is not captured.
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Element/releasePointerCapture)
     */
    releasePointerCapture(pointerId: number): void;

    /**
     * @returns Whether this window has captured `pointerId`.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Element/hasPointerCapture)
     */
    hasPointerCapture(pointerId: number): boolean;

    addEventListener<K extends keyof WindowEventMap>(
      type: K,
      listener: (this: Window, ev: WindowEventMap[K]) => any,
      options?: boolean | AddEventListenerOptions,
    ): void;
    addEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | AddEventListenerOptions,
    ): void;
    removeEventListener<K extends keyof WindowEventMap>(
      type: K,
      listener: (this: Window, ev: WindowEventMap[K]) => any,
      options?: boolean | EventListenerOptions,
    ): void;
    removeEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | EventListenerOptions,
    ): void;
  }

  /**
   * How {@link WindowSurface} maps its bitmap into the window.
   *
   * `"scale-down"` is the default. Pixels outside the fitted rectangle are
   * black, or clear when the window is transparent.
   */
  type ObjectFit = "none" | "contain" | "cover" | "fill" | "scale-down";

  /**
   * How {@link WindowSurface} samples its bitmap when the fitted size differs.
   *
   * `"auto"` and `"smooth"` are bilinear. `"crisp-edges"` is nearest-neighbor.
   * `"pixelated"` uses nearest-neighbor to the nearest integer scale, then
   * bilinear to the final size. The getter returns `"auto"`, `"smooth"`,
   * `"crisp-edges"`, or `"pixelated"`.
   */
  type ImageRendering =
    | "auto"
    | "smooth"
    | "crisp-edges"
    | "pixelated"
    | "optimizeQuality"
    | "optimizeSpeed";

  /** Events dispatched on a {@link WindowSurface}. */
  interface WindowSurfaceEventMap {
    visibilitychange: Event;
    beforeinput: InputEvent;
    copy: ClipboardEvent;
    cut: ClipboardEvent;
    paste: ClipboardEvent;
    pointerdown: PointerEvent;
    pointerup: PointerEvent;
    pointermove: PointerEvent;
    pointerrawupdate: PointerEvent;
    pointercancel: PointerEvent;
    pointerover: PointerEvent;
    pointerout: PointerEvent;
    pointerenter: PointerEvent;
    pointerleave: PointerEvent;
    gotpointercapture: PointerEvent;
    lostpointercapture: PointerEvent;
    mousedown: MouseEvent;
    mouseup: MouseEvent;
    mousemove: MouseEvent;
    mouseover: MouseEvent;
    mouseout: MouseEvent;
    mouseenter: MouseEvent;
    mouseleave: MouseEvent;
    click: MouseEvent;
    dblclick: MouseEvent;
    auxclick: MouseEvent;
    contextmenu: MouseEvent;
  }

  /**
   * The bitmap presented by a {@link Window}.
   *
   * Drawing on this surface, replacing its bitmap, or changing
   * {@link WindowSurface.objectFit} or {@link WindowSurface.imageRendering}
   * is presented on the next event-loop turn.
   * {@link Window.requestAnimationFrame} draws a later frame.
   *
   * This is not an {@link OffscreenCanvas} and cannot be transferred.
   * {@link WindowSurface.transferControlToOffscreen} returns the canvas a
   * worker draws. The constructor throws. The initial bitmap is
   * `round(cssWidth * devicePixelRatio)` by `round(cssHeight * devicePixelRatio)`.
   */
  class WindowSurface extends EventTarget {
    /**
     * @throws {@link TypeError} Illegal constructor.
     */
    constructor();

    /** Bitmap width in device pixels. */
    width: number;

    /** Bitmap height in device pixels. */
    height: number;

    /**
     * OS window title.
     *
     * The constructor `title` sets the initial value. The default is `"yuzuko"`.
     * {@link Window} has no `title` attribute.
     */
    title: string;

    /**
     * How the bitmap is fitted into the window.
     *
     * Matching is case-insensitive. An unrecognized value is ignored.
     *
     * @defaultValue `"scale-down"`
     */
    objectFit: ObjectFit;

    /**
     * How the bitmap is sampled when it is scaled.
     *
     * `"optimizeQuality"` is stored as `"smooth"`. `"optimizeSpeed"` is stored
     * as `"pixelated"`. Matching is case-insensitive. An unrecognized value
     * is ignored. A bitmap that is already the size of its fitted rectangle
     * is not resampled.
     *
     * @defaultValue `"auto"`
     */
    imageRendering: ImageRendering;

    /**
     * `"visible"` or `"hidden"`.
     *
     * The value follows OS occlusion. A minimized, hidden, or fully covered
     * window is `"hidden"`. Losing focus does not change it. Platforms without
     * occlusion events stay `"visible"` until the window closes. Closing a
     * visible surface sets `"hidden"` and fires `visibilitychange`.
     *
     * @defaultValue `"visible"`
     */
    readonly visibilityState: "visible" | "hidden";

    /**
     * Returns the drawing context, or `null` when this surface already has a
     * different kind.
     *
     * `"webgl"` and `"webgl2"` return `null`. Any other unknown kind throws.
     * After {@link WindowSurface.transferControlToOffscreen}, this throws.
     *
     * @throws {@link TypeError} When `contextType` is not a supported kind.
     * @throws {@link DOMException} `InvalidStateError` when control was transferred.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvas/getContext)
     */
    getContext(
      contextType: "2d",
      options?: CanvasRenderingContext2DSettings,
    ): OffscreenCanvasRenderingContext2D | null;
    getContext(
      contextType: "webgpu",
      options?: any,
    ): GPUCanvasContext | null;
    getContext(
      contextType: "bitmaprenderer",
      options?: ImageBitmapRenderingContextSettings,
    ): BitmapContext | null;
    getContext(
      contextType: "webgl" | "webgl2",
      options?: any,
    ): null;
    getContext(contextType: string, options?: any): RenderingContext | null;

    /**
     * Moves the bitmap into an {@link ImageBitmap} and clears this surface.
     *
     * @throws {@link DOMException} `InvalidStateError` when control was transferred.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvas/transferToImageBitmap)
     */
    transferToImageBitmap(): ImageBitmap;

    /**
     * Encodes the bitmap as PNG, JPEG, or WebP.
     *
     * WebP encoding is lossless, so `quality` does not change those bytes.
     * JPEG XL encoding is not available.
     *
     * @param options - `type` is `image/png`, `image/jpeg`, or `image/webp`.
     * `quality` is from 0 to 1.
     * @throws {@link DOMException} `InvalidStateError` when control was transferred.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvas/convertToBlob)
     */
    convertToBlob(options?: ImageEncodeOptions): Promise<Blob>;

    /**
     * Returns the {@link OffscreenCanvas} a worker draws for this window.
     *
     * Calling it again, or calling it after `getContext`, throws. After it
     * returns, drawing methods on this surface throw `InvalidStateError`.
     * A worker `requestAnimationFrame` copies that canvas into this window.
     * The window presents that copy on the next turn of the main event loop.
     *
     * @throws {@link DOMException} `InvalidStateError` when a context already
     * exists or control was already transferred.
     */
    transferControlToOffscreen(): OffscreenCanvas;

    /**
     * Attaches one new virtual accessibility root and returns it.
     *
     * The explainer hangs this tree from a canvas. Children are added with
     * `appendChild` on the returned node. A second call throws
     * `InvalidStateError`. After the window's `close()`, this throws
     * `InvalidStateError` before the already-attached check. Closing keeps
     * the stored root. Further platform updates stop. Tree edits still succeed.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is
     * closed, or when a root is already attached.
     * @throws {@link TypeError} On illegal invocation.
     */
    attachAccessibleRoot(): AccessibleNode;

    /**
     * The attached root, or `null` before one is attached.
     *
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     * @throws {@link TypeError} On illegal invocation.
     */
    readonly accessibleRoot: AccessibleNode | null;

    addEventListener<K extends keyof WindowSurfaceEventMap>(
      type: K,
      listener: (this: WindowSurface, ev: WindowSurfaceEventMap[K]) => any,
      options?: boolean | AddEventListenerOptions,
    ): void;
    addEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | AddEventListenerOptions,
    ): void;
    removeEventListener<K extends keyof WindowSurfaceEventMap>(
      type: K,
      listener: (this: WindowSurface, ev: WindowSurfaceEventMap[K]) => any,
      options?: boolean | EventListenerOptions,
    ): void;
    removeEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | EventListenerOptions,
    ): void;
  }

  /**
   * One menu item.
   *
   * An item has `action` or `submenu`, not both. The application menu's top
   * level must be submenus. A tray or dock menu may contain top-level actions.
   */
  interface MenuItem {
    /** Text shown for the item. */
    label: string;

    /** Value delivered as `action` on `menuclick`. */
    action?: string;

    /** Nested items. Present instead of `action`. */
    submenu?: MenuItem[];
  }

  /**
   * A menu description.
   *
   * The instance has no methods. {@link setMenu}, {@link Tray.setMenu}, and
   * {@link Dock.setMenu} install it.
   */
  class Menu {
    /**
     * @param items - The menu contents.
     * @throws {@link TypeError} When `items` is not an array, an item is not an
     * object, an item has no label, or an item has both or neither of `action`
     * and `submenu`.
     */
    constructor(items: readonly MenuItem[]);
  }

  /**
   * Installs `menu` as the application menu.
   *
   * macOS uses the application menu. Windows attaches the same menu to the
   * focused window. A selection dispatches `menuclick` on `Yuzuko`. The event
   * is an {@link Event} with an `action` string. There is no context menu.
   *
   * @param menu - The menu to install. Its top level must be submenus.
   * @throws {@link TypeError} When `menu` is not a {@link Menu}, or its top
   * level is not submenus.
   * @throws {@link DOMException} `NotSupportedError` on Linux. winit does not
   * expose a GTK window, so the menu bar cannot be installed.
   */
  function setMenu(menu: Menu): void;

  /**
   * `menuclick` dispatched on `Yuzuko`, a {@link Tray}, or {@link Dock}.
   *
   * There is no `MenuClickEvent` constructor. The event is an {@link Event}
   * with an own `action` property.
   */
  interface MenuClickEvent extends Event {
    /** The `action` of the selected item. */
    readonly action: string;
  }

  /**
   * Rectangle of a tray icon, in CSS pixels.
   */
  interface TrayBounds {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  /**
   * Options for {@link Tray.setIcon}.
   */
  interface TrayIconOptions {
    /**
     * Draw the icon as a macOS template image.
     *
     * @defaultValue false
     */
    template?: boolean;
  }

  /** Events dispatched on a {@link Tray}. */
  interface TrayEventMap {
    click: Event;
    menuclick: MenuClickEvent;
  }

  /**
   * A status item.
   *
   * An open tray keeps the process alive. A dedicated worker has no `Tray`.
   * A primary click dispatches `click`. A menu selection dispatches
   * `menuclick` with `action`.
   */
  class Tray extends EventTarget {
    /**
     * Adds a status item.
     *
     * @throws {@link DOMException} `NotSupportedError` when the system cannot
     * create a status item.
     */
    constructor();

    /**
     * Replaces the icon with a PNG.
     *
     * @param bytes - PNG bytes, as a buffer source.
     * @param options - `template: true` asks macOS to draw a template image.
     * @throws {@link TypeError} When `bytes` is not PNG bytes the decoder accepts.
     * @throws {@link DOMException} `NotSupportedError` when `template` is true
     * on Windows or Linux. Those systems have no template tray icon.
     * @throws {@link DOMException} `NotSupportedError` when the status item
     * rejects the icon.
     * @throws {@link DOMException} `InvalidStateError` when the tray is closed.
     */
    setIcon(bytes: BufferSource, options?: TrayIconOptions): void;

    /**
     * Sets the tooltip, or clears it when `text` is `null`.
     *
     * @throws {@link TypeError} When `text` is not a string or `null`.
     * @throws {@link DOMException} `NotSupportedError` on Linux. The status
     * item has no tooltip.
     * @throws {@link DOMException} `InvalidStateError` when the tray is closed.
     */
    setTooltip(text: string | null): void;

    /**
     * Installs `menu`, or clears it when `menu` is `null`.
     *
     * The menu may contain top-level actions.
     *
     * @throws {@link TypeError} When `menu` is not a {@link Menu} or `null`.
     * @throws {@link DOMException} `NotSupportedError` on Linux when replacing
     * a menu that is already installed, or when clearing a menu.
     * @throws {@link DOMException} `InvalidStateError` when the tray is closed.
     */
    setMenu(menu: Menu | null): void;

    /** Removes the status item. A second call does nothing. */
    close(): void;

    /**
     * The icon rectangle in CSS pixels, or `null`.
     *
     * `null` includes a closed tray, a zero rectangle, and the Linux
     * AppIndicator backend, which does not report the rectangle. The scale is
     * the main screen scale when one exists.
     */
    readonly bounds: TrayBounds | null;

    addEventListener<K extends keyof TrayEventMap>(
      type: K,
      listener: (this: Tray, ev: TrayEventMap[K]) => any,
      options?: boolean | AddEventListenerOptions,
    ): void;
    addEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | AddEventListenerOptions,
    ): void;
    removeEventListener<K extends keyof TrayEventMap>(
      type: K,
      listener: (this: Tray, ev: TrayEventMap[K]) => any,
      options?: boolean | EventListenerOptions,
    ): void;
    removeEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | EventListenerOptions,
    ): void;
  }

  /**
   * `reopen` dispatched on {@link Dock}.
   *
   * There is no constructor. The event is an {@link Event} with an own
   * `hasVisibleWindows` property. It is delivered only on macOS.
   */
  interface DockReopenEvent extends Event {
    /** Whether the app had a visible window when the Dock icon was clicked. */
    readonly hasVisibleWindows: boolean;
  }

  /** Events dispatched on {@link Dock}. */
  interface DockEventMap {
    menuclick: MenuClickEvent;
    reopen: DockReopenEvent;
  }

  /**
   * The process Dock target.
   *
   * There is one `Yuzuko.dock`. A dedicated worker has no `dock`. Writes the
   * system cannot do throw `NotSupportedError`.
   */
  interface Dock extends EventTarget {
    /**
     * Sets the Dock badge.
     *
     * `null` or `""` clears it.
     *
     * @throws {@link TypeError} When `text` is not a string or `null`.
     * @throws {@link DOMException} `NotSupportedError` on Windows and Linux.
     * Those systems have no Dock badge. Also thrown on macOS when the call is
     * not on the main thread.
     */
    setBadge(text: string | null): void;

    /**
     * Shows or hides the app in the macOS Dock and the app switcher.
     *
     * @throws {@link TypeError} When `visible` is not a boolean.
     * @throws {@link DOMException} `NotSupportedError` on Windows and Linux.
     * Also thrown on macOS when the activation policy is rejected.
     */
    setVisible(visible: boolean): void;

    /**
     * Installs the Dock menu, or clears it when `menu` is `null`.
     *
     * The menu may contain top-level actions. A selection dispatches
     * `menuclick` on `Yuzuko.dock`.
     *
     * @throws {@link TypeError} When `menu` is not a {@link Menu} or `null`.
     * @throws {@link DOMException} `NotSupportedError` on Windows and Linux.
     * Those systems have no Dock menu.
     */
    setMenu(menu: Menu | null): void;

    /**
     * Requests attention.
     *
     * On macOS, `bounce()` requests it once and `bounce(true)` requests it
     * until the app is focused. On Windows and Linux, the call requests
     * attention for an open window.
     *
     * @param critical - `true` requests attention until focus. Omitted is `false`.
     * @throws {@link TypeError} When the argument is present and is not a boolean.
     * @throws {@link DOMException} `NotSupportedError` on Windows and Linux
     * when no window is open.
     */
    bounce(critical?: boolean): void;

    addEventListener<K extends keyof DockEventMap>(
      type: K,
      listener: (this: Dock, ev: DockEventMap[K]) => any,
      options?: boolean | AddEventListenerOptions,
    ): void;
    addEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | AddEventListenerOptions,
    ): void;
    removeEventListener<K extends keyof DockEventMap>(
      type: K,
      listener: (this: Dock, ev: DockEventMap[K]) => any,
      options?: boolean | EventListenerOptions,
    ): void;
    removeEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | EventListenerOptions,
    ): void;
  }

  /**
   * The process Dock object.
   *
   * Absent in a dedicated worker.
   */
  const dock: Dock;

  /**
   * The main realm's {@link FontFaceSet}.
   *
   * `globalThis.fonts` is absent on the main global. A dedicated worker
   * exposes its own set as `self.fonts` and `globalThis.fonts` instead, and
   * does not use `Yuzuko.fonts`. A face added to one set is not visible in
   * the other.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet)
   */
  const fonts: FontFaceSet;

  /**
   * The Canvas 2D renderer chosen at process startup.
   *
   * `"gpu"` uses the process wgpu device. `"cpu"` is selected when no device
   * exists, when vello's GPU renderer cannot be created, or when
   * `YUZUKO_CANVAS=cpu` or `YUZUKO_SKIP_GPU=1` is set. `YUZUKO_CANVAS=gpu`
   * requests the GPU. A failed GPU request selects `"cpu"`. The value does
   * not change.
   */
  const canvasRenderer: "gpu" | "cpu";

  /**
   * Arguments recorded for this process.
   *
   * The array is frozen. `push` and other mutations throw `TypeError`.
   * Assigning this property replaces it in the current realm and leaves
   * another realm's array unchanged. `yuzuko run` records the tokens after
   * the entry. `yuzuko repl` and `yuzuko test` record an empty array. A
   * packaged executable records its arguments except every token whose text
   * starts with `-psn_`. A dedicated worker receives the same strings.
   */
  let args: readonly string[];

  /**
   * Environment variables of this process.
   *
   * The methods do not read `this`. A change is visible to a later read in
   * every realm. On Windows a lookup ignores ASCII case, and {@link Env.set}
   * of a name that differs only by that case updates the existing variable.
   * On the other targets the name is case-sensitive. Bytes that are not
   * UTF-8 become U+FFFD. `get`, `delete`, and `has` have length 1. `set` has
   * length 2. `toObject` has length 0.
   */
  interface Env {
    /**
     * Returns the stored string, or `undefined` when `name` is absent.
     *
     * @throws {@link TypeError} `The name must be a string.` when `name` is
     * not a string. `The name must not be empty.` when `name` is empty.
     * ``The name must not contain `=` or NUL.`` when `name` contains `=` or
     * NUL.
     */
    get(name: string): string | undefined;

    /**
     * Stores `value` under `name`.
     *
     * @throws {@link TypeError} for a `name` that {@link Env.get} rejects.
     * `The value must be a string.` when `value` is not a string.
     * `The value must not contain NUL.` when `value` contains NUL.
     */
    set(name: string, value: string): void;

    /**
     * Removes `name`. Removing a name that is absent succeeds.
     *
     * @throws {@link TypeError} for a `name` that {@link Env.get} rejects.
     */
    delete(name: string): void;

    /**
     * Returns whether `name` is present.
     *
     * @throws {@link TypeError} for a `name` that {@link Env.get} rejects.
     */
    has(name: string): boolean;

    /**
     * Returns a plain object snapshot of the environment at this call.
     *
     * Later {@link Env.set} and {@link Env.delete} calls leave the object
     * unchanged. Property names use the spelling the operating system stores.
     * A key `__proto__` is an own property and does not change the prototype.
     */
    toObject(): { [key: string]: string };
  }

  /** The process environment. See {@link Env}. */
  const env: Env;

  /**
   * Returns the process current directory as an absolute path.
   *
   * The string has no trailing separator except for a root. A component that
   * is not UTF-8 uses U+FFFD for those bytes.
   *
   * @throws {@link DOMException} `OperationError` with
   * `The current directory could not be read.` when the operating system
   * cannot report the directory.
   */
  function cwd(): string;

  /**
   * Sets the process current directory.
   *
   * `directory` is a string or a `file:` URL. A relative string resolves
   * against the current directory. An empty string names the current
   * directory and succeeds without changing it. The directory may be inside
   * a packaged bundle. The call does not modify the bundle. File-system
   * paths are relative to this directory.
   *
   * @throws {@link TypeError} `The path must be a string or a file URL.`
   * when `directory` is neither. `The URL scheme must be file.` for another
   * scheme. `The URL is invalid.` when the URL cannot be read.
   * `The file URL is invalid.` when a `file:` URL is not a path.
   * `The path must not contain NUL.` when the path contains NUL.
   * @throws {@link DOMException} `NotFoundError` with
   * `The directory was not found.` when the path is missing.
   * `InvalidStateError` with `The path is not a directory.` when the path
   * exists and is not a directory. `NotAllowedError` with
   * `The directory could not be changed.` when the operating system denies
   * the change. `OperationError` with `The directory could not be changed.`
   * for any other failure.
   */
  function chdir(directory: string | URL): void;

  /**
   * Ends the process with status `code` and does not return.
   *
   * An omitted or `undefined` code is 0. `-0` is 0. The code is a number
   * primitive from 0 through 255 whose fractional part is 0. The function
   * does not apply `ToNumber`. Extra arguments are ignored. `try` / `catch`
   * does not observe a call that ends the process. The first call chooses
   * the status. Open windows, trays, audio, timers, workers, and pending
   * promises do not keep the process alive after the call. A call from a
   * dedicated worker ends the whole process and does not dispatch an error
   * event for that worker.
   *
   * @throws {@link TypeError} `The exit code must be an integer from 0 through 255.`
   * when `code` is not that integer. The process stays alive.
   */
  function exit(code?: number): never;

  /**
   * Bound or remote UDP address.
   *
   * `hostname` is an IP address without brackets, or `localhost` on input.
   * A returned address uses the IP the socket is bound to or the datagram
   * came from.
   */
  interface DatagramAddr {
    /** IP address, or `localhost` when naming a destination or a bind. */
    hostname?: string;

    /**
     * UDP port.
     *
     * `0` on {@link listenDatagram} asks for an ephemeral port. A
     * destination port must be from 1 through 65535.
     */
    port?: number;
  }

  /** Options for {@link listenDatagram}. Omitted fields use the defaults. */
  interface ListenDatagramOptions {
    /**
     * Address to bind.
     *
     * @defaultValue `"127.0.0.1"`
     */
    hostname?: string;

    /**
     * Port to bind. `0` asks the operating system for a free port.
     *
     * @defaultValue 0
     */
    port?: number;
  }

  /**
   * One received datagram.
   *
   * `hostname` and `port` are the source. `data` is a copy of the payload.
   */
  interface DatagramMessage {
    /** Payload bytes. Empty when the datagram was empty. */
    data: Uint8Array<ArrayBuffer>;

    /** Source IP address, without brackets. */
    hostname: string;

    /** Source port, from 1 through 65535. */
    port: number;
  }

  /**
   * A bound UDP socket.
   *
   * Installed on the main realm and on a dedicated worker when the runtime
   * is built with the `udp` feature, which is enabled by default. Absent from
   * an `AudioWorklet` processor realm. A build that does not enable `udp`
   * leaves `Datagram` and {@link listenDatagram} absent. There is no
   * permission prompt.
   *
   * `send` and `receive` run the operating-system call on a background
   * thread. A bad argument throws {@link TypeError} before a promise is
   * returned. A closed socket rejects {@link Error}. There is no multicast,
   * broadcast option, or connected mode.
   */
  class Datagram {
    private constructor();

    /**
     * The bound address.
     *
     * Each read returns a new object. `hostname` is the bound IP, and `port`
     * is not `0` after an ephemeral bind.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * a `Datagram`.
     */
    readonly addr: DatagramAddr;

    /**
     * Sends one datagram to `addr`.
     *
     * @param data - Payload. A view sends only its own bytes.
     * @param addr - Destination. An omitted hostname is `127.0.0.1`.
     * @returns The number of bytes the operating system accepted.
     * @throws {@link TypeError} `Not enough arguments` when an argument is
     * missing. `The data must be a BufferSource.` when `data` is not a buffer
     * source. `The buffer is detached.` when it is detached.
     * `The datagram is too large.` when it is longer than 65507 bytes.
     * `The address must be an object.` when `addr` is not an object.
     * `The address could not be read.` when `addr` cannot be read.
     * `The hostname must be an IP address or "localhost".` when the hostname
     * is not an IP address or `localhost`.
     * `The port must be an integer from 1 through 65535.` when the port is
     * not in that range. `The address family does not match the socket.` when
     * the destination family differs. `illegal invocation` when the receiver
     * is not a `Datagram`. `The socket operation failed.` when the promise
     * cannot be created.
     */
    send(data: AllowSharedBufferSource, addr: DatagramAddr): Promise<number>;

    /**
     * Receives the next datagram.
     *
     * At most one receive waits. The promise rejects {@link Error} with
     * `The socket is closed.` when the socket is closed, including when
     * {@link Datagram.close} runs while this call is waiting, and with
     * `A receive is already waiting.` when another receive is waiting.
     * Another operating-system failure rejects `The datagram could not be received.`
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * a `Datagram`. `The socket operation failed.` when the promise cannot
     * be created.
     */
    receive(): Promise<DatagramMessage>;

    /**
     * Closes the socket.
     *
     * A waiting {@link Datagram.receive} rejects. Dropping the last handle
     * also closes it.
     *
     * @throws {@link Error} `The socket is closed.` when the socket is
     * already closed.
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * a `Datagram`.
     */
    close(): void;
  }

  /**
   * Binds a UDP socket.
   *
   * An omitted argument, `null`, or `undefined` binds `127.0.0.1` and an
   * ephemeral port. `hostname` defaults to `"127.0.0.1"`. `localhost` means
   * that address. Any other hostname must be an IP address without brackets.
   * There is no DNS lookup. `port` defaults to `0`. The call blocks the
   * caller for the bind.
   *
   * @throws {@link TypeError} `Options must be an object.` when `options` is
   * not an object, `null`, or `undefined`.
   * `The options could not be read.` when the options cannot be read.
   * `The hostname must be an IP address or "localhost".` when the hostname
   * is not. `The port must be an integer from 0 through 65535.` when the
   * port is not.
   * @throws {@link Error} `The socket could not bind.` when the operating
   * system refuses the address.
   */
  function listenDatagram(options?: ListenDatagramOptions | null): Datagram;

  /**
   * Bound or remote TCP address.
   *
   * `hostname` is an IP address without brackets, or `localhost` on input.
   * A returned address uses the IP the socket is bound to or the peer
   * connected from.
   */
  interface TcpAddr {
    /** IP address, or `localhost` when naming a destination or a bind. */
    hostname?: string;

    /**
     * TCP port.
     *
     * `0` on {@link listenTcp} asks for an ephemeral port. A destination
     * port must be from 1 through 65535.
     */
    port?: number;
  }

  /** Options for {@link listenTcp}. Omitted fields use the defaults. */
  interface ListenTcpOptions {
    /**
     * Address to bind.
     *
     * @defaultValue `"127.0.0.1"`
     */
    hostname?: string;

    /**
     * Port to bind. `0` asks the operating system for a free port.
     *
     * @defaultValue 0
     */
    port?: number;
  }

  /**
   * A bound TCP listener.
   *
   * Installed on the main realm and on a dedicated worker when the runtime
   * is built with the `tcp` feature, which is enabled by default. Absent from
   * an `AudioWorklet` processor realm. A build that does not enable `tcp`
   * leaves this class and {@link listenTcp} absent. There is no permission
   * prompt.
   */
  class TcpListener {
    private constructor();

    /**
     * The bound address.
     *
     * Each read returns a new object. `hostname` is the bound IP, and `port`
     * is not `0` after an ephemeral bind.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * a `TcpListener`.
     */
    readonly addr: TcpAddr;

    /**
     * Accepts the next connection.
     *
     * At most one accept waits. The promise rejects {@link Error} with
     * `The socket is closed.` when the listener is closed, including when
     * {@link TcpListener.close} runs while this call is waiting, and with
     * `An accept is already waiting.` when another accept is waiting.
     * Another operating-system failure rejects
     * `The connection could not be accepted.`
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * a `TcpListener`. `The socket operation failed.` when the promise cannot
     * be created.
     */
    accept(): Promise<TcpConn>;

    /**
     * Closes the listener.
     *
     * A waiting {@link TcpListener.accept} rejects. Dropping the last handle
     * also closes it.
     *
     * @throws {@link Error} `The socket is closed.` when the listener is
     * already closed.
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * a `TcpListener`.
     */
    close(): void;
  }

  /**
   * One TCP connection.
   *
   * `read` and `write` run the operating-system call on a background thread.
   * A bad argument throws {@link TypeError} before a promise is returned.
   * Nagle's algorithm is disabled when the operating system allows it.
   * There is no TLS and no half-close.
   */
  class TcpConn {
    private constructor();

    /**
     * The local address. Each read returns a new object.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * a `TcpConn`.
     */
    readonly localAddr: TcpAddr;

    /**
     * The peer address. Each read returns a new object.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * a `TcpConn`.
     */
    readonly remoteAddr: TcpAddr;

    /**
     * Reads bytes into `buffer`.
     *
     * Resolves the number of bytes copied, which may be shorter than
     * `buffer`. Resolves `null` when the peer ended the stream. A
     * zero-length buffer resolves `0` and does not wait. At most one read
     * waits. A write may wait at the same time.
     *
     * @param buffer - Destination. A view receives bytes only in its own range.
     * @throws {@link TypeError} `Not enough arguments` when `buffer` is
     * missing. `The data must be a BufferSource.` when it is not a buffer
     * source. `The buffer is detached.` when it is detached.
     * `The buffer is too large.` when it is longer than 2147483647 bytes.
     * `illegal invocation` when the receiver is not a `TcpConn`.
     * `The socket operation failed.` when the promise cannot be created.
     */
    read(buffer: AllowSharedBufferSource): Promise<number | null>;

    /**
     * Writes every byte of `data`.
     *
     * The bytes are copied before the promise is returned. An empty buffer
     * resolves `0`. The promise resolves the number of bytes written, which
     * is `data.byteLength`.
     *
     * @param data - Payload. A view sends only its own bytes.
     * @throws {@link TypeError} `Not enough arguments` when `data` is
     * missing. `The data must be a BufferSource.` when it is not a buffer
     * source. `The buffer is detached.` when it is detached.
     * `The buffer is too large.` when it is longer than 2147483647 bytes.
     * `illegal invocation` when the receiver is not a `TcpConn`.
     * `The socket operation failed.` when the promise cannot be created.
     */
    write(data: AllowSharedBufferSource): Promise<number>;

    /**
     * Closes the connection.
     *
     * A waiting {@link TcpConn.read} or {@link TcpConn.write} rejects.
     * Dropping the last handle also closes it.
     *
     * @throws {@link Error} `The socket is closed.` when the connection is
     * already closed.
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * a `TcpConn`.
     */
    close(): void;
  }

  /**
   * Binds a TCP socket.
   *
   * An omitted argument, `null`, or `undefined` binds `127.0.0.1` and an
   * ephemeral port. `hostname` defaults to `"127.0.0.1"`. `localhost` means
   * that address. Any other hostname must be an IP address without brackets.
   * There is no DNS lookup. `port` defaults to `0`. The call blocks the
   * caller for the bind.
   *
   * @throws {@link TypeError} `Options must be an object.` when `options` is
   * not an object, `null`, or `undefined`.
   * `The options could not be read.` when the options cannot be read.
   * `The hostname must be an IP address or "localhost".` when the hostname
   * is not. `The port must be an integer from 0 through 65535.` when the
   * port is not.
   * @throws {@link Error} `The socket could not bind.` when the operating
   * system refuses the address.
   */
  function listenTcp(options?: ListenTcpOptions | null): TcpListener;

  /**
   * Connects a TCP socket.
   *
   * `addr.hostname` defaults to `"127.0.0.1"`. `localhost` means that
   * address. Any other hostname must be an IP address without brackets.
   * There is no DNS lookup. The call returns a promise and does not block
   * the caller for the handshake.
   *
   * @param addr - Destination. An omitted hostname is `127.0.0.1`.
   * @throws {@link TypeError} `Not enough arguments` when `addr` is missing.
   * `The address must be an object.` when it is not an object.
   * `The address could not be read.` when it cannot be read.
   * `The hostname must be an IP address or "localhost".` when the hostname
   * is not. `The port must be an integer from 1 through 65535.` when the
   * port is not. `The socket operation failed.` when the promise cannot be
   * created.
   */
  function connectTcp(addr: TcpAddr): Promise<TcpConn>;

  /**
   * Bound HTTP server address.
   *
   * `hostname` is an IP address without brackets, or `localhost` on input.
   * A returned address uses the IP the socket is bound to.
   */
  interface HttpAddr {
    /** IP address, or `localhost` when naming a bind. */
    hostname?: string;

    /**
     * TCP port.
     *
     * `0` on {@link listenHttp} asks for an ephemeral port.
     */
    port?: number;
  }

  /** Options for {@link listenHttp}. Omitted fields use the defaults. */
  interface ListenHttpOptions {
    /**
     * Address to bind.
     *
     * @defaultValue `"127.0.0.1"`
     */
    hostname?: string;

    /**
     * Port to bind. `0` asks the operating system for a free port.
     *
     * @defaultValue 0
     */
    port?: number;
  }

  /**
   * An HTTP/1 server.
   *
   * Installed on the main realm and on a dedicated worker when the runtime
   * is built with the `http` feature, which is enabled by default. Absent
   * from an `AudioWorklet` processor realm. A build that does not enable
   * `http` leaves this class and {@link listenHttp} absent. The feature does
   * not enable `tcp` or `fetch`. There is no permission prompt. There is no
   * TLS and no HTTP/2.
   */
  class HttpServer {
    private constructor();

    /**
     * The bound address.
     *
     * Each read returns a new object. `hostname` is the bound IP, and `port`
     * is not `0` after an ephemeral bind.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * an `HttpServer`.
     */
    readonly addr: HttpAddr;

    /**
     * Accepts the next HTTP/1 request.
     *
     * At most one accept waits. The promise rejects {@link Error} with
     * `The socket is closed.` when the server is closed, including when
     * {@link HttpServer.close} runs while this call is waiting, and with
     * `An accept is already waiting.` when another accept is waiting.
     * A header block longer than 65536 bytes, or a body longer than
     * 1048576 bytes, rejects `The request is too large.` A malformed
     * request rejects `The request could not be read.` An operating-system
     * failure while accepting the connection rejects
     * `The connection could not be accepted.`
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * an `HttpServer`. `The socket operation failed.` when the promise cannot
     * be created.
     */
    accept(): Promise<HttpRequest>;

    /**
     * Closes the listening socket.
     *
     * A waiting {@link HttpServer.accept} rejects. An accepted request stays
     * open. Dropping the last handle also closes the listener.
     *
     * @throws {@link Error} `The socket is closed.` when the server is
     * already closed.
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * an `HttpServer`.
     */
    close(): void;
  }

  /**
   * One HTTP request on a connection accepted by {@link HttpServer}.
   *
   * `respond` writes one HTTP/1.1 response and closes the connection.
   * A bad argument throws {@link TypeError} before a promise is returned.
   */
  class HttpRequest {
    private constructor();

    /**
     * The request method.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * an `HttpRequest`.
     */
    readonly method: string;

    /**
     * The origin-form request target, including the query.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * an `HttpRequest`.
     */
    readonly url: string;

    /**
     * The request body.
     *
     * Each read returns a new `Uint8Array`. An absent `Content-Length` is
     * empty.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is not
     * an `HttpRequest`.
     */
    readonly body: Uint8Array<ArrayBuffer>;

    /**
     * Writes the response and closes the connection.
     *
     * A string is encoded as UTF-8 and sent as `text/plain;charset=UTF-8`.
     * A buffer is sent as `application/octet-stream`. The bytes are copied
     * before the promise is returned. `status` defaults to `200`.
     *
     * @param body - Response payload. A view sends only its own bytes.
     * @param status - HTTP status from 100 through 599.
     * @throws {@link TypeError} `Not enough arguments` when `body` is
     * missing. `The data must be a string or a BufferSource.` when it is
     * neither. `The buffer is detached.` when it is detached.
     * `The data is too large.` when it is longer than 1048576 bytes.
     * `The status must be an integer from 100 through 599.` when `status`
     * is present and is not. `illegal invocation` when the receiver is not
     * an `HttpRequest`. `The socket operation failed.` when the promise
     * cannot be created.
     */
    respond(
      body: string | AllowSharedBufferSource,
      status?: number,
    ): Promise<void>;
  }

  /**
   * Binds an HTTP/1 server.
   *
   * An omitted argument, `null`, or `undefined` binds `127.0.0.1` and an
   * ephemeral port. `hostname` defaults to `"127.0.0.1"`. `localhost` means
   * that address. Any other hostname must be an IP address without brackets.
   * There is no DNS lookup. `port` defaults to `0`. The call blocks the
   * caller for the bind.
   *
   * @throws {@link TypeError} `Options must be an object.` when `options` is
   * not an object, `null`, or `undefined`.
   * `The options could not be read.` when the options cannot be read.
   * `The hostname must be an IP address or "localhost".` when the hostname
   * is not. `The port must be an integer from 0 through 65535.` when the
   * port is not.
   * @throws {@link Error} `The socket could not bind.` when the operating
   * system refuses the address.
   */
  function listenHttp(options?: ListenHttpOptions | null): HttpServer;

  /** A process signal `addSignalListener` can catch. */
  type SignalName = "SIGINT" | "SIGTERM";

  /**
   * Runs `listener` when this process receives `signal`.
   *
   * Installed on the main realm and on a dedicated worker when the runtime
   * is built with the `signal` feature, which is enabled by default. Absent
   * from an `AudioWorklet` processor realm. A build that does not enable
   * `signal` leaves this function and {@link removeSignalListener} absent.
   * The feature does not enable `window`. There is no permission prompt.
   * The same function object is registered once for a signal. Listeners run
   * on that realm's thread, with no arguments, and do not keep the process
   * alive. While one is registered, that signal does not terminate the
   * process. On Windows, `"SIGTERM"` is `CTRL_BREAK_EVENT` and `"SIGINT"` is
   * `CTRL_C_EVENT`. A Windows process without a console does not observe
   * those events.
   *
   * @param signal - `"SIGINT"` or `"SIGTERM"`. A string primitive.
   * @param listener - Called once for each delivery, in registration order.
   * @throws {@link TypeError} `Not enough arguments` when an argument is
   * missing. `The signal must be a string.` when `signal` is not a string.
   * `The signal must be "SIGINT" or "SIGTERM".` when it is another string.
   * `The listener must be a function.` when `listener` is not a function.
   * @throws {@link Error} `The signal could not be caught.` when the
   * operating system refuses the handler.
   */
  function addSignalListener(
    signal: SignalName,
    listener: () => void,
  ): void;

  /**
   * Removes a listener registered with {@link addSignalListener}.
   *
   * Removing a function that is not registered succeeds. The last listener
   * for a signal, across every realm, restores that signal's previous
   * disposition. Shutting down one realm does not remove another realm's
   * listeners.
   *
   * @param signal - `"SIGINT"` or `"SIGTERM"`. A string primitive.
   * @param listener - The function passed to {@link addSignalListener}.
   * @throws {@link TypeError} `Not enough arguments` when an argument is
   * missing. `The signal must be a string.` when `signal` is not a string.
   * `The signal must be "SIGINT" or "SIGTERM".` when it is another string.
   * `The listener must be a function.` when `listener` is not a function.
   */
  function removeSignalListener(
    signal: SignalName,
    listener: () => void,
  ): void;

  /**
   * Asks the operating system to deliver `signal` to this process.
   *
   * Installed only for `yuzuko test` when the `signal` feature is enabled.
   * Absent during `yuzuko run` and `yuzuko repl`. The call returns after the
   * delivery has been queued for this realm. It does not call `listener`.
   * On Unix this is `raise`. On Windows it is `GenerateConsoleCtrlEvent`
   * for the console process group.
   *
   * @param signal - `"SIGINT"` or `"SIGTERM"`. A string primitive.
   * @throws {@link TypeError} `Not enough arguments` when `signal` is
   * missing. `The signal must be a string.` when it is not a string.
   * `The signal must be "SIGINT" or "SIGTERM".` when it is another string.
   * @throws {@link Error} `The signal is not caught.` when this realm has
   * no listener for `signal`. `The signal could not be delivered.` when
   * the operating system does not accept it.
   */
  function raiseSignal(signal: SignalName): void;

  /**
   * Arguments for {@link spawn}.
   *
   * Omitted `args` passes no arguments. Fields other than `args`, `stdin`,
   * `stdout`, `stderr`, `env`, and `cwd` are ignored.
   */
  interface SpawnOptions {
    /** Arguments passed to the program. There is no shell. */
    args?: string[] | null;

    /**
     * Standard input.
     *
     * `"null"` closes it, `"inherit"` uses the parent's input, and
     * `"piped"` exposes {@link Child.stdin}. The default is `"null"`.
     * A string primitive. `"collect"` is rejected.
     */
    stdin?: "null" | "inherit" | "piped" | null;

    /**
     * Standard output.
     *
     * `"collect"` retains the bytes for {@link Child.output}. `"null"`
     * discards them, `"inherit"` uses the parent's output, and `"piped"`
     * exposes {@link Child.stdout}. The default is `"collect"`. A string
     * primitive.
     */
    stdout?: "null" | "inherit" | "piped" | "collect" | null;

    /**
     * Standard error. The same modes as {@link SpawnOptions.stdout}.
     *
     * The default is `"collect"`.
     */
    stderr?: "null" | "inherit" | "piped" | "collect" | null;

    /**
     * Environment names to set or remove over the inherited environment.
     *
     * A string sets that name. `null` removes it. The environment is not
     * cleared. On Windows, names are case-insensitive and removals are
     * applied before assignments.
     */
    env?: { [name: string]: string | null } | null;

    /**
     * Working directory for the child.
     *
     * A string or a `file:` URL. Omitted, `null`, `undefined`, and `""`
     * inherit the current directory. A relative string is relative to the
     * process current directory.
     */
    cwd?: string | URL | null;
  }

  /**
   * The exit state and captured output of a {@link Child}.
   *
   * `code` is `null` when a signal terminated the process. `signal` is
   * `null` when the process exited, and it is always `null` on Windows.
   * A stream that was `"null"` or `"inherit"` is an empty `Uint8Array`.
   */
  interface ChildOutput {
    code: number | null;
    signal: number | null;
    stdout: Uint8Array<ArrayBuffer>;
    stderr: Uint8Array<ArrayBuffer>;
  }

  /**
   * The exit state of a {@link Child}, without its output.
   *
   * `code` is `null` when a signal terminated the process. `signal` is
   * `null` when the process exited, and it is always `null` on Windows.
   */
  interface ChildStatus {
    code: number | null;
    signal: number | null;
  }

  /**
   * A child process started by {@link spawn}.
   *
   * `output`, `status`, and piped reads and writes run outside the calling
   * thread. A running child keeps the event loop alive until it exits. A
   * collected stream keeps it alive until that collection finishes, and a
   * piped read or write keeps it alive until that promise settles. Dropping
   * this object does not terminate the process. It closes a piped standard
   * input that is still open. Realm shutdown terminates a child that is
   * still running.
   */
  class Child {
    private constructor();

    /**
     * The operating-system process id.
     *
     * The value stays after the process exits.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`.
     */
    readonly pid: number;

    /**
     * How standard input was opened: `"null"`, `"inherit"`, or `"piped"`.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`.
     */
    readonly stdinMode: "null" | "inherit" | "piped";

    /**
     * How standard output was opened.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`.
     */
    readonly stdoutMode: "null" | "inherit" | "piped" | "collect";

    /**
     * How standard error was opened.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`.
     */
    readonly stderrMode: "null" | "inherit" | "piped" | "collect";

    /**
     * Byte sink for piped standard input.
     *
     * `null` unless {@link Child.stdinMode} is `"piped"`. Each read returns
     * the same stream. A chunk that is not a buffer source errors the
     * stream with `The data must be a BufferSource.` A write that returns
     * no bytes errors it with `The write wrote no bytes.` Closing the
     * stream closes standard input.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`.
     */
    readonly stdin: WritableStream<AllowSharedBufferSource> | null;

    /**
     * Byte stream of piped standard output.
     *
     * `null` unless {@link Child.stdoutMode} is `"piped"`. Each read returns
     * the same stream. A pull reads at most 16384 bytes. There is no byte
     * cap.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`.
     */
    readonly stdout: ReadableStream<Uint8Array<ArrayBuffer>> | null;

    /**
     * Byte stream of piped standard error.
     *
     * `null` unless {@link Child.stderrMode} is `"piped"`. Each read returns
     * the same stream. A pull reads at most 16384 bytes.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`.
     */
    readonly stderr: ReadableStream<Uint8Array<ArrayBuffer>> | null;

    /**
     * Collects the exit status and the captured output.
     *
     * A second call returns the same promise. A `"collect"` stream holds at
     * most 1048576 bytes. A larger stream rejects the promise with
     * {@link Error} `The child output is too large.` and terminates the
     * process. {@link Child.status} rejects with that same error. Another
     * collection failure rejects with {@link Error}
     * `The child process failed.` A piped standard input still allows this
     * call.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`. `The child output is not collected.` when standard
     * output or standard error is `"piped"`. `The child process failed.`
     * when the promise cannot be created.
     */
    output(): Promise<ChildOutput>;

    /**
     * Resolves when the process exits.
     *
     * A second call returns the same promise. This does not read the pipes.
     * A collected stream that exceeds 1048576 bytes rejects the promise
     * with {@link Error} `The child output is too large.`
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`. `The child process failed.` when the promise cannot
     * be created.
     */
    status(): Promise<ChildStatus>;

    /**
     * Reads piped standard output into `buffer`.
     *
     * The promise resolves the number of bytes written into `buffer`, or
     * `null` at the end of the stream. A zero-length buffer resolves `0`
     * and does not read. One read can wait.
     *
     * @param buffer - Destination bytes. An `ArrayBuffer`, a
     * `SharedArrayBuffer`, or a view, including a view with a byte offset.
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`. `Not enough arguments` when `buffer` is missing.
     * `The data must be a BufferSource.` when it is not one.
     * `The buffer is detached.` when it is detached.
     * `The buffer is too large.` when it is longer than 2147483647 bytes.
     * `The standard output is not piped.` when standard output is not
     * `"piped"`. `A read is already waiting.` when a read is in progress.
     * `The bytes could not be read.` when the promise cannot be created.
     * @throws {@link Error} `The bytes could not be read.` when the read
     * fails. The process is left running.
     */
    readStdout(buffer: AllowSharedBufferSource): Promise<number | null>;

    /**
     * Reads piped standard error into `buffer`.
     *
     * The promise resolves the number of bytes written into `buffer`, or
     * `null` at the end of the stream. The checks match
     * {@link Child.readStdout}, with `The standard error is not piped.`
     * when standard error is not `"piped"`.
     *
     * @param buffer - Destination bytes.
     * @throws {@link TypeError} The same failures as {@link Child.readStdout}
     * for a missing receiver, a missing or unusable buffer, a read that is
     * already waiting, or a promise that cannot be created.
     * `The standard error is not piped.` when standard error is not
     * `"piped"`.
     * @throws {@link Error} `The bytes could not be read.` when the read
     * fails.
     */
    readStderr(buffer: AllowSharedBufferSource): Promise<number | null>;

    /**
     * Writes `data` to piped standard input.
     *
     * The bytes are copied before the call returns. The promise resolves
     * the number of bytes written. One write can wait.
     *
     * @param data - Bytes to write.
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`. `Not enough arguments` when `data` is missing.
     * `The data must be a BufferSource.` when it is not one.
     * `The buffer is detached.` when it is detached.
     * `The buffer is too large.` when it is longer than 2147483647 bytes.
     * `The standard input is not piped.` when standard input is not
     * `"piped"`. `The standard input is closed.` when it is already closed.
     * `A write is already waiting.` when a write is in progress.
     * `The bytes could not be written.` when the promise cannot be created.
     * @throws {@link Error} `The bytes could not be written.` when the
     * write fails.
     */
    writeStdin(data: AllowSharedBufferSource): Promise<number>;

    /**
     * Closes piped standard input.
     *
     * A write that is already waiting finishes first, and the pipe then
     * closes. Dropping the {@link Child} closes an input that is still open.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`. `The standard input is not piped.` when it is not
     * `"piped"`. `The standard input is closed.` when it is already closed.
     */
    closeStdin(): void;

    /**
     * Terminates the process if it is still running.
     *
     * A process that has already exited is left alone. Calling `kill`
     * again succeeds. On Unix this sends `SIGKILL`. On Windows this calls
     * `TerminateProcess` with exit code 1. The pipes close, so a blocked
     * read ends.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Child`.
     */
    kill(): void;
  }

  /**
   * Starts `command` as a child process.
   *
   * Installed on the main realm and on a dedicated worker when the runtime
   * is built with the `child` feature, which is enabled by default. Absent
   * from an `AudioWorklet` processor realm. A build that does not enable
   * `child` leaves this function and {@link Child} absent. The feature does
   * not enable `window`. There is no permission prompt and no shell. The
   * child inherits the environment and the current directory unless
   * `options.env` or `options.cwd` says otherwise. Standard input defaults
   * to closed. Standard output and standard error are collected unless a
   * mode selects another disposition. The call blocks only while the
   * operating system creates the process.
   *
   * @param command - Program path or name. A non-empty string primitive.
   * @param options - Arguments, standard streams, environment, and working
   * directory. Omitted, `null`, and `undefined` pass no arguments and use
   * the defaults.
   * @throws {@link TypeError} `Not enough arguments` when `command` is
   * missing. `The command must be a string.` when it is not a string.
   * `The command must not be empty.` when it is empty.
   * `Options must be an object.` when `options` is not an object, `null`,
   * or `undefined`. `The options could not be read.` when the options
   * cannot be read. `The arguments must be an array of strings.` when
   * `args` is not an array of string primitives.
   * `There are too many arguments.` when `args` has more than 4096
   * elements. `The command and arguments must not contain a null
   * character.` when one does.
   * `The standard input must be "null", "inherit", or "piped".` when
   * `stdin` is another value. `The standard stream must be "null",
   * "inherit", "piped", or "collect".` when `stdout` or `stderr` is
   * another value. `The environment must be an object.` when `env` is not
   * an object. `The name must be a string.`, `The name must not be empty.`,
   * and `The name must not contain `=` or NUL.` for an environment name.
   * `The value must be a string or null.` and `The value must not contain
   * NUL.` for an environment value. `The path must be a string or a file
   * URL.`, `The URL scheme must be file.`, `The URL is invalid.`,
   * `The file URL is invalid.`, and `The path must not contain NUL.` for
   * `cwd`.
   * @throws {@link Error} `The child process could not be started.` when
   * the operating system refuses the program. `The directory was not
   * found.` when `cwd` is missing. `The path is not a directory.` when
   * `cwd` is not a directory.
   */
  function spawn(
    command: string,
    options?: SpawnOptions | null,
  ): Child;

  /**
   * A parameter type {@link dlopen} can pass to a C function.
   *
   * `"i32"`, `"pointer"`, `"buffer"`, and `"cstring"` are the integer class
   * and may be mixed, up to 4. `"f64"` cannot be mixed with them, and at
   * most two `"f64"` parameters are accepted.
   */
  type ForeignType = "i32" | "f64" | "pointer" | "buffer" | "cstring";

  /**
   * A result type {@link dlopen} can return from a C function.
   *
   * `"buffer"` is not a result. An `"f64"` function returns `"void"` or
   * `"f64"`. An integer-class function returns `"void"`, `"i32"`,
   * `"pointer"`, or `"cstring"`.
   */
  type ForeignResult = "void" | "i32" | "f64" | "pointer" | "cstring";

  /**
   * The `extern "C"` shape of one symbol passed to {@link dlopen}.
   *
   * Omitted `parameters` means no arguments. Omitted `result` means `void`.
   * An array longer than 4 is rejected before its elements are read.
   * `"f64"` shapes are `() -> void`, `() -> f64`, `(f64) -> f64`,
   * `(f64, f64) -> f64`, `(f64) -> void`, and `(f64, f64) -> void`. Every
   * other accepted shape is an integer-class combination of up to 4
   * parameters with a `void`, `"i32"`, `"pointer"`, or `"cstring"` result.
   */
  interface ForeignFunction {
    /**
     * Argument types. Each entry is a string primitive `"i32"`, `"f64"`,
     * `"pointer"`, `"buffer"`, or `"cstring"`.
     */
    parameters?: ForeignType[] | null;
    /**
     * Return type. A string primitive `"void"`, `"i32"`, `"f64"`,
     * `"pointer"`, or `"cstring"`.
     */
    result?: ForeignResult | null;
  }

  /**
   * A native library opened by {@link dlopen}.
   *
   * `symbols` and `close` run on the caller thread. An open library does
   * not keep the event loop alive. Dropping this object unloads the
   * library, so it must stay reachable while a symbol function runs.
   * Realm shutdown unloads a library that is still open.
   */
  class Library {
    private constructor();

    /**
     * The functions resolved when the library was opened.
     *
     * Each read returns the same object. Its own properties are host
     * functions named by the C symbol. A function's `length` is its
     * parameter count. `i32` arguments use `ToInt32`. `f64` arguments use
     * `ToNumber`. A `pointer` is `null`, a bigint primitive, or a
     * non-negative integer number primitive, and the result is a bigint
     * (`0n` for the null pointer). A `buffer` is an `ArrayBuffer`, a
     * `SharedArrayBuffer`, or a view. A `cstring` argument is a string
     * primitive, and a `cstring` result is a string or `null`. A `void`
     * result is `undefined`. An `i32` or `f64` result is a number. A buffer
     * or C-string pointer is valid only for that call. Extra arguments are
     * ignored.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Library`, or when the called function is not one of these
     * symbols. `Not enough arguments` when a symbol call omits a
     * parameter. `The argument could not be converted.` when a number,
     * bigint, or pointer cannot be converted. `The data must be a
     * BufferSource.`, `The buffer is detached.`, and `The buffer is too
     * large.` for a buffer argument. `The string must be a string.` and
     * `The string must not contain a null character.` for a C string.
     * @throws {@link Error} `The library is closed.` when a symbol is
     * called after {@link Library.close} or after this object is dropped.
     * `The result is not a C string.` when a string result has no NUL
     * within 1048576 bytes.
     */
    readonly symbols: {
      [name: string]: (
        ...args: Array<
          number | bigint | null | AllowSharedBufferSource | string
        >
      ) => number | bigint | string | null | undefined;
    };

    /**
     * Unloads the library.
     *
     * On Unix this is `dlclose`. On Windows this is `FreeLibrary`. A
     * second call throws. Calls through `symbols` then throw as well.
     * If the operating system refuses to unload, the library still stays
     * closed to JavaScript.
     *
     * @throws {@link TypeError} `illegal invocation` when the receiver is
     * not a `Library`.
     * @throws {@link Error} `The library is closed.` when the library was
     * already closed.
     */
    close(): void;
  }

  /**
   * Loads a native library and returns functions for its C symbols.
   *
   * Installed on the main realm and on a dedicated worker when the runtime
   * is built with the `ffi` feature, which is enabled by default. Absent
   * from an `AudioWorklet` processor realm. A build that does not enable
   * `ffi` leaves this function and {@link Library} absent. The feature does
   * not enable `window`. There is no permission prompt. The call blocks
   * while the operating system loads `path` and resolves every symbol.
   * On Unix this is `dlopen` with `RTLD_NOW` and `RTLD_LOCAL`, then
   * `dlsym`. The C name has no Mach-O underscore. On Windows this is
   * `LoadLibraryW`, then `GetProcAddress`. Symbol definitions are checked
   * before the library is opened.
   *
   * @param path - Library path. A non-empty string primitive.
   * @param symbols - Own enumerable string keys are C names. Each value
   * describes that function's parameters and result.
   * @throws {@link TypeError} `Not enough arguments` when an argument is
   * missing. `The path must be a string.` when `path` is not a string.
   * `The path must not be empty.` when it is empty.
   * `The path must not contain a null character.` when it contains one.
   * `The symbols must be an object.` when `symbols` is not an object.
   * `The symbols could not be read.` when the object cannot be read.
   * `The symbol name must be a string.` when a name is not a string.
   * `The symbol name must not be empty.` when a name is empty.
   * `The symbol name must not contain a null character.` when it contains
   * one. `There are too many symbols.` when there are more than 256.
   * `The symbol definition must be an object.` when a value is not an
   * object. `The function type is not supported.` when the parameters and
   * result are not one of the supported shapes.
   * @throws {@link Error} `The library could not be opened.` when the
   * operating system refuses the library or a symbol is missing. No
   * library is returned.
   */
  function dlopen(
    path: string,
    symbols: { [name: string]: ForeignFunction },
  ): Library;

  /**
   * Result of {@link Updater.check}.
   */
  interface UpdateCheck {
    /**
     * `true` when the feed version is strictly greater than the packaged version.
     */
    available: boolean;

    /**
     * The feed version when `available` is true. Otherwise the packaged version.
     */
    version: string;
  }

  /**
   * Update client for a packaged application.
   *
   * Installed only on the main global of a packaged entry. Absent during
   * `yuzuko run`, `yuzuko repl`, `yuzuko test`, and inside a dedicated worker.
   * `check` and `download` take no URL. There is no `apply()`. The running
   * process is not replaced. The next launch applies a staged update.
   */
  interface Updater {
    /**
     * Reads `{baseUrl}/feed.json` and remembers a newer update.
     *
     * Nothing is downloaded. A thrown check leaves the remembered update
     * unchanged. Redirects are not followed.
     *
     * @returns `available` and the version to report.
     * @throws {@link DOMException} `InvalidStateError` when the package has no
     * feed, no usable trust anchor, a version that cannot be compared, or a
     * feed document, version, payload path, or signature encoding that is not
     * valid. The messages are `There is no update feed.`,
     * `There is no update trust anchor.`,
     * `The packaged version cannot be compared.`, and
     * `The update feed is not valid.`
     * @throws {@link DOMException} `NetworkError` when the status is not 200,
     * the connection fails, the body is chunked, a limit is exceeded, or the
     * response times out. The message is `The update could not be downloaded.`
     * A connection that closes before `Content-Length` bytes arrive uses
     * `The connection closed before the body ended.`
     */
    check(): Promise<UpdateCheck>;

    /**
     * Downloads the payload remembered by the last successful newer {@link Updater.check}.
     *
     * The signature is verified before anything is staged. A failed download
     * does not replace `app/` or the stage.
     *
     * @throws {@link DOMException} `InvalidStateError` when no newer update is
     * remembered, the signature does not verify, or the payload framing does
     * not match. The messages are `There is no update to download.`,
     * `The update signature is not valid.`, and
     * `The update payload is not valid.`
     * @throws {@link DOMException} `NetworkError` for the same transport
     * failures as {@link Updater.check}.
     */
    download(): Promise<void>;
  }

  /**
   * Present only on the main global of a packaged application.
   */
  const updater: Updater;

  /**
   * Options shared by the {@link test} call forms.
   */
  interface TestOptions {
    /** When true, the test is reported and not run. */
    ignore?: boolean;

    /**
     * When true, the run executes only tests marked `only`, then exits 1.
     */
    only?: boolean;
  }

  /**
   * Object form of {@link test}.
   */
  interface TestDefinition extends TestOptions {
    /** Name reported by the runner. */
    name: string;

    /** Test body. It may be async and receives a {@link TestContext}. */
    fn: (t: TestContext) => void | Promise<void>;
  }

  /**
   * Argument passed to a test function.
   */
  interface TestContext {
    /**
     * Runs `fn` as a nested step.
     *
     * A thrown step fails the test.
     *
     * @throws {@link TypeError} When the name or function is missing.
     */
    step(name: string, fn: () => void | Promise<void>): void;
  }

  /**
   * Registers a test.
   *
   * Installed only for `yuzuko test`, together with {@link build}. Absent
   * during `yuzuko run` and `yuzuko repl`. Registration happens while the
   * module evaluates. A nested call throws. `fn` may be async.
   *
   * @throws {@link TypeError} When the arguments do not match a call form, or
   * when called from inside a running test.
   *
   * @example
   * ```ts
   * Yuzuko.test("addition", () => {
   *   if (1 + 1 !== 2) throw new Error("bad sum");
   * });
   * ```
   */
  function test(fn: (t: TestContext) => void | Promise<void>): void;
  function test(
    name: string,
    fn: (t: TestContext) => void | Promise<void>,
  ): void;
  function test(
    name: string,
    options: TestOptions,
    fn: (t: TestContext) => void | Promise<void>,
  ): void;
  function test(definition: TestDefinition): void;

  namespace test {
    /**
     * Registers a test that is reported and not run.
     */
    function ignore(
      name: string,
      fn: (t: TestContext) => void | Promise<void>,
    ): void;
    function ignore(definition: TestDefinition): void;

    /**
     * Registers a focused test.
     *
     * If any test sets `only`, the run executes only those tests and exits 1.
     */
    function only(
      name: string,
      fn: (t: TestContext) => void | Promise<void>,
    ): void;
    function only(definition: TestDefinition): void;
  }

  /**
   * Host description installed with {@link test}.
   *
   * Absent outside `yuzuko test`.
   */
  namespace build {
    /**
     * The host operating system.
     *
     * `"darwin"`, `"linux"`, or `"windows"`.
     */
    const os: "darwin" | "linux" | "windows" | "unknown";
  }

  /**
   * Input injection for `yuzuko test`.
   *
   * Installed only when that command runs with `YUZUKO_TEST_INPUT=1`.
   * Absent from `yuzuko run`, `yuzuko repl`, and `yuzuko test` otherwise.
   *
   * @internal
   */
  namespace __test {
    /**
     * Delivers events to `window` and flushes that window's input.
     *
     * `events` is an array of records. `kind` is `"pointer"`, `"key"`,
     * `"ime"`, `"wheel"`, `"focus"`, `"blur"`, `"resize"`, `"visibility"`,
     * or `"trayclick"`.
     *
     * @throws {@link TypeError} When `window` is not a {@link Window} or
     * `events` is not an array of objects.
     * @throws {@link DOMException} `InvalidStateError` when the window is closed.
     */
    function inject(
      window: Window,
      events: readonly Record<string, unknown>[],
    ): void;

    /**
     * Replaces the gamepad snapshot used by `yuzuko test`.
     */
    function gamepad(devices: unknown): void;
  }

  /**
   * Open file. There is no public constructor. {@link open} and
   * {@link create} return one.
   *
   * Installed on the main realm and on a dedicated worker when the runtime
   * is built with the `fs` feature, which is enabled by default. Absent from
   * an `AudioWorklet` processor realm. A build that does not enable `fs`
   * leaves `FsFile` and the other file-system members absent.
   *
   * A synchronous method blocks the calling thread. An asynchronous method
   * runs the operating-system call on a background thread. A bad argument
   * throws {@link TypeError}. An operating-system failure throws, or rejects
   * with, one of {@link errors.NotFound}, {@link errors.AlreadyExists},
   * {@link errors.PermissionDenied}, {@link errors.IsADirectory},
   * {@link errors.NotADirectory}, {@link errors.NotSupported},
   * {@link errors.Busy}, {@link errors.Interrupted},
   * {@link errors.InvalidData}, {@link errors.BadResource}, or
   * {@link errors.FilesystemLoop}. A symbolic-link loop is
   * {@link errors.FilesystemLoop}. A directory that is not empty, and any
   * other operating-system failure, is a plain {@link Error}. Modifying a
   * packaged application bundle throws {@link errors.NotSupported}.
   *
   * A path string is absolute or relative to the process current directory.
   * A {@link URL} must be `file:`. On Windows, `mode` on {@link open},
   * {@link writeFile}, and {@link mkdir} is ignored. {@link chmod} uses the
   * owner write bit as the read-only attribute. {@link FileInfo} reports `0`
   * for `mode`, `dev`, `ino`, `nlink`, `uid`, `gid`, `blksize`, and `blocks`,
   * and `false` for the device-type flags.
   *
   * `[Symbol.dispose]` and `[Symbol.asyncDispose]` are installed when those
   * symbols exist. Both close the file.
   */
  class FsFile {
    private constructor();

    /**
     * Reads into `buffer`.
     *
     * @returns The number of bytes read, or `null` at the end of the file.
     * A zero-length view returns `0`.
     * @throws {@link TypeError} When `buffer` is not an {@link ArrayBufferView},
     * including when it is a bare {@link ArrayBuffer}, or when it is detached
     * or longer than 2147483647 bytes.
     * @throws {@link errors.BadResource} When the file is closed.
     */
    read(buffer: ArrayBufferView<ArrayBufferLike>): Promise<number | null>;

    /** Synchronous {@link FsFile.read}. It blocks the caller. */
    readSync(buffer: ArrayBufferView<ArrayBufferLike>): number | null;

    /**
     * Writes `data` once. The result can be a short write.
     *
     * @throws {@link TypeError} When `data` is not a buffer source, or when
     * it is detached or longer than 2147483647 bytes.
     * @throws {@link errors.BadResource} When the file is closed.
     */
    write(data: AllowSharedBufferSource): Promise<number>;

    /** Synchronous {@link FsFile.write}. It blocks the caller. */
    writeSync(data: AllowSharedBufferSource): number;

    /**
     * Moves the position by `offset` from `whence`.
     *
     * `whence` is {@link SeekMode.Start}, {@link SeekMode.Current}, or
     * {@link SeekMode.End}. The offset is a finite integer. A seek from the
     * start cannot be negative.
     *
     * @returns The new position, which can be above 2^53.
     * @throws {@link TypeError} When `offset` or `whence` is not one of those
     * values.
     * @throws {@link errors.BadResource} When the file is closed.
     */
    seek(offset: number, whence: number): Promise<number>;

    /** Synchronous {@link FsFile.seek}. It blocks the caller. */
    seekSync(offset: number, whence: number): number;

    /**
     * Describes the open descriptor.
     *
     * @throws {@link errors.BadResource} When the file is closed.
     */
    stat(): Promise<FileInfo>;

    /** Synchronous {@link FsFile.stat}. It blocks the caller. */
    statSync(): FileInfo;

    /**
     * Sets the file length. An omitted length is `0`.
     *
     * @throws {@link TypeError} When `len` is not a finite integer of at least 0.
     * @throws {@link errors.BadResource} When the file is closed.
     */
    truncate(len?: number): Promise<void>;

    /** Synchronous {@link FsFile.truncate}. It blocks the caller. */
    truncateSync(len?: number): void;

    /**
     * Flushes the file and its metadata.
     *
     * @throws {@link errors.BadResource} When the file is closed.
     */
    sync(): Promise<void>;

    /** Synchronous {@link FsFile.sync}. It blocks the caller. */
    syncSync(): void;

    /**
     * Flushes the file data.
     *
     * @throws {@link errors.BadResource} When the file is closed.
     */
    syncData(): Promise<void>;

    /** Synchronous {@link FsFile.syncData}. It blocks the caller. */
    syncDataSync(): void;

    /**
     * Acquires an advisory lock on the whole file.
     *
     * An omitted `exclusive` is a shared lock. `null` is shared. The call
     * waits until the lock is available. {@link FsFile.close} waits for it.
     * On Unix a second lock in the same process succeeds.
     *
     * @throws {@link TypeError} When `exclusive` cannot be read.
     * @throws {@link errors.BadResource} When the file is closed.
     */
    lock(exclusive?: boolean): Promise<void>;

    /** Synchronous {@link FsFile.lock}. It blocks the caller. */
    lockSync(exclusive?: boolean): void;

    /**
     * Tries to acquire an advisory lock on the whole file.
     *
     * @returns `true` when this call acquired the lock.
     * @throws {@link TypeError} When `exclusive` cannot be read.
     * @throws {@link errors.BadResource} When the file is closed.
     */
    tryLock(exclusive?: boolean): Promise<boolean>;

    /** Synchronous {@link FsFile.tryLock}. It blocks the caller. */
    tryLockSync(exclusive?: boolean): boolean;

    /**
     * Releases an advisory lock.
     *
     * On Unix, unlocking a file that is not locked succeeds. On Windows, that
     * unlock throws a plain {@link Error}.
     *
     * @throws {@link errors.BadResource} When the file is closed.
     */
    unlock(): Promise<void>;

    /** Synchronous {@link FsFile.unlock}. It blocks the caller. */
    unlockSync(): void;

    /**
     * Sets the access and modification times of the open descriptor.
     *
     * Each time is a {@link Date} or a finite number of seconds since the
     * Unix epoch. A string, boolean, `null`, or `undefined` is rejected.
     *
     * @throws {@link TypeError} When a time is not a {@link Date} or a finite
     * number, or when it is out of range.
     * @throws {@link errors.BadResource} When the file is closed.
     * @throws {@link errors.NotSupported} When a bundle root is set and the
     * descriptor is inside the application bundle, or its path cannot be
     * resolved.
     */
    utime(atime: Date | number, mtime: Date | number): Promise<void>;

    /** Synchronous {@link FsFile.utime}. It blocks the caller. */
    utimeSync(atime: Date | number, mtime: Date | number): void;

    /**
     * Reports whether this descriptor is a terminal.
     *
     * A regular file returns `false`.
     *
     * @throws {@link errors.BadResource} When the file is closed.
     */
    isTerminal(): boolean;

    /**
     * Puts a terminal into raw mode, or restores the saved mode.
     *
     * The first `true` saves the terminal mode. A later `true` starts from
     * that saved mode. `false` restores it. `false` with nothing saved does
     * nothing. {@link FsFile.close} restores a saved mode. `cbreak` keeps
     * signal processing.
     *
     * @throws {@link TypeError} When `options` is not an object. A `mode`
     * conversion that throws is rethrown.
     * @throws {@link errors.NotSupported} When `mode` is `true` and the file
     * is not a terminal.
     * @throws {@link errors.BadResource} When the file is closed.
     */
    setRaw(mode: boolean, options?: SetRawOptions): void;

    /**
     * Byte stream of this file.
     *
     * Each pull reads at most 16384 bytes. The same stream is returned for
     * the life of the file. Cancelling the stream does not close the file.
     * Calling this getter on a value that is not an {@link FsFile} throws
     * {@link TypeError}.
     */
    readonly readable: ReadableStream<Uint8Array<ArrayBuffer>>;

    /**
     * Byte sink for this file.
     *
     * A short write is repeated. An empty chunk writes nothing. The same
     * stream is returned for the life of the file. Closing the stream does
     * not close the file. A chunk that is not a buffer source errors the
     * stream with `Data must be a BufferSource.` A write that returns 0
     * errors it with `The write wrote no bytes.`
     */
    readonly writable: WritableStream<AllowSharedBufferSource>;

    /**
     * Closes the descriptor.
     *
     * @throws {@link errors.BadResource} When the file is already closed.
     */
    close(): void;

    /** Closes the file. Installed when `Symbol.dispose` exists. */
    [Symbol.dispose](): void;

    /**
     * Closes the file and returns a promise. Installed when
     * `Symbol.asyncDispose` exists.
     */
    [Symbol.asyncDispose](): Promise<void>;
  }

  /**
   * Options for {@link FsFile.setRaw}.
   */
  interface SetRawOptions {
    /**
     * Keep the terminal's signal processing.
     *
     * @defaultValue false
     */
    cbreak?: boolean;
  }

  /**
   * One record from {@link FsWatcher.next}.
   *
   * `flag` is present only for a rescan.
   */
  interface FsEvent {
    kind: "access" | "create" | "modify" | "remove" | "rename" | "any" | "other";
    /** Absolute paths. A directory watch on macOS names the directory. */
    paths: string[];
    flag?: "rescan";
  }

  /**
   * Directory watcher returned by {@link watchFs}. There is no public
   * constructor.
   *
   * Installed with {@link FsFile} when the runtime is built with `fs`. An
   * open watcher keeps the process alive until {@link FsWatcher.close} or
   * {@link FsWatcher.return}. On macOS a directory event names the watched
   * directory. On Windows, more than 60 paths throws {@link errors.NotSupported}.
   */
  class FsWatcher {
    private constructor();

    /**
     * Resolves the next event, or `{ done: true }` after the watcher is
     * closed and its queue is empty.
     *
     * Queued events are returned before `done`.
     */
    next(): Promise<IteratorResult<FsEvent>>;

    /**
     * Stops the watcher and resolves `{ done: true }`.
     *
     * A watcher that is already closed still resolves `{ done: true }`.
     * This call does not consume a queued event.
     */
    return(value?: unknown): Promise<IteratorResult<FsEvent>>;

    /**
     * Stops the watcher.
     *
     * @throws {@link errors.BadResource} When the watcher is already closed.
     */
    close(): void;

    /**
     * Returns this watcher. Installed when `Symbol.asyncIterator` exists.
     *
     * Calling it on a value that is not an {@link FsWatcher} throws
     * {@link TypeError}.
     */
    [Symbol.asyncIterator](): FsWatcher;
  }

  /**
   * Where {@link FsFile.seek} measures `offset` from.
   *
   * This is a plain object. `typeof Yuzuko.SeekMode` is `"object"`.
   */
  enum SeekMode {
    Start = 0,
    Current = 1,
    End = 2,
  }

  /**
   * File-system error classes captured when the realm starts.
   *
   * The thrown object is an instance of the constructor installed at startup.
   */
  namespace errors {
    class NotFound extends Error {
      constructor(message?: string);
    }
    class AlreadyExists extends Error {
      constructor(message?: string);
    }
    class PermissionDenied extends Error {
      constructor(message?: string);
    }
    class IsADirectory extends Error {
      constructor(message?: string);
    }
    class NotADirectory extends Error {
      constructor(message?: string);
    }
    class NotSupported extends Error {
      constructor(message?: string);
    }
    class Busy extends Error {
      constructor(message?: string);
    }
    class Interrupted extends Error {
      constructor(message?: string);
    }
    class InvalidData extends Error {
      constructor(message?: string);
    }
    class BadResource extends Error {
      constructor(message?: string);
    }
    class FilesystemLoop extends Error {
      constructor(message?: string);
    }
  }

  /** Result of {@link stat} and {@link FsFile.stat}. */
  interface FileInfo {
    isFile: boolean;
    isDirectory: boolean;
    isSymlink: boolean;
    /** Length in bytes. */
    size: number;
    /** Modification time, or `null` when it is missing or before the Unix epoch. */
    mtime: Date | null;
    /** Access time, or `null` when it is missing or before the Unix epoch. */
    atime: Date | null;
    /** Creation time, or `null` when it is missing or before the Unix epoch. */
    birthtime: Date | null;
    /**
     * Unix mode bits.
     *
     * On Windows this is `0`.
     */
    mode: number;
    /** On Windows this is `0`. */
    dev: number;
    /** On Windows this is `0`. */
    ino: number;
    /** On Windows this is `0`. */
    nlink: number;
    /** On Windows this is `0`. */
    uid: number;
    /** On Windows this is `0`. */
    gid: number;
    /** On Windows this is `0`. */
    blksize: number;
    /** On Windows this is `0`. */
    blocks: number;
    /** On Windows this is `false`. */
    isBlockDevice: boolean;
    /** On Windows this is `false`. */
    isCharDevice: boolean;
    /** On Windows this is `false`. */
    isFifo: boolean;
    /** On Windows this is `false`. */
    isSocket: boolean;
  }

  /** One {@link readDir} result. */
  interface DirEntry {
    name: string;
    isFile: boolean;
    isDirectory: boolean;
    isSymlink: boolean;
  }

  /**
   * Options for {@link open}.
   *
   * Omitted options read. On an object, `read` defaults to true and the other
   * booleans default to false. `signal` is ignored.
   */
  interface OpenOptions {
    /** @defaultValue true */
    read?: boolean;
    /** @defaultValue false */
    write?: boolean;
    /** @defaultValue false */
    append?: boolean;
    /** @defaultValue false */
    truncate?: boolean;
    /** @defaultValue false */
    create?: boolean;
    /** @defaultValue false */
    createNew?: boolean;
    /**
     * Unix permission bits, from 0 through 262143. The process umask still
     * applies. Ignored on Windows.
     */
    mode?: number;
    /** Ignored. */
    signal?: unknown;
  }

  /**
   * Options for {@link writeFile} and {@link writeTextFile}.
   *
   * The file is created and truncated unless `append` is set. `signal` is
   * ignored.
   */
  interface WriteFileOptions {
    /** @defaultValue false */
    append?: boolean;
    /** @defaultValue true */
    create?: boolean;
    /** @defaultValue false */
    createNew?: boolean;
    /**
     * Unix permission bits, from 0 through 262143. The process umask still
     * applies. Ignored on Windows.
     */
    mode?: number;
    /** Ignored. */
    signal?: unknown;
  }

  /** Options for {@link mkdir}. */
  interface MkdirOptions {
    /**
     * Create missing parents. Succeeds when the directory already exists.
     *
     * @defaultValue false
     */
    recursive?: boolean;
    /**
     * Unix permission bits, from 0 through 262143. The process umask still
     * applies. Ignored on Windows.
     */
    mode?: number;
  }

  /** Options for {@link remove}. */
  interface RemoveOptions {
    /**
     * Delete a directory tree. A symlink is still removed as a file.
     *
     * @defaultValue false
     */
    recursive?: boolean;
  }

  /**
   * Options for {@link symlink}.
   *
   * Unix ignores `type`. Windows creates that kind of link.
   */
  interface SymlinkOptions {
    /**
     * @defaultValue `"file"`
     */
    type?: "file" | "dir" | "junction";
  }

  /** Options for {@link watchFs}. */
  interface WatchOptions {
    /**
     * Watch nested directories.
     *
     * @defaultValue true
     */
    recursive?: boolean;
  }

  /** Options for {@link makeTempDir} and {@link makeTempFile}. */
  interface MakeTempOptions {
    /** Directory that receives the new path. The default is the system temporary directory. */
    dir?: string | URL;
    /** @defaultValue `""` */
    prefix?: string;
    /** @defaultValue `""` */
    suffix?: string;
  }

  /**
   * Opens `path`.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL, when
   * `options` is not an object, or when the flags disagree.
   * @throws {@link errors.NotSupported} When a write would modify the
   * application bundle.
   */
  function open(path: string | URL, options?: OpenOptions): Promise<FsFile>;

  /** Synchronous {@link open}. It blocks the caller. */
  function openSync(path: string | URL, options?: OpenOptions): FsFile;

  /**
   * Opens `path` for write, creating or truncating it.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL.
   */
  function create(path: string | URL): Promise<FsFile>;

  /** Synchronous {@link create}. It blocks the caller. */
  function createSync(path: string | URL): FsFile;

  /**
   * Reads `path` as bytes.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL.
   * @throws {@link errors.IsADirectory} When `path` is a directory.
   * @throws {@link errors.NotFound} When no file is at `path`.
   */
  function readFile(path: string | URL): Promise<Uint8Array<ArrayBuffer>>;

  /** Synchronous {@link readFile}. It blocks the caller. */
  function readFileSync(path: string | URL): Uint8Array<ArrayBuffer>;

  /**
   * Reads `path` as UTF-8 text.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL.
   * @throws {@link errors.InvalidData} When the bytes are not UTF-8.
   * @throws {@link errors.IsADirectory} When `path` is a directory.
   */
  function readTextFile(path: string | URL): Promise<string>;

  /** Synchronous {@link readTextFile}. It blocks the caller. */
  function readTextFileSync(path: string | URL): string;

  /**
   * Writes `data` to `path`.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL, when
   * `data` is not a buffer source, or when `options` is not an object.
   * @throws {@link errors.NotSupported} When the resolved path is inside the
   * application bundle.
   */
  function writeFile(
    path: string | URL,
    data: AllowSharedBufferSource,
    options?: WriteFileOptions,
  ): Promise<void>;

  /** Synchronous {@link writeFile}. It blocks the caller. */
  function writeFileSync(
    path: string | URL,
    data: AllowSharedBufferSource,
    options?: WriteFileOptions,
  ): void;

  /**
   * Writes `text` to `path` as UTF-8.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL, when
   * `text` is not a string, or when `options` is not an object.
   */
  function writeTextFile(
    path: string | URL,
    text: string,
    options?: WriteFileOptions,
  ): Promise<void>;

  /** Synchronous {@link writeTextFile}. It blocks the caller. */
  function writeTextFileSync(
    path: string | URL,
    text: string,
    options?: WriteFileOptions,
  ): void;

  /**
   * Creates a directory at `path`.
   *
   * The path is resolved, including symlinks and `..`, before anything is
   * created.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL, or
   * when `options` is not an object.
   * @throws {@link errors.AlreadyExists} When the directory exists and
   * `recursive` is not set, or when the resolved path exists and is not a
   * directory.
   */
  function mkdir(path: string | URL, options?: MkdirOptions): Promise<void>;

  /** Synchronous {@link mkdir}. It blocks the caller. */
  function mkdirSync(path: string | URL, options?: MkdirOptions): void;

  /**
   * Removes `path`.
   *
   * A symlink is removed as a file and is not followed.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL, or
   * when `options` is not an object.
   */
  function remove(path: string | URL, options?: RemoveOptions): Promise<void>;

  /** Synchronous {@link remove}. It blocks the caller. */
  function removeSync(path: string | URL, options?: RemoveOptions): void;

  /**
   * Renames `oldPath` to `newPath`.
   *
   * @throws {@link TypeError} When either path is not a string or a file URL.
   */
  function rename(oldPath: string | URL, newPath: string | URL): Promise<void>;

  /** Synchronous {@link rename}. It blocks the caller. */
  function renameSync(oldPath: string | URL, newPath: string | URL): void;

  /**
   * Describes `path`, following a final symlink.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL.
   * @throws {@link errors.NotFound} When `path` does not exist.
   */
  function stat(path: string | URL): Promise<FileInfo>;

  /** Synchronous {@link stat}. It blocks the caller. */
  function statSync(path: string | URL): FileInfo;

  /**
   * Describes `path` without following a final symlink.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL.
   */
  function lstat(path: string | URL): Promise<FileInfo>;

  /** Synchronous {@link lstat}. It blocks the caller. */
  function lstatSync(path: string | URL): FileInfo;

  /**
   * Lists `path`.
   *
   * The array is not sorted.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL.
   * @throws {@link errors.NotADirectory} When `path` is not a directory.
   */
  function readDir(path: string | URL): Promise<DirEntry[]>;

  /** Synchronous {@link readDir}. It blocks the caller. */
  function readDirSync(path: string | URL): DirEntry[];

  /**
   * Copies `from` onto `to`, overwriting `to`.
   *
   * @throws {@link TypeError} When either path is not a string or a file URL.
   * @throws {@link errors.NotSupported} When `to` resolves inside the
   * application bundle.
   */
  function copyFile(from: string | URL, to: string | URL): Promise<void>;

  /** Synchronous {@link copyFile}. It blocks the caller. */
  function copyFileSync(from: string | URL, to: string | URL): void;

  /**
   * Returns the canonical path of `path`.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL.
   * @throws {@link errors.NotFound} When `path` does not exist.
   */
  function realPath(path: string | URL): Promise<string>;

  /** Synchronous {@link realPath}. It blocks the caller. */
  function realPathSync(path: string | URL): string;

  /**
   * Sets the length of the file at `path`. The file must already exist.
   * An omitted length is `0`.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL, or
   * when `len` is not a finite integer of at least 0.
   */
  function truncate(path: string | URL, len?: number): Promise<void>;

  /** Synchronous {@link truncate}. It blocks the caller. */
  function truncateSync(path: string | URL, len?: number): void;

  /**
   * Creates an empty directory with a new name and returns its absolute path.
   *
   * @throws {@link TypeError} When `options` is not an object, or when
   * `prefix` or `suffix` is not a string or contains a path separator or NUL.
   * @throws {@link errors.AlreadyExists} When a unique name cannot be created.
   */
  function makeTempDir(options?: MakeTempOptions): Promise<string>;

  /** Synchronous {@link makeTempDir}. It blocks the caller. */
  function makeTempDirSync(options?: MakeTempOptions): string;

  /**
   * Creates an empty file with a new name and returns its absolute path.
   *
   * @throws {@link TypeError} When `options` is not an object, or when
   * `prefix` or `suffix` is not a string or contains a path separator or NUL.
   */
  function makeTempFile(options?: MakeTempOptions): Promise<string>;

  /** Synchronous {@link makeTempFile}. It blocks the caller. */
  function makeTempFileSync(options?: MakeTempOptions): string;

  /**
   * Sets the permissions of `path`. The process umask does not apply.
   *
   * On Unix the operating system may store fewer than 18 bits. On macOS the
   * stored mode is 16 bits. On Windows only the owner write bit is used, as
   * the read-only attribute.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL, or
   * when `mode` is not an integer from 0 through 262143.
   * @throws {@link errors.NotSupported} When the resolved path is inside the
   * application bundle.
   */
  function chmod(path: string | URL, mode: number): Promise<void>;

  /** Synchronous {@link chmod}. It blocks the caller. */
  function chmodSync(path: string | URL, mode: number): void;

  /**
   * Changes the owner of `path`. `null` leaves that id unchanged.
   *
   * On Windows this throws {@link errors.NotSupported} with
   * `Changing the owner is not supported on Windows.` and does not touch the
   * file system.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL, or
   * when `uid` or `gid` is not `null` or an integer from 0 through 4294967295.
   * @throws {@link errors.NotSupported} On Windows.
   */
  function chown(
    path: string | URL,
    uid: number | null,
    gid: number | null,
  ): Promise<void>;

  /** Synchronous {@link chown}. It blocks the caller. */
  function chownSync(
    path: string | URL,
    uid: number | null,
    gid: number | null,
  ): void;

  /**
   * Returns the process file-creation mask.
   *
   * An omitted mask is left in place. On Unix, reading the mask sets it to 0
   * and restores it, and another thread can observe that window. On Windows
   * the result is `0` and a mask in range is ignored.
   *
   * @throws {@link TypeError} When `mask` is present and is not an integer
   * from 0 through 262143.
   */
  function umask(mask?: number): number;

  /**
   * Creates a hard link from `oldPath` to `newPath`.
   *
   * The final component of neither path is followed.
   *
   * @throws {@link TypeError} When either path is not a string or a file URL.
   * @throws {@link errors.NotSupported} When either resolved directory entry
   * is inside the application bundle.
   */
  function link(oldPath: string | URL, newPath: string | URL): Promise<void>;

  /** Synchronous {@link link}. It blocks the caller. */
  function linkSync(oldPath: string | URL, newPath: string | URL): void;

  /**
   * Creates a symbolic link at `newPath` whose text is `oldPath`.
   *
   * A string is stored as that string. A file URL is stored as its
   * file-system path. The target does not have to exist. Unix ignores
   * `options.type`. On Windows, creating a symlink can throw
   * {@link errors.PermissionDenied} when the process may not create one.
   *
   * @throws {@link TypeError} When either path is not a string or a file URL,
   * when `options` is not an object, or when `type` is not `"file"`,
   * `"dir"`, or `"junction"`.
   * @throws {@link errors.NotSupported} When `newPath` is inside the
   * application bundle.
   */
  function symlink(
    oldPath: string | URL,
    newPath: string | URL,
    options?: SymlinkOptions,
  ): Promise<void>;

  /** Synchronous {@link symlink}. It blocks the caller. */
  function symlinkSync(
    oldPath: string | URL,
    newPath: string | URL,
    options?: SymlinkOptions,
  ): void;

  /**
   * Returns the text stored in the symbolic link at `path`.
   *
   * The text is not canonicalized. A NUL becomes U+FFFD.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL.
   */
  function readLink(path: string | URL): Promise<string>;

  /** Synchronous {@link readLink}. It blocks the caller. */
  function readLinkSync(path: string | URL): string;

  /**
   * Sets the access and modification times of `path`, following a final
   * symlink.
   *
   * Each time is a {@link Date} or a finite number of seconds since the Unix
   * epoch. A string, boolean, `null`, or `undefined` is rejected.
   *
   * @throws {@link TypeError} When `path` is not a string or a file URL, when
   * a time is not a {@link Date} or a finite number, or when a time is out of
   * range.
   * @throws {@link errors.NotSupported} When the resolved path is inside the
   * application bundle.
   */
  function utime(
    path: string | URL,
    atime: Date | number,
    mtime: Date | number,
  ): Promise<void>;

  /** Synchronous {@link utime}. It blocks the caller. */
  function utimeSync(
    path: string | URL,
    atime: Date | number,
    mtime: Date | number,
  ): void;

  /**
   * Watches `paths` and returns immediately.
   *
   * `paths` is a string, a file URL, or an array of those. The paths must
   * exist. `recursive` defaults to true. On macOS a directory watch reports
   * the watched directory. On Windows, more than 60 paths throws
   * {@link errors.NotSupported} before existence is checked.
   *
   * @throws {@link TypeError} When `paths` is empty or not a path, or when
   * `options` is not an object.
   * @throws {@link errors.NotFound} When a path does not exist.
   * @throws {@link errors.NotSupported} On Windows, when more than 60 paths
   * are given or a file path has no parent directory. On an operating system
   * other than Linux, macOS, and Windows, `watchFs` throws
   * {@link errors.NotSupported}.
   */
  function watchFs(
    paths: string | URL | readonly (string | URL)[],
    options?: WatchOptions,
  ): FsWatcher;

  /**
   * Registers a listener on the `Yuzuko` object.
   *
   * The object is an {@link EventTarget}. A menu selection dispatches
   * `menuclick`. The event is an {@link Event} with an `action` string.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventTarget/addEventListener)
   */
  function addEventListener(
    type: "menuclick",
    listener: (ev: MenuClickEvent) => any,
    options?: boolean | AddEventListenerOptions,
  ): void;
  function addEventListener(
    type: string,
    listener: EventListenerOrEventListenerObject | null,
    options?: boolean | AddEventListenerOptions,
  ): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventTarget/removeEventListener)
   */
  function removeEventListener(
    type: "menuclick",
    listener: (ev: MenuClickEvent) => any,
    options?: boolean | EventListenerOptions,
  ): void;
  function removeEventListener(
    type: string,
    listener: EventListenerOrEventListenerObject | null,
    options?: boolean | EventListenerOptions,
  ): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventTarget/dispatchEvent)
   */
  function dispatchEvent(event: Event): boolean;
}

/**
 * Alias of `globalThis` in every realm. Assignment does not replace it.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/self)
 */
declare const self: Window & typeof globalThis;

/**
 * Alias of `globalThis` on the main realm.
 *
 * This is the execution realm, not a {@link Yuzuko.Window}. There is no global
 * `Window` constructor. A dedicated worker does not have `window`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/window)
 */
declare var window: Window & typeof globalThis;

/**
 * Reports an error on `globalThis`.
 *
 * The handler is called as `onerror(message, filename, lineno, colno, error)`.
 * Returning `true` prevents the default report.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/error_event)
 */
declare var onerror: OnErrorEventHandler;

/**
 * Fired when a rejection has no handler.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/unhandledrejection_event)
 */
declare var onunhandledrejection:
  | ((this: Window, ev: PromiseRejectionEvent) => any)
  | null;

/**
 * Fired when a handler is attached after `unhandledrejection`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/rejectionhandled_event)
 */
declare var onrejectionhandled:
  | ((this: Window, ev: PromiseRejectionEvent) => any)
  | null;

/**
 * Fired on the main global when a gamepad connects.
 *
 * There is no secure-context gate. An already-connected device is exposed
 * without waiting for a button press. A dedicated worker does not have this
 * handler.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/gamepadconnected_event)
 */
declare var ongamepadconnected:
  | ((this: Window, ev: GamepadEvent) => any)
  | null;

/**
 * Fired on the main global when a gamepad disconnects.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/gamepaddisconnected_event)
 */
declare var ongamepaddisconnected:
  | ((this: Window, ev: GamepadEvent) => any)
  | null;

/**
 * The event currently being dispatched. Replaceable.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/event)
 */
declare var event: Event | undefined;

/**
 * Schedules `handler` after `timeout` milliseconds.
 *
 * Extra arguments are passed to a function handler. A string handler is
 * evaluated as code. A negative or non-finite timeout is clamped to 0.
 * Nested timeouts below 4ms are clamped.
 *
 * @returns A handle for {@link clearTimeout}.
 * @throws {@link TypeError} When the callback is missing.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/setTimeout)
 */
declare function setTimeout(
  handler: TimerHandler,
  timeout?: number,
  ...arguments: any[]
): number;

/**
 * Cancels a timeout.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/clearTimeout)
 */
declare function clearTimeout(id?: number): void;

/**
 * Repeats `handler` every `timeout` milliseconds.
 *
 * @returns A handle for {@link clearInterval}.
 * @throws {@link TypeError} When the callback is missing.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/setInterval)
 */
declare function setInterval(
  handler: TimerHandler,
  timeout?: number,
  ...arguments: any[]
): number;

/**
 * Cancels an interval.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/clearInterval)
 */
declare function clearInterval(id?: number): void;

/**
 * Queues `callback` as a microtask.
 *
 * @throws {@link TypeError} When `callback` is not a function.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/queueMicrotask)
 */
declare function queueMicrotask(callback: () => void): void;

/**
 * Reports `error` on `globalThis` as an {@link ErrorEvent}.
 *
 * Returning `true` from `onerror` suppresses the console report.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/reportError)
 */
declare function reportError(error: any): void;

/**
 * Clones a value using the structured clone algorithm.
 *
 * Supported values include the WinterTC built-ins in this runtime,
 * `ArrayBuffer`, typed arrays, `Map`, `Set`, `Date`, `RegExp`, and `Error`.
 * It also copies `Blob`, `File`, `DOMException`, `DOMPoint`,
 * `DOMPointReadOnly`, `DOMRect`, `DOMRectReadOnly`, `DOMQuad`, `DOMMatrix`,
 * `DOMMatrixReadOnly`, `CryptoKey`, `ImageData`, `ImageBitmap`, `AudioData`,
 * `VideoFrame`, `EncodedAudioChunk`, and `EncodedVideoChunk`.
 *
 * @param value - The value to clone.
 * @param options - `transfer` moves an `ArrayBuffer`, a `MessagePort`, an
 * `OffscreenCanvas`, an `ImageBitmap`, an `AudioData`, a `VideoFrame`, a
 * `ReadableStream`, a `WritableStream`, or a `TransformStream`.
 * @throws {@link DOMException} `DataCloneError` when `value` contains a host
 * object this runtime cannot clone, including a {@link Yuzuko.Window}, a
 * {@link Yuzuko.WindowSurface}, a closed {@link VideoFrame} or
 * {@link AudioData}, a detached {@link ImageBitmap}, a locked stream, a
 * stream left out of `transfer`, or a listed value that cannot be
 * transferred.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/structuredClone)
 */
declare function structuredClone(
  value: any,
  options?: StructuredSerializeOptions,
): any;

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/structuredClone)
 */
interface StructuredSerializeOptions {
  transfer?: Transferable[];
}

/**
 * Decodes a base64 string.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/atob)
 */
declare function atob(data: string): string;

/**
 * Encodes a binary string as base64.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/btoa)
 */
declare function btoa(data: string): string;

/**
 * Schedules `callback` during a later idle period.
 *
 * The main global only. A dedicated worker does not have this function.
 * A period is at most 50ms and also ends at the next timer or animation
 * frame. It does not start when less than 5ms remains before that timer or
 * frame. A hidden window does not throttle the period. There is no document.
 *
 * @param callback - Receives an {@link IdleDeadline}.
 * @param options - `timeout` still runs the callback when no idle period starts.
 * `didTimeout` is then true and `timeRemaining()` is 0.
 * @returns A handle for {@link cancelIdleCallback}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/requestIdleCallback)
 */
declare function requestIdleCallback(
  callback: IdleRequestCallback,
  options?: IdleRequestOptions,
): number;

/**
 * Cancels an idle callback.
 *
 * Main global only.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/cancelIdleCallback)
 */
declare function cancelIdleCallback(handle: number): void;

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/requestIdleCallback)
 */
interface IdleRequestOptions {
  timeout?: number;
}

/**
 * Time remaining in an idle period.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/IdleDeadline)
 */
interface IdleDeadline {

  /**
   * Milliseconds left in this period, or 0 when `didTimeout` is true.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/IdleDeadline/timeRemaining)
   */
  timeRemaining(): number;

  /**
   * True when the callback ran because `timeout` elapsed.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/IdleDeadline/didTimeout)
   */
  readonly didTimeout: boolean;
}
declare var IdleDeadline: {
  prototype: IdleDeadline;
  new(): IdleDeadline;
};


/**
 * Logging object.
 *
 * `dirxml` and `table` print like `log`. `table` does not draw a grid.
 * `clear` succeeds and does not erase a surface. `time`, `timeLog`, and
 * `timeEnd` share one table. Objects print their contents, uncolored.
 * A host class instance prints that class's attributes, so
 * `AbortController` includes `signal`. A getter on a user class is
 * printed too. Inherited data properties stay hidden. The printer
 * does not call a user inspection hook. A revoked proxy throws
 * `TypeError`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console)
 */
declare var console: Console;

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console)
 */
interface Console {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/log_static) */
  log(...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/info_static) */
  info(...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/debug_static) */
  debug(...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/warn_static) */
  warn(...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/error_static) */
  error(...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/trace_static) */
  trace(...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/assert_static) */
  assert(condition?: boolean, ...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/time_static) */
  time(label?: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/timeLog_static) */
  timeLog(label?: string, ...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/timeEnd_static) */
  timeEnd(label?: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/count_static) */
  count(label?: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/countReset_static) */
  countReset(label?: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/group_static) */
  group(...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/groupCollapsed_static) */
  groupCollapsed(...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/groupEnd_static) */
  groupEnd(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/dir_static) */
  dir(...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/dirxml_static) */
  dirxml(...data: any[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/table_static) */
  table(tabularData?: any, properties?: readonly string[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/clear_static) */
  clear(): void;
}

/**
 * Milliseconds since the time origin.
 *
 * `now()` and event `timeStamp` are not coarsened. This process is not a
 * cross-origin browser context. `toJSON()` returns `{ timeOrigin }`.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Performance)
 */
interface Performance extends EventTarget {

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Performance/now)
   */
  now(): DOMHighResTimeStamp;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Performance/timeOrigin)
   */
  readonly timeOrigin: DOMHighResTimeStamp;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Performance/toJSON)
   */
  toJSON(): { timeOrigin: DOMHighResTimeStamp };
}
declare var Performance: {
  prototype: Performance;
  new(): Performance;
};


/**
 * The realm's performance object.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/performance)
 */
declare var performance: Performance;

/**
 * Host information for this realm.
 *
 * `userAgent` is `Yuzuko/` followed by the runtime version. `platform` is
 * `"Win32"` on Windows, `"MacIntel"` on macOS, and `"Linux"` on Linux.
 * `clipboard`, `gpu`, `wakeLock`, and `getGamepads` exist on the main global.
 * A worker navigator has `userAgent` and `platform` only.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Navigator)
 */
interface Navigator {

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Navigator/userAgent)
   */
  readonly userAgent: string;

  /**
   * Historical platform token: `"Win32"`, `"MacIntel"`, or `"Linux"`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Navigator/platform)
   */
  readonly platform: string;

  /**
   * The process clipboard. Main global only.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Navigator/clipboard)
   */
  readonly clipboard: Clipboard;

  /**
   * Screen wake lock for this realm. Main global only.
   *
   * Absent without the `window` feature, and absent on a dedicated worker.
   * There is no permission prompt and no document. Hiding a window does not
   * release a sentinel.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Navigator/wakeLock)
   */
  readonly wakeLock: WakeLock;

  /**
   * The process WebGPU entry. Main global only.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Navigator/gpu)
   */
  readonly gpu: GPU;

  /**
   * Connected gamepads.
   *
   * Empty slots are `null`. There is no secure-context gate, permissions
   * policy, or document. `touches` stays empty, `pose` is null, and `hand`
   * is `""`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Navigator/getGamepads)
   */
  getGamepads(): (Gamepad | null)[];
}
declare var Navigator: {
  prototype: Navigator;
  new(): Navigator;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/navigator)
 */
declare var navigator: Navigator;

/**
 * Screen wake lock for the main realm.
 *
 * `request()` asks the operating system to keep the display awake while a
 * sentinel is held. macOS creates an IOPMAssertion of type
 * `PreventUserIdleDisplaySleep` named `Yuzuko`. Windows calls
 * `SetThreadExecutionState` with `ES_DISPLAY_REQUIRED`. Linux starts
 * `systemd-inhibit` for `idle:sleep`.
 *
 * @throws {@link TypeError} Illegal constructor. Use {@link Navigator.wakeLock}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WakeLock)
 */
interface WakeLock {
  /**
   * Acquires a screen wake lock.
   *
   * An omitted type, or `"screen"`, resolves a sentinel. Any other type
   * rejects `TypeError`.
   *
   * @param type Wake lock type. The only accepted value is `"screen"`.
   * @returns A sentinel while the operating system holds the display awake.
   * @throws {@link TypeError} `The wake lock type is invalid` when `type` is
   * not `"screen"`.
   * @throws {@link DOMException} `NotAllowedError` with
   * `The platform refused the screen wake lock` when the operating system
   * refuses the request. On Linux this includes a missing `systemd-inhibit`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WakeLock/request)
   */
  request(type?: WakeLockType): Promise<WakeLockSentinel>;
}

/** The only wake lock type is `"screen"`. */
type WakeLockType = "screen";

declare var WakeLock: {
  prototype: WakeLock;
  new(): WakeLock;
};

/**
 * One hold on the screen wake lock.
 *
 * `release()` sets `released` and dispatches `release` before the promise
 * resolves. A second call resolves and does not dispatch again. The operating
 * system assertion stays until the last held sentinel releases.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WakeLockSentinel)
 */
interface WakeLockSentinel extends EventTarget {
  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WakeLockSentinel/released)
   */
  readonly released: boolean;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WakeLockSentinel/type)
   */
  readonly type: WakeLockType;

  /**
   * Ends this sentinel's hold.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WakeLockSentinel/release)
   */
  release(): Promise<void>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WakeLockSentinel/release_event)
   */
  onrelease: ((this: WakeLockSentinel, ev: Event) => any) | null;
}
declare var WakeLockSentinel: {
  prototype: WakeLockSentinel;
  new(): WakeLockSentinel;
};

/**
 * Target of events.
 *
 * `globalThis` inherits this prototype. Pointer and key events are delivered
 * to {@link Yuzuko.Window}, not to `globalThis`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventTarget)
 */
declare class EventTarget {
  constructor();


  /**
   * Registers `callback` for `type`.
   *
   * `options` may be a capture boolean or a dictionary with `capture`,
   * `once`, `passive`, and `signal`. A null `signal` throws. `null` or
   * `undefined` as `callback` registers nothing.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventTarget/addEventListener)
   */
  addEventListener(
    type: string,
    callback: EventListenerOrEventListenerObject | null,
    options?: AddEventListenerOptions | boolean,
  ): void;

  /**
   * Removes a listener registered with the same capture flag.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventTarget/removeEventListener)
   */
  removeEventListener(
    type: string,
    callback: EventListenerOrEventListenerObject | null,
    options?: EventListenerOptions | boolean,
  ): void;

  /**
   * Dispatches `event` at this target.
   *
   * @returns `false` when `preventDefault()` was called on a cancelable event.
   * @throws {@link TypeError} When `event` is not an {@link Event}.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventTarget/dispatchEvent)
   */
  dispatchEvent(event: Event): boolean;
}

/**
 * Dictionary for `new Event`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/Event)
 */
interface EventInit {
  bubbles?: boolean;
  cancelable?: boolean;
  composed?: boolean;
}

/**
 * An event moving through a dispatch path.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event)
 */
interface Event {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/type) */
  readonly type: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/bubbles) */
  readonly bubbles: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/cancelable) */
  readonly cancelable: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/composed) */
  readonly composed: boolean;

  /**
   * Milliseconds from the time origin. Not coarsened.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/timeStamp)
   */
  readonly timeStamp: DOMHighResTimeStamp;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/defaultPrevented) */
  readonly defaultPrevented: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/eventPhase) */
  readonly eventPhase: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/target) */
  readonly target: EventTarget | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/currentTarget) */
  readonly currentTarget: EventTarget | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/srcElement) */
  readonly srcElement: EventTarget | null;

  /**
   * False after `preventDefault()`. Assigning `false` calls `preventDefault()`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/returnValue)
   */
  returnValue: boolean;

  /**
   * True for events dispatched by the host.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/isTrusted)
   */
  readonly isTrusted: boolean;

  readonly NONE: 0;
  readonly CAPTURING_PHASE: 1;
  readonly AT_TARGET: 2;
  readonly BUBBLING_PHASE: 3;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/preventDefault)
   */
  preventDefault(): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/stopPropagation)
   */
  stopPropagation(): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/stopImmediatePropagation)
   */
  stopImmediatePropagation(): void;

  /**
   * The dispatch path. For a pointer event this is `[surface, window]`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/composedPath)
   */
  composedPath(): EventTarget[];

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Event/initEvent)
   */
  initEvent(type: string, bubbles?: boolean, cancelable?: boolean): void;
}
declare var Event: {
  prototype: Event;
  new(type: string, eventInitDict?: EventInit): Event;
  readonly NONE: 0;
  readonly CAPTURING_PHASE: 1;
  readonly AT_TARGET: 2;
  readonly BUBBLING_PHASE: 3;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CustomEvent/CustomEvent)
 */
interface CustomEventInit<T = any> extends EventInit {
  detail?: T;
}

/**
 * An event with a `detail` value.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CustomEvent)
 */
interface CustomEvent<T = any> extends Event {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CustomEvent/detail) */
  readonly detail: T;
}
declare var CustomEvent: {
  prototype: CustomEvent;
  new<T>(type: string, eventInitDict?: CustomEventInit<T>): CustomEvent<T>;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ErrorEvent/ErrorEvent)
 */
interface ErrorEventInit extends EventInit {
  message?: string;
  filename?: string;
  lineno?: number;
  colno?: number;
  error?: any;
}

/**
 * An uncaught exception reported on `globalThis` or a worker.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ErrorEvent)
 */
interface ErrorEvent extends Event {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ErrorEvent/message) */
  readonly message: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ErrorEvent/filename) */
  readonly filename: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ErrorEvent/lineno) */
  readonly lineno: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ErrorEvent/colno) */
  readonly colno: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ErrorEvent/error) */
  readonly error: any;
}
declare var ErrorEvent: {
  prototype: ErrorEvent;
  new(type: string, eventInitDict?: ErrorEventInit): ErrorEvent;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/PromiseRejectionEvent/PromiseRejectionEvent)
 */
interface PromiseRejectionEventInit extends EventInit {
  promise: Promise<any>;
  reason?: any;
}

/**
 * An unhandled or later-handled rejection.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/PromiseRejectionEvent)
 */
interface PromiseRejectionEvent extends Event {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PromiseRejectionEvent/promise) */
  readonly promise: Promise<any>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PromiseRejectionEvent/reason) */
  readonly reason: any;
}
declare var PromiseRejectionEvent: {
  prototype: PromiseRejectionEvent;
  new(type: string, eventInitDict: PromiseRejectionEventInit): PromiseRejectionEvent;
};


/**
 * A DOM exception.
 *
 * `name` selects the legacy `code`. Unknown names use code 0. The prototype
 * inherits from `Error`. An omitted `message` is `""`. An omitted `name` is
 * `"Error"`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMException)
 */
declare class DOMException extends Error {
  constructor(message?: string, name?: string);
  static readonly INDEX_SIZE_ERR: 1;
  static readonly DOMSTRING_SIZE_ERR: 2;
  static readonly HIERARCHY_REQUEST_ERR: 3;
  static readonly WRONG_DOCUMENT_ERR: 4;
  static readonly INVALID_CHARACTER_ERR: 5;
  static readonly NO_DATA_ALLOWED_ERR: 6;
  static readonly NO_MODIFICATION_ALLOWED_ERR: 7;
  static readonly NOT_FOUND_ERR: 8;
  static readonly NOT_SUPPORTED_ERR: 9;
  static readonly INUSE_ATTRIBUTE_ERR: 10;
  static readonly INVALID_STATE_ERR: 11;
  static readonly SYNTAX_ERR: 12;
  static readonly INVALID_MODIFICATION_ERR: 13;
  static readonly NAMESPACE_ERR: 14;
  static readonly INVALID_ACCESS_ERR: 15;
  static readonly VALIDATION_ERR: 16;
  static readonly TYPE_MISMATCH_ERR: 17;
  static readonly SECURITY_ERR: 18;
  static readonly NETWORK_ERR: 19;
  static readonly ABORT_ERR: 20;
  static readonly URL_MISMATCH_ERR: 21;
  static readonly QUOTA_EXCEEDED_ERR: 22;
  static readonly TIMEOUT_ERR: 23;
  static readonly INVALID_NODE_TYPE_ERR: 24;
  static readonly DATA_CLONE_ERR: 25;


  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMException/name) */
  readonly name: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMException/message) */
  readonly message: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMException/code) */
  readonly code: number;

  readonly INDEX_SIZE_ERR: 1;
  readonly DOMSTRING_SIZE_ERR: 2;
  readonly HIERARCHY_REQUEST_ERR: 3;
  readonly WRONG_DOCUMENT_ERR: 4;
  readonly INVALID_CHARACTER_ERR: 5;
  readonly NO_DATA_ALLOWED_ERR: 6;
  readonly NO_MODIFICATION_ALLOWED_ERR: 7;
  readonly NOT_FOUND_ERR: 8;
  readonly NOT_SUPPORTED_ERR: 9;
  readonly INUSE_ATTRIBUTE_ERR: 10;
  readonly INVALID_STATE_ERR: 11;
  readonly SYNTAX_ERR: 12;
  readonly INVALID_MODIFICATION_ERR: 13;
  readonly NAMESPACE_ERR: 14;
  readonly INVALID_ACCESS_ERR: 15;
  readonly VALIDATION_ERR: 16;
  readonly TYPE_MISMATCH_ERR: 17;
  readonly SECURITY_ERR: 18;
  readonly NETWORK_ERR: 19;
  readonly ABORT_ERR: 20;
  readonly URL_MISMATCH_ERR: 21;
  readonly QUOTA_EXCEEDED_ERR: 22;
  readonly TIMEOUT_ERR: 23;
  readonly INVALID_NODE_TYPE_ERR: 24;
  readonly DATA_CLONE_ERR: 25;
}

/**
 * A quota failure.
 *
 * `quota` and `requested` are null. `code` is 22.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/QuotaExceededError)
 */
declare class QuotaExceededError extends DOMException {
  /**
   * @param message - Stored as `message`. Omitted is `""`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/QuotaExceededError/QuotaExceededError)
   */
  constructor(message?: string);

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/QuotaExceededError/quota) */
  readonly quota: null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/QuotaExceededError/requested) */
  readonly requested: null;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/UIEvent/UIEvent)
 */
interface UIEventInit extends EventInit {
  view?: Window | null;
  detail?: number;
}

/**
 * Base of the input events.
 *
 * `view` is null. There is no browser window.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/UIEvent)
 */
interface UIEvent extends Event {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/UIEvent/view) */
  readonly view: Window | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/UIEvent/detail) */
  readonly detail: number;
}
declare var UIEvent: {
  prototype: UIEvent;
  new(type: string, eventInitDict?: UIEventInit): UIEvent;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FocusEvent/FocusEvent)
 */
interface FocusEventInit extends UIEventInit {
  relatedTarget?: EventTarget | null;
}

/**
 * Focus moving to or from a {@link Yuzuko.Window}.
 *
 * The events do not bubble or cancel, and `relatedTarget` from the host is null.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FocusEvent)
 */
interface FocusEvent extends UIEvent {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FocusEvent/relatedTarget) */
  readonly relatedTarget: EventTarget | null;
}
declare var FocusEvent: {
  prototype: FocusEvent;
  new(type: string, eventInitDict?: FocusEventInit): FocusEvent;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/MouseEvent)
 */
interface MouseEventInit extends UIEventInit {
  screenX?: number;
  screenY?: number;
  clientX?: number;
  clientY?: number;
  button?: number;
  buttons?: number;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
  metaKey?: boolean;
  relatedTarget?: EventTarget | null;
  movementX?: number;
  movementY?: number;
}

/**
 * A mouse event derived from a pointer event, or a constructed mouse event.
 *
 * Host pointer events target {@link Yuzuko.Window.surface} and propagate
 * through the window. `clientX`, `pageX`, and `offsetX` are CSS pixels from
 * that window's top-left.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent)
 */
interface MouseEvent extends UIEvent {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/screenX) */
  readonly screenX: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/screenY) */
  readonly screenY: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/clientX) */
  readonly clientX: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/clientY) */
  readonly clientY: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/pageX) */
  readonly pageX: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/pageY) */
  readonly pageY: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/offsetX) */
  readonly offsetX: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/offsetY) */
  readonly offsetY: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/movementX) */
  readonly movementX: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/movementY) */
  readonly movementY: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/button) */
  readonly button: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/buttons) */
  readonly buttons: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/ctrlKey) */
  readonly ctrlKey: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/shiftKey) */
  readonly shiftKey: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/altKey) */
  readonly altKey: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/metaKey) */
  readonly metaKey: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/relatedTarget) */
  readonly relatedTarget: EventTarget | null;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MouseEvent/getModifierState)
   */
  getModifierState(key: string): boolean;
}
declare var MouseEvent: {
  prototype: MouseEvent;
  new(type: string, eventInitDict?: MouseEventInit): MouseEvent;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WheelEvent/WheelEvent)
 */
interface WheelEventInit extends MouseEventInit {
  deltaX?: number;
  deltaY?: number;
  deltaZ?: number;
  deltaMode?: number;
}

/**
 * A wheel event dispatched on a {@link Yuzuko.Window}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WheelEvent)
 */
interface WheelEvent extends MouseEvent {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WheelEvent/deltaX) */
  readonly deltaX: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WheelEvent/deltaY) */
  readonly deltaY: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WheelEvent/deltaZ) */
  readonly deltaZ: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WheelEvent/deltaMode) */
  readonly deltaMode: number;

  readonly DOM_DELTA_PIXEL: 0;
  readonly DOM_DELTA_LINE: 1;
  readonly DOM_DELTA_PAGE: 2;
}
declare var WheelEvent: {
  prototype: WheelEvent;
  new(type: string, eventInitDict?: WheelEventInit): WheelEvent;
  readonly DOM_DELTA_PIXEL: 0x00;
  readonly DOM_DELTA_LINE: 0x01;
  readonly DOM_DELTA_PAGE: 0x02;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/KeyboardEvent)
 */
interface KeyboardEventInit extends UIEventInit {
  key?: string;
  code?: string;
  location?: number;
  repeat?: boolean;
  isComposing?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
  metaKey?: boolean;
}

/**
 * A key event dispatched on a {@link Yuzuko.Window}.
 *
 * `key` comes from the winit logical key. `code` maps the physical key to a
 * UI Events code. An unknown key is `"Unidentified"`. `isComposing` is true
 * while the preedit string is non-empty. `preventDefault()` on `keydown`
 * suppresses the following text input. A `Commit` the OS has already produced
 * is still delivered as `compositionend`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent)
 */
interface KeyboardEvent extends UIEvent {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/key) */
  readonly key: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/code) */
  readonly code: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/location) */
  readonly location: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/repeat) */
  readonly repeat: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/isComposing) */
  readonly isComposing: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/ctrlKey) */
  readonly ctrlKey: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/shiftKey) */
  readonly shiftKey: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/altKey) */
  readonly altKey: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/metaKey) */
  readonly metaKey: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/charCode) */
  readonly charCode: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/keyCode) */
  readonly keyCode: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/which) */
  readonly which: number;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/KeyboardEvent/getModifierState)
   */
  getModifierState(key: string): boolean;
}
declare var KeyboardEvent: {
  prototype: KeyboardEvent;
  new(type: string, eventInitDict?: KeyboardEventInit): KeyboardEvent;
  readonly DOM_KEY_LOCATION_STANDARD: 0x00;
  readonly DOM_KEY_LOCATION_LEFT: 0x01;
  readonly DOM_KEY_LOCATION_RIGHT: 0x02;
  readonly DOM_KEY_LOCATION_NUMPAD: 0x03;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CompositionEvent/CompositionEvent)
 */
interface CompositionEventInit extends UIEventInit {
  data?: string;
}

/**
 * An IME composition event on a {@link Yuzuko.Window}.
 *
 * IME does not run until {@link Yuzuko.Window.setImeAllowed} is true.
 * `compositionstart` has `data === ""`. `compositionupdate` carries the
 * preedit string. `compositionend` carries the committed string.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CompositionEvent)
 */
interface CompositionEvent extends UIEvent {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CompositionEvent/data) */
  readonly data: string;
}
declare var CompositionEvent: {
  prototype: CompositionEvent;
  new(type: string, eventInitDict?: CompositionEventInit): CompositionEvent;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/InputEvent/InputEvent)
 */
interface InputEventInit extends UIEventInit {
  data?: string | null;
  isComposing?: boolean;
  inputType?: string;
}

/**
 * A `beforeinput` event on a {@link Yuzuko.WindowSurface}.
 *
 * After `compositionend`, one event uses `inputType` `"insertText"` and the
 * committed string. Events during composition use `"insertCompositionText"`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/InputEvent)
 */
interface InputEvent extends UIEvent {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/InputEvent/data) */
  readonly data: string | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/InputEvent/isComposing) */
  readonly isComposing: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/InputEvent/inputType) */
  readonly inputType: string;
}
declare var InputEvent: {
  prototype: InputEvent;
  new(type: string, eventInitDict?: InputEventInit): InputEvent;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/PointerEvent)
 */
interface PointerEventInit extends MouseEventInit {
  pointerId?: number;
  width?: number;
  height?: number;
  pressure?: number;
  tangentialPressure?: number;
  tiltX?: number;
  tiltY?: number;
  twist?: number;
  pointerType?: string;
  isPrimary?: boolean;
}

/**
 * A pointer event.
 *
 * The host target is {@link Yuzuko.Window.surface}. The path is the surface,
 * then the window. `pointerId` is per window. `pointerrawupdate` is dispatched
 * for each pointer move, including while the pointer is locked, before
 * `pointermove`. It bubbles and is not cancelable. Missing OS values are 0.
 * A mouse uses `pressure` 0.5 while a button is down and 0 otherwise.
 * `movementX` and `movementY` are relative while the pointer is locked.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent)
 */
interface PointerEvent extends MouseEvent {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/pointerId) */
  readonly pointerId: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/width) */
  readonly width: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/height) */
  readonly height: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/pressure) */
  readonly pressure: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/tangentialPressure) */
  readonly tangentialPressure: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/tiltX) */
  readonly tiltX: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/tiltY) */
  readonly tiltY: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/twist) */
  readonly twist: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/azimuthAngle) */
  readonly azimuthAngle: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/altitudeAngle) */
  readonly altitudeAngle: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/pointerType) */
  readonly pointerType: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/isPrimary) */
  readonly isPrimary: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/persistentDeviceId) */
  readonly persistentDeviceId: number;

  /**
   * Moves coalesced into this `pointermove`, or the single move carried by
   * this `pointerrawupdate`. Other types return an empty list.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/getCoalescedEvents)
   */
  getCoalescedEvents(): PointerEvent[];

  /**
   * Predicted events. The host returns an empty list.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/PointerEvent/getPredictedEvents)
   */
  getPredictedEvents(): PointerEvent[];
}
declare var PointerEvent: {
  prototype: PointerEvent;
  new(type: string, eventInitDict?: PointerEventInit): PointerEvent;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AbortSignal/AbortSignal)
 */
interface AbortSignalEventMap {
  abort: Event;
}

/**
 * A signal that can abort an operation.
 *
 * An abort event dispatched by {@link AbortController.abort} or
 * {@link AbortSignal.abort} is not trusted. {@link AbortSignal.abort} returns
 * a signal that is already aborted. An omitted reason is an `AbortError`
 * DOMException. {@link AbortSignal.timeout} aborts after `milliseconds`.
 * {@link AbortSignal.any} aborts when any input aborts.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AbortSignal)
 */
interface AbortSignal extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AbortSignal/aborted) */
  readonly aborted: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AbortSignal/reason) */
  readonly reason: any;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AbortSignal/abort_event) */
  onabort: ((this: AbortSignal, ev: Event) => any) | null;

  /**
   * Throws `reason` when this signal has aborted.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AbortSignal/throwIfAborted)
   */
  throwIfAborted(): void;

  addEventListener<K extends keyof AbortSignalEventMap>(
    type: K,
    listener: (this: AbortSignal, ev: AbortSignalEventMap[K]) => any,
    options?: boolean | AddEventListenerOptions,
  ): void;
  addEventListener(
    type: string,
    listener: EventListenerOrEventListenerObject | null,
    options?: boolean | AddEventListenerOptions,
  ): void;
}
declare var AbortSignal: {
  prototype: AbortSignal;
  new(): AbortSignal;
  abort(reason?: any): AbortSignal;
  any(signals: AbortSignal[]): AbortSignal;
  timeout(milliseconds: number): AbortSignal;
};


/**
 * Creates an {@link AbortSignal} and aborts it.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AbortController)
 */
interface AbortController {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AbortController/signal) */
  readonly signal: AbortSignal;

  /**
   * Aborts {@link AbortController.signal}.
   *
   * @param reason - Stored as the signal's `reason`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AbortController/abort)
   */
  abort(reason?: any): void;
}
declare var AbortController: {
  prototype: AbortController;
  new(): AbortController;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessageEvent/MessageEvent)
 */
interface MessageEventInit<T = any> extends EventInit {
  data?: T;
  origin?: string;
  lastEventId?: string;
  ports?: MessagePort[];
}

/**
 * A message from a worker, a port, an event source, or a WebSocket.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessageEvent)
 */
interface MessageEvent<T = any> extends Event {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessageEvent/data) */
  readonly data: T;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessageEvent/origin) */
  readonly origin: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessageEvent/lastEventId) */
  readonly lastEventId: string;

  /**
   * Always `null`. There is no document and no service worker.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessageEvent/source)
   */
  readonly source: null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessageEvent/ports) */
  readonly ports: readonly MessagePort[];
}
declare var MessageEvent: {
  prototype: MessageEvent;
  new<T>(type: string, eventInitDict?: MessageEventInit<T>): MessageEvent<T>;
};


/**
 * One end of a message channel.
 *
 * `postMessage` carries the values {@link structuredClone} can carry.
 *
 * @throws {@link TypeError} Illegal constructor. Use {@link MessageChannel}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessagePort)
 */
interface MessagePort extends EventTarget {

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessagePort/postMessage)
   */
  postMessage(message: any, transfer: Transferable[]): void;
  postMessage(message: any, options?: StructuredSerializeOptions): void;

  /**
   * Starts delivery of queued messages.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessagePort/start)
   */
  start(): void;

  /**
   * Disentangles this port.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessagePort/close)
   */
  close(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessagePort/message_event) */
  onmessage: ((this: MessagePort, ev: MessageEvent) => any) | null;
}
declare var MessagePort: {
  prototype: MessagePort;
  new(): MessagePort;
};


/**
 * A pair of entangled {@link MessagePort} objects.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessageChannel)
 */
interface MessageChannel {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessageChannel/port1) */
  readonly port1: MessagePort;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MessageChannel/port2) */
  readonly port2: MessagePort;
}
declare var MessageChannel: {
  prototype: MessageChannel;
  new(): MessageChannel;
};


/**
 * A URL.
 *
 * `createObjectURL` accepts a {@link Blob}. `file:` URLs used by `fetch` must
 * stay under the entry module's directory.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/URL)
 */
interface URL {
  href: string;
  readonly origin: string;
  protocol: string;
  username: string;
  password: string;
  host: string;
  hostname: string;
  port: string;
  pathname: string;
  search: string;
  hash: string;
  readonly searchParams: URLSearchParams;
  toJSON(): string;
}
declare var URL: {
  prototype: URL;
  new(url: string | URL, base?: string | URL): URL;
  canParse(url: string | URL, base?: string | URL): boolean;
  createObjectURL(obj: Blob): string;
  parse(url: string | URL, base?: string | URL): URL | null;
  revokeObjectURL(url: string): void;
};


/**
 * Query parameters.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams)
 */
interface URLSearchParams {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/size) */
  readonly size: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/append) */
  append(name: string, value: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/delete) */
  delete(name: string, value?: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/get) */
  get(name: string): string | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/getAll) */
  getAll(name: string): string[];

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/has) */
  has(name: string, value?: string): boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/set) */
  set(name: string, value: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/sort) */
  sort(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/forEach) */
  forEach(
    callback: (value: string, key: string, parent: URLSearchParams) => void,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/entries) */
  entries(): IterableIterator<[string, string]>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/keys) */
  keys(): IterableIterator<string>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/values) */
  values(): IterableIterator<string>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLSearchParams/toString) */
  toString(): string;

  [Symbol.iterator](): IterableIterator<[string, string]>;
}
declare var URLSearchParams: {
  prototype: URLSearchParams;
  new(init?: string[][] | Record<string, string> | string | URLSearchParams): URLSearchParams;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/URLPattern)
 */
interface URLPatternInit {
  protocol?: string;
  username?: string;
  password?: string;
  hostname?: string;
  port?: string;
  pathname?: string;
  search?: string;
  hash?: string;
  baseURL?: string;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/URLPattern)
 */
interface URLPatternOptions {
  ignoreCase?: boolean;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/exec)
 */
interface URLPatternResult {
  inputs: (string | URLPatternInit)[];
  protocol: URLPatternComponentResult;
  username: URLPatternComponentResult;
  password: URLPatternComponentResult;
  hostname: URLPatternComponentResult;
  port: URLPatternComponentResult;
  pathname: URLPatternComponentResult;
  search: URLPatternComponentResult;
  hash: URLPatternComponentResult;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/exec)
 */
interface URLPatternComponentResult {
  input: string;
  groups: Record<string, string | undefined>;
}

/**
 * A pattern matched against URLs.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern)
 */
interface URLPattern {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/protocol) */
  readonly protocol: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/username) */
  readonly username: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/password) */
  readonly password: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/hostname) */
  readonly hostname: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/port) */
  readonly port: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/pathname) */
  readonly pathname: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/search) */
  readonly search: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/hash) */
  readonly hash: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/hasRegExpGroups) */
  readonly hasRegExpGroups: boolean;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/test)
   */
  test(input?: string | URLPatternInit, baseURL?: string): boolean;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/exec)
   */
  exec(
    input?: string | URLPatternInit,
    baseURL?: string,
  ): URLPatternResult | null;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/URLPattern/generate)
   */
  generate(init?: URLPatternInit): string;

}
declare var URLPattern: {
  prototype: URLPattern;
  new(input: URLPatternInput, baseURL: string | URL, options?: URLPatternOptions): URLPattern;
  new(input?: URLPatternInput, options?: URLPatternOptions): URLPattern;
};


/**
 * UTF-8 encoder.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextEncoder)
 */
interface TextEncoder {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextEncoder/encoding) */
  readonly encoding: "utf-8";

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextEncoder/encode)
   */
  encode(input?: string): Uint8Array<ArrayBuffer>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextEncoder/encodeInto)
   */
  encodeInto(
    source: string,
    destination: Uint8Array<ArrayBufferLike>,
  ): TextEncoderEncodeIntoResult;
}
declare var TextEncoder: {
  prototype: TextEncoder;
  new(): TextEncoder;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextEncoder/encodeInto)
 */
interface TextEncoderEncodeIntoResult {
  read: number;
  written: number;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoder/TextDecoder)
 */
interface TextDecoderOptions {
  fatal?: boolean;
  ignoreBOM?: boolean;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoder/decode)
 */
interface TextDecodeOptions {
  stream?: boolean;
}

/**
 * A text decoder. An omitted label is `"utf-8"`.
 *
 * @throws {@link RangeError} When `label` is unknown, or when `fatal` is
 * used with an encoding that cannot report errors.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoder)
 */
interface TextDecoder {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoder/encoding) */
  readonly encoding: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoder/fatal) */
  readonly fatal: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoder/ignoreBOM) */
  readonly ignoreBOM: boolean;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoder/decode)
   */
  decode(input?: BufferSource, options?: TextDecodeOptions): string;
}
declare var TextDecoder: {
  prototype: TextDecoder;
  new(label?: string, options?: TextDecoderOptions): TextDecoder;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Blob/Blob)
 */
interface BlobPropertyBag {
  type?: string;
  endings?: "transparent" | "native";
}

/**
 * A blob of bytes.
 *
 * The bytes stay shared until `text()`, `arrayBuffer()`, `bytes()`, or a
 * blob URL snapshot reads them.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Blob)
 */
interface Blob {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Blob/size) */
  readonly size: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Blob/type) */
  readonly type: string;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Blob/slice)
   */
  slice(start?: number, end?: number, contentType?: string): Blob;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Blob/text) */
  text(): Promise<string>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Blob/arrayBuffer) */
  arrayBuffer(): Promise<ArrayBuffer>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Blob/bytes) */
  bytes(): Promise<Uint8Array<ArrayBuffer>>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Blob/stream) */
  stream(): ReadableStream<Uint8Array<ArrayBuffer>>;

  /**
   * UTF-8 text. The blob type's charset is ignored.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Blob/textStream)
   */
  textStream(): ReadableStream<string>;
}
declare var Blob: {
  prototype: Blob;
  new(blobParts?: BlobPart[], options?: BlobPropertyBag): Blob;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/File/File)
 */
interface FilePropertyBag extends BlobPropertyBag {
  lastModified?: number;
}

/**
 * A named blob.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/File)
 */
interface File extends Blob {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/File/name) */
  readonly name: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/File/lastModified) */
  readonly lastModified: number;
}
declare var File: {
  prototype: File;
  new(fileBits: BlobPart[], fileName: string, options?: FilePropertyBag): File;
};


/**
 * Progress of an underlying read.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ProgressEvent)
 */
interface ProgressEvent<T extends EventTarget = EventTarget> extends Event {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ProgressEvent/lengthComputable) */
  readonly lengthComputable: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ProgressEvent/loaded) */
  readonly loaded: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ProgressEvent/target) */
  readonly target: T | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ProgressEvent/total) */
  readonly total: number;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ProgressEvent/ProgressEvent) */
interface ProgressEventInit extends EventInit {
  lengthComputable?: boolean;
  loaded?: number;
  total?: number;
}
declare var ProgressEvent: {
  prototype: ProgressEvent;
  new(type: string, eventInitDict?: ProgressEventInit): ProgressEvent;
};

/**
 * Reads a blob asynchronously.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader)
 */
interface FileReader extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/readyState) */
  readonly readyState: 0 | 2 | 1;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/result) */
  readonly result: string | ArrayBuffer | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/error) */
  readonly error: DOMException | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/loadstart_event) */
  onloadstart: ((this: FileReader, ev: ProgressEvent<FileReader>) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/progress_event) */
  onprogress: ((this: FileReader, ev: ProgressEvent<FileReader>) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/load_event) */
  onload: ((this: FileReader, ev: ProgressEvent<FileReader>) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/abort_event) */
  onabort: ((this: FileReader, ev: ProgressEvent<FileReader>) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/error_event) */
  onerror: ((this: FileReader, ev: ProgressEvent<FileReader>) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/loadend_event) */
  onloadend: ((this: FileReader, ev: ProgressEvent<FileReader>) => any) | null;

  readonly EMPTY: 0;
  readonly LOADING: 1;
  readonly DONE: 2;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/readAsArrayBuffer) */
  readAsArrayBuffer(blob: Blob): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/readAsBinaryString) */
  readAsBinaryString(blob: Blob): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/readAsDataURL) */
  readAsDataURL(blob: Blob): void;

  /**
   * @param encoding - A label passed to {@link TextDecoder}.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/readAsText)
   */
  readAsText(blob: Blob, encoding?: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/abort) */
  abort(): void;
}
declare var FileReader: {
  prototype: FileReader;
  new(): FileReader;
  readonly EMPTY: 0;
  readonly LOADING: 1;
  readonly DONE: 2;
};


/**
 * A header list.
 *
 * Forbidden request and response headers are not enforced.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Headers)
 */
interface Headers {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Headers/get) */
  get(name: string): string | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Headers/has) */
  has(name: string): boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Headers/set) */
  set(name: string, value: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Headers/append) */
  append(name: string, value: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Headers/delete) */
  delete(name: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Headers/getSetCookie) */
  getSetCookie(): string[];

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Headers/forEach) */
  forEach(callback: (value: string, key: string, parent: Headers) => void): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Headers/entries) */
  entries(): IterableIterator<[string, string]>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Headers/keys) */
  keys(): IterableIterator<string>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Headers/values) */
  values(): IterableIterator<string>;

  [Symbol.iterator](): IterableIterator<[string, string]>;
}
declare var Headers: {
  prototype: Headers;
  new(init?: HeadersInit): Headers;
};


/**
 * Fields read by the {@link Request} constructor.
 *
 * `mode`, `credentials`, `referrer`, `referrerPolicy`, `destination`, and
 * `keepalive` are not read and have no getters. `cache` defaults to
 * `"no-store"`. `redirect: "manual"` returns the redirect response.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/Request)
 */
interface RequestInit {
  method?: string;
  headers?: HeadersInit;
  body?: BodyInit | null;
  redirect?: RequestRedirect;
  cache?: RequestCache;
  integrity?: string;
  signal?: AbortSignal | null;
  duplex?: "half";
  priority?: RequestPriority;
  /** Must be `null` when present. */
  window?: null;
}

/**
 * An HTTP request.
 *
 * Getters for `mode`, `credentials`, `referrer`, `referrerPolicy`,
 * `destination`, and `keepalive` are absent. `duplex` reads back as `"half"`.
 * `input` is a URL or another request. `init` overrides fields of `input`.
 *
 * @throws {@link DOMException} `NotSupportedError` when `integrity` is a
 * non-empty string. Subresource Integrity is not supported.
 * @throws {@link TypeError} When `window` is present and not `null`, or when
 * `duplex` is not `"half"`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request)
 */
interface Request {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/method) */
  readonly method: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/url) */
  readonly url: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/redirect) */
  readonly redirect: RequestRedirect;

  /**
   * Defaults to `"no-store"`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/cache)
   */
  readonly cache: RequestCache;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/duplex) */
  readonly duplex: "half";

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/headers) */
  readonly headers: Headers;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/signal) */
  readonly signal: AbortSignal;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/bodyUsed) */
  readonly bodyUsed: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/body) */
  readonly body: ReadableStream<Uint8Array<ArrayBuffer>> | null;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/clone)
   */
  clone(): Request;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/text) */
  text(): Promise<string>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/arrayBuffer) */
  arrayBuffer(): Promise<ArrayBuffer>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/bytes) */
  bytes(): Promise<Uint8Array<ArrayBuffer>>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/blob) */
  blob(): Promise<Blob>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/json) */
  json(): Promise<any>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/formData) */
  formData(): Promise<FormData>;

  /**
   * A stream of text decoded from the body.
   *
   * @throws {@link TypeError} When the body has already been used.
   */
  textStream(): ReadableStream<string>;
}
declare var Request: {
  prototype: Request;
  new(input: RequestInfo | URL, init?: RequestInit): Request;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/Response)
 */
interface ResponseInit {
  status?: number;
  statusText?: string;
  headers?: HeadersInit;
}

/**
 * An HTTP response.
 *
 * `type` for `fetch` and `new Response()` is `"default"`. There is no CORS
 * enforcement, so `Response.type` is not `"cors"` or `"opaque"`.
 *
 * @throws {@link RangeError} When {@link Response.redirect} is called with a
 * `status` other than 301, 302, 303, 307, or 308.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response)
 */
interface Response {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/status) */
  readonly status: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/statusText) */
  readonly statusText: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/ok) */
  readonly ok: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/url) */
  readonly url: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/redirected) */
  readonly redirected: boolean;

  /**
   * `"default"` for a normal response, or `"error"` from {@link Response.error}.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/type)
   */
  readonly type: ResponseType;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/headers) */
  readonly headers: Headers;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/bodyUsed) */
  readonly bodyUsed: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/body) */
  readonly body: ReadableStream<Uint8Array<ArrayBuffer>> | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/clone) */
  clone(): Response;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/text) */
  text(): Promise<string>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/arrayBuffer) */
  arrayBuffer(): Promise<ArrayBuffer>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/bytes) */
  bytes(): Promise<Uint8Array<ArrayBuffer>>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/blob) */
  blob(): Promise<Blob>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/json) */
  json(): Promise<any>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/formData) */
  formData(): Promise<FormData>;

  /**
   * A stream of text decoded from the body.
   *
   * @throws {@link TypeError} When the body has already been used.
   */
  textStream(): ReadableStream<string>;

}
declare var Response: {
  prototype: Response;
  new(body?: BodyInit | null, init?: ResponseInit): Response;
  error(): Response;
  json(data: any, init?: ResponseInit): Response;
  redirect(url: string | URL, status?: number): Response;
};


/**
 * Fetches `input`.
 *
 * Present when the runtime is built with the `fetch` Cargo feature, which is
 * enabled by default. A build that does not enable `fetch` omits this
 * function, {@link Headers}, {@link Request}, {@link Response}, `caches`,
 * {@link XMLHttpRequest}, and {@link EventSource}. The `socket` feature,
 * which installs {@link WebSocket}, is separate.
 *
 * Schemes are `file:`, `blob:`, `data:`, `http:`, and `https:`. `http:` and
 * `https:` follow redirects, retry a `421` once on a new connection, and
 * cancel with `AbortSignal`. A blob URL `Range` answers `206` or a network
 * error. A `file:` URL must stay under the entry directory and is not read
 * until the body is pulled. CORS, forbidden headers, and forbidden methods
 * are not enforced.
 *
 * @throws {@link DOMException} `NotSupportedError` when the URL scheme is not
 * one of those five, or when `init.integrity` is a non-empty string.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/fetch)
 */
declare function fetch(
  input: RequestInfo | URL,
  init?: RequestInit,
): Promise<Response>;

/**
 * Form fields.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FormData)
 */
interface FormData {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FormData/append) */
  append(name: string, value: string | Blob, fileName?: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FormData/delete) */
  delete(name: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FormData/get) */
  get(name: string): FormDataEntryValue | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FormData/getAll) */
  getAll(name: string): FormDataEntryValue[];

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FormData/has) */
  has(name: string): boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FormData/set) */
  set(name: string, value: string | Blob, fileName?: string): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FormData/forEach) */
  forEach(
    callback: (
      value: FormDataEntryValue,
      key: string,
      parent: FormData,
    ) => void,
  ): void;

  entries(): IterableIterator<[string, FormDataEntryValue]>;
  keys(): IterableIterator<string>;
  values(): IterableIterator<FormDataEntryValue>;
  [Symbol.iterator](): IterableIterator<[string, FormDataEntryValue]>;
}
declare var FormData: {
  prototype: FormData;
  new(): FormData;
};


/**
 * A named cache.
 *
 * `add` and `match` are absent.
 *
 * @throws {@link TypeError} Illegal constructor. Use {@link caches}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Cache)
 */
interface Cache {

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Cache/put)
   */
  put(request: RequestInfo | URL, response: Response): Promise<void>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Cache/keys)
   */
  keys(): Promise<readonly Request[]>;
}
declare var Cache: {
  prototype: Cache;
  new(): Cache;
};


/**
 * The process cache storage.
 *
 * `has`, `keys`, and `match` are absent.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CacheStorage)
 */
interface CacheStorage {

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CacheStorage/open)
   */
  open(cacheName: string): Promise<Cache>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CacheStorage/delete)
   */
  delete(cacheName: string): Promise<boolean>;
}
declare var CacheStorage: {
  prototype: CacheStorage;
  new(): CacheStorage;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/caches)
 */
declare var caches: CacheStorage;

/**
 * A request helper.
 *
 * `send` loads `http:`, `https:`, and `data:`, collects the response, then
 * queues `onload` or `onerror`. The handler receives a trusted
 * {@link ProgressEvent}. This object is not an {@link EventTarget}, so the
 * event is not dispatched: `target` is the request, and `currentTarget` is
 * the request only while the handler runs. `responseText` uses the
 * `Content-Type` charset, unless `overrideMimeType` set one. `responseType`,
 * upload progress, and event handlers other than `onload` and `onerror` are
 * absent.
 */
interface XMLHttpRequest {

  /**
   * Stores the method and URL.
   *
   * @throws {@link DOMException} `SyntaxError` when the method is invalid.
   */
  open(method: string, url: string): void;

  /**
   * Appends a request header.
   *
   * @throws {@link DOMException} `InvalidStateError` when `open` has not been
   * called or `send` has already been called.
   * @throws {@link DOMException} `SyntaxError` when the name or value is invalid.
   */
  setRequestHeader(name: string, value: string): void;

  /**
   * Performs the request.
   *
   * @throws {@link DOMException} `InvalidStateError` when the request is not open.
   * @throws {@link TypeError} When the URL cannot be resolved.
   */
  send(): void;

  /** The response header, or `null` before the request is done. */
  getResponseHeader(name: string): string | null;

  /** All response headers except `Set-Cookie`, or `""` before the request is done. */
  getAllResponseHeaders(): string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/XMLHttpRequest/status) */
  readonly status: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/XMLHttpRequest/statusText) */
  readonly statusText: string;

  /**
   * The decoded response, or `""` before the request is done.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/XMLHttpRequest/responseText)
   */
  readonly responseText: string;

  /**
   * Replaces the charset used by `responseText`.
   *
   * @throws {@link DOMException} `InvalidStateError` when the response is done.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/XMLHttpRequest/overrideMimeType)
   */
  overrideMimeType(mime: string): void;

  onload: ((this: XMLHttpRequest, ev: ProgressEvent) => any) | null;
  onerror: ((this: XMLHttpRequest, ev: ProgressEvent) => any) | null;
}
declare var XMLHttpRequest: {
  prototype: XMLHttpRequest;
  new(): XMLHttpRequest;
  readonly UNSENT: 0;
  readonly OPENED: 1;
  readonly HEADERS_RECEIVED: 2;
  readonly LOADING: 3;
  readonly DONE: 4;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/ReadableStream)
 */
interface UnderlyingSource<R = any> {
  start?: UnderlyingSourceStartCallback<R>;
  pull?: UnderlyingSourcePullCallback<R>;
  cancel?: UnderlyingSourceCancelCallback;
  type?: ReadableStreamType;
  autoAllocateChunkSize?: number;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/ReadableStream)
 */
interface UnderlyingByteSource {
  start?: (controller: ReadableByteStreamController) => any;
  pull?: (controller: ReadableByteStreamController) => void | PromiseLike<void>;
  cancel?: UnderlyingSourceCancelCallback;
  type: "bytes";
  autoAllocateChunkSize?: number;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/ReadableStream)
 */
interface QueuingStrategy<T = any> {
  highWaterMark?: number;
  size?: QueuingStrategySize<T>;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/getReader)
 */
interface ReadableStreamGetReaderOptions {
  mode?: "byob";
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamBYOBReader/read)
 */
interface ReadableStreamBYOBReaderReadOptions {
  min?: number;
}

/**
 * A readable stream. {@link ReadableStream.from} creates a stream from an
 * iterable.
 *
 * A transfer locks this stream. The destination is a new
 * {@link ReadableStream}. A locked stream, or a stream left out of the
 * transfer list, throws `DataCloneError`. A transferred byte stream arrives
 * as a default stream.
 *
 * @throws {@link TypeError} When the argument of {@link ReadableStream.from}
 * is not iterable.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream)
 */
interface ReadableStream<R = any> {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/locked) */
  readonly locked: boolean;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/cancel)
   */
  cancel(reason?: any): Promise<void>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/getReader)
   */
  getReader(options: { mode: "byob" }): ReadableStreamBYOBReader;
  getReader(options?: { mode?: undefined }): ReadableStreamDefaultReader<R>;
  getReader(
    options?: ReadableStreamGetReaderOptions,
  ): ReadableStreamDefaultReader<R> | ReadableStreamBYOBReader;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/pipeThrough)
   */
  pipeThrough<T>(
    transform: { readable: ReadableStream<T>; writable: WritableStream<R> },
    options?: StreamPipeOptions,
  ): ReadableStream<T>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/pipeTo)
   */
  pipeTo(destination: WritableStream<R>, options?: StreamPipeOptions): Promise<void>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/tee)
   */
  tee(): [ReadableStream<R>, ReadableStream<R>];

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/values)
   */
  values(options?: { preventCancel?: boolean }): AsyncIterableIterator<R>;

  [Symbol.asyncIterator](options?: {
    preventCancel?: boolean;
  }): AsyncIterableIterator<R>;

}
declare var ReadableStream: {
  prototype: ReadableStream;
  new(underlyingSource: UnderlyingByteSource, strategy?: { highWaterMark?: number }): ReadableStream<Uint8Array<ArrayBuffer>>;
  new<R = any>(underlyingSource: UnderlyingDefaultSource<R>, strategy?: QueuingStrategy<R>): ReadableStream<R>;
  new<R = any>(underlyingSource?: UnderlyingSource<R>, strategy?: QueuingStrategy<R>): ReadableStream<R>;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStream/pipeTo)
 */
interface StreamPipeOptions {
  preventClose?: boolean;
  preventAbort?: boolean;
  preventCancel?: boolean;
  signal?: AbortSignal;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamDefaultReader)
 */
interface ReadableStreamDefaultReader<R = any> {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamDefaultReader/closed) */
  readonly closed: Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamDefaultReader/read) */
  read(): Promise<ReadableStreamReadResult<R>>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamDefaultReader/cancel) */
  cancel(reason?: any): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamDefaultReader/releaseLock) */
  releaseLock(): void;
}
declare var ReadableStreamDefaultReader: {
  prototype: ReadableStreamDefaultReader;
  new<R = any>(stream: ReadableStream<R>): ReadableStreamDefaultReader<R>;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamDefaultController)
 */
interface ReadableStreamDefaultController<R = any> {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamDefaultController/desiredSize) */
  readonly desiredSize: number | null;

  /**
   * Enqueues `chunk`. On an owning stream the second argument is
   * `{ transfer }`, and that list moves an `ArrayBuffer`, a `MessagePort`,
   * an `ImageBitmap`, an `AudioData`, or a `VideoFrame`.
   *
   * @throws {@link DOMException} `DataCloneError` when a listed value cannot
   * be transferred.
   * @throws {@link TypeError} when a non-owning stream is given a transfer list.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamDefaultController/enqueue)
   */
  enqueue(chunk?: R): void;
  enqueue(chunk: R, options: StructuredSerializeOptions): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamDefaultController/close) */
  close(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamDefaultController/error) */
  error(e?: any): void;
}
declare var ReadableStreamDefaultController: {
  prototype: ReadableStreamDefaultController;
  new(): ReadableStreamDefaultController;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamBYOBReader)
 */
interface ReadableStreamBYOBReader {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamBYOBReader/closed) */
  readonly closed: Promise<void>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamBYOBReader/read)
   */
  read<T extends ArrayBufferView>(
    view: T,
    options?: ReadableStreamBYOBReaderReadOptions,
  ): Promise<ReadableStreamReadResult<T>>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamBYOBReader/cancel) */
  cancel(reason?: any): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamBYOBReader/releaseLock) */
  releaseLock(): void;
}
declare var ReadableStreamBYOBReader: {
  prototype: ReadableStreamBYOBReader;
  new(stream: ReadableStream<Uint8Array<ArrayBuffer>>): ReadableStreamBYOBReader;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableByteStreamController)
 */
interface ReadableByteStreamController {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableByteStreamController/desiredSize) */
  readonly desiredSize: number | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableByteStreamController/byobRequest) */
  readonly byobRequest: ReadableStreamBYOBRequest | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableByteStreamController/enqueue) */
  enqueue(chunk: ArrayBufferView): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableByteStreamController/close) */
  close(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableByteStreamController/error) */
  error(e?: any): void;
}
declare var ReadableByteStreamController: {
  prototype: ReadableByteStreamController;
  new(): ReadableByteStreamController;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamBYOBRequest)
 */
interface ReadableStreamBYOBRequest {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamBYOBRequest/view) */
  readonly view: ArrayBufferView<ArrayBuffer> | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamBYOBRequest/respond) */
  respond(bytesWritten: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamBYOBRequest/respondWithNewView) */
  respondWithNewView(view: ArrayBufferView): void;
}
declare var ReadableStreamBYOBRequest: {
  prototype: ReadableStreamBYOBRequest;
  new(): ReadableStreamBYOBRequest;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStream/WritableStream)
 */
interface UnderlyingSink<W = any> {
  start?: UnderlyingSinkStartCallback;
  write?: (
    chunk: W,
    controller: WritableStreamDefaultController,
  ) => void | PromiseLike<void>;
  close?: () => void | PromiseLike<void>;
  abort?: (reason?: any) => void | PromiseLike<void>;
}

/**
 * A transfer locks this stream. The destination is a new
 * {@link WritableStream}. A locked stream, or a stream left out of the
 * transfer list, throws `DataCloneError`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStream)
 */
interface WritableStream<W = any> {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStream/locked) */
  readonly locked: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStream/abort) */
  abort(reason?: any): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStream/close) */
  close(): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStream/getWriter) */
  getWriter(): WritableStreamDefaultWriter<W>;
}
declare var WritableStream: {
  prototype: WritableStream;
  new<W = any>(underlyingSink?: UnderlyingSink<W>, strategy?: QueuingStrategy<W>): WritableStream<W>;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStreamDefaultWriter)
 */
interface WritableStreamDefaultWriter<W = any> {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStreamDefaultWriter/closed) */
  readonly closed: Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStreamDefaultWriter/ready) */
  readonly ready: Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStreamDefaultWriter/desiredSize) */
  readonly desiredSize: number | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStreamDefaultWriter/write) */
  write(chunk?: W): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStreamDefaultWriter/close) */
  close(): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStreamDefaultWriter/abort) */
  abort(reason?: any): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStreamDefaultWriter/releaseLock) */
  releaseLock(): void;
}
declare var WritableStreamDefaultWriter: {
  prototype: WritableStreamDefaultWriter;
  new<W = any>(stream: WritableStream<W>): WritableStreamDefaultWriter<W>;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStreamDefaultController)
 */
interface WritableStreamDefaultController {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStreamDefaultController/signal) */
  readonly signal: AbortSignal;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WritableStreamDefaultController/error) */
  error(e?: any): void;
}
declare var WritableStreamDefaultController: {
  prototype: WritableStreamDefaultController;
  new(): WritableStreamDefaultController;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TransformStream/TransformStream)
 */
interface Transformer<I = any, O = any> {
  start?: TransformerStartCallback<O>;
  transform?: (
    chunk: I,
    controller: TransformStreamDefaultController<O>,
  ) => void | PromiseLike<void>;
  flush?: (controller: TransformStreamDefaultController<O>) => void | PromiseLike<void>;
}

/**
 * A transfer locks both sides. The destination is a new
 * {@link TransformStream}. A locked side, or a stream left out of the
 * transfer list, throws `DataCloneError`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TransformStream)
 */
interface TransformStream<I = any, O = any> {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TransformStream/readable) */
  readonly readable: ReadableStream<O>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TransformStream/writable) */
  readonly writable: WritableStream<I>;
}
declare var TransformStream: {
  prototype: TransformStream;
  new<I = any, O = any>(transformer?: Transformer<I, O>, writableStrategy?: QueuingStrategy<I>, readableStrategy?: QueuingStrategy<O>): TransformStream<I, O>;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TransformStreamDefaultController)
 */
interface TransformStreamDefaultController<O = any> {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TransformStreamDefaultController/desiredSize) */
  readonly desiredSize: number | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TransformStreamDefaultController/enqueue) */
  enqueue(chunk?: O): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TransformStreamDefaultController/error) */
  error(reason?: any): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TransformStreamDefaultController/terminate) */
  terminate(): void;
}
declare var TransformStreamDefaultController: {
  prototype: TransformStreamDefaultController;
  new(): TransformStreamDefaultController;
};


/**
 * A strategy that counts chunks.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CountQueuingStrategy)
 */
interface CountQueuingStrategy {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CountQueuingStrategy/highWaterMark) */
  readonly highWaterMark: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CountQueuingStrategy/size) */
  readonly size: QueuingStrategySize;
}
declare var CountQueuingStrategy: {
  prototype: CountQueuingStrategy;
  new(init: QueuingStrategyInit): CountQueuingStrategy;
};


/**
 * A strategy that uses `byteLength`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ByteLengthQueuingStrategy)
 */
interface ByteLengthQueuingStrategy {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ByteLengthQueuingStrategy/highWaterMark) */
  readonly highWaterMark: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ByteLengthQueuingStrategy/size) */
  readonly size: QueuingStrategySize<ArrayBufferView>;
}
declare var ByteLengthQueuingStrategy: {
  prototype: ByteLengthQueuingStrategy;
  new(init: QueuingStrategyInit): ByteLengthQueuingStrategy;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextEncoderStream)
 */
interface TextEncoderStream {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextEncoderStream/encoding) */
  readonly encoding: "utf-8";

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextEncoderStream/readable) */
  readonly readable: ReadableStream<Uint8Array<ArrayBuffer>>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextEncoderStream/writable) */
  readonly writable: WritableStream<string>;
}
declare var TextEncoderStream: {
  prototype: TextEncoderStream;
  new(): TextEncoderStream;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoderStream)
 */
interface TextDecoderStream {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoderStream/encoding) */
  readonly encoding: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoderStream/fatal) */
  readonly fatal: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoderStream/ignoreBOM) */
  readonly ignoreBOM: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoderStream/readable) */
  readonly readable: ReadableStream<string>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextDecoderStream/writable) */
  readonly writable: WritableStream<BufferSource>;
}
declare var TextDecoderStream: {
  prototype: TextDecoderStream;
  new(label?: string, options?: TextDecoderOptions): TextDecoderStream;
};


/**
 * Compresses bytes.
 *
 * @throws {@link TypeError} When `format` is not `gzip`, `deflate`, or `deflate-raw`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CompressionStream)
 */
interface CompressionStream {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CompressionStream/readable) */
  readonly readable: ReadableStream<Uint8Array<ArrayBuffer>>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CompressionStream/writable) */
  readonly writable: WritableStream<BufferSource>;
}
declare var CompressionStream: {
  prototype: CompressionStream;
  new(format: CompressionFormat): CompressionStream;
};


/**
 * Decompresses bytes.
 *
 * @throws {@link TypeError} When `format` is not `gzip`, `deflate`, or `deflate-raw`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DecompressionStream)
 */
interface DecompressionStream {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DecompressionStream/readable) */
  readonly readable: ReadableStream<Uint8Array<ArrayBuffer>>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DecompressionStream/writable) */
  readonly writable: WritableStream<BufferSource>;
}
declare var DecompressionStream: {
  prototype: DecompressionStream;
  new(format: CompressionFormat): DecompressionStream;
};


/**
 * A cryptographic key.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CryptoKey)
 */
interface CryptoKey {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CryptoKey/type) */
  readonly type: "public" | "private" | "secret";

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CryptoKey/extractable) */
  readonly extractable: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CryptoKey/algorithm) */
  readonly algorithm: KeyAlgorithm;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CryptoKey/usages) */
  readonly usages: KeyUsage[];
}
declare var CryptoKey: {
  prototype: CryptoKey;
  new(): CryptoKey;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CryptoKey/algorithm) */
interface KeyAlgorithm {
  name: string;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/digest) */
interface Algorithm {
  name: string;
}

/**
 * SubtleCrypto.
 *
 * Each operation rejects `NotSupportedError` when the algorithm name, hash,
 * curve, or key format is not implemented. `generateKey` also rejects
 * `NotSupportedError` for HKDF and PBKDF2. An unrecognized hash is
 * `NotSupportedError` before key usages are checked.
 *
 * @throws {@link TypeError} Illegal constructor.
 * @throws {@link TypeError} When {@link SubtleCrypto.supports} is called with
 * fewer than two arguments.
 * @throws {@link DOMException} `NotSupportedError`
 * {@link SubtleCrypto.supports} is not implemented.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto)
 */
interface SubtleCrypto {

  /**
   * @throws {@link DOMException} `NotSupportedError` when the digest is not
   * SHA-1, SHA-256, SHA-384, or SHA-512.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/digest)
   */
  digest(algorithm: AlgorithmIdentifier, data: BufferSource): Promise<ArrayBuffer>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when the algorithm cannot
   * generate a key, including HKDF, PBKDF2, and the unsupported suites.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/generateKey)
   */
  generateKey(
    algorithm: AlgorithmIdentifier,
    extractable: boolean,
    keyUsages: Iterable<KeyUsage>,
  ): Promise<CryptoKey | CryptoKeyPair>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when the format or
   * algorithm is not implemented for that key.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/importKey)
   */
  importKey(
    format: KeyFormat,
    keyData: BufferSource | JsonWebKey,
    algorithm: AlgorithmIdentifier,
    extractable: boolean,
    keyUsages: Iterable<KeyUsage>,
  ): Promise<CryptoKey>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when the format is not
   * supported for that algorithm. That check happens before the extractable check.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/exportKey)
   */
  exportKey(format: "jwk", key: CryptoKey): Promise<JsonWebKey>;
  exportKey(format: Exclude<KeyFormat, "jwk">, key: CryptoKey): Promise<ArrayBuffer>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when the wrapping
   * algorithm is not AES-KW, AES-GCM, AES-CBC, or AES-CTR.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/wrapKey)
   */
  wrapKey(
    format: KeyFormat,
    key: CryptoKey,
    wrappingKey: CryptoKey,
    wrapAlgorithm: AlgorithmIdentifier,
  ): Promise<ArrayBuffer>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/unwrapKey)
   */
  unwrapKey(
    format: KeyFormat,
    wrappedKey: BufferSource,
    unwrappingKey: CryptoKey,
    unwrapAlgorithm: AlgorithmIdentifier,
    unwrappedKeyAlgorithm: AlgorithmIdentifier,
    extractable: boolean,
    keyUsages: Iterable<KeyUsage>,
  ): Promise<CryptoKey>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when the algorithm is not
   * HMAC, RSASSA-PKCS1-v1_5, RSA-PSS, ECDSA, Ed25519, or Ed448.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/sign)
   */
  sign(
    algorithm: AlgorithmIdentifier,
    key: CryptoKey,
    data: BufferSource,
  ): Promise<ArrayBuffer>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when the algorithm is not
   * one accepted by {@link SubtleCrypto.sign}.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/verify)
   */
  verify(
    algorithm: AlgorithmIdentifier,
    key: CryptoKey,
    signature: BufferSource,
    data: BufferSource,
  ): Promise<boolean>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when the algorithm is not
   * RSA-OAEP, AES-GCM, AES-CBC, or AES-CTR.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/encrypt)
   */
  encrypt(
    algorithm: AlgorithmIdentifier,
    key: CryptoKey,
    data: BufferSource,
  ): Promise<ArrayBuffer>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when the algorithm is not
   * one accepted by {@link SubtleCrypto.encrypt}.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/decrypt)
   */
  decrypt(
    algorithm: AlgorithmIdentifier,
    key: CryptoKey,
    data: BufferSource,
  ): Promise<ArrayBuffer>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when the algorithm is not
   * HKDF, PBKDF2, ECDH, X25519, or X448.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/deriveBits)
   */
  deriveBits(
    algorithm: AlgorithmIdentifier,
    baseKey: CryptoKey,
    length?: number | null,
  ): Promise<ArrayBuffer>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when the base algorithm
   * cannot derive or the derived algorithm is not HMAC or AES.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/SubtleCrypto/deriveKey)
   */
  deriveKey(
    algorithm: AlgorithmIdentifier,
    baseKey: CryptoKey,
    derivedKeyType: AlgorithmIdentifier,
    extractable: boolean,
    keyUsages: Iterable<KeyUsage>,
  ): Promise<CryptoKey>;

  /**
   * @throws {@link DOMException} `NotSupportedError` ML-KEM and the hybrid
   * KEMs are not implemented.
   */
  encapsulateKey(
    algorithm: AlgorithmIdentifier,
    encapsulationKey: CryptoKey,
    encapsulatedKeyAlgorithm: AlgorithmIdentifier,
    extractable: boolean,
    keyUsages: Iterable<KeyUsage>,
  ): Promise<never>;

  /**
   * @throws {@link DOMException} `NotSupportedError` The operation is not implemented.
   */
  encapsulateBits(
    algorithm: AlgorithmIdentifier,
    encapsulationKey: CryptoKey,
  ): Promise<never>;

  /**
   * @throws {@link DOMException} `NotSupportedError` The operation is not implemented.
   */
  decapsulateKey(
    algorithm: AlgorithmIdentifier,
    decapsulationKey: CryptoKey,
    ciphertext: BufferSource,
    decapsulatedKeyAlgorithm: AlgorithmIdentifier,
    extractable: boolean,
    keyUsages: Iterable<KeyUsage>,
  ): Promise<never>;

  /**
   * @throws {@link DOMException} `NotSupportedError` The operation is not implemented.
   */
  decapsulateBits(
    algorithm: AlgorithmIdentifier,
    decapsulationKey: CryptoKey,
    ciphertext: BufferSource,
  ): Promise<never>;

  /**
   * @throws {@link DOMException} `NotSupportedError` The operation is not implemented.
   */
  getPublicKey(key: CryptoKey, keyUsages: Iterable<KeyUsage>): Promise<never>;

}
declare var SubtleCrypto: {
  prototype: SubtleCrypto;
  new(): SubtleCrypto;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CryptoKeyPair) */
interface CryptoKeyPair {
  publicKey: CryptoKey;
  privateKey: CryptoKey;
}

/**
 * The realm's crypto object.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Crypto)
 */
interface Crypto {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Crypto/subtle) */
  readonly subtle: SubtleCrypto;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Crypto/getRandomValues)
   */
  getRandomValues<T extends ArrayBufferView>(array: T): T;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Crypto/randomUUID)
   */
  randomUUID(): `${string}-${string}-${string}-${string}-${string}`;
}
declare var Crypto: {
  prototype: Crypto;
  new(): Crypto;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/crypto)
 */
declare var crypto: Crypto;

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/DOMMatrix)
 */
interface DOMMatrix2DInit {
  a?: number;
  b?: number;
  c?: number;
  d?: number;
  e?: number;
  f?: number;
  m11?: number;
  m12?: number;
  m21?: number;
  m22?: number;
  m41?: number;
  m42?: number;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/DOMMatrix)
 */
interface DOMMatrixInit extends DOMMatrix2DInit {
  m13?: number;
  m14?: number;
  m23?: number;
  m24?: number;
  m31?: number;
  m32?: number;
  m33?: number;
  m34?: number;
  m43?: number;
  m44?: number;
  is2D?: boolean;
}

/**
 * A read-only matrix.
 *
 * On the main global, `String(matrix)` uses the CSS matrix form. In a
 * dedicated worker there is no stringifier, no CSS-string constructor, and
 * no `setMatrixValue` on {@link DOMMatrix}. `WebKitCSSMatrix` is absent.
 *
 * @throws {@link TypeError} In a worker, when the argument is a string.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly)
 */
interface DOMMatrixReadOnly {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/a) */
  readonly a: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/b) */
  readonly b: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/c) */
  readonly c: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/d) */
  readonly d: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/e) */
  readonly e: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/f) */
  readonly f: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m11) */
  readonly m11: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m12) */
  readonly m12: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m13) */
  readonly m13: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m14) */
  readonly m14: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m21) */
  readonly m21: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m22) */
  readonly m22: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m23) */
  readonly m23: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m24) */
  readonly m24: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m31) */
  readonly m31: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m32) */
  readonly m32: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m33) */
  readonly m33: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m34) */
  readonly m34: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m41) */
  readonly m41: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m42) */
  readonly m42: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m43) */
  readonly m43: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/m44) */
  readonly m44: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/isIdentity) */
  readonly isIdentity: boolean;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/is2D) */
  readonly is2D: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/translate) */
  translate(tx?: number, ty?: number, tz?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/scale) */
  scale(scaleX?: number, scaleY?: number, scaleZ?: number, originX?: number, originY?: number, originZ?: number): DOMMatrix;
  scaleNonUniform(scaleX?: number, scaleY?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/scale3d) */
  scale3d(scale?: number, originX?: number, originY?: number, originZ?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/rotate) */
  rotate(rotX?: number, rotY?: number, rotZ?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/rotateFromVector) */
  rotateFromVector(x?: number, y?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/rotateAxisAngle) */
  rotateAxisAngle(x?: number, y?: number, z?: number, angle?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/skewX) */
  skewX(sx?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/skewY) */
  skewY(sy?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/multiply) */
  multiply(other?: DOMMatrixInit): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/flipX) */
  flipX(): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/flipY) */
  flipY(): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/inverse) */
  inverse(): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/transformPoint) */
  transformPoint(point?: DOMPointInit): DOMPoint;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/toFloat32Array) */
  toFloat32Array(): Float32Array<ArrayBuffer>;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/toFloat64Array) */
  toFloat64Array(): Float64Array<ArrayBuffer>;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/toJSON) */
  toJSON(): any;
  /**
   * Absent in a dedicated worker.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/toString)
   */
  toString(): string;
}
declare var DOMMatrixReadOnly: {
  prototype: DOMMatrixReadOnly;
  new(init?: string | number[]): DOMMatrixReadOnly;
  fromFloat32Array(array32: Float32Array<ArrayBuffer>): DOMMatrixReadOnly;
  fromFloat64Array(array64: Float64Array<ArrayBuffer>): DOMMatrixReadOnly;
  fromMatrix(other?: DOMMatrixInit): DOMMatrixReadOnly;
};


/**
 * A mutable matrix.
 *
 * `setMatrixValue` is absent in a dedicated worker.
 *
 * @throws {@link TypeError} In a worker, when `init` is a string.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix)
 */
interface DOMMatrix extends DOMMatrixReadOnly {

  a: number;
  b: number;
  c: number;
  d: number;
  e: number;
  f: number;
  m11: number;
  m12: number;
  m13: number;
  m14: number;
  m21: number;
  m22: number;
  m23: number;
  m24: number;
  m31: number;
  m32: number;
  m33: number;
  m34: number;
  m41: number;
  m42: number;
  m43: number;
  m44: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/multiplySelf) */
  multiplySelf(other?: DOMMatrixInit): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/preMultiplySelf) */
  preMultiplySelf(other?: DOMMatrixInit): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/translateSelf) */
  translateSelf(tx?: number, ty?: number, tz?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/scaleSelf) */
  scaleSelf(scaleX?: number, scaleY?: number, scaleZ?: number, originX?: number, originY?: number, originZ?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/scale3dSelf) */
  scale3dSelf(scale?: number, originX?: number, originY?: number, originZ?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/rotateSelf) */
  rotateSelf(rotX?: number, rotY?: number, rotZ?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/rotateFromVectorSelf) */
  rotateFromVectorSelf(x?: number, y?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/rotateAxisAngleSelf) */
  rotateAxisAngleSelf(x?: number, y?: number, z?: number, angle?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/skewXSelf) */
  skewXSelf(sx?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/skewYSelf) */
  skewYSelf(sy?: number): DOMMatrix;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/invertSelf) */
  invertSelf(): DOMMatrix;

  /**
   * Absent in a dedicated worker.
   *
   * @throws {@link TypeError} In a worker this method is not installed.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrix/setMatrixValue)
   */
  setMatrixValue(transformList: string): DOMMatrix;
}
declare var DOMMatrix: {
  prototype: DOMMatrix;
  new(init?: string | number[]): DOMMatrix;
  fromFloat32Array(array32: Float32Array<ArrayBuffer>): DOMMatrix;
  fromFloat64Array(array64: Float64Array<ArrayBuffer>): DOMMatrix;
  fromMatrix(other?: DOMMatrixInit): DOMMatrix;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMPoint/DOMPoint) */
interface DOMPointInit {
  x?: number;
  y?: number;
  z?: number;
  w?: number;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMPointReadOnly)
 */
interface DOMPointReadOnly {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMPointReadOnly/x) */
  readonly x: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMPointReadOnly/y) */
  readonly y: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMPointReadOnly/z) */
  readonly z: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMPointReadOnly/w) */
  readonly w: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMPointReadOnly/matrixTransform) */
  matrixTransform(matrix?: DOMMatrixInit): DOMPoint;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMPointReadOnly/toJSON) */
  toJSON(): any;
}
declare var DOMPointReadOnly: {
  prototype: DOMPointReadOnly;
  new(x?: number, y?: number, z?: number, w?: number): DOMPointReadOnly;
  fromPoint(other?: DOMPointInit): DOMPointReadOnly;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMPoint)
 */
interface DOMPoint extends DOMPointReadOnly {

  x: number;
  y: number;
  z: number;
  w: number;
}
declare var DOMPoint: {
  prototype: DOMPoint;
  new(x?: number, y?: number, z?: number, w?: number): DOMPoint;
  fromPoint(other?: DOMPointInit): DOMPoint;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRect/DOMRect) */
interface DOMRectInit {
  x?: number;
  y?: number;
  width?: number;
  height?: number;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectReadOnly)
 */
interface DOMRectReadOnly {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectReadOnly/x) */
  readonly x: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectReadOnly/y) */
  readonly y: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectReadOnly/width) */
  readonly width: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectReadOnly/height) */
  readonly height: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectReadOnly/top) */
  readonly top: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectReadOnly/right) */
  readonly right: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectReadOnly/bottom) */
  readonly bottom: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectReadOnly/left) */
  readonly left: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectReadOnly/toJSON) */
  toJSON(): any;
}
declare var DOMRectReadOnly: {
  prototype: DOMRectReadOnly;
  new(x?: number, y?: number, width?: number, height?: number): DOMRectReadOnly;
  fromRect(other?: DOMRectInit): DOMRectReadOnly;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRect)
 */
interface DOMRect extends DOMRectReadOnly {

  x: number;
  y: number;
  width: number;
  height: number;
}
declare var DOMRect: {
  prototype: DOMRect;
  new(x?: number, y?: number, width?: number, height?: number): DOMRect;
  fromRect(other?: DOMRectInit): DOMRect;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMQuad)
 */
interface DOMQuad {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMQuad/p1) */
  readonly p1: DOMPoint;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMQuad/p2) */
  readonly p2: DOMPoint;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMQuad/p3) */
  readonly p3: DOMPoint;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMQuad/p4) */
  readonly p4: DOMPoint;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMQuad/getBounds) */
  getBounds(): DOMRect;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMQuad/toJSON) */
  toJSON(): any;
}
declare var DOMQuad: {
  prototype: DOMQuad;
  new(p1?: DOMPointInit, p2?: DOMPointInit, p3?: DOMPointInit, p4?: DOMPointInit): DOMQuad;
  fromQuad(other?: DOMQuadInit): DOMQuad;
  fromRect(other?: DOMRectInit): DOMQuad;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMQuad/DOMQuad) */
interface DOMQuadInit {
  p1?: DOMPointInit;
  p2?: DOMPointInit;
  p3?: DOMPointInit;
  p4?: DOMPointInit;
}

/**
 * A list of rectangles.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectList)
 */
interface DOMRectList {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectList/length) */
  readonly length: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMRectList/item) */
  item(index: number): DOMRect | null;
}
declare var DOMRectList: {
  prototype: DOMRectList;
  new(): DOMRectList;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/FontFace)
 */
interface FontFaceDescriptors {
  style?: string;
  weight?: string;
  stretch?: string;
  unicodeRange?: string;
  featureSettings?: string;
  display?: FontDisplay;
  ascentOverride?: string;
  descentOverride?: string;
  lineGapOverride?: string;
  sizeAdjust?: string;
  variationSettings?: string;
}

/**
 * A font face.
 *
 * Sources are an `ArrayBuffer` or a URL that `fetch` can load. Formats are
 * TTF, OTF, WOFF, and WOFF2. A WOFF2 `glyf` or `loca` preprocessing transform
 * is rejected. CSS `@font-face` is not implemented. `style` and `weight` are
 * read-only after construction. `stretch` and `width` can be assigned.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace)
 */
interface FontFace {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/family) */
  family: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/style) */
  style: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/weight) */
  weight: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/stretch) */
  stretch: string;
  /**
   * Alias of {@link FontFace.stretch}.
   */
  width: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/unicodeRange) */
  unicodeRange: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/featureSettings) */
  featureSettings: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/display) */
  display: FontDisplay;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/ascentOverride) */
  ascentOverride: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/descentOverride) */
  descentOverride: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/lineGapOverride) */
  lineGapOverride: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/sizeAdjust) */
  readonly sizeAdjust: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/variationSettings) */
  variationSettings: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/status) */
  readonly status: FontFaceLoadStatus;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/loaded) */
  readonly loaded: Promise<FontFace>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFace/load)
   */
  load(): Promise<FontFace>;
}
declare var FontFace: {
  prototype: FontFace;
  new(family: string, source: string | BufferSource, descriptors?: FontFaceDescriptors): FontFace;
};


/**
 * The set of font faces for one realm.
 *
 * On the main global this object is `Yuzuko.fonts`. In a worker it is
 * `self.fonts`. `add` returns the set. `forEach`, `keys`, `values`,
 * `entries`, and `@@iterator` walk the faces in insertion order. `load`,
 * `check`, `ready`, and the `loading`, `loadingdone`, and `loadingerror`
 * events are absent. A family that has finished `load()` and is in the set
 * can be named from the Canvas 2D `font` property.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet)
 */
interface FontFaceSet extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/add) */
  add(font: FontFace): this;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/delete) */
  delete(font: FontFace): boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/has) */
  has(font: FontFace): boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/clear) */
  clear(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/forEach) */
  forEach(callbackfn: (value: FontFace, key: FontFace, parent: FontFaceSet) => void, thisArg?: any): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/entries) */
  entries(): SetIterator<[FontFace, FontFace]>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/keys) */
  keys(): SetIterator<FontFace>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/values) */
  values(): SetIterator<FontFace>;

  [Symbol.iterator](): SetIterator<FontFace>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/size) */
  readonly size: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/status) */
  readonly status: FontFaceSetLoadStatus;
}
declare var FontFaceSet: {
  prototype: FontFaceSet;
  new(): FontFaceSet;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSetLoadEvent)
 */
interface FontFaceSetLoadEvent extends Event {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSetLoadEvent/fontfaces) */
  readonly fontfaces: readonly FontFace[];
}
declare var FontFaceSetLoadEvent: {
  prototype: FontFaceSetLoadEvent;
  new(type: string, eventInitDict?: FontFaceSetLoadEventInit): FontFaceSetLoadEvent;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSetLoadEvent/FontFaceSetLoadEvent) */
interface FontFaceSetLoadEventInit extends EventInit {
  fontfaces?: FontFace[];
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvas/convertToBlob)
 */
interface ImageEncodeOptions {
  /**
   * `image/png`, `image/jpeg`, or `image/webp`.
   *
   * WebP is lossless, so `quality` does not change those bytes.
   */
  type?: string;
  /** From 0 to 1. Used by JPEG. */
  quality?: number;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvas/getContext)
 */
interface CanvasRenderingContext2DSettings {
  /** `"srgb"` or `"display-p3"`. @defaultValue `"srgb"` */
  colorSpace?: PredefinedColorSpace;
  /** `"unorm8"` or `"float16"`. @defaultValue `"unorm8"` */
  colorType?: "unorm8" | "float16";
  alpha?: boolean;
  desynchronized?: boolean;
  willReadFrequently?: boolean;
}

/**
 * A canvas that is not on screen.
 *
 * `new OffscreenCanvas(w, h)` has no window. Assigning `width` or `height`
 * reallocates the buffer. A window bitmap is {@link Yuzuko.WindowSurface}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvas)
 */
interface OffscreenCanvas extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvas/width) */
  width: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvas/height) */
  height: number;

  /**
   * `"webgl"` and `"webgl2"` return `null`.
   *
   * @throws {@link TypeError} When `contextType` is not a supported kind.
   * @throws {@link DOMException} `InvalidStateError` when the canvas is detached.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvas/getContext)
   */
  getContext(
    contextType: "2d",
    options?: CanvasRenderingContext2DSettings,
  ): OffscreenCanvasRenderingContext2D | null;
  getContext(contextType: "webgpu", options?: any): GPUCanvasContext | null;
  getContext(
    contextType: "bitmaprenderer",
    options?: ImageBitmapRenderingContextSettings,
  ): BitmapContext | null;
  getContext(contextType: "webgl" | "webgl2", options?: any): null;
  getContext(contextType: string, options?: any): RenderingContext | null;

  /**
   * @throws {@link DOMException} `InvalidStateError` when the canvas is detached.
   * @throws {@link DOMException} `NotSupportedError` when no canvas hooks are installed.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvas/transferToImageBitmap)
   */
  transferToImageBitmap(): ImageBitmap;

  /**
   * @throws {@link DOMException} `InvalidStateError` when the canvas is detached.
   * @throws {@link DOMException} `NotSupportedError` when no canvas hooks are installed.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvas/convertToBlob)
   */
  convertToBlob(options?: ImageEncodeOptions): Promise<Blob>;
}
declare var OffscreenCanvas: {
  prototype: OffscreenCanvas;
  new(width: number, height: number): OffscreenCanvas;
};


/**
 * Level A Canvas 2D context.
 *
 * The constructor name is `OffscreenCanvasRenderingContext2D`. The rasterizer
 * is vello. Path, text, and gradient paints are sRGB unorm8. `display-p3`
 * and `float16` stores keep `putImageData` and a matching `drawImage`.
 * `Yuzuko.canvasRenderer` is `"gpu"` or `"cpu"`.
 *
 * `shadowBlur`, `shadowColor`, `shadowOffsetX`, and `shadowOffsetY` are
 * stored with the canvas state. Invalid values are ignored. Shadows are not
 * painted. `globalCompositeOperation` stores a recognized operator, including
 * `"clear"` and the blend modes. Any other value is ignored. Painting uses
 * `"source-over"`. `createConicGradient` throws `NotSupportedError`. `filter`
 * is parsed and stored and is not painted.
 * There is no `CanvasFilter` class.
 *
 * A window surface's context exposes that surface as `canvas`.
 *
 * @throws {@link TypeError} Illegal constructor. Use `getContext("2d")`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OffscreenCanvasRenderingContext2D)
 */
interface OffscreenCanvasRenderingContext2D {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/canvas) */
  readonly canvas: OffscreenCanvas | Yuzuko.WindowSurface;

  /**
   * Returns `colorSpace` and `colorType` from the context.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/getContextAttributes)
   */
  getContextAttributes(): {
    alpha: boolean;
    colorSpace: PredefinedColorSpace;
    colorType: "unorm8" | "float16";
    desynchronized: boolean;
    willReadFrequently: boolean;
  };

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/save) */
  save(): void;

  /**
   * @throws {@link DOMException} `InvalidStateError` when the top of the stack is a layer.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/restore)
   */
  restore(): void;

  /**
   * Opens a layer.
   *
   * `null`, `undefined`, and `""` omit the filter. Any other unparsable
   * value, including a filter dictionary, throws `TypeError` and does not
   * open a layer. The filter is not painted. Entering a layer sets
   * `globalAlpha` to `1`, `globalCompositeOperation` to `"source-over"`,
   * the shadow attributes to their defaults, `filter` to `"none"`, and the
   * clip list to empty.
   *
   * @throws {@link TypeError} When `filter` does not parse.
   */
  beginLayer(options?: { filter?: string | null }): void;

  /**
   * Composites the current layer.
   *
   * @throws {@link DOMException} `InvalidStateError` when the top of the stack
   * is a save or the stack is empty.
   */
  endLayer(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/reset) */
  reset(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/resetTransform) */
  resetTransform(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/scale) */
  scale(x: number, y: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/rotate) */
  rotate(angle: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/translate) */
  translate(x: number, y: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/transform) */
  transform(a: number, b: number, c: number, d: number, e: number, f: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/setTransform) */
  setTransform(a: number, b: number, c: number, d: number, e: number, f: number): void;
  setTransform(transform?: DOMMatrix2DInit): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/getTransform) */
  getTransform(): DOMMatrix;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/globalAlpha) */
  globalAlpha: number;

  /**
   * A compositing or blend operator.
   *
   * A recognized value is stored, including `"clear"`. Any other value is
   * ignored. Painting uses `"source-over"`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/globalCompositeOperation)
   */
  globalCompositeOperation: GlobalCompositeOperation;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/fillStyle) */
  fillStyle: string | CanvasGradient | CanvasPattern;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/strokeStyle) */
  strokeStyle: string | CanvasGradient | CanvasPattern;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/createLinearGradient) */
  createLinearGradient(x0: number, y0: number, x1: number, y1: number): CanvasGradient;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/createRadialGradient) */
  createRadialGradient(
    x0: number,
    y0: number,
    r0: number,
    x1: number,
    y1: number,
    r1: number,
  ): CanvasGradient;

  /**
   * @throws {@link DOMException} `NotSupportedError` Conic gradients are outside level A.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/createConicGradient)
   */
  createConicGradient(startAngle: number, x: number, y: number): CanvasGradient;

  /**
   * Sources are {@link ImageBitmap}, {@link OffscreenCanvas}, and {@link VideoFrame}.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/createPattern)
   */
  createPattern(
    image: CanvasImageSource,
    repetition: string | null,
  ): CanvasPattern | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/lineWidth) */
  lineWidth: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/lineCap) */
  lineCap: CanvasLineCap;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/lineJoin) */
  lineJoin: CanvasLineJoin;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/miterLimit) */
  miterLimit: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/setLineDash) */
  setLineDash(segments: number[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/getLineDash) */
  getLineDash(): number[];

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/lineDashOffset) */
  lineDashOffset: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/fillRect) */
  fillRect(x: number, y: number, w: number, h: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/strokeRect) */
  strokeRect(x: number, y: number, w: number, h: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/clearRect) */
  clearRect(x: number, y: number, w: number, h: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/beginPath) */
  beginPath(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/closePath) */
  closePath(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/moveTo) */
  moveTo(x: number, y: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/lineTo) */
  lineTo(x: number, y: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/quadraticCurveTo) */
  quadraticCurveTo(cpx: number, cpy: number, x: number, y: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/bezierCurveTo) */
  bezierCurveTo(
    cp1x: number,
    cp1y: number,
    cp2x: number,
    cp2y: number,
    x: number,
    y: number,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/rect) */
  rect(x: number, y: number, w: number, h: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/arc) */
  arc(
    x: number,
    y: number,
    radius: number,
    startAngle: number,
    endAngle: number,
    counterclockwise?: boolean,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/ellipse) */
  ellipse(
    x: number,
    y: number,
    radiusX: number,
    radiusY: number,
    rotation: number,
    startAngle: number,
    endAngle: number,
    counterclockwise?: boolean,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/arcTo) */
  arcTo(x1: number, y1: number, x2: number, y2: number, radius: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/roundRect) */
  roundRect(x: number, y: number, w: number, h: number, radii?: number | number[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/fill) */
  fill(fillRule?: CanvasFillRule): void;
  fill(path: Path2D, fillRule?: CanvasFillRule): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/stroke) */
  stroke(path?: Path2D): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/clip) */
  clip(fillRule?: CanvasFillRule): void;
  clip(path: Path2D, fillRule?: CanvasFillRule): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/isPointInPath) */
  isPointInPath(x: number, y: number, fillRule?: CanvasFillRule): boolean;
  isPointInPath(path: Path2D, x: number, y: number, fillRule?: CanvasFillRule): boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/isPointInStroke) */
  isPointInStroke(x: number, y: number): boolean;
  isPointInStroke(path: Path2D, x: number, y: number): boolean;

  /**
   * Sources are {@link ImageBitmap}, {@link OffscreenCanvas}, and {@link VideoFrame}.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/drawImage)
   */
  drawImage(image: CanvasImageSource, dx: number, dy: number): void;
  drawImage(
    image: CanvasImageSource,
    dx: number,
    dy: number,
    dw: number,
    dh: number,
  ): void;
  drawImage(
    image: CanvasImageSource,
    sx: number,
    sy: number,
    sw: number,
    sh: number,
    dx: number,
    dy: number,
    dw: number,
    dh: number,
  ): void;

  /**
   * `false` samples nearest. `true` uses {@link OffscreenCanvasRenderingContext2D.imageSmoothingQuality}.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/imageSmoothingEnabled)
   */
  imageSmoothingEnabled: boolean;

  /**
   * `"low"` samples nearest. `"medium"` and `"high"` sample linearly.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/imageSmoothingQuality)
   */
  imageSmoothingQuality: ImageSmoothingQuality;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/createImageData) */
  createImageData(sw: number, sh: number, settings?: ImageDataSettings): ImageData;
  createImageData(imageData: ImageData): ImageData;

  /**
   * May stall on a GPU readback.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/getImageData)
   */
  getImageData(
    sx: number,
    sy: number,
    sw: number,
    sh: number,
    settings?: ImageDataSettings,
  ): ImageData;

  /**
   * A buffer of the other typed-array kind throws `InvalidStateError`.
   *
   * @throws {@link DOMException} `InvalidStateError` when `imageData.data` does
   * not match `pixelFormat`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/putImageData)
   */
  putImageData(imageData: ImageData, dx: number, dy: number): void;
  putImageData(
    imageData: ImageData,
    dx: number,
    dy: number,
    dirtyX: number,
    dirtyY: number,
    dirtyWidth: number,
    dirtyHeight: number,
  ): void;

  /**
   * A CSS font shorthand.
   *
   * A list uses the first loaded family from this realm's `FontFaceSet`, or
   * else the first of `sans-serif`, `serif`, and `monospace`. A size that
   * names only an unregistered family is stored. Shaping uses `sans-serif`
   * for that family. A value that does not parse is ignored.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/font)
   */
  font: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/textAlign) */
  textAlign: CanvasTextAlign;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/textBaseline) */
  textBaseline: CanvasTextBaseline;

  /**
   * A BCP 47 tag, `""`, or `"inherit"`.
   *
   * The default is `"inherit"`. There is no document, so `"inherit"` and `""`
   * shape with an unknown language. The text stack applies the primary
   * language subtag. A tag it cannot parse is stored and shaped as unknown.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/lang)
   */
  lang: string;

  /**
   * `"ltr"`, `"rtl"`, or `"inherit"`.
   *
   * `"inherit"` is stored as left-to-right. Any other value is ignored.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/direction)
   */
  direction: CanvasDirection;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/fillText) */
  fillText(text: string, x: number, y: number, maxWidth?: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/strokeText) */
  strokeText(text: string, x: number, y: number, maxWidth?: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/measureText) */
  measureText(text: string): TextMetrics;

  /**
   * Draws one cluster from {@link TextMetrics.getTextClusters}.
   *
   * `options` may replace `align`, `baseline`, `x`, and `y` for this call.
   */
  fillTextCluster(cluster: TextCluster, options?: TextClusterOptions): void;

  /** Draws one cluster. See {@link OffscreenCanvasRenderingContext2D.fillTextCluster}. */
  strokeTextCluster(cluster: TextCluster, options?: TextClusterOptions): void;

  /**
   * A CSS filter string. The default is `"none"`.
   *
   * A value that parses is stored. The getter returns that string. `""`,
   * `inherit`, `initial`, `unset`, `revert`, `revert-layer`, `null`, and
   * `undefined` do not parse, so the current filter stays. `calc()`, `hwb()`,
   * `lab()`, `lch()`, `oklab()`, `oklch()`, and `color-mix()` do not parse.
   * The stored filter is not painted.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/filter)
   */
  filter: string;

  /**
   * A non-negative finite value is stored. Any other value is ignored.
   * Shadows are not painted.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/shadowBlur)
   */
  shadowBlur: number;

  /**
   * A CSS color is stored. An invalid color is ignored. The default is
   * `"rgba(0, 0, 0, 0)"`. Shadows are not painted.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/shadowColor)
   */
  shadowColor: string;

  /**
   * A finite value is stored, including a negative offset. A non-finite
   * value is ignored. Shadows are not painted.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/shadowOffsetX)
   */
  shadowOffsetX: number;

  /**
   * A finite value is stored, including a negative offset. A non-finite
   * value is ignored. Shadows are not painted.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/shadowOffsetY)
   */
  shadowOffsetY: number;
}
declare var OffscreenCanvasRenderingContext2D: {
  prototype: OffscreenCanvasRenderingContext2D;
  new(): OffscreenCanvasRenderingContext2D;
};


/** Options for drawing one {@link TextCluster}. */
interface TextClusterOptions {
  align?: CanvasTextAlign;
  baseline?: CanvasTextBaseline;
  x?: number;
  y?: number;
}

/**
 * Measured text.
 *
 * The classic `actualBoundingBox*` getters are advance approximations.
 * `getActualBoundingBox` uses unhinted outline bounds. A shaper cluster that
 * covers more than one grapheme stays one cluster.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics)
 */
interface TextMetrics {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/width) */
  readonly width: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/actualBoundingBoxAscent) */
  readonly actualBoundingBoxAscent: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/actualBoundingBoxDescent) */
  readonly actualBoundingBoxDescent: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/fontBoundingBoxAscent) */
  readonly fontBoundingBoxAscent: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/fontBoundingBoxDescent) */
  readonly fontBoundingBoxDescent: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/actualBoundingBoxLeft) */
  readonly actualBoundingBoxLeft: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/actualBoundingBoxRight) */
  readonly actualBoundingBoxRight: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/emHeightAscent) */
  readonly emHeightAscent: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/emHeightDescent) */
  readonly emHeightDescent: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/hangingBaseline) */
  readonly hangingBaseline: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/alphabeticBaseline) */
  readonly alphabeticBaseline: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextMetrics/ideographicBaseline) */
  readonly ideographicBaseline: number;

  /**
   * Advance boxes from `start` to `end`, in UTF-16 code units.
   */
  getSelectionRects(start: number, end: number): DOMRect[];

  /**
   * Outline bounds from `start` to `end`.
   *
   * The bounds are unhinted outlines at the font's default variation, not
   * rasterized ink.
   */
  getActualBoundingBox(start: number, end: number): DOMRect;

  /**
   * Clusters for the whole run, or for `start` through `end`.
   *
   * `options` changes the returned clusters and does not change this object.
   */
  getTextClusters(
    start?: number,
    end?: number,
    options?: TextClusterOptions,
  ): TextCluster[];

  /**
   * UTF-16 index at `offset` in the text origin.
   *
   * Values outside the run return `0` or the UTF-16 length according to
   * `direction`.
   */
  getIndexFromOffset(offset: number): number;
}
declare var TextMetrics: {
  prototype: TextMetrics;
  new(): TextMetrics;
};


/**
 * One shaped cluster.
 *
 * Indices are UTF-16 code units. Positions are relative to the text origin
 * after `textAlign` and `textBaseline`.
 *
 * @throws {@link TypeError} Illegal constructor.
 */
declare class TextCluster {
  constructor();

  readonly x: number;
  readonly y: number;
  readonly start: number;
  readonly end: number;
  readonly align: CanvasTextAlign;
  readonly baseline: CanvasTextBaseline;
}

/**
 * A canvas gradient.
 *
 * Stops accept CSS colors, `color(srgb ...)`, and `color(display-p3 ...)`.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasGradient)
 */
interface CanvasGradient {

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasGradient/addColorStop)
   */
  addColorStop(offset: number, color: string): void;
}
declare var CanvasGradient: {
  prototype: CanvasGradient;
  new(): CanvasGradient;
};


/**
 * A canvas pattern.
 *
 * Repeat modes are `repeat`, `repeat-x`, `repeat-y`, and `no-repeat`.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasPattern)
 */
interface CanvasPattern {

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasPattern/setTransform)
   */
  setTransform(transform?: DOMMatrix2DInit): void;
}
declare var CanvasPattern: {
  prototype: CanvasPattern;
  new(): CanvasPattern;
};


/**
 * A path.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Path2D)
 */
interface Path2D {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Path2D/addPath) */
  addPath(path: Path2D, transform?: DOMMatrix2DInit): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/closePath) */
  closePath(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/moveTo) */
  moveTo(x: number, y: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/lineTo) */
  lineTo(x: number, y: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/quadraticCurveTo) */
  quadraticCurveTo(cpx: number, cpy: number, x: number, y: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/bezierCurveTo) */
  bezierCurveTo(
    cp1x: number,
    cp1y: number,
    cp2x: number,
    cp2y: number,
    x: number,
    y: number,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/rect) */
  rect(x: number, y: number, w: number, h: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/arc) */
  arc(
    x: number,
    y: number,
    radius: number,
    startAngle: number,
    endAngle: number,
    counterclockwise?: boolean,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/ellipse) */
  ellipse(
    x: number,
    y: number,
    radiusX: number,
    radiusY: number,
    rotation: number,
    startAngle: number,
    endAngle: number,
    counterclockwise?: boolean,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/arcTo) */
  arcTo(x1: number, y1: number, x2: number, y2: number, radius: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CanvasRenderingContext2D/roundRect) */
  roundRect(x: number, y: number, w: number, h: number, radii?: number | number[]): void;
}
declare var Path2D: {
  prototype: Path2D;
  new(path?: Path2D | string): Path2D;
};


/**
 * Bitmap renderer context.
 *
 * The constructor is installed as `BitmapContext`. `getContext("bitmaprenderer")`
 * returns this object.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageBitmapRenderingContext)
 */
declare class BitmapContext {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageBitmapRenderingContext/canvas) */
  readonly canvas: OffscreenCanvas | Yuzuko.WindowSurface;

  /**
   * `null` clears the bitmap.
   *
   * @throws {@link TypeError} When `bitmap` is not an {@link ImageBitmap} or `null`.
   * @throws {@link DOMException} `InvalidStateError` when the bitmap is detached.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageBitmapRenderingContext/transferFromImageBitmap)
   */
  transferFromImageBitmap(bitmap: ImageBitmap | null): void;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageBitmapRenderingContext/ImageBitmapRenderingContext)
 */
interface ImageBitmapRenderingContextSettings {
  alpha?: boolean;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageData/ImageData)
 */
interface ImageDataSettings {
  colorSpace?: PredefinedColorSpace;
  pixelFormat?: "rgba-unorm8" | "rgba-float16";
}

/**
 * Pixel data.
 *
 * `rgba-unorm8` is stored in a `Uint8ClampedArray`. `rgba-float16` is stored
 * in a `Float16Array`. Alpha is unpremultiplied. Float16 components are the
 * encoded values of the stored color space and are not clamped to 0..1.
 * `structuredClone` copies the pixels, color space, and pixel format into a
 * new typed array. `ImageData` is not transferable. Transferring `data`'s
 * buffer detaches that buffer, and the clone keeps the pixels.
 *
 * @throws {@link DOMException} `InvalidStateError` when the typed array does
 * not match `pixelFormat`.
 * @throws {@link DOMException} `DataCloneError` when an `ImageData` is listed
 * for transfer, or when its data buffer is detached.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageData)
 */
interface ImageData {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageData/width) */
  readonly width: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageData/height) */
  readonly height: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageData/colorSpace) */
  readonly colorSpace: PredefinedColorSpace;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageData/pixelFormat) */
  readonly pixelFormat: "rgba-unorm8" | "rgba-float16";

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageData/data) */
  readonly data: ImageDataArray;
}
declare var ImageData: {
  prototype: ImageData;
  new(sw: number, sh: number, settings?: ImageDataSettings): ImageData;
  new(data: ImageDataArray, sw: number, sh?: number, settings?: ImageDataSettings): ImageData;
};


/**
 * A decoded bitmap.
 *
 * Color space and sample format are not exposed. Animated images contribute
 * the first frame. The longer edge is at most 16384 pixels and the bitmap is
 * at most 100 million pixels. `structuredClone` copies the bitmap and leaves
 * this one open. Listing it in a transfer list detaches it: `width` and
 * `height` become 0. `close()` does the same.
 *
 * @throws {@link TypeError} Illegal constructor. Use {@link createImageBitmap}.
 * @throws {@link DOMException} `DataCloneError` when a detached bitmap is cloned.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageBitmap)
 */
interface ImageBitmap {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageBitmap/width) */
  readonly width: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageBitmap/height) */
  readonly height: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageBitmap/close) */
  close(): void;
}
declare var ImageBitmap: {
  prototype: ImageBitmap;
  new(): ImageBitmap;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/createImageBitmap)
 */
interface ImageBitmapOptions {
  imageOrientation?: "from-image" | "flipY" | "none";
  premultiplyAlpha?: "none" | "premultiply" | "default";
  colorSpaceConversion?: "none" | "default";
  resizeWidth?: number;
  resizeHeight?: number;
  resizeQuality?: "pixelated" | "low" | "medium" | "high";
}

/**
 * Decodes or copies `image`.
 *
 * A `Blob` is identified by its signature: JPEG, PNG, WebP, or JPEG XL.
 * `colorSpaceConversion: "none"` keeps an sRGB or Display P3 source.
 * `"default"` stores sRGB. Any other source profile is converted to sRGB.
 * `resizeQuality` `"pixelated"` and `"low"` are nearest-neighbor. `"medium"`
 * and `"high"` use a triangle filter. `premultiplyAlpha: "default"` leaves
 * samples unpremultiplied. Decoding does not block the event loop. A decoder
 * panic rejects `InvalidStateError`.
 *
 * @throws {@link DOMException} `InvalidStateError` when the input cannot be
 * decoded or a size limit is exceeded.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/createImageBitmap)
 */
declare function createImageBitmap(
  image: ImageBitmapSource,
  options?: ImageBitmapOptions,
): Promise<ImageBitmap>;
declare function createImageBitmap(
  image: ImageBitmapSource,
  sx: number,
  sy: number,
  sw: number,
  sh: number,
  options?: ImageBitmapOptions,
): Promise<ImageBitmap>;

/**
 * WebGPU entry point.
 *
 * One process device is shared with Canvas 2D. It enables `timestamp-query`,
 * `depth-clip-control`, and `depth32float-stencil8` when the adapter
 * supports them, and wgpu external textures when the adapter supports that
 * feature. `YUZUKO_SKIP_GPU=1` makes {@link GPU.requestAdapter} resolve
 * `null`. `wgslLanguageFeatures` is empty.
 *
 * Blend state and alpha to coverage are applied. `unclippedDepth: true`
 * throws `NotSupportedError` without `depth-clip-control`.
 * `depth32float-stencil8` throws `NotSupportedError` without that feature.
 * `externalTexture` and `importExternalTexture` throw `NotSupportedError`
 * without wgpu external textures. A `src1` blend factor throws
 * `NotSupportedError` without `dual-source-blending`. Multisample `count`
 * is 1 or 4, and a color attachment may set `resolveTarget`. Storage texture
 * `access` may be `"read-only"`, `"read-write"`, or `"atomic"`. An access
 * the format does not allow is a validation error. `setViewport` and
 * `setScissorRect` are on the render pass. Debug markers, bundle viewport
 * and scissor, and `writeTimestamp` stay absent. `GPUQuerySet.destroy`
 * makes a later `resolveQuerySet` a validation error.
 *
 * @throws {@link TypeError} Illegal constructor. Use {@link Navigator.gpu}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPU)
 */
interface GPU {

  /**
   * Resolves `null` when no adapter exists.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPU/requestAdapter)
   */
  requestAdapter(options?: GPURequestAdapterOptions): Promise<GPUAdapter | null>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPU/getPreferredCanvasFormat)
   */
  getPreferredCanvasFormat(): GPUTextureFormat;

  /**
   * Empty.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPU/wgslLanguageFeatures)
   */
  readonly wgslLanguageFeatures: WGSLLanguageFeatures;
}
declare var GPU: {
  prototype: GPU;
  new(): GPU;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPU/requestAdapter) */
interface GPURequestAdapterOptions {
  powerPreference?: GPUPowerPreference;
  forceFallbackAdapter?: boolean;
}

/**
 * The shared adapter.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUAdapter)
 */
interface GPUAdapter {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUAdapter/features) */
  readonly features: GPUSupportedFeatures;

  /**
   * The shared device's wgpu defaults, or the adapter's own limits when a
   * downlevel adapter cannot meet those defaults.
   *
   * `maxBindGroupsPlusVertexBuffers` can be below the WebGPU minimum.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUAdapter/limits)
   */
  readonly limits: GPUSupportedLimits;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUAdapter/isFallbackAdapter) */
  readonly isFallbackAdapter: boolean;

  /**
   * Requests a JavaScript device on the shared wgpu device.
   *
   * A feature the shared device does not have rejects. `timestamp-query`,
   * `depth-clip-control`, and `depth32float-stencil8` are already enabled
   * when the adapter supports them. `requiredLimits` the shared device
   * cannot satisfy also reject. `destroy` loses that JavaScript device only.
   *
   * @throws {@link TypeError} When a required feature is not enabled on the
   * shared device, the feature name is invalid, or a required limit is
   * outside the shared device.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUAdapter/requestDevice)
   */
  requestDevice(descriptor?: GPUDeviceDescriptor): Promise<GPUDevice>;
}
declare var GPUAdapter: {
  prototype: GPUAdapter;
  new(): GPUAdapter;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/GPUDevice) */
interface GPUDeviceDescriptor {
  label?: string;
  requiredFeatures?: GPUFeatureName[];
  requiredLimits?: Record<string, number | undefined>;
}

/**
 * A JavaScript WebGPU device.
 *
 * `lost` resolves from `destroy`, not from garbage collection. Uncaptured
 * errors are printed and are not dispatched as `uncapturederror`.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice)
 */
interface GPUDevice {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/label) */
  label: string;

  /**
   * Resolves `{ reason: "destroyed", message }` from {@link GPUDevice.destroy}.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/lost)
   */
  readonly lost: Promise<GPUDeviceLostInfo>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/features) */
  readonly features: GPUSupportedFeatures;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/limits) */
  readonly limits: GPUSupportedLimits;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/queue) */
  readonly queue: GPUQueue;

  /**
   * Loses this JavaScript device. The shared wgpu device stays.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/destroy)
   */
  destroy(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBuffer) */
  createBuffer(descriptor: GPUBufferDescriptor): GPUBuffer;

  /**
   * `sampleCount` is 1 or 4. `depth32float-stencil8` throws
   * `NotSupportedError` when that feature is not enabled.
   *
   * @throws {@link TypeError} When `sampleCount` is not 1 or 4.
   * @throws {@link DOMException} `NotSupportedError` when the format needs a
   * feature the process device does not have.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createTexture)
   */
  createTexture(descriptor: GPUTextureDescriptor): GPUTexture;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createSampler) */
  createSampler(descriptor?: GPUSamplerDescriptor): GPUSampler;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createShaderModule) */
  createShaderModule(descriptor: GPUShaderModuleDescriptor): GPUShaderModule;

  /**
   * Storage texture `access` is `"write-only"`, `"read-only"`, `"read-write"`,
   * or `"atomic"`. An access the format does not allow is a validation error.
   * `externalTexture` is a layout entry when the process device supports wgpu
   * external textures.
   *
   * @throws {@link DOMException} `NotSupportedError` when `externalTexture` is
   * set and the process device has no wgpu external textures.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroupLayout)
   */
  createBindGroupLayout(descriptor: GPUBindGroupLayoutDescriptor): GPUBindGroupLayout;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createPipelineLayout) */
  createPipelineLayout(descriptor: GPUPipelineLayoutDescriptor): GPUPipelineLayout;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroup) */
  createBindGroup(descriptor: GPUBindGroupDescriptor): GPUBindGroup;

  /**
   * Blend state and `alphaToCoverageEnabled` are applied. `unclippedDepth:
   * true` throws `NotSupportedError` without `depth-clip-control`.
   * `depth32float-stencil8` throws `NotSupportedError` without that feature.
   * A `src1` blend factor throws `NotSupportedError` without
   * `dual-source-blending`. Multisample `count` is 1 or 4.
   *
   * @throws {@link DOMException} `NotSupportedError` for `unclippedDepth`
   * without `depth-clip-control`, for `depth32float-stencil8` without that
   * feature, and for a `src1` factor without `dual-source-blending`.
   * @throws {@link TypeError} When the multisample `count` is not 1 or 4.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline)
   */
  createRenderPipeline(descriptor: GPURenderPipelineDescriptor): GPURenderPipeline;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createCommandEncoder) */
  createCommandEncoder(descriptor?: GPUCommandEncoderDescriptor): GPUCommandEncoder;

  /**
   * `sampleCount` is 1 or 4. Viewport and scissor stay absent on the encoder.
   * `depth32float-stencil8` throws `NotSupportedError` without that feature.
   *
   * @throws {@link TypeError} When `sampleCount` is not 1 or 4.
   * @throws {@link DOMException} `NotSupportedError` when a format needs a
   * feature the process device does not have.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderBundleEncoder)
   */
  createRenderBundleEncoder(descriptor: GPURenderBundleEncoderDescriptor): GPURenderBundleEncoder;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createComputePipeline) */
  createComputePipeline(descriptor: GPUComputePipelineDescriptor): GPUComputePipeline;

  /**
   * The promise is already settled. Compilation runs on the calling thread.
   * A pipeline error rejects `OperationError`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createComputePipelineAsync)
   */
  createComputePipelineAsync(descriptor: GPUComputePipelineDescriptor): Promise<GPUComputePipeline>;

  /**
   * A `"timestamp"` query set throws `NotSupportedError` when `timestamp-query`
   * is not enabled.
   *
   * @throws {@link DOMException} `NotSupportedError` when `timestamp-query` is
   * not enabled.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createQuerySet)
   */
  createQuerySet(descriptor: GPUQuerySetDescriptor): GPUQuerySet;

  /**
   * Copies a `VideoFrame` into an RGBA external texture.
   *
   * A fragment shader samples it with `textureSampleBaseClampToEdge`.
   * `colorSpace` is `"srgb"` or `"display-p3"`. A closed frame throws
   * `InvalidStateError`. There is no `HTMLVideoElement`. Without wgpu
   * external-texture support this throws `NotSupportedError`.
   *
   * @throws {@link DOMException} `NotSupportedError` when the process device
   * has no wgpu external textures.
   * @throws {@link DOMException} `InvalidStateError` when the frame is closed.
   * @throws {@link TypeError} When `source` is not a `VideoFrame`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/importExternalTexture)
   */
  importExternalTexture(descriptor: GPUExternalTextureDescriptor): GPUExternalTexture;

  /**
   * `filter` is `"validation"`, `"out-of-memory"`, or `"internal"`.
   *
   * @throws {@link TypeError} When `filter` is not one of those strings.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/pushErrorScope)
   */
  pushErrorScope(filter: GPUErrorFilter): void;

  /**
   * A validation failure is a {@link GPUValidationError}.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/popErrorScope)
   */
  popErrorScope(): Promise<GPUError | null>;
}
declare var GPUDevice: {
  prototype: GPUDevice;
  new(): GPUDevice;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/lost) */
interface GPUDeviceLostInfo {
  readonly reason: GPUDeviceLostReason;
  readonly message: string;
}


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderBundleEncoder) */
interface GPURenderBundleEncoderDescriptor {
  label?: string;
  colorFormats: (GPUTextureFormat | null)[];
  depthStencilFormat?: GPUTextureFormat;
  sampleCount?: GPUSize32;
  depthReadOnly?: boolean;
  stencilReadOnly?: boolean;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createComputePipeline) */
interface GPUComputePipelineDescriptor {
  label?: string;
  layout: GPUPipelineLayout | "auto";
  compute: GPUProgrammableStage;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createComputePipeline) */
interface GPUProgrammableStage {
  module: GPUShaderModule;
  entryPoint?: string;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createQuerySet) */
interface GPUQuerySetDescriptor {
  label?: string;
  type: GPUQueryType;
  count: GPUSize32;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUValidationError)
 */
interface GPUValidationError {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUValidationError/message) */
  readonly message: string;
}
declare var GPUValidationError: {
  prototype: GPUValidationError;
  new(message: string): GPUValidationError;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUOutOfMemoryError)
 */
interface GPUOutOfMemoryError {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUOutOfMemoryError/message) */
  readonly message: string;
}
declare var GPUOutOfMemoryError: {
  prototype: GPUOutOfMemoryError;
  new(message: string): GPUOutOfMemoryError;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUInternalError)
 */
interface GPUInternalError {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUInternalError/message) */
  readonly message: string;
}
declare var GPUInternalError: {
  prototype: GPUInternalError;
  new(message: string): GPUInternalError;
};


/**
 * The device queue.
 *
 * `getMappedRange` returns a copy, and `unmap` does not detach that `ArrayBuffer`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUQueue)
 */
interface GPUQueue {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUQueue/label) */
  label: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUQueue/writeBuffer) */
  writeBuffer(
    buffer: GPUBuffer,
    bufferOffset: number,
    data: BufferSource,
    dataOffset?: number,
    size?: number,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUQueue/submit) */
  submit(commandBuffers: Iterable<GPUCommandBuffer>): void;

  /**
   * Copies an {@link ImageBitmap}, {@link OffscreenCanvas}, or {@link VideoFrame}.
   *
   * Matching unorm8 bytes upload directly. A float16 source, a color-space
   * change, a sub-rectangle, or a `bgra8unorm` destination is converted on
   * the CPU. `colorSpace` is `"srgb"` or `"display-p3"`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUQueue/copyExternalImageToTexture)
   */
  copyExternalImageToTexture(
    source: GPUCopyExternalImageSourceInfo,
    destination: GPUCopyExternalImageDestInfo,
    copySize: GPUExtent3D,
  ): void;
}
declare var GPUQueue: {
  prototype: GPUQueue;
  new(): GPUQueue;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUQueue/copyExternalImageToTexture) */
interface GPUCopyExternalImageSourceInfo {
  source: GPUCopyExternalImageSource;
  origin?: GPUOrigin2D;
  flipY?: boolean;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUQueue/copyExternalImageToTexture) */
interface GPUCopyExternalImageDestInfo {
  texture: GPUTexture;
  mipLevel?: number;
  origin?: GPUOrigin3D;
  aspect?: GPUTextureAspect;
  colorSpace?: PredefinedColorSpace;
  premultipliedAlpha?: boolean;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUBuffer)
 */
interface GPUBuffer {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUBuffer/label) */
  label: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUBuffer/size) */
  readonly size: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUBuffer/destroy) */
  destroy(): void;
  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUBuffer/mapAsync)
   */
  mapAsync(mode: GPUMapModeFlags, offset?: number, size?: number): Promise<void>;
  /**
   * Returns a copy. `unmap` does not detach it.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUBuffer/getMappedRange)
   */
  getMappedRange(offset?: number, size?: number): ArrayBuffer;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUBuffer/unmap) */
  unmap(): void;
}
declare var GPUBuffer: {
  prototype: GPUBuffer;
  new(): GPUBuffer;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBuffer) */
interface GPUBufferDescriptor {
  label?: string;
  size: number;
  usage: GPUBufferUsageFlags;
  mappedAtCreation?: boolean;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUTexture)
 */
interface GPUTexture {
  label: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUTexture/format) */
  readonly format: GPUTextureFormat;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUTexture/width) */
  readonly width: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUTexture/height) */
  readonly height: number;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUTexture/createView) */
  createView(descriptor?: GPUTextureViewDescriptor): GPUTextureView;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUTexture/destroy) */
  destroy(): void;
}
declare var GPUTexture: {
  prototype: GPUTexture;
  new(): GPUTexture;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createTexture) */
interface GPUTextureDescriptor {
  label?: string;
  size: GPUExtent3D;
  format: GPUTextureFormat;
  usage: GPUTextureUsageFlags;
  mipLevelCount?: number;
  sampleCount?: number;
  dimension?: GPUTextureDimension;
  viewFormats?: GPUTextureFormat[];
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUTextureView) */
interface GPUTextureView extends GPUObjectBase {}
declare var GPUTextureView: {
  prototype: GPUTextureView;
  new(): GPUTextureView;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUTexture/createView) */
interface GPUTextureViewDescriptor {
  label?: string;
  format?: GPUTextureFormat;
  dimension?: GPUTextureViewDimension;
  aspect?: GPUTextureAspect;
  baseMipLevel?: number;
  mipLevelCount?: number;
  baseArrayLayer?: number;
  arrayLayerCount?: number;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUSampler) */
interface GPUSampler extends GPUObjectBase {}
declare var GPUSampler: {
  prototype: GPUSampler;
  new(): GPUSampler;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createSampler) */
interface GPUSamplerDescriptor {
  label?: string;
  addressModeU?: GPUAddressMode;
  addressModeV?: GPUAddressMode;
  addressModeW?: GPUAddressMode;
  magFilter?: GPUFilterMode;
  minFilter?: GPUFilterMode;
  mipmapFilter?: GPUMipmapFilterMode;
  lodMinClamp?: number;
  lodMaxClamp?: number;
  maxAnisotropy?: number;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUShaderModule) */
interface GPUShaderModule extends GPUObjectBase {}
declare var GPUShaderModule: {
  prototype: GPUShaderModule;
  new(): GPUShaderModule;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createShaderModule) */
interface GPUShaderModuleDescriptor {
  label?: string;
  code: string;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUBindGroupLayout) */
interface GPUBindGroupLayout extends GPUObjectBase {}
declare var GPUBindGroupLayout: {
  prototype: GPUBindGroupLayout;
  new(): GPUBindGroupLayout;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroupLayout) */
interface GPUBindGroupLayoutDescriptor {
  label?: string;
  entries: GPUBindGroupLayoutEntry[];
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroupLayout) */
interface GPUBindGroupLayoutEntry {
  binding: number;
  visibility: GPUShaderStageFlags;
  buffer?: GPUBufferBindingLayout;
  sampler?: GPUSamplerBindingLayout;
  texture?: GPUTextureBindingLayout;
  storageTexture?: GPUStorageTextureBindingLayout;
  externalTexture?: GPUExternalTextureBindingLayout;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroupLayout) */
interface GPUExternalTextureBindingLayout {}

/**
 * A `VideoFrame` imported as an external texture.
 *
 * The host keeps the sampled plane until this object is collected.
 *
 * @throws {@link TypeError} Illegal constructor. Use
 * {@link GPUDevice.importExternalTexture}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUExternalTexture)
 */
interface GPUExternalTexture extends GPUObjectBase {}
declare var GPUExternalTexture: {
  prototype: GPUExternalTexture;
  new(): GPUExternalTexture;
};

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/importExternalTexture) */
interface GPUExternalTextureDescriptor {
  label?: string;
  source: VideoFrame;
  colorSpace?: PredefinedColorSpace;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroupLayout) */
interface GPUStorageTextureBindingLayout {
  access?: GPUStorageTextureAccess;
  format: GPUTextureFormat;
  viewDimension?: GPUTextureViewDimension;
}

type GPUStorageTextureAccess = "atomic" | "read-only" | "read-write" | "write-only";

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroupLayout) */
interface GPUBufferBindingLayout {
  type?: GPUBufferBindingType;
  hasDynamicOffset?: boolean;
  minBindingSize?: number;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroupLayout) */
interface GPUSamplerBindingLayout {
  type?: GPUSamplerBindingType;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroupLayout) */
interface GPUTextureBindingLayout {
  sampleType?: GPUTextureSampleType;
  viewDimension?: GPUTextureViewDimension;
  multisampled?: boolean;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUBindGroup) */
interface GPUBindGroup extends GPUObjectBase {}
declare var GPUBindGroup: {
  prototype: GPUBindGroup;
  new(): GPUBindGroup;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroup) */
interface GPUBindGroupDescriptor {
  label?: string;
  layout: GPUBindGroupLayout;
  entries: GPUBindGroupEntry[];
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroup) */
interface GPUBindGroupEntry {
  binding: number;
  resource: GPUBindingResource;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroup) */
interface GPUBufferBinding {
  buffer: GPUBuffer;
  offset?: number;
  size?: number;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUPipelineLayout) */
interface GPUPipelineLayout extends GPUObjectBase {}
declare var GPUPipelineLayout: {
  prototype: GPUPipelineLayout;
  new(): GPUPipelineLayout;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createPipelineLayout) */
interface GPUPipelineLayoutDescriptor {
  label?: string;
  bindGroupLayouts: (GPUBindGroupLayout | null)[];
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPipeline)
 */
interface GPURenderPipeline {
  label: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPipeline/getBindGroupLayout) */
  getBindGroupLayout(index: number): GPUBindGroupLayout;
}
declare var GPURenderPipeline: {
  prototype: GPURenderPipeline;
  new(): GPURenderPipeline;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline) */
interface GPURenderPipelineDescriptor {
  label?: string;
  layout: GPUPipelineLayout | "auto";
  vertex: GPUVertexState;
  fragment?: GPUFragmentState;
  primitive?: GPUPrimitiveState;
  depthStencil?: GPUDepthStencilState;
  multisample?: GPUMultisampleState;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDepthStencilState) */
interface GPUDepthStencilState {
  format: GPUTextureFormat;
  depthWriteEnabled?: boolean;
  depthCompare?: GPUCompareFunction;
  stencilFront?: GPUStencilFaceState;
  stencilBack?: GPUStencilFaceState;
  stencilReadMask?: number;
  stencilWriteMask?: number;
  depthBias?: number;
  depthBiasSlopeScale?: number;
  depthBiasClamp?: number;
}

type GPUCompareFunction = "always" | "equal" | "greater" | "greater-equal" | "less" | "less-equal" | "never" | "not-equal";

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUStencilFaceState) */
interface GPUStencilFaceState {
  compare?: GPUCompareFunction;
  failOp?: GPUStencilOperation;
  depthFailOp?: GPUStencilOperation;
  passOp?: GPUStencilOperation;
}

type GPUStencilOperation = "decrement-clamp" | "decrement-wrap" | "increment-clamp" | "increment-wrap" | "invert" | "keep" | "replace" | "zero";

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline) */
interface GPUVertexState {
  module: GPUShaderModule;
  entryPoint?: string;
  buffers?: (GPUVertexBufferLayout | null)[];
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline) */
interface GPUVertexBufferLayout {
  arrayStride: number;
  stepMode?: GPUVertexStepMode;
  attributes: GPUVertexAttribute[];
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline) */
interface GPUVertexAttribute {
  format: GPUVertexFormat;
  offset: number;
  shaderLocation: number;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline) */
interface GPUFragmentState {
  module: GPUShaderModule;
  entryPoint?: string;
  targets: (GPUColorTargetState | null)[];
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline) */
interface GPUColorTargetState {
  format: GPUTextureFormat;
  blend?: GPUBlendState;
  writeMask?: GPUColorWriteFlags;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline) */
interface GPUBlendState {
  color: GPUBlendComponent;
  alpha: GPUBlendComponent;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline) */
interface GPUBlendComponent {
  operation?: GPUBlendOperation;
  srcFactor?: GPUBlendFactor;
  dstFactor?: GPUBlendFactor;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline) */
interface GPUPrimitiveState {
  topology?: GPUPrimitiveTopology;
  stripIndexFormat?: GPUIndexFormat;
  frontFace?: GPUFrontFace;
  cullMode?: GPUCullMode;
  /**
   * Requires `depth-clip-control` when true.
   *
   * @throws {@link DOMException} `NotSupportedError` from
   * {@link GPUDevice.createRenderPipeline} when this is true and the process
   * device does not have `depth-clip-control`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUPrimitiveState/unclippedDepth)
   */
  unclippedDepth?: boolean;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline) */
interface GPUMultisampleState {
  count?: number;
  mask?: number;
  alphaToCoverageEnabled?: boolean;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder)
 */
interface GPUCommandEncoder {
  label: string;

  /**
   * A color attachment may set `resolveTarget`.
   *
   * @throws {@link DOMException} `InvalidStateError` when the encoder is finished
   * or a pass is already open.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/beginRenderPass)
   */
  beginRenderPass(descriptor: GPURenderPassDescriptor): GPURenderPassEncoder;

  /**
   * @throws {@link DOMException} `InvalidStateError` when the encoder is finished
   * or a pass is already open.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/beginComputePass)
   */
  beginComputePass(descriptor?: GPUComputePassDescriptor): GPUComputePassEncoder;

  /**
   * A size of 0 throws `RangeError`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/copyBufferToBuffer)
   */
  copyBufferToBuffer(
    source: GPUBuffer,
    sourceOffset: number,
    destination: GPUBuffer,
    destinationOffset: number,
    size: number,
  ): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/copyTextureToBuffer)
   */
  copyTextureToBuffer(
    source: GPUImageCopyTexture,
    destination: GPUImageCopyBuffer,
    copySize: GPUExtent3D,
  ): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/resolveQuerySet)
   */
  resolveQuerySet(
    querySet: GPUQuerySet,
    firstQuery: number,
    queryCount: number,
    destination: GPUBuffer,
    destinationOffset: number,
  ): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/finish)
   */
  finish(descriptor?: GPUCommandBufferDescriptor): GPUCommandBuffer;
}
declare var GPUCommandEncoder: {
  prototype: GPUCommandEncoder;
  new(): GPUCommandEncoder;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder) */
interface GPUCommandEncoderDescriptor {
  label?: string;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/beginRenderPass) */
interface GPURenderPassDescriptor {
  label?: string;
  colorAttachments: (GPURenderPassColorAttachment | null)[];
  depthStencilAttachment?: GPURenderPassDepthStencilAttachment;
  occlusionQuerySet?: GPUQuerySet;
  timestampWrites?: GPURenderPassTimestampWrites;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/beginRenderPass) */
interface GPURenderPassColorAttachment {
  view: GPUTexture | GPUTextureView;
  resolveTarget?: GPUTexture | GPUTextureView;
  clearValue?: GPUColor;
  loadOp: GPULoadOp;
  storeOp: GPUStoreOp;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/beginComputePass) */
interface GPUComputePassDescriptor {
  label?: string;
  timestampWrites?: GPUComputePassTimestampWrites;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/beginComputePass) */
interface GPUComputePassTimestampWrites {
  beginningOfPassWriteIndex?: GPUSize32;
  endOfPassWriteIndex?: GPUSize32;
  querySet: GPUQuerySet;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandBuffer) */
interface GPUCommandBuffer extends GPUObjectBase {}
declare var GPUCommandBuffer: {
  prototype: GPUCommandBuffer;
  new(): GPUCommandBuffer;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/finish) */
interface GPUCommandBufferDescriptor {
  label?: string;
}

/**
 * A render pass.
 *
 * `setViewport` and `setScissorRect` restrict later draws. Debug markers
 * are absent.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder)
 */
interface GPURenderPassEncoder {
  label: string;

  /**
   * @throws {@link TypeError} When an argument is not finite.
   * @throws {@link RangeError} When `width` or `height` is negative, a depth
   * is outside 0 through 1, or `minDepth` is greater than `maxDepth`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/setViewport)
   */
  setViewport(
    x: number,
    y: number,
    width: number,
    height: number,
    minDepth: number,
    maxDepth: number,
  ): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/setScissorRect)
   */
  setScissorRect(
    x: GPUIntegerCoordinate,
    y: GPUIntegerCoordinate,
    width: GPUIntegerCoordinate,
    height: GPUIntegerCoordinate,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/setPipeline) */
  setPipeline(pipeline: GPURenderPipeline): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/setIndexBuffer) */
  setIndexBuffer(
    buffer: GPUBuffer,
    indexFormat: GPUIndexFormat,
    offset?: number,
    size?: number,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/setVertexBuffer) */
  setVertexBuffer(
    slot: number,
    buffer: GPUBuffer | null,
    offset?: number,
    size?: number,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/setBindGroup) */
  setBindGroup(
    index: number,
    bindGroup: GPUBindGroup | null,
    dynamicOffsets?: Iterable<number>,
  ): void;
  setBindGroup(
    index: number,
    bindGroup: GPUBindGroup | null,
    dynamicOffsetsData: Uint32Array<ArrayBufferLike>,
    dynamicOffsetsDataStart: number,
    dynamicOffsetsDataLength: number,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/draw) */
  draw(
    vertexCount: number,
    instanceCount?: number,
    firstVertex?: number,
    firstInstance?: number,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/drawIndexed) */
  drawIndexed(
    indexCount: number,
    instanceCount?: number,
    firstIndex?: number,
    baseVertex?: number,
    firstInstance?: number,
  ): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/drawIndirect) */
  drawIndirect(indirectBuffer: GPUBuffer, indirectOffset: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/drawIndexedIndirect) */
  drawIndexedIndirect(indirectBuffer: GPUBuffer, indirectOffset: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/beginOcclusionQuery) */
  beginOcclusionQuery(queryIndex: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/endOcclusionQuery) */
  endOcclusionQuery(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/executeBundles) */
  executeBundles(bundles: GPURenderBundle[]): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/end) */
  end(): void;
}
declare var GPURenderPassEncoder: {
  prototype: GPURenderPassEncoder;
  new(): GPURenderPassEncoder;
};


/**
 * A canvas WebGPU context.
 *
 * `getCurrentTexture` returns the same texture until that surface's rendering
 * update. An {@link OffscreenCanvas} that is not a window surface has no
 * frame, so the texture stays until `unconfigure`, resize, or disposal.
 * Canvas textures also get `TEXTURE_BINDING`, `COPY_SRC`, and
 * `RENDER_ATTACHMENT`. `alphaMode` `"opaque"` and `"premultiplied"` are
 * stored. Presentation blits stored texels and does not composite.
 * `display-p3` is accepted. The swapchain present is still unorm8 sRGB.
 *
 * A window surface's context exposes that surface as `canvas`.
 *
 * @throws {@link TypeError} Illegal constructor. Use `getContext("webgpu")`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCanvasContext)
 */
interface GPUCanvasContext {

  /** Always `""`. */
  readonly label: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCanvasContext/canvas) */
  readonly canvas: OffscreenCanvas | Yuzuko.WindowSurface;

  /**
   * `depth32float-stencil8` throws `NotSupportedError` without that feature.
   *
   * @throws {@link DOMException} `NotSupportedError` when a format needs a
   * feature the process device does not have.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCanvasContext/configure)
   */
  configure(configuration: GPUCanvasConfiguration): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCanvasContext/unconfigure) */
  unconfigure(): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCanvasContext/getCurrentTexture)
   */
  getCurrentTexture(): GPUTexture;
}
declare var GPUCanvasContext: {
  prototype: GPUCanvasContext;
  new(): GPUCanvasContext;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCanvasContext/configure) */
interface GPUCanvasConfiguration {
  device: GPUDevice;
  format: GPUTextureFormat;
  usage?: GPUTextureUsageFlags;
  viewFormats?: GPUTextureFormat[];
  colorSpace?: PredefinedColorSpace;
  alphaMode?: GPUCanvasAlphaMode;
}

type GPUImageCopyTexture = {
  texture: GPUTexture;
  mipLevel?: number;
  origin?: GPUOrigin3D;
  aspect?: GPUTextureAspect;
};
type GPUImageCopyBuffer = {
  buffer: GPUBuffer;
  offset?: number;
  bytesPerRow?: number;
  rowsPerImage?: number;
};

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUBufferUsage) */
declare const GPUBufferUsage: {
  readonly MAP_READ: 0x0001;
  readonly MAP_WRITE: 0x0002;
  readonly COPY_SRC: 0x0004;
  readonly COPY_DST: 0x0008;
  readonly INDEX: 0x0010;
  readonly VERTEX: 0x0020;
  readonly UNIFORM: 0x0040;
  readonly STORAGE: 0x0080;
  readonly INDIRECT: 0x0100;
  readonly QUERY_RESOLVE: 0x0200;
};

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUTextureUsage) */
declare const GPUTextureUsage: {
  readonly COPY_SRC: 0x01;
  readonly COPY_DST: 0x02;
  readonly TEXTURE_BINDING: 0x04;
  readonly STORAGE_BINDING: 0x08;
  readonly RENDER_ATTACHMENT: 0x10;
};

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUShaderStage) */
declare const GPUShaderStage: {
  readonly VERTEX: 0x1;
  readonly FRAGMENT: 0x2;
  readonly COMPUTE: 0x4;
};

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUColorWrite) */
declare const GPUColorWrite: {
  readonly RED: 0x1;
  readonly GREEN: 0x2;
  readonly BLUE: 0x4;
  readonly ALPHA: 0x8;
  readonly ALL: 0xf;
};

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUMapMode) */
declare const GPUMapMode: {
  readonly READ: 0x0001;
  readonly WRITE: 0x0002;
};

/**
 * Block length for an audio context.
 *
 * Omitted and `"default"` are 128. An integer from 1 through
 * `floor(6 * sampleRate)` is that exact length. Offline `"hardware"` is 128.
 * Realtime `"hardware"` is the default output device's current buffer size
 * when that size is in range, and 128 otherwise. On macOS the query is Core
 * Audio. Windows and Linux use 128. No playback stream is opened.
 */
type AudioRenderSizeHint = "default" | "hardware" | number;

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioContext/AudioContext)
 */
interface AudioContextOptions {
  sampleRate?: number;
  latencyHint?: AudioContextLatencyCategory | number;
  renderSizeHint?: AudioRenderSizeHint;
  sinkId?: string;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OfflineAudioContext/OfflineAudioContext)
 */
interface OfflineAudioContextOptions {
  numberOfChannels?: number;
  length: number;
  sampleRate: number;
  renderSizeHint?: AudioRenderSizeHint;
}

/**
 * A base audio context.
 *
 * There is no audio output device. The rendered {@link AudioBuffer} is the
 * signal. A new {@link AudioContext} starts `"running"`. There is no
 * secure-context gate, so `audioWorklet` is available. Media-element
 * sources, media-stream nodes, and `ScriptProcessorNode` stay absent.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext)
 */
interface BaseAudioContext extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/destination) */
  readonly destination: AudioDestinationNode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/sampleRate) */
  readonly sampleRate: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/currentTime) */
  readonly currentTime: number;

  /**
   * The block length chosen by `renderSizeHint`.
   *
   * `baseLatency` is this value divided by `sampleRate`.
   */
  readonly renderQuantumSize: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/state) */
  readonly state: AudioContextState;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/statechange_event) */
  onstatechange: ((this: BaseAudioContext, ev: Event) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/audioWorklet) */
  readonly audioWorklet: AudioWorklet;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/listener) */
  readonly listener: AudioListener;

  /**
   * Decodes WAV, FLAC, MP3, Ogg Vorbis, or Ogg Opus.
   *
   * Ogg Opus becomes an `AudioBuffer` at 48000 Hz. The channel count
   * comes from the Opus header.
   *
   * The `ArrayBuffer` is detached. A panic rejects `InvalidStateError`.
   *
   * @throws {@link TypeError} When `audioData` is not an `ArrayBuffer`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/decodeAudioData)
   */
  decodeAudioData(
    audioData: ArrayBuffer,
    successCallback?: DecodeSuccessCallback,
    errorCallback?: DecodeErrorCallback,
  ): Promise<AudioBuffer>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when `numberOfChannels`
   * is outside 1 through 32, `length` is outside 1 through 20000000, or
   * `sampleRate` is outside 3000 through 768000.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createBuffer)
   */
  createBuffer(
    numberOfChannels: number,
    length: number,
    sampleRate: number,
  ): AudioBuffer;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createBufferSource) */
  createBufferSource(): AudioBufferSourceNode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createConstantSource) */
  createConstantSource(): ConstantSourceNode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createGain) */
  createGain(): GainNode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createOscillator) */
  createOscillator(): OscillatorNode;

  /**
   * @throws {@link DOMException} `IndexSizeError` when `real` and `imag` have
   * different lengths or fewer than two coefficients.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createPeriodicWave)
   */
  createPeriodicWave(
    real: Float32Array<ArrayBufferLike>,
    imag: Float32Array<ArrayBufferLike>,
    constraints?: PeriodicWaveConstraints,
  ): PeriodicWave;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createAnalyser) */
  createAnalyser(): AnalyserNode;

  /**
   * @throws {@link DOMException} `NotSupportedError` when `maxDelayTime` is
   * not greater than 0 and less than 180.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createDelay)
   */
  createDelay(maxDelayTime?: number): DelayNode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createBiquadFilter) */
  createBiquadFilter(): BiquadFilterNode;

  /**
   * @throws {@link TypeError} When a coefficient list is missing.
   * @throws {@link DOMException} `NotSupportedError` when either coefficient
   * list length is outside 1 through 20.
   * @throws {@link DOMException} `InvalidStateError` when every feedforward
   * coefficient is 0 or the first feedback coefficient is 0.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createIIRFilter)
   */
  createIIRFilter(feedforward: number[], feedback: number[]): IIRFilterNode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createWaveShaper) */
  createWaveShaper(): WaveShaperNode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createPanner) */
  createPanner(): PannerNode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createStereoPanner) */
  createStereoPanner(): StereoPannerNode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createConvolver) */
  createConvolver(): ConvolverNode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createDynamicsCompressor) */
  createDynamicsCompressor(): DynamicsCompressorNode;

  /**
   * @throws {@link DOMException} `IndexSizeError` when `numberOfInputs` is
   * outside 1 through 32.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createChannelMerger)
   */
  createChannelMerger(numberOfInputs?: number): ChannelMergerNode;

  /**
   * @throws {@link DOMException} `IndexSizeError` when `numberOfOutputs` is
   * outside 1 through 32.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createChannelSplitter)
   */
  createChannelSplitter(numberOfOutputs?: number): ChannelSplitterNode;
}
declare var BaseAudioContext: {
  prototype: BaseAudioContext;
  new(): BaseAudioContext;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createPeriodicWave) */
interface PeriodicWaveConstraints {
  disableNormalization?: boolean;
}

/**
 * A realtime context.
 *
 * `currentTime` stays 0 until a source starts. Playback then opens the output
 * device named by `sinkId`, or the default device when `sinkId` is `""`, and
 * keeps rendering until `suspend` or `close`, which keeps the process alive.
 * The device callback only copies a buffer. When the device cannot be opened,
 * a timer advances the clock. `baseLatency` is `renderQuantumSize / sampleRate`.
 * With a device, `outputLatency` is the 100 ms playback buffer. On the timer
 * it is 0. `setSinkId` is absent.
 *
 * @throws {@link DOMException} `NotSupportedError` when `sampleRate` is outside
 * 3000 through 768000, or when `renderSizeHint` is an integer outside 1 through
 * `floor(6 * sampleRate)`.
 * @throws {@link DOMException} `NotFoundError` when `sinkId` names an output
 * device that is not present. `""` does not require a device.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioContext)
 */
interface AudioContext extends BaseAudioContext {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioContext/baseLatency) */
  readonly baseLatency: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioContext/outputLatency) */
  readonly outputLatency: number;

  /**
   * The requested output-device name. `""` is the default device.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioContext/sinkId)
   */
  readonly sinkId: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioContext/suspend) */
  suspend(): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioContext/resume) */
  resume(): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioContext/close) */
  close(): Promise<void>;
}
declare var AudioContext: {
  prototype: AudioContext;
  new(contextOptions?: AudioContextOptions): AudioContext;
};


/**
 * An offline context.
 *
 * The three-argument constructor uses a quantum of 128. `"hardware"` is 128.
 *
 * @throws {@link DOMException} `NotSupportedError` for the same channel,
 * length, rate, and `renderSizeHint` limits as {@link BaseAudioContext.createBuffer}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OfflineAudioContext)
 */
interface OfflineAudioContext extends BaseAudioContext {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/OfflineAudioContext/length) */
  readonly length: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/OfflineAudioContext/startRendering) */
  startRendering(): Promise<AudioBuffer>;

  /**
   * Pauses at a render quantum boundary.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OfflineAudioContext/suspend)
   */
  suspend(suspendTime: number): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/OfflineAudioContext/resume) */
  resume(): Promise<void>;
}
declare var OfflineAudioContext: {
  prototype: OfflineAudioContext;
  new(contextOptions: OfflineAudioContextOptions): OfflineAudioContext;
  new(numberOfChannels: number, length: number, sampleRate: number): OfflineAudioContext;
};


/**
 * PCM samples.
 *
 * @throws {@link DOMException} `NotSupportedError` when the channel count,
 * length, or sample rate is outside the limits documented on
 * {@link BaseAudioContext.createBuffer}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBuffer)
 */
interface AudioBuffer {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBuffer/sampleRate) */
  readonly sampleRate: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBuffer/length) */
  readonly length: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBuffer/duration) */
  readonly duration: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBuffer/numberOfChannels) */
  readonly numberOfChannels: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBuffer/getChannelData) */
  getChannelData(channel: number): Float32Array<ArrayBuffer>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBuffer/copyFromChannel) */
  copyFromChannel(destination: Float32Array<ArrayBuffer>, channelNumber: number, bufferOffset?: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBuffer/copyToChannel) */
  copyToChannel(source: Float32Array<ArrayBuffer>, channelNumber: number, bufferOffset?: number): void;
}
declare var AudioBuffer: {
  prototype: AudioBuffer;
  new(options: AudioBufferOptions): AudioBuffer;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBuffer/AudioBuffer) */
interface AudioBufferOptions {
  numberOfChannels?: number;
  length: number;
  sampleRate: number;
}

/**
 * An audio parameter.
 *
 * Reading `value` returns the timeline at `currentTime`, clamped to `minValue`
 * and `maxValue`. That read does not include samples connected to the
 * parameter. `value`, `defaultValue`, `minValue`, and `maxValue` are
 * single-precision floats. Assigning `value` sets the intrinsic value and inserts
 * `setValueAtTime` at `currentTime`. `NaN` is ignored. An event at the same
 * time replaces the previous event. A ramp with no earlier event throws
 * `InvalidStateError`. Each method returns this parameter.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam)
 */
interface AudioParam {

  /**
   * The computed value at `currentTime`. Audio-rate inputs are not included.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/value)
   */
  value: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/defaultValue) */
  readonly defaultValue: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/minValue) */
  readonly minValue: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/maxValue) */
  readonly maxValue: number;

  /**
   * `"a-rate"` or `"k-rate"`.
   *
   * @throws {@link TypeError} When the assigned value is neither.
   * @throws {@link DOMException} `InvalidStateError` when this parameter is
   * k-rate only and the value is `"a-rate"`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/automationRate)
   */
  automationRate: AutomationRate;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/setValueAtTime) */
  setValueAtTime(value: number, startTime: number): AudioParam;

  /**
   * Throws `InvalidStateError` when no event is strictly earlier than `endTime`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/linearRampToValueAtTime)
   */
  linearRampToValueAtTime(value: number, endTime: number): AudioParam;

  /**
   * Throws `RangeError` when `value` is not positive or the previous value is
   * not positive, and `InvalidStateError` when no event is strictly earlier.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/exponentialRampToValueAtTime)
   */
  exponentialRampToValueAtTime(value: number, endTime: number): AudioParam;

  /**
   * Throws `RangeError` when `timeConstant` is not positive.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/setTargetAtTime)
   */
  setTargetAtTime(target: number, startTime: number, timeConstant: number): AudioParam;

  /**
   * Throws `InvalidStateError` when `values` has fewer than two samples or the
   * curve overlaps an event. Throws `RangeError` when `duration` is not positive.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/setValueCurveAtTime)
   */
  setValueCurveAtTime(values: Iterable<number>, startTime: number, duration: number): AudioParam;

  /**
   * Removes events at or after `cancelTime`. An in-progress curve stays.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/cancelScheduledValues)
   */
  cancelScheduledValues(cancelTime: number): AudioParam;

  /**
   * Holds the value at `cancelTime`. A following ramp is shortened to that time.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/cancelAndHoldAtTime)
   */
  cancelAndHoldAtTime(cancelTime: number): AudioParam;
}
declare var AudioParam: {
  prototype: AudioParam;
  new(): AudioParam;
};


/**
 * Named parameters on an {@link AudioWorkletNode}.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParamMap)
 */
interface AudioParamMap {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParamMap/size) */
  readonly size: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParamMap/get) */
  get(name: string): AudioParam | undefined;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParamMap/has) */
  has(name: string): boolean;
}
declare var AudioParamMap: {
  prototype: AudioParamMap;
  new(): AudioParamMap;
};


/**
 * A node in the audio graph.
 *
 * `connect` to an {@link AudioParam} returns `undefined`. Only output 0 of
 * that connection contributes samples, summed into one channel.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode)
 */
interface AudioNode extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/context) */
  readonly context: BaseAudioContext;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/numberOfInputs) */
  readonly numberOfInputs: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/numberOfOutputs) */
  readonly numberOfOutputs: number;

  /**
   * @throws {@link DOMException} `NotSupportedError` when the count is 0 or
   * greater than 32, or when this node cannot use that count.
   * @throws {@link DOMException} `IndexSizeError` when the count exceeds the
   * node's maximum and that node reports the limit as an index error.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/channelCount)
   */
  channelCount: number;

  /**
   * An unrecognized value leaves the previous mode.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/channelCountMode)
   */
  channelCountMode: ChannelCountMode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/channelInterpretation) */
  channelInterpretation: ChannelInterpretation;

  /**
   * @throws {@link DOMException} `InvalidAccessError` when the parameter
   * belongs to a different context.
   * @throws {@link DOMException} `IndexSizeError` when `output` is out of range.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/connect)
   */
  connect(destinationNode: AudioNode, output?: number, input?: number): AudioNode;
  connect(destinationParam: AudioParam, output?: number): void;

  /**
   * @throws {@link DOMException} `InvalidAccessError` when this node is not
   * connected to `destinationParam`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/disconnect)
   */
  disconnect(): void;
  disconnect(output: number): void;
  disconnect(destinationNode: AudioNode): void;
  disconnect(destinationNode: AudioNode, output: number): void;
  disconnect(destinationNode: AudioNode, output: number, input: number): void;
  disconnect(destinationParam: AudioParam): void;
}
declare var AudioNode: {
  prototype: AudioNode;
  new(): AudioNode;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDestinationNode)
 */
interface AudioDestinationNode extends AudioNode {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDestinationNode/maxChannelCount) */
  readonly maxChannelCount: number;
}
declare var AudioDestinationNode: {
  prototype: AudioDestinationNode;
  new(): AudioDestinationNode;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioScheduledSourceNode)
 */
interface AudioScheduledSourceNode extends AudioNode {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioScheduledSourceNode/start) */
  start(when?: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioScheduledSourceNode/stop) */
  stop(when?: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioScheduledSourceNode/ended_event) */
  onended: ((this: AudioScheduledSourceNode, ev: Event) => any) | null;
}
declare var AudioScheduledSourceNode: {
  prototype: AudioScheduledSourceNode;
  new(): AudioScheduledSourceNode;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GainNode)
 */
interface GainNode extends AudioNode {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GainNode/gain) */
  readonly gain: AudioParam;
}
declare var GainNode: {
  prototype: GainNode;
  new(context: BaseAudioContext, options?: GainOptions): GainNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GainNode/GainNode) */
interface GainOptions extends AudioNodeOptions {
  gain?: number;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/AudioNode) */
interface AudioNodeOptions {
  channelCount?: number;
  channelCountMode?: ChannelCountMode;
  channelInterpretation?: ChannelInterpretation;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ConstantSourceNode)
 */
interface ConstantSourceNode extends AudioScheduledSourceNode {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ConstantSourceNode/offset) */
  readonly offset: AudioParam;
}
declare var ConstantSourceNode: {
  prototype: ConstantSourceNode;
  new(context: BaseAudioContext, options?: ConstantSourceOptions): ConstantSourceNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ConstantSourceNode/ConstantSourceNode) */
interface ConstantSourceOptions {
  offset?: number;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/OscillatorNode)
 */
interface OscillatorNode extends AudioScheduledSourceNode {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/OscillatorNode/type) */
  type: OscillatorType;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/OscillatorNode/frequency) */
  readonly frequency: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/OscillatorNode/detune) */
  readonly detune: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/OscillatorNode/setPeriodicWave) */
  setPeriodicWave(periodicWave: PeriodicWave): void;
}
declare var OscillatorNode: {
  prototype: OscillatorNode;
  new(context: BaseAudioContext, options?: OscillatorOptions): OscillatorNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/OscillatorNode/OscillatorNode) */
interface OscillatorOptions extends AudioNodeOptions {
  type?: OscillatorType;
  frequency?: number;
  detune?: number;
  periodicWave?: PeriodicWave;
}

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBufferSourceNode)
 */
interface AudioBufferSourceNode extends AudioScheduledSourceNode {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBufferSourceNode/buffer) */
  buffer: AudioBuffer | null;

  /**
   * k-rate. Setting `automationRate` to `"a-rate"` throws `InvalidStateError`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBufferSourceNode/playbackRate)
   */
  readonly playbackRate: AudioParam;

  /**
   * k-rate. Setting `automationRate` to `"a-rate"` throws `InvalidStateError`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBufferSourceNode/detune)
   */
  readonly detune: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBufferSourceNode/loop) */
  loop: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBufferSourceNode/loopStart) */
  loopStart: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBufferSourceNode/loopEnd) */
  loopEnd: number;
}
declare var AudioBufferSourceNode: {
  prototype: AudioBufferSourceNode;
  new(context: BaseAudioContext, options?: AudioBufferSourceOptions): AudioBufferSourceNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBufferSourceNode/AudioBufferSourceNode) */
interface AudioBufferSourceOptions {
  buffer?: AudioBuffer | null;
  detune?: number;
  loop?: boolean;
  loopEnd?: number;
  loopStart?: number;
  playbackRate?: number;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PeriodicWave/PeriodicWave) */
interface PeriodicWaveOptions extends PeriodicWaveConstraints {
  real?: number[] | Float32Array<ArrayBufferLike>;
  imag?: number[] | Float32Array<ArrayBufferLike>;
}

/**
 * Loads worklet processors.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioWorklet)
 */
interface AudioWorklet extends EventTarget {
  /**
   * Exchanges the values {@link structuredClone} can carry, including a
   * transferred {@link AudioData} or {@link VideoFrame}. The processor realm
   * does not define those constructors. Methods on the received object still
   * run.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioWorklet/port)
   */
  readonly port: MessagePort;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioWorklet/addModule)
   */
  addModule(moduleURL: string, options?: WorkletOptions): Promise<void>;
}
declare var AudioWorklet: {
  prototype: AudioWorklet;
  new(): AudioWorklet;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Worklet/addModule) */
interface WorkletOptions {
  credentials?: RequestCredentials;
}

/**
 * A node implemented by `registerProcessor`.
 *
 * `process` sees `Float32Array` channels of length `renderQuantumSize`.
 * An empty processor name throws `NotSupportedError`. A duplicate
 * `registerProcessor` name throws `NotSupportedError`. A parameter descriptor
 * with an empty or duplicate name, or a default outside its range, throws
 * `NotSupportedError`.
 *
 * @throws {@link DOMException} `NotSupportedError` when `name` is empty or
 * `outputChannelCount` contains 0 or a count above 32.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioWorkletNode)
 */
interface AudioWorkletNode extends AudioNode {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioWorkletNode/port) */
  readonly port: MessagePort;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioWorkletNode/parameters) */
  readonly parameters: AudioParamMap;
}
declare var AudioWorkletNode: {
  prototype: AudioWorkletNode;
  new(context: BaseAudioContext, name: string, options?: AudioWorkletNodeOptions): AudioWorkletNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioWorkletNode/AudioWorkletNode) */
interface AudioWorkletNodeOptions extends AudioNodeOptions {
  numberOfInputs?: number;
  numberOfOutputs?: number;
  outputChannelCount?: number[];
  parameterData?: Record<string, number>;
  processorOptions?: any;
}

/**
 * The listener a `PannerNode` spatializes against.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener)
 */
interface AudioListener {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener/positionX) */
  readonly positionX: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener/positionY) */
  readonly positionY: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener/positionZ) */
  readonly positionZ: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener/forwardX) */
  readonly forwardX: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener/forwardY) */
  readonly forwardY: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener/forwardZ) */
  readonly forwardZ: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener/upX) */
  readonly upX: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener/upY) */
  readonly upY: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener/upZ) */
  readonly upZ: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener/setPosition) */
  setPosition(x: number, y: number, z: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioListener/setOrientation) */
  setOrientation(x: number, y: number, z: number, xUp: number, yUp: number, zUp: number): void;
}
declare var AudioListener: {
  prototype: AudioListener;
  new(): AudioListener;
};


/**
 * Delays the input by `delayTime` seconds.
 *
 * @throws {@link DOMException} `NotSupportedError` when `maxDelayTime` is not
 * greater than 0 and less than 180.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DelayNode)
 */
interface DelayNode extends AudioNode {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DelayNode/delayTime) */
  readonly delayTime: AudioParam;
}
declare var DelayNode: {
  prototype: DelayNode;
  new(context: BaseAudioContext, options?: DelayOptions): DelayNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DelayNode/DelayNode) */
interface DelayOptions extends AudioNodeOptions {
  delayTime?: number;
  maxDelayTime?: number;
}

/**
 * An unrecognized `type` in the constructor dictionary throws `TypeError`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/BiquadFilterNode)
 */
interface BiquadFilterNode extends AudioNode {
  /**
   * An unrecognized value leaves the previous type.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/BiquadFilterNode/type)
   */
  type: BiquadFilterType;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BiquadFilterNode/frequency) */
  readonly frequency: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BiquadFilterNode/detune) */
  readonly detune: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BiquadFilterNode/Q) */
  readonly Q: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BiquadFilterNode/gain) */
  readonly gain: AudioParam;

  /**
   * @throws {@link DOMException} `InvalidAccessError` when the arrays have
   * different lengths.
   * @throws {@link TypeError} When an argument is not a `Float32Array`, or a
   * `Float32Array` is shared.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/BiquadFilterNode/getFrequencyResponse)
   */
  getFrequencyResponse(frequencyHz: Float32Array<ArrayBuffer>, magResponse: Float32Array<ArrayBuffer>, phaseResponse: Float32Array<ArrayBuffer>): void;
}
declare var BiquadFilterNode: {
  prototype: BiquadFilterNode;
  new(context: BaseAudioContext, options?: BiquadFilterOptions): BiquadFilterNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BiquadFilterNode/BiquadFilterNode) */
interface BiquadFilterOptions extends AudioNodeOptions {
  type?: BiquadFilterType;
  frequency?: number;
  detune?: number;
  Q?: number;
  gain?: number;
}

/**
 * @throws {@link TypeError} When `feedforward` or `feedback` is missing.
 * @throws {@link DOMException} `NotSupportedError` when a coefficient list
 * length is outside 1 through 20.
 * @throws {@link DOMException} `InvalidStateError` when every feedforward
 * coefficient is 0 or the first feedback coefficient is 0.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/IIRFilterNode)
 */
interface IIRFilterNode extends AudioNode {
  /**
   * @throws {@link DOMException} `InvalidAccessError` when the arrays have
   * different lengths.
   * @throws {@link TypeError} When an argument is not a `Float32Array`, or a
   * `Float32Array` is shared.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/IIRFilterNode/getFrequencyResponse)
   */
  getFrequencyResponse(frequencyHz: Float32Array<ArrayBuffer>, magResponse: Float32Array<ArrayBuffer>, phaseResponse: Float32Array<ArrayBuffer>): void;
}
declare var IIRFilterNode: {
  prototype: IIRFilterNode;
  new(context: BaseAudioContext, options: IIRFilterOptions): IIRFilterNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/IIRFilterNode/IIRFilterNode) */
interface IIRFilterOptions extends AudioNodeOptions {
  feedforward: number[];
  feedback: number[];
}

/**
 * Oversampling interpolates from the previous sample, shapes each step, and
 * averages back down. It does not use a half-band filter. An unrecognized
 * `oversample` in the constructor dictionary throws `TypeError`.
 *
 * @throws {@link DOMException} `InvalidStateError` when `curve` has fewer
 * than two values.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WaveShaperNode)
 */
interface WaveShaperNode extends AudioNode {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WaveShaperNode/curve) */
  curve: Float32Array<ArrayBuffer> | null;

  /**
   * An unrecognized value leaves the previous mode.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WaveShaperNode/oversample)
   */
  oversample: OverSampleType;
}
declare var WaveShaperNode: {
  prototype: WaveShaperNode;
  new(context: BaseAudioContext, options?: WaveShaperOptions): WaveShaperNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WaveShaperNode/WaveShaperNode) */
interface WaveShaperOptions extends AudioNodeOptions {
  curve?: number[] | Float32Array<ArrayBufferLike>;
  oversample?: OverSampleType;
}

/**
 * Equal-power stereo pan. The output is stereo.
 *
 * @throws {@link DOMException} `NotSupportedError` when `channelCount` is
 * above 2 or `channelCountMode` is `"max"`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/StereoPannerNode)
 */
interface StereoPannerNode extends AudioNode {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/StereoPannerNode/pan) */
  readonly pan: AudioParam;
}
declare var StereoPannerNode: {
  prototype: StereoPannerNode;
  new(context: BaseAudioContext, options?: StereoPannerOptions): StereoPannerNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/StereoPannerNode/StereoPannerNode) */
interface StereoPannerOptions extends AudioNodeOptions {
  pan?: number;
}

/**
 * Equal-power panner. `"HRTF"` throws `NotSupportedError`. The output is
 * stereo. The cone points from the source toward the listener.
 *
 * @throws {@link DOMException} `NotSupportedError` when `panningModel` is
 * `"HRTF"`, `channelCount` is above 2, or `channelCountMode` is `"max"`.
 * @throws {@link TypeError} When `panningModel` or `distanceModel` is not a
 * supported value.
 * @throws {@link RangeError} When `refDistance` is negative, `maxDistance` is
 * not positive, or `rolloffFactor` is negative.
 * @throws {@link DOMException} `InvalidStateError` when `coneOuterGain` is
 * outside 0 through 1.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode)
 */
interface PannerNode extends AudioNode {
  /**
   * @throws {@link DOMException} `NotSupportedError` when the value is `"HRTF"`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/panningModel)
   */
  panningModel: PanningModelType;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/positionX) */
  readonly positionX: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/positionY) */
  readonly positionY: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/positionZ) */
  readonly positionZ: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/orientationX) */
  readonly orientationX: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/orientationY) */
  readonly orientationY: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/orientationZ) */
  readonly orientationZ: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/distanceModel) */
  distanceModel: DistanceModelType;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/refDistance) */
  refDistance: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/maxDistance) */
  maxDistance: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/rolloffFactor) */
  rolloffFactor: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/coneInnerAngle) */
  coneInnerAngle: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/coneOuterAngle) */
  coneOuterAngle: number;

  /**
   * @throws {@link DOMException} `InvalidStateError` when the value is outside
   * 0 through 1.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/coneOuterGain)
   */
  coneOuterGain: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/setPosition) */
  setPosition(x: number, y: number, z: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/setOrientation) */
  setOrientation(x: number, y: number, z: number): void;
}
declare var PannerNode: {
  prototype: PannerNode;
  new(context: BaseAudioContext, options?: PannerOptions): PannerNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/PannerNode/PannerNode) */
interface PannerOptions extends AudioNodeOptions {
  panningModel?: PanningModelType;
  distanceModel?: DistanceModelType;
  positionX?: number;
  positionY?: number;
  positionZ?: number;
  orientationX?: number;
  orientationY?: number;
  orientationZ?: number;
  refDistance?: number;
  maxDistance?: number;
  rolloffFactor?: number;
  coneInnerAngle?: number;
  coneOuterAngle?: number;
  coneOuterGain?: number;
}

/**
 * Convolution. A buffer whose sample rate differs from the context is
 * linearly resampled. Changing `normalize` does not rescale until `buffer`
 * is set again. `buffer` returns the same object that was assigned.
 *
 * @throws {@link DOMException} `NotSupportedError` when `channelCount` is
 * above 2, `channelCountMode` is `"max"`, or the impulse response has a
 * channel count other than 1, 2, or 4.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ConvolverNode)
 */
interface ConvolverNode extends AudioNode {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ConvolverNode/buffer) */
  buffer: AudioBuffer | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ConvolverNode/normalize) */
  normalize: boolean;
}
declare var ConvolverNode: {
  prototype: ConvolverNode;
  new(context: BaseAudioContext, options?: ConvolverOptions): ConvolverNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ConvolverNode/ConvolverNode) */
interface ConvolverOptions extends AudioNodeOptions {
  buffer?: AudioBuffer | null;
  disableNormalization?: boolean;
}

/**
 * Dynamics compression without look-ahead. `threshold`, `knee`, `ratio`,
 * `attack`, `release`, and `makeupGain` are k-rate. `"automatic"` locks
 * `makeupGain`. `reduction` is the last sample of the quantum, in decibels,
 * and includes makeup gain.
 *
 * @throws {@link DOMException} `NotSupportedError` when `channelCount` is
 * above 2 or `channelCountMode` is `"max"`.
 * @throws {@link TypeError} When `makeupGain` is neither a number nor
 * `"automatic"`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DynamicsCompressorNode)
 */
interface DynamicsCompressorNode extends AudioNode {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DynamicsCompressorNode/threshold) */
  readonly threshold: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DynamicsCompressorNode/knee) */
  readonly knee: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DynamicsCompressorNode/ratio) */
  readonly ratio: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DynamicsCompressorNode/attack) */
  readonly attack: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DynamicsCompressorNode/release) */
  readonly release: AudioParam;

  /**
   * @throws {@link DOMException} `InvalidStateError` when the parameter is
   * automatic and a script modifies it.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DynamicsCompressorNode/makeupGain)
   */
  readonly makeupGain: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DynamicsCompressorNode/reduction) */
  readonly reduction: number;
}
declare var DynamicsCompressorNode: {
  prototype: DynamicsCompressorNode;
  new(context: BaseAudioContext, options?: DynamicsCompressorOptions): DynamicsCompressorNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DynamicsCompressorNode/DynamicsCompressorNode) */
interface DynamicsCompressorOptions extends AudioNodeOptions {
  threshold?: number;
  knee?: number;
  ratio?: number;
  attack?: number;
  release?: number;
  makeupGain?: number | "automatic";
}

/**
 * Pass-through analysis. Time-domain data is a speakers down-mix to mono.
 * An analyser that is not upstream of the destination is still rendered.
 *
 * @throws {@link DOMException} `IndexSizeError` when `fftSize` is not a power
 * of two from 32 through 32768, `minDecibels` is not below `maxDecibels`, or
 * `smoothingTimeConstant` is outside 0 through 1.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AnalyserNode)
 */
interface AnalyserNode extends AudioNode {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AnalyserNode/fftSize) */
  fftSize: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AnalyserNode/frequencyBinCount) */
  readonly frequencyBinCount: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AnalyserNode/minDecibels) */
  minDecibels: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AnalyserNode/maxDecibels) */
  maxDecibels: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AnalyserNode/smoothingTimeConstant) */
  smoothingTimeConstant: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AnalyserNode/getFloatTimeDomainData) */
  getFloatTimeDomainData(array: Float32Array<ArrayBuffer>): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AnalyserNode/getByteTimeDomainData) */
  getByteTimeDomainData(array: Uint8Array<ArrayBuffer>): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AnalyserNode/getFloatFrequencyData) */
  getFloatFrequencyData(array: Float32Array<ArrayBuffer>): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AnalyserNode/getByteFrequencyData) */
  getByteFrequencyData(array: Uint8Array<ArrayBuffer>): void;
}
declare var AnalyserNode: {
  prototype: AnalyserNode;
  new(context: BaseAudioContext, options?: AnalyserOptions): AnalyserNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AnalyserNode/AnalyserNode) */
interface AnalyserOptions extends AudioNodeOptions {
  fftSize?: number;
  minDecibels?: number;
  maxDecibels?: number;
  smoothingTimeConstant?: number;
}

/**
 * One mono input per output channel. `channelCount` stays 1 and
 * `channelCountMode` stays `"explicit"`.
 *
 * @throws {@link DOMException} `IndexSizeError` when `numberOfInputs` is
 * outside 1 through 32.
 * @throws {@link DOMException} `InvalidStateError` when `channelCount` is not
 * 1 or `channelCountMode` is not `"explicit"`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ChannelMergerNode)
 */
interface ChannelMergerNode extends AudioNode {}
declare var ChannelMergerNode: {
  prototype: ChannelMergerNode;
  new(context: BaseAudioContext, options?: ChannelMergerOptions): ChannelMergerNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ChannelMergerNode/ChannelMergerNode) */
interface ChannelMergerOptions extends AudioNodeOptions {
  numberOfInputs?: number;
}

/**
 * Splits each input channel into its own mono output. `channelCount` stays
 * equal to `numberOfOutputs`, `channelCountMode` stays `"explicit"`, and
 * `channelInterpretation` stays `"discrete"`.
 *
 * @throws {@link DOMException} `IndexSizeError` when `numberOfOutputs` is
 * outside 1 through 32.
 * @throws {@link DOMException} `InvalidStateError` when `channelCount`,
 * `channelCountMode`, or `channelInterpretation` differs from those locked
 * values.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ChannelSplitterNode)
 */
interface ChannelSplitterNode extends AudioNode {}
declare var ChannelSplitterNode: {
  prototype: ChannelSplitterNode;
  new(context: BaseAudioContext, options?: ChannelSplitterOptions): ChannelSplitterNode;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ChannelSplitterNode/ChannelSplitterNode) */
interface ChannelSplitterOptions extends AudioNodeOptions {
  numberOfOutputs?: number;
}

/**
 * Color space of a {@link VideoFrame}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoColorSpace)
 */
interface VideoColorSpace {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoColorSpace/fullRange) */
  readonly fullRange: boolean | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoColorSpace/matrix) */
  readonly matrix: VideoMatrixCoefficients | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoColorSpace/primaries) */
  readonly primaries: VideoColorPrimaries | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoColorSpace/transfer) */
  readonly transfer: VideoTransferCharacteristics | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoColorSpace/toJSON) */
  toJSON(): VideoColorSpaceInit;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoColorSpace/VideoColorSpace) */
interface VideoColorSpaceInit {
  primaries?: VideoColorPrimaries | null;
  transfer?: VideoTransferCharacteristics | null;
  matrix?: VideoMatrixCoefficients | null;
  fullRange?: boolean | null;
}
declare var VideoColorSpace: {
  prototype: VideoColorSpace;
  new(init?: VideoColorSpaceInit): VideoColorSpace;
};

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/VideoFrame) */
interface VideoFrameInit {
  duration?: number | null;
  timestamp?: number;
  alpha?: AlphaOption;
  visibleRect?: DOMRectInit;
  displayWidth?: number;
  displayHeight?: number;
  /**
   * @throws {@link DOMException} `NotSupportedError` when the value is not 0.
   */
  rotation?: number;
  /**
   * @throws {@link DOMException} `NotSupportedError` when the value is true.
   */
  flip?: boolean;
}

/**
 * Byte layout of one plane.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/PlaneLayout)
 */
interface PlaneLayout {
  offset: number;
  stride: number;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/VideoFrame) */
interface VideoFrameBufferInit {
  format: VideoPixelFormat;
  codedWidth: number;
  codedHeight: number;
  timestamp: number;
  duration?: number | null;
  layout?: PlaneLayout[];
  visibleRect?: DOMRectInit;
  displayWidth?: number;
  displayHeight?: number;
  colorSpace?: VideoColorSpaceInit;
  /**
   * @throws {@link DOMException} `NotSupportedError` when the value is not 0.
   */
  rotation?: number;
  /**
   * @throws {@link DOMException} `NotSupportedError` when the value is true.
   */
  flip?: boolean;
}

/**
 * Options for {@link VideoFrame.copyTo}.
 *
 * `format` may be the source format or `RGBA`, `BGRA`, `RGBX`, or `BGRX`.
 * Another format rejects `NotSupportedError`. An RGB `format` is written in
 * `colorSpace`, which defaults to `srgb`. `display-p3` converts from the
 * frame's sRGB or Display P3 samples. `colorSpace` is ignored otherwise.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/copyTo)
 */
interface VideoFrameCopyToOptions {
  rect?: DOMRectInit;
  layout?: PlaneLayout[];
  format?: VideoPixelFormat;
  colorSpace?: PredefinedColorSpace;
}

/**
 * A video frame.
 *
 * `new VideoFrame(image, init)` copies an `ImageBitmap`, an `OffscreenCanvas`,
 * or another `VideoFrame`. `new VideoFrame(buffer, init)` copies an 8-bit
 * buffer. `timestamp` is required unless the source is a `VideoFrame`.
 * `drawImage`, `createImageBitmap`, and `copyExternalImageToTexture` accept a
 * frame. `close()` releases it. A second `close()` does nothing. After
 * `close()`, `format` is null, the sizes are 0, `visibleRect` and `codedRect`
 * are null, and `colorSpace` members are null. `timestamp` and `duration`
 * stay. `clone`, `allocationSize`, `copyTo`, `drawImage`, and
 * `createImageBitmap` then throw `InvalidStateError`.
 * `structuredClone` and `postMessage` copy the frame and leave it open.
 * Listing it in a transfer list closes it, and the receiver gets the pixels.
 * `drawImage` uses those pixels at their pixel size.
 *
 * @throws {@link TypeError} when an argument is missing or invalid.
 * @throws {@link DOMException} `NotSupportedError` for a 10-bit or 12-bit
 * pixel format, or for a non-zero `rotation` or `flip: true`.
 * @throws {@link DOMException} `InvalidStateError` when the source image or
 * canvas is closed or has no pixels.
 * @throws {@link DOMException} `DataCloneError` when a closed frame is cloned.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame)
 */
interface VideoFrame {

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/format)
   */
  readonly format: VideoPixelFormat | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/colorSpace) */
  readonly colorSpace: VideoColorSpace;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/codedWidth) */
  readonly codedWidth: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/codedHeight) */
  readonly codedHeight: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/displayWidth) */
  readonly displayWidth: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/displayHeight) */
  readonly displayHeight: number;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/visibleRect)
   */
  readonly visibleRect: DOMRectReadOnly | null;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/codedRect)
   */
  readonly codedRect: DOMRectReadOnly | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/timestamp) */
  readonly timestamp: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/duration) */
  readonly duration: number | null;

  /**
   * @throws {@link DOMException} `InvalidStateError` when the frame is closed.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/allocationSize)
   */
  allocationSize(options?: VideoFrameCopyToOptions): number;

  /**
   * @throws {@link DOMException} `NotSupportedError` when `options.format` is
   * not the source format and not an RGB format.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/copyTo)
   */
  copyTo(destination: AllowSharedBufferSource, options?: VideoFrameCopyToOptions): Promise<PlaneLayout[]>;

  /**
   * @throws {@link DOMException} `InvalidStateError` when the frame is closed.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/clone)
   */
  clone(): VideoFrame;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/close) */
  close(): void;
}
declare var VideoFrame: {
  prototype: VideoFrame;
  new(image: CanvasImageSource, init?: VideoFrameInit): VideoFrame;
  new(data: AllowSharedBufferSource, init: VideoFrameBufferInit): VideoFrame;
};


/**
 * An encoded video chunk.
 *
 * `structuredClone` and `postMessage` copy `type`, `timestamp`, `duration`,
 * and the bytes. The chunk is not transferable.
 *
 * @throws {@link DOMException} `DataCloneError` when the chunk is listed for
 * transfer.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedVideoChunk)
 */
interface EncodedVideoChunk {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedVideoChunk/type) */
  readonly type: EncodedVideoChunkType;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedVideoChunk/timestamp) */
  readonly timestamp: number;

  /**
   * Microseconds. Missing or null is null. A non-negative finite number is
   * truncated toward 0.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedVideoChunk/duration)
   */
  readonly duration: number | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedVideoChunk/byteLength) */
  readonly byteLength: number;

  /**
   * Copies the encoded bytes into `destination`.
   *
   * @throws {@link TypeError} when `destination` is detached or shorter than
   * `byteLength`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedVideoChunk/copyTo)
   */
  copyTo(destination: AllowSharedBufferSource): void;
}
declare var EncodedVideoChunk: {
  prototype: EncodedVideoChunk;
  new(init: EncodedVideoChunkInit): EncodedVideoChunk;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedVideoChunk/EncodedVideoChunk) */
interface EncodedVideoChunkInit {
  type: EncodedVideoChunkType;
  timestamp: number;
  duration?: number;
  data: AllowSharedBufferSource;
}

/**
 * Decodes H.264 elementary chunks.
 *
 * `avc1` and `avc3` are the only video codecs. The decoder is hardware-only:
 * VideoToolbox on Darwin, the Media Foundation transform decoding through
 * DXVA on Windows, and VA-API on Linux. There is no software decoder.
 * `hardwareAcceleration: "prefer-software"` does not select one. `vp8`,
 * `vp09`, `av01`, and every other codec string fail `configure`.
 * `VideoEncoder` is absent.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder)
 */
interface VideoDecoder extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/state) */
  readonly state: CodecState;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/decodeQueueSize) */
  readonly decodeQueueSize: number;

  /**
   * @throws {@link TypeError} when `config` is missing a required member,
   * `codec` is empty, `description` is detached, or a present dimension is 0.
   * @throws {@link DOMException} `NotSupportedError` when `codec` is not
   * `avc1` or `avc3`, or when the hardware decoder is missing or rejects a
   * structurally valid `avcC`. On Windows the decoder is missing when no
   * Direct3D 11 device offers the H.264 decoder profile. On Linux it is
   * missing when VA-API has no H.264 profile. No frame is produced.
   * @throws {@link DOMException} `InvalidStateError` when the decoder is closed.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/configure)
   */
  configure(config: VideoDecoderConfig): void;

  /**
   * Returns before `output` or `error` runs. Those callbacks are microtasks.
   * A sample the decoder rejects closes the decoder and reports
   * `EncodingError` to `error`. This method does not throw `DataError`.
   *
   * @throws {@link DOMException} `InvalidStateError` when the decoder is not
   * configured.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/decode)
   */
  decode(chunk: EncodedVideoChunk): void;

  /**
   * Resolves without waiting for another hardware picture. Frames `decode`
   * already produced are delivered before this promise settles.
   *
   * @throws {@link DOMException} `InvalidStateError` when the decoder is not
   * configured.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/flush)
   */
  flush(): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/reset) */
  reset(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/close) */
  close(): void;
}
declare var VideoDecoder: {
  prototype: VideoDecoder;
  new(init: VideoDecoderInit): VideoDecoder;
  /**
   * Resolves `{ supported, config }` without opening a decoder and without
   * producing a frame. `supported` is true only for an `avc1` or `avc3`
   * codec string that `configure` accepts, and false for any other codec
   * string. An invalid config rejects `TypeError`. Too few arguments reject
   * `TypeError`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/isConfigSupported)
   */
  isConfigSupported(config: VideoDecoderConfig): Promise<VideoDecoderSupport>;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/VideoDecoder) */
interface VideoDecoderInit {
  output: (frame: VideoFrame) => void;
  error: (error: DOMException) => void;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/configure) */
interface VideoDecoderConfig {
  codec: string;
  description?: AllowSharedBufferSource;
  codedWidth?: number;
  codedHeight?: number;
  hardwareAcceleration?: HardwareAcceleration;
  optimizeForLatency?: boolean;
}

/**
 * Decoded or constructed audio samples.
 *
 * `new AudioData(init)` copies `u8`, `s16`, `s32`, or `f32` samples,
 * interleaved or planar. `sampleRate`, `numberOfFrames`, and
 * `numberOfChannels` must be greater than 0. At most 32 channels and
 * 16,777,216 frames are stored. `timestamp` is a microsecond count and may
 * be negative. `duration` is `numberOfFrames / sampleRate * 1_000_000`,
 * truncated toward 0. `close()` may be called more than once. After
 * `close()`, `format` is null, and `sampleRate`, `numberOfFrames`,
 * `numberOfChannels`, and `duration` are 0. `timestamp` stays. `clone`,
 * `allocationSize`, and `copyTo` then throw `InvalidStateError`.
 * `structuredClone` and `postMessage` copy the samples and leave this object
 * open. Listing it in a transfer list closes it. `transfer` in `init` does
 * not detach the listed buffers.
 *
 * @throws {@link TypeError} when an argument is missing or invalid.
 * @throws {@link DOMException} `DataCloneError` when closed audio data is cloned.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData)
 */
interface AudioData {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/format) */
  readonly format: AudioSampleFormat | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/sampleRate) */
  readonly sampleRate: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/numberOfFrames) */
  readonly numberOfFrames: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/numberOfChannels) */
  readonly numberOfChannels: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/timestamp) */
  readonly timestamp: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/duration) */
  readonly duration: number;

  /**
   * @throws {@link DOMException} `InvalidStateError` when the audio data is closed.
   * @throws {@link TypeError} when `options` is missing or invalid.
   * @throws {@link RangeError} when `planeIndex`, `frameOffset`, or `frameCount` is out of range.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/allocationSize)
   */
  allocationSize(options: AudioDataCopyToOptions): number;

  /**
   * Writes one channel when `format` is planar, or every channel when it is
   * interleaved. `format` defaults to the stored format. A same-format copy
   * keeps the stored bytes. A different format converts through the unit
   * interval.
   *
   * @throws {@link DOMException} `InvalidStateError` when the audio data is closed.
   * @throws {@link TypeError} when `destination` or `options` is invalid.
   * @throws {@link RangeError} when `planeIndex`, `frameOffset`, or
   * `frameCount` is out of range, or when `destination` is shorter than the copy.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/copyTo)
   */
  copyTo(destination: AllowSharedBufferSource, options: AudioDataCopyToOptions): void;

  /**
   * @throws {@link DOMException} `InvalidStateError` when the audio data is closed.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/clone)
   */
  clone(): AudioData;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/close) */
  close(): void;
}
declare var AudioData: {
  prototype: AudioData;
  new(init: AudioDataInit): AudioData;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/AudioData) */
interface AudioDataInit {
  format: AudioSampleFormat;
  sampleRate: number;
  numberOfFrames: number;
  numberOfChannels: number;
  timestamp: number;
  data: AllowSharedBufferSource;
  /**
   * Listed buffers stay attached. The constructor copies `data` and ignores
   * this list.
   */
  transfer?: ArrayBuffer[];
}

/**
 * Options for {@link AudioData.allocationSize} and {@link AudioData.copyTo}.
 *
 * `planeIndex` is required. An interleaved destination accepts only `0`. A
 * planar destination selects one channel. `frameOffset` defaults to 0.
 * `frameCount` defaults to the frames that remain.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/copyTo)
 */
interface AudioDataCopyToOptions {
  planeIndex: number;
  frameOffset?: number;
  frameCount?: number;
  format?: AudioSampleFormat;
}

/**
 * An encoded audio chunk.
 *
 * `structuredClone` and `postMessage` copy `type`, `timestamp`, `duration`,
 * and the bytes. The chunk is not transferable.
 *
 * @throws {@link DOMException} `DataCloneError` when the chunk is listed for
 * transfer.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedAudioChunk)
 */
interface EncodedAudioChunk {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedAudioChunk/type) */
  readonly type: EncodedAudioChunkType;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedAudioChunk/timestamp) */
  readonly timestamp: number;

  /**
   * Microseconds. Missing or null is null. A non-negative finite number is
   * truncated toward 0.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedAudioChunk/duration)
   */
  readonly duration: number | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedAudioChunk/byteLength) */
  readonly byteLength: number;

  /**
   * Copies the encoded bytes into `destination`.
   *
   * @throws {@link TypeError} when `destination` is detached or shorter than
   * `byteLength`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedAudioChunk/copyTo)
   */
  copyTo(destination: AllowSharedBufferSource): void;
}
declare var EncodedAudioChunk: {
  prototype: EncodedAudioChunk;
  new(init: EncodedAudioChunkInit): EncodedAudioChunk;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedAudioChunk/EncodedAudioChunk) */
interface EncodedAudioChunkInit {
  type: EncodedAudioChunkType;
  timestamp: number;
  duration?: number;
  data: AllowSharedBufferSource;
}

/**
 * Decodes AAC elementary packets.
 *
 * `mp4a` is the only audio codec. `opus`, `vorbis`, and every other codec
 * string fail `configure`. `AudioEncoder` is absent. AAC uses a portable
 * decoder on every target.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder)
 */
interface AudioDecoder extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/state) */
  readonly state: CodecState;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/decodeQueueSize) */
  readonly decodeQueueSize: number;

  /**
   * @throws {@link TypeError} when `config` is missing a required member,
   * `codec` is empty, `description` is detached, or `sampleRate` or
   * `numberOfChannels` is not greater than 0.
   * @throws {@link DOMException} `NotSupportedError` when `codec` is not `mp4a`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/configure)
   */
  configure(config: AudioDecoderConfig): void;

  /**
   * Returns before `output` or `error` runs. Those callbacks are microtasks.
   * A packet the decoder rejects closes the decoder and reports
   * `EncodingError` to `error`.
   *
   * @throws {@link DOMException} `InvalidStateError` when the decoder is not
   * configured.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/decode)
   */
  decode(chunk: EncodedAudioChunk): void;

  /**
   * Resolves without waiting for more packets. Samples `decode` already
   * produced are delivered before this promise settles.
   *
   * @throws {@link DOMException} `InvalidStateError` when the decoder is not
   * configured.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/flush)
   */
  flush(): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/reset) */
  reset(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/close) */
  close(): void;
}
declare var AudioDecoder: {
  prototype: AudioDecoder;
  new(init: AudioDecoderInit): AudioDecoder;
  /**
   * Resolves `{ supported, config }` without opening a decoder and without
   * producing samples. `supported` is true only for an `mp4a` codec string
   * that `configure` accepts, and false for any other codec string. An
   * invalid config rejects `TypeError`. Too few arguments reject `TypeError`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/isConfigSupported)
   */
  isConfigSupported(config: AudioDecoderConfig): Promise<AudioDecoderSupport>;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/AudioDecoder) */
interface AudioDecoderInit {
  output: (data: AudioData) => void;
  error: (error: DOMException) => void;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/configure) */
interface AudioDecoderConfig {
  codec: string;
  sampleRate: number;
  numberOfChannels: number;
  description?: AllowSharedBufferSource;
}

/**
 * Decodes a still image.
 *
 * `isTypeSupported` resolves `true` for `image/png`, `image/jpeg`,
 * `image/webp`, and `image/jxl`. The constructor still returns a decoder for
 * any other type. `completed`, `tracks.ready`, and `decode()` then reject
 * `NotSupportedError`. A supported still image has one track, `frameCount` 1,
 * `animated` false, and `repetitionCount` 0. `frameIndex` other than 0
 * rejects `RangeError`. `desiredWidth`, `desiredHeight`, and
 * `preferAnimation` do not change the result. A decoder panic rejects
 * `EncodingError`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageDecoder)
 */
interface ImageDecoder {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageDecoder/type) */
  readonly type: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageDecoder/complete) */
  readonly complete: boolean;

  /**
   * Rejects `NotSupportedError` for an unsupported type, and `EncodingError`
   * when the bytes cannot be decoded.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageDecoder/completed)
   */
  readonly completed: Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageDecoder/tracks) */
  readonly tracks: ImageTrackList;

  /**
   * @param options - `frameIndex` other than 0 rejects `RangeError`.
   * `completeFramesOnly` is accepted and does not change the still image.
   * @throws {@link DOMException} `NotSupportedError` when the type is not
   * supported. The rejection is from this promise.
   * @throws {@link DOMException} `InvalidStateError` when the track is
   * deselected, or when `close()` rejects a pending decode.
   * @throws {@link DOMException} `AbortError` when `reset()` rejects a pending decode.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageDecoder/decode)
   */
  decode(options?: ImageDecodeOptions): Promise<ImageDecodeResult>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageDecoder/reset) */
  reset(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageDecoder/close) */
  close(): void;

}
declare var ImageDecoder: {
  prototype: ImageDecoder;
  new(init: ImageDecoderInit): ImageDecoder;
  isTypeSupported(type: string): Promise<boolean>;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageDecoder/ImageDecoder) */
interface ImageDecoderInit {
  type: string;
  data: ImageBufferSource;
  colorSpaceConversion?: ColorSpaceConversion;
  desiredWidth?: number;
  desiredHeight?: number;
  preferAnimation?: boolean;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageDecoder/decode) */
interface ImageDecodeOptions {
  frameIndex?: number;
  completeFramesOnly?: boolean;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageDecoder/decode) */
interface ImageDecodeResult {
  image: VideoFrame;
  complete: boolean;
}

/**
 * Tracks of an {@link ImageDecoder}.
 *
 * `ready` rejects `NotSupportedError` for an unsupported type.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageTrackList)
 */
interface ImageTrackList {
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageTrackList/ready) */
  readonly ready: Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageTrackList/length) */
  readonly length: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageTrackList/selectedIndex) */
  readonly selectedIndex: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageTrackList/selectedTrack) */
  readonly selectedTrack: ImageTrack | null;
}
declare var ImageTrackList: {
  prototype: ImageTrackList;
  new(): ImageTrackList;
};


/**
 * The single still-image track.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageTrack)
 */
interface ImageTrack {
  /**
   * Always false. Animated images contribute the first frame only.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageTrack/animated)
   */
  readonly animated: boolean;

  /**
   * Always 1 for a supported still image.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageTrack/frameCount)
   */
  readonly frameCount: number;

  /**
   * Always 0.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageTrack/repetitionCount)
   */
  readonly repetitionCount: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ImageTrack/selected) */
  selected: boolean;
}
declare var ImageTrack: {
  prototype: ImageTrack;
  new(): ImageTrack;
};


/**
 * A server-sent event client.
 *
 * The URL scheme must be `http:` or `https:`. `withCredentials` is stored and
 * does not change the request. There is no cookie jar and no CORS check.
 * The default reconnection time is 3000ms.
 *
 * @throws {@link DOMException} `SyntaxError` when the scheme is not `http` or `https`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventSource)
 */
interface EventSource extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventSource/url) */
  readonly url: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventSource/withCredentials) */
  readonly withCredentials: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventSource/readyState) */
  readonly readyState: number;

  readonly CONNECTING: 0;
  readonly OPEN: 1;
  readonly CLOSED: 2;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventSource/close) */
  close(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventSource/open_event) */
  onopen: ((this: EventSource, ev: Event) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventSource/message_event) */
  onmessage: ((this: EventSource, ev: MessageEvent) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventSource/error_event) */
  onerror: ((this: EventSource, ev: Event) => any) | null;
}
declare var EventSource: {
  prototype: EventSource;
  new(url: string | URL, eventSourceInitDict?: EventSourceInit): EventSource;
  readonly CONNECTING: 0;
  readonly OPEN: 1;
  readonly CLOSED: 2;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EventSource/EventSource) */
interface EventSourceInit {
  withCredentials?: boolean;
}

/**
 * Address space required by a WebSocket connection.
 *
 * The connected address must match exactly. `"private"` is not accepted.
 */
type TargetAddressSpace = "loopback" | "local" | "public";

/**
 * Second argument of {@link WebSocket}.
 */
interface WebSocketInit {
  protocols?: string | string[];
  targetAddressSpace?: TargetAddressSpace;
}

/**
 * A WebSocket.
 *
 * `extensions` stays `""`. There is no permessage-deflate. `wss:` trusts the
 * webpki root set. Binary messages follow `binaryType`.
 *
 * The constructor takes a `ws:` or `wss:` URL. `protocols` is a protocol
 * string, a sequence of protocol strings, or a {@link WebSocketInit}. A
 * further argument is ignored.
 *
 * @throws {@link TypeError} When `targetAddressSpace` is not `loopback`,
 * `local`, or `public`, including the legacy alias `"private"`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket)
 */
interface WebSocket extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/url) */
  readonly url: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/readyState) */
  readonly readyState: 0 | 2 | 1 | 3;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/bufferedAmount) */
  readonly bufferedAmount: number;

  /**
   * Always `""`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/extensions)
   */
  readonly extensions: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/protocol) */
  readonly protocol: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/binaryType) */
  binaryType: BinaryType;

  readonly CONNECTING: 0;
  readonly OPEN: 1;
  readonly CLOSING: 2;
  readonly CLOSED: 3;

  /**
   * @param code - 1000 or a code from 3000 through 4999.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/close)
   */
  close(code?: number, reason?: string): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/send)
   */
  send(data: string | BufferSource | Blob): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/open_event) */
  onopen: ((this: WebSocket, ev: Event) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/message_event) */
  onmessage: ((this: WebSocket, ev: MessageEvent) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/error_event) */
  onerror: ((this: WebSocket, ev: Event) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocket/close_event) */
  onclose: ((this: WebSocket, ev: CloseEvent) => any) | null;
}
declare var WebSocket: {
  prototype: WebSocket;
  new(url: string | URL, protocols?: string | string[]): WebSocket;
  readonly CONNECTING: 0;
  readonly OPEN: 1;
  readonly CLOSING: 2;
  readonly CLOSED: 3;
};


/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/CloseEvent)
 */
interface CloseEvent extends Event {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CloseEvent/wasClean) */
  readonly wasClean: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CloseEvent/code) */
  readonly code: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CloseEvent/reason) */
  readonly reason: string;
}
declare var CloseEvent: {
  prototype: CloseEvent;
  new(type: string, eventInitDict?: CloseEventInit): CloseEvent;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/CloseEvent/CloseEvent) */
interface CloseEventInit extends EventInit {
  wasClean?: boolean;
  code?: number;
  reason?: string;
}

/**
 * Options for {@link WebSocketStream}.
 */
interface WebSocketStreamOptions {
  protocols?: string[];
  signal?: AbortSignal;
}

/**
 * A WebSocket exposed as streams.
 *
 * `signal` aborts only the handshake. Text messages are strings and binary
 * messages are `Uint8Array` values. A handshake failure rejects both `opened`
 * and `closed`, and those rejections are marked handled. `extensions` in the
 * opened result stays `""`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocketStream)
 */
declare class WebSocketStream {
  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocketStream/WebSocketStream)
   */
  constructor(url: string | URL, options?: WebSocketStreamOptions);

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocketStream/url) */
  readonly url: string;

  /**
   * Resolves when the handshake finishes.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocketStream/opened)
   */
  readonly opened: Promise<WebSocketConnection>;

  /**
   * Resolves `{ closeCode, reason }` after a clean close and rejects with
   * {@link WebSocketError} after an unclean close.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocketStream/closed)
   */
  readonly closed: Promise<WebSocketCloseInfo>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocketStream/close)
   */
  close(options?: WebSocketCloseInfo): void;
}

/** The value {@link WebSocketStream.opened} resolves. */
interface WebSocketConnection {
  readable: ReadableStream<string | Uint8Array<ArrayBuffer>>;
  writable: WritableStream<string | BufferSource>;
  protocol: string;
  extensions: string;
}

/** Close information for {@link WebSocketStream}. */
interface WebSocketCloseInfo {
  closeCode?: number;
  reason?: string;
}

/**
 * A WebSocket stream failure.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocketError)
 */
declare class WebSocketError extends DOMException {
  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocketError/WebSocketError)
   */
  constructor(message?: string, options?: WebSocketCloseInfo);

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocketError/closeCode) */
  readonly closeCode: number | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebSocketError/reason) */
  readonly reason: string;
}

/**
 * Requested congestion control for a {@link WebTransport} session.
 */
type WebTransportCongestionControl = "default" | "throughput" | "low-latency";

/**
 * Datagram support reported by a {@link WebTransport} session.
 *
 * `"pending"` is the value until `ready` resolves.
 */
type WebTransportReliabilityMode =
  | "pending"
  | "supports-unreliable"
  | "reliable-only";

/**
 * Options for {@link WebTransport}.
 *
 * `serverCertificateHashes` replaces the webpki roots when the list is
 * non-empty, including when every algorithm is unknown. Unknown algorithms
 * are ignored. `allowPooling: true` with that non-empty list throws
 * `NotSupportedError`. Pooling is otherwise ignored.
 * Omitted `datagramsReadableType` uses a default readable stream, and
 * `getReader({ mode: "byob" })` on it throws `TypeError`. `"bytes"` uses a
 * readable byte stream. A BYOB view shorter than the datagram rejects the
 * read with `RangeError`. A view whose element size is not 1, including a
 * `DataView`, rejects the read with `TypeError`. An empty datagram in that
 * mode rejects the read with `TypeError`. Any other string throws
 * `TypeError`. A non-empty `protocols` list throws
 * `NotSupportedError`. An empty list is accepted. A `Headers` object throws
 * `NotSupportedError`, including an empty one. A non-empty header sequence
 * or record throws `NotSupportedError`. An empty sequence or record is
 * accepted. A value that is not a `HeadersInit` throws `TypeError`.
 * `anticipatedConcurrentIncomingUnidirectionalStreams` and
 * `anticipatedConcurrentIncomingBidirectionalStreams` must be `null` or an
 * integer from 0 through 65535. Any other number throws `TypeError`.
 * In-range values are ignored, and those attributes are absent.
 * `requireUnreliable` rejects `ready` and `closed` when the session cannot
 * carry datagrams.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/WebTransport)
 */
interface WebTransportOptions {
  allowPooling?: boolean;
  requireUnreliable?: boolean;
  congestionControl?: WebTransportCongestionControl;
  serverCertificateHashes?: WebTransportHash[];
  protocols?: string[];
  datagramsReadableType?: string;
  headers?: HeadersInit;
  anticipatedConcurrentIncomingUnidirectionalStreams?: number | null;
  anticipatedConcurrentIncomingBidirectionalStreams?: number | null;
}

/**
 * One certificate hash in {@link WebTransportOptions.serverCertificateHashes}.
 *
 * `algorithm` other than `"sha-256"` is ignored. A `"sha-256"` `value` is 32
 * bytes. A missing `algorithm` or `value`, a `value` that is not a
 * `BufferSource`, a `SharedArrayBuffer`, a view of one, or a resizable
 * buffer throws `TypeError`.
 */
interface WebTransportHash {
  algorithm: string;
  value: BufferSource;
}

/**
 * Close information for {@link WebTransport}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransportCloseInfo)
 */
interface WebTransportCloseInfo {
  closeCode?: number;
  reason?: string;
}

/**
 * Options for {@link WebTransport.createBidirectionalStream} and
 * {@link WebTransport.createUnidirectionalStream}.
 *
 * `sendOrder` and `waitUntilAvailable` are ignored. A non-null `sendGroup`
 * throws `NotSupportedError`.
 */
interface WebTransportSendStreamOptions {
  sendGroup?: object | null;
  sendOrder?: number;
  waitUntilAvailable?: boolean;
}

/**
 * A bidirectional WebTransport stream.
 *
 * `readable` is a readable byte stream. Chunks on both sides are `Uint8Array`
 * values.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransportBidirectionalStream)
 */
interface WebTransportBidirectionalStream {
  readonly readable: ReadableStream;
  readonly writable: WritableStream;
}

/**
 * Datagram duplex exposed by {@link WebTransport.datagrams}.
 *
 * This is a plain object. Chunks are `Uint8Array` values. Omitted
 * `datagramsReadableType` makes `readable` a default readable stream, and
 * `getReader({ mode: "byob" })` throws `TypeError`. `"bytes"` makes
 * `readable` a readable byte stream. Both modes use a high-water mark of 0.
 * In bytes mode one read returns one datagram. A BYOB view shorter than the
 * datagram rejects the read with `RangeError`. A view whose element size is
 * not 1, including a `DataView`, rejects the read with `TypeError`.
 */
interface WebTransportDatagrams {
  readable: ReadableStream<Uint8Array<ArrayBuffer>>;
  writable: WritableStream<Uint8Array<ArrayBuffer>>;
}

/**
 * Options for {@link WebTransportError}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransportError/WebTransportError)
 */
interface WebTransportErrorOptions {
  source?: "stream" | "session";
  streamErrorCode?: number | null;
}

/**
 * An HTTP/3 WebTransport session.
 *
 * The URL must be `https` and must not contain a fragment. A relative URL
 * is resolved against the entry URL. The constructor is on `globalThis` in
 * the main realm and in a dedicated worker. This is not an `EventTarget`.
 * `ready` and `closed` reject when the handshake fails, and those
 * rejections are marked handled. `datagrams` is a plain
 * `{ readable, writable }`. Chunks are `Uint8Array` values. The readable
 * side of a bidirectional stream, and each incoming unidirectional stream,
 * is a readable byte stream. `reliability` is `"pending"` until `ready`
 * resolves.
 *
 * @throws {@link DOMException} `SyntaxError` when the URL is invalid, the
 * scheme is not `https`, or the URL has a fragment.
 * @throws {@link TypeError} When `options` is not an object, `protocols` is
 * not an array, `headers` is not a `HeadersInit`, `congestionControl` is
 * not `"default"`, `"throughput"`, or `"low-latency"`,
 * `datagramsReadableType` is a string other than `"bytes"`, a certificate
 * hash is malformed, or an anticipated incoming-stream count is not an
 * unsigned short.
 * @throws {@link DOMException} `NotSupportedError` when `protocols` is
 * non-empty, `headers` is a `Headers` object or a non-empty sequence or
 * record, or `allowPooling` is true with a non-empty
 * `serverCertificateHashes` list.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport)
 */
declare class WebTransport {
  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/WebTransport)
   */
  constructor(url: string | URL, options?: WebTransportOptions);

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/url)
   */
  readonly url: string;

  /**
   * Resolves when the handshake finishes.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/ready)
   */
  readonly ready: Promise<undefined>;

  /**
   * Resolves `{ closeCode, reason }` after a clean close and rejects with
   * {@link WebTransportError} after an unclean close.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/closed)
   */
  readonly closed: Promise<WebTransportCloseInfo>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/datagrams)
   */
  readonly datagrams: WebTransportDatagrams;

  /**
   * Each value's `readable` is a readable byte stream.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/incomingBidirectionalStreams)
   */
  readonly incomingBidirectionalStreams: ReadableStream<WebTransportBidirectionalStream>;

  /**
   * Each value is a readable byte stream.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/incomingUnidirectionalStreams)
   */
  readonly incomingUnidirectionalStreams: ReadableStream<ReadableStream<Uint8Array<ArrayBuffer>>>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/congestionControl)
   */
  readonly congestionControl: WebTransportCongestionControl;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/reliability)
   */
  readonly reliability: WebTransportReliabilityMode;

  /**
   * @param closeInfo - `closeCode` is an unsigned long. `reason` is a
   * USVString of at most 1024 UTF-8 bytes.
   * @throws {@link DOMException} `SyntaxError` when `reason` is longer than
   * 1024 UTF-8 bytes.
   * @throws {@link TypeError} When `closeInfo` is not an object.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/close)
   */
  close(closeInfo?: WebTransportCloseInfo): void;

  /**
   * @throws {@link DOMException} `NotSupportedError` when `sendGroup` is
   * non-null.
   * @throws {@link TypeError} When `options` is not an object.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/createBidirectionalStream)
   */
  createBidirectionalStream(
    options?: WebTransportSendStreamOptions,
  ): Promise<WebTransportBidirectionalStream>;

  /**
   * @throws {@link DOMException} `NotSupportedError` when `sendGroup` is
   * non-null.
   * @throws {@link TypeError} When `options` is not an object.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/createUnidirectionalStream)
   */
  createUnidirectionalStream(
    options?: WebTransportSendStreamOptions,
  ): Promise<WritableStream<Uint8Array<ArrayBuffer>>>;
}

/**
 * A WebTransport stream or session failure.
 *
 * `name` is `"WebTransportError"` and `code` is `0`. `source` defaults to
 * `"stream"`. `streamErrorCode` defaults to `null`.
 *
 * @throws {@link TypeError} When `options` is not an object or `source` is
 * not `"stream"` or `"session"`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransportError)
 */
declare class WebTransportError extends DOMException {
  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransportError/WebTransportError)
   */
  constructor(message?: string, options?: WebTransportErrorOptions);

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransportError/source)
   */
  readonly source: "stream" | "session";

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransportError/streamErrorCode)
   */
  readonly streamErrorCode: number | null;
}

/**
 * A dedicated worker.
 *
 * `new Worker(url)` runs a classic script. `{ type: "module" }` runs an ES
 * module. There is no `SharedWorker`. `postMessage` carries values
 * `structuredClone` can carry, including a transferred `ArrayBuffer`,
 * `MessagePort`, `OffscreenCanvas`, `ImageBitmap`, `AudioData`,
 * `VideoFrame`, `ReadableStream`, `WritableStream`, or `TransformStream`,
 * and a cloned `ImageData`, `EncodedAudioChunk`, or `EncodedVideoChunk`.
 * A {@link Yuzuko.Window} cannot be transferred.
 * A WebGPU device stays on the thread that created it.
 *
 * @throws {@link TypeError} When the URL or `type` is invalid.
 * @throws {@link DOMException} `DataCloneError` when `postMessage` carries a
 * value that cannot be cloned, including a {@link Yuzuko.Window}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Worker)
 */
interface Worker extends EventTarget {

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Worker/postMessage)
   */
  postMessage(message: any, transfer: Transferable[]): void;
  postMessage(message: any, options?: StructuredSerializeOptions): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Worker/terminate) */
  terminate(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Worker/message_event) */
  onmessage: ((this: Worker, ev: MessageEvent) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Worker/error_event) */
  onerror: ((this: Worker, ev: ErrorEvent) => any) | null;
}
declare var Worker: {
  prototype: Worker;
  new(scriptURL: string | URL, options?: WorkerOptions): Worker;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Worker/Worker) */
interface WorkerOptions {
  /** `"classic"` or `"module"`. */
  type?: WorkerType;
  name?: string;
}

/**
 * Prototype of a worker global.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerGlobalScope)
 */
declare class WorkerGlobalScope extends EventTarget {
  constructor();

  /**
   * Imports classic scripts.
   *
   * @throws {@link TypeError} In a module worker. `importScripts` is not available there.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerGlobalScope/importScripts)
   */
  importScripts(...urls: string[]): void;
}

/**
 * The global scope inside `new Worker`.
 *
 * The worker has the web globals in this file except `requestIdleCallback`,
 * `cancelIdleCallback`, `Yuzuko.Window`, `Yuzuko.Menu`, `Yuzuko.setMenu`,
 * `Yuzuko.Tray`, `Yuzuko.dock`, `Yuzuko.updater`, `navigator.clipboard`, and
 * `navigator.getGamepads`. `self.fonts` and `globalThis.fonts` are that
 * worker's {@link FontFaceSet}. `requestAnimationFrame` runs the callback at
 * the slowest refresh rate among the open windows whose canvas this worker
 * holds, or at 60 Hz when it holds none, and then copies each transferred
 * window canvas into the window that owns it. That window presents the copy
 * on the next turn of the main event loop.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DedicatedWorkerGlobalScope)
 */
declare class DedicatedWorkerGlobalScope extends WorkerGlobalScope {
  constructor();

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DedicatedWorkerGlobalScope/postMessage)
   */
  postMessage(message: any, transfer: Transferable[]): void;
  postMessage(message: any, options?: StructuredSerializeOptions): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DedicatedWorkerGlobalScope/close)
   */
  close(): void;

  /**
   * Schedules `callback`, then copies each transferred window canvas.
   *
   * The period is the slowest refresh rate among the open windows whose
   * OffscreenCanvas this worker holds. A window that has not transferred its
   * canvas does not count. With none, the period is 60 Hz. A transfer, a
   * close, or a move to another monitor applies on the next frame.
   *
   * Browsers throw `NotSupportedError` when the worker has no owner window.
   * This runtime does not. It schedules the callback on the worker event loop.
   *
   * @throws {@link TypeError} When `callback` is not a function.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DedicatedWorkerGlobalScope/requestAnimationFrame)
   */
  requestAnimationFrame(callback: FrameRequestCallback): number;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/DedicatedWorkerGlobalScope/cancelAnimationFrame)
   */
  cancelAnimationFrame(handle: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DedicatedWorkerGlobalScope/name) */
  readonly name: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerGlobalScope/location) */
  readonly location: WorkerLocation;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerGlobalScope/navigator) */
  readonly navigator: WorkerNavigator;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerGlobalScope/fonts) */
  readonly fonts: FontFaceSet;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DedicatedWorkerGlobalScope/message_event) */
  onmessage: ((this: DedicatedWorkerGlobalScope, ev: MessageEvent) => any) | null;

  onmessageerror: ((this: DedicatedWorkerGlobalScope, ev: MessageEvent) => any) | null;
}

/**
 * `location` inside a dedicated worker.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerLocation)
 */
declare class WorkerLocation {
  constructor();

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerLocation/href) */
  readonly href: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerLocation/origin) */
  readonly origin: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerLocation/protocol) */
  readonly protocol: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerLocation/host) */
  readonly host: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerLocation/hostname) */
  readonly hostname: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerLocation/port) */
  readonly port: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerLocation/pathname) */
  readonly pathname: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerLocation/search) */
  readonly search: string;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerLocation/hash) */
  readonly hash: string;
}

/**
 * `navigator` inside a dedicated worker.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerNavigator)
 */
declare class WorkerNavigator {
  constructor();

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/WorkerNavigator/userAgent) */
  readonly userAgent: string;

  readonly platform: string;
}

/**
 * A media query list for one {@link Yuzuko.Window}.
 *
 * `new MediaQueryList()` throws. Obtain one from {@link Yuzuko.Window.matchMedia}.
 * `change` fires when the result changes.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MediaQueryList)
 */
interface MediaQueryList extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MediaQueryList/matches) */
  readonly matches: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MediaQueryList/media) */
  readonly media: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MediaQueryList/change_event) */
  onchange: ((this: MediaQueryList, ev: MediaQueryListEvent) => any) | null;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MediaQueryList/addListener)
   */
  addListener(callback: ((this: MediaQueryList, ev: MediaQueryListEvent) => void) | null): void;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MediaQueryList/removeListener)
   */
  removeListener(callback: ((this: MediaQueryList, ev: MediaQueryListEvent) => void) | null): void;
}
declare var MediaQueryList: {
  prototype: MediaQueryList;
  new(): MediaQueryList;
};


/**
 * A change to a {@link MediaQueryList}.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/MediaQueryListEvent)
 */
interface MediaQueryListEvent extends Event {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MediaQueryListEvent/matches) */
  readonly matches: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/MediaQueryListEvent/media) */
  readonly media: string;
}
declare var MediaQueryListEvent: {
  prototype: MediaQueryListEvent;
  new(type: string, eventInitDict?: MediaQueryListEventInit): MediaQueryListEvent;
};


/**
 * A clipboard item.
 *
 * `text/plain` and `image/png` are the only types. A malformed type string
 * throws `TypeError`. Any other type throws `NotSupportedError`.
 * `ClipboardItem.supports` returns true for those two types.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ClipboardItem)
 */
interface ClipboardItem {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ClipboardItem/presentationStyle) */
  readonly presentationStyle: PresentationStyle;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ClipboardItem/types) */
  readonly types: readonly string[];

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ClipboardItem/getType)
   */
  getType(type: string): Promise<Blob>;

}
declare var ClipboardItem: {
  prototype: ClipboardItem;
  new(items: Record<string, string | Blob | PromiseLike<string | Blob>>, options?: ClipboardItemOptions): ClipboardItem;
  supports(type: string): boolean;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ClipboardItem/ClipboardItem) */
interface ClipboardItemOptions {
  presentationStyle?: PresentationStyle;
}

/**
 * The system clipboard.
 *
 * Reads and writes require a pointer or key press on the focused
 * {@link Yuzuko.Window} within five seconds. There is no permission prompt
 * and no secure-context gate. Text is published to the OS pasteboard.
 * `image/png` is stored as the written bytes and also published as an image
 * when the PNG decodes.
 *
 * @throws {@link TypeError} Illegal constructor. Use {@link Navigator.clipboard}.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Clipboard)
 */
interface Clipboard extends EventTarget {

  /**
   * @throws {@link DOMException} `NotAllowedError` without a recent pointer or
   * key press on the focused window, or when the window is not focused.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Clipboard/readText)
   */
  readText(): Promise<string>;

  /**
   * @throws {@link DOMException} `NotAllowedError` without a recent pointer or
   * key press on the focused window.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Clipboard/writeText)
   */
  writeText(data: string): Promise<void>;

  /**
   * @throws {@link DOMException} `NotAllowedError` without a recent pointer or
   * key press on the focused window.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Clipboard/read)
   */
  read(): Promise<ClipboardItem[]>;

  /**
   * @throws {@link DOMException} `NotAllowedError` without a recent pointer or
   * key press on the focused window.
   * @throws {@link DOMException} `NotSupportedError` when an item type is not
   * `text/plain` or `image/png`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Clipboard/write)
   */
  write(data: ClipboardItem[]): Promise<void>;
}
declare var Clipboard: {
  prototype: Clipboard;
  new(): Clipboard;
};


/**
 * `copy`, `cut`, and `paste` on the focused {@link Yuzuko.WindowSurface}.
 *
 * The host sends these for Ctrl/Cmd+C, X, and V. `clipboardData` is null.
 * `ClipboardChangeEvent` is absent.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ClipboardEvent)
 */
interface ClipboardEvent extends Event {

  /**
   * Null for host `copy`, `cut`, and `paste` events.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/ClipboardEvent/clipboardData)
   */
  readonly clipboardData: null;
}
declare var ClipboardEvent: {
  prototype: ClipboardEvent;
  new(type: string, eventInitDict?: ClipboardEventInit): ClipboardEvent;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ClipboardEvent/ClipboardEvent) */
interface ClipboardEventInit extends EventInit {
  clipboardData?: null;
}

/**
 * A connected gamepad.
 *
 * `touches` stays empty, `pose` is null, and `hand` is `""`. There is no
 * secure-context gate.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad)
 */
interface Gamepad {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/id) */
  readonly id: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/index) */
  readonly index: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/connected) */
  readonly connected: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/timestamp) */
  readonly timestamp: DOMHighResTimeStamp;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/mapping) */
  readonly mapping: GamepadMappingType;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/axes) */
  readonly axes: readonly number[];

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/buttons) */
  readonly buttons: readonly GamepadButton[];

  /**
   * Always empty.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/touches)
   */
  readonly touches: readonly GamepadTouch[];

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/vibrationActuator) */
  readonly vibrationActuator: GamepadHapticActuator;

  /**
   * Always `""`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/hand)
   */
  readonly hand: GamepadHand;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/hapticActuators) */
  readonly hapticActuators: readonly GamepadHapticActuator[];

  /**
   * Always null. No pose is reported.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/pose)
   */
  readonly pose: GamepadPose | null;
}
declare var Gamepad: {
  prototype: Gamepad;
  new(): Gamepad;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Gamepad/hand) */
type GamepadHand = "" | "left" | "right";

/**
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadButton)
 */
interface GamepadButton {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadButton/pressed) */
  readonly pressed: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadButton/touched) */
  readonly touched: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadButton/value) */
  readonly value: number;
}
declare var GamepadButton: {
  prototype: GamepadButton;
  new(): GamepadButton;
};


/**
 * Haptic actuator for one gamepad.
 *
 * `effects` contains `"dual-rumble"` when the device can play that effect.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadHapticActuator)
 */
interface GamepadHapticActuator {

  /**
   * `"dual-rumble"` when the device can play it. Otherwise empty.
   * The same frozen array is returned for the life of the actuator.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadHapticActuator/effects)
   */
  readonly effects: readonly GamepadHapticEffectType[];

  /**
   * Plays `"dual-rumble"` when that type is listed in `effects`.
   * `trigger-rumble` rejects `NotSupportedError`.
   * The sum of `duration` and `startDelay` is capped at 5000 ms.
   * The promise resolves `"complete"` or `"preempted"`.
   *
   * @throws {@link DOMException} `InvalidStateError` when the gamepad is disconnected.
   * @throws {@link DOMException} `NotSupportedError` when `type` is not in `effects`.
   * @throws {@link TypeError} When `type` is not a haptic effect type, or the
   * parameters are not a valid effect.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadHapticActuator/playEffect)
   */
  playEffect(type: GamepadHapticEffectType, params?: GamepadEffectParameters): Promise<GamepadHapticsResult>;

  /**
   * Resolves `"complete"` while the gamepad is connected.
   * A playing effect resolves `"preempted"`.
   *
   * @throws {@link DOMException} `InvalidStateError` when the gamepad is disconnected.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadHapticActuator/reset)
   */
  reset(): Promise<GamepadHapticsResult>;
}
declare var GamepadHapticActuator: {
  prototype: GamepadHapticActuator;
  new(): GamepadHapticActuator;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadHapticActuator/playEffect) */
interface GamepadEffectParameters {
  duration?: number;
  startDelay?: number;
  strongMagnitude?: number;
  weakMagnitude?: number;
  leftTrigger?: number;
  rightTrigger?: number;
}

/**
 * A pose that reports nothing.
 *
 * Every vector is null and both flags are false.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose)
 */
declare class GamepadPose {
  constructor();

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/hasOrientation) */
  readonly hasOrientation: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/hasPosition) */
  readonly hasPosition: boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/position) */
  readonly position: Float32Array<ArrayBuffer> | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/linearVelocity) */
  readonly linearVelocity: Float32Array<ArrayBuffer> | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/linearAcceleration) */
  readonly linearAcceleration: Float32Array<ArrayBuffer> | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/orientation) */
  readonly orientation: Float32Array<ArrayBuffer> | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/angularVelocity) */
  readonly angularVelocity: Float32Array<ArrayBuffer> | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/angularAcceleration) */
  readonly angularAcceleration: Float32Array<ArrayBuffer> | null;
}

/**
 * Declared for {@link Gamepad.touches}, which stays empty.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadTouch)
 */
interface GamepadTouch {
  touchId: number;
  surfaceId: number;
  position: Float32Array<ArrayBuffer>;
  surfaceDimensions: DOMRectReadOnly | null;
}

/**
 * `gamepadconnected` and `gamepaddisconnected` on the main `globalThis`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadEvent)
 */
interface GamepadEvent extends Event {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadEvent/gamepad) */
  readonly gamepad: Gamepad;
}
declare var GamepadEvent: {
  prototype: GamepadEvent;
  new(type: string, eventInitDict?: GamepadEventInit): GamepadEvent;
};


/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadEvent/GamepadEvent) */
interface GamepadEventInit extends EventInit {
  gamepad?: Gamepad | null;
}
