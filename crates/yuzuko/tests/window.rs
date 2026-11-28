use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn workspace() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn yuzuko(args: &[&str]) -> std::process::Output {
  Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(args)
    .env_remove("YUZUKO_HEADLESS")
    .output()
    .expect("spawn yuzuko")
}

fn text(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn headless_env_exits_when_the_last_window_closes() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/close_last_surface.ts"])
    .env("YUZUKO_HEADLESS", "1")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout), "both\n");
}

#[test]
fn omitted_constructor_size_is_1280_by_720() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/print_size.ts"])
    .env("YUZUKO_HEADLESS", "1")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(text(&output.stdout), "1280x720\n10x11\n");
}

#[test]
fn run_rejects_removed_window_flags() {
  let headless =
    yuzuko(&["run", "--headless", "tests/fixtures/close_last_surface.ts"]);
  assert_eq!(headless.status.code(), Some(2));
  assert!(
    text(&headless.stderr).contains("unknown flag --headless"),
    "{}",
    text(&headless.stderr)
  );
  let size =
    yuzuko(&["run", "--size", "400x250", "tests/fixtures/print_size.ts"]);
  assert_eq!(size.status.code(), Some(2));
  assert!(
    text(&size.stderr).contains("unknown flag --size"),
    "{}",
    text(&size.stderr)
  );
}

#[test]
fn input_sequences_pass_under_yuzuko_test() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "-A", "tests/window_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env("YUZUKO_TEST_INPUT", "1")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | 15 passed | 0 failed | 0 ignored"),
    "{stdout}\n{}",
    text(&output.stderr)
  );
}

#[test]
fn trusted_touch_events_are_dispatched() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "-A", "tests/touch_dispatch_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env("YUZUKO_TEST_INPUT", "1")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | 2 passed | 0 failed | 0 ignored"),
    "{stdout}\n{}",
    text(&output.stderr)
  );
}

#[test]
fn event_handler_attributes_pass_under_yuzuko_test() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "-A", "tests/event_handler_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env("YUZUKO_TEST_INPUT", "1")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | 2 passed | 0 failed | 0 ignored"),
    "{stdout}\n{}",
    text(&output.stderr)
  );
}

#[test]
fn touch_events_follow_the_constructor() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "-A", "tests/touch_event_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | 3 passed | 0 failed | 0 ignored"),
    "{stdout}\n{}",
    text(&output.stderr)
  );
}

#[test]
fn pointer_event_idl_matches() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "-A", "tests/pointer_idl_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | 7 passed | 0 failed | 0 ignored"),
    "{stdout}\n{}",
    text(&output.stderr)
  );
}

#[test]
fn legacy_event_initializers_match() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "-A", "tests/legacy_init_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | 7 passed | 0 failed | 0 ignored"),
    "{stdout}\n{}",
    text(&output.stderr)
  );
}

