//! One file's page: the rendered view as a picture, in a buffer of its own that the file's
//! window shows. The typesetter ([`crate::typeset`]) lays the text out and paints it; this is the
//! editor's side of it: the pixel buffers the windows show, where the reader is in each, the
//! link the keyboard is on, and when to lay out and paint again.
//!
//! A placement shows a pixel buffer from its top-left corner, one pixel to a pixel, and can't be
//! moved (contract §11), so the buffer is the window and not the page: a scroll paints the part
//! of the page the window shows, and presents it. Each window showing the page has a view of
//! its own, as two browser windows on one page have: its own pixel buffer, a layout for its own
//! width, its own place and its own ringed link. So the narrower of two windows side by side
//! shows the page set for its width, and a key scrolls by the height of the window it's typed
//! in. Two views of one width share their layout. A window split off one showing the page
//! starts where that one is. A view's buffer is made at its window's first `Resize` that says
//! what a cell is in pixels, and grows with the area but never shrinks, since a resize blanks
//! the picture until the next present.
//!
//! What's the page's and not a window's: its file, the `<details>` the reader opened, and where
//! the first window to show it starts.
//!
//! The state is shared with the extension, as a board's is: a command reaches it without checking
//! the surface out, `save_session` reads it with no `Ctx`, and the surface's own events find it
//! there. Whoever changes it ends with [`State::refresh`], which lays a view out again if the
//! text, its width, the scale, the colours, the pictures or the opened details are no longer
//! what its layout was made from, and paints what is owed, so the present rides that turn's
//! frame.
use crate::typeset::{
    AlertInk, Env, Fonts, LinkBox, Marks, Page, Palette, Picture, Pictures, Rgb, Syntax,
};
use crate::{Colours, Markdown, plan};
use die_core::{
    Args, BufferId, Button, CellPixels, Ctx, Error, Event, Frame, PixelBufferId, PixelProducer,
    PixelRect, PointerKind, RenderInfo, Result, Surface, SurfacePointer, TextBuf, Underline,
    WindowId,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// What `j` and `k` scroll by, in CSS pixels: two lines and a half of text.
const STEP: f32 = 60.0;
/// The room left over a heading or a link that was scrolled to, in CSS pixels.
const GAP: f32 = 16.0;

/// One page's state, shared by its surface and the extension.
pub(crate) type Shared = Rc<RefCell<State>>;
/// What every page is drawn with, shared by them all.
pub(crate) type SharedKit = Rc<RefCell<Kit>>;
/// What a job read: each picture file, and the picture or why there's none.
pub(crate) type Read = Vec<(PathBuf, std::result::Result<Picture, String>)>;

// ------------------------------------------------------------------ what pages share

/// What every page is drawn with: your options, the fonts, loaded once when the first page
/// wants them, and the pictures.
#[derive(Default)]
pub(crate) struct Kit {
    /// Your `palette` option.
    pub colours: Colours,
    /// Your `front_matter` option.
    pub front_matter: bool,
    /// Your `fonts` option.
    pub own_fonts: Option<PathBuf>,
    fonts: Option<Fonts>,
    /// Whether the fonts were asked for: once, whatever came of it.
    tried: bool,
    pictures: Pictures,
    /// The pictures a job is reading now.
    loading: HashSet<PathBuf>,
    /// Goes up when pictures come back: a layout made before then is behind.
    arrivals: u64,
}

impl Kit {
    /// Whether there are fonts to set a page in. They're loaded at the first ask, and a failure
    /// is said once: the view is the one in place from then on.
    pub fn has_fonts(&mut self, ctx: &mut Ctx) -> bool {
        if !self.tried {
            self.tried = true;
            match Fonts::load(self.own_fonts.as_deref()) {
                Ok(fonts) => self.fonts = Some(fonts),
                Err(why) => ctx.messages.push(format!(
                    "markdown: no fonts to set a page in, so the view is in place: {why}"
                )),
            }
        }
        self.fonts.is_some()
    }

    /// The colours your option names, as they are now.
    fn palette(&self, ctx: &Ctx) -> Palette {
        match self.colours {
            Colours::Theme => theme_palette(ctx),
            Colours::GithubDark => Palette::github_dark(),
            Colours::GithubLight => Palette::github_light(),
        }
    }

    /// Pictures a job read: each is the page's from the next layout, or known to be missing.
    pub fn arrived(&mut self, read: Read) {
        for (path, picture) in read {
            self.loading.remove(&path);
            match picture {
                Ok(picture) => self.pictures.insert(path, picture),
                Err(_) => self.pictures.fail(path),
            }
        }
        self.arrivals += 1;
    }
}

/// The faces a page's code blocks are coloured in, a role each in [`Syntax`]'s order, with the
/// base face each is on. Only a face's foreground is read; one the theme gives none is the
/// block's own text colour, so a theme that knows nothing of them draws code plain.
pub(crate) const CODE_FACES: [(&str, Option<&str>); 14] = [
    ("markdown-code-keyword", None),
    ("markdown-code-string", None),
    ("markdown-code-comment", Some("shadow")),
    ("markdown-code-number", None),
    ("markdown-code-function", None),
    ("markdown-code-type", None),
    ("markdown-code-constant", None),
    ("markdown-code-meta", None),
    ("markdown-code-variable", None),
    ("markdown-code-tag", None),
    ("markdown-code-attr", None),
    ("markdown-code-inserted", Some("success")),
    ("markdown-code-deleted", Some("error")),
    ("markdown-code-heading", Some("heading")),
];

/// The page's colours as the theme has them: the faces the view in place draws in, read for
/// their colours alone, since a page's weights and sizes are the sheet's. What a face doesn't
/// say is the nearest role's, and a theme with no colours of its own gets mono-dark's.
pub(crate) fn theme_palette(ctx: &Ctx) -> Palette {
    let rgb = |c: die_core::Rgb| Rgb(c.0, c.1, c.2);
    let style = |name: &str| ctx.face_style(name).unwrap_or_default();
    let fg = |name: &str| style(name).fg.map(rgb);
    let bg = |name: &str| style(name).bg.map(rgb);
    let (paper, dark) = (style("default"), Palette::mono_dark());
    if paper.fg.is_none() && paper.bg.is_none() {
        return dark;
    }
    let canvas = paper.bg.map_or(dark.canvas, rgb);
    let text = paper.fg.map_or(dark.text, rgb);
    let panel = bg(plan::CODE_BLOCK).or(bg(plan::CODE)).unwrap_or(canvas);
    // A quote's text is GitHub's muted colour, so the face that styles one says what muted is.
    // The quote face stands on no base face, so a theme that styles only those gives it the
    // quiet one's colour, as the other quiet roles have.
    let quote = fg(plan::QUOTE).or_else(|| fg("shadow")).unwrap_or(text);
    let muted = quote;
    let subtle = fg(plan::META).unwrap_or(muted);
    let border = fg(plan::RULE).unwrap_or(subtle);
    let strong = fg(plan::BOLD).unwrap_or(text);
    let code_block = fg(plan::CODE_BLOCK).unwrap_or(text);
    let [
        keyword,
        string,
        comment,
        number,
        function,
        type_,
        constant,
        meta,
        variable,
        tag,
        attr,
        inserted,
        deleted,
        heading,
    ] = CODE_FACES.map(|(name, _)| fg(name).unwrap_or(code_block));
    // A link the theme doesn't colour is underlined, or nothing would tell it from text.
    let link = fg(plan::LINK).unwrap_or(text);
    let underlined = (style(plan::LINK).underline).is_some_and(|u| u != Underline::None);
    let ansi = ctx.theme().ansi;
    let ink = |bar: usize, title: usize| AlertInk {
        bar: rgb(ansi[bar]),
        title: rgb(ansi[title]),
    };
    Palette {
        canvas,
        panel,
        pill: bg(plan::CODE).unwrap_or(panel),
        text,
        muted,
        subtle,
        border,
        hairline: border,
        headings: plan::H.map(|h| fg(h).unwrap_or(text)),
        strong,
        em: fg(plan::ITALIC).unwrap_or(text),
        del: fg(plan::STRIKE).unwrap_or(muted),
        link,
        link_underline: underlined || link == text,
        code: fg(plan::CODE).unwrap_or(text),
        code_block,
        quote,
        table_header: fg(plan::TABLE_HEADER).unwrap_or(strong),
        marker: fg(plan::MARKER).unwrap_or(muted),
        check: rgb(ansi[2]),
        // Note, tip, important, warning, caution: blue, green, magenta, yellow, red, the bar in
        // the colour and the title in its bright one.
        alerts: [ink(4, 12), ink(2, 10), ink(5, 13), ink(3, 11), ink(1, 9)],
        syntax: Syntax {
            keyword,
            string,
            comment,
            number,
            function,
            type_,
            constant,
            meta,
            variable,
            tag,
            attr,
            inserted,
            deleted,
            heading,
        },
    }
}

// ------------------------------------------------------------------ places

/// Where a page is asked to go.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    /// Its top.
    Top,
    /// The line showing this byte of the source at the window's top, or the page's top if
    /// that's the first line it has: a line a link names, and the place a restart kept.
    Line(usize),
    /// The line showing this byte of the source, with the window's top `offset` physical pixels
    /// under the line's: the reader's own place, to the pixel.
    Place {
        /// The byte.
        source: usize,
        /// The window's top, from the line's.
        offset: f32,
    },
    /// The heading with this anchor, a little under the window's top.
    Heading(String),
}

