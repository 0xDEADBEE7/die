//! One file's page: the rendered view as a picture, in a buffer of its own that the file's
//! window shows. The typesetter ([`crate::typeset`]) lays the text out and paints it; this is the
//! editor's side of it: the pixel buffer the window shows, where the reader is, the link the
//! keyboard is on, and when to lay out and paint again.
//!
//! A placement shows a pixel buffer from its top-left corner, one pixel to a pixel, and can't be
//! moved (contract §11), so the buffer is the window and not the page: a scroll paints the part
//! of the page the window shows, and presents it. The buffer is made at the first `Resize` that
//! says what a cell is in pixels, and grows with the area but never shrinks, since a resize
//! blanks the picture until the next present. Two windows on one page have one picture between
//! them, as large as the largest of them each way: the smaller shows its top-left part.
//!
//! The state is shared with the extension, as a board's is: a command reaches it without checking
//! the surface out, `save_session` reads it with no `Ctx`, and the surface's own events find it
//! there. Whoever changes it ends with [`State::refresh`], which lays out again if the text, the
//! width, the scale, the colours, the pictures or the opened details are no longer what the
//! layout was made from, and paints what is owed, so the present rides that turn's frame.
use crate::typeset::{AlertInk, Env, Fonts, LinkBox, Marks, Page, Palette, Picture, Pictures, Rgb};
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
        link: fg(plan::LINK).unwrap_or(text),
        link_underline: (style(plan::LINK).underline).is_some_and(|u| u != Underline::None),
        code: fg(plan::CODE).unwrap_or(text),
        code_block: fg(plan::CODE_BLOCK).unwrap_or(text),
        quote,
        table_header: fg(plan::TABLE_HEADER).unwrap_or(strong),
        marker: fg(plan::MARKER).unwrap_or(muted),
        check: rgb(ansi[2]),
        // Note, tip, important, warning, caution: blue, green, magenta, yellow, red, the bar in
        // the colour and the title in its bright one.
        alerts: [ink(4, 12), ink(2, 10), ink(5, 13), ink(3, 11), ink(1, 9)],
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
fn carried(t: &TextBuf, since: u64, pos: usize) -> usize {
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

/// Where an anchor is on the page: the top of the heading whose slug it is, or None. The
/// engine's `Page::anchor`, which knows a footnote's `fn-` and `fnref-` anchors too, takes this
/// over when it arrives.
fn anchor_y(page: &Page, name: &str) -> Option<f32> {
    let slug = name.to_lowercase();
    page.headings().iter().find(|h| h.slug == slug).map(|h| h.y)
}

// ------------------------------------------------------------------ one page

/// A layout, and what it was made from.
struct Laid {
    page: Page,
    width: u32,
    scale: f32,
    palette: Palette,
    arrivals: u64,
    opened: Vec<usize>,
}

/// One file's page.
pub(crate) struct State {
    /// The file's buffer, whose text the page shows.
    pub file: BufferId,
    /// The page's own buffer, once it's made.
    pub buf: Option<BufferId>,
    /// The page's buffer's name, for the status line.
    pub title: String,
    kit: SharedKit,
    /// What the latest `Resize` said a cell is in pixels: the scale, and the wheel's unit.
    cell: CellPixels,
    /// The windows showing the page, each with its area in physical pixels. The picture is as
    /// wide as the widest and as tall as the tallest, so none of them shows stale rows.
    areas: BTreeMap<WindowId, (u32, u32)>,
    laid: Option<Laid>,
    /// The text's revision the layout was made for, which the places kept count their bytes in.
    rev: u64,
    /// The page's row at the window's top, in physical pixels.
    top: i32,
    /// The same before rounding. A new layout puts the kept place at a fraction of a pixel, and
    /// the place is measured from here, so a round trip between two layouts comes back to the
    /// pixel it left instead of creeping one each time.
    exact: f32,
    /// What the wheel has asked for that isn't a whole pixel yet.
    carry: f32,
    /// Where to go once there's a layout, and the revision its bytes are counted in.
    wait: Option<(Target, u64)>,
    /// The link the keyboard is on: its index into the page's links, and where it leads, which
    /// finds it again in the next layout.
    ring: Option<(usize, String)>,
    /// The link the pointer's button went down on.
    pressed: Option<usize>,
    /// Where to put the ring once there's a layout: the link that was ringed when the page was
    /// put away.
    pending: Option<String>,
    /// The `<details>` the reader has opened, each by the byte offset the engine names it by,
    /// in order.
    opened: Vec<usize>,
    /// The windows the toggle showed the page in, where nothing has scrolled it since: the
    /// toggle back leaves their cursors alone.
    pub unmoved: Vec<WindowId>,
    producer: Option<PixelProducer>,
    /// The picture isn't what the page, the place or the ring say it should be.
    stale: bool,
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
            kit,
            cell: CellPixels::default(),
            areas: BTreeMap::new(),
            laid: None,
            rev,
            top: 0,
            exact: 0.0,
            carry: 0.0,
            wait: None,
            ring: None,
            pressed: None,
            pending: None,
            opened: vec![],
            unmoved: vec![],
            producer: None,
            stale: false,
            refused: false,
            leaving: false,
        }
    }

    /// The area the picture covers, in physical pixels: the widest and the tallest of the
    /// windows showing the page, each way.
    pub fn area(&self) -> (u32, u32) {
        let widest = self.areas.values().map(|a| a.0).max();
        let tallest = self.areas.values().map(|a| a.1).max();
        (widest.unwrap_or(0), tallest.unwrap_or(0))
    }

    /// Physical pixels to a CSS pixel. A host that hasn't said counts as 1.
    fn scale(&self) -> f32 {
        match self.cell.scale {
            s if s > 0.0 => s as f32,
            _ => 1.0,
        }
    }

    /// The layout, once there is one.
    pub fn page(&self) -> Option<&Page> {
        self.laid.as_ref().map(|l| &l.page)
    }

    /// The pixel buffer's id, once there is one.
    pub fn pixels(&self) -> Option<PixelBufferId> {
        self.producer.as_ref().map(PixelProducer::id)
    }

    /// The page's row at the window's top.
    pub fn top(&self) -> i32 {
        self.top
    }

    /// The link the keyboard is on, as an index into the page's links.
    pub fn ring(&self) -> Option<usize> {
        self.ring.as_ref().map(|r| r.0)
    }

    /// The link the keyboard is on.
    pub fn ringed(&self) -> Option<&LinkBox> {
        self.page()?.links().get(self.ring()?)
    }

    /// Where the ringed link leads, with or without a layout to find it in.
    pub fn ring_dest(&self) -> Option<String> {
        let ringed = self.ring.as_ref().map(|r| r.1.clone());
        ringed.or_else(|| self.pending.clone())
    }

    /// Ring the link that leads here, once there's a layout: the ring a page put away had.
    pub fn ring_later(&mut self, dest: Option<String>) {
        self.pending = dest;
    }

    /// The furthest the page scrolls: its last row at the window's bottom.
    fn limit(&self) -> i32 {
        let height = self.page().map_or(0.0, Page::height);
        (height.ceil() as i32 - self.area().1 as i32).max(0)
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

    /// Where the reader is, for `C-o`: on the layout, or where the page is going once it has
    /// one.
    pub fn mark(&self) -> Option<Mark> {
        let (place, rev) = match (self.place(), &self.wait) {
            (Some(place), _) => (place, self.rev),
            (None, Some((to, rev))) => (to.clone(), *rev),
            (None, None) => return None,
        };
        Some(Mark {
            file: self.file,
            place,
            rev,
        })
    }

    /// The place a restart keeps: the byte of the source the line at the window's top shows.
    pub fn kept(&self) -> usize {
        match (self.page(), &self.wait) {
            (Some(page), _) => page.source_at(self.top as f32),
            (None, Some((Target::Line(source) | Target::Place { source, .. }, _))) => *source,
            _ => 0,
        }
    }

    /// The row of `page` a target is at, or why there's none.
    fn aim(&self, page: &Page, to: &Target) -> std::result::Result<f32, String> {
        // A line's top needn't be a whole pixel, and the window's top is: the first whole one
        // in the line, so the line at the top is this one, and not the foot of the one over it.
        let whole = |y: f32| (y - 0.01).ceil();
        Ok(match to {
            Target::Top => 0.0,
            Target::Line(source) if page.source_at(0.0) >= *source => 0.0,
            Target::Line(source) => whole(page.y_of(*source)),
            Target::Place { source, offset } => page.y_of(*source) + offset,
            Target::Heading(anchor) => {
                let y = anchor_y(page, anchor).ok_or_else(|| format!("No heading #{anchor}"))?;
                y - GAP * self.scale()
            }
        })
    }

    /// Go to a place: now if there's a layout to find it in, and otherwise when the next one is
    /// made, as for a page that has yet to hear its first `Resize`.
    pub fn go(&mut self, ctx: &mut Ctx, to: Target) {
        match self.page().map(|page| self.aim(page, &to)) {
            Some(Ok(top)) => {
                self.scroll_to(top);
            }
            Some(Err(why)) => ctx.messages.push(why),
            None => {
                let rev = ctx.text(self.file).map_or(self.rev, TextBuf::rev);
                self.wait = Some((to, rev));
            }
        }
    }

    /// Go to the heading an anchor names, or say there's none.
    pub fn anchor(&mut self, anchor: &str) -> Result {
        let page = self
            .page()
            .ok_or_else(|| Error::Msg("No page yet".into()))?;
        let top = self.aim(page, &Target::Heading(anchor.into()));
        self.scroll_to(top.map_err(Error::Msg)?);
        Ok(())
    }

    /// Scroll so the page's row `top` is the window's first, held between the page's ends.
    /// True if that moved it.
    fn scroll_to(&mut self, top: f32) -> bool {
        let top = (top.round() as i32).clamp(0, self.limit());
        if top == self.top {
            return false;
        }
        (self.top, self.exact, self.stale) = (top, top as f32, true);
        self.unmoved.clear();
        true
    }

    /// The page's end doesn't come up past the window's bottom: a taller window, a shorter page.
    fn hold(&mut self) {
        let limit = self.limit();
        if self.top > limit {
            (self.top, self.exact, self.stale) = (limit, limit as f32, true);
        }
    }

    /// `j` and `k`: so many steps down, or up if it's negative.
    pub fn step(&mut self, steps: f32) {
        self.scroll_to(self.top as f32 + steps * STEP * self.scale());
    }

    /// `C-d`, `C-f` and their opposites: so much of a window down, or up if it's negative.
    pub fn windows(&mut self, part: f32) {
        self.scroll_to(self.top as f32 + part * self.area().1 as f32);
    }

    /// `gg` and `G`: the page's top, or its bottom.
    pub fn end(&mut self, bottom: bool) {
        self.scroll_to(if bottom { self.limit() as f32 } else { 0.0 });
    }

    /// `]]` and `[[`: the `count`th heading on from the window's top, or back from it.
    pub fn heading(&mut self, forward: bool, count: usize) {
        let Some(page) = self.page() else { return };
        let gap = GAP * self.scale();
        let mut top = self.top as f32;
        // At the page's top, a title in the window's upper half is where the reader is, though
        // the page can't go up to put it where a heading goes: the next one is the one after.
        let half = self.area().1 as f32 / 2.0;
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
        self.scroll_to(top);
    }

    /// `<Tab>` and `<S-Tab>`: ring the `count`th link on, or back, and scroll it into view. With
    /// none ringed in view it starts from the first one the window shows, or the last, going
    /// back; past the last link is the first, so a count is taken round the links, not walked.
    pub fn link(&mut self, forward: bool, count: usize) -> Result {
        let Some(page) = self.page() else {
            return Ok(());
        };
        let links = page.links();
        if links.is_empty() {
            return Err(Error::Msg("No links on this page".into()));
        }
        let height = self.area().1 as f32;
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
        let gap = GAP * self.scale();
        let to = first(&links[at]).and_then(|r| match r {
            r if r.y < top => Some(r.y - gap),
            r if r.y + r.h > bottom => Some(r.y + r.h + gap - height),
            _ => None,
        });
        (self.ring, self.stale) = (Some((at, links[at].dest.clone())), true);
        if let Some(to) = to {
            self.scroll_to(to);
        }
        Ok(())
    }

    /// `<Esc>`, and a click off a link: no link is ringed.
    pub fn unring(&mut self) {
        if self.ring.take().is_some() {
            self.stale = true;
        }
    }

    /// A `<details>` was followed: open if it was closed, closed if it was open. The next
    /// refresh lays the page out again with the place kept.
    pub fn toggle_details(&mut self, offset: usize) {
        match self.opened.binary_search(&offset) {
            Ok(i) => {
                self.opened.remove(i);
            }
            Err(i) => self.opened.insert(i, offset),
        }
    }

    /// The status line: the page's name, how far down it is as Vim's ruler says it, and where
    /// the ringed link leads, which a page has no other way to show.
    fn status(&self) -> String {
        let limit = self.limit();
        let at = match self.top {
            _ if self.laid.is_none() => String::new(),
            _ if limit == 0 => "All".into(),
            0 => "Top".into(),
            top if top >= limit => "Bot".into(),
            top => format!("{}%", i64::from(top) * 100 / i64::from(limit)),
        };
        let link = self.ringed().map(|l| format!("  {}", l.dest));
        format!("{}  {at}{}", self.title, link.unwrap_or_default())
    }

    // -------------------------------------------------------------- layout and paint

    /// Bring the picture up to date: lay out again if the text, the width, the scale, the
    /// colours, the pictures or the opened details have changed since the layout was made, and
    /// paint if anything is owed. There's no theme hook, so the colours are read and compared
    /// each time. A page no window shows has no area and no pixel buffer, and nothing is done
    /// for it.
    pub fn refresh(&mut self, ctx: &mut Ctx) {
        if self.producer.is_none() || self.areas.is_empty() {
            return;
        }
        let (width, scale) = (self.area().0, self.scale());
        let (palette, arrivals) = {
            let kit = self.kit.borrow();
            (kit.palette(ctx), kit.arrivals)
        };
        let Ok(rev) = ctx.text(self.file).map(TextBuf::rev) else {
            return;
        };
        let current = |l: &Laid| {
            (l.width, l.scale, l.arrivals) == (width, scale, arrivals)
                && l.palette == palette
                && l.opened == self.opened
        };
        if rev != self.rev || !self.laid.as_ref().is_some_and(current) {
            self.lay_out(ctx, palette);
        }
        if self.stale {
            self.paint();
        }
    }

    /// Lay the text out as it is now, for the area's width, and keep the reader's place: the
    /// line at the window's top stays there, to the pixel, wherever the new layout puts it.
    fn lay_out(&mut self, ctx: &mut Ctx, palette: Palette) {
        let (width, scale) = (self.area().0, self.scale());
        let dir = ctx.directory(self.file).map(Path::to_path_buf);
        let kit = self.kit.clone();
        let mut kit = kit.borrow_mut();
        let Kit {
            fonts: Some(fonts),
            pictures,
            loading,
            arrivals,
            front_matter,
            ..
        } = &mut *kit
        else {
            return;
        };
        let Ok(t) = ctx.text(self.file) else { return };
        // The place kept counts bytes of the text the last layout was made for, and its offset
        // is in that layout's pixels.
        let was = self.rev;
        let keep = match self.wait.take() {
            Some((to, since)) => Some(carry(t, since, to)),
            None => self.place().map(|to| match carry(t, was, to) {
                Target::Place { source, offset } => Target::Place {
                    source,
                    offset: offset * scale / self.laid.as_ref().map_or(scale, |l| l.scale),
                },
                to => to,
            }),
        };
        // The one place a layout is asked for. The engine's `opened: &self.opened` goes here
        // when it arrives.
        let env = Env {
            width,
            scale,
            palette: &palette,
            dir: dir.as_deref(),
            front_matter: *front_matter,
            tab_width: t.settings().tab_width,
        };
        let rev = t.rev();
        let page = Page::new(&t.rope().to_string(), &env, fonts, pictures);
        // A picture is asked for once: answered, or with a job already reading it, it isn't.
        let asked = |p: &&PathBuf| !pictures.answered(p) && loading.insert((*p).clone());
        let wanted: Vec<PathBuf> = page.wants().iter().filter(asked).cloned().collect();
        let arrivals = *arrivals;
        drop(kit);

        let top = match keep.map(|to| self.aim(&page, &to)) {
            Some(Ok(top)) => top,
            Some(Err(why)) => {
                ctx.messages.push(why);
                0.0
            }
            None => 0.0,
        };
        // The same link stays ringed: where it was among the links if it still leads there,
        // or wherever it has moved to; and a page brought back rings the link it had.
        let ringed = self.ring.take().map(|(i, dest)| (Some(i), dest));
        let ringed = ringed.or_else(|| self.pending.take().map(|dest| (None, dest)));
        self.ring = ringed.and_then(|(i, dest)| {
            let links = page.links();
            let same = |l: &LinkBox| l.dest == dest;
            let kept = i.filter(|i| links.get(*i).is_some_and(same));
            Some((kept.or_else(|| links.iter().position(same))?, dest))
        });
        self.laid = Some(Laid {
            page,
            width,
            scale,
            palette,
            arrivals,
            opened: self.opened.clone(),
        });
        self.rev = rev;
        self.exact = top.clamp(0.0, self.limit() as f32);
        self.top = self.exact.round() as i32;
        self.stale = true;
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

    /// Paint the part of the page the windows show, and present the area. The buffer may be
    /// larger than the area, having grown for a wider window once, and the rest of it is out of
    /// sight.
    fn paint(&mut self) {
        let (width, height) = self.area();
        let top = self.top;
        let (Some(laid), Some(producer)) = (&self.laid, &mut self.producer) else {
            return;
        };
        // An index the layout hasn't got never reaches the typesetter.
        let focus = self.ring.as_ref().map(|r| r.0);
        let marks = Marks {
            focus: focus.filter(|i| *i < laid.page.links().len()),
        };
        let mut kit = self.kit.borrow_mut();
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

    // -------------------------------------------------------------- events

    /// The windows that have left the page are forgotten: the picture is for those that show
    /// it, and a page none shows is about to go.
    pub fn prune(&mut self, ctx: &Ctx) {
        let was = self.area();
        self.areas.retain(|w, _| ctx.window_buffer(*w) == self.buf);
        if self.area() != was {
            self.stale = true;
            self.hold();
        }
    }

    /// A window showing the page has an area this size. The pixel buffer is made the first time
    /// the cells have a size in pixels, and grown when the area outgrows it.
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
        if width == 0 || height == 0 || self.leaving {
            return;
        }
        let was = (self.cell, self.area());
        self.prune(ctx);
        if ctx.window_buffer(window) == self.buf {
            self.areas.insert(window, (width, height));
        }
        let (width, height) = self.area();
        if width == 0 || height == 0 {
            return;
        }
        // A taller area shows rows of the buffer that were last painted for another place, and
        // the page's end may have come up past the window's bottom.
        self.cell = cell;
        self.stale |= was != (cell, (width, height));
        self.hold();
        let Some(producer) = &mut self.producer else {
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
                Ok((_, producer)) => (self.producer, self.stale) = (Some(producer), true),
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
            self.stale = true;
        }
    }

    /// The link under the pointer.
    fn link_under(&self, p: &SurfacePointer) -> Option<usize> {
        self.page()?.link_at(p.x, p.y + self.top as f32)
    }

    /// The wheel scrolls by pixels, and a click on a link follows it.
    fn pointer(&mut self, ctx: &mut Ctx, p: &SurfacePointer) {
        match p.kind {
            PointerKind::Scroll { dy, .. } if dy.is_finite() => {
                // In cells, and a trackpad's are fractions of one: what isn't a whole pixel
                // waits for the next. A delta past any number goes to the end and carries
                // nothing, since infinity has no fraction.
                let by = dy * self.cell.height as f32 + self.carry;
                self.carry = if by.is_finite() { by.fract() } else { 0.0 };
                self.scroll_to(self.top as f32 + by.trunc());
            }
            PointerKind::Press(Button::Left) => {
                self.pressed = self.link_under(p);
                if self.pressed.is_none() {
                    self.unring();
                }
            }
            PointerKind::Release(Button::Left) => {
                // A click is a press and a release on one link.
                let (over, pressed) = (self.link_under(p), self.pressed.take());
                if over.is_none() || over != pressed {
                    return;
                }
                let dest = over.and_then(|i| self.page()?.links().get(i).map(|l| l.dest.clone()));
                (self.ring, self.stale) = (over.zip(dest), true);
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

/// The page's buffer: a surface that shows the pixel buffer over its whole area and writes no
/// cells. It gives no text, so there's no copy state: core would draw that cursor in cells, a
/// line to a row, over letters that aren't in cells.
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

    /// Placed at every render, since a frame keeps its cells and not its placements. The cells
    /// stay blank: where the buffer hasn't been presented yet, the theme's background shows.
    fn render(&mut self, frame: &mut Frame, _info: &RenderInfo) {
        let mut st = self.state.borrow_mut();
        // Whoever moves the page paints it in the same turn. This catches what was left owed, a
        // frame late, since the present asks for the frame after this one.
        if st.stale {
            st.paint();
        }
        if let Some(producer) = &st.producer {
            frame.place(producer.id(), -1);
        }
    }

    fn status(&self) -> Option<String> {
        Some(self.state.borrow().status())
    }

    /// A size, the focus, the wheel and a click. Keys are the keymap's: one it doesn't bind is
    /// undefined here, as on any surface.
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
            // A window may have left the page for another buffer, which sends no `Resize`.
            Event::Focus(_) => {
                st.prune(ctx);
                false
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
