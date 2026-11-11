//! `navigator.maxTouchPoints` for connected touch screens.
//!
//! Pointer Events takes the maximum across digitizers, not the sum, and
//! uses 0 when no maximum is known. A trackpad is excluded. A touch screen
//! whose maximum cannot be read counts as 1.

pub(crate) fn max_touch_points() -> i32 {
  largest_contact_count(&screen_contact_counts())
}

/// Empty is no touch screen. `None` or a non-positive count is a touch
/// screen whose maximum is unknown.
fn largest_contact_count(devices: &[Option<i32>]) -> i32 {
  if devices.is_empty() {
    return 0;
  }
  let mut best = 1;
  for count in devices {
    if let Some(count) = count.filter(|count| *count > 0) {
      best = best.max(count);
    }
  }
  best
}

fn plausible(count: i64) -> Option<i32> {
  let count = i32::try_from(count).ok()?;
  (1..=1024).contains(&count).then_some(count)
}

#[cfg(windows)]
fn screen_contact_counts() -> Vec<Option<i32>> {
  use windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics;
  use windows_sys::Win32::UI::WindowsAndMessaging::NID_EXTERNAL_TOUCH;
  use windows_sys::Win32::UI::WindowsAndMessaging::NID_INTEGRATED_TOUCH;
  use windows_sys::Win32::UI::WindowsAndMessaging::NID_READY;
  use windows_sys::Win32::UI::WindowsAndMessaging::SM_DIGITIZER;
  use windows_sys::Win32::UI::WindowsAndMessaging::SM_MAXIMUMTOUCHES;

  // `SM_MAXIMUMTOUCHES` also rises for some precision touchpads. The
  // digitizer flags keep those out. A touch screen with a non-positive
  // maximum still counts as 1.
  let Ok(digitizer) = u32::try_from(unsafe { GetSystemMetrics(SM_DIGITIZER) })
  else {
    return Vec::new();
  };
  let touch = NID_INTEGRATED_TOUCH | NID_EXTERNAL_TOUCH;
  if (digitizer & NID_READY) == 0 || (digitizer & touch) == 0 {
    return Vec::new();
  }
  let maximum = unsafe { GetSystemMetrics(SM_MAXIMUMTOUCHES) };
  vec![plausible(i64::from(maximum))]
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
fn screen_contact_counts() -> Vec<Option<i32>> {
  let Ok(entries) = std::fs::read_dir("/sys/class/input") else {
    return Vec::new();
  };
  let mut counts = Vec::new();
  for entry in entries.flatten() {
    let name = entry.file_name();
    let Some(name) = name.to_str() else {
      continue;
    };
    if !numbered_name(name, "input") {
      continue;
    }
    let dir = entry.path();
    let prop = sysfs_text(&dir.join("capabilities/prop"));
    let key = sysfs_text(&dir.join("capabilities/key"));
    let abs = sysfs_text(&dir.join("capabilities/abs"));
    if !is_direct_touch_screen(&prop, &key, &abs) {
      continue;
    }
    // Opening the event node is not a permission-checked read. Failure
    // still counts this touch screen as 1.
    counts.push(slot_count(&dir));
  }
  counts
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
fn sysfs_text(path: &std::path::Path) -> String {
  std::fs::read_to_string(path).unwrap_or_default()
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
fn numbered_name(name: &str, prefix: &str) -> bool {
  let Some(rest) = name.strip_prefix(prefix) else {
    return false;
  };
  !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
fn slot_count(input_dir: &std::path::Path) -> Option<i32> {
  let Ok(entries) = std::fs::read_dir(input_dir) else {
    return None;
  };
  let mut best: Option<i32> = None;
  for entry in entries.flatten() {
    let name = entry.file_name();
    let Some(name) = name.to_str() else {
      continue;
    };
    if !numbered_name(name, "event") {
      continue;
    }
    let Some(count) = event_slots(name) else {
      continue;
    };
    best = Some(best.map_or(count, |current| current.max(count)));
  }
  best
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
fn event_slots(name: &str) -> Option<i32> {
  use std::os::unix::io::AsRawFd;

  let path = format!("/dev/input/{name}");
  let file = std::fs::File::open(path).ok()?;
  let mut info = InputAbsinfo::default();
  let rc =
    unsafe { ioctl(file.as_raw_fd(), eviocgabs(ABS_MT_SLOT), &mut info) };
  if rc < 0 {
    return None;
  }
  let span = i64::from(info.maximum) - i64::from(info.minimum) + 1;
  plausible(span)
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
#[repr(C)]
#[derive(Default)]
struct InputAbsinfo {
  value: i32,
  minimum: i32,
  maximum: i32,
  fuzz: i32,
  flat: i32,
  resolution: i32,
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
unsafe extern "C" {
  fn ioctl(fd: i32, request: u64, value: *mut InputAbsinfo) -> i32;
}

#[cfg(target_os = "macos")]
fn screen_contact_counts() -> Vec<Option<i32>> {
  macos::screen_contact_counts()
}

#[cfg(target_os = "macos")]
mod macos {
  use std::ffi::CStr;
  use std::ffi::c_void;

  use super::contact_count_maximum;
  use super::is_touch_screen_usage;

  const UTF8: u32 = 0x0800_0100;
  /// `kCFNumberSInt32Type`. `CFNumberType` is a `CFIndex`.
  const SINT32: isize = 3;

  #[link(name = "CoreFoundation", kind = "framework")]
  unsafe extern "C" {
    fn CFStringCreateWithCString(
      allocator: *const c_void,
      string: *const i8,
      encoding: u32,
    ) -> *const c_void;
    fn CFRelease(value: *const c_void);
    fn CFGetTypeID(value: *const c_void) -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(
      number: *const c_void,
      kind: isize,
      value: *mut c_void,
    ) -> u8;
    fn CFDataGetTypeID() -> usize;
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
    fn CFDataGetLength(data: *const c_void) -> isize;
  }

  #[link(name = "IOKit", kind = "framework")]
  unsafe extern "C" {
    fn IOServiceMatching(name: *const i8) -> *const c_void;
    fn IOServiceGetMatchingServices(
      main_port: u32,
      matching: *const c_void,
      existing: *mut u32,
    ) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IORegistryEntryCreateCFProperty(
      entry: u32,
      key: *const c_void,
      allocator: *const c_void,
      options: u32,
    ) -> *const c_void;
  }

  struct Cf(*const c_void);

  impl Drop for Cf {
    fn drop(&mut self) {
      if !self.0.is_null() {
        unsafe { CFRelease(self.0) };
      }
    }
  }

  struct Io(u32);

  impl Drop for Io {
    fn drop(&mut self) {
      if self.0 != 0 {
        unsafe {
          IOObjectRelease(self.0);
        }
      }
    }
  }

  pub(super) fn screen_contact_counts() -> Vec<Option<i32>> {
    // Registry properties only. Opening the HID device can prompt for
    // Input Monitoring, and this getter must not.
    let matching = unsafe { IOServiceMatching(c"IOHIDDevice".as_ptr()) };
    if matching.is_null() {
      return Vec::new();
    }
    let mut raw = 0u32;
    // The matching dictionary is consumed even when this call fails.
    let status = unsafe { IOServiceGetMatchingServices(0, matching, &mut raw) };
    if status != 0 {
      if raw != 0 {
        unsafe { IOObjectRelease(raw) };
      }
      return Vec::new();
    }
    if raw == 0 {
      return Vec::new();
    }
    let iterator = Io(raw);
    let mut counts = Vec::new();
    loop {
      let service = Io(unsafe { IOIteratorNext(iterator.0) });
      if service.0 == 0 {
        break;
      }
      if let Some(count) = touch_screen_contacts(service.0) {
        counts.push(count);
      }
    }
    counts
  }

  fn touch_screen_contacts(service: u32) -> Option<Option<i32>> {
    let device = is_touch_screen_pair(
      registry_number(service, c"DeviceUsagePage"),
      registry_number(service, c"DeviceUsage"),
    );
    let primary = is_touch_screen_pair(
      registry_number(service, c"PrimaryUsagePage"),
      registry_number(service, c"PrimaryUsage"),
    );
    if !device && !primary {
      return None;
    }
    let Some(bytes) = registry_data(service, c"ReportDescriptor") else {
      return Some(None);
    };
    Some(contact_count_maximum(&bytes))
  }

  fn is_touch_screen_pair(page: Option<i32>, usage: Option<i32>) -> bool {
    match (page, usage) {
      (Some(page), Some(usage)) => is_touch_screen_usage(page, usage),
      _ => false,
    }
  }

  fn registry_number(service: u32, key: &CStr) -> Option<i32> {
    let name = cf_string(key);
    if name.0.is_null() {
      return None;
    }
    let value = Cf(unsafe {
      IORegistryEntryCreateCFProperty(service, name.0, std::ptr::null(), 0)
    });
    if value.0.is_null() {
      return None;
    }
    let mut number = 0i32;
    let ok = unsafe {
      CFGetTypeID(value.0) == CFNumberGetTypeID()
        && CFNumberGetValue(
          value.0,
          SINT32,
          &mut number as *mut i32 as *mut c_void,
        ) != 0
    };
    ok.then_some(number)
  }

  fn registry_data(service: u32, key: &CStr) -> Option<Vec<u8>> {
    let name = cf_string(key);
    if name.0.is_null() {
      return None;
    }
    let value = Cf(unsafe {
      IORegistryEntryCreateCFProperty(service, name.0, std::ptr::null(), 0)
    });
    if value.0.is_null() {
      return None;
    }
    unsafe {
      if CFGetTypeID(value.0) != CFDataGetTypeID() {
        return None;
      }
      let length = CFDataGetLength(value.0);
      if length <= 0 || length > 65_536 {
        return None;
      }
      let pointer = CFDataGetBytePtr(value.0);
      if pointer.is_null() {
        return None;
      }
      Some(std::slice::from_raw_parts(pointer, length as usize).to_vec())
    }
  }

  fn cf_string(text: &CStr) -> Cf {
    Cf(unsafe {
      CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8)
    })
  }
}

#[cfg(not(any(
  windows,
  target_os = "macos",
  all(target_os = "linux", target_pointer_width = "64"),
)))]
fn screen_contact_counts() -> Vec<Option<i32>> {
  Vec::new()
}

/// `_IOR('E', 0x40 + axis, sizeof(input_absinfo))`. The struct is 24 bytes
/// on the supported Linux targets, where the request is an unsigned long.
#[cfg(any(test, all(target_os = "linux", target_pointer_width = "64"),))]
const fn eviocgabs(axis: u32) -> u64 {
  const IOC_READ: u64 = 2;
  let nr = 0x40 + axis as u64;
  (IOC_READ << 30) | (24 << 16) | ((b'E' as u64) << 8) | nr
}

#[cfg(any(test, all(target_os = "linux", target_pointer_width = "64"),))]
const ABS_MT_SLOT: u32 = 0x2f;

/// Kernel `input_print_bitmap`: hex unsigned-long words, high word first.
/// Empty high words are omitted. The last word is word 0. Words are 64 bits
/// on the supported Linux targets.
#[cfg(any(test, all(target_os = "linux", target_pointer_width = "64"),))]
fn bitmap_has_bit(text: &str, bit: u32) -> bool {
  let mut words = Vec::new();
  for token in text.split_whitespace() {
    if token.len() > 16 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
      return false;
    }
    let Ok(word) = u64::from_str_radix(token, 16) else {
      return false;
    };
    words.push(word);
  }
  if words.is_empty() {
    return false;
  }
  let word_index = (bit / 64) as usize;
  let Some(place) = words.len().checked_sub(word_index + 1) else {
    return false;
  };
  words[place] & (1u64 << (bit % 64)) != 0
}

#[cfg(any(test, all(target_os = "linux", target_pointer_width = "64"),))]
fn is_direct_touch_screen(prop: &str, key: &str, abs: &str) -> bool {
  const INPUT_PROP_DIRECT: u32 = 0x01;
  const BTN_TOUCH: u32 = 0x14a;
  const ABS_X: u32 = 0x00;
  const ABS_Y: u32 = 0x01;
  const ABS_MT_POSITION_X: u32 = 0x35;
  const ABS_MT_POSITION_Y: u32 = 0x36;
  if !bitmap_has_bit(prop, INPUT_PROP_DIRECT) || !bitmap_has_bit(key, BTN_TOUCH)
  {
    return false;
  }
  let single = bitmap_has_bit(abs, ABS_X) && bitmap_has_bit(abs, ABS_Y);
  let multi = bitmap_has_bit(abs, ABS_MT_POSITION_X)
    && bitmap_has_bit(abs, ABS_MT_POSITION_Y);
  single || multi
}

#[cfg(any(test, target_os = "macos"))]
fn is_touch_screen_usage(page: i32, usage: i32) -> bool {
  page == 0x0D && usage == 0x04
}

/// Short HID items. A one-byte logical maximum of `0xFF` is -1 and is not a
/// contact count. The logical maximum is the global value at the next
/// Input, Output, or Feature main item after Contact Count Maximum (`0x55`
/// on the Digitizer page).
#[cfg(any(test, target_os = "macos"))]
fn contact_count_maximum(bytes: &[u8]) -> Option<i32> {
  const STACK_LIMIT: usize = 16;
  const MAIN: u8 = 0;
  const GLOBAL: u8 = 1;
  const LOCAL: u8 = 2;
  const INPUT: u8 = 0x8;
  const OUTPUT: u8 = 0x9;
  const FEATURE: u8 = 0xB;
  const USAGE_PAGE: u8 = 0;
  const LOGICAL_MAXIMUM: u8 = 2;
  const PUSH: u8 = 0xA;
  const POP: u8 = 0xB;
  const USAGE: u8 = 0;
  const USAGE_MINIMUM: u8 = 1;
  const USAGE_MAXIMUM: u8 = 2;

  let mut globals = Globals {
    usage_page: 0,
    logical_max: 0,
  };
  let mut stack = Vec::new();
  let mut pending = false;
  let mut usage_min: Option<Usage> = None;
  let mut usage_max: Option<Usage> = None;
  let mut found: Option<i32> = None;
  let mut index = 0;
  while index < bytes.len() {
    let prefix = bytes[index];
    if prefix == 0xFE {
      if bytes.len() - index < 3 {
        break;
      }
      let size = bytes[index + 1] as usize;
      let Some(next) = index
        .checked_add(3)
        .and_then(|start| start.checked_add(size))
      else {
        break;
      };
      if next > bytes.len() {
        break;
      }
      index = next;
      continue;
    }
    let size = match prefix & 0x03 {
      0 => 0,
      1 => 1,
      2 => 2,
      _ => 4,
    };
    let item_type = (prefix >> 2) & 0x03;
    let tag = prefix >> 4;
    let start = index + 1;
    let Some(end) = start.checked_add(size) else {
      break;
    };
    if end > bytes.len() {
      break;
    }
    let data = &bytes[start..end];
    index = end;
    match item_type {
      MAIN => {
        if pending && matches!(tag, INPUT | OUTPUT | FEATURE) {
          if let Some(count) = plausible(globals.logical_max) {
            found = Some(found.map_or(count, |current| current.max(count)));
          }
        }
        pending = false;
        usage_min = None;
        usage_max = None;
      }
      GLOBAL => match tag {
        USAGE_PAGE => globals.usage_page = unsigned_item(data),
        LOGICAL_MAXIMUM => globals.logical_max = signed_item(data),
        PUSH => {
          if stack.len() < STACK_LIMIT {
            stack.push(globals);
          }
        }
        POP => {
          if let Some(saved) = stack.pop() {
            globals = saved;
          }
        }
        _ => {}
      },
      LOCAL => {
        let usage = usage_item(globals.usage_page, data);
        match tag {
          USAGE => {
            if is_contact_usage(usage) {
              pending = true;
            }
          }
          USAGE_MINIMUM => {
            usage_min = Some(usage);
            if usage_range_has_contact(usage_min, usage_max) {
              pending = true;
            }
          }
          USAGE_MAXIMUM => {
            usage_max = Some(usage);
            if usage_range_has_contact(usage_min, usage_max) {
              pending = true;
            }
          }
          _ => {}
        }
      }
      _ => {}
    }
  }
  found
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy)]
struct Globals {
  usage_page: u32,
  logical_max: i64,
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy)]
struct Usage {
  page: u32,
  id: u32,
}

