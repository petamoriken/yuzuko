//! Generic-family text layout through parley and fontique.
//!
//! A family in `Yuzuko.fonts` that has finished `load()` is registered here
//! and can be named from the Canvas 2D `font` property.

mod font;
pub(crate) mod fonts;

pub use font::font_css;
pub use font::parse_font;

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
use parley::FontFeatures;
use parley::GenericFamily;
use parley::LayoutContext;
use parley::StyleProperty;

use crate::paint::record::Family;
use crate::paint::record::FontDesc;
use crate::paint::record::FontKerning;
use crate::paint::record::FontStretch;
use crate::paint::record::FontVariantCaps;

struct Registered {
  family: FamilyId,
  width: FontWidth,
  style: FontStyle,
  weight: FontWeight,
}

struct TextEnv {
  fonts: FontContext,
  layout: LayoutContext,
  /// Interned family names. Indexes are `Family::Named` and `Family::Unloaded`.
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

/// Index of `token` in the family-name table, adding it when absent.
pub fn intern_family(token: &str) -> u32 {
  TEXT.with(|slot| {
    let mut env = slot.borrow_mut();
    if let Some(id) = env.names.iter().position(|item| item == token) {
      return id as u32;
    }
    env.names.push(token.to_string());
    (env.names.len() - 1) as u32
  })
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
  pub font: vello_cpu::peniko::FontData,
  pub size: f32,
  pub glyphs: Vec<(u32, f32, f32)>,
}

/// One positioned glyph in advance space. `y` is relative to the alphabetic
/// baseline and increases downward.
#[derive(Clone)]
pub struct ShapedGlyph {
  pub font: vello_cpu::peniko::FontData,
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
  pub glyphs: Vec<ShapedGlyph>,
  pub ink: Option<Ink>,
}

#[derive(Clone)]
pub struct ShapedText {
  /// Unspaced advance from the shaper, in CSS pixels.
  pub width: f32,
  /// Letter and word spacing added after shaping, in CSS pixels.
  ///
  /// Kept in `f64` so a later subtraction of two measured widths is exact.
  /// The shaper's `f32` advance would round a 20px gap off that equality.
  pub spacing: f64,
  pub ascent: f32,
  pub descent: f32,
  /// Em box above the alphabetic baseline. Ascent and descent sum to the
  /// font size.
  pub em_ascent: f32,
  pub em_descent: f32,
  /// Hanging baseline above the alphabetic baseline.
  pub hanging: f32,
  /// Ideographic baseline above the alphabetic baseline. Negative when the
  /// font has no BASE `ideo` coordinate and the descent is used instead.
  pub ideographic: f32,
  pub runs: Vec<ShapedRun>,
  pub clusters: Vec<ShapedCluster>,
}

/// Spacing and OpenType features for one `measureText` or text draw.
///
/// Letter and word spacing are not given to parley. Parley would add them
/// again as `f32`, and the measured width would no longer match the gaps
/// added here.
#[derive(Clone, Debug)]
pub struct TextShape {
  pub letter_px: f64,
  pub word_px: f64,
  pub stretch: FontWidth,
  /// CSS font-feature list. Empty when no feature is overridden.
  pub features: String,
  /// Lowercase letters use 0.7 of the font size. Set when `smcp` does not
  /// change the glyphs.
  pub synthetic_small_caps: bool,
}

impl Default for TextShape {
  fn default() -> Self {
    Self {
      letter_px: 0.0,
      word_px: 0.0,
      stretch: FontWidth::NORMAL,
      features: String::new(),
      synthetic_small_caps: false,
    }
  }
}

impl TextShape {
  pub fn from_canvas(
    font: &FontDesc,
    kerning: FontKerning,
    stretch: FontStretch,
    variant: FontVariantCaps,
    letter_spacing: &str,
    word_spacing: &str,
  ) -> Self {
    let mut features = String::new();
    if !kerning.kerns() {
      features.push_str("\"kern\" off");
    }
    let tags = if variant == FontVariantCaps::Normal && font.small_caps {
      &["smcp"][..]
    } else {
      variant.features()
    };
    for tag in tags {
      if !features.is_empty() {
        features.push_str(", ");
      }
      features.push('"');
      features.push_str(tag);
      features.push_str("\" on");
    }
    Self {
      letter_px: spacing_px(letter_spacing, font.size),
      word_px: spacing_px(word_spacing, font.size),
      stretch: FontWidth::parse_css(stretch.as_str())
        .unwrap_or(FontWidth::NORMAL),
      features,
      synthetic_small_caps: false,
    }
  }
}

impl ShapedText {
  /// Unspaced advance plus letter and word spacing.
  pub fn advance(&self) -> f64 {
    f64::from(self.width) + self.spacing
  }

