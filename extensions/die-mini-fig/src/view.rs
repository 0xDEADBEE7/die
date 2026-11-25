//! A figure in a window: mini-fig's one surface, the same for every figure.
//!
//! Its pixel buffer is sized from the window's cells at each `Resize` and placed at every
//! render. The figure is drawn into a picture of its own when it changes (read again, sized,
//! scrolled), and whenever the pointer or a key moves, the focused panel's ring and the one
//! card are drawn over a copy of that and presented. The pointer finds hits; `Tab` and `S-Tab`
//! move the focus between panels, and `h`, `l`, `j` and `k` the cursor over the focused panel's
//! hits, the same code for every kind of panel. While a window shows the figure its spec and
//! data files are looked at once a second, and read again when one has changed.

use crate::canvas::{Canvas, Pen, Rect, Theme};
use crate::dash::{self, Drawn, Ready};
use crate::hit::{self, Dir, Hit};
use crate::spec;
use die_core::{
    BufferId, CellPixels, Ctx, Event, Frame, PixelBufferId, PixelProducer, PixelRect, PointerKind,
    RenderInfo, Sub, Surface, SurfacePointer, WindowId,
};
use die_draw::Book;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

/// The surface's kind, which names its keymap.
pub const KIND: &str = "mini-fig";

/// A hit's place in key order: its panel, its series, its place along.
pub type Place = (usize, usize, usize);

/// What a key does to a figure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// `Tab` and `S-Tab`: the focus to the next or the previous panel.
    Next,
    Prev,
    /// `h`, `l`, `k` and `j`: the cursor over the focused panel's hits.
    Move(Dir),
    /// `r`: the spec and its data read again.
    Reload,
    /// `Esc`: the cursor cleared, or else the focus.
    Clear,
}

/// A figure: its spec, what was read from it, and how it's shown.
pub struct View {
    /// The spec's file.
    pub path: PathBuf,
    /// The buffer it was opened from, which the toggle goes back to.
    pub from: Option<BufferId>,
    book: Rc<RefCell<Book>>,
    theme: Theme,
    /// The spec read and ready to draw, or why it can't be read.
    figure: Result<Ready, String>,
    /// When each file it was read from last changed.
    stamps: Vec<(Option<SystemTime>, PathBuf)>,
    /// The cells in pixels, and the area in physical pixels, from the latest `Resize`.
    cell: CellPixels,
    area: (u32, u32),
    producer: Option<PixelProducer>,
    /// The figure as drawn, without the ring or the card: none when it must be drawn again.
    base: Option<tiny_skia::Pixmap>,
    /// What that drawing left: the panels' frames and the hits.
    pub drawn: Drawn,
    /// How far the rows are scrolled up under the header, in logical px.
    pub scroll: f32,
    /// The focused panel.
    pub focus: Option<usize>,
    /// The hit whose card shows, by its place in key order.
    pub on: Option<Place>,
    /// The pointer put it there, rather than a key: moving off it takes it away.
    pointed: bool,
    /// The look at the files once a second, while a window shows the figure.
    poll: Option<Sub>,
}

impl View {
    /// The figure of the spec at `path`, read now.
    pub fn new(path: PathBuf, book: Rc<RefCell<Book>>) -> View {
        let mut v = View {
            path,
            from: None,
            book,
            theme: Theme::default(),
            figure: Err(String::new()),
            stamps: Vec::new(),
            cell: CellPixels::default(),
            area: (0, 0),
            producer: None,
            base: None,
            drawn: Drawn::default(),
            scroll: 0.0,
            focus: None,
            on: None,
            pointed: false,
            poll: None,
        };
        v.load();
        v
    }

    /// Read the spec and its tables again, noting when each file last changed.
    pub fn load(&mut self) {
        let fig = spec::read(&self.path);
        let mut files = vec![self.path.clone()];
        if let Ok(fig) = &fig {
            files.extend(fig.files());
        }
        self.stamps = files.into_iter().map(|p| (modified(&p), p)).collect();
        self.figure = fig.map(Ready::new);
        self.base = None;
    }

    /// Whether a file it was read from has changed since.
    fn changed(&self) -> bool {
        self.stamps.iter().any(|(t, p)| modified(p) != *t)
    }

    /// The figure's pixel buffer, once a window has given it a size.
    pub fn pixels(&self) -> Option<PixelBufferId> {
        self.producer.as_ref().map(PixelProducer::id)
    }

    /// The hit whose card shows, if it's drawn.
    pub fn hit(&self) -> Option<&Hit> {
        Some(&self.drawn.hits[index(&self.drawn.hits, self.on)?])
    }

    fn scale(&self) -> f32 {
        if self.cell.scale > 0.0 {
            self.cell.scale as f32
        } else {
            1.0
        }
    }

