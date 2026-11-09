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
    "function function function function function function function function function object object function"
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
  command.current_dir(&dir).args(["run", "-A", "main.js"]);
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
const syncBytes = Yuzuko.readFileSync("note.txt");
check(syncBytes.length === 5 && syncBytes[0] === 104, "readFileSync");

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

const previousMask = Yuzuko.umask(0o077);
const duringMask = Yuzuko.umask(previousMask);
const unix = duringMask === 0o077;

await Yuzuko.writeTextFile("mode.txt", "m");
Yuzuko.chmodSync("mode.txt", 0o644);
await Yuzuko.chmod("mode.txt", 0o644);
if (unix) {
  const mode = Yuzuko.statSync("mode.txt").mode & 0o777;
  check(mode === 0o644, "chmod " + mode.toString(8));
}
Yuzuko.removeSync("mode.txt");

if (unix) {
  const info = Yuzuko.statSync("note.txt");
  Yuzuko.chownSync("note.txt", null, null);
  Yuzuko.chownSync("note.txt", info.uid, info.gid);
} else {
  let owner = false;
  try {
    Yuzuko.chownSync("missing-owner.txt", null, null);
  } catch (error) {
    owner = error instanceof Yuzuko.errors.NotSupported
      && error.message === "Changing the owner is not supported on Windows.";
  }
  check(owner, "chown");
}

let badMask = false;
try {
  Yuzuko.umask(null);
} catch (error) {
  badMask = error instanceof TypeError;
}
check(badMask, "umask");

Yuzuko.writeTextFileSync("link-src.txt", "L");
let hardLinked = false;
try {
  Yuzuko.linkSync("link-src.txt", "link-dst.txt");
  hardLinked = Yuzuko.readTextFileSync("link-dst.txt") === "L";
} catch (error) {
  hardLinked = !unix && error instanceof Yuzuko.errors.PermissionDenied;
}
check(hardLinked, "link");

let badType = false;
try {
  Yuzuko.symlinkSync("link-src.txt", "bad-link", { type: "nope" });
} catch (error) {
  badType = error instanceof TypeError;
}
check(badType, "link type");

let symlinked = false;
try {
  Yuzuko.symlinkSync("link-src.txt", "link-sym");
  symlinked = Yuzuko.readLinkSync("link-sym") === "link-src.txt";
  if (symlinked) {
    symlinked = (await Yuzuko.readLink("link-sym")) === "link-src.txt";
  }
} catch (error) {
  symlinked = !unix && error instanceof Yuzuko.errors.PermissionDenied;
}
check(symlinked, "symlink");

let badTime = false;
try {
  Yuzuko.utimeSync("note.txt", "1700000000", 0);
} catch (error) {
  badTime = error instanceof TypeError
    && error.message === "The time must be a Date or a finite number.";
}
check(badTime, "utime type");

let outOfRange = false;
try {
  Yuzuko.utimeSync("note.txt", 1e20, 0);
} catch (error) {
  outOfRange = error instanceof TypeError
    && error.message === "The time is out of range.";
}
check(outOfRange, "utime range");

const stamp = new Date(1700000000000);
Yuzuko.utimeSync("note.txt", stamp, 1700000000);
const timed = Yuzuko.statSync("note.txt");
check(
  timed.mtime instanceof Date
    && Math.abs(timed.mtime.getTime() - 1700000000000) < 2000,
  "utime",
);
const timedFile = Yuzuko.openSync("note.txt");
timedFile.utimeSync(1700000000, stamp);
timedFile.close();

const locked = Yuzuko.openSync("note.txt", { read: true, write: true });
const tried = locked.tryLockSync();
check(tried === true || tried === false, "tryLock");
if (tried) {
  locked.unlockSync();
}
await locked.lock(true);
await locked.unlock();
check(locked.isTerminal() === false, "isTerminal");
let raw = false;
try {
  locked.setRaw(true);
} catch (error) {
  raw = error instanceof Yuzuko.errors.NotSupported
    && error.message === "The file is not a terminal.";
}
check(raw, "setRaw");
locked.setRaw(false);

let readableThis = false;
try {
  Object.getOwnPropertyDescriptor(
    Yuzuko.FsFile.prototype,
    "readable",
  ).get.call(undefined);
} catch (error) {
  readableThis = error instanceof TypeError;
}
check(readableThis, "readable this");

check(locked.writable === locked.writable, "writable");
const writer = locked.writable.getWriter();
await writer.write(new Uint8Array([33]));
await writer.close();
check(locked.readable === locked.readable, "readable");
const reader = locked.readable.getReader();
const chunk = await reader.read();
const streamBytes = chunk.value;
check(
  chunk.done === false
    && streamBytes
    && streamBytes.length === 4
    && streamBytes[0] === 101
    && streamBytes[1] === 108
    && streamBytes[2] === 108
    && streamBytes[3] === 111,
  "stream",
);
await reader.cancel();
const tail = locked.readSync(new Uint8Array(1));
check(tail === null, "stream left the file open");
locked.close();

let constructed = false;
try {
  new Yuzuko.FsWatcher();
} catch (error) {
  constructed = error instanceof TypeError;
}
check(constructed, "FsWatcher");

let invoked = false;
try {
  Yuzuko.FsWatcher.prototype[Symbol.asyncIterator].call({});
} catch (error) {
  invoked = error instanceof TypeError;
}
check(invoked, "asyncIterator");

function nextEvent(watcher) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(function () {
      reject(new Error("watch timed out"));
    }, 10000);
    watcher.next().then(function (value) {
      clearTimeout(timer);
      resolve(value);
    }, function (error) {
      clearTimeout(timer);
      reject(error);
    });
  });
}

Yuzuko.mkdirSync("watched");
const watcher = Yuzuko.watchFs("watched");
try {
  check(watcher[Symbol.asyncIterator]() === watcher, "iterator");
  Yuzuko.writeTextFileSync("watched/child.txt", "w");
  const event = await nextEvent(watcher);
  const kind = event.value && event.value.kind;
  const paths = event.value && event.value.paths;
  const known = kind === "access" || kind === "create" || kind === "modify"
    || kind === "remove" || kind === "rename" || kind === "any"
    || kind === "other";
  check(
    event.done === false && known
      && (kind === "any" || (paths && paths.length > 0)),
    "watch " + JSON.stringify(event.value),
  );
} finally {
  watcher.close();
}

const quiet = Yuzuko.watchFs("watched");
try {
  const done = await quiet.return();
  check(done.done === true, "return");
  const after = await nextEvent(quiet);
  check(after.done === true, "next after return");
} finally {
  let watcherClosed = false;
  try {
    quiet.close();
  } catch (error) {
    watcherClosed = error instanceof Yuzuko.errors.BadResource
      && error.message === "The watcher is closed.";
  }
  check(watcherClosed, "watcher closed");
}

let badPaths = false;
try {
  Yuzuko.watchFs([]);
} catch (error) {
  badPaths = error instanceof TypeError;
}
check(badPaths, "watch paths");

Yuzuko.removeSync("note.txt");
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
