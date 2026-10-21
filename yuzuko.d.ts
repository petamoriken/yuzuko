/// <reference lib="esnext" />

/**
 * API reference for the Yuzuko runtime.
 *
 * Web-standard and WinterTC APIs are declared on the global scope. Runtime
 * APIs are declared on {@link Yuzuko}. This file stands alone. It declares the
 * types this runtime has. A name that `lib.dom.d.ts` also declares is defined
 * here with this runtime's members, so a program that loads both reports
 * duplicate identifiers. The ambient declarations describe the main realm.
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
type ChannelCountMode = "clamped-max" | "explicit" | "max";
type ChannelInterpretation = "discrete" | "speakers";
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
type GPUBindingResource = GPUSampler | GPUTexture | GPUTextureView | GPUBuffer | GPUBufferBinding;
type GPUBlendFactor = "constant" | "dst" | "dst-alpha" | "one" | "one-minus-constant" | "one-minus-dst" | "one-minus-dst-alpha" | "one-minus-src" | "one-minus-src-alpha" | "src" | "src-alpha" | "src-alpha-saturated" | "zero";
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
interface GPUComputePassEncoder extends GPUBindingCommandsMixin, GPUDebugCommandsMixin, GPUObjectBase {
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
interface GPUDebugCommandsMixin {
  insertDebugMarker(markerLabel: string): void;
  popDebugGroup(): void;
  pushDebugGroup(groupLabel: string): void;
}
interface GPUObjectBase {
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
  destroy(): void;
}

type GPUSize32Out = number;
type GPUQueryType = "occlusion" | "timestamp";
interface GPURenderBundleEncoder extends GPUBindingCommandsMixin, GPUDebugCommandsMixin, GPUObjectBase, GPURenderCommandsMixin {
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
interface GPURenderBundleDescriptor {}
interface GPURenderBundle {}
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
type ReadableStreamType = "bytes";
type RenderingContext = OffscreenCanvasRenderingContext2D | BitmapContext | GPUCanvasContext;
type RequestCache = "default" | "force-cache" | "no-cache" | "no-store" | "only-if-cached" | "reload";
type RequestCredentials = "include" | "omit" | "same-origin";
type RequestInfo = Request | string;
type RequestPriority = "auto" | "high" | "low";
type RequestRedirect = "error" | "follow" | "manual";
type ResponseType = "basic" | "cors" | "default" | "error" | "opaque" | "opaqueredirect";
type TimerHandler = string | Function;
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
 * Alias of `globalThis` in every realm.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Window/self)
 */
declare var self: Window & typeof globalThis;

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
 *
 * @param value - The value to clone.
 * @param options - `transfer` moves the listed objects.
 * @throws {@link DOMException} `DataCloneError` when `value` contains a host
 * object this runtime cannot clone, including a {@link Yuzuko.Window}, a
 * {@link Yuzuko.WindowSurface}, or a {@link VideoFrame}.
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
 * `dirxml` and `table` print like `log`. `clear` succeeds and does not erase
 * a surface. `time`, `timeLog`, and `timeEnd` share one table.
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
 * `clipboard`, `gpu`, and `getGamepads` exist on the main global. A worker
 * navigator has `userAgent` and `platform` only.
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
 * then the window. `pointerId` is per window. `pointerrawupdate` is not
 * implemented. Missing OS values are 0. A mouse uses `pressure` 0.5 while a
 * button is down and 0 otherwise. `movementX` and `movementY` are relative
 * while the pointer is locked.
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
   * Moves coalesced into this `pointermove`. Other types return an empty list.
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
  encode(input?: string): Uint8Array;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/TextEncoder/encodeInto)
   */
  encodeInto(
    source: string,
    destination: Uint8Array,
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
  bytes(): Promise<Uint8Array>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Blob/stream) */
  stream(): ReadableStream<Uint8Array>;
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
  onloadstart: ((this: FileReader, ev: Event) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/progress_event) */
  onprogress: ((this: FileReader, ev: Event) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/load_event) */
  onload: ((this: FileReader, ev: Event) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/abort_event) */
  onabort: ((this: FileReader, ev: Event) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/error_event) */
  onerror: ((this: FileReader, ev: Event) => any) | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FileReader/loadend_event) */
  onloadend: ((this: FileReader, ev: Event) => any) | null;

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
  readonly body: ReadableStream<Uint8Array> | null;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/clone)
   */
  clone(): Request;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/text) */
  text(): Promise<string>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/arrayBuffer) */
  arrayBuffer(): Promise<ArrayBuffer>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Request/bytes) */
  bytes(): Promise<Uint8Array>;

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
  readonly body: ReadableStream<Uint8Array> | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/clone) */
  clone(): Response;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/text) */
  text(): Promise<string>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/arrayBuffer) */
  arrayBuffer(): Promise<ArrayBuffer>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/Response/bytes) */
  bytes(): Promise<Uint8Array>;

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
 * A small request helper used to normalize headers.
 *
 * `responseType`, upload progress, and the event attributes other than the
 * `load` and `error` tasks fired by `send` are absent. The call is synchronous
 * until the response headers and body are collected, then `onload` or
 * `onerror` runs as a task.
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

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/XMLHttpRequest/responseText) */
  readonly responseText: string;

  onload: ((this: XMLHttpRequest, ev: Event) => any) | null;
  onerror: ((this: XMLHttpRequest, ev: Event) => any) | null;
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

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/ReadableStreamDefaultController/enqueue) */
  enqueue(chunk?: R): void;

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
  toFloat32Array(): Float32Array;
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/DOMMatrixReadOnly/toFloat64Array) */
  toFloat64Array(): Float64Array;
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
 * `self.fonts`. `load`, `check`, `ready`, and the set iterators are absent.
 * A family that has finished `load()` and is in the set can be named from
 * the Canvas 2D `font` property.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet)
 */
