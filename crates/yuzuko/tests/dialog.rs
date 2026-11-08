use std::path::PathBuf;
use std::process::Command;
#[cfg(any(target_os = "macos", windows))]
use std::process::Stdio;
#[cfg(any(target_os = "macos", windows))]
use std::time::{Duration, Instant};

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn headless_dialogs_throw_not_supported() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "-A", "tests/dialog_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .output()
    .expect("spawn yuzuko");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stdout}\n{stderr}");
  assert!(
    stdout.contains("ok | 2 passed | 0 failed | 0 ignored"),
    "{stdout}\n{stderr}"
  );
}

#[cfg(any(target_os = "macos", windows))]
fn native_dialog_lock() -> std::sync::MutexGuard<'static, ()> {
  static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
  LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn dialogs_accept_the_default_button() {
  let _guard = native_dialog_lock();
  let mut child = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/dialog_alert.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env_remove("YUZUKO_SKIP_GPU")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  let start = Instant::now();
  let status = loop {
    if let Some(status) = child.try_wait().expect("poll dialog") {
      break status;
    }
    if start.elapsed() > Duration::from_secs(12) {
      let _ = child.kill();
      panic!("dialog did not close");
    }
    #[cfg(target_os = "macos")]
    press_return();
    #[cfg(windows)]
    press_default(child.id());
    std::thread::sleep(Duration::from_millis(200));
  };
  let output = child.wait_with_output().expect("dialog output");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(status.success(), "{stdout}\n{stderr}");
  assert_eq!(stdout, "yes\nAda\nafter\n", "{stderr}");
}

/// Cancels the open and save dialogs. Escape on macOS, `IDCANCEL` on the
/// common-dialog class on Windows. The default button is OK, so this does
/// not reuse `press_default`.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn file_picker_cancel_returns_null() {
  let _guard = native_dialog_lock();
  let mut child = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/file_picker.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env_remove("YUZUKO_SKIP_GPU")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  let start = Instant::now();
  let status = loop {
    if let Some(status) = child.try_wait().expect("poll file picker") {
      break status;
    }
    if start.elapsed() > Duration::from_secs(12) {
      let _ = child.kill();
      panic!("file picker did not close");
    }
    #[cfg(target_os = "macos")]
    press_escape();
    #[cfg(windows)]
    press_cancel(child.id());
    std::thread::sleep(Duration::from_millis(200));
  };
  let output = child.wait_with_output().expect("file picker output");
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(status.success(), "{stdout}\n{stderr}");
  assert_eq!(stdout, "open-cancel\nsave-cancel\n", "{stderr}");
}

#[cfg(target_os = "macos")]
fn system_events(script: &str) -> bool {
  Command::new("osascript")
    .arg("-e")
    .arg(script)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .is_ok_and(|status| status.success())
}

/// The panels take the key only while `yuzuko` is front. A failed
/// activation sends nothing, so the key does not land in another app.
#[cfg(target_os = "macos")]
fn send_when_front(script: &str) {
  let front = "tell application \"System Events\" \
    to set frontmost of first process whose name is \"yuzuko\" to true";
  if system_events(front) {
    let _ = system_events(script);
  }
}

#[cfg(target_os = "macos")]
fn press_return() {
  send_when_front("tell application \"System Events\" to keystroke return");
}

#[cfg(target_os = "macos")]
fn press_escape() {
  send_when_front("tell application \"System Events\" to key code 53");
}

/// Presses the default button of each dialog the child process shows,
/// without typing into another window. A `MessageBoxW` has the `#32770`
/// class. Its default button is the child with `BS_DEFPUSHBUTTON`, and the
/// command carries that button's own ID: an `MB_OK` box names its OK button
/// `IDCANCEL` and ignores `IDOK`. The runtime prompt has `YuzukoPrompt` and
/// takes `IDOK`.
#[cfg(windows)]
fn press_default(pid: u32) {
  use windows_sys::Win32::Foundation::{HWND, LPARAM};
  use windows_sys::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, EnumChildWindows, EnumWindows, GWL_STYLE, GetClassNameW,
    GetDlgCtrlID, GetWindowLongW, GetWindowThreadProcessId, IDOK,
    IsWindowVisible, PostMessageW, WM_COMMAND,
  };
  fn class_of(hwnd: HWND) -> String {
    let mut class = [0u16; 64];
    let length =
      unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    String::from_utf16_lossy(&class[..length.max(0) as usize])
  }
  unsafe extern "system" fn default_button(child: HWND, found: LPARAM) -> i32 {
    let style = unsafe { GetWindowLongW(child, GWL_STYLE) };
    if class_of(child) == "Button" && style & 0xf == BS_DEFPUSHBUTTON {
      unsafe { *(found as *mut i32) = GetDlgCtrlID(child) };
      return 0;
    }
    1
  }
  unsafe extern "system" fn visit(hwnd: HWND, pid: LPARAM) -> i32 {
    let mut owner = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut owner) };
    if owner as LPARAM != pid || unsafe { IsWindowVisible(hwnd) } == 0 {
      return 1;
    }
    let class = class_of(hwnd);
    if class == "#32770" {
      let mut id = 0i32;
      unsafe {
        EnumChildWindows(
          hwnd,
          Some(default_button),
          (&mut id as *mut i32) as LPARAM,
        )
      };
      if id != 0 {
        unsafe { PostMessageW(hwnd, WM_COMMAND, id as usize, 0) };
      }
    } else if class == "YuzukoPrompt" {
      unsafe { PostMessageW(hwnd, WM_COMMAND, IDOK as usize, 0) };
    }
    1
  }
  unsafe { EnumWindows(Some(visit), pid as LPARAM) };
}

/// Posts `IDCANCEL` to a common file dialog (`#32770`). `MessageBoxW` is the
/// same class, and its OK button can be `IDCANCEL`, so this helper stays on
/// the file-picker test.
#[cfg(windows)]
fn press_cancel(pid: u32) {
  use windows_sys::Win32::Foundation::{HWND, LPARAM};
  use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowThreadProcessId, IDCANCEL,
    IsWindowVisible, PostMessageW, WM_COMMAND,
  };
  fn class_of(hwnd: HWND) -> String {
    let mut class = [0u16; 64];
    let length =
      unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    String::from_utf16_lossy(&class[..length.max(0) as usize])
  }
  unsafe extern "system" fn visit(hwnd: HWND, pid: LPARAM) -> i32 {
    let mut owner = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut owner) };
    if owner as LPARAM != pid || unsafe { IsWindowVisible(hwnd) } == 0 {
      return 1;
    }
    if class_of(hwnd) == "#32770" {
      unsafe { PostMessageW(hwnd, WM_COMMAND, IDCANCEL as usize, 0) };
    }
    1
  }
  unsafe { EnumWindows(Some(visit), pid as LPARAM) };
}
