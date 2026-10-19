//! Messages between DIE's main thread and a terminal's engine host, and their wire format.
//!
//! The in-process shape moves these values through channels. The helper shape encodes them
//! on a Unix stream socket; image pixels never cross the socket (an shm fd rides along with
//! SCM_RIGHTS instead).
//!
//! Versioning. The handshake (`Hello`/`HelloReply`) is a frozen bootstrap layer: its layout
//! never changes, so any two builds can at least tell each other what they speak. After it,
//! messages are framed as `u32 len | u8 tag | payload`. Same major = compatible; both sides
//! use the lower minor and send only messages that exist in it; unknown tags are skipped by
//! length. A different major is refused cleanly and the helper keeps running.
use crate::shm::ShmMap;
use std::sync::Arc;

pub const MAGIC: [u8; 8] = *b"DIE-TERM";
pub const PROTO_MAJOR: u16 = 1;
pub const PROTO_MINOR: u16 = 1;
/// Names this build (S2_BUILD_TAG at compile time), so two builds can be told apart.
pub fn build_id() -> String {
    format!("{}-{}+{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), option_env!("S2_BUILD_TAG").unwrap_or("spike"))
}

/// The protocol version this process speaks (overridable to simulate other builds).
pub fn my_version() -> (u16, u16) {
    let get = |k: &str, d: u16| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    (get("S2_PROTO_MAJOR", PROTO_MAJOR), get("S2_PROTO_MINOR", PROTO_MINOR))
}

/// One DIE cell (24 bytes on the wire). Colours are 0 for "default", else 0x01RRGGBB.
/// `ch` is a codepoint, or `GRAPHEME | index` into the terminal's grapheme table.
/// `link` is 0 or an index into the terminal's link table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub ch: u32,
    pub fg: u32,
    pub bg: u32,
    pub ul_color: u32,
    pub link: u32,
    pub attrs: u16,
    /// 0 none, 1 single, 2 double, 3 curly, 4 dotted, 5 dashed
    pub ul: u8,
    /// 0 narrow, 1 wide, 2 spacer tail, 3 spacer head
    pub wide: u8,
}
pub const GRAPHEME: u32 = 0x8000_0000;
pub const CELL_BYTES: usize = 24;

