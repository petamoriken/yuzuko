//! Recorded Canvas 2D commands. One list is replayed on either renderer.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use kurbo::Affine;
use kurbo::BezPath;
use kurbo::Cap;
use kurbo::Join;
use kurbo::Rect;
use kurbo::Stroke;
use vello_cpu::peniko::Fill;
use yuzuko_image::ColorSpace;
use yuzuko_image::SampleFormat;

use crate::paint::color::CssColor;
use crate::paint::shade::Shade;

/// Output headroom when the bitmap is presented. Painting does not read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToneMapping {
  /// Headroom 0. The default.
  Standard,
  /// Headroom is not limited. The swapchain stays unorm8.
  Extended,
}

#[derive(Clone, Copy, Debug)]
pub struct Attrs {
  pub alpha: bool,
  pub color_space: ColorSpace,
  pub format: SampleFormat,
  pub desynchronized: bool,
  pub will_read_frequently: bool,
  pub tone_mapping: ToneMapping,
}

impl Attrs {
  pub fn vello_target(self) -> bool {
    self.color_space == ColorSpace::Srgb && self.format == SampleFormat::Unorm8
  }
}

impl Default for Attrs {
  fn default() -> Self {
    Self {
      alpha: true,
      color_space: ColorSpace::Srgb,
      format: SampleFormat::Unorm8,
      desynchronized: false,
      will_read_frequently: false,
      tone_mapping: ToneMapping::Standard,
    }
  }
}

#[derive(Clone, Debug)]
pub struct ImageBuf {
  pub width: u32,
  pub height: u32,
  pub display_p3: bool,
  pub float16: bool,
  pub premultiplied: bool,
  pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum Brush {
  Solid(CssColor),
  Linear {
    start: (f32, f32),
    end: (f32, f32),
    stops: Arc<[(f32, CssColor)]>,
  },
  Radial {
    start: (f32, f32, f32),
    end: (f32, f32, f32),
    stops: Arc<[(f32, CssColor)]>,
  },
  /// One full turn, clockwise in canvas space, starting at `start` radians
  /// from the positive x-axis.
  Conic {
    start: f32,
    center: (f32, f32),
    stops: Arc<[(f32, CssColor)]>,
  },
  Pattern {
    image: Arc<ImageBuf>,
    repeat_x: bool,
    repeat_y: bool,
    quality: SampleQuality,
  },
}

#[derive(Clone, Debug)]
pub struct Clip {
  pub path: BezPath,
  pub fill: Fill,
  pub transform: Affine,
}

#[derive(Clone, Debug)]
pub enum Command {
  Fill {
    path: BezPath,
    fill: Fill,
    brush: Brush,
    transform: Affine,
    /// Maps the brush when `transform` does not. The current path is already
    /// in device space, so its fill uses an identity geometry transform and
    /// puts the current matrix here.
    brush_transform: Affine,
    alpha: f32,
    shade: Shade,
    filter: Arc<[crate::paint::filter::Resolved]>,
    clips: Arc<[Clip]>,
  },
  Stroke {
    path: BezPath,
    stroke: Stroke,
    brush: Brush,
    transform: Affine,
    brush_transform: Affine,
    alpha: f32,
    shade: Shade,
    filter: Arc<[crate::paint::filter::Resolved]>,
    clips: Arc<[Clip]>,
  },
  Glyphs {
    font: vello_cpu::peniko::FontData,
    size: f32,
    glyphs: Arc<[(u32, f32, f32)]>,
    brush: Brush,
    transform: Affine,
    alpha: f32,
    stroke: Option<Stroke>,
    shade: Shade,
    filter: Arc<[crate::paint::filter::Resolved]>,
    clips: Arc<[Clip]>,
  },
  Clear {
    rect: Rect,
    transform: Affine,
    clips: Arc<[Clip]>,
  },
  Put {
    image: Arc<ImageBuf>,
    dx: i32,
    dy: i32,
  },
  Image {
    image: Arc<ImageBuf>,
    transform: Affine,
    alpha: f32,
    quality: SampleQuality,
    shade: Shade,
    filter: Arc<[crate::paint::filter::Resolved]>,
    clips: Arc<[Clip]>,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
  Start,
  End,
  Left,
  Right,
  Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Baseline {
  Top,
  Hanging,
  Middle,
  Alphabetic,
  Ideographic,
  Bottom,
}

/// Canvas `direction`. `"inherit"` shapes left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Direction {
  #[default]
  Inherit,
  Ltr,
  Rtl,
}

impl Direction {
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "inherit" => Some(Self::Inherit),
      "ltr" => Some(Self::Ltr),
      "rtl" => Some(Self::Rtl),
      _ => None,
    }
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::Inherit => "inherit",
      Self::Ltr => "ltr",
      Self::Rtl => "rtl",
    }
  }

  /// There is no document, so `"inherit"` is left to right.
  pub fn is_rtl(self) -> bool {
    matches!(self, Self::Rtl)
  }
}