    /// The area in logical px.
    fn size(&self) -> (f32, f32) {
        let s = self.scale();
        (self.area.0 as f32 / s, self.area.1 as f32 / s)
    }

    /// A window showing the figure has this many cells: its buffer made or sized for them, the
    /// figure drawn again, and the files watched. Nothing before a cell has a size in pixels.
    fn resized(
        &mut self,
        ctx: &mut Ctx,
        w: WindowId,
        (cols, rows): (usize, usize),
        cell: CellPixels,
    ) {
        let area = (cols as u32 * cell.width, rows as u32 * cell.height);
        if area.0 == 0 || area.1 == 0 {
            return;
        }
        if (area, cell) != (self.area, self.cell) {
            (self.area, self.cell, self.base) = (area, cell, None);
        }
        let made = match &mut self.producer {
            Some(p) if p.size() != area => p.resize(area.0, area.1),
            Some(_) => Ok(()),
            None => ctx
                .pixel_buffer(area.0, area.1)
                .map(|(_, p)| self.producer = Some(p)),
        };
        if let Err(e) = made {
            ctx.messages.push(format!("mini-fig: no pixel buffer: {e}"));
            return;
        }
        if let Some(buf) = ctx.window_buffer(w) {
            self.watch(ctx, buf);
        }
        self.paint();
    }

    /// Look at the files once a second while a window shows the figure, reading them again when
    /// one has changed. Once none does, the look stops, until a window's `Resize` starts it.
    fn watch(&mut self, ctx: &mut Ctx, buf: BufferId) {
        if self.poll.as_ref().is_some_and(Sub::is_active) {
            return;
        }
        let poll = ctx.every(Duration::from_secs(1), move |ctx| {
            let mut windows = ctx.window_list().into_iter();
            let shown = windows.any(|w| ctx.window_buffer(w) == Some(buf));
            ctx.with_surface(buf, |v: &mut View, _| {
                if !shown {
                    v.poll = None;
                } else if v.changed() {
                    v.load();
                    v.paint();
                }
            })?;
            Ok(())
        });
        self.poll = Some(poll);
    }

    /// Draw what's owed and present it: the figure into its own picture when that's gone, then
    /// over a copy of it the focused panel's ring and the card of the hit the cursor is on.
    pub fn paint(&mut self) {
        let (scale, (w, h)) = (self.scale(), self.size());
        let Some(p) = self.producer.as_mut() else {
            return;
        };
        let mut book = self.book.borrow_mut();
        let (book, th) = (&mut *book, &self.theme);
        if self.base.is_none() {
            let Some(mut c) = Canvas::new(book, (w, h), scale, th.surface) else {
                return;
            };
            self.drawn = match &self.figure {
                Ok(ready) => dash::draw(&mut c, th, ready, self.scroll),
                Err(why) => {
                    dash::message(&mut c, th, why);
                    Drawn::default()
                }
            };
            self.base = Some(c.pixmap);
            self.focus = self.focus.filter(|&f| f < self.drawn.frames.len());
        }
        let Some(base) = self.base.clone() else {
            return;
        };
        let mut c = Canvas::over(book, base, scale);
        let frames = &self.drawn.frames;
        if let Some(f) = self.focus.and_then(|i| frames.get(i))
            && frames.len() > 1
        {
            // Cut where the frame goes under the header, as the panel is.
            let top = self
                .figure
                .as_ref()
                .map_or(0.0, |r| dash::header(&r.fig) - 4.0);
            let y = f.y.max(top);
            c.outline(
                Rect::new(f.x, y, f.w, f.bottom() - y),
                8.0,
                Pen::new(1.0, th.dim),
            );
        }
        if let Some(i) = index(&self.drawn.hits, self.on) {
            hit::card(&mut c, th, &self.drawn.hits[i], Rect::new(0.0, 0.0, w, h));
        }
        let (out, data) = (p.pixels_mut(), c.pixmap.data());
        if out.len() == data.len() {
            out.copy_from_slice(data);
            p.present(PixelRect::whole(self.area.0, self.area.1));
        }
    }

    /// The pointer: over a hit it shows that hit's card, and off every hit it takes away a
    /// card it showed; a press focuses the panel under it; the wheel scrolls the rows.
    fn pointer(&mut self, p: &SurfacePointer) {
        let s = self.scale();
        let (x, y) = (p.x / s, p.y / s);
        let before = (self.on, self.focus, self.scroll);
        match p.kind {
            PointerKind::Move => {
                let at = hit::at(&self.drawn.hits, x, y).map(|i| self.drawn.hits[i].key);
                if at.is_some() || self.pointed {
                    (self.on, self.pointed) = (at, at.is_some());
                }
            }
            PointerKind::Press(_) => {
                self.focus = self.drawn.frames.iter().position(|f| f.contains(x, y));
            }
            PointerKind::Scroll { dy, .. } => {
                self.scroll_to(self.scroll + dy * self.cell.height as f32 / s)
            }
            _ => {}
        }
        if before != (self.on, self.focus, self.scroll) {
            self.paint();
        }
    }

