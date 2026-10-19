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