/// Canvas `fontKerning`. `"auto"` and `"normal"` keep the kern feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontKerning {
  #[default]
  Auto,
  Normal,
  None,
}

impl FontKerning {
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "auto" => Some(Self::Auto),
      "normal" => Some(Self::Normal),
      "none" => Some(Self::None),
      _ => None,
    }
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::Auto => "auto",
      Self::Normal => "normal",
      Self::None => "none",
    }
  }

  /// `"none"` is the only value that disables kerning.
  pub fn kerns(self) -> bool {
    !matches!(self, Self::None)
  }
}

/// Canvas `fontStretch`. Keywords are case-sensitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontStretch {
  UltraCondensed,
  ExtraCondensed,
  Condensed,
  SemiCondensed,
  #[default]
  Normal,
  SemiExpanded,
  Expanded,
  ExtraExpanded,
  UltraExpanded,
}

impl FontStretch {
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "ultra-condensed" => Some(Self::UltraCondensed),
      "extra-condensed" => Some(Self::ExtraCondensed),
      "condensed" => Some(Self::Condensed),
      "semi-condensed" => Some(Self::SemiCondensed),
      "normal" => Some(Self::Normal),
      "semi-expanded" => Some(Self::SemiExpanded),
      "expanded" => Some(Self::Expanded),
      "extra-expanded" => Some(Self::ExtraExpanded),
      "ultra-expanded" => Some(Self::UltraExpanded),
      _ => None,
    }
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::UltraCondensed => "ultra-condensed",
      Self::ExtraCondensed => "extra-condensed",
      Self::Condensed => "condensed",
      Self::SemiCondensed => "semi-condensed",
      Self::Normal => "normal",
      Self::SemiExpanded => "semi-expanded",
      Self::Expanded => "expanded",
      Self::ExtraExpanded => "extra-expanded",
      Self::UltraExpanded => "ultra-expanded",
    }
  }
}

/// Canvas `fontVariantCaps`. Keywords are case-sensitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontVariantCaps {
  #[default]
  Normal,
  SmallCaps,
  AllSmallCaps,
  PetiteCaps,
  AllPetiteCaps,
  Unicase,
  TitlingCaps,
}

impl FontVariantCaps {
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "normal" => Some(Self::Normal),
      "small-caps" => Some(Self::SmallCaps),
      "all-small-caps" => Some(Self::AllSmallCaps),
      "petite-caps" => Some(Self::PetiteCaps),
      "all-petite-caps" => Some(Self::AllPetiteCaps),
      "unicase" => Some(Self::Unicase),
      "titling-caps" => Some(Self::TitlingCaps),
      _ => None,
    }
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::Normal => "normal",
      Self::SmallCaps => "small-caps",
      Self::AllSmallCaps => "all-small-caps",
      Self::PetiteCaps => "petite-caps",
      Self::AllPetiteCaps => "all-petite-caps",
      Self::Unicase => "unicase",
      Self::TitlingCaps => "titling-caps",
    }
  }

  /// OpenType features for this keyword. Empty for `"normal"`.
  pub fn features(self) -> &'static [&'static str] {
    match self {
      Self::Normal => &[],
      Self::SmallCaps => &["smcp"],
      Self::AllSmallCaps => &["c2sc", "smcp"],
      Self::PetiteCaps => &["pcap"],
      Self::AllPetiteCaps => &["c2pc", "pcap"],
      Self::Unicase => &["unic"],
      Self::TitlingCaps => &["titl"],
    }
  }
}

