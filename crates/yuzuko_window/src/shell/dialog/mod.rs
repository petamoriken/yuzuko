//! Native `alert`, `confirm`, `prompt`, file, and directory dialogs.
//!
//! `Yuzuko.setDialogLabels` replaces the labels these dialogs draw.
//! Windows `MessageBox` and the system file dialogs keep their own buttons.

mod labels;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use yuzuko_core::op::{OpError, OpException};

pub(crate) fn install(scope: &mut v8::PinScope) {
  let yuzuko = crate::shell::menu::yuzuko_object(scope);
  let function = v8::Function::new(scope, set_dialog_labels).unwrap();
  let key = v8::String::new(scope, "setDialogLabels").unwrap();
  yuzuko.set(scope, key.into(), function.into());
}

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

pub(crate) struct FileFilter {
  /// Windows and Linux show this beside the extensions. AppKit keeps one
  /// extension list, so macOS does not read it.
  #[cfg_attr(target_os = "macos", allow(dead_code))]
  pub name: String,
  pub extensions: Vec<String>,
  pub all: bool,
}

pub(crate) enum Picker {
  Open {
    multiple: bool,
    filters: Vec<FileFilter>,
  },
  Save {
    suggested: Option<String>,
    filters: Vec<FileFilter>,
  },
  Directory,
}

pub(crate) fn parse_picker(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Value>>,
) -> Result<Picker, OpError> {
  let object = options_object(scope, options)?;
  let Some(object) = object else {
    return Ok(Picker::Open {
      multiple: false,
      filters: Vec::new(),
    });
  };
  let mode = match get_field(scope, object, "mode")? {
    Some(value) if value.is_string() => js_string(scope, value)?,
    Some(_) => {
      return Err(OpError::type_(
        "mode must be \"open\", \"save\", or \"directory\".",
      ));
    }
    None => "open".to_string(),
  };
  match mode.as_str() {
    "open" => {
      reject_present(
        scope,
        object,
        "suggestedName",
        "suggestedName applies only to a save dialog.",
      )?;
      Ok(Picker::Open {
        multiple: parse_multiple(scope, object)?,
        filters: parse_filters(scope, object)?,
      })
    }
    "save" => {
      reject_present(
        scope,
        object,
        "multiple",
        "multiple applies only to an open dialog.",
      )?;
      Ok(Picker::Save {
        filters: parse_filters(scope, object)?,
        suggested: parse_suggested(scope, object)?,
      })
    }
    "directory" => {
      reject_present(
        scope,
        object,
        "multiple",
        "multiple applies only to an open dialog.",
      )?;
      reject_present(
        scope,
        object,
        "filters",
        "filters apply only to a file dialog.",
      )?;
      reject_present(
        scope,
        object,
        "suggestedName",
        "suggestedName applies only to a save dialog.",
      )?;
      Ok(Picker::Directory)
    }
    _ => Err(OpError::type_(
      "mode must be \"open\", \"save\", or \"directory\".",
    )),
  }
}

fn reject_present(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  message: &str,
) -> Result<(), OpError> {
  if get_field(scope, object, name)?.is_some() {
    Err(OpError::type_(message))
  } else {
    Ok(())
  }
}

fn parse_multiple(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<bool, OpError> {
  match get_field(scope, object, "multiple")? {
    Some(value) if value.is_boolean() => Ok(value.is_true()),
    Some(_) => Err(OpError::type_("multiple must be a boolean.")),
    None => Ok(false),
  }
}

fn parse_suggested(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Option<String>, OpError> {
  match get_field(scope, object, "suggestedName")? {
    Some(value) => {
      let name = js_string(scope, value)?;
      if name.is_empty() || name.contains(['/', '\\', '\0']) {
        return Err(OpError::type_("suggestedName must be a file name."));
      }
      Ok(Some(name))
    }
    None => Ok(None),
  }
}

pub(crate) fn pick_files(
  id: u64,
  title: &str,
  multiple: bool,
  filters: &[FileFilter],
) -> Result<Option<Vec<String>>, String> {
  #[cfg(target_os = "macos")]
  {
    let parent = macos::parent(id)?;
    macos::pick_files(parent.as_deref(), title, multiple, filters)
  }
  #[cfg(target_os = "windows")]
  {
    let parent = windows::parent(id)?;
    windows::pick_files(parent, title, multiple, filters)
  }
  #[cfg(target_os = "linux")]
  {
    let _ = id;
    linux::pick_files(title, multiple, filters)
  }
  #[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux"
  )))]
  {
    let _ = (id, title, multiple, filters);
    Err("This system has no dialog.".to_string())
  }
}

