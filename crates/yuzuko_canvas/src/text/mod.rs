//! Generic-family text layout through parley and fontique.
//!
//! A family in `Yuzuko.fonts` that has finished `load()` is registered here
//! and can be named from the Canvas 2D `font` property.

pub(crate) mod fonts;

use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::Arc;

use skrifa::MetadataProvider;
use unicode_segmentation::UnicodeSegmentation;

use fontique::Blob;
use fontique::FamilyId;
use fontique::FontInfoOverride;
use fontique::FontStyle;
use fontique::FontWeight;
use fontique::FontWidth;
use fontique::Language;
use parley::Alignment;
use parley::AlignmentOptions;
use parley::FontContext;
use parley::FontFamily;
use parley::GenericFamily;
use parley::LayoutContext;
use parley::StyleProperty;

use crate::paint::record::Align;
use crate::paint::record::Baseline;
use crate::paint::record::Family;
use crate::paint::record::FontDesc;

struct Registered {
  family: FamilyId,
  width: FontWidth,
  style: FontStyle,
  weight: FontWeight,
}

struct TextEnv {
  fonts: FontContext,
  layout: LayoutContext,
  /// Interned family names. Indexes are `Family::Named`.
  names: Vec<String>,
  /// Families currently in `Yuzuko.fonts` with a finished `load()`.
  ready: Vec<String>,
  registered: Vec<Registered>,
}

std::thread_local! {
  static TEXT: RefCell<TextEnv> = RefCell::new(TextEnv {
    fonts: FontContext::new(),
    layout: LayoutContext::new(),
    names: Vec::new(),
    ready: Vec::new(),
    registered: Vec::new(),
  });
}

/// Replace the loaded faces the Canvas 2D `font` property can name.
pub fn set_loaded_faces(faces: &[(String, Vec<u8>)]) {
  TEXT.with(|slot| {
    let mut env = slot.borrow_mut();
    let registered = std::mem::take(&mut env.registered);
    for registered in registered {
      env.fonts.collection.unregister_font(
        registered.family,
        registered.width,
        registered.style,
        registered.weight,
      );
    }
    env.ready.clear();
    for (family, bytes) in faces {
      if family.is_empty() {
        continue;
      }
      if !env.ready.iter().any(|name| name == family) {
        env.ready.push(family.clone());
      }
      register_one(&mut env, family, bytes);
    }
  });
}

/// Whether `bytes` are a font fontique can register under `family`.
pub fn font_is_usable(family: &str, bytes: &[u8]) -> bool {
  // An empty CSS family is still a loadable face. fontique needs a name.
  let family = if family.is_empty() {
    "Yuzuko Empty"
  } else {
    family
  };
  TEXT.with(|slot| {
    let mut env = slot.borrow_mut();
    let before = env.registered.len();
    register_one(&mut env, family, bytes);
    let added = env.registered.len() > before;
    while env.registered.len() > before {
      let registered = env.registered.pop().unwrap();
      env.fonts.collection.unregister_font(
        registered.family,
        registered.width,
        registered.style,
        registered.weight,
      );
    }
    added
  })
}

fn register_one(env: &mut TextEnv, family: &str, bytes: &[u8]) {
  let data: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(bytes.to_vec());
  let blob = Blob::new(data);
  let info = FontInfoOverride {
    family_name: Some(family),
    ..FontInfoOverride::default()
  };
  let added = env.fonts.collection.register_fonts(blob, Some(info));
  for (family_id, infos) in added {
    for info in infos {
      env.registered.push(Registered {
        family: family_id,
        width: info.width(),
        style: info.style(),
        weight: info.weight(),
      });
    }
  }
}

/// `Some` when `token` names a loaded family in `Yuzuko.fonts`.
pub fn ready_family(token: &str) -> Option<u32> {
  TEXT.with(|slot| {
    let mut env = slot.borrow_mut();
    let index = env
      .ready
      .iter()
      .position(|name| name.eq_ignore_ascii_case(token))?;
    let name = env.ready[index].clone();
    if let Some(id) = env.names.iter().position(|item| item == &name) {
      return Some(id as u32);
    }
    env.names.push(name);
    Some((env.names.len() - 1) as u32)
  })
}