interface FontFaceSet extends EventTarget {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/add) */
  add(font: FontFace): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/delete) */
  delete(font: FontFace): boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/has) */
  has(font: FontFace): boolean;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/FontFaceSet/clear) */
  clear(): void;

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
 *
 * @throws {@link DOMException} `InvalidStateError` when the typed array does
 * not match `pixelFormat`.
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
 * at most 100 million pixels.
 *
 * @throws {@link TypeError} Illegal constructor. Use {@link createImageBitmap}.
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
 * One process device is shared with Canvas 2D and is created with no optional
 * features. `YUZUKO_SKIP_GPU=1` makes {@link GPU.requestAdapter} resolve `null`.
 * `wgslLanguageFeatures` is empty. `label` on GPU objects reads back as `""`.
 *
 * Render bundles, compute pipelines, compute passes, occlusion queries,
 * timestamp queries, and a depth-stencil attachment throw `NotSupportedError`.
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
   * A non-empty `requiredFeatures` list rejects. `requiredLimits` the shared
   * device cannot satisfy also reject. `destroy` loses that JavaScript device
   * only.
   *
   * @throws {@link TypeError} When `requiredFeatures` is non-empty or a
   * required limit is outside the shared device.
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

  /** Reads back as `""`. [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/label) */
  readonly label: string;

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

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createTexture) */
  createTexture(descriptor: GPUTextureDescriptor): GPUTexture;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createSampler) */
  createSampler(descriptor?: GPUSamplerDescriptor): GPUSampler;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createShaderModule) */
  createShaderModule(descriptor: GPUShaderModuleDescriptor): GPUShaderModule;

  /**
   * A storage binding throws `NotSupportedError`.
   *
   * @throws {@link DOMException} `NotSupportedError` when an entry is outside
   * the implemented binding types.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroupLayout)
   */
  createBindGroupLayout(descriptor: GPUBindGroupLayoutDescriptor): GPUBindGroupLayout;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createPipelineLayout) */
  createPipelineLayout(descriptor: GPUPipelineLayoutDescriptor): GPUPipelineLayout;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createBindGroup) */
  createBindGroup(descriptor: GPUBindGroupDescriptor): GPUBindGroup;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderPipeline) */
  createRenderPipeline(descriptor: GPURenderPipelineDescriptor): GPURenderPipeline;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createCommandEncoder) */
  createCommandEncoder(descriptor?: GPUCommandEncoderDescriptor): GPUCommandEncoder;

  /**
   * @throws {@link DOMException} `NotSupportedError` Render bundles are not implemented.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createRenderBundleEncoder)
   */
  createRenderBundleEncoder(descriptor: GPURenderBundleEncoderDescriptor): GPURenderBundleEncoder;

  /**
   * @throws {@link DOMException} `NotSupportedError` Compute pipelines are not implemented.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createComputePipeline)
   */
  createComputePipeline(descriptor: GPUComputePipelineDescriptor): GPUComputePipeline;

  /**
   * @throws {@link DOMException} `NotSupportedError` Compute pipelines are not implemented.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createComputePipelineAsync)
   */
  createComputePipelineAsync(descriptor: GPUComputePipelineDescriptor): Promise<GPUComputePipeline>;

  /**
   * @throws {@link DOMException} `NotSupportedError` Query sets are not implemented.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUDevice/createQuerySet)
   */
  createQuerySet(descriptor: GPUQuerySetDescriptor): GPUQuerySet;

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


