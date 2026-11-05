//! Installed fonts for `local()` and `queryLocalFonts`.
//!
//! The catalog is the system collection fontique can see. Faces registered
//! through `FontFace` are not part of it. Name strings prefer `en-US`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use fontique::FontInfo;
use fontique::FontStyle;
use fontique::SourceKind;
use skrifa::FontRef;
use skrifa::MetadataProvider;
use skrifa::string::StringId;
use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_core::op::new_dom_exception;
use yuzuko_macros::op;

struct FaceRecord {
  postscript: String,
  full: String,
  family: String,
  style: String,
  enumerable: bool,
  specific: Vec<String>,
  families: Vec<String>,
  weight: f32,
  width_ratio: f32,
  slope: FontStyle,
  info: FontInfo,
}

std::thread_local! {
  static CATALOG: RefCell<Option<Vec<FaceRecord>>> = const { RefCell::new(None) };
}

struct Names {
  postscript: String,
  full: String,
  family: String,
  style: String,
  specific: Vec<String>,
  families: Vec<String>,
}

pub(super) fn bytes_for_local(
  name: &str,
  style: &str,
  weight: &str,
  width: &str,
) -> Option<Vec<u8>> {
  with_catalog(|faces| {
    let face = pick(faces, name, style, weight, width)?;
    let index = face.info.index();
    let bytes = file_bytes(&face.info)?;
    extract_face(&bytes, index)
  })
}

fn with_catalog<T>(body: impl FnOnce(&[FaceRecord]) -> T) -> T {
  CATALOG.with(|slot| {
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
      *slot = Some(build_catalog());
    }
    body(slot.as_deref().unwrap_or(&[]))
  })
}

fn build_catalog() -> Vec<FaceRecord> {
  let built =
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(build_catalog_inner));
  built.unwrap_or_default()
}

fn build_catalog_inner() -> Vec<FaceRecord> {
  let mut collection = fontique::Collection::new(fontique::CollectionOptions {
    shared: false,
    system_fonts: true,
  });
  let family_names: Vec<String> =
    collection.family_names().map(str::to_string).collect();
  let mut cache =
    fontique::SourceCache::new(fontique::SourceCacheOptions::default());
  let mut faces: Vec<FaceRecord> = Vec::new();
  let mut index_of: HashMap<(PathBuf, u32), usize> = HashMap::new();
  for family_name in family_names {
    let Some(family) = collection.family_by_name(&family_name) else {
      continue;
    };
    let label = family.name().to_string();
    for font in family.fonts() {
      let Some(key) = source_key(font) else {
        continue;
      };
      if let Some(slot) = index_of.get(&key).copied() {
        push_key(&mut faces[slot].families, &label);
        continue;
      }
      let font = font.clone();
      let described =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
          describe(&font, &mut cache, &label)
        }));
      let Ok(Some(record)) = described else {
        continue;
      };
      index_of.insert(key, faces.len());
      faces.push(record);
    }
  }
  faces
}

fn source_key(font: &FontInfo) -> Option<(PathBuf, u32)> {
  match font.source().kind() {
    SourceKind::Path(path) => Some((path.as_ref().to_path_buf(), font.index())),
    SourceKind::Memory(_) => None,
  }
}

fn describe(
  font: &FontInfo,
  cache: &mut fontique::SourceCache,
  family_label: &str,
) -> Option<FaceRecord> {
  let blob = font.load(Some(cache))?;
  let index = font.index();
  let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    read_names(blob.data(), index, family_label)
  }));
  let names = parsed.ok().flatten()?;
  let enumerable = valid_postscript(&names.postscript);
  Some(FaceRecord {
    postscript: names.postscript,
    full: names.full,
    family: names.family,
    style: names.style,
    enumerable,
    specific: names.specific,
    families: names.families,
    weight: font.weight().value(),
    width_ratio: font.width().ratio(),
    slope: font.style(),
    info: font.clone(),
  })
}

