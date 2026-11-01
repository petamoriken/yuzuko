use std::path::PathBuf;
use std::process::Command;

#[cfg(feature = "fs")]
use std::fs;
#[cfg(feature = "fs")]
use std::io::Read;
#[cfg(feature = "fs")]
use std::process::Stdio;
#[cfg(feature = "fs")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "fs")]
use std::thread;
#[cfg(feature = "fs")]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn types() -> String {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/cli/fs_build.ts"])
    .output()
    .expect("spawn yuzuko");
  assert_eq!(
    output.status.code(),
    Some(0),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[cfg(feature = "fs")]
#[test]
fn fs_feature_installs_the_file_system() {
  assert_eq!(
    types(),
    "function function function function function function function function function object object undefined"
  );
}

#[cfg(not(feature = "fs"))]
#[test]
fn fs_feature_omits_the_file_system() {
  assert_eq!(
    types(),
    "undefined undefined undefined undefined undefined undefined undefined undefined undefined undefined undefined undefined"
  );
}

#[cfg(feature = "fs")]
#[test]
fn fs_feature_round_trip() {
  static NEXT: AtomicU64 = AtomicU64::new(0);
  let tick = NEXT.fetch_add(1, Ordering::Relaxed);
  let nanos = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_nanos())
    .unwrap_or(0);
  let dir = std::env::temp_dir().join(format!(
    "yuzuko-fs-roundtrip-{}-{}-{}",
    std::process::id(),
    nanos,
    tick
  ));
  fs::create_dir_all(&dir).expect("temp dir");
  let _guard = DirGuard(dir.clone());
  fs::write(dir.join("main.js"), MAIN).expect("main.js");
  fs::write(dir.join("worker.js"), WORKER).expect("worker.js");

  let mut command = Command::new(env!("CARGO_BIN_EXE_yuzuko"));
  command.current_dir(&dir).args(["run", "main.js"]);
  let (code, stdout, stderr) =
    output_with_timeout(command, Duration::from_secs(45));
  assert_eq!(
    (code, stdout.trim()),
    (Some(0), "ok"),
    "stdout:\n{stdout}\nstderr:\n{stderr}"
  );
}

#[cfg(feature = "fs")]
fn output_with_timeout(
  mut command: Command,
  timeout: Duration,
) -> (Option<i32>, String, String) {
  let mut child = command
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  let mut stdout_pipe = child.stdout.take().expect("stdout");
  let mut stderr_pipe = child.stderr.take().expect("stderr");
  let stdout_thread = thread::spawn(move || {
    let mut buf = Vec::new();
    let _ = stdout_pipe.read_to_end(&mut buf);
    buf
  });
  let stderr_thread = thread::spawn(move || {
    let mut buf = Vec::new();
    let _ = stderr_pipe.read_to_end(&mut buf);
    buf
  });
  let started = Instant::now();
  let status = loop {
    match child.try_wait() {
      Ok(Some(status)) => break status,
      Ok(None) if started.elapsed() > timeout => {
        let _ = child.kill();
        let status = child.wait().expect("wait after kill");
        let stdout =
          String::from_utf8_lossy(&stdout_thread.join().unwrap_or_default())
            .into_owned();
        let stderr =
          String::from_utf8_lossy(&stderr_thread.join().unwrap_or_default())
            .into_owned();
        panic!(
          "timed out after {timeout:?}: {status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
      }
      Ok(None) => thread::sleep(Duration::from_millis(50)),
      Err(error) => panic!("wait failed: {error}"),
    }
  };
  let stdout =
    String::from_utf8_lossy(&stdout_thread.join().expect("stdout thread"))
      .into_owned();
  let stderr =
    String::from_utf8_lossy(&stderr_thread.join().expect("stderr thread"))
      .into_owned();
  (status.code(), stdout, stderr)
}

#[cfg(feature = "fs")]
struct DirGuard(PathBuf);

#[cfg(feature = "fs")]
impl Drop for DirGuard {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.0);
  }
}

