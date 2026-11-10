//! What a custom surface draws and hears (contract §10, scope D8 and D18): the cell frame it
//! renders into, over its window's area, which is a text window's text area with no gutter, so
//! the buffer's maximum width and centering give it margins, the pixel buffers it places over
//! that area (§11), its cursor and status segment, and the pointer, paste and resize it's told
//! of. Core resolves the frame's cells into the scene, so a surface doesn't need the theme: a
//! default color is the `default` face's.
#![warn(missing_docs)]
use crate::app::{Button, PointerEvent, PointerKind};
use crate::error::*;
use crate::ext::*;
use crate::faces::{Rgb, Style, Underline};
use crate::ids::*;
use crate::keymap::State;
use crate::keys::Mods;
use crate::layout::Geometry;
use crate::pixels::PixelBufferId;
use crate::pointer::{CellHit, WindowPart};
use crate::scene::{Cell, CursorShape, Glyph, Placement, Scene, WindowPlacement, glyphs};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

mod copy;
pub(crate) use copy::Copy;
pub use copy::SurfaceText;

/// A color a surface gives a cell.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Color {
    /// The `default` face's: its foreground for `fg`, its background for `bg`. An underline's
    /// default is the cell's foreground.
    #[default]
    Default,
    /// This color, as a terminal's true color or its palette resolves.
    Rgb(Rgb),
}

/// One cell of a [`Frame`]. Core resolves what the scene's [`Cell`] doesn't hold: reverse video
/// swaps the colors, faint keeps 65 % of the foreground and takes 35 % of the background (as
/// ghostel does), before the swap, invisible text draws a space with no line, and blinking text
/// draws steadily.
/// Cells are built with [`FrameCell::new`] and then their fields set, so a field added later
/// (overline and the link id) breaks no surface.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct FrameCell {
    /// What it draws. A wide character's second cell is [`Glyph::Continuation`].
    pub glyph: Glyph,
    /// The foreground.
    pub fg: Color,
    /// The background.
    pub bg: Color,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Faint, or dim.
    pub faint: bool,
    /// Reverse video.
    pub reverse: bool,
    /// Invisible.
    pub invisible: bool,
    /// Struck through.
    pub strike: bool,
    /// Blinking, which draws steadily for now.
    pub blink: bool,
    /// The underline's style.
    pub underline: Underline,
    /// The underline's color.
    pub underline_color: Color,
}

impl FrameCell {
    /// `glyph` in these colors, with no attributes.
    pub fn new(glyph: Glyph, fg: Color, bg: Color) -> FrameCell {
        FrameCell {
            glyph,
            fg,
            bg,
            bold: false,
            italic: false,
            faint: false,
            reverse: false,
            invisible: false,
            strike: false,
            blink: false,
            underline: Underline::None,
            underline_color: Color::Default,
        }
    }

    /// A space in the default colors.
    pub fn blank() -> FrameCell {
        FrameCell::new(Glyph::Char(' '), Color::Default, Color::Default)
    }

    /// The cell the scene draws for this one, with `default`'s colors `fg` and `bg`.
    fn resolve(&self, fg: Rgb, bg: Rgb) -> Cell {
        let pick = |c: Color, d: Rgb| match c {
            Color::Default => d,
            Color::Rgb(c) => c,
        };
        let (mut fg, mut bg) = (pick(self.fg, fg), pick(self.bg, bg));
        if self.faint {
            let mix = |f: u8, b: u8| ((u16::from(f) * 166 + u16::from(b) * 90) / 256) as u8;
            fg = Rgb(mix(fg.0, bg.0), mix(fg.1, bg.1), mix(fg.2, bg.2));
        }
        if self.reverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        let shown = !self.invisible || self.glyph == Glyph::Continuation;
        let glyph = if shown {
            self.glyph.clone()
        } else {
            Glyph::Char(' ')
        };
        let mut c = Cell::new(glyph, fg, bg);
        c.bold = self.bold;
        c.italic = self.italic;
        c.strike = self.strike && !self.invisible;
        c.underline = if self.invisible {
            Underline::None
        } else {
            self.underline
        };
        c.underline_color = pick(self.underline_color, fg);
        c
    }
}

