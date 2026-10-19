# Agents

Implementation contract for Yuzuko. The product spec is `docs/spec.md`. Intentional specification divergences are `docs/deviations.md`. Read all three before implementing. Update `docs/spec.md` before changing the specification.

## Invariants

- Build the runtime on rusty_v8 (the `v8` crate). Do not depend on `deno` or `deno_core`.
- Do not add a WebView, CEF, an HTML parser, or a DOM tree.
- `getContext` returns a context only for `"2d"`, `"webgpu"`, and `"bitmaprenderer"`.
- Web-standard and WinterTC APIs go on `globalThis`. Everything else goes on `Yuzuko`. Do not add a global `Window` constructor. The main global has no `requestAnimationFrame`. It does have `requestIdleCallback`, `cancelIdleCallback`, `EventSource`, `WebSocket`, `WebSocketStream`, `VideoDecoder`, `EncodedVideoChunk`, `VideoFrame`, `AudioDecoder`, `EncodedAudioChunk`, and `AudioData`. `WebSocket` accepts `targetAddressSpace` of `loopback`, `local`, or `public`. A dedicated worker has `requestAnimationFrame` and `cancelAnimationFrame`, and does not have `requestIdleCallback`. Workers do have `EventSource`, `WebSocket`, and `WebSocketStream`.
- A window is `new Yuzuko.Window()`. It is an `EventTarget`. Drawing happens on `window.surface`, a `Yuzuko.WindowSurface`. That surface is not an `OffscreenCanvas` and cannot be transferred. `transferControlToOffscreen()` returns the `OffscreenCanvas` a worker draws. Presenting happens in that window's rendering update. `window.surface.visibilityState` is `"visible"` or `"hidden"`, and `visibilitychange` fires on the surface. OS focus dispatches `focus` and `blur` as `FocusEvent`s on the window. Pointer events target `window.surface` and propagate through the window. `window.matchMedia` tracks that window's CSS size, `devicePixelRatio`, and a Media Queries Level 4 subset.
- A dedicated worker runs on its own thread and enters only its own V8 isolate. winit stays on the main thread. wgpu stays on the thread that created the device. Post other background completions to a task queue and wake that realm's event loop.
- An unimplemented web API must not succeed as a no-op. An unimplemented method throws `NotSupportedError`. An unimplemented global is absent.
- Do not panic on user input or file bytes. Catch a decoder panic with `catch_unwind` and throw `InvalidStateError`.
- Record behavior that differs from a specification in `docs/deviations.md` in the same change.

## How to proceed

Supported targets are the five triples in `docs/spec.md`. Write window, GPU, and input code through winit and wgpu. Do not hide a shared feature behind `target_os = "macos"`.

Do not add an API outside the requested scope. On the main global, `Yuzuko.fonts` is the `FontFaceSet` and `globalThis.fonts` is absent. A dedicated worker exposes the same kind of set as `self.fonts` and `globalThis.fonts`.

Public API names are fixed by the execution model in `docs/spec.md`. Do not add a second window type beside `Yuzuko.Window`.

The codecs are hardware-only H.264 (`avc1` and `avc3`) and AAC (`mp4a`). Other codecs are not decoded. There is no `HTMLVideoElement` and no container demux.

## Repository

```text
AGENTS.md
docs/spec.md
docs/deviations.md
crates/yuzuko/            CLI
crates/yuzuko_core/       isolate, event loop, modules, extensions
crates/yuzuko_dom/
crates/yuzuko_winter/
crates/yuzuko_window/
crates/yuzuko_image/
crates/yuzuko_canvas/
crates/yuzuko_webgpu/
crates/yuzuko_audio/
crates/yuzuko_codec/
examples/
tests/wpt/suite/         web-platform-tests submodule
tests/wpt/expectations/
tests/fixtures/images/
```

Feature crates depend on `yuzuko_core`. `yuzuko_core` does not depend on winit or wgpu. The binary calls each crate's `install`. The runtime crates are `yuzuko`, `yuzuko_core`, `yuzuko_dom`, `yuzuko_winter`, `yuzuko_window`, `yuzuko_image`, `yuzuko_canvas`, `yuzuko_webgpu`, `yuzuko_audio`, and `yuzuko_codec`.

Identifiers, comments, commit messages, JavaScript exception messages, and docs are English.

## Bindings

- Host functions that take and return plain values use `#[op]` from `yuzuko_macros`. The macro converts arguments and the return value. `Err(OpError)` becomes a `TypeError`, a `RangeError`, or a `DOMException`. Throw `DOMException` with the constructor captured for the realm, not with a lookup of `globalThis.DOMException`.
- A host class is an inherent `#[op] impl` of a `v8::cppgc::GarbageCollected` struct. `#[constructor]` returns `Self`. Methods and accessors take `&self` and mutate through interior mutability. `#[static_method]`, `#[getter]`, `#[setter]`, and `#[rename("name")]` map onto the prototype or constructor. `#[op(inherit = Parent)]` sets the prototype parent and requires `#[repr(C)]` with a first field named `base`. `#[this]` passes the JavaScript receiver when it is not the cppgc object, which is how `globalThis` uses `EventTarget`. `#[construct_hook]` runs after the instance is wrapped. `Type::install` defines the constructor on `globalThis` as non-enumerable. `instanceof` follows that template chain.
- Embedded JavaScript closes over the intrinsics it needs while the realm is still pristine. Do not look those methods up again when the callback runs. Public prototypes stay extensible.
- A conversion the specification defines as `ToString` or `ToNumber` still uses that conversion, so a user `toString` or `valueOf` hook runs at the web API boundary. Internal bookkeeping does not go back through `globalThis` or a prototype.
- Host objects use cppgc. Build the prototype chain so `instanceof` matches the specification.
- `globalThis` inherits from `EventTarget` and is the execution realm, not a window. Deliver input events to `Yuzuko.Window`. Entries are ES modules only.
- Do not build an IDL compiler. WebGPU is a handwritten subset in `yuzuko_webgpu`. Generating every interface would publish methods that must throw `NotSupportedError`.
- Do not `unwrap` across a callback boundary. Map failure to `TypeError`, `RangeError`, or `DOMException`.
- Call an extension's `install` once, during isolate initialization.

## Tests

A behavior change includes its test in the same change.

- Web-platform behavior is an upstream WPT file under `tests/wpt/suite`, run with `cargo test -p yuzuko --test wpt`. There is no `yuzuko wpt` subcommand. Update `tests/wpt/expectations` in the same change when results change, including newly passing subtests.
- Behavior WPT does not cover is a `Yuzuko.test` in a `*_test.js`, `*.test.js`, `*_test.ts`, or `*.test.ts` file, run with `yuzuko test`. `Yuzuko.test` exists only in that command.
- Resolve imports with `oxc_resolver` and erase TypeScript with `oxc_transformer`. Do not shell out to `tsc`, and do not typecheck during `run` or `test`.
- Do not copy WPT cases into the repository.
- Pixel comparisons for Canvas 2D use the vello CPU renderer. GPU present is a smoke test. Canvas 2D must fall back to the CPU when the GPU fails.
- Pin a new divergence with a test and add it to `docs/deviations.md`.