/** Placeholder for the unimplemented render-bundle descriptor. */
interface GPURenderBundleEncoderDescriptor {
  label?: string;
}

/** Placeholder for the unimplemented compute descriptor. */
interface GPUComputePipelineDescriptor {
  label?: string;
}

/** Placeholder for the unimplemented query-set descriptor. */
interface GPUQuerySetDescriptor {
  label?: string;
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
  /** Reads back as `""`. */
  readonly label: string;

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
  /** Reads back as `""`. */
  readonly label: string;
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
  readonly label: string;
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
interface GPUTextureView {
  readonly label: string;
}
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
interface GPUSampler {
  readonly label: string;
}
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
interface GPUShaderModule {
  readonly label: string;
}
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
interface GPUBindGroupLayout {
  readonly label: string;
}
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
}

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
interface GPUBindGroup {
  readonly label: string;
}
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
interface GPUPipelineLayout {
  readonly label: string;
}
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
  readonly label: string;
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
  multisample?: GPUMultisampleState;
}

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
  readonly label: string;

  /**
   * @throws {@link DOMException} `NotSupportedError` when `depthStencilAttachment`,
   * `occlusionQuerySet`, or `timestampWrites` is set.
   * @throws {@link DOMException} `InvalidStateError` when the encoder is finished
   * or a pass is already open.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/beginRenderPass)
   */
  beginRenderPass(descriptor: GPURenderPassDescriptor): GPURenderPassEncoder;

  /**
   * @throws {@link DOMException} `NotSupportedError` Compute passes are not implemented.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/beginComputePass)
   */
  beginComputePass(descriptor?: GPUComputePassDescriptor): GPUComputePassEncoder;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandEncoder/copyTextureToBuffer)
   */
  copyTextureToBuffer(
    source: GPUImageCopyTexture,
    destination: GPUImageCopyBuffer,
    copySize: GPUExtent3D,
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

/** Placeholder. {@link GPUCommandEncoder.beginComputePass} throws. */
interface GPUComputePassDescriptor {
  label?: string;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCommandBuffer) */
interface GPUCommandBuffer {
  readonly label: string;
}
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
 * `setIndexBuffer`, viewport, scissor, and indirect draws are absent.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder)
 */
interface GPURenderPassEncoder {
  readonly label: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/setPipeline) */
  setPipeline(pipeline: GPURenderPipeline): void;

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

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPURenderPassEncoder/draw) */
  draw(
    vertexCount: number,
    instanceCount?: number,
    firstVertex?: number,
    firstInstance?: number,
  ): void;

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

  readonly label: string;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GPUCanvasContext/canvas) */
  readonly canvas: OffscreenCanvas | Yuzuko.WindowSurface;

  /**
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
 * secure-context gate, so `audioWorklet` is available. Node types outside
 * this surface are absent.
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

  /**
   * Decodes WAV, FLAC, MP3, or Ogg Vorbis.
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

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/BaseAudioContext/createPeriodicWave) */
  createPeriodicWave(
    real: Float32Array,
    imag: Float32Array,
    constraints?: PeriodicWaveConstraints,
  ): PeriodicWave;
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
 * No output device is opened. `baseLatency` is `renderQuantumSize / sampleRate`.
 * `outputLatency` is reported by the context.
 *
 * @throws {@link DOMException} `NotSupportedError` when `sampleRate` is outside
 * 3000 through 768000, when `renderSizeHint` is an integer outside 1 through
 * `floor(6 * sampleRate)`, or when `sinkId` is a non-empty string.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioContext)
 */
interface AudioContext extends BaseAudioContext {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioContext/baseLatency) */
  readonly baseLatency: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioContext/outputLatency) */
  readonly outputLatency: number;

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
  getChannelData(channel: number): Float32Array;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBuffer/copyFromChannel) */
  copyFromChannel(destination: Float32Array, channelNumber: number, bufferOffset?: number): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBuffer/copyToChannel) */
  copyToChannel(source: Float32Array, channelNumber: number, bufferOffset?: number): void;
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
 * Assigning `value` works. The automation timeline throws `NotSupportedError`.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam)
 */
