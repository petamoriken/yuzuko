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

fn prompt(
  parent: windows_sys::Win32::Foundation::HWND,
  title: &str,
  message: &str,
  default_value: &str,
) -> Result<super::Answer, String> {
  use std::sync::Once;
  use std::sync::OnceLock;
  use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
  use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
  use windows_sys::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW,
    DefWindowProcW, DestroyWindow, DispatchMessageW, EnableWindow,
    GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, HMENU, MSG, RegisterClassW,
    SW_SHOW, SetWindowLongPtrW, ShowWindow, TranslateMessage, WM_CLOSE,
    WM_COMMAND, WM_CREATE, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_EX_DLGMODALFRAME, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
  };
  const EDIT: isize = 101;
  const OK: isize = 1;
  const CANCEL: isize = 2;
  struct State {
    edit: HWND,
    ok: bool,
    text: String,
    done: bool,
    default_value: String,
    message: String,
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
      let label = wide(&state.message);
      let initial = wide(&state.default_value);
      unsafe {
        CreateWindowExW(
          0,
          wide("STATIC").as_ptr(),
          label.as_ptr(),
          WS_CHILD | WS_VISIBLE,
          12,
          12,
          380,
          40,
          hwnd,
          std::ptr::null_mut(),
          instance,
          std::ptr::null_mut(),
        );
        state.edit = CreateWindowExW(
          0,
          wide("EDIT").as_ptr(),
          initial.as_ptr(),
          WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP,
          12,
          56,
          380,
          24,
          hwnd,
          EDIT as HMENU,
          instance,
          std::ptr::null_mut(),
        );
        CreateWindowExW(
          0,
          wide("BUTTON").as_ptr(),
          wide("OK").as_ptr(),
          WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON,
          210,
          96,
          86,
          26,
          hwnd,
          OK as HMENU,
          instance,
          std::ptr::null_mut(),
        );
        CreateWindowExW(
          0,
          wide("BUTTON").as_ptr(),
          wide("Cancel").as_ptr(),
          WS_CHILD | WS_VISIBLE | WS_TABSTOP,
          306,
          96,
          86,
          26,
          hwnd,
          CANCEL as HMENU,
          instance,
          std::ptr::null_mut(),
        );
      }
      return 0;
    }
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    if state.is_null() {
      return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let state = unsafe { &mut *state };
    match msg {
      WM_COMMAND => {
        let id = (wparam & 0xffff) as isize;
        if id == OK {
          state.text = edit_text(state.edit);
          state.ok = true;
          state.done = true;
          unsafe { DestroyWindow(hwnd) };
          return 0;
        }
        if id == CANCEL {
          state.ok = false;
          state.done = true;
          unsafe { DestroyWindow(hwnd) };
          return 0;
        }
        0
      }
      WM_CLOSE => {
        state.ok = false;
        state.done = true;
        unsafe { DestroyWindow(hwnd) };
        0
      }
      _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
  }
  static CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();
  static REGISTER: Once = Once::new();
  REGISTER.call_once(|| {
    let class_name = CLASS_NAME.get_or_init(|| wide("YuzukoPrompt"));
    let class = WNDCLASSW {
      lpfnWndProc: Some(proc),
      hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
      lpszClassName: class_name.as_ptr(),
      ..unsafe { std::mem::zeroed() }
    };
    unsafe { RegisterClassW(&class) };
  });
  let mut state = State {
    edit: std::ptr::null_mut(),
    ok: false,
    text: String::new(),
    done: false,
    default_value: default_value.to_string(),
    message: message.to_string(),
  };
  let caption = wide(title);
  let class_name = wide("YuzukoPrompt");
  unsafe { EnableWindow(parent, 0) };
  let hwnd = unsafe {
    CreateWindowExW(
      WS_EX_DLGMODALFRAME,
      class_name.as_ptr(),
      caption.as_ptr(),
      WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
      CW_USEDEFAULT,
      CW_USEDEFAULT,
      420,
      170,
      parent,
      std::ptr::null_mut(),
      GetModuleHandleW(std::ptr::null()),
      (&mut state as *mut State).cast(),
    )
  };
  if hwnd.is_null() {
    unsafe { EnableWindow(parent, 1) };
    return Err("This window has no dialog.".to_string());
  }
  unsafe { ShowWindow(hwnd, SW_SHOW) };
  while !state.done {
    let mut message = unsafe { std::mem::zeroed::<MSG>() };
    let code = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
    if code == 0 || code == -1 {
      break;
    }
    unsafe {
      TranslateMessage(&message);
      DispatchMessageW(&message);
    }
  }
  unsafe { EnableWindow(parent, 1) };
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