/// Canvas `textRendering`. Stored only. It does not change the paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextRendering {
  #[default]
  Auto,
  OptimizeSpeed,
  OptimizeLegibility,
  GeometricPrecision,
}

impl TextRendering {
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "auto" => Some(Self::Auto),
      "optimizeSpeed" => Some(Self::OptimizeSpeed),
      "optimizeLegibility" => Some(Self::OptimizeLegibility),
      "geometricPrecision" => Some(Self::GeometricPrecision),
      _ => None,
    }
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::Auto => "auto",
      Self::OptimizeSpeed => "optimizeSpeed",
      Self::OptimizeLegibility => "optimizeLegibility",
      Self::GeometricPrecision => "geometricPrecision",
    }
  }
}

#[derive(Clone, Debug)]
pub struct FontDesc {
  pub size: f32,
  pub weight: u16,
  pub italic: bool,
  /// `small-caps` in the font shorthand. Selects the `smcp` feature.
  pub small_caps: bool,
  pub family: Family,
  /// Canonical shorthand, including `small-caps` and every family.
  pub css: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
  Sans,
  Serif,
  Mono,
  /// Index into the loaded-family table in `text`.
  Named(u32),
  /// Index into that table for a family that is not loaded. Shaping uses
  /// `sans-serif`.
  Unloaded(u32),
}

impl Default for FontDesc {
  fn default() -> Self {
    Self {
      size: 10.0,
      weight: 400,
      italic: false,
      small_caps: false,
      family: Family::Sans,
      css: "10px sans-serif".to_string(),
    }
  }
}

impl Align {
  /// Horizontal shift of the text origin, in unscaled advance space.
  /// `fillText` multiplies this by the `maxWidth` scale.
  pub fn offset(self, rtl: bool, width: f64) -> f64 {
    match self {
      Self::Left => 0.0,
      Self::Right => -width,
      Self::Center => -width / 2.0,
      Self::Start => {
        if rtl {
          -width
        } else {
          0.0
        }
      }
      Self::End => {
        if rtl {
          0.0
        } else {
          -width
        }
      }
    }
  }
}

impl Baseline {
  /// Distance from the anchor down to the alphabetic baseline.
  ///
  /// `hanging` and `ideographic` are distances of those baselines above the
  /// alphabetic baseline. A negative ideographic value sits below it.
  pub fn offset(
    self,
    ascent: f64,
    descent: f64,
    hanging: f64,
    ideographic: f64,
  ) -> f64 {
    match self {
      Self::Alphabetic => 0.0,
      Self::Top => ascent,
      Self::Hanging => hanging,
      Self::Middle => (ascent - descent) / 2.0,
      Self::Ideographic => ideographic,
      Self::Bottom => -descent,
    }
  }
}

#[derive(Clone, Debug)]
pub enum Style {
  Color(CssColor),
  Gradient(u64),
  Pattern(u64),
}

/// Canvas `imageSmoothingQuality`.
///
/// `"low"` samples bilinear. `"medium"` samples bicubic. `"high"` prefilters
/// a scaled `drawImage` with Lanczos-3, then samples that bitmap bilinear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmoothingQuality {
  Low,
  Medium,
  High,
}

impl SmoothingQuality {
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "low" => Some(Self::Low),
      "medium" => Some(Self::Medium),
      "high" => Some(Self::High),
      _ => None,
    }
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::Low => "low",
      Self::Medium => "medium",
      Self::High => "high",
    }
  }
}

