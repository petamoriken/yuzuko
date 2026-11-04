//! Button labels for the dialogs this runtime draws.
//!
//! The words are not taken from Intl or ICU. `Yuzuko.setDialogLabels` stores
//! them, and each dialog reads the store when it opens. Windows `MessageBox`
//! and the system file dialogs do not read it.

use std::sync::Mutex;

/// Visible labels. The initial values are the English words the dialogs used
/// before an application called `setDialogLabels`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Labels {
  pub ok: String,
  pub cancel: String,
  pub open: String,
  pub save: String,
  pub select: String,
}

impl Default for Labels {
  fn default() -> Self {
    Self {
      ok: "OK".to_string(),
      cancel: "Cancel".to_string(),
      open: "Open".to_string(),
      save: "Save".to_string(),
      select: "Select".to_string(),
    }
  }
}

/// Fields present in one `setDialogLabels` call. `None` keeps the stored text.
#[derive(Default)]
pub(super) struct Patch {
  pub ok: Option<String>,
  pub cancel: Option<String>,
  pub open: Option<String>,
  pub save: Option<String>,
  pub select: Option<String>,
}

/// A label the platform can show. Empty text and NUL are rejected. NUL would
/// truncate a Windows wide string.
pub(super) fn valid_label(text: &str) -> bool {
  !text.is_empty() && !text.contains('\0')
}

pub(super) fn current() -> Labels {
  let mut guard = lock();
  guard.get_or_insert_with(Labels::default).clone()
}

pub(super) fn replace(patch: Patch) {
  let mut guard = lock();
  let labels = guard.get_or_insert_with(Labels::default);
  apply(labels, &patch);
}

fn apply(labels: &mut Labels, patch: &Patch) {
  if let Some(value) = &patch.ok {
    labels.ok.clone_from(value);
  }
  if let Some(value) = &patch.cancel {
    labels.cancel.clone_from(value);
  }
  if let Some(value) = &patch.open {
    labels.open.clone_from(value);
  }
  if let Some(value) = &patch.save {
    labels.save.clone_from(value);
  }
  if let Some(value) = &patch.select {
    labels.select.clone_from(value);
  }
}

fn lock() -> std::sync::MutexGuard<'static, Option<Labels>> {
  static LABELS: Mutex<Option<Labels>> = Mutex::new(None);
  LABELS.lock().unwrap_or_else(|error| error.into_inner())
}

/// GTK treats `_` as a mnemonic. Doubling it keeps that character visible.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) fn gtk_label(text: &str) -> String {
  text.replace('_', "__")
}

/// A Win32 button treats `&` as a mnemonic. Doubling it keeps it visible.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(super) fn win_label(text: &str) -> String {
  text.replace('&', "&&")
}

/// Accept and cancel buttons for the Windows prompt, in 96-DPI pixels.
///
/// `ok_text` and `cancel_text` are measured widths of the visible labels.
/// The caller scales the returned rect by the window DPI. Do not divide the
/// measurement by DPI first: the stock font's extent is already the width
/// those 96-DPI units describe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(super) struct PromptButtons {
  pub width: i32,
  pub ok_x: i32,
  pub ok_width: i32,
  pub cancel_x: i32,
  pub cancel_width: i32,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(super) fn prompt_button_row(
  ok_text: i32,
  cancel_text: i32,
) -> PromptButtons {
  const CLIENT_WIDTH: i32 = 404;
  const MIN_BUTTON: i32 = 86;
  const TEXT_PAD: i32 = 24;
  const EDGE: i32 = 12;
  const GAP: i32 = 10;
  const TEXT_LIMIT: i32 = 4096;
  let button = |text: i32| {
    let text = text.clamp(0, TEXT_LIMIT);
    (text + TEXT_PAD).max(MIN_BUTTON)
  };
  let ok_width = button(ok_text);
  let cancel_width = button(cancel_text);
  let needed = EDGE + ok_width + GAP + cancel_width + EDGE;
  let width = CLIENT_WIDTH.max(needed);
  let cancel_x = width - EDGE - cancel_width;
  let ok_x = cancel_x - GAP - ok_width;
  PromptButtons {
    width,
    ok_x,
    ok_width,
    cancel_x,
    cancel_width,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn omitted_fields_keep_their_labels() {
    let mut labels = Labels::default();
    apply(
      &mut labels,
      &Patch {
        ok: Some("はい".to_string()),
        select: Some("選択".to_string()),
        ..Patch::default()
      },
    );
    assert_eq!(labels.ok, "はい");
    assert_eq!(labels.cancel, "Cancel");
    assert_eq!(labels.open, "Open");
    assert_eq!(labels.save, "Save");
    assert_eq!(labels.select, "選択");
    apply(
      &mut labels,
      &Patch {
        cancel: Some("いいえ".to_string()),
        ..Patch::default()
      },
    );
    assert_eq!(labels.ok, "はい");
    assert_eq!(labels.cancel, "いいえ");
    assert_eq!(labels.select, "選択");
  }

  #[test]
  fn empty_and_nul_are_rejected() {
    assert!(valid_label("OK"));
    assert!(valid_label("キャンセル"));
    assert!(valid_label(" "));
    assert!(!valid_label(""));
    assert!(!valid_label("O\0K"));
  }

  #[test]
  fn mnemonics_stay_visible() {
    assert_eq!(gtk_label("OK"), "OK");
    assert_eq!(gtk_label("a_b"), "a__b");
    assert_eq!(win_label("OK"), "OK");
    assert_eq!(win_label("A&B"), "A&&B");
  }

  #[test]
  fn english_prompt_buttons_keep_their_places() {
    let row = prompt_button_row(40, 40);
    assert_eq!(row.width, 404);
    assert_eq!(row.ok_width, 86);
    assert_eq!(row.cancel_width, 86);
    assert_eq!(row.cancel_x, 306);
    assert_eq!(row.ok_x, 210);
  }

  #[test]
  fn long_prompt_labels_widen_the_client() {
    let row = prompt_button_row(200, 200);
    assert_eq!(row.ok_width, 224);
    assert_eq!(row.cancel_width, 224);
    assert_eq!(row.ok_x, 12);
    assert_eq!(row.cancel_x, 12 + 224 + 10);
    assert_eq!(row.width, row.cancel_x + row.cancel_width + 12);
    assert!(row.width > 404);
  }

  #[test]
  fn replace_updates_the_process_store() {
    let saved = current();
    let _restore = Restore(saved.clone());
    replace(Patch {
      ok: Some("はい".to_string()),
      ..Patch::default()
    });
    let next = current();
    assert_eq!(next.ok, "はい");
    assert_eq!(next.cancel, saved.cancel);
    assert_eq!(next.open, saved.open);
    assert_eq!(next.save, saved.save);
    assert_eq!(next.select, saved.select);
  }

  struct Restore(Labels);

  impl Drop for Restore {
    fn drop(&mut self) {
      replace(Patch {
        ok: Some(self.0.ok.clone()),
        cancel: Some(self.0.cancel.clone()),
        open: Some(self.0.open.clone()),
        save: Some(self.0.save.clone()),
        select: Some(self.0.select.clone()),
      });
    }
  }
}
