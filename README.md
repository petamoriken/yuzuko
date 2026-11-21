# Yuzuko

Desktop JavaScript and TypeScript runtime built on [rusty_v8](https://github.com/denoland/rusty_v8), inspired by [Deno Desktop](https://docs.deno.com/runtime/desktop/). A program draws its own pixels and presents them in native windows. There is no WebView and no HTML document.

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

Startup opens no window. `new Yuzuko.Window()` opens one. Drawing happens on `window.surface`. `getContext` accepts `"2d"`, `"webgpu"`, and `"bitmaprenderer"`. Animation frames are requested on that window.

`yuzuko run game.ts` and `yuzuko run game.js` run the same kind of program. TypeScript is erased before it runs. The runtime does not typecheck.

Web-standard and WinterTC APIs are properties of `globalThis`. APIs that belong to this runtime are properties of `Yuzuko`. [docs/overview.md](docs/overview.md) introduces each surface. Behavior is specified in [docs/spec.md](docs/spec.md). Differences from the web platform are listed in [docs/deviations.md](docs/deviations.md).

A published build includes windows and input, Canvas 2D, WebGPU, images, fonts, Web Audio, H.264, AV1, AAC, Opus, and Vorbis decoding, fetch, WebSocket, WebTransport, files, datagrams, TCP, an HTTP server, signals, child processes, foreign functions, and an accessibility tree. Workers and `Intl` are in every build.

## Install

```sh
npm install -g @yuzuko/cli
yuzuko run game.ts
```

`@yuzuko/cli` requires Node.js 22 or newer. It depends on `@yuzuko/cli-darwin-arm64`, `@yuzuko/cli-darwin-x64`, `@yuzuko/cli-linux-arm64`, `@yuzuko/cli-linux-x64`, and `@yuzuko/cli-win32-x64`. npm installs the package for this machine, and that package contains the executable the command runs. A Linux install needs the shared libraries named under Platforms.

Each [GitHub release](https://github.com/petamoriken/yuzuko/releases) also contains one binary for each supported machine, and `yuzuko.d.ts`.

```sh
npm install -D @yuzuko/types
```

`@yuzuko/types` is the `yuzuko.d.ts` from that release. It declares the main realm of a default build. Include it beside other types with `/// <reference types="@yuzuko/types" />`. Listing `@yuzuko/types` in `compilerOptions.types` also loads it, and that list replaces the default `@types` set.

## Commands

```text
yuzuko run <entry.ts> [arg...]
yuzuko run --watch [--no-clear-screen] <entry.ts> [arg...]
yuzuko run --watch-hmr [--no-clear-screen] <entry.ts> [arg...]
yuzuko test [<path>...] [--filter <substring>]
yuzuko package <entry.ts> [--config <file>] [--output <path>]
yuzuko repl
```

`yuzuko` with no command starts the same session as `yuzuko repl`. `yuzuko --help` prints the usage above.

`yuzuko run` evaluates an ES module. Tokens after the entry become `Yuzuko.args`. `--watch` restarts that module in a new realm when a loaded file changes. `--watch-hmr` re-evaluates a changed module and the modules that import it, in the same realm. A `Yuzuko.Window` or playing `AudioContext` created by a module that is not re-evaluated stays. `--no-clear-screen` leaves the terminal contents in place between those runs.

`yuzuko test` runs `Yuzuko.test`. That function exists only for this command. Discovery follows `*_test.js`, `*.test.js`, `*_test.ts`, and `*.test.ts`. `--filter` keeps tests whose names contain the substring.

`yuzuko repl` evaluates classic-script chunks from the terminal. The prompt is `> `.

The programs in `examples/` open a window and draw on its surface. Escape closes the window.

```sh
yuzuko run examples/hello.ts
yuzuko run examples/breakout.ts
yuzuko run examples/paint.ts
yuzuko run examples/clock.ts
```

`hello.ts` draws a title. `breakout.ts` is a paddle game: the pointer or the arrow keys move the paddle, and a click or Space serves. `paint.ts` draws with the pointer. `clock.ts` draws the time.

## Permissions

Host access is modeled on [Deno's permissions](https://docs.deno.com/runtime/fundamentals/security/): `read`, `write`, `net`, `env`, `run`, and `ffi`, granted with `--allow-*` and `--deny-*`. `yuzuko run` asks on a terminal when a grant is missing. A denied call throws `Yuzuko.errors.NotCapable`.

Flags belong before the entry:

```text
-A, --allow-all
--allow-read[=<path>...] (-R), --allow-write[=<path>...] (-W)
--allow-net[=<host>...] (-N), --allow-env[=<name>...] (-E)
--allow-run[=<command>...]
--allow-ffi[=<path>...]
```

The matching `--deny-*` flags remove grants. `--no-prompt` skips the question. A bare flag covers the whole kind. A value after `=` is a comma-separated list.

`yuzuko repl` and `yuzuko test` do not ask. `permissions` in `yuzuko.json` supplies the grants for `yuzuko run` and for a packaged application. A packaged application that includes the object does not ask, and an omitted name is denied. A packaged application that omits the key allows every check. The rules are in [docs/spec.md](docs/spec.md#permissions).

## Packaging

`yuzuko package` reads `yuzuko.json` or `yuzuko.jsonc` and writes an application that starts from a double click. Discovery starts at the entry's directory and walks toward the root. `--config` names the file and skips that walk. macOS output is an ad-hoc-signed `.app`. Windows and Linux outputs are directories. The bundle contains the executable that ran the command, the module graph, and the assets listed in the config.

```json
{
  "name": "My Game",
  "version": "1.0.0",
  "identifier": "com.example.mygame",
  "output": {
    "macos": "./dist/MyGame.app",
    "windows": "./dist/MyGame",
    "linux": "./dist/my-game"
  },
  "permissions": {
    "read": ["./assets"],
    "net": ["updates.example.com"]
  }
}
```

`--output` replaces the path for this operating system. A command-line value wins over the file. Field definitions are in [docs/spec.md](docs/spec.md#packaging).

## Platforms

| Triple | OS | Architecture |
| --- | --- | --- |
| `aarch64-apple-darwin` | macOS | arm64 |
| `x86_64-apple-darwin` | macOS | Intel |
| `x86_64-pc-windows-msvc` | Windows | x86_64 |
| `aarch64-unknown-linux-gnu` | Linux | arm64 |
| `x86_64-unknown-linux-gnu` | Linux | x86_64 |

A Linux machine that runs a published binary needs the GTK 3, libxdo, libudev, libayatana-appindicator, and ALSA shared libraries, and glibc 2.31 or later.

## License

[MIT](LICENSE.md)

## Development

[AGENTS.md](AGENTS.md) is the implementation contract. Window, GPU, and input code uses winit and wgpu, so one implementation serves every target above.

```sh
cargo run -p yuzuko -- run examples/hello.ts
cargo run -p yuzuko -- test tests/pass_test.ts
cargo test -p yuzuko --test wpt
cargo test -p yuzuko --test cts
```

`cargo build -p yuzuko` is the full runtime. `cargo build -p yuzuko --no-default-features` is the JavaScript runtime alone. `yuzuko package` copies the executable that invoked it and does not rebuild it. Cargo features are listed under Build features in [docs/spec.md](docs/spec.md#build-features).

The test suite runs on macOS arm64, Windows 11 x86_64, and Ubuntu 20.04 x86_64 under WSL2. Windows builds link the C runtime statically, which `.cargo/config.toml` sets for `x86_64-pc-windows-msvc`. A Linux build needs the GTK 3, xdo, udev, AppIndicator, and ALSA development packages:

```sh
sudo apt-get install libgtk-3-dev libxdo-dev libudev-dev libayatana-appindicator3-dev libasound2-dev
```

`cargo test -p yuzuko --test wpt` starts the WPT server with Python 3.9 or later and expects the `web-platform.test` names from `./wpt make-hosts-file` in the hosts file. There is no `yuzuko wpt` subcommand. Rust unit tests use `cargo test`. Script tests use `yuzuko test`.

`cargo test -p yuzuko --test cts` runs the WebGPU CTS pin in `tests/webgpu/suite`. From that directory, `npm ci --ignore-scripts` and `npx grunt standalone` compile `out/` first. There is no `yuzuko cts` subcommand. The enrolled query is `unittests:*`. `YUZUKO_CTS_FILTER` selects one other query.

The manual Release workflow builds the default release binary for every supported triple. It uploads those binaries and `yuzuko.d.ts` to the GitHub release for the tag, and it can publish `@yuzuko/cli`, the platform packages, and `@yuzuko/types`.
