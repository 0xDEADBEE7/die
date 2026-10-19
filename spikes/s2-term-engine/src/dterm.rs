//! Battery support, ported from the 2026-10-19 [patch] prototype (proposal/patchtest/app/src/lib.rs).
//! The only engine-wiring change: no per-thread `set_png_decoder`; DIE's global hook instead.
//! die-term-shaped prototype: one parser thread per terminal owns the
//! (!Send) libghostty Terminal + RenderState; the "editor" side talks to it
//! over channels and receives plain-data frames (cells + placements) and effects.
use libghostty_vt::{
    fmt::{Format, Formatter, FormatterOptions},
    key, mouse,
    kitty::graphics as kg,
    render::{CellIterator, Dirty, RenderState, RowIterator},
    screen::CellWide,
    style::{RgbColor, StyleColor, Underline},
    terminal::{
        ClipboardLocation, ClipboardReadError, Point, PointCoordinate, SizeReportSize, Terminal,
    },
};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

pub const BOLD: u16 = 1;
pub const ITALIC: u16 = 2;
pub const FAINT: u16 = 4;
pub const BLINK: u16 = 8;
pub const INVERSE: u16 = 16;
pub const INVISIBLE: u16 = 32;
pub const STRIKE: u16 = 64;
pub const OVERLINE: u16 = 128;

/// What DIE's Metal renderer would consume per cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DCell {
    pub text: String,
    pub fg: Option<[u8; 3]>,
    pub bg: Option<[u8; 3]>,
    pub attrs: u16,
    /// 0 none, 1 single, 2 double, 3 curly, 4 dotted, 5 dashed
    pub ul: u8,
    pub ul_color: Option<[u8; 3]>,
    /// 0 narrow, 1 wide, 2 spacer tail, 3 spacer head
    pub wide: u8,
    pub link: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub dirty: u8,
    pub rows: Vec<(u16, Vec<DCell>)>,
    pub cursor: Option<(u16, u16)>,
}

#[derive(Clone, Debug)]
pub struct DPlacement {
    pub image_id: u32,
    pub placement_id: u32,
    pub is_virtual: bool,
    pub z: i32,
    pub viewport: Option<(i32, i32)>,
    pub visible: bool,
    pub grid: (u32, u32),
    pub px: (u32, u32),
    pub src: (u32, u32, u32, u32),
    pub offset: (u32, u32),
    pub img_wh: (u32, u32),
    pub img_fmt: String,
    pub img_gen: u64,
    pub img_bytes: usize,
}

#[derive(Debug, Clone)]
pub enum Effect {
    Pty(Vec<u8>),
    Bell,
    Title(String),
    Pwd(String),
    ClipWrite { loc: String, mime_data: Vec<(String, Vec<u8>)> },
    ClipRead { loc: String },
    Notify { title: String, body: String },
    Progress { state: String, pct: Option<u8> },
    Hold(bool),
}

fn rgb(c: RgbColor) -> [u8; 3] {
    [c.r, c.g, c.b]
}

pub struct Ctx {
    pub term: Terminal<'static, 'static>,
    pub rs: RenderState<'static>,
    pub rows_it: RowIterator<'static>,
    pub cells_it: CellIterator<'static>,
    pub pl_it: kg::PlacementIterator<'static>,
    pub kenc: key::Encoder<'static>,
    pub kev: key::Event<'static>,
    pub menc: mouse::Encoder<'static>,
    pub mev: mouse::Event<'static>,
    pub cell_px: (u32, u32),
    pub clip_reply: Option<Vec<u8>>, // policy for OSC 52 reads (None = deny)
}