/// Kernel stored on an image command.
///
/// `Nearest` is nearest-neighbor. `Low` is bilinear. `Medium` and `High` are
/// bicubic. `drawImage` at `"high"` resamples before recording and stores
/// `Low` on the resampled bitmap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleQuality {
  Nearest,
  Low,
  Medium,
  High,
}

#[derive(Clone, Debug)]
pub struct State {
  pub transform: Affine,
  pub alpha: f32,
  pub fill: Style,
  pub stroke_style: Style,
  pub line_width: f64,
  pub cap: Cap,
  pub join: Join,
  pub miter_limit: f64,
  pub dash: Vec<f64>,
  pub dash_offset: f64,
  pub smoothing: bool,
  pub quality: SmoothingQuality,
  pub font: FontDesc,
  pub align: Align,
  pub baseline: Baseline,
  pub direction: Direction,
  /// BCP 47 tag, `""`, or `"inherit"`. Part of the canvas state.
  pub lang: String,
  /// CSS length, numeric spelling preserved and the unit lowercased.
  pub letter_spacing: String,
  /// CSS length, numeric spelling preserved and the unit lowercased.
  pub word_spacing: String,
  pub font_kerning: FontKerning,
  pub font_stretch: FontStretch,
  pub font_variant_caps: FontVariantCaps,
  pub text_rendering: TextRendering,
  /// CSS filter string. Painted when the drawing is composited.
  pub filter: String,
  /// Compositing operator used when this state is painted.
  pub composite: String,
  pub shadow_blur: f64,
  pub shadow_offset_x: f64,
  pub shadow_offset_y: f64,
  pub shadow_color: CssColor,
  pub clips: Vec<Clip>,
}

/// One `save` or `beginLayer` frame. The two calls share this stack.
pub enum StackEntry {
  Save(State),
  Layer {
    saved: State,
    /// CSS filter from `beginLayer({filter})`, when the call passed one.
    layer_filter: Option<String>,
  },
}

pub struct ClosedLayer {
  pub commands: Vec<Command>,
  pub alpha: f32,
  pub clips: Vec<Clip>,
  /// Filter from `beginLayer({ filter })`, applied to the layer bitmap.
  pub layer_filter: Option<String>,
}

pub enum LayerEnd {
  /// `endLayer` while the top frame is a `save`.
  Save,
  /// `endLayer` with no open layer.
  None,
}

impl Default for State {
  fn default() -> Self {
    Self {
      transform: Affine::IDENTITY,
      alpha: 1.0,
      fill: Style::Color(CssColor::BLACK),
      stroke_style: Style::Color(CssColor::BLACK),
      line_width: 1.0,
      cap: Cap::Butt,
      join: Join::Miter,
      miter_limit: 10.0,
      dash: Vec::new(),
      dash_offset: 0.0,
      smoothing: true,
      quality: SmoothingQuality::Low,
      font: FontDesc::default(),
      align: Align::Start,
      baseline: Baseline::Alphabetic,
      direction: Direction::Inherit,
      lang: "inherit".to_string(),
      letter_spacing: "0px".to_string(),
      word_spacing: "0px".to_string(),
      font_kerning: FontKerning::Auto,
      font_stretch: FontStretch::Normal,
      font_variant_caps: FontVariantCaps::Normal,
      text_rendering: TextRendering::Auto,
      filter: "none".to_string(),
      composite: "source-over".to_string(),
      shadow_blur: 0.0,
      shadow_offset_x: 0.0,
      shadow_offset_y: 0.0,
      shadow_color: CssColor::TRANSPARENT,
      clips: Vec::new(),
    }
  }
}

impl State {
  pub fn stroke(&self) -> Stroke {
    let mut dash = self.dash.clone();
    if dash.len() % 2 == 1 {
      let extra = dash.clone();
      dash.extend(extra);
    }
    Stroke::new(self.line_width)
      .with_caps(self.cap)
      .with_join(self.join)
      .with_miter_limit(self.miter_limit)
      .with_dashes(self.dash_offset, dash)
  }