impl Default for FrameCell {
    fn default() -> FrameCell {
        FrameCell::blank()
    }
}

/// A cell whose colors are resolved already, as die-term's engine gives them, drawn as it is.
impl From<Cell> for FrameCell {
    fn from(c: Cell) -> FrameCell {
        let mut f = FrameCell::new(c.glyph, Color::Rgb(c.fg), Color::Rgb(c.bg));
        (f.bold, f.italic, f.strike) = (c.bold, c.italic, c.strike);
        (f.underline, f.underline_color) = (c.underline, Color::Rgb(c.underline_color));
        f
    }
}

/// The cells a surface renders into: its window's area, row by row. Core keeps it between
/// frames, so a surface can rewrite only the rows that changed. It's new, and blank, at a
/// surface's first render, after a resize, and when a window of another size drew it last.
/// What it places ([`Frame::place`]) isn't kept: a surface places what it shows at every render.
#[derive(Default)]
pub struct Frame {
    cols: usize,
    rows: usize,
    cells: Vec<FrameCell>,
    new: bool,
    /// Drawn in the last composed frame.
    seen: bool,
    /// The pixel buffers placed this render, each over the whole area, with its z.
    placed: Vec<(PixelBufferId, i8)>,
}

impl Frame {
    /// A blank frame of this size, as a test renders into.
    pub fn new(cols: usize, rows: usize) -> Frame {
        let mut f = Frame::default();
        f.fit(cols, rows);
        f
    }
    /// Its width in cells.
    pub fn cols(&self) -> usize {
        self.cols
    }
    /// Its height in cells.
    pub fn rows(&self) -> usize {
        self.rows
    }
    /// Whether it holds nothing the surface wrote (see [`Frame`]), so every row needs writing.
    pub fn is_new(&self) -> bool {
        self.new
    }
    /// Row `r`'s cells, or none past the last row.
    pub fn row(&self, r: usize) -> &[FrameCell] {
        self.cells
            .get(r * self.cols..(r + 1) * self.cols)
            .unwrap_or(&[])
    }
    /// Row `r`'s cells to write, or none past the last row.
    pub fn row_mut(&mut self, r: usize) -> &mut [FrameCell] {
        let cols = self.cols;
        self.cells
            .get_mut(r * cols..(r + 1) * cols)
            .unwrap_or(&mut [])
    }
    /// Put a cell at (row, col). Outside the frame, nothing happens.
    pub fn put(&mut self, row: usize, col: usize, cell: FrameCell) {
        if let Some(c) = self.row_mut(row).get_mut(col) {
            *c = cell;
        }
    }
    /// Blank every cell.
    pub fn clear(&mut self) {
        self.cells.fill(FrameCell::blank());
    }
    /// Show a pixel buffer ([`Ctx::pixel_buffer`]) over the whole area, this render: its pixels
    /// from the area's top-left corner, 1:1 in physical pixels and clipped at the area's edge,
    /// so one sized to the area from a `Resize`'s cells and [`CellPixels`] fills it. At a
    /// negative `z` it draws under the cells' glyphs and over their backgrounds, so text put on
    /// the frame shows over it; at 0 and above, over the glyphs ([`Placement`]). Placed again
    /// at the same z, it draws later. A buffer whose producer is gone draws nothing.
    pub fn place(&mut self, buffer: PixelBufferId, z: i8) {
        self.placed.push((buffer, z));
    }
    /// What's placed this render, in order, each with its z.
    pub fn placed(&self) -> &[(PixelBufferId, i8)] {
        &self.placed
    }
    /// The row's characters, with trailing spaces trimmed, for tests.
    pub fn row_text(&self, r: usize) -> String {
        let s: String = self.row(r).iter().map(|c| c.glyph.to_string()).collect();
        s.trim_end().to_string()
    }
    /// Write `text` on `row` from `col` in `style`'s colors and attributes, as a text window lays
    /// it out: a grapheme a cell and a wide one two, the second a continuation, and a control
    /// character as `^I`. It stops where the row does. Returns the column after it.
    pub fn text(&mut self, row: usize, mut col: usize, text: &str, style: &FrameCell) -> usize {
        for (glyph, w, _) in glyphs(text, Style::default()) {
            if col + w > self.cols {
                break;
            }
            self.put(
                row,
                col,
                FrameCell {
                    glyph,
                    ..style.clone()
                },
            );
            for i in 1..w {
                let glyph = Glyph::Continuation;
                self.put(
                    row,
                    col + i,
                    FrameCell {
                        glyph,
                        ..style.clone()
                    },
                );
            }
            col += w;
        }
        col
    }
    /// Make it `cols` × `rows`, blank and new if that's another size.
    fn fit(&mut self, cols: usize, rows: usize) {
        if (cols, rows) != (self.cols, self.rows) || self.cells.len() != cols * rows {
            (self.cols, self.rows) = (cols, rows);
            self.cells = vec![FrameCell::blank(); cols * rows];
            self.new = true;
        }
    }
}