impl Ctx {
    pub fn new(cols: u16, rows: u16, cw: u32, ch: u32, fx: Sender<Effect>) -> Ctx {
        // DIE's process-global PNG hook (G10). Idempotent; never the wrapper's thread-local.
        crate::hook::install();
        let mut term = Terminal::new(cols, rows).unwrap();
        // Defaults are a 10 KB byte cap (about one page) and no line cap: set both.
        term.set_scrollback_max_lines(Some(10_000)).unwrap();
        term.set_scrollback_max_bytes(Some(64 << 20)).unwrap();
        // Needed before any input or snapshot-encode of a mid-sequence parser fails.
        term.set_continuation_max_bytes(4096).unwrap();
        // Ghostty app default (grapheme-width-method=unicode): mode 2027 on.
        term.set_default_mode(libghostty_vt::terminal::Mode::GRAPHEME_CLUSTER, true).unwrap();
        term.resize(cols, rows, cw, ch).unwrap();
        term.set_kitty_image_storage_limit(64 * 1024 * 1024).unwrap();
        let t = fx.clone();
        term.on_pty_write(move |_, d| {
            let _ = t.send(Effect::Pty(d.to_vec()));
        })
        .unwrap();
        let t = fx.clone();
        term.on_bell(move |_| {
            let _ = t.send(Effect::Bell);
        })
        .unwrap();
        let t = fx.clone();
        term.on_title_changed(move |term| {
            let _ = t.send(Effect::Title(term.title().unwrap_or("").to_string()));
        })
        .unwrap();
        let t = fx.clone();
        term.on_pwd_changed(move |term| {
            let _ = t.send(Effect::Pwd(term.pwd().unwrap_or("").to_string()));
        })
        .unwrap();
        term.on_xtversion(|_| Some("DIE(0.1.0)")).unwrap();
        // XTWINOPS 14/16/18 in *physical* pixels: answer from the terminal's own size.
        term.on_size(|term| {
            Some(SizeReportSize {
                rows: term.rows().ok()?,
                columns: term.cols().ok()?,
                cell_width: term.width_px().ok()? / u32::from(term.cols().ok()?),
                cell_height: term.height_px().ok()? / u32::from(term.rows().ok()?),
            })
        })
        .unwrap();
        let t = fx.clone();
        term.on_clipboard_write(move |_, w| {
            let loc = format!("{:?}", w.location());
            let mime_data = w
                .contents()
                .map(|c| (c.mime.to_string(), c.data.to_vec()))
                .collect();
            let _ = t.send(Effect::ClipWrite { loc, mime_data });
            w.reply(Ok(()), false);
        })
        .unwrap();
        let t = fx.clone();
        term.on_clipboard_read(move |_, r| {
            let _ = t.send(Effect::ClipRead { loc: format!("{:?}", r.location()) });
            // Synchronous: this blocks only this terminal's parser thread.
            r.reply(Err(ClipboardReadError::Denied), &[], false);
        })
        .unwrap();
        let t = fx.clone();
        term.on_desktop_notification(move |_, n| {
            let _ = t.send(Effect::Notify {
                title: String::from_utf8_lossy(n.title()).into_owned(),
                body: String::from_utf8_lossy(n.body()).into_owned(),
            });
        })
        .unwrap();
        let t = fx.clone();
        term.on_progress_report(move |_, p| {
            let _ = t.send(Effect::Progress {
                state: format!("{:?}", p.state()),
                pct: p.progress(),
            });
        })
        .unwrap();
        let t = fx;
        term.on_render_hold(move |_, held| {
            let _ = t.send(Effect::Hold(held));
        })
        .unwrap();
        let _ = ClipboardLocation::Standard;
        Ctx {
            term,
            rs: RenderState::new().unwrap(),
            rows_it: RowIterator::new().unwrap(),
            cells_it: CellIterator::new().unwrap(),
            pl_it: kg::PlacementIterator::new().unwrap(),
            kenc: key::Encoder::new().unwrap(),
            kev: key::Event::new().unwrap(),
            menc: mouse::Encoder::new().unwrap(),
            mev: mouse::Event::new().unwrap(),
            cell_px: (cw, ch),
            clip_reply: None,
        }
    }

    pub fn write(&mut self, b: &[u8]) {
        self.term.vt_write(b);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (cw, ch) = self.cell_px;
        self.term.resize(cols, rows, cw, ch).unwrap();
    }

