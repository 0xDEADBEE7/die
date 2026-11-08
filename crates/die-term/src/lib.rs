//! die-term's engine side: one terminal of Ghostty's engine, libghostty-vt, with bytes in and
//! cells out, keys in and bytes out (scope §6, D10). A [`Term`] needs no `Ctx` and no PTY, so it
//! runs inside DIE now and in the terminal's helper process later (D20). The surface on a PTY
//! is built on it.
//!
//! What a program sends goes to [`Term::feed`], which parses a bounded amount per call, in bytes
//! and in time.
//! The engine's answers to queries collect for the PTY ([`Term::take_replies`]), and what it
//! asks of DIE collects as [`Event`]s ([`Term::take_events`]). [`Term::frame`] turns the screen,
//! or a scrolled-back viewport, into rows of die-core cells. Keys, the pointer, pastes and focus
//! become bytes for the PTY ([`Term::key`], [`Term::mouse`], [`Term::paste`], [`Term::focus`]).
//! [`Term::modes`] says what the surface routes by: the alternate screen (scope D18's Esc), mouse
//! tracking and bracketed paste. [`Term::text`] is copy state's text (D19, scope §5.5), which
//! [`CopyText`] gives core's snapshot whole, then as it follows new output.
//!
//! The engine's defaults that DIE overrides are the record's (docs/records/die-term-engine.md
//! §2): 10,000 lines of scrollback capped at 64 MB, mode 2027, Option as Alt, set colors, and
//! DIE's own identity.
//!
//! The terminal buffer is a [`TermSurface`]: a `Term` on one of core's PTYs, with D18's insert
//! and normal states. [`Terminals`] is the extension that opens them (`term`), and [`open`] runs
//! any program in one.

#[cfg(test)]
use die_test_cap as _;

mod commands;
mod copy;
mod input;
mod screen;
mod surface;
mod text;

pub use commands::{NAME, Spec, Terminals, busy, open, to_clipboard};
pub use copy::CopyText;
pub use input::{PasteRefused, Pointer, PointerAction, PointerButton};
pub use screen::{Frame, TermCursor};
pub use surface::{CopyFn, Escape, KIND, TermSurface};
pub use text::{Line, Mark, Text};

use die_core::Rgb;
use libghostty_vt::style::{Palette, RgbColor};
use libghostty_vt::terminal::{
    ClipboardLocation, ClipboardReadError, ConformanceLevel, DeviceAttributeFeature,
    DeviceAttributes, DeviceType, Mode, PrimaryDeviceAttributes, ScrollViewport,
    SecondaryDeviceAttributes, SizeReportSize, TertiaryDeviceAttributes,
};
use libghostty_vt::{Terminal, screen::Screen};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Lines of scrollback per terminal, as in vterm (scope §6).
pub const SCROLLBACK_LINES: usize = 10_000;
/// The cap on a terminal's scrollback, in bytes of the engine's pages.
pub const SCROLLBACK_BYTES: usize = 64 << 20;
/// The cap on a screen's images (scope §6), once DIE draws kitty images. Until then the engine's
/// limit is 0, which turns kitty graphics off: a program's query gets no answer, so it shows its
/// text instead of images nothing would draw.
pub const IMAGE_BYTES: u64 = 64 << 20;
/// The most bytes one [`Term::feed`] parses: 0.22 ms of a build log's flood (0.4 ms at most)
/// on an M5 Pro.
pub const FEED_MAX: usize = 64 << 10;
/// How long one [`Term::feed`] parses before it returns. It parses [`FEED_SLICE`] at a time and
/// stops after the slice that passes this, so output crafted to be slow (REP, DECALN, ED, RIS:
/// up to 5 µs a byte) holds the caller for a few ms, not the 0.3 s 64 KiB of it takes.
pub const FEED_BUDGET: Duration = Duration::from_millis(2);
/// What [`Term::feed`] parses at once: 1 KiB, at most about 5 ms of the slowest output. The
/// lines a slice pushes into the scrollback are counted after it, so no ordinary flood can push
/// out the row the count follows (see [`Term::added_since`]).
pub const FEED_SLICE: usize = 1 << 10;
/// The most replies held for the PTY. Past it, new ones are dropped: a program that sends
/// queries faster than it reads would lose them anyway.
pub const REPLIES_MAX: usize = 1 << 20;
/// What XTVERSION answers. Not "ghostty": yazi matches that as a substring and moves to
/// Unicode-placeholder images (docs/records/die-term-engine.md §9).
pub const XTVERSION: &str = concat!("DIE ", env!("CARGO_PKG_VERSION"));
/// The terminal name, as `TERM` has it and XTGETTCAP reports (scope §6).
pub const TERM_NAME: &str = "xterm-256color";
/// How long a program may hold the screen with synchronized output (mode 2026) before the
/// frame goes out anyway, as Ghostty's own default.
pub const HOLD_TIMEOUT: Duration = Duration::from_secs(1);

