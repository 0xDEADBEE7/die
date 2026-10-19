//! The engine side of one terminal: owns every !Send engine object. Runs on the parser
//! thread (in-process shape) or in the helper process. Shape-independent.
use crate::proto::{Cell, Effect, FrameDelta, GRAPHEME, ImageUpload, Pixels, Placement, Row};
use libghostty_vt::{
    fmt::{Format, Formatter, FormatterOptions},
    key, kitty::graphics as kg, mouse,
    render::{CellIterator, Dirty, RenderState, RowIterator},
    screen::CellWide,
    style::{RgbColor, StyleColor, Underline},
    terminal::{ClipboardReadError, Mode, Point, PointCoordinate, SizeReportSize, Terminal},
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct Cfg {
    pub cols: u16,
    pub rows: u16,
    pub cell_w: u32,
    pub cell_h: u32,
}

/// Collected inside engine callbacks (which run synchronously inside vt_write).
#[derive(Default)]
pub struct Outbox {
    pub pty: Vec<u8>,
    pub effects: Vec<Effect>,
    pub hold: Option<bool>,
}

pub struct Engine {
    pub term: Terminal<'static, 'static>,
    rs: RenderState<'static>,
    rows_it: RowIterator<'static>,
    cells_it: CellIterator<'static>,
    pl_it: kg::PlacementIterator<'static>,
    kenc: key::Encoder<'static>,
    kev: key::Event<'static>,
    menc: mouse::Encoder<'static>,
    mev: mouse::Event<'static>,
    pub cfg: Cfg,
    pub out: Rc<RefCell<Outbox>>,
    graphemes: HashMap<String, u32>,
    new_graphemes: Vec<(u32, String)>,
    links: HashMap<String, u32>,
    new_links: Vec<(u32, String)>,
    /// image id -> generation already handed to DIE
    pub images_sent: HashMap<u32, u64>,
    last_placements: Vec<Placement>,
    seq: u64,
    pub held: bool,
    gbuf: Vec<char>,
    sbuf: String,
}

fn rgb(c: RgbColor) -> u32 {
    0x0100_0000 | (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32
}

impl Engine {
    /// G1 subset: DIE's engine defaults and callbacks.
    pub fn new(cfg: Cfg) -> Engine {
        crate::hook::install(); // process-global; no-op after the first call
        let mut term = Terminal::new(cfg.cols, cfg.rows).unwrap();
        term.set_scrollback_max_lines(Some(10_000)).unwrap();
        term.set_scrollback_max_bytes(Some(64 << 20)).unwrap();
        term.set_continuation_max_bytes(4096).unwrap();
        term.set_default_mode(Mode::GRAPHEME_CLUSTER, true).unwrap();
        term.resize(cfg.cols, cfg.rows, cfg.cell_w, cfg.cell_h).unwrap();
        term.set_kitty_image_storage_limit(64 << 20).unwrap();
        let out = Rc::new(RefCell::new(Outbox::default()));
        let o = out.clone();
        term.on_pty_write(move |_, d| o.borrow_mut().pty.extend_from_slice(d)).unwrap();
        let o = out.clone();
        term.on_bell(move |_| o.borrow_mut().effects.push(Effect::Bell)).unwrap();
        let o = out.clone();
        term.on_title_changed(move |t| o.borrow_mut().effects.push(Effect::Title(t.title().unwrap_or("").to_string())))
            .unwrap();
        let o = out.clone();
        term.on_pwd_changed(move |t| o.borrow_mut().effects.push(Effect::Pwd(t.pwd().unwrap_or("").to_string())))
            .unwrap();
        term.on_xtversion(|_| Some("DIE(0.0.0-s2)")).unwrap();
        term.on_size(|t| {
            Some(SizeReportSize {
                rows: t.rows().ok()?,
                columns: t.cols().ok()?,
                cell_width: t.width_px().ok()? / u32::from(t.cols().ok()?),
                cell_height: t.height_px().ok()? / u32::from(t.rows().ok()?),
            })
        })
        .unwrap();
        let o = out.clone();
        term.on_clipboard_write(move |_, w| {
            let data = w.contents().find(|c| c.mime.starts_with("text/plain")).map(|c| c.data.to_vec());
            o.borrow_mut().effects.push(Effect::ClipboardWrite(data.unwrap_or_default()));
            w.reply(Ok(()), false);
        })
        .unwrap();
        // Deadlock rule: the synchronous read callback replies here, on the engine side.
        // Denied by default; DIE is never asked and never waited on.
        term.on_clipboard_read(|_, r| r.reply(Err(ClipboardReadError::Denied), &[], false)).unwrap();
        let o = out.clone();
        term.on_desktop_notification(move |_, n| {
            o.borrow_mut().effects.push(Effect::Notify {
                title: String::from_utf8_lossy(n.title()).into_owned(),
                body: String::from_utf8_lossy(n.body()).into_owned(),
            })
        })
        .unwrap();
        let o = out.clone();
        term.on_render_hold(move |_, held| o.borrow_mut().hold = Some(held)).unwrap();
        Engine {
            term,
            rs: RenderState::new().unwrap(),
            rows_it: RowIterator::new().unwrap(),
            cells_it: CellIterator::new().unwrap(),
            pl_it: kg::PlacementIterator::new().unwrap(),
            kenc: key::Encoder::new().unwrap(),
            kev: key::Event::new().unwrap(),
            menc: mouse::Encoder::new().unwrap(),
            mev: mouse::Event::new().unwrap(),
            cfg,
            out,
            graphemes: HashMap::new(),
            new_graphemes: Vec::new(),
            links: HashMap::new(),
            new_links: Vec::new(),
            images_sent: HashMap::new(),
            last_placements: Vec::new(),
            seq: 0,
            held: false,
            gbuf: Vec::with_capacity(8),
            sbuf: String::new(),
        }
    }

    pub fn write(&mut self, b: &[u8]) {
        self.term.vt_write(b);
        if let Some(h) = self.out.borrow_mut().hold.take() {
            self.held = h;
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cw: u32, ch: u32) {
        self.cfg = Cfg { cols, rows, cell_w: cw, cell_h: ch };
        self.term.resize(cols, rows, cw, ch).unwrap();
    }

    /// Forget what DIE has (reattach): the next frame is full and every table is resent.
    pub fn reset_sync(&mut self) {
        self.new_graphemes = self.graphemes.iter().map(|(s, i)| (*i, s.clone())).collect();
        self.new_links = self.links.iter().map(|(s, i)| (*i, s.clone())).collect();
        self.images_sent.clear();
        self.last_placements.clear();
    }

    /// G2: render state to DIE cells. `full` includes every row; otherwise dirty rows only.
    /// Returns None when nothing changed.
    pub fn frame(&mut self, full: bool, t_read: u64) -> Option<FrameDelta> {
        let placements = self.placements_if_changed();
        let snap = self.rs.update(&self.term).unwrap();
        let dirty = snap.dirty().unwrap();
        if !full && matches!(dirty, Dirty::Clean) && placements.is_none() {
            return None;
        }
        let colors = snap.colors().unwrap();
        let cursor = snap.cursor_viewport().unwrap().map(|c| (c.x, c.y));
        let mut rows = Vec::new();
        let mut link_rows = Vec::new();
        {
            let mut ri = self.rows_it.update(&snap).unwrap();
            let mut y_all = 0u16;
            loop {
                let (y, row) = if full || matches!(dirty, Dirty::Full) {
                    match ri.next() {
                        Some(r) => {
                            y_all += 1;
                            (y_all - 1, r)
                        }
                        None => break,
                    }
                } else {
                    match ri.next_dirty() {
                        Some(x) => x,
                        None => break,
                    }
                };
                let mut cells = Vec::with_capacity(self.cfg.cols as usize);
                let mut ci = self.cells_it.update(row).unwrap();
                let mut any_link = false;
                while let Some(c) = ci.next() {
                    let mut d = Cell::default();
                    let n = c.graphemes_len().unwrap();
                    if n == 1 {
                        self.gbuf.resize(1, '\0');
                        c.graphemes_buf(&mut self.gbuf).unwrap();
                        d.ch = self.gbuf[0] as u32;
                    } else if n > 1 {
                        self.gbuf.resize(n, '\0');
                        c.graphemes_buf(&mut self.gbuf).unwrap();
                        self.sbuf.clear();
                        self.sbuf.extend(self.gbuf.iter());
                        let id = match self.graphemes.get(&self.sbuf) {
                            Some(&id) => id,
                            None => {
                                let id = self.graphemes.len() as u32;
                                self.graphemes.insert(self.sbuf.clone(), id);
                                self.new_graphemes.push((id, self.sbuf.clone()));
                                id
                            }
                        };
                        d.ch = GRAPHEME | id;
                    }
                    d.fg = c.fg_color().unwrap().map_or(0, rgb);
                    d.bg = c.bg_color().unwrap().map_or(0, rgb);
                    let raw = c.raw_cell().unwrap();
                    d.wide = match raw.wide().unwrap() {
                        CellWide::Narrow => 0,
                        CellWide::Wide => 1,
                        CellWide::SpacerTail => 2,
                        CellWide::SpacerHead => 3,
                    };
                    if raw.has_hyperlink().unwrap() {
                        d.link = u32::MAX; // resolved below, outside the iterator borrow
                        any_link = true;
                    }
                    if c.has_styling().unwrap() {
                        let s = c.style().unwrap();
                        let mut a = 0u16;
                        for (on, bit) in [s.bold, s.italic, s.faint, s.blink, s.inverse, s.invisible, s.strikethrough, s.overline]
                            .into_iter()
                            .zip(0..)
                        {
                            if on {
                                a |= 1 << bit;
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
                            StyleColor::None => 0,
                            StyleColor::Rgb(c) => rgb(c),
                            StyleColor::Palette(i) => rgb(colors.palette[i.0 as usize]),
                        };
                    }
                    cells.push(d);
                }
                if !full {
                    row.set_dirty(false).unwrap();
                }
                if any_link {
                    link_rows.push(rows.len());
                }
                rows.push(Row { y, cells });
            }
        }
        snap.clean().unwrap();
        // G7 subset: intern OSC 8 URIs for flagged cells.
        let mut buf = vec![0u8; 2048];
        for ri in link_rows {
            let y = rows[ri].y;
            for x in 0..rows[ri].cells.len() {
                if rows[ri].cells[x].link == u32::MAX {
                    let uri = self
                        .term
                        .grid_ref(Point::Viewport(PointCoordinate { x: x as u16, y: y as u32 }))
                        .ok()
                        .and_then(|r| r.hyperlink_uri(&mut buf).ok())
                        .map(|n| String::from_utf8_lossy(&buf[..n]).into_owned())
                        .unwrap_or_default();
                    let next = self.links.len() as u32 + 1;
                    let new_links = &mut self.new_links;
                    let id = *self.links.entry(uri.clone()).or_insert_with(|| {
                        new_links.push((next, uri));
                        next
                    });
                    rows[ri].cells[x].link = id;
                }
            }
        }
        self.seq += 1;
        Some(FrameDelta {
            seq: self.seq,
            cols: self.cfg.cols,
            rows: self.cfg.rows,
            full: full || matches!(dirty, Dirty::Full),
            cursor,
            t_read,
            t_built: crate::util::now_ns(),
            new_graphemes: std::mem::take(&mut self.new_graphemes),
            new_links: std::mem::take(&mut self.new_links),
            dirty: rows,
            placements,
        })
    }

    /// G8 subset: current placements (direct and virtual) as DIE placements.
    pub fn placements(&mut self) -> Vec<Placement> {
        let g = self.term.kitty_graphics().unwrap();
        let mut v = Vec::new();
        let mut it = self.pl_it.update(&g).unwrap();
        while let Some(p) = it.next() {
            let id = p.image_id().unwrap();
            let Some(img) = g.image(id) else { continue };
            let mut d = Placement {
                image_id: id,
                placement_id: p.placement_id().unwrap(),
                image_gen: img.generation().unwrap(),
                z: p.z().unwrap(),
                is_virtual: p.is_virtual().unwrap(),
                offset: (p.x_offset().unwrap(), p.y_offset().unwrap()),
                ..Default::default()
            };
            if let Ok(i) = p.placement_render_info(&img, &self.term) {
                d.col = i.viewport_col;
                d.row = i.viewport_row;
                d.grid = (i.grid_cols, i.grid_rows);
                d.px = (i.pixel_width, i.pixel_height);
                d.src = [i.source_x, i.source_y, i.source_width, i.source_height];
            }
            v.push(d);
        }
        v
    }

    fn placements_if_changed(&mut self) -> Option<Vec<Placement>> {
        let p = self.placements();
        if p == self.last_placements {
            return None;
        }
        self.last_placements = p.clone();
        Some(p)
    }

    /// Images referenced by placements that DIE doesn't have yet, copied once (converted
    /// to RGBA) into memory from `alloc`: heap for in-process, shm for the helper.
    pub fn new_images(&mut self, alloc: &mut dyn FnMut(usize) -> (Pixels, *mut u8)) -> Vec<ImageUpload> {
        let mut out = Vec::new();
        let g = self.term.kitty_graphics().unwrap();
        let mut seen = Vec::new();
        {
            let mut it = self.pl_it.update(&g).unwrap();
            while let Some(p) = it.next() {
                seen.push(p.image_id().unwrap());
            }
        }
        seen.sort_unstable();
        seen.dedup();
        for id in seen {
            let Some(img) = g.image(id) else { continue };
            let generation = img.generation().unwrap();
            if self.images_sent.get(&id) == Some(&generation) {
                continue;
            }
            let t_ready = crate::util::now_ns();
            let (w, h) = (img.width().unwrap(), img.height().unwrap());
            let Some(src) = img.data().unwrap() else { continue };
            let n = w as usize * h as usize * 4;
            let (pixels, dst) = alloc(n);
            // SAFETY: alloc returns n writable bytes that nothing else references yet.
            let dst = unsafe { std::slice::from_raw_parts_mut(dst, n) };
            match img.format().unwrap() {
                kg::ImageFormat::Rgba | kg::ImageFormat::Png => dst.copy_from_slice(&src[..n]),
                kg::ImageFormat::Rgb => {
                    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(3)) {
                        d.copy_from_slice(&[s[0], s[1], s[2], 255]);
                    }
                }
                kg::ImageFormat::Gray => {
                    for (d, s) in dst.chunks_exact_mut(4).zip(src.iter()) {
                        d.copy_from_slice(&[*s, *s, *s, 255]);
                    }
                }
                kg::ImageFormat::GrayAlpha => {
                    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(2)) {
                        d.copy_from_slice(&[s[0], s[0], s[0], s[1]]);
                    }
                }
                _ => continue,
            }
            self.images_sent.insert(id, generation);
            out.push(ImageUpload { image_id: id, generation, width: w, height: h, pixels, t_ready, t_sent: crate::util::now_ns() });
        }
        out
    }

    /// Image ids DIE holds that no placement references any more.
    pub fn gone_images(&mut self) -> Vec<u32> {
        let live: Vec<u32> = self.last_placements.iter().map(|p| p.image_id).collect();
        let gone: Vec<u32> = self.images_sent.keys().copied().filter(|id| !live.contains(id)).collect();
        for id in &gone {
            self.images_sent.remove(id);
        }
        gone
    }

    pub fn encode_key(&mut self, k: i32, mods: u16, action: i32, text: &str, unshifted: u32) -> Vec<u8> {
        let Ok(k) = key::Key::try_from(k) else { return Vec::new() };
        let Ok(a) = key::Action::try_from(action) else { return Vec::new() };
        self.kenc.set_options_from_terminal(&self.term);
        self.kenc.set_macos_option_as_alt(key::OptionAsAlt::True);
        let mods = key::Mods::from_bits_truncate(mods);
        self.kev
            .set_key(k)
            .set_mods(mods)
            .set_action(a)
            .set_consumed_mods(if !text.is_empty() && mods.contains(key::Mods::SHIFT) { key::Mods::SHIFT } else { key::Mods::empty() })
            .set_utf8((!text.is_empty()).then(|| text.to_string()))
            .set_unshifted_codepoint(char::from_u32(unshifted).unwrap_or('\0'));
        let mut v = Vec::new();
        self.kenc.encode_to_vec(&self.kev, &mut v).unwrap();
        v
    }

    pub fn encode_mouse(&mut self, action: i32, button: i32, mods: u16, x: f32, y: f32, any: bool) -> Vec<u8> {
        let Ok(a) = mouse::Action::try_from(action) else { return Vec::new() };
        let b = mouse::Button::try_from(button).ok();
        let Cfg { cols, rows, cell_w, cell_h } = self.cfg;
        self.menc
            .set_options_from_terminal(&self.term)
            .set_size(mouse::EncoderSize {
                screen_width: cols as u32 * cell_w,
                screen_height: rows as u32 * cell_h,
                cell_width: cell_w,
                cell_height: cell_h,
                padding_top: 0,
                padding_bottom: 0,
                padding_right: 0,
                padding_left: 0,
            })
            .set_any_button_pressed(any);
        self.mev.set_action(a).set_button(b).set_mods(key::Mods::from_bits_truncate(mods)).set_position(mouse::Position { x, y });
        let mut v = Vec::new();
        self.menc.encode_to_vec(&self.mev, &mut v).unwrap();
        v
    }

    /// Plain text of scrollback + screen, soft wraps kept (copy state would page cells).
    pub fn scrollback_text(&self) -> (u32, Vec<u8>) {
        let mut f = Formatter::new(
            &self.term,
            FormatterOptions::new().with_format(Format::Plain).with_unwrap(false).with_trim(true),
        )
        .unwrap();
        let b = f.format_alloc(None).unwrap().to_vec();
        let lines = b.split(|&c| c == b'\n').count() as u32;
        (lines, b)
    }
}

/// A heap allocator for in-process image copies (calloc'd, so fresh pages like shm).
pub fn heap_alloc(n: usize) -> (Pixels, *mut u8) {
    let mut a: Arc<Vec<u8>> = Arc::new(vec![0u8; n]);
    let p = Arc::get_mut(&mut a).unwrap().as_mut_ptr();
    (Pixels::Heap(a), p)
}
