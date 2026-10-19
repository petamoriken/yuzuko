//! Core Audio default-output buffer size.

const fn fourcc(text: &[u8; 4]) -> u32 {
  u32::from_be_bytes(*text)
}

#[repr(C)]
struct Address {
  selector: u32,
  scope: u32,
  element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
  fn AudioObjectGetPropertyData(
    object: u32,
    address: *const Address,
    qualifier_size: u32,
    qualifier: *const std::ffi::c_void,
    data_size: *mut u32,
    data: *mut std::ffi::c_void,
  ) -> i32;
}

pub fn output_buffer_frames() -> Option<u32> {
  let device = property_u32(1, fourcc(b"dOut"))?;
  if device == 0 {
    return None;
  }
  let frames = property_u32(device, fourcc(b"fsiz"))?;
  if frames == 0 { None } else { Some(frames) }
}

fn property_u32(object: u32, selector: u32) -> Option<u32> {
  let address = Address {
    selector,
    scope: fourcc(b"glob"),
    element: 0,
  };
  let mut size = 4u32;
  let mut value = 0u32;
  let status = unsafe {
    AudioObjectGetPropertyData(
      object,
      &address,
      0,
      std::ptr::null(),
      &mut size,
      (&mut value as *mut u32).cast(),
    )
  };
  if status != 0 || size < 4 {
    None
  } else {
    Some(value)
  }
}