  /// `imageSmoothingEnabled: false` selects nearest-neighbor.
  pub fn sample_quality(&self) -> SampleQuality {
    if !self.smoothing {
      return SampleQuality::Nearest;
    }
    match self.quality {
      SmoothingQuality::Low => SampleQuality::Low,
      SmoothingQuality::Medium => SampleQuality::Medium,
      SmoothingQuality::High => SampleQuality::High,
    }
  }

  /// Pattern brushes have no destination size at record time, so `"high"`
  /// uses the same bicubic kernel as `"medium"`.
  pub fn pattern_quality(&self) -> SampleQuality {
    match self.sample_quality() {
      SampleQuality::High => SampleQuality::Medium,
      quality => quality,
    }
  }

  pub fn clips(&self) -> Arc<[Clip]> {
    Arc::from(self.clips.as_slice())
  }

  pub fn shade(&self) -> Shade {
    Shade::from_canvas(
      &self.composite,
      self.shadow_blur,
      self.shadow_offset_x,
      self.shadow_offset_y,
      self.shadow_color,
    )
  }

  /// Resolves `filter` against this state's font size and the canvas bitmap.
  pub fn paint_filter(
    &self,
    width: u32,
    height: u32,
  ) -> Arc<[crate::paint::filter::Resolved]> {
    let Some(ops) = crate::paint::filter::parse(&self.filter) else {
      return Arc::from([]);
    };
    Arc::from(
      crate::paint::filter::resolve(
        &ops,
        self.font.size,
        width as f32,
        height as f32,
      )
      .as_slice(),
    )
  }
}

pub fn empty_filter() -> Arc<[crate::paint::filter::Resolved]> {
  Arc::from([])
}

pub struct Drawing {
  id: u64,
  pub width: u32,
  pub height: u32,
  pub attrs: Attrs,
  pub state: State,
  pub stack: Vec<StackEntry>,
  pub path: BezPath,
  /// The current path before the current transformation matrix is applied.
  /// Hit testing uses this when the device path overflows.
  pub user_path: BezPath,
  pub commands: Vec<Command>,
  /// Command lists of open layers, outermost first. One entry per
  /// `StackEntry::Layer`.
  layers: Vec<Vec<Command>>,
}

impl Drawing {
  #[cfg(test)]
  pub(crate) fn from_commands(
    width: u32,
    height: u32,
    attrs: Attrs,
    commands: Vec<Command>,
  ) -> Self {
    let mut drawing = Self::fresh(0, width, height, attrs);
    drawing.commands = commands;
    drawing
  }

  fn fresh(id: u64, width: u32, height: u32, attrs: Attrs) -> Self {
    Self {
      id,
      width,
      height,
      attrs,
      state: State::default(),
      stack: Vec::new(),
      path: BezPath::new(),
      user_path: BezPath::new(),
      commands: Vec::new(),
      layers: Vec::new(),
    }
  }

  pub fn push_command(&mut self, command: Command) {
    if let Some(layer) = self.layers.last_mut() {
      layer.push(command);
    } else {
      self.commands.push(command);
      // A layer's list is not on screen until it is composited into
      // `commands`. The window presents this canvas on the next turn.
      yuzuko_window::note_canvas_draw(self.id);
    }
  }

  /// Draws are recorded into a layer list until `endLayer`.
  pub fn in_layer(&self) -> bool {
    !self.layers.is_empty()
  }

  /// Saves the state, resets layer rendering attributes, and records later
  /// draws into a new command list. The transform is left in place.
  pub fn begin_layer(&mut self, layer_filter: Option<String>) {
    let saved = self.state.clone();
    self.stack.push(StackEntry::Layer {
      saved,
      layer_filter,
    });
    self.layers.push(Vec::new());
    self.state.alpha = 1.0;
    self.state.composite = "source-over".to_string();
    self.state.shadow_blur = 0.0;
    self.state.shadow_offset_x = 0.0;
    self.state.shadow_offset_y = 0.0;
    self.state.shadow_color = CssColor::TRANSPARENT;
    self.state.filter = "none".to_string();
    self.state.clips.clear();
  }

