//! `CanvasRenderingContext2D` level A.

#![allow(clippy::too_many_arguments)]

use std::cell::RefCell;
use std::sync::Arc;

use kurbo::Affine;
use kurbo::Cap;
use kurbo::Join;
use kurbo::Rect;
use vello::peniko::Fill;
use yuzuko_core::op::{Js, New, OpError};
use yuzuko_image::ImageBitmap;
use yuzuko_image::ImageData;
use yuzuko_image::Raster;
use yuzuko_image::SampleFormat;
use yuzuko_image::{ColorSpace, convert};
use yuzuko_image::{FramePixels, frame_pixels};
use yuzuko_macros::op;

use crate::geom::matrix::DomMatrix;
use crate::geom::matrix::Matrix;
use crate::geom::path;
use crate::paint::color::CssColor;
use crate::paint::color::parse_color;
use crate::paint::objects::CanvasGradient;
use crate::paint::objects::CanvasPattern;
use crate::paint::objects::TextMetrics;
use crate::paint::objects::{self};
use crate::paint::record::Align;
use crate::paint::record::Baseline;
use crate::paint::record::Command;
use crate::paint::record::Drawing;
use crate::paint::record::ImageBuf;
use crate::paint::record::Style;
use crate::paint::record::{self};
use crate::paint::replay;
use crate::text;

pub struct Context2D {
  id: u64,
  canvas: RefCell<v8::TracedReference<v8::Object>>,
}

unsafe impl v8::cppgc::GarbageCollected for Context2D {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(canvas) = self.canvas.try_borrow() {
      visitor.trace(&*canvas);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"OffscreenCanvasRenderingContext2D"
  }
}

impl Context2D {
  pub fn create(
    scope: &mut v8::PinScope,
    id: u64,
    canvas: v8::Local<v8::Object>,
  ) -> Self {
    Self {
      id,
      canvas: RefCell::new(yuzuko_core::op::traced(scope, canvas)),
    }
  }

  fn update<T>(
    &self,
    body: impl FnOnce(&mut Drawing) -> Result<T, OpError>,
  ) -> Result<T, OpError> {
    match record::with_drawing(self.id, body) {
      Some(result) => result,
      None => Err(OpError::dom(
        "InvalidStateError",
        "The canvas has no 2D context",
      )),
    }
  }
}

fn js_of(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Js {
  Js(v8::Global::new(scope, value))
}

fn layer_filter_argument(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<String>, OpError> {
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("beginLayer options must be a dictionary"))?;
  let key = v8::String::new(scope, "filter").unwrap();
  let filter = object
    .get(scope, key.into())
    .ok_or_else(|| OpError::type_("failed to read a dictionary member"))?;
  if filter.is_null_or_undefined() {
    return Ok(None);
  }
  let text = filter.to_rust_string_lossy(scope);
  if text.is_empty() {
    return Ok(None);
  }
  if crate::paint::filter::is_filter(&text) {
    Ok(Some(text))
  } else {
    Err(OpError::type_("The filter is not a CSS filter value"))
  }
}

fn covered(bytes: &[u8]) -> bool {
  bytes
    .chunks(4)
    .any(|pixel| pixel.get(3).copied().unwrap_or(0) > 0)
}

fn finite(values: &[f64]) -> bool {
  values.iter().all(|value| value.is_finite())
}

fn record_segment<T>(
  drawing: &mut crate::paint::record::Drawing,
  connect: bool,
  build: impl Fn(&mut kurbo::BezPath) -> T,
) -> T {
  let matrix = drawing.state.transform;
  let _ = path::append_user(
    &mut drawing.user_path,
    Affine::IDENTITY,
    connect,
    &build,
  );
  path::append_user(&mut drawing.path, matrix, connect, &build)
}

fn record_continuing(
  drawing: &mut crate::paint::record::Drawing,
  build: impl Fn(&mut kurbo::BezPath),
) {
  let matrix = drawing.state.transform;
  path::append_continuing(&mut drawing.user_path, Affine::IDENTITY, &build);
  path::append_continuing(&mut drawing.path, matrix, &build);
}

fn normalize_span(origin: f64, size: f64) -> (f64, f64) {
  if size < 0.0 {
    (origin + size, -size)
  } else {
    (origin, size)
  }
}

fn bitmap_length(value: f64) -> Result<u32, OpError> {
  if !value.is_finite() {
    return Err(OpError::type_("The dimension is not finite"));
  }
  let truncated = value.trunc();
  if truncated == 0.0 {
    return Err(OpError::dom(
      "IndexSizeError",
      "ImageData width and height must be non-zero",
    ));
  }
  let size = truncated.abs();
  if size > f64::from(u32::MAX) {
    return Err(OpError::range("ImageData is too large"));
  }
  Ok(size as u32)
}

fn rule_of(name: &str) -> Option<Fill> {
  match name {
    "nonzero" => Some(Fill::NonZero),
    "evenodd" => Some(Fill::EvenOdd),
    _ => None,
  }
}

fn paint_fill(
  drawing: &mut Drawing,
  path: kurbo::BezPath,
  fill: Fill,
  user_space: bool,
) {
  if drawing.state.alpha <= 0.0 || drawing.width == 0 || drawing.height == 0 {
    return;
  }
  let Some(brush) =
    record::resolve_style(&drawing.state.fill, drawing.state.nearest())
  else {
    return;
  };
  // Shapes and `Path2D` are transformed at paint time. The current path was
  // transformed when it was built, so painting it again would apply the
  // matrix twice. Its brush still uses the matrix from paint time.
  let matrix = drawing.state.transform;
  let (geometry, brush_transform) = if user_space {
    (matrix, Affine::IDENTITY)
  } else {
    (Affine::IDENTITY, matrix)
  };
  drawing.push_command(Command::Fill {
    path,
    fill,
    brush,
    transform: geometry,
    brush_transform,
    alpha: drawing.state.alpha,
    clips: drawing.state.clips(),
  });
}

fn paint_stroke(
  drawing: &mut Drawing,
  mut path: kurbo::BezPath,
  user_space: bool,
) {
  if drawing.state.alpha <= 0.0
    || drawing.state.line_width <= 0.0
    || drawing.width == 0
    || drawing.height == 0
  {
    return;
  }
  let Some(brush) =
    record::resolve_style(&drawing.state.stroke_style, drawing.state.nearest())
  else {
    return;
  };
  let matrix = drawing.state.transform;
  if !user_space {
    // The stroke width is scaled by the paint-time matrix. Move the device
    // path back to user space so that matrix scales the stroke once.
    let Some(inverse) = path::inverse(matrix) else {
      return;
    };
    path.apply_affine(inverse);
  }
  drawing.push_command(Command::Stroke {
    path,
    stroke: drawing.state.stroke(),
    brush,
    transform: matrix,
    brush_transform: Affine::IDENTITY,
    alpha: drawing.state.alpha,
    clips: drawing.state.clips(),
  });
}

fn style_value(scope: &mut v8::PinScope, style: &Style) -> Js {
  match style {
    Style::Color(color) => {
      let text = v8::String::new(scope, &serialize_color(*color)).unwrap();
      let value: v8::Local<v8::Value> = text.into();
      js_of(scope, value)
    }
    Style::Gradient(id) | Style::Pattern(id) => {
      let value = record::style_object(scope, *id)
        .map(v8::Local::<v8::Value>::from)
        .unwrap_or_else(|| {
          let null: v8::Local<v8::Value> = v8::null(scope).into();
          null
        });
      js_of(scope, value)
    }
  }
}

fn serialize_color(color: CssColor) -> String {
  crate::paint::color::serialize(color)
}

/// HTML compositing operators plus the CSS blend modes. `"clear"` is kept
/// because the canvas tests still assign it. Matching is case-sensitive.
fn is_composite(value: &str) -> bool {
  matches!(
    value,
    "clear"
      | "copy"
      | "destination-atop"
      | "destination-in"
      | "destination-out"
      | "destination-over"
      | "lighter"
      | "source-atop"
      | "source-in"
      | "source-out"
      | "source-over"
      | "xor"
      | "color"
      | "color-burn"
      | "color-dodge"
      | "darken"
      | "difference"
      | "exclusion"
      | "hard-light"
      | "hue"
      | "lighten"
      | "luminosity"
      | "multiply"
      | "overlay"
      | "saturation"
      | "screen"
      | "soft-light"
  )
}

fn set_style(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<Style> {
  if value.is_string() {
    let text = value.to_rust_string_lossy(scope);
    return parse_color(&text).map(Style::Color);
  }
  if let Some(id) = objects::gradient_id(scope, value) {
    if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
      record::remember_style(id, v8::Global::new(scope, object));
    }
    return Some(Style::Gradient(id));
  }
  if let Some(id) = objects::pattern_id(scope, value) {
    if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
      record::remember_style(id, v8::Global::new(scope, object));
    }
    return Some(Style::Pattern(id));
  }
  None
}

