# Supported APIs

Yuzuko is a desktop JavaScript and TypeScript runtime built on rusty_v8. A script draws its own pixels and presents them in native windows. There is no WebView and no HTML document.

Web-standard and WinterTC APIs are properties of `globalThis`. APIs that belong to this runtime are properties of `Yuzuko`. `Yuzuko` is present on the main realm and in a dedicated worker in every build. There is no global `Window` constructor.

The default build enables every feature on this page. `cargo build -p yuzuko --no-default-features` is the JavaScript runtime alone. `yuzuko package` copies the executable that invoked it, so a packaged application keeps that build's features. Supported targets are macOS arm64 and Intel, Windows x86_64, and Linux arm64 and x86_64.

This page introduces the surfaces. Normative behavior is [spec.md](spec.md). Intentional divergences from the web platform are [deviations.md](deviations.md). Declarations for the surface are [`yuzuko.d.ts`](../yuzuko.d.ts).

## Where a name lives

`globalThis` is the execution realm. It inherits from `EventTarget`. `self` is that same object. `onerror` and `onunhandledrejection` are delivered there. Pointer and key events go to the window that received them.

A dedicated worker runs on its own thread, in its own isolate. It receives the JavaScript runtime and the feature surfaces that are not tied to an OS window: fetch, sockets, files, datagrams, TCP, the HTTP server, signals, child processes, foreign functions, Web Audio, WebCodecs, images, Canvas 2D on an `OffscreenCanvas`, fonts, and WebGPU. It has `requestAnimationFrame` and `cancelAnimationFrame`. It does not have `requestIdleCallback`, `Yuzuko.Window`, the application menu, tray, dock, dialog labels, clipboard, gamepad, screen Wake Lock, or the accessibility nodes.

An `AudioWorklet` processor realm has `Intl`, `AudioWorkletProcessor`, and the processor `port`. `Yuzuko` and the feature surfaces are not installed there.

A name from a feature that was not enabled is absent. An unimplemented method throws `NotSupportedError`.

`image`, `canvas`, `webgpu`, `codec`, and `accessibility` enable `window`. `fetch` and `socket` are independent of each other. The `http` server enables neither `fetch` nor `tcp`. `audio`, `fs`, `udp`, `tcp`, `http`, `signal`, `child`, and `ffi` do not enable `window`.

## JavaScript runtime

These names are in every build.

Events and tasks: `Event`, `EventTarget`, `CustomEvent`, `ErrorEvent`, `PromiseRejectionEvent`, `DOMException`, `AbortController`, `AbortSignal`, `queueMicrotask`, `reportError`, and `structuredClone`.

Time and text: `setTimeout`, `clearTimeout`, `setInterval`, `clearInterval`, `performance`, `console`, `atob`, `btoa`, `TextEncoder`, `TextDecoder`, `TextEncoderStream`, `TextDecoderStream`, `URL`, `URLSearchParams`, and `URLPattern`.

Bytes: `Blob`, `File`, `FormData`, streams, `CompressionStream`, and `DecompressionStream`. `crypto`, `CryptoKey`, and `SubtleCrypto` provide digests, signatures, encryption, key derivation, and the additional algorithms recorded in [deviations.md](deviations.md).

`MessageChannel` and dedicated workers are in every build. The main global has `postMessage`, which posts to that same realm. A dedicated worker's `postMessage` posts to its owner. The main realm has `requestIdleCallback` and `cancelIdleCallback`. The main global has no `requestAnimationFrame`. A frame belongs to a window, or to a dedicated worker that draws into a transferred canvas.

`Intl` uses the full ICU locale data linked into this V8 build. `WebAssembly` includes streaming compilation, a direct `.wasm` import, `import source`, and `import defer`. `import source` of JavaScript or TypeScript yields a `ModuleSource` for `new Worker`. Text and bytes imports use `with { type: "text" }` and `with { type: "bytes" }`.

`navigator.userAgent` is `Yuzuko/<version>`. `navigator.hardwareConcurrency` is the number of logical processors available to the process, and it is at least 1.

The application entry is an ES module. TypeScript is erased with oxc before V8 parses it. The runtime does not typecheck. Classic scripts run in a dedicated worker and in the REPL. The Annex B `Object.prototype.__proto__` accessor is removed before user code runs. An object literal `{ __proto__: null }` still sets the prototype.

