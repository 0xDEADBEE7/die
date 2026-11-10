//! What a renderer is: both renderers draw the same model through the same view into the
//! window's area, and the surface tells them the same things. [`crate::pixels`] draws a pixel
//! buffer placed under the area's glyphs, where the host shows pixels (M3a); [`crate::cells`]
//! draws cells, where it doesn't (Linux until the wgpu ring) or where you ask for them.
//!
//! The division of labour: the surface owns the model, the view, the selection and the gesture;
//! it calls [`Renderer::resize`] when its area changes, [`Renderer::board_changed`] when the
//! committed picture does (the board, the view, the selection or the theme),
//! [`Renderer::update`] after each pointer event, so a renderer that draws as events come can,
//! [`Renderer::place`] at every render, and [`Renderer::render`] whenever core asks for a frame
//! while something's changed, with a [`Scene`] that also carries the gesture's live element,
//! which isn't on the board yet, and the tool dial while it's open.

use crate::dial::Dial;
use crate::model::{Board, CellSize, Element, Id, Rect};
use crate::view::View;
use die_core::{CellPixels, Color, Ctx, Frame, RenderInfo, Rgb};

/// What a render draws: the board and what sits over it.
pub struct Scene<'a> {
    /// The committed elements.
    pub board: &'a Board,
    /// Where they are in the area.
    pub view: &'a View,
    /// Each ink's colour, as the theme has it now; index 0 is ink `1`.
    pub inks: &'a [Color; 6],
    /// The `default` face's foreground, which [`Color::Default`] means.
    pub fg: Rgb,
    /// The selected elements, drawn in reverse video.
    pub selection: &'a [Id],
    /// The gesture's element in progress, not on the board yet.
    pub live: Option<&'a Element>,
    /// A marquee being dragged, in board units.
    pub marquee: Option<Rect>,
    /// The tool dial while it's open, in board units: drawn over the committed picture as the
    /// marquee is, never through [`Damage`], since the picture doesn't change under it.
    pub dial: Option<Dial>,
    /// A cell in board units, for labels.
    pub cell: CellSize,
}

/// How much of the committed picture changed.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Damage {
    /// Nothing since the last render.
    #[default]
    None,
    /// Only the view: the picture is the same, elsewhere in the area (a pan, a zoom).
    View,
    /// Everything: undo, a theme change, a resize, the selection.
    All,
    /// This region, in board units: an element added, moved or removed.
    Region(Rect),
}

impl Damage {
    /// This damage and `more` together.
    pub fn with(self, more: Damage) -> Damage {
        match (self, more) {
            (Damage::None, d) | (d, Damage::None) => d,
            (Damage::View, d) | (d, Damage::View) => d,
            (Damage::Region(a), Damage::Region(b)) => Damage::Region(a.union(&b)),
            _ => Damage::All,
        }
    }
}

/// A way to draw a board into the window's area.
pub trait Renderer {
    /// Its name, for messages: `cells` or `pixels`.
    fn name(&self) -> &'static str;
    /// The area is `cols` × `rows` cells of `cell` pixels now: the physical size is
    /// `cols × cell.width` by `rows × cell.height`. With `ctx`, since a pixel buffer is made
    /// here.
    fn resize(&mut self, cols: usize, rows: usize, cell: CellPixels, ctx: &mut Ctx);
    /// The committed picture changed by `damage`: what's cached of it is stale there.
    fn board_changed(&mut self, damage: Damage);
    /// A pointer event changed the scene: a renderer that draws pixels draws them now, in the
    /// event's own turn, so the frame that follows carries them.
    fn update(&mut self, _scene: &Scene<'_>) {}
    /// Every render, whether or not anything changed: a renderer that shows a pixel buffer places
    /// it, since a frame keeps its cells between renders but not its placements.
    fn place(&mut self, _frame: &mut Frame) {}
    /// Draw the scene into the frame. Called when something changed or the frame is new, never
    /// otherwise, so a renderer that draws nothing new keeps the frame as it is.
    fn render(&mut self, frame: &mut Frame, scene: &Scene<'_>, info: &RenderInfo);
    /// Whether it draws at the view's zoom. The cell renderer draws at 1:1, so the zoom keys
    /// say so instead.
    fn zooms(&self) -> bool {
        false
    }
}