/// A place the reader left, for `C-o`: on this page or on the one a link led from. The
/// extension keeps them by file, so they outlive the page.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mark {
    /// The file's buffer.
    pub file: BufferId,
    /// Where on its page.
    pub place: Target,
    /// The text's revision the place's bytes are counted in.
    pub rev: u64,
}

impl Mark {
    /// The place in the text as it is now: its bytes carried through the edits since.
    pub fn now(&self, t: &TextBuf) -> Target {
        carry(t, self.rev, self.place.clone())
    }
}

/// What a window that leaves the page takes with it.
pub(crate) struct Left {
    /// The window.
    pub window: WindowId,
    /// The byte of the source the line at its top showed.
    pub source: Option<usize>,
    /// Its place, to the pixel.
    pub mark: Option<Mark>,
    /// Whether it was scrolled since the toggle showed the page there.
    pub moved: bool,
}

/// A target whose bytes were counted at revision `since`, in the text as it is now.
fn carry(t: &TextBuf, since: u64, to: Target) -> Target {
    match to {
        Target::Line(source) => Target::Line(carried(t, since, source)),
        Target::Place { source, offset } => Target::Place {
            source: carried(t, since, source),
            offset,
        },
        to => to,
    }
}

/// Where a byte of the text as it was at revision `since` is now. A byte in text that was
/// replaced is where the replacement starts, and one the log no longer reaches stays, held in
/// the text.
pub(crate) fn carried(t: &TextBuf, since: u64, pos: usize) -> usize {
    let Some(edits) = t.edits_since(since) else {
        return pos.min(t.len());
    };
    let through = |pos: usize, c: &die_core::Change| match pos {
        p if p >= c.old_end => p - c.old_end + c.new_end,
        p if p > c.start => c.start,
        p => p,
    };
    edits.fold(pos, through).min(t.len())
}

/// A target whose offset, in physical pixels, is for a scale `by` times this one's.
fn scaled(to: Target, by: f32) -> Target {
    match to {
        Target::Place { source, offset } => Target::Place {
            source,
            offset: offset * by,
        },
        to => to,
    }
}

/// Where an anchor is on the page: a heading's, a footnote's or a reference's, or None. A
/// heading's is tried as written and then in lower case, since slugs are lower case and people
/// write `#Installation` as often as `#installation`.
fn anchor_y(page: &Page, name: &str) -> Option<f32> {
    page.anchor(name)
        .or_else(|| page.anchor(&name.to_lowercase()))
}