#[derive(Clone)]
pub struct ShapedRun {
  pub font: vello::peniko::FontData,
  pub size: f32,
  pub glyphs: Vec<(u32, f32, f32)>,
}

/// One positioned glyph in advance space. `y` is relative to the alphabetic
/// baseline and increases downward.
#[derive(Clone)]
pub struct ShapedGlyph {
  pub font: vello::peniko::FontData,
  pub size: f32,
  pub id: u32,
  pub x: f32,
  pub y: f32,
}

/// Outline bounds in advance space. `y` increases downward from the
/// alphabetic baseline.
#[derive(Clone, Copy)]
pub struct Ink {
  pub x0: f32,
  pub y0: f32,
  pub x1: f32,
  pub y1: f32,
}

/// One extended grapheme cluster, in visual order.
#[derive(Clone)]
pub struct ShapedCluster {
  pub start: u32,
  pub end: u32,
  /// Visual left edge in unscaled advance space.
  pub left: f32,
  pub advance: f32,
  pub rtl: bool,
  pub glyphs: Vec<ShapedGlyph>,
  pub ink: Option<Ink>,
}

#[derive(Clone)]
pub struct ShapedText {
  pub width: f32,
  pub ascent: f32,
  pub descent: f32,
  pub runs: Vec<ShapedRun>,
  pub clusters: Vec<ShapedCluster>,
  pub utf16_len: u32,
}

/// `Ok(None)` leaves the previous font. `Err` is an unregistered family.
pub fn parse_font(input: &str) -> Result<Option<FontDesc>, ()> {
  let text = input.trim();
  if text.is_empty() {
    return Ok(None);
  }
  let mut size = None;
  let mut weight = 400u16;
  let mut italic = false;
  let mut family = None;
  let mut named = false;
  for token in text.split_whitespace() {
    let token = token.trim_matches(',');
    if token.is_empty() {
      continue;
    }
    let lower = token.to_ascii_lowercase();
    if let Some(parsed) = parse_size(&lower) {
      size = Some(parsed);
      continue;
    }
    match lower.as_str() {
      "normal" => {}
      "italic" | "oblique" => italic = true,
      "bold" => weight = 700,
      "bolder" | "lighter" => {}
      "small-caps" | "ultra-condensed" | "extra-condensed" | "condensed"
      | "semi-condensed" | "semi-expanded" | "expanded" | "extra-expanded"
      | "ultra-expanded" => {}
      "sans-serif" if family.is_none() => family = Some(Family::Sans),
      "serif" if family.is_none() => family = Some(Family::Serif),
      "monospace" if family.is_none() => family = Some(Family::Mono),
      other
        if other.parse::<u16>().ok().is_some_and(|value| {
          (100..=900).contains(&value) && value.is_multiple_of(100)
        }) =>
      {
        weight = other.parse().unwrap_or(400);
      }
      _ if lower.ends_with("px") || lower.ends_with("pt") => return Ok(None),
      _ => {
        if family.is_none() {
          let bare = token.trim_matches('"').trim_matches('\'');
          if let Some(id) = ready_family(bare) {
            family = Some(Family::Named(id));
          } else {
            named = true;
          }
        } else {
          named = true;
        }
      }
    }
  }
  let Some(size) = size else {
    return Ok(None);
  };
  if !(size.is_finite() && size > 0.0) {
    return Ok(None);
  }
  // A list uses the first loaded family or generic. A size with only an
  // unregistered name throws.
  let Some(family) = family else {
    if named {
      return Err(());
    }
    return Ok(None);
  };
  Ok(Some(FontDesc {
    size,
    weight,
    italic,
    family,
  }))
}

fn shaping_language(lang: &str) -> Option<Language> {
  if lang.is_empty() || lang == "inherit" {
    return None;
  }
  Language::parse(lang).ok()
}