    /// Render-state -> DIE cells. `all=false` returns dirty rows only.
    pub fn frame(&mut self, all: bool) -> Frame {
        let Ctx { term, rs, rows_it, cells_it, .. } = self;
        let snap = rs.update(term).unwrap();
        let dirty = match snap.dirty().unwrap() {
            Dirty::Clean => 0,
            Dirty::Partial => 1,
            Dirty::Full => 2,
        };
        let colors = snap.colors().unwrap();
        let cursor = snap.cursor_viewport().unwrap().map(|c| (c.x, c.y));
        let mut out = Frame { dirty, rows: Vec::new(), cursor };
        {
            let mut ri = rows_it.update(&snap).unwrap();
            let mut y_all = 0u16;
            loop {
                let (y, row) = if all {
                    match ri.next() {
                        Some(r) => {
                            y_all += 1;
                            (y_all - 1, r)
                        }
                        None => break,
                    }
                } else {
                    match ri.next_dirty() {
                        Some((y, r)) => (y, r),
                        None => break,
                    }
                };
                let mut cells = Vec::new();
                let mut ci = cells_it.update(row).unwrap();
                while let Some(c) = ci.next() {
                    let mut d = DCell::default();
                    if c.graphemes_len().unwrap() > 0 {
                        c.graphemes_utf8(&mut d.text).unwrap();
                    }
                    d.fg = c.fg_color().unwrap().map(rgb);
                    d.bg = c.bg_color().unwrap().map(rgb);
                    let raw = c.raw_cell().unwrap();
                    d.wide = match raw.wide().unwrap() {
                        CellWide::Narrow => 0,
                        CellWide::Wide => 1,
                        CellWide::SpacerTail => 2,
                        CellWide::SpacerHead => 3,
                    };
                    d.link = raw.has_hyperlink().unwrap();
                    if c.has_styling().unwrap() {
                        let s = c.style().unwrap();
                        let mut a = 0;
                        for (on, bit) in [
                            (s.bold, BOLD),
                            (s.italic, ITALIC),
                            (s.faint, FAINT),
                            (s.blink, BLINK),
                            (s.inverse, INVERSE),
                            (s.invisible, INVISIBLE),
                            (s.strikethrough, STRIKE),
                            (s.overline, OVERLINE),
                        ] {
                            if on {
                                a |= bit;
                            }
                        }
                        d.attrs = a;
                        d.ul = match s.underline {
                            Underline::None => 0,
                            Underline::Single => 1,
                            Underline::Double => 2,
                            Underline::Curly => 3,
                            Underline::Dotted => 4,
                            Underline::Dashed => 5,
                            _ => 1,
                        };
                        d.ul_color = match s.underline_color {
                            StyleColor::None => None,
                            StyleColor::Rgb(c) => Some(rgb(c)),
                            StyleColor::Palette(i) => Some(rgb(colors.palette[i.0 as usize])),
                        };
                    }
                    cells.push(d);
                }
                if !all {
                    row.set_dirty(false).unwrap();
                }
                out.rows.push((y, cells));
            }
        }
        snap.clean().unwrap();
        out
    }

    pub fn hyperlink_at(&self, x: u16, y: u32) -> Option<String> {
        let r = self.term.grid_ref(Point::Viewport(PointCoordinate { x, y })).ok()?;
        let mut buf = vec![0u8; 4096];
        let n = r.hyperlink_uri(&mut buf).ok()?;
        (n > 0).then(|| String::from_utf8_lossy(&buf[..n]).into_owned())
    }

    pub fn placements(&mut self) -> Vec<DPlacement> {
        let Ctx { term, pl_it, .. } = self;
        let g = term.kitty_graphics().unwrap();
        let mut v = Vec::new();
        let mut it = pl_it.update(&g).unwrap();
        while let Some(p) = it.next() {
            let id = p.image_id().unwrap();
            let Some(img) = g.image(id) else { continue };
            let info = p.placement_render_info(&img, term);
            let (vp, visible, grid, px, src) = match info {
                Ok(i) => (
                    Some((i.viewport_col, i.viewport_row)),
                    i.viewport_visible,
                    (i.grid_cols, i.grid_rows),
                    (i.pixel_width, i.pixel_height),
                    (i.source_x, i.source_y, i.source_width, i.source_height),
                ),
                Err(_) => (None, false, (0, 0), (0, 0), (0, 0, 0, 0)),
            };
            let grid = if grid == (0, 0) {
                p.grid_size(&img, term).map(|g| (g.cols, g.rows)).unwrap_or((0, 0))
            } else {
                grid
            };
            v.push(DPlacement {
                image_id: id,
                placement_id: p.placement_id().unwrap(),
                is_virtual: p.is_virtual().unwrap(),
                z: p.z().unwrap(),
                viewport: vp,
                visible,
                grid,
                px,
                src,
                offset: (p.x_offset().unwrap(), p.y_offset().unwrap()),
                img_wh: (img.width().unwrap(), img.height().unwrap()),
                img_fmt: format!("{:?}", img.format().unwrap()),
                img_gen: img.generation().unwrap(),
                img_bytes: img.data().unwrap().map(|d| d.len()).unwrap_or(0),
            });
        }
        v
    }

    pub fn key(
        &mut self,
        k: key::Key,
        mods: key::Mods,
        action: key::Action,
        utf8: Option<&str>,
        unshifted: char,
    ) -> Vec<u8> {
        self.kenc.set_options_from_terminal(&self.term);
        self.kenc.set_macos_option_as_alt(key::OptionAsAlt::True);
        self.kev
            .set_key(k)
            .set_mods(mods)
            .set_action(action)
            .set_consumed_mods(if utf8.is_some() && mods.contains(key::Mods::SHIFT) {
                key::Mods::SHIFT
            } else {
                key::Mods::empty()
            })
            .set_utf8(utf8.map(|s| s.to_string()))
            .set_unshifted_codepoint(unshifted);
        let mut v = Vec::new();
        self.kenc.encode_to_vec(&self.kev, &mut v).unwrap();
        v
    }

