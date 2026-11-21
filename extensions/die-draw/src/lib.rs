//! die-draw: the drawing the extensions share, lifted from die-markdown under the rule of two
//! (scope D11, D14). It depends on none of DIE's crates, so any extension may use it, and it
//! knows nothing of the editor. Its parts:
//!
//! - `fonts`: the system's fonts and DIE's own, found by looking, and text set in them a
//!   character at a time ([`Book`], [`Pass`], [`Run`]), with each glyph's coverage ([`Mask`]);
//! - `kern` and `gsub`: the kerning and the tabular figures read from a face's own tables;
//! - [`text`]: a line set once, then measured and drawn ([`Line`], [`Extent`], [`TextLook`]);
//! - [`blit`]: a premultiplied RGBA8 target with clipping ([`Pixels`], [`Clip`]);
//! - [`image`]: PNG in and out;
//! - [`export`]: the command lines that put a PNG on the pasteboard and open a file.
//!
//! Sizes are logical (CSS px, points) with a scale beside them; what's drawn is physical.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

mod fonts;
mod gsub;
mod kern;

pub mod blit;
pub mod export;
pub mod image;
pub mod text;

pub use blit::{Clip, Pixels, blend, smoothing};
pub use fonts::{
    Book, Inst, Mask, Metrics, Pass, Run, Shaped, Style as FontStyle, TOFU, Weight, fill, font_dir,
};
pub use text::{Extent, Line, Placed, TextLook, bins};

/// A colour as red, green and blue: an opaque ink.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);