  /// Union of glyph-outline bounds in advance space. Empty when no glyph
  /// has an outline, which includes a space.
  pub fn ink_union(&self) -> Option<Ink> {
    let mut ink = None;
    for cluster in &self.clusters {
      if let Some(part) = cluster.ink {
        union_ink(&mut ink, (part.x0, part.y0, part.x1, part.y1));
      }
    }
    ink
  }
}

fn shaping_language(lang: &str) -> Option<Language> {
  if lang.is_empty() || lang == "inherit" {
    return None;
  }
  Language::parse(lang).ok()
}

pub fn shape(
  text: &str,
  font: &FontDesc,
  rtl: bool,
  lang: &str,
  shaping: &TextShape,
) -> ShapedText {
  // Canvas converts those four controls to U+0020 and does not collapse
  // them. U+000B stays. U+0000 is dropped before shaping.
  let text = prepare_text(text);
  if text.is_empty() {
    return empty_shaped(font.size);
  }
  let shaped = layout(&text, font, rtl, lang, shaping);
  // `smcp` is a no-op when the face has no small-cap glyphs. Draw lowercase
  // at 0.7 of the font size instead of applying the feature a second time.
  if !shaping.features.contains("\"smcp\"") {
    return shaped;
  }
  let mut plain = shaping.clone();
  plain.features = without_caps(&shaping.features);
  let plain_shaped = layout(&text, font, rtl, lang, &plain);
  if glyph_ids(&shaped) != glyph_ids(&plain_shaped) {
    return shaped;
  }
  plain.synthetic_small_caps = true;
  layout(&text, font, rtl, lang, &plain)
}

fn layout(
  text: &str,
  font: &FontDesc,
  rtl: bool,
  lang: &str,
  shaping: &TextShape,
) -> ShapedText {
  let wrapped = if rtl {
    format!("\u{2067}{text}\u{2069}")
  } else {
    format!("\u{2066}{text}\u{2069}")
  };
  TEXT.with(|env| {
    let mut env = env.borrow_mut();
    let named = match font.family {
      Family::Named(id) => env.names.get(id as usize).cloned(),
      Family::Unloaded(_) => None,
      _ => None,
    };
    let TextEnv { fonts, layout, .. } = &mut *env;
    let mut builder = layout.ranged_builder(fonts, &wrapped, 1.0, true);
    builder.push_default(StyleProperty::FontSize(font.size));
    if shaping.synthetic_small_caps {
      push_lowercase_size(&mut builder, &wrapped, font.size * 0.7);
    }
    builder.push_default(StyleProperty::FontWeight(FontWeight::new(
      font.weight as f32,
    )));
    builder.push_default(StyleProperty::FontStyle(if font.italic {
      FontStyle::Italic
    } else {
      FontStyle::Normal
    }));
    builder.push_default(StyleProperty::FontWidth(shaping.stretch));
    if !shaping.features.is_empty() {
      builder.push_default(FontFeatures::from(shaping.features.as_str()));
    }
    if let Some(name) = named {
      builder.push_default(FontFamily::Source(Cow::Owned(name)));
    } else {
      let generic = match font.family {
        Family::Sans => GenericFamily::SansSerif,
        Family::Serif => GenericFamily::Serif,
        Family::Mono => GenericFamily::Monospace,
        Family::Named(_) | Family::Unloaded(_) => GenericFamily::SansSerif,
      };
      builder.push_default(generic);
    }
    // `"inherit"` has no document or `<canvas>` to read. An empty string is
    // the unknown language. A tag this stack cannot parse is unknown too.
    // A parsed tag keeps its language, script, and region.
    builder.push_default(StyleProperty::Locale(shaping_language(lang)));
    let mut built = builder.build(&wrapped);
    built.break_all_lines(None);
    built.align(Alignment::Start, AlignmentOptions::default());
    let placeholder_ascent = font.size * 0.8;
    let placeholder_descent = font.size * 0.2;
    let (em_ascent, em_descent) =
      normalized_em(placeholder_ascent, placeholder_descent, font.size);
    let mut shaped = ShapedText {
      width: built.width(),
      spacing: 0.0,
      ascent: placeholder_ascent,
      descent: placeholder_descent,
      em_ascent,
      em_descent,
      hanging: placeholder_ascent * 0.8,
      ideographic: -placeholder_descent,
      runs: Vec::new(),
      clusters: Vec::new(),
    };
    if let Some(line) = built.get(0) {
      let mut clusters = Vec::new();
      let mut typographic_ascent = None;
      let mut typographic_descent = None;
      let mut sample_font = None;
      for item in line.items() {
        let parley::PositionedLayoutItem::GlyphRun(run) = item else {
          continue;
        };
        let metrics = run.run().font_metrics();
        typographic_ascent = Some(
          typographic_ascent
            .unwrap_or(metrics.ascent)
            .max(metrics.ascent),
        );
        typographic_descent = Some(
          typographic_descent
            .unwrap_or(metrics.descent)
            .max(metrics.descent),
        );
        let face = run.run().font().clone();
        if sample_font.is_none() {
          sample_font = Some(face.clone());
        }
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
              glyphs,
              ink,
            });
          }
          cursor += advance;
        }
      }
      if let (Some(ascent), Some(descent)) =
        (typographic_ascent, typographic_descent)
      {
        // Quantize rounds the line's typographic ascent and descent.
        shaped.ascent = ascent.round();
        shaped.descent = descent.round();
      }
      let (em_ascent, em_descent) =
        normalized_em(shaped.ascent, shaped.descent, font.size);
      shaped.em_ascent = em_ascent;
      shaped.em_descent = em_descent;
      if let Some((hanging, ideographic)) = sample_font
        .as_ref()
        .and_then(|sample| base_offsets(sample, font.size))
      {
        shaped.hanging = hanging;
        shaped.ideographic = ideographic;
      } else {
        shaped.hanging = shaped.ascent * 0.8;
        shaped.ideographic = -shaped.descent;
      }
      shaped.clusters = merge_graphemes(text, clusters);
      shaped.spacing = apply_spacing(
        text,
        &mut shaped.clusters,
        shaping.letter_px,
        shaping.word_px,
      );
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

fn glyph_ids(shaped: &ShapedText) -> Vec<u32> {
  shaped
    .runs
    .iter()
    .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.0))
    .collect()
}