  /// Pops a layer opened by `begin_layer`. The caller composites `commands`.
  pub fn end_layer(&mut self) -> Result<ClosedLayer, LayerEnd> {
    match self.stack.last() {
      Some(StackEntry::Layer { .. }) => {}
      Some(StackEntry::Save(_)) => return Err(LayerEnd::Save),
      None => return Err(LayerEnd::None),
    }
    let Some(StackEntry::Layer {
      saved,
      layer_filter,
    }) = self.stack.pop()
    else {
      return Err(LayerEnd::None);
    };
    let commands = self.layers.pop().unwrap_or_default();
    let alpha = saved.alpha;
    let clips = saved.clips.clone();
    self.state = saved;
    Ok(ClosedLayer {
      commands,
      alpha,
      clips,
      layer_filter,
    })
  }

  pub fn reset_state(&mut self) {
    self.state = State::default();
    self.stack.clear();
    self.layers.clear();
    self.path = BezPath::new();
    self.user_path = BezPath::new();
  }

  pub fn clear_bitmap(&mut self) {
    if self.commands.is_empty() {
      return;
    }
    self.commands.clear();
    yuzuko_window::note_canvas_draw(self.id);
  }
}

struct Slot {
  drawing: Option<Drawing>,
  bitmap: Option<ImageBuf>,
}

std::thread_local! {
  static SLOTS: RefCell<HashMap<u64, Slot>> = RefCell::new(HashMap::new());
  static NEXT_STYLE: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
  static GRADIENTS: RefCell<HashMap<u64, LiveGradient>> =
    RefCell::new(HashMap::new());
  static PATTERNS: RefCell<HashMap<u64, LivePattern>> =
    RefCell::new(HashMap::new());
  static STYLE_OBJECTS: RefCell<HashMap<u64, v8::Global<v8::Object>>> =
    RefCell::new(HashMap::new());
}

pub type LinearEnds = ((f32, f32), (f32, f32));
pub type RadialEnds = ((f32, f32, f32), (f32, f32, f32));

#[derive(Clone, Copy)]
pub struct ConicGradient {
  pub start: f32,
  pub center: (f32, f32),
}

pub struct LiveGradient {
  pub linear: Option<LinearEnds>,
  pub radial: Option<RadialEnds>,
  pub conic: Option<ConicGradient>,
  pub stops: Vec<(f32, CssColor)>,
}

#[derive(Clone)]
pub struct LivePattern {
  pub image: Arc<ImageBuf>,
  pub repeat_x: bool,
  pub repeat_y: bool,
}

fn dimensions(width: f64, height: f64) -> (u32, u32) {
  let width = width.round().max(0.0).min(f64::from(u16::MAX)) as u32;
  let height = height.round().max(0.0).min(f64::from(u16::MAX)) as u32;
  (width, height)
}

pub fn open_2d(id: u64, width: f64, height: f64, attrs: Attrs) {
  let (width, height) = dimensions(width, height);
  SLOTS.with(|slots| {
    let mut slots = slots.borrow_mut();
    if slots
      .get(&id)
      .and_then(|slot| slot.drawing.as_ref())
      .is_some()
    {
      return;
    }
    slots.insert(
      id,
      Slot {
        drawing: Some(Drawing::fresh(id, width, height, attrs)),
        bitmap: None,
      },
    );
  });
}

pub fn take_slot(id: u64) -> Option<Box<dyn Any + Send>> {
  let slot = SLOTS.with(|slots| slots.borrow_mut().remove(&id))?;
  Some(Box::new(slot))
}

pub fn put_slot(id: u64, value: Box<dyn Any + Send>) {
  let Ok(slot) = value.downcast::<Slot>() else {
    return;
  };
  SLOTS.with(|slots| {
    slots.borrow_mut().insert(id, *slot);
  });
}

