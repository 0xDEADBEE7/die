//! Hands-on battery against the Ghostty CI artifact via libghostty-rs (links override).
use s2_term_engine::dterm::*;
use libghostty_vt::{
    build_info, focus,
    key::{Action as KA, Key, Mods},
    mouse::{Action as MA, Button},
    paste,
    terminal::{ClipboardMime, Mode, Point, PointCoordinate, Terminal},
};
use std::sync::atomic::{AtomicUsize, Ordering};

static FAIL: AtomicUsize = AtomicUsize::new(0);
static PASS: AtomicUsize = AtomicUsize::new(0);
fn check(name: &str, ok: bool, detail: impl std::fmt::Display) {
    if ok {
        PASS.fetch_add(1, Ordering::Relaxed);
        println!("  PASS {name}: {detail}");
    } else {
        FAIL.fetch_add(1, Ordering::Relaxed);
        println!("  FAIL {name}: {detail}");
    }
}

const CW: u32 = 18; // physical px (9x18pt @2x)
const CH: u32 = 36;

fn main() {
    // DIE installs its hook once, at startup, on the main thread.
    s2_term_engine::hook::install();
    println!("== build info");
    {
        let t = TermThread::spawn(80, 24, CW, CH);
        let s = t.run(|_c| {
            format!(
                "version={:?} build={:?} optimize={:?} kitty_graphics={:?} simd={:?}",
                build_info::version_string(),
                build_info::build_version(),
                build_info::optimize_mode(),
                build_info::supports_kitty_graphics(),
                build_info::supports_simd()
            )
        });
        println!("  {s}");
    }
    cells_and_dirty();
    reflow_and_export();
    kitty_direct();
    kitty_placeholder();
    keys();
    mouse_all();
    paste_focus();
    osc_callbacks();
    replies_sync();
    image_limit();
    threads_and_snapshot();
    println!(
        "\nTOTAL: {} pass, {} fail",
        PASS.load(Ordering::Relaxed),
        FAIL.load(Ordering::Relaxed)
    );
    let (p0, f0) = (PASS.load(Ordering::Relaxed), FAIL.load(Ordering::Relaxed));
    s2_wiring();
    println!(
        "S2 WIRING CHECKS: {} pass, {} fail",
        PASS.load(Ordering::Relaxed) - p0,
        FAIL.load(Ordering::Relaxed) - f0
    );
}