#[cfg(any(test, target_os = "macos"))]
fn is_contact_usage(usage: Usage) -> bool {
  usage.page == 0x0D && usage.id == 0x55
}

#[cfg(any(test, target_os = "macos"))]
fn usage_range_has_contact(min: Option<Usage>, max: Option<Usage>) -> bool {
  match (min, max) {
    (Some(min), Some(max)) => {
      min.page == 0x0D && max.page == 0x0D && min.id <= 0x55 && 0x55 <= max.id
    }
    _ => false,
  }
}

#[cfg(any(test, target_os = "macos"))]
fn usage_item(page: u32, data: &[u8]) -> Usage {
  if let [first, second, third, fourth] = data {
    let id = u32::from(u16::from_le_bytes([*first, *second]));
    let item_page = u32::from(u16::from_le_bytes([*third, *fourth]));
    Usage {
      page: if item_page == 0 { page } else { item_page },
      id,
    }
  } else {
    Usage {
      page,
      id: unsigned_item(data),
    }
  }
}

#[cfg(any(test, target_os = "macos"))]
fn unsigned_item(data: &[u8]) -> u32 {
  match data {
    [] => 0,
    [byte] => u32::from(*byte),
    [low, high] => u32::from(u16::from_le_bytes([*low, *high])),
    [first, second, third, fourth] => {
      u32::from_le_bytes([*first, *second, *third, *fourth])
    }
    _ => 0,
  }
}