fn external_source(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  self_id: u64,
) -> Result<Option<Raster>, OpError> {
  if let Ok(object) = v8::Local::<v8::Object>::try_from(value)
    && let Some(host) =
      unsafe { yuzuko_core::op::host_ref::<ImageBitmap>(scope, object) }
  {
    if host.get().is_detached() {
      return Err(OpError::dom("InvalidStateError", "The image is detached"));
    }
    return host.get().copy_raster().map(Some).ok_or_else(|| {
      OpError::dom("InvalidStateError", "The image is detached")
    });
  }
  if let Some((id, width, height)) =
    yuzuko_window::offscreen_canvas(scope, value)
  {
    if width == 0.0 || height == 0.0 {
      return Err(OpError::dom(
        "InvalidStateError",
        "The canvas has no pixels",
      ));
    }
    if id == self_id {
      return Ok(None);
    }
    if let Some(snapshot) = yuzuko_window::snapshot_canvas(id) {
      return snapshot_raster(snapshot).map(Some);
    }
    return Raster::new(
      width as u32,
      height as u32,
      ColorSpace::Srgb,
      SampleFormat::Unorm8,
    )
    .map(Some)
    .map_err(|_| {
      OpError::dom("InvalidStateError", "The canvas has no pixels")
    });
  }
  match frame_pixels(scope, value) {
    FramePixels::Ready(raster) => return Ok(Some(raster)),
    FramePixels::Closed => {
      return Err(OpError::dom("InvalidStateError", "The frame is closed"));
    }
    FramePixels::NotFrame => {}
  }
  Err(OpError::type_(
    "The provided value is not an ImageBitmap, a VideoFrame, or an OffscreenCanvas",
  ))
}

fn snapshot_raster(
  snapshot: yuzuko_window::CanvasSnapshot,
) -> Result<Raster, OpError> {
  let space = if snapshot.display_p3 {
    ColorSpace::DisplayP3
  } else {
    ColorSpace::Srgb
  };
  let format = if snapshot.float16 {
    SampleFormat::Float16
  } else {
    SampleFormat::Unorm8
  };
  let mut raster = Raster::new(snapshot.width, snapshot.height, space, format)
    .map_err(|_| {
      OpError::dom("InvalidStateError", "The canvas has no pixels")
    })?;
  if raster.bytes.len() != snapshot.bytes.len() {
    return Err(OpError::dom(
      "InvalidStateError",
      "The canvas has no pixels",
    ));
  }
  raster.bytes = snapshot.bytes;
  Ok(raster)
}

