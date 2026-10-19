# Yuzuko

Desktop JavaScript and TypeScript runtime built on [rusty_v8](https://github.com/denoland/rusty_v8), inspired by [Deno Desktop](https://docs.deno.com/runtime/desktop/). It has no WebView and no HTML document. A script draws its own pixels and presents them in native windows.

```js
const window = new Yuzuko.Window({ title: "yuzuko", width: 1280, height: 720 });
const ctx = window.surface.getContext("2d");

window.addEventListener("pointerdown", (e) => {
  ctx.fillRect(e.offsetX, e.offsetY, 4, 4);
});

window.requestAnimationFrame(function frame() {
  window.requestAnimationFrame(frame);
});
```

TypeScript entries are erased with oxc before V8 parses them. `yuzuko run game.ts` and `yuzuko run game.js` are the same kind of program. Standard web and WinterTC APIs live on `globalThis`. Runtime-specific APIs live on `Yuzuko`.

`yuzuko run` executes a TypeScript or JavaScript module. `yuzuko test` runs `Yuzuko.test`. Windows, Canvas, and WebGPU come in later phases.

```sh
cargo run -p yuzuko -- run examples/hello.ts
cargo run -p yuzuko -- test tests/pass_test.ts
```

Behavior is specified in [docs/spec.md](docs/spec.md). Intentional divergences from the web platform are in [docs/deviations.md](docs/deviations.md). [AGENTS.md](AGENTS.md) is the implementation contract.

## Platforms

| Triple | OS | Architecture |
| --- | --- | --- |
| `aarch64-apple-darwin` | macOS | arm64 |
| `x86_64-apple-darwin` | macOS | Intel |
| `x86_64-pc-windows-msvc` | Windows | x86_64 |
| `aarch64-unknown-linux-gnu` | Linux | arm64 |
| `x86_64-unknown-linux-gnu` | Linux | x86_64 |

The first machine that must run is macOS arm64. Window, GPU, and input code uses winit and wgpu so the other targets stay reachable.

## License

[MIT](LICENSE.md)