fn read_names(bytes: &[u8], index: u32, family_label: &str) -> Option<Names> {
  let font = FontRef::from_index(bytes, index).ok()?;
  let postscript =
    name_of(&font, StringId::POSTSCRIPT_NAME).unwrap_or_default();
  let full = name_of(&font, StringId::FULL_NAME).unwrap_or_default();
  let family1 = name_of(&font, StringId::FAMILY_NAME).unwrap_or_default();
  let family16 =
    name_of(&font, StringId::TYPOGRAPHIC_FAMILY_NAME).unwrap_or_default();
  let style2 = name_of(&font, StringId::SUBFAMILY_NAME).unwrap_or_default();
  let style17 =
    name_of(&font, StringId::TYPOGRAPHIC_SUBFAMILY_NAME).unwrap_or_default();
  let family = if !family1.is_empty() {
    family1.clone()
  } else if !family16.is_empty() {
    family16.clone()
  } else if !family_label.is_empty() {
    family_label.to_string()
  } else {
    return None;
  };
  let style = if !style2.is_empty() {
    style2
  } else if !style17.is_empty() {
    style17
  } else {
    "Regular".to_string()
  };
  let full = if !full.is_empty() {
    full
  } else if style.eq_ignore_ascii_case("regular") {
    family.clone()
  } else {
    format!("{family} {style}")
  };
  let mut specific = Vec::new();
  push_key(&mut specific, &postscript);
  push_key(&mut specific, &full);
  let mut families = Vec::new();
  push_key(&mut families, &family1);
  push_key(&mut families, &family16);
  push_key(&mut families, family_label);
  push_key(&mut families, &family);
  Some(Names {
    postscript,
    full,
    family,
    style,
    specific,
    families,
  })
}

fn name_of(font: &FontRef<'_>, id: StringId) -> Option<String> {
  let text = font.localized_strings(id).english_or_first()?.to_string();
  let text = text.trim();
  if text.is_empty() {
    None
  } else {
    Some(text.to_string())
  }
}

fn push_key(keys: &mut Vec<String>, value: &str) {
  let key = value.trim().to_lowercase();
  if key.is_empty() || keys.iter().any(|item| item == &key) {
    return;
  }
  keys.push(key);
}

fn valid_postscript(name: &str) -> bool {
  let count = name.chars().count();
  if count == 0 || count >= 64 {
    return false;
  }
  name.chars().all(|ch| {
    let code = ch as u32;
    (0x21..=0x7E).contains(&code)
      && !matches!(
        ch,
        '[' | ']' | '(' | ')' | '{' | '}' | '<' | '>' | '/' | '%'
      )
  })
}

fn pick<'a>(
  faces: &'a [FaceRecord],
  name: &str,
  style: &str,
  weight: &str,
  width: &str,
) -> Option<&'a FaceRecord> {
  let key = name.trim().to_lowercase();
  if key.is_empty() {
    return None;
  }
  let specific: Vec<&FaceRecord> = faces
    .iter()
    .filter(|face| face.specific.iter().any(|item| item == &key))
    .collect();
  let chosen = if specific.is_empty() {
    faces
      .iter()
      .filter(|face| face.families.iter().any(|item| item == &key))
      .collect()
  } else {
    specific
  };
  if chosen.is_empty() {
    None
  } else {
    Some(closest(chosen, style, weight, width))
  }
}

fn closest<'a>(
  faces: Vec<&'a FaceRecord>,
  style: &str,
  weight: &str,
  width: &str,
) -> &'a FaceRecord {
  let want_style = requested_style(style);
  let want_weight = weight_value(weight);
  let want_width = width_percent(width);
  faces
    .into_iter()
    .min_by_key(|face| {
      let width_dist =
        ((face.width_ratio * 100.0).round() as i32 - want_width).unsigned_abs();
      let style_dist = style_distance(face.slope, want_style);
      let weight_dist = (face.weight - want_weight).abs() as u32;
      (width_dist, style_dist, weight_dist)
    })
    .expect("local face")
}

fn requested_style(value: &str) -> FontStyle {
  let value = value.trim().to_ascii_lowercase();
  if value.starts_with("italic") {
    FontStyle::Italic
  } else if value.starts_with("oblique") {
    FontStyle::Oblique(Some(14.0))
  } else {
    FontStyle::Normal
  }
}

fn style_distance(face: FontStyle, want: FontStyle) -> u8 {
  let class = |style: FontStyle| match style {
    FontStyle::Normal => 0,
    FontStyle::Italic => 1,
    FontStyle::Oblique(_) => 2,
  };
  let face = class(face);
  let want = class(want);
  if face == want {
    0
  } else if face == 0 || want == 0 {
    2
  } else {
    1
  }
}

fn weight_value(value: &str) -> f32 {
  let Some(token) = value.split_whitespace().next() else {
    return 400.0;
  };
  match token.to_ascii_lowercase().as_str() {
    "normal" => 400.0,
    "bold" => 700.0,
    _ => token
      .parse::<f32>()
      .ok()
      .filter(|number| number.is_finite())
      .unwrap_or(400.0),
  }
}

fn width_percent(value: &str) -> i32 {
  let Some(token) = value.split_whitespace().next() else {
    return 100;
  };
  let percent = match token.to_ascii_lowercase().as_str() {
    "ultra-condensed" => 50.0,
    "extra-condensed" => 62.5,
    "condensed" => 75.0,
    "semi-condensed" => 87.5,
    "normal" => 100.0,
    "semi-expanded" => 112.5,
    "expanded" => 125.0,
    "extra-expanded" => 150.0,
    "ultra-expanded" => 200.0,
    _ => token
      .strip_suffix('%')
      .and_then(|number| number.parse::<f32>().ok())
      .filter(|number| number.is_finite())
      .unwrap_or(100.0),
  };
  percent.round() as i32
}

