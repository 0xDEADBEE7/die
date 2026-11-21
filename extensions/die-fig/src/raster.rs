//! A scene to pixels: tiny-skia for paths, gradients, clips and opacity, die-draw for text, into
//! premultiplied RGBA8 within a damage box; the measure over die-draw's fonts; and PNG export.

use crate::scene::{Measure, Rect, Scene, TextSpec};
use crate::theme::Color;

/// The text measure over die-draw's fonts, at the scene's scale.
pub struct BookMeasure<'a> {
    /// The fonts.
    pub book: &'a mut die_draw::Book,
    /// Physical px to a logical px.
    pub scale: f32,
}

impl Measure for BookMeasure<'_> {
    fn width(&mut self, _text: &str, _spec: TextSpec) -> f32 {
        todo!("P1-render: BookMeasure::width")
    }

    fn ascent(&mut self, _spec: TextSpec) -> f32 {
        todo!("P1-render: BookMeasure::ascent")
    }

    fn descent(&mut self, _spec: TextSpec) -> f32 {
        todo!("P1-render: BookMeasure::descent")
    }

    fn cap(&mut self, _spec: TextSpec) -> f32 {
        todo!("P1-render: BookMeasure::cap")
    }

    fn fit(&mut self, _text: &str, _spec: TextSpec, _max: f32) -> String {
        todo!("P1-render: BookMeasure::fit")
    }
}

/// A box of physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelBox {
    /// The left column.
    pub x: u32,
    /// The top row.
    pub y: u32,
    /// The width.
    pub w: u32,
    /// The height.
    pub h: u32,
}

/// Draws `scene` into premultiplied RGBA8 `pixels` (`stride` bytes a row, physical size
/// `width × height`), only inside `damage` if given (physical px), clearing `damage` (or the
/// whole buffer) first to `clear` (None = transparent). tiny-skia for paths, die-draw for text.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    _scene: &Scene,
    _pixels: &mut [u8],
    _width: u32,
    _height: u32,
    _stride: usize,
    _damage: Option<PixelBox>,
    _clear: Option<Color>,
    _book: Option<&mut die_draw::Book>,
) {
    todo!("P1-render: raster::draw")
}

/// A logical rectangle's physical pixels at `scale`: rounded out, clipped to `width × height`.
pub fn to_physical(_r: Rect, _scale: f32, _width: u32, _height: u32) -> PixelBox {
    todo!("P1-render: to_physical")
}

/// A PNG of the scene, opaque on `background`, at the scene's scale.
pub fn png(
    _scene: &Scene,
    _background: Color,
    _book: Option<&mut die_draw::Book>,
) -> Result<Vec<u8>, String> {
    todo!("P1-render: raster::png")
}
