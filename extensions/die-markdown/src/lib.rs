//! die-markdown: a rendered view of a markdown file, as an extension (scope D14). It depends on
//! die-core alone, so it's platform-neutral and out of the product: `cargo xtask lint` holds it
//! to that. Its parts:
//!
//! - this file: [`Markdown`], the extension, with your [`Options`], the `markdown` mode a
//!   markdown file's buffer gets, the `markdown-toggle` and `markdown-follow-link` commands, the
//!   page's keys, and the faces both views take their colours from;
//! - [`typeset`]: from markdown source to a laid-out page and its pixels, as GitHub draws a
//!   README. It knows nothing of the editor;
//! - `page`: one file's page in the editor: a buffer of its own, shown in the file's window,
//!   that scrolls the picture, follows links and follows the file;
//! - [`plan`]: from markdown source to the decorations that make the view in place: what to
//!   hide, what to show in its place, which faces go where, and where the links lead. It knows
//!   nothing of the editor either;
//! - `view`: the mode, one buffer's view in place: it puts a plan on the buffer and takes it off.
//!
//! There are two rendered views, and `markdown-toggle` shows the one the host can. Where it
//! shows pixels, the view is a page: the system's proportional font, headings that are large,
//! code in panels, tables with a grid, pictures. It's a picture of the file, in a buffer beside
//! the file's own, so the file's cursor and undo wait where they were. Where it doesn't (Linux,
//! for now), or with `page` off, the view is in place: the same buffer and the same text with
//! the markup hidden under decorations, in the cell grid's one size of letter. Either is for
//! reading: raw is where you edit. Nothing runs for a buffer whose view is raw.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

mod page;
pub mod plan;
pub mod typeset;
mod view;

pub use view::View;

use die_core::session::{Kept, Value, json};
use die_core::{
    Args, BufferId, Checkout, Ctx, Error, Extension, FileTarget, Key, KeyEvent, PixelBufferId,
    Prefix, Registry, Result, ShowIn, State, Style, Underline, WindowId,
};
use page::{Left, Mark, Target};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use view::Rendered;

/// The mode's name, which names its keymap: your config binds the toggle's key there.
pub const MODE: &str = "markdown";
/// The keymap over a link in the view in place.
pub const LINK_KEYS: &str = "markdown-link";
/// The page's kind, which names its keymap: your config binds the toggle's key there too.
pub const PAGE: &str = "markdown-page";

/// Files a link leads to that DIE hands to [`Options::opener`] rather than open as text:
/// pictures, PDFs and web pages (web content goes to a real browser).
const OUTSIDE: [&str; 11] = [
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg", "pdf", "html", "htm",
];

// ------------------------------------------------------------------ options

/// Your options (C7), as fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The file name extensions, without the dot and in any case, whose files get the mode.
    pub extensions: Vec<String>,
    /// Show front matter dimmed in the rendered view, where the default hides it, as your
    /// preview's export strips it.
    pub front_matter: bool,
    /// The program a link's address goes to when it isn't a file DIE opens: a web address, a
    /// picture, a PDF. None is `xdg-open` where there is one, and `open` otherwise.
    pub opener: Option<String>,
    /// The rendered view is a page drawn in pixels, wherever the host shows pixels and the
    /// system has fonts to set it in. False, or a host that shows none, gives the view in place.
    pub page: bool,
    /// The colours a page is drawn in.
    pub palette: Colours,
    /// The directory DIE's own fonts are in, which a page falls back on where the system has
    /// none it knows. None finds it as DIE does: `$DIE_FONT_DIR`, the app's bundle, the
    /// workspace's `fonts/`.
    pub fonts: Option<PathBuf>,
}

/// The colours a page is drawn in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Colours {
    /// Your theme's: the `markdown-*` faces, so a page is in the colours of everything else.
    #[default]
    Theme,
    /// GitHub's own dark colours.
    GithubDark,
    /// GitHub's own light colours.
    GithubLight,
}

impl Default for Options {
    /// The extensions Emacs's markdown-mode takes, front matter hidden, the system's opener,
    /// and a page in the theme's colours.
    fn default() -> Options {
        let extensions = ["md", "markdown", "mkd", "mdown", "mkdn", "mdwn"];
        Options {
            extensions: extensions.map(String::from).to_vec(),
            front_matter: false,
            opener: None,
            page: true,
            palette: Colours::Theme,
            fonts: None,
        }
    }
}

/// A file's page as one window has it, for a test to read ([`Markdown::page`]): where it is,
/// what it shows, and the layout, which says where its links and headings are and where a byte
/// of the source is, so a test asks the page where to click and what to expect.
pub struct Probe<'a> {
    /// The page's own buffer.
    pub buffer: BufferId,
    /// The page's row at the window's top, in physical pixels.
    pub top: i32,
    /// The window's area, which its picture covers, in physical pixels.
    pub area: (u32, u32),
    /// The page's height in physical pixels as the window has it laid out, 0 while there's no
    /// layout.
    pub height: f32,
    /// The link the keyboard is on in the window, as an index into the page's links.
    pub ring: Option<usize>,
    /// The window's pixel buffer, once it has one.
    pub pixels: Option<PixelBufferId>,
    /// The window's layout, once it has one.
    pub page: Option<&'a typeset::Page>,
}

/// The page's colours as the theme has them now: what [`Colours::Theme`] draws in.
pub fn theme_palette(ctx: &Ctx) -> typeset::Palette {
    page::theme_palette(ctx)
}

// ------------------------------------------------------------------ the extension

/// The places `C-o` goes back through on one file's page in one window, the newest last, and
/// the places it came back from, for `L`.
#[derive(Default)]
struct Trail {
    back: Vec<Mark>,
    forward: Vec<Mark>,
}

/// What a file keeps of a page that was put away: the byte of the source its top line showed,
/// where the file's next page starts, and where its ringed link led.
struct Away {
    at: usize,
    ring: Option<(String, u64)>,
}

/// Where a window left a file's page for the file: the cursor there, its row in the window, the
/// text's revision then, and the page's place to the pixel and its ringed link, which the toggle
/// back brings back while the cursor is still there, at that row.
struct Return {
    cursor: usize,
    row: usize,
    rev: u64,
    mark: Mark,
    ring: Option<(String, u64)>,
}

/// The extension: your options, which buffers' view in place is on, the pages, and what a file
/// keeps of a page that's gone.
#[derive(Default)]
pub struct Markdown {
    /// Your options.
    pub options: Options,
    rendered: Rendered,
    /// Each page, by its file's buffer: one a file, whichever windows show it. A page no window
    /// shows is put away when the command ends, so these are the pages on screen.
    pages: BTreeMap<BufferId, page::Shared>,
    /// Each window's trail of followed links on each file's page, which outlives the page, as
    /// a browser window has a history of its own.
    trails: BTreeMap<(WindowId, BufferId), Trail>,
    /// Where a page that was put away was, by its file. The file's next page starts there.
    places: BTreeMap<BufferId, Away>,
    /// The `<details>` a file's last page had opened, and the text's revision their offsets
    /// count in: the file's next page opens them again, as a browser keeps them for as long as
    /// the page lives.
    opened: BTreeMap<BufferId, (Vec<usize>, u64)>,
    /// Where a window left a file's page for the file, by file and window.
    returns: BTreeMap<(BufferId, WindowId), Return>,
    /// The fonts and the pictures, which the pages share.
    kit: page::SharedKit,
}