#[derive(Clone, Debug, Default)]
pub struct Row {
    pub y: u16,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placement {
    pub image_id: u32,
    pub placement_id: u32,
    pub image_gen: u64,
    pub z: i32,
    pub col: i32,
    pub row: i32,
    pub grid: (u32, u32),
    pub px: (u32, u32),
    pub src: [u32; 4],
    pub offset: (u32, u32),
    pub is_virtual: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FrameDelta {
    pub seq: u64,
    pub cols: u16,
    pub rows: u16,
    /// Every row is included (first frame, resize, reattach).
    pub full: bool,
    pub cursor: Option<(u16, u16)>,
    /// Earliest PTY read (ns, CLOCK_UPTIME_RAW) whose bytes this frame first shows.
    pub t_read: u64,
    /// When the engine side finished building the frame.
    pub t_built: u64,
    pub new_graphemes: Vec<(u32, String)>,
    pub new_links: Vec<(u32, String)>,
    pub dirty: Vec<Row>,
    /// Present when placements changed since the previous frame.
    pub placements: Option<Vec<Placement>>,
}

#[derive(Clone, Debug)]
pub enum Pixels {
    /// In-process: a heap copy shared by Arc.
    Heap(Arc<Vec<u8>>),
    /// Helper: shared memory. In the helper it holds the fd to send; in DIE, the mapping.
    Shm(Arc<ShmMap>),
}

impl Pixels {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Pixels::Heap(a) => a,
            Pixels::Shm(m) => m.bytes(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImageUpload {
    pub image_id: u32,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    /// Always RGBA8 by the time it reaches DIE.
    pub pixels: Pixels,
    /// Engine side: the image was found complete in engine storage.
    pub t_ready: u64,
    /// Engine side: the copy into heap/shm was done and the message handed to the transport.
    pub t_sent: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Title(String),
    Pwd(String),
    Bell,
    ClipboardWrite(Vec<u8>),
    Notify { title: String, body: String },
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub pid: u32,
    pub png_decodes: u64,
    pub png_decode_ns: u64,
    pub bytes_parsed: u64,
    pub frames: u64,
    pub loop_wakeups: u64,
    pub images_copied: u64,
    pub image_bytes_copied: u64,
}

#[derive(Clone, Debug)]
pub enum ToDie {
    Frame(FrameDelta),
    Image(ImageUpload),
    ImageGone { image_id: u32 },
    Effect(Effect),
    /// Plain-text scrollback (copy state / reattach check): lines, text.
    Scrollback { req: u32, lines: u32, text: Vec<u8>, t_built: u64 },
    Exited { status: i32 },
    Stats(Stats),
}

#[derive(Clone, Debug)]
pub enum ToEngine {
    /// A key event, encoded by the engine's key encoder on the engine side.
    Key { key: i32, mods: u16, action: i32, text: String, unshifted: u32, t_sent: u64 },
    /// Raw bytes to the PTY (tests only; DIE would use Key/Paste).
    Text(Vec<u8>),
    Mouse { action: i32, button: i32, mods: u16, x: f32, y: f32, any_pressed: bool },
    Resize { cols: u16, rows: u16, cell_w: u32, cell_h: u32 },
    /// DIE presented a frame at vsync: the engine side may build the next one.
    Ack { seq: u64 },
    Focus(bool),
    FetchScrollback { req: u32 },
    GetStats,
    /// Kill the child and exit.
    Shutdown,
}

// ---------------------------------------------------------------- codec

pub struct W(pub Vec<u8>);
impl W {
    pub fn msg(tag: u8, cap: usize) -> W {
        let mut v = Vec::with_capacity(cap + 5);
        v.extend_from_slice(&[0, 0, 0, 0, tag]);
        W(v)
    }
    pub fn finish(mut self) -> Vec<u8> {
        let n = (self.0.len() - 4) as u32;
        self.0[..4].copy_from_slice(&n.to_le_bytes());
        self.0
    }
    pub fn u8(&mut self, v: u8) { self.0.push(v) }
    pub fn u16(&mut self, v: u16) { self.0.extend_from_slice(&v.to_le_bytes()) }
    pub fn u32(&mut self, v: u32) { self.0.extend_from_slice(&v.to_le_bytes()) }
    pub fn i32(&mut self, v: i32) { self.0.extend_from_slice(&v.to_le_bytes()) }
    pub fn u64(&mut self, v: u64) { self.0.extend_from_slice(&v.to_le_bytes()) }
    pub fn f32(&mut self, v: f32) { self.0.extend_from_slice(&v.to_le_bytes()) }
    pub fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
    }
    pub fn str(&mut self, v: &str) { self.bytes(v.as_bytes()) }
    fn cells(&mut self, cells: &[Cell]) {
        self.u16(cells.len() as u16);
        self.0.reserve(cells.len() * CELL_BYTES);
        for c in cells {
            let mut b = [0u8; CELL_BYTES];
            b[0..4].copy_from_slice(&c.ch.to_le_bytes());
            b[4..8].copy_from_slice(&c.fg.to_le_bytes());
            b[8..12].copy_from_slice(&c.bg.to_le_bytes());
            b[12..16].copy_from_slice(&c.ul_color.to_le_bytes());
            b[16..20].copy_from_slice(&c.link.to_le_bytes());
            b[20..22].copy_from_slice(&c.attrs.to_le_bytes());
            b[22] = c.ul;
            b[23] = c.wide;
            self.0.extend_from_slice(&b);
        }
    }
}

pub struct R<'a> {
    b: &'a [u8],
    i: usize,
}
impl<'a> R<'a> {
    pub fn new(b: &'a [u8]) -> Self { R { b, i: 0 } }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }
    pub fn u8(&mut self) -> Option<u8> { Some(self.take(1)?[0]) }
    pub fn u16(&mut self) -> Option<u16> { Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?)) }
    pub fn u32(&mut self) -> Option<u32> { Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?)) }
    pub fn i32(&mut self) -> Option<i32> { Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?)) }
    pub fn u64(&mut self) -> Option<u64> { Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?)) }
    pub fn f32(&mut self) -> Option<f32> { Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?)) }
    pub fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    pub fn str(&mut self) -> Option<String> { Some(String::from_utf8_lossy(self.bytes()?).into_owned()) }
    fn cells(&mut self) -> Option<Vec<Cell>> {
        let n = self.u16()? as usize;
        let raw = self.take(n * CELL_BYTES)?;
        let le32 = |b: &[u8], o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        Some(
            raw.chunks_exact(CELL_BYTES)
                .map(|b| Cell {
                    ch: le32(b, 0),
                    fg: le32(b, 4),
                    bg: le32(b, 8),
                    ul_color: le32(b, 12),
                    link: le32(b, 16),
                    attrs: u16::from_le_bytes([b[20], b[21]]),
                    ul: b[22],
                    wide: b[23],
                })
                .collect(),
        )
    }
}