/// A cell's size in physical pixels, and the display's scale, as the host says them. Zero means
/// not known, as before a window opens and in the harness.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct CellPixels {
    /// A cell's width.
    pub width: u32,
    /// A cell's height.
    pub height: u32,
    /// Physical pixels to a logical one: 2 on a Retina display.
    pub scale: f64,
}

/// What a render is for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RenderInfo {
    /// The window being drawn.
    pub window: WindowId,
    /// It takes input: it's focused, no prompt has the keys, and DIE's window has the keyboard.
    /// Otherwise core draws the cursor hollow.
    pub focused: bool,
    /// The buffer's modal state. Passthrough is insert to you (scope D18).
    pub state: State,
    /// The cells in pixels.
    pub cell: CellPixels,
}

/// Where a surface's cursor is and how it looks. Core draws it [`CursorShape::Hollow`] in a
/// window that doesn't take input, and doesn't draw one that isn't visible.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SurfaceCursor {
    /// Its row in the area.
    pub row: usize,
    /// Its column in the area.
    pub col: usize,
    /// Block, bar or underline (DECSCUSR), or hollow.
    pub shape: CursorShape,
    /// Shown at all (DECTCEM).
    pub visible: bool,
    /// Blinking, which draws steadily for now.
    pub blinking: bool,
}

impl SurfaceCursor {
    /// A visible, steady block at (row, col).
    pub fn at(row: usize, col: usize) -> SurfaceCursor {
        SurfaceCursor {
            row,
            col,
            shape: CursorShape::Block,
            visible: true,
            blinking: false,
        }
    }
}

impl CursorShape {
    /// The shape core gives a text window's cursor in `state`, with passthrough's as insert's,
    /// for a surface that follows the modal state as evil-ghostel does on the main screen.
    pub fn for_state(state: State) -> CursorShape {
        match state {
            State::Insert | State::Passthrough => CursorShape::Bar,
            State::Replace => CursorShape::Underline,
            State::OperatorPending => CursorShape::HalfBlock,
            State::Normal | State::Visual | State::CommandLine | State::Copy => CursorShape::Block,
        }
    }
}

/// The pointer over a surface's window, or dragging from one: its press, release, motion and
/// precise scrolls, as [`PointerEvent`] has them, in the area's own cells and pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SurfacePointer {
    /// What it did.
    pub kind: PointerKind,
    /// The window it's over, or that had the press it's dragging from.
    pub window: WindowId,
    /// The cell's row in the area, held inside it during a drag.
    pub row: usize,
    /// The cell's column in the area, held inside it during a drag.
    pub col: usize,
    /// Physical pixels right of the area's left edge, held inside it during a drag.
    pub x: f32,
    /// Physical pixels below the area's top edge, held inside it during a drag.
    pub y: f32,
    /// 1 for a click, 2 for a double click, and so on. 0 for anything but a press or release.
    pub clicks: u8,
    /// The modifiers held.
    pub mods: Mods,
}

/// What core keeps for surfaces.
#[derive(Default)]
pub(crate) struct Surfaces {
    /// Each surface's frame, by buffer, as its last render left it.
    frames: HashMap<BufferId, Frame>,
    pub(crate) cell: CellPixels,
    /// The surface window a press went to, and its button, which get the drags and the release:
    /// another button clicked meanwhile ends nothing.
    pressed: Option<(WindowId, Button)>,
}