/// What the engine asks of DIE while it parses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The title changed (OSC 0 or 2).
    Title(String),
    /// The working directory changed, as the program sent it: a `file://` URL (OSC 7), or a
    /// bare path (ConEmu's OSC 9;9).
    Pwd(String),
    /// BEL.
    Bell,
    /// The program wrote to a clipboard (OSC 52). Reads are always denied, inside the engine.
    Clipboard {
        /// The X11 primary or selection clipboard rather than the standard one.
        primary: bool,
        /// The text.
        text: String,
    },
    /// A desktop notification (OSC 9 or 777).
    Notify {
        /// Its title; empty for OSC 9.
        title: String,
        /// Its body.
        body: String,
    },
}

/// How a program tracks the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseTracking {
    /// It doesn't: the pointer selects and the wheel scrolls.
    None,
    /// Presses only (mode 9).
    X10,
    /// Presses and releases (mode 1000).
    Normal,
    /// And motion while a button is held (mode 1002).
    Button,
    /// And all motion (mode 1003).
    Any,
}

/// How mouse reports are encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseFormat {
    /// X10's bytes.
    X10,
    /// UTF-8 coordinates (mode 1005).
    Utf8,
    /// SGR (mode 1006).
    Sgr,
    /// urxvt (mode 1015).
    Urxvt,
    /// SGR in pixels (mode 1016).
    SgrPixels,
}

/// The modes a terminal's surface routes input by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modes {
    /// The program is on the alternate screen (vim, less, htop): Esc in insert goes to it
    /// (scope D18), and the wheel may send arrow keys.
    pub alt_screen: bool,
    /// How the program tracks the mouse.
    pub mouse: MouseTracking,
    /// How its reports are encoded.
    pub mouse_format: MouseFormat,
    /// It asked for bracketed paste (mode 2004).
    pub bracketed_paste: bool,
    /// It asked for focus reports (mode 1004).
    pub focus_events: bool,
    /// The wheel sends arrow keys on the alternate screen when the mouse isn't tracked (1007).
    pub alt_scroll: bool,
    /// Cursor keys in application mode (DECCKM).
    pub app_cursor_keys: bool,
    /// The kitty keyboard flags the program set (0 is legacy encoding).
    pub kitty_flags: u8,
}

/// Where the viewport is in the scrollback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    /// The absolute row at the viewport's top (see [`Line::row`]).
    pub top: u64,
    /// Rows of scrollback above the active screen.
    pub history: u64,
    /// The viewport follows the active screen.
    pub at_bottom: bool,
}

/// Collected inside the engine's callbacks, which run during `vt_write`, `paste` and `resize`.
/// Nothing holds a borrow across those calls: a panic in a callback aborts the process.
#[derive(Default)]
struct Outbox {
    pty: Vec<u8>,
    /// A paste is writing: its bytes aren't replies, and [`REPLIES_MAX`] doesn't hold them.
    pasting: bool,
    events: Vec<Event>,
    hold: Option<bool>,
}

/// One terminal: the engine, its render state and encoders, and what they've produced.
pub struct Term {
    term: Terminal<'static, 'static>,
    out: Rc<RefCell<Outbox>>,
    screen: screen::Screen,
    input: input::Input,
    history: text::History,
    cell_px: (u32, u32),
    held: Option<Instant>,
}

/// The engine's Result, for calls that fail only on a bad argument or out of memory.
type EngineResult<T> = Result<T, libghostty_vt::Error>;