fn premul_image(mut raster: Raster) -> ImageBuf {
  convert(&mut raster, ColorSpace::Srgb);
  if raster.format != SampleFormat::Unorm8 {
    let mut bytes = Vec::new();
    for y in 0..raster.height {
      for x in 0..raster.width {
        let sample = raster.sample(i64::from(x), i64::from(y));
        for channel in sample {
          bytes.push((channel.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
      }
    }
    raster =
      Raster::from_rgba8(raster.width, raster.height, ColorSpace::Srgb, &bytes)
        .expect("rgba");
  }
  yuzuko_image::premultiply(&mut raster);
  ImageBuf {
    width: raster.width,
    height: raster.height,
    display_p3: false,
    float16: false,
    premultiplied: true,
    bytes: raster.bytes,
  }
}

fn crop_image(
  raster: &Raster,
  sx: f64,
  sy: f64,
  sw: f64,
  sh: f64,
) -> Result<ImageBuf, OpError> {
  if sw == 0.0 || sh == 0.0 {
    return Err(OpError::dom(
      "IndexSizeError",
      "The source rectangle has zero area",
    ));
  }
  let flip_x = sw < 0.0;
  let flip_y = sh < 0.0;
  let sw = sw.abs();
  let sh = sh.abs();
  let width = sw.ceil() as u32;
  let height = sh.ceil() as u32;
  if width == 0 || height == 0 {
    return Err(OpError::dom(
      "IndexSizeError",
      "The source rectangle has zero area",
    ));
  }
  let mut cropped =
    Raster::new(width, height, raster.color_space, raster.format)
      .map_err(|_| OpError::range("The image is too large"))?;
  cropped.premultiplied = raster.premultiplied;
  for y in 0..height {
    for x in 0..width {
      let src_x = if flip_x {
        sx - sw + f64::from(x) + 1.0
      } else {
        sx + f64::from(x)
      };
      let src_y = if flip_y {
        sy - sh + f64::from(y) + 1.0
      } else {
        sy + f64::from(y)
      };
      cropped.write_sample(
        i64::from(x),
        i64::from(y),
        raster.sample(src_x.floor() as i64, src_y.floor() as i64),
      );
    }
  }
  Ok(premul_image(cropped))
}

#[op(name = "OffscreenCanvasRenderingContext2D")]
impl Context2D {
  #[getter]
  fn canvas(&self, scope: &mut v8::PinScope) -> Js {
    let value = yuzuko_core::op::traced_local(scope, &self.canvas.borrow())
      .map(v8::Local::<v8::Value>::from)
      .unwrap_or_else(|| v8::null(scope).into());
    Js(v8::Global::new(scope, value))
  }

  fn get_context_attributes(
    &self,
    scope: &mut v8::PinScope,
  ) -> Result<Js, OpError> {
    let attrs = self.update(|drawing| Ok(drawing.attrs))?;
    let object = v8::Object::new(scope);
    let set = |name: &str, value: v8::Local<v8::Value>| {
      let key = v8::String::new(scope, name).unwrap();
      object.set(scope, key.into(), value);
    };
    set("alpha", v8::Boolean::new(scope, attrs.alpha).into());
    set(
      "desynchronized",
      v8::Boolean::new(scope, attrs.desynchronized).into(),
    );
    set(
      "willReadFrequently",
      v8::Boolean::new(scope, attrs.will_read_frequently).into(),
    );
    let space = match attrs.color_space {
      ColorSpace::Srgb => "srgb",
      ColorSpace::DisplayP3 => "display-p3",
    };
    let format = match attrs.format {
      SampleFormat::Unorm8 => "unorm8",
      SampleFormat::Float16 => "float16",
    };
    let space_value: v8::Local<v8::Value> =
      v8::String::new(scope, space).unwrap().into();
    let format_value: v8::Local<v8::Value> =
      v8::String::new(scope, format).unwrap().into();
    set("colorSpace", space_value);
    set("colorType", format_value);
    let object_value: v8::Local<v8::Value> = object.into();
    Ok(js_of(scope, object_value))
  }

  fn save(&self) -> Result<(), OpError> {
    self.update(|drawing| {
      let state = drawing.state.clone();
      drawing.stack.push(record::StackEntry::Save(state));
      Ok(())
    })
  }

  fn restore(&self) -> Result<(), OpError> {
    self.update(|drawing| match drawing.stack.last() {
      Some(record::StackEntry::Layer { .. }) => Err(OpError::dom(
        "InvalidStateError",
        "restore does not match beginLayer",
      )),
      Some(record::StackEntry::Save(_)) => {
        if let Some(record::StackEntry::Save(state)) = drawing.stack.pop() {
          drawing.state = state;
        }
        Ok(())
      }
      None => Ok(()),
    })
  }

  fn begin_layer(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let layer_filter = if args.length() == 0 {
      None
    } else {
      layer_filter_argument(scope, args.get(0))?
    };
    self.update(|drawing| {
      drawing.begin_layer(layer_filter);
      Ok(())
    })
  }

  fn end_layer(&self) -> Result<(), OpError> {
    self.update(|drawing| {
      let closed = match drawing.end_layer() {
        Ok(closed) => closed,
        Err(record::LayerEnd::Save) => {
          return Err(OpError::dom(
            "InvalidStateError",
            "endLayer does not match save",
          ));
        }
        Err(record::LayerEnd::None) => {
          return Err(OpError::dom(
            "InvalidStateError",
            "endLayer has no matching beginLayer",
          ));
        }
      };
      // The filter was validated. vello does not apply it.
      let _ = closed.layer_filter;
      if closed.commands.is_empty() || closed.alpha <= 0.0 {
        return Ok(());
      }
      let Some(image) =
        replay::layer_image(&closed.commands, drawing.width, drawing.height)
      else {
        return Ok(());
      };
      if !covered(&image.bytes) {
        return Ok(());
      }
      drawing.push_command(Command::Image {
        image: Arc::new(image),
        transform: Affine::IDENTITY,
        alpha: closed.alpha,
        nearest: true,
        clips: Arc::from(closed.clips.as_slice()),
      });
      Ok(())
    })
  }

  fn reset(&self) -> Result<(), OpError> {
    self.update(|drawing| {
      drawing.reset_state();
      drawing.clear_bitmap();
      Ok(())
    })
  }

  fn reset_transform(&self) -> Result<(), OpError> {
    self.update(|drawing| {
      drawing.state.transform = Affine::IDENTITY;
      Ok(())
    })
  }

  fn scale(&self, x: f64, y: f64) -> Result<(), OpError> {
    if !finite(&[x, y]) {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.transform *= Affine::scale_non_uniform(x, y);
      Ok(())
    })
  }

  fn rotate(&self, angle: f64) -> Result<(), OpError> {
    if !angle.is_finite() {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.transform *= Affine::rotate(angle);
      Ok(())
    })
  }

  fn translate(&self, x: f64, y: f64) -> Result<(), OpError> {
    if !finite(&[x, y]) {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.transform *= Affine::translate((x, y));
      Ok(())
    })
  }

  fn transform(
    &self,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
  ) -> Result<(), OpError> {
    if !finite(&[a, b, c, d, e, f]) {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.transform *= Affine::new([a, b, c, d, e, f]);
      Ok(())
    })
  }

  fn set_transform(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    if args.length() == 0 {
      return self.update(|drawing| {
        drawing.state.transform = Affine::IDENTITY;
        Ok(())
      });
    }
    if args.length() == 1 {
      let matrix =
        crate::geom::matrix::read_canvas_transform(scope, args.get(0))?;
      if !finite(&[matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f])
      {
        return Ok(());
      }
      return self.update(|drawing| {
        drawing.state.transform = matrix.affine();
        Ok(())
      });
    }
    if args.length() < 6 {
      return Err(OpError::type_(
        "setTransform requires six numbers or a DOMMatrix",
      ));
    }
    let values = [
      objects::number(scope, args.get(0))?,
      objects::number(scope, args.get(1))?,
      objects::number(scope, args.get(2))?,
      objects::number(scope, args.get(3))?,
      objects::number(scope, args.get(4))?,
      objects::number(scope, args.get(5))?,
    ];
    if !finite(&values) {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.transform = Affine::new([
        values[0], values[1], values[2], values[3], values[4], values[5],
      ]);
      Ok(())
    })
  }

  fn get_transform(&self) -> Result<New<DomMatrix>, OpError> {
    self.update(|drawing| {
      Ok(New(DomMatrix::from_matrix(Matrix::from_affine(
        drawing.state.transform,
      ))))
    })
  }

  #[getter]
  fn global_alpha(&self) -> Result<f64, OpError> {
    self.update(|drawing| Ok(f64::from(drawing.state.alpha)))
  }

  #[setter]
  fn global_alpha(&self, value: f64) -> Result<(), OpError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.alpha = value as f32;
      Ok(())
    })
  }

  #[getter]
  fn global_composite_operation(&self) -> Result<String, OpError> {
    self.update(|drawing| Ok(drawing.state.composite.clone()))
  }

  #[setter]
  fn global_composite_operation(&self, value: String) -> Result<(), OpError> {
    if !is_composite(&value) {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.composite = value;
      Ok(())
    })
  }

  #[getter]
  fn fill_style(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    self.update(|drawing| Ok(style_value(scope, &drawing.state.fill)))
  }

  #[setter]
  fn fill_style(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let Some(style) = set_style(scope, value) else {
      return Ok(());
    };
    self.update(|drawing| {
      drawing.state.fill = style;
      Ok(())
    })
  }

  #[getter]
  fn stroke_style(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    self.update(|drawing| Ok(style_value(scope, &drawing.state.stroke_style)))
  }

  #[setter]
  fn stroke_style(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let Some(style) = set_style(scope, value) else {
      return Ok(());
    };
    self.update(|drawing| {
      drawing.state.stroke_style = style;
      Ok(())
    })
  }

  fn create_linear_gradient(
    &self,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
  ) -> Result<New<CanvasGradient>, OpError> {
    if !finite(&[x0, y0, x1, y1]) {
      return Err(OpError::type_("The gradient points are not finite"));
    }
    let _ = self;
    Ok(New(CanvasGradient::linear(x0, y0, x1, y1)))
  }

  fn create_radial_gradient(
    &self,
    x0: f64,
    y0: f64,
    r0: f64,
    x1: f64,
    y1: f64,
    r1: f64,
  ) -> Result<New<CanvasGradient>, OpError> {
    if !finite(&[x0, y0, r0, x1, y1, r1]) {
      return Err(OpError::type_("The gradient points are not finite"));
    }
    let _ = self;
    Ok(New(CanvasGradient::radial(x0, y0, r0, x1, y1, r1)?))
  }

  fn create_pattern(
    &self,
    scope: &mut v8::PinScope,
    image: v8::Local<v8::Value>,
    repetition: String,
  ) -> Result<Js, OpError> {
    let (repeat_x, repeat_y) = match repetition.as_str() {
      "repeat" => (true, true),
      "repeat-x" => (true, false),
      "repeat-y" => (false, true),
      "no-repeat" => (false, false),
      "" => (true, true),
      _ => {
        return Err(OpError::dom(
          "SyntaxError",
          "The pattern repetition is invalid",
        ));
      }
    };
    let external = external_source(scope, image, self.id)?;
    let raster = match external {
      Some(raster) => raster,
      None => self.update(|drawing| {
        replay::rasterize(drawing).ok_or_else(|| {
          OpError::dom("InvalidStateError", "The canvas has no pixels")
        })
      })?,
    };
    if raster.width == 0 || raster.height == 0 {
      return Ok(Js::null(scope));
    }
    let image = Arc::new(premul_image(raster));
    let pattern = CanvasPattern::new(crate::paint::record::LivePattern {
      image,
      repeat_x,
      repeat_y,
    });
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, pattern) };
    if let Some(host) =
      unsafe { yuzuko_core::op::host_ref::<CanvasPattern>(scope, object) }
    {
      record::remember_style(host.get().id(), v8::Global::new(scope, object));
    }
    let object_value: v8::Local<v8::Value> = object.into();
    Ok(js_of(scope, object_value))
  }

  #[getter]
  fn line_width(&self) -> Result<f64, OpError> {
    self.update(|drawing| Ok(drawing.state.line_width))
  }

  #[setter]
  fn line_width(&self, value: f64) -> Result<(), OpError> {
    if !value.is_finite() || value <= 0.0 {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.line_width = value;
      Ok(())
    })
  }

  #[getter]
  fn line_cap(&self) -> Result<String, OpError> {
    self.update(|drawing| {
      Ok(
        match drawing.state.cap {
          Cap::Butt => "butt",
          Cap::Round => "round",
          Cap::Square => "square",
        }
        .to_string(),
      )
    })
  }

  #[setter]
  fn line_cap(&self, value: String) -> Result<(), OpError> {
    let cap = match value.as_str() {
      "butt" => Cap::Butt,
      "round" => Cap::Round,
      "square" => Cap::Square,
      _ => return Ok(()),
    };
    self.update(|drawing| {
      drawing.state.cap = cap;
      Ok(())
    })
  }

  #[getter]
  fn line_join(&self) -> Result<String, OpError> {
    self.update(|drawing| {
      Ok(
        match drawing.state.join {
          Join::Miter => "miter",
          Join::Round => "round",
          Join::Bevel => "bevel",
        }
        .to_string(),
      )
    })
  }

  #[setter]
  fn line_join(&self, value: String) -> Result<(), OpError> {
    let join = match value.as_str() {
      "miter" => Join::Miter,
      "round" => Join::Round,
      "bevel" => Join::Bevel,
      _ => return Ok(()),
    };
    self.update(|drawing| {
      drawing.state.join = join;
      Ok(())
    })
  }

  #[getter]
  fn miter_limit(&self) -> Result<f64, OpError> {
    self.update(|drawing| Ok(drawing.state.miter_limit))
  }

  #[setter]
  fn miter_limit(&self, value: f64) -> Result<(), OpError> {
    if !value.is_finite() || value <= 0.0 {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.miter_limit = value;
      Ok(())
    })
  }

  fn set_line_dash(
    &self,
    scope: &mut v8::PinScope,
    segments: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let array = v8::Local::<v8::Array>::try_from(segments)
      .map_err(|_| OpError::type_("setLineDash requires a sequence"))?;
    let mut dash = Vec::new();
    for index in 0..array.length() {
      let value = array.get_index(scope, index).unwrap();
      let number = objects::number(scope, value)?;
      if !number.is_finite() || number < 0.0 {
        return Err(OpError::dom(
          "IndexSizeError",
          "A dash length is negative or not finite",
        ));
      }
      dash.push(number);
    }
    self.update(|drawing| {
      drawing.state.dash = dash;
      Ok(())
    })
  }

  fn get_line_dash(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    let dash = self.update(|drawing| Ok(drawing.state.dash.clone()))?;
    let array = v8::Array::new(scope, dash.len() as i32);
    for (index, value) in dash.iter().enumerate() {
      let number = v8::Number::new(scope, *value);
      array.set_index(scope, index as u32, number.into());
    }
    let value: v8::Local<v8::Value> = array.into();
    Ok(js_of(scope, value))
  }

  #[getter]
  fn line_dash_offset(&self) -> Result<f64, OpError> {
    self.update(|drawing| Ok(drawing.state.dash_offset))
  }

  #[setter]
  fn line_dash_offset(&self, value: f64) -> Result<(), OpError> {
    if !value.is_finite() {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.dash_offset = value;
      Ok(())
    })
  }

  fn fill_rect(&self, x: f64, y: f64, w: f64, h: f64) -> Result<(), OpError> {
    if !finite(&[x, y, w, h]) {
      return Ok(());
    }
    self.update(|drawing| {
      paint_fill(drawing, path::rect_path(x, y, w, h), Fill::NonZero, true);
      Ok(())
    })
  }

  fn stroke_rect(&self, x: f64, y: f64, w: f64, h: f64) -> Result<(), OpError> {
    if !finite(&[x, y, w, h]) {
      return Ok(());
    }
    self.update(|drawing| {
      paint_stroke(drawing, path::rect_path(x, y, w, h), true);
      Ok(())
    })
  }

  fn clear_rect(&self, x: f64, y: f64, w: f64, h: f64) -> Result<(), OpError> {
    if !finite(&[x, y, w, h]) || w == 0.0 || h == 0.0 {
      return Ok(());
    }
    self.update(|drawing| {
      if drawing.width == 0 || drawing.height == 0 {
        return Ok(());
      }
      let x1 = x + w;
      let y1 = y + h;
      drawing.push_command(Command::Clear {
        rect: Rect::new(x.min(x1), y.min(y1), x.max(x1), y.max(y1)),
        transform: drawing.state.transform,
        clips: drawing.state.clips(),
      });
      Ok(())
    })
  }

  fn begin_path(&self) -> Result<(), OpError> {
    self.update(|drawing| {
      drawing.path = kurbo::BezPath::new();
      drawing.user_path = kurbo::BezPath::new();
      Ok(())
    })
  }

  fn close_path(&self) -> Result<(), OpError> {
    self.update(|drawing| {
      path::close(&mut drawing.path);
      path::close(&mut drawing.user_path);
      Ok(())
    })
  }

  fn move_to(&self, x: f64, y: f64) -> Result<(), OpError> {
    self.update(|drawing| {
      record_segment(drawing, false, |segment| {
        path::move_to(segment, x, y);
      });
      Ok(())
    })
  }

  fn line_to(&self, x: f64, y: f64) -> Result<(), OpError> {
    self.update(|drawing| {
      let connect = !drawing.path.elements().is_empty();
      record_segment(drawing, connect, |segment| {
        path::line_to(segment, x, y);
      });
      Ok(())
    })
  }

  fn quadratic_curve_to(
    &self,
    cpx: f64,
    cpy: f64,
    x: f64,
    y: f64,
  ) -> Result<(), OpError> {
    self.update(|drawing| {
      record_continuing(drawing, |segment| {
        path::quad_to(segment, cpx, cpy, x, y);
      });
      Ok(())
    })
  }

  fn bezier_curve_to(
    &self,
    cp1x: f64,
    cp1y: f64,
    cp2x: f64,
    cp2y: f64,
    x: f64,
    y: f64,
  ) -> Result<(), OpError> {
    self.update(|drawing| {
      record_continuing(drawing, |segment| {
        path::curve_to(segment, cp1x, cp1y, cp2x, cp2y, x, y);
      });
      Ok(())
    })
  }

  fn rect(&self, x: f64, y: f64, w: f64, h: f64) -> Result<(), OpError> {
    self.update(|drawing| {
      record_segment(drawing, false, |segment| {
        path::add_rect(segment, x, y, w, h);
      });
      Ok(())
    })
  }

  fn arc(
    &self,
    x: f64,
    y: f64,
    radius: f64,
    start: f64,
    end: f64,
    counterclockwise: Option<bool>,
  ) -> Result<(), OpError> {
    self.update(|drawing| {
      record_segment(drawing, true, |segment| {
        path::add_ellipse(
          segment,
          x,
          y,
          radius,
          radius,
          0.0,
          start,
          end,
          counterclockwise.unwrap_or(false),
        )
      })
    })
  }

  fn ellipse(
    &self,
    x: f64,
    y: f64,
    radius_x: f64,
    radius_y: f64,
    rotation: f64,
    start: f64,
    end: f64,
    counterclockwise: Option<bool>,
  ) -> Result<(), OpError> {
    self.update(|drawing| {
      record_segment(drawing, true, |segment| {
        path::add_ellipse(
          segment,
          x,
          y,
          radius_x,
          radius_y,
          rotation,
          start,
          end,
          counterclockwise.unwrap_or(false),
        )
      })
    })
  }

  fn arc_to(
    &self,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    radius: f64,
  ) -> Result<(), OpError> {
    self.update(|drawing| {
      let matrix = drawing.state.transform;
      path::append_arc_to(
        &mut drawing.user_path,
        Affine::IDENTITY,
        x1,
        y1,
        x2,
        y2,
        radius,
      )?;
      path::append_arc_to(&mut drawing.path, matrix, x1, y1, x2, y2, radius)
    })
  }

  fn round_rect(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    if args.length() < 4 {
      return Err(OpError::type_("roundRect requires a rectangle"));
    }
    let x = objects::number(scope, args.get(0))?;
    let y = objects::number(scope, args.get(1))?;
    let w = objects::number(scope, args.get(2))?;
    let h = objects::number(scope, args.get(3))?;
    let radii = if args.length() >= 5 {
      objects::radii_of(scope, args.get(4))?
    } else {
      [0.0; 8]
    };
    if !finite(&[x, y, w, h]) || !finite(&radii) {
      return Ok(());
    }
    self.update(|drawing| {
      record_segment(drawing, false, |segment| {
        path::add_round_rect(segment, x, y, w, h, radii);
      });
      Ok(())
    })
  }

  fn fill(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    self.update(|drawing| {
      let (path, fill, user_space) = path_and_rule(scope, args, &drawing.path)?;
      paint_fill(drawing, path, fill, user_space);
      Ok(())
    })
  }

  fn stroke(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    self.update(|drawing| {
      let (path, user_space) = if args.length() >= 1 {
        let path = objects::path_of(scope, args.get(0))
          .ok_or_else(|| OpError::type_("The path argument is not a Path2D"))?;
        (path, true)
      } else {
        (drawing.path.clone(), false)
      };
      paint_stroke(drawing, path, user_space);
      Ok(())
    })
  }

  fn clip(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    self.update(|drawing| {
      let (path, fill, user_space) = path_and_rule(scope, args, &drawing.path)?;
      let transform = if user_space {
        drawing.state.transform
      } else {
        Affine::IDENTITY
      };
      drawing.state.clips.push(crate::paint::record::Clip {
        path,
        fill,
        transform,
      });
      Ok(())
    })
  }

  fn is_point_in_path(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<bool, OpError> {
    self.update(|drawing| {
      let (path, x, y, fill, user_space) =
        point_args(scope, args, &drawing.path)?;
      let even = fill == Fill::EvenOdd;
      if !x.is_finite() || !y.is_finite() {
        return Ok(false);
      }
      if user_space {
        return Ok(path::hit(&path, drawing.state.transform, x, y, even));
      }
      let matrix = drawing.state.transform;
      let det = matrix.determinant();
      if det == 0.0 || det.is_nan() {
        return Ok(false);
      }
      if path::non_finite(&drawing.path) {
        let point = matrix.inverse() * kurbo::Point::new(x, y);
        return Ok(path::contains(&drawing.user_path, point.x, point.y, even));
      }
      Ok(path::contains(&path, x, y, even))
    })
  }

  fn is_point_in_stroke(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<bool, OpError> {
    self.update(|drawing| {
      let (path, x, y, _, user_space) = point_args(scope, args, &drawing.path)?;
      let stroke = drawing.state.stroke();
      if user_space {
        return Ok(path::hit_stroke(
          &path,
          drawing.state.transform,
          &stroke,
          x,
          y,
        ));
      }
      let Some(inverse) = path::inverse(drawing.state.transform) else {
        return Ok(false);
      };
      let mut local = path;
      local.apply_affine(inverse);
      let point = inverse * kurbo::Point::new(x, y);
      Ok(path::hit_stroke(
        &local,
        Affine::IDENTITY,
        &stroke,
        point.x,
        point.y,
      ))
    })
  }

  fn draw_image(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    if args.length() < 3 {
      return Err(OpError::type_(
        "drawImage requires an image and a destination",
      ));
    }
    let image = args.get(0);
    let numbers = (1..args.length())
      .map(|index| objects::number(scope, args.get(index)))
      .collect::<Result<Vec<f64>, _>>()?;
    let external = external_source(scope, image, self.id)?;
    if !finite(&numbers) {
      return Ok(());
    }
    self.update(|drawing| {
      let source = match external {
        Some(raster) => raster,
        None => replay::rasterize(drawing).ok_or_else(|| {
          OpError::dom("InvalidStateError", "The canvas has no pixels")
        })?,
      };
      let (sx, sy, sw, sh, dx, dy, dw, dh) = match numbers.len() {
        2 => (
          0.0,
          0.0,
          f64::from(source.width),
          f64::from(source.height),
          numbers[0],
          numbers[1],
          f64::from(source.width),
          f64::from(source.height),
        ),
        4 => (
          0.0,
          0.0,
          f64::from(source.width),
          f64::from(source.height),
          numbers[0],
          numbers[1],
          numbers[2],
          numbers[3],
        ),
        8 => (
          numbers[0], numbers[1], numbers[2], numbers[3], numbers[4],
          numbers[5], numbers[6], numbers[7],
        ),
        _ => {
          return Err(OpError::type_("drawImage argument count is invalid"));
        }
      };
      let (sx, sw) = normalize_span(sx, sw);
      let (sy, sh) = normalize_span(sy, sh);
      let (dx, dw) = normalize_span(dx, dw);
      let (dy, dh) = normalize_span(dy, dh);
      if sw == 0.0 || sh == 0.0 || dw == 0.0 || dh == 0.0 {
        return Ok(());
      }
      let image = crop_image(&source, sx, sy, sw, sh)?;
      let placement = Affine::translate((dx, dy))
        * Affine::scale_non_uniform(dw / sw, dh / sh);
      drawing.push_command(Command::Image {
        image: Arc::new(image),
        transform: drawing.state.transform * placement,
        alpha: drawing.state.alpha,
        nearest: drawing.state.nearest(),
        clips: drawing.state.clips(),
      });
      Ok(())
    })
  }

  #[getter]
  fn image_smoothing_enabled(&self) -> Result<bool, OpError> {
    self.update(|drawing| Ok(drawing.state.smoothing))
  }

  #[setter]
  fn image_smoothing_enabled(&self, value: bool) -> Result<(), OpError> {
    self.update(|drawing| {
      drawing.state.smoothing = value;
      Ok(())
    })
  }

  #[getter]
  fn image_smoothing_quality(&self) -> Result<String, OpError> {
    self.update(|drawing| {
      Ok(if drawing.state.quality_low {
        "low".to_string()
      } else {
        "medium".to_string()
      })
    })
  }

  #[setter]
  fn image_smoothing_quality(&self, value: String) -> Result<(), OpError> {
    let low = match value.as_str() {
      "low" => true,
      "medium" | "high" => false,
      _ => return Ok(()),
    };
    self.update(|drawing| {
      drawing.state.quality_low = low;
      Ok(())
    })
  }

  fn create_image_data(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<New<ImageData>, OpError> {
    if args.length() < 1 {
      return Err(OpError::type_("createImageData requires a size"));
    }
    if let Some(raster) = image_data_raster(scope, args.get(0)) {
      let settings = if args.length() >= 2 {
        Some(args.get(1))
      } else {
        None
      };
      let (space, format) = image_settings(scope, settings, raster.format)?;
      let blank = Raster::new(raster.width, raster.height, space, format)
        .map_err(|_| OpError::range("ImageData is too large"))?;
      return Ok(New(ImageData::from_raster(scope, blank)?));
    }
    if args.length() < 2 {
      return Err(OpError::type_("createImageData requires a height"));
    }
    let width = bitmap_length(objects::number(scope, args.get(0))?)?;
    let height = bitmap_length(objects::number(scope, args.get(1))?)?;
    let settings = if args.length() >= 3 {
      Some(args.get(2))
    } else {
      None
    };
    let (space, format) =
      image_settings(scope, settings, SampleFormat::Unorm8)?;
    let raster = Raster::new(width, height, space, format)
      .map_err(|_| OpError::range("ImageData is too large"))?;
    Ok(New(ImageData::from_raster(scope, raster)?))
  }

  fn get_image_data(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<New<ImageData>, OpError> {
    if args.length() < 4 {
      return Err(OpError::type_("getImageData requires a rectangle"));
    }
    let sx = objects::number(scope, args.get(0))?;
    let sy = objects::number(scope, args.get(1))?;
    let sw = objects::number(scope, args.get(2))?;
    let sh = objects::number(scope, args.get(3))?;
    if !finite(&[sx, sy, sw, sh]) {
      return Err(OpError::type_("getImageData arguments are not finite"));
    }
    if sw.trunc() == 0.0 || sh.trunc() == 0.0 {
      return Err(OpError::dom(
        "IndexSizeError",
        "The source rectangle is empty",
      ));
    }
    let settings = if args.length() >= 5 {
      Some(args.get(4))
    } else {
      None
    };
    self.update(|drawing| {
      if drawing.width == 0 || drawing.height == 0 {
        return Err(OpError::dom(
          "InvalidStateError",
          "The canvas has no pixels",
        ));
      }
      let source = replay::rasterize(drawing).ok_or_else(|| {
        OpError::dom("InvalidStateError", "The canvas has no pixels")
      })?;
      let (space, format) = image_settings(scope, settings, source.format)?;
      let image = crop_straight(&source, sx, sy, sw, sh, space, format)?;
      Ok(New(ImageData::from_raster(scope, image)?))
    })
  }

  fn put_image_data(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    if args.length() < 3 {
      return Err(OpError::type_("putImageData requires an ImageData"));
    }
    let Some(mut raster) = image_data_raster(scope, args.get(0)) else {
      return Err(OpError::type_("putImageData requires an ImageData"));
    };
    let mut dx = objects::number(scope, args.get(1))?;
    let mut dy = objects::number(scope, args.get(2))?;
    if !dx.is_finite() || !dy.is_finite() {
      return Err(OpError::type_("putImageData destination is not finite"));
    }
    if args.length() >= 7 {
      let dirty_x = objects::number(scope, args.get(3))?;
      let dirty_y = objects::number(scope, args.get(4))?;
      let dirty_w = objects::number(scope, args.get(5))?;
      let dirty_h = objects::number(scope, args.get(6))?;
      if !finite(&[dirty_x, dirty_y, dirty_w, dirty_h]) {
        return Err(OpError::type_(
          "putImageData dirty rectangle is not finite",
        ));
      }
      if dirty_w.trunc() == 0.0 || dirty_h.trunc() == 0.0 {
        return Ok(());
      }
      let (origin_x, origin_y, width, height) =
        positive_rect(dirty_x, dirty_y, dirty_w, dirty_h);
      raster = crop_straight(
        &raster,
        origin_x,
        origin_y,
        width,
        height,
        raster.color_space,
        raster.format,
      )?;
      dx += origin_x;
      dy += origin_y;
    }
    let image = ImageBuf {
      width: raster.width,
      height: raster.height,
      display_p3: raster.color_space == ColorSpace::DisplayP3,
      float16: raster.format == SampleFormat::Float16,
      premultiplied: false,
      bytes: raster.bytes,
    };
    self.update(|drawing| {
      drawing.push_command(Command::Put {
        image: Arc::new(image),
        dx: dx.round() as i32,
        dy: dy.round() as i32,
      });
      Ok(())
    })
  }

  #[getter]
  fn font(&self) -> Result<String, OpError> {
    self.update(|drawing| Ok(text::font_css(drawing.state.font)))
  }

  #[setter]
  fn font(&self, value: String) -> Result<(), OpError> {
    let Some(font) = text::parse_font(&value) else {
      return Ok(());
    };
    self.update(|drawing| {
      drawing.state.font = font;
      Ok(())
    })
  }

  #[getter]
  fn text_align(&self) -> Result<String, OpError> {
    self.update(|drawing| {
      Ok(
        match drawing.state.align {
          Align::Start => "start",
          Align::End => "end",
          Align::Left => "left",
          Align::Right => "right",
          Align::Center => "center",
        }
        .to_string(),
      )
    })
  }

  #[setter]
  fn text_align(&self, value: String) -> Result<(), OpError> {
    let align = match value.as_str() {
      "start" => Align::Start,
      "end" => Align::End,
      "left" => Align::Left,
      "right" => Align::Right,
      "center" => Align::Center,
      _ => return Ok(()),
    };
    self.update(|drawing| {
      drawing.state.align = align;
      Ok(())
    })
  }

  #[getter]
  fn text_baseline(&self) -> Result<String, OpError> {
    self.update(|drawing| {
      Ok(
        match drawing.state.baseline {
          Baseline::Top => "top",
          Baseline::Hanging => "hanging",
          Baseline::Middle => "middle",
          Baseline::Alphabetic => "alphabetic",
          Baseline::Ideographic => "ideographic",
          Baseline::Bottom => "bottom",
        }
        .to_string(),
      )
    })
  }

  #[setter]
  fn text_baseline(&self, value: String) -> Result<(), OpError> {
    let baseline = match value.as_str() {
      "top" => Baseline::Top,
      "hanging" => Baseline::Hanging,
      "middle" => Baseline::Middle,
      "alphabetic" => Baseline::Alphabetic,
      "ideographic" => Baseline::Ideographic,
      "bottom" => Baseline::Bottom,
      _ => return Ok(()),
    };
    self.update(|drawing| {
      drawing.state.baseline = baseline;
      Ok(())
    })
  }

  #[getter]
  fn lang(&self) -> Result<String, OpError> {
    self.update(|drawing| Ok(drawing.state.lang.clone()))
  }

  #[setter]
  fn lang(&self, value: String) -> Result<(), OpError> {
    self.update(|drawing| {
      drawing.state.lang = value;
      Ok(())
    })
  }

  #[getter]
  fn direction(&self) -> Result<String, OpError> {
    self.update(|drawing| {
      Ok(if drawing.state.rtl {
        "rtl".to_string()
      } else {
        "ltr".to_string()
      })
    })
  }

  #[setter]
  fn direction(&self, value: String) -> Result<(), OpError> {
    let rtl = match value.as_str() {
      "rtl" => true,
      "ltr" | "inherit" => false,
      _ => return Ok(()),
    };
    self.update(|drawing| {
      drawing.state.rtl = rtl;
      Ok(())
    })
  }

  fn fill_text(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    self.draw_text(scope, args, false)
  }

  fn stroke_text(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    self.draw_text(scope, args, true)
  }

  fn measure_text(&self, text: String) -> Result<New<TextMetrics>, OpError> {
    self.update(|drawing| {
      let shaped = text::shape(
        &text,
        drawing.state.font,
        drawing.state.rtl,
        &drawing.state.lang,
      );
      Ok(New(TextMetrics::from_shape(
        shaped,
        drawing.state.align,
        drawing.state.baseline,
        drawing.state.rtl,
      )))
    })
  }

  #[length(3)]
  fn fill_text_cluster(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    self.draw_cluster(scope, args, false)
  }

  #[length(3)]
  fn stroke_text_cluster(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    self.draw_cluster(scope, args, true)
  }

  #[getter]
  fn filter(&self) -> Result<String, OpError> {
    self.update(|drawing| Ok(drawing.state.filter.clone()))
  }

  #[setter]
  fn filter(&self, value: String) -> Result<(), OpError> {
    if !crate::paint::filter::is_filter(&value) {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.filter = value;
      Ok(())
    })
  }

  #[getter]
  fn shadow_blur(&self) -> Result<f64, OpError> {
    self.update(|drawing| Ok(drawing.state.shadow_blur))
  }

  #[setter]
  fn shadow_blur(&self, value: f64) -> Result<(), OpError> {
    if !value.is_finite() || value < 0.0 {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.shadow_blur = value;
      Ok(())
    })
  }

  #[getter]
  fn shadow_color(&self) -> Result<String, OpError> {
    self.update(|drawing| Ok(serialize_color(drawing.state.shadow_color)))
  }

  #[setter]
  fn shadow_color(&self, value: String) -> Result<(), OpError> {
    let Some(color) = parse_color(&value) else {
      return Ok(());
    };
    self.update(|drawing| {
      drawing.state.shadow_color = color;
      Ok(())
    })
  }

  #[getter]
  fn shadow_offset_x(&self) -> Result<f64, OpError> {
    self.update(|drawing| Ok(drawing.state.shadow_offset_x))
  }

  #[setter]
  fn shadow_offset_x(&self, value: f64) -> Result<(), OpError> {
    if !value.is_finite() {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.shadow_offset_x = value;
      Ok(())
    })
  }

  #[getter]
  fn shadow_offset_y(&self) -> Result<f64, OpError> {
    self.update(|drawing| Ok(drawing.state.shadow_offset_y))
  }

  #[setter]
  fn shadow_offset_y(&self, value: f64) -> Result<(), OpError> {
    if !value.is_finite() {
      return Ok(());
    }
    self.update(|drawing| {
      drawing.state.shadow_offset_y = value;
      Ok(())
    })
  }

  fn create_conic_gradient(&self) -> Result<(), OpError> {
    let _ = self;
    Err(objects::unsupported())
  }
}