impl Ctx {
    /// Say what the host's cells are in physical pixels, and the display's scale. Each window
    /// showing a surface hears a [`Event::Resize`] with them, queued (C3).
    pub fn set_cell_pixels(&mut self, cell: CellPixels) {
        if self.surfaces.cell == cell {
            return;
        }
        self.surfaces.cell = cell;
        let shown: Vec<_> = self
            .windows
            .iter()
            .map(|(w, win)| (w, win.buffer))
            .collect();
        for (w, buf) in shown {
            self.surface_resized(w, buf);
        }
    }
    /// The cells in pixels, as the host last said them.
    pub fn cell_pixels(&self) -> CellPixels {
        self.surfaces.cell
    }
    /// Where window `w` shows its surface, if it shows one: the text area its buffer's
    /// `max_width` and `center` give a window this size, with no gutter (contract §9, "Gutter and
    /// width"), `text_w` cells from `text_x` on every row, and never past the window's edge.
    pub fn surface_area(&self, w: WindowId) -> Option<Geometry> {
        let win = self.windows.get(w)?;
        let Content::Custom { settings, .. } = &self.buffers.get(win.buffer)?.content else {
            return None;
        };
        let mut g = settings.place(win.cols, win.rows, 0);
        g.text_w = g.text_w.min(win.cols.saturating_sub(g.text_x));
        Some(g)
    }
    /// A surface buffer's maximum width or centering changed: its snapshot takes them, and each
    /// window showing it tells the surface its new area.
    pub(crate) fn surface_area_changed(&mut self, buf: BufferId) {
        self.copy_widths(buf);
        for w in self.windows_showing(buf) {
            self.surface_resized(w, buf);
        }
    }
    /// Tell a window's surface its area's size, if it shows one (C3).
    pub(crate) fn surface_resized(&mut self, w: WindowId, buf: BufferId) {
        let Some(Content::Custom { owner, .. }) = self.buffers.get(buf).map(|b| &b.content) else {
            return;
        };
        let (owner, cell) = (*owner, self.surfaces.cell);
        let Some(Geometry {
            text_w: cols, rows, ..
        }) = self.surface_area(w)
        else {
            return;
        };
        self.defer_as(owner, move |ctx| {
            let ev = Event::Resize {
                window: w,
                cols,
                rows,
                cell,
            };
            ctx.checkout_surface(buf, |s, ctx| s.event(&ev, ctx))?;
            // The surface's text reflowed, so copy state starts over on it.
            ctx.copy_retake(buf);
            Ok(())
        });
    }
    /// What a surface's buffer says of itself now, as whether it takes plain Esc in passthrough
    /// ([`Surface::takes_esc`]). False for text, or a surface checked out.
    pub(crate) fn surface_says(&self, buf: BufferId, f: impl Fn(&dyn Surface) -> bool) -> bool {
        match self.buffers.get(buf).map(|b| &b.content) {
            Some(Content::Custom { obj: Some(s), .. }) => f(s.as_ref()),
            _ => false,
        }
    }
    /// Forget the frames of surfaces the last composed frame didn't draw.
    pub(crate) fn sweep_frames(&mut self) {
        self.surfaces
            .frames
            .retain(|_, f| std::mem::take(&mut f.seen));
    }