/// Encode a message for the helper socket. Returns the bytes and whether an shm fd must ride
/// along (images); the caller attaches it.
pub fn encode_to_die(m: &ToDie) -> Vec<u8> {
    match m {
        ToDie::Frame(f) => {
            let cells: usize = f.dirty.iter().map(|r| r.cells.len()).sum();
            let mut w = W::msg(1, 64 + cells * CELL_BYTES + f.dirty.len() * 4);
            w.u64(f.seq);
            w.u16(f.cols);
            w.u16(f.rows);
            w.u8(f.full as u8 | (f.cursor.is_some() as u8) << 1 | (f.placements.is_some() as u8) << 2);
            let (cx, cy) = f.cursor.unwrap_or((0, 0));
            w.u16(cx);
            w.u16(cy);
            w.u64(f.t_read);
            w.u64(f.t_built);
            for table in [&f.new_graphemes, &f.new_links] {
                w.u32(table.len() as u32);
                for (id, s) in table.iter() {
                    w.u32(*id);
                    w.str(s);
                }
            }
            w.u16(f.dirty.len() as u16);
            for r in &f.dirty {
                w.u16(r.y);
                w.cells(&r.cells);
            }
            if let Some(ps) = &f.placements {
                w.u32(ps.len() as u32);
                for p in ps {
                    w.u32(p.image_id);
                    w.u32(p.placement_id);
                    w.u64(p.image_gen);
                    w.i32(p.z);
                    w.i32(p.col);
                    w.i32(p.row);
                    for v in [p.grid.0, p.grid.1, p.px.0, p.px.1, p.src[0], p.src[1], p.src[2], p.src[3], p.offset.0, p.offset.1] {
                        w.u32(v);
                    }
                    w.u8(p.is_virtual as u8);
                }
            }
            w.finish()
        }
        ToDie::Image(i) => {
            let mut w = W::msg(2, 48);
            w.u32(i.image_id);
            w.u64(i.generation);
            w.u32(i.width);
            w.u32(i.height);
            w.u64(i.pixels.bytes().len() as u64);
            w.u64(i.t_ready);
            w.u64(i.t_sent);
            w.finish()
        }
        ToDie::ImageGone { image_id } => {
            let mut w = W::msg(3, 4);
            w.u32(*image_id);
            w.finish()
        }
        ToDie::Effect(e) => {
            let mut w = W::msg(4, 32);
            match e {
                Effect::Title(s) => { w.u8(1); w.str(s) }
                Effect::Pwd(s) => { w.u8(2); w.str(s) }
                Effect::Bell => w.u8(3),
                Effect::ClipboardWrite(b) => { w.u8(4); w.bytes(b) }
                Effect::Notify { title, body } => { w.u8(5); w.str(title); w.str(body) }
            }
            w.finish()
        }
        ToDie::Scrollback { req, lines, text, t_built } => {
            let mut w = W::msg(5, text.len() + 24);
            w.u32(*req);
            w.u32(*lines);
            w.u64(*t_built);
            w.bytes(text);
            w.finish()
        }
        ToDie::Exited { status } => {
            let mut w = W::msg(6, 4);
            w.i32(*status);
            w.finish()
        }
        ToDie::Stats(s) => {
            let mut w = W::msg(7, 64);
            w.u32(s.pid);
            for v in [s.png_decodes, s.png_decode_ns, s.bytes_parsed, s.frames, s.loop_wakeups, s.images_copied, s.image_bytes_copied] {
                w.u64(v);
            }
            w.finish()
        }
    }
}