/// S2 additions: DIE's exact wiring (no default features, DIE's process-global PNG hook).
fn s2_wiring() {
    use s2_term_engine::hook;
    println!("== S2: DIE's PNG hook is process-global (wrapper issue #104)");
    let d = hook::DECODES.load(Ordering::Relaxed);
    let f = hook::FAILURES.load(Ordering::Relaxed);
    // kitty_direct 1 + kitty_placeholder 1 + 8 threads x 4 = 34 PNGs decoded above.
    check("battery PNGs all went through DIE's hook", d == 34 && f == 0, format!("decodes={d} failures={f}"));
    // A thread that never calls any install function, using the raw wrapper API.
    let r = std::thread::spawn(|| {
        let mut t = Terminal::new(40, 10).unwrap();
        t.set_kitty_image_storage_limit(1 << 20).unwrap();
        let png = png_rgba(8, 8, |x, y| [x as u8 * 30, y as u8 * 30, 7, 255]);
        for ch in kitty_chunked("q=2,a=t,f=100,i=5", &png) {
            t.vt_write(&ch);
        }
        let g = t.kitty_graphics().unwrap();
        g.image(5).map(|i| (i.width().unwrap(), i.height().unwrap(), i.data().unwrap().map(|d| d.len())))
    })
    .join()
    .unwrap();
    check(
        "PNG on a fresh thread with no per-thread install decodes",
        r == Some((8, 8, Some(256))),
        format!("{r:?}"),
    );
    check(
        "hook count rose by one for that thread",
        hook::DECODES.load(Ordering::Relaxed) == d + 1,
        hook::DECODES.load(Ordering::Relaxed),
    );
    // Grey+alpha and palette PNGs go through the GA expansion / palette paths.
    let mut ga = Vec::new();
    {
        let mut e = png::Encoder::new(&mut ga, 3, 2);
        e.set_color(png::ColorType::GrayscaleAlpha);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&[10, 255, 20, 128, 30, 0, 40, 255, 50, 255, 60, 255]).unwrap();
    }
    let t = TermThread::spawn(40, 10, CW, CH);
    for ch in kitty_chunked("q=2,a=T,f=100,i=6", &ga) {
        t.write(ch);
    }
    let px = t.run(|c| {
        let g = c.term.kitty_graphics().unwrap();
        g.image(6).and_then(|i| i.data().unwrap().map(|d| d[..8].to_vec()))
    });
    check("grey+alpha PNG expands to RGBA", px.as_deref() == Some(&[10, 10, 10, 255, 20, 20, 20, 128][..]), format!("{px:?}"));
    // A PNG over the storage limit: decoded, then refused at storage (ENOMEM).
    let t = TermThread::spawn(40, 10, CW, CH);
    t.run(|c| {
        c.term.set_kitty_image_storage_limit(100_000).unwrap();
    });
    let big = png_rgba(400, 400, |_, _| [1, 2, 3, 255]);
    let (f0, d0) = (hook::FAILURES.load(Ordering::Relaxed), hook::DECODES.load(Ordering::Relaxed));
    for ch in kitty_chunked("a=T,f=100,i=9", &big) {
        t.write(ch);
    }
    let reply = t.pty_replies();
    check(
        "PNG over the storage limit is refused (ENOMEM)",
        reply.contains("ENOMEM"),
        format!(
            "reply={reply:?}; hook decodes +{} failures +{} (the decode ran; the storage limit refused it)",
            hook::DECODES.load(Ordering::Relaxed) - d0,
            hook::FAILURES.load(Ordering::Relaxed) - f0
        ),
    );
    // A tiny PNG whose header claims 20000x20000 (1.6 GB RGBA): the hook allocates the output
    // from the engine's allocator before decoding, so the engine's cap must refuse it there.
    let mut forged = png_rgba(2, 2, |_, _| [5, 5, 5, 255]);
    forged[16..20].copy_from_slice(&20000u32.to_be_bytes());
    forged[20..24].copy_from_slice(&20000u32.to_be_bytes());
    let crc = crc32(&forged[12..29]);
    forged[29..33].copy_from_slice(&crc.to_be_bytes());
    let t = TermThread::spawn(40, 10, CW, CH);
    let (f0, d0) = (hook::FAILURES.load(Ordering::Relaxed), hook::DECODES.load(Ordering::Relaxed));
    let t0 = std::time::Instant::now();
    for ch in kitty_chunked("a=T,f=100,i=10", &forged) {
        t.write(ch);
    }
    let reply = t.pty_replies();
    let dt = t0.elapsed();
    check(
        "forged 20000x20000 PNG header is refused without a 1.6 GB allocation",
        !reply.contains(";OK") && hook::FAILURES.load(Ordering::Relaxed) == f0 + 1 && hook::DECODES.load(Ordering::Relaxed) == d0,
        format!(
            "reply={reply:?} in {dt:?}; hook failures +{} (refused at: png header {}, engine allocator {}, pixel data {})",
            hook::FAILURES.load(Ordering::Relaxed) - f0,
            hook::REFUSED_HEADER.load(Ordering::Relaxed),
            hook::REFUSED_ALLOC.load(Ordering::Relaxed),
            hook::REFUSED_DATA.load(Ordering::Relaxed)
        ),
    );
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn cells_and_dirty() {
    println!("== cells + dirty rows");
    let t = TermThread::spawn(80, 24, CW, CH);
    t.write(
        "\x1b[1;3;4:3;58;2;255;0;0;38;2;1;2;3;48;5;17mHi\x1b[0m \u{4e16}\u{754c} \x1b]8;;https://example.com/x\x1b\\link\x1b]8;;\x1b\\ \x1b[9;53;2;5;7mS\x1b[0m e\u{301} \u{1F44D}\u{1F3FD}\x1b[4:4;58:5:196mD\x1b[0m",
    );
    let f = t.run(|c| c.frame(true));
    let r0 = &f.rows[0].1;
    let show: Vec<String> = r0
        .iter()
        .take(20)
        .map(|c| format!("{:?}{}{}", c.text, if c.wide == 1 { "W" } else if c.wide == 2 { "T" } else { "" }, if c.link { "L" } else { "" }))
        .collect();
    println!("  row0: {}", show.join(" "));
    let h = &r0[0];
    check(
        "bold+italic+curly ul + ul colour + truecolor fg + 256 bg",
        h.text == "H" && h.attrs & BOLD != 0 && h.attrs & ITALIC != 0 && h.ul == 3 && h.ul_color == Some([255, 0, 0]) && h.fg == Some([1, 2, 3]) && h.bg.is_some(),
        format!("{h:?}"),
    );
    check("wide CJK + spacer tail", r0[3].text == "\u{4e16}" && r0[3].wide == 1 && r0[4].wide == 2, format!("{:?}/{:?}", r0[3], r0[4]));
    check("OSC 8 cell flag", r0[8].link && r0[8].text == "l", format!("{:?}", r0[8]));
    let uri = t.run(|c| c.hyperlink_at(8, 0));
    check("OSC 8 uri via grid_ref", uri.as_deref() == Some("https://example.com/x"), format!("{uri:?}"));
    let s = &r0[13];
    check(
        "strike+overline+faint+blink+inverse",
        s.attrs & (STRIKE | OVERLINE | FAINT | BLINK | INVERSE) == (STRIKE | OVERLINE | FAINT | BLINK | INVERSE),
        format!("{s:?}"),
    );
    check("combining grapheme in one cell", r0[15].text == "e\u{301}", format!("{:?}", r0[15]));
    check("emoji+skin-tone ZWJ-less cluster wide", r0[17].text == "\u{1F44D}\u{1F3FD}" && r0[17].wide == 1, format!("{:?}", r0[17]));
    check("dotted underline + palette ul colour", r0[19].ul == 4 && r0[19].ul_color.is_some(), format!("{:?}", r0[19]));
    // dirty tracking
    let fresh = TermThread::spawn(80, 24, CW, CH);
    fresh.write("hello");
    let f0 = fresh.run(|c| c.frame(false));
    let f1 = t.run(|c| c.frame(false));
    t.write("\x1b[6;1Hzz");
    let f2 = t.run(|c| c.frame(false));
    let ys: Vec<u16> = f2.rows.iter().map(|r| r.0).collect();
    check("first frame full", f0.dirty == 2, format!("dirty={} rows={}", f0.dirty, f0.rows.len()));
    check("clean frame yields no rows", f1.dirty == 0 && f1.rows.is_empty(), format!("dirty={} rows={}", f1.dirty, f1.rows.len()));
    check("partial frame: only touched rows (+ old cursor row)", f2.dirty == 1 && ys.contains(&5) && ys.len() <= 3, format!("dirty={} rows={ys:?} cursor={:?}", f2.dirty, f2.cursor));
}