fn engine_rgb(c: Rgb) -> RgbColor {
    RgbColor {
        r: c.0,
        g: c.1,
        b: c.2,
    }
}

/// Device attributes: DA1 as Ghostty's app answers it (`62;22`, level 2 with ANSI color), DA2
/// a VT220, DA3 unit 0.
const DA: DeviceAttributes = DeviceAttributes {
    primary: PrimaryDeviceAttributes::new(
        ConformanceLevel::LEVEL_2,
        &[DeviceAttributeFeature::ANSI_COLOR],
    ),
    secondary: SecondaryDeviceAttributes {
        device_type: DeviceType::VT220,
        firmware_version: 0,
        rom_cartridge: 0,
    },
    tertiary: TertiaryDeviceAttributes { unit_id: 0 },
};

impl Term {
    /// A terminal of `cols` by `rows` cells, each `cell_px` physical pixels (width, height), in
    /// die-core's fallback colors until [`Term::set_colors`] gives the theme's.
    pub fn new(cols: u16, rows: u16, cell_px: (u32, u32)) -> Result<Term, String> {
        let cell_px = (cell_px.0.max(1), cell_px.1.max(1));
        Term::build(cols.max(1), rows.max(1), cell_px).map_err(|e| format!("die-term: {e}"))
    }

    fn build(cols: u16, rows: u16, cell_px: (u32, u32)) -> EngineResult<Term> {
        let mut term = Terminal::new(cols, rows)?;
        term.set_scrollback_max_lines(Some(SCROLLBACK_LINES))?
            .set_scrollback_max_bytes(Some(SCROLLBACK_BYTES))?
            // Without it, a snapshot or a sequence split across reads fails to continue.
            .set_continuation_max_bytes(4096)?
            // Skin-tone emoji and other clusters in one cell, as Ghostty's app has it.
            .set_default_mode(Mode::GRAPHEME_CLUSTER, true)?
            .set_kitty_image_storage_limit(0)?
            .set_terminfo_name(TERM_NAME)?;
        term.resize(cols, rows, cell_px.0, cell_px.1)?;
        let out = Rc::new(RefCell::new(Outbox::default()));
        let o = out.clone();
        term.on_pty_write(move |_, d| {
            let mut o = o.borrow_mut();
            if o.pasting || o.pty.len() + d.len() <= REPLIES_MAX {
                o.pty.extend_from_slice(d);
            }
        })?;
        let o = out.clone();
        term.on_bell(move |_| o.borrow_mut().events.push(Event::Bell))?;
        let o = out.clone();
        term.on_title_changed(move |t| {
            let title = t.title().unwrap_or_default().to_string();
            o.borrow_mut().events.push(Event::Title(title));
        })?;
        let o = out.clone();
        term.on_pwd_changed(move |t| {
            let pwd = t.pwd().unwrap_or_default().to_string();
            o.borrow_mut().events.push(Event::Pwd(pwd));
        })?;
        term.on_xtversion(|_| Some(XTVERSION))?;
        term.on_device_attributes(|_| Some(DA))?;
        // XTWINOPS 14, 16 and 18 in physical pixels, from the size `resize` gave.
        term.on_size(|t| {
            let (cols, rows) = (t.cols().ok()?, t.rows().ok()?);
            Some(SizeReportSize {
                rows,
                columns: cols,
                cell_width: t.width_px().ok()? / u32::from(cols.max(1)),
                cell_height: t.height_px().ok()? / u32::from(rows.max(1)),
            })
        })?;
        let o = out.clone();
        term.on_clipboard_write(move |_, w| {
            let primary = w.location() != ClipboardLocation::Standard;
            let text = w
                .contents()
                .find(|c| c.mime.starts_with("text/plain"))
                .map(|c| String::from_utf8_lossy(c.data).into_owned());
            if let Some(text) = text {
                o.borrow_mut()
                    .events
                    .push(Event::Clipboard { primary, text });
            }
            // The reply may write to the PTY, through the callback above.
            w.reply(Ok(()), false);
        })?;
        // A read is answered here, inside the engine's synchronous callback, so DIE is never
        // asked and never waited on (the record's deadlock rule).
        term.on_clipboard_read(|_, r| r.reply(Err(ClipboardReadError::Denied), &[], false))?;
        let o = out.clone();
        term.on_desktop_notification(move |_, n| {
            let title = String::from_utf8_lossy(n.title()).into_owned();
            let body = String::from_utf8_lossy(n.body()).into_owned();
            o.borrow_mut().events.push(Event::Notify { title, body });
        })?;
        let o = out.clone();
        term.on_render_hold(move |_, held| o.borrow_mut().hold = Some(held))?;
        let history = text::History::new(&term)?;
        let mut t = Term {
            term,
            out,
            screen: screen::Screen::new()?,
            input: input::Input::new()?,
            history,
            cell_px,
            held: None,
        };
        t.set_colors(DEFAULT_FG, DEFAULT_BG, None, &engine_ansi());
        Ok(t)
    }