impl Markdown {
    /// The extension with these options.
    pub fn new(options: Options) -> Markdown {
        Markdown {
            options,
            ..Markdown::default()
        }
    }

    /// A file's page as `window` has it, read by `read`, or None if the buffer has no page.
    /// A window that doesn't show the page, or None, reads the window the reader was in last.
    /// For tests.
    pub fn page<R>(
        &self,
        file: BufferId,
        window: Option<WindowId>,
        read: impl FnOnce(&Probe<'_>) -> R,
    ) -> Option<R> {
        let st = self.pages.get(&file)?.borrow();
        let w = window;
        Some(read(&Probe {
            buffer: st.buf?,
            top: st.top(w),
            area: st.area(w),
            height: st.page(w).map_or(0.0, typeset::Page::height),
            ring: st.ring(w),
            pixels: st.pixels(w),
            page: st.page(w),
        }))
    }

    /// `markdown-toggle`: the other view. On a page, its file again. On text, the page where
    /// one can be had and the view in place where it can't. The buffer gets the mode if it
    /// hasn't got it, so the command works on a buffer with no file too.
    fn toggle(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let buf = a.buffer.or(ctx.focused_buffer());
        let buf = buf.ok_or_else(|| Error::Msg("No buffer here".into()))?;
        if let Some(state) = self.page_at(buf) {
            return self.leave_page(ctx, a, &state);
        }
        ctx.enable_mode::<View>(buf)?;
        // A view in place that's on, as one an older session brought back, goes off first.
        let in_place = ctx.with_mode(buf, |v: &mut View, _| v.is_rendered())?;
        if !in_place && self.can_page(ctx) {
            return self.show_page(ctx, a, buf);
        }
        ctx.with_mode(buf, |v: &mut View, ctx| v.toggle(buf, ctx))?
    }

    /// Whether a page can be had: your option, a host that shows pixels, and fonts.
    fn can_page(&mut self, ctx: &mut Ctx) -> bool {
        self.options.page && ctx.pixels_supported() && self.kit.borrow_mut().has_fonts(ctx)
    }

    /// The page whose buffer this is.
    fn page_at(&self, buf: BufferId) -> Option<page::Shared> {
        let own = |s: &&page::Shared| s.borrow().buf == Some(buf);
        self.pages.values().find(own).cloned()
    }

    /// The page the command was typed on.
    fn page_here(&self, ctx: &Ctx, a: &Args) -> Result<page::Shared> {
        let here = a.buffer.or(ctx.focused_buffer());
        let page = here.and_then(|b| self.page_at(b));
        page.ok_or_else(|| Error::Msg("No page here".into()))
    }

    /// A file's page, made if there's none: a buffer named after the file's, in the file's
    /// directory, with the window's whole width, since the page centres its own column. It
    /// shows nothing until a window shows it and says how large it is, and it starts where the
    /// file's last page was put away, if one was.
    fn page_for(&mut self, ctx: &mut Ctx, file: BufferId) -> Result<page::Shared> {
        if let Some(state) = self.pages.get(&file) {
            return Ok(state.clone());
        }
        let rev = ctx.text(file)?.rev();
        // A view in place that's on, as one an older session brought back, goes off: a file has
        // one rendered view at a time, and leaving the page lands on raw text.
        let raw = |v: &mut View, ctx: &mut Ctx| match v.is_rendered() {
            true => v.clear(file, ctx),
            false => Ok(()),
        };
        ctx.with_mode(file, raw).unwrap_or(Ok(()))?;
        let name = format!("{} *page*", ctx.buffer_name(file).unwrap_or_default());
        let state = Rc::new(RefCell::new(page::State::new(file, rev, self.kit.clone())));
        let buf = ctx.create_surface(&name, page::PageSurface::new(state.clone()));
        if let Some(dir) = ctx.directory(file).map(Path::to_path_buf) {
            ctx.set_directory(buf, dir);
        }
        ctx.change_settings(buf, |s| s.max_width = None)?;
        // The name core gave it, which has a number after it if another file's page had it.
        let title = ctx.buffer_name(buf).unwrap_or_default().to_owned();
        {
            let mut st = state.borrow_mut();
            (st.buf, st.title, st.named) = (Some(buf), title, named(ctx, file));
            if let Some(away) = self.places.remove(&file) {
                st.go(ctx, None, Target::Line(away.at));
                st.ring_later(away.ring);
            }
            if let Some((opened, rev)) = self.opened.remove(&file) {
                st.reopen(opened, rev);
            }
        }
        self.pages.insert(file, state.clone());
        Ok(state)
    }

    /// From text to its page, in this window: the line the cursor is on stays at the height it
    /// was, so the eye finds its place, and where this window's toggle from the page left the
    /// cursor, the page comes back to the pixel with the link it had ringed; a page that was put
    /// away, or that this window left by a link while another kept it up, comes back where it
    /// was, since the reader left it by a link and not by the cursor. A second window on the
    /// same file shows the same page, with a place of its own (C10).
    fn show_page(&mut self, ctx: &mut Ctx, a: &Args, file: BufferId) -> Result {
        // Read while the window still shows the text: core parks its view when it leaves.
        let win = a.window.or(ctx.focused());
        let back = win.and_then(|w| self.returns.remove(&(file, w)));
        let cell = ctx.cell_pixels().height as f32;
        let t = ctx.text(file)?;
        let at = win.and_then(|w| {
            let cursor = t.cursor(w)?;
            let row = t.screen_pos(w, cursor).map_or(0, |(row, _)| row);
            let stayed = |r: &Return| r.row == row && page::carried(t, r.rev, r.cursor) == cursor;
            if let Some(r) = back.filter(stayed) {
                return Some((r.mark.now(t), r.ring));
            }
            let source = t.line_start(t.line_of(cursor));
            Some(match row {
                // On the top row, the file's first line is the page's top, its padding and all.
                0 => (Target::Line(source), None),
                row => {
                    let offset = -(row as f32 * cell);
                    (Target::Place { source, offset }, None)
                }
            })
        });
        // A window that left the page for elsewhere while another kept it up comes back where
        // it was, as to a page put away.
        let up = win.zip(self.pages.get(&file));
        let parked = up.is_some_and(|(w, s)| s.borrow().parks(w));
        let at = at.filter(|_| !self.places.contains_key(&file) && !parked);
        let state = self.page_for(ctx, file)?;
        let buf = state.borrow().buf.ok_or(Checkout::Gone)?;
        let shown = ctx.show(buf, ShowIn::Current)?;
        let mut st = state.borrow_mut();
        // A page brought back where a link left it was put there by no cursor, so leaving it
        // moves the cursor to its top line, as leaving a scrolled page does.
        if let Some((to, ring)) = at {
            st.ring_later(ring);
            st.go(ctx, shown, to);
            st.unmoved.extend(shown);
        }
        st.refresh(ctx);
        Ok(())
    }

    /// From a page to its file, in this window. If the page was scrolled since the toggle
    /// showed it here, the cursor goes to the line at the page's top, at the window's top;
    /// otherwise the cursor and the scroll are as they were left. The page goes when no window
    /// shows it any more.
    fn leave_page(&mut self, ctx: &mut Ctx, a: &Args, state: &page::Shared) -> Result {
        let win = a.window.or(ctx.focused());
        let (file, buf, left) = {
            let st = state.borrow();
            (st.file, st.buf, win.and_then(|w| st.left(w)))
        };
        let shown = ctx.show(file, ShowIn::Current)?;
        {
            // The window's view goes now, so the command's end doesn't take it for a window
            // that left by `C-^`.
            let mut st = state.borrow_mut();
            st.unmoved.retain(|w| Some(*w) != shown);
            st.forget(win);
        }
        if let (Some(mut left), Some(w)) = (left, shown) {
            left.window = w;
            self.landed(ctx, file, left)?;
        }
        if let Some(buf) = buf.filter(|b| !shows(ctx, *b)) {
            self.pages.remove(&file);
            self.keep_opened(file, &state.borrow());
            // The window has left it, and core has queued telling it so. It goes after that, in
            // the same turn: told when already gone, core would report an error.
            ctx.defer(move |ctx| {
                ctx.kill(buf);
                Ok(())
            });
        }
        Ok(())
    }

    /// A window left a file's page for the file, by the toggle or by `C-^`, `:b` or `:e`. If
    /// the page was scrolled there since the toggle showed it, the cursor goes to the line at
    /// the page's top, at the window's top; otherwise the cursor and the scroll are as they were
    /// left. The page's place is kept for the toggle back either way.
    fn landed(&mut self, ctx: &mut Ctx, file: BufferId, left: Left) -> Result {
        let w = left.window;
        let t = ctx.text_mut(file)?;
        let row = match (left.source, left.moved) {
            (Some(source), true) => {
                let line = t.line_start(t.line_of(source.min(t.len())));
                t.set_cursor(w, line);
                t.set_scroll(w, line, 0);
                Some(0)
            }
            _ => t
                .cursor(w)
                .and_then(|c| t.screen_pos(w, c))
                .map(|(row, _)| row),
        };
        if let (Some(cursor), Some(row), Some(mark)) = (t.cursor(w), row, left.mark) {
            let rev = t.rev();
            let back = Return {
                cursor,
                row,
                rev,
                mark,
                ring: left.ring,
            };
            self.returns.insert((file, w), back);
        }
        Ok(())
    }

    /// A file's page is going: the `<details>` it opened stay with the file.
    fn keep_opened(&mut self, file: BufferId, st: &page::State) {
        match st.opened() {
            (opened, _) if opened.is_empty() => self.opened.remove(&file),
            kept => self.opened.insert(file, kept),
        };
    }

    /// A key of the page that only moves it, in the window it was typed in: done, then painted
    /// in the same turn.
    fn on_page(&mut self, ctx: &mut Ctx, a: &Args, act: Move) -> Result {
        let state = self.page_here(ctx, a)?;
        let mut st = state.borrow_mut();
        let w = a.window.or(ctx.focused());
        let done = act(&mut st, w, a.count.unwrap_or(1).max(1));
        st.refresh(ctx);
        done
    }

    /// `markdown-page-forward`: a window down, less an eighth to keep your place. Shift isn't
    /// part of a character's chord, so `<S-Space>` comes here as `<Space>` does, and goes back,
    /// as a browser's does.
    fn forward(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let space = |k: &KeyEvent| k.mods.shift && k.key == Key::Char(' ');
        let part = if a.keys.last().is_some_and(space) {
            -0.875
        } else {
            0.875
        };
        let state = self.page_here(ctx, a)?;
        let mut st = state.borrow_mut();
        let w = a.window.or(ctx.focused());
        st.windows(w, part * a.count.unwrap_or(1).max(1) as f32);
        st.refresh(ctx);
        Ok(())
    }

    /// Show a file's page in this window at a place, with the place it was come from, if any,
    /// as the newest `C-o` goes back to.
    fn open_page(
        &mut self,
        ctx: &mut Ctx,
        file: BufferId,
        to: Target,
        from: Option<Mark>,
    ) -> Result {
        let state = self.page_for(ctx, file)?;
        let buf = state.borrow().buf.ok_or(Checkout::Gone)?;
        let shown = ctx.show(buf, ShowIn::Current)?;
        if let (Some(from), Some(w)) = (from, shown) {
            self.left(w, file, from);
        }
        let mut st = state.borrow_mut();
        // A link arrives at a place of its own, and rings nothing: what was ringed when the page
        // was put away comes back with the toggle or `C-o`.
        st.ring_later(None);
        st.go(ctx, shown, to);
        st.refresh(ctx);
        Ok(())
    }

    /// A place was left for `file`'s page in window `w`: the newest `C-o` there goes back to,
    /// and the way forward from it is forgotten, as a browser forgets it.
    fn left(&mut self, w: WindowId, file: BufferId, from: Mark) {
        let trail = self.trails.entry((w, file)).or_default();
        trail.back.push(from);
        trail.forward.clear();
    }

    /// `markdown-page-older` and `markdown-page-newer`: back through the places links were
    /// followed from in this window, on this page or the one a link led from, and forward
    /// again. Each file keeps two lists in each window, and a place left goes on the other list
    /// of the file arrived at. A place in a file whose buffer has gone is stepped over.
    fn travel(&mut self, ctx: &mut Ctx, a: &Args, back: bool) -> Result {
        let mut state = self.page_here(ctx, a)?;
        let win = a.window.or(ctx.focused());
        let mut win = win.ok_or_else(|| Error::Msg("No window here".into()))?;
        for _ in 0..a.count.unwrap_or(1).max(1) {
            let (file, here) = {
                let st = state.borrow();
                (st.file, st.mark(Some(win)))
            };
            let to = loop {
                let trail = self.trails.entry((win, file)).or_default();
                let list = if back {
                    &mut trail.back
                } else {
                    &mut trail.forward
                };
                let none = if back {
                    "No older place"
                } else {
                    "No newer place"
                };
                let to = list.pop().ok_or_else(|| Error::Msg(none.into()))?;
                if ctx.is_live(to.file) {
                    break to;
                }
            };
            let there = self.page_for(ctx, to.file)?;
            let buf = there.borrow().buf.ok_or(Checkout::Gone)?;
            win = ctx.show(buf, ShowIn::Current)?.unwrap_or(win);
            let trail = self.trails.entry((win, to.file)).or_default();
            let list = if back {
                &mut trail.forward
            } else {
                &mut trail.back
            };
            list.extend(here);
            let target = to.now(ctx.text(to.file)?);
            let mut st = there.borrow_mut();
            // Back to a page another window kept up, with the link this one had ringed.
            if let Some(ring) = st.parked_ring(win) {
                st.ring_later(Some(ring));
            }
            st.go(ctx, Some(win), target);
            st.refresh(ctx);
            drop(st);
            state = there;
        }
        self.settle(ctx);
        Ok(())
    }

    /// After a command, and after anything else that may have changed which windows show what:
    /// a page a window shows is brought up to date, the windows that left it, its file's name
    /// and the theme's colours included, and one no window shows is put away. A window that
    /// left a page for the page's own file, by `C-^`, `:b` or `:e`, left it as the toggle does.
    fn settle(&mut self, ctx: &mut Ctx) {
        // A window that's gone takes its ways back with it.
        if !(self.trails.is_empty() && self.returns.is_empty()) {
            let live = ctx.window_list();
            self.trails.retain(|(w, _), _| live.contains(w));
            self.returns.retain(|(_, w), _| live.contains(w));
        }
        if self.pages.is_empty() {
            return;
        }
        let (mut away, mut home) = (vec![], vec![]);
        for (file, state) in &self.pages {
            let mut st = state.borrow_mut();
            let left = st.departed(ctx).into_iter();
            let left: Vec<Left> = left
                .filter(|l| ctx.window_buffer(l.window) == Some(*file))
                .collect();
            match st.buf.is_some_and(|b| shows(ctx, b)) {
                true => {
                    st.prune(ctx);
                    follow_name(ctx, &mut st);
                    st.refresh(ctx);
                }
                // Left for its own file, or killed itself, as by `:bd` on it: its place isn't
                // kept, so the file's next page starts at the cursor.
                false => {
                    let gone = !st.buf.is_some_and(|b| ctx.is_live(b));
                    away.push((*file, left.is_empty() && !gone));
                }
            }
            home.extend(left.into_iter().map(|l| (*file, l)));
        }
        for (file, left) in home {
            if let Err(e) = self.landed(ctx, file, left) {
                ctx.messages.push(format!("markdown: {e}"));
            }
        }
        for (file, keep) in away {
            self.put_away(ctx, file, keep);
        }
    }

    /// No window shows a file's page any more, a link, `C-^` or `:e` having taken its window:
    /// it goes, as it does on the toggle, and if `keep`, its place stays with the file, so the
    /// file's next page, by the toggle or by `C-o`, starts there. The `<details>` it opened
    /// stay with the file either way.
    fn put_away(&mut self, ctx: &mut Ctx, file: BufferId, keep: bool) {
        let Some(state) = self.pages.remove(&file) else {
            return;
        };
        let st = state.borrow();
        self.keep_opened(file, &st);
        if keep {
            let away = Away {
                at: st.kept(),
                ring: st.ring_dest(),
            };
            self.places.insert(file, away);
        }
        if let Some(buf) = st.buf.filter(|b| ctx.is_live(*b)) {
            // Core may have queued telling it a window left; it goes after that, in the same
            // turn: told when already gone, core would report an error.
            ctx.defer(move |ctx| {
                ctx.kill(buf);
                Ok(())
            });
        }
    }

    /// A buffer was killed. A file's page goes with the file, its trail and its kept place too,
    /// and a page killed by itself, as by `:bd` on it, is forgotten.
    fn killed(&mut self, ctx: &mut Ctx, buf: BufferId) {
        self.trails.retain(|(_, file), _| *file != buf);
        self.places.remove(&buf);
        self.opened.remove(&buf);
        self.returns.retain(|(file, _), _| *file != buf);
        match self.pages.remove(&buf) {
            Some(state) => {
                let page = state.borrow().buf;
                if let Some(page) = page {
                    ctx.kill(page);
                }
            }
            None => self.pages.retain(|_, s| s.borrow().buf != Some(buf)),
        }
    }

    /// A buffer's text changed: its page follows, shown or not by the file's own window.
    fn edited(&mut self, ctx: &mut Ctx, buf: BufferId) {
        if let Some(state) = self.pages.get(&buf) {
            state.borrow_mut().refresh(ctx);
        }
    }

    /// Pictures a job read have come back: the pages are laid out again with them.
    pub(crate) fn pictures(&mut self, ctx: &mut Ctx, read: page::Read) {
        self.kit.borrow_mut().arrived(read);
        for state in self.pages.values() {
            state.borrow_mut().refresh(ctx);
        }
    }

    /// A page a restart remade can't be drawn after all, as on a host that shows no pixels:
    /// each window showing it shows its file with the view in place on, and the page goes.
    pub(crate) fn in_place_instead(&mut self, ctx: &mut Ctx, file: BufferId) -> Result {
        let Some(state) = self.pages.remove(&file) else {
            return Ok(());
        };
        let Some(page) = state.borrow().buf else {
            return Ok(());
        };
        let focused = ctx.focused();
        for w in ctx.window_list() {
            if ctx.window_buffer(w) == Some(page) {
                ctx.focus(w);
                ctx.show(file, ShowIn::Current)?;
            }
        }
        if let Some(w) = focused {
            ctx.focus(w);
        }
        ctx.enable_mode::<View>(file)?;
        let render = |v: &mut View, ctx: &mut Ctx| match v.is_rendered() {
            true => Ok(()),
            false => v.render(file, ctx),
        };
        ctx.with_mode(file, render)??;
        ctx.defer(move |ctx| {
            ctx.kill(page);
            Ok(())
        });
        Ok(())
    }

    /// `markdown-follow-link`: where the link under the cursor goes, or on a page the ringed
    /// link. In text it reads the text as it is, so it works raw and in the view in place.
    fn follow(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let none = || Error::Msg("No link here".into());
        let buf = a.buffer.or(ctx.focused_buffer()).ok_or_else(none)?;
        let win = a.window.or(ctx.focused()).ok_or_else(none)?;
        if let Some(state) = self.page_at(buf) {
            let followed = self.follow_on_page(ctx, &state, win);
            // A click isn't a command, so nothing else looks at what the windows show now.
            self.settle(ctx);
            return followed;
        }
        let t = ctx.text(buf)?;
        let at = t.cursor(win).ok_or_else(none)?;
        let planned = plan::plan(&t.rope().to_string(), &LINKS);
        let link = planned.links.iter().find(|l| l.range.contains(&at));
        let link = link.ok_or_else(none)?;
        let rendered = ctx.with_mode(buf, |v: &mut View, _| v.is_rendered());
        let dir = ctx.directory(buf).unwrap_or(Path::new("/"));
        match Dest::read(&link.dest).rooted(dir) {
            Dest::Anchor(anchor) => {
                let pos = heading(&planned, &anchor)?;
                // A file's buffer goes by its name, which keeps the place left, for `C-o`.
                match ctx.file(buf).map(|f| f.path.clone()) {
                    Some(path) => self.go(ctx, path, buf, pos),
                    None => {
                        ctx.text_mut(buf)?.set_cursor(win, pos);
                        Ok(())
                    }
                }
            }
            Dest::Outside(address) => {
                self.outside(ctx, address);
                Ok(())
            }
            // A path from the root is the file it names by now.
            Dest::File(path, anchor) | Dest::Root(path, anchor) => {
                let image = link.image;
                let Some(target) = self.file_target(ctx, buf, &path, image)? else {
                    return Ok(());
                };
                self.open_text(ctx, target, anchor, matches!(rendered, Ok(true)))
            }
        }
    }

    /// The file a link names, read against the directory of the buffer the link is in, or None
    /// if it has gone to your opener: a picture, a PDF, a web page. A file that isn't there is
    /// the error, and nothing opens.
    fn file_target(
        &mut self,
        ctx: &mut Ctx,
        buf: BufferId,
        path: &Path,
        image: bool,
    ) -> Result<Option<FileTarget>> {
        // `notes.md:12` is line 12 of `notes.md`, as `die notes.md:12` reads it, unless a file
        // has the whole name.
        let dir = ctx.directory(buf).unwrap_or(Path::new("/"));
        let target = FileTarget::parse(path.as_os_str(), dir);
        let path = &target.path;
        if !path.exists() {
            return Err(Error::Msg(format!("No file {}", path.display())));
        }
        let kind = path.extension().and_then(|e| e.to_str());
        if image || kind.is_some_and(|k| OUTSIDE.iter().any(|o| o.eq_ignore_ascii_case(k))) {
            self.outside(ctx, path.display().to_string());
            return Ok(None);
        }
        Ok(Some(target))
    }

    /// Open a link's file as text in this window, as `:e` would: at the heading the link names,
    /// at its line, or at its top. From the view in place the file it leads to is shown in
    /// place too, if it's markdown.
    fn open_text(
        &mut self,
        ctx: &mut Ctx,
        mut target: FileTarget,
        anchor: Option<String>,
        rendered: bool,
    ) -> Result {
        let to = ctx.open_file(&target.path)?;
        if rendered {
            let render = |v: &mut View, ctx: &mut Ctx| match v.is_rendered() {
                true => Ok(()),
                false => v.render(to, ctx),
            };
            ctx.with_mode(to, render).unwrap_or(Ok(()))?;
        }
        // A heading the file hasn't got is said, and the file opens at its top, as a browser
        // opens a page whose anchor is gone.
        if let Some(anchor) = anchor {
            let text = ctx.text(to)?.rope().to_string();
            match heading(&plan::plan(&text, &LINKS), &anchor) {
                Ok(pos) => target.line = Some(ctx.text(to)?.line_of(pos) + 1),
                Err(e) => ctx.messages.push(e.to_string()),
            }
        }
        // With no line of its own it's the top, which keeps the place left, for `C-o`.
        target.line = target.line.or(Some(1));
        ctx.open_target(&target)?;
        Ok(())
    }

    /// Follow the ringed link of a page. A heading here scrolls to it, and a `<details>` opens
    /// or closes. A markdown file's page takes this one's place in the window, at the heading
    /// or the line the link names, and any other file opens as text; this page is put away
    /// either way, and comes back where it was. The rest is your opener's. Where the reader
    /// leaves a place for another, the place is kept for `C-o`.
    fn follow_on_page(&mut self, ctx: &mut Ctx, state: &page::Shared, w: WindowId) -> Result {
        let (file, link, here) = {
            let st = state.borrow();
            (st.file, st.ringed(Some(w)).cloned(), st.mark(Some(w)))
        };
        let none = || Error::Msg("No link is ringed: <Tab> rings one".into());
        let link = link.ok_or_else(none)?;
        // The engine lists a `<details>` summary as a link to `details:` and its byte offset:
        // following it is a toggle, here, with the place kept and nothing to come back from.
        let details = link.dest.strip_prefix("details:");
        if let Some(offset) = details.and_then(|o| o.parse().ok()) {
            let mut st = state.borrow_mut();
            st.toggle_details(offset);
            st.refresh(ctx);
            return Ok(());
        }
        let dir = ctx.directory(file).unwrap_or(Path::new("/"));
        match Dest::read(&link.dest).rooted(dir) {
            Dest::Anchor(anchor) => {
                let mut st = state.borrow_mut();
                if st.unfold(Some(w), &anchor) {
                    st.refresh(ctx);
                }
                st.anchor(Some(w), &anchor)?;
                st.refresh(ctx);
                drop(st);
                if let Some(here) = here {
                    self.left(w, file, here);
                }
                Ok(())
            }
            Dest::Outside(address) => {
                self.outside(ctx, address);
                Ok(())
            }
            // A path from the root is the file it names by now.
            Dest::File(path, anchor) | Dest::Root(path, anchor) => {
                let Some(target) = self.file_target(ctx, file, &path, link.image)? else {
                    return Ok(());
                };
                let kind = target.path.extension().and_then(|e| e.to_str());
                let ours = |e: &String| kind.is_some_and(|k| e.eq_ignore_ascii_case(k));
                if !self.options.extensions.iter().any(ours) {
                    return self.open_text(ctx, target, anchor, false);
                }
                let to = ctx.open_file(&target.path)?;
                // `notes.md:0` is line 1, as `die notes.md:0` opens it.
                let at = match (anchor, target.line) {
                    (Some(anchor), _) => Target::Heading(anchor),
                    (None, Some(line)) => {
                        Target::Line(ctx.text(to)?.line_start(line.saturating_sub(1)))
                    }
                    (None, None) => Target::Top,
                };
                self.open_page(ctx, to, at, here)
            }
        }
    }

    /// Show `path`'s buffer in this window with the cursor on the line `pos` is in, as `:e`
    /// would, keeping the place it leaves.
    fn go(&mut self, ctx: &mut Ctx, path: PathBuf, buf: BufferId, pos: usize) -> Result {
        let line = Some(ctx.text(buf)?.line_of(pos) + 1);
        let col = None;
        ctx.open_target(&FileTarget { path, line, col })?;
        Ok(())
    }

    /// Hand an address to your opener, on a thread of its own, and say so if it fails.
    fn outside(&mut self, ctx: &mut Ctx, address: String) {
        let opener = self.options.opener.clone();
        let path = die_core::process::path().to_owned();
        ctx.messages.push(format!("Opening {address}"));
        let said = address.clone();
        let work = move || {
            let run = |program: &str| {
                let mut cmd = Command::new(program);
                cmd.arg(&address).env("PATH", &path);
                cmd.stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                cmd.status()
            };
            let status = match &opener {
                Some(program) => run(program),
                None => run("xdg-open").or_else(|_| run("open")),
            };
            match status {
                Ok(s) if s.success() => Ok(()),
                Ok(s) => Err(s.to_string()),
                Err(e) => Err(e.to_string()),
            }
        };
        let done = move |ctx: &mut Ctx, opened: std::result::Result<(), String>| {
            if let Err(why) = opened {
                ctx.messages.push(format!("Couldn't open {said}: {why}"));
            }
            Ok(())
        };
        ctx.spawn(work, done).detach();
    }
}

/// The plan `markdown-follow-link` reads links and headings from: nothing in it depends on a
/// width, and front matter stays, so nothing a link could be in is left out.
const LINKS: plan::Options = plan::Options {
    width: 0,
    tab_width: 0,
    front_matter: true,
};

/// Where the heading an anchor names starts, by GitHub's rule for anchors.
fn heading(planned: &plan::Plan, anchor: &str) -> Result<usize> {
    let slug = anchor.to_lowercase();
    let found = planned.headings.iter().find(|h| h.slug == slug);
    let found = found.ok_or_else(|| Error::Msg(format!("No heading #{anchor}")));
    Ok(found?.range.start)
}

/// Where a link leads, read from its address as written.
#[derive(Debug, PartialEq)]
enum Dest {
    /// A heading in this buffer: `#install`.
    Anchor(String),
    /// Anything with a scheme but `file:`: a web address, `mailto:`.
    Outside(String),
    /// A file, absolute or relative to this buffer's directory, and a heading in it.
    File(PathBuf, Option<String>),
    /// A path from the repository's root, as GitHub reads `/docs/guide.md`, and a heading in it.
    Root(PathBuf, Option<String>),
}

impl Dest {
    fn read(written: &str) -> Dest {
        if let Some(anchor) = written.strip_prefix('#') {
            return Dest::Anchor(decoded(anchor));
        }
        // A scheme is two characters or more, so `C:` isn't one, and has no dot in it, though
        // it may by the book: `notes.md:12` is a file and a line.
        let scheme = written.split_once(':').map(|(s, _)| s).filter(|s| {
            let rest = |c: char| c.is_ascii_alphanumeric() || "+-".contains(c);
            s.len() > 1 && s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(rest)
        });
        let (path, rooted) = match scheme {
            Some(s) if s.eq_ignore_ascii_case("file") => {
                (written[s.len() + 1..].trim_start_matches("//"), false)
            }
            Some(_) => return Dest::Outside(written.into()),
            None => (written, written.starts_with('/')),
        };
        let (path, anchor) = match path.split_once('#') {
            Some((path, anchor)) => (path, Some(decoded(anchor))),
            None => (path, None),
        };
        let path = PathBuf::from(decoded(path));
        match rooted {
            true => Dest::Root(path, anchor),
            false => Dest::File(path, anchor),
        }
    }