## Process

`Yuzuko.args`, `Yuzuko.env`, `Yuzuko.cwd`, `Yuzuko.chdir`, `Yuzuko.exit`, `Yuzuko.execPath`, `Yuzuko.pid`, `Yuzuko.ppid`, and `Yuzuko.version` are in every build, on the main realm and in a dedicated worker. `Yuzuko.errors` is too.

`args` is the argument vector recorded at launch. `env` reads and writes the process environment. `cwd` and `chdir` are the process current directory. `exit(code)` ends the process with a status from 0 through 255. `execPath()` is the absolute path of this executable. `pid` and `ppid` are this process and its parent. `version` is `{ yuzuko, v8 }`. Which optional surfaces exist is chosen when the runtime is built.

`Yuzuko.permissions` is in every build. The names are `read`, `write`, `net`, `env`, `run`, and `ffi`. `yuzuko run` asks on a terminal when a grant is missing. `yuzuko repl` and `yuzuko test` install a sandbox and do not ask. `permissions` in `yuzuko.json` supplies the grants for `yuzuko run` and for a packaged application. A packaged application that includes the object does not ask, and an omitted name is denied. A packaged application that omits the key allows every check. A denied call throws `Yuzuko.errors.NotCapable`.

## Windows and input

Feature: `window`.

`new Yuzuko.Window()` opens one native window. Startup does not open a window on its own. The window is an `EventTarget`. Drawing happens on `window.surface`, a `Yuzuko.WindowSurface`, which has `getContext`, `convertToBlob`, and `transferToImageBitmap`. The surface is not an `OffscreenCanvas` and cannot be transferred. `transferControlToOffscreen()` returns the `OffscreenCanvas` a worker draws. That window presents the copy on a later turn of the main event loop.

`getContext` returns a context for `"2d"`, `"webgpu"`, and `"bitmaprenderer"` when the matching drawing feature is enabled. `"webgl"` and `"webgl2"` return `null`.

Pointer events target the surface and propagate through the window. `pointerrawupdate` reports each move from the operating system, including while the pointer is locked. Keyboard, IME composition, wheel, focus, blur, resize, and close target the window. IME stays off until `setImeAllowed(true)`. Focus and blur are `FocusEvent`s. `requestPointerLock` and `requestFullscreen` belong to the surface. `matchMedia` follows that window's CSS size, `devicePixelRatio`, and a Media Queries Level 4 subset. `navigator.keyboard.lock` on the main global reserves keys for the focused window.

`requestAnimationFrame` on a window runs at the refresh rate of the monitor that window is on. A Canvas 2D or `bitmaprenderer` change is also presented on the next event-loop turn. `surface.visibilityState` is `"visible"` or `"hidden"`, and `visibilitychange` fires on the surface.

The window can move, resize, show, hide, and stay above other windows. `alert`, `confirm`, `prompt`, and `pickFiles` are modal dialogs for that window. `Yuzuko.setDialogLabels`, on the main realm, replaces the button labels the runtime draws. `filehover`, `filehovercancel`, and `filedrop` carry paths dropped on the window. Wayland does not report those drops.

`Yuzuko.Menu` and `Yuzuko.setMenu` install one application menu. A selection dispatches `menuclick`. On Linux, winit exposes no GTK window, so `setMenu` throws `NotSupportedError`. `setContextMenu` stores the menu a context click shows. `showContextMenu` opens a menu at a client point.

On the main global, `navigator.getGamepads()` reports connected gamepads. `playEffect("dual-rumble")` runs when that device can play force feedback. `navigator.clipboard` reads and writes `text/plain` and `image/png` after a recent pointer or key press on the focused window. `navigator.wakeLock.request("screen")` asks the operating system to keep the display awake.

`Yuzuko.Tray` is a status item. `Yuzuko.dock` sets the badge, Dock visibility, Dock menu, and attention request. Tooltip, template icons, and several Dock calls throw `NotSupportedError` on the platforms named in the spec.

`YUZUKO_HEADLESS=1`, and `yuzuko test`, skip OS window creation. The drawing buffer and animation frames still run.

## Images

Feature: `image`. It enables `window`.