    /// Set the default foreground, background and cursor colors and the 16 ANSI colors, from
    /// the theme. OSC 10, 11 and 4 queries answer with them. The next frame is full.
    pub fn set_colors(&mut self, fg: Rgb, bg: Rgb, cursor: Option<Rgb>, ansi: &[Rgb; 16]) {
        let mut palette = Palette::default();
        for (slot, c) in palette.0.iter_mut().zip(ansi) {
            *slot = engine_rgb(*c);
        }
        let _ = self.term.set_default_fg_color(Some(engine_rgb(fg)));
        let _ = self.term.set_default_bg_color(Some(engine_rgb(bg)));
        let _ = self.term.set_default_cursor_color(cursor.map(engine_rgb));
        let _ = self.term.set_default_color_palette(Some(palette));
        self.screen.redraw();
    }

    /// Parse what the program sent: at most [`FEED_MAX`] bytes of `bytes`, and no more once
    /// [`FEED_BUDGET`] has gone. Returns how many it took; feed the rest on a later call, and to
    /// keep a turn short, on a later turn. A sequence split between calls continues.
    pub fn feed(&mut self, bytes: &[u8]) -> usize {
        let start = Instant::now();
        let max = bytes.len().min(FEED_MAX);
        let mut n = 0;
        while n < max {
            let end = max.min(n + FEED_SLICE);
            self.term.vt_write(&bytes[n..end]);
            n = end;
            self.history.update(&mut self.term);
            if start.elapsed() >= FEED_BUDGET {
                break;
            }
        }
        let hold = self.out.borrow_mut().hold.take();
        match hold {
            Some(true) => self.held = Some(Instant::now()),
            Some(false) => self.held = None,
            None => {}
        }
        n
    }