impl Context2D {
  fn draw_text(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    stroke: bool,
  ) -> Result<(), OpError> {
    if args.length() < 3 {
      return Err(OpError::type_("The text position is missing"));
    }
    let text = args
      .get(0)
      .to_string(scope)
      .map(|text| text.to_rust_string_lossy(scope))
      .unwrap_or_default();
    let x = objects::number(scope, args.get(1))?;
    let y = objects::number(scope, args.get(2))?;
    let max_width = if args.length() >= 4 && !args.get(3).is_undefined() {
      Some(objects::number(scope, args.get(3))?)
    } else {
      None
    };
    // Non-finite coordinates draw nothing. A maxWidth that is not positive
    // prepares no glyphs.
    if !finite(&[x, y])
      || max_width.is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
      return Ok(());
    }
    self.update(|drawing| {
      let shaped = text::shape(
        &text,
        drawing.state.font,
        drawing.state.rtl,
        &drawing.state.lang,
      );
      if shaped.width <= 0.0 {
        return Ok(());
      }
      let mut scale = 1.0;
      if let Some(max_width) = max_width
        && max_width >= 0.0
        && f64::from(shaped.width) > max_width
      {
        scale = max_width / f64::from(shaped.width);
      }
      let align = drawing
        .state
        .align
        .offset(drawing.state.rtl, f64::from(shaped.width))
        * scale;
      let ascent = f64::from(shaped.ascent);
      let descent = f64::from(shaped.descent);
      let baseline = drawing.state.baseline.offset(ascent, descent);
      let transform = drawing.state.transform
        * Affine::translate((x + align, y + baseline))
        * Affine::scale_non_uniform(scale, 1.0);
      for run in shaped.runs {
        let glyphs: Arc<[(u32, f32, f32)]> = Arc::from(run.glyphs.as_slice());
        if stroke {
          let Some(brush) = record::resolve_style(
            &drawing.state.stroke_style,
            drawing.state.nearest(),
          ) else {
            continue;
          };
          drawing.push_command(Command::Glyphs {
            font: run.font,
            size: run.size,
            glyphs,
            brush,
            transform,
            alpha: drawing.state.alpha,
            stroke: Some(drawing.state.stroke()),
            clips: drawing.state.clips(),
          });
        } else {
          let Some(brush) =
            record::resolve_style(&drawing.state.fill, drawing.state.nearest())
          else {
            continue;
          };
          drawing.push_command(Command::Glyphs {
            font: run.font,
            size: run.size,
            glyphs,
            brush,
            transform,
            alpha: drawing.state.alpha,
            stroke: None,
            clips: drawing.state.clips(),
          });
        }
      }
      Ok(())
    })
  }

  fn draw_cluster(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    stroke: bool,
  ) -> Result<(), OpError> {
    if args.length() < 3 {
      return Err(OpError::type_("The text cluster position is missing"));
    }
    let cluster = objects::text_cluster(scope, args.get(0))?;
    let x = objects::number(scope, args.get(1))?;
    let y = objects::number(scope, args.get(2))?;
    let options = if args.length() >= 4 && !args.get(3).is_undefined() {
      objects::cluster_paint_options(scope, args.get(3))?
    } else {
      objects::ClusterPaintOptions::default()
    };
    if !finite(&[x, y]) {
      return Ok(());
    }
    let (stored_align, stored_baseline) = cluster.get().paint_style();
    let align = options.align.unwrap_or(stored_align);
    let baseline = options.baseline.unwrap_or(stored_baseline);
    let (stored_x, stored_y) = cluster.get().anchor();
    let anchor_x = x + options.x.unwrap_or(stored_x);
    let anchor_y = y + options.y.unwrap_or(stored_y);
    let glyphs = cluster.get().placed(align, baseline);
    drop(cluster);
    self.update(|drawing| {
      let transform =
        drawing.state.transform * Affine::translate((anchor_x, anchor_y));
      push_shaped_glyphs(drawing, &glyphs, stroke, transform);
      Ok(())
    })
  }
}