fn reflow_and_export() {
    println!("== resize/reflow + scrollback export");
    let t = TermThread::spawn(80, 24, CW, CH);
    let mut s = String::new();
    s.push_str(&"0123456789".repeat(20));
    s.push_str("\r\n");
    for i in 0..200 {
        s.push_str(&format!("line {i} {}\r\n", "x".repeat(i % 70)));
    }
    s.push_str("$ prompt");
    t.write(s);
    let before = t.run(|c| c.text(true));
    t.run(|c| c.resize(40, 24));
    let narrow = t.run(|c| c.text(true));
    let (cx, cy) = t.run(|c| (c.term.cursor_x().unwrap(), c.term.cursor_y().unwrap()));
    t.run(|c| c.resize(120, 30));
    let wide = t.run(|c| c.text(true));
    check("unwrapped text identical 80->40", before == narrow, format!("len {} vs {}", before.len(), narrow.len()));
    check("unwrapped text identical 40->120", before == wide, format!("len {} vs {}", before.len(), wide.len()));
    check("cursor stays after prompt", cx == 8, format!("cursor at 40 cols=({cx},{cy})"));
    let wrapped40 = t.run(|c| {
        c.resize(40, 24);
        let s = c.text(false);
        c.resize(80, 24);
        s
    });
    check("soft-wrapped export at 40 cols has more lines", wrapped40.lines().count() > before.lines().count(), format!("{} vs {}", wrapped40.lines().count(), before.lines().count()));
    // alt screen does not reflow / keeps primary
    t.write("\x1b[?1049h\x1b[2J\x1b[Halt-screen");
    let alt = t.run(|c| c.text(true));
    t.write("\x1b[?1049l");
    let back = t.run(|c| c.text(true));
    check("alt screen isolated, primary restored", alt.trim() == "alt-screen" && back == before, format!("alt={:?}", alt.trim()));
    // big export
    let t2 = TermThread::spawn(120, 40, CW, CH);
    let mut big = String::new();
    for i in 0..12_000 {
        big.push_str(&format!("\x1b[3{}mrow {i} lorem ipsum dolor sit amet\x1b[0m\r\n", i % 8));
    }
    t2.write(big);
    let (n, dt, sb) = t2.run(|c| {
        let t0 = std::time::Instant::now();
        let s = c.text(true);
        (s.lines().count(), t0.elapsed(), c.term.scrollback_rows().unwrap())
    });
    check("scrollback export (10k max_lines)", n >= 9_000, format!("{n} lines exported in {dt:?}; scrollback_rows={sb}"));
}