`createImageBitmap`, `ImageBitmap`, and `ImageData` decode and store pixels. A `Blob` is recognized by its signature: JPEG, PNG, WebP, JPEG XL, or AVIF. AVIF is one still frame at profiles 0–2 and 8, 10, and 12 bits, stored as unorm8 sRGB. The stored color space is `"srgb"` or `"display-p3"`. Samples are unorm8 or float16. Decoding runs off the event loop. `createImageBitmap` uses the first frame of an animation.

## Canvas 2D and fonts

Feature: `canvas`. It enables `image` and `window`.

The GPU rasterizer is vello_gpu and the CPU rasterizer is vello_cpu. `Yuzuko.canvasRenderer` is `"gpu"` or `"cpu"`, chosen once at process startup. When the GPU cannot produce a device, or vello_gpu cannot be created on that device, the CPU renderer produces the pixels and they are still presented. `YUZUKO_CANVAS=cpu` or `YUZUKO_CANVAS=gpu` overrides the choice. `YUZUKO_SKIP_GPU=1` selects `"cpu"`.

The context draws paths, text, gradients, patterns, images, layers, filters, and shadows, and it stores the compositing operators in the spec. `lang` selects localized glyphs. `convertToBlob` encodes PNG, JPEG, and WebP. A `display-p3` or `float16` backing store keeps matching `putImageData` and `drawImage` samples. Path, text, and gradient paints are rasterized in sRGB unorm8.

`FontFace` is on `globalThis`. On the main global the set is `Yuzuko.fonts`, and `globalThis.fonts` is absent. A dedicated worker exposes its own set as `self.fonts` and `globalThis.fonts`. A source is font bytes, one URL, or a CSS `src` list of `url()` and `local()`. A URL is loaded with `fetch`. The formats are TTF, OTF, WOFF, and WOFF2. `queryLocalFonts` and `FontData` list installed faces. `sans-serif`, `serif`, and `monospace` draw through the operating-system UI fonts.

## WebGPU

Feature: `webgpu`. It enables `image` and `window`.

`navigator.gpu` is a handwritten WebGPU subset on wgpu. The process has one device, which Canvas 2D shares when it renders on the GPU. Each window has its own swapchain. The subset covers adapters and devices, canvas configuration, shaders, render and compute passes, render bundles, buffers, textures, samplers, query sets, and `copyExternalImageToTexture` from an `ImageBitmap`, an `OffscreenCanvas`, or a `VideoFrame`. A method outside that subset throws `NotSupportedError`.

`YUZUKO_SKIP_GPU=1` makes `requestAdapter()` resolve `null`. A WebGPU device stays on the thread that created it.

## Web Audio

Feature: `audio`.

`AudioContext` and `OfflineAudioContext` are on `globalThis`. `decodeAudioData` decodes WAV, FLAC, MP3, Ogg Vorbis, and Ogg Opus in Rust. The graph includes buffer, oscillator, and constant sources, gain, delay, biquad and IIR filters, a wave shaper, stereo and spatial panners, a convolver, a compressor, an analyser, channel split and merge, and `AudioWorklet`. `Yuzuko.audioOutputDevices()` lists the host output names that `sinkId` accepts. `AudioWorklet.addModule` loads the processor with `fetch`.

## Pictures and video

Feature: `codec`. It enables `image` and `window`.

`VideoDecoder`, `EncodedVideoChunk`, `VideoFrame`, `AudioDecoder`, `EncodedAudioChunk`, `AudioData`, and `ImageDecoder` are on `globalThis`. Video decoding is hardware H.264, `avc1` and `avc3`, plus AV1, `av01`, at profiles 0–2 and 8, 10, and 12 bits. H.264 uses VideoToolbox on Darwin, Direct3D 11 on Windows, and VA-API on Linux. AV1 uses VideoToolbox on Darwin and the Media Foundation transform on Windows when that hardware decoder accepts the configuration, and rav1d otherwise. Linux AV1 uses VA-API when `VAProfileAV1Profile0` is present, and rav1d otherwise. `prefer-software` selects rav1d. Audio decoding for this API is AAC, `mp4a`, Opus, `opus`, and Vorbis, `vorbis`. An unsupported codec string closes the decoder and reports `NotSupportedError` to `error`. `ImageDecoder` returns every frame of an animated PNG, WebP, JPEG XL, or GIF, decodes BMP as one still frame, decodes AVIF as one still frame at profiles 0–2 and 8, 10, and 12 bits, and does not advance frames. A major brand of `avis` is rejected.