    /// The pointer left: the card it showed goes.
    fn left(&mut self) {
        if std::mem::take(&mut self.pointed) {
            self.on = None;
            self.paint();
        }
    }

    /// Rows scrolled this far up, as far as they go.
    fn scroll_to(&mut self, to: f32) {
        let most = (self.drawn.height - self.size().1).max(0.0);
        let to = to.clamp(0.0, most);
        if to != self.scroll {
            (self.scroll, self.base) = (to, None);
        }
    }

    /// Scroll a panel's frame into the area under the header, its top first.
    fn show_panel(&mut self, panel: usize) {
        let (Some(f), Ok(ready)) = (self.drawn.frames.get(panel), &self.figure) else {
            return;
        };
        let (top, h) = (dash::header(&ready.fig), self.size().1);
        if f.y < top {
            self.scroll_to(self.scroll - (top - f.y));
        } else if f.bottom() > h - 12.0 {
            self.scroll_to(self.scroll + (f.bottom() - h + 12.0).min(f.y - top));
        }
    }

    /// A key: the focus, the cursor, a reload or a clear, and the figure presented again.
    pub fn key(&mut self, key: Key) {
        let n = self.drawn.frames.len();
        match key {
            Key::Next | Key::Prev if n > 0 => {
                let f = match (self.focus, key) {
                    (None, Key::Next) => 0,
                    (None, _) => n - 1,
                    (Some(f), Key::Next) => (f + 1) % n,
                    (Some(f), _) => (f + n - 1) % n,
                };
                (self.focus, self.on) = (Some(f), None);
                self.show_panel(f);
            }
            Key::Move(dir) => {
                let hits = &self.drawn.hits;
                // From the hovered hit's panel, else the focused one.
                let from = index(hits, self.on);
                let on = from.map(|i| hits[i].key.0);
                let panel = on
                    .filter(|_| self.pointed)
                    .or(self.focus)
                    .or(on)
                    .unwrap_or(0);
                let to = hit::step(hits, panel, from, dir);
                (self.focus, self.pointed) = (Some(panel), false);
                self.on = to.map(|i| hits[i].key).or(self.on);
                self.show_panel(panel);
            }
            Key::Reload => self.load(),
            Key::Clear => {
                if self.on.take().is_none() {
                    self.focus = None;
                }
            }
            Key::Next | Key::Prev => {}
        }
        self.paint();
    }
}

/// When a file last changed; None when it can't be read.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Where the hit at a place in key order is among `hits`, if it's drawn.
fn index(hits: &[Hit], place: Option<Place>) -> Option<usize> {
    hits.binary_search_by_key(&place?, |h| h.key).ok()
}

impl Surface for View {
    fn kind(&self) -> &'static str {
        KIND
    }

    /// The pixel buffer over the whole area, at every render: a frame keeps its cells, not its
    /// placements. It was drawn and presented in the turn that changed it.
    fn render(&mut self, frame: &mut Frame, _info: &RenderInfo) {
        if let Some(p) = &self.producer {
            frame.place(p.id(), -1);
        }
    }

    /// The figure's title, and what the cursor is on, or the focused panel's title, or why a
    /// panel can't be drawn; or why the figure can't be read.
    fn status(&self) -> Option<String> {
        let fig = match &self.figure {
            Ok(r) => &r.fig,
            Err(why) => return Some(format!("mini-fig: {why}")),
        };
        let name = self.path.file_name().map(|n| n.to_string_lossy());
        let title = match fig.title.as_str() {
            "" => name.unwrap_or_default().to_string(),
            t => t.to_string(),
        };
        let panel = self.focus.and_then(|i| fig.panels().nth(i));
        let panel = panel.map(|p| p.title.clone()).filter(|t| !t.is_empty());
        let failed = self.figure.as_ref().ok().and_then(Ready::failed);
        let on = self.hit().map(hit::words).or(panel).or(failed);
        Some(match on {
            Some(on) => format!("mini-fig: {title} · {on}"),
            None => format!("mini-fig: {title}"),
        })
    }

    /// A size, the pointer and its leaving. Keys come through the kind's keymap.
    fn event(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        match ev {
            Event::Resize {
                window,
                cols,
                rows,
                cell,
            } => self.resized(ctx, *window, (*cols, *rows), *cell),
            Event::Pointer(p) => self.pointer(p),
            Event::PointerLeft { .. } => self.left(),
            _ => return false,
        }
        true
    }

    /// Every pointer sample, not one a cell: the card follows the hand.
    fn fine_motion(&self) -> bool {
        true
    }
}