pub(crate) fn pick_save_file(
  id: u64,
  title: &str,
  suggested: Option<&str>,
  filters: &[FileFilter],
) -> Result<Option<Vec<String>>, String> {
  #[cfg(target_os = "macos")]
  {
    let parent = macos::parent(id)?;
    macos::pick_save(parent.as_deref(), title, suggested, filters)
  }
  #[cfg(target_os = "windows")]
  {
    let parent = windows::parent(id)?;
    windows::pick_save(parent, title, suggested, filters)
  }
  #[cfg(target_os = "linux")]
  {
    let _ = id;
    linux::pick_save(title, suggested, filters)
  }
  #[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux"
  )))]
  {
    let _ = (id, title, suggested, filters);
    Err("This system has no dialog.".to_string())
  }
}

pub(crate) fn pick_directory(
  id: u64,
  title: &str,
) -> Result<Option<Vec<String>>, String> {
  #[cfg(target_os = "macos")]
  {
    let parent = macos::parent(id)?;
    macos::pick_directory(parent.as_deref(), title)
  }
  #[cfg(target_os = "windows")]
  {
    let parent = windows::parent(id)?;
    windows::pick_directory(parent, title)
  }
  #[cfg(target_os = "linux")]
  {
    let _ = id;
    linux::pick_directory(title)
  }
  #[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux"
  )))]
  {
    let _ = (id, title);
    Err("This system has no dialog.".to_string())
  }
}

pub(super) fn absolute_path(path: std::path::PathBuf) -> String {
  let path = if path.is_absolute() {
    path
  } else {
    match std::env::current_dir() {
      Ok(dir) => dir.join(path),
      Err(_) => path,
    }
  };
  path.to_string_lossy().into_owned()
}

fn options_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  options: Option<v8::Local<'_, v8::Value>>,
) -> Result<Option<v8::Local<'s, v8::Object>>, OpError> {
  let Some(value) = options else {
    return Ok(None);
  };
  if value.is_undefined() {
    return Ok(None);
  }
  if value.is_null() || value.is_string() || !value.is_object() {
    return Err(OpError::type_("The file picker options must be an object."));
  }
  let global = v8::Global::new(scope, value);
  let value = v8::Local::new(scope, global);
  v8::Local::<v8::Object>::try_from(value)
    .map(Some)
    .map_err(|_| OpError::type_("The file picker options must be an object."))
}

fn get_field<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  let Some(key) = v8::String::new(scope, name) else {
    return Err(OpError::type_("The file picker options must be an object."));
  };
  let captured = {
    v8::tc_scope!(let tc, scope);
    let object = yuzuko_core::op::root(tc, object);
    match object.get(tc, key.into()) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => {
        let exception =
          tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
        Err(v8::Global::new(tc, exception))
      }
    }
  };
  match captured {
    Ok(global) => {
      let value = v8::Local::new(scope, global);
      if value.is_undefined() {
        Ok(None)
      } else {
        Ok(Some(value))
      }
    }
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      Err(OpError::thrown(scope, exception))
    }
  }
}

fn array_item<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  list: v8::Local<'_, v8::Array>,
  index: u32,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    let list = yuzuko_core::op::root(tc, list);
    match list.get_index(tc, index) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => {
        if let Some(exception) = tc.exception() {
          Err(v8::Global::new(tc, exception))
        } else {
          let missing: v8::Local<v8::Value> = v8::undefined(tc).into();
          Ok(v8::Global::new(tc, missing))
        }
      }
    }
  };
  match captured {
    Ok(global) => Ok(v8::Local::new(scope, global)),
    Err(exception) => {
      let exception = v8::Local::new(scope, exception);
      Err(OpError::thrown(scope, exception))
    }
  }
}

