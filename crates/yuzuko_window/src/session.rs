//! Process-wide headless mode from `yuzuko test` and `YUZUKO_HEADLESS`.

use std::sync::atomic::{AtomicBool, Ordering};

const DEFAULT_WIDTH: f64 = 1280.0;
const DEFAULT_HEIGHT: f64 = 720.0;

static HEADLESS: AtomicBool = AtomicBool::new(false);

fn env_headless() -> bool {
  std::env::var("YUZUKO_HEADLESS").ok().as_deref() == Some("1")
}

pub fn configure(headless: bool) {
  HEADLESS.store(headless || env_headless(), Ordering::SeqCst);
}

pub fn headless() -> bool {
  HEADLESS.load(Ordering::SeqCst) || env_headless()
}

pub fn default_size() -> (f64, f64) {
  (DEFAULT_WIDTH, DEFAULT_HEIGHT)
}