/// The row of `page` a target is at, at `scale`, or why there's none.
fn aim(page: &Page, to: &Target, scale: f32) -> std::result::Result<f32, String> {
    // A line's top needn't be a whole pixel, and the window's top is: the first whole one in
    // the line, so the line at the top is this one, and not the foot of the one over it.
    let whole = |y: f32| (y - 0.01).ceil();
    Ok(match to {
        Target::Top => 0.0,
        Target::Line(source) if page.source_at(0.0) >= *source => 0.0,
        Target::Line(source) => whole(page.y_of(*source)),
        Target::Place { source, offset } => page.y_of(*source) + offset,
        Target::Heading(anchor) => {
            let y = anchor_y(page, anchor).ok_or_else(|| format!("No heading #{anchor}"))?;
            y - GAP * scale
        }
    })
}

// ------------------------------------------------------------------ one window's view

/// A layout, and what it was made from.
struct Laid {
    page: Rc<Page>,
    width: u32,
    scale: f32,
    palette: Palette,
    arrivals: u64,
    opened: Vec<usize>,
    /// The text's revision, which the places it gives count their bytes in.
    rev: u64,
}

/// One window's look at the page.
#[derive(Default)]
struct View {
    /// The window's area in physical pixels, from its latest `Resize`: none before the first.
    area: (u32, u32),
    producer: Option<PixelProducer>,
    laid: Option<Laid>,
    /// The page's row at the window's top, in physical pixels.
    top: i32,
    /// The same before rounding: where a layout put the place, to a fraction of a pixel.
    exact: f32,
    /// What the wheel has asked for that isn't a whole pixel yet.
    carry: f32,
    /// Where to go once there's a layout, and the revision its bytes are counted in.
    wait: Option<(Target, u64)>,
    /// The place the latest layout was aimed at, its revision and the scale of its offset,
    /// while nothing has scrolled since: the next layout aims there too. Worked out again from
    /// where a layout put the top, a place in a wrapped line would be the start of the line it
    /// fell in, and a round trip of widths would take the reader up a line each time.
    aimed: Option<(Target, u64, f32)>,
    /// The link the keyboard is on: its index into the page's links, and where it leads, which
    /// finds it again in the next layout.
    ring: Option<(usize, String)>,
    /// Where the link to ring once there's a layout leads.
    pending: Option<String>,
    /// The link the pointer's button went down on.
    pressed: Option<usize>,
    /// The picture isn't what the page, the place or the ring say it should be.
    stale: bool,
}

impl View {
    fn page(&self) -> Option<&Page> {
        self.laid.as_ref().map(|l| &*l.page)
    }

    /// The furthest the page scrolls: its last row at the window's bottom.
    fn limit(&self) -> i32 {
        let height = self.page().map_or(0.0, Page::height);
        (height.ceil() as i32 - self.area.1 as i32).max(0)
    }

    /// Where the reader is on the layout: the very top as the top, which stays the top whatever
    /// a new layout puts over the first line; anywhere else the line at the window's top and how
    /// far under its top the window's is.
    fn place(&self) -> Option<Target> {
        let page = self.page()?;
        if self.top == 0 {
            return Some(Target::Top);
        }
        let source = page.source_at(self.exact);
        Some(Target::Place {
            source,
            offset: self.exact - page.y_of(source),
        })
    }

    /// Where the reader is, and the revision its bytes count in: where the layout was aimed,
    /// if nothing has scrolled since; where it is on the layout; or where it's going.
    fn mark(&self) -> Option<(Target, u64)> {
        if let Some((to, rev, _)) = &self.aimed {
            return Some((to.clone(), *rev));
        }
        match (self.place(), &self.laid, &self.wait) {
            (Some(place), Some(laid), _) => Some((place, laid.rev)),
            (None, _, Some(wait)) => Some(wait.clone()),
            _ => None,
        }
    }

    /// The byte of the source the line at the window's top shows, or the one it's going to.
    fn source_at_top(&self) -> Option<usize> {
        match (self.page(), &self.wait) {
            (Some(page), _) => Some(page.source_at(self.top as f32)),
            (None, Some((Target::Line(source) | Target::Place { source, .. }, _))) => Some(*source),
            _ => None,
        }
    }

    /// Scroll so the page's row `top` is the window's first, held between the page's ends.
    /// True if that moved it.
    fn scroll_to(&mut self, top: f32) -> bool {
        let top = (top.round() as i32).clamp(0, self.limit());
        if top == self.top {
            return false;
        }
        (self.top, self.exact, self.stale, self.aimed) = (top, top as f32, true, None);
        true
    }

    /// The page's end doesn't come up past the window's bottom: a taller window, a shorter page.
    fn hold(&mut self) {
        let limit = self.limit();
        if self.top > limit {
            (self.top, self.exact, self.stale) = (limit, limit as f32, true);
        }
    }

    fn ring(&self) -> Option<usize> {
        self.ring.as_ref().map(|r| r.0)
    }

    fn ringed(&self) -> Option<&LinkBox> {
        self.page()?.links().get(self.ring()?)
    }

    /// Where the ringed link leads, with or without a layout to find it in.
    fn ring_dest(&self) -> Option<String> {
        let ringed = self.ring.as_ref().map(|r| r.1.clone());
        ringed.or_else(|| self.pending.clone())
    }

    /// The link under the pointer.
    fn link_under(&self, p: &SurfacePointer) -> Option<usize> {
        self.page()?.link_at(p.x, p.y + self.top as f32)
    }

    /// Where `]]` and `[[` go: the `count`th heading on from the window's top, or back from it.
    fn heading(&self, scale: f32, forward: bool, count: usize) -> Option<f32> {
        let page = self.page()?;
        let gap = GAP * scale;
        let mut top = self.top as f32;
        // At the page's top, a title in the window's upper half is where the reader is, though
        // the page can't go up to put it where a heading goes: the next one is the one after.
        let half = self.area.1 as f32 / 2.0;
        if let (true, 0, Some(first)) = (forward, self.top, page.headings().first())
            && first.y < half
        {
            top = top.max(first.y - gap);
        }
        for _ in 0..count {
            // By a pixel, since where the last one was scrolled to is a whole number of them.
            let tops = page.headings().iter().map(|h| h.y - gap);
            let next = match forward {
                true => tops.filter(|y| *y > top + 1.0).reduce(f32::min),
                false => tops.filter(|y| *y < top - 1.0).reduce(f32::max),
            };
            let Some(next) = next else { break };
            top = next;
        }
        Some(top)
    }