#[test]
fn pointer_and_touch_init_match() {
  let output = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["test", "-A", "tests/pointer_touch_init_test.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .output()
    .expect("spawn yuzuko");
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  assert!(
    stdout.contains("ok | 3 passed | 0 failed | 0 ignored"),
    "{stdout}\n{}",
    text(&output.stderr)
  );
}

#[test]
fn opens_an_os_window() {
  let mut child = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/show_window.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  let start = Instant::now();
  let status = loop {
    if let Some(status) = child.try_wait().expect("poll window") {
      break status;
    }
    if start.elapsed() > Duration::from_secs(20) {
      let _ = child.kill();
      panic!("OS window did not close");
    }
    std::thread::sleep(Duration::from_millis(30));
  };
  let output = child.wait_with_output().expect("window output");
  assert!(status.success(), "{}", text(&output.stderr));
  let stdout = text(&output.stdout);
  let mut lines = stdout.lines();
  assert_eq!(lines.next(), Some("320x200"), "{stdout}");
  let surface = lines.next().unwrap_or("");
  let physical = lines.next().unwrap_or("");
  assert_eq!(surface, physical, "{stdout}");
  assert_eq!(lines.next(), Some("true"), "{stdout}");
  let delta = lines.next().unwrap_or("");
  let mut parts = delta.split(',');
  let dx: f64 = parts.next().unwrap_or("").parse().unwrap_or(999.0);
  let dy: f64 = parts.next().unwrap_or("").parse().unwrap_or(999.0);
  assert!(
    (dx - 30.0).abs() < 2.0 && (dy + 10.0).abs() < 2.0,
    "{stdout}"
  );
  assert_eq!(lines.next(), Some("true"), "{stdout}");
  assert_eq!(lines.next(), Some("true"), "{stdout}");
  assert_eq!(lines.next(), Some("true"), "{stdout}");
  assert_eq!(lines.next(), Some("renamed"), "{stdout}");
  assert_eq!(lines.next(), Some("false"), "{stdout}");
  assert_eq!(lines.next(), Some("true"), "{stdout}");
  assert_eq!(lines.next(), Some("330x180"), "{stdout}");
  assert!(stdout.contains("window-closed\n"), "{stdout}");
}

/// Moves one message from `from` to `to`, file descriptors included, as a
/// Wayland connection needs. `false` when `from` closed or a call failed.
#[cfg(target_os = "linux")]
fn relay_message(from: std::os::fd::RawFd, to: std::os::fd::RawFd) -> bool {
  const FD: usize = std::mem::size_of::<libc::c_int>();
  let mut data = [0u8; 8192];
  let mut control = [0u64; 64];
  let mut iov = libc::iovec {
    iov_base: data.as_mut_ptr().cast(),
    iov_len: data.len(),
  };
  // Safety: the header points at `iov` and `control`, which outlive the
  // calls, and each length is that buffer's size.
  unsafe {
    let mut message: libc::msghdr = std::mem::zeroed();
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = std::mem::size_of_val(&control) as _;
    let read = libc::recvmsg(from, &mut message, libc::MSG_CMSG_CLOEXEC);
    if read <= 0 {
      return false;
    }
    let mut fds = Vec::new();
    let mut header = libc::CMSG_FIRSTHDR(&message);
    while !header.is_null() {
      if (*header).cmsg_level == libc::SOL_SOCKET
        && (*header).cmsg_type == libc::SCM_RIGHTS
      {
        let count =
          ((*header).cmsg_len as usize - libc::CMSG_LEN(0) as usize) / FD;
        let first = libc::CMSG_DATA(header).cast::<libc::c_int>();
        for index in 0..count {
          fds.push(first.add(index).read_unaligned());
        }
      }
      header = libc::CMSG_NXTHDR(&message, header);
    }
    let mut out_iov = libc::iovec {
      iov_base: data.as_mut_ptr().cast(),
      iov_len: read as usize,
    };
    let mut out_control = [0u64; 64];
    let mut out: libc::msghdr = std::mem::zeroed();
    out.msg_iov = &mut out_iov;
    out.msg_iovlen = 1;
    if !fds.is_empty() {
      out.msg_control = out_control.as_mut_ptr().cast();
      out.msg_controllen = libc::CMSG_SPACE((fds.len() * FD) as u32) as _;
      let header = libc::CMSG_FIRSTHDR(&out);
      (*header).cmsg_level = libc::SOL_SOCKET;
      (*header).cmsg_type = libc::SCM_RIGHTS;
      (*header).cmsg_len = libc::CMSG_LEN((fds.len() * FD) as u32) as _;
      let first = libc::CMSG_DATA(header).cast::<libc::c_int>();
      for (index, fd) in fds.iter().enumerate() {
        first.add(index).write_unaligned(*fd);
      }
    }
    let sent = libc::sendmsg(to, &out, libc::MSG_NOSIGNAL);
    for fd in fds {
      libc::close(fd);
    }
    sent == read
  }
}

/// Relays one client to the compositor socket `upstream` and closes both
/// ends once `cut` has passed, which is what a compositor exit looks like
/// to the client.
#[cfg(target_os = "linux")]
fn cut_wayland_after(
  listener: std::os::unix::net::UnixListener,
  upstream: PathBuf,
  cut: Duration,
) -> std::thread::JoinHandle<()> {
  use std::os::fd::AsRawFd;
  std::thread::spawn(move || {
    let Ok((client, _)) = listener.accept() else {
      return;
    };
    let Ok(server) = std::os::unix::net::UnixStream::connect(&upstream) else {
      return;
    };
    let deadline = Instant::now() + cut;
    let (client_fd, server_fd) = (client.as_raw_fd(), server.as_raw_fd());
    while Instant::now() < deadline {
      let mut polls = [
        libc::pollfd {
          fd: client_fd,
          events: libc::POLLIN,
          revents: 0,
        },
        libc::pollfd {
          fd: server_fd,
          events: libc::POLLIN,
          revents: 0,
        },
      ];
      // Safety: `polls` is a live array of two `pollfd`.
      if unsafe { libc::poll(polls.as_mut_ptr(), 2, 20) } < 0 {
        return;
      }
      if polls[0].revents != 0 && !relay_message(client_fd, server_fd) {
        return;
      }
      if polls[1].revents != 0 && !relay_message(server_fd, client_fd) {
        return;
      }
    }
    let _ = client.shutdown(std::net::Shutdown::Both);
  })
}

/// Runs only where a Wayland compositor is reachable. CI uses Xvfb and
/// skips it.
#[cfg(target_os = "linux")]
#[test]
fn a_lost_wayland_connection_closes_windows_without_spinning() {
  let Some(display) = std::env::var_os("WAYLAND_DISPLAY") else {
    eprintln!("skipped: WAYLAND_DISPLAY is not set");
    return;
  };
  let display = PathBuf::from(display);
  let upstream = if display.is_absolute() {
    display
  } else {
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else {
      eprintln!("skipped: XDG_RUNTIME_DIR is not set");
      return;
    };
    PathBuf::from(runtime).join(display)
  };
  if !upstream.exists() {
    eprintln!("skipped: {} does not exist", upstream.display());
    return;
  }
  let socket = std::env::temp_dir()
    .join(format!("yuzuko-wayland-cut-{}", std::process::id()));
  let _ = std::fs::remove_file(&socket);
  let listener =
    std::os::unix::net::UnixListener::bind(&socket).expect("bind relay");
  let relay = cut_wayland_after(listener, upstream, Duration::from_secs(4));
  let mut child = Command::new(env!("CARGO_BIN_EXE_yuzuko"))
    .current_dir(workspace())
    .args(["run", "tests/fixtures/connection_lost.ts"])
    .env_remove("YUZUKO_HEADLESS")
    .env_remove("DISPLAY")
    .env("WAYLAND_DISPLAY", &socket)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn yuzuko");
  let start = Instant::now();
  while child.try_wait().expect("poll yuzuko").is_none() {
    if start.elapsed() > Duration::from_secs(40) {
      let _ = child.kill();
      panic!("yuzuko kept running after the connection was lost");
    }
    std::thread::sleep(Duration::from_millis(30));
  }
  let output = child.wait_with_output().expect("yuzuko output");
  let _ = relay.join();
  let _ = std::fs::remove_file(&socket);
  let stdout = text(&output.stdout);
  let stderr = text(&output.stderr);
  assert_eq!(output.status.code(), Some(0), "{stdout}\n{stderr}");
  assert_eq!(
    stdout,
    "opened\nclose\nTypeError The connection to the window system was lost.\ntimer\n",
    "{stderr}"
  );
  assert_eq!(
    stderr
      .matches("error: The connection to the window system was lost")
      .count(),
    1,
    "{stderr}"
  );
  // Before the fix the backend printed one line per turn, without bound.
  assert!(stderr.len() < 64 * 1024, "stderr is {} bytes", stderr.len());
}