fn kitty_direct() {
    println!("== kitty graphics: direct placements");
    let t = TermThread::spawn(80, 24, CW, CH);
    // PNG, chunked, a=T with explicit cell size and placement id
    let png = png_rgba(64, 48, |x, y| [x as u8 * 4, y as u8 * 5, 128, 255]);
    for ch in kitty_chunked("a=T,f=100,i=1,p=7,c=4,r=2,C=1", &png) {
        t.write(ch);
    }
    let r1 = t.pty_replies();
    // RGBA raw, chunked (big enough for >1 chunk), at cursor with z=-1, X/Y offsets
    let rgba: Vec<u8> = (0..(100 * 80)).flat_map(|i| [(i % 251) as u8, 0, 200, 255]).collect();
    t.write("\x1b[10;20H");
    let chunks = kitty_chunked("a=T,f=32,s=100,v=80,i=2,z=-1,X=3,Y=5,C=1", &rgba);
    let nchunks = chunks.len();
    for ch in chunks {
        t.write(ch);
    }
    let r2 = t.pty_replies();
    check("PNG a=T chunked OK", r1.contains("i=1,p=7;OK"), &r1);
    check(&format!("RGBA a=T {nchunks} chunks OK"), r2.contains("i=2;OK"), &r2);
    let ps = t.run(|c| c.placements());
    for p in &ps {
        println!("  {p:?}");
    }
    let p1 = ps.iter().find(|p| p.image_id == 1);
    let p2 = ps.iter().find(|p| p.image_id == 2);
    check(
        "PNG placement geometry (c=4,r=2 -> 72x72px at 0,0)",
        p1.is_some_and(|p| p.viewport == Some((0, 0)) && p.grid == (4, 2) && p.px == (72, 72) && p.placement_id == 7 && p.img_fmt == "Rgba" && p.img_bytes == 64 * 48 * 4),
        format!("{p1:?}"),
    );
    check(
        "RGBA placement geometry (100x80px -> 6x3 cells @ (19,9), z=-1, offsets 3,5)",
        p2.is_some_and(|p| p.viewport == Some((19, 9)) && p.grid == (6, 3) && p.z == -1 && p.offset == (3, 5)),
        format!("{p2:?}"),
    );
    // scroll so image 1 goes partly above the viewport -> negative row
    t.write("\x1b[24;1H\n");
    let ps = t.run(|c| c.placements());
    let p1 = ps.iter().find(|p| p.image_id == 1);
    check("placement moves with scroll (negative viewport row)", p1.is_some_and(|p| p.viewport == Some((0, -1)) && p.visible), format!("{p1:?}"));
    // delete variants
    t.write("\x1b_Ga=d,d=i,i=2\x1b\\");
    let ps = t.run(|c| c.placements());
    check("a=d,d=i removes image 2 placements", ps.iter().all(|p| p.image_id != 2), format!("{} left", ps.len()));
    // source-rect crop
    t.write("\x1b[2;30H\x1b_Ga=p,i=1,p=9,x=8,y=4,w=16,h=12,C=1\x1b\\");
    let ps = t.run(|c| c.placements());
    let p9 = ps.iter().find(|p| p.placement_id == 9);
    check("a=p crop source rect", p9.is_some_and(|p| p.src == (8, 4, 16, 12)), format!("{p9:?}"));
    // query
    t.write("\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\");
    let q = t.pty_replies();
    check("a=q support query answered", q.contains("i=31;OK"), &q);
}

const DIAC: [char; 8] = ['\u{0305}', '\u{030D}', '\u{030E}', '\u{0310}', '\u{0312}', '\u{033D}', '\u{033E}', '\u{033F}'];

fn kitty_placeholder() {
    println!("== kitty graphics: Unicode placeholders (virtual placements)");
    let t = TermThread::spawn(80, 24, CW, CH);
    // yazi-style: a=T,U=1 with no c/r; kitten-icat style: separate a=p,U=1,c,r with p id
    let id: u32 = 0x00ab_cdef;
    let raw = vec![200u8; 100 * 70 * 3];
    for ch in kitty_chunked(&format!("q=2,a=T,C=1,U=1,f=24,s=100,v=70,i={id}"), &raw) {
        t.write(ch);
    }
    let png = png_rgba(40, 40, |_, _| [0, 255, 0, 255]);
    for ch in kitty_chunked("q=2,a=t,f=100,i=42", &png) {
        t.write(ch);
    }
    t.write("\x1b_Gq=2,a=p,U=1,i=42,p=5,c=3,r=1\x1b\\");
    let mut s = format!("\x1b[38;2;{};{};{}m", (id >> 16) & 255, (id >> 8) & 255, id & 255);
    for y in 0..2u16 {
        s += &format!("\x1b[{};10H", 3 + y);
        for x in 0..6usize {
            s.push('\u{10EEEE}');
            s.push(DIAC[y as usize]);
            s.push(DIAC[x]);
        }
    }
    // second image: palette fg 42, placement id in underline colour, diacritics omitted after first cell
    s += "\x1b[0m\x1b[38;5;42m\x1b[58;5;5m\x1b[6;1H\u{10EEEE}\u{0305}\u{0305}\u{10EEEE}\u{10EEEE}\x1b[0m";
    t.write(s);
    let replies = t.pty_replies();
    let ps = t.run(|c| c.placements());
    for p in &ps {
        println!("  {p:?}");
    }
    let v1 = ps.iter().find(|p| p.image_id == id);
    let v2 = ps.iter().find(|p| p.image_id == 42);
    check("virtual placement listed with is_virtual + grid from s/v", v1.is_some_and(|p| p.is_virtual && p.grid == (6, 2)), format!("{v1:?}"));
    check("virtual placement has NO viewport geometry from C API", v1.is_some_and(|p| !p.visible), format!("viewport={:?} visible={:?}", v1.map(|p| p.viewport), v1.map(|p| p.visible)));
    check("a=p,U=1 virtual placement: id/pid/c/r readable", v2.is_some_and(|p| p.is_virtual && p.placement_id == 5 && p.grid == (3, 1)), format!("{v2:?}"));
    check("image pixels readable for virtual placement", v1.is_some_and(|p| p.img_bytes == 100 * 70 * 3) && v2.is_some_and(|p| p.img_bytes == 40 * 40 * 4), "");
    let (cells, rowflag, cellfg, ul) = t.run(|c| {
        let f = c.frame(true);
        let row = &f.rows[2].1;
        let rf = c
            .term
            .grid_ref(Point::Active(PointCoordinate { x: 9, y: 2 }))
            .and_then(|r| r.row())
            .and_then(|r| r.has_kitty_virtual_placeholder())
            .unwrap_or(false);
        let st = c.term.grid_ref(Point::Active(PointCoordinate { x: 0, y: 5 })).unwrap().style().unwrap();
        (row[9..12].to_vec(), rf, f.rows[5].1[0].fg, format!("{:?}", st.underline_color))
    });
    let cps: Vec<Vec<u32>> = cells.iter().map(|c| c.text.chars().map(|ch| ch as u32).collect()).collect();
    check(
        "placeholder cell graphemes = U+10EEEE + row/col diacritics",
        cps[0] == vec![0x10EEEE, 0x305, 0x305] && cps[1] == vec![0x10EEEE, 0x305, 0x30D],
        format!("{:x?}", cps),
    );
    check("image id recoverable from 24-bit fg", cells[0].fg == Some([0xab, 0xcd, 0xef]), format!("{:?}", cells[0].fg));
    check("row flag has_kitty_virtual_placeholder", rowflag, rowflag);
    check("palette fg (id 42) + underline colour (placement 5) readable", cellfg.is_some() && ul.contains("Palette") && ul.contains('5'), format!("fg={cellfg:?} ul={ul}"));
    println!("  replies(q=2 suppressed)={replies:?}");
}

fn keys() {
    println!("== key encoding (kitty flags 0/1/31, press/repeat/release)");
    let t = TermThread::spawn(80, 24, CW, CH);
    type V = (&'static str, Key, Mods, Option<&'static str>, char);
    let vecs: Vec<V> = vec![
        ("a", Key::A, Mods::empty(), Some("a"), 'a'),
        ("shift+a", Key::A, Mods::SHIFT, Some("A"), 'a'),
        ("ctrl+c", Key::C, Mods::CTRL, None, 'c'),
        ("alt+x", Key::X, Mods::ALT, Some("x"), 'x'),
        ("enter", Key::Enter, Mods::empty(), None, '\0'),
        ("esc", Key::Escape, Mods::empty(), None, '\0'),
        ("tab", Key::Tab, Mods::empty(), None, '\0'),
        ("backspace", Key::Backspace, Mods::empty(), None, '\0'),
        ("up", Key::ArrowUp, Mods::empty(), None, '\0'),
        ("ctrl+up", Key::ArrowUp, Mods::CTRL, None, '\0'),
        ("f5", Key::F5, Mods::empty(), None, '\0'),
        ("ctrl+[", Key::BracketLeft, Mods::CTRL, None, '['),
        ("ctrl+i", Key::I, Mods::CTRL, None, 'i'),
        ("shift+enter", Key::Enter, Mods::SHIFT, None, '\0'),
    ];
    let mut table = Vec::new();
    for flags in [0u8, 1, 31] {
        t.write(format!("\x1b[<u\x1b[={flags};1u"));
        let got = t.run(|c| c.term.kitty_keyboard_flags().unwrap().bits());
        assert_eq!(got, flags);
        for (name, k, m, u, un) in &vecs {
            let mut row = Vec::new();
            for a in [KA::Press, KA::Repeat, KA::Release] {
                let (k, m, u, un) = (*k, *m, *u, *un);
                let b = t.run(move |c| c.key(k, m, a, u, un));
                row.push(esc(&b));
            }
            table.push((flags, *name, row));
        }
    }
    for (f, n, r) in &table {
        println!("  flags={f:<2} {n:<12} press={:<16} repeat={:<16} release={}", r[0], r[1], r[2]);
    }
    let get = |f: u8, n: &str| table.iter().find(|x| x.0 == f && x.1 == n).unwrap().2.clone();
    check("flags0 a / release empty", get(0, "a") == ["a", "a", ""], format!("{:?}", get(0, "a")));
    check("flags0 ctrl+c = ETX", get(0, "ctrl+c")[0] == "\\x03", format!("{:?}", get(0, "ctrl+c")));
    check("flags0 alt+x = ESC x (option-as-alt=true)", get(0, "alt+x")[0] == "\\ex", format!("{:?}", get(0, "alt+x")));
    check("flags0 ctrl+up xterm", get(0, "ctrl+up")[0] == "\\e[1;5A", format!("{:?}", get(0, "ctrl+up")));
    check("flags1 esc = CSI 27 u", get(1, "esc")[0] == "\\e[27u", format!("{:?}", get(1, "esc")));
    check("flags1 ctrl+c = CSI 99;5u", get(1, "ctrl+c")[0] == "\\e[99;5u", format!("{:?}", get(1, "ctrl+c")));
    check("flags31 a press/repeat/release", get(31, "a") == ["\\e[97;;97u", "\\e[97;1:2;97u", "\\e[97;1:3u"], format!("{:?}", get(31, "a")));
    check("flags31 shift+a alternates", get(31, "shift+a")[0] == "\\e[97:65;2;65u", format!("{:?}", get(31, "shift+a")));
    check("flags31 enter as CSI u w/ release", get(31, "enter") == ["\\e[13u", "\\e[13;1:2u", "\\e[13;1:3u"], format!("{:?}", get(31, "enter")));
    check("flags31 up release", get(31, "up")[2] == "\\e[1;1:3A", format!("{:?}", get(31, "up")));
    // Known divergence from kitty legacy mode (deliberate upstream): DIE shim needed
    let ctrl_br = get(0, "ctrl+[")[0].clone();
    check("KNOWN-GAP legacy ctrl+[ (kitty sends ESC)", ctrl_br == "\\e", format!("ghostty sends {ctrl_br}"));
    let ctrl_i = get(0, "ctrl+i")[0].clone();
    check("KNOWN-GAP legacy ctrl+i (kitty sends TAB)", ctrl_i == "\\t", format!("ghostty sends {ctrl_i}"));
    let se = get(0, "shift+enter")[0].clone();
    check("KNOWN-GAP legacy shift+enter (kitty sends CR)", se == "\\r", format!("ghostty sends {se}"));
}

fn mouse_all() {
    println!("== mouse encoding (C API mouse encoder)");
    let t = TermThread::spawn(80, 24, CW, CH);
    let px = |cx: u32, cy: u32| ((cx * CW + 4) as f32, (cy * CH + 7) as f32);
    let modes = [("X10(9)", 9u16), ("normal(1000)", 1000), ("button(1002)", 1002), ("any(1003)", 1003)];
    let fmts = [("x10", 0u16), ("utf8(1005)", 1005), ("sgr(1006)", 1006), ("urxvt(1015)", 1015), ("sgr-px(1016)", 1016)];
    let mut res = std::collections::BTreeMap::new();
    for (mn, m) in modes {
        for (fname, f) in fmts {
            t.write(format!("\x1b[?9l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?1016l\x1b[?{m}h"));
            if f != 0 {
                t.write(format!("\x1b[?{f}h"));
            }
            let (p0x, p0y) = px(2, 3);
            let (p1x, p1y) = px(5, 3);
            let (p2x, p2y) = px(6, 4);
            let (fx, fy) = px(250, 3); // beyond X10 223 limit (screen is 80 cols, so clamp/oob)
            let out = t.run(move |c| {
                let mut o = Vec::new();
                o.push(("press L", c.mouse(MA::Press, Some(Button::Left), Mods::empty(), p0x, p0y, false)));
                o.push(("drag L", c.mouse(MA::Motion, Some(Button::Left), Mods::empty(), p1x, p1y, true)));
                o.push(("release L", c.mouse(MA::Release, Some(Button::Left), Mods::empty(), p1x, p1y, false)));
                o.push(("motion", c.mouse(MA::Motion, None, Mods::empty(), p2x, p2y, false)));
                o.push(("wheel up", c.mouse(MA::Press, Some(Button::Four), Mods::empty(), p0x, p0y, false)));
                o.push(("ctrl+shift R", c.mouse(MA::Press, Some(Button::Right), Mods::CTRL | Mods::SHIFT, p0x, p0y, false)));
                o.push(("oob x", c.mouse(MA::Press, Some(Button::Left), Mods::empty(), fx, fy, false)));
                let tracking = c.term.is_mouse_tracking().unwrap();
                (o, tracking)
            });
            let line: Vec<String> = out.0.iter().map(|(n, b)| format!("{n}={}", esc(b))).collect();
            println!("  {mn:<13} {fname:<12} tracking={} {}", out.1, line.join("  "));
            res.insert((m, f), out.0.into_iter().map(|(n, b)| (n, esc(&b))).collect::<Vec<_>>());
        }
    }
    let g = |m: u16, f: u16, n: &str| res[&(m, f)].iter().find(|x| x.0 == n).unwrap().1.clone();
    check("X10(9): press only, no mods, no release", g(9, 0, "press L") == "\\e[M #$" && g(9, 0, "release L").is_empty() && g(9, 0, "drag L").is_empty(), format!("{} / {}", g(9, 0, "press L"), g(9, 0, "release L")));
    check("normal(1000) X10 bytes: press+release(3), no motion", g(1000, 0, "press L") == "\\e[M #$" && g(1000, 0, "release L") == "\\e[M#&$" && g(1000, 0, "drag L").is_empty(), format!("{} {}", g(1000, 0, "press L"), g(1000, 0, "release L")));
    check("SGR(1006) press/release/wheel", g(1000, 1006, "press L") == "\\e[<0;3;4M" && g(1000, 1006, "release L") == "\\e[<0;6;4m" && g(1000, 1006, "wheel up") == "\\e[<64;3;4M", format!("{} {} {}", g(1000, 1006, "press L"), g(1000, 1006, "release L"), g(1000, 1006, "wheel up")));
    check("SGR mods ctrl+shift right = 2+4+16", g(1000, 1006, "ctrl+shift R") == "\\e[<22;3;4M", g(1000, 1006, "ctrl+shift R"));
    check("button(1002): drag reported, bare motion not", g(1002, 1006, "drag L") == "\\e[<32;6;4M" && g(1002, 1006, "motion").is_empty(), format!("{} / {:?}", g(1002, 1006, "drag L"), g(1002, 1006, "motion")));
    check("any(1003): bare motion reported (35)", g(1003, 1006, "motion") == "\\e[<35;7;5M", g(1003, 1006, "motion"));
    check("SGR-pixels(1016): pixel coords", g(1000, 1016, "press L") == format!("\\e[<0;{};{}M", 2 * CW + 4 + 1, 3 * CH + 7 + 1) || g(1000, 1016, "press L") == format!("\\e[<0;{};{}M", 2 * CW + 4, 3 * CH + 7), g(1000, 1016, "press L"));
    check("urxvt(1015)", g(1000, 1015, "press L") == "\\e[32;3;4M", g(1000, 1015, "press L"));
    check("utf8(1005) press", g(1000, 1005, "press L") == "\\e[M #$", g(1000, 1005, "press L"));
    // tracking off -> encoder writes nothing (DIE handles selection/scroll locally)
    t.write("\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?9l");
    let off = t.run(move |c| c.mouse(MA::Press, Some(Button::Left), Mods::empty(), 10.0, 10.0, false));
    check("tracking off => no bytes", off.is_empty(), esc(&off));
    let shape = t.run(|c| {
        c.write(b"\x1b]22;pointer\x1b\\");
        format!("{:?}", c.term.mouse_shape().unwrap())
    });
    check("OSC 22 mouse shape", shape == "Pointer", shape);
}

fn paste_focus() {
    println!("== bracketed paste + focus reporting");
    let t = TermThread::spawn(80, 24, CW, CH);
    let do_paste = |t: &TermThread, text: &'static str, unsafe_ok: bool| -> (String, String) {
        let r = t.run(move |c| {
            let mimes = [ClipboardMime::new("text/plain")];
            let res = c.term.paste(
                paste::Options::new().with_allow_unsafe(unsafe_ok),
                &mimes,
                |_m: &str, w: &mut dyn std::io::Write| w.write_all(text.as_bytes()),
            );
            format!("{res:?}")
        });
        (r, t.pty_replies())
    };
    let (r, o) = do_paste(&t, "hello", false);
    check("unbracketed safe paste", o == "hello", format!("{r} {o}"));
    let (r, o) = do_paste(&t, "rm -rf ~\n", false);
    check("unbracketed newline paste rejected", r.contains("Rejected") && o.is_empty(), format!("{r} {o:?}"));
    let (r, o) = do_paste(&t, "a\nb", true);
    check("allow_unsafe: newline -> CR", o == "a\\rb", format!("{r} {o}"));
    t.write("\x1b[?2004h");
    let (r, o) = do_paste(&t, "a\nb", false);
    check("bracketed paste wraps 200~/201~", o == "\\e[200~a\\nb\\e[201~", format!("{r} {o}"));
    let (r, o) = do_paste(&t, "x\x1b[201~y", false);
    check("bracketed paste with terminator rejected", r.contains("Rejected"), format!("{r} {o:?}"));
    // focus
    let off = t.run(|c| c.term.mode(Mode::FOCUS_EVENT).unwrap());
    t.write("\x1b[?1004h");
    let on = t.run(|c| c.term.mode(Mode::FOCUS_EVENT).unwrap());
    let mut buf = [0u8; 8];
    let n = focus::Event::Gained.encode(&mut buf).unwrap();
    let gi = esc(&buf[..n]);
    let n = focus::Event::Lost.encode(&mut buf).unwrap();
    let lo = esc(&buf[..n]);
    check("focus mode tracked + encoder", !off && on && gi == "\\e[I" && lo == "\\e[O", format!("off={off} on={on} {gi} {lo}"));
}