    /// What the engine wrote for the PTY since the last call: its answers to queries (DA, DSR,
    /// XTVERSION, XTWINOPS, mode 2048's size reports), up to [`REPLIES_MAX`]. Key, mouse, paste
    /// and focus bytes are returned by their own calls instead.
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out.borrow_mut().pty)
    }

    /// What the engine asked of DIE since the last call, in order.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.out.borrow_mut().events)
    }

    /// Resize to `cols` by `rows` cells of `cell_px` physical pixels. The primary screen
    /// reflows; marks from before it no longer count (see [`Term::added_since`]), and the next
    /// frame is full. A program that asked for in-band size reports (mode 2048) gets one, in
    /// [`Term::take_replies`].
    pub fn resize(&mut self, cols: u16, rows: u16, cell_px: (u32, u32)) {
        let (cols, rows) = (cols.max(1), rows.max(1));
        self.cell_px = (cell_px.0.max(1), cell_px.1.max(1));
        let _ = self.term.resize(cols, rows, self.cell_px.0, self.cell_px.1);
        if self.out.borrow_mut().hold.take() == Some(false) {
            self.held = None;
        }
        self.history.restart(&mut self.term);
        self.screen.redraw();
    }

    /// The size in cells: (columns, rows).
    pub fn size(&self) -> (u16, u16) {
        (self.term.cols().unwrap_or(1), self.term.rows().unwrap_or(1))
    }

    /// A cell's size in physical pixels: (width, height).
    pub fn cell_px(&self) -> (u32, u32) {
        self.cell_px
    }

    /// The title the program set (OSC 0 or 2), or "".
    pub fn title(&self) -> &str {
        self.term.title().unwrap_or_default()
    }

    /// The working directory the program reported, as [`Event::Pwd`] has it, or "".
    pub fn pwd(&self) -> &str {
        self.term.pwd().unwrap_or_default()
    }

    /// The modes a surface routes input by.
    pub fn modes(&self) -> Modes {
        let on = |m: Mode| self.term.mode(m).unwrap_or(false);
        let mouse = if on(Mode::ANY_MOUSE) {
            MouseTracking::Any
        } else if on(Mode::BUTTON_MOUSE) {
            MouseTracking::Button
        } else if on(Mode::NORMAL_MOUSE) {
            MouseTracking::Normal
        } else if on(Mode::X10_MOUSE) {
            MouseTracking::X10
        } else {
            MouseTracking::None
        };
        let mouse_format = if on(Mode::SGR_PIXELS_MOUSE) {
            MouseFormat::SgrPixels
        } else if on(Mode::SGR_MOUSE) {
            MouseFormat::Sgr
        } else if on(Mode::URXVT_MOUSE) {
            MouseFormat::Urxvt
        } else if on(Mode::UTF8_MOUSE) {
            MouseFormat::Utf8
        } else {
            MouseFormat::X10
        };
        Modes {
            alt_screen: self.alt_screen(),
            mouse,
            mouse_format,
            bracketed_paste: on(Mode::BRACKETED_PASTE),
            focus_events: on(Mode::FOCUS_EVENT),
            alt_scroll: on(Mode::ALT_SCROLL),
            app_cursor_keys: on(Mode::DECCKM),
            kitty_flags: self.term.kitty_keyboard_flags().map_or(0, |f| f.bits()),
        }
    }

    /// The program is on the alternate screen.
    pub fn alt_screen(&self) -> bool {
        matches!(self.term.active_screen(), Ok(Screen::Alternate))
    }

    /// The changed rows of the viewport as die-core cells, with the cursor; every row when
    /// `full`, or after a resize or a color change. None when nothing changed, and while the
    /// program holds the screen with synchronized output, for up to [`HOLD_TIMEOUT`]; then the
    /// hold ends. A full frame shows what's there even during a hold, and leaves it held.
    pub fn frame(&mut self, full: bool) -> Option<Frame> {
        if let Some(since) = self.held {
            if since.elapsed() >= HOLD_TIMEOUT {
                let _ = self.term.set_mode(Mode::SYNC_OUTPUT, false);
                self.held = None;
            } else if !full {
                return None;
            }
        }
        self.screen.frame(&self.term, full)
    }

    /// Scroll the viewport by `rows`, up into the scrollback when negative.
    pub fn scroll(&mut self, rows: isize) {
        self.term.scroll_viewport(ScrollViewport::Delta(rows));
    }

    /// Show the active screen again, as typing does.
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_viewport(ScrollViewport::Bottom);
    }

    /// Show the viewport with absolute row `row` (see [`Line::row`]) at its top, as far as the
    /// scrollback allows.
    pub fn scroll_to_row(&mut self, row: u64) {
        let screen_row = row.saturating_sub(self.history.pruned());
        let screen_row = usize::try_from(screen_row).unwrap_or(usize::MAX);
        self.term.scroll_viewport(ScrollViewport::Row(screen_row));
    }

    /// Where the viewport is.
    pub fn viewport(&self) -> Viewport {
        let bar = self.term.scrollbar().ok();
        Viewport {
            top: self.history.pruned() + bar.map_or(0, |b| b.offset),
            history: self.term.scrollback_rows().unwrap_or(0) as u64,
            at_bottom: self.term.viewport_active().unwrap_or(true),
        }
    }
}

/// die-core's fallback colors, until the theme's are set.
const DEFAULT_FG: Rgb = Rgb(0xd0, 0xd0, 0xd0);
const DEFAULT_BG: Rgb = Rgb(0x10, 0x10, 0x10);

/// The engine's own first 16 colors.
fn engine_ansi() -> [Rgb; 16] {
    let p = Palette::default();
    std::array::from_fn(|i| Rgb(p.0[i].r, p.0[i].g, p.0[i].b))
}