/// HID data is little-endian and sign-extended. A one-byte `0xFF` is -1.
#[cfg(any(test, target_os = "macos"))]
fn signed_item(data: &[u8]) -> i64 {
  match data {
    [] => 0,
    [byte] => i64::from(*byte as i8),
    [low, high] => i64::from(i16::from_le_bytes([*low, *high])),
    [first, second, third, fourth] => {
      i64::from(i32::from_le_bytes([*first, *second, *third, *fourth]))
    }
    _ => 0,
  }
}

#[cfg(test)]
mod tests {
  use super::ABS_MT_SLOT;
  use super::bitmap_has_bit;
  use super::contact_count_maximum;
  use super::eviocgabs;
  use super::is_direct_touch_screen;
  use super::is_touch_screen_usage;
  use super::largest_contact_count;
  use super::max_touch_points;
  use super::plausible;

  #[test]
  fn contact_count_uses_the_largest_reported_maximum() {
    assert_eq!(largest_contact_count(&[]), 0);
    assert_eq!(largest_contact_count(&[None]), 1);
    assert_eq!(largest_contact_count(&[Some(0), Some(-1)]), 1);
    assert_eq!(largest_contact_count(&[Some(5)]), 5);
    assert_eq!(largest_contact_count(&[Some(5), Some(10)]), 10);
    assert_eq!(largest_contact_count(&[Some(4), None]), 4);
    assert_eq!(plausible(0), None);
    assert_eq!(plausible(1), Some(1));
    assert_eq!(plausible(1024), Some(1024));
    assert_eq!(plausible(1025), None);
    assert_eq!(plausible(-1), None);
  }

