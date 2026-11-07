//! Compositing operator and shadow stored with one canvas draw.

use vello_cpu::peniko::BlendMode;
use vello_cpu::peniko::Compose;
use vello_cpu::peniko::Mix;

use crate::paint::color::CssColor;

#[derive(Clone, Copy, Debug)]
pub struct Shadow {
  /// `shadowBlur`. σ is half of this value and is capped at 128.
  pub blur: f32,
  /// Canvas pixels. The current transform does not scale or rotate these.
  pub offset_x: f32,
  pub offset_y: f32,
  /// Straight sRGB red, green, blue, and alpha.
  pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct Shade {
  pub blend: BlendMode,
  pub shadow: Option<Shadow>,
}

impl Shade {
  pub fn source_over() -> Self {
    Self {
      blend: BlendMode::default(),
      shadow: None,
    }
  }

  pub fn is_plain(self) -> bool {
    self.shadow.is_none() && self.blend == BlendMode::default()
  }

  /// Porter-Duff operators that change pixels the source does not cover.
  pub fn paints_when_clear(self) -> bool {
    self.blend.is_destructive()
  }

  pub fn from_canvas(
    composite: &str,
    blur: f64,
    offset_x: f64,
    offset_y: f64,
    color: CssColor,
  ) -> Self {
    let color = color.to_srgb_peniko().components;
    let moved = offset_x != 0.0 || offset_y != 0.0;
    let shadow = if color[3] > 0.0 && (blur > 0.0 || moved) {
      Some(Shadow {
        blur: blur as f32,
        offset_x: offset_x as f32,
        offset_y: offset_y as f32,
        color,
      })
    } else {
      None
    };
    Self {
      blend: blend_mode(composite),
      shadow,
    }
  }
}

fn blend_mode(name: &str) -> BlendMode {
  let (mix, compose) = match name {
    "clear" => (Mix::Normal, Compose::Clear),
    "copy" => (Mix::Normal, Compose::Copy),
    "destination-atop" => (Mix::Normal, Compose::DestAtop),
    "destination-in" => (Mix::Normal, Compose::DestIn),
    "destination-out" => (Mix::Normal, Compose::DestOut),
    "destination-over" => (Mix::Normal, Compose::DestOver),
    "lighter" => (Mix::Normal, Compose::Plus),
    "source-atop" => (Mix::Normal, Compose::SrcAtop),
    "source-in" => (Mix::Normal, Compose::SrcIn),
    "source-out" => (Mix::Normal, Compose::SrcOut),
    "source-over" => (Mix::Normal, Compose::SrcOver),
    "xor" => (Mix::Normal, Compose::Xor),
    "color" => (Mix::Color, Compose::SrcOver),
    "color-burn" => (Mix::ColorBurn, Compose::SrcOver),
    "color-dodge" => (Mix::ColorDodge, Compose::SrcOver),
    "darken" => (Mix::Darken, Compose::SrcOver),
    "difference" => (Mix::Difference, Compose::SrcOver),
    "exclusion" => (Mix::Exclusion, Compose::SrcOver),
    "hard-light" => (Mix::HardLight, Compose::SrcOver),
    "hue" => (Mix::Hue, Compose::SrcOver),
    "lighten" => (Mix::Lighten, Compose::SrcOver),
    "luminosity" => (Mix::Luminosity, Compose::SrcOver),
    "multiply" => (Mix::Multiply, Compose::SrcOver),
    "overlay" => (Mix::Overlay, Compose::SrcOver),
    "saturation" => (Mix::Saturation, Compose::SrcOver),
    "screen" => (Mix::Screen, Compose::SrcOver),
    "soft-light" => (Mix::SoftLight, Compose::SrcOver),
    _ => (Mix::Normal, Compose::SrcOver),
  };
  BlendMode::new(mix, compose)
}

#[cfg(test)]
mod tests {
  use super::Shade;
  use crate::paint::color::CssColor;
  use vello_cpu::peniko::Compose;

  fn shade(name: &str, blur: f64, x: f64, y: f64, color: CssColor) -> Shade {
    Shade::from_canvas(name, blur, x, y, color)
  }

  #[test]
  fn operators_and_shadows_follow_the_canvas_rules() {
    assert!(
      shade("source-over", 0.0, 0.0, 0.0, CssColor::TRANSPARENT).is_plain()
    );
    assert!(shade("nope", 0.0, 0.0, 0.0, CssColor::TRANSPARENT).is_plain());

    let lighter = shade("lighter", 0.0, 0.0, 0.0, CssColor::TRANSPARENT);
    assert_eq!(lighter.blend.compose, Compose::Plus);
    assert!(!lighter.paints_when_clear());

    assert!(
      shade("source-in", 0.0, 0.0, 0.0, CssColor::BLACK).paints_when_clear()
    );
    assert!(
      shade("copy", 0.0, 0.0, 0.0, CssColor::TRANSPARENT).paints_when_clear()
    );
    assert!(
      shade("clear", 0.0, 0.0, 0.0, CssColor::TRANSPARENT).paints_when_clear()
    );
    assert!(
      !shade("xor", 0.0, 0.0, 0.0, CssColor::TRANSPARENT).paints_when_clear()
    );

    let multiply = shade("multiply", 0.0, 0.0, 0.0, CssColor::TRANSPARENT);
    assert!(!multiply.is_plain());
    assert!(!multiply.paints_when_clear());

    let shadow = shade("source-over", 0.0, 3.0, 0.0, CssColor::BLACK);
    assert!(shadow.shadow.is_some());
    assert!(!shadow.is_plain());
    assert!(
      shade("source-over", 4.0, 2.0, 0.0, CssColor::TRANSPARENT)
        .shadow
        .is_none()
    );
    assert!(
      shade("source-over", 0.0, 0.0, 0.0, CssColor::BLACK)
        .shadow
        .is_none()
    );
  }
}