fn osc_callbacks() {
    println!("== OSC callbacks");
    let t = TermThread::spawn(80, 24, CW, CH);
    t.write("\x1b]2;my title\x07\x1b]7;file://host.local/Users/me/proj\x1b\\");
    t.write("\x1b]52;c;aGVsbG8gY2xpcA==\x07\x1b]52;c;?\x07");
    t.write("\x1b]9;plain osc9\x07\x1b]777;notify;Build;done ok\x07\x1b]9;4;1;42\x07\x1b]9;4;0\x07");
    t.write("\x1b]99;i=1:d=0;kitty notif\x1b\\\x1b]66;s=2;BIG\x1b\\\x07");
    let fx = t.drain();
    for e in &fx {
        println!("  effect: {e:?}");
    }
    let has = |p: &dyn Fn(&Effect) -> bool| fx.iter().any(p);
    check("OSC 2 title", has(&|e| matches!(e, Effect::Title(s) if s == "my title")), "");
    check("OSC 7 pwd", has(&|e| matches!(e, Effect::Pwd(s) if s.contains("/Users/me/proj"))), "");
    check("OSC 52 write", has(&|e| matches!(e, Effect::ClipWrite{mime_data, ..} if mime_data.iter().any(|(_, d)| d == b"hello clip"))), "");
    check("OSC 52 read -> callback (sync, on parser thread)", has(&|e| matches!(e, Effect::ClipRead{..})), "");
    check("OSC 9 notify", has(&|e| matches!(e, Effect::Notify{body, ..} if body == "plain osc9")), "");
    check("OSC 777 notify", has(&|e| matches!(e, Effect::Notify{title, body} if title == "Build" && body == "done ok")), "");
    check("OSC 9;4 progress", has(&|e| matches!(e, Effect::Progress{pct: Some(42), ..})), "");
    check("KNOWN-GAP OSC 99 kitty notification", has(&|e| matches!(e, Effect::Notify{body, ..} if body.contains("kitty notif"))), "dropped: no callback fires");
    let after66 = t.run(|c| c.text(true));
    check("KNOWN-GAP OSC 66 text sizing", after66.contains("BIG"), format!("screen={:?}", after66.trim()));
    // OSC 133 semantic prompts: readable per row; exit status needs the SEMANTIC_PROMPT callback (not wrapped)
    let t = TermThread::spawn(80, 24, CW, CH);
    t.write("\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07out1\r\nout2\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ \x1b]133;B\x07");
    let (sem, at_prompt) = t.run(|c| {
        let mut v = Vec::new();
        for y in 0..4u32 {
            let r = c.term.grid_ref(Point::Active(PointCoordinate { x: 0, y })).unwrap().row().unwrap();
            let cell = c.term.grid_ref(Point::Active(PointCoordinate { x: 0, y })).unwrap().cell().unwrap();
            v.push(format!("{:?}/{:?}", r.semantic_prompt().unwrap(), cell.semantic_content().unwrap()));
        }
        (v, c.term.is_cursor_at_prompt().unwrap())
    });
    println!("  rows: {sem:?} cursor_at_prompt={at_prompt}");
    check("OSC 133 prompt/output marks readable", sem[0].starts_with("Prompt") && sem[1].contains("Output") && sem[3].starts_with("Prompt") && at_prompt, format!("{sem:?}"));
}