    /// Which link `<Tab>` or `<S-Tab>` rings, and where the page goes to show it, if anywhere.
    /// With none ringed in view it starts from the first one the window shows, or the last,
    /// going back; past the last link is the first, so a count is taken round the links, not
    /// walked.
    fn next_link(
        &self,
        scale: f32,
        forward: bool,
        count: usize,
    ) -> Option<Result<(usize, Option<f32>)>> {
        let links = self.page()?.links();
        if links.is_empty() {
            return Some(Err(Error::Msg("No links on this page".into())));
        }
        let height = self.area.1 as f32;
        let (top, bottom) = (self.top as f32, self.top as f32 + height);
        let first = |l: &LinkBox| l.rects.first().copied();
        let seen = |l: &LinkBox| first(l).is_some_and(|r| r.y + r.h > top && r.y < bottom);
        let under = |l: &LinkBox| first(l).is_some_and(|r| r.y + r.h > top);
        let over = |l: &LinkBox| first(l).is_some_and(|r| r.y < bottom);
        let n = links.len();
        let from = self.ring().filter(|i| links.get(*i).is_some_and(seen));
        // From a ringed link every step is a move; from none the first step picks where to
        // start, and the rest move.
        let (start, steps) = match (from, forward) {
            (Some(i), _) => (i, count.max(1)),
            (None, true) => (links.iter().position(under).unwrap_or(0), count.max(1) - 1),
            (None, false) => (
                links.iter().rposition(over).unwrap_or(n - 1),
                count.max(1) - 1,
            ),
        };
        let at = match forward {
            true => (start + steps % n) % n,
            false => (start + n - steps % n) % n,
        };
        let gap = GAP * scale;
        let to = first(&links[at]).and_then(|r| match r {
            r if r.y < top => Some(r.y - gap),
            r if r.y + r.h > bottom => Some(r.y + r.h + gap - height),
            _ => None,
        });
        Some(Ok((at, to)))
    }

    /// Paint the part of the page the window shows, and present its area. The buffer may be
    /// larger than the area, having grown for a larger window once, and the rest of it is out
    /// of sight.
    fn paint(&mut self, kit: &SharedKit) {
        let (width, height) = self.area;
        let top = self.top;
        let (Some(laid), Some(producer)) = (&self.laid, &mut self.producer) else {
            return;
        };
        // An index the layout hasn't got never reaches the typesetter.
        let focus = self.ring.as_ref().map(|r| r.0);
        let marks = Marks {
            focus: focus.filter(|i| *i < laid.page.links().len()),
        };
        let mut kit = kit.borrow_mut();
        let Kit {
            fonts: Some(fonts),
            pictures,
            ..
        } = &mut *kit
        else {
            return;
        };
        let (wide, tall) = producer.size();
        let (width, height) = (width.min(wide), height.min(tall));
        let out = producer.pixels_mut();
        (laid.page).paint(fonts, pictures, out, wide, tall, top, 0..height, &marks);
        producer.present(PixelRect::new(0, 0, width, height));
        self.stale = false;
    }
}

// ------------------------------------------------------------------ one page

/// One file's page.
pub(crate) struct State {
    /// The file's buffer, whose text the page shows.
    pub file: BufferId,
    /// The page's own buffer, once it's made.
    pub buf: Option<BufferId>,
    /// The page's buffer's name, for the status line.
    pub title: String,
    /// The file's name and directory the page's were taken from, which follow the file's.
    pub named: (String, Option<PathBuf>),
    kit: SharedKit,
    /// What the latest `Resize` said a cell is in pixels: the scale, and the wheel's unit.
    cell: CellPixels,
    /// Each window showing the page, with its view.
    views: BTreeMap<WindowId, View>,
    /// The window the reader was in last: the place a restart keeps and a window split off it
    /// starts at, and the one a test or the session asks about when it names none.
    last: Option<WindowId>,
    /// The window the surface was drawn for last, whose status line core asks for next.
    drawing: Option<WindowId>,
    /// Where the first window to show the page goes, and the revision its bytes are counted in:
    /// the place a page put away or a restart kept.
    seed: Option<(Target, u64)>,
    /// Where the link the first window rings leads: the one ringed when the page was put away.
    pending: Option<String>,
    /// The `<details>` the reader has toggled, each by the byte offset of its tag, in order.
    opened: Vec<usize>,
    /// The text's revision those offsets are counted in: they move with the file's edits.
    opened_rev: u64,
    /// The windows the toggle showed the page in, where nothing has scrolled it since: the
    /// toggle back leaves their cursors alone.
    pub unmoved: Vec<WindowId>,
    /// A pixel buffer was refused, which is said once.
    refused: bool,
    /// The host shows no pixels after all, and the view in place is on its way.
    leaving: bool,
}

impl State {
    /// A page of `file`, whose text is at revision `rev`, with nothing drawn yet.
    pub fn new(file: BufferId, rev: u64, kit: SharedKit) -> State {
        State {
            file,
            buf: None,
            title: String::new(),
            named: (String::new(), None),
            kit,
            cell: CellPixels::default(),
            views: BTreeMap::new(),
            last: None,
            drawing: None,
            seed: None,
            pending: None,
            opened: vec![],
            opened_rev: rev,
            unmoved: vec![],
            refused: false,
            leaving: false,
        }
    }

    /// Physical pixels to a CSS pixel. A host that hasn't said counts as 1.
    fn scale(&self) -> f32 {
        match self.cell.scale {
            s if s > 0.0 => s as f32,
            _ => 1.0,
        }
    }