`Yuzuko.demuxMp4` reads a progressive MP4 file into those H.264, AV1, and AAC chunks. It does not decode or present them. A `VideoFrame` can be drawn with `drawImage`, `createImageBitmap`, or `copyExternalImageToTexture`.

## HTTP client

Feature: `fetch`.

`fetch`, `Headers`, `Request`, `Response`, `caches`, `XMLHttpRequest`, and `EventSource` load `file:`, `blob:`, `data:`, `http:`, and `https:`. A `file:` URL stays under the entry module's directory. `https:` negotiates HTTP/2 and uses HTTP/3 on a later request to that origin after an `Alt-Svc` `h3` advertisement whose host and port are granted. `http:` stays HTTP/1.1. The client follows the unrestricted WinterTC fetch posture in the spec. `EventSource` reads an `http:` or `https:` event stream.

## Sockets

Feature: `socket`.

`WebSocket` and `WebSocketStream` open `ws:` and `wss:`. The constructor accepts `targetAddressSpace` of `loopback`, `local`, or `public`, and the connected address has to fall in that space. `WebTransport` opens an `https` session on its own QUIC connection, with datagrams and unidirectional and bidirectional streams. `WebSocketStream` exposes the socket as streams. Binary `WebSocketStream` chunks are `Uint8Array` values.

## Files

Feature: `fs`.

File-system methods live on `Yuzuko` in the main realm and in a dedicated worker: `open`, `create`, `readFile`, `readTextFile`, `writeFile`, `writeTextFile`, `mkdir`, `remove`, `rename`, `stat`, `lstat`, `readDir`, `copyFile`, `realPath`, `truncate`, `makeTempDir`, `makeTempFile`, `chmod`, `chown`, `link`, `symlink`, `readLink`, `utime`, and `watchFs`. Each of those except `watchFs` has a synchronous twin. `umask` is synchronous. `Yuzuko.FsFile` is the open descriptor, with reads, writes, seeks, locks, terminal mode, and `readable` and `writable` streams. `Yuzuko.FsWatcher` reports directory changes. Failures throw the classes on `Yuzuko.errors`.

A relative path is joined to the process current directory before the permission check, and that absolute path is opened. Each call checks the read or write permission before the operating-system call. A packaged application can read its bundle and cannot modify it. `file:` fetch stays inside the entry directory and does not use this feature.

## Datagrams

Feature: `udp`.

`Yuzuko.listenDatagram` binds a UDP socket and returns a `Yuzuko.Datagram`. `send` and `receive` each move one datagram. `connect` stores a default peer, `setBroadcast` sets `SO_BROADCAST`, and `joinMulticast` and `leaveMulticast` join or leave a group. `reusePort: true` sets `SO_REUSEPORT` before the bind on macOS and Linux. The bind hostname is an IP address or `localhost`. A DNS name is rejected.

## TCP

Feature: `tcp`.

`Yuzuko.listenTcp` returns a `Yuzuko.TcpListener`. `accept` resolves a `Yuzuko.TcpConn`. `Yuzuko.connectTcp` opens a connection. `read` and `write` copy bytes. `reusePort: true` sets `SO_REUSEPORT` before the bind on macOS and Linux. The hostname is an IP address or `localhost`. A DNS name is rejected. The socket does not speak TLS.

## HTTP server

Feature: `http`.

`Yuzuko.listenHttp` returns a `Yuzuko.HttpServer` for HTTP/1.0 and HTTP/1.1. `accept` resolves a `Yuzuko.HttpRequest` with `method`, `url`, and `body`. `respond` writes one HTTP/1.1 response, including caller headers, and closes that connection. `reusePort: true` sets `SO_REUSEPORT` before the bind on macOS and Linux. This feature does not install the HTTP client or the TCP types.

## Signals

Feature: `signal`.

`Yuzuko.addSignalListener` and `Yuzuko.removeSignalListener` catch the Unix signals in the spec, including `SIGINT` and `SIGTERM`. The first listener replaces that signal's default terminate action. On Windows only `SIGINT` (Ctrl+C) and `SIGTERM` (Ctrl+Break) can be caught. A listener does not keep the process alive.

## Child processes

Feature: `child`.