    /// x,y in physical pixels relative to the surface origin.
    pub fn mouse(
        &mut self,
        action: mouse::Action,
        button: Option<mouse::Button>,
        mods: key::Mods,
        x: f32,
        y: f32,
        any_pressed: bool,
    ) -> Vec<u8> {
        let (cw, ch) = self.cell_px;
        let cols = u32::from(self.term.cols().unwrap());
        let rows = u32::from(self.term.rows().unwrap());
        self.menc
            .set_options_from_terminal(&self.term)
            .set_size(mouse::EncoderSize {
                screen_width: cols * cw,
                screen_height: rows * ch,
                cell_width: cw,
                cell_height: ch,
                padding_top: 0,
                padding_bottom: 0,
                padding_right: 0,
                padding_left: 0,
            })
            .set_any_button_pressed(any_pressed);
        self.mev
            .set_action(action)
            .set_button(button)
            .set_mods(mods)
            .set_position(mouse::Position { x, y });
        let mut v = Vec::new();
        self.menc.encode_to_vec(&self.mev, &mut v).unwrap();
        v
    }

    pub fn text(&self, unwrap: bool) -> String {
        let mut f = Formatter::new(
            &self.term,
            FormatterOptions::new().with_format(Format::Plain).with_unwrap(unwrap).with_trim(true),
        )
        .unwrap();
        let b = f.format_alloc(None).unwrap();
        String::from_utf8_lossy(&b).into_owned()
    }
}

type Job = Box<dyn FnOnce(&mut Ctx) + Send>;

/// Editor-side handle; the Terminal lives on its own thread.
pub struct TermThread {
    tx: Option<Sender<Job>>,
    pub fx: Receiver<Effect>,
    h: Option<JoinHandle<()>>,
}

impl TermThread {
    pub fn spawn(cols: u16, rows: u16, cw: u32, ch: u32) -> TermThread {
        let (tx, rx) = mpsc::channel::<Job>();
        let (fx_tx, fx) = mpsc::channel();
        let h = std::thread::Builder::new()
            .name("die-term".into())
            .spawn(move || {
                let mut ctx = Ctx::new(cols, rows, cw, ch, fx_tx);
                while let Ok(job) = rx.recv() {
                    job(&mut ctx);
                }
            })
            .unwrap();
        TermThread { tx: Some(tx), fx, h: Some(h) }
    }

    pub fn run<R: Send + 'static>(&self, f: impl FnOnce(&mut Ctx) -> R + Send + 'static) -> R {
        let (rtx, rrx) = mpsc::sync_channel(1);
        self.tx
            .as_ref()
            .unwrap()
            .send(Box::new(move |c| {
                let _ = rtx.send(f(c));
            }))
            .unwrap();
        rrx.recv().unwrap()
    }

    pub fn write(&self, b: impl Into<Vec<u8>>) {
        let b = b.into();
        self.run(move |c| c.write(&b));
    }

    pub fn drain(&self) -> Vec<Effect> {
        self.fx.try_iter().collect()
    }

    pub fn pty_replies(&self) -> String {
        let mut s = Vec::new();
        for e in self.drain() {
            if let Effect::Pty(b) = e {
                s.extend(b);
            }
        }
        esc(&s)
    }
}

impl Drop for TermThread {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(h) = self.h.take() {
            let _ = h.join();
        }
    }
}

pub fn esc(b: &[u8]) -> String {
    let mut s = String::new();
    for &c in b {
        match c {
            0x1b => s.push_str("\\e"),
            b'\r' => s.push_str("\\r"),
            b'\n' => s.push_str("\\n"),
            b'\t' => s.push_str("\\t"),
            0x20..=0x7e => s.push(c as char),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s
}

pub fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for ch in data.chunks(3) {
        let n = (u32::from(ch[0]) << 16)
            | (u32::from(*ch.get(1).unwrap_or(&0)) << 8)
            | u32::from(*ch.get(2).unwrap_or(&0));
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if ch.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if ch.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

/// Kitty APC, chunked at 4096 base64 bytes (m=1 continuation) like kitten icat.
pub fn kitty_chunked(ctrl: &str, payload: &[u8]) -> Vec<Vec<u8>> {
    let enc = b64(payload);
    let chunks: Vec<&[u8]> = enc.as_bytes().chunks(4096).collect();
    let mut out = Vec::new();
    for (i, c) in chunks.iter().enumerate() {
        let more = if i + 1 < chunks.len() { 1 } else { 0 };
        let head = if i == 0 { format!("{ctrl},m={more}") } else { format!("m={more}") };
        let mut v = format!("\x1b_G{head};").into_bytes();
        v.extend_from_slice(c);
        v.extend_from_slice(b"\x1b\\");
        out.push(v);
    }
    out
}

pub fn png_rgba(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&f(x, y));
        }
    }
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(&px).unwrap();
    }
    out
}