    /// The window whose view answers for `w`: its own, if it shows the page, or else the one
    /// the reader was in last, or any.
    fn here(&self, w: Option<WindowId>) -> Option<WindowId> {
        let has = |w: &WindowId| self.views.contains_key(w);
        let any = || self.views.keys().next().copied();
        w.filter(has).or(self.last.filter(has)).or_else(any)
    }

    fn view(&self, w: Option<WindowId>) -> Option<&View> {
        self.views.get(&self.here(w)?)
    }

    /// The view of the window a key was typed in, which is the one the reader is in now.
    fn acting(&mut self, w: Option<WindowId>) -> Option<(WindowId, &mut View)> {
        let w = self.here(w)?;
        self.last = Some(w);
        Some((w, self.views.get_mut(&w)?))
    }

    /// Scroll the view of the window a key was typed in to the row `to` works out from it.
    fn scroll(&mut self, w: Option<WindowId>, to: impl FnOnce(&View, f32) -> Option<f32>) {
        let scale = self.scale();
        let Some((w, view)) = self.acting(w) else {
            return;
        };
        if let Some(top) = to(view, scale)
            && view.scroll_to(top)
        {
            self.unmoved.retain(|u| *u != w);
        }
    }

    /// The area a window's view covers, in physical pixels.
    pub fn area(&self, w: Option<WindowId>) -> (u32, u32) {
        self.view(w).map_or((0, 0), |v| v.area)
    }

    /// A window's layout, once there is one.
    pub fn page(&self, w: Option<WindowId>) -> Option<&Page> {
        self.view(w)?.page()
    }

    /// A window's pixel buffer, once there is one.
    pub fn pixels(&self, w: Option<WindowId>) -> Option<PixelBufferId> {
        self.view(w)?.producer.as_ref().map(PixelProducer::id)
    }

    /// The page's row at a window's top.
    pub fn top(&self, w: Option<WindowId>) -> i32 {
        self.view(w).map_or(0, |v| v.top)
    }

    /// The link the keyboard is on in a window, as an index into its page's links.
    pub fn ring(&self, w: Option<WindowId>) -> Option<usize> {
        self.view(w)?.ring()
    }

    /// The link the keyboard is on in a window.
    pub fn ringed(&self, w: Option<WindowId>) -> Option<&LinkBox> {
        self.view(w)?.ringed()
    }

    /// Where the ringed link of the window the reader was in last leads, with or without a
    /// layout to find it in.
    pub fn ring_dest(&self) -> Option<String> {
        let view = self.view(None).and_then(View::ring_dest);
        view.or_else(|| self.pending.clone())
    }

    /// Ring the link that leads here, once the first window to show the page has a layout: the
    /// ring a page put away had. None rings nothing, as for a page a link arrives at.
    pub fn ring_later(&mut self, dest: Option<String>) {
        self.pending = dest;
    }

    /// Where the reader is in a window, for `C-o`: on its layout, or where it's going once it
    /// has one.
    pub fn mark(&self, w: Option<WindowId>) -> Option<Mark> {
        let place = self.view(w).and_then(View::mark);
        let (place, rev) = place.or_else(|| self.seed.clone())?;
        Some(Mark {
            file: self.file,
            place,
            rev,
        })
    }

    /// The place a restart keeps: the byte of the source the line at the top of the window the
    /// reader was in last shows.
    pub fn kept(&self) -> usize {
        let seed = || match &self.seed {
            Some((Target::Line(source) | Target::Place { source, .. }, _)) => Some(*source),
            _ => None,
        };
        let top = self.view(None).and_then(View::source_at_top);
        top.or_else(seed).unwrap_or(0)
    }

    /// What a window takes with it when it leaves the page, if it shows it.
    pub fn left(&self, w: WindowId) -> Option<Left> {
        let view = self.views.get(&w)?;
        let mark = view.mark().map(|(place, rev)| Mark {
            file: self.file,
            place,
            rev,
        });
        Some(Left {
            window: w,
            source: view.source_at_top(),
            mark,
            moved: !self.unmoved.contains(&w),
        })
    }

    /// The windows that had the page and show something else now: what each took with it.
    pub fn departed(&self, ctx: &Ctx) -> Vec<Left> {
        let gone = self
            .views
            .keys()
            .filter(|w| ctx.window_buffer(**w) != self.buf);
        gone.filter_map(|w| self.left(*w)).collect()
    }

    /// A window has left the page by the toggle: its view goes, its pixel buffer with it.
    pub fn forget(&mut self, w: Option<WindowId>) {
        if let Some(w) = w {
            self.views.remove(&w);
        }
        if self.last.is_some_and(|w| !self.views.contains_key(&w)) {
            self.last = None;
        }
    }

    /// The windows that have left the page are forgotten, their pixel buffers with them.
    pub fn prune(&mut self, ctx: &Ctx) {
        let buf = self.buf;
        self.views.retain(|w, _| ctx.window_buffer(*w) == buf);
        if self.last.is_some_and(|w| !self.views.contains_key(&w)) {
            self.last = None;
        }
    }

    /// Go to a place in a window: now if it has a layout to find it in, and otherwise when its
    /// next one is made, as for a window that has yet to hear its first `Resize`. With no
    /// window, the place is where the first window to show the page goes.
    pub fn go(&mut self, ctx: &mut Ctx, w: Option<WindowId>, to: Target) {
        let rev = ctx.text(self.file).map_or(self.opened_rev, TextBuf::rev);
        let Some(w) = w else {
            self.seed = Some((to, rev));
            return;
        };
        let scale = self.scale();
        // A window new to the page goes here, and not where the page was put away; it rings
        // what the page was to ring, unless that was taken off.
        let view = self.views.entry(w).or_insert_with(|| {
            self.seed = None;
            View {
                pending: self.pending.take(),
                ..View::default()
            }
        });
        self.last = Some(w);
        match view.page().map(|page| aim(page, &to, scale)) {
            Some(Ok(top)) => {
                if view.scroll_to(top) {
                    self.unmoved.retain(|u| *u != w);
                }
            }
            Some(Err(why)) => ctx.messages.push(why),
            None => view.wait = Some((to, rev)),
        }
    }

