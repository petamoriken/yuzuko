//! The host bundle holds the real `yuzuko` binary: a `.app` on macOS and a
//! directory on Windows and Linux.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use yuzuko::{Target, load_package, package};

fn bin() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_yuzuko"))
}

fn scratch(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!(
    "yuzuko-package-{}-{}-{name}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap_or_default()
      .as_nanos()
  ));
  let _ = fs::remove_dir_all(&dir);
  fs::create_dir_all(&dir).unwrap();
  dir
}

fn write(path: &Path, text: &str) {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).unwrap();
  }
  fs::write(path, text).unwrap();
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
  Command::new(bin())
    .current_dir(dir)
    .args(args)
    .env("YUZUKO_HEADLESS", "1")
    .output()
    .expect("spawn yuzuko")
}

fn text(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes).into_owned()
}

const ENTRY: &str = r#"import { mark } from "./lib.ts";
import type { Missing } from "./missing-type.ts";
const loadDynamic = () => import("./missing-dynamic.ts");
void loadDynamic;
const asset = new URL("../assets/note.txt", import.meta.url);
const body = await (await fetch(asset)).text();
const win = new Yuzuko.Window();
const titled = new Yuzuko.Window({ title: "scripted" });
console.log(body.trim());
console.log(win.surface.title);
console.log(titled.surface.title);
console.log(import.meta.url);
console.log(mark);
console.log(typeof Yuzuko.test);
console.log("updater:" + typeof Yuzuko.updater);
let blocked = false;
try {
  await fetch(new URL("/etc/passwd", import.meta.url));
} catch (error) {
  blocked = true;
}
console.log(blocked ? "blocked" : "leaked");
titled.close();
await new Promise((resolve) => setTimeout(resolve, 1500));
win.close();
"#;

const LIB: &str = "export const mark = \"local-import-marker\";\n";
const NOTE: &str = "fixture-asset-token\n";

fn game_tree(dir: &Path, config: &str) {
  write(&dir.join("yuzuko.jsonc"), config);
  write(&dir.join("src/main.ts"), ENTRY);
  write(&dir.join("src/lib.ts"), LIB);
  write(&dir.join("assets/note.txt"), NOTE);
}

fn base_config(extra: &str) -> String {
  format!(
    r#"{{
  // display name
  "name": "My Game",
  "version": "1.4.2",
  "assets": ["./assets/**"],
  "release": {{ "baseUrl": "https://updates.example.invalid/game" }},
  {extra}
}}
"#
  )
}