#[cfg(feature = "fs")]
const MAIN: &str = r#"
function check(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

await Yuzuko.writeTextFile("note.txt", "hello");
check((await Yuzuko.readTextFile("note.txt")) === "hello", "readTextFile");
check(Yuzuko.readTextFileSync("note.txt") === "hello", "readTextFileSync");
const bytes = await Yuzuko.readFile("note.txt");
check(bytes.length === 5 && bytes[0] === 104, "readFile");

await Yuzuko.mkdir("sub/nested", { recursive: true });
const file = await Yuzuko.open("sub/nested/data.bin", {
  write: true,
  create: true,
  read: true,
});
const written = await file.write(new Uint8Array([120, 121, 122]));
check(written === 3, "write " + written);
const position = await file.seek(0, Yuzuko.SeekMode.Start);
check(position === 0, "seek " + position);
const view = new Uint8Array(3);
const count = await file.read(view);
check(
  count === 3 && view[0] === 120 && view[1] === 121 && view[2] === 122,
  "read " + count,
);
await file.close();

const info = await Yuzuko.stat("note.txt");
check(info.isFile === true && info.size === 5, "stat");
const linked = await Yuzuko.lstat("note.txt");
check(linked.isSymlink === false, "lstat");

const opened = Yuzuko.openSync("note.txt");
opened.close();
let closed = false;
try {
  opened.readSync(new Uint8Array(1));
} catch (error) {
  closed = error instanceof Yuzuko.errors.BadResource;
}
check(closed, "closed");

if (typeof Symbol.dispose === "symbol") {
  const disposable = Yuzuko.openSync("note.txt");
  disposable[Symbol.dispose]();
  let disposed = false;
  try {
    disposable.readSync(new Uint8Array(1));
  } catch (error) {
    disposed = error instanceof Yuzuko.errors.BadResource;
  }
  check(disposed, "dispose");
}

let exists = false;
try {
  Yuzuko.mkdirSync("sub");
} catch (error) {
  exists = error instanceof Yuzuko.errors.AlreadyExists;
}
check(exists, "mkdir exists");

let directory = false;
try {
  Yuzuko.readFileSync("sub");
} catch (error) {
  directory = error instanceof Yuzuko.errors.IsADirectory;
}
check(directory, "directory");

const names = Yuzuko.readDirSync(".").map((entry) => entry.name);
check(
  names.includes("note.txt") && names.includes("sub"),
  "readDir " + names.join(","),
);

await Yuzuko.copyFile("note.txt", "copy.txt");
await Yuzuko.rename("copy.txt", "moved.txt");
Yuzuko.truncateSync("moved.txt", 2);
check(Yuzuko.readTextFileSync("moved.txt") === "he", "truncate");
check(Yuzuko.realPathSync("moved.txt").length > 0, "realPath");

const temp = Yuzuko.makeTempFileSync({ prefix: "yz-", suffix: ".txt" });
check(temp.includes("yz-") && temp.endsWith(".txt"), "temp " + temp);
Yuzuko.removeSync(temp);

Yuzuko.removeSync("sub", { recursive: true });
Yuzuko.removeSync("moved.txt");
let missing = false;
try {
  Yuzuko.statSync("moved.txt");
} catch (error) {
  missing = error instanceof Yuzuko.errors.NotFound;
}
check(missing, "missing");

await Yuzuko.writeTextFile("note.txt", "hello");
const worker = new Worker(new URL("./worker.js", import.meta.url), {
  type: "module",
});
try {
  const reply = await new Promise((resolve, reject) => {
    worker.onmessage = (event) => resolve(event.data);
    worker.onerror = (event) => {
      reject(new Error(String(event.message || "worker error")));
    };
  });
  check(reply === "hello", "worker " + reply);
} finally {
  worker.terminate();
}

Yuzuko.removeSync("note.txt");
check(typeof Yuzuko.chmod === "undefined", "chmod");
console.log("ok");
"#;

#[cfg(feature = "fs")]
const WORKER: &str = r#"
try {
  const text = await Yuzuko.readTextFile("note.txt");
  postMessage(text);
} catch (error) {
  postMessage("error:" + error.name + ":" + error.message);
}
"#;