fn replies_sync() {
    println!("== replies: XTWINOPS (physical px), DA, XTVERSION, sync output 2026");
    let t = TermThread::spawn(100, 30, CW, CH);
    for (q, want) in [
        ("\x1b[14t", format!("\\e[4;{};{}t", 30 * CH, 100 * CW)),
        ("\x1b[16t", format!("\\e[6;{CH};{CW}t")),
        ("\x1b[18t", "\\e[8;30;100t".to_string()),
        ("\x1b[19t", "\\e[9;30;100t".to_string()),
        ("\x1b[>q", "\\eP>|DIE(0.1.0)\\e\\".to_string()),
        ("\x1b[?2026$p", "\\e[?2026;2$y".to_string()),
        ("\x1b[?u", "\\e[?0u".to_string()),
    ] {
        t.write(q);
        let r = t.pty_replies();
        check(&format!("reply {}", esc(q.as_bytes())), r == want, format!("got {r} want {want}"));
    }
    t.write("\x1b[c");
    println!("  DA1 -> {}", t.pty_replies());
    t.write("\x1b[?2026hframe\x1b[?2026l");
    let fx = t.drain();
    let holds: Vec<bool> = fx.iter().filter_map(|e| if let Effect::Hold(h) = e { Some(*h) } else { None }).collect();
    check("mode 2026 render-hold callback begin/end", holds == [true, false], format!("{holds:?}"));
    // resize updates XTWINOPS 14t
    t.run(|c| c.resize(50, 10));
    t.write("\x1b[14t");
    let r = t.pty_replies();
    check("14t after resize", r == format!("\\e[4;{};{}t", 10 * CH, 50 * CW), r);
    // in-band resize 2048
    t.write("\x1b[?2048h");
    let r0 = t.pty_replies();
    t.run(|c| c.resize(60, 12));
    let r1 = t.pty_replies();
    println!("  mode 2048 enable -> {r0}; after resize -> {r1}");
}

