# Yuzuko

Desktop JavaScript runtime built on rusty_v8. It has no WebView and no HTML document. Scripts draw their own pixels and present them in native windows.

This file is the product spec. The implementation contract is `AGENTS.md` at the repository root. Intentional divergences from the web platform live in `docs/deviations.md`.

## Product

`yuzuko run game.ts` or `yuzuko run game.js` evaluates an ES module in V8. TypeScript is erased by oxc before V8 parses it. Startup does not open a window. Each `new Yuzuko.Window()` opens one native window.

Standard APIs live on `globalThis`. APIs that are in neither the web platform nor WinterTC live on `Yuzuko`. There is no global `Window` constructor.

The runtime provides:

- Multiple `Yuzuko.Window` objects. Each is an input `EventTarget`. Its `surface` is the `Yuzuko.WindowSurface` presented on screen.
- Pointer, wheel, keyboard, and IME composition events delivered to that surface. IME is off by default.
- `createImageBitmap` for JPEG, PNG, WebP, and JPEG XL.
- Canvas 2D through vello, on the GPU when the environment can use one and on the CPU otherwise. WebGPU uses the process wgpu device.
- Presenting only that surface at the end of its own `requestAnimationFrame` rendering update.
- The WinterTC Minimum Common API (the ECMA-429 2025 snapshot; the draft is dated 31 July 2026), as specified below.
- Dedicated workers, Web Audio, hardware H.264 and AAC decoding, packaging, and automatic updates.

## Platforms

The supported targets are:

| Triple | OS | Architecture |
| --- | --- | --- |
| `aarch64-apple-darwin` | macOS | arm64 |
| `x86_64-apple-darwin` | macOS | Intel |
| `x86_64-pc-windows-msvc` | Windows | x86_64 |
| `aarch64-unknown-linux-gnu` | Linux | arm64 |
| `x86_64-unknown-linux-gnu` | Linux | x86_64 |

Window, GPU, and input code uses winit and wgpu so every target stays reachable. A shared feature is not implemented behind `target_os = "macos"` alone. An OS-specific gap, such as an IME or Display P3 surface difference, is a row in `docs/deviations.md` with the OS name.

## Out of scope

Do not add these as a side effect. Update this section before adding one.

- WebView, CEF, an HTML parser, CSS, a DOM tree, `document`, `HTMLCanvasElement`, and navigation
- WebGL and WebGL2. `getContext("webgl")` and `getContext("webgl2")` return `null`
- Node compatibility, an npm client, `node:` builtins, CommonJS evaluation, and JSX or TSX. TypeScript runs by type erasure, not by type checking
- A dependency on `deno` or `deno_core`.
- Permission prompts. A shipped app runs as a trusted local application
- WebRTC, WebXR, and Keyboard Lock
- `HTMLVideoElement`, container demux, and a media presentation clock
- Frames after the first frame of an animated image, and CSS `@font-face`
- Context menus
- A dedicated save-data API. Update this section before adding one

## Namespaces

| Place | Contents |
| --- | --- |
| `globalThis` | WinterTC, plus web-standard constructors and functions, including `OffscreenCanvas`, `createImageBitmap`, `PointerEvent`, and `navigator.gpu` |
| `Yuzuko` | Runtime-specific APIs. The members include `Window` and `canvasRenderer`. `Yuzuko.test` exists only under `yuzuko test`. `Yuzuko.Menu` and `Yuzuko.setMenu` install the application menu. On the main global, `Yuzuko.fonts` is the `FontFaceSet` and `globalThis.fonts` is absent. A dedicated worker exposes that set as `self.fonts` and `globalThis.fonts` instead. `Yuzuko.updater` is installed only on a packaged application's main global |

`globalThis` is the execution realm and inherits from `EventTarget`. `self === globalThis`. `onerror` and `onunhandledrejection` are delivered here. Pointer and key events are not.

`Yuzuko.canvasRenderer` is `"gpu"` or `"cpu"`. It is chosen once at process startup and does not change. `YUZUKO_CANVAS=cpu` or `YUZUKO_CANVAS=gpu` overrides the choice. A GPU request that fails to produce a device selects `"cpu"`.

## Execution model

The entry point is an ES module, in JavaScript or TypeScript. Classic scripts are not supported. Module top-level bindings do not leak onto `globalThis`.

```js
const win = new Yuzuko.Window({
  title: "yuzuko",
  width: 1280,
  height: 720,
});
const ctx = win.surface.getContext("2d");

win.addEventListener("pointerdown", (e) => {
  ctx.fillRect(e.offsetX, e.offsetY, 4, 4);
});

win.addEventListener("compositionupdate", (e) => {
  win.setCompositionCaret(24, win.clientHeight - 32, 200, 24);
});

win.requestAnimationFrame(function frame() {
  win.requestAnimationFrame(frame);
});

const tools = new Yuzuko.Window({ title: "tools", width: 400, height: 600 });
```

`Window` extends `EventTarget`, not `OffscreenCanvas`. `win.surface` is a `Yuzuko.WindowSurface`. It is not an `OffscreenCanvas` and cannot be transferred to a worker. It has the same drawing methods as `OffscreenCanvas`: `width`, `height`, `getContext`, `convertToBlob`, and `transferToImageBitmap`. `transferControlToOffscreen()` returns an `OffscreenCanvas` that presents in that window. Calling it again, or calling it after `getContext`, throws `InvalidStateError`. After it returns, drawing methods on the surface throw `InvalidStateError`. The returned canvas is what a worker receives. The constructor `width` and `height` are CSS pixels and default to 1280×720 when omitted. `x` and `y` are the frame's initial origin in CSS pixels. When both are omitted the window is centered on the primary monitor, or placed at `0, 0` when that monitor does not exist. If only one of `x` or `y` is set, the other is `0`. `resizable` defaults to `false` and can be assigned after creation. `decorations` and `active` are creation-only and default to `true`. `transparent` defaults to `false` and is creation-only. It asks the OS to composite the window's alpha. Unpainted pixels and the area outside the fitted bitmap are clear instead of black. `visible` defaults to `true`; `false` creates the window hidden. `maximized` defaults to `false`. `minWidth`, `minHeight`, `maxWidth`, and `maxHeight` limit the client size. An omitted edge is unset. Setting only one maximum uses 100000 CSS pixels for the other edge when telling the OS. `resizeTo` and `resizeBy` clamp into those limits. The initial `width` and `height` are clamped the same way. There is no `reload`. The constructor `title` is the initial OS title. `WindowSurface.title` reads and writes that title. `Window` has no `title` attribute.

| Member | Meaning |
| --- | --- |
| `surface` | The `WindowSurface` drawn inside this window. Its bitmap does not follow later window resizes |
| `resizable` | Whether the OS window can be resized. The default is `false`. Assigning it updates the live window |
| `clientWidth` / `clientHeight` | Window client size in CSS pixels |
| `devicePixelRatio` | That window's winit scale factor |
| `surface.title` | OS window title. The constructor `title` sets the initial value. The default is `"yuzuko"` |
| `cursor` | `"default"`, `"pointer"`, `"crosshair"`, `"text"`, `"none"`, and the other winit cursor names |
| `screenX` / `screenY` | Client origin in CSS pixels. `screenLeft` and `screenTop` are the same values |
| `innerX` / `innerY` | Client origin. Same as `screenX` / `screenY` |
| `outerX` / `outerY` | Frame origin in CSS pixels, including the title bar |
| `visible` | Whether the OS window is shown. The constructor defaults to `true`. `false` creates it hidden until `show()` |
| `alwaysOnTop` | Keep the window above normal windows. The default is `false` |
| `decorations` | Whether the OS window has a title bar and borders. Set only in the constructor. The default is `true` |
| `active` | Whether the window is focused when it opens. Set only in the constructor. The default is `true` |
| `maximized` | Whether the window opens maximized. Not fullscreen. The default is `false` |
| `transparent` | Composite the window's alpha. Set only in the constructor. The default is `false` |
| `minWidth` / `minHeight` | Minimum client size in CSS pixels. Unset when omitted |
| `maxWidth` / `maxHeight` | Maximum client size in CSS pixels. Unset when omitted |
| `requestAnimationFrame` / `cancelAnimationFrame` | Runs before this window's rendering update |
| `resizeTo(width, height)` | Sets the client size in CSS pixels, clamped to `minWidth` / `maxWidth` and `minHeight` / `maxHeight`. Does not resize `surface` |
| `resizeBy(x, y)` | Adds CSS pixels to the client size, with the same clamp. Does not resize `surface` |
| `moveTo(x, y)` / `moveBy(x, y)` | Move the frame in CSS pixels. Positive `y` is downward |
| `show()` / `hide()` | Show or hide the OS window |
| `focus()` | Focus this window |
| `close()` | Dispatches a cancelable `close` event on this window |
| `alert(message)` | Shows a modal dialog with OK. The message is `ToString`. Blocks until the user dismisses it |
| `confirm(message)` | Shows a modal dialog with OK and Cancel. Returns `true` for OK |
| `prompt(message, defaultValue)` | Shows a modal dialog with a text field, OK, and Cancel. Returns the entered string, or `null` when cancelled. An omitted `defaultValue` is `""` |
| `setImeAllowed(allowed)` | IME for this window. The default is `false` |
| `setCompositionCaret(x, y, width, height)` | IME candidate rectangle, in CSS pixels relative to this window's client origin |
| `requestPointerLock()` / `exitPointerLock()` | Pointer lock for this window |
| `requestFullscreen()` / `exitFullscreen()` | Fullscreen for this window |
| `matchMedia(query)` | A `MediaQueryList` for this window's CSS size, `devicePixelRatio`, and display preferences. A `change` event fires when the result changes |

