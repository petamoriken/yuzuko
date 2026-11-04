//! AppKit `NSAlert` dialogs for one window.

pub(super) fn parent(
  id: u64,
) -> Result<Option<objc2::rc::Retained<objc2_app_kit::NSWindow>>, String> {
  use objc2::rc::Retained;
  use objc2_app_kit::NSView;
  use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
  let view = crate::os::with_window(id, |window| {
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
      RawWindowHandle::AppKit(appkit) => {
        Some(appkit.ns_view.as_ptr() as *mut NSView)
      }
      _ => None,
    }
  })
  .ok_or_else(|| "This window has no dialog.".to_string())?;
  let Some(view) = view else {
    return Err("This window has no dialog.".to_string());
  };
  let view = unsafe { Retained::retain(view) }
    .ok_or_else(|| "This window has no dialog.".to_string())?;
  Ok(view.window())
}

pub(super) fn present(
  parent: Option<&objc2_app_kit::NSWindow>,
  title: &str,
  message: &str,
  dialog: super::Dialog<'_>,
) -> Result<super::Answer, String> {
  use objc2::MainThreadMarker;
  use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSTextField, NSWindow,
  };
  use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
  let Some(mtm) = MainThreadMarker::new() else {
    return Err("Dialogs run on the main thread.".to_string());
  };
  let _ = parent;
  let alert = NSAlert::new(mtm);
  alert.setMessageText(&NSString::from_str(message));
  alert.addButtonWithTitle(&NSString::from_str("OK"));
  let field = match dialog {
    super::Dialog::Alert => None,
    super::Dialog::Confirm => {
      alert.addButtonWithTitle(&NSString::from_str("Cancel"));
      None
    }
    super::Dialog::Prompt(default_value) => {
      alert.addButtonWithTitle(&NSString::from_str("Cancel"));
      let field = NSTextField::textFieldWithString(
        &NSString::from_str(default_value),
        mtm,
      );
      field.setFrame(NSRect {
        origin: NSPoint { x: 0.0, y: 0.0 },
        size: NSSize {
          width: 260.0,
          height: 24.0,
        },
      });
      alert.setAccessoryView(Some(&field));
      Some(field)
    }
  };
  alert.layout();
  let panel: objc2::rc::Retained<NSWindow> = alert.window();
  panel.setTitle(&NSString::from_str(title));
  let response = alert.runModal();
  if response != NSAlertFirstButtonReturn {
    return Ok(super::Answer::Cancel);
  }
  if let Some(field) = field {
    let value: objc2::rc::Retained<NSString> =
      unsafe { objc2::msg_send![&*field, stringValue] };
    return Ok(super::Answer::Text(value.to_string()));
  }
  Ok(super::Answer::Ok)
}

pub(super) fn pick_files(
  parent: Option<&objc2_app_kit::NSWindow>,
  title: &str,
  multiple: bool,
  filters: &[super::FileFilter],
) -> Result<Option<Vec<String>>, String> {
  use objc2::ClassType;
  use objc2::MainThreadMarker;
  use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
  let Some(mtm) = MainThreadMarker::new() else {
    return Err("Dialogs run on the main thread.".to_string());
  };
  // The owner is retained by the caller for the modal call, matching
  // `NSAlert`. AppKit does not take it as a parent of `runModal`.
  let _ = parent;
  let panel = NSOpenPanel::openPanel(mtm);
  panel.setCanChooseFiles(true);
  panel.setCanChooseDirectories(false);
  panel.setAllowsMultipleSelection(multiple);
  let save = panel.as_super();
  configure_panel(save, title, None, filters);
  let response = save.runModal();
  if response < 0 {
    return Err("The file dialog failed.".to_string());
  }
  if response != NSModalResponseOK {
    return Ok(None);
  }
  let urls = panel.URLs();
  let mut paths = Vec::new();
  for index in 0..urls.count() {
    let url = urls.objectAtIndex(index);
    let Some(path) = url.path() else {
      continue;
    };
    paths.push(super::absolute_path(std::path::PathBuf::from(
      path.to_string(),
    )));
  }
  if paths.is_empty() {
    Ok(None)
  } else {
    Ok(Some(paths))
  }
}

pub(super) fn pick_save(
  parent: Option<&objc2_app_kit::NSWindow>,
  title: &str,
  suggested: Option<&str>,
  filters: &[super::FileFilter],
) -> Result<Option<Vec<String>>, String> {
  use objc2::MainThreadMarker;
  use objc2_app_kit::{NSModalResponseOK, NSSavePanel};
  let Some(mtm) = MainThreadMarker::new() else {
    return Err("Dialogs run on the main thread.".to_string());
  };
  let _ = parent;
  let panel = NSSavePanel::savePanel(mtm);
  configure_panel(&panel, title, suggested, filters);
  let response = panel.runModal();
  if response < 0 {
    return Err("The file dialog failed.".to_string());
  }
  if response != NSModalResponseOK {
    return Ok(None);
  }
  let Some(url) = panel.URL() else {
    return Ok(None);
  };
  let Some(path) = url.path() else {
    return Ok(None);
  };
  Ok(Some(vec![super::absolute_path(std::path::PathBuf::from(
    path.to_string(),
  ))]))
}

fn configure_panel(
  panel: &objc2_app_kit::NSSavePanel,
  title: &str,
  suggested: Option<&str>,
  filters: &[super::FileFilter],
) {
  use objc2_foundation::NSString;
  panel.setTitle(Some(&NSString::from_str(title)));
  if let Some(name) = suggested {
    panel.setNameFieldStringValue(&NSString::from_str(name));
  }
  apply_extensions(panel, filters);
}

/// AppKit has one allowed-type list. Names are not shown. `*` allows every file.
#[allow(deprecated)]
fn apply_extensions(
  panel: &objc2_app_kit::NSSavePanel,
  filters: &[super::FileFilter],
) {
  use objc2_foundation::{NSArray, NSString};
  if filters.is_empty() || filters.iter().any(|filter| filter.all) {
    return;
  }
  let mut names = Vec::new();
  for filter in filters {
    for extension in &filter.extensions {
      if !names.iter().any(|known: &String| known == extension) {
        names.push(extension.clone());
      }
    }
  }
  if names.is_empty() {
    return;
  }
  let strings: Vec<_> =
    names.iter().map(|name| NSString::from_str(name)).collect();
  let refs: Vec<&NSString> = strings.iter().map(|item| item.as_ref()).collect();
  let array = NSArray::from_slice(&refs);
  panel.setAllowedFileTypes(Some(&array));
}