fn without_caps(features: &str) -> String {
  features
    .split(", ")
    .filter(|part| {
      !["smcp", "c2sc", "pcap", "c2pc", "unic", "titl"]
        .iter()
        .any(|tag| part.contains(tag))
    })
    .collect::<Vec<_>>()
    .join(", ")
}

fn push_lowercase_size(
  builder: &mut parley::RangedBuilder<'_, [u8; 4]>,
  wrapped: &str,
  size: f32,
) {
  let mut byte = 0;
  let mut run = None;
  for ch in wrapped.chars() {
    let next = byte + ch.len_utf8();
    if ch.is_lowercase() {
      if run.is_none() {
        run = Some(byte);
      }
    } else if let Some(start) = run.take() {
      builder.push(StyleProperty::FontSize(size), start..byte);
    }
    byte = next;
  }
  if let Some(start) = run {
    builder.push(StyleProperty::FontSize(size), start..byte);
  }
}

fn prepare_text(text: &str) -> String {
  let mut prepared = String::with_capacity(text.len());
  for ch in text.chars() {
    match ch {
      '\u{0000}' => {}
      '\u{0009}' | '\u{000A}' | '\u{000C}' | '\u{000D}' => prepared.push(' '),
      other => prepared.push(other),
    }
  }
  prepared
}

fn empty_shaped(size: f32) -> ShapedText {
  let ascent = size * 0.8;
  let descent = size * 0.2;
  let (em_ascent, em_descent) = normalized_em(ascent, descent, size);
  ShapedText {
    width: 0.0,
    spacing: 0.0,
    ascent,
    descent,
    em_ascent,
    em_descent,
    hanging: ascent * 0.8,
    ideographic: -descent,
    runs: Vec::new(),
    clusters: Vec::new(),
  }
}