`Yuzuko.spawn` starts a program without a shell and returns a `Yuzuko.Child`. Standard input defaults to closed. Standard output and standard error default to collected output, each up to 1048576 bytes. `"piped"` exposes those streams as web streams. `clearEnv` starts from an empty environment. `uid` and `gid` are Unix ids. An `AbortSignal` can abort the child. `output`, `status`, and `kill` observe the process. On Unix, `kill()` sends `SIGKILL` and `kill(signal)` can send another signal. On Windows, `kill()` terminates the process.

## Foreign functions

Feature: `ffi`.

`Yuzuko.dlopen` loads a shared library and returns a `Yuzuko.Library`. Each symbol is one `extern "C"` function. Parameters are `"i32"`, `"f64"`, `"pointer"`, `"buffer"`, or `"cstring"`, in the combinations the spec allows. The loader is `dlopen` on Unix. On Windows a bare name is `LoadLibraryW`, and a path is `LoadLibraryExW` of the absolute file. A name with no path separator is allowed only when `ffi` is granted as a whole. A buffer or C-string pointer is valid only for that call.

## Accessibility

Feature: `accessibility`. It enables `window`. The names are installed on the main realm only.

`Yuzuko.AccessibleNode` is a virtual accessibility node. `surface.attachAccessibleRoot()` attaches one tree to that window, and `appendChild` builds it. Assistive-technology actions arrive as events on the node. The published tree is an AccessKit tree: `accesskit_macos` on macOS, UI Automation through `accesskit_windows` on Windows, and AT-SPI through `accesskit_unix` on Linux. `window.getComputedAccessibleNode` reads one published node as a `Yuzuko.ComputedAccessibleNode`.

## Packaging and updates

`yuzuko package` reads `yuzuko.json` or `yuzuko.jsonc` and writes an application that starts from a double click. macOS output is an ad-hoc-signed `.app`. Windows and Linux outputs are directories that carry an application id and an icon. The bundle contains the `yuzuko` executable that ran the command, the module graph, and the assets listed in the config.

`Yuzuko.appVersion` is the packaged manifest version, or `null` during `yuzuko run`, `yuzuko repl`, and `yuzuko test`. `Yuzuko.autoUpdate({ url, publicKey })` reads `{url}/feed.json` and stages a signed newer payload. `url` is an absolute `http:` or `https:` URL, or a `URL`. `publicKey` is standard base64 of the Ed25519 public key. Both names are on the main global and absent in a dedicated worker. The next launch verifies the stage with the key stored beside it, replaces the `app/` tree, and leaves the executable and icons in place. A later version of the application can pass a different key and a different URL.

## Commands

`yuzuko run` evaluates an ES module. Permission flags before the entry grant or deny host access, and `--no-prompt` skips the question. `yuzuko run --watch` restarts that module in a new realm when a loaded file changes. `yuzuko run --watch-hmr` re-evaluates a changed module and the modules that statically import it, in the same realm. A `Yuzuko.Window` or playing `AudioContext` created by a module that is not re-evaluated stays. A window or context created by a re-evaluated module is closed when the new evaluation does not keep it. `yuzuko test` runs `Yuzuko.test` and installs that function only for the test process. `Yuzuko.build` reports this runtime's target triple on the main realm and in a dedicated worker. `os` is `darwin`, `linux`, or `windows`. `standalone` is `true` for a packaged application and `false` for `yuzuko run`, `yuzuko repl`, and `yuzuko test`. `yuzuko repl`, and `yuzuko` with no command, evaluate classic-script chunks from the terminal. `yuzuko package` builds the bundle above. `@yuzuko/cli` requires Node.js 22 and runs the executable from its `@yuzuko/cli-*` optional dependency. `@yuzuko/types` is the root `yuzuko.d.ts`.

The programs in [`examples/`](../examples/) open a window and draw on its surface.

## Out of scope

A WebView, CEF, an HTML parser, CSS, a DOM tree, `document`, and `HTMLCanvasElement`. WebGL, WebGL2, WebRTC, and WebXR. Node compatibility, an npm client, `node:` builtins, CommonJS evaluation, and JSX. Permission prompts. `HTMLVideoElement`, a media presentation clock, and video or audio encoders. Shared workers, service workers, and worklets other than `AudioWorklet`. CSS `@font-face`.