`WindowSurface.objectFit` is `"scale-down"` by default. `"none"`, `"contain"`, `"cover"`, and `"fill"` are the other values. The surface bitmap is centered in the window. Pixels outside that rectangle are black. `"none"` does not scale. `"contain"` fits the whole bitmap inside. `"cover"` covers the window and may crop. `"fill"` stretches to the window. `"scale-down"` is `"none"` when the bitmap already fits, and `"contain"` otherwise.

`WindowSurface.imageRendering` is `"auto"` by default. `"smooth"`, `"crisp-edges"`, and `"pixelated"` are the other values. `"auto"` and `"smooth"` sample bilinearly. `"crisp-edges"` samples the nearest pixel. `"pixelated"` scales to the nearest integer multiple with nearest-neighbor, then samples bilinearly to the final size. `"optimizeQuality"` is `"smooth"` and `"optimizeSpeed"` is `"pixelated"`. The property does not change a bitmap that is already the size of its fitted rectangle.

`WindowSurface.visibilityState` is `"visible"` or `"hidden"`. A new surface is `"visible"`. `visibilitychange` fires on the surface when the value changes. It does not bubble and cannot be cancelled. The state follows whether that OS window is on screen: minimized, hidden, or fully covered is `"hidden"`. Losing focus does not hide the surface. Closing the window sets `"hidden"` and fires `visibilitychange` when the surface was visible. `requestAnimationFrame` keeps running while the surface is hidden.

`new OffscreenCanvas(w, h)` is a buffer with no window. Assigning its `width` or `height` reallocates the buffer as the specification requires. Assigning `width` or `height` on a `WindowSurface` changes only that bitmap. The initial bitmap is device pixels: `round(cssWidth * devicePixelRatio)` by `round(cssHeight * devicePixelRatio)`. On a 2x display a 1280×720 window starts with a 2560×1440 surface. `clientX`, `clientY`, `pageX`, `pageY`, `offsetX`, and `offsetY` are CSS pixels from the window's top-left, with `y` growing downward.

`matchMedia` understands `screen` and `all`, `not` / `and` / `or`, and comma-separated lists. Width and height use `px`. `min-width` and `max-width` remain, and level 4 ranges such as `(width >= 800px)` and `(200px <= width <= 800px)` work for `width`, `height`, `resolution`, and `device-pixel-ratio`. `resolution` accepts `dppx`, `x`, `dpi`, and `dpcm`. `1dppx` is `devicePixelRatio`. `device-pixel-ratio` and `-webkit-device-pixel-ratio` are the same value, including the `min-` and `max-` prefixes. Also recognized: `orientation`, `color-gamut` (`srgb`, `p3`, `rec2020`), `prefers-color-scheme`, `prefers-reduced-motion`, `prefers-contrast`, and `pointer`. An unknown feature or a broken query does not match. `MediaQueryList` and `MediaQueryListEvent` cannot be constructed with `new`.

There is no global `requestAnimationFrame` on the main global. A frame belongs to a surface. A dedicated worker has `requestAnimationFrame` and `cancelAnimationFrame`. A frame there runs the callback, then copies each transferred window canvas into the window that owns it.

`requestIdleCallback(callback)` and `cancelIdleCallback(handle)` are on the main `globalThis`. `IdleDeadline` cannot be constructed with `new`. An idle period is at most 50ms. It also ends at the next timer or the next window animation frame when that comes first. The period does not start when less than 5ms remains before that timer or frame. A positive `timeout` still runs the callback then: `didTimeout` is true and `timeRemaining()` is 0. A callback scheduled during an idle period runs in a later period, and that later period does not start until the current deadline. Dedicated workers do not have these functions. A thrown callback is reported on `globalThis`.

### Frame order

For each window:

1. Dispatch that window's OS events to its `Window`, one at a time, and run a microtask checkpoint after each event.
2. Run the `requestAnimationFrame` callbacks already queued on that surface. Callbacks queued during those callbacks wait for the next frame.
3. Run a microtask checkpoint.
4. Update that surface. For WebGPU, follow "update the rendering of a WebGPU canvas": expire the current texture and present. For Canvas 2D, rasterize the recorded commands and present. For `bitmaprenderer`, present the transferred bitmap.
5. If another animation frame is queued, request a redraw of that window.

When only timers or idle callbacks remain, the event loop sleeps until the next timer or idle deadline.

`getCurrentTexture()` returns the same `GPUTexture` until that surface's rendering update for the frame. The next call after the update returns a new texture.

`close` is cancelable. If `preventDefault()` is not called, only that window is destroyed. Other windows and outstanding timers or promises remain. When no `Window` is open and the task queue is empty, the process exits with status 0. An uncaught exception and an unhandled rejection exit with status 1. A promise microtask runs before the next macrotask. Posting a V8 foreground task wakes the event loop.

### Headless mode

`YUZUKO_HEADLESS=1` makes `Window` skip OS window creation. The drawing buffer and animation frames (60 Hz) still run on the object. `yuzuko test` creates windows headless. Input injection is `Yuzuko.__test`, installed only for that command. `yuzuko run` does not install it. Web-platform tests run under `cargo test`, not under a `yuzuko` subcommand.

## Input

OS events come from winit 0.30. The mapping below is fixed.

`PointerEvent` is the source of truth. Its target is that window's `WindowSurface`, not `globalThis`. The path is the surface, then the `Window`. Capturing listeners on the window run first, then listeners on the surface, then bubbling listeners on the window when the event bubbles. `stopPropagation()` skips the rest of that path. `stopImmediatePropagation()` also skips the remaining listeners on the current object. During dispatch, `composedPath()` is `[surface, window]`. The `pointerId` space is per window. UI Events then derives `mousemove`, `mousedown`, `mouseup`, `click`, `dblclick`, `auxclick`, and `contextmenu` on that same path. `gotpointercapture` and `lostpointercapture` use it too. `WheelEvent` is dispatched on the `Window`.

A `PointerEvent` carries:

- `pointerId`, `pointerType` (`"mouse"`, `"pen"`, `"touch"`), and `isPrimary`
- `clientX` / `clientY`, `screenX` / `screenY`, and `offsetX` / `offsetY` (CSS pixels from that surface's origin)
- `button` and `buttons`
- `pressure`, `tangentialPressure`, `tiltX`, `tiltY`, `twist`, `width`, and `height`. Missing OS values are 0. A mouse uses `pressure` 0.5 while a button is down and 0 otherwise
- `movementX` / `movementY`, which are relative while the pointer is locked
- `ctrlKey`, `shiftKey`, `altKey`, `metaKey`, and `timeStamp`
- `getCoalescedEvents()` on `pointermove`, returning the moves coalesced into that frame
- `setPointerCapture`, `releasePointerCapture`, and `hasPointerCapture` on that surface

`pointerrawupdate` is not implemented.

Pointer lock is `surface.requestPointerLock()` and returns a promise. Success dispatches `pointerlockchange` on that surface. Failure dispatches `pointerlockerror` on that surface. The implementation is winit cursor grab plus hiding the cursor. Lock is per window.

Fullscreen is `surface.requestFullscreen()`. Leaving it is `surface.exitFullscreen()`.

### Keyboard and IME

`keydown` and `keyup` are `KeyboardEvent`s. The fields are `key`, `code`, `location`, `repeat`, `isComposing`, the four modifiers, and `timeStamp`. `key` comes from the winit logical key. `code` maps the physical key to a UI Events code. The map lives in `yuzuko_window`. An unknown key is `"Unidentified"`.

IME does not run while that surface's `setImeAllowed` is `false`. The default is `false`. A text field calls `setImeAllowed(true)` and `setCompositionCaret(...)`.

winit `Ime` maps as follows:

| winit | DOM |
| --- | --- |
| `Enabled` | `compositionstart` with `data === ""` |
| `Preedit(text, range)` | `compositionupdate` with `data === text` |
| `Commit(text)` | `compositionend` with `data === text` |
| `Disabled` | `compositionend` when a composition is open |

While the preedit string is non-empty, the corresponding `keydown` and `keyup` have `isComposing === true`. After `compositionend`, one `beforeinput` with `inputType` `"insertText"` and the committed string is dispatched on that surface. `beforeinput` during composition uses `insertCompositionText`. The constructors `KeyboardEvent`, `CompositionEvent`, `InputEvent`, `PointerEvent`, `MouseEvent`, and `WheelEvent` are web standards and live on `globalThis`.

winit and the host OS can disagree with browsers, including a Windows `key` of `"Process"` and `compositionstart` arriving on the other side of `keydown`. Record each observed difference in `docs/deviations.md` with the OS name. The acceptance bar is that the composing string and the committed string are readable from `data`.

If `keydown` calls `preventDefault()`, that key does not produce a normal text input. A `Commit` the OS has already produced is still delivered as `compositionend`.

Focus changes dispatch `focus` and `blur` as `FocusEvent`s on that `Yuzuko.Window`. They do not bubble or cancel, and `relatedTarget` is null. `WindowSurface` does not receive them. Resize and scale-factor changes dispatch `resize` on the window. Size is read from `clientWidth` and `clientHeight`.

## Images

`createImageBitmap` accepts `Blob`, `ImageData`, `ImageBitmap`, and `OffscreenCanvas`, including the crop arguments `sx, sy, sw, sh`.

A `Blob` is identified by its signature.

| Format | Decoder |
| --- | --- |
| JPEG | `zune-jpeg`. EXIF orientation comes from `exif` |
| PNG | `png`, including `tRNS`. 8-bit samples stay unorm8. 16-bit samples are stored as float16 |
| WebP | `image-webp`, lossy and lossless, with alpha. Samples stay unorm8 |
| JPEG XL | `jxl-oxide` (pure Rust). Do not link `libjxl`. Sample depth above 8 bits is stored as float16 |

An `ImageBitmap` stores a color space and a sample format. The color space is `"srgb"` or `"display-p3"`. The sample format is unorm8 or float16. `ImageBitmap` does not expose those choices as attributes. `premultiplyAlpha` follows `"default"`, `"premultiply"`, and `"none"`. `imageOrientation` accepts `"from-image"` and `"flipY"`. `resizeWidth` / `resizeHeight` use nearest for `"pixelated"` and `"low"`, and triangle for `"medium"` and `"high"`.

`colorSpaceConversion: "none"` keeps an sRGB or Display P3 source in that space, including its sample format. `colorSpaceConversion: "default"` stores sRGB. A source profile that is neither sRGB nor Display P3 is converted to sRGB with `moxcms` for either setting. Conversion between sRGB and Display P3 uses the CSS Color 4 matrices: linearize, apply the matrix, then encode again. Float samples wider than float16 round to float16. Eight-bit sources stay unorm8.

`createImageBitmap` from an `ImageData` or a 2D canvas copies that object's color space and sample format. Float16 components are the encoded values of the stored color space, not linear-light values, and are not clamped to 0..1. `ImageData` stores alpha unpremultiplied.

Animated images contribute the first frame only. Input that cannot be decoded rejects with `InvalidStateError`. Decoding does not block the event loop: it runs on a dedicated thread, and the host object is created on the main thread afterward. A decoder panic is caught with `catch_unwind` and becomes `InvalidStateError`.

The limits are 16384 pixels on the longer edge and 100 million pixels in total. Exceeding either rejects with `InvalidStateError`.

`fetch` loads `file:`, `blob:`, `data:`, `http:`, and `https:`. A `file:` URL must stay under the entry module's directory. `http:` and `https:` follow redirects, retry a `421` once on a new connection, and cancel with `AbortSignal`. A blob URL `Range` answers `206` or a network error. This works:

```js
const url = new URL("./hero.jxl", import.meta.url);
const bitmap = await createImageBitmap(await (await fetch(url)).blob());
```

## OffscreenCanvas

`getContext` accepts `"2d"`, `"webgpu"`, and `"bitmaprenderer"`. A different kind from the one already created returns `null`. An unimplemented drawing method throws `NotSupportedError`.

### Canvas 2D

The rasterizer is vello. Text shaping uses parley and fontique. When `Yuzuko.canvasRenderer === "gpu"`, the scene runs on the process wgpu device and the result is written to that canvas texture. When there is no device, or vello's GPU renderer cannot be created on that device, the renderer is `"cpu"` and vello's CPU implementation produces the pixels. If a wgpu device exists, those pixels are uploaded to a texture and presented. If no device exists, they are presented through a CPU framebuffer. WebGPU does not use this fallback: `requestAdapter()` resolves to `null` when no adapter exists.

Vello paints an sRGB unorm8 target. `render_to_texture` takes `Rgba8Unorm`. Scene colors are converted to sRGB and packed to 8 bits before the draw. Image brushes are `Rgba8` or `Bgra8`. The CPU pixmap is premultiplied RGBA8. A Display P3 source color can be supplied, and vello converts it to sRGB while packing, which clips chroma outside sRGB.

`getContext("2d", { colorSpace, colorType })` still selects `"srgb"` or `"display-p3"`, and `"unorm8"` or `"float16"`. The defaults are `"srgb"` and `"unorm8"`. `getContextAttributes()` returns them. The default context is the vello target.

A `display-p3` or `float16` context has its own backing store, owned here rather than by vello. On the GPU that store is `Rgba8Unorm` interpreted as Display P3, or `Rgba16Float` in the canvas color space. Without a device it is a CPU buffer of the same layout. `putImageData` and a `drawImage` whose source already matches the destination write that store and keep the samples. Path fills, strokes, text, and gradients are painted by vello in sRGB unorm8 and then converted into the store, so those paints lose chroma outside sRGB and precision beyond 8 bits.

`fillStyle` and `strokeStyle` accept the existing sRGB forms plus `color(srgb ...)` and `color(display-p3 ...)`, with or without alpha. Gradient stops accept the same forms. An unrecognized color does not change the current style. `getImageData`, `putImageData`, and `createImageData` take `ImageDataSettings`. `colorSpace` is `"srgb"` or `"display-p3"`. `pixelFormat` is `"rgba-unorm8"`, stored in a `Uint8ClampedArray`, or `"rgba-float16"`, stored in V8's `Float16Array`. A buffer of the other typed-array kind throws `InvalidStateError`. `getImageData` converts when the requested color space or pixel format differs from the backing store. Float16 `ImageData` keeps values outside 0..1.

Present uses an explicit wgpu `SurfaceColorSpace`, never `Auto`. An sRGB unorm8 canvas uses `Srgb`. A Display P3 unorm8 canvas uses `DisplayP3` when the surface capabilities list that pair, and is converted to sRGB for present otherwise. A float16 canvas uses `ExtendedDisplayP3` or `ExtendedSrgb` with `Rgba16Float` only when that pair is advertised. Otherwise present quantizes it to the sRGB unorm8 swapchain. `Auto` on `Rgba16Float` resolves to linear scRGB, which is a different encoding.

Conformance pixel tests always use the CPU renderer so GPU vendors do not perturb the result.

Level A is:

- State stack: `save`, `restore`, `reset`, `resetTransform`
- Transforms: `scale`, `rotate`, `translate`, `transform`, `setTransform`, `getTransform`
- `globalAlpha`. `globalCompositeOperation` accepts `"source-over"` only. Any other value throws `NotSupportedError`
- Styles: CSS color strings, `color(srgb ...)` and `color(display-p3 ...)`, linear gradients, radial gradients, and `createPattern` from `ImageBitmap` or `OffscreenCanvas` with all four repeat modes
- Lines: `lineWidth`, `lineCap`, `lineJoin`, `miterLimit`, `setLineDash`, `getLineDash`, `lineDashOffset`
- Rectangles and paths: `fillRect`, `strokeRect`, `clearRect`, `rect`, `beginPath`, `closePath`, `moveTo`, `lineTo`, `quadraticCurveTo`, `bezierCurveTo`, `arc`, `arcTo`, `ellipse`, `roundRect`, `fill`, `stroke`, `clip`. Fill rules are nonzero and evenodd
- `Path2D`, `isPointInPath`, `isPointInStroke`
- All three `drawImage` overloads. Sources are `ImageBitmap` and `OffscreenCanvas`
- `imageSmoothingEnabled`. `false` or quality `"low"` selects nearest. Anything else selects linear
- `getImageData`, `putImageData`, `createImageData`, including `colorSpace` and `pixelFormat`. `getImageData` may stall on a GPU readback
- Text: `font`, `lang`, `textAlign`, `textBaseline`, `direction`, `fillText`, `strokeText`, `measureText`. Families are `sans-serif`, `serif`, and `monospace`, mapped to the OS UI font. CJK glyphs must draw. `lang` is a BCP 47 tag, `""`, or `"inherit"` (the default) and is part of the canvas state. It selects localized glyphs when the text is shaped
- Enhanced text metrics on `TextMetrics`: `getSelectionRects(start, end)`, `getActualBoundingBox(start, end)`, `getTextClusters()`, `getTextClusters(start, end, options)`, and `getIndexFromOffset(offset)`. Indices are UTF-16 code units. `TextCluster` exposes `x`, `y`, `start`, `end`, `align`, and `baseline`. `fillTextCluster` and `strokeTextCluster` draw one cluster
- Layers: `beginLayer()` and `endLayer()`. `beginLayer` takes an optional dictionary `{ filter }`. Draws between the calls are one group. The explainers are [Enhanced Text Metrics](https://github.com/fserb/canvas2D/blob/master/spec/enhanced-textmetrics.md) and [Layers](https://github.com/fserb/canvas2D/blob/master/spec/layers.md)

`filter` is a CSS filter string, default `"none"`. A value that parses as `none` or a `<filter-value-list>` is stored, and the getter returns that assigned string. `none` is case-insensitive. `""`, `inherit`, `initial`, `unset`, `revert`, `revert-layer`, `null`, and `undefined` do not parse, so the current filter stays. `calc()` is not accepted. There is no `CanvasFilter` class. `beginLayer({ filter })` uses the same CSS syntax. `null`, `undefined`, and `""` omit the argument. Any other unparsable value, including a filter dictionary, throws `TypeError` and does not open a layer.

`beginLayer` and `endLayer` share a stack with `save` and `restore`. `restore` throws `InvalidStateError` when the top entry is a layer. `endLayer` throws `InvalidStateError` when the top entry is a save or the stack is empty. `reset` discards open layers, and a later `endLayer` throws. An open layer is not rasterized. `endLayer` composites it. Entering a layer sets `globalAlpha` to `1`, `filter` to `"none"`, and the clip list to empty. The saved `globalAlpha` and clips then apply to that bitmap. The current transform continues inside the layer and is restored on `endLayer`. `setTransform` and `resetTransform` replace the matrix until the layer ends. The current path is not saved. A parent `translate` therefore moves draws inside the layer, and a clip around the layer clips the bitmap rather than surviving `endLayer` as a context clip.

vello does not run CSS filters, so `ctx.filter` and a layer `filter` do not change pixels. `shadow*` still throws `NotSupportedError`, and a composite operation other than `source-over` still throws `NotSupportedError`.

Outside level A (`shadow*`, a composite operation other than `source-over`, and an unregistered named font) throws `NotSupportedError`. Named fonts are resolved by `FontFace`. A size that names only an unregistered family throws `NotSupportedError`. CSS `@font-face` is not implemented. Extending past level A updates this list and `docs/deviations.md` in the same change. `YUZUKO_SKIP_GPU=1` reports `Yuzuko.canvasRenderer === "cpu"`.

Cluster positions are relative to the text origin after `textAlign` and `textBaseline`. `getTextClusters` options may set `align`, `baseline`, `x`, and `y` for the returned clusters and do not change the `TextMetrics` object. `getSelectionRects` uses advance boxes and the font ascent and descent. `getActualBoundingBox` uses glyph outline bounds. `getIndexFromOffset` increases from left to right in that origin space. Values outside the run return `0` or the UTF-16 length according to `direction`. `fillTextCluster` and `strokeTextCluster` use the measured font, direction, and `lang`, and an options dictionary may replace `align`, `baseline`, `x`, and `y` for that call without writing the cluster. The fill or stroke style, transform, alpha, and clips are the context's current values. The classic `actualBoundingBox*` getters stay advance approximations.

`convertToBlob` produces `image/png`, `image/jpeg`, and `image/webp`. JPEG XL encoding is out of scope. `putImageData` of a float16 Display P3 `ImageData` round-trips through `getImageData` within one float16 ulp. A path painted with `color(display-p3 ...)` is compared after vello's sRGB unorm8 conversion, not as a wide-gamut round trip. The same command list draws on both the GPU and CPU renderers.

### WebGPU

WebGPU is implemented on wgpu. The process has one default adapter. Canvas 2D uses the same `wgpu::Device`. Each `Window` has its own swapchain. `copyExternalImageToTexture` copies from `ImageBitmap` and `OffscreenCanvas` on the GPU, and takes a CPU path only when the formats do not match. The copy converts from the source color space into the destination `colorSpace`, `"srgb"` or `"display-p3"`. A float16 source is quantized to the destination texture format.

Every wgpu call in v1 happens on the main thread. `YUZUKO_SKIP_GPU=1` makes `requestAdapter` resolve `null`.

The WebGPU surface is:

- `navigator.gpu.requestAdapter`, `requestDevice`, and `getPreferredCanvasFormat`
- `GPUCanvasContext.configure`, `getCurrentTexture`, and `unconfigure`. `alphaMode` accepts `"opaque"` and `"premultiplied"`
- Shader modules, render pipelines, command encoders, and render passes
- Buffers, textures, samplers, bind groups, bind group layouts, `queue.writeBuffer`, and `queue.submit`
- `copyExternalImageToTexture` drawing an `ImageBitmap` or a `VideoFrame`
- `pushErrorScope` / `popErrorScope` for `"validation"` and `"out-of-memory"`. A validation failure reported through `popErrorScope` is a `GPUValidationError`
- `GPUDevice.lost` and `destroy`. `device.lost` resolves from `destroy`
- Adapter limits and features published in specification vocabulary
- Unimplemented methods throw `NotSupportedError`

Render bundles, compute, occlusion queries, timestamp queries, and a depth-stencil attachment throw `NotSupportedError`.

## WinterTC

The baseline is the common API index at <https://min-common-api.proposal.wintertc.org/>. `globalThis` is a WinterTC execution realm, not a `Window` interface. As WinterTC allows, added global properties do not have to be `readonly`.

The realm includes:

- `Event`, `EventTarget`, `CustomEvent`, `ErrorEvent`, `PromiseRejectionEvent`, `DOMException`
- `atob`, `btoa`, `setTimeout`, `clearTimeout`, `setInterval`, `clearInterval`, `queueMicrotask`, `reportError`, `structuredClone`, `performance`, `self`, `onerror`, `onunhandledrejection`, `onrejectionhandled`, `navigator.userAgent`, and `console` `log` / `info` / `warn` / `error` / `debug` / `trace` / `assert` / `time` / `timeLog` / `timeEnd` / `count` / `countReset` / `group` / `groupCollapsed` / `groupEnd` / `dir`
- `AbortController`, `AbortSignal`
- `TextEncoder`, `TextDecoder`, `URL`, `URLSearchParams`, `Blob`, `File`
- V8's built-in `WebAssembly`, including `compileStreaming` and `instantiateStreaming`. Direct `.wasm` imports and `import source` of `.wasm`
- `fetch`, `Headers`, `Request`, and `Response` for `file:`, `blob:`, `data:`, `http:`, and `https:`
- `createImageBitmap`, `ImageBitmap`, `ImageData`
- `OffscreenCanvas`, `CanvasRenderingContext2D`, `ImageBitmapRenderingContext`, `Path2D`
- WebGPU and `navigator.gpu`
- The streams interfaces, including byte-stream readers, `ReadableStream.from`, owning streams that transfer `ArrayBuffer` and `MessagePort`, `TextEncoderStream`, and `TextDecoderStream`
- `FormData`, `CompressionStream`, `DecompressionStream`
- `URLPattern`
- `Crypto`, `CryptoKey`, `SubtleCrypto`, `crypto`, including Ed448 and X448
- `MessageChannel`, `MessagePort`, `MessageEvent`
- `caches`

`fetch` for `http:` and `https:` follows redirects, retries a `421` once on a new connection, cancels with `AbortSignal`, and answers a blob URL `Range` with `206` or a network error. A cached request does not keep the caller's `AbortSignal`. Shared `DOMMatrix` methods are inherited from `DOMMatrixReadOnly`. `WebKitCSSMatrix` is not defined.

Dedicated workers are available. WinterTC does not require them.

Streams, URLPattern, and compression algorithms may be implemented in embedded JavaScript when the tests pass. `EventTarget`, the event classes, `DOMException`, `AbortController`, `URL`, `URLSearchParams`, `Blob`, and `File` are `#[op]` classes. Windows, canvases, WebGPU, decoding, sockets, timers, URL parsing, encoding, Blob bytes, and the crypto boundary are Rust.

`structuredClone` covers the WinterTC built-ins in this runtime, plus `ArrayBuffer`, typed arrays, `Map`, `Set`, `Date`, `RegExp`, and `Error`. An unsupported host object throws `DataCloneError`.

## Architecture

```text
main thread
  winit event loop (N windows)     headless timer
         |                                |
         +------------ LoopDriver --------+
                        |
                   yuzuko_core
                   V8 isolate + cppgc
                   task queue, timers, microtasks
                        |
         +--------------+--------------+
         |              |              |
   yuzuko_window   yuzuko_canvas   yuzuko_webgpu
   Window           vello gpu/cpu   wgpu WebGPU
   pointer/key/IME       \          /
                          wgpu Device
```

The main realm's isolate stays on the main thread. A dedicated worker runs on its own thread and enters only its own isolate. winit stays on the main thread. wgpu stays on the thread that created the device. Other background completions are posted to a task queue and wake that realm's event loop. Only the main thread touches winit.

There is one V8 platform per process. Posting a foreground task always hits the waker. Timer delays do not spawn an OS thread per timer.

Crates:

| Crate | Role |
| --- | --- |
| `yuzuko` | CLI. `run`, `test`, `package`, and `repl`. Calls each extension's `install` |
| `yuzuko_core` | Isolate, platform, event loop, module loader (`oxc_resolver`, oxc type erasure), extension trait, timers, console, exception conversion |
| `yuzuko_dom` | `EventTarget` and event classes |
| `yuzuko_winter` | URL, encoding, Blob, fetch, crypto, streams |
| `yuzuko_window` | `Yuzuko.Window`, winit, input mapping, IME, pointer lock, fullscreen |
| `yuzuko_image` | Decoders and `ImageBitmap` / `createImageBitmap` |
| `yuzuko_canvas` | `OffscreenCanvas`, Canvas 2D, `bitmaprenderer` |
| `yuzuko_webgpu` | WebGPU |
| `yuzuko_audio` | Web Audio |
| `yuzuko_codec` | WebCodecs |

`yuzuko_core` does not depend on winit or wgpu. Feature crates depend on core, and the binary assembles the feature crates.

Host objects are allocated with cppgc so V8 owns their lifetime. GPU resources that have `destroy()` are destroyed explicitly and do not wait for GC.

## Modules

Every static import, dynamic `import()`, and entry path goes through `oxc_resolver`, then, for TypeScript, through `oxc_transformer`. The loader does not invoke `tsc` or the oxc CLI. `import.meta.url` is the absolute `file:` URL of the source file that was resolved, including a `.ts` or `.mts` URL.

Resolution options:

- Conditions are `import` and `default`. The `types` and `require` conditions are not used, so a resolver result is never a `.d.ts` or a CommonJS entry chosen by `require`.
- Extensions searched are `.ts`, `.mts`, `.js`, `.mjs`, `.json`, and `.wasm`.
- `extensionAlias` maps a `.js` specifier to `.ts` then `.js`, and a `.mjs` specifier to `.mts` then `.mjs`. `import "./foo.js"` loads `foo.ts` when that file exists.
- `tsconfig.json` discovery, `extends`, project references, `baseUrl`, and `paths` are oxc_resolver's. `compilerOptions.target`, `module`, `jsx`, and `strict` do not change loading.
- A bare specifier resolves through `package.json` `"imports"` and `"exports"` when oxc_resolver finds a file. Nothing is downloaded. `node:` throws `TypeError`. A result whose module type is CommonJS, an addon, or a declaration file throws `TypeError`.
- JSON is loaded only with `with { type: "json" }`. A `.wasm` evaluation import is linked as described below.

### WebAssembly modules

The loader loads `.wasm` as an ES module, in two forms: a direct import and `import source`.

A direct import instantiates the module and exposes its exports:

```js
import { add } from "./add.wasm";
```

`await import("./add.wasm")` does the same. The namespace follows the WebAssembly ES module integration. A function export is a JavaScript function. A memory export is a `WebAssembly.Memory`. A table export is a `WebAssembly.Table`. A global export is the value it holds.

Imports declared inside the Wasm module are specifiers. The loader resolves each one relative to the `.wasm` file with `oxc_resolver`, evaluates that module, and passes its exports as the import object. The target may be JavaScript, TypeScript, or another `.wasm` module. A missing export or a type mismatch fails instantiation and rejects the module graph.

A source-phase import compiles the bytes and does not instantiate them:

```js
import source mod from "./add.wasm";
// mod instanceof WebAssembly.Module
```

The Wasm module's own imports are not evaluated for a source-phase import. `WebAssembly.instantiate` is how that `Module` is instantiated later. oxc parses `import source` and leaves it in the graph. A source-phase import of JavaScript or TypeScript throws `TypeError`.

The pinned `v8` must report the module request phase to the host resolver. Invalid Wasm bytes reject evaluation. Dedicated workers use this same loader.

TypeScript source (`.ts`, `.mts`) is parsed with oxc and passed through oxc's TypeScript transform before V8 compiles the module. The transform erases types, interfaces, type aliases, type-only imports and exports, assertions, `satisfies`, and generics. It also rewrites runtime TypeScript syntax that erasure cannot delete: `enum`, `namespace`, and parameter properties. JavaScript syntax is not downleveled. The JSX transform is off, and `.tsx` fails to load.

A type-only import is gone before V8 resolves specifiers, so its target is not fetched. Import specifiers that remain are left unchanged. oxc does not rewrite `.ts` extensions to `.js` in the output. The host resolver sees the original specifier.

Erasure is cached in memory for the process, keyed by path, modified time, and length. The loader does not write a `.js` file next to the source. oxc's codegen source map is attached to the V8 script origin, so an exception stack uses the TypeScript path and line.

Parse and transform failures reject module evaluation with the source path and span. Dynamic `import()` and workers use this same loader.

Image decoders are pure Rust. TLS uses rustls. HTTP uses hyper. Versions are pinned in `Cargo.lock`. The oxc crates used for resolution and erasure stay on one version. A `v8` upgrade, and an oxc upgrade, are each their own change.

## CLI

```text
yuzuko run <entry.js|entry.ts>
yuzuko test [<path>...] [--filter <substring>]
yuzuko package <entry.js|entry.ts> [--config <file>] [--output <path>]
yuzuko repl
```

`yuzuko repl` starts one interactive session in the same realm as `yuzuko run`. `console.log` prints. `Yuzuko` is present. `Yuzuko.test` is absent. A session that never constructs a window opens none. `yuzuko` with no command prints usage and does not start a session.

The prompt is `> `. Each complete chunk is evaluated, its completion value is printed, and queued microtasks and already-due macrotasks run before the next prompt. A timer that schedules itself again with delay 0 runs once for that chunk and does not keep the prompt waiting. `1 + 1` prints `2`. A TypeScript annotation is erased before evaluation, so `const labeled: number = 3` then `labeled` prints `3`. Top-level `await` prints the resolved value. `await import()` of a relative `.ts` file loads it the same way `yuzuko run` loads a module, from the current directory. A thrown exception is reported and the next chunk still runs. EOF on an empty line exits 0.

An unclosed `(`, `{`, `[`, string, template literal, or block comment keeps the continuation prompt `... ` and is not evaluated. A trailing `.` that continues a member access does the same. A finished invalid chunk reports a syntax error, and the next chunk still runs.

On a terminal, Tab completes names from this realm. `Ma` offers `Math`. `Math.` offers properties including `abs`. A binding created by an earlier chunk in the same session is offered. A name that does not exist is not invented. Up and Down recall chunks submitted in this session. History ends when the process exits. A pipe reads complete lines and treats EOF as the end of the session. The editor does not pump the event loop while it waits for a key. REPL chunks are not ES modules. See `docs/deviations.md`.

Rust unit tests use `cargo test`. Script tests use `Yuzuko.test`. Web-platform behavior uses upstream WPT, also under `cargo test`. There is no `yuzuko wpt` subcommand. WPT is a test of this runtime, not a product command.

## Tests

### `Yuzuko.test`

`Yuzuko.test` is installed only when the process is `yuzuko test`. `yuzuko run` does not have it.

```js
Yuzuko.test("addition", () => {
  if (1 + 1 !== 2) throw new Error("bad sum");
});

Yuzuko.test("async decode", async () => {
  const bitmap = await createImageBitmap(blob);
  if (bitmap.width !== 2) throw new Error("width");
});

Yuzuko.test({
  name: "macos window",
  ignore: Yuzuko.build.os !== "darwin",
  fn() {},
});

Yuzuko.test.ignore("later", () => {});
Yuzuko.test.only("focus this", () => {});
```

The call forms are `(fn)` when the function has a name, `(name, fn)`, `(name, options, fn)`, and `(options)`. `options` is `{ name, fn, ignore, only }`. `fn` may be async and receives a `TestContext`. `t.step(name, fn)` registers a nested step. A nested `Yuzuko.test` call throws. Registration happens while the module evaluates, before any test runs.

Discovery follows `*_test.js`, `*.test.js`, `*_test.ts`, and `*.test.ts`. The test runner loads those files through the same oxc resolver and TypeScript transform. Arguments are files or directories. `--filter` keeps tests whose names contain the substring.

The runner evaluates one file at a time in one isolate, in registration order. A thrown exception or a rejected promise fails that test. Ignored tests are reported and do not fail the run. If any test sets `only`, the run executes only those tests and then exits 1, so a focused test cannot stay green. The summary line is `ok | <passed> passed | <failed> failed | <ignored> ignored`. Exit status is 0 when every executed test passes and none set `only`. It is 1 when a test fails or any test set `only`.

There is no op or resource sanitizer. Those track a host-call layer this runtime does not have. The runner does fail a test that leaves a timer it started, or that produces an unhandled rejection after the test function settles.

`Yuzuko.build.os` is `"darwin"`, `"linux"`, or `"windows"`, and it is part of the test namespace, installed with `Yuzuko.test`.

Use `Yuzuko.test` for behavior WPT does not cover: `Window`, input mapping, IME event order, present, the canvas renderer fallback, the CLI, and timer and microtask order.

### WPT

Web APIs are tested with [web-platform-tests](https://github.com/web-platform-tests/wpt), upstream, not with copies of those tests.

`tests/wpt/suite` is a git submodule pinned to one upstream commit. Upgrading the pin is its own change, separate from a feature change. `tests/wpt/expectations/` holds one JSON file per top-level suite.

`cargo test -p yuzuko --test wpt` executes the enrolled testharness tests in a fresh realm per file. The runner loads upstream `testharness.js` and reports each subtest. It runs `.any.js` variants and testharness documents that do not need a layout engine. Dedicated `.worker.js` files run in a worker. Files that need a DOM node, CSS layout, or a WebView stay in the expectation file as `{"ignore": true}`. Suites that need the network (`fetch`, and any test that loads an HTTP resource) use the WPT server from that cargo test. `url`, `encoding`, and the other enrolled directories that do not load an HTTP resource run without that server.

Expectation leaves use these values:

| Value | Meaning |
| --- | --- |
| `true` | Every subtest is expected to pass |
| `false` | The whole file is expected to fail |
| `{"expectedFailures": ["name"]}` | Those subtests fail. The rest pass |
| `{"ignore": true}` | Skip the file. `YUZUKO_WPT_NO_IGNORE=1` runs it anyway |

The cargo test fails when a result differs from the expectation, including a subtest that starts passing. `YUZUKO_WPT_UPDATE=1 cargo test -p yuzuko --test wpt` rewrites the expectation files to the results just observed. The next run then passes. `YUZUKO_WPT_NO_IGNORE=1` runs files marked `{"ignore": true}`. Update expectations in the same change that changes the results.

The first runner is single-process and sequential, because V8 and winit stay on the main thread. Splitting suites across processes, one top-level directory each, can come later.

A change that adds or changes a web API enrolls the upstream directories for that API and runs them. Tests in those directories that need an absent host stay ignored in expectations, with the reason in the review of that change.

The enrolled upstream directories are `url`, `encoding`, `hr-time`, `html/webappapis/atob`, `html/webappapis/timers`, `FileAPI`, `dom/events`, `wasm/jsapi`, `console`, constructor tests under `pointerevents` and `uievents` that do not need an element, `html/canvas` image and `createImageBitmap` tests that operate on blobs (plus the image fixtures those tests use), `html/canvas/offscreen` and the 2D context tests that do not need a displayed HTML canvas (run on the CPU renderer), `webgpu`, `fetch`, `streams`, `compression`, `urlpattern`, `WebCryptoAPI`, `mimesniff`, `wasm/webapi`, `css/geometry`, `gamepad`, `clipboard-apis`, `css/css-font-loading`, dedicated-worker tests that do not need a document, and `webaudio`.

`requestidlecallback` is enrolled with the main-global idle callbacks. `eventsource` and `websockets` are enrolled with those clients. `webcodecs` is enrolled with `VideoDecoder`, `AudioDecoder`, `VideoFrame`, `AudioData`, and `ImageDecoder`. The JPEG XL `ImageDecoder` documents are enrolled with that decoder. `dom/abort` is enrolled with `AbortSignal`. `html/webappapis/microtask-queuing` and `html/webappapis/structured-clone` are enrolled with `queueMicrotask` and `structuredClone`. `imagebitmap-renderingcontext` is enrolled with `ImageBitmapRenderingContext`. Files that need a document, a frame, or `requestAnimationFrame` on `globalThis` stay ignored or fail as recorded.

Pixel comparisons for vello output use the CPU renderer. Unorm8 sRGB pixels that did not cross a color-space boundary match exactly. A conversion from that output into another color space may differ by one unorm8 code point. `putImageData` and `ImageBitmap` float16 samples may differ by one float16 ulp. Where WPT has no reference, a `Yuzuko.test` golden image covers the case. The GPU renderer has a smoke test that one frame reaches a surface. Test both `YUZUKO_CANVAS=cpu` and the `"cpu"` fallback when the GPU is unavailable. WebGPU tests may skip when `YUZUKO_SKIP_GPU=1` and no GPU is available. A skip is not a failure.

Event order, the key map, and the IME state machine are `Yuzuko.test` tests. Decoders also have golden files, corrupt input, and limit checks beside the WPT cases.

## Gamepad

The web Gamepad API. `navigator.getGamepads()` returns the connected gamepads. `gamepadconnected` and `gamepaddisconnected` fire on `globalThis`, and so do `ongamepadconnected` and `ongamepaddisconnected`. These do not live on `Yuzuko`. Physical devices are read with gilrs while the event loop runs, and an already-connected device is exposed without waiting for a button press. Haptic `playEffect` and `pulse` reject `NotSupportedError`. `GamepadPose` reports no pose.

## Clipboard

`navigator.clipboard` `writeText`, `readText`, and `ClipboardItem` for `text/plain` and `image/png`. Reads and writes succeed only after a pointer or key press on the focused `Window`. The same calls reject with `NotAllowedError` without that action, and when the window is not focused. There is no permission prompt. `copy`, `cut`, and `paste` are dispatched on that surface. `ClipboardChangeEvent` is absent.

## Menu bar

`Yuzuko` is an `EventTarget`. `Yuzuko.Menu` and `Yuzuko.setMenu(menu)` install one application menu with `muda`. A selection delivered through the same input path as pointer and key events dispatches `menuclick` on `Yuzuko` and carries the item's `action` string. macOS has one menu for the app. On Windows the same menu is attached to the focused window. On Linux, winit does not expose a GTK window, so `setMenu` throws `NotSupportedError`. Context menus stay out of scope.

## Tray and dock

`Yuzuko.Tray` is an `EventTarget`. `new Yuzuko.Tray()` adds a status item. `setIcon(bytes, options)` takes PNG bytes. `options.template` asks macOS to draw that icon as a template image. `setTooltip(text)`, `setMenu(menu)`, and `close()` update the item. `menu` is a `Yuzuko.Menu` and may contain top-level actions. `bounds` is `{ x, y, width, height }` in CSS pixels, or `null` when the system cannot report the rectangle. A primary click dispatches `click` on that tray. A menu selection dispatches `menuclick` on that tray with `action`. An open tray keeps the process alive. The constructor throws `NotSupportedError` when no status item can be created.

`Yuzuko.dock` is one `EventTarget` for the process. `setBadge(text)` sets the badge, and `null` or `""` clears it. `setVisible(false)` hides the app from the macOS Dock and the app switcher. `setMenu(menu)` installs the Dock menu. `bounce()` requests attention once, and `bounce(true)` requests it until the app is focused. `menuclick` and `reopen` are dispatched on `Yuzuko.dock`. `reopen` carries `hasVisibleWindows`. Writes the system cannot do throw `NotSupportedError`. A dedicated worker has no `Tray` and no `dock`. There is no tray panel.

## FontFace

`FontFace` lives on `globalThis`. There is no `document`, so the `FontFaceSet` is `Yuzuko.fonts`, not `globalThis.fonts`. A dedicated worker exposes `self.fonts` and `globalThis.fonts` as that worker's set. Sources are an `ArrayBuffer` and a URL that `fetch` can load. Formats are TTF, OTF, WOFF, and WOFF2. A family that has finished `load()` and is in `Yuzuko.fonts` can be named from the Canvas 2D `font` property. The generic families remain. `serif` and `monospace` still draw. A size that names only an unregistered family throws `NotSupportedError`. `globalThis.fonts` and `document` stay absent on the main global. CSS `@font-face` is not implemented.

## Workers

Dedicated workers only. `new Worker(url)` and `new Worker(url, { type: "module" })`. The worker global is a dedicated-worker scope: `self === globalThis`. It has the WinterTC subset already on the main global, plus `postMessage`, `onmessage`, `importScripts`, `navigator`, `location`, `close`, `onerror`, `btoa`, `atob`, the timer functions, `requestAnimationFrame`, and `cancelAnimationFrame`. It has no `Yuzuko.Window`, no `document`, and no `SharedWorker`. A classic worker script and `importScripts` run. The application entry stays an ES module. A worker may start another dedicated worker. An evaluation error and `terminate()` are reported to the owner and stop further worker tasks. `postMessage` carries values that `structuredClone` can carry, including a transferred `ArrayBuffer` and a `MessagePort`. The worker keeps running while the owner is busy.

On the worker global, `self.fonts` and `globalThis.fonts` are the same `FontFaceSet`. A face that finishes `load()` there is usable from that set. On the main global, `globalThis.fonts` stays absent and `Yuzuko.fonts` remains the main `FontFaceSet`. A face added to one set is not visible in the other.

In a worker, `DOMMatrix` and `DOMMatrixReadOnly` do not expose the Window stringifier or the CSS-string constructor. `String(new DOMMatrix())` is `"[object DOMMatrix]"`. `String(new DOMMatrixReadOnly())` is `"[object DOMMatrixReadOnly]"`. `new DOMMatrix("matrix(1, 0, 0, 1, 0, 0)")` and `new DOMMatrix("")` throw `TypeError`. `setMatrixValue` is absent. On the main global, `String(new DOMMatrix())` stays `"matrix(1, 0, 0, 1, 0, 0)"`. `WebKitCSSMatrix` stays absent in both realms.

An `OffscreenCanvas` transferred with `postMessage` arrives usable in the dedicated worker. A `"2d"` context can draw, and the sender's canvas is detached. A `Yuzuko.Window` and a `Yuzuko.WindowSurface` cannot be transferred. Transferring a `Yuzuko.Window` throws `DataCloneError`. They stay usable on the main thread. `WindowSurface.transferControlToOffscreen()` returns the `OffscreenCanvas` a worker draws. A worker `requestAnimationFrame` callback then copies that canvas into the window that owns it. A WebGPU device can be used only on the thread that created it. Using or transferring it from the other side fails.

Shared workers, service workers, and worklets stay out of scope.

## Web Audio

`AudioContext` and `OfflineAudioContext` live on `globalThis`. `decodeAudioData` turns WAV, FLAC, MP3, and Ogg Vorbis into an `AudioBuffer` through pure-Rust decoders. An `AudioBufferSourceNode` plays that buffer. `GainNode`, `OscillatorNode`, and `ConstantSourceNode` run in the same graph into an `AudioDestinationNode`. `OfflineAudioContext.startRendering()` resolves an `AudioBuffer`. `suspend` pauses at a render quantum boundary, with `currentTime` equal to that boundary, and `resume` continues. A buffer through gain `0.5` renders at half amplitude. There is no audio output device. The rendered buffer is the signal.

`renderSizeHint` on `AudioContextOptions` and `OfflineAudioContextOptions` is `"default"`, `"hardware"`, or an integer. `BaseAudioContext.renderQuantumSize` is that block length for the life of the context, and `AudioWorkletGlobalScope.renderQuantumSize` is the same value. Omitted and `"default"` are 128. An integer from 1 through floor(6 × sampleRate), including 1 and a non-power of two, is that exact length. Any other integer throws `NotSupportedError`. `baseLatency` is larger when the hint is 4096 than when it is 128. An `AudioWorklet` `process` uses that same length, so a hint of 256 makes `process` see length 256. Offline `"hardware"` is 128. Realtime `"hardware"` is the default output device's current buffer size in frames when the OS reports one inside 1 through floor(6 × sampleRate), and 128 when it does not. That size is already visible at construction. On this Mac the query is Core Audio `kAudioDevicePropertyBufferFrameSize`. Windows and Linux still use 128. `baseLatency` is `renderQuantumSize / sampleRate`. The three-argument `OfflineAudioContext` constructor uses 128.

`AudioWorklet.addModule` evaluates a processor script. `registerProcessor` defines it. An `AudioWorkletNode` calls `process` once per quantum with `Float32Array` channels of length `renderQuantumSize`. The node port and the processor port exchange messages. `currentFrame`, `currentTime`, `sampleRate`, and `renderQuantumSize` are visible in the worklet. A `parameterDescriptors` entry is an `AudioParam`, and `process` receives that parameter's block. Setting `AudioParam.value` works. The automation timeline (`setValueAtTime`, ramps, and curves) throws `NotSupportedError`.

`AudioBuffer`, `PeriodicWave`, `AudioParam`, `AudioParamMap`, `AudioNode`, `AudioScheduledSourceNode`, `AudioDestinationNode`, `GainNode`, `AudioBufferSourceNode`, `OscillatorNode`, `ConstantSourceNode`, `AudioWorklet`, and `AudioWorkletNode` are part of this surface. `BaseAudioContext` is not constructable. Update this section before adding more node types.

## Server messages

`EventSource` and `WebSocket` live on `globalThis`, including a dedicated worker. `EventSource` reads an `http:` or `https:` `text/event-stream` response and dispatches `MessageEvent`s. The default reconnection time is 3000ms. `retry` replaces it. `close()` stops the stream. `withCredentials` is stored on the object.

`new WebSocket(url)` opens a `ws:` or `wss:` connection. The second argument is a protocol string, a sequence of protocol strings, or a `WebSocketInit` dictionary `{ protocols, targetAddressSpace }`. `targetAddressSpace` is `"loopback"`, `"local"`, or `"public"`. The connection fails unless the connected address is in that space. Any other value, including the legacy alias `"private"`, throws `TypeError`. A further argument is ignored. `send` accepts a string, a `BufferSource`, or a `Blob`. `binaryType` is `"blob"` or `"arraybuffer"`. `close(code, reason)` uses code 1000 or a code from 3000 through 4999. `extensions` stays `""`.

`new WebSocketStream(url, options)` opens the same kind of connection. `options.protocols` is a sequence of protocol strings. `options.signal` aborts the handshake. `opened` resolves to `{ readable, writable, protocol, extensions }`. Text messages are strings and binary messages are `Uint8Array` values. `closed` resolves to `{ closeCode, reason }` after a clean close and rejects with `WebSocketError` after an unclean close. `close({ closeCode, reason })` sends that close. A readable `pull` reads one message, and further socket reads wait, which applies receive backpressure.

## WebCodecs

A game draws video onto a canvas. There is no `HTMLVideoElement` and no media element. The runtime does not demux a container and does not advance frames by itself. `VideoDecoder`, `EncodedVideoChunk`, and `VideoFrame` live on `globalThis`. A game, or a JS demuxer, supplies the chunks. `drawImage` and `createImageBitmap` accept a `VideoFrame`. `copyExternalImageToTexture` copies a `VideoFrame`. `VideoFrame.close()` releases the frame. A later draw after `close()` fails.

`AudioDecoder`, `EncodedAudioChunk`, and `AudioData` live on `globalThis`. Samples copied from `AudioData` fill an `AudioBuffer` that an `AudioContext` can play.

The video codec is H.264 (`avc1` and `avc3`). The audio codec is AAC (`mp4a`). Both work on every target triple, not only through a macOS decoder. H.264 uses only that triple's platform hardware decoder: VideoToolbox on Darwin with `kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder`, a hardware Media Foundation transform on Windows, and VA-API on Linux. The Linux decoder submits picture and slice buffers with `vaBeginPicture`, `vaRenderPicture`, and `vaEndPicture`, then maps the surface. The Windows decoder reads the `ProcessOutput` sample. There is no software H.264 decoder and no software fallback when that decoder is missing or rejects the config. `hardwareAcceleration: "prefer-software"` does not select a software H.264 decoder. A structurally valid `avcC` that the hardware decoder rejects throws `NotSupportedError` and does not produce a frame. AAC is a portable decoder. Any other codec string, including `vp8`, `vp09`, `av01`, `opus`, and `vorbis`, fails `configure` with `NotSupportedError` and produces no frame or samples.

`ImageDecoder` lives on `globalThis`. `type` is an image MIME type. `data` is a `BufferSource` or a `ReadableStream` of `BufferSource` chunks. `isTypeSupported` resolves `true` for `image/png`, `image/jpeg`, `image/webp`, and `image/jxl`. Any other type resolves `false`. The constructor still returns a decoder for an unsupported type. `completed`, `tracks.ready`, and `decode()` then reject `NotSupportedError`. A supported still image has one track, `frameCount` 1, `animated` false, and `repetitionCount` 0. `decode()` resolves `{ image, complete }` with a `VideoFrame`. `frameIndex` other than 0 rejects with `RangeError`. Deselecting the track makes `decode()` reject `InvalidStateError`. `close()` rejects a pending `decode()` with `InvalidStateError`. `reset()` rejects one with `AbortError`. Decoding uses the same pure-Rust decoders as `createImageBitmap`, on a background thread, and a panic becomes `EncodingError`. `colorSpaceConversion` `"default"` stores sRGB and `"none"` keeps an sRGB or Display P3 source. `desiredWidth`, `desiredHeight`, and `preferAnimation` do not change the result. A truncated buffer that cannot be decoded rejects `completed` and `tracks.ready` with `EncodingError`.

`VideoEncoder` and `AudioEncoder` are absent. `postMessage` of a `VideoFrame` throws `DataCloneError`.

## Packaging

`yuzuko package` produces an application that starts from a double click. The bundle contains the runtime, the entry module graph, and the assets listed in the config. The targets are the five triples in Platforms. The macOS output is a `.app`. Windows and Linux outputs are directories. There is no WebView and no `backend` choice. The `Contents/MacOS` binary and opening the `.app` both run the packaged entry. `codesign --verify` succeeds for an ad-hoc signature. `import.meta.url` is a `file:` URL under `Contents/Resources/app/`. A listed asset outside the entry directory is fetched from that tree, and a `file:` URL outside it fails. An omitted window title is the config `name`. The launch directory stays the caller's current directory. Windows and Linux directories from the same packager store the AppUserModelID, the `.desktop` id, and the icon bytes.

The config file is `yuzuko.json` or `yuzuko.jsonc`. `yuzuko.jsonc` is JSON with `//` and `/* */` comments and with trailing commas. `yuzuko.json` is strict JSON. Discovery starts at the entry file's directory and walks toward the root. The nearest directory that contains either file wins. Both names in that directory is an error. The entry is the path argument. `yuzuko package` with no path fails before anything is written. `--config` names the file and skips the walk. `--output` replaces the output path for the host OS. A command-line value wins over the file.

Unknown keys are errors. There is no `exports` key. Paths are relative to the config file and must stay inside that directory.

```jsonc
{
  // Display name. The default window title when `title` is omitted.
  "name": "My Game",
  "version": "1.0.0",
  "identifier": "com.example.mygame",
  "icons": {
    "macos": "./icons/app.icns",
    "windows": "./icons/app.ico",
    "linux": "./icons/app.png"
  },
  "assets": ["./assets/**"],
  "output": {
    "macos": "./dist/MyGame.app",
    "windows": "./dist/MyGame",
    "linux": "./dist/my-game"
  },
  "macos": {
    "codesignIdentity": "-"
  },
  "release": {
    "baseUrl": "https://releases.example.com/my-game"
    // "publicKey" is standard base64 of the 32-byte Ed25519 public key.
  }
}
```

| Field | Meaning |
| --- | --- |
| `name` | Display name. Used as the default `Window` title, the macOS menu-bar name, and the bundle name. When omitted, the config directory's name is used |
| `version` | Stored in the bundle and compared by the updater. Dot-separated decimal integers from 0 through 2^64−1, at most 32 components, each at most 20 digits. A missing trailing component is 0, so `1.0` equals `1.0.0`. A missing version is `"0.0.0"`. Any other form fails `yuzuko package` before anything is written |
| `identifier` | Reverse-DNS id. macOS `CFBundleIdentifier`, the Windows AppUserModelID, and the Linux `.desktop` id. When omitted, `com.yuzuko.<slug>` from `name`. At least two labels. Each label starts with an ASCII letter and then contains letters, digits, or `-` |
| `icons.macos` | An `.icns` path, or an array of `{ "path", "size" }` PNGs assembled at package time. A missing file is an error. No entry means the bundle has no custom icon |
| `icons.windows` | An `.ico` path. PNGs are not assembled |
| `icons.linux` | A `.png` path, or the same PNG array as macOS |
| `assets` | Globs copied into the bundle, relative to the config file. The module graph is included without being listed. Paths that escape the config directory are errors |
| `output.macos` | A path ending in `.app` |
| `output.windows` | A directory path. `.msi` is not produced |
| `output.linux` | A directory path. `.deb`, `.rpm`, and `.AppImage` are not produced |
| `macos.codesignIdentity` | Passed to `codesign`. `"-"` or omission is an ad-hoc signature so the `.app` opens from Finder on this Mac. A named identity that `codesign` rejects fails the command |
| `release.baseUrl` | Absolute `http:` or `https:` URL. Stored in the bundle. Nothing is fetched by `yuzuko package`. The updater reads this URL and no other |
| `release.publicKey` | Standard base64 of a 32-byte Ed25519 public key. Written as raw bytes to `yuzuko.update-key`. Omission is allowed. A value that is not valid standard base64, not a valid Ed25519 point, or a low-order point fails `yuzuko package` before anything is written. The updater verifies with this key and never with a key from the feed |

An output path whose extension is `.dmg`, `.msi`, `.deb`, `.rpm`, or `.AppImage` fails the command. Those formats are not produced.

Inside the `.app`, the entry and the assets keep their paths relative to the config file, under `Contents/Resources/app/`. `import.meta.url` and `fetch` of a `file:` URL refer to that tree. The process current directory stays the directory from which the user launched the app.

Window position, size, `decorations`, `transparent`, and the other `Yuzuko.Window` options stay in the script. The config does not open a window. `yuzuko run` reads the same file only to fill an omitted window `title` from `name`.

There is no `backend` key. There is no `deepLinks` key. There is no `errorReporting` key. A bad identifier, a missing icon, an unknown key, an `exports` key, and both config names in one directory fail before anything is written.

## Automatic updates

Packaged applications only. `Yuzuko.updater` is installed on the main global when a packaged entry starts. It is absent for `yuzuko run`, `yuzuko repl`, `yuzuko test`, and a dedicated worker. `check()` and `download()` take no URL. Extra arguments are ignored. Nothing is fetched while `yuzuko package` runs, and nothing is fetched at process start. There is no `apply()` method, and the running process is not replaced.

`release.baseUrl` is the only feed URL. A package with no `release.baseUrl` does not contact a default host. `check()` reads `{baseUrl}/feed.json`, inserting a slash before `feed.json` when `baseUrl` has no trailing slash. A query or fragment on `baseUrl` is not sent. Userinfo is not sent. Redirects are not followed.

The feed document is JSON. Unknown fields are ignored.

```json
{
  "version": "1.2.0",
  "payload": "1.2.0.yuzuko",
  "signature": "<standard base64 of the 64-byte signature>"
}
```

`version` is a string in the form in the config table. `payload` is a relative path under `baseUrl`: no scheme, no leading slash, no empty, `.`, or `..` segment, no query, and no backslash. Each path is at most 1024 bytes. `signature` is standard base64, with padding, of the 64-byte Ed25519 signature of the exact payload bytes. A missing `signature`, a signature that is not that encoding, or a payload path that is not relative rejects `check()` and stages nothing.

`release.publicKey` is the trust anchor. `yuzuko package` writes the 32 raw bytes to `yuzuko.update-key`. On macOS that file is `Contents/Resources/yuzuko.update-key`. A Windows or Linux directory bundle stores it next to `yuzuko.bundle.json`. Apply and `download()` never write this file. The feed cannot supply a key. A package may omit the key. `check()` then throws `InvalidStateError` and does not open a connection.

Newer means the feed version is strictly greater than the manifest `version` under the numeric rules in the config table. `1.10` is greater than `1.9`. Leading zeros do not change a component's value.

`check()` returns a promise of `{ available, version }`. It does not download the payload. When the feed version is strictly greater, `available` is true, `version` is the feed version, and that result is remembered until the next successful `check()` in this process. When the feed version is equal or older, `available` is false, `version` is the packaged manifest version, and the remembered update is cleared. A thrown `check()` leaves the remembered update unchanged.

`download()` returns a promise of `undefined`. It downloads the payload remembered by the last successful newer `check()` in this process. With no remembered update it throws `InvalidStateError` and does not open a connection. The response must be HTTP 200. `Transfer-Encoding: chunked` is a failure. When `Content-Length` is present, end of stream before that many bytes is a truncated body. Without `Content-Length`, the body ends when the connection closes. A feed document larger than 1 MiB, or a payload larger than 512 MiB, is a failure. A response that does not finish within 30 seconds is a failure. `http:` and `https:` are both fetched. `https:` uses rustls and the webpki root set.

The signature is verified with `yuzuko.update-key` before anything is staged. The payload is this framing. Its version line must be the same string as the feed `version` and must be strictly greater than the manifest version.

```text
YUZUKO1
<version>
<entry>
<count>
```

Then, for each of the `count` files, a `/`-separated relative path, a decimal byte length, and exactly that many bytes. `count` is at most 100000. The entry path must name one of the files. Paths follow the same rules as the feed's `payload`. There is no trailing byte. Duplicate paths are rejected. A payload that does not verify, or that does not meet this framing, is not staged.

A verified payload is written as `Updates/payload` (the exact bytes) and `Updates/signature` (the raw 64-byte signature), under `Contents/Resources` on macOS and next to the manifest in a directory bundle. A failed check or download does not create or replace those files and does not change `app/`.

The next launch of that same packaged executable applies a staged update before it reads the entry. It verifies the staged bytes again with `yuzuko.update-key` and requires the payload version to be strictly greater than the manifest version. It then replaces the whole `app/` tree, sets the manifest `version` and `entry`, and deletes the stage. It does not replace the executable, `Info.plist`, icons, or `yuzuko.update-key`. The launch that called `download()` keeps running the entry it already loaded. A feed that is not newer does not replace the entry. A bad or missing signature, a transport failure, or a truncated body does not. When apply fails, the stage is deleted and that launch runs the previous entry.

| Failure | Exception | Message |
| --- | --- | --- |
| No `release.baseUrl` | `InvalidStateError` | There is no update feed. |
| No usable trust anchor | `InvalidStateError` | There is no update trust anchor. |
| Manifest version is not comparable | `InvalidStateError` | The packaged version cannot be compared. |
| `download()` with no remembered newer feed | `InvalidStateError` | There is no update to download. |
| Feed document, version, payload path, or signature encoding | `InvalidStateError` | The update feed is not valid. |
| Signature does not verify | `InvalidStateError` | The update signature is not valid. |
| Payload framing does not match | `InvalidStateError` | The update payload is not valid. |
| Status is not 200, the connection fails, the body is chunked, a limit is exceeded, or the response times out | `NetworkError` | The update could not be downloaded. |
| The connection closes before `Content-Length` bytes arrive | `NetworkError` | The connection closed before the body ended. |

## Risks

- A level A path or text operation that vello cannot draw throws `NotSupportedError` on both the GPU and CPU renderers. Do not switch that operation to a second 2D library. Vello's render target stays sRGB unorm8. A `display-p3` or `float16` backing store is not a vello target.
- winit IME differs by OS by more than the specification describes. Split event-sequence tests by OS.
- The WebGPU surface is the list in this file. Behavior outside that list is not implied.