/// Decode one message body (after the length prefix). Images need their mapping supplied
/// by the caller (`take_shm`), which pops the next fd received with SCM_RIGHTS.
pub fn decode_to_die(body: &[u8], take_shm: &mut dyn FnMut(usize) -> Option<Arc<ShmMap>>) -> Option<Option<ToDie>> {
    let mut r = R::new(body);
    let tag = r.u8()?;
    Some(Some(match tag {
        1 => {
            let seq = r.u64()?;
            let cols = r.u16()?;
            let rows = r.u16()?;
            let flags = r.u8()?;
            let (cx, cy) = (r.u16()?, r.u16()?);
            let t_read = r.u64()?;
            let t_built = r.u64()?;
            let mut tables = [Vec::new(), Vec::new()];
            for t in tables.iter_mut() {
                let n = r.u32()?;
                for _ in 0..n {
                    t.push((r.u32()?, r.str()?));
                }
            }
            let [new_graphemes, new_links] = tables;
            let nrows = r.u16()?;
            let mut dirty = Vec::with_capacity(nrows as usize);
            for _ in 0..nrows {
                let y = r.u16()?;
                dirty.push(Row { y, cells: r.cells()? });
            }
            let placements = if flags & 4 != 0 {
                let n = r.u32()?;
                let mut v = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    let image_id = r.u32()?;
                    let placement_id = r.u32()?;
                    let image_gen = r.u64()?;
                    let (z, col, row) = (r.i32()?, r.i32()?, r.i32()?);
                    let mut u = [0u32; 10];
                    for x in u.iter_mut() {
                        *x = r.u32()?;
                    }
                    v.push(Placement {
                        image_id,
                        placement_id,
                        image_gen,
                        z,
                        col,
                        row,
                        grid: (u[0], u[1]),
                        px: (u[2], u[3]),
                        src: [u[4], u[5], u[6], u[7]],
                        offset: (u[8], u[9]),
                        is_virtual: r.u8()? != 0,
                    });
                }
                Some(v)
            } else {
                None
            };
            ToDie::Frame(FrameDelta {
                seq,
                cols,
                rows,
                full: flags & 1 != 0,
                cursor: (flags & 2 != 0).then_some((cx, cy)),
                t_read,
                t_built,
                new_graphemes,
                new_links,
                dirty,
                placements,
            })
        }
        2 => {
            let image_id = r.u32()?;
            let generation = r.u64()?;
            let (width, height) = (r.u32()?, r.u32()?);
            let len = r.u64()? as usize;
            let (t_ready, t_sent) = (r.u64()?, r.u64()?);
            let map = take_shm(len)?;
            ToDie::Image(ImageUpload { image_id, generation, width, height, pixels: Pixels::Shm(map), t_ready, t_sent })
        }
        3 => ToDie::ImageGone { image_id: r.u32()? },
        4 => ToDie::Effect(match r.u8()? {
            1 => Effect::Title(r.str()?),
            2 => Effect::Pwd(r.str()?),
            3 => Effect::Bell,
            4 => Effect::ClipboardWrite(r.bytes()?.to_vec()),
            5 => Effect::Notify { title: r.str()?, body: r.str()? },
            _ => return Some(None),
        }),
        5 => {
            let req = r.u32()?;
            let lines = r.u32()?;
            let t_built = r.u64()?;
            ToDie::Scrollback { req, lines, t_built, text: r.bytes()?.to_vec() }
        }
        6 => ToDie::Exited { status: r.i32()? },
        7 => {
            let pid = r.u32()?;
            let mut u = [0u64; 7];
            for x in u.iter_mut() {
                *x = r.u64()?;
            }
            ToDie::Stats(Stats {
                pid,
                png_decodes: u[0],
                png_decode_ns: u[1],
                bytes_parsed: u[2],
                frames: u[3],
                loop_wakeups: u[4],
                images_copied: u[5],
                image_bytes_copied: u[6],
            })
        }
        _ => return Some(None), // unknown tag from a newer minor: skip
    }))
}