  #[test]
  fn hid_contact_count_maximum_is_the_logical_maximum() {
    assert_eq!(
      contact_count_maximum(&[
        0x05, 0x0D, 0x09, 0x55, 0x25, 0x0A, 0x75, 0x08, 0x95, 0x01, 0xB1, 0x02
      ]),
      Some(10)
    );
    assert_eq!(
      contact_count_maximum(&[0x05, 0x0D, 0x09, 0x55, 0x25, 0xFF, 0xB1, 0x02]),
      None
    );
    assert_eq!(
      contact_count_maximum(&[
        0x05, 0x0D, 0x09, 0x55, 0x26, 0x0A, 0x00, 0xB1, 0x02
      ]),
      Some(10)
    );
    assert_eq!(
      contact_count_maximum(&[0x05, 0x0D, 0x25, 0x0A, 0x09, 0x55, 0xB1, 0x02]),
      Some(10)
    );
    assert_eq!(
      contact_count_maximum(&[
        0x0B, 0x55, 0x00, 0x0D, 0x00, 0x25, 0x0A, 0xB1, 0x02
      ]),
      Some(10)
    );
    assert_eq!(
      contact_count_maximum(&[
        0x05, 0x0D, 0x25, 0x0A, 0xA4, 0x25, 0x63, 0xB4, 0x09, 0x55, 0xB1, 0x02
      ]),
      Some(10)
    );
    assert_eq!(
      contact_count_maximum(&[
        0x05, 0x0D, 0x09, 0x55, 0xA1, 0x01, 0x25, 0x0A, 0xB1, 0x02
      ]),
      None
    );
    assert_eq!(
      contact_count_maximum(&[
        0x05, 0x0D, 0x19, 0x50, 0x29, 0x60, 0x25, 0x07, 0xB1, 0x02
      ]),
      Some(7)
    );
    assert_eq!(
      contact_count_maximum(&[
        0x05, 0x0D, 0x19, 0x01, 0x29, 0x10, 0x25, 0x07, 0xB1, 0x02
      ]),
      None
    );
    assert_eq!(
      contact_count_maximum(&[
        0x05, 0x0D, 0x09, 0x55, 0x25, 0x04, 0xB1, 0x02, 0x09, 0x55, 0x25, 0xFF,
        0xB1, 0x02
      ]),
      Some(4)
    );
    assert_eq!(contact_count_maximum(&[0x05, 0x0D, 0x09]), None);
    assert_eq!(
      contact_count_maximum(&[
        0xFE, 0x02, 0x00, 0xAA, 0xBB, 0x05, 0x0D, 0x09, 0x55, 0x25, 0x05, 0xB1,
        0x02
      ]),
      Some(5)
    );
    let mut pushed = vec![0x05, 0x0D, 0x25, 0x03];
    pushed.extend(std::iter::repeat_n(0xA4, 17));
    pushed.extend([0x25, 0x09]);
    pushed.extend(std::iter::repeat_n(0xB4, 17));
    pushed.extend([0x09, 0x55, 0xB1, 0x02]);
    assert_eq!(contact_count_maximum(&pushed), Some(3));
    assert_eq!(
      contact_count_maximum(&[
        0x05, 0x0D, 0x09, 0x55, 0x26, 0x00, 0x04, 0xB1, 0x02
      ]),
      Some(1024)
    );
    assert_eq!(
      contact_count_maximum(&[
        0x05, 0x0D, 0x09, 0x55, 0x26, 0x01, 0x04, 0xB1, 0x02
      ]),
      None
    );
  }