fn push_shaped_glyphs(
  drawing: &mut Drawing,
  glyphs: &[text::ShapedGlyph],
  stroke: bool,
  transform: Affine,
) {
  if glyphs.is_empty() || drawing.state.alpha <= 0.0 {
    return;
  }
  let mut groups: Vec<(vello::peniko::FontData, f32, Vec<(u32, f32, f32)>)> =
    Vec::new();
  for glyph in glyphs {
    let same = groups.last().is_some_and(|(font, size, _)| {
      *size == glyph.size
        && font.index == glyph.font.index
        && std::ptr::eq(
          font.data.data().as_ptr(),
          glyph.font.data.data().as_ptr(),
        )
    });
    if same {
      if let Some(group) = groups.last_mut() {
        group.2.push((glyph.id, glyph.x, glyph.y));
      }
    } else {
      groups.push((
        glyph.font.clone(),
        glyph.size,
        vec![(glyph.id, glyph.x, glyph.y)],
      ));
    }
  }
  for (font, size, glyphs) in groups {
    let glyphs: Arc<[(u32, f32, f32)]> = Arc::from(glyphs.as_slice());
    if stroke {
      let Some(brush) = record::resolve_style(
        &drawing.state.stroke_style,
        drawing.state.nearest(),
      ) else {
        continue;
      };
      drawing.push_command(Command::Glyphs {
        font,
        size,
        glyphs,
        brush,
        transform,
        alpha: drawing.state.alpha,
        stroke: Some(drawing.state.stroke()),
        clips: drawing.state.clips(),
      });
    } else {
      let Some(brush) =
        record::resolve_style(&drawing.state.fill, drawing.state.nearest())
      else {
        continue;
      };
      drawing.push_command(Command::Glyphs {
        font,
        size,
        glyphs,
        brush,
        transform,
        alpha: drawing.state.alpha,
        stroke: None,
        clips: drawing.state.clips(),
      });
    }
  }
}