pub fn encode_to_engine(m: &ToEngine) -> Vec<u8> {
    match m {
        ToEngine::Key { key, mods, action, text, unshifted, t_sent } => {
            let mut w = W::msg(1, 32);
            w.i32(*key);
            w.u16(*mods);
            w.i32(*action);
            w.str(text);
            w.u32(*unshifted);
            w.u64(*t_sent);
            w.finish()
        }
        ToEngine::Text(b) => {
            let mut w = W::msg(2, b.len() + 4);
            w.bytes(b);
            w.finish()
        }
        ToEngine::Mouse { action, button, mods, x, y, any_pressed } => {
            let mut w = W::msg(3, 24);
            w.i32(*action);
            w.i32(*button);
            w.u16(*mods);
            w.f32(*x);
            w.f32(*y);
            w.u8(*any_pressed as u8);
            w.finish()
        }
        ToEngine::Resize { cols, rows, cell_w, cell_h } => {
            let mut w = W::msg(4, 12);
            w.u16(*cols);
            w.u16(*rows);
            w.u32(*cell_w);
            w.u32(*cell_h);
            w.finish()
        }
        ToEngine::Ack { seq } => {
            let mut w = W::msg(5, 8);
            w.u64(*seq);
            w.finish()
        }
        ToEngine::Focus(f) => {
            let mut w = W::msg(6, 1);
            w.u8(*f as u8);
            w.finish()
        }
        ToEngine::FetchScrollback { req } => {
            let mut w = W::msg(7, 4);
            w.u32(*req);
            w.finish()
        }
        ToEngine::GetStats => W::msg(8, 0).finish(),
        ToEngine::Shutdown => W::msg(9, 0).finish(),
    }
}

pub fn decode_to_engine(body: &[u8]) -> Option<Option<ToEngine>> {
    let mut r = R::new(body);
    Some(Some(match r.u8()? {
        1 => ToEngine::Key {
            key: r.i32()?,
            mods: r.u16()?,
            action: r.i32()?,
            text: r.str()?,
            unshifted: r.u32()?,
            t_sent: r.u64()?,
        },
        2 => ToEngine::Text(r.bytes()?.to_vec()),
        3 => ToEngine::Mouse {
            action: r.i32()?,
            button: r.i32()?,
            mods: r.u16()?,
            x: r.f32()?,
            y: r.f32()?,
            any_pressed: r.u8()? != 0,
        },
        4 => ToEngine::Resize { cols: r.u16()?, rows: r.u16()?, cell_w: r.u32()?, cell_h: r.u32()? },
        5 => ToEngine::Ack { seq: r.u64()? },
        6 => ToEngine::Focus(r.u8()? != 0),
        7 => ToEngine::FetchScrollback { req: r.u32()? },
        8 => ToEngine::GetStats,
        9 => ToEngine::Shutdown,
        _ => return Some(None),
    }))
}