interface AudioParam {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/value) */
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
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/automationRate)
   */
  automationRate: AutomationRate;

  /**
   * @throws {@link DOMException} `NotSupportedError` Automation is not implemented.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/setValueAtTime)
   */
  setValueAtTime(value: number, startTime: number): AudioParam;

  /**
   * @throws {@link DOMException} `NotSupportedError` Automation is not implemented.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/linearRampToValueAtTime)
   */
  linearRampToValueAtTime(value: number, endTime: number): AudioParam;

  /**
   * @throws {@link DOMException} `NotSupportedError` Automation is not implemented.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/exponentialRampToValueAtTime)
   */
  exponentialRampToValueAtTime(value: number, endTime: number): AudioParam;

  /**
   * @throws {@link DOMException} `NotSupportedError` Automation is not implemented.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/setTargetAtTime)
   */
  setTargetAtTime(target: number, startTime: number, timeConstant: number): AudioParam;

  /**
   * @throws {@link DOMException} `NotSupportedError` Automation is not implemented.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/setValueCurveAtTime)
   */
  setValueCurveAtTime(values: Iterable<number>, startTime: number, duration: number): AudioParam;

  /**
   * @throws {@link DOMException} `NotSupportedError` Automation is not implemented.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioParam/cancelScheduledValues)
   */
  cancelScheduledValues(cancelTime: number): AudioParam;

  /**
   * @throws {@link DOMException} `NotSupportedError` Automation is not implemented.
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
 * Connecting an output to another {@link AudioParam} throws `NotSupportedError`.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode)
 */
interface AudioNode extends EventTarget {

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

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/channelCountMode) */
  channelCountMode: ChannelCountMode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/channelInterpretation) */
  channelInterpretation: ChannelInterpretation;

  /**
   * @throws {@link DOMException} `NotSupportedError` when `destination` is an
   * {@link AudioParam}.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/connect)
   */
  connect(destinationNode: AudioNode, output?: number, input?: number): AudioNode;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioNode/disconnect) */
  disconnect(): void;
  disconnect(output: number): void;
  disconnect(destinationNode: AudioNode): void;
  disconnect(destinationNode: AudioNode, output: number): void;
  disconnect(destinationNode: AudioNode, output: number, input: number): void;
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

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBufferSourceNode/playbackRate) */
  readonly playbackRate: AudioParam;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioBufferSourceNode/detune) */
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
  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioWorklet/port) */
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
 * A video frame.
 *
 * `drawImage`, `createImageBitmap`, and `copyExternalImageToTexture` accept a
 * frame. `close()` releases it. A later draw fails. `postMessage` of a frame
 * throws `DataCloneError`. `VideoColorSpace` is absent, so `colorSpace` is a
 * string (`"srgb"` or `"display-p3"`) rather than that interface.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame)
 */
interface VideoFrame {

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/format)
   */
  readonly format: VideoPixelFormat | null;

  /**
   * `"srgb"` or `"display-p3"`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/colorSpace)
   */
  readonly colorSpace: "srgb" | "display-p3";

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/codedWidth) */
  readonly codedWidth: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/codedHeight) */
  readonly codedHeight: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/displayWidth) */
  readonly displayWidth: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/displayHeight) */
  readonly displayHeight: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/timestamp) */
  readonly timestamp: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/duration) */
  readonly duration: number | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoFrame/close) */
  close(): void;
}
declare var VideoFrame: {
  prototype: VideoFrame;
  new(): VideoFrame;
};


/**
 * An encoded video chunk.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedVideoChunk)
 */
interface EncodedVideoChunk {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedVideoChunk/type) */
  readonly type: EncodedVideoChunkType;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedVideoChunk/timestamp) */
  readonly timestamp: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedVideoChunk/byteLength) */
  readonly byteLength: number;
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
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/decode)
   */
  decode(chunk: EncodedVideoChunk): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/flush) */
  flush(): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/reset) */
  reset(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/VideoDecoder/close) */
  close(): void;
}
declare var VideoDecoder: {
  prototype: VideoDecoder;
  new(init: VideoDecoderInit): VideoDecoder;
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
 * Decoded audio samples.
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
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/copyTo)
   */
  copyTo(destination: BufferSource, options: AudioDataCopyToOptions): void;

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
  data: BufferSource;
}

/** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioData/copyTo) */
interface AudioDataCopyToOptions {
  planeIndex: number;
  frameOffset?: number;
  frameCount?: number;
  format?: AudioSampleFormat;
}

/**
 * An encoded audio chunk.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedAudioChunk)
 */