fn file_bytes(info: &FontInfo) -> Option<Vec<u8>> {
  let loaded =
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| info.load(None)));
  let blob = loaded.ok().flatten()?;
  Some(blob.data().to_vec())
}

/// One face from a font file. A TrueType collection is rewritten so the
/// table offsets stand alone. Any other file is returned unchanged.
fn extract_face(bytes: &[u8], index: u32) -> Option<Vec<u8>> {
  if bytes.len() < 12 {
    return None;
  }
  let magic = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
  if magic != u32::from_be_bytes(*b"ttcf") {
    return Some(bytes.to_vec());
  }
  let count = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
  if index >= count {
    return None;
  }
  let entry = 12 + index as usize * 4;
  if entry + 4 > bytes.len() {
    return None;
  }
  let face_at = u32::from_be_bytes([
    bytes[entry],
    bytes[entry + 1],
    bytes[entry + 2],
    bytes[entry + 3],
  ]) as usize;
  if face_at + 12 > bytes.len() {
    return None;
  }
  let tables =
    u16::from_be_bytes([bytes[face_at + 4], bytes[face_at + 5]]) as usize;
  let directory = face_at + 12;
  if directory + tables * 16 > bytes.len() {
    return None;
  }
  let mut copies = Vec::with_capacity(tables);
  for table in 0..tables {
    let record = directory + table * 16;
    let offset = u32::from_be_bytes([
      bytes[record + 8],
      bytes[record + 9],
      bytes[record + 10],
      bytes[record + 11],
    ]) as usize;
    let length = u32::from_be_bytes([
      bytes[record + 12],
      bytes[record + 13],
      bytes[record + 14],
      bytes[record + 15],
    ]) as usize;
    if offset.saturating_add(length) > bytes.len() {
      return None;
    }
    copies.push((record, offset, length));
  }
  let mut out = Vec::new();
  out.extend_from_slice(&bytes[face_at..face_at + 12]);
  let mut body = Vec::new();
  let mut written = Vec::with_capacity(tables);
  let mut cursor = (12 + tables * 16) as u32;
  for (record, offset, length) in copies {
    written.push((record, cursor, length));
    body.extend_from_slice(&bytes[offset..offset + length]);
    let pad = (4 - length % 4) % 4;
    body.extend(std::iter::repeat_n(0, pad));
    cursor += (length + pad) as u32;
  }
  for (record, offset, length) in written {
    out.extend_from_slice(&bytes[record..record + 8]);
    out.extend_from_slice(&offset.to_be_bytes());
    out.extend_from_slice(&(length as u32).to_be_bytes());
  }
  out.extend_from_slice(&body);
  Some(out)
}

struct FontData {
  postscript_name: String,
  full_name: String,
  family: String,
  style: String,
  info: FontInfo,
}

unsafe impl v8::cppgc::GarbageCollected for FontData {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"FontData"
  }
}

#[op]
impl FontData {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[getter]
  fn postscript_name(&self) -> String {
    self.postscript_name.clone()
  }

  #[getter]
  fn full_name(&self) -> String {
    self.full_name.clone()
  }

  #[getter]
  fn family(&self) -> String {
    self.family.clone()
  }

  #[getter]
  fn style(&self) -> String {
    self.style.clone()
  }

  #[promise]
  fn blob(&self, scope: &mut v8::PinScope) -> Js {
    let resolver = v8::PromiseResolver::new(scope).unwrap();
    let promise = resolver.get_promise(scope);
    match file_bytes(&self.info) {
      Some(bytes) => {
        let blob = yuzuko_winter::new_blob(scope, bytes, "");
        resolver.resolve(scope, blob.into());
      }
      None => {
        let error = new_dom_exception(
          scope,
          "NotReadableError",
          "The font data could not be read",
        )
        .map(v8::Local::<v8::Value>::from)
        .unwrap_or_else(|| {
          let message =
            v8::String::new(scope, "The font data could not be read").unwrap();
          v8::Exception::type_error(scope, message)
        });
        resolver.reject(scope, error);
      }
    }
    let value: v8::Local<v8::Value> = promise.into();
    Js(v8::Global::new(scope, value))
  }
}

pub(super) fn install(scope: &mut v8::PinScope) {
  FontData::install(scope);
  let global = scope.get_current_context().global(scope);
  let function = v8::Function::builder(query_local_fonts)
    .length(0)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap();
  let name = v8::String::new(scope, "queryLocalFonts").unwrap();
  function.set_name(name);
  global.set(scope, name.into(), function.into());
}