fn parse_size(token: &str) -> Option<f32> {
  if let Some(number) = token.strip_suffix("px") {
    return number.parse::<f32>().ok().filter(|size| size.is_finite());
  }
  if let Some(number) = token.strip_suffix("pt") {
    return number
      .parse::<f32>()
      .ok()
      .filter(|size| size.is_finite())
      .map(|size| size * 4.0 / 3.0);
  }
  None
}

pub fn shape(text: &str, font: FontDesc, rtl: bool, lang: &str) -> ShapedText {
  let wrapped = if rtl {
    format!("\u{2067}{text}\u{2069}")
  } else {
    format!("\u{2066}{text}\u{2069}")
  };
  TEXT.with(|env| {
    let mut env = env.borrow_mut();
    let named = match font.family {
      Family::Named(id) => env.names.get(id as usize).cloned(),
      _ => None,
    };
    let TextEnv { fonts, layout, .. } = &mut *env;
    let mut builder = layout.ranged_builder(fonts, &wrapped, 1.0, true);
    builder.push_default(StyleProperty::FontSize(font.size));
    builder.push_default(StyleProperty::FontWeight(FontWeight::new(
      font.weight as f32,
    )));
    builder.push_default(StyleProperty::FontStyle(if font.italic {
      FontStyle::Italic
    } else {
      FontStyle::Normal
    }));
    if let Some(name) = named {
      builder.push_default(FontFamily::Source(Cow::Owned(name)));
    } else {
      let generic = match font.family {
        Family::Sans => GenericFamily::SansSerif,
        Family::Serif => GenericFamily::Serif,
        Family::Mono => GenericFamily::Monospace,
        Family::Named(_) => GenericFamily::SansSerif,
      };
      builder.push_default(generic);
    }
    // `"inherit"` has no document or `<canvas>` to read. An empty string is
    // the unknown language. A tag this stack cannot parse is unknown too.
    builder.push_default(StyleProperty::Locale(shaping_language(lang)));
    let mut built = builder.build(&wrapped);
    built.break_all_lines(None);
    built.align(Alignment::Start, AlignmentOptions::default());
    let mut shaped = ShapedText {
      width: built.width(),
      ascent: font.size * 0.8,
      descent: font.size * 0.2,
      runs: Vec::new(),
      clusters: Vec::new(),
      utf16_len: text.encode_utf16().count() as u32,
    };
    if let Some(line) = built.get(0) {
      let metrics = line.metrics();
      shaped.ascent = metrics.ascent;
      shaped.descent = metrics.descent;
      let mut clusters = Vec::new();
      for item in line.items() {
        let parley::PositionedLayoutItem::GlyphRun(run) = item else {
          continue;
        };
        let face = run.run().font().clone();
        let size = run.run().font_size();
        let mut cursor = run.offset();
        for cluster in run.run().visual_clusters() {
          let advance = cluster.advance();
          let range = cluster.text_range();
          let mut pen = cursor;
          let mut glyphs = Vec::new();
          let mut ink = None;
          for glyph in cluster.glyphs() {
            let x = pen + glyph.x;
            let y = glyph.y;
            if let Some(bounds) = outline_ink(&face, size, glyph.id, x, y) {
              union_ink(&mut ink, bounds);
            }
            glyphs.push(ShapedGlyph {
              font: face.clone(),
              size,
              id: glyph.id,
              x,
              y,
            });
            pen += glyph.advance;
          }
          if let Some((start, end)) = original_span(text, range) {
            clusters.push(ShapedCluster {
              start,
              end,
              left: cursor,
              advance,
              rtl: cluster.is_rtl(),
              glyphs,
              ink,
            });
          }
          cursor += advance;
        }
      }
      shaped.clusters = merge_graphemes(text, clusters);
      for cluster in &shaped.clusters {
        for glyph in &cluster.glyphs {
          let same = shaped.runs.last().is_some_and(|run| {
            run.size == glyph.size
              && run.font.index == glyph.font.index
              && std::ptr::eq(
                run.font.data.data().as_ptr(),
                glyph.font.data.data().as_ptr(),
              )
          });
          if same {
            if let Some(run) = shaped.runs.last_mut() {
              run.glyphs.push((glyph.id, glyph.x, glyph.y));
            }
          } else {
            shaped.runs.push(ShapedRun {
              font: glyph.font.clone(),
              size: glyph.size,
              glyphs: vec![(glyph.id, glyph.x, glyph.y)],
            });
          }
        }
      }
    }
    shaped
  })
}