pub fn present_bitmap(id: u64, width: u32, height: u32, bytes: Vec<u8>) {
  SLOTS.with(|slots| {
    slots.borrow_mut().insert(
      id,
      Slot {
        drawing: None,
        bitmap: Some(ImageBuf {
          width,
          height,
          display_p3: false,
          float16: false,
          premultiplied: false,
          bytes,
        }),
      },
    );
  });
  yuzuko_window::note_canvas_draw(id);
}

pub fn open_bitmap(id: u64, width: f64, height: f64) {
  let (width, height) = dimensions(width, height);
  SLOTS.with(|slots| {
    slots.borrow_mut().insert(
      id,
      Slot {
        drawing: None,
        bitmap: Some(ImageBuf {
          width,
          height,
          display_p3: false,
          float16: false,
          premultiplied: false,
          bytes: vec![0; width as usize * height as usize * 4],
        }),
      },
    );
  });
}

pub fn resized(id: u64, width: f64, height: f64) {
  let (width, height) = dimensions(width, height);
  let changed = SLOTS.with(|slots| {
    let mut slots = slots.borrow_mut();
    let Some(slot) = slots.get_mut(&id) else {
      return false;
    };
    if let Some(drawing) = slot.drawing.as_mut() {
      let attrs = drawing.attrs;
      *drawing = Drawing::fresh(id, width, height, attrs);
    }
    if let Some(bitmap) = slot.bitmap.as_mut() {
      bitmap.width = width;
      bitmap.height = height;
      bitmap.bytes = vec![0; width as usize * height as usize * 4];
      bitmap.display_p3 = false;
      bitmap.float16 = false;
    }
    true
  });
  if changed {
    yuzuko_window::note_canvas_draw(id);
  }
}

pub fn dispose(id: u64) {
  let _ = SLOTS.try_with(|slots| {
    slots.borrow_mut().remove(&id);
  });
  yuzuko_window::clear_canvas_present(id);
}

/// `beginLayer` is still open. `endLayer` and `reset` close it.
pub fn layers_open(id: u64) -> bool {
  with_drawing(id, |drawing| !drawing.layers.is_empty()).unwrap_or(false)
}

pub fn with_drawing<T>(
  id: u64,
  body: impl FnOnce(&mut Drawing) -> T,
) -> Option<T> {
  SLOTS.with(|slots| {
    let mut slots = slots.borrow_mut();
    let drawing = slots.get_mut(&id)?.drawing.as_mut()?;
    Some(body(drawing))
  })
}

pub fn with_bitmap<T>(
  id: u64,
  body: impl FnOnce(&mut ImageBuf) -> T,
) -> Option<T> {
  SLOTS.with(|slots| {
    let mut slots = slots.borrow_mut();
    let bitmap = slots.get_mut(&id)?.bitmap.as_mut()?;
    Some(body(bitmap))
  })
}

pub fn alloc_gradient(gradient: LiveGradient) -> u64 {
  let id = NEXT_STYLE.with(|next| {
    let id = next.get();
    next.set(id + 1);
    id
  });
  GRADIENTS.with(|gradients| {
    gradients.borrow_mut().insert(id, gradient);
  });
  id
}

pub fn with_gradient<T>(
  id: u64,
  body: impl FnOnce(&mut LiveGradient) -> T,
) -> Option<T> {
  GRADIENTS.with(|gradients| {
    let mut gradients = gradients.borrow_mut();
    Some(body(gradients.get_mut(&id)?))
  })
}

pub fn drop_gradient(id: u64) {
  let _ = GRADIENTS.try_with(|gradients| {
    gradients.borrow_mut().remove(&id);
  });
}

pub fn alloc_pattern(pattern: LivePattern) -> u64 {
  let id = NEXT_STYLE.with(|next| {
    let id = next.get();
    next.set(id + 1);
    id
  });
  PATTERNS.with(|patterns| {
    patterns.borrow_mut().insert(id, pattern);
  });
  id
}

pub fn pattern(id: u64) -> Option<LivePattern> {
  PATTERNS.with(|patterns| patterns.borrow().get(&id).cloned())
}