    /// Go to the heading an anchor names in a window, or say there's none.
    pub fn anchor(&mut self, w: Option<WindowId>, anchor: &str) -> Result {
        let scale = self.scale();
        let page = self.page(w);
        let page = page.ok_or_else(|| Error::Msg("No page yet".into()))?;
        let top = aim(page, &Target::Heading(anchor.into()), scale).map_err(Error::Msg)?;
        self.scroll(w, |_, _| Some(top));
        Ok(())
    }

    /// `j` and `k`: so many steps down, or up if it's negative.
    pub fn step(&mut self, w: Option<WindowId>, steps: f32) {
        self.scroll(w, |v, scale| Some(v.top as f32 + steps * STEP * scale));
    }

    /// `C-d`, `C-f` and their opposites: so much of the window down, or up if it's negative.
    pub fn windows(&mut self, w: Option<WindowId>, part: f32) {
        self.scroll(w, |v, _| Some(v.top as f32 + part * v.area.1 as f32));
    }

    /// `gg` and `G`: the page's top, or its bottom at the window's.
    pub fn end(&mut self, w: Option<WindowId>, bottom: bool) {
        self.scroll(w, |v, _| Some(if bottom { v.limit() as f32 } else { 0.0 }));
    }

    /// `]]` and `[[`: the `count`th heading on from the window's top, or back from it.
    pub fn heading(&mut self, w: Option<WindowId>, forward: bool, count: usize) {
        self.scroll(w, |v, scale| v.heading(scale, forward, count));
    }

    /// `<Tab>` and `<S-Tab>`: ring the `count`th link on, or back, in the window they're typed
    /// in, and scroll it into that window's view.
    pub fn link(&mut self, w: Option<WindowId>, forward: bool, count: usize) -> Result {
        let scale = self.scale();
        let next = self
            .view(w)
            .and_then(|v| v.next_link(scale, forward, count));
        let Some((at, to)) = next.transpose()? else {
            return Ok(());
        };
        if let Some((_, view)) = self.acting(w) {
            let dest = view.page().map(|p| p.links()[at].dest.clone());
            (view.ring, view.stale) = (dest.map(|d| (at, d)), true);
        }
        if let Some(to) = to {
            self.scroll(w, |_, _| Some(to));
        }
        Ok(())
    }

    /// `<Esc>`, and a click off a link: no link is ringed in the window.
    pub fn unring(&mut self, w: Option<WindowId>) {
        if let Some((_, view)) = self.acting(w)
            && view.ring.take().is_some()
        {
            view.stale = true;
        }
    }

    /// Open the closed `<details>` an anchor's place is in, outermost first, as a browser opens
    /// the ones a link leads into; whether there were any. The next refresh shows the place.
    pub fn unfold(&mut self, w: Option<WindowId>, anchor: &str) -> bool {
        let lower = anchor.to_lowercase();
        let blocks = self
            .page(w)
            .and_then(|p| p.folded(anchor).or_else(|| p.folded(&lower)));
        let blocks = blocks.map(<[usize]>::to_vec).unwrap_or_default();
        for &offset in &blocks {
            self.toggle_details(offset);
        }
        !blocks.is_empty()
    }

    /// A `<details>` was followed: open if it was closed, closed if it was open. The next
    /// refresh lays the page out again with each window's place kept.
    pub fn toggle_details(&mut self, offset: usize) {
        match self.opened.binary_search(&offset) {
            Ok(i) => {
                self.opened.remove(i);
            }
            Err(i) => self.opened.insert(i, offset),
        }
    }

    /// The `<details>` the reader has toggled, and the text's revision their offsets count in:
    /// what the file keeps of them when the page goes.
    pub fn opened(&self) -> (Vec<usize>, u64) {
        (self.opened.clone(), self.opened_rev)
    }

    /// The `<details>` a page of this file had toggled, counted at revision `rev`: the next
    /// refresh carries them through the edits since.
    pub fn reopen(&mut self, opened: Vec<usize>, rev: u64) {
        (self.opened, self.opened_rev) = (opened, rev);
    }

    /// The status line: the page's name, how far down it is in the window as Vim's ruler says
    /// it, and where the ringed link leads, which a page has no other way to show.
    fn status(&self) -> String {
        let view = self.view(self.drawing);
        let at = match view {
            Some(v) if v.laid.is_some() => match (v.top, v.limit()) {
                (_, 0) => "All".into(),
                (0, _) => "Top".into(),
                (top, limit) if top >= limit => "Bot".into(),
                (top, limit) => format!("{}%", i64::from(top) * 100 / i64::from(limit)),
            },
            _ => String::new(),
        };
        // A summary leads nowhere: its address is the typesetter's name for the block.
        let link = view
            .and_then(View::ringed)
            .map(|l| match l.dest.starts_with("details:") {
                true => "  opens or closes the details".to_string(),
                false => format!("  {}", l.dest),
            });
        format!("{}  {at}{}", self.title, link.unwrap_or_default())
    }

    // -------------------------------------------------------------- layout and paint

    /// Bring the pictures up to date: lay a window's view out again if the text, its width,
    /// the scale, the colours, the pictures or the opened details have changed since its
    /// layout was made, and paint what is owed. There's no theme hook, so the colours are read
    /// and compared each time. A window with no area or no pixel buffer yet gets nothing.
    pub fn refresh(&mut self, ctx: &mut Ctx) {
        let Ok(t) = ctx.text(self.file) else {
            return;
        };
        let rev = t.rev();
        if rev != self.opened_rev {
            let through = |o: &usize| carried(t, self.opened_rev, *o);
            let mut opened: Vec<usize> = self.opened.iter().map(through).collect();
            opened.sort_unstable();
            opened.dedup();
            (self.opened, self.opened_rev) = (opened, rev);
        }
        let windows: Vec<WindowId> = (self.views.iter())
            .filter(|(w, v)| v.producer.is_some() && ctx.window_buffer(**w) == self.buf)
            .map(|(w, _)| *w)
            .collect();
        if windows.is_empty() {
            return;
        }
        let scale = self.scale();
        let (palette, arrivals) = {
            let kit = self.kit.borrow();
            (kit.palette(ctx), kit.arrivals)
        };
        for w in windows {
            let (behind, shared) = {
                let width = self.views[&w].area.0;
                let current = |l: &Laid| {
                    (l.width, l.scale, l.arrivals, l.rev) == (width, scale, arrivals, rev)
                        && l.palette == palette
                        && l.opened == self.opened
                };
                let behind = !self.views[&w].laid.as_ref().is_some_and(current);
                // Another window of this width may have its layout already.
                let laid = self.views.values().filter_map(|v| v.laid.as_ref());
                (
                    behind,
                    laid.filter(|l| current(l)).map(|l| l.page.clone()).next(),
                )
            };
            if behind {
                self.lay_out(ctx, w, &palette, shared);
            }
            if let Some(view) = self.views.get_mut(&w)
                && view.stale
            {
                view.paint(&self.kit);
            }
        }
    }