fn query_local_fonts(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let filter = match read_filter(scope, &args) {
    Ok(filter) => filter,
    Err(FilterError::Thrown) => return,
    Err(FilterError::Type(message)) => {
      let message = v8::String::new(scope, message).unwrap();
      let error = v8::Exception::type_error(scope, message);
      scope.throw_exception(error);
      return;
    }
  };
  let array = font_array(scope, filter.as_deref());
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  resolver.resolve(scope, array.into());
  ret.set(resolver.get_promise(scope).into());
}

enum FilterError {
  Type(&'static str),
  Thrown,
}

fn read_filter(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<Option<Vec<String>>, FilterError> {
  if args.length() == 0 {
    return Ok(None);
  }
  let value = args.get(0);
  if value.is_undefined() || value.is_null() {
    return Ok(None);
  }
  let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
    FilterError::Type("queryLocalFonts options must be an object")
  })?;
  let key = v8::String::new(scope, "postscriptNames").ok_or(
    FilterError::Type("queryLocalFonts options must be an object"),
  )?;
  let value = object.get(scope, key.into()).ok_or(FilterError::Thrown)?;
  if value.is_undefined() {
    return Ok(None);
  }
  if value.is_null() {
    return Err(FilterError::Type("postscriptNames must be a sequence"));
  }
  let list = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| FilterError::Type("postscriptNames must be a sequence"))?;
  let mut names = Vec::new();
  for index in 0..list.length() {
    let Some(item) = list.get_index(scope, index) else {
      return Err(FilterError::Thrown);
    };
    let Some(text) = item.to_string(scope) else {
      return Err(FilterError::Thrown);
    };
    names.push(text.to_rust_string_lossy(scope));
  }
  Ok(Some(names))
}

fn font_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  filter: Option<&[String]>,
) -> v8::Local<'s, v8::Array> {
  let mut selected = Vec::new();
  with_catalog(|faces| {
    for face in faces {
      if !face.enumerable {
        continue;
      }
      if let Some(names) = filter
        && !names.iter().any(|name| name == &face.postscript)
      {
        continue;
      }
      selected.push(FontData {
        postscript_name: face.postscript.clone(),
        full_name: face.full.clone(),
        family: face.family.clone(),
        style: face.style.clone(),
        info: face.info.clone(),
      });
    }
  });
  selected
    .sort_by(|left, right| left.postscript_name.cmp(&right.postscript_name));
  let array = v8::Array::new(scope, selected.len() as i32);
  for (index, data) in selected.into_iter().enumerate() {
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, data) };
    array.set_index(scope, index as u32, object.into());
  }
  array
}

#[cfg(test)]
mod tests {
  use super::extract_face;
  use super::valid_postscript;

  #[test]
  fn postscript_names_match_the_opentype_rules() {
    assert!(valid_postscript("Arial-BoldMT"));
    assert!(!valid_postscript(""));
    assert!(!valid_postscript("A[rial"));
    assert!(!valid_postscript(&"A".repeat(64)));
  }

  #[test]
  fn a_collection_face_becomes_its_own_sfnt() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"ttcf");
    bytes.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    let face0_at = 20u32;
    let face0 = tiny_face(b"AAAA", face0_at + 28);
    let face1_at = face0_at + face0.len() as u32;
    let face1 = tiny_face(b"BBBB", face1_at + 28);
    bytes.extend_from_slice(&face0_at.to_be_bytes());
    bytes.extend_from_slice(&face1_at.to_be_bytes());
    bytes.extend_from_slice(&face0);
    bytes.extend_from_slice(&face1);
    let extracted = extract_face(&bytes, 1).unwrap();
    assert_eq!(&extracted[..4], &0x0001_0000u32.to_be_bytes());
    assert!(extracted.windows(4).any(|window| window == b"BBBB"));
    assert!(!extracted.windows(4).any(|window| window == b"AAAA"));
    assert_eq!(extract_face(&face0, 0).unwrap(), face0);
  }

  fn tiny_face(payload: &[u8; 4], data_at: u32) -> Vec<u8> {
    let mut face = Vec::new();
    face.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    face.extend_from_slice(&1u16.to_be_bytes());
    face.extend_from_slice(&16u16.to_be_bytes());
    face.extend_from_slice(&0u16.to_be_bytes());
    face.extend_from_slice(&0u16.to_be_bytes());
    face.extend_from_slice(b"test");
    face.extend_from_slice(&0u32.to_be_bytes());
    face.extend_from_slice(&data_at.to_be_bytes());
    face.extend_from_slice(&4u32.to_be_bytes());
    face.extend_from_slice(payload);
    face
  }
}