  #[test]
  fn direct_touch_screen_requires_contact_and_axes() {
    assert!(bitmap_has_bit("400 0 0 0 0 0", 0x14a));
    assert!(bitmap_has_bit("3", 0));
    assert!(bitmap_has_bit("3", 1));
    assert!(bitmap_has_bit("2", 1));
    assert!(!bitmap_has_bit("1", 1));
    assert!(!bitmap_has_bit("0", 0x14a));
    assert!(!bitmap_has_bit("zz", 0));
    assert!(is_direct_touch_screen("2", "400 0 0 0 0 0", "3"));
    assert!(is_direct_touch_screen("3", "400 0 0 0 0 0", "3"));
    assert!(!is_direct_touch_screen("1", "400 0 0 0 0 0", "3"));
    assert!(!is_direct_touch_screen("2", "0", "3"));
    assert!(is_direct_touch_screen(
      "2",
      "400 0 0 0 0 0",
      "60000000000000"
    ));
    assert!(!is_direct_touch_screen("2", "400 0 0 0 0 0", "1"));
    assert_eq!(eviocgabs(ABS_MT_SLOT), 0x8018_456f);
  }

  #[test]
  fn touch_pad_is_not_a_touch_screen() {
    assert!(is_touch_screen_usage(0x0D, 0x04));
    assert!(!is_touch_screen_usage(0x0D, 0x05));
    assert!(!is_touch_screen_usage(0x0D, 0x01));
    assert!(!is_touch_screen_usage(0x01, 0x02));
  }

  #[test]
  fn host_touch_points_are_in_range() {
    let count = max_touch_points();
    assert!((0..=1024).contains(&count), "{count}");
  }
}