    /// Draw a surface's window: its area from its frame, with the margins left blank in the
    /// default face, as a text window's are. Returns its cursor, as `draw_window` does, with the
    /// shape it asks for.
    pub(crate) fn draw_surface(
        &mut self,
        scene: &mut Scene,
        p: &WindowPlacement,
        base: Style,
        focused: bool,
    ) -> Option<(usize, usize, bool, Option<CursorShape>)> {
        let buf = self.windows.get(p.window)?.buffer;
        let g = self.surface_area(p.window)?;
        let (cols, rows) = (g.text_w, g.rows);
        self.copy_sync(buf, true);
        let top = self.copy_top(buf, p.window);
        let info = RenderInfo {
            window: p.window,
            focused: focused && self.prompt.is_none() && self.app_focused,
            state: self.state_of(buf),
            cell: self.surfaces.cell,
        };
        let mut frame = self.surfaces.frames.remove(&buf).unwrap_or_default();
        frame.fit(cols, rows);
        frame.placed.clear();
        let Some(Content::Custom { obj: Some(s), .. }) =
            self.buffers.get_mut(buf).map(|b| &mut b.content)
        else {
            return None;
        };
        let drawn = catch_unwind(AssertUnwindSafe(|| {
            s.scroll_to(top);
            s.render(&mut frame, &info);
            s.cursor(&info)
        }));
        // Until the next good render (§15), which finds the frame blank and new, as at first.
        let panicked = drawn.is_err();
        let cursor = drawn.unwrap_or_else(|_| {
            frame.clear();
            frame.placed.clear();
            frame.text(0, 0, "<render panicked>", &FrameCell::blank());
            None
        });
        let blank = Cell::styled(Glyph::Char(' '), base);
        for r in 0..rows {
            for (c, cell) in frame.row(r).iter().enumerate() {
                let x = p.col + g.text_x + c;
                scene.put(p.row + r, x, cell.resolve(blank.fg, blank.bg));
            }
        }
        // Over the area, at its offset, as the cells are; a destroyed buffer's placement goes.
        for &(buffer, z) in &frame.placed {
            if self.pixels.is_alive(buffer) {
                scene.placements.push(Placement {
                    buffer,
                    col: p.col + g.text_x,
                    row: p.row,
                    cols,
                    rows,
                    z,
                });
            }
        }
        if panicked {
            frame.clear();
        }
        frame.new = panicked;
        frame.seen = true;
        self.surfaces.frames.insert(buf, frame);
        if top.is_some() {
            return self.draw_copy(scene, p, buf, focused);
        }
        let cursor = cursor.filter(|c| c.visible && c.row < rows && c.col < cols)?;
        Some((cursor.row, g.text_x + cursor.col, false, Some(cursor.shape)))
    }

    /// What a window's status line says of `buf`: its status text, or for a surface that gives
    /// one, the surface's own, since your status text is from a file and a surface knows where it
    /// is (a terminal's directory). The second part, for a surface only, is its state as Evil's
    /// tag, which goes at the right: a terminal's cursor is the program's, so it can't always say.
    pub(crate) fn status_line(&mut self, buf: BufferId) -> (String, &'static str) {
        let Some(Content::Custom { obj, .. }) = self.buffers.get(buf).map(|b| &b.content) else {
            return (self.status_text(buf), "");
        };
        let text = match obj.as_ref().and_then(|s| s.status()) {
            Some(own) => own,
            None => self.status_text(buf),
        };
        let tag = match self.state_of(buf) {
            State::Insert | State::Passthrough => "<I>",
            State::Visual => "<V>",
            State::Replace => "<R>",
            State::OperatorPending => "<O>",
            State::Normal | State::Copy | State::CommandLine => "<N>",
        };
        (text, tag)
    }

    /// The surface window a pointer event goes to, if any: the one that had the press, for a
    /// drag or a release, or else the one whose area it's over, or for the wheel whose margin.
    /// A drag or release from a press that went elsewhere, as to text or a margin, isn't a
    /// surface's. With a prompt open, only a press's drags and release.
    pub(crate) fn surface_target(
        &self,
        p: &PointerEvent,
        hit: Option<CellHit>,
    ) -> Option<WindowId> {
        let held = matches!(p.kind, PointerKind::Drag(_) | PointerKind::Release(_));
        let wheel = matches!(p.kind, PointerKind::Scroll { .. });
        // Copy state's pointer is a text window's: a click moves the cursor, a drag selects,
        // the wheel scrolls the snapshot, unless the surface takes the wheel now.
        let copying = |w: WindowId| self.window_buffer(w).is_some_and(|b| self.text(b).is_ok());
        let in_area = |w: WindowId| {
            let (Some(at), Some(g)) = (self.drawn.placement(w), self.surface_area(w)) else {
                return false;
            };
            let x = at.col + g.text_x;
            wheel || (x..x + g.text_w).contains(&p.col)
        };
        let w = match self.surfaces.pressed {
            Some((w, _)) if held => w,
            _ if held || self.prompt.is_some() => return None,
            _ => hit.filter(|h| h.part == WindowPart::Text)?.window,
        };
        if !held && !in_area(w) {
            return None;
        }
        let buf = self.window_buffer(w)?;
        let custom = matches!(self.buffers.get(buf)?.content, Content::Custom { .. });
        let taken = wheel && self.surface_says(buf, |s| s.takes_wheel());
        (custom && (taken || !copying(w))).then_some(w)
    }