/// Split complete frames off the front of `buf`. Returns (consumed, bodies).
pub fn split_frames(buf: &[u8]) -> (usize, Vec<(usize, usize)>) {
    let mut i = 0;
    let mut out = Vec::new();
    while buf.len() - i >= 4 {
        let n = u32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as usize;
        if buf.len() - i - 4 < n {
            break;
        }
        out.push((i + 4, n));
        i += 4 + n;
    }
    (i, out)
}

// ---------------------------------------------------------------- frozen handshake

/// Hello (DIE -> helper): MAGIC | u16 major | u16 minor | u16 len | build id.
pub fn hello(major: u16, minor: u16) -> Vec<u8> {
    let mut v = MAGIC.to_vec();
    v.extend_from_slice(&major.to_le_bytes());
    v.extend_from_slice(&minor.to_le_bytes());
    let id = build_id();
    v.extend_from_slice(&(id.len() as u16).to_le_bytes());
    v.extend_from_slice(id.as_bytes());
    v
}

/// Parse a Hello. Returns (major, minor, build, consumed) once complete.
pub fn parse_hello(b: &[u8]) -> Result<Option<(u16, u16, String, usize)>, String> {
    if b.len() < 14 {
        return Ok(None);
    }
    if b[..8] != MAGIC {
        return Err("bad magic".into());
    }
    let major = u16::from_le_bytes([b[8], b[9]]);
    let minor = u16::from_le_bytes([b[10], b[11]]);
    let n = u16::from_le_bytes([b[12], b[13]]) as usize;
    if b.len() < 14 + n {
        return Ok(None);
    }
    Ok(Some((major, minor, String::from_utf8_lossy(&b[14..14 + n]).into_owned(), 14 + n)))
}

/// HelloReply (helper -> DIE): MAGIC | u8 status (0 ok, 1 refused) | u16 major | u16 minor |
/// u32 helper pid | u32 child pid | u16 len | text (build id, or the refusal reason).
pub fn hello_reply(ok: bool, major: u16, minor: u16, helper_pid: u32, child_pid: u32, text: &str) -> Vec<u8> {
    let mut v = MAGIC.to_vec();
    v.push(if ok { 0 } else { 1 });
    v.extend_from_slice(&major.to_le_bytes());
    v.extend_from_slice(&minor.to_le_bytes());
    v.extend_from_slice(&helper_pid.to_le_bytes());
    v.extend_from_slice(&child_pid.to_le_bytes());
    v.extend_from_slice(&(text.len() as u16).to_le_bytes());
    v.extend_from_slice(text.as_bytes());
    v
}

#[derive(Debug, Clone)]
pub struct HelloReply {
    pub ok: bool,
    pub major: u16,
    pub minor: u16,
    pub helper_pid: u32,
    pub child_pid: u32,
    pub text: String,
}

pub fn parse_hello_reply(b: &[u8]) -> Result<Option<(HelloReply, usize)>, String> {
    if b.len() < 23 {
        return Ok(None);
    }
    if b[..8] != MAGIC {
        return Err("bad magic".into());
    }
    let n = u16::from_le_bytes([b[21], b[22]]) as usize;
    if b.len() < 23 + n {
        return Ok(None);
    }
    Ok(Some((
        HelloReply {
            ok: b[8] == 0,
            major: u16::from_le_bytes([b[9], b[10]]),
            minor: u16::from_le_bytes([b[11], b[12]]),
            helper_pid: u32::from_le_bytes(b[13..17].try_into().unwrap()),
            child_pid: u32::from_le_bytes(b[17..21].try_into().unwrap()),
            text: String::from_utf8_lossy(&b[23..23 + n]).into_owned(),
        },
        23 + n,
    )))
}

/// The helper's compatibility rule.
pub fn negotiate(mine: (u16, u16), peer: (u16, u16)) -> Result<u16, String> {
    if mine.0 != peer.0 {
        return Err(format!(
            "incompatible protocol: helper speaks {}.{}, DIE speaks {}.{}; the terminal stays alive, detached",
            mine.0, mine.1, peer.0, peer.1
        ));
    }
    Ok(mine.1.min(peer.1))
}
