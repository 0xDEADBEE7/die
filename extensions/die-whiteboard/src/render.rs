//! What a renderer is: both renderers draw the same model through the same view into the
//! window's area, and the surface tells them the same things. [`crate::cells`] draws cells and is
//! what runs today; [`crate::pixels`] will draw a pixel buffer behind a placement once core's
//! pixel slice (M3a) has landed, and slots in here without the model, the tools or the keymap
//! changing.
//!
//! The division of labour: the surface owns the model, the view, the selection and the gesture;
//! it calls [`Renderer::resize`] when its area changes, [`Renderer::board_changed`] when the
//! committed picture does (the board, the view, the selection or the theme), and
//! [`Renderer::render`] whenever core asks for a frame while something's changed, with a
//! [`Scene`] that also carries the gesture's live element, which isn't on the board yet.

use crate::model::{Board, CellSize, Element, Id, Rect};
use crate::view::View;
use die_core::{CellPixels, Color, Frame, RenderInfo};

/// What a render draws: the board and what sits over it.
pub struct Scene<'a> {
    /// The committed elements.
    pub board: &'a Board,
    /// Where they are in the area.
    pub view: &'a View,
    /// Each ink's colour, as the theme has it now; index 0 is ink `1`.
    pub inks: &'a [Color; 6],
    /// The selected elements, drawn in reverse video.
    pub selection: &'a [Id],
    /// The gesture's element in progress, not on the board yet.
    pub live: Option<&'a Element>,
    /// A marquee being dragged, in board units.
    pub marquee: Option<Rect>,
    /// A cell in board units, for labels.
    pub cell: CellSize,
}

/// How much of the committed picture changed.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Damage {
    /// Nothing since the last render.
    #[default]
    None,
    /// Everything: a view change, undo, a theme change, a resize.
    All,
    /// This region, in board units: an element added, moved or removed.
    Region(Rect),
}

impl Damage {
    /// This damage and `more` together.
    pub fn with(self, more: Damage) -> Damage {
        match (self, more) {
            (Damage::None, d) | (d, Damage::None) => d,
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
    /// `cols × cell.width` by `rows × cell.height`.
    fn resize(&mut self, cols: usize, rows: usize, cell: CellPixels);
    /// The committed picture changed by `damage`: what's cached of it is stale there.
    fn board_changed(&mut self, damage: Damage);
    /// Draw the scene into the frame. Called when something changed or the frame is new, never
    /// otherwise, so a renderer that draws nothing new keeps the frame as it is.
    fn render(&mut self, frame: &mut Frame, scene: &Scene<'_>, info: &RenderInfo);
    /// Whether it draws at the view's zoom. The cell renderer draws at 1:1, so the zoom keys
    /// say so instead.
    fn zooms(&self) -> bool {
        false
    }
}