    /// Give a pointer event to the surface in `w`, in its area's cells and pixels.
    pub(crate) fn pointer_to_surface(&mut self, w: WindowId, p: PointerEvent) {
        // The first button down holds the pointer until it comes up: a second button's press
        // and release meanwhile are passed on, and take nothing. The same button pressed again
        // begins afresh, its release having been lost.
        let held = self.surfaces.pressed.map(|(_, b)| b);
        match p.kind {
            PointerKind::Press(b) if held.is_none_or(|h| h == b) => {
                self.surfaces.pressed = Some((w, b));
            }
            PointerKind::Release(b) if held == Some(b) => self.surfaces.pressed = None,
            _ => {}
        }
        let (Some(at), Some(g), Some(buf)) = (
            self.drawn.placement(w),
            self.surface_area(w),
            self.window_buffer(w),
        ) else {
            return;
        };
        let (left, cols, rows) = (at.col + g.text_x, g.text_w, g.rows);
        let row = p.row.saturating_sub(at.row).min(rows.saturating_sub(1));
        let col = p.col.saturating_sub(left).min(cols.saturating_sub(1));
        let (cw, ch) = (
            self.surfaces.cell.width as f32,
            self.surfaces.cell.height as f32,
        );
        let (x, y) = match p.px {
            Some((x, y)) => (x - left as f32 * cw, y - at.row as f32 * ch),
            None => (col as f32 * cw, row as f32 * ch),
        };
        let inside =
            |v: f32, cells: usize, size: f32| v.clamp(0.0, (cells as f32 * size - 1.0).max(0.0));
        let ev = SurfacePointer {
            kind: p.kind,
            window: w,
            row,
            col,
            x: inside(x, cols, cw),
            y: inside(y, rows, ch),
            clicks: p.clicks,
            mods: p.mods,
        };
        let _ = self.surface_event(buf, &Event::Pointer(ev));
    }
}

/// The most `surface-paste` sends, its count's copies together.
const PASTE_MAX: usize = 64 << 20;

pub(crate) fn register_commands(reg: &mut Registry) {
    reg.command(
        "surface-paste",
        "Paste into this buffer's surface as one piece, which a terminal brackets: the register \
         given, as the keymap reads it (die-vim's `+` is the clipboard), or else what the system \
         clipboard holds, as many times as the count says",
        |ctx, a| {
            let buf = a.buffer.or_else(|| ctx.focused_buffer());
            let buf = buf.ok_or_else(|| Error::Msg("No buffer to paste into".into()))?;
            let text = match a.register {
                Some(r) => ctx.register_text(r)?,
                None => (ctx.clipboard().map(|r| r.text))
                    .or_else(|| ctx.register('"').map(|r| r.text.clone()))
                    .filter(|t| !t.is_empty())
                    .ok_or_else(|| Error::Msg("Nothing to paste".into()))?,
            };
            if !matches!(
                ctx.buffers.get(buf).map(|b| &b.content),
                Some(Content::Custom { .. })
            ) {
                return Err(Error::Msg(
                    "This buffer shows no surface to paste into".into(),
                ));
            }
            // `3p` pastes it three times, as evil-ghostel's `p` does.
            let n = a.count.unwrap_or(1);
            if text.len().saturating_mul(n) > PASTE_MAX {
                return Err(Error::Msg("That's more than 64 MiB to paste".into()));
            }
            if !text.is_empty() {
                ctx.surface_event(buf, &Event::Paste(text.repeat(n)))?; // `_` pastes nothing
            }
            Ok(())
        },
    );
}