interface EncodedAudioChunk {

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedAudioChunk/type) */
  readonly type: EncodedAudioChunkType;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedAudioChunk/timestamp) */
  readonly timestamp: number;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/EncodedAudioChunk/byteLength) */
  readonly byteLength: number;
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
   * @throws {@link DOMException} `NotSupportedError` when `codec` is not `mp4a`.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/configure)
   */
  configure(config: AudioDecoderConfig): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/decode) */
  decode(chunk: EncodedAudioChunk): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/flush) */
  flush(): Promise<void>;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/reset) */
  reset(): void;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/AudioDecoder/close) */
  close(): void;
}
declare var AudioDecoder: {
  prototype: AudioDecoder;
  new(init: AudioDecoderInit): AudioDecoder;
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
  readable: ReadableStream<string | Uint8Array>;
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
 * `datagramsReadableType: "bytes"` throws `NotSupportedError`. Any other
 * string throws `TypeError`. A non-empty `protocols` list throws
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
 * `readable` and `writable` are `ReadableStream` and `WritableStream`.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransportBidirectionalStream)
 */
interface WebTransportBidirectionalStream {
  readable: ReadableStream<Uint8Array>;
  writable: WritableStream<Uint8Array>;
}

/**
 * Datagram duplex exposed by {@link WebTransport.datagrams}.
 *
 * This is a plain object. Chunks are `Uint8Array` values.
 */
interface WebTransportDatagrams {
  readable: ReadableStream<Uint8Array>;
  writable: WritableStream<Uint8Array>;
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
 * `{ readable, writable }`. Chunks are `Uint8Array` values. `reliability`
 * is `"pending"` until `ready` resolves.
 *
 * @throws {@link DOMException} `SyntaxError` when the URL is invalid, the
 * scheme is not `https`, or the URL has a fragment.
 * @throws {@link TypeError} When `options` is not an object, `protocols` is
 * not an array, `headers` is not a `HeadersInit`, `congestionControl` is
 * not `"default"`, `"throughput"`, or `"low-latency"`,
 * `datagramsReadableType` is a string other than `"bytes"`, a certificate
 * hash is malformed, or an anticipated incoming-stream count is not an
 * unsigned short.
 * @throws {@link DOMException} `NotSupportedError` when
 * `datagramsReadableType` is `"bytes"`, `protocols` is non-empty, `headers`
 * is a `Headers` object or a non-empty sequence or record, or
 * `allowPooling` is true with a non-empty `serverCertificateHashes` list.
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
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/incomingBidirectionalStreams)
   */
  readonly incomingBidirectionalStreams: ReadableStream<WebTransportBidirectionalStream>;

  /**
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/WebTransport/incomingUnidirectionalStreams)
   */
  readonly incomingUnidirectionalStreams: ReadableStream<ReadableStream<Uint8Array>>;

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
  ): Promise<WritableStream<Uint8Array>>;
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
 * `structuredClone` can carry. A {@link Yuzuko.Window} cannot be transferred.
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
 * A haptic actuator that does not play effects.
 *
 * @throws {@link TypeError} Illegal constructor.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadHapticActuator)
 */
interface GamepadHapticActuator {

  /**
   * Always empty.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadHapticActuator/effects)
   */
  readonly effects: readonly GamepadHapticEffectType[];

  /**
   * @throws {@link DOMException} `InvalidStateError` when the gamepad is disconnected.
   * @throws {@link DOMException} `NotSupportedError` when it is connected.
   * Haptic effects are not played.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadHapticActuator/playEffect)
   */
  playEffect(type: GamepadHapticEffectType, params?: GamepadEffectParameters): Promise<GamepadHapticsResult>;

  /**
   * @throws {@link DOMException} `InvalidStateError` when the gamepad is disconnected.
   * @throws {@link DOMException} `NotSupportedError` when it is connected.
   *
   * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadHapticActuator/pulse)
   */
  pulse(value: number, duration: number): Promise<GamepadHapticsResult>;

  /**
   * Resolves `"complete"` while the gamepad is connected.
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
  readonly position: Float32Array | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/linearVelocity) */
  readonly linearVelocity: Float32Array | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/linearAcceleration) */
  readonly linearAcceleration: Float32Array | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/orientation) */
  readonly orientation: Float32Array | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/angularVelocity) */
  readonly angularVelocity: Float32Array | null;

  /** [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadPose/angularAcceleration) */
  readonly angularAcceleration: Float32Array | null;
}

/**
 * Declared for {@link Gamepad.touches}, which stays empty.
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/GamepadTouch)
 */
interface GamepadTouch {
  touchId: number;
  surfaceId: number;
  position: Float32Array;
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
