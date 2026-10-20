//! Win32 dialogs for one window.

pub(super) fn parent(
  id: u64,
) -> Result<windows_sys::Win32::Foundation::HWND, String> {
  use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
  crate::os::with_window(id, |window| {
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
      RawWindowHandle::Win32(win) => Some(win.hwnd.get() as _),
      _ => None,
    }
  })
  .flatten()
  .ok_or_else(|| "This window has no dialog.".to_string())
}

pub(super) fn present(
  parent: windows_sys::Win32::Foundation::HWND,
  title: &str,
  message: &str,
  dialog: super::Dialog<'_>,
) -> Result<super::Answer, String> {
  use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDOK, MB_OK, MB_OKCANCEL, MessageBoxW,
  };
  let caption = wide(title);
  let text = wide(message);
  match dialog {
    super::Dialog::Alert => {
      unsafe {
        MessageBoxW(parent, text.as_ptr(), caption.as_ptr(), MB_OK);
      }
      Ok(super::Answer::Ok)
    }
    super::Dialog::Confirm => {
      let code = unsafe {
        MessageBoxW(parent, text.as_ptr(), caption.as_ptr(), MB_OKCANCEL)
      };
      if code == IDOK {
        Ok(super::Answer::Ok)
      } else {
        Ok(super::Answer::Cancel)
      }
    }
    super::Dialog::Prompt(default_value) => {
      prompt(parent, title, message, default_value)
    }
  }
}