pub fn drop_pattern(id: u64) {
  let _ = PATTERNS.try_with(|patterns| {
    patterns.borrow_mut().remove(&id);
  });
}

pub fn remember_style(id: u64, object: v8::Global<v8::Object>) {
  STYLE_OBJECTS.with(|objects| {
    objects.borrow_mut().insert(id, object);
  });
}

pub fn style_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  STYLE_OBJECTS.with(|objects| {
    objects
      .borrow()
      .get(&id)
      .map(|object| v8::Local::new(scope, object))
  })
}

pub fn resolve_style(style: &Style, quality: SampleQuality) -> Option<Brush> {
  match style {
    Style::Color(color) => Some(Brush::Solid(*color)),
    Style::Gradient(id) => with_gradient(*id, |gradient| {
      let stops: Arc<[(f32, CssColor)]> = Arc::from(gradient.stops.as_slice());
      if let Some((start, end)) = gradient.linear {
        Brush::Linear { start, end, stops }
      } else if let Some((start, end)) = gradient.radial {
        Brush::Radial { start, end, stops }
      } else if let Some(conic) = gradient.conic {
        Brush::Conic {
          start: conic.start,
          center: conic.center,
          stops,
        }
      } else {
        Brush::Solid(CssColor::BLACK)
      }
    }),
    Style::Pattern(id) => pattern(*id).map(|pattern| Brush::Pattern {
      image: pattern.image,
      repeat_x: pattern.repeat_x,
      repeat_y: pattern.repeat_y,
      quality,
    }),
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use kurbo::Affine;
  use kurbo::Rect;

  use super::{
    Attrs, Command, dispose, open_2d, present_bitmap, resized, with_drawing,
  };

  const ID: u64 = 8_100_041;

  struct Guard;

  impl Drop for Guard {
    fn drop(&mut self) {
      dispose(ID);
      yuzuko_window::clear_canvas_present(ID);
    }
  }

  fn clear_rect() -> Command {
    Command::Clear {
      rect: Rect::new(0.0, 0.0, 1.0, 1.0),
      transform: Affine::IDENTITY,
      clips: Arc::from([]),
    }
  }

  #[test]
  fn a_draw_marks_the_canvas_for_the_next_present() {
    let _guard = Guard;
    open_2d(ID, 8.0, 8.0, Attrs::default());
    assert!(!yuzuko_window::canvas_needs_present(ID));

    with_drawing(ID, |drawing| {
      drawing.begin_layer(None);
      drawing.push_command(clear_rect());
      assert!(drawing.end_layer().is_ok());
    });
    assert!(!yuzuko_window::canvas_needs_present(ID));

    with_drawing(ID, |drawing| {
      drawing.push_command(clear_rect());
    });
    assert!(yuzuko_window::canvas_needs_present(ID));

    yuzuko_window::clear_canvas_present(ID);
    with_drawing(ID, |drawing| {
      let _ = drawing.width;
    });
    assert!(!yuzuko_window::canvas_needs_present(ID));

    with_drawing(ID, |drawing| drawing.clear_bitmap());
    assert!(yuzuko_window::canvas_needs_present(ID));
    yuzuko_window::clear_canvas_present(ID);
    with_drawing(ID, |drawing| drawing.clear_bitmap());
    assert!(!yuzuko_window::canvas_needs_present(ID));

    assert!(!resized_without_slot());
    resized(ID, 4.0, 4.0);
    assert!(yuzuko_window::canvas_needs_present(ID));
    yuzuko_window::clear_canvas_present(ID);

    present_bitmap(ID, 1, 1, vec![0, 0, 0, 0]);
    assert!(yuzuko_window::canvas_needs_present(ID));
  }

  fn resized_without_slot() -> bool {
    const MISSING: u64 = 8_100_042;
    resized(MISSING, 2.0, 2.0);
    let pending = yuzuko_window::canvas_needs_present(MISSING);
    yuzuko_window::clear_canvas_present(MISSING);
    pending
  }
}