fn path_and_rule(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  current: &kurbo::BezPath,
) -> Result<(kurbo::BezPath, Fill, bool), OpError> {
  if args.length() == 0 {
    return Ok((current.clone(), Fill::NonZero, false));
  }
  let first = args.get(0);
  if first.is_string() {
    let name = first.to_rust_string_lossy(scope);
    let fill = rule_of(&name).ok_or_else(|| {
      OpError::type_("The provided value is not a valid canvas fill rule")
    })?;
    return Ok((current.clone(), fill, false));
  }
  let Some(path) = objects::path_of(scope, first) else {
    return Err(OpError::type_("The path argument is not a Path2D"));
  };
  if args.length() >= 2 && args.get(1).is_string() {
    let name = args.get(1).to_rust_string_lossy(scope);
    let fill = rule_of(&name).ok_or_else(|| {
      OpError::type_("The provided value is not a valid canvas fill rule")
    })?;
    return Ok((path, fill, true));
  }
  Ok((path, Fill::NonZero, true))
}

fn point_args(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  current: &kurbo::BezPath,
) -> Result<(kurbo::BezPath, f64, f64, Fill, bool), OpError> {
  if args.length() < 2 {
    return Err(OpError::type_("The point is missing"));
  }
  let first = args.get(0);
  if let Some(path) = objects::path_of(scope, first) {
    if args.length() < 3 {
      return Err(OpError::type_("The point is missing"));
    }
    let x = objects::number(scope, args.get(1))?;
    let y = objects::number(scope, args.get(2))?;
    let fill = if args.length() >= 4 {
      optional_fill_rule(scope, args.get(3))?
    } else {
      Fill::NonZero
    };
    return Ok((path, x, y, fill, true));
  }
  if first.is_null() || first.is_undefined() || first.is_object() {
    return Err(OpError::type_("The path argument is not a Path2D"));
  }
  let x = objects::number(scope, first)?;
  let y = objects::number(scope, args.get(1))?;
  let fill = if args.length() >= 3 {
    optional_fill_rule(scope, args.get(2))?
  } else {
    Fill::NonZero
  };
  Ok((current.clone(), x, y, fill, false))
}