    /// A path from the repository's root as the file it names, read from `dir`, the directory
    /// of the buffer the link is in: the repository's, where `dir` is in one that has the file,
    /// and the system's otherwise.
    fn rooted(self, dir: &Path) -> Dest {
        let Dest::Root(path, anchor) = self else {
            return self;
        };
        let root = die_core::project_root(dir);
        let inside = root.join(path.strip_prefix("/").unwrap_or(&path));
        let found = FileTarget::parse(inside.as_os_str(), dir).path.exists();
        match root.join(".git").exists() && found {
            true => Dest::File(inside, anchor),
            false => Dest::File(path, anchor),
        }
    }
}

/// A file's buffer's name and directory, which its page's name and directory follow.
fn named(ctx: &Ctx, file: BufferId) -> (String, Option<PathBuf>) {
    let name = ctx.buffer_name(file).unwrap_or_default().to_owned();
    (name, ctx.directory(file).map(Path::to_path_buf))
}

/// A page's status line and directory follow its file's name and directory, which `:saveas`
/// changes. Core has no way to rename a buffer, so the page's buffer keeps its first name.
fn follow_name(ctx: &mut Ctx, st: &mut page::State) {
    let now = named(ctx, st.file);
    if now == st.named {
        return;
    }
    st.title = format!("{} *page*", now.0);
    if let (Some(buf), Some(dir)) = (st.buf, &now.1) {
        ctx.set_directory(buf, dir.clone());
    }
    st.named = now;
}

/// `%20` and the like as the characters they stand for.
fn decoded(written: &str) -> String {
    let bytes = written.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(bytes.len()), 0);
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            Some(byte) if bytes[i] == b'%' => {
                out.push(byte);
                i += 3;
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a window shows the buffer.
fn shows(ctx: &Ctx, buf: BufferId) -> bool {
    let mut windows = ctx.window_list().into_iter();
    windows.any(|w| ctx.window_buffer(w) == Some(buf))
}

/// Plan again, if `buf` has the mode and its view in place is on and behind.
fn refresh(ctx: &mut Ctx, buf: BufferId) -> Result {
    // No mode on it, or one that's busy: nothing of this extension's to do.
    ctx.with_mode(buf, |v: &mut View, ctx| v.refresh(buf, ctx))
        .unwrap_or(Ok(()))
}

/// Every face the view in place draws in, then those a page's code blocks are coloured in: its
/// name, the base face it's on, and what it adds. A theme that styles only the base faces still
/// draws a rendered file sensibly, and yours can style each by name. A page in the theme's
/// colours reads the same faces for their colours.
pub fn faces() -> [(&'static str, Option<&'static str>, Style); 32] {
    use Underline::{Double, Single};
    let s = Style::new;
    let view = [
        (plan::H[0], Some("heading"), s().underline(Double)),
        (plan::H[1], Some("heading"), s().underline(Single)),
        (plan::H[2], Some("heading"), s()),
        (plan::H[3], Some("bold"), s()),
        (plan::H[4], Some("bold"), s()),
        (plan::H[5], Some("bold"), s()),
        (plan::BOLD, Some("bold"), s()),
        (plan::ITALIC, None, s().italic()),
        (plan::STRIKE, Some("shadow"), s().strike()),
        (plan::CODE, Some("highlight"), s()),
        (plan::CODE_BLOCK, Some("highlight"), s()),
        (plan::LINK, Some("link"), s()),
        (plan::QUOTE, None, s().italic()),
        (plan::MARKER, Some("shadow"), s()),
        (plan::RULE, Some("shadow"), s()),
        (plan::TABLE_HEADER, Some("bold"), s()),
        (plan::META, Some("shadow"), s()),
        (plan::ALERT, Some("bold"), s()),
    ];
    let code = page::CODE_FACES.map(|(name, base)| (name, base, s()));
    std::array::from_fn(|i| match view.get(i) {
        Some(face) => *face,
        None => code[i - view.len()],
    })
}

/// What a key of the page that only moves it does, in the window it was typed in, with the
/// count.
type Move = fn(&mut page::State, Option<WindowId>, usize) -> Result;

/// The page's own keys that only move it: each command's name, what it says of itself, its keys
/// and what it does. A page has no text, so none of die-vim's keys are its own, and these are
/// the ones a reader's hands expect.
const MOVES: [(&str, &str, &str, Move); 12] = [
    (
        "markdown-page-down",
        "Scroll the page down a little, two lines and a half of text. A count repeats.",
        "j <Down> <C-e>",
        |p, w, n| {
            p.step(w, n as f32);
            Ok(())
        },
    ),
    (
        "markdown-page-up",
        "Scroll the page up a little, two lines and a half of text. A count repeats.",
        "k <Up> <C-y>",
        |p, w, n| {
            p.step(w, -(n as f32));
            Ok(())
        },
    ),
    (
        "markdown-page-half-down",
        "Scroll the page down half a window. A count repeats.",
        "<C-d>",
        |p, w, n| {
            p.windows(w, 0.5 * n as f32);
            Ok(())
        },
    ),
    (
        "markdown-page-half-up",
        "Scroll the page up half a window. A count repeats.",
        "<C-u>",
        |p, w, n| {
            p.windows(w, -0.5 * n as f32);
            Ok(())
        },
    ),
    (
        "markdown-page-backward",
        "Scroll the page up a window, less an eighth to keep your place. A count repeats.",
        "<C-b> <PageUp>",
        |p, w, n| {
            p.windows(w, -0.875 * n as f32);
            Ok(())
        },
    ),
    (
        "markdown-page-top",
        "Go to the page's top.",
        "gg <Home>",
        |p, w, _| {
            p.end(w, false);
            Ok(())
        },
    ),
    (
        "markdown-page-bottom",
        "Go to the page's bottom.",
        "G <End>",
        |p, w, _| {
            p.end(w, true);
            Ok(())
        },
    ),
    (
        "markdown-page-next-heading",
        "Go to the next heading, or the count's.",
        "]]",
        |p, w, n| {
            p.heading(w, true, n);
            Ok(())
        },
    ),
    (
        "markdown-page-previous-heading",
        "Go to the heading before, or the count's.",
        "[[",
        |p, w, n| {
            p.heading(w, false, n);
            Ok(())
        },
    ),
    (
        "markdown-page-next-link",
        "Ring the next link, starting from the first one in view, and scroll to it: <CR> follows the ringed link.",
        "<Tab>",
        |p, w, n| p.link(w, true, n),
    ),
    (
        "markdown-page-previous-link",
        "Ring the link before, starting from the last one in view, and scroll to it.",
        "<S-Tab>",
        |p, w, n| p.link(w, false, n),
    ),
    (
        "markdown-page-no-link",
        "Take the ring off the ringed link.",
        "<Esc>",
        |p, w, _| {
            p.unring(w);
            Ok(())
        },
    ),
];

/// die-vim's `C-w` keys, a copy of die-jellyfish's table, which copies die-whiteboard's, which
/// copies die-vim's own: a surface without text inherits none of die-vim's keys, since those
/// are the text kind's, and an extension depends on die-core alone, so it can't ask die-vim for
/// them. Without an `<Esc>` row, as the jellyfish's: `vim-cancel` wants a buffer with text.
const WINDOW_KEYS: [(&str, &str); 31] = [
    ("h <C-h> <Left> <C-Left>", "vim-window-left"),
    ("j <C-j> <Down> <C-Down>", "vim-window-down"),
    ("k <C-k> <Up> <C-Up>", "vim-window-up"),
    ("l <C-l> <Right> <C-Right>", "vim-window-right"),
    ("w <C-w>", "vim-window-next"),
    ("W <C-S-w>", "vim-window-prev"),
    ("t <C-t>", "vim-window-top-left"),
    ("b <C-b>", "vim-window-bottom-right"),
    ("p <C-p>", "vim-window-mru"),
    ("s S <C-s> <C-S-s>", "vim-window-split"),
    ("v <C-v>", "vim-window-vsplit"),
    ("n <C-n>", "vim-window-new"),
    ("c <C-c>", "vim-window-delete"),
    ("q <C-q>", "vim-quit"),
    ("o <C-o>", "vim-window-only"),
    ("= <C-=>", "vim-window-balance"),
    ("+", "vim-window-taller"),
    ("-", "vim-window-shorter"),
    (">", "vim-window-wider"),
    ("<lt>", "vim-window-narrower"),
    ("_ <C-_>", "vim-window-set-height"),
    ("|", "vim-window-set-width"),
    ("H <C-S-h>", "vim-window-move-far-left"),
    ("J <C-S-j>", "vim-window-move-very-bottom"),
    ("K <C-S-k>", "vim-window-move-very-top"),
    ("L <C-S-l>", "vim-window-move-far-right"),
    ("r <C-r>", "vim-window-rotate-downwards"),
    ("R <C-S-r>", "vim-window-rotate-upwards"),
    ("x <C-x>", "vim-window-exchange"),
    (":", "vim-window-ex"),
    ("0 1 2 3 4 5 6 7 8 9", "vim-window-digit-argument"),
];

impl Extension for Markdown {
    fn name(&self) -> &'static str {
        "markdown"
    }

    fn register(&mut self, reg: &mut Registry) {
        for (name, parent, style) in faces() {
            reg.face(name, parent, style);
        }
        let (rendered, front_matter) = (self.rendered.clone(), self.options.front_matter);
        let extensions = self.options.extensions.clone();
        reg.mode(MODE, move || View::new(rendered.clone(), front_matter))
            .files(move |path| {
                let kind = path.extension().and_then(|e| e.to_str());
                kind.is_some_and(|k| extensions.iter().any(|e| e.eq_ignore_ascii_case(k)))
            });
        reg.ext_command(
            "markdown-toggle",
            "Show this buffer's markdown rendered, or raw again. Rendered is a page drawn as GitHub draws a README, where DIE can draw one, and otherwise the text itself with its markup hidden. Either is for reading.",
            Markdown::toggle,
        );
        reg.ext_command(
            "markdown-follow-link",
            "Go where the link under the cursor leads, or on a page the ringed link: a heading here, a file in this window, or your browser.",
            Markdown::follow,
        );
        // On a link in the view in place, where the cursor can only be on its text. No leader
        // key is taken: your config binds the toggle in the mode's keymap, and in the page's.
        reg.keymap(LINK_KEYS)
            .bind(State::Normal, "<CR>", "markdown-follow-link");

        // The pages are drawn with what your options say.
        {
            let mut kit = self.kit.borrow_mut();
            kit.colours = self.options.palette;
            kit.front_matter = self.options.front_matter;
            kit.own_fonts = self.options.fonts.clone();
        }
        use State::Normal;
        for (name, doc, keys, act) in MOVES {
            reg.command(name, doc, move |ctx, a| {
                ctx.with_ext(|m: &mut Markdown, ctx| m.on_page(ctx, a, act))?
            });
            for key in keys.split_whitespace() {
                reg.kind_keymap(PAGE).bind(Normal, key, name);
            }
        }
        reg.ext_command(
            "markdown-page-forward",
            "Scroll the page down a window, less an eighth to keep your place, or up with Shift held on Space. A count repeats.",
            Markdown::forward,
        );
        reg.ext_command(
            "markdown-page-older",
            "Go back to where the last link was followed from, on this page or the page it led from. A count repeats.",
            |m: &mut Markdown, ctx, a| m.travel(ctx, a, true),
        );
        reg.ext_command(
            "markdown-page-newer",
            "Go forward again, to where markdown-page-older came back from. A count repeats.",
            |m: &mut Markdown, ctx, a| m.travel(ctx, a, false),
        );
        // A count, which a surface's keys get from no one else: die-vim's digits are the text
        // kind's.
        reg.command(
            "markdown-page-digit",
            "A digit of the count for the page's next key.",
            |ctx, a| {
                let digit = a.char().and_then(|c| c.to_digit(10)).unwrap_or(0) as usize;
                let count = a
                    .count
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(digit);
                ctx.set_prefix(Prefix {
                    count: Some(count),
                    register: a.register,
                });
                Ok(())
            },
        );
        // `C-i` is `<Tab>` to a keymap, as in GUI Emacs and Vim, and `<Tab>` is the links', so
        // forward is `L` and back is `H` too, as a browser's Vim keys have them.
        let k = reg.kind_keymap(PAGE);
        for key in ["<C-f>", "<Space>", "<PageDown>"] {
            k.bind(Normal, key, "markdown-page-forward");
        }
        k.bind(Normal, "<CR>", "markdown-follow-link")
            .bind(Normal, "<C-o>", "markdown-page-older")
            .bind(Normal, "H", "markdown-page-older")
            .bind(Normal, "L", "markdown-page-newer");
        for digit in 0..10 {
            k.bind(Normal, &digit.to_string(), "markdown-page-digit");
        }
        // The keys a buffer can't do without, bound as die-jellyfish binds them for its own
        // surface. There's nothing to write here, so ZZ is ZQ.
        k.bind(Normal, "ZZ", "vim-quit")
            .bind(Normal, "ZQ", "vim-quit")
            .bind(Normal, "<C-^>", "vim-alternate-buffer")
            .bind(Normal, ":", "vim-ex");
        for (keys, name) in WINDOW_KEYS {
            for key in keys.split_whitespace() {
                k.bind(Normal, &format!("<C-w>{key}"), name);
            }
        }
    }

    /// The two things a view in place follows: its text, which a reload from disk changes under
    /// it, and its width, which a command can change with no window of its own changing size
    /// (`\z`, or another window leaving the buffer). Each rendered buffer costs a comparison
    /// after a command, and a raw one nothing. A page follows its file's text the same way, and
    /// after a command it's where the theme's colours are compared and where a page no window
    /// shows any more is noticed; with no page, that's one look at an empty list.
    fn init(&mut self, ctx: &mut Ctx) -> Result {
        ctx.on_edited(|ctx, buf| {
            refresh(ctx, buf)?;
            Ok(ctx.with_ext(|m: &mut Markdown, ctx| m.edited(ctx, buf))?)
        })
        .detach();
        let rendered = self.rendered.clone();
        ctx.on_command_ended(move |ctx| {
            let bufs: Vec<BufferId> = rendered.borrow().iter().map(|b| b.0).collect();
            bufs.into_iter().try_for_each(|buf| refresh(ctx, buf))?;
            Ok(ctx.with_ext(|m: &mut Markdown, ctx| m.settle(ctx))?)
        })
        .detach();
        let killed =
            |ctx: &mut Ctx, buf| Ok(ctx.with_ext(|m: &mut Markdown, ctx| m.killed(ctx, buf))?);
        ctx.on_buffer_killed(killed).detach();
        Ok(())
    }

    /// Which buffers are rendered, so a restart shows them as they were. One whose view is in
    /// place is kept with whether it was read-only before the view made it so: core keeps the
    /// setting as the view has it. One with a page is kept with the page's place, as the byte
    /// of the source its top line shows, and whether the focused window showed it; one whose
    /// page was put away is kept with that place alone, a plain byte. All ride on the file's
    /// buffer, since the session keeps no surface's.
    fn save_session(&self, ctx: &Ctx) -> Kept {
        let rendered = self.rendered.borrow();
        let live = rendered.iter().filter(|b| ctx.is_live(b.0));
        let mut buffers: Vec<_> = live.map(|b| (b.0, Value::Bool(b.1))).collect();
        let focused = ctx.focused_buffer();
        for (file, state) in &self.pages {
            let st = state.borrow();
            let shown = st.buf.is_some() && st.buf == focused;
            buffers.push((*file, json!({ "page": st.kept(), "shown": shown })));
        }
        let away = self.places.iter().filter(|(f, _)| ctx.is_live(**f));
        buffers.extend(away.map(|(f, away)| (*f, json!(away.at))));
        Kept {
            state: Value::Null,
            buffers,
        }
    }

    /// The views as they were. A page is made again and waits for its window to say how large
    /// it is: the host connects its pixels after this, so whether there can be a page is your
    /// option's to say here, and the first `Resize` falls back on the view in place if the host
    /// shows none. Core drops a window whose buffer it didn't keep, a page's among them, so the
    /// page the focused window showed is shown in this one, as a board is. A page put away is
    /// its place, waiting for the file's next page.
    fn restore_session(&mut self, ctx: &mut Ctx, kept: Kept) -> Result {
        let mut show = None;
        for (buf, kept) in kept.buffers {
            // A page put away: with `page` off there's none to start there, and the file was
            // raw.
            if let Some(at) = kept.as_u64() {
                let away = Away {
                    at: at as usize,
                    ring: None,
                };
                if self.options.page {
                    self.places.insert(buf, away);
                }
                continue;
            }
            let place = kept.get("page").and_then(Value::as_u64);
            let back = match place.filter(|_| self.options.page) {
                Some(at) => self.page_for(ctx, buf).map(|state| {
                    let mut st = state.borrow_mut();
                    st.go(ctx, None, Target::Line(at as usize));
                    if kept.get("shown") == Some(&Value::Bool(true)) {
                        show = st.buf;
                    }
                }),
                // A session an earlier build wrote has the buffer's own read-only setting here.
                // One with a page, restored with `page` off, never had its setting touched.
                None => ctx.enable_mode::<View>(buf).and_then(|()| {
                    ctx.with_mode(buf, |v: &mut View, ctx| match kept.as_bool() {
                        Some(read_only) => v.restore(buf, read_only, ctx),
                        None => v.render(buf, ctx),
                    })?
                }),
            };
            if let Err(e) = back {
                ctx.messages.push(format!("markdown: {e}"));
            }
        }
        if let Some(buf) = show {
            ctx.show(buf, ShowIn::Current)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_says_where_it_leads() {
        let file = |p: &str, a: Option<&str>| Dest::File(p.into(), a.map(String::from));
        assert_eq!(Dest::read("#set-up"), Dest::Anchor("set-up".into()));
        assert_eq!(Dest::read("notes.md"), file("notes.md", None));
        assert_eq!(
            Dest::read("../a%20b/c.md#x-y"),
            file("../a b/c.md", Some("x-y"))
        );
        assert_eq!(Dest::read("file:///tmp/a.md"), file("/tmp/a.md", None));
        assert_eq!(Dest::read("C:notes.md"), file("C:notes.md", None));
        assert_eq!(Dest::read("notes.md:12"), file("notes.md:12", None));
        let root = Dest::Root("/docs/a.md".into(), Some("x".into()));
        assert_eq!(Dest::read("/docs/a.md#x"), root);
        for outside in ["https://example.com/a#b", "mailto:you@example.com"] {
            assert_eq!(Dest::read(outside), Dest::Outside(outside.into()));
        }
    }
}
