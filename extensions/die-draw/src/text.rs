//! A line of text set once, then measured and drawn.
//!
//! The skeleton's bodies are crude: they set every character at the stand-in's fixed advance
//! and draw nothing. The lift replaces them.

use crate::Rgb;
use crate::blit::{Clip, Pixels};
use crate::fonts::{Inst, Pass, Style, Weight};

/// How a run is set. `size` is in logical px (CSS px, points): optical size and tracking are
/// looked up at it. The physical size is `size × scale`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TextLook {
    /// The face.
    pub style: Style,
    /// The size in logical px.
    pub size: f32,
    /// Digits as tabular figures.
    pub tabular: bool,
}

impl TextLook {
    /// The proportional face at a weight and a size.
    pub fn new(weight: Weight, size: f32) -> TextLook {
        TextLook {
            style: Style {
                weight,
                ..Style::default()
            },
            size,
            tabular: false,
        }
    }

    /// The same with tabular figures.
    pub fn tabular(self) -> TextLook {
        TextLook {
            tabular: true,
            ..self
        }
    }

    /// The same in the monospace.
    pub fn mono(self) -> TextLook {
        TextLook {
            style: Style {
                mono: true,
                ..self.style
            },
            ..self
        }
    }
}

/// A line's measures in physical px: its advance, ascent and descent (both positive), and its
/// cap height, all from the primary face.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Extent {
    /// How far the pen moves over the line.
    pub width: f32,
    /// How far the face rises above the baseline.
    pub ascent: f32,
    /// How far it drops below, positive.
    pub descent: f32,
    /// How tall its capitals and digits are.
    pub cap: f32,
}

impl Extent {
    /// Ascent and descent.
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }

    /// The baseline that centres the line in a box `height` tall from `top`.
    pub fn baseline_in(&self, top: f32, height: f32) -> f32 {
        top + (height - self.height()) / 2.0 + self.ascent
    }
}

/// A glyph placed on a line: `x` from the line's pen origin, in physical px.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    /// Where it starts.
    pub x: f32,
    /// Its face.
    pub inst: Inst,
    /// Its glyph.
    pub id: u16,
}

/// A line set: its glyphs, its measures, its size in physical px and its subpixel bins.
#[derive(Clone, Debug, Default)]
pub struct Line {
    /// The glyphs, left to right.
    pub glyphs: Vec<Placed>,
    /// Its measures.
    pub extent: Extent,
    /// The size it was set at, in physical px.
    pub px: f32,
    /// Subpixel positions per pixel it's drawn at.
    pub bins: u8,
}

/// Subpixel positions per pixel at a scale: four under 1.5, else two.
pub fn bins(scale: f32) -> u8 {
    if scale < 1.5 { 4 } else { 2 }
}

impl Pass<'_> {
    /// A line's measures without setting it.
    pub fn extent(&mut self, text: &str, look: TextLook, scale: f32) -> Extent {
        let px = look.size * scale;
        let mut run = self.run(look.style, look.size, scale).tabular(look.tabular);
        let mut width = 0.0;
        for c in text.chars() {
            let s = self.next(&mut run, c);
            width += s.kern + s.advance;
        }
        Extent {
            width,
            ascent: 0.8 * px,
            descent: 0.2 * px,
            cap: 0.7 * px,
        }
    }

    /// A line set in a look at a scale.
    pub fn set(&mut self, text: &str, look: TextLook, scale: f32) -> Line {
        let mut run = self.run(look.style, look.size, scale).tabular(look.tabular);
        let mut glyphs = Vec::new();
        let mut pen = 0.0;
        for c in text.chars() {
            let s = self.next(&mut run, c);
            pen += s.kern;
            glyphs.push(Placed {
                x: pen,
                inst: s.inst,
                id: s.id,
            });
            pen += s.advance;
        }
        let px = look.size * scale;
        Line {
            glyphs,
            extent: Extent {
                width: pen,
                ascent: 0.8 * px,
                descent: 0.2 * px,
                cap: 0.7 * px,
            },
            px,
            bins: bins(scale),
        }
    }

    /// A line cut with '…' to fit `max` physical px, or the whole line if it fits.
    pub fn set_within(&mut self, text: &str, look: TextLook, scale: f32, max: f32) -> Line {
        let whole = self.set(text, look, scale);
        if whole.extent.width <= max {
            return whole;
        }
        let mut cut: String = text.chars().collect();
        while cut.pop().is_some() {
            let line = self.set(&format!("{cut}…"), look, scale);
            if line.extent.width <= max {
                return line;
            }
        }
        self.set("…", look, scale)
    }

    /// Draws a line at pen `x` and `baseline` (physical px), in `ink` at `alpha`, clipped to
    /// `clip`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        _line: &Line,
        _to: &mut Pixels<'_>,
        _x: f32,
        _baseline: f32,
        _ink: Rgb,
        _alpha: u8,
        _clip: Clip,
    ) {
    }
}