fn parse_filters(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<Vec<FileFilter>, OpError> {
  let Some(value) = get_field(scope, object, "filters")? else {
    return Ok(Vec::new());
  };
  let Ok(list) = v8::Local::<v8::Array>::try_from(value) else {
    return Err(OpError::type_("filters must be an array."));
  };
  let mut filters = Vec::new();
  for index in 0..list.length() {
    let item = array_item(scope, list, index)?;
    filters.push(parse_filter(scope, item)?);
  }
  Ok(filters)
}

fn parse_filter(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<FileFilter, OpError> {
  if value.is_null() || value.is_string() || !value.is_object() {
    return Err(OpError::type_("A file filter must be an object."));
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Err(OpError::type_("A file filter must be an object."));
  };
  let name = match get_field(scope, object, "name")? {
    Some(text) => js_string(scope, text)?,
    None => String::new(),
  };
  if name.contains('\0') {
    return Err(OpError::type_("A file filter name must be text."));
  }
  let Some(extensions_value) = get_field(scope, object, "extensions")? else {
    return Err(OpError::type_("extensions must be an array of extensions."));
  };
  let Ok(list) = v8::Local::<v8::Array>::try_from(extensions_value) else {
    return Err(OpError::type_("extensions must be an array of extensions."));
  };
  if list.length() == 0 {
    return Err(OpError::type_("extensions must be an array of extensions."));
  }
  let mut extensions = Vec::new();
  let mut all = false;
  for index in 0..list.length() {
    let item = array_item(scope, list, index)?;
    let text = js_string(scope, item)?;
    if text == "*" {
      all = true;
      continue;
    }
    if text.is_empty() || text.contains(['/', '\\', '.', '\0', '*']) {
      return Err(OpError::type_("A file extension has no separator."));
    }
    extensions.push(text);
  }
  if all {
    extensions.clear();
  }
  let name = if name.is_empty() {
    if all {
      "All files".to_string()
    } else {
      extensions.join(", ")
    }
  } else {
    name
  };
  Ok(FileFilter {
    name,
    extensions,
    all,
  })
}

fn set_dialog_labels(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _return: v8::ReturnValue,
) {
  if let Err(error) = assign_labels(scope, args.get(0)) {
    OpException::throw(error, scope);
  }
}

/// Reads every field before storing, so a thrown getter leaves the labels
/// unchanged.
fn assign_labels(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  let Some(object) = labels_object(scope, value)? else {
    return Ok(());
  };
  let patch = labels::Patch {
    ok: read_label(scope, object, "ok")?,
    cancel: read_label(scope, object, "cancel")?,
    open: read_label(scope, object, "open")?,
    save: read_label(scope, object, "save")?,
    select: read_label(scope, object, "select")?,
  };
  labels::replace(patch);
  Ok(())
}

fn labels_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, v8::Value>,
) -> Result<Option<v8::Local<'s, v8::Object>>, OpError> {
  if value.is_undefined() {
    return Ok(None);
  }
  if value.is_null() || value.is_string() || !value.is_object() {
    return Err(OpError::type_("The dialog labels must be an object."));
  }
  let global = v8::Global::new(scope, value);
  let value = v8::Local::new(scope, global);
  v8::Local::<v8::Object>::try_from(value)
    .map(Some)
    .map_err(|_| OpError::type_("The dialog labels must be an object."))
}

fn read_label(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<String>, OpError> {
  let Some(value) = get_field(scope, object, name)? else {
    return Ok(None);
  };
  if !value.is_string() {
    return Err(OpError::type_(format!("{name} must be a string.")));
  }
  let text = js_string(scope, value)?;
  if !labels::valid_label(&text) {
    return Err(OpError::type_(format!("{name} must be text.")));
  }
  Ok(Some(text))
}
