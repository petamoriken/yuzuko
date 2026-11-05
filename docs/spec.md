# Yuzuko

Desktop JavaScript runtime built on rusty_v8. It has no WebView and no HTML document. Scripts draw their own pixels and present them in native windows.

The default Cargo features include every surface below. Build features lists the JavaScript runtime that remains when no feature is enabled.

[overview.md](overview.md) introduces those surfaces by feature. This file is the product spec. The implementation contract is `AGENTS.md` at the repository root. Intentional divergences from the web platform live in [deviations.md](deviations.md).

## Contents

**Runtime**

- [Product](#product)
- [Platforms](#platforms)
- [Out of scope](#out-of-scope)
- [Build features](#build-features)
- [Namespaces](#namespaces)
- [Process](#process)

**Windows and input**

- [Execution model](#execution-model)
  - [Dialog labels](#dialog-labels)
  - [File picker](#file-picker)
  - [File drop](#file-drop)
  - [Frame order](#frame-order)
  - [Headless mode](#headless-mode)
- [Input](#input)
  - [Keyboard and IME](#keyboard-and-ime)
  - [Keyboard lock](#keyboard-lock)
- [Gamepad](#gamepad)
- [Clipboard](#clipboard)
- [Screen Wake Lock](#screen-wake-lock)
- [Menu bar](#menu-bar)
- [Context menu](#context-menu)
- [Tray and dock](#tray-and-dock)

**Drawing**

- [Images](#images)
- [OffscreenCanvas](#offscreencanvas)
  - [Canvas 2D](#canvas-2d)
  - [WebGPU](#webgpu)
- [FontFace](#fontface)

**Media**

- [Web Audio](#web-audio)
- [WebCodecs](#webcodecs)
- [Workers](#workers)

**Web platform**

- [WinterTC](#wintertc)
- [Internationalization](#internationalization)
- [Server messages](#server-messages)

**Accessibility**

- [Virtual accessibility nodes](#virtual-accessibility-nodes)
- [Computed accessibility tree](#computed-accessibility-tree)

**Host services**

- [File system](#file-system)
- [Datagrams](#datagrams)
- [TCP](#tcp)
- [HTTP](#http)
- [Signals](#signals)
- [Child processes](#child-processes)
- [Foreign functions](#foreign-functions)

**Modules, console, and tests**

- [Modules](#modules)
  - [WebAssembly modules](#webassembly-modules)
  - [Text and bytes modules](#text-and-bytes-modules)
  - [Deferred evaluation](#deferred-evaluation)
- [Console](#console)
- [CLI](#cli)
- [Tests](#tests)
  - [Yuzuko.test](#yuzukotest)
  - [WPT](#wpt)

**Packaging**

- [Packaging](#packaging)
- [Automatic updates](#automatic-updates)

**Implementation**

- [Architecture](#architecture)
- [Risks](#risks)

## Product

`yuzuko run game.ts` or `yuzuko run game.js` evaluates an ES module in V8. TypeScript is erased by oxc before V8 parses it. Startup does not open a window. Each `new Yuzuko.Window()` opens one native window.

Standard APIs live on `globalThis`. APIs that are in neither the web platform nor WinterTC live on `Yuzuko`. There is no global `Window` constructor.

The runtime provides:

- Multiple `Yuzuko.Window` objects. Each is an input `EventTarget`. Its `surface` is the `Yuzuko.WindowSurface` presented on screen.
- Pointer, wheel, keyboard, and IME composition events delivered to that surface. IME is off by default.
- `createImageBitmap` for JPEG, PNG, WebP, and JPEG XL.
- Canvas 2D through vello, on the GPU when the environment can use one and on the CPU otherwise. WebGPU uses the process wgpu device.
- Presenting that surface when its Canvas 2D or `bitmaprenderer` contents change, and at the end of a due `requestAnimationFrame` callback.
- The WinterTC Minimum Common API (the ECMA-429 2025 snapshot; the draft is dated 31 July 2026), as specified below.
- ECMA-402 `Intl` with the full ICU locale data linked into this V8 build.
- Dedicated workers in every build. Web Audio when the runtime is built with `audio`. Hardware H.264 and AAC decoding when it is built with `codec`. Packaging and automatic updates are part of the CLI.
- `Yuzuko` file-system methods when the runtime is built with the `fs` feature.

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
- Permission prompts. A shipped app runs as a trusted local application. Which optional surfaces are present is chosen when that app's runtime is built. See Build features
- WebRTC and WebXR
- `HTMLVideoElement` and a media presentation clock
- CSS `@font-face`

## Build features

Cargo features of the `yuzuko` package select optional surfaces. The default set enables every feature, so `cargo build -p yuzuko` is the full runtime. `cargo build -p yuzuko --no-default-features` is the JavaScript runtime alone. `cargo build -p yuzuko --no-default-features --features fetch,fs` selects a subset. `yuzuko package` copies the executable that invoked it and does not rebuild it, and there is no `yuzuko.json` key for features.

These names are compiled into every build. They are not Cargo features. `AbortController` and `AbortSignal` are in this set.

- `Event`, `EventTarget`, `CustomEvent`, `ErrorEvent`, `PromiseRejectionEvent`, `DOMException`
- `AbortController`, `AbortSignal`
- `atob`, `btoa`, `setTimeout`, `clearTimeout`, `setInterval`, `clearInterval`, `queueMicrotask`, `reportError`, `structuredClone`, `performance`, `console`, `navigator.userAgent`, `navigator.hardwareConcurrency`
- `TextEncoder`, `TextDecoder`, `URL`, `URLSearchParams`, `URLPattern`, `Blob`, `File`
- Streams, `FormData`, `CompressionStream`, `DecompressionStream`
- `Crypto`, `CryptoKey`, `SubtleCrypto`
- `MessageChannel`, dedicated workers, and a dedicated worker's `requestAnimationFrame` and `FileReaderSync`
- `requestIdleCallback` on the main realm
- `Intl` and `WebAssembly`
- The `Yuzuko` object on the main realm and in a dedicated worker. Members are attached by the features below

`Yuzuko.updater` stays in the CLI. It is installed only on a packaged application's main global. The updater's feed client is not one of these features.

| Feature | Installed names | Also enables |
| --- | --- | --- |
| `fetch` | `fetch`, `Headers`, `Request`, `Response`, `caches`, `XMLHttpRequest`, `EventSource`, and the HTTP client, including HTTP/3 | |
| `fs` | The `Yuzuko` file system | |
| `socket` | `WebSocket`, `WebSocketStream`, `WebTransport` | |
| `udp` | `Yuzuko.listenDatagram` and `Yuzuko.Datagram` | |
| `tcp` | `Yuzuko.listenTcp`, `Yuzuko.connectTcp`, `Yuzuko.TcpListener`, and `Yuzuko.TcpConn` | |
| `http` | `Yuzuko.listenHttp`, `Yuzuko.HttpServer`, and `Yuzuko.HttpRequest` | |
| `signal` | `Yuzuko.addSignalListener` and `Yuzuko.removeSignalListener` | |
| `child` | `Yuzuko.spawn` and `Yuzuko.Child` | |
| `ffi` | `Yuzuko.dlopen` and `Yuzuko.Library` | |
| `window` | `Yuzuko.Window`, pointer events including `pointerrawupdate`, key, and the other input events, the application menu, tray, dock, clipboard, gamepad, and screen Wake Lock. `OffscreenCanvas` is installed here | |
| `accessibility` | `Yuzuko.AccessibleNode`, `Yuzuko.ComputedAccessibleNode`, `WindowSurface.attachAccessibleRoot`, `WindowSurface.accessibleRoot`, and `Window.getComputedAccessibleNode` | `window` |
| `image` | `createImageBitmap`, `ImageBitmap`, `ImageData` | `window` |
| `canvas` | Canvas 2D, `bitmaprenderer`, `Path2D`, `DOMMatrix`, `FontFace`, `FontData`, `queryLocalFonts`, `Yuzuko.fonts`, `Yuzuko.canvasRenderer` | `image` |
| `webgpu` | `navigator.gpu` and the WebGPU surface | `image` |
| `audio` | Web Audio and `Yuzuko.audioOutputDevices` | |
| `codec` | `VideoDecoder`, `EncodedVideoChunk`, `VideoFrame`, `AudioDecoder`, `EncodedAudioChunk`, `AudioData`, `ImageDecoder`, `Yuzuko.demuxMp4` | `image` |

`image`, `canvas`, `webgpu`, `codec`, and `accessibility` enable `window`. `image`, `canvas`, `webgpu`, and `codec` do so because image sources share the window canvas snapshot. `audio`, `fetch`, `fs`, `socket`, `udp`, `tcp`, `http`, `signal`, `child`, and `ffi` do not. `fetch` and `socket` are separate: a build can include the HTTP client without `WebSocket`, and `WebSocket` without the HTTP client. The `http` feature is the server and does not enable `fetch` or `tcp`. The `signal` feature does not enable `window`. The `child` feature does not enable `window`. The `ffi` feature does not enable `window`. With `window` and without `canvas`, `getContext("2d")` and `getContext("bitmaprenderer")` return `null`. With `window` and without `webgpu`, `getContext("webgpu")` returns `null`. A name from a feature that was not enabled is absent on the main realm and in a dedicated worker. An `AudioWorklet` processor realm does not gain the file system, datagram sockets, TCP sockets, an HTTP server, signal listeners, a child process, foreign functions, or virtual accessibility nodes.

## Namespaces

| Place | Contents |
| --- | --- |
| `globalThis` | The JavaScript runtime in every build, plus the web-standard constructors for the enabled features. `OffscreenCanvas` and `PointerEvent` require `window`. `navigator.wakeLock` and `navigator.keyboard` require `window`. `createImageBitmap` requires `image`. `queryLocalFonts` and `FontData` require `canvas`. `navigator.gpu` requires `webgpu` |
| `Yuzuko` | Present on the main realm and in a dedicated worker in every build. Runtime-specific APIs live here. `Window` requires `window`. `canvasRenderer` and `fonts` require `canvas`. `Yuzuko.test` exists only under `yuzuko test`. `Yuzuko.Menu` and `Yuzuko.setMenu` install the application menu when `window` is enabled. `Yuzuko.setDialogLabels` sets dialog button labels when `window` is enabled, on the main realm only. On the main global, `Yuzuko.fonts` is the `FontFaceSet` and `globalThis.fonts` is absent. A dedicated worker exposes that set as `self.fonts` and `globalThis.fonts` instead. `Yuzuko.readFile`, `Yuzuko.FsFile`, and the other file-system members are installed when the runtime is built with `fs`, on the main global and in a dedicated worker. `Yuzuko.listenDatagram` and `Yuzuko.Datagram` are installed when the runtime is built with `udp`, on the main global and in a dedicated worker. `Yuzuko.listenTcp`, `Yuzuko.connectTcp`, `Yuzuko.TcpListener`, and `Yuzuko.TcpConn` are installed when the runtime is built with `tcp`, on the main global and in a dedicated worker. `Yuzuko.listenHttp`, `Yuzuko.HttpServer`, and `Yuzuko.HttpRequest` are installed when the runtime is built with `http`, on the main global and in a dedicated worker. `Yuzuko.addSignalListener` and `Yuzuko.removeSignalListener` are installed when the runtime is built with `signal`, on the main global and in a dedicated worker. `Yuzuko.spawn` and `Yuzuko.Child` are installed when the runtime is built with `child`, on the main global and in a dedicated worker. `Yuzuko.dlopen` and `Yuzuko.Library` are installed when the runtime is built with `ffi`, on the main global and in a dedicated worker. `Yuzuko.audioOutputDevices` is installed when the runtime is built with `audio`, on the main global and in a dedicated worker. `Yuzuko.demuxMp4` is installed when the runtime is built with `codec`, on the main global and in a dedicated worker. `Yuzuko.AccessibleNode`, `Yuzuko.ComputedAccessibleNode`, `WindowSurface.attachAccessibleRoot`, `WindowSurface.accessibleRoot`, and `Window.getComputedAccessibleNode` require `accessibility`. They are installed on the main realm only, are absent in a dedicated worker, and are not on `globalThis`. `Yuzuko.updater` is installed only on a packaged application's main global |

`Yuzuko.args`, `Yuzuko.env`, `Yuzuko.cwd`, `Yuzuko.chdir`, and `Yuzuko.exit` are installed with that object in every build, including a build with no Cargo features. A dedicated worker has them. An AudioWorklet processor context does not receive the `Yuzuko` object.

`globalThis` is the execution realm and inherits from `EventTarget`. `self === globalThis`, and assigning to `self` leaves that binding unchanged. `onerror` and `onunhandledrejection` are delivered here. A throw inside `queueMicrotask` is reported with `reportError` on that realm: the `error` event's `error` is the thrown value. During `yuzuko test` that throw fails the test. Pointer and key events are not.

`navigator.hardwareConcurrency` is present on the main global and in a dedicated worker in every build. It is the number of logical processors available to the process, and it is at least 1. When the operating system does not report a count, the value is 1.

`Yuzuko.canvasRenderer` is `"gpu"` or `"cpu"`. It is chosen once at process startup and does not change. `YUZUKO_CANVAS=cpu` or `YUZUKO_CANVAS=gpu` overrides the choice. A GPU request that fails to produce a device selects `"cpu"`.

## Process

`Yuzuko.args`, `Yuzuko.env`, `Yuzuko.cwd`, `Yuzuko.chdir`, and `Yuzuko.exit` belong to every build. The main realm and a dedicated worker each receive them when that realm starts. There is no permission prompt.

`Yuzuko.args` is a frozen array of strings. The property on `Yuzuko` is writable. Assigning it in one realm leaves the other realm's array unchanged. `push` and other mutations of the array throw `TypeError`. `yuzuko run` records the tokens after the entry, described in CLI. `yuzuko repl` and `yuzuko test` record an empty array. Their flags and paths are not script arguments. A packaged executable records the arguments it was given, except every token whose text starts with `-psn_`. macOS may insert those tokens in any position. A packaged entry cannot receive an argument that starts with `-psn_`. A dedicated worker receives the same strings this process recorded at launch.

`Yuzuko.env.get(name)` returns the stored string, or `undefined` when `name` is absent. `set(name, value)` stores a string. `delete(name)` removes `name`, and removing a name that is absent succeeds. `has(name)` returns a boolean. `toObject()` returns a plain object snapshot of the environment at that call. Later `set` and `delete` calls leave the object unchanged. `toObject` uses the spelling the operating system stores. The methods do not read `this`. Extra arguments are ignored. `get`, `delete`, and `has` have length 1. `set` has length 2. `toObject` has length 0.

`name` and `value` must be string primitives. Another type does not run `ToString`. A name that is not a string throws `TypeError` with `The name must be a string.` An empty name throws `The name must not be empty.` A name that contains `=` or NUL throws `The name must not contain `=` or NUL.` A value that is not a string throws `The value must be a string.` A value that contains NUL throws `The value must not contain NUL.` On Windows a lookup ignores ASCII case, and `set` of a name that differs only by that case updates the existing variable. On the other targets the name is case-sensitive. Bytes that are not UTF-8 become U+FFFD. The environment is process-wide. A change in one realm is visible to a later read in every realm.

`Yuzuko.cwd()` returns the process current directory as an absolute path. The string has no trailing separator except for a root. A component that is not UTF-8 uses U+FFFD for those bytes. When the operating system cannot report the directory, the call throws `DOMException` `OperationError` with `The current directory could not be read.` `cwd` has length 0.

`Yuzuko.chdir(directory)` sets that directory for the process. `directory` is a string or a `file:` URL. A relative string resolves against the current directory. An empty string names the current directory and succeeds without asking the operating system to change it. The directory may be inside a packaged bundle. The call does not modify the bundle. A path that is not a string or a file URL throws `TypeError` with `The path must be a string or a file URL.` Another URL scheme throws `The URL scheme must be file.` An unusable URL throws `The URL is invalid.` A `file:` URL that is not a path throws `The file URL is invalid.` A path that contains NUL throws `The path must not contain NUL.` A missing path throws `DOMException` `NotFoundError` with `The directory was not found.` A path that exists and is not a directory throws `InvalidStateError` with `The path is not a directory.` When the operating system denies the change, the call throws `NotAllowedError` with `The directory could not be changed.` Any other failure throws `OperationError` with the same message. Those messages do not include operating-system text. `chdir` has length 1. File-system paths are relative to this directory. See File system.

`Yuzuko.exit(code)` ends the process with status `code` and does not return. An omitted or `undefined` code is 0. `-0` is 0. The code is a number primitive from 0 through 255 whose fractional part is 0. A boolean, a string, `null`, a `Number` object, `NaN`, a non-integer, or a number outside that range throws `TypeError` with `The exit code must be an integer from 0 through 255.` and the process stays alive. The function does not apply `ToNumber`. `exit` has length 0. Extra arguments are ignored. `try` / `catch` does not observe a call that ends the process, and later statements in that turn do not run. The first call chooses the status. A later call keeps it. Open windows, trays, playing audio contexts, timers, workers, jobs, and pending promises do not keep the process alive after the call. A call from a dedicated worker ends the whole process, including the main realm, and does not dispatch an error event for that worker. During `yuzuko test` the run stops with that status and the summary line is not printed. During `yuzuko repl` the session ends with that status and does not print a completion value or an exception stack. A timer can call `exit` while a terminal, a Windows console, or a Unix pipe is waiting for input. A Windows pipe blocks until the next line, so a timer's `exit` during that read waits until the line arrives.

## Execution model

The entry point is an ES module, in JavaScript or TypeScript. Classic scripts are not supported. Module top-level bindings do not leak onto `globalThis`.

```js
const win = new Yuzuko.Window({
  title: "yuzuko",
  width: 1280,
  height: 720,
});
const ctx = win.surface.getContext("2d");
ctx.fillStyle = "#141820";
ctx.fillRect(0, 0, 1280, 720);

win.addEventListener("pointerdown", (e) => {
  ctx.fillRect(e.offsetX, e.offsetY, 4, 4);
});

win.addEventListener("compositionupdate", (e) => {
  win.setCompositionCaret(24, win.clientHeight - 32, 200, 24);
});

const tools = new Yuzuko.Window({ title: "tools", width: 400, height: 600 });
```

The `fillRect` above is presented on the next turn of the event loop. `requestAnimationFrame` is how a later frame draws again.

`Window` extends `EventTarget`, not `OffscreenCanvas`. `win.surface` is a `Yuzuko.WindowSurface`. It is not an `OffscreenCanvas` and cannot be transferred to a worker. It has the same drawing methods as `OffscreenCanvas`: `width`, `height`, `getContext`, `convertToBlob`, and `transferToImageBitmap`. `transferControlToOffscreen()` returns an `OffscreenCanvas` that presents in that window. Calling it again, or calling it after `getContext`, throws `InvalidStateError`. After it returns, drawing methods on the surface throw `InvalidStateError`. The returned canvas is what a worker receives. The constructor `width` and `height` are CSS pixels and default to 1280×720 when omitted. `x` and `y` are the frame's initial origin in CSS pixels. When both are omitted the window is centered on the primary monitor, or placed at `0, 0` when that monitor does not exist. If only one of `x` or `y` is set, the other is `0`. `resizable` defaults to `false` and can be assigned after creation. `decorations` and `active` are creation-only and default to `true`. `transparent` defaults to `false` and is creation-only. It asks the OS to composite the window's alpha. Unpainted pixels and the area outside the fitted bitmap are clear instead of black. `visible` defaults to `true`; `false` creates the window hidden. `maximized` defaults to `false`. `minWidth`, `minHeight`, `maxWidth`, and `maxHeight` limit the client size. An omitted edge is unset. Setting only one maximum uses 100000 CSS pixels for the other edge when telling the OS. `resizeTo` and `resizeBy` clamp into those limits. The initial `width` and `height` are clamped the same way. There is no `reload`. The constructor `title` is the initial OS title. `WindowSurface.title` reads and writes that title. `Window` has no `title` attribute.

| Member | Meaning |
| --- | --- |
| `surface` | The `WindowSurface` drawn inside this window. Its bitmap does not follow later window resizes |
| `resizable` | Whether the OS window can be resized. The default is `false`. Assigning it updates the live window |
| `clientWidth` / `clientHeight` | Window client size in CSS pixels |
| `devicePixelRatio` | That window's winit scale factor |
| `surface.title` | OS window title. The constructor `title` sets the initial value. The default is `"yuzuko"` |
| `cursor` | `"default"`, `"pointer"`, `"crosshair"`, `"text"`, `"none"`, and the other winit cursor names. Matching is case-insensitive. An unrecognized name is ignored |
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
| `requestAnimationFrame` / `cancelAnimationFrame` | Runs before this window's rendering update, at the refresh rate of the monitor that window is on. A Canvas 2D or `bitmaprenderer` change is presented on the next event-loop turn without this callback. After the window is closed, `requestAnimationFrame` returns `0` and does not queue a callback |
| `resizeTo(width, height)` | Sets the client size in CSS pixels, clamped to `minWidth` / `maxWidth` and `minHeight` / `maxHeight`. Does not resize `surface` |
| `resizeBy(x, y)` | Adds CSS pixels to the client size, with the same clamp. Does not resize `surface` |
| `moveTo(x, y)` / `moveBy(x, y)` | Move the frame in CSS pixels. Positive `y` is downward |
| `show()` / `hide()` | Show or hide the OS window |
| `focus()` | Focus this window |
| `close()` | Dispatches a cancelable `close` event on this window |
| `alert(message)` | Shows a modal dialog with one accept button. The label starts as OK. The message is `ToString`. Blocks until the user dismisses it |
| `confirm(message)` | Shows a modal dialog with accept and cancel buttons. The labels start as OK and Cancel. Returns `true` for the accept button |
| `prompt(message, defaultValue)` | Shows a modal dialog with a text field and accept and cancel buttons. The labels start as OK and Cancel. Returns the entered string, or `null` when cancelled. An omitted `defaultValue` is `""` |
| `pickFiles(options)` | Shows a modal open, save, or directory dialog. Returns absolute paths, or `null` when cancelled |
| `setContextMenu(menu)` | Stores the menu an uncancelled context click shows. `null` clears it. Does not pop a menu |
| `showContextMenu(menu, x, y)` | Shows `menu` at CSS pixels in the client area. Does not replace `setContextMenu` |
| `setImeAllowed(allowed)` | IME for this window. The default is `false` |
| `setCompositionCaret(x, y, width, height)` | IME candidate rectangle, in CSS pixels relative to this window's client origin |
| `requestPointerLock()` / `exitPointerLock()` | Pointer lock for this window |
| `requestFullscreen()` / `exitFullscreen()` | Fullscreen for this window |
| `matchMedia(query)` | A `MediaQueryList` for this window's CSS size, `devicePixelRatio`, and display preferences. A `change` event fires when the result changes |

`WindowSurface.objectFit` is `"scale-down"` by default. `"none"`, `"contain"`, `"cover"`, and `"fill"` are the other values. Matching is case-insensitive. Any other value is ignored. The surface bitmap is centered in the window. Pixels outside that rectangle are black. `"none"` does not scale. `"contain"` fits the whole bitmap inside. `"cover"` covers the window and may crop. `"fill"` stretches to the window. `"scale-down"` is `"none"` when the bitmap already fits, and `"contain"` otherwise.

`WindowSurface.imageRendering` is `"auto"` by default. `"smooth"`, `"crisp-edges"`, and `"pixelated"` are the other values. Matching is case-insensitive. Any other value is ignored. `"auto"` and `"smooth"` sample bilinearly. `"crisp-edges"` samples the nearest pixel. `"pixelated"` scales to the nearest integer multiple with nearest-neighbor, then samples bilinearly to the final size. `"optimizeQuality"` is `"smooth"` and `"optimizeSpeed"` is `"pixelated"`. The property does not change a bitmap that is already the size of its fitted rectangle.

`WindowSurface.visibilityState` is `"visible"` or `"hidden"`. A new surface is `"visible"`. `visibilitychange` fires on the surface when the value changes. It does not bubble and cannot be cancelled. The state follows whether that OS window is on screen: minimized, hidden, or fully covered is `"hidden"`. Losing focus does not hide the surface. Closing the window sets `"hidden"` and fires `visibilitychange` when the surface was visible. `requestAnimationFrame` keeps running while the surface is hidden.

`new OffscreenCanvas(w, h)` is a buffer with no window. `width` and `height` on an `OffscreenCanvas`, and the same attributes on a `WindowSurface`, are `[EnforceRange] unsigned long long`. A finite value is truncated toward zero. A non-finite value, or a value outside 0 through `2^53 - 1`, throws `TypeError`. Assigning either attribute reallocates that bitmap. On a `WindowSurface` the assignment does not change the window's CSS size. The initial surface bitmap is device pixels: `round(cssWidth * devicePixelRatio)` by `round(cssHeight * devicePixelRatio)`. On a 2x display a 1280×720 window starts with a 2560×1440 surface. `clientX`, `clientY`, `pageX`, `pageY`, `offsetX`, and `offsetY` are CSS pixels from the window's top-left, with `y` growing downward.

`matchMedia` understands `screen` and `all`, `not` / `and` / `or`, and comma-separated lists. Width and height use `px`. `min-width` and `max-width` remain, and level 4 ranges such as `(width >= 800px)` and `(200px <= width <= 800px)` work for `width`, `height`, `resolution`, and `device-pixel-ratio`. `resolution` accepts `dppx`, `x`, `dpi`, and `dpcm`. `1dppx` is `devicePixelRatio`. `device-pixel-ratio` and `-webkit-device-pixel-ratio` are the same value, including the `min-` and `max-` prefixes. Also recognized: `orientation`, `color-gamut` (`srgb`, `p3`, `rec2020`), `prefers-color-scheme`, `prefers-reduced-motion`, `prefers-contrast`, and `pointer`. An unknown feature or a broken query does not match. `MediaQueryList` and `MediaQueryListEvent` cannot be constructed with `new`.

There is no global `requestAnimationFrame` on the main global. A frame belongs to a surface. A dedicated worker has `requestAnimationFrame` and `cancelAnimationFrame`. A frame there runs the callback, then copies each transferred window canvas into the window that owns it. That window presents the copy on the next turn of the main event loop.

A window's animation frames run at the refresh rate of the monitor that window is on. On macOS that rate is the screen's maximum, so a variable-refresh display runs at its maximum while frames are scheduled. Linux and Windows use the monitor's current mode. The next frame after the window moves uses the new monitor. A rate that cannot be read, a rate below 10 Hz or above 1000 Hz, and a headless window run at 60 Hz. A dedicated worker uses the slowest of those rates among the open windows whose canvas it holds. Each worker follows only the canvases it holds. A window that has not transferred a canvas does not count, and `new OffscreenCanvas` does not count. With no transferred window canvas, the worker runs at 60 Hz. Closing a window, or moving one to another monitor, applies the next time that worker schedules a frame.

`requestIdleCallback(callback)` and `cancelIdleCallback(handle)` are on the main `globalThis`. `IdleDeadline` cannot be constructed with `new`. An idle period is at most 50ms. It also ends at the next timer or the next window animation frame when that comes first. The period does not start when less than 5ms remains before that timer or frame. A positive `timeout` still runs the callback then: `didTimeout` is true and `timeRemaining()` is 0. A callback scheduled during an idle period runs in a later period, and that later period does not start until the current deadline. Dedicated workers do not have these functions. A thrown callback is reported on `globalThis`.

### Dialog labels

`Yuzuko.setDialogLabels(labels)` replaces the button labels the runtime draws. It is installed on the main realm when `window` is enabled. A dedicated worker does not have it. The call does not open a dialog, does not require a window, and works in a headless process, including `yuzuko test`. Omitted `labels`, or `undefined`, changes nothing. `null`, a string, or any other non-object throws `TypeError`. Each present field must be a string. A number is not converted with `ToString`. An empty string or a string that contains NUL throws `TypeError`. An omitted field keeps its current label. Unknown properties are ignored. The fields are read in the order `ok`, `cancel`, `open`, `save`, `select`. A thrown getter stops the call and leaves every label unchanged.

The initial labels are `OK`, `Cancel`, `Open`, `Save`, and `Select`. `ok` is the accept button on alert, confirm, and prompt. `cancel` is the other button on confirm and prompt, and on a Linux file chooser. `open`, `save`, and `select` are the accept buttons on a Linux open, save, and directory chooser. The message and the window title stay the caller's strings. Intl and ICU do not supply these words.

macOS alert, confirm, and prompt use `ok` and `cancel`. The Windows prompt uses them and widens its buttons when a label is longer than the English word. Linux alert, confirm, prompt, and the file chooser use the configured labels. Linux alert and confirm do not use GTK's stock button text. Windows alert and confirm use `MessageBox`, so their buttons stay in the operating-system language and ignore these labels. macOS file dialogs and Windows file and directory dialogs use the system buttons and ignore them too. On Linux `_` in a label is shown as itself. On the Windows prompt `&` is shown as itself.

### File picker

`pickFiles(options)` blocks until the native dialog returns, the same way `alert` does. Omitted `options`, or `undefined`, uses the defaults. `null`, a string, or any other non-object throws `TypeError` before the window is checked. A closed window throws `InvalidStateError`. A headless process, including `yuzuko test` and `YUZUKO_HEADLESS=1`, throws `NotSupportedError`. There is no `pickSaveFile`, no `pickDirectory`, and no File System Access API.

`mode` is `"open"`, `"save"`, or `"directory"`. An omitted `mode` is `"open"`. Any other value throws `TypeError`. `"open"` selects existing files. `"save"` asks for one file name. `"directory"` selects one directory and can create a directory. `multiple` applies only to `"open"`. `filters` applies to `"open"` and `"save"`. `suggestedName` applies only to `"save"`. A field that does not apply throws `TypeError`. Unknown properties are ignored.

`multiple` defaults to `false` and must be a boolean when present. `filters` is an array of `{ name, extensions }`. `extensions` is an array of extensions without a leading dot. A token that is exactly `*` selects every file. Other extensions in that same filter are ignored, and that filter may sit beside other filters. An empty `extensions`, a missing `extensions`, or a value that is not an array throws `TypeError`. An extension that is empty or contains `/`, `\`, `.`, NUL, or `*` throws `TypeError`. `name` and each extension are converted with `ToString`. A `name` that contains NUL throws `TypeError`. An empty or omitted `name` is `All files` when the filter selects every file, and otherwise the extensions joined by `, `. `suggestedName` is a file name for `"save"`. An empty name, `/`, `\`, or NUL throws `TypeError`. `.` is allowed.

`"open"` with `multiple: false`, `"save"`, and `"directory"` each return an array of one path. Cancelling, or accepting an empty selection, returns `null`. The paths are absolute and are not canonicalized. Linux shows a GTK 3 chooser and does not parent it to the winit window. Windows and Linux show each filter, including one that selects every file, as its own choice. macOS keeps one allowed-type list: extensions from every filter are united, and there is no separate choice for each `name`. If any filter selects every file, or `filters` is omitted, macOS allows every file. `"directory"` does not use those filters. macOS uses `NSOpenPanel` and chooses directories only. Linux shows a GTK 3 folder chooser. Windows shows `SHBrowseForFolder`, parented to the window, with `BIF_RETURNONLYFSDIRS`, `BIF_NEWDIALOGSTYLE`, and `BIF_EDITBOX`. The dialog title is the window title. On Linux the chooser's accept button is `open`, `save`, or `select`, and its cancel button is `cancel`. macOS and Windows file dialogs use the system buttons.

### File drop

A file dragged over a window dispatches `filehover` on that window. Leaving without a drop dispatches `filehovercancel`. Releasing the files dispatches `filedrop`. These events do not bubble and cannot be cancelled. They target the window, not `surface`, and they are not delivered to `Yuzuko`. These events do not carry a `DataTransfer`. There is no HTML drag-and-drop.

`filedrop` and `filehover` carry `paths`, a new array of absolute paths in the order the operating system reported them. The paths are not canonicalized. A relative path is joined to the process current directory. Several files in one hover or one drop become one event. `filehovercancel` has no `paths`. macOS, Windows, and X11 deliver these events. Wayland does not. A headless window, including `yuzuko test`, does not receive an operating-system drop. `Yuzuko.__test.inject` delivers the same events when `YUZUKO_TEST_INPUT=1`.

### Frame order

Canvas 2D drawing, a `bitmaprenderer` update, a bitmap copied from a worker frame, and a change to `objectFit` or `imageRendering` are presented on the next turn of the event loop. That includes drawing at the top level of a script. The present runs once. It does not repeat until one of those changes again, and it does not queue an animation frame. A due animation frame still runs first, and drawing done by that callback is included in the same present. WebGPU still updates only in the steps below.

For each window:

1. Dispatch that window's OS events to its `Window`, one at a time, and run a microtask checkpoint after each event.
2. When an animation frame is due, run the `requestAnimationFrame` callbacks already queued on that surface. Callbacks queued during those callbacks wait for the next frame.
3. Run a microtask checkpoint.
4. When that frame ran, or the canvas changed as described above, update that surface. For WebGPU, follow "update the rendering of a WebGPU canvas": expire the current texture and present. For Canvas 2D, rasterize the recorded commands and present. For `bitmaprenderer`, present the transferred bitmap.
5. If another animation frame is queued, request a redraw of that window.

When only timers or idle callbacks remain, the event loop sleeps until the next timer or idle deadline.

`getCurrentTexture()` returns the same `GPUTexture` until that surface's rendering update for the frame. The next call after the update returns a new texture.

`close` is cancelable. If `preventDefault()` is not called, only that window is destroyed. While `close` is being dispatched, and while the `visibilitychange` that follows a close that was not cancelled is being dispatched, the window is still open, so `requestAnimationFrame` queues a callback and returns a positive handle. Destroying the window drops callbacks queued on it, including one queued in those listeners. After the window is closed, `requestAnimationFrame` returns `0` and does not queue a callback or request a redraw. `0` is never a live handle. `cancelAnimationFrame` does nothing for an unknown handle, including `0`. A callback that is already running can call `close()` and then `requestAnimationFrame`, which returns `0`. Other windows and outstanding timers or promises remain. When no `Window` is open and the task queue is empty, the process exits with status 0. An uncaught exception and an unhandled rejection exit with status 1. `Yuzuko.exit` ends the process with the status from that call before this idle exit. A promise microtask runs before the next macrotask. Posting a V8 foreground task wakes the event loop.

### Headless mode

`YUZUKO_HEADLESS=1` makes `Window` skip OS window creation. The drawing buffer and animation frames (60 Hz) still run on the object. `yuzuko test` creates windows headless. Input injection is `Yuzuko.__test`. That command installs it only when `YUZUKO_TEST_INPUT=1`. `yuzuko run` and `yuzuko repl` do not install it. Web-platform tests run under `cargo test`, not under a `yuzuko` subcommand.

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
- `getCoalescedEvents()` on `pointermove` and `pointerrawupdate`. A `pointermove` returns the moves coalesced into that frame. A `pointerrawupdate` returns one event, and that event's type is `pointerrawupdate`
- `setPointerCapture`, `releasePointerCapture`, and `hasPointerCapture` on that surface

`pointerrawupdate` is a `PointerEvent` for each pointer move the operating system reports, including while the pointer is locked. It is dispatched on that surface before the `pointermove` for the same samples. It bubbles and is not cancelable. Its `movementX` and `movementY` are that sample's movement. Several moves that one turn coalesces into a single `pointermove` each dispatch their own `pointerrawupdate`. An unlocked move comes from winit `WindowEvent::CursorMoved`. While the pointer is locked, a move comes from winit `DeviceEvent::MouseMotion` and `CursorMoved` is not turned into a pointer event.

Pointer lock is `surface.requestPointerLock()` and returns a promise. Success dispatches `pointerlockchange` on that surface. Failure dispatches `pointerlockerror` on that surface. The implementation is winit cursor grab plus hiding the cursor. Lock is per window. `movementX` and `movementY` on the locked move are that motion's delta.

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

### Keyboard lock

`navigator.keyboard` is installed with the `window` feature on the main global. It is the same object on every read. `Keyboard` extends `EventTarget`. A dedicated worker does not have `navigator.keyboard` or `Keyboard`. Those names are absent when `window` is off. `getLayoutMap` is absent, and `layoutchange` does not fire.

`lock(keyCodes)` reserves UI Events `code` values for the focused window. An omitted argument, `undefined`, or an empty list reserves every key. Each code is converted with `ToString`. A code is accepted when this runtime can report it from `KeyboardEvent.code`, including `"Unidentified"`. A duplicate is stored once. A list longer than 1024 codes rejects `TypeError` with `The key code list is too long`. A value that is not iterable rejects `TypeError` with `The key code list is not a sequence`. The promise settles in the same turn. A later `lock` replaces the reserved codes and does not reject the earlier call with `AbortError`. Fullscreen is not required. There is no permission prompt. Holding Escape does not show an overlay and does not leave fullscreen.

No focused window rejects `InvalidStateError` with `No window is focused` and leaves the previous reservation. A code this runtime cannot report rejects `InvalidAccessError` with `The key code is not a valid code` and clears the reservation. `unlock()` clears it. Closing the focused window clears it. Losing focus keeps the reservation and drops the operating-system inhibitor until a window of this process is focused again.

Reserved keys still dispatch `keydown` and `keyup` on that window. A headless process records the reservation and does not change operating-system shortcuts.

macOS calls `PushSymbolicHotKeyMode` with `kHIHotKeyModeAllDisabled` while the reserved set is empty or contains `Escape`, `Tab`, `Space`, `MetaLeft`, or `MetaRight`, and only while a window of this process is focused. The mode applies while the process is frontmost. Command-Tab and Command-Option-Escape stay with the system. Without assistive access the call does not change shortcuts. A list that names none of those keys does not change the symbolic hotkey mode.

Windows installs a low-level keyboard hook while a lock is held and a window is focused. The hook swallows a reserved key only when a window of this process is in the foreground, then posts that key to the window. Ctrl+Alt+Delete stays with the system. If the hook cannot be installed, `lock()` still resolves.

Linux calls `XGrabKeyboard` on an X11 window only when the list is empty, and only while that window is focused. A partial list does not grab. Wayland does not grab, so compositor shortcuts stay with the compositor. A failed grab still resolves `lock()`.

Focus changes dispatch `focus` and `blur` as `FocusEvent`s on that `Yuzuko.Window`. They do not bubble or cancel, and `relatedTarget` is null. `WindowSurface` does not receive them. Resize and scale-factor changes dispatch `resize` on the window. Size is read from `clientWidth` and `clientHeight`.

## Gamepad

The web Gamepad API. `navigator.getGamepads()` returns the connected gamepads. `gamepadconnected` and `gamepaddisconnected` fire on `globalThis`, and so do `ongamepadconnected` and `ongamepaddisconnected`. These do not live on `Yuzuko`. Physical devices are read with gilrs while the event loop runs, and an already-connected device is exposed without waiting for a button press. `GamepadHapticActuator.effects` is a frozen array listing `"dual-rumble"` when that device can play force feedback, and is otherwise empty. `playEffect("dual-rumble")` drives the low-frequency and high-frequency motors. Magnitudes are in the range [0, 1]. `duration` and `startDelay` are milliseconds, and their sum is capped at 5000. The playing promise resolves `"complete"` when that time has elapsed. It resolves `"preempted"` when another effect replaces it, when `reset()` stops it, or when the gamepad disconnects. `reset()` resolves `"complete"` while the gamepad is connected. `trigger-rumble` is not listed and rejects `NotSupportedError`. A parameter that is not a valid effect rejects `TypeError`. A call on a disconnected gamepad rejects `InvalidStateError`. `GamepadPose` reports no pose.

## Clipboard

`navigator.clipboard` `writeText`, `readText`, and `ClipboardItem` for `text/plain` and `image/png`. Reads and writes succeed only after a pointer or key press on the focused `Window`. The same calls reject with `NotAllowedError` without that action, and when the window is not focused. There is no permission prompt. `copy`, `cut`, and `paste` are dispatched on that surface.

`DataTransfer` is on the main global when `window` is enabled. `new DataTransfer()` creates an empty read/write store. `dropEffect` and `effectAllowed` start as `"none"`. A dedicated worker has no `DataTransfer`. `items`, `files`, `setDragImage`, `DataTransferItem`, `DataTransferItemList`, and `DragEvent` are absent.

`types` is a frozen array of the stored formats, in list order. The same array is returned until the list changes. `setData` always changes the list: an existing format is removed and the new value is appended, so that format moves to the end, and the next `types` read is a new array. `clearData` with no argument removes every string entry. `clearData(format)` removes that format. Either call leaves the current `types` array in place when it removes nothing. `getData`, `setData`, and `clearData` convert `format` with `ToString` and then to ASCII lowercase. `"text"` means `text/plain`. `"url"` means `text/uri-list`. `getData` of a missing format returns `""`. `getData("url")` returns the first `text/uri-list` line that is not empty and does not start with `#`. Line breaks are LF and CRLF. When no such line exists, the result is `""`. `getData("text/uri-list")` returns the stored string unchanged.

`dropEffect` accepts `"none"`, `"copy"`, `"link"`, and `"move"`. `effectAllowed` accepts `"none"`, `"copy"`, `"copyLink"`, `"copyMove"`, `"link"`, `"linkMove"`, `"move"`, `"all"`, and `"uninitialized"`, and only while the store is read/write. Any other string is ignored. On a read-only store, `setData` and `clearData` do nothing.

A host `copy` or `cut` carries a new empty read/write `DataTransfer`. A host `paste` carries a read-only `DataTransfer`. Its `text/plain` entry is the string `readText` would return when that cache holds text, including `""`. When the cache has no text, `text/plain` is absent and `getData` returns `""`. The paste store does not include `text/html` or `image/png`. If the host event is cancelled and the store contains `text/plain`, including `""`, that string is published the same way as `writeText`: it replaces the cached text, clears a cached `image/png`, and fires `clipboardchange`. Any other format stays on the `DataTransfer` and is not written to the operating system. An uncancelled host `copy` or `cut` leaves the clipboard unchanged. There is no selection to copy or delete. A `ClipboardEvent` constructed or dispatched by script does not write the clipboard.

`new ClipboardEvent(type)` has `clipboardData` null. An init value that is not an object is ignored, and `clipboardData` stays null. `null`, a missing `clipboardData`, or `undefined` stores null. A `DataTransfer` is stored and returned as that same object. Any other value throws `TypeError` with `clipboardData is not a DataTransfer`. A thrown `clipboardData` getter throws that exception.

`ClipboardChangeEvent` is on the main global. `new ClipboardChangeEvent(type, init)` stores `init.types` and `init.changeId`. `types` defaults to an empty sequence. A present value that is not an array, including a string, throws `TypeError`. Each entry is converted with `ToString`. The getter returns a new frozen array. `changeId` defaults to `0n`. A present value is converted with `ToBigInt`. A value that does not fit in 128 bits throws `TypeError`. A user-constructed event keeps the types it was given.

The host event type is `clipboardchange`. It does not bubble and cannot be cancelled. `isTrusted` is true. `types` lists the mandatory types that are present, in the order `text/plain`, `text/html`, `image/png`. Any other type is omitted. An event can list no types. `onclipboardchange` is the handler on `navigator.clipboard`. Assigning a function listens. Assigning any other value, including `null`, clears it.

A successful `write` or `writeText` dispatches the event before the promise resolves when a window is focused. A call that rejects before the pasteboard is updated does not dispatch. Listeners run before `await` continues. macOS uses the pasteboard `changeCount` as `changeId`. Windows uses `GetClipboardSequenceNumber`. On those systems a later poll sees the same sequence and does not dispatch again. The first observation of the pasteboard is not a change. A change observed while no window is focused replaces one pending event, and that event is dispatched once after the next `focus` event. `blur` does not dispatch it. Linux dispatches for `write` and `writeText` only. A cancelled host `copy` or `cut` that publishes `text/plain` dispatches it the same way. A dedicated worker has no `Clipboard`, no `ClipboardChangeEvent`, and no `DataTransfer`.

## Screen Wake Lock

`navigator.wakeLock` is installed with the `window` feature on the main global. It is the same object on every read. A dedicated worker does not have `navigator.wakeLock`, `WakeLock`, or `WakeLockSentinel`. Those names are absent when `window` is off.

`request()` with an omitted type, or with `"screen"`, resolves a `WakeLockSentinel`. Any other value is converted with `ToString`. A result other than `"screen"` rejects `TypeError` with `The wake lock type is invalid`. If that conversion throws, `request()` throws the same exception. `type` is `"screen"`. `released` is false until `release()`. `onrelease` starts as null.

`release()` sets `released` to true, dispatches a trusted `release` event on that sentinel, and then resolves `undefined`. The event does not bubble and cannot be cancelled. `released` is already true while the listeners run. A second `release()` resolves `undefined` and does not dispatch again.

The runtime asks the operating system to keep the display awake while at least one sentinel is unreleased. The first successful `request()` creates the platform assertion. The last `release()` drops it. macOS calls `IOPMAssertionCreateWithName` with type `PreventUserIdleDisplaySleep`, level on, and name `Yuzuko`, and drops it with `IOPMAssertionRelease`. Windows calls `SetThreadExecutionState` with `ES_CONTINUOUS | ES_DISPLAY_REQUIRED`, and calls it with `ES_CONTINUOUS` when the last sentinel releases. Linux spawns `systemd-inhibit --what=idle:sleep --who=Yuzuko --why=screen wake lock --mode=block sleep infinity` and kills that child when the last sentinel releases. If the operating system refuses the request, `request()` rejects `NotAllowedError` with `The platform refused the screen wake lock`.

There is no permission prompt and no document. Hiding a window does not release a sentinel. `new WakeLock()` and `new WakeLockSentinel()` throw `TypeError` with `Illegal constructor`.

## Menu bar

`Yuzuko` is an `EventTarget`. `Yuzuko.Menu` and `Yuzuko.setMenu(menu)` install one application menu with `muda`. A selection delivered through the same input path as pointer and key events dispatches `menuclick` on `Yuzuko` and carries the item's `action` string. macOS has one menu for the app. On Windows the same menu is attached to the focused window. On Linux, winit does not expose a GTK window, so `setMenu` throws `NotSupportedError`.

## Context menu

`setContextMenu(menu)` stores a `Yuzuko.Menu` on that window. The menu may contain top-level actions. `null` clears it and does not show a menu. A value that is not a `Menu` or `null` throws `TypeError` before the window is checked. A closed window throws `InvalidStateError`. The call works in a headless process. It does not pop a menu.

`showContextMenu(menu, x, y)` shows that menu at `x` and `y` CSS pixels from the client origin. It does not replace the menu stored by `setContextMenu`. Non-finite coordinates throw `TypeError` before the window is checked. A closed window throws `InvalidStateError`. A value that is not a `Menu` throws `TypeError` after the window is checked. A headless process throws `NotSupportedError`. On Linux the call throws `NotSupportedError` because winit exposes no GTK window. The popup is synchronous. A selection dispatches `menuclick` on that window before the method returns. The event is trusted and carries the item's `action` string.

A `contextmenu` event whose default action is allowed shows the menu stored by `setContextMenu` at `clientX` and `clientY` when that menu is stored and the platform can show it. `preventDefault()` leaves that menu hidden. The listener can then call `showContextMenu` with a different menu. A headless process, a window with no stored menu, and Linux do not throw from that default action.

## Tray and dock

`Yuzuko.Tray` is an `EventTarget`. `new Yuzuko.Tray()` adds a status item. `setIcon(bytes, options)` takes PNG bytes. `options.template` asks macOS to draw that icon as a template image. `setTooltip(text)`, `setMenu(menu)`, and `close()` update the item. `menu` is a `Yuzuko.Menu` and may contain top-level actions. `bounds` is `{ x, y, width, height }` in CSS pixels, or `null` when the system cannot report the rectangle. A primary click dispatches `click` on that tray. A menu selection dispatches `menuclick` on that tray with `action`. An open tray keeps the process alive. The constructor throws `NotSupportedError` when no status item can be created.

`Yuzuko.dock` is one `EventTarget` for the process. `setBadge(text)` sets the badge, and `null` or `""` clears it. `setVisible(false)` hides the app from the macOS Dock and the app switcher. `setMenu(menu)` installs the Dock menu. `bounce()` requests attention once, and `bounce(true)` requests it until the app is focused. `menuclick` and `reopen` are dispatched on `Yuzuko.dock`. `reopen` carries `hasVisibleWindows`. Writes the system cannot do throw `NotSupportedError`. A dedicated worker has no `Tray` and no `dock`. There is no tray panel.

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

`ImageData` is serializable. `structuredClone` copies its pixels, color space, and pixel format, including float16 Display P3, into a new typed array. The clone does not share the source buffer. `ImageData` is not transferable. Listing one in a transfer list throws `DataCloneError`. Transferring its `data` buffer detaches that buffer, and the clone keeps the pixels.

`ImageBitmap` is serializable and transferable. `structuredClone` without a transfer list copies the bitmap and leaves the source open. Listing the bitmap in the transfer list of `structuredClone`, `postMessage`, or an owning stream detaches the source: `width` and `height` are 0, and `close()` has the same effect. A detached bitmap throws `DataCloneError`.

`createImageBitmap` uses the first frame of an animated image. `ImageDecoder` returns every frame of an animated PNG, WebP, or JPEG XL and does not advance those frames. Input that cannot be decoded rejects with `InvalidStateError`. Decoding does not block the event loop: it runs on a dedicated thread, and the host object is created on the main thread afterward. A decoder panic is caught with `catch_unwind` and becomes `InvalidStateError`.

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

`getContext("2d", { colorSpace, colorType })` still selects `"srgb"` or `"display-p3"`, and `"unorm8"` or `"float16"`. The defaults are `"srgb"` and `"unorm8"`. `getContextAttributes()` returns them. The default context is the vello target. `"srgb-linear"` and `"display-p3-linear"` throw `TypeError` from `getContext` and from `ImageDataSettings`.

A `display-p3` or `float16` context has its own backing store, owned here rather than by vello. On the GPU that store is `Rgba8Unorm` interpreted as Display P3, or `Rgba16Float` in the canvas color space. Without a device it is a CPU buffer of the same layout. `putImageData` and a `drawImage` whose source already matches the destination write that store and keep the samples. An opaque axis-aligned solid `fillRect` on a float16 store is written in that store's color space at float16 precision, so a `color(display-p3 ...)` fill can produce encoded components outside 0..1. Other path fills, strokes, text, and gradients are painted by vello in sRGB unorm8 and then converted into the store, so those paints lose chroma outside sRGB and precision beyond 8 bits.

`fillStyle` and `strokeStyle` accept the existing sRGB forms plus `color(srgb ...)` and `color(display-p3 ...)`, with or without alpha. Gradient stops accept the same forms. An unrecognized color does not change the current style. `getImageData`, `putImageData`, and `createImageData` take `ImageDataSettings`. `colorSpace` is `"srgb"` or `"display-p3"`. `pixelFormat` is `"rgba-unorm8"`, stored in a `Uint8ClampedArray`, or `"rgba-float16"`, stored in V8's `Float16Array`. A buffer of the other typed-array kind throws `InvalidStateError`. `getImageData` converts when the requested color space or pixel format differs from the backing store. An omitted `colorSpace` is `"srgb"`. An omitted `pixelFormat` is `"rgba-unorm8"`. `createImageData(sw, sh)` uses those defaults. `createImageData(imageData)` copies that object's color space and pixel format. `putImageData` converts into the canvas color space in floating point when either side is float16. An 8-bit channel is the encoded 0..1 value times 255. Float16 `ImageData` keeps values outside 0..1.

Present uses an explicit wgpu `SurfaceColorSpace`, never `Auto`. An sRGB unorm8 canvas uses `Srgb`. A Display P3 unorm8 canvas uses `DisplayP3` when the surface capabilities list that pair, and is converted to sRGB for present otherwise. A float16 canvas uses `ExtendedDisplayP3` or `ExtendedSrgb` with `Rgba16Float` only when that pair is advertised. Otherwise present quantizes it to the sRGB unorm8 swapchain. `Auto` on `Rgba16Float` resolves to linear scRGB, which is a different encoding.

Conformance pixel tests always use the CPU renderer so GPU vendors do not perturb the result.

Level A is:

- State stack: `save`, `restore`, `reset`, `resetTransform`
- Transforms: `scale`, `rotate`, `translate`, `transform`, `setTransform`, `getTransform`
- `globalAlpha`. `globalCompositeOperation` stores `source-over`, `source-in`, `source-out`, `source-atop`, `destination-over`, `destination-in`, `destination-out`, `destination-atop`, `lighter`, `copy`, `xor`, `clear`, and the CSS blend modes. Any other value is ignored. Painting uses the stored operator inside the current clip
- Styles: CSS color strings, `color(srgb ...)` and `color(display-p3 ...)`, linear gradients, radial gradients, conic gradients, and `createPattern` from `ImageBitmap` or `OffscreenCanvas` with all four repeat modes
- Lines: `lineWidth`, `lineCap`, `lineJoin`, `miterLimit`, `setLineDash`, `getLineDash`, `lineDashOffset`
- Rectangles and paths: `fillRect`, `strokeRect`, `clearRect`, `rect`, `beginPath`, `closePath`, `moveTo`, `lineTo`, `quadraticCurveTo`, `bezierCurveTo`, `arc`, `arcTo`, `ellipse`, `roundRect`, `fill`, `stroke`, `clip`. Fill rules are nonzero and evenodd. A negative `roundRect` edge flips that axis and keeps each radius on the corner named before the flip. One negative edge reverses the subpath. Two negative edges restore the positive direction
- `Path2D`, `isPointInPath`, `isPointInStroke`
- All three `drawImage` overloads. Sources are `ImageBitmap` and `OffscreenCanvas`
- `imageSmoothingEnabled` defaults to true. `imageSmoothingQuality` is `"low"`, `"medium"`, or `"high"` and defaults to `"low"`. An unrecognized quality is ignored. `false` selects nearest. `"low"`, `"medium"`, and `"high"` select linear sampling
- `getImageData`, `putImageData`, `createImageData`, including `colorSpace` and `pixelFormat`. `getImageData` may stall on a GPU readback
- Text: `font`, `lang`, `textAlign`, `textBaseline`, `direction`, `fillText`, `strokeText`, `measureText`. The shorthand accepts `italic` or `oblique`, `small-caps`, a weight, a `px` or `pt` size, an optional line height, and a family list. The getter omits a weight of 400, a normal style, and the line height. `small-caps` is stored and returned. `cursive` and `fantasy` stay in the list. A family is a loaded `FontFace`, a generic (`sans-serif`, `serif`, or `monospace`, mapped to the OS UI font), or another family name. A list uses the first loaded family, or else the first of those generics. A size that names only an unregistered family, or only `cursive` or `fantasy`, is stored and shaped as `sans-serif`. A value that does not parse is ignored. CJK glyphs must draw. `lang` is a BCP 47 tag, `""`, or `"inherit"` (the default) and is part of the canvas state. It selects localized glyphs when the text is shaped. `direction` is `"ltr"`, `"rtl"`, or `"inherit"` and defaults to `"inherit"`. An unrecognized value is ignored. There is no document, so `"inherit"` is shaped left to right
- Enhanced text metrics on `TextMetrics`: `getSelectionRects(start, end)`, `getActualBoundingBox(start, end)`, `getTextClusters()`, `getTextClusters(start, end, options)`, and `getIndexFromOffset(offset)`. Indices are UTF-16 code units. `TextCluster` exposes `x`, `y`, `start`, `end`, `align`, and `baseline`. `fillTextCluster` and `strokeTextCluster` draw one cluster
- Layers: `beginLayer()` and `endLayer()`. `beginLayer` takes an optional dictionary `{ filter }`. Draws between the calls are one group. The explainers are [Enhanced Text Metrics](https://github.com/fserb/canvas2D/blob/master/spec/enhanced-textmetrics.md) and [Layers](https://github.com/fserb/canvas2D/blob/master/spec/layers.md)

`filter` is a CSS filter string, default `"none"`. A value that parses as `none` or a `<filter-value-list>` is stored, and the getter returns that assigned string. `none` is case-insensitive. `""`, `inherit`, `initial`, `unset`, `revert`, `revert-layer`, `null`, and `undefined` do not parse, so the current filter stays. `calc()` is not accepted. There is no `CanvasFilter` class. `beginLayer({ filter })` uses the same CSS syntax. `null`, `undefined`, and `""` omit the argument. Any other unparsable value, including a filter dictionary, throws `TypeError` and does not open a layer.

`beginLayer` and `endLayer` share a stack with `save` and `restore`. `restore` throws `InvalidStateError` when the top entry is a layer. `endLayer` throws `InvalidStateError` when the top entry is a save or the stack is empty. `reset` discards open layers, and a later `endLayer` throws. An open layer is not rasterized. `endLayer` composites it. Entering a layer sets `globalAlpha` to `1`, `globalCompositeOperation` to `"source-over"`, `shadowBlur`, `shadowOffsetX`, and `shadowOffsetY` to `0`, `shadowColor` to transparent black, `filter` to `"none"`, and the clip list to empty. The saved `globalAlpha` and clips then apply to that bitmap. The current transform continues inside the layer and is restored on `endLayer`. `setTransform` and `resetTransform` replace the matrix until the layer ends. The current path is not saved. A parent `translate` therefore moves draws inside the layer, and a clip around the layer clips the bitmap rather than surviving `endLayer` as a context clip.

`ctx.filter` and a layer `filter` are applied when the drawing is composited. The functions are `blur`, `brightness`, `contrast`, `drop-shadow`, `grayscale`, `hue-rotate`, `invert`, `opacity`, `saturate`, `sepia`, and `url()`. They run left to right on the shape's premultiplied sRGB unorm8 ink, before the composite operator. Lengths are canvas pixels and are not scaled by the current transform. A `url()` reference has no SVG filter, so that function clears the ink and later functions in the list do not run. When a filter is set, the canvas shadow is included in that bitmap and the result is composited once. A filtered drawing is rasterized on the CPU. A shadow is painted when `shadowColor` has nonzero alpha and `shadowBlur` or either offset is nonzero. Offsets and blur are canvas pixels and are not affected by the current transform. The blur is a Gaussian whose σ is `shadowBlur` / 2, capped at 128. The shadow is composited with the current operator inside the clip, then the shape is composited the same way. A shape outside the canvas still casts a shadow into it. `clearRect` and `putImageData` do not use the operator or the shadow. A non-finite blur or offset is ignored, and a negative blur is ignored. An unparsable shadow color is ignored. A recognized `globalCompositeOperation` is stored and used inside the clip. `lighter` is additive. A list with an operator other than `source-over`, or with a shadow, is rasterized on the CPU.

`createConicGradient(startAngle, x, y)` paints one clockwise turn in canvas space. Offset 0 is `startAngle` radians from the positive x-axis, and offset 1 is that same ray. A non-finite argument throws `TypeError`. A gradient with no color stops does not paint. `addColorStop` throws `TypeError` when `offset` is not finite, `IndexSizeError` when `offset` is outside 0 through 1, and `SyntaxError` when `color` does not parse. Named fonts are resolved by `FontFace`. CSS `@font-face` is not implemented. Extending past level A updates this list and `docs/deviations.md` in the same change. `YUZUKO_SKIP_GPU=1` reports `Yuzuko.canvasRenderer === "cpu"`.

Cluster positions are relative to the text origin after `textAlign` and `textBaseline`. `getTextClusters` options may set `align`, `baseline`, `x`, and `y` for the returned clusters and do not change the `TextMetrics` object. `getSelectionRects`, `getActualBoundingBox`, and an explicit `getTextClusters` range take `[EnforceRange] unsigned long` indexes. A non-finite value, or a value outside 0 through 2^32 - 1, throws `TypeError`. `getSelectionRects` throws `IndexSizeError` when `start` or `end` is greater than the UTF-16 length. `getActualBoundingBox` and an explicit `getTextClusters` range throw `IndexSizeError` when `start` is greater than or equal to that length, or `end` is greater than that length. An omitted `getTextClusters` range is the whole run. `getSelectionRects` uses advance boxes and the font ascent and descent. `getActualBoundingBox` returns the advance box of the classic `actualBoundingBox*` getters for the whole run: `x` is 0, `y` is the negation of `actualBoundingBoxAscent`, `width` is the advance, and `height` is the ascent plus the descent. A partial range does not tighten that rectangle, and `textAlign` and `textBaseline` do not move it. `getIndexFromOffset` increases from left to right in that origin space. Values outside the run return `0` or the UTF-16 length according to `direction`. `fillTextCluster` and `strokeTextCluster` use the measured font, direction, and `lang`, and an options dictionary may replace `align`, `baseline`, `x`, and `y` for that call without writing the cluster. The fill or stroke style, transform, alpha, and clips are the context's current values. The classic `actualBoundingBox*` getters stay advance approximations.

`convertToBlob` produces `image/png`, `image/jpeg`, and `image/webp`. JPEG XL encoding is out of scope. `putImageData` of a float16 Display P3 `ImageData` round-trips through `getImageData` within one float16 ulp. An opaque solid `fillRect` on a float16 canvas uses that same conversion. Any other path painted with `color(display-p3 ...)` is compared after vello's sRGB unorm8 conversion, not as a wide-gamut round trip. A plain `source-over` list with no shadow and no filter draws on both the GPU and CPU renderers. Any other operator, a shadow, or a filter is rasterized on the CPU.

### WebGPU

WebGPU is implemented on wgpu. The process has one default adapter. Canvas 2D uses the same `wgpu::Device`. Each `Window` has its own swapchain. `copyExternalImageToTexture` copies from `ImageBitmap` and `OffscreenCanvas` on the GPU, and takes a CPU path only when the formats do not match. The copy converts from the source color space into the destination `colorSpace`, `"srgb"` or `"display-p3"`. A float16 source is quantized to the destination texture format.

Every wgpu call in v1 happens on the main thread. `YUZUKO_SKIP_GPU=1` makes `requestAdapter` resolve `null`.

The WebGPU surface is:

- `navigator.gpu.requestAdapter`, `requestDevice`, and `getPreferredCanvasFormat`
- `GPUCanvasContext.configure`, `getCurrentTexture`, and `unconfigure`. `alphaMode` accepts `"opaque"` and `"premultiplied"`
- Shader modules, render pipelines, compute pipelines, command encoders, render passes, and compute passes
- Render bundle encoders, `finish`, and `executeBundles`
- Buffers, textures, samplers, bind groups, bind group layouts, query sets, `queue.writeBuffer`, and `queue.submit`
- `copyBufferToBuffer`, `copyTextureToBuffer`, `resolveQuerySet`, and indexed and indirect draws (`setIndexBuffer`, `drawIndexed`, `drawIndirect`, `drawIndexedIndirect`, `dispatchWorkgroupsIndirect`)
- Depth and stencil attachments, occlusion queries, and timestamp queries when `timestamp-query` is enabled
- `copyExternalImageToTexture` drawing an `ImageBitmap` or a `VideoFrame`
- `pushErrorScope` / `popErrorScope` for `"validation"` and `"out-of-memory"`. A validation failure reported through `popErrorScope` is a `GPUValidationError`
- `GPUDevice.lost` and `destroy`. `device.lost` resolves from `destroy`
- Adapter limits and features published in specification vocabulary
- Unimplemented methods throw `NotSupportedError`

A bind group layout entry is one binding: a buffer (`"uniform"`, `"storage"`, or `"read-only-storage"`, with `hasDynamicOffset` and `minBindingSize`), a sampler (`"filtering"`, `"non-filtering"`, or `"comparison"`), a sampled texture (`sampleType`, `viewDimension`, and `multisampled`), or a storage texture. Storage texture `access` is `"write-only"`, `"read-only"`, `"read-write"`, or `"atomic"`. An access the format does not allow, including `"atomic"` when the device has no image atomics, is a validation error. `externalTexture` is a layout entry when the process device supports wgpu external textures. `importExternalTexture` copies a `VideoFrame` into an RGBA external texture. A fragment shader samples it with `textureSampleBaseClampToEdge`. A closed frame throws `InvalidStateError`. There is no `HTMLVideoElement`. Without that support, the binding and the import throw `NotSupportedError`. `setBindGroup` applies dynamic offsets. A null bind group with a non-empty offset list throws `TypeError`.

A render pipeline accepts `cullMode`, `frontFace`, `stripIndexFormat`, `writeMask`, a blend state, and a depth-stencil state for `depth16unorm`, `depth24plus`, `depth24plus-stencil8`, `depth32float`, `stencil8`, and `depth32float-stencil8` when that feature is enabled. Without `depth32float-stencil8`, that format throws `NotSupportedError`. `unclippedDepth: true` requires `depth-clip-control` and otherwise throws `NotSupportedError`. `alphaToCoverageEnabled` is applied, including on a multisampled pipeline. A `src1` blend factor throws `NotSupportedError` without `dual-source-blending`. Multisample `count` is 1 or 4. Any other count throws `TypeError`. A color attachment may set `resolveTarget` to a single-sample view of the same format. `createTexture` `sampleCount` is 1 or 4, and any other count throws `TypeError`. A render pass keeps a null color attachment in its slot and can draw with only a depth attachment. `setViewport` and `setScissorRect` on that pass restrict rasterization. A non-finite viewport argument throws `TypeError`, and a viewport outside the allowed range throws `RangeError`. `pushDebugGroup`, `popDebugGroup`, and `insertDebugMarker` are on the command encoder, the render pass, and the compute pass. An empty `popDebugGroup` is a validation error. wgpu reports it when `finish()` encodes the command buffer, including a pop recorded on a render or compute pass. A render bundle encoder does not expose debug markers, viewport, or scissor. `createComputePipelineAsync` returns a promise that is already settled. Compilation runs on the calling thread, and a pipeline error rejects that promise.

The process device enables `timestamp-query`, `depth-clip-control`, and `depth32float-stencil8` when the adapter supports them, and wgpu external textures when the adapter supports that feature. Device creation retries with `timestamp-query` alone, then with no optional features, if a larger set fails. Those WebGPU features are visible without listing them in `requiredFeatures`. wgpu external textures are not a WebGPU feature name. `requestDevice` rejects a feature the shared device does not have. Creating a `"timestamp"` query set without the feature throws `NotSupportedError`. Occlusion queries do not need a feature. `GPUQuerySet.destroy` makes a later `resolveQuerySet` of that set a validation error. `writeTimestamp` stays absent.

Accepted texture formats are `bgra8unorm`, `rgba8unorm`, `bgra8unorm-srgb`, `rgba8unorm-srgb`, `r8unorm`, `rg8unorm`, `rgba16float`, `r16float`, `rg16float`, `r32float`, `rg32float`, `rgba32float`, `r8uint`, `r8sint`, `r16uint`, `r16sint`, `r32uint`, `r32sint`, `rg8uint`, `rg8sint`, `rg16uint`, `rg16sint`, `rg32uint`, `rg32sint`, `rgba8uint`, `rgba8sint`, `rgba16uint`, `rgba16sint`, `rgba32uint`, `rgba32sint`, and the depth and stencil formats above. Any other name throws `TypeError`.

## FontFace

`FontFace` lives on `globalThis`. There is no `document`, so the `FontFaceSet` is `Yuzuko.fonts`, not `globalThis.fonts`. A dedicated worker exposes `self.fonts` and `globalThis.fonts` as that worker's set. A source is an `ArrayBuffer` or a string. A string that contains `url(` or `local(` is a CSS `src` list, tried in order. Each item is `url()` with an optional `format()` and an optional `tech()`, or `local()`. `format()` keeps the source when it is omitted or when any keyword is `woff2`, `woff`, `truetype`, or `opentype`, compared without regard to case. The same keyword with a `-variations` suffix counts as that format. Any other keyword skips that source and does not fetch it. `tech()` is parsed and does not select or reject a source. `url()` loads through `fetch`. A build without the `fetch` feature has no `fetch`, and a URL source fails. A string with neither `url(` nor `local(`, and with no space, tab, newline, or comma, is one URL. Formats are TTF, OTF, WOFF, and WOFF2. `local()` reads an installed font whose PostScript name, full name, family name, or typographic family name matches, compared without regard to case. A PostScript or full-name match selects that face. A family match uses the face closest in stretch, then style, then weight. A miss, a failed fetch, or bytes that are not a font tries the next item. When the list is exhausted the load fails with `NetworkError`. When the last item returns bytes that are not a font, the load fails with `SyntaxError`. An unparsable `src` list fails at `load()` with `SyntaxError`. The constructor still accepts the string. A family that has finished `load()` and is in `Yuzuko.fonts` can be named from the Canvas 2D `font` property. `add` returns the set. `forEach`, `keys`, `values`, `entries`, and `@@iterator` walk the faces in insertion order. `check(font, text)` reports whether that CSS `font` shorthand can draw `text` without waiting for a face in this set. `text` defaults to a single space. An unparsable shorthand, including a CSS-wide keyword, throws `SyntaxError`. `load(font, text)` returns a promise for the matching faces and starts a load that has not finished. An unparsable shorthand rejects `SyntaxError`. `ready` fulfills with the set while `status` is `"loaded"`, and a new promise is created when a load starts. The set dispatches `loading` when a load starts, then `loadingdone` and, when a face fails, `loadingerror`. The generic families remain. `serif` and `monospace` still draw. A size that names only an unregistered family is stored and shaped as `sans-serif`. `globalThis.fonts` and `document` stay absent on the main global. CSS `@font-face` is not implemented.

`queryLocalFonts()` is on `globalThis` in the main realm and in a dedicated worker. It resolves to the installed fonts this runtime can read, sorted by PostScript name. Each entry is a `FontData` with `postscriptName`, `fullName`, `family`, and `style`. Those strings prefer the `en-US` name, then `en`, then the first localization. `family` is name ID 1, or name ID 16 when that is missing. `style` is name ID 2, or name ID 17 when that is missing, then `Regular`. A missing full name uses the family, or the family and style when the style is not `Regular`. A face with no valid PostScript name is omitted. A valid name is 1 to 63 characters from U+0021 through U+007E, excluding `[](){}<>/%`. `blob()` resolves to a `Blob` of that font file with an empty type. `postscriptNames` keeps only exact PostScript name matches. An empty list resolves to an empty array. `null` and `undefined` are omitted. Any other value that is not an object, or a `postscriptNames` value that is not an array, throws `TypeError`. There is no permission prompt.

## Web Audio

`AudioContext` and `OfflineAudioContext` live on `globalThis`. `decodeAudioData` turns WAV, FLAC, MP3, Ogg Vorbis, and Ogg Opus into an `AudioBuffer` through pure-Rust decoders. Ogg Opus is decoded at 48000 Hz, which is the Opus playback rate, and the channel count comes from `OpusHead`. A build without the `audio` feature has no `AudioContext` and no `decodeAudioData`. An `AudioBufferSourceNode` plays that buffer. `GainNode`, `OscillatorNode`, and `ConstantSourceNode` run in the same graph into an `AudioDestinationNode`. `OfflineAudioContext.startRendering()` resolves an `AudioBuffer`. `suspend` rounds the time up to a render quantum boundary, with `currentTime` equal to that boundary, and `resume` continues. A buffer through gain `0.5` renders at half amplitude. `OfflineAudioContext` keeps the rendered buffer as the signal and does not open an output device. A realtime `AudioContext` keeps `currentTime` at 0 until a source starts. Starting a source while the context is `"running"`, and `resume` after that start, renders on the isolate thread until `suspend` or `close`. While the output device is open, a playback turn renders quanta until the 100 ms buffer is full, yielding so other tasks can run. The timer fallback still renders one quantum per turn. Rendering continues after every source has ended. A context that has started playback keeps the process alive until `suspend` or `close`. The first of those turns opens the output device named by a string `sinkId`, including the default device when `sinkId` is `""`, and fills a 100 ms playback buffer. Later turns keep that buffer filled, including while the REPL waits for the next key. When `sinkId` is `{ type: "none" }`, that turn does not open a device. The device callback only copies that buffer. `outputLatency` is 100 ms while the device is open. The context requests `f32` stereo at its `sampleRate` when the device accepts that configuration. A different device rate or channel count is reached by linear resampling on the isolate thread. A non-`f32` device is converted in the callback. When the device cannot be opened, a timer advances one quantum at a time and `outputLatency` is 0. `suspend` pauses the device and the clock. `resume` continues them. `close` stops the device before it returns. `baseLatency` stays `renderQuantumSize / sampleRate`.

`renderSizeHint` on `AudioContextOptions` and `OfflineAudioContextOptions` is `"default"`, `"hardware"`, or an integer. `BaseAudioContext.renderQuantumSize` is that block length for the life of the context, and `AudioWorkletGlobalScope.renderQuantumSize` is the same value. Omitted and `"default"` are 128. An integer from 1 through floor(6 × sampleRate), including 1 and a non-power of two, is that exact length. Any other integer throws `NotSupportedError`. `baseLatency` is larger when the hint is 4096 than when it is 128. An `AudioWorklet` `process` uses that same length, so a hint of 256 makes `process` see length 256. Offline `"hardware"` is 128. Realtime `"hardware"` is the default output device's current buffer size in frames when the OS reports one inside 1 through floor(6 × sampleRate), and 128 when it does not. That size is already visible at construction. On this Mac the query is Core Audio `kAudioDevicePropertyBufferFrameSize`. Windows and Linux still use 128. `baseLatency` is `renderQuantumSize / sampleRate`. The three-argument `OfflineAudioContext` constructor uses 128.

`AudioWorklet.addModule` loads the processor script with `fetch` and evaluates it. A build without the `fetch` feature rejects `AbortError`. `registerProcessor` defines it. An `AudioWorkletNode` calls `process` once per quantum with `Float32Array` channels of length `renderQuantumSize`. The node port and the processor port exchange the same structured-clone values, including a transferred `AudioData` or `VideoFrame`. The worklet global does not define `AudioData` or `VideoFrame`. Methods on the received object still run. `currentFrame`, `currentTime`, `sampleRate`, and `renderQuantumSize` are visible in the worklet. A `parameterDescriptors` entry is an `AudioParam`, and `process` receives that parameter's block. Setting `AudioParam.value` sets the intrinsic value and inserts `setValueAtTime` at `currentTime`. `setValueAtTime`, `linearRampToValueAtTime`, `exponentialRampToValueAtTime`, `setTargetAtTime`, `setValueCurveAtTime`, `cancelScheduledValues`, and `cancelAndHoldAtTime` schedule that timeline. A ramp with no earlier event throws `InvalidStateError`. `AudioNode.connect` to an `AudioParam` returns `undefined`. Only output 0 of that connection contributes samples, and those samples are summed into one channel. A k-rate parameter holds the value at the start of the quantum for the block. An a-rate parameter has one value per sample. `AudioParam` `value`, `defaultValue`, `minValue`, and `maxValue` are single-precision floats.

`AudioContext` `sinkId` is an output-device name or `{ type: "none" }`. `""` is the default device and does not require a device to exist. Any other string is a host output-device name. `{ type: "none" }` does not open an output device, and each read of `sinkId` returns a new object whose `type` is `"none"`. An unknown name throws `NotFoundError` when the context is constructed. A `type` other than `"none"` throws `TypeError` from the constructor. `setSinkId` returns a promise. `setSinkId("")` selects the default device. `setSinkId({ type: "none" })` selects no device, and the same call again resolves. An unknown device string rejects `NotFoundError` and leaves the previous sink. `{ type: "something_else" }` rejects `TypeError`. After `close()`, `setSinkId` rejects `InvalidStateError`. A context that has already opened a device keeps that device. There is no `mediaDevices.enumerateDevices`. `Yuzuko.audioOutputDevices()` is installed with the `audio` feature on the main global and in a dedicated worker, and it is absent from an `AudioWorklet` processor realm. It returns a new array of host output-device names. Each name can be passed to `sinkId` or `setSinkId`. `""` is not included. An empty name from the host is omitted, and a repeated name is listed once. The default output device is listed by its own name even when the host enumeration leaves it out, as ALSA does with its `default` PCM. The call is synchronous. A failure to list the devices throws `NotSupportedError` with `The audio outputs could not be listed`.

`DelayNode`, `BiquadFilterNode`, `IIRFilterNode`, `WaveShaperNode`, `StereoPannerNode`, `PannerNode`, `ConvolverNode`, `DynamicsCompressorNode`, `AnalyserNode`, `ChannelMergerNode`, and `ChannelSplitterNode` run in that graph. `BaseAudioContext.listener` is the `AudioListener` a `PannerNode` spatializes against. `panningModel` `"equalpower"` uses equal-power gains. `"HRTF"` downmixes to mono, applies the distance and cone gain, and writes stereo with an azimuth-dependent delay on the far ear and a lower far-ear gain. A one-sample click from a source on one side of the listener peaks on different frames. The delay carries across render quanta. An unrecognized `panningModel` throws `TypeError`. `coneOuterGain` outside 0 through 1 throws `InvalidStateError`. Assigning an unrecognized `channelCountMode` leaves the previous value. The same string in a constructor dictionary throws `TypeError`. An `AudioBufferSourceNode`'s `playbackRate` and `detune` are k-rate. Setting either `automationRate` to `"a-rate"` throws `InvalidStateError`. Assigning an unrecognized `BiquadFilterNode` `type` leaves the previous value. Assigning an unrecognized `WaveShaperNode` `oversample` leaves the previous value. The same strings in a constructor dictionary throw `TypeError`. Speaker layouts of 1, 2, 4, and 6 channels use the specification mix. Any other count uses discrete mixing. `AudioNode.context` is the owning `BaseAudioContext`. `PeriodicWave` throws `IndexSizeError` when both `real` and `imag` are present and their lengths differ, or when fewer than two coefficients remain. An omitted `real` or `imag` is a zero sequence of the other length. `createPeriodicWave` requires both sequences and throws `IndexSizeError` on the same length checks. `IIRFilterNode` and `createIIRFilter` throw `TypeError` when a coefficient list is missing. `AudioBuffer`, `PeriodicWave`, `AudioParam`, `AudioParamMap`, `AudioNode`, `AudioScheduledSourceNode`, `AudioDestinationNode`, `GainNode`, `AudioBufferSourceNode`, `OscillatorNode`, `ConstantSourceNode`, `AudioWorklet`, `AudioWorkletNode`, `AudioListener`, and those processing nodes are part of this surface. `BaseAudioContext` is not constructable. `MediaElementAudioSourceNode`, `MediaStreamAudioSourceNode`, `MediaStreamAudioDestinationNode`, and `ScriptProcessorNode` stay absent.

## WebCodecs

A game draws video onto a canvas. There is no `HTMLVideoElement` and no media element. `Yuzuko.demuxMp4` reads a progressive MP4 file into H.264 and AAC chunks. The runtime does not advance frames by itself. `VideoDecoder`, `EncodedVideoChunk`, and `VideoFrame` live on `globalThis`. A game, or a JS demuxer, supplies the chunks. `drawImage` and `createImageBitmap` accept a `VideoFrame`. `copyExternalImageToTexture` copies a `VideoFrame`. `VideoFrame.close()` releases the frame. A later draw after `close()` fails. `colorSpace` returns a new `VideoColorSpace` each time. sRGB is primaries `bt709`, transfer `iec61966-2-1`, matrix `rgb`, and `fullRange` true. Display P3 uses primaries `smpte432` with that transfer, matrix, and range. `new VideoColorSpace(init)` accepts the WebCodecs color enum strings and defaults a missing member to null. `new VideoFrame()` or `new AudioData()` with a missing or invalid argument throws `TypeError`.

`Yuzuko.demuxMp4(data)` reads a progressive MP4 file. It is installed with the `codec` feature on the main global and in a dedicated worker, and it is absent from an `AudioWorklet` processor realm. `data` is a `BufferSource`. The call is synchronous. It returns `{ video, audio }`. `video` is every `avc1` or `avc3` track, and `audio` is every `mp4a` track. Any other track is omitted. A track with no samples has an empty `chunks` array. A video track has `codec`, `description`, `codedWidth`, `codedHeight`, and `chunks`. `codec` is the four-character code, a dot, and the three `avcC` profile bytes in uppercase hexadecimal, such as `avc1.42E01E`. `description` is that `avcC` record. `chunks` is an array of `EncodedVideoChunk` in decode order. The sample bytes stay as they are stored, including length-prefixed NAL units. A sample listed in `stss` has `type` `"key"`. A track with no `stss` marks every sample `"key"`. An empty `stss` marks none of them `"key"`. An audio track has `codec`, `description`, `sampleRate`, `numberOfChannels`, and `chunks`. `codec` is `mp4a.`, the `esds` object type indication in lowercase hexadecimal, a dot, and the audio object type, such as `mp4a.40.2`. `description` is the `AudioSpecificConfig`. `sampleRate` and `numberOfChannels` come from that config when it names them, and otherwise from the sample entry. A channel configuration of 0 uses the sample entry count, which may be 0. A `wave` box may contain `esds`. Every audio chunk has `type` `"key"`. `timestamp` and `duration` are microseconds on the media timeline. Each value is `ticks * 1_000_000 / timescale`, truncated toward 0, so durations may not sum to the next timestamp. Composition offsets apply to video timestamps. One edit-list entry with a non-negative media time and a media rate of 1 is subtracted from those timestamps, so a timestamp may be negative. Any other edit list is left unused. There is no presentation clock and no frame is decoded. The caller passes `chunks` to `VideoDecoder` or `AudioDecoder`.

A value that is not a `BufferSource` throws `TypeError` with `The MP4 data is not a BufferSource`. A detached buffer throws `TypeError` with `The MP4 data is detached`. An empty buffer throws `EncodingError` with `The MP4 data is empty`. A file with no `moov` throws `EncodingError` with `The MP4 file has no movie`. A movie with no H.264 or AAC track throws `EncodingError` with `The MP4 file has no H.264 or AAC track`. `moof` without samples in `moov` throws `EncodingError` with `Fragmented MP4 is not supported`. A sample past the end of the file throws `EncodingError` with `The MP4 sample is outside the file`. More than one sample entry used by one track throws `EncodingError` with `The MP4 track changes sample entries`. An encrypted sample entry throws `EncodingError` with `Encrypted MP4 is not supported`. An H.264 entry without `avcC` throws `EncodingError` with `The H.264 track has no avcC`. An AAC entry without an `AudioSpecificConfig` throws `EncodingError` with `The AAC track has no AudioSpecificConfig`. More than 2,000,000 samples in one track throws `EncodingError` with `The MP4 track has too many samples`. A failure while copying the samples into JavaScript throws `EncodingError` with `The MP4 samples could not be returned`. Any other malformed box throws `EncodingError` with `The MP4 file is not a valid container`.

`new VideoFrame(image, init)` copies an `ImageBitmap`, an `OffscreenCanvas`, or another `VideoFrame`. `new VideoFrame(buffer, init)` copies an 8-bit buffer whose `format` is `RGBA`, `BGRA`, `RGBX`, `BGRX`, `I420`, `I420A`, `I422`, `I444`, or `NV12`. `timestamp` is required unless the source is a `VideoFrame`. `duration`, `alpha` (`keep` or `discard`), `visibleRect`, and a `displayWidth` / `displayHeight` pair are stored. A buffer constructor keeps the visible rectangle and places it at the origin, so the coded size becomes that rectangle. An image or canvas keeps its coded size. Copying a `VideoFrame` keeps the coded planes. When display size is omitted, it scales from the source display size and the new visible rectangle. `allocationSize` returns the byte size of a `copyTo`. `copyTo` returns a promise of plane layouts and writes sample bytes into the destination, leaving bytes outside each sample row untouched. An RGB `format` in the copy options converts samples into `colorSpace`, which defaults to `srgb`. `display-p3` uses the same sRGB and Display P3 matrices as canvas. `colorSpace` is ignored when `format` is omitted or is not an RGB format. `clone` returns an independent frame. `close()` may be called more than once. After `close()`, `format` is null, `codedWidth`, `codedHeight`, `displayWidth`, and `displayHeight` are 0, `visibleRect` and `codedRect` are null, and every `colorSpace` member is null. `timestamp` and `duration` stay. `clone`, `allocationSize`, `copyTo`, `drawImage`, and `createImageBitmap` then throw `InvalidStateError`. `I420P10` and the other 10-bit and 12-bit formats throw `NotSupportedError`. `rotation` is clockwise degrees. It is rounded to a multiple of 90 the way `Math.round` rounds, then reduced to 0, 90, 180, or 270. A non-finite value throws `TypeError`. `flip` mirrors horizontally after that rotation. `visibleRect` stays in coded pixels. `copyTo` returns those coded samples. When `displayWidth` and `displayHeight` are omitted, a rotation of 90 or 270 swaps them relative to the visible size. Copying a `VideoFrame` composes the new rotation and flip with the source. `drawImage`, `createImageBitmap`, and `copyExternalImageToTexture` sample the oriented visible pixels at that oriented size. An explicit display size does not scale the draw further.

`AudioDecoder`, `EncodedAudioChunk`, and `AudioData` live on `globalThis`. `new AudioData(init)` copies `u8`, `s16`, `s32`, or `f32` samples, interleaved or planar. `sampleRate` must be greater than 0. `numberOfFrames` and `numberOfChannels` must be greater than 0, at most 16,777,216 frames and 32 channels. `data` must cover every sample. `timestamp` is a microsecond `long long` and may be negative. `duration` is `numberOfFrames / sampleRate * 1_000_000`, truncated toward 0. `allocationSize` and `copyTo` take `planeIndex`, and optional `frameOffset`, `frameCount`, and `format`. An interleaved destination requires `planeIndex` 0 and writes every channel. A planar destination writes one channel. `copyTo` converts among those eight formats. A same-format copy keeps the stored bytes. `frameOffset` past the end, or a `frameCount` that passes the end, throws `RangeError`. A destination shorter than `allocationSize` throws `RangeError`. `clone` returns an independent copy. `close()` may be called more than once. After `close()`, `format` is null, `sampleRate`, `numberOfFrames`, `numberOfChannels`, and `duration` are 0, and `timestamp` stays. `clone`, `allocationSize`, and `copyTo` then throw `InvalidStateError`. `transfer` is ignored: the bytes are copied and the source buffer stays attached. Samples copied from `AudioData` fill an `AudioBuffer` that an `AudioContext` can play.

The video codec is H.264 (`avc1` and `avc3`). The audio codec is AAC (`mp4a`). Both work on every target triple, not only through a macOS decoder. H.264 uses only that triple's platform hardware decoder: VideoToolbox on Darwin with `kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder`, the Media Foundation H.264 transform decoding through DXVA on a Direct3D 11 device on Windows, and VA-API on Linux. The Linux decoder submits picture and slice buffers with `vaBeginPicture`, `vaRenderPicture`, and `vaEndPicture`, then maps the surface. The Windows decoder requires a device that offers the `H264_VLD_NOFGT` decoder profile, feeds the transform an Annex B stream, and accepts only output samples that are DXGI surfaces, so a software fallback inside the transform fails the decode. There is no software H.264 decoder and no software fallback when that decoder is missing or rejects the config. `hardwareAcceleration: "prefer-software"` does not select a software H.264 decoder. A structurally valid `avcC` that the hardware decoder rejects throws `NotSupportedError` and does not produce a frame. An omitted or empty `description` is an Annex B byte stream. `configure` accepts it and leaves the decoder configured without opening the hardware decoder. The hardware session opens when `decode` processes an access unit that contains both an SPS and a PPS. A present `description` is a length-prefixed `avcC` and is not scanned for start codes. AAC is a portable decoder. Any other codec string, including `vp8`, `vp09`, `av01`, `opus`, and `vorbis`, fails `configure` with `NotSupportedError` and produces no frame or samples. An empty codec, a missing required member, a detached `description`, or a zero `codedWidth`, `codedHeight`, `displayAspectWidth`, `displayAspectHeight`, `sampleRate`, or `numberOfChannels` throws `TypeError` from `configure`. `VideoDecoder.isConfigSupported` and `AudioDecoder.isConfigSupported` resolve `{ supported, config }` without opening a decoder and without producing a frame or samples. `supported` is true when `configure` would accept the codec string (`avc1`, `avc3`, or `mp4a`) and false for any other codec string. The returned video `config` copies `codec`, `description` as a new buffer, the dimensions, `hardwareAcceleration`, and `colorSpace`. An omitted `colorSpace` member is null. A member the runtime does not recognize is left out. A missing or invalid config, and a call with too few arguments, rejects `TypeError`. `hardwareAcceleration: "prefer-software"` does not select a software decoder and does not change `supported`. `VideoDecoder.decode` enqueues the chunk and returns before the hardware decoder runs. `decodeQueueSize` is the number of enqueued chunks. It decreases when a chunk is taken for decoding, and the `ondequeue` property is called with no arguments when it decreases. `VideoDecoder` is not an `EventTarget`, so that call is only the function property and no `dequeue` event is dispatched. A key chunk is required after construction, `configure`, and `reset`. `decode` of a delta chunk then throws `DataError` and leaves the decoder configured. The check uses the chunk's `type` and does not inspect the bitstream. Queued chunks run in a microtask. `output` is called as each picture is produced, so a script that assigns `output` before that microtask sees the frame. A sample the hardware decoder rejects closes the decoder, calls `error` with `EncodingError`, and rejects an in-flight `flush` with that same error. `flush` returns a promise that settles after the queued chunks have been processed. When the decoder is not configured, including when it is closed, `flush` does not throw: the promise rejects `InvalidStateError`. `reset` and `close` reject an in-flight `flush` with `AbortError`, drop queued chunks, and set `decodeQueueSize` to 0. `configure` does that same reset before opening the new configuration. `AudioDecoder` still runs `decode` to completion before it returns, keeps `decodeQueueSize` at 0, does not throw `DataError`, resolves `flush` without waiting, and does not reject `flush` with `AbortError`.

`ImageDecoder` lives on `globalThis`. `type` is an image MIME type. `data` is a `BufferSource` or a `ReadableStream` of `BufferSource` chunks. `isTypeSupported` resolves `true` for `image/png`, `image/jpeg`, `image/webp`, and `image/jxl`. Any other type resolves `false`. The constructor still returns a decoder for an unsupported type. `completed`, `tracks.ready`, and `decode()` then reject `NotSupportedError`. A supported still image has one track, `frameCount` 1, `animated` false, and `repetitionCount` 0. An animated PNG, WebP, or JPEG XL has `animated` true, `frameCount` set to the number of frames, and `repetitionCount` set to the extra plays after the first. A file play count of 0 means forever, so `repetitionCount` is `Infinity`. One play is `0`. `decode()` resolves `{ image, complete }` with a `VideoFrame`. `decode({ frameIndex })` returns that frame. `timestamp` is the sum of the earlier durations and `duration` is that frame's delay, both in microseconds. A still frame has `timestamp` 0 and `duration` null. A `frameIndex` past the last frame rejects `RangeError`. `createImageBitmap` uses the first frame. The runtime does not advance frames. `image/gif` is not supported. Deselecting the track makes `decode()` reject `InvalidStateError`. `close()` rejects a pending `decode()` with `InvalidStateError`. `reset()` rejects one with `AbortError`. Decoding uses the same pure-Rust decoders as `createImageBitmap`, on a background thread, and a panic becomes `EncodingError`. `colorSpaceConversion` `"default"` stores sRGB and `"none"` keeps an sRGB or Display P3 source. `desiredWidth` and `desiredHeight` scale every frame. One of them preserves aspect ratio. `preferAnimation` does not select another track, because a file has one track. A zero desired dimension throws `TypeError`. A truncated buffer that cannot be decoded rejects `completed` and `tracks.ready` with `EncodingError`.

`VideoEncoder` and `AudioEncoder` are absent. `structuredClone` of a `VideoFrame` or an `AudioData` copies it and leaves the source open. Listing it in the transfer list of `postMessage` or `structuredClone` closes the source, and the receiver gets the pixels or samples. `EncodedVideoChunk` and `EncodedAudioChunk` keep an optional `duration`. A missing or null `duration` is null. A non-negative finite number is truncated toward 0. `copyTo` copies the bytes into the destination. A destination that is detached or shorter than `byteLength` throws `TypeError`. The chunks clone, including `duration`, and are not transferable. A closed frame or audio data throws `DataCloneError`. A dedicated worker can receive each of them.

## Workers

Dedicated workers only. `new Worker(url)` and `new Worker(url, { type: "module" })`. The worker global is a dedicated-worker scope: `self === globalThis`. It has the WinterTC subset already on the main global, plus `postMessage`, `onmessage`, `importScripts`, `navigator`, `location`, `close`, `onerror`, `btoa`, `atob`, the timer functions, `requestAnimationFrame`, `cancelAnimationFrame`, and `FileReaderSync`. It has no `Yuzuko.Window`, no `Yuzuko.setDialogLabels`, no `Yuzuko.AccessibleNode`, no `Yuzuko.ComputedAccessibleNode`, no `document`, and no `SharedWorker`. A classic worker script and `importScripts` run. The application entry stays an ES module. A worker may start another dedicated worker. An evaluation error and `terminate()` are reported to the owner and stop further worker tasks. `postMessage` carries values that `structuredClone` can carry, including a transferred `ArrayBuffer`, `MessagePort`, `OffscreenCanvas`, `ImageBitmap`, `AudioData`, `VideoFrame`, `ReadableStream`, `WritableStream`, or `TransformStream`, and a cloned `ImageData`, `EncodedAudioChunk`, or `EncodedVideoChunk`. The worker keeps running while the owner is busy.

`new Worker` loads `file:` under the entry directory, `http:` and `https:` paths under that sandbox, `data:` including a `;base64` payload, and `blob:`. `importScripts` loads `file:`, `data:`, and `blob:` the same way. An `http:` or `https:` `importScripts` argument is fetched. Its `Content-Type` must be a JavaScript MIME type, ignoring parameters and ASCII case: `application/ecmascript`, `application/javascript`, `application/x-ecmascript`, `application/x-javascript`, `text/ecmascript`, `text/javascript`, `text/javascript1.0`, `text/javascript1.1`, `text/javascript1.2`, `text/javascript1.3`, `text/javascript1.4`, `text/javascript1.5`, `text/jscript`, `text/livescript`, `text/x-ecmascript`, or `text/x-javascript`. Any other type fails with `NetworkError`. A `blob:` URL created on one thread can be loaded on another. Parsing the URL captures the blob bytes. `importScripts` parses every argument before it runs any of those scripts, so `revokeObjectURL` inside an earlier script does not fail a later URL of that call. A revoked URL that was not captured fails with `NetworkError`. `close()` finishes the script that called it and then stops further tasks on that worker. `terminate()` aborts the worker, including the running script. `FileReaderSync` is on that worker and is absent from the main global. `readAsText`, `readAsBinaryString`, `readAsDataURL`, and `readAsArrayBuffer` read a `Blob` before the call returns, using the same bytes as `FileReader`. `readAsBinaryString` returns one code unit per byte. An empty blob with no type reads as `data:application/octet-stream;base64,`. A value that is not a `Blob` throws `TypeError`.

On the worker global, `self.fonts` and `globalThis.fonts` are the same `FontFaceSet`. `queryLocalFonts` and `FontData` are on that global. A face that finishes `load()` there is usable from that set. On the main global, `globalThis.fonts` stays absent and `Yuzuko.fonts` remains the main `FontFaceSet`. `queryLocalFonts` and `FontData` are on the main global as well. A face added to one set is not visible in the other.

In a worker, `DOMMatrix` and `DOMMatrixReadOnly` do not expose the Window stringifier or the CSS-string constructor. `String(new DOMMatrix())` is `"[object DOMMatrix]"`. `String(new DOMMatrixReadOnly())` is `"[object DOMMatrixReadOnly]"`. `new DOMMatrix("matrix(1, 0, 0, 1, 0, 0)")` and `new DOMMatrix("")` throw `TypeError`. `setMatrixValue` is absent. On the main global, `String(new DOMMatrix())` stays `"matrix(1, 0, 0, 1, 0, 0)"`. `WebKitCSSMatrix` stays absent in both realms.

An `OffscreenCanvas` transferred with `postMessage` arrives usable in the dedicated worker. A `"2d"` context can draw, and the sender's canvas is detached. A `Yuzuko.Window` and a `Yuzuko.WindowSurface` cannot be transferred. Transferring a `Yuzuko.Window` throws `DataCloneError`. They stay usable on the main thread. `WindowSurface.transferControlToOffscreen()` returns the `OffscreenCanvas` a worker draws. A worker `requestAnimationFrame` callback then copies that canvas into the window that owns it. A WebGPU device can be used only on the thread that created it. Using or transferring it from the other side fails. A transferred `ImageBitmap`, `AudioData`, or `VideoFrame` arrives usable, and the sender is detached or closed.

Shared workers, service workers, and worklets stay out of scope.

## WinterTC

The baseline is the common API index at <https://min-common-api.proposal.wintertc.org/>. `globalThis` is a WinterTC execution realm, not a `Window` interface. As WinterTC allows, added global properties do not have to be `readonly`.

The realm includes:

- `Event`, `EventTarget`, `CustomEvent`, `ErrorEvent`, `PromiseRejectionEvent`, `DOMException`
- `atob`, `btoa`, `setTimeout`, `clearTimeout`, `setInterval`, `clearInterval`, `queueMicrotask`, `reportError`, `structuredClone`, `performance`, `self`, `onerror`, `onunhandledrejection`, `onrejectionhandled`, `navigator.userAgent`, `navigator.hardwareConcurrency`, and `console` `log` / `info` / `warn` / `error` / `debug` / `trace` / `assert` / `time` / `timeLog` / `timeEnd` / `count` / `countReset` / `group` / `groupCollapsed` / `groupEnd` / `dir`
- `AbortController`, `AbortSignal`
- `TextEncoder`, `TextDecoder`, `URL`, `URLSearchParams`, `Blob`, `File`
- V8's built-in `WebAssembly`, including `compileStreaming` and `instantiateStreaming`. Direct `.wasm` imports and `import source` of `.wasm`. A deferred `.wasm` import instantiates on the first observation of its namespace
- `fetch`, `Headers`, `Request`, and `Response` for `file:`, `blob:`, `data:`, `http:`, and `https:`, when the runtime is built with the `fetch` feature
- `createImageBitmap`, `ImageBitmap`, `ImageData`, when the runtime is built with the `image` feature
- `OffscreenCanvas`, when the runtime is built with the `window` feature. `CanvasRenderingContext2D`, `ImageBitmapRenderingContext`, `Path2D`, and `DOMMatrix`, when it is built with the `canvas` feature
- WebGPU and `navigator.gpu`, when the runtime is built with the `webgpu` feature
- The streams interfaces, including byte-stream readers, `ReadableStream.from`, owning streams that transfer `ArrayBuffer`, `MessagePort`, `ImageBitmap`, `AudioData`, and `VideoFrame`, transferable `ReadableStream`, `WritableStream`, and `TransformStream`, `TextEncoderStream`, and `TextDecoderStream`
- `FormData`, `CompressionStream`, `DecompressionStream`
- `URLPattern`
- `Crypto`, `CryptoKey`, `SubtleCrypto`, and `crypto`. Digests are SHA-1, SHA-256, SHA-384, SHA-512, SHA3-256, SHA3-384, SHA3-512, SHAKE128, and SHAKE256. Signatures are HMAC, KMAC128, KMAC256, RSASSA-PKCS1-v1_5, RSA-PSS, ECDSA, Ed25519, Ed448, ML-DSA-44, ML-DSA-65, ML-DSA-87, and SLH-DSA-SHA2-128f. Encryption and wrapping are RSA-OAEP, AES-GCM, AES-CBC, AES-CTR, AES-KW, AES-OCB, and ChaCha20-Poly1305. Derivation is HKDF, PBKDF2, Argon2d, Argon2i, Argon2id, ECDH, X25519, and X448. Encapsulation is ML-KEM-512, ML-KEM-768, ML-KEM-1024, MLKEM768-P256, MLKEM768-X25519, and MLKEM1024-P384. `SubtleCrypto.supports` reports whether an operation is implemented. `getPublicKey` derives a public key from a private key
- `MessageChannel`, `MessagePort`, `MessageEvent`
- `caches`, when the runtime is built with the `fetch` feature
- ECMA-402 `Intl`, as specified under Internationalization

`fetch` for `http:` and `https:` follows redirects, retries a `421` once on a new connection, cancels with `AbortSignal`, and answers a blob URL `Range` with `206` or a network error. `https:` negotiates HTTP/2 by ALPN and uses HTTP/3 on a later request to that origin after an `Alt-Svc` `h3` advertisement. `http:` stays HTTP/1.1. A cached request does not keep the caller's `AbortSignal`. `DOMMatrix` and `DOMMatrixReadOnly` are installed with the `canvas` feature. Shared `DOMMatrix` methods are inherited from `DOMMatrixReadOnly`. `WebKitCSSMatrix` is not defined.

`XMLHttpRequest`, installed with `fetch`, exposes `open`, `setRequestHeader`, `send`, `overrideMimeType`, `getResponseHeader`, `getAllResponseHeaders`, `status`, `statusText`, and `responseText`. `send` loads `http:`, `https:`, and `data:`. It collects the response before returning, then queues `onload` or `onerror`. The handler receives a trusted `ProgressEvent`. The request is not an `EventTarget`, so the event is not dispatched. `target` is the request, and `currentTarget` is the request only while the handler runs. A load sets `loaded` and `total` to the body length, and `lengthComputable` is true only when that length is not 0. An error leaves `loaded`, `total`, and `lengthComputable` at their zero values. `responseText` is empty until the response is done. The charset comes from the response `Content-Type`. A `charset` parameter on the MIME type passed to `overrideMimeType` replaces it, and `open` does not clear that override. A label that does not name an encoding, and a missing charset, decode as UTF-8. The replacement encoding stays in force when the label maps to it. A leading UTF-8, UTF-16LE, or UTF-16BE byte order mark selects that encoding and is omitted from the text. `overrideMimeType` throws `InvalidStateError` once the response is done. `Blob.textStream()` returns a `ReadableStream` of UTF-8 strings and ignores the blob type's charset.

`fetch`, `fs`, `socket`, `udp`, `tcp`, `http`, `signal`, `child`, `ffi`, `window`, `accessibility`, `image`, `canvas`, `webgpu`, `audio`, and `codec` are the Cargo features in Build features, and the default build enables every one. `yuzuko package` copies the executable that invoked it, so the packaged game has the features of that build. A game that loads bytes with `fetch`, including a `file:` URL under the entry directory, keeps the `fetch` feature. A `FontFace` whose source is a URL and `AudioWorklet.addModule` call `fetch`. Without `fetch` the font load fails and `addModule` rejects `AbortError`.

Dedicated workers are available. WinterTC does not require them.

The Annex B accessor `Object.prototype.__proto__` is deleted before user code runs. The main realm, a dedicated worker, and an `AudioWorklet` processor realm each do this. An object literal `{ __proto__: null }` still sets [[Prototype]]. `Object.getPrototypeOf` and `Object.setPrototypeOf` still get and set it. Reading or assigning `obj.__proto__` uses an own property when one exists, and an assignment creates one. `Object.assign` of `JSON.parse('{"__proto__": null}')` leaves [[Prototype]] unchanged. There is no flag that restores the accessor.

Streams, URLPattern, and compression algorithms may be implemented in embedded JavaScript when the tests pass. `EventTarget`, the event classes, `DOMException`, `AbortController`, `URL`, `URLSearchParams`, `Blob`, and `File` are `#[op]` classes. Windows, canvases, WebGPU, decoding, sockets, timers, URL parsing, encoding, Blob bytes, and the crypto boundary are Rust.

`structuredClone` covers the WinterTC built-ins in this runtime, plus `ArrayBuffer`, typed arrays, `Map`, `Set`, `Date`, `RegExp`, and `Error`. It also copies `Blob`, `File`, `DOMException`, `DOMPoint`, `DOMPointReadOnly`, `DOMRect`, `DOMRectReadOnly`, `DOMQuad`, `DOMMatrix`, `DOMMatrixReadOnly`, `CryptoKey`, `ImageData`, `ImageBitmap`, `AudioData`, `VideoFrame`, `EncodedAudioChunk`, and `EncodedVideoChunk`. A copy leaves the source usable. The `transfer` option moves an `ArrayBuffer`, a `MessagePort`, an `OffscreenCanvas`, an `ImageBitmap`, an `AudioData`, a `VideoFrame`, a `ReadableStream`, a `WritableStream`, or a `TransformStream`. A transferred `ImageBitmap` is detached. A transferred `AudioData` or `VideoFrame` is closed. A closed `VideoFrame` reports a null `format`, zero sizes, and null `visibleRect` and `codedRect`, and keeps `timestamp` and `duration`. `clone`, `allocationSize`, `copyTo`, and drawing it throw `InvalidStateError`. A closed `AudioData` reports a null `format`, and `sampleRate`, `numberOfFrames`, `numberOfChannels`, and `duration` are 0. `timestamp` stays. `clone`, `allocationSize`, and `copyTo` throw `InvalidStateError`. An `EncodedAudioChunk` or an `EncodedVideoChunk` is cloned and is not transferable. A closed or detached source throws `DataCloneError`. An unsupported host object throws `DataCloneError`. A transferred readable or writable is locked, and a transferred transform locks both sides. The destination is a new stream. Chunks, errors, cancel, and abort cross with structured clone, and the transfer counts one extra chunk of backpressure. A locked stream throws `DataCloneError`. A stream left out of the transfer list throws `DataCloneError`. A transferred byte stream arrives as a default stream. A `MessagePort`, an `OffscreenCanvas`, a `ReadableStream`, a `WritableStream`, or a `TransformStream` that is listed for transfer but not reachable from the value stays usable. An `ImageBitmap`, `AudioData`, or `VideoFrame` in that position is still detached.

## Internationalization

ECMA-402 `Intl` uses the full ICU locale data linked into this V8 build, the same data Deno ships with rusty_v8. Locales beyond English resolve from that data. `Collator`, `DateTimeFormat`, `DisplayNames`, `DurationFormat`, `ListFormat`, `Locale`, `NumberFormat`, `PluralRules`, `RelativeTimeFormat`, and `Segmenter` are present on `Intl`. The main realm, a dedicated worker, and an `AudioWorklet` processor realm each have this object.

`Intl.DateTimeFormat.supportedLocalesOf` accepts `ja`, `zh-Hans`, `ar`, `th`, and `hi`. `Intl.NumberFormat("ja-JP", { style: "currency", currency: "JPY" })` formats `1234` as `￥1,234`. `Intl.PluralRules("ar")` selects `zero`, `two`, and `few` for `0`, `2`, and `3`. `Intl.Segmenter("ja", { granularity: "word" })` splits `東京都庁` into `東京` and `都庁`. `Intl.Collator("sv")` orders `ä` after `z`. `Intl.supportedValuesOf("calendar")` includes `japanese` and `islamic`. `Intl.DisplayNames(["zh-Hans"], { type: "region" })` names `JP` as `日本`.

The default locale and time zone are the host values ICU reports for the process. Every realm shares them. A `Date` and an `Intl.DateTimeFormat` that omit `timeZone` use that zone.

`Intl.v8BreakIterator` is absent. Breaks go through `Intl.Segmenter`.

Dialog button labels are not taken from `Intl` or ICU. The application sets them with `Yuzuko.setDialogLabels`.

## Server messages

`EventSource` and `WebSocket` live on `globalThis`, including a dedicated worker. `EventSource` is installed with the `fetch` feature and is absent from a build without it. `EventSource` reads an `http:` or `https:` `text/event-stream` response and dispatches `MessageEvent`s. The default reconnection time is 3000ms. `retry` replaces it. `close()` stops the stream. `withCredentials` is stored on the object.

`new WebSocket(url)` opens a `ws:` or `wss:` connection. The second argument is a protocol string, a sequence of protocol strings, or a `WebSocketInit` dictionary `{ protocols, targetAddressSpace }`. `targetAddressSpace` is `"loopback"`, `"local"`, or `"public"`. The connection fails unless the connected address is in that space. Any other value, including the legacy alias `"private"`, throws `TypeError`. A further argument is ignored. `send` accepts a string, a `BufferSource`, or a `Blob`. `binaryType` is `"blob"` or `"arraybuffer"`. `close(code, reason)` uses code 1000 or a code from 3000 through 4999. `extensions` stays `""`.

`new WebSocketStream(url, options)` opens the same kind of connection. `options.protocols` is a sequence of protocol strings. `options.signal` aborts the handshake. `opened` resolves to `{ readable, writable, protocol, extensions }`. Text messages are strings and binary messages are `Uint8Array` values. `closed` resolves to `{ closeCode, reason }` after a clean close and rejects with `WebSocketError` after an unclean close. `close({ closeCode, reason })` sends that close. A readable `pull` reads one message, and further socket reads wait, which applies receive backpressure.

`new WebTransport(url, options)` opens an `https` session on its own QUIC connection. It is on `globalThis` in the main realm and in a dedicated worker. `ready` resolves when the handshake finishes. `reliability` is `"pending"` until then, then `"supports-unreliable"` or `"reliable-only"`. `closed` resolves `{ closeCode, reason }` after a clean close and rejects with `WebTransportError` after an unclean close. A failed handshake rejects both `ready` and `closed`, and those rejections are marked handled. `datagrams` is `{ readable, writable }`. With `datagramsReadableType` omitted, `readable` is a default readable stream whose high-water mark is 0. Each chunk is one datagram as a `Uint8Array`, and `getReader({ mode: "byob" })` throws `TypeError`. `datagramsReadableType: "bytes"` makes `readable` a readable byte stream whose high-water mark is 0. One read returns one datagram. A BYOB view shorter than that datagram rejects the read with `RangeError`. A view whose element size is not 1, including a `DataView`, rejects the read with `TypeError`. An empty datagram in that mode rejects the read with `TypeError`. Any other string throws `TypeError` from the constructor. `createBidirectionalStream()` resolves `{ readable, writable }`. That `readable`, and each value read from `incomingUnidirectionalStreams`, is a readable byte stream, so a short BYOB view keeps the unread bytes for the next read. `createUnidirectionalStream()` resolves a `WritableStream`. `incomingBidirectionalStreams` and `incomingUnidirectionalStreams` are default readable streams of those values. `close({ closeCode, reason })` closes the session. `congestionControl` is `"default"`, `"throughput"`, or `"low-latency"`.

## Virtual accessibility nodes

The `accessibility` feature installs `Yuzuko.AccessibleNode` and `Yuzuko.ComputedAccessibleNode` on the main realm, `attachAccessibleRoot()` and `accessibleRoot` on `Yuzuko.WindowSurface`, and `getComputedAccessibleNode()` on `Yuzuko.Window`. An `AudioWorklet` processor realm does not have them. A dedicated worker does not have `Yuzuko.AccessibleNode` or `Yuzuko.ComputedAccessibleNode`. With the feature off, the names are absent. Neither constructor is on `globalThis`. A bare `AccessibleNode` or `ComputedAccessibleNode` reference throws `ReferenceError`. The feature enables `window`. There is no permission prompt.

This is the Accessibility Object Model Phase 3 virtual accessibility node draft. The explainer hangs that tree from a canvas: `attachAccessibleRoot()`, then `accessibleRoot.appendChild(new AccessibleNode())`. There is no document and no canvas element, so those members are on `WindowSurface`. `parent`, `firstChild`, `lastChild`, `previousSibling`, and `nextSibling` read the authored `AccessibleNode` tree. The published tree is read through `ComputedAccessibleNode`.

`new Yuzuko.AccessibleNode()` creates a node. It is an `EventTarget`. `attachAccessibleRoot()` has length 0. It returns one new root and does not accept an existing node. A second call throws `InvalidStateError` with `An accessible root is already attached.` `accessibleRoot` returns that root, or `null` before one is attached. Children of the root are added with `appendChild`. After the window's `close()`, both `attachAccessibleRoot()` and `accessibleRoot` throw `InvalidStateError` with `The window is closed.` The closed check runs before the already-attached check. Closing keeps the stored root. Further platform updates stop. Tree edits on the nodes still succeed. Calling either member on a value that is not a `WindowSurface` throws `TypeError` with `illegal invocation`.

`appendChild(node)`, `insertBefore(node, child)`, `replaceChild(node, child)`, and `removeChild(node)` accept only `AccessibleNode` values. Any other value throws `TypeError` with `The node must be an AccessibleNode.` `insertBefore` has length 1. `null` or `undefined` for `child` appends. `insertBefore(node, node)` keeps that node in place when it is already a child, and throws `NotFoundError` with `The node is not a child.` when it is not. Moving a node detaches it from its previous parent first. A cycle throws `HierarchyRequestError` with `The node is an ancestor.` Appending a node onto itself is that ancestor error. The accessible root cannot be moved, which is checked before the ancestor check, and throws `HierarchyRequestError` with `The accessible root cannot be moved.` `removeChild` and `replaceChild` of a node that is not a child throw `NotFoundError` with `The node is not a child.` `replaceChild` returns the removed child. When `node` and `child` are the same child, it returns that child and does not change the tree. A missing required argument throws `TypeError` with `Not enough arguments`.

The authored properties are `role`, `name`, `description`, `roleDescription`, `valueText`, `checked`, `pressed`, `disabled`, `expanded`, `hidden`, `selected`, `busy`, `modal`, `required`, `readOnly`, `multiselectable`, `multiline`, `valueNow`, `valueMin`, `valueMax`, `colCount`, `rowCount`, `colIndex`, `colSpan`, `rowIndex`, `rowSpan`, `level`, `setSize`, `posInSet`, `live`, `orientation`, `focusable`, `offsetLeft`, `offsetTop`, `offsetWidth`, `offsetHeight`, and `offsetParent`. Omitted ARIA members are absent. There is no `label` property. `name` is the accessible name.

`role`, `name`, `description`, `roleDescription`, and `valueText` use `ToString`. `null` or `undefined` clears the property, and the getter then returns `null`. An explicit empty string is stored and the getter returns `""`. `checked` and `pressed` are `"true"`, `"false"`, or `"mixed"`, or `null`. A boolean uses `ToString`, so `true` becomes `"true"`. Any other token throws `TypeError` with `The value is not a valid token.` `disabled`, `expanded`, `hidden`, `selected`, `busy`, `modal`, `required`, `readOnly`, `multiselectable`, and `multiline` are booleans or `null`. `null` and `undefined` clear them. Any other value uses the ordinary boolean conversion. `focusable` defaults to `false` and is not nullable. `valueNow`, `valueMin`, and `valueMax` are finite numbers or `null`. A non-finite number throws `TypeError` with `The value must be a finite number.` `colCount`, `rowCount`, `colIndex`, `colSpan`, `rowIndex`, `rowSpan`, `level`, `setSize`, and `posInSet` are IDL `long` values or `null`. `null` and `undefined` are checked before `ToInt32`, because `null` would become `0`. `live` is `"off"`, `"polite"`, or `"assertive"`, or `null`. `orientation` is `"horizontal"` or `"vertical"`, or `null`. Any other token throws `TypeError` with `The value is not a valid token.`

`offsetLeft`, `offsetTop`, `offsetWidth`, and `offsetHeight` are CSS pixel lengths. The getter returns `null` or a string such as `"30px"`. The setter accepts `null`, `undefined`, a finite number, or a string primitive that is a bare number or a `px` length. The `px` suffix is case-insensitive. Ends are trimmed. `"30 px"` is rejected because of the internal space. Exponents and other units, including `"30em"`, throw `TypeError` with `The offset must be a CSS pixel length.` A string object or any other object throws that error. A non-finite number throws that error. `offsetParent` is `null` or a strict ancestor `AccessibleNode`. Any other value throws `TypeError` with `The offset parent must be an AccessibleNode.` A node that is not a strict ancestor, including the node itself, throws `HierarchyRequestError` with `The offset parent is not an ancestor.`

When `offsetParent` is unset, `offsetLeft` and `offsetTop` are relative to the parent. When it is set, they are relative to that ancestor and are not accumulated through the parents in between. The author root's parent is not a JavaScript node, so its offsets are relative to the window and `parent` is `null`. A missing width or height is a zero-size rectangle. A non-finite length is published as 0. A non-finite or non-positive window scale is published as 1.

`focus()` moves accessible focus only. It does not move operating-system focus and it does not dispatch `focus`, `blur`, or `accessiblefocus`. A node that is not `focusable` throws `InvalidStateError` with `The node is not focusable.` A node that is not connected to a root throws `InvalidStateError` with `The node is not connected.` A connected node whose window is closed throws `InvalidStateError` with `The window is closed.` Those checks run in that order.

Assistive-technology actions are events. They bubble through the authored parent chain and are not cancelable. They do not bubble to the window. `dispatchEvent` stays single-target. The events are `accessibleclick`, `accessiblecontextmenu`, `accessibledecrement`, `accessiblefocus`, `accessibleincrement`, `accessiblescrollintoview`, and `accessiblesetvalue`. There are no `onaccessible*` handlers. `accessiblesetvalue` is a `CustomEvent` whose `detail` is the requested finite number, or `null` when the request has no number. The event does not change `valueNow` or `valueText`. Delivering these events reveals that an assistive technology is in use. There is no permission prompt.

`Yuzuko.__test.inject`, when `accessibility` is enabled, accepts a record `{ kind: "accessible", type, node, value }`. `type` is `click`, `contextmenu`, `decrement`, `focus`, `increment`, `scrollintoview`, or `setvalue`. `node` is an `AccessibleNode`. `value` is the number for `setvalue`. An unknown `type` throws `TypeError` with `The value is not a valid token.` When `accessibility` is off, that `kind` throws `TypeError` with `unknown inject kind accessible`. The call dispatches the event before it returns.

The platform tree is an AccessKit tree updated through `accesskit_winit`. The AccessKit root is a synthetic window node. It is not a JavaScript `AccessibleNode`. Its role is `window`, its name is the window title, and its bounds are the client area in physical pixels. The author root is its only child. Before a root is attached, activation still publishes that window node. The toolkit name is `Yuzuko`. Every update is a full tree. Focus is the focused virtual node when that node is in the published tree, and the window node otherwise. Removing a focused node, or a focused descendant, clears that focus. Moving a node does not.

Recognized `role` tokens are `alert`, `alertdialog`, `application`, `article`, `banner`, `blockquote`, `button`, `caption`, `cell`, `checkbox`, `code`, `columnheader`, `combobox`, `complementary`, `contentinfo`, `definition`, `dialog`, `directory`, `document`, `emphasis`, `feed`, `figure`, `form`, `generic`, `grid`, `gridcell`, `group`, `heading`, `image`, `img`, `link`, `list`, `listbox`, `listitem`, `log`, `main`, `marquee`, `math`, `menu`, `menubar`, `menuitem`, `menuitemcheckbox`, `menuitemradio`, `meter`, `navigation`, `none`, `note`, `option`, `paragraph`, `presentation`, `progressbar`, `radio`, `radiogroup`, `region`, `row`, `rowgroup`, `rowheader`, `scrollbar`, `search`, `searchbox`, `separator`, `slider`, `spinbutton`, `status`, `strong`, `switch`, `tab`, `table`, `tablist`, `tabpanel`, `term`, `textbox`, `time`, `timer`, `toolbar`, `tooltip`, `tree`, `treegrid`, `treeitem`, and `window`. The match is case-sensitive after `ToString`. `generic`, `none`, `presentation`, an omitted role, and `""` publish a generic container. The getter returns `null` when `role` was cleared and `""` when it was set to an empty string. `directory` publishes a list. `image` and `img` publish an image. `option` publishes a list-box option. `progressbar` publishes a progress indicator. `radio` publishes a radio button. `scrollbar` publishes a scroll bar. `searchbox` publishes a search input. `separator` publishes a splitter. `textbox` publishes a text input, or a multiline text input when `multiline` is true. An unknown token publishes an unknown role. The author role `window` is distinct from the synthetic window root.

`name` is published as the AccessKit label. `valueText` is the AccessKit value. `valueNow`, `valueMin`, and `valueMax` are the numeric value and its range. Both `valueText` and `valueNow` may be set. `checked` and `pressed` share one toggled state. `checkbox`, `radio`, `menuitemcheckbox`, `menuitemradio`, `switch`, and `option` publish `checked`. `button` publishes `pressed`. Any other role publishes `checked` when it is set, and otherwise `pressed`. `hidden`, `disabled`, `busy`, `modal`, `required`, `readOnly`, and `multiselectable` are published only when true. `false` and `null` omit the flag. `expanded` and `selected` are published when set, including `false`.

AccessKit `row_index`, `column_index`, `level`, and `position_in_set` are zero-based. The authored `rowIndex`, `colIndex`, `level`, and `posInSet` are the ARIA one-based numbers. A value of 1 or more is published as that value minus 1. A value of 0 is published as 0. A negative value is stored and omitted. `colSpan`, `rowSpan`, `colCount`, `rowCount`, and `setSize` are published when they are 0 or greater. A negative count is stored and omitted. `setSize` is written on the author node.

Actions come from the role, not from whether a listener is present. `click` is enabled for `button`, `link`, `checkbox`, `radio`, `tab`, `menuitem`, `menuitemcheckbox`, `menuitemradio`, `switch`, `option`, and `treeitem`. `increment`, `decrement`, and `setValue` are enabled for `slider`, `scrollbar`, and `spinbutton`. `setValue` is also enabled for `textbox` and `searchbox`. `focus` is enabled when `focusable` is true. `expand` and `collapse` are enabled when `expanded` is set, including `false`. `showContextMenu` and `scrollIntoView` are enabled on every node.

On macOS the adapter is `accesskit_macos`. On Windows it is UI Automation through `accesskit_windows`. On Linux it is AT-SPI through `accesskit_unix`, using that crate's default `async-io` executor. A window is created invisible, the adapter is attached, and then the requested visibility is applied. A headless process, including `yuzuko test`, builds the same tree and does not create an adapter.

## Computed accessibility tree

`Yuzuko.ComputedAccessibleNode` is the Phase 4 computed accessibility tree for one published node. `new Yuzuko.ComputedAccessibleNode()` throws `TypeError` with `Illegal constructor`. It is not an `EventTarget`. There is no element, so `Yuzuko.Window.getComputedAccessibleNode(node)` takes an `AccessibleNode`. It has length 1.

A missing argument throws `TypeError` with `Not enough arguments`. A value that is not an `AccessibleNode` throws `TypeError` with `The node must be an AccessibleNode.` Calling it on a value that is not a `Window` throws `TypeError` with `illegal invocation`. Those checks throw. They do not reject the promise. After the window's `close()`, the call throws `InvalidStateError` with `The window is closed.` before it returns a promise. The closed check runs after the argument checks.

The returned promise settles on a queued task. The snapshot is the published tree at that task, so a write made after the call and before the task is included. A node that is not in this window's published tree resolves `null`. That includes a detached node and a node whose root is on another window. The same published node resolves the same `ComputedAccessibleNode` while that object is reachable. If the window closes before the task runs, the promise rejects with `InvalidStateError` and `The window is closed.`

`ensureUpToDate()` has length 0. It returns a promise that resolves `undefined` after replacing that window's snapshot. Calling it on a value that is not a `ComputedAccessibleNode` rejects `TypeError` with `illegal invocation`. A closed window rejects `InvalidStateError` with `The window is closed.` A node that has left the tree keeps its object. After the next snapshot, its string and boolean properties are `null`, `focusable` is `false`, and its links are `null`. Property reads keep the previous snapshot until `ensureUpToDate()` or another `getComputedAccessibleNode()` for that window replaces it.

The synthetic window node is in the snapshot. It is not an `AccessibleNode`. Its `role` is `"window"`, its `name` is the window title, including `""`, and its offsets are the client size in CSS pixels at `0`, `0`. Its `parent` and `offsetParent` are `null`. The author root's computed `parent` is that window node. Before a root is attached, the window node has no children.

`role` is the published token, not the authored string. `generic`, `none`, `presentation`, an omitted role, and `""` are `"generic"`. `img` is `"image"`. `directory` is `"list"`. An unknown token is `"unknown"`. `textbox` stays `"textbox"`. `multiline` is `true` only when that text box is published as a multiline text input, and `null` otherwise. `name`, `description`, `roleDescription`, and `valueText` are the published strings, or `null` when omitted. An empty string stays `""`.

`checked` and `pressed` are the one published toggled state, `"true"`, `"false"`, or `"mixed"`. `checkbox`, `radio`, `menuitemcheckbox`, `menuitemradio`, `switch`, and `option` publish `checked`. `button` publishes `pressed`. Any other role publishes `checked` when it is set, and otherwise `pressed`. The other of the pair is `null`.

`hidden`, `disabled`, `busy`, `modal`, `required`, `readOnly`, and `multiselectable` are `true` when published and `null` otherwise. `false` is omitted. `expanded` and `selected` are the published boolean, including `false`, or `null` when unset. `focusable` is the published boolean and defaults to `false`. `valueNow`, `valueMin`, and `valueMax` are the published numbers or `null`. `rowIndex`, `colIndex`, `level`, and `posInSet` are the published zero-based values. A value of 1 or more is that value minus 1. A value of 0 stays 0. A negative value is `null`. `colSpan`, `rowSpan`, `colCount`, `rowCount`, and `setSize` are the published values when they are 0 or greater. A negative count is `null`. `live` and `orientation` are the published tokens or `null`.

`offsetLeft`, `offsetTop`, `offsetWidth`, and `offsetHeight` are CSS pixel strings of the resolved local offset, such as `"0px"`. A missing or non-finite length is `"0px"`. They are not physical pixels and they are not accumulated through parents. `offsetParent` is the computed node of the authored offset parent when that node is a strict ancestor in the published tree, and `null` otherwise. `parent`, `firstChild`, `lastChild`, `previousSibling`, and `nextSibling` walk the published tree. There is no `children` list. `atomic`, `autocomplete`, `keyShortcuts`, `placeholder`, and `label` are absent.

## File system

The `fs` feature installs file-system functions on `Yuzuko` in the main realm and in a dedicated worker. An `AudioWorklet` processor realm does not have them. With the feature off, the names are absent. There is no permission prompt. A build that includes `fs` may read and write any path the operating-system user can use. A packaged application can read its bundle and cannot modify it. `yuzuko run`, `yuzuko repl`, and `yuzuko test` record no bundle root, so the project tree can be written. `file:` fetch stays limited to the entry directory and does not depend on `fs`.

A path string is a file-system path. A relative path uses the process current directory. `Yuzuko.cwd` reads that directory and `Yuzuko.chdir` replaces it. The path does not use the entry directory or the directory of `import.meta.url`. A `URL` object must be `file:`. A string that looks like `file://...` is a path, not a URL. An empty string is the current directory. A path component that is not UTF-8 is returned with U+FFFD for those bytes. An unpaired surrogate in a JavaScript string becomes U+FFFD.

For a packaged app, the process current directory is the directory from which the user launched the app. `Yuzuko.chdir` may name a directory inside the bundle and does not modify the bundle. A write through a `file:` URL under `import.meta.url` is inside the bundle and fails. A save file is an absolute path outside the bundle.

A synchronous call blocks the calling thread, including the main event loop. An asynchronous call runs the operating-system call on a background thread and settles the promise on that realm's event loop. A bad argument throws `TypeError` before the call. An operating-system failure rejects the promise, or throws from the synchronous form.

The functions are `open`, `create`, `readFile`, `readTextFile`, `writeFile`, `writeTextFile`, `mkdir`, `remove`, `rename`, `stat`, `lstat`, `readDir`, `copyFile`, `realPath`, `truncate`, `makeTempDir`, `makeTempFile`, `chmod`, `chown`, `link`, `symlink`, `readLink`, and `utime`. Each of those has a synchronous twin whose name ends in `Sync`. `umask` is synchronous and has no twin. `watchFs` returns an `FsWatcher` immediately. `create` and `createSync` take only a path and open it for write, with create and truncate, and without read. `readDir` returns an array of `{ name, isFile, isDirectory, isSymlink }`. The array is not sorted and is not an iterator. Directory entry types do not follow a symlink.

`Yuzuko.FsFile` has no public constructor. `new Yuzuko.FsFile()` throws. `read` and `readSync` take an `ArrayBufferView`, including a `DataView`, and return the number of bytes read, or `null` at the end of the file. A zero-length view returns `0` and does not change the view. Bytes past the read are left as they were. A bare `ArrayBuffer` throws `TypeError` with `The buffer must be a Uint8Array.` `write` and `writeSync` take a buffer source and return the count from one write, which can be short. `seek` and `seekSync` take an offset and a whence and return the new position. `stat` and `statSync` describe the open descriptor. `truncate` and `truncateSync` default the length to 0. `sync` and `syncSync` flush the file and its metadata. `syncData` and `syncDataSync` flush the file data. `lock`, `tryLock`, `unlock`, `utime`, `isTerminal`, `setRaw`, `readable`, and `writable` are also on `FsFile`. `close` releases the descriptor. A second close throws `BadResource` with `The file is closed.` When `Symbol.dispose` exists, it calls `close`. When `Symbol.asyncDispose` exists, it returns a promise for that close. Dropping the last handle closes the descriptor. `close` waits for an in-flight lock or terminal change on that file, then restores a terminal mode saved by `setRaw`.

`Yuzuko.SeekMode` is the object `{ Start: 0, Current: 1, End: 2 }`. Whence must be exactly 0, 1, or 2, including a non-finite value, or the call throws `whence must be 0, 1, or 2.` The offset must be a finite integer. A non-finite offset throws `The seek offset must be a finite number.` A non-integer throws `The seek offset must be an integer.` A negative offset from the start throws `A seek from the start cannot be negative.` Current and end allow a negative offset. The returned position is a number and can be above 2^53.

`Yuzuko.errors` is an object of classes: `NotFound`, `AlreadyExists`, `PermissionDenied`, `IsADirectory`, `NotADirectory`, `NotSupported`, `Busy`, `Interrupted`, `InvalidData`, `BadResource`, and `FilesystemLoop`. The thrown value is an instance of the constructor installed at startup. Replacing `Yuzuko.errors.NotFound` later does not change the thrown object. A symbolic-link loop throws `FilesystemLoop`. `DirectoryNotEmpty` and any other operating-system failure throw a plain `Error` whose message is the operating-system text. `TypeError` stays `TypeError`.

`chmod` and `chmodSync` set the mode to an integer from 0 through 262143. The process umask does not change that result. The operating system may store fewer bits. On macOS the stored mode is 16 bits. The call follows the final symlink. On Windows only the owner write bit is used: when `0o200` is clear the file becomes read-only, and when it is set the read-only attribute is cleared. The other bits are ignored.

`chown` and `chownSync` take a user id and a group id. `null` leaves that id unchanged. `undefined` throws `TypeError`. Each number is a finite integer from 0 through 4294967295. A non-integer throws `The user id must be an integer or null.` or `The group id must be an integer or null.` The call follows the final symlink. On Windows both forms throw `NotSupported` with `Changing the owner is not supported on Windows.` after the arguments are checked and before the file system is touched, including when the path does not exist.

`umask` returns the previous mask. An omitted argument leaves the mask unchanged. On Unix an omitted mask is read by setting the mask to 0 and restoring it, and another thread can observe that window. A supplied mask is an integer from 0 through 262143. On Windows the result is always 0 and a mask in that range is ignored. A bad mask still throws `TypeError`. `umask` does not use a path and does not consult the bundle root.

`link` and `linkSync` create a hard link. The final component of neither path is followed. Both paths are bundle mutations of the directory entry. `symlink` and `symlinkSync` store `oldPath` as the link text. A string is stored as that string, and a file URL is stored as its file-system path. The target does not have to exist, and `oldPath` is not resolved against the bundle. Only `newPath` is created. `options.type` is `"file"`, `"dir"`, or `"junction"`. Omitted means `"file"`. Any other value throws `The link type must be "file", "dir", or "junction".` Unix ignores the type. Windows creates that kind of link. A junction is a mount-point reparse point. A relative junction target is joined to the current directory for the reparse text. Creating a symlink can throw `PermissionDenied` on Windows when the process is not allowed to create one. `readLink` and `readLinkSync` return the stored text without canonicalizing it. A NUL in that text becomes U+FFFD. A path that is not a symlink throws the operating-system error. Reading a link does not modify the bundle.

`utime` and `utimeSync`, and `FsFile.utime` and `FsFile.utimeSync`, set the access time and the modification time. Each time is a `Date` or a finite number of seconds since the Unix epoch, and a fractional second is kept. A string, boolean, `null`, or `undefined` throws `The time must be a Date or a finite number.` before number conversion, so the string `"1700000000"` is rejected. Another object uses `ToNumber`. A non-finite result throws that same message. A time the operating system cannot store throws `The time is out of range.` Those checks throw before the call. The path form follows the final symlink. `FsFile.utime` uses the open descriptor. When a bundle root is set, a descriptor whose path cannot be resolved is rejected.

`lock` and `lockSync` take an optional exclusive flag. An omitted or `undefined` flag is a shared lock. A present value uses `ToBoolean`, so `null` is shared. A conversion that throws rejects or throws `The lock mode could not be read.` The lock is advisory and covers the whole file. The asynchronous form waits on a background thread. `tryLock` and `tryLockSync` return a boolean and do not wait. A closed file throws `BadResource` with `The file is closed.` On Unix the lock is per process, so a second lock in the same process succeeds. On Windows a lock is per handle. Unlocking a file that is not locked succeeds on Unix. On Windows that unlock throws a plain `Error`. An interrupted wait inside the lock is retried. The lock is not a bundle check.

`isTerminal` returns a boolean. A descriptor that is not a terminal returns `false` and does not throw. `setRaw(mode, options)` is synchronous. `mode` uses `ToBoolean`, and a conversion that throws is rethrown. `options.cbreak` defaults to false. `null` or `undefined` options use that default. The first `setRaw(true)` saves the terminal mode. A later `setRaw(true)` starts from that saved mode and does not replace it. `setRaw(false)` restores it. `setRaw(false)` with nothing saved does nothing, including on a file that is not a terminal. `setRaw(true)` on a file that is not a terminal throws `NotSupported` with `The file is not a terminal.` `cbreak` keeps signal characters. The call is not a bundle check.

`readable` and `writable` are getters. Each returns the same stream for that file. Calling the getter on a value that is not an `FsFile` throws `TypeError` with `illegal invocation`. `readable` is a default readable stream. A pull reads at most 16384 bytes into a new buffer. `null` closes the stream. A read of 0 enqueues nothing. Any other count enqueues that prefix. A rejected read errors the stream. Cancelling the stream does not close the file. `writable` accepts a view, an `ArrayBuffer`, or a `SharedArrayBuffer`. Anything else errors the stream with `Data must be a BufferSource.` A short write is repeated until the chunk is consumed. A write that returns 0 errors the stream with `The write wrote no bytes.` An empty chunk resolves without writing. Closing or aborting the writable stream does not close the file. Both streams use the file's current position.

`watchFs(paths, options)` returns a `Yuzuko.FsWatcher`. There is no public constructor. `paths` is a string, a file URL, or an array of those. An empty array throws `At least one path is required.` A value that is none of those throws `The paths must be a string, a file URL, or an array.` A bad element uses the path error. Each path must already exist, and that check follows a symlink. Watching is a read, so a path inside the bundle is allowed. `options.recursive` defaults to true. `null` or `undefined` options use that default. `{ recursive: null }` is not recursive. The watcher is an async iterator. `[Symbol.asyncIterator]()` returns the watcher, and calling it on another object throws `TypeError` with `illegal invocation`. `next` resolves `{ value, done }`. `value` is `{ kind, paths, flag }`. `kind` is `"access"`, `"create"`, `"modify"`, `"remove"`, `"rename"`, `"any"`, or `"other"`. `paths` are absolute path strings. `flag` is `"rescan"` when the watcher lost events, and it is omitted otherwise. A NUL in a reported path becomes U+FFFD. One action can produce several events. `close` stops the watcher. A second `close` throws `BadResource` with `The watcher is closed.` `return` stops the watcher and resolves `{ done: true }` even when it is already closed, and it does not consume a queued event. After the watcher is closed, `next` returns queued events and then `{ done: true }`. An open watcher keeps the process alive until `close` or `return`. On macOS the watch is a kqueue vnode watch. A directory event names that directory, not the child that changed. On Linux the watch is inotify and names the child. On Windows the watch is `ReadDirectoryChangesW`. More than 60 paths throws `NotSupported` with `Too many watch paths.` before the paths are checked. A file is watched through its parent, and only that file's events are reported. A file with no parent throws `NotSupported` with `The path has no parent directory.` A recursive watch that would add more than 4096 directories reports one `{ kind: "any", flag: "rescan", paths: [] }` and stops adding watches. An operating system other than Linux, macOS, and Windows throws `NotSupported` with `watchFs is not supported on this operating system.` before the paths are checked.

An option named `signal` is ignored. Unknown option fields are ignored.

Omitted `open` options, and `null` or `undefined`, mean `{ read: true }`. On an options object, `read` defaults to true and `write`, `append`, `truncate`, `create`, and `createNew` default to false, so `{ write: true }` is read and write. A missing property keeps the default. A present property uses `ToBoolean`, so `{ read: null }` does not read. `truncate` without `write` or `append` throws `truncate requires write access.` `create` or `createNew` without `write` or `append` throws `create requires write or append access.` `read`, `write`, and `append` all false throws `The file must be opened for reading or writing.` `append` together with `truncate` throws `append and truncate cannot both be set.` When both `create` and `createNew` are set, `createNew` is used. `mode` is an integer from 0 through 262143. On Unix it is applied to the new file or directory and the process umask still applies. On Windows `mode` is ignored. Opening a directory is allowed. A later read of that descriptor is operating-system dependent.

`writeFile` and `writeTextFile` create the file and truncate it unless `append` is set. `append` does not truncate. `create` defaults to true. Parent directories are not created. `writeFile` data is a buffer source, not a string (`Data must be a BufferSource.`). `writeTextFile` requires a string (`Text must be a string.`) and writes UTF-8. A buffer longer than 2147483647 bytes throws `The buffer is too large.` A detached buffer throws `The buffer is detached.` `readFile` has no size limit. A directory throws `IsADirectory` with `The path is a directory.` A symlink to a directory does too. `readTextFile` of bytes that are not UTF-8 throws `InvalidData` with `The file is not valid UTF-8.`

`mkdir` takes `{ recursive, mode }`. It resolves `.`, `..`, and symlinks, then creates that path, so a `..` that leaves the bundle does not create a directory inside the bundle on the way. Non-recursive creation of an existing directory throws `AlreadyExists`. Recursive creation succeeds when the resolved path is already a directory, and throws `AlreadyExists` when that path exists and is not a directory. `remove` without `recursive` deletes a file, an empty directory, or a symlink, and does not follow that symlink. `recursive: true` deletes a directory tree. A symlink is still deleted as a file. A directory that is not empty throws a plain `Error`. `rename` renames both paths. `copyFile` overwrites the destination and returns undefined. `truncate` on a path requires the file to exist. `stat` follows a final symlink. `lstat` does not. `realPath` returns the canonical path.

`FileInfo` is a plain object: `isFile`, `isDirectory`, `isSymlink`, `size`, `mtime`, `atime`, `birthtime`, `mode`, `dev`, `ino`, `nlink`, `uid`, `gid`, `blksize`, `blocks`, `isBlockDevice`, `isCharDevice`, `isFifo`, and `isSocket`. Times are `Date` objects, or `null` when the time is missing or before the Unix epoch. On Windows the identity fields are 0 and the device-type flags are false.

`makeTempDir` and `makeTempFile` take `{ dir, prefix, suffix }`. The default directory is the operating-system temporary directory. An omitted prefix or suffix is empty. A prefix or suffix that is not a string throws `The prefix must be a string.` or `The suffix must be a string.` One that contains `/`, `\`, or NUL throws `The prefix must not contain a path separator or NUL.` or the same sentence for the suffix. Those checks throw before the call. The name is the prefix, the time in hexadecimal, `-`, the process id, `-`, a counter in hexadecimal, and the suffix. A name that already exists is tried again, up to 64 times, and then throws `AlreadyExists` with `Could not create a unique temporary path.` The returned path is absolute, with parent symlinks resolved.

A path that is not a string or a `file:` URL throws `The path must be a string or a file URL.` Another URL scheme throws `The URL scheme must be file.` An unusable URL throws `The URL is invalid.` A `file:` URL that is not a path throws `The file URL is invalid.` Options that are not an object, `null`, or `undefined` throw `Options must be an object.` A getter that throws, or a field that cannot be read, throws `The options could not be read.` `mode` outside 0 through 262143 throws `mode must be an integer from 0 through 262143.` A length that is not a finite integer of at least 0 throws `The length must be an integer that is at least 0.` A panic in the background call rejects or throws `The file operation failed.`

A packaged launch records the bundle root before the entry runs. On macOS the root is the `.app` bundle. On Windows and Linux it is the directory that contains the executable and `yuzuko.bundle.json`. A mutation whose resolved path is that directory, or a path inside it, throws `NotSupported` with `Cannot modify the application bundle.` `open` for write, `writeFile`, `writeTextFile`, `truncate`, `chmod`, `chown`, `utime`, and a `copyFile` destination follow the final symlink, so a symlink outside the bundle that points inside it is rejected. `remove`, `rename`, `link`, `mkdir`, `symlink`'s new path, and a temporary path keep the directory entry: deleting a symlink that lives outside the bundle is allowed when the symlink points into the bundle. The text stored in a symbolic link is not a bundle check. `FsFile.utime` rejects a descriptor inside the bundle when a bundle root is set. The updater writes `Updates/` from Rust and is not subject to this guard.

## Datagrams

The `udp` feature installs `Yuzuko.listenDatagram` and `Yuzuko.Datagram` on the main realm and in a dedicated worker. An `AudioWorklet` processor realm does not have them. With the feature off, the names are absent. There is no permission prompt. A build that includes `udp` may send and receive datagrams on any address the operating-system user can use. The names are not on `globalThis`. `listenDatagram` is not a constructor. `Yuzuko.Datagram` has no public constructor. `new Yuzuko.Datagram()` throws `TypeError` with `Illegal constructor`. Calling `send`, `receive`, or `close`, or reading `addr`, on a value that is not a `Datagram` throws `TypeError` with `illegal invocation`.

`listenDatagram(options)` binds a UDP socket and returns the `Datagram`. It has length 0. An omitted argument, `null`, or `undefined` binds `127.0.0.1` and port `0`. `options` otherwise must be an object. Anything else throws `TypeError` with `Options must be an object.` Unknown fields are ignored. `hostname` defaults to `"127.0.0.1"` when it is omitted or `undefined`. A present value is converted with `ToString`. `localhost` is ASCII case-insensitive and means `127.0.0.1`. Any other value must be an IP address, without surrounding brackets. A DNS name, an empty string, or a bracketed address throws `TypeError` with `The hostname must be an IP address or "localhost".` There is no DNS lookup. `port` defaults to `0` when it is omitted or `undefined`. A present value is converted with `ToNumber` and must be an integer from 0 through 65535. `-0` is 0. Any other result throws `TypeError` with `The port must be an integer from 0 through 65535.` If `ToString` or `ToNumber` throws, or a getter throws, `listenDatagram` throws that exception. When the options cannot be read for any other reason, it throws `TypeError` with `The options could not be read.` Port `0` asks the operating system for an ephemeral port. The call binds before it returns and blocks the caller for that bind. A failure throws `Error` with `The socket could not bind.` The message does not include operating-system text.

`addr` returns a new plain object on each read. `hostname` is the bound IP address, so a `localhost` bind reports `127.0.0.1`. `port` is the bound port and is not `0` after an ephemeral bind. Those values do not change.

`send(data, addr)` has length 2. Fewer arguments throw `TypeError` with `Not enough arguments.` `data` is a `BufferSource`: an `ArrayBuffer`, a `SharedArrayBuffer`, or a view, including a view with a byte offset. A string or any other value throws `TypeError` with `The data must be a BufferSource.` A detached buffer throws `The buffer is detached.` A buffer longer than 65507 bytes throws `The datagram is too large.` Those checks throw before a promise is returned. `addr` must be an object. `null`, `undefined`, or any other value throws `TypeError` with `The address must be an object.` A failure while reading it throws `TypeError` with `The address could not be read.` It uses the same hostname rules, including the `127.0.0.1` default. An omitted port is `0`. Its port uses `ToNumber` and must be an integer from 1 through 65535, so an omitted port throws. Any other result throws `TypeError` with `The port must be an integer from 1 through 65535.` A destination whose address family differs from the bound socket throws `TypeError` with `The address family does not match the socket.` The call then returns a promise and sends on a background thread. The promise resolves to the number of bytes the operating system accepted. An empty datagram resolves `0`. A closed socket rejects `Error` with `The socket is closed.` Any other operating-system failure rejects `Error` with `The datagram could not be sent.` A panic in the background call rejects that same message. The messages do not include operating-system text.

`receive()` has length 0. It returns a promise for the next datagram and does not block the realm thread. The result is a plain object `{ data, hostname, port }`. `data` is a new `Uint8Array` of the payload the operating system delivered, at most 65536 bytes. `hostname` and `port` are the source address. `hostname` has no brackets. At most one `receive` waits on a socket. A second call rejects `Error` with `A receive is already waiting.` A closed socket rejects `Error` with `The socket is closed.` `close` while a receive is waiting rejects that same message. A receive that the operating system reports as a connection reset keeps waiting, because UDP has no connection. Any other operating-system failure rejects `Error` with `The datagram could not be received.` A panic in the background call rejects that same message.

`close()` has length 0. It drops the socket. A second `close` throws `Error` with `The socket is closed.` Dropping the last handle also closes the socket. An open socket does not keep the process alive. A waiting `send` or `receive` does, until that promise settles. When the runtime cannot create that promise, `send` and `receive` throw `TypeError` with `The socket operation failed.` There is no multicast, no broadcast option, and no connected mode.

## TCP

The `tcp` feature installs `Yuzuko.listenTcp`, `Yuzuko.connectTcp`, `Yuzuko.TcpListener`, and `Yuzuko.TcpConn` on the main realm and in a dedicated worker. An `AudioWorklet` processor realm does not have them. With the feature off, the names are absent. There is no permission prompt. A build that includes `tcp` may connect to any address the operating-system user can use. The names are not on `globalThis`. `listenTcp` and `connectTcp` are not constructors. `Yuzuko.TcpListener` and `Yuzuko.TcpConn` have no public constructor. `new Yuzuko.TcpListener()` and `new Yuzuko.TcpConn()` throw `TypeError` with `Illegal constructor`. Calling a method, or reading an address, on a value that is not that object throws `TypeError` with `illegal invocation`.

`listenTcp(options)` binds a TCP socket and returns the `TcpListener`. It has length 0. An omitted argument, `null`, or `undefined` binds `127.0.0.1` and port `0`. `options` otherwise must be an object. Anything else throws `TypeError` with `Options must be an object.` Unknown fields are ignored. `hostname` defaults to `"127.0.0.1"` when it is omitted or `undefined`. A present value is converted with `ToString`. `localhost` is ASCII case-insensitive and means `127.0.0.1`. Any other value must be an IP address, without surrounding brackets. A DNS name, an empty string, or a bracketed address throws `TypeError` with `The hostname must be an IP address or "localhost".` There is no DNS lookup. `port` defaults to `0` when it is omitted or `undefined`. A present value is converted with `ToNumber` and must be an integer from 0 through 65535. `-0` is 0. Any other result throws `TypeError` with `The port must be an integer from 0 through 65535.` If `ToString` or `ToNumber` throws, or a getter throws, `listenTcp` throws that exception. When the options cannot be read for any other reason, it throws `TypeError` with `The options could not be read.` Port `0` asks the operating system for an ephemeral port. The call binds before it returns and blocks the caller for that bind. A failure throws `Error` with `The socket could not bind.` The message does not include operating-system text.

`addr` on a listener returns a new plain object on each read. `hostname` is the bound IP address, so a `localhost` bind reports `127.0.0.1`. `port` is the bound port and is not `0` after an ephemeral bind.

`accept()` has length 0. It returns a promise for the next connection and does not block the realm thread. The promise resolves a `TcpConn`. At most one `accept` waits on a listener. A second call rejects `Error` with `An accept is already waiting.` A closed listener rejects `Error` with `The socket is closed.` `close` while an accept is waiting rejects that same message. Any other operating-system failure rejects `Error` with `The connection could not be accepted.` A panic in the background call rejects that same message. The messages do not include operating-system text. When the runtime cannot create that promise, `accept` throws `TypeError` with `The socket operation failed.`

`connectTcp(addr)` has length 1. Fewer arguments throw `TypeError` with `Not enough arguments.` `addr` must be an object. `null`, `undefined`, or any other value throws `TypeError` with `The address must be an object.` A failure while reading it throws `TypeError` with `The address could not be read.` It uses the same hostname rules, including the `127.0.0.1` default. An omitted port is `0`. Its port uses `ToNumber` and must be an integer from 1 through 65535, so an omitted port throws. Any other result throws `TypeError` with `The port must be an integer from 1 through 65535.` The call then returns a promise and connects on a background thread. The promise resolves a `TcpConn`. A failure rejects `Error` with `The connection could not be opened.` A panic rejects that same message. When the runtime cannot create that promise, `connectTcp` throws `TypeError` with `The socket operation failed.`

`localAddr` and `remoteAddr` each return a new plain object on each read. `hostname` is an IP address without brackets. `port` is the local or peer port. A connection disables Nagle's algorithm when the operating system allows it.

`read(buffer)` has length 1. `buffer` is a `BufferSource`: an `ArrayBuffer`, a `SharedArrayBuffer`, or a view, including a view with a byte offset. A string or any other value throws `TypeError` with `The data must be a BufferSource.` A detached buffer throws `The buffer is detached.` A buffer longer than 2147483647 bytes throws `The buffer is too large.` A zero-length buffer resolves `0` and does not wait. Otherwise the call returns a promise and reads on a background thread. The promise resolves the number of bytes copied into `buffer`, which may be less than `buffer.byteLength`, or `null` when the peer has ended the stream. The copy happens when the promise settles. If `buffer` was detached or no longer covers those bytes, the promise rejects `Error` with `The buffer is detached.` At most one `read` waits on a connection. A second call rejects `Error` with `A read is already waiting.` A `write` may wait at the same time. A closed connection rejects `Error` with `The socket is closed.` `close` while a read is waiting rejects that same message, including when the operating system reports end of stream because this side closed. Any other operating-system failure rejects `Error` with `The bytes could not be read.` A panic rejects that same message. When the runtime cannot create that promise, `read` throws `TypeError` with `The socket operation failed.`

`write(data)` has length 1. `data` uses the same `BufferSource` rules and the same 2147483647-byte limit. Those checks throw before a promise is returned. The bytes are copied before the call returns, so the caller may reuse the buffer. An empty buffer resolves `0` and does not wait. Otherwise the call returns a promise, writes every copied byte, and resolves that count. It does not resolve a short write. At most one `write` waits. A second call rejects `Error` with `A write is already waiting.` A closed connection rejects `Error` with `The socket is closed.` Any other operating-system failure rejects `Error` with `The bytes could not be written.` A panic rejects that same message. When the runtime cannot create that promise, `write` throws `TypeError` with `The socket operation failed.`

`close()` has length 0 on a listener and on a connection. It ends the socket. A second `close` throws `Error` with `The socket is closed.` Dropping the last handle also closes it. An open listener or connection does not keep the process alive. A waiting `accept`, `connectTcp`, `read`, or `write` does, until that promise settles. There is no TLS, no DNS lookup, and no half-close.

## HTTP

The `http` feature installs `Yuzuko.listenHttp`, `Yuzuko.HttpServer`, and `Yuzuko.HttpRequest` on the main realm and in a dedicated worker. An `AudioWorklet` processor realm does not have them. With the feature off, the names are absent. There is no permission prompt. A build that includes `http` may accept connections on any address the operating-system user can use. The feature does not enable `tcp` or `fetch`. The names are not on `globalThis`. `listenHttp` is not a constructor. `Yuzuko.HttpServer` and `Yuzuko.HttpRequest` have no public constructor. `new Yuzuko.HttpServer()` and `new Yuzuko.HttpRequest()` throw `TypeError` with `Illegal constructor`. Calling a method, or reading `addr`, `method`, `url`, or `body`, on a value that is not that object throws `TypeError` with `illegal invocation`.

`listenHttp(options)` binds a TCP socket and returns the `HttpServer`. It has length 0. An omitted argument, `null`, or `undefined` binds `127.0.0.1` and port `0`. `options` otherwise must be an object. Anything else throws `TypeError` with `Options must be an object.` Unknown fields are ignored. `hostname` defaults to `"127.0.0.1"` when it is omitted or `undefined`. A present value is converted with `ToString`. `localhost` is ASCII case-insensitive and means `127.0.0.1`. Any other value must be an IP address, without surrounding brackets. A DNS name, an empty string, or a bracketed address throws `TypeError` with `The hostname must be an IP address or "localhost".` There is no DNS lookup. `port` defaults to `0` when it is omitted or `undefined`. A present value is converted with `ToNumber` and must be an integer from 0 through 65535. `-0` is 0. Any other result throws `TypeError` with `The port must be an integer from 0 through 65535.` If `ToString` or `ToNumber` throws, or a getter throws, `listenHttp` throws that exception. When the options cannot be read for any other reason, it throws `TypeError` with `The options could not be read.` Port `0` asks the operating system for an ephemeral port. The call binds before it returns and blocks the caller for that bind. A failure throws `Error` with `The socket could not bind.` The message does not include operating-system text.

The server speaks HTTP/1.0 and HTTP/1.1. There is no TLS and no HTTP/2. Each accepted connection carries one request. The request version must be `HTTP/1.0` or `HTTP/1.1`. The request target must be origin-form, beginning with `/`, at most 8192 bytes, and is returned without decoding. A missing `Host` header is accepted. `Transfer-Encoding` is accepted only when every coding is `identity`, and it does not replace `Content-Length`. A missing `Content-Length` is an empty body. Duplicate `Content-Length` values must be identical. The server does not send `100 Continue`. Bytes after the request body are ignored. The connection disables Nagle's algorithm when the operating system allows it.

`addr` on a server returns a new plain object on each read. `hostname` is the bound IP address, so a `localhost` bind reports `127.0.0.1`. `port` is the bound port and is not `0` after an ephemeral bind.

`accept()` has length 0. It returns a promise for the next request and does not block the realm thread. The promise resolves an `HttpRequest`. At most one `accept` waits. A second call rejects `Error` with `An accept is already waiting.` A closed server rejects `Error` with `The socket is closed.` `close` while an accept is waiting rejects that same message. A header block longer than 65536 bytes, or a body longer than 1048576 bytes, rejects `Error` with `The request is too large.` A malformed request rejects `Error` with `The request could not be read.` An operating-system failure while accepting the connection rejects `Error` with `The connection could not be accepted.` A panic in the background call rejects `The request could not be read.` The messages do not include operating-system text. When the runtime cannot create that promise, `accept` throws `TypeError` with `The socket operation failed.`

`method` is the request method. `url` is the request target, including the query. `body` returns a new `Uint8Array` of the request body on each read.

`respond(body, status)` has length 1. `body` is a string primitive or a `BufferSource`: an `ArrayBuffer`, a `SharedArrayBuffer`, or a view, including a view with a byte offset. A string is encoded as UTF-8. Any other value throws `TypeError` with `The data must be a string or a BufferSource.` A detached buffer throws `The buffer is detached.` A body longer than 1048576 bytes throws `The data is too large.` Those checks throw before a promise is returned. The bytes are copied before the call returns, so the caller may reuse the buffer. `status` defaults to `200` when it is omitted or `undefined`. A present value is converted with `ToNumber` and must be an integer from 100 through 599. Any other result throws `TypeError` with `The status must be an integer from 100 through 599.` The call then returns a promise and writes on a background thread. The response is `HTTP/1.1` with that status, a reason phrase, `Content-Length`, `Content-Type`, and `Connection: close`. A string uses `text/plain;charset=UTF-8`. A buffer uses `application/octet-stream`. The promise resolves `undefined`, and the connection then closes. A second `respond` while a write is waiting rejects `Error` with `A response is already waiting.` A second `respond` after the write has succeeded throws `Error` with `The response was already sent.` A closed request rejects `Error` with `The socket is closed.` A failed write rejects `Error` with `The response could not be written.` and closes the request. A panic rejects that same message. When the runtime cannot create that promise, `respond` throws `TypeError` with `The socket operation failed.`

`close()` has length 0. It closes the listening socket. A second `close` throws `Error` with `The socket is closed.` Dropping the last server handle also closes the listener. An accepted request stays open until `respond` finishes or that request is dropped. Dropping a request closes its connection. An open server or request does not keep the process alive. A waiting `accept` or `respond` does, until that promise settles.

## Signals

The `signal` feature installs `Yuzuko.addSignalListener` and `Yuzuko.removeSignalListener` on the main realm and in a dedicated worker. An `AudioWorklet` processor realm does not have them. With the feature off, the names are absent. There is no permission prompt. The feature does not enable `window`. The names are not on `globalThis`. Neither function is a constructor.

`addSignalListener(signal, listener)` and `removeSignalListener(signal, listener)` each have length 2. Fewer arguments throw `TypeError` with `Not enough arguments`. Extra arguments are ignored. `signal` must be the string primitive `"SIGINT"` or `"SIGTERM"`. The call does not use `ToString`. Any other type throws `TypeError` with `The signal must be a string.` Any other string throws `TypeError` with `The signal must be "SIGINT" or "SIGTERM".` `listener` must be a function. Any other value throws `TypeError` with `The listener must be a function.`

`addSignalListener` registers `listener` for that signal in the calling realm. The same function object is registered once; a second add for that pair does nothing. `removeSignalListener` removes one registration of that function object in the calling realm. Removing a function that is not registered succeeds. Listeners run in registration order, on the realm thread, with no arguments and with `this` as `undefined`. A return value is ignored. A listener added during a delivery does not run for that delivery. A listener removed before it is called does not run. A listener that throws does not skip the later listeners for that delivery. The exception is reported with `reportError`. During `yuzuko test` it fails the test.

The first listener for a signal, in any realm, replaces that signal's default terminate action. The handler only records the signal. It does not run JavaScript. Each recorded signal is delivered once to every realm that has a listener for it. Those listeners then run on that realm's thread. Removing the last listener for a signal, across every realm, restores the previous disposition. Shutting down a realm drops only that realm's listeners. A dedicated worker's shutdown does not restore the default action while the main realm, or another worker, still has a listener. A registered listener does not keep the process alive. A delivery that has been recorded and not yet run does, until those listeners are called. If the operating system cannot install the handler, `addSignalListener` throws `Error` with `The signal could not be caught.` and does not register that listener. A signal that cannot be recorded is dropped.

On Unix the handler is installed with `sigaction`, `SA_RESTART`, and without `SA_RESETHAND`. `SIGINT` and `SIGTERM` are those signals. On Windows, `SIGINT` is `CTRL_C_EVENT` and `SIGTERM` is `CTRL_BREAK_EVENT`, because Windows has no `SIGTERM`. A Windows process without a console does not observe those events. The handler returns that it handled the event only while a listener for that signal remains, so the process is not terminated. Other console events keep their default action.

`Yuzuko.raiseSignal(signal)` exists only during `yuzuko test`, on the main realm and in a dedicated worker, and only when `signal` is enabled. It is absent during `yuzuko run` and `yuzuko repl`. It has length 1. It uses the same `signal` checks. It asks the operating system to deliver that signal to this process and returns after the delivery has been queued for this realm. It does not call the listener. No listener for that signal in this realm throws `Error` with `The signal is not caught.` and does not deliver the signal. If the operating system does not accept the signal, or the delivery is not queued, it throws `Error` with `The signal could not be delivered.` On Unix the call is `raise`. On Windows it is `GenerateConsoleCtrlEvent` for the console process group.

## Child processes

The `child` feature installs `Yuzuko.spawn` and `Yuzuko.Child` on the main realm and in a dedicated worker. An `AudioWorklet` processor realm does not have them. With the feature off, the names are absent. There is no permission prompt. The feature does not enable `window`. The names are not on `globalThis`. `Yuzuko.spawn` is not a constructor. `new Yuzuko.Child()` throws `TypeError` with `Illegal constructor`.

`Yuzuko.spawn(command, options)` has length 1. Fewer arguments throw `TypeError` with `Not enough arguments`. Extra arguments are ignored. `command` must be a non-empty string primitive. The call does not use `ToString`. Any other type throws `TypeError` with `The command must be a string.` An empty string throws `TypeError` with `The command must not be empty.` `options` may be omitted, `null`, or `undefined`. Omitted options pass no arguments and use the default streams, environment, and directory. Any other value that is not an object, including an array, throws `TypeError` with `Options must be an object.` A getter that throws throws that exception. `options.args` may be omitted, `null`, or `undefined`, and then the child receives no arguments. Otherwise it must be an array of at most 4096 string primitives. Any other value throws `TypeError` with `The arguments must be an array of strings.` More than 4096 arguments throws `TypeError` with `There are too many arguments.` A `command` or argument that contains a null character throws `TypeError` with `The command and arguments must not contain a null character.` Fields other than `args`, `stdin`, `stdout`, `stderr`, `env`, and `cwd` are ignored and are not read.

`options.stdin` may be omitted, `null`, or `undefined`, and then standard input is `"null"`. Otherwise it must be the string primitive `"null"`, `"inherit"`, or `"piped"`. The call does not use `ToString`. Any other value, including `"collect"`, throws `TypeError` with `The standard input must be "null", "inherit", or "piped".` `options.stdout` and `options.stderr` may be omitted, `null`, or `undefined`, and then that stream is `"collect"`. Otherwise each must be the string primitive `"null"`, `"inherit"`, `"piped"`, or `"collect"`. The call does not use `ToString`. Any other value throws `TypeError` with `The standard stream must be "null", "inherit", "piped", or "collect".`

`options.env` may be omitted, `null`, or `undefined`, and then the child inherits the environment. Otherwise it must be an object and not an array. Any other value throws `TypeError` with `The environment must be an object.` Own enumerable string keys overlay the inherited environment. The environment is not cleared. Names inherited from the prototype are ignored, and so are symbol keys. A key follows `Yuzuko.env`: a non-empty string with no `=` and no NUL. The call does not use `ToString`. An empty name throws `TypeError` with `The name must not be empty.` A name containing `=` or NUL throws `TypeError` with `The name must not contain `=` or NUL.` A value must be a string primitive or `null`. `null` removes that name. A string sets it. Removals are applied before assignments, so on Windows, where names are case-insensitive, a removed spelling still ends as the assigned value when both are present. `undefined` or any other value throws `TypeError` with `The value must be a string or null.` A string containing NUL throws `TypeError` with `The value must not contain NUL.`

`options.cwd` may be omitted, `null`, `undefined`, or `""`, and then the child inherits the current directory. Otherwise it is a string or a `file:` URL, checked the same way as `Yuzuko.chdir`. A value that is neither throws `TypeError` with `The path must be a string or a file URL.` A URL that is not `file:` throws `TypeError` with `The URL scheme must be file.` An invalid URL throws `TypeError` with `The URL is invalid.` A `file:` URL that is not a path throws `TypeError` with `The file URL is invalid.` A path containing NUL throws `TypeError` with `The path must not contain NUL.` A relative path is relative to the process current directory. The path must already be a directory. A missing path throws `Error` with `The directory was not found.` A path that is not a directory throws `Error` with `The path is not a directory.` These failures are not `DOMException`. Any other failure to create the process throws `Error` with `The child process could not be started.`

The call does not use a shell. `"null"` closes that stream. `"inherit"` uses the parent's stream. `"piped"` connects a pipe. `"collect"` connects a pipe that the runtime reads. The call blocks only while the operating system creates the process. If that fails, `spawn` throws `Error` with `The child process could not be started.` and no child remains. JavaScript signal listeners are not installed in the child. A running child keeps the event loop alive until it has exited. A collected stream keeps it alive until that collection finishes. A piped read or write keeps it alive until that promise settles. If a pipe fills and the caller does not read it, or a piped standard input stays open while the child reads until the end of input, the child blocks, and `status()` and `output()` wait until the caller reads, closes standard input, or kills the process. Realm shutdown terminates any child that is still running. Dropping the `Child` object does not terminate the process. It does close a piped standard input that is still open.

The returned `Child` has `pid`, the process id as a number, including after the child has exited. `stdinMode`, `stdoutMode`, and `stderrMode` are those mode strings. `stdin` is a `WritableStream` when standard input is `"piped"`, and `null` otherwise. `stdout` and `stderr` are each a `ReadableStream` when that stream is `"piped"`, and `null` otherwise. Each read of one of those getters returns the same stream. A pull reads at most 16384 bytes. Piped streams have no byte cap. A chunk that is not a buffer source errors the writable stream with `TypeError` `The data must be a BufferSource.` A write that returns no bytes errors it with `TypeError` `The write wrote no bytes.` Closing the writable stream closes standard input. The getters throw `TypeError` with `illegal invocation` when the receiver is not a `Child`.

`readStdout(buffer)` and `readStderr(buffer)` have length 1. `buffer` is a `BufferSource`: an `ArrayBuffer`, a `SharedArrayBuffer`, or a view, including a view with a byte offset. Any other value throws `TypeError` with `The data must be a BufferSource.` A detached buffer throws `The buffer is detached.` A buffer longer than 2147483647 bytes throws `The buffer is too large.` Those checks throw before a promise is returned. A zero-length buffer resolves `0` and does not read. Otherwise the promise resolves the number of bytes written into `buffer`, or `null` at the end of the stream. A later read after the end resolves `null` again. One read can wait on each stream. A second read throws `TypeError` with `A read is already waiting.` When that stream is not `"piped"`, the call throws `TypeError` with `The standard output is not piped.` or `The standard error is not piped.` A read failure rejects `Error` with `The bytes could not be read.` and does not terminate the process. If the promise cannot be created, the call throws `TypeError` with `The bytes could not be read.`

`writeStdin(data)` has length 1. `data` uses those same `BufferSource` checks, which throw before a promise is returned. The bytes are copied before the call returns. The promise resolves the number of bytes written. One write can wait. A second write throws `TypeError` with `A write is already waiting.` When standard input is not `"piped"`, the call throws `TypeError` with `The standard input is not piped.` After it has been closed, a write throws `TypeError` with `The standard input is closed.` A write failure rejects `Error` with `The bytes could not be written.` If the promise cannot be created, the call throws `TypeError` with `The bytes could not be written.` `closeStdin()` has length 0. It closes a piped standard input. A second call throws `TypeError` with `The standard input is closed.` When standard input is not `"piped"`, it throws `TypeError` with `The standard input is not piped.` A write that is already waiting finishes, and the pipe then closes.

`output()` has length 0. It returns a promise for `{ code, signal, stdout, stderr }` when neither standard output nor standard error is `"piped"`. A piped standard input still allows `output()`. When either of those two streams is `"piped"`, `output()` throws `TypeError` with `The child output is not collected.` A second call returns the same promise. `code` is the exit status, or `null` when the process was terminated by a signal. `signal` is that signal's number, or `null` when the process exited. On Windows `signal` is always `null`. `stdout` and `stderr` are `Uint8Array` values. A `"collect"` stream holds the bytes and is limited to 1048576 bytes. A `"null"` or `"inherit"` stream is an empty `Uint8Array`. A larger collected stream terminates the process and rejects `output()` and `status()` with `Error` `The child output is too large.` Another collection failure rejects with `Error` `The child process failed.` If the promise cannot be created, `output()` throws `TypeError` with `The child process failed.`

`status()` has length 0. It returns a promise for `{ code, signal }` with the same `code` and `signal` rules. A second call returns the same promise. It does not read the pipes. If the promise cannot be created, `status()` throws `TypeError` with `The child process failed.` `kill()` has length 0. It terminates the process when it is still running, and does nothing when the process has already exited. Calling it again succeeds. On Unix it sends `SIGKILL`. On Windows it calls `TerminateProcess` with exit code 1. Killing the process closes its pipes, so a blocked read ends. `output()`, `status()`, `kill()`, `readStdout`, `readStderr`, `writeStdin`, and `closeStdin` throw `TypeError` with `illegal invocation` when the receiver is not a `Child`.

## Foreign functions

The `ffi` feature installs `Yuzuko.dlopen` and `Yuzuko.Library` on the main realm and in a dedicated worker. An `AudioWorklet` processor realm does not have them. With the feature off, the names are absent. There is no permission prompt. The feature does not enable `window`. The names are not on `globalThis`. `Yuzuko.dlopen` is not a constructor. `new Yuzuko.Library()` throws `TypeError` with `Illegal constructor`.

`Yuzuko.dlopen(path, symbols)` has length 2. Fewer arguments throw `TypeError` with `Not enough arguments`. Extra arguments are ignored. `path` must be a non-empty string primitive. The call does not use `ToString`. Any other type throws `TypeError` with `The path must be a string.` An empty string throws `TypeError` with `The path must not be empty.` A path that contains a null character throws `TypeError` with `The path must not contain a null character.` `symbols` must be an object. `null`, `undefined`, and any other non-object throw `TypeError` with `The symbols must be an object.` Reading a property that throws throws that exception. A failure to read the object throws `TypeError` with `The symbols could not be read.`

Each own enumerable string property is one symbol. Names inherited from the prototype are ignored, and so are symbol keys. The property name is the C name passed to the loader. A name that is not a string throws `TypeError` with `The symbol name must be a string.` An empty name throws `TypeError` with `The symbol name must not be empty.` A name that contains a null character throws `TypeError` with `The symbol name must not contain a null character.` More than 256 symbols throws `TypeError` with `There are too many symbols.` The property value must be an object and not an array. Any other value throws `TypeError` with `The symbol definition must be an object.`

`parameters` may be omitted, `null`, or `undefined`, and then the function takes no arguments. Otherwise it must be an array of at most 4 string primitives. An array longer than 4 is rejected before its elements are read. Each element is `"i32"`, `"f64"`, `"pointer"`, `"buffer"`, or `"cstring"`. `result` may be omitted, `null`, or `undefined`, and then the result is `void`. Otherwise it must be the string primitive `"void"`, `"i32"`, `"f64"`, `"pointer"`, or `"cstring"`. `"buffer"` is not a result. These strings do not use `ToString`. An `"f64"` parameter cannot be mixed with another parameter type, there are at most 2 `"f64"` parameters, and `result` is then `void` or `"f64"`. `"i32"`, `"pointer"`, `"buffer"`, and `"cstring"` are the integer class. They may be mixed with each other, there are at most 4, and `result` is then `void`, `"i32"`, `"pointer"`, or `"cstring"`. A `void`, `"i32"`, `"pointer"`, or `"cstring"` result with no parameters is allowed. Any other shape throws `TypeError` with `The function type is not supported.` These checks run before the library is opened.

The call blocks while the operating system loads the library and resolves every symbol. On Unix it is `dlopen` with `RTLD_NOW` and `RTLD_LOCAL`, then `dlsym` for each C name. The Mach-O leading underscore is not part of that name. On Windows it is `LoadLibraryW` of the path as UTF-16, then `GetProcAddress` for each name. On any other operating system the load fails. If the load fails, or any symbol is missing, `dlopen` throws `Error` with `The library could not be opened.` and does not return a library. A partial library is not left open.

The returned `Library` has `symbols`, one object. Each read returns that same object. Its own properties are host functions, named by the C names. A function's length is its parameter count. Calling one is not a construct call. Fewer arguments than the length throw `TypeError` with `Not enough arguments`. Extra arguments are ignored and are not converted. An `"i32"` argument uses `ToInt32`, so `valueOf` runs. An `"f64"` argument uses `ToNumber`. If `ToInt32` or `ToNumber` throws, that exception propagates. A `"pointer"` argument is `null`, a bigint primitive, or a number primitive. `null` and `0n` are the null pointer. A bigint must be an unsigned value that fits in a pointer. A number must be an integer from 0 through `Number.MAX_SAFE_INTEGER` that fits in a pointer. The call does not use `ToBigInt` or `ToNumber`, so a boxed bigint, a number object, and `undefined` are rejected. A `"buffer"` argument is a `BufferSource`: an `ArrayBuffer`, a `SharedArrayBuffer`, or a view, including a view with a byte offset. A detached buffer throws `TypeError` with `The buffer is detached.` Any other value throws `TypeError` with `The data must be a BufferSource.` A buffer longer than 2147483647 bytes throws `The buffer is too large.` The pointer passed to C addresses those bytes. A zero-length buffer may have a null data pointer, and that pointer is what is passed. The length is not implied; the caller passes it as a separate `"i32"` when the C function needs it. A `"cstring"` argument is a string primitive. The call does not use `ToString`. Any other value throws `TypeError` with `The string must be a string.` A string containing a null character throws `TypeError` with `The string must not contain a null character.` The pointer passed to C addresses that string as UTF-8 with a trailing NUL. A conversion that fails without an exception throws `TypeError` with `The argument could not be converted.`

A buffer or C-string pointer is valid only for that synchronous call. The C function must not retain it and must not call back into JavaScript. Bytes written into a buffer are visible in the JavaScript buffer when the call returns. A `void` result is `undefined`. An `"i32"` or `"f64"` result is a number. An `"i32"` result is the low 32 bits, read as a signed integer. A `"pointer"` result is a bigint. The null pointer is `0n`, not JavaScript `null`. A `"cstring"` result reads bytes until NUL, at most 1048576 bytes, and returns a string. Invalid UTF-8 becomes U+FFFD. A null pointer returns JavaScript `null`. If no NUL appears within 1048576 bytes, the call throws `Error` with `The result is not a C string.` An invalid native pointer has undefined behavior and may crash the process.

There is no `libffi`. An `"f64"` call uses one homogeneous `extern "C"` function of that shape: `() -> void`, `() -> f64`, `(f64) -> f64`, `(f64, f64) -> f64`, `(f64) -> void`, or `(f64, f64) -> void`. An integer-class call passes each argument in a pointer-sized integer register and passes only the declared arguments. An `"i32"` argument is zero-extended into that register. On the five targets in Platforms, a callee compiled for `int` reads the low 32 bits, and an `int` result is those low 32 bits of the return register. Every combination of up to 4 integer-class parameters with a `void`, `"i32"`, `"pointer"`, or `"cstring"` result is supported, including a result with no parameters.

`close()` has length 0. It unloads the library. On Unix that is `dlclose`. On Windows it is `FreeLibrary`. If the operating system refuses to unload, the library still stays closed to JavaScript. A second `close` throws `Error` with `The library is closed.` A call after `close` throws `Error` with `The library is closed.` `symbols`, `close`, and a symbol call throw `TypeError` with `illegal invocation` when the receiver or the function is not the library's. Dropping the `Library` unloads it. The `Library` must stay reachable while a symbol function runs. An open library does not keep the process alive. Realm shutdown unloads every library that is still open.

## Modules

Every static import, dynamic `import()`, `import defer`, `import.defer()`, `import.source()`, and entry path goes through `oxc_resolver`, then, for TypeScript, through `oxc_transformer`. The loader does not invoke `tsc` or the oxc CLI. `import.meta.url` is the absolute `file:` URL of the source file that was resolved, including a `.ts` or `.mts` URL.

Resolution options:

- Conditions are `import` and `default`. The `types` and `require` conditions are not used, so a resolver result is never a `.d.ts` or a CommonJS entry chosen by `require`.
- Extensions searched are `.ts`, `.mts`, `.js`, `.mjs`, `.json`, and `.wasm`.
- `extensionAlias` maps a `.js` specifier to `.ts` then `.js`, and a `.mjs` specifier to `.mts` then `.mjs`. `import "./foo.js"` loads `foo.ts` when that file exists.
- `tsconfig.json` discovery, `extends`, project references, `baseUrl`, and `paths` are oxc_resolver's. `compilerOptions.target`, `module`, `jsx`, and `strict` do not change loading.
- A bare specifier resolves through `package.json` `"imports"` and `"exports"` when oxc_resolver finds a file. Nothing is downloaded. `node:` throws `TypeError`. A result whose module type is CommonJS, an addon, or a declaration file throws `TypeError`.
- JSON, text, and bytes modules load with `with { type: "json" }`, `with { type: "text" }`, and `with { type: "bytes" }`. A `.wasm` evaluation import is linked as described below.

### WebAssembly modules

The loader loads `.wasm` as an ES module, in two forms: a direct import and `import source`.

A direct import instantiates the module and exposes its exports:

```js
import { add } from "./add.wasm";
```

`await import("./add.wasm")` does the same. The namespace follows the WebAssembly ES module integration. A function export is a JavaScript function. A memory export is a `WebAssembly.Memory`. A table export is a `WebAssembly.Table`. A global export is the value it holds.

`import defer * as ns from "./add.wasm"` compiles and links that module. `new WebAssembly.Instance`, and evaluation of the Wasm module's own imports, run on the first observation of `ns`.

Imports declared inside the Wasm module are specifiers. The loader resolves each one relative to the `.wasm` file with `oxc_resolver`, evaluates that module, and passes its exports as the import object. The target may be JavaScript, TypeScript, or another `.wasm` module. A missing export or a type mismatch fails instantiation and rejects the module graph.

A source-phase import compiles the bytes and does not instantiate them:

```js
import source mod from "./add.wasm";
// mod instanceof WebAssembly.Module
```

The Wasm module's own imports are not evaluated for a source-phase import. `WebAssembly.instantiate` is how that `Module` is instantiated later. oxc parses `import source` and leaves it in the graph. A source-phase import of JavaScript or TypeScript throws `TypeError`.

The pinned `v8` must report the module request phase to the host resolver. Invalid Wasm bytes reject evaluation. Dedicated workers use this same loader.

### Text and bytes modules

`import value from "./file" with { type: "text" }` and `import(url, { with: { type: "text" } })` load any file as a synthetic module whose default export is a string. The bytes are decoded as UTF-8. An invalid sequence becomes U+FFFD. A leading byte order mark stays in the string as U+FEFF. The file stays raw source, so `.js`, `.ts`, `.json`, `.wasm`, and extension-less files are left unparsed.

`import value from "./file" with { type: "bytes" }` and `import(url, { with: { type: "bytes" } })` load any file as a synthetic module whose default export is a `Uint8Array` of the raw bytes. `value.buffer` is an immutable `ArrayBuffer`: `immutable` is true, `resizable` is false, and the buffer stays attached. `resize`, `transfer`, `transferToImmutable`, and a write through the view throw `TypeError`. An empty file is a zero-length view.

The `type` attribute is what selects the record. `file.wasm` or `file.json` with `{ type: "text" }` or `{ type: "bytes" }` is that synthetic module. A `.json` file imported without `{ type: "json" }` throws. `{ type: "json" }` on a file whose extension is not `.json` loads that file as JavaScript or TypeScript. An unrecognized `type` on JavaScript or TypeScript, such as `"css"` or `"javascript"`, leaves the module as a script. A `.txt` file needs `{ type: "text" }` or `{ type: "bytes" }`.

The module map key is the resolved URL together with the module type. JavaScript, TypeScript, and a Wasm evaluation import share one record. `"text"`, `"bytes"`, and `"json"` are separate records of the same URL. A failed load leaves its key empty, so a later import with the attribute can succeed, and a later import without the attribute fails again. The same URL and type reuse one module record, and a repeated bytes import is the same `Uint8Array`.

The string or `Uint8Array` is created when the module evaluates. `import defer` waits until the deferred namespace is observed. `import.defer(specifier, { with })` takes the same attributes. The only export is `default`, so a named import fails to link. `import source` stays limited to WebAssembly. Dedicated workers use this loader.

`ArrayBuffer.prototype.immutable`, `transferToImmutable`, and `sliceToImmutable` are part of the realm. `immutable` is false on an ordinary buffer. `transferToImmutable` detaches its receiver and returns an immutable buffer. `sliceToImmutable` returns an immutable copy of a byte range and leaves its receiver attached.

### Deferred evaluation

`import defer` fetches and links the module with the rest of the graph. The module body runs when code first observes the deferred namespace. The static form is `import defer * as ns from "./mod.ts"`. `import defer from "./mod.ts"` is an ordinary default import whose local name is `defer`. `import defer { x } from "./mod.ts"` and `import defer name from "./mod.ts"` are syntax errors.

A string key other than `"then"` observes the namespace through `[[Get]]`, `[[GetOwnProperty]]`, `[[HasProperty]]`, `[[DefineOwnProperty]]`, and `[[Delete]]`. `[[OwnPropertyKeys]]` observes it for every key, so `Object.keys`, `Object.getOwnPropertySymbols`, `Object.isFrozen`, and `Object.isSealed` evaluate the module. Reading `"then"` or a symbol, including `Symbol.toStringTag`, leaves the module unevaluated. `ns[Symbol.toStringTag]` is `"Deferred Module"` before and after evaluation. `Object.prototype.toString` reports `[object Deferred Module]`.

When the module does not export `then`, `ns.then` is `undefined`. When it does export `then`, that binding stays on the deferred namespace. Reading it before evaluation throws `ReferenceError` and leaves the module unevaluated. `"then" in ns` is true and leaves the module unevaluated. After another observation evaluates the module, the read returns the export.

A deferred namespace and the ordinary namespace of the same module are different objects. Repeated `import defer` of one module share one deferred namespace. An ordinary import evaluates the module once, and the deferred namespace then reads that same module state.

A module with top-level await, and the modules its evaluation needs, runs while the importer is evaluated. A synchronous module reached only through `import defer` stays unevaluated until observation. `import.defer(specifier)` fulfills with the deferred namespace after asynchronous dependencies settle, and rejects when one of those dependencies throws. A parse error, a link error, or a missing module fails the importer immediately. An evaluation error is thrown by the observation, and a later observation throws that same error.

JSON still requires `with { type: "json" }`. The text is parsed when the deferred namespace is observed. Text and bytes still require `with { type: "text" }` and `with { type: "bytes" }`. The string or `Uint8Array` is created when that namespace is observed. `import.defer(specifier, { with })` accepts the same import attributes as `import()`. Dedicated workers use this same loader.

TypeScript source (`.ts`, `.mts`) is parsed with oxc and passed through oxc's TypeScript transform before V8 compiles the module. The transform erases types, interfaces, type aliases, type-only imports and exports, assertions, `satisfies`, and generics. It also rewrites runtime TypeScript syntax that erasure cannot delete: `enum`, `namespace`, and parameter properties. JavaScript syntax is not downleveled. The JSX transform is off, and `.tsx` fails to load.

A type-only import is gone before V8 resolves specifiers, so its target is not fetched. Import specifiers that remain are left unchanged. oxc does not rewrite `.ts` extensions to `.js` in the output. The host resolver sees the original specifier.

Erasure is cached in memory for the process, keyed by path, modified time, and length. The loader does not write a `.js` file next to the source. oxc's codegen source map is attached to the V8 script origin, so an exception stack uses the TypeScript path and line.

Parse and transform failures reject module evaluation with the source path and span. Dynamic `import()` and workers use this same loader.

Image decoders are pure Rust. TLS uses rustls. HTTP/1.1 and HTTP/2 use hyper. HTTP/3 uses quinn and h3. `https:` offers ALPN `h2` and `http/1.1`. `http:` stays HTTP/1.1. A later request to an `https` origin uses HTTP/3 after an `Alt-Svc` `h3` advertisement. Versions are pinned in `Cargo.lock`. The oxc crates used for resolution and erasure stay on one version. A `v8` upgrade, and an oxc upgrade, are each their own change.

## Console

`console` prints with one inspector. `log`, `info`, `debug`, `warn`, `error`, `trace`, `assert`, `dir`, `dirxml`, `table`, `group`, `groupCollapsed`, and the values after a `timeLog` label use it. `time`, `timeEnd`, `count`, `countReset`, and the `timeLog` label still use `ToString`. A throwing label conversion is rethrown. `console.group()` with no arguments prints `console.group`.

The inspector prints the contents of an object as text. `console` writes that text with no color. It does not call a user inspection hook. A non-enumerable symbol property is omitted, including `Symbol.iterator`, `Symbol.toStringTag`, and `Symbol.species` when they are methods or accessors. An enumerable symbol key is printed. In a value position a symbol description that is not an identifier is quoted, as `Symbol("a b")`. As a property key the same description stays unquoted, as `Symbol(a b)`.

A subclass prints under its own constructor name. The printer reads fields through the ordinary getters, so a subclass of `Array`, `Map`, `Error`, `URL`, `Headers`, or `URLSearchParams` still prints. `class MyArray extends Array` with two elements prints `MyArray(2) [ 1, 2 ]`.

Depth is 4. A plain object nested deeper than that prints as `[Name]`, such as `[Object]`. An empty object or empty array at that depth still prints `{}` or `[]`. A null prototype with no constructor prints `[Object: null prototype]`. `compact` is 3 and `breakLength` is 80. Line breaking uses UTF-16 length. Array columns use display width. An array or typed array shows at most 100 elements and then `... N more items`. A `Map` or `Set` shows at most 100 entries. A string nested inside a value is cut after 10000 UTF-16 code units with `... N more characters`. A string passed directly to `console.log` is written raw, without quotes and without that cut. The REPL quotes a string completion. Nested strings use the first of `"`, `'`, and `` ` `` that the string does not contain. That quote and `\` are escaped. U+0008, U+000C, U+000A, U+000D, U+0009, and U+000B print as `\b`, `\f`, `\n`, `\r`, `\t`, and `\v`. Other controls through U+001F, and U+007F through U+009F, print as `\xHH`. U+2028 is left unchanged.

An own accessor prints `[Getter]`, `[Setter]`, or `[Getter/Setter]` and is not called, including when that accessor shadows a host attribute. A getter that is not an own property runs when that field is requested, which is how `DataView`, `URL`, a host attribute, and a getter on a user class read their values. `class OnlyGet { get a() { return 1 } }` prints `OnlyGet { a: 1 }`. A setter without a getter prints `[Setter]`. When that getter throws, the printer keeps going and prints `[Getter]` or `[Getter/Setter]`. A prototype object does not run its accessors, so `AbortController.prototype` prints `signal` as `[Getter]` and prints under that constructor name. A throwing `Symbol.toStringTag` is omitted. An ordinary object prints its own enumerable properties. Inherited data properties stay hidden.

A host class instance prints the attributes of the nearest built-in interface on its prototype chain. `AbortController` prints `signal`. `AbortSignal` prints `aborted`, `reason`, and `onabort`. `new AbortController()` prints:

```
AbortController {
  signal: AbortSignal { aborted: false, reason: undefined, onabort: null }
}
```

`Event` prints `bubbles`, `cancelable`, `composed`, `currentTarget`, `defaultPrevented`, `eventPhase`, `srcElement`, `target`, `returnValue`, `timeStamp`, and `type`. `CustomEvent` adds `detail`. `MessageEvent` adds `data`, `origin`, `lastEventId`, `source`, and `ports`. `ErrorEvent` adds `message`, `filename`, `lineno`, `colno`, and `error`. `CloseEvent` adds `wasClean`, `code`, and `reason`. `ProgressEvent` adds `lengthComputable`, `loaded`, and `total`. `PromiseRejectionEvent` adds `promise` and `reason`. Getters on a host prototype between the instance and that interface follow those attributes, which is how `PointerEvent` prints `clientX` and `pointerId`. `File` prints `name`, `lastModified`, `size`, and `type`. `Blob` prints `size` and `type`. `Request` prints `bodyUsed`, `headers`, `method`, `redirect`, `url`, `cache`, `duplex`, `signal`, and `body`. `Response` prints `body`, `bodyUsed`, `headers`, `ok`, `redirected`, `status`, `statusText`, `url`, and `type`. `TextEncoder` prints `encoding`. `TextDecoder` prints `encoding`, `fatal`, and `ignoreBOM`. `Performance` prints `timeOrigin`. `ImageData` prints `data`, `width`, `height`, `pixelFormat`, and `colorSpace`. `ReadableStream` and `WritableStream` print `locked`. The same rule lists the attributes of `TextEncoderStream`, `TextDecoderStream`, `TransformStream`, `CompressionStream`, `DecompressionStream`, stream readers, writers, and controllers, `MessageChannel`, `MessagePort`, `FileReader`, `Worker`, `WebSocket`, `WebSocketStream`, `EventSource`, `DOMPoint`, `DOMRect`, `DOMQuad`, and `DOMMatrix`. An attribute this runtime does not implement is omitted. A known interface omits every other own property, so an own field on an `AbortController` stays hidden, and a data field added by a subclass stays hidden. A getter on a subclass prototype is printed after those attributes. The subclass still prints under its own constructor name. A known interface still prints its attributes when a plain object at that depth would print `[Name]`. Aborting a controller with itself uses the circular marker. A host class outside that list, such as `Yuzuko.Window`, prints the getters on its host prototypes, parent before child, and then its own enumerable properties. `EventTarget` has no attributes, so an own property prints, as `EventTarget { a: 1 }`. Methods stay hidden. Non-enumerable names stay hidden except `cause` on an error and `errors` on an `AggregateError` when that value is an array. Those two are wrapped in brackets when they are not enumerable, as `[cause]:` and `[errors]:`.

The first repeated object is `<ref *1>` where it is expanded and `[Circular *1]` where it appears again. Later identities continue from that count. A revoked proxy throws `TypeError` with the message `Cannot inspect a proxy that has been revoked.` A live proxy prints its target.

The first argument is a format string only when it is a string primitive and another argument follows. `%s` is `ToString` and rethrows. `%d` and `%i` are `ToNumber` truncated toward zero, so `{ valueOf() { return 3.9 } }` prints `3` and `"10px"` prints `NaN`. A symbol prints `NaN` for `%d`, `%i`, and `%f`. `%f` is `ToNumber`. `%o` and `%O` inspect. `%j` is `JSON.stringify`. A circular structure prints `[Circular]`. `undefined`, a function, or a symbol leaves the characters `%j` and still consumes the argument. `%c` consumes its argument and prints nothing. `%%` prints `%`. A trailing `%` stays. An unknown specifier such as `%q` stays and does not consume an argument. A specifier with no argument left stays. Further arguments are separated by a space.

`URL` prints `href`, `origin`, `protocol`, `username`, `password`, `host`, `hostname`, `port`, `pathname`, `hash`, and `search`, including empty strings. `searchParams` and other own properties are omitted. `Headers` prints entries sorted by UTF-16 code units, and duplicate names are joined with `, `. `URLSearchParams` prints pairs in insertion order as `"name" => "value"`. That text stays on one line when its UTF-16 length is at most 80, so `URLSearchParams { "a" => "1", "b" => "2" }` is one line. When it wraps, the closing `}` stays on the last pair's line. A subclass of any of the three uses the subclass constructor name.

A `BigInt` prints with an `n` suffix. `-0` prints as `-0`. An array puts spaces inside the brackets, as `[ 1, 2 ]`. A hole prints `<1 empty item>` or `<N empty items>`. Integer indexes come first, in numeric order. Other keys keep insertion order. An error prints its stack, then any extra own properties. A `Promise`, `Map`, `Set`, typed array, `ArrayBuffer`, `DataView`, `Date`, `RegExp`, and function print in the shapes covered by the console tests. `WeakMap` and `WeakSet` print `<items unknown>`.

A `Temporal.Instant`, `Temporal.ZonedDateTime`, `Temporal.PlainDate`, `Temporal.PlainTime`, `Temporal.PlainDateTime`, `Temporal.PlainYearMonth`, `Temporal.PlainMonthDay`, or `Temporal.Duration` prints as `ToString`, including a subclass. `Temporal.PlainDate.from("2020-01-02")` prints `2020-01-02`. `new Temporal.Duration()` prints `PT0S`. `Temporal.Duration.from({ hours: -1, minutes: -2 })` prints `-PT1H2M`. Own properties stay hidden. The same value printed twice stays as two copies. `ToString` runs, so a replaced `toString` supplies the text. A non-primitive result continues to `valueOf`. A throw from that conversion is rethrown. `Temporal`, `Temporal.Now`, a Temporal prototype, and an object that only sets `Symbol.toStringTag` print as ordinary objects. An instance whose prototype is null prints with that constructor name and an empty body, as `[PlainDate: null prototype] {}`. A Temporal value nested past the depth limit still prints its `ToString` text.

## CLI

```text
yuzuko run <entry.js|entry.ts> [arg...]
yuzuko test [<path>...] [--filter <substring>]
yuzuko package <entry.js|entry.ts> [--config <file>] [--output <path>]
yuzuko repl
```

`yuzuko` with no command starts the same session as `yuzuko repl`. `console.log` prints. `Yuzuko` is present. `Yuzuko.test` is absent. A session that never constructs a window opens none. `yuzuko help`, `yuzuko --help`, and `yuzuko -h` print usage and do not start a session.

`yuzuko run <entry> [arg...]` evaluates the entry module. Tokens after the entry become `Yuzuko.args`, including a token that starts with `-`. A token that starts with `-` before the entry is an unknown flag and the process exits 2. `--` before the entry consumes that separator and takes the next token as the entry, including an entry whose name starts with `-`. One `--` placed immediately after the entry is discarded and is not an argument. `yuzuko run game.ts --fullscreen` records `["--fullscreen"]`. `yuzuko run game.ts -- --fullscreen` records the same array. `yuzuko run game.ts -- --` records `["--"]`. `yuzuko run -- script.ts -- x` records `["x"]`. `yuzuko run` with no entry, including `yuzuko run --`, exits 2.

The prompt is `> `. Each complete chunk is evaluated, its completion value is printed with the console inspector, and queued microtasks and already-due macrotasks run before the next prompt. A string completion is quoted. A timer that schedules itself again with delay 0 runs once for that chunk and does not keep the prompt waiting. `1 + 1` prints `2`. A TypeScript annotation is erased before evaluation, so `const labeled: number = 3` then `labeled` prints `3`. Top-level `await` prints the resolved value. `await import()` of a relative `.ts` file loads it the same way `yuzuko run` loads a module, from the current directory. A thrown exception is reported and the next chunk still runs. EOF on an empty line exits 0. `Yuzuko.exit` ends the session with that status. The completion value and an exception stack are not printed. A timer can call it while the prompt is waiting, except during a Windows pipe read, which blocks until the next line. See Process.

A completion is colored when stdout is a terminal. A non-empty `FORCE_COLOR` colors it when stdout is not a terminal. A non-empty `NO_COLOR` disables color when `FORCE_COLOR` is empty or absent. `console` prints uncolored text. A number, a bigint, and a boolean are yellow. `undefined` and an empty array hole are grey. `null` is bold. A string and a symbol are green. A date is magenta. A Temporal object is cyan. A regular expression is red. A function, `<ref *1>`, `[Circular *1]`, `[Getter]`, `[Setter]`, `[Getter/Setter]`, a collapsed object such as `[Object]`, `<pending>`, `<rejected>`, `<items unknown>`, `(detached)`, and `[Uint8Contents]` are cyan. An identifier property name has no color. A syntax error and a thrown exception keep their text and are red.

An unclosed `(`, `{`, `[`, string, template literal, or block comment keeps the continuation prompt `... ` and is not evaluated. A trailing `.` that continues a member access does the same. A finished invalid chunk reports a syntax error, and the next chunk still runs.

On a terminal, the line editor uses the Node.js readline emacs bindings. Left and Right, or Ctrl-B and Ctrl-F, move one character. Up and Down, or Ctrl-P and Ctrl-N, recall chunks submitted in this session. Ctrl-A and Home move to the start of the current line. Ctrl-E and End move to the end of that line. Ctrl-Left and Escape then B move one word left. Ctrl-Right and Escape then F move one word right. Backspace deletes the character on the left. Ctrl-D and Delete delete the character on the right, and Ctrl-D on an empty buffer ends the session. Ctrl-U deletes back to the start of the line. Ctrl-K deletes through the end of the line. Ctrl-W deletes the word on the left. Escape then D deletes the word on the right. Ctrl-Y inserts the text last removed by Ctrl-U or Ctrl-K. Ctrl-L clears the screen and redraws the buffer. History ends when the process exits. Tab completes names from this realm. `Ma` offers `Math`. `Math.` offers properties including `abs`. A binding created by an earlier chunk in the same session is offered. A name that does not exist is not invented. A pipe reads complete lines and treats EOF as the end of the session. While a Unix terminal or a Windows console waits for the next key, the event loop still runs. Timers, host tasks, and open windows keep receiving turns, so `requestAnimationFrame`, input, and drawing continue, and a key still interrupts that wait. A host job that has already finished runs before the editor blocks, and a job that finishes during the wait interrupts it. The Windows console runs with virtual terminal input, so the keys above arrive as the same sequences a Unix terminal sends. The same switch colors the input line. The cursor column counts the input characters and ignores color codes. A string or template is green. A number, a bigint, `true`, `false`, and `null` are yellow. A keyword is cyan. `undefined` and a comment are grey. `Infinity` and `NaN` are yellow. An identifier before `(` is intense blue. A regular expression is red. `from` is cyan only when a string literal follows it. A Unix pipe also runs the loop while it waits for the next line. A Windows pipe blocks until the next line arrives. REPL chunks are not ES modules. See `docs/deviations.md`.

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

The runner evaluates one file at a time in one isolate, in registration order. A thrown exception or a rejected promise fails that test. Ignored tests are reported and do not fail the run. If any test sets `only`, the run executes only those tests and then exits 1, so a focused test cannot stay green. The summary line is `ok | <passed> passed | <failed> failed | <ignored> ignored`. Exit status is 0 when every executed test passes and none set `only`. It is 1 when a test fails or any test set `only`. `Yuzuko.exit` during evaluation or from a test function stops the run with that status, and the summary line is not printed.

There is no op or resource sanitizer. Those track a host-call layer this runtime does not have. The runner does fail a test that leaves a timer it started, or that produces an unhandled rejection after the test function settles.

`Yuzuko.build.os` is `"darwin"`, `"linux"`, or `"windows"`, and it is part of the test namespace, installed with `Yuzuko.test`.

`Yuzuko.__test` injects pointer, key, IME, wheel, focus, menu, tray, and gamepad input, and, when `accessibility` is enabled, assistive-technology actions. `yuzuko test` installs it only when `YUZUKO_TEST_INPUT=1`. Without that variable the property is absent. `yuzuko run` and `yuzuko repl` do not install it.

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

The enrolled upstream directories are `url`, `encoding`, `hr-time`, `html/webappapis/atob`, `html/webappapis/timers`, `FileAPI`, `dom/events`, `wasm/jsapi`, `console`, constructor tests under `pointerevents` and `uievents` that do not need an element, `html/canvas` image and `createImageBitmap` tests that operate on blobs (plus the image fixtures those tests use), `html/canvas/offscreen` and the 2D context tests that do not need a displayed HTML canvas (run on the CPU renderer), `webgpu`, `fetch`, `streams`, `compression`, `urlpattern`, `WebCryptoAPI`, `mimesniff`, `wasm/webapi`, `css/geometry`, `gamepad`, `clipboard-apis`, the `DataTransfer` string-store tests under `html/editing/dnd/datastore`, `screen-wake-lock`, `css/css-font-loading`, dedicated-worker tests that do not need a document, and `webaudio`.

`requestidlecallback` is enrolled with the main-global idle callbacks. `eventsource` and `websockets` are enrolled with those clients. `webcodecs` is enrolled with `VideoDecoder`, `AudioDecoder`, `VideoFrame`, `AudioData`, and `ImageDecoder`. The JPEG XL `ImageDecoder` documents are enrolled with that decoder. `dom/abort` is enrolled with `AbortSignal`. `html/webappapis/microtask-queuing` and `html/webappapis/structured-clone` are enrolled with `queueMicrotask` and `structuredClone`. `imagebitmap-renderingcontext` is enrolled with `ImageBitmapRenderingContext`. Files that need a document, a frame, or `requestAnimationFrame` on `globalThis` stay ignored or fail as recorded.

Pixel comparisons for vello output use the CPU renderer. Unorm8 sRGB pixels that did not cross a color-space boundary match exactly. A conversion from that output into another color space may differ by one unorm8 code point. `putImageData` and `ImageBitmap` float16 samples may differ by one float16 ulp. Where WPT has no reference, a `Yuzuko.test` golden image covers the case. The GPU renderer has a smoke test that one frame reaches a surface. Test both `YUZUKO_CANVAS=cpu` and the `"cpu"` fallback when the GPU is unavailable. WebGPU tests may skip when `YUZUKO_SKIP_GPU=1` and no GPU is available. A skip is not a failure.

Event order, the key map, and the IME state machine are `Yuzuko.test` tests. Decoders also have golden files, corrupt input, and limit checks beside the WPT cases.

## Packaging

`yuzuko package` produces an application that starts from a double click. The bundle contains the `yuzuko` executable that ran the command, the entry module graph, and the assets listed in the config. That executable keeps the Cargo features it was built with. `yuzuko package` does not rebuild it. A packager built with `--no-default-features` is the JavaScript runtime from Build features. `--features` selects the subset. The default packager includes every feature. The targets are the five triples in Platforms. The macOS output is a `.app`. Windows and Linux outputs are directories. There is no WebView and no `backend` choice. The `Contents/MacOS` binary and opening the `.app` both run the packaged entry. A bundled executable always runs that entry. Arguments other than a `-psn_` token become `Yuzuko.args`, and the executable does not dispatch `run`, `test`, `package`, or `repl`. `codesign --verify` succeeds for an ad-hoc signature. `import.meta.url` is a `file:` URL under `Contents/Resources/app/`. A listed asset outside the entry directory is fetched from that tree, and a `file:` URL outside it fails. An omitted window title is the config `name`. The launch directory stays the caller's current directory. Windows and Linux directories from the same packager store the AppUserModelID, the `.desktop` id, and the icon bytes. The Windows executable in a bundle is marked as a GUI-subsystem image, so a double click opens no console window. Standard handles a parent passes in still receive output. A Windows bundle sets that AppUserModelID for the process before its first window and uses `AppIcon.ico` for the window and taskbar icon. A Linux bundle sets the X11 `WM_CLASS` and the Wayland `app_id` to the identifier, uses the widest PNG under `icons/` as the window icon, and its `.desktop` entry names the identifier as `StartupWMClass`.

The module graph follows static imports and exports, including `import defer` and `import source`. Type-only imports stay unloaded. `import()`, `import.defer()`, and `import.source()` are followed when the argument reduces to one specifier and that specifier resolves to a file. The specifier may be a string literal, a template literal with no substitution, or those strings joined by `+`. It may also be `import.meta.resolve` of such a specifier. A second argument is part of that form only when it is `import.meta.url`. It may also be `new URL(specifier, import.meta.url)`, or that expression's `.href` or `.toString()`. Parentheses and TypeScript `as`, `satisfies`, angle assertions, and non-null assertions wrap the same forms. A dynamic specifier that fails to resolve stays out of the bundle, so a missing file, a `node:` specifier, or a remote URL does not fail `yuzuko package`. A computed argument, a template substitution, a binding, and a `Worker` URL stay out. `.wasm` and `.json` files in the graph are copied as bytes. Imports written inside those files are followed only from a parsed JavaScript or TypeScript module. A static import, a re-export, or a concrete dynamic import whose options object literal says `type: "text"` or `type: "bytes"` copies that file and leaves its source unparsed. Another edge that imports the same file as JavaScript or TypeScript parses it.

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
| `icons.windows` | An `.ico` path. PNGs are not assembled. It becomes the executable's icon resource, which Explorer, taskbar pins, and shortcuts show, and is copied as `AppIcon.ico` for the window icon. A file that is not a valid icon directory fails `yuzuko package` |
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
| `yuzuko` | CLI. `run`, `test`, `package`, and `repl`. Calls each enabled extension's `install`. Links a feature crate only when that Cargo feature is on |
| `yuzuko_core` | Isolate, platform, event loop, module loader (`oxc_resolver`, oxc type erasure), extension trait, timers, console, exception conversion. The `fs` feature compiles the `Yuzuko` file system. The `udp` feature compiles `Yuzuko.listenDatagram`. The `tcp` feature compiles `Yuzuko.listenTcp` and `Yuzuko.connectTcp`. The `http` feature compiles `Yuzuko.listenHttp`. The `signal` feature compiles `Yuzuko.addSignalListener`. The `child` feature compiles `Yuzuko.spawn`. The `ffi` feature compiles `Yuzuko.dlopen` |
| `yuzuko_dom` | `EventTarget`, event classes, and `AbortController`. The `window` feature installs pointer and key events. The `fetch` feature installs `EventSource`. The `socket` feature installs `WebSocket`, `WebSocketStream`, and `WebTransport` |
| `yuzuko_winter` | URL, encoding, Blob, crypto, streams. The `fetch` feature compiles the HTTP client. The `socket` feature compiles `WebSocket` and `WebTransport` |
| `yuzuko_window` | `Yuzuko.Window`, winit, input mapping, IME, pointer lock, keyboard lock, fullscreen. Compiled with the `window` feature. The `accessibility` feature compiles virtual and computed accessibility nodes and links AccessKit |
| `yuzuko_image` | Decoders and `ImageBitmap` / `createImageBitmap`. Compiled with the `image` feature |
| `yuzuko_canvas` | Canvas 2D and `bitmaprenderer`. Compiled with the `canvas` feature. `OffscreenCanvas` itself is installed by `yuzuko_window` |
| `yuzuko_webgpu` | WebGPU. Compiled with the `webgpu` feature |
| `yuzuko_audio` | Web Audio. Compiled with the `audio` feature |
| `yuzuko_codec` | WebCodecs and `Yuzuko.demuxMp4`. Compiled with the `codec` feature |

`yuzuko_core` does not depend on winit or wgpu. Feature crates depend on core, and the binary assembles the feature crates.

Host objects are allocated with cppgc so V8 owns their lifetime. GPU resources that have `destroy()` are destroyed explicitly and do not wait for GC.

## Risks

- A level A path or text operation that vello cannot draw throws `NotSupportedError` on both the GPU and CPU renderers. Do not switch that operation to a second 2D library. Vello's render target stays sRGB unorm8. A `display-p3` or `float16` backing store is not a vello target.
- winit IME differs by OS by more than the specification describes. Split event-sequence tests by OS.
- The WebGPU surface is the list in this file. Behavior outside that list is not implied.