/// UTF-16 span of `range` in `original`. The shaped string is wrapped in one
/// bidi isolate on each side.
fn original_span(
  original: &str,
  range: std::ops::Range<usize>,
) -> Option<(u32, u32)> {
  let prefix = "\u{2066}".len();
  let end_limit = prefix + original.len();
  if range.end <= prefix || range.start >= end_limit {
    return None;
  }
  let start = range.start.max(prefix) - prefix;
  let end = range.end.min(end_limit) - prefix;
  let start = floor_boundary(original, start);
  let end = floor_boundary(original, end);
  if start >= end {
    return None;
  }
  Some((utf16_len(&original[..start]), utf16_len(&original[..end])))
}

fn floor_boundary(text: &str, byte: usize) -> usize {
  let byte = byte.min(text.len());
  if text.is_char_boundary(byte) {
    byte
  } else {
    (0..byte)
      .rev()
      .find(|index| text.is_char_boundary(*index))
      .unwrap_or(0)
  }
}

fn utf16_len(text: &str) -> u32 {
  text.encode_utf16().count() as u32
}

fn merge_graphemes(
  text: &str,
  clusters: Vec<ShapedCluster>,
) -> Vec<ShapedCluster> {
  if clusters.is_empty() {
    return clusters;
  }
  let mut graphemes = Vec::new();
  let mut utf16 = 0u32;
  for grapheme in text.graphemes(true) {
    let len = utf16_len(grapheme);
    graphemes.push((utf16, utf16 + len));
    utf16 += len;
  }
  let mut merged = Vec::new();
  for (start, end) in graphemes {
    let parts: Vec<&ShapedCluster> = clusters
      .iter()
      .filter(|cluster| cluster.start >= start && cluster.start < end)
      .collect();
    let Some(first) = parts.first() else {
      continue;
    };
    let mut cluster = (*first).clone();
    cluster.start = start;
    cluster.end =
      end.max(parts.iter().map(|part| part.end).max().unwrap_or(end));
    cluster.left = parts
      .iter()
      .map(|part| part.left)
      .fold(f32::INFINITY, f32::min);
    let right = parts
      .iter()
      .map(|part| part.left + part.advance)
      .fold(cluster.left, f32::max);
    cluster.advance = (right - cluster.left).max(0.0);
    cluster.glyphs = parts
      .iter()
      .flat_map(|part| part.glyphs.iter().cloned())
      .collect();
    cluster.ink = None;
    for part in &parts {
      if let Some(ink) = part.ink {
        union_ink(&mut cluster.ink, (ink.x0, ink.y0, ink.x1, ink.y1));
      }
    }
    merged.push(cluster);
  }
  merged.sort_by(|left, right| {
    left
      .left
      .partial_cmp(&right.left)
      .unwrap_or(std::cmp::Ordering::Equal)
  });
  merged
}

fn outline_ink(
  font: &vello::peniko::FontData,
  size: f32,
  id: u32,
  x: f32,
  y: f32,
) -> Option<(f32, f32, f32, f32)> {
  let face = skrifa::FontRef::from_index(font.data.data(), font.index).ok()?;
  let metrics = face.glyph_metrics(
    skrifa::instance::Size::new(size),
    skrifa::instance::LocationRef::default(),
  );
  let bounds = metrics.bounds(skrifa::GlyphId::new(id))?;
  let x0 = x + bounds.x_min;
  let x1 = x + bounds.x_max;
  // Font bounds are y-up. Layout `y` is y-down from the baseline.
  let y0 = y - bounds.y_max;
  let y1 = y - bounds.y_min;
  if x1 > x0 && y1 > y0 {
    Some((x0, y0, x1, y1))
  } else {
    None
  }
}