    /// Lay the text out as it is now for a window's width, or take the layout another window
    /// of that width has, and keep the window's place: the line at the window's top stays
    /// there, to the pixel, wherever the new layout puts it.
    fn lay_out(&mut self, ctx: &mut Ctx, w: WindowId, palette: &Palette, shared: Option<Rc<Page>>) {
        let scale = self.scale();
        if shared.is_none() && self.kit.borrow().fonts.is_none() {
            return;
        }
        let dir = ctx.directory(self.file).map(Path::to_path_buf);
        let Some(view) = self.views.get_mut(&w) else {
            return;
        };
        let Ok(t) = ctx.text(self.file) else { return };
        let rev = t.rev();
        // The place kept counts bytes of the text the last layout was made for, and its offset
        // is in that layout's pixels.
        let keep = match (view.wait.take(), view.aimed.take()) {
            (Some((to, since)), _) => Some(carry(t, since, to)),
            (None, Some((to, since, was))) => Some(scaled(carry(t, since, to), scale / was)),
            (None, None) => view
                .laid
                .as_ref()
                .and_then(|l| Some(scaled(carry(t, l.rev, view.place()?), scale / l.scale))),
        };
        let width = view.area.0;
        let mut wanted: Vec<PathBuf> = vec![];
        let page = match shared {
            Some(page) => page,
            None => {
                let mut kit = self.kit.borrow_mut();
                let Kit {
                    fonts: Some(fonts),
                    pictures,
                    loading,
                    front_matter,
                    ..
                } = &mut *kit
                else {
                    return;
                };
                // The one place a layout is asked for.
                let env = Env {
                    width,
                    scale,
                    palette,
                    dir: dir.as_deref(),
                    front_matter: *front_matter,
                    tab_width: t.settings().tab_width,
                    opened: &self.opened,
                };
                let page = Page::new(&t.rope().to_string(), &env, fonts, pictures);
                // A picture is asked for once: answered, or with a job already reading it, it
                // isn't.
                let asked = |p: &&PathBuf| !pictures.answered(p) && loading.insert((*p).clone());
                wanted = page.wants().iter().filter(asked).cloned().collect();
                Rc::new(page)
            }
        };
        let arrivals = self.kit.borrow().arrivals;

        let aimed = keep.as_ref().map(|to| aim(&page, to, scale));
        let top = match &aimed {
            Some(Ok(top)) => *top,
            _ => 0.0,
        };
        // The same link stays ringed: where it was among the links if it still leads there,
        // or wherever it has moved to; and a page brought back rings the link it had.
        let ringed = view.ring.take().map(|(i, dest)| (Some(i), dest));
        let ringed = ringed.or_else(|| view.pending.take().map(|dest| (None, dest)));
        view.ring = ringed.and_then(|(i, dest)| {
            let links = page.links();
            let same = |l: &LinkBox| l.dest == dest;
            let kept = i.filter(|i| links.get(*i).is_some_and(same));
            Some((kept.or_else(|| links.iter().position(same))?, dest))
        });
        // The next layout aims where this one did, until something scrolls; a heading's place
        // is where it put the window, since the heading itself may go; and the very top is the
        // top, whatever is typed over its first line.
        let heading = matches!(keep, Some(Target::Heading(_)));
        view.aimed = match (keep, &aimed) {
            (Some(_), Some(Ok(top))) if *top < 0.5 => Some((Target::Top, rev, scale)),
            (Some(to), Some(Ok(_))) if !heading => Some((to, rev, scale)),
            _ => None,
        };
        view.laid = Some(Laid {
            page,
            width,
            scale,
            palette: palette.clone(),
            arrivals,
            opened: self.opened.clone(),
            rev,
        });
        view.exact = top.clamp(0.0, view.limit() as f32);
        view.top = view.exact.round() as i32;
        view.stale = true;
        if heading && matches!(aimed, Some(Ok(_))) {
            view.aimed = view.place().map(|to| (to, rev, scale));
        }
        if let Some(Err(why)) = aimed {
            ctx.messages.push(why);
        }
        if !wanted.is_empty() {
            let work = move || -> Read {
                let read = |path: PathBuf| {
                    let picture = Picture::load(&path);
                    (path, picture)
                };
                wanted.into_iter().map(read).collect()
            };
            let done = |ctx: &mut Ctx, read: Read| {
                Ok(ctx.with_ext(|m: &mut Markdown, ctx| m.pictures(ctx, read))?)
            };
            ctx.spawn(work, done).detach();
        }
    }

    // -------------------------------------------------------------- events

    /// A view for a window new to the page: where the page was asked to go before any window
    /// showed it, or else where the window the reader was in last is, as a window split off
    /// another starts where that one was, ringing what it rings.
    fn start(&mut self) -> View {
        let mut view = View::default();
        match (self.seed.take(), self.view(None)) {
            (Some(seed), _) => (view.wait, view.pending) = (Some(seed), self.pending.take()),
            (None, Some(last)) => (view.wait, view.pending) = (last.mark(), last.ring_dest()),
            (None, None) => view.pending = self.pending.take(),
        }
        view
    }

