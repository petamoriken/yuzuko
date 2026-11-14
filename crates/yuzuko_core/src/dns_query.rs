//! Typed DNS records through the system resolver.
//!
//! `"A"` and `"AAAA"` stay on `getaddrinfo`. Every other type asks the
//! system resolver and parses the answer. The wire parser does not follow a
//! `CNAME` into another type.

#[cfg(unix)]
use std::sync::Mutex;

#[cfg(any(unix, test))]
const CLASS_IN: u16 = 1;
#[cfg(unix)]
const NO_DATA: i32 = 4;
#[cfg(any(unix, test))]
const HOP_LIMIT: u8 = 64;

#[cfg(unix)]
static RESOLVER: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
  Cname,
  Ns,
  Ptr,
  Mx,
  Txt,
  Srv,
  Soa,
  Caa,
  Naptr,
}

impl Kind {
  pub(crate) fn parse(name: &str) -> Option<Self> {
    Some(match name {
      "CNAME" => Self::Cname,
      "NS" => Self::Ns,
      "PTR" => Self::Ptr,
      "MX" => Self::Mx,
      "TXT" => Self::Txt,
      "SRV" => Self::Srv,
      "SOA" => Self::Soa,
      "CAA" => Self::Caa,
      "NAPTR" => Self::Naptr,
      _ => return None,
    })
  }