fn union_ink(ink: &mut Option<Ink>, bounds: (f32, f32, f32, f32)) {
  let (x0, y0, x1, y1) = bounds;
  match ink {
    Some(ink) => {
      ink.x0 = ink.x0.min(x0);
      ink.y0 = ink.y0.min(y0);
      ink.x1 = ink.x1.max(x1);
      ink.y1 = ink.y1.max(y1);
    }
    None => *ink = Some(Ink { x0, y0, x1, y1 }),
  }
}

/// Anchor of one cluster in unscaled advance space.
pub fn cluster_anchor_x(
  align: Align,
  rtl: bool,
  left: f64,
  advance: f64,
) -> f64 {
  match align {
    Align::Left => left,
    Align::Right => left + advance,
    Align::Center => left + advance / 2.0,
    Align::Start => {
      if rtl {
        left + advance
      } else {
        left
      }
    }
    Align::End => {
      if rtl {
        left
      } else {
        left + advance
      }
    }
  }
}

pub struct PlacedRect {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

impl ShapedText {
  pub fn selection_rects(
    &self,
    start: u32,
    end: u32,
    align: Align,
    baseline: Baseline,
    rtl: bool,
  ) -> Vec<PlacedRect> {
    if start >= end {
      return Vec::new();
    }
    let shift = align.offset(rtl, f64::from(self.width));
    let base = baseline.offset(f64::from(self.ascent), f64::from(self.descent));
    let height = f64::from(self.ascent + self.descent);
    let y = -f64::from(self.ascent) + base;
    let mut rects = Vec::new();
    let mut group: Option<(f32, f32)> = None;
    for cluster in &self.clusters {
      if cluster.end <= start || cluster.start >= end {
        if let Some((left, right)) = group.take() {
          rects.push(span_rect(left, right, shift, y, height));
        }
        continue;
      }
      let right = cluster.left + cluster.advance;
      group = Some(match group {
        Some((left, previous)) => (left.min(cluster.left), previous.max(right)),
        None => (cluster.left, right),
      });
    }
    if let Some((left, right)) = group {
      rects.push(span_rect(left, right, shift, y, height));
    }
    rects
  }

  pub fn actual_box(
    &self,
    start: u32,
    end: u32,
    align: Align,
    baseline: Baseline,
    rtl: bool,
  ) -> PlacedRect {
    let shift = align.offset(rtl, f64::from(self.width));
    let base = baseline.offset(f64::from(self.ascent), f64::from(self.descent));
    let mut ink = None;
    for cluster in &self.clusters {
      if cluster.end <= start || cluster.start >= end {
        continue;
      }
      if let Some(bounds) = cluster.ink {
        union_ink(&mut ink, (bounds.x0, bounds.y0, bounds.x1, bounds.y1));
      }
    }
    if let Some(ink) = ink {
      return PlacedRect {
        x: f64::from(ink.x0) + shift,
        y: f64::from(ink.y0) + base,
        width: f64::from(ink.x1 - ink.x0),
        height: f64::from(ink.y1 - ink.y0),
      };
    }
    let left = self
      .clusters
      .iter()
      .find(|cluster| cluster.end > start && cluster.start < end)
      .map(|cluster| cluster.left)
      .unwrap_or(0.0);
    PlacedRect {
      x: f64::from(left) + shift,
      y: base,
      width: 0.0,
      height: 0.0,
    }
  }