#[test]
fn refusals_write_nothing_and_do_not_panic() {
  let dir = scratch("refuse");
  let missing_icon = dir.join("missing-icon.app");
  game_tree(
    &dir.join("bad-icon"),
    "{\n  \"name\": \"Bad\",\n  \"icons\": { \"macos\": \"./nope.icns\" }\n}\n",
  );
  write(&dir.join("bad-icon/src/main.ts"), "console.log(1)\n");
  let output = run(
    &dir,
    &[
      "package",
      "bad-icon/src/main.ts",
      "--config",
      "bad-icon/yuzuko.jsonc",
      "--output",
      missing_icon.to_str().unwrap(),
    ],
  );
  assert_ne!(output.status.code(), Some(0), "{}", text(&output.stdout));
  assert!(
    !text(&output.stderr).contains("panicked"),
    "{}",
    text(&output.stderr)
  );
  assert!(!missing_icon.exists());

  let cases: &[(&str, &str)] = &[
    ("bad-id", r#"{ "name": "Bad", "identifier": "com" }"#),
    ("unknown", r#"{ "name": "Bad", "backend": "cef" }"#),
    ("deep", r#"{ "name": "Bad", "deepLinks": ["app"] }"#),
    (
      "errors",
      r#"{ "name": "Bad", "errorReporting": { "url": "https://x" } }"#,
    ),
    (
      "exports",
      r#"{ "name": "Bad", "exports": "./src/main.ts" }"#,
    ),
    (
      "escape",
      r#"{ "name": "Bad", "icons": { "linux": "../outside.ts" } }"#,
    ),
    (
      "url",
      r#"{ "name": "Bad", "release": { "baseUrl": "file:///tmp" } }"#,
    ),
    (
      "png-ico",
      r#"{ "name": "Bad", "icons": { "windows": [ { "path": "./a.png", "size": 16 } ] } }"#,
    ),
    (
      "not-app",
      r#"{ "name": "Bad", "output": { "macos": "./Foo.txt" } }"#,
    ),
    (
      "dmg",
      r#"{ "name": "Bad", "output": { "macos": "./Foo.dmg" } }"#,
    ),
    (
      "msi",
      r#"{ "name": "Bad", "output": { "windows": "./Foo.msi" } }"#,
    ),
    (
      "deb",
      r#"{ "name": "Bad", "output": { "linux": "./Foo.deb" } }"#,
    ),
    (
      "rpm",
      r#"{ "name": "Bad", "output": { "linux": "./Foo.rpm" } }"#,
    ),
    (
      "appimage",
      r#"{ "name": "Bad", "output": { "linux": "./Foo.AppImage" } }"#,
    ),
    ("comma", "{ \"name\": \"Bad\", }\n"),
    ("slug", r#"{ "name": "123" }"#),
    ("digit", r#"{ "name": "Bad", "identifier": "com.2game" }"#),
    (
      "empty-label",
      r#"{ "name": "Bad", "identifier": "com..game" }"#,
    ),
    (
      "absolute",
      r#"{ "name": "Bad", "icons": { "linux": "/etc/passwd" } }"#,
    ),
    (
      "backslash",
      r#"{ "name": "Bad", "icons": { "linux": ".\\app.png" } }"#,
    ),
    (
      "icon-size",
      r#"{ "name": "Bad", "icons": { "macos": [ { "path": "./a.png", "size": 15 } ] } }"#,
    ),
  ];
  for (name, body) in cases {
    let root = dir.join(name);
    write(&root.join("yuzuko.json"), body);
    write(&root.join("src/main.ts"), "console.log(1)\n");
    let dest = dir.join(format!("{name}.app"));
    let output = run(
      &root,
      &["package", "src/main.ts", "--output", dest.to_str().unwrap()],
    );
    assert_ne!(
      output.status.code(),
      Some(0),
      "{name}: {}",
      text(&output.stderr)
    );
    assert!(
      !text(&output.stderr).contains("panicked"),
      "{name}: {}",
      text(&output.stderr)
    );
    if *name == "exports" {
      assert!(
        text(&output.stderr).contains("unknown field"),
        "{name}: {}",
        text(&output.stderr)
      );
    }
    assert!(!dest.exists(), "{name}");
  }

  let both = dir.join("both");
  fs::create_dir_all(&both).unwrap();
  write(&both.join("yuzuko.json"), r#"{ "name": "A" }"#);
  write(&both.join("yuzuko.jsonc"), r#"{ "name": "B" }"#);
  write(&both.join("src/main.ts"), "console.log(1)\n");
  let dest = dir.join("both.app");
  let output = run(
    &both,
    &["package", "src/main.ts", "--output", dest.to_str().unwrap()],
  );
  assert_ne!(output.status.code(), Some(0));
  assert!(!dest.exists());
  let run_both = run(&both, &["run", "src/main.ts"]);
  assert_ne!(
    run_both.status.code(),
    Some(0),
    "{}",
    text(&run_both.stderr)
  );

  let bare = dir.join("bare");
  fs::create_dir_all(&bare).unwrap();
  write(&bare.join("yuzuko.json"), r#"{ "name": "Bare" }"#);
  let dest = dir.join("bare.app");
  let output = run(&bare, &["package"]);
  assert_ne!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(
    text(&output.stderr).contains("needs an entry path"),
    "{}",
    text(&output.stderr)
  );
  assert!(!dest.exists());
  assert!(
    fs::read_dir(&bare)
      .unwrap()
      .all(|entry| { entry.unwrap().file_name() == "yuzuko.json" })
  );

  let no_entry = run(&dir, &["run"]);
  assert_ne!(no_entry.status.code(), Some(0));

  let ok = dir.join("ok");
  write(&ok.join("yuzuko.json"), r#"{ "name": "Ok" }"#);
  write(&ok.join("src/main.ts"), "console.log(1)\n");
  for extension in [".dmg", ".msi", ".deb", ".rpm", ".AppImage"] {
    let dest = dir.join(format!("forbid{extension}"));
    let output = run(
      &ok,
      &["package", "src/main.ts", "--output", dest.to_str().unwrap()],
    );
    assert_ne!(
      output.status.code(),
      Some(0),
      "{extension}: {}",
      text(&output.stderr)
    );
    assert!(
      !text(&output.stderr).contains("panicked"),
      "{extension}: {}",
      text(&output.stderr)
    );
    assert!(!dest.exists(), "{extension}");
  }
  // Only a macOS bundle must end in `.app`. Windows and Linux write a
  // directory under any name.
  if cfg!(target_os = "macos") {
    let not_app = dir.join("not-an-app.txt");
    let output = run(
      &ok,
      &[
        "package",
        "src/main.ts",
        "--output",
        not_app.to_str().unwrap(),
      ],
    );
    assert_ne!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(!not_app.exists());
  }

  let no_output = dir.join("no-output");
  write(&no_output.join("yuzuko.json"), r#"{ "name": "No Output" }"#);
  write(&no_output.join("src/main.ts"), "console.log(1)\n");
  let output = run(&no_output, &["package", "src/main.ts"]);
  assert_ne!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(
    text(&output.stderr).contains("needs --output"),
    "{}",
    text(&output.stderr)
  );
  assert!(!no_output.join("No Output.app").exists());
  assert!(!no_output.join("no-output.app").exists());

  let empty = dir.join("empty");
  fs::create_dir_all(&empty).unwrap();
  let dest = dir.join("empty.app");
  let output = run(
    &empty,
    &["package", "main.ts", "--output", dest.to_str().unwrap()],
  );
  assert_ne!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(
    text(&output.stderr).contains("yuzuko.json was not found"),
    "{}",
    text(&output.stderr)
  );
  assert!(!dest.exists());

  let near = dir.join("near");
  write(&near.join("yuzuko.json"), r#"{ "name": "Parent" }"#);
  write(
    &near.join("child/yuzuko.json"),
    r#"{ "name": "Child", "identifier": "com" }"#,
  );
  write(&near.join("child/main.ts"), "console.log(1)\n");
  let dest = dir.join("near.app");
  let output = run(
    &near,
    &[
      "package",
      "child/main.ts",
      "--output",
      dest.to_str().unwrap(),
    ],
  );
  assert_ne!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(
    text(&output.stderr).contains("two labels"),
    "{}",
    text(&output.stderr)
  );
  assert!(!dest.exists());
  let skipped = run(
    &near.join("child"),
    &["package", "main.ts", "--config", "../yuzuko.json"],
  );
  assert_ne!(skipped.status.code(), Some(0), "{}", text(&skipped.stderr));
  assert!(
    text(&skipped.stderr).contains("needs --output"),
    "{}",
    text(&skipped.stderr)
  );
  assert!(
    !text(&skipped.stderr).contains("two labels"),
    "{}",
    text(&skipped.stderr)
  );

  write(&dir.join("outside.ts"), "console.log(1)\n");
  let dest = dir.join("escaped-entry.app");
  let output = run(
    &dir,
    &[
      "package",
      "--config",
      "near/yuzuko.json",
      "outside.ts",
      "--output",
      dest.to_str().unwrap(),
    ],
  );
  assert_ne!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(
    text(&output.stderr).contains("escapes"),
    "{}",
    text(&output.stderr)
  );
  assert!(!dest.exists());

  let unclosed = dir.join("unclosed");
  write(
    &unclosed.join("yuzuko.jsonc"),
    "{ \"name\": \"Bad\", /* never closed\n",
  );
  write(&unclosed.join("src/main.ts"), "console.log(1)\n");
  let dest = dir.join("unclosed.app");
  let output = run(
    &unclosed,
    &["package", "src/main.ts", "--output", dest.to_str().unwrap()],
  );
  assert_ne!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(!dest.exists());

  // Creating a symlink on Windows needs Developer Mode or an elevated token.
  #[cfg(unix)]
  {
    let link = dir.join("link");
    write(
      &link.join("yuzuko.json"),
      r#"{ "name": "Link", "assets": ["./assets/**"] }"#,
    );
    write(&link.join("main.ts"), "console.log(1)\n");
    write(&link.join("assets/keep.txt"), "keep\n");
    std::os::unix::fs::symlink("/etc/passwd", link.join("assets/passwd"))
      .unwrap();
    let dest = dir.join("link.app");
    let output = run(
      &link,
      &["package", "main.ts", "--output", dest.to_str().unwrap()],
    );
    assert_ne!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(
      text(&output.stderr).contains("escapes"),
      "{}",
      text(&output.stderr)
    );
    assert!(!text(&output.stderr).contains("panicked"));
    assert!(!dest.exists());
    assert!(!dir.join(".link.app.partial").exists());
  }

  let host = dir.join("host");
  write(&host.join("outside.ts"), "export const x = 1;\n");
  write(
    &host.join("app/main.ts"),
    "import { x } from \"../outside.ts\";\nconsole.log(x);\n",
  );
  write(&host.join("app/yuzuko.json"), r#"{ "name": "Host" }"#);
  let dest = dir.join("host.app");
  let output = run(
    &host.join("app"),
    &["package", "main.ts", "--output", dest.to_str().unwrap()],
  );
  assert_ne!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(
    text(&output.stderr).contains("outside"),
    "{}",
    text(&output.stderr)
  );
  assert!(!dest.exists());

  for (name, source) in [
    ("http", "import \"https://example.com/mod.js\";\n"),
    ("node", "import \"node:fs\";\n"),
    ("data", "import \"data:text/javascript,export{}\";\n"),
  ] {
    let root = dir.join(name);
    write(&root.join("yuzuko.json"), r#"{ "name": "Remote" }"#);
    write(&root.join("main.ts"), source);
    let dest = dir.join(format!("{name}.app"));
    let output = run(
      &root,
      &["package", "main.ts", "--output", dest.to_str().unwrap()],
    );
    assert_ne!(
      output.status.code(),
      Some(0),
      "{name}: {}",
      text(&output.stderr)
    );
    assert!(!text(&output.stderr).contains("panicked"), "{name}");
    assert!(!dest.exists(), "{name}");
  }

  // The Windows icon is written into the executable, so it must parse.
  if cfg!(windows) {
    let root = dir.join("bad-ico");
    write(
      &root.join("yuzuko.json"),
      r#"{ "name": "Bad", "icons": { "windows": "./app.ico" } }"#,
    );
    write(&root.join("app.ico"), "ICO-BYTES");
    write(&root.join("src/main.ts"), "console.log(1)\n");
    let dest = dir.join("bad-ico-out");
    let output = run(
      &root,
      &["package", "src/main.ts", "--output", dest.to_str().unwrap()],
    );
    assert_ne!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(
      text(&output.stderr).contains("not a valid .ico"),
      "{}",
      text(&output.stderr)
    );
    assert!(!dest.exists());
    assert!(!dir.join(".bad-ico-out.partial").exists());
  }

  let comment = dir.join("comment");
  write(
    &comment.join("yuzuko.json"),
    "{ \"name\": \"Bad\" /* no */ }\n",
  );
  write(&comment.join("src/main.ts"), "console.log(1)\n");
  let dest = dir.join("comment.app");
  let output = run(
    &comment,
    &["package", "src/main.ts", "--output", dest.to_str().unwrap()],
  );
  assert_ne!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(!dest.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn mac_app_launches_with_assets_title_and_adhoc_signature() {
  let dir = scratch("mac");
  // `output.macos` is a decoy; `--output` wins and the decoy is not written.
  let config = base_config(r#""output": { "macos": "./decoy.app" },"#);
  game_tree(&dir, &config);
  let app = dir.join("MyGame.app");
  let output = Command::new(bin())
    .current_dir(&dir)
    .args(["package", "src/main.ts", "--output", app.to_str().unwrap()])
    .output()
    .unwrap();
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(!dir.join("decoy.app").exists());
  assert!(!dir.join(".MyGame.app.partial").exists());
  assert!(app.join("Contents/Resources/app/assets/note.txt").is_file());
  assert_eq!(
    fs::read(app.join("Contents/Resources/app/assets/note.txt")).unwrap(),
    NOTE.as_bytes()
  );
  assert!(app.join("Contents/Resources/app/src/lib.ts").is_file());
  assert!(
    !app
      .join("Contents/Resources/app/src/missing-type.ts")
      .exists()
  );
  assert!(
    !app
      .join("Contents/Resources/app/src/missing-dynamic.ts")
      .exists()
  );
  assert!(!app.join("Contents/Resources/AppIcon.icns").exists());
  assert!(!app.join("Contents/Frameworks").exists());
  let plist = fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
  assert!(
    plist.contains("<key>CFBundleName</key><string>My Game</string>"),
    "{plist}"
  );
  assert!(
    plist.contains(
      "<key>CFBundleIdentifier</key><string>com.yuzuko.my-game</string>"
    ),
    "{plist}"
  );
  assert!(plist.contains("<string>1.4.2</string>"), "{plist}");
  assert!(
    plist.contains("https://updates.example.invalid/game"),
    "{plist}"
  );
  let manifest =
    fs::read_to_string(app.join("Contents/Resources/yuzuko.bundle.json"))
      .unwrap();
  assert!(
    manifest.contains("https://updates.example.invalid/game"),
    "{manifest}"
  );
  assert!(
    manifest.contains("\"entry\": \"src/main.ts\""),
    "{manifest}"
  );
  assert!(!app.join("Contents/Resources/yuzuko.update-key").exists());
  assert!(!plist.contains("CFBundleIconFile"), "{plist}");
  let exe_name = plist
    .split("<key>CFBundleExecutable</key><string>")
    .nth(1)
    .unwrap()
    .split("</string>")
    .next()
    .unwrap();
  assert_eq!(exe_name, "my-game");
  let exe = app.join("Contents/MacOS").join(exe_name);
  let macos_files = fs::read_dir(app.join("Contents/MacOS"))
    .unwrap()
    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
    .collect::<Vec<_>>();
  assert_eq!(macos_files, vec!["my-game".to_string()]);
  let header = fs::read(&exe).unwrap();
  assert!(header.len() > 4);
  assert_eq!(&header[0..4], &[0xcf, 0xfa, 0xed, 0xfe], "not a Mach-O");
  let verify = Command::new("codesign")
    .args(["--verify", "--verbose", app.to_str().unwrap()])
    .output()
    .unwrap();
  assert_eq!(verify.status.code(), Some(0), "{}", text(&verify.stderr));
  let details = Command::new("codesign")
    .args(["-dv", app.to_str().unwrap()])
    .output()
    .unwrap();
  let described = format!("{}{}", text(&details.stdout), text(&details.stderr));
  assert!(
    described.to_ascii_lowercase().contains("adhoc"),
    "{described}"
  );
  assert!(described.contains("com.yuzuko.my-game"), "{described}");

  let dashed = dir.join("Dashed.app");
  write(
    &dir.join("yuzuko.jsonc"),
    &base_config("\"macos\": { \"codesignIdentity\": \"-\" },"),
  );
  let output = Command::new(bin())
    .current_dir(&dir)
    .args([
      "package",
      "src/main.ts",
      "--output",
      dashed.to_str().unwrap(),
    ])
    .output()
    .unwrap();
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let verify = Command::new("codesign")
    .args(["--verify", dashed.to_str().unwrap()])
    .output()
    .unwrap();
  assert_eq!(verify.status.code(), Some(0), "{}", text(&verify.stderr));
  let details = Command::new("codesign")
    .args(["-dv", dashed.to_str().unwrap()])
    .output()
    .unwrap();
  let described = format!("{}{}", text(&details.stdout), text(&details.stderr));
  assert!(
    described.to_ascii_lowercase().contains("adhoc"),
    "{described}"
  );

  let rejected = dir.join("Rejected.app");
  fs::create_dir_all(rejected.join("Contents")).unwrap();
  fs::write(rejected.join("Contents/sentinel"), "keep").unwrap();
  write(
    &dir.join("yuzuko.jsonc"),
    &base_config(
      "\"macos\": { \"codesignIdentity\": \"Yuzuko Not A Signing Identity\" },",
    ),
  );
  let output = Command::new(bin())
    .current_dir(&dir)
    .args([
      "package",
      "src/main.ts",
      "--output",
      rejected.to_str().unwrap(),
    ])
    .output()
    .unwrap();
  assert_ne!(output.status.code(), Some(0), "{}", text(&output.stdout));
  assert_eq!(
    fs::read_to_string(rejected.join("Contents/sentinel")).unwrap(),
    "keep"
  );
  assert!(!dir.join(".Rejected.app.partial").exists());

  let sentinel = dir.join("launch-cwd");
  fs::create_dir_all(&sentinel).unwrap();
  let first = launch(&exe, &sentinel, &[]);
  let second = launch(&exe, &sentinel, &["-psn_0_12345"]);
  assert_eq!(first.status, 0, "{}", first.stderr);
  assert_eq!(second.status, 0, "{}", second.stderr);
  assert_eq!(first.stdout, second.stdout);
  assert!(
    first.stdout.contains("fixture-asset-token"),
    "{}",
    first.stdout
  );
  assert!(first.stdout.contains("My Game"), "{}", first.stdout);
  assert!(first.stdout.contains("scripted"), "{}", first.stdout);
  assert!(first.stdout.contains("updater:object"), "{}", first.stdout);
  assert!(
    first.stdout.contains("Contents/Resources/app"),
    "{}",
    first.stdout
  );
  assert!(
    !first
      .stdout
      .contains(dir.join("src/main.ts").to_str().unwrap())
  );
  assert!(
    first.stdout.contains("local-import-marker"),
    "{}",
    first.stdout
  );
  assert!(first.stdout.contains("undefined"), "{}", first.stdout);
  assert!(first.stdout.contains("blocked"), "{}", first.stdout);
  assert!(
    first.cwd.contains(sentinel.to_str().unwrap()),
    "{}",
    first.cwd
  );
  assert!(!first.cwd.contains("Contents/Resources"), "{}", first.cwd);

  let opened = open_app(&app);
  assert_eq!(opened.0, 0, "{}", opened.1);
  let opened = open_app(&app);
  assert_eq!(opened.0, 0, "{}", opened.1);

  let bundled_run = Command::new(&exe).args(["run"]).output().unwrap();
  assert_ne!(
    bundled_run.status.code(),
    Some(0),
    "{}",
    text(&bundled_run.stdout)
  );
  let mixed = Command::new(&exe)
    .args(["-psn_0_1", "extra"])
    .output()
    .unwrap();
  assert_ne!(mixed.status.code(), Some(0), "{}", text(&mixed.stdout));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn directory_bundle_launches_with_assets_and_title() {
  let dir = scratch("dir");
  // `output.macos` is a decoy; `--output` wins and the decoy is not written.
  let config = base_config(
    r#""output": { "macos": "./decoy.app" },
  "icons": { "windows": "./icons/app.ico", "linux": "./icons/app.png" },"#,
  );
  game_tree(&dir, &config);
  fs::create_dir_all(dir.join("icons")).unwrap();
  fs::write(dir.join("icons/app.ico"), one_pixel_ico()).unwrap();
  fs::write(dir.join("icons/app.png"), one_pixel_png()).unwrap();
  let bundle = dir.join("MyGame");
  let output = Command::new(bin())
    .current_dir(&dir)
    .args([
      "package",
      "src/main.ts",
      "--output",
      bundle.to_str().unwrap(),
    ])
    .output()
    .unwrap();
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert!(!dir.join("decoy.app").exists());
  assert!(!dir.join(".MyGame.partial").exists());
  assert!(!bundle.join("Contents").exists());
  assert_eq!(
    fs::read(bundle.join("app/assets/note.txt")).unwrap(),
    NOTE.as_bytes()
  );
  assert!(bundle.join("app/src/lib.ts").is_file());
  assert!(!bundle.join("app/src/missing-type.ts").exists());
  assert!(!bundle.join("app/src/missing-dynamic.ts").exists());
  let manifest = fs::read_to_string(bundle.join("yuzuko.bundle.json")).unwrap();
  assert!(
    manifest.contains("https://updates.example.invalid/game"),
    "{manifest}"
  );
  assert!(
    manifest.contains("\"entry\": \"src/main.ts\""),
    "{manifest}"
  );
  assert!(!bundle.join("yuzuko.update-key").exists());
  let exe = if cfg!(windows) {
    assert_eq!(
      fs::read_to_string(bundle.join("AppUserModelID")).unwrap(),
      "com.yuzuko.my-game"
    );
    bundle.join("my-game.exe")
  } else {
    let desktop =
      fs::read_to_string(bundle.join("com.yuzuko.my-game.desktop")).unwrap();
    assert!(desktop.contains("Name=My Game"), "{desktop}");
    assert!(desktop.contains("Exec=my-game"), "{desktop}");
    bundle.join("my-game")
  };
  let header = fs::read(&exe).unwrap();
  if cfg!(windows) {
    assert_eq!(&header[0..2], b"MZ", "not a PE image");
    // A double click opens no console: the copy is a GUI-subsystem image.
    let pe = u32::from_le_bytes(header[0x3c..0x40].try_into().unwrap());
    let at = pe as usize + 24 + 68;
    assert_eq!(u16::from_le_bytes([header[at], header[at + 1]]), 2);
    let runtime = fs::read(bin()).unwrap();
    assert_eq!(u16::from_le_bytes([runtime[at], runtime[at + 1]]), 3);
    // Explorer reads the icon from the executable's resources.
    #[cfg(windows)]
    {
      assert_eq!(icon_count(&bin()), 0, "the runtime has its own icon");
      assert_eq!(icon_count(&exe), 1, "the bundle icon was not embedded");
    }
  } else {
    assert_eq!(&header[0..4], b"\x7fELF", "not an ELF image");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let mode = fs::metadata(&exe).unwrap().permissions().mode();
      assert_eq!(mode & 0o111, 0o111, "{mode:o}");
    }
  }

  let sentinel = dir.join("launch-cwd");
  fs::create_dir_all(&sentinel).unwrap();
  let first = launch(&exe, &sentinel, &[]);
  assert_eq!(first.status, 0, "{}", first.stderr);
  assert!(
    first.stdout.contains("fixture-asset-token"),
    "{}",
    first.stdout
  );
  assert!(first.stdout.contains("My Game"), "{}", first.stdout);
  assert!(first.stdout.contains("scripted"), "{}", first.stdout);
  assert!(first.stdout.contains("updater:object"), "{}", first.stdout);
  assert!(
    first.stdout.contains("/app/src/main.ts"),
    "{}",
    first.stdout
  );
  assert!(
    first.stdout.contains("local-import-marker"),
    "{}",
    first.stdout
  );
  assert!(first.stdout.contains("undefined"), "{}", first.stdout);
  assert!(first.stdout.contains("blocked"), "{}", first.stdout);
  if cfg!(target_os = "linux") {
    assert_eq!(first.cwd, sentinel.to_str().unwrap());
  }

  let bundled_run = Command::new(&exe).args(["run"]).output().unwrap();
  assert_ne!(
    bundled_run.status.code(),
    Some(0),
    "{}",
    text(&bundled_run.stdout)
  );
}

#[test]
fn run_uses_config_only_for_the_omitted_title() {
  let dir = scratch("run");
  write(&dir.join("yuzuko.json"), "{}\n");
  write(
    &dir.join("title.ts"),
    "const win = new Yuzuko.Window();\nconsole.log(win.surface.title);\nwin.close();\n",
  );
  let output = run(&dir, &["run", "title.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(
    text(&output.stdout).trim(),
    dir.file_name().unwrap().to_str().unwrap()
  );

  write(&dir.join("yuzuko.json"), "{ \"name\": \"Configured\" }\n");
  let output = run(&dir, &["run", "title.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout).trim(), "Configured");

  write(
    &dir.join("explicit.ts"),
    "const win = new Yuzuko.Window({ title: \"scripted\" });\nconsole.log(win.surface.title);\nwin.close();\n",
  );
  let output = run(&dir, &["run", "explicit.ts"]);
  assert_eq!(text(&output.stdout).trim(), "scripted");

  let bare = scratch("run-none");
  write(&bare.join("plain.ts"), "console.log(\"no-window\");\n");
  let output = run(&bare, &["run", "plain.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout), "no-window\n");

  write(
    &bare.join("title.ts"),
    "const win = new Yuzuko.Window();\nconsole.log(win.surface.title);\nwin.close();\n",
  );
  let output = run(&bare, &["run", "title.ts"]);
  assert_eq!(text(&output.stdout).trim(), "yuzuko");

  let boxed = scratch("run-box");
  write(&boxed.join("secret.txt"), "run-sandbox-token\n");
  write(
    &boxed.join("sub/entry.ts"),
    r#"let blocked = false;
try {
  await fetch(new URL("../secret.txt", import.meta.url));
} catch (error) {
  blocked = true;
}
console.log(blocked ? "run-blocked" : "run-leaked");
"#,
  );
  let output = run(&boxed, &["run", "sub/entry.ts"]);
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout).trim(), "run-blocked");
  assert!(!text(&output.stdout).contains("run-sandbox-token"));
  assert!(fs::read_dir(&dir).unwrap().all(|entry| {
    !entry
      .unwrap()
      .file_name()
      .to_string_lossy()
      .ends_with(".app")
  }));
}

#[test]
fn windows_and_linux_layouts_use_the_packager() {
  let dir = scratch("other");
  write(&dir.join("stand-in.bin"), "STANDIN-RUNTIME");
  write(&dir.join("icons/app.ico"), "ICO-BYTES");
  write(&dir.join("icons/app.png"), "PNG-BYTES");
  write(&dir.join("src/main.ts"), "console.log(1)\n");
  write(
    &dir.join("yuzuko.json"),
    r#"{
      "name": "Pack Game",
      "version": "9.9.9",
      "identifier": "com.example.game2",
      "icons": { "windows": "./icons/app.ico", "linux": "./icons/app.png" }
    }"#,
  );
  let config = load_package(&dir.join("src/main.ts"), None).unwrap();
  let win = dir.join("win-out");
  package(&config, Target::Windows, &dir.join("stand-in.bin"), &win).unwrap();
  assert_eq!(
    fs::read_to_string(win.join("AppUserModelID")).unwrap(),
    "com.example.game2"
  );
  assert_eq!(fs::read(win.join("AppIcon.ico")).unwrap(), b"ICO-BYTES");
  assert_eq!(
    fs::read(win.join("pack-game.exe")).unwrap(),
    b"STANDIN-RUNTIME"
  );
  let linux = dir.join("linux-out");
  package(&config, Target::Linux, &dir.join("stand-in.bin"), &linux).unwrap();
  assert!(win.is_dir());
  assert!(!win.with_extension("msi").exists());
  assert!(linux.join("com.example.game2.desktop").is_file());
  assert_eq!(fs::read(linux.join("icons/app.png")).unwrap(), b"PNG-BYTES");
  let desktop =
    fs::read_to_string(linux.join("com.example.game2.desktop")).unwrap();
  assert!(desktop.contains("Name=Pack Game"), "{desktop}");
  assert!(desktop.contains("Exec=pack-game"), "{desktop}");
  assert!(
    desktop.contains("StartupWMClass=com.example.game2"),
    "{desktop}"
  );
  assert!(linux.is_dir());
  assert!(!linux.with_extension("deb").exists());
  assert!(!linux.with_extension("rpm").exists());
  assert!(!linux.with_extension("AppImage").exists());

  write(&dir.join("icons/small.png"), "PNG-ARRAY");
  write(
    &dir.join("yuzuko.json"),
    r#"{
      "name": "Pack Game",
      "icons": { "windows": [ { "path": "./icons/small.png", "size": 32 } ] }
    }"#,
  );
  let bad_win = dir.join("bad-win");
  let bad_linux = dir.join("bad-linux");
  let loaded = load_package(&dir.join("src/main.ts"), None);
  assert!(loaded.is_err());
  assert!(!bad_win.exists());
  assert!(!bad_linux.exists());

  write(&dir.join("icons/128.png"), "PNG-128-FIXTURE");
  write(
    &dir.join("yuzuko.json"),
    r#"{
      "name": "Icon Game",
      "icons": {
        "macos": [ { "path": "./icons/128.png", "size": 128 } ],
        "linux": [ { "path": "./icons/128.png", "size": 128 } ]
      }
    }"#,
  );
  let config = load_package(&dir.join("src/main.ts"), None).unwrap();
  // A `.app` is signed with `codesign`, which exists only on macOS.
  let icon_app = dir.join("Icon.app");
  if cfg!(target_os = "macos") {
    package(&config, Target::Macos, &bin(), &icon_app).unwrap();
    let icns =
      fs::read(icon_app.join("Contents/Resources/AppIcon.icns")).unwrap();
    assert!(
      icns.windows(15).any(|window| window == b"PNG-128-FIXTURE"),
      "icns missing png"
    );
  }
  let linux_icons = dir.join("linux-pngs");
  package(
    &config,
    Target::Linux,
    &dir.join("stand-in.bin"),
    &linux_icons,
  )
  .unwrap();
  assert_eq!(
    fs::read(linux_icons.join("icons/128.png")).unwrap(),
    b"PNG-128-FIXTURE"
  );
  if cfg!(target_os = "macos") {
    let icon_plist =
      fs::read_to_string(icon_app.join("Contents/Info.plist")).unwrap();
    assert!(icon_plist.contains("CFBundleIconFile"), "{icon_plist}");
  }

  let named = dir.join("My Game");
  write(&named.join("src/main.ts"), "console.log(1)\n");
  write(&named.join("yuzuko.json"), "{ \"version\": \"1.0.0\", }\n");
  let loaded = load_package(&named.join("src/main.ts"), None);
  assert!(loaded.is_err(), "strict json rejects a trailing comma");
  write(&named.join("yuzuko.json"), "{}\n");
  let config = load_package(&named.join("src/main.ts"), None).unwrap();
  assert_eq!(config.name, "My Game");
  assert_eq!(config.version, "0.0.0");
  assert_eq!(config.identifier, "com.yuzuko.my-game");
  let defaults = dir.join("defaults-win");
  package(
    &config,
    Target::Windows,
    &dir.join("stand-in.bin"),
    &defaults,
  )
  .unwrap();
  assert_eq!(
    fs::read_to_string(defaults.join("AppUserModelID")).unwrap(),
    "com.yuzuko.my-game"
  );
  let manifest =
    fs::read_to_string(defaults.join("yuzuko.bundle.json")).unwrap();
  assert!(manifest.contains("\"name\": \"My Game\""), "{manifest}");
  assert!(manifest.contains("\"version\": \"0.0.0\""), "{manifest}");
  let defaults_linux = dir.join("defaults-linux");
  package(
    &config,
    Target::Linux,
    &dir.join("stand-in.bin"),
    &defaults_linux,
  )
  .unwrap();
  assert!(defaults_linux.join("com.yuzuko.my-game.desktop").is_file());

  write(&dir.join("icons/app.icns"), "ICNS-FILE-FIXTURE");
  write(
    &dir.join("yuzuko.json"),
    r#"{
      "name": "Icon File",
      "icons": { "macos": "./icons/app.icns" }
    }"#,
  );
  if !cfg!(target_os = "macos") {
    return;
  }
  let config = load_package(&dir.join("src/main.ts"), None).unwrap();
  let icns_app = dir.join("Icns.app");
  package(&config, Target::Macos, &bin(), &icns_app).unwrap();
  assert_eq!(
    fs::read(icns_app.join("Contents/Resources/AppIcon.icns")).unwrap(),
    b"ICNS-FILE-FIXTURE"
  );
  let icns_plist =
    fs::read_to_string(icns_app.join("Contents/Info.plist")).unwrap();
  assert!(
    icns_plist.contains("<key>CFBundleName</key><string>Icon File</string>"),
    "{icns_plist}"
  );
  assert!(icns_plist.contains("CFBundleIconFile"), "{icns_plist}");
}

/// An already-GUI PE still has its icon checked. A subsystem change is not
/// required. A stand-in that is not a PE is covered above and is not parsed.
#[test]
fn windows_icon_is_checked_for_every_pe() {
  let dir = scratch("pe-icon");
  write(&dir.join("src/main.ts"), "console.log(1)\n");
  write(&dir.join("icons/app.ico"), "ICO-BYTES");
  write(
    &dir.join("yuzuko.json"),
    r#"{
      "name": "Pack Game",
      "identifier": "com.example.game2",
      "icons": { "windows": "./icons/app.ico" }
    }"#,
  );
  let config = load_package(&dir.join("src/main.ts"), None).unwrap();
  let runtime = dir.join("runtime.exe");

  fs::write(&runtime, pe_image(0x20b, 2)).unwrap();
  let bad = dir.join("bad-out");
  let error = package(&config, Target::Windows, &runtime, &bad).unwrap_err();
  assert!(error.to_string().contains("not a valid .ico"), "{error}");
  assert!(!bad.exists());
  assert!(!dir.join(".bad-out.partial").exists());

  fs::write(&runtime, pe_image(0x10b, 3)).unwrap();
  let bad_console = dir.join("bad-console");
  let error =
    package(&config, Target::Windows, &runtime, &bad_console).unwrap_err();
  assert!(error.to_string().contains("not a valid .ico"), "{error}");
  assert!(!bad_console.exists());

  // The resource API needs a complete executable. A synthetic image is only
  // embedded on Windows by the launch test, which copies the real runtime.
  if cfg!(windows) {
    return;
  }
  let ico = one_pixel_ico();
  fs::write(dir.join("icons/app.ico"), &ico).unwrap();
  let already_gui = pe_image(0x20b, 2);
  fs::write(&runtime, &already_gui).unwrap();
  let gui_out = dir.join("gui-out");
  package(&config, Target::Windows, &runtime, &gui_out).unwrap();
  let gui_exe = gui_out.join("pack-game.exe");
  assert_eq!(fs::read(gui_out.join("AppIcon.ico")).unwrap(), ico);
  assert_eq!(fs::read(&gui_exe).unwrap(), already_gui);

  fs::write(&runtime, pe_image(0x10b, 3)).unwrap();
  let console_out = dir.join("console-out");
  package(&config, Target::Windows, &runtime, &console_out).unwrap();
  let mut expected = pe_image(0x10b, 2);
  expected[0xd8..0xdc].fill(0);
  assert_eq!(
    fs::read(console_out.join("pack-game.exe")).unwrap(),
    expected
  );
}

/// PE32 or PE32+ with `e_lfanew` at `0x80`, a checksum of `0x1234`, and the
/// given subsystem. `2` is GUI and `3` is console.
fn pe_image(magic: u16, subsystem: u16) -> Vec<u8> {
  let mut image = vec![0u8; 0x200];
  image[0..2].copy_from_slice(b"MZ");
  image[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
  image[0x80..0x84].copy_from_slice(b"PE\0\0");
  image[0x94..0x96].copy_from_slice(&240u16.to_le_bytes());
  image[0x98..0x9a].copy_from_slice(&magic.to_le_bytes());
  image[0xd8..0xdc].copy_from_slice(&0x1234u32.to_le_bytes());
  image[0xdc..0xde].copy_from_slice(&subsystem.to_le_bytes());
  image
}

struct Launch {
  status: i32,
  stdout: String,
  stderr: String,
  cwd: String,
}

fn launch(exe: &Path, cwd: &Path, args: &[&str]) -> Launch {
  let mut child = Command::new(exe)
    .current_dir(cwd)
    .args(args)
    .env_remove("YUZUKO_HEADLESS")
    .env_remove("YUZUKO_SKIP_GPU")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn bundle");
  std::thread::sleep(Duration::from_millis(300));
  let cwd_text = process_cwd(child.id());
  let start = Instant::now();
  let output = loop {
    if let Some(status) = child.try_wait().unwrap() {
      let output = child.wait_with_output().unwrap();
      break (status.code().unwrap_or(1), output);
    }
    if start.elapsed() > Duration::from_secs(30) {
      let _ = child.kill();
      panic!("bundle did not exit");
    }
    std::thread::sleep(Duration::from_millis(50));
  };
  Launch {
    status: output.0,
    stdout: text(&output.1.stdout),
    stderr: text(&output.1.stderr),
    cwd: cwd_text,
  }
}

fn one_pixel_png() -> Vec<u8> {
  use base64::Engine;
  base64::prelude::BASE64_STANDARD
    .decode(
      "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
    )
    .unwrap()
}

/// An `.ico` whose one image is a PNG, as Windows Vista and later accept.
fn one_pixel_ico() -> Vec<u8> {
  let png = one_pixel_png();
  let mut ico = vec![0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 32, 0];
  ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
  ico.extend_from_slice(&22u32.to_le_bytes());
  ico.extend_from_slice(&png);
  ico
}

#[cfg(windows)]
fn icon_count(exe: &Path) -> u32 {
  use std::os::windows::ffi::OsStrExt;
  use windows_sys::Win32::UI::Shell::ExtractIconExW;
  let path: Vec<u16> = exe
    .as_os_str()
    .encode_wide()
    .chain(std::iter::once(0))
    .collect();
  // -1 asks for the count rather than an icon.
  unsafe {
    ExtractIconExW(
      path.as_ptr(),
      -1,
      std::ptr::null_mut(),
      std::ptr::null_mut(),
      0,
    )
  }
}

/// `""` when this OS does not expose another process's directory.
fn process_cwd(pid: u32) -> String {
  if cfg!(target_os = "macos") {
    Command::new("lsof")
      .args(["-a", "-p", &pid.to_string(), "-d", "cwd"])
      .output()
      .map(|output| text(&output.stdout))
      .unwrap_or_default()
  } else if cfg!(target_os = "linux") {
    fs::read_link(format!("/proc/{pid}/cwd"))
      .map(|path| path.to_string_lossy().into_owned())
      .unwrap_or_default()
  } else {
    String::new()
  }
}

#[cfg(target_os = "macos")]
fn open_app(app: &Path) -> (i32, String) {
  let mut child = Command::new("open")
    .args(["-W", app.to_str().unwrap()])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("open");
  let start = Instant::now();
  loop {
    if let Some(status) = child.try_wait().unwrap() {
      let output = child.wait_with_output().unwrap();
      let combined =
        format!("{}{}", text(&output.stdout), text(&output.stderr));
      return (status.code().unwrap_or(1), combined);
    }
    if start.elapsed() > Duration::from_secs(30) {
      let _ = child.kill();
      return (1, "open timed out".to_string());
    }
    std::thread::sleep(Duration::from_millis(50));
  }
}