    /// A window showing the page has an area this size. Its pixel buffer is made the first
    /// time the cells have a size in pixels, and grown when the area outgrows it.
    fn resized(
        &mut self,
        ctx: &mut Ctx,
        window: WindowId,
        cols: usize,
        rows: usize,
        cell: CellPixels,
    ) {
        let (width, height) = (cols as u32 * cell.width, rows as u32 * cell.height);
        // The host hasn't said what a cell is yet, and its next `Resize` will; or it shows no
        // pixels, and the view in place is on its way.
        if width == 0 || height == 0 || self.leaving || ctx.window_buffer(window) != self.buf {
            return;
        }
        // A new scale lays every window out again, at the next refresh.
        self.cell = cell;
        if !self.views.contains_key(&window) {
            let view = self.start();
            self.views.insert(window, view);
            self.last.get_or_insert(window);
        }
        let Some(view) = self.views.get_mut(&window) else {
            return;
        };
        // A taller area shows rows of the buffer that were last painted for another place, and
        // the page's end may have come up past the window's bottom.
        view.stale |= view.area != (width, height);
        view.area = (width, height);
        view.hold();
        let Some(producer) = &mut view.producer else {
            // A page a restart remade was made on your option alone, before the host could say
            // whether it shows pixels: this is where it says, and where the fonts are read.
            if !(ctx.pixels_supported() && self.kit.borrow_mut().has_fonts(ctx)) {
                self.leaving = true;
                let file = self.file;
                ctx.defer(move |ctx| {
                    ctx.with_ext(|m: &mut Markdown, ctx| m.in_place_instead(ctx, file))?
                });
                return;
            }
            match ctx.pixel_buffer(width, height) {
                Ok((_, producer)) => (view.producer, view.stale) = (Some(producer), true),
                Err(e) if !self.refused => {
                    ctx.messages
                        .push(format!("markdown: no pixel buffer for the page: {e}"));
                    self.refused = true;
                }
                Err(_) => {}
            }
            return;
        };
        let (wide, tall) = producer.size();
        let want = (width.max(wide), height.max(tall));
        if want != (wide, tall) && producer.resize(want.0, want.1).is_ok() {
            view.stale = true;
        }
    }

    /// The wheel scrolls the window it's over by pixels, and a click on a link follows it.
    fn pointer(&mut self, ctx: &mut Ctx, p: &SurfacePointer) {
        let (w, cell) = (Some(p.window), self.cell.height as f32);
        let Some((_, view)) = self.acting(w).filter(|(at, _)| *at == p.window) else {
            return;
        };
        match p.kind {
            PointerKind::Scroll { dy, .. } if dy.is_finite() => {
                // In cells, and a trackpad's are fractions of one: what isn't a whole pixel
                // waits for the next. A delta past any number goes to the end and carries
                // nothing, since infinity has no fraction.
                let by = dy * cell + view.carry;
                view.carry = if by.is_finite() { by.fract() } else { 0.0 };
                let to = view.top as f32 + by.trunc();
                self.scroll(w, |_, _| Some(to));
            }
            PointerKind::Press(Button::Left) => {
                view.pressed = view.link_under(p);
                if view.pressed.is_none() {
                    self.unring(w);
                }
            }
            PointerKind::Release(Button::Left) => {
                // A click is a press and a release on one link.
                let (over, pressed) = (view.link_under(p), view.pressed.take());
                if over.is_none() || over != pressed {
                    return;
                }
                let dest = over.and_then(|i| view.page()?.links().get(i).map(|l| l.dest.clone()));
                (view.ring, view.stale) = (over.zip(dest), true);
                // From the top of the loop: where it leads may put another buffer in this
                // window, and this surface is checked out until the event is over.
                let args = Args {
                    window: Some(p.window),
                    buffer: self.buf,
                    ..Args::default()
                };
                ctx.defer(move |ctx| ctx.run("markdown-follow-link", &args));
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ the surface

/// The page's buffer: a surface that shows a window's pixel buffer over its whole area and
/// writes no cells. It gives no text, so there's no copy state: core would draw that cursor in
/// cells, a line to a row, over letters that aren't in cells.
pub(crate) struct PageSurface {
    state: Shared,
}

impl PageSurface {
    /// The surface of a page.
    pub fn new(state: Shared) -> PageSurface {
        PageSurface { state }
    }
}

impl Surface for PageSurface {
    fn kind(&self) -> &'static str {
        crate::PAGE
    }

    /// Once for each window showing the page, which places its own pixel buffer, at every
    /// render, since a frame keeps its cells and not its placements. The cells stay blank:
    /// where the buffer hasn't been presented yet, the theme's background shows. Core asks for
    /// the window's status line next, which is this window's.
    fn render(&mut self, frame: &mut Frame, info: &RenderInfo) {
        let mut guard = self.state.borrow_mut();
        let st = &mut *guard;
        st.drawing = Some(info.window);
        let Some(view) = st.views.get_mut(&info.window) else {
            return;
        };
        if info.focused {
            st.last = Some(info.window);
        }
        // Whoever moves the page paints it in the same turn. This catches what was left owed, a
        // frame late, since the present asks for the frame after this one.
        if view.stale {
            view.paint(&st.kit);
        }
        if let Some(producer) = &view.producer {
            frame.place(producer.id(), -1);
        }
    }

    fn status(&self) -> Option<String> {
        Some(self.state.borrow().status())
    }

    /// A size, the wheel and a click. Keys are the keymap's: one it doesn't bind is undefined
    /// here, as on any surface. A window that leaves the page for another buffer sends nothing
    /// here; the command's end notices.
    fn event(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        let mut st = self.state.borrow_mut();
        let taken = match ev {
            Event::Resize {
                window,
                cols,
                rows,
                cell,
            } => {
                st.resized(ctx, *window, *cols, *rows, *cell);
                true
            }
            Event::Pointer(p) => {
                st.pointer(ctx, p);
                true
            }
            _ => false,
        };
        st.refresh(ctx);
        taken
    }
}