fn optional_fill_rule(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Fill, OpError> {
  if value.is_undefined() {
    return Ok(Fill::NonZero);
  }
  if !value.is_string() {
    return Err(OpError::type_(
      "The provided value is not a valid canvas fill rule",
    ));
  }
  rule_of(&value.to_rust_string_lossy(scope)).ok_or_else(|| {
    OpError::type_("The provided value is not a valid canvas fill rule")
  })
}

fn image_data_raster(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<Raster> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let host = unsafe { yuzuko_core::op::host_ref::<ImageData>(scope, object) }?;
  host.get().snapshot(scope).ok()
}

fn image_settings(
  scope: &mut v8::PinScope,
  settings: Option<v8::Local<v8::Value>>,
  default_format: SampleFormat,
) -> Result<(ColorSpace, SampleFormat), OpError> {
  let Some(settings) = settings.filter(|value| !value.is_null_or_undefined())
  else {
    return Ok((ColorSpace::Srgb, default_format));
  };
  let object = v8::Local::<v8::Object>::try_from(settings)
    .map_err(|_| OpError::type_("ImageData settings must be an object"))?;
  let color = dictionary_string(scope, object, "colorSpace")?;
  let format = dictionary_string(scope, object, "pixelFormat")?;
  let color = match color.as_deref() {
    None | Some("srgb") => ColorSpace::Srgb,
    Some("display-p3") => ColorSpace::DisplayP3,
    Some(_) => {
      return Err(OpError::type_(
        "The provided value is not a valid enum value of type PredefinedColorSpace",
      ));
    }
  };
  let format = match format.as_deref() {
    None => default_format,
    Some("rgba-unorm8") => SampleFormat::Unorm8,
    Some("rgba-float16") => SampleFormat::Float16,
    Some(_) => {
      return Err(OpError::type_(
        "The provided value is not a valid enum value of type ImageDataPixelFormat",
      ));
    }
  };
  Ok((color, format))
}

fn dictionary_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<String>, OpError> {
  let key = v8::String::new(scope, name).unwrap();
  let value = object
    .get(scope, key.into())
    .ok_or_else(|| OpError::type_("failed to read a dictionary member"))?;
  if value.is_null_or_undefined() {
    return Ok(None);
  }
  Ok(Some(value.to_rust_string_lossy(scope)))
}

#[inline]
fn pixel_index(origin: f64, offset: u32) -> i64 {
  let base = if origin.is_finite() {
    origin.floor().clamp(i64::MIN as f64, i64::MAX as f64) as i64
  } else {
    0
  };
  base.saturating_add(i64::from(offset))
}

/// Moves a negative extent to the opposite corner and returns a positive size.
fn positive_rect(x: f64, y: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
  let w = w.trunc();
  let h = h.trunc();
  let (x, w) = if w < 0.0 { (x + w, -w) } else { (x, w) };
  let (y, h) = if h < 0.0 { (y + h, -h) } else { (y, h) };
  (x, y, w, h)
}

fn crop_straight(
  raster: &Raster,
  sx: f64,
  sy: f64,
  sw: f64,
  sh: f64,
  space: ColorSpace,
  format: SampleFormat,
) -> Result<Raster, OpError> {
  let (origin_x, origin_y, sw, sh) = positive_rect(sx, sy, sw, sh);
  let width = sw as u32;
  let height = sh as u32;
  if width == 0 || height == 0 {
    return Err(OpError::dom("IndexSizeError", "The rectangle is empty"));
  }
  let mut output =
    Raster::new(width, height, raster.color_space, raster.format)
      .map_err(|_| OpError::range("ImageData is too large"))?;
  for y in 0..height {
    for x in 0..width {
      let src_x = pixel_index(origin_x, x);
      let src_y = pixel_index(origin_y, y);
      output.write_sample(
        i64::from(x),
        i64::from(y),
        raster.sample(src_x, src_y),
      );
    }
  }
  convert(&mut output, space);
  if output.format != format {
    output = reformat(output, format);
  }
  Ok(output)
}

fn reformat(raster: Raster, format: SampleFormat) -> Raster {
  let mut output =
    Raster::new(raster.width, raster.height, raster.color_space, format)
      .expect("reformat");
  for y in 0..raster.height {
    for x in 0..raster.width {
      output.write_sample(
        i64::from(x),
        i64::from(y),
        raster.sample(i64::from(x), i64::from(y)),
      );
    }
  }
  output
}