/// Blink normalizes the typographic box so the two sides sum to the font size.
fn normalized_em(ascent: f32, descent: f32, size: f32) -> (f32, f32) {
  let sum = ascent + descent;
  if sum <= 0.0 {
    return (size, 0.0);
  }
  let em_ascent = (ascent * size / sum).round();
  (em_ascent, size.round() - em_ascent)
}

/// `hang` and `ideo` from the font's BASE table, in CSS pixels above the
/// alphabetic baseline. `None` when the default script does not place both.
fn base_offsets(
  font: &vello_cpu::peniko::FontData,
  size: f32,
) -> Option<(f32, f32)> {
  use skrifa::raw::TableProvider;
  use skrifa::raw::tables::base::BaseAxis;
  use skrifa::raw::tables::base::BaseInstance;
  use skrifa::raw::tables::base::baseline_tags;

  let face = skrifa::FontRef::from_index(font.data.data(), font.index).ok()?;
  let upem = f32::from(face.head().ok()?.units_per_em());
  if upem <= 0.0 {
    return None;
  }
  let instance = BaseInstance::new(face.base().ok()?);
  let scale = size / upem;
  let script = skrifa::Tag::new(b"DFLT");
  let hanging = instance
    .baseline(baseline_tags::HANGING, BaseAxis::Horizontal, script)?
    .to_f32()
    * scale;
  let ideographic = instance
    .baseline(
      baseline_tags::IDEO_EMBOX_BOTTOM_OR_LEFT,
      BaseAxis::Horizontal,
      script,
    )?
    .to_f32()
    * scale;
  Some((hanging, ideographic))
}

/// Shift clusters by the spacing of every scalar that ends at or before the
/// cluster. The returned total includes the gap after the last scalar.
fn apply_spacing(
  text: &str,
  clusters: &mut [ShapedCluster],
  letter_px: f64,
  word_px: f64,
) -> f64 {
  if letter_px == 0.0 && word_px == 0.0 {
    return 0.0;
  }
  let mut ends = Vec::new();
  let mut acc = 0.0f64;
  let mut index = 0u32;
  for ch in text.chars() {
    index += ch.len_utf16() as u32;
    acc += letter_px;
    if ch == ' ' {
      acc += word_px;
    }
    ends.push((index, acc));
  }
  for cluster in clusters.iter_mut() {
    let mut shift = 0.0f64;
    for (end, extra) in &ends {
      if *end <= cluster.start {
        shift = *extra;
      } else {
        break;
      }
    }
    let shift = shift as f32;
    cluster.left += shift;
    if let Some(ink) = cluster.ink.as_mut() {
      ink.x0 += shift;
      ink.x1 += shift;
    }
    for glyph in &mut cluster.glyphs {
      glyph.x += shift;
    }
  }
  acc
}

/// A canvas spacing length. The number keeps its spelling and the unit is
/// lowercased. Units are `px`, `em`, `ex`, `ch`, `ic`, `in`, `cm`, and `mm`.
pub fn parse_spacing(input: &str) -> Option<String> {
  let input = input.trim();
  let bytes = input.as_bytes();
  if bytes.is_empty() {
    return None;
  }
  let mut index = 0;
  if bytes[0] == b'+' || bytes[0] == b'-' {
    index = 1;
  }
  let digits = index;
  if index >= bytes.len() || !bytes[index].is_ascii_digit() {
    return None;
  }
  while index < bytes.len() && bytes[index].is_ascii_digit() {
    index += 1;
  }
  if index < bytes.len() && bytes[index] == b'.' {
    index += 1;
    let fraction = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
      index += 1;
    }
    if index == fraction {
      return None;
    }
  }
  if index == digits {
    return None;
  }
  let unit_at = index;
  while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
    index += 1;
  }
  if index != bytes.len() || unit_at == bytes.len() {
    return None;
  }
  let unit = input[unit_at..].to_ascii_lowercase();
  match unit.as_str() {
    "px" | "em" | "ex" | "ch" | "ic" | "in" | "cm" | "mm" => {}
    _ => return None,
  }
  let mut stored = input[..unit_at].to_string();
  stored.push_str(&unit);
  Some(stored)
}

