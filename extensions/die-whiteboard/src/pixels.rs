//! The pixel renderer: a stub until core's pixel slice (M3a, the brief's Part B) has landed.
//! The next builder fills this file; nothing outside it changes but the one line in
//! `surface.rs` that picks a renderer.
//!
//! What it will do, as the brief settles it:
//!
//! - **One pixel buffer** sized to the area in physical pixels, from [`Renderer::resize`]'s
//!   `cols × cell.width` by `rows × cell.height`: `ctx.pixel_buffer(w, h)` gives a
//!   `PixelBufferId` and a `Producer`, which owns the premultiplied RGBA8 buffer. The producer
//!   comes to the renderer through a constructor the surface calls from `event()`, where it has
//!   `Ctx`; `render()` has none, as the contract says.
//! - **One placement** covering the whole area at z = -1, from `frame.place(id, -1)` in
//!   [`Renderer::render`]: below the glyph layer, above the cell backgrounds, so labels written
//!   with `frame.text` over it draw in the editor's font (call 4 of the brief's settled calls).
//! - **Rasterization** with tiny-skia through [`crate::canvas`], in physical pixels at
//!   `view.factor(cell.scale)`, translated by `view.pan`: [`crate::canvas::draw_element`] for
//!   each element, the same calls the exporter makes at its own scale.
//! - **A committed raster cached**: [`Renderer::board_changed`] with [`Damage::Region`]
//!   re-rasterizes that rect of the committed picture and `present(damage)`s it; [`Damage::All`]
//!   re-rasterizes everything (undo, zoom, resize, a theme change). While a gesture goes, the
//!   live element's new segment is drawn over a copy of the committed raster's damage rect, and
//!   that rect alone is presented, so a pointer sample costs one small rasterization. During a
//!   pan the committed raster is translated, and re-rasterized when the gesture ends.
//! - **On the main thread**, inside `event()` and `render()`: the producer thread is the
//!   contract's shape (§11) and stays in reserve.
//! - [`Renderer::zooms`] says true, so `+`, `-`, `0` and Cmd+wheel change the view's zoom.
//!
//! The surface picks it when the host has a pixel sink (`Ctx::pixels_supported()`, which M3a's
//! core half added) and falls back to [`crate::cells`] otherwise (Linux, until the wgpu ring). A placement
//! whose buffer core has no pixels for yet draws nothing, so the first frame after a resize
//! shows the cells' labels alone until the raster is presented.

use crate::render::Renderer;

/// Whether the pixel renderer exists in this build. False until M3a's core slice has landed
/// and this file is filled; the surface reads it to pick a renderer.
pub const AVAILABLE: bool = false;

/// The pixel renderer, once it exists. Until then nothing constructs one: [`AVAILABLE`] is
/// false, and [`new`] gives none.
pub fn new() -> Option<Box<dyn Renderer>> {
    None
}