  fn wire(self) -> u16 {
    match self {
      Self::Cname => 5,
      Self::Ns => 2,
      Self::Ptr => 12,
      Self::Mx => 15,
      Self::Txt => 16,
      Self::Srv => 33,
      Self::Soa => 6,
      Self::Caa => 257,
      Self::Naptr => 35,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MxRecord {
  pub preference: u16,
  pub exchange: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SrvRecord {
  pub priority: u16,
  pub weight: u16,
  pub port: u16,
  pub target: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SoaRecord {
  pub mname: String,
  pub rname: String,
  pub serial: u32,
  pub refresh: u32,
  pub retry: u32,
  pub expire: u32,
  pub minimum: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CaaRecord {
  pub critical: bool,
  pub tag: String,
  pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NaptrRecord {
  pub order: u16,
  pub preference: u16,
  pub flags: String,
  pub services: String,
  pub regexp: String,
  pub replacement: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Typed {
  Names(Vec<String>),
  Mx(Vec<MxRecord>),
  Txt(Vec<Vec<String>>),
  Srv(Vec<SrvRecord>),
  Soa(Vec<SoaRecord>),
  Caa(Vec<CaaRecord>),
  Naptr(Vec<NaptrRecord>),
}

impl Typed {
  fn empty(kind: Kind) -> Self {
    match kind {
      Kind::Cname | Kind::Ns | Kind::Ptr => Self::Names(Vec::new()),
      Kind::Mx => Self::Mx(Vec::new()),
      Kind::Txt => Self::Txt(Vec::new()),
      Kind::Srv => Self::Srv(Vec::new()),
      Kind::Soa => Self::Soa(Vec::new()),
      Kind::Caa => Self::Caa(Vec::new()),
      Kind::Naptr => Self::Naptr(Vec::new()),
    }
  }
}

pub(crate) fn lookup(host: &str, kind: Kind) -> Result<Typed, ()> {
  #[cfg(unix)]
  {
    let packet = unix_query(host, kind.wire())?;
    if packet.is_empty() {
      return Ok(Typed::empty(kind));
    }
    parse_message(&packet, kind)
  }
  #[cfg(windows)]
  {
    windows_query(host, kind)
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = (host, kind);
    Err(())
  }
}

#[cfg(any(unix, test))]
fn parse_message(message: &[u8], kind: Kind) -> Result<Typed, ()> {
  if message.len() < 12 {
    return Err(());
  }
  let flags = u16_at(message, 2)?;
  let rcode = flags & 0x000f;
  if rcode != 0 {
    return Err(());
  }
  let questions = u16_at(message, 4)? as usize;
  let answers = u16_at(message, 6)? as usize;
  let mut pos = 12;
  for _ in 0..questions {
    pos = skip_name(message, pos)?;
    pos = pos.checked_add(4).ok_or(())?;
    if pos > message.len() {
      return Err(());
    }
  }
  let mut found = Typed::empty(kind);
  let qtype = kind.wire();
  for _ in 0..answers {
    pos = skip_name(message, pos)?;
    if pos.checked_add(10).ok_or(())? > message.len() {
      return Err(());
    }
    let typ = u16_at(message, pos)?;
    let class = u16_at(message, pos + 2)?;
    let rdlen = u16_at(message, pos + 8)? as usize;
    let rdata = pos + 10;
    let next = rdata.checked_add(rdlen).ok_or(())?;
    if next > message.len() {
      return Err(());
    }
    if typ == qtype && class == CLASS_IN {
      push_rdata(&mut found, message, rdata, next)?;
    }
    pos = next;
  }
  Ok(found)
}

#[cfg(any(unix, test))]
fn push_rdata(
  found: &mut Typed,
  message: &[u8],
  start: usize,
  end: usize,
) -> Result<(), ()> {
  match found {
    Typed::Names(rows) => {
      let (name, at) = read_name(message, start)?;
      if at > end {
        return Err(());
      }
      rows.push(name);
    }
    Typed::Mx(rows) => {
      if start + 2 > end {
        return Err(());
      }
      let preference = u16_at(message, start)?;
      let (exchange, at) = read_name(message, start + 2)?;
      if at > end {
        return Err(());
      }
      rows.push(MxRecord {
        preference,
        exchange,
      });
    }
    Typed::Txt(rows) => {
      let mut strings = Vec::new();
      let mut at = start;
      while at < end {
        let (text, next) = read_char_string(message, at, end)?;
        strings.push(text);
        at = next;
      }
      rows.push(strings);
    }
    Typed::Srv(rows) => {
      if start + 6 > end {
        return Err(());
      }
      let priority = u16_at(message, start)?;
      let weight = u16_at(message, start + 2)?;
      let port = u16_at(message, start + 4)?;
      let (target, at) = read_name(message, start + 6)?;
      if at > end {
        return Err(());
      }
      rows.push(SrvRecord {
        priority,
        weight,
        port,
        target,
      });
    }
    Typed::Soa(rows) => {
      let (mname, at) = read_name(message, start)?;
      if at > end {
        return Err(());
      }
      let (rname, at) = read_name(message, at)?;
      if at.checked_add(20).ok_or(())? > end {
        return Err(());
      }
      rows.push(SoaRecord {
        mname,
        rname,
        serial: u32_at(message, at)?,
        refresh: u32_at(message, at + 4)?,
        retry: u32_at(message, at + 8)?,
        expire: u32_at(message, at + 12)?,
        minimum: u32_at(message, at + 16)?,
      });
    }
    Typed::Caa(rows) => {
      if start + 2 > end {
        return Err(());
      }
      let flag = message[start];
      let tag_len = message[start + 1] as usize;
      let tag_at = start + 2;
      let value_at = tag_at.checked_add(tag_len).ok_or(())?;
      if value_at > end {
        return Err(());
      }
      rows.push(CaaRecord {
        critical: (flag & 1) != 0,
        tag: String::from_utf8_lossy(&message[tag_at..value_at]).into_owned(),
        value: String::from_utf8_lossy(&message[value_at..end]).into_owned(),
      });
    }
    Typed::Naptr(rows) => {
      if start + 4 > end {
        return Err(());
      }
      let order = u16_at(message, start)?;
      let preference = u16_at(message, start + 2)?;
      let (flags, at) = read_char_string(message, start + 4, end)?;
      let (services, at) = read_char_string(message, at, end)?;
      let (regexp, at) = read_char_string(message, at, end)?;
      let (replacement, name_end) = read_name(message, at)?;
      if name_end > end {
        return Err(());
      }
      rows.push(NaptrRecord {
        order,
        preference,
        flags,
        services,
        regexp,
        replacement,
      });
    }
  }
  Ok(())
}

#[cfg(any(unix, test))]
fn u16_at(message: &[u8], pos: usize) -> Result<u16, ()> {
  let end = pos.checked_add(2).ok_or(())?;
  let bytes = message.get(pos..end).ok_or(())?;
  Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

#[cfg(any(unix, test))]
fn u32_at(message: &[u8], pos: usize) -> Result<u32, ()> {
  let end = pos.checked_add(4).ok_or(())?;
  let bytes = message.get(pos..end).ok_or(())?;
  Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(any(unix, test))]
fn read_char_string(
  message: &[u8],
  pos: usize,
  limit: usize,
) -> Result<(String, usize), ()> {
  if pos >= limit || pos >= message.len() {
    return Err(());
  }
  let len = message[pos] as usize;
  let start = pos + 1;
  let end = start.checked_add(len).ok_or(())?;
  if end > limit || end > message.len() {
    return Err(());
  }
  Ok((
    String::from_utf8_lossy(&message[start..end]).into_owned(),
    end,
  ))
}

/// A domain name and the position just after it in the uncompressed stream.
#[cfg(any(unix, test))]
fn read_name(message: &[u8], mut pos: usize) -> Result<(String, usize), ()> {
  let mut labels = Vec::new();
  let mut hops = 0u8;
  let mut end = None;
  loop {
    if pos >= message.len() {
      return Err(());
    }
    let len = message[pos];
    if len == 0 {
      if end.is_none() {
        end = Some(pos + 1);
      }
      break;
    }
    if (len & 0xC0) == 0xC0 {
      if pos + 1 >= message.len() {
        return Err(());
      }
      if end.is_none() {
        end = Some(pos + 2);
      }
      let offset = (((len as usize) & 0x3F) << 8) | message[pos + 1] as usize;
      hops = hops.checked_add(1).ok_or(())?;
      if hops > HOP_LIMIT {
        return Err(());
      }
      pos = offset;
      continue;
    }
    if (len & 0xC0) != 0 {
      return Err(());
    }
    let start = pos + 1;
    let stop = start + len as usize;
    if stop > message.len() {
      return Err(());
    }
    labels.push(String::from_utf8_lossy(&message[start..stop]).into_owned());
    pos = stop;
    hops = hops.checked_add(1).ok_or(())?;
    if hops > HOP_LIMIT {
      return Err(());
    }
  }
  Ok((labels.join("."), end.ok_or(())?))
}

#[cfg(any(unix, test))]
fn skip_name(message: &[u8], pos: usize) -> Result<usize, ()> {
  let (_, end) = read_name(message, pos)?;
  Ok(end)
}

#[cfg(unix)]
fn unix_query(host: &str, qtype: u16) -> Result<Vec<u8>, ()> {
  let Ok(name) = std::ffi::CString::new(host) else {
    return Err(());
  };
  let mut buf = vec![0u8; 65535];
  let _guard = RESOLVER.lock().unwrap_or_else(|error| error.into_inner());
  let len = unsafe {
    res_query(
      name.as_ptr(),
      CLASS_IN as libc::c_int,
      qtype as libc::c_int,
      buf.as_mut_ptr(),
      buf.len() as libc::c_int,
    )
  };
  if len < 0 {
    return if h_errno_value() == NO_DATA {
      Ok(Vec::new())
    } else {
      Err(())
    };
  }
  let len = len as usize;
  if len > buf.len() {
    return Err(());
  }
  buf.truncate(len);
  Ok(buf)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[link(name = "resolv")]
unsafe extern "C" {
  #[link_name = "res_9_query"]
  fn res_query(
    dname: *const libc::c_char,
    class: libc::c_int,
    type_: libc::c_int,
    answer: *mut u8,
    anslen: libc::c_int,
  ) -> libc::c_int;
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
#[link(name = "resolv")]
unsafe extern "C" {
  fn res_query(
    dname: *const libc::c_char,
    class: libc::c_int,
    type_: libc::c_int,
    answer: *mut u8,
    anslen: libc::c_int,
  ) -> libc::c_int;
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe extern "C" {
  static h_errno: libc::c_int;
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn h_errno_value() -> i32 {
  unsafe { h_errno }
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
unsafe extern "C" {
  fn __h_errno_location() -> *mut libc::c_int;
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
fn h_errno_value() -> i32 {
  unsafe { *__h_errno_location() }
}

#[cfg(windows)]
fn windows_query(host: &str, kind: Kind) -> Result<Typed, ()> {
  use std::ffi::CString;

  use windows_sys::Win32::NetworkManagement::Dns::{
    DNS_QUERY_STANDARD, DNS_RECORDA, DnsFree, DnsFreeRecordList, DnsQuery_A,
  };

  const DNS_INFO_NO_RECORDS: u32 = 9501;
  let Ok(name) = CString::new(host) else {
    return Err(());
  };
  let qtype = kind.wire();
  let mut head: *mut DNS_RECORDA = std::ptr::null_mut();
  let status = unsafe {
    DnsQuery_A(
      name.as_ptr() as windows_sys::core::PCSTR,
      qtype,
      DNS_QUERY_STANDARD,
      std::ptr::null_mut(),
      &mut head,
      std::ptr::null_mut(),
    )
  };
  if status == DNS_INFO_NO_RECORDS {
    return Ok(Typed::empty(kind));
  }
  if status != 0 || head.is_null() {
    if !head.is_null() {
      unsafe { DnsFree(head.cast(), DnsFreeRecordList) };
    }
    return Err(());
  }
  let mut found = Typed::empty(kind);
  let mut record = head;
  while !record.is_null() {
    let current = unsafe { &*record };
    let section = unsafe { current.Flags.DW } & 0b11;
    if section == 1
      && current.wType == qtype
      && windows_push(&mut found, current).is_err()
    {
      unsafe { DnsFree(head.cast(), DnsFreeRecordList) };
      return Err(());
    }
    record = current.pNext;
  }
  unsafe { DnsFree(head.cast(), DnsFreeRecordList) };
  Ok(found)
}

#[cfg(windows)]
fn windows_push(
  found: &mut Typed,
  record: &windows_sys::Win32::NetworkManagement::Dns::DNS_RECORDA,
) -> Result<(), ()> {
  match found {
    Typed::Names(rows) => {
      let data = unsafe { record.Data.PTR };
      rows.push(dns_text(data.pNameHost));
    }
    Typed::Mx(rows) => {
      let data = unsafe { record.Data.MX };
      rows.push(MxRecord {
        preference: data.wPreference,
        exchange: dns_text(data.pNameExchange),
      });
    }
    Typed::Txt(rows) => {
      let data = unsafe { record.Data.TXT };
      let count = data.dwStringCount as usize;
      let base = data.pStringArray.as_ptr();
      let mut strings = Vec::with_capacity(count);
      for index in 0..count {
        let pointer = unsafe { *base.add(index) };
        strings.push(lossy_c(pointer));
      }
      rows.push(strings);
    }
    Typed::Srv(rows) => {
      let data = unsafe { record.Data.SRV };
      rows.push(SrvRecord {
        priority: data.wPriority,
        weight: data.wWeight,
        port: data.wPort,
        target: dns_text(data.pNameTarget),
      });
    }
    Typed::Soa(rows) => {
      let data = unsafe { record.Data.SOA };
      rows.push(SoaRecord {
        mname: dns_text(data.pNamePrimaryServer),
        rname: dns_text(data.pNameAdministrator),
        serial: data.dwSerialNo,
        refresh: data.dwRefresh,
        retry: data.dwRetry,
        expire: data.dwExpire,
        minimum: data.dwDefaultTtl,
      });
    }
    Typed::Caa(rows) => {
      let data = unsafe { record.Data.UNKNOWN };
      let count = data.dwByteCount as usize;
      if count < 2 {
        return Err(());
      }
      let bytes =
        unsafe { std::slice::from_raw_parts(data.bData.as_ptr(), count) };
      let tag_len = bytes[1] as usize;
      if tag_len + 2 > bytes.len() {
        return Err(());
      }
      rows.push(CaaRecord {
        critical: (bytes[0] & 1) != 0,
        tag: String::from_utf8_lossy(&bytes[2..2 + tag_len]).into_owned(),
        value: String::from_utf8_lossy(&bytes[2 + tag_len..]).into_owned(),
      });
    }
    Typed::Naptr(rows) => {
      let data = unsafe { record.Data.NAPTR };
      rows.push(NaptrRecord {
        order: data.wOrder,
        preference: data.wPreference,
        flags: lossy_c(data.pFlags),
        services: lossy_c(data.pService),
        regexp: lossy_c(data.pRegularExpression),
        replacement: dns_text(data.pReplacement),
      });
    }
  }
  Ok(())
}

#[cfg(windows)]
fn dns_text(pointer: windows_sys::core::PSTR) -> String {
  let text = lossy_c(pointer);
  text.strip_suffix('.').unwrap_or(&text).to_string()
}

#[cfg(windows)]
fn lossy_c(pointer: windows_sys::core::PSTR) -> String {
  if pointer.is_null() {
    return String::new();
  }
  unsafe {
    std::ffi::CStr::from_ptr(pointer.cast())
      .to_string_lossy()
      .into_owned()
  }
}

#[cfg(test)]
mod tests {
  use super::{Kind, Typed, parse_message};

  fn mx_packet() -> Vec<u8> {
    vec![
      0x00, 0x01, 0x81, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
      0x07, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 0x03, b'c', b'o', b'm',
      0x00, 0x00, 0x0f, 0x00, 0x01, 0xC0, 0x0C, 0x00, 0x0f, 0x00, 0x01, 0x00,
      0x00, 0x00, 0x3c, 0x00, 0x09, 0x00, 0x0a, 0x04, b'm', b'a', b'i', b'l',
      0xC0, 0x0C,
    ]
  }

  #[test]
  fn mx_answer_uses_compression_and_ignores_another_type() {
    let parsed = parse_message(&mx_packet(), Kind::Mx).expect("mx");
    let Typed::Mx(rows) = parsed else {
      panic!("not mx");
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].preference, 10);
    assert_eq!(rows[0].exchange, "mail.example.com");

    let mut mixed = mx_packet();
    mixed[7] = 2;
    // Insert a CNAME answer before the MX by raising ANCOUNT and prefixing
    // a record that is not MX. The existing MX stays the second answer.
    let cname = [
      0xC0, 0x0C, 0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x3c, 0x00, 0x02,
      0xC0, 0x0C,
    ];
    let at = 12 + 1 + 7 + 1 + 3 + 1 + 4;
    mixed.splice(at..at, cname);
    let parsed = parse_message(&mixed, Kind::Mx).expect("mixed");
    let Typed::Mx(rows) = parsed else {
      panic!("not mx");
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].exchange, "mail.example.com");
    let Typed::Names(names) = parse_message(&mixed, Kind::Cname).unwrap()
    else {
      panic!("cname");
    };
    assert_eq!(names, vec!["example.com".to_string()]);
  }

  #[test]
  fn nxdomain_and_a_pointer_loop_fail() {
    let mut packet = mx_packet();
    packet[3] = 0x83;
    assert!(parse_message(&packet, Kind::Mx).is_err());
    let looped = [
      0x00, 0x01, 0x81, 0x80, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
      0xC0, 0x0C, 0x00, 0x0f, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04,
      0x00, 0x01, 0xC0, 0x0C,
    ];
    assert!(parse_message(&looped, Kind::Mx).is_err());
  }

  #[test]
  fn txt_caa_and_srv_rdata_keep_every_record() {
    let txt = [
      0x00, 0x01, 0x81, 0x80, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
      0x00, 0x00, 0x10, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x03,
      b'f', b'o', b'o', 0x03, b'b', b'a', b'r',
    ];
    let Typed::Txt(rows) = parse_message(&txt, Kind::Txt).unwrap() else {
      panic!("txt");
    };
    assert_eq!(rows, vec![vec!["foo".to_string(), "bar".to_string()]]);

    let caa = [
      0x00, 0x01, 0x81, 0x80, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
      0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x01,
      0x05, b'i', b's', b's', b'u', b'e', b'c', b'a',
    ];
    let Typed::Caa(rows) = parse_message(&caa, Kind::Caa).unwrap() else {
      panic!("caa");
    };
    assert!(rows[0].critical);
    assert_eq!(rows[0].tag, "issue");
    assert_eq!(rows[0].value, "ca");

    let srv = [
      0x00, 0x01, 0x81, 0x80, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
      0x00, 0x00, 0x21, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0f, 0x00,
      0x01, 0x00, 0x02, 0x01, 0xbb, 0x07, b'e', b'x', b'a', b'm', b'p', b'l',
      b'e', 0x00,
    ];
    let Typed::Srv(rows) = parse_message(&srv, Kind::Srv).unwrap() else {
      panic!("srv");
    };
    assert_eq!(rows[0].priority, 1);
    assert_eq!(rows[0].weight, 2);
    assert_eq!(rows[0].port, 443);
    assert_eq!(rows[0].target, "example");
  }
}