pub fn spacing_px(css: &str, font_size: f32) -> f64 {
  let bytes = css.as_bytes();
  let mut index = 0;
  while index < bytes.len() && !bytes[index].is_ascii_alphabetic() {
    index += 1;
  }
  if index == 0 || index == bytes.len() {
    return 0.0;
  }
  let Ok(number) = css[..index].parse::<f64>() else {
    return 0.0;
  };
  let font = f64::from(font_size);
  match &css[index..] {
    "px" => number,
    "in" => number * 96.0,
    "cm" => number * 96.0 / 2.54,
    "mm" => number * 96.0 / 25.4,
    "em" | "ic" => number * font,
    "ex" | "ch" => number * font * 0.5,
    _ => 0.0,
  }
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
  font: &vello_cpu::peniko::FontData,
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

#[cfg(test)]
mod tests {
  use super::TextShape;
  use super::parse_spacing;
  use super::shape;
  use crate::paint::record::FontDesc;

  fn glyph_ids(lang: &str) -> Vec<u32> {
    shape(
      "骨",
      &FontDesc::default(),
      false,
      lang,
      &TextShape::default(),
    )
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
    let shaped = shape(
      "Ab",
      &FontDesc::default(),
      false,
      "inherit",
      &TextShape::default(),
    );
    assert_eq!(shaped.clusters.len(), 2);
    assert!(shaped.clusters[1].left > shaped.clusters[0].left);
    assert!(shaped.clusters[0].glyphs.iter().any(|glyph| {
      shaped
        .runs
        .iter()
        .any(|run| run.glyphs.iter().any(|positioned| positioned.0 == glyph.id))
    }));
    let accent = shape(
      "e\u{0301}",
      &FontDesc::default(),
      false,
      "inherit",
      &TextShape::default(),
    );
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

  #[test]
  fn ink_union_covers_drawn_glyphs() {
    let font = FontDesc {
      size: 80.0,
      ..FontDesc::default()
    };
    let letter = shape("A", &font, false, "inherit", &TextShape::default());
    let space = shape(" ", &font, false, "inherit", &TextShape::default());
    let ink = letter.ink_union().expect("letter ink");
    assert!(ink.x1 > ink.x0);
    assert!(ink.y1 > ink.y0);
    assert!(space.ink_union().is_none());
    let descender = shape("g", &font, false, "inherit", &TextShape::default())
      .ink_union()
      .expect("descender ink");
    assert!(descender.y1 > ink.y1);
  }

  #[test]
  fn small_caps_changes_width() {
    use crate::paint::record::FontKerning;
    use crate::paint::record::FontStretch;
    use crate::paint::record::FontVariantCaps;
    use crate::text::parse_font;
    let small = parse_font("small-caps 32px serif").unwrap();
    let normal = parse_font("32px serif").unwrap();
    let shaping = TextShape::from_canvas(
      &small,
      FontKerning::Auto,
      FontStretch::Normal,
      FontVariantCaps::Normal,
      "0px",
      "0px",
    );
    let small_caps = shape("Hello World", &small, false, "inherit", &shaping);
    let plain = shape(
      "Hello World",
      &normal,
      false,
      "inherit",
      &TextShape::default(),
    );
    assert_ne!(small_caps.advance(), plain.advance());
  }

  #[test]
  fn spacing_keeps_spelling_and_adds_exact_gaps() {
    assert_eq!(parse_spacing("1PX"), Some("1px".to_string()));
    assert_eq!(parse_spacing("-0.1em"), Some("-0.1em".to_string()));
    assert_eq!(parse_spacing("1EX"), Some("1ex".to_string()));
    assert_eq!(parse_spacing("normal"), None);
    assert_eq!(parse_spacing("NaN"), None);
    let font = FontDesc::default();
    let plain = shape(
      "Hello World",
      &font,
      false,
      "inherit",
      &TextShape::default(),
    );
    let letters = shape(
      "Hello World",
      &font,
      false,
      "inherit",
      &TextShape {
        letter_px: 3.0,
        ..TextShape::default()
      },
    );
    assert!((letters.advance() - plain.advance() - 33.0).abs() < 1e-9);
    let words = shape(
      "Hello World, again",
      &font,
      false,
      "inherit",
      &TextShape {
        word_px: 5.0,
        ..TextShape::default()
      },
    );
    let plain_words = shape(
      "Hello World, again",
      &font,
      false,
      "inherit",
      &TextShape::default(),
    );
    assert!((words.advance() - plain_words.advance() - 10.0).abs() < 1e-9);
    let nul = shape("\u{0000}", &font, false, "inherit", &TextShape::default());
    assert_eq!(nul.advance(), 0.0);
  }
}