fn wide(text: &str) -> Vec<u16> {
  text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The `prompt` window class. Its OK and Cancel buttons use the dialog IDs
/// `IDOK` and `IDCANCEL`, so `IsDialogMessageW` maps Enter and Esc onto them.
const PROMPT_CLASS: &str = "YuzukoPrompt";

fn prompt(
  parent: windows_sys::Win32::Foundation::HWND,
  title: &str,
  message: &str,
  default_value: &str,
) -> Result<super::Answer, String> {
  use std::sync::Once;
  use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
  use windows_sys::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, DEFAULT_GUI_FONT, GetStockObject,
  };
  use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
  use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
  use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, SetFocus,
  };
  use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CREATESTRUCTW,
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    ES_AUTOHSCROLL, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW,
    GetWindowRect, HMENU, IDCANCEL, IDOK, IsDialogMessageW, MSG,
    RegisterClassW, SW_SHOW, SendMessageW, SetWindowLongPtrW, ShowWindow,
    TranslateMessage, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_SETFONT, WNDCLASSW,
    WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_DLGMODALFRAME, WS_OVERLAPPED,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
  };
  const EDIT: isize = 101;
  const EM_SETSEL: u32 = 0x00b1;
  /// Client size in 96-DPI pixels.
  const WIDTH: i32 = 404;
  const HEIGHT: i32 = 134;
  struct State {
    parent: HWND,
    edit: HWND,
    ok: bool,
    text: String,
    done: bool,
    default_value: String,
    message: String,
    scale: f64,
  }
  fn finish(hwnd: HWND, state: &mut State, ok: bool) {
    if ok {
      state.text = edit_text(state.edit);
    }
    state.ok = ok;
    state.done = true;
    // Enable the owner before the dialog goes away so activation returns to
    // it instead of another application's window.
    unsafe {
      EnableWindow(state.parent, 1);
      DestroyWindow(hwnd);
    }
  }
  unsafe extern "system" fn proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
  ) -> LRESULT {
    if msg == WM_CREATE {
      let created = lparam as *const CREATESTRUCTW;
      let state = unsafe { (*created).lpCreateParams as *mut State };
      unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as _) };
      let state = unsafe { &mut *state };
      let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
      let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
      let px = |value: i32| (f64::from(value) * state.scale).round() as i32;
      let child = |class: &str,
                   text: &str,
                   style: u32,
                   rect: (i32, i32, i32, i32),
                   id: isize| {
        let class = wide(class);
        let text = wide(text);
        let control = unsafe {
          CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            px(rect.0),
            px(rect.1),
            px(rect.2),
            px(rect.3),
            hwnd,
            id as HMENU,
            instance,
            std::ptr::null_mut(),
          )
        };
        unsafe { SendMessageW(control, WM_SETFONT, font as WPARAM, 1) };
        control
      };
      child("STATIC", &state.message, 0, (12, 12, 380, 40), 0);
      state.edit = child(
        "EDIT",
        &state.default_value,
        WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        (12, 56, 380, 24),
        EDIT,
      );
      child(
        "BUTTON",
        "OK",
        WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        (210, 96, 86, 26),
        IDOK as isize,
      );
      child(
        "BUTTON",
        "Cancel",
        WS_TABSTOP | BS_PUSHBUTTON as u32,
        (306, 96, 86, 26),
        IDCANCEL as isize,
      );
      return 0;
    }
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    if state.is_null() {
      return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let state = unsafe { &mut *state };
    match msg {
      WM_COMMAND => {
        let id = (wparam & 0xffff) as i32;
        if id == IDOK {
          finish(hwnd, state, true);
        } else if id == IDCANCEL {
          finish(hwnd, state, false);
        }
        0
      }
      WM_CLOSE => {
        finish(hwnd, state, false);
        0
      }
      _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
  }
  static REGISTER: Once = Once::new();
  REGISTER.call_once(|| {
    let class_name = wide(PROMPT_CLASS);
    let class = WNDCLASSW {
      lpfnWndProc: Some(proc),
      hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
      lpszClassName: class_name.as_ptr(),
      hbrBackground: (COLOR_BTNFACE + 1) as usize as _,
      ..unsafe { std::mem::zeroed() }
    };
    // RegisterClassW copies the name, so the buffer can drop afterward.
    unsafe { RegisterClassW(&class) };
  });
  let dpi = unsafe { GetDpiForWindow(parent) };
  let scale = if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 };
  // The window procedure writes this state through the pointer it gets at
  // creation, so the loop below reads it through the same pointer.
  let state = Box::into_raw(Box::new(State {
    parent,
    edit: std::ptr::null_mut(),
    ok: false,
    text: String::new(),
    done: false,
    default_value: default_value.to_string(),
    message: message.to_string(),
    scale,
  }));
  let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
  let mut frame = RECT {
    left: 0,
    top: 0,
    right: (f64::from(WIDTH) * scale).round() as i32,
    bottom: (f64::from(HEIGHT) * scale).round() as i32,
  };
  unsafe { AdjustWindowRectEx(&mut frame, style, 0, WS_EX_DLGMODALFRAME) };
  let width = frame.right - frame.left;
  let height = frame.bottom - frame.top;
  let mut owner = RECT {
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
  };
  unsafe { GetWindowRect(parent, &mut owner) };
  let x = owner.left + (owner.right - owner.left - width) / 2;
  let y = owner.top + (owner.bottom - owner.top - height) / 2;
  let caption = wide(title);
  let class_name = wide(PROMPT_CLASS);
  unsafe { EnableWindow(parent, 0) };
  let hwnd = unsafe {
    CreateWindowExW(
      WS_EX_DLGMODALFRAME,
      class_name.as_ptr(),
      caption.as_ptr(),
      style,
      x,
      y,
      width,
      height,
      parent,
      std::ptr::null_mut(),
      GetModuleHandleW(std::ptr::null()),
      state.cast(),
    )
  };
  if hwnd.is_null() {
    unsafe {
      EnableWindow(parent, 1);
      drop(Box::from_raw(state));
    }
    return Err("This window has no dialog.".to_string());
  }
  unsafe {
    ShowWindow(hwnd, SW_SHOW);
    SetFocus((*state).edit);
    SendMessageW((*state).edit, EM_SETSEL, 0, -1);
  }
  loop {
    // `DispatchMessageW` runs the window procedure, which sets `done`.
    if unsafe { (*state).done } {
      break;
    }
    let mut message = unsafe { std::mem::zeroed::<MSG>() };
    let code = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
    if code == 0 || code == -1 {
      break;
    }
    // Tab moves focus, Enter presses OK, and Esc presses Cancel.
    if unsafe { IsDialogMessageW(hwnd, &message) } != 0 {
      continue;
    }
    unsafe {
      TranslateMessage(&message);
      DispatchMessageW(&message);
    }
  }
  if !unsafe { (*state).done } {
    unsafe {
      EnableWindow(parent, 1);
      DestroyWindow(hwnd);
    }
  }
  // The window is destroyed, so its procedure no longer holds the pointer.
  let state = unsafe { Box::from_raw(state) };
  if state.ok {
    Ok(super::Answer::Text(state.text))
  } else {
    Ok(super::Answer::Cancel)
  }
}

fn edit_text(edit: windows_sys::Win32::Foundation::HWND) -> String {
  use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, GetWindowTextW,
  };
  let length = unsafe { GetWindowTextLengthW(edit) };
  if length <= 0 {
    return String::new();
  }
  let mut buffer = vec![0u16; length as usize + 1];
  let written =
    unsafe { GetWindowTextW(edit, buffer.as_mut_ptr(), buffer.len() as i32) };
  if written <= 0 {
    return String::new();
  }
  String::from_utf16_lossy(&buffer[..written as usize])
}
