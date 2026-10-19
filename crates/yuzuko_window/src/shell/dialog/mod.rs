//! Native `alert`, `confirm`, and `prompt` for one `Yuzuko.Window`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use yuzuko_core::op::OpError;

pub(crate) fn js_string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => {
        let exception =
          tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
        Err(v8::Global::new(tc, exception))
      }
    }
  };
  match captured {
    Ok(text) => Ok(text),
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      Err(OpError::thrown(scope, exception))
    }
  }
}

pub fn alert(id: u64, title: &str, message: &str) -> Result<(), String> {
  present(id, title, message, Dialog::Alert).map(|_| ())
}

pub fn confirm(id: u64, title: &str, message: &str) -> Result<bool, String> {
  match present(id, title, message, Dialog::Confirm)? {
    Answer::Ok => Ok(true),
    Answer::Cancel => Ok(false),
    Answer::Text(_) => Ok(false),
  }
}

pub fn prompt(
  id: u64,
  title: &str,
  message: &str,
  default_value: &str,
) -> Result<Option<String>, String> {
  match present(id, title, message, Dialog::Prompt(default_value))? {
    Answer::Text(text) => Ok(Some(text)),
    Answer::Ok => Ok(Some(String::new())),
    Answer::Cancel => Ok(None),
  }
}

enum Dialog<'a> {
  Alert,
  Confirm,
  Prompt(&'a str),
}

enum Answer {
  Ok,
  Cancel,
  Text(String),
}

fn present(
  id: u64,
  title: &str,
  message: &str,
  dialog: Dialog<'_>,
) -> Result<Answer, String> {
  #[cfg(target_os = "macos")]
  {
    let parent = macos::parent(id)?;
    macos::present(parent.as_deref(), title, message, dialog)
  }
  #[cfg(target_os = "windows")]
  {
    let parent = windows::parent(id)?;
    windows::present(parent, title, message, dialog)
  }
  #[cfg(target_os = "linux")]
  {
    let _ = id;
    linux::present(title, message, dialog)
  }
  #[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux"
  )))]
  {
    let _ = (id, title, message, dialog);
    Err("This system has no dialog.".to_string())
  }
}