  pub fn index_at(&self, offset: f64, align: Align, rtl: bool) -> u32 {
    let shift = align.offset(rtl, f64::from(self.width));
    let x = (offset - shift) as f32;
    let left = self
      .clusters
      .iter()
      .map(|cluster| cluster.left)
      .fold(0.0_f32, f32::min);
    let right = self.width;
    if self.clusters.is_empty() || x < left {
      return if rtl { self.utf16_len } else { 0 };
    }
    if x > right {
      return if rtl { 0 } else { self.utf16_len };
    }
    for cluster in &self.clusters {
      let edge = cluster.left + cluster.advance;
      if x > edge && (edge - right).abs() > f32::EPSILON {
        continue;
      }
      let on_left = x <= cluster.left + cluster.advance * 0.5;
      return if cluster.rtl == on_left {
        cluster.end
      } else {
        cluster.start
      };
    }
    if rtl { 0 } else { self.utf16_len }
  }
}

impl ShapedCluster {
  /// Glyphs relative to this cluster's alignment point.
  pub fn placed(
    &self,
    align: Align,
    baseline: Baseline,
    ascent: f32,
    descent: f32,
  ) -> Vec<ShapedGlyph> {
    let rel =
      cluster_anchor_x(align, self.rtl, 0.0, f64::from(self.advance)) as f32;
    let base = baseline.offset(f64::from(ascent), f64::from(descent)) as f32;
    self
      .glyphs
      .iter()
      .map(|glyph| ShapedGlyph {
        font: glyph.font.clone(),
        size: glyph.size,
        id: glyph.id,
        x: glyph.x - self.left - rel,
        y: glyph.y + base,
      })
      .collect()
  }
}

fn span_rect(
  left: f32,
  right: f32,
  shift: f64,
  y: f64,
  height: f64,
) -> PlacedRect {
  let x0 = f64::from(left.min(right)) + shift;
  let x1 = f64::from(left.max(right)) + shift;
  PlacedRect {
    x: x0,
    y,
    width: (x1 - x0).max(0.0),
    height: height.max(0.0),
  }
}

pub fn font_css(font: FontDesc) -> String {
  let style = if font.italic { "italic " } else { "" };
  let weight = if font.weight == 400 {
    String::new()
  } else {
    format!("{} ", font.weight)
  };
  let family = match font.family {
    Family::Sans => "sans-serif".to_string(),
    Family::Serif => "serif".to_string(),
    Family::Mono => "monospace".to_string(),
    Family::Named(id) => TEXT.with(|slot| {
      slot
        .borrow()
        .names
        .get(id as usize)
        .cloned()
        .unwrap_or_else(|| "sans-serif".to_string())
    }),
  };
  format!("{style}{weight}{}px {family}", font.size)
}

#[cfg(test)]
mod tests {
  use super::shape;
  use crate::paint::record::FontDesc;

  fn glyph_ids(lang: &str) -> Vec<u32> {
    shape("骨", FontDesc::default(), false, lang)
      .runs
      .into_iter()
      .flat_map(|run| run.glyphs.into_iter().map(|glyph| glyph.0))
      .collect()
  }

  #[test]
  fn unknown_language_matches_and_tags_shape() {
    let unknown = glyph_ids("inherit");
    assert_eq!(unknown, glyph_ids(""));
    assert_eq!(unknown, glyph_ids("not a tag"));
    assert!(!glyph_ids("ja").is_empty());
    assert!(!glyph_ids("zh-Hans").is_empty());
  }

  #[test]
  fn clusters_follow_graphemes_and_advances() {
    let shaped = shape("Ab", FontDesc::default(), false, "inherit");
    assert_eq!(shaped.clusters.len(), 2);
    assert!(shaped.clusters[1].left > shaped.clusters[0].left);
    assert!(shaped.clusters[0].glyphs.iter().any(|glyph| {
      shaped
        .runs
        .iter()
        .any(|run| run.glyphs.iter().any(|positioned| positioned.0 == glyph.id))
    }));
    let accent = shape("e\u{0301}", FontDesc::default(), false, "inherit");
    assert_eq!(accent.clusters.len(), 1);
    assert_eq!(accent.clusters[0].end, 2);
    let runs_x: Vec<f32> = shaped
      .runs
      .iter()
      .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.1))
      .collect();
    assert_eq!(runs_x.len(), 2);
    assert!(runs_x[1] > runs_x[0]);
  }
}