fn image_limit() {
    println!("== image storage limit");
    let t = TermThread::spawn(80, 24, CW, CH);
    let (default_limit, set1) = t.run(|c| {
        let fresh = Terminal::new(80, 24).unwrap();
        let d = fresh.kitty_image_storage_limit().unwrap();
        c.term.set_kitty_image_storage_limit(1_000_000).unwrap();
        (d, c.term.kitty_image_storage_limit().unwrap())
    });
    println!("  default limit on fresh Terminal = {default_limit} bytes; after set = {set1}");
    let rgba = vec![7u8; 600 * 600 * 4];
    for ch in kitty_chunked("a=t,f=32,s=600,v=600,i=3", &rgba) {
        t.write(ch);
    }
    let r = t.pty_replies();
    check("over-limit image refused (1.44MB > 1MB)", r.contains("ENOMEM") || r.contains("EFBIG") || !r.contains("OK"), &r);
    t.run(|c| {
        c.term.set_kitty_image_storage_limit(0).unwrap();
    });
    t.write("\x1b_Gi=4,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\");
    let r = t.pty_replies();
    check("limit 0 disables protocol (query unanswered)", !r.contains("OK"), format!("{r:?}"));
}

fn threads_and_snapshot() {
    println!("== thread-per-terminal + snapshot restart");
    let ts: Vec<TermThread> = (0..8).map(|_| TermThread::spawn(120, 40, CW, CH)).collect();
    let t0 = std::time::Instant::now();
    let hs: Vec<_> = ts
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            std::thread::spawn(move || {
                let png = png_rgba(320, 200, |x, y| [x as u8, y as u8, i as u8, 255]);
                for k in 0..200 {
                    t.write(format!("\x1b[3{}mterm {i} line {k}\x1b[0m\r\n", k % 8));
                    if k % 50 == 0 {
                        for ch in kitty_chunked(&format!("q=2,a=T,f=100,i={}", k + 1), &png) {
                            t.write(ch);
                        }
                    }
                    let _ = t.run(|c| c.frame(false));
                }
                t
            })
        })
        .collect();
    let ts: Vec<TermThread> = hs.into_iter().map(|h| h.join().unwrap()).collect();
    let n: usize = ts.iter().map(|t| t.run(|c| c.placements().len())).sum();
    check("8 terminals on 8 parser threads, concurrent writes+frames+PNG", n == 8 * 4, format!("{n} placements in {:?}", t0.elapsed()));
    drop(ts);
    let t = TermThread::spawn(80, 24, CW, CH);
    let mut s = String::new();
    for i in 0..3000 {
        s.push_str(&format!("\x1b[3{}mhist {i}\x1b[0m\r\n", i % 8));
    }
    s.push_str("\x1b[1;31mpartial\x1b[");
    t.write(s);
    let (same, len, dt) = t.run(|c| {
        let before = c.text(true);
        let t0 = std::time::Instant::now();
        let mut buf = Vec::new();
        c.term.encode_snapshot(&mut buf).unwrap();
        let mut dec = libghostty_vt::snapshot::Decoder::new_buf(&buf).unwrap();
        dec.set_max_continuation_bytes(4096).unwrap();
        let mut t2 = dec.decode().unwrap();
        let dt = t0.elapsed();
        t2.vt_write(b"32mX");
        let after = {
            let mut f = libghostty_vt::fmt::Formatter::new(&t2, libghostty_vt::fmt::FormatterOptions::new().with_unwrap(true).with_trim(true)).unwrap();
            String::from_utf8_lossy(&f.format_alloc(None).unwrap()).into_owned()
        };
        (after.trim_end().strip_suffix('X').map(|a| a.trim_end()) == Some(before.trim_end()) || after.starts_with(before.trim_end()), buf.len(), dt)
    });
    check("snapshot encode/decode round trip incl. split SGR", same, format!("{len} bytes, {dt:?}"));
}
