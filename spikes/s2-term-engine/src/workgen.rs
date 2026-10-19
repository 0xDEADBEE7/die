//! Deterministic workload generators. They run as the program on the PTY (`s2 gen ...`)
//! and write to stdout, so bytes go through the kernel PTY exactly as a real program's do.
//! Markers carry the writer's clock (CLOCK_UPTIME_RAW ns) for end-to-end latency.
use crate::util::{Rng, now_ns};
use std::io::Write;

fn out() -> std::fs::File {
    use std::os::fd::FromRawFd;
    // SAFETY: fd 1 is our stdout (the PTY slave); ManuallyDrop-like use for process lifetime.
    unsafe { std::fs::File::from_raw_fd(1) }
}

fn title(o: &mut impl Write, s: &str) {
    write!(o, "\x1b]2;{s}\x07").unwrap();
}

/// w2: `bytes` of mixed build-log text, as fast as the PTY takes it.
pub fn w2(bytes: usize) {
    let mut o = out();
    let mut rng = Rng(0x5eed_0002);
    let mut block = Vec::with_capacity(1 << 20);
    let crates = ["serde", "libc", "tokio", "syn", "proc-macro2", "png", "die-core", "die-term"];
    let mut i = 0u64;
    while block.len() < (1 << 20) {
        i += 1;
        match rng.below(10) {
            0..=3 => {
                let c = crates[rng.below(crates.len() as u64) as usize];
                write!(block, "\x1b[1m\x1b[32m   Compiling\x1b[0m {c} v0.{}.{} (/Users/me/src/{c})\r\n", rng.below(30), rng.below(9)).unwrap();
            }
            4 => {
                write!(block, "\x1b[1m\x1b[33mwarning\x1b[0m\x1b[1m: unused variable: `x{i}`\x1b[0m\r\n").unwrap();
                write!(block, "  \x1b[1m\x1b[94m-->\x1b[0m src/lib.rs:{}:{}\r\n", rng.below(900), rng.below(80)).unwrap();
            }
            5 => {
                // a long line that wraps at most widths
                let n = 120 + rng.below(200) as usize;
                for k in 0..n {
                    block.push(b'a' + ((k as u64 + i) % 26) as u8);
                }
                block.extend_from_slice(b"\r\n");
            }
            6 => block.extend_from_slice(b"y\r\ny\r\ny\r\ny\r\n"),
            _ => {
                write!(block, "test tests::case_{i:05} ... \x1b[32mok\x1b[0m  ({}ms) \u{2713} \u{4e16}\u{754c}\r\n", rng.below(500)).unwrap();
            }
        }
    }
    std::thread::sleep(std::time::Duration::from_secs(1)); // let DIE start listening
    title(&mut o, &format!("W2-START {}", now_ns()));
    let mut left = bytes;
    while left > 0 {
        let n = left.min(block.len());
        o.write_all(&block[..n]).unwrap();
        left -= n;
    }
    title(&mut o, &format!("W2-DONE {}", now_ns()));
    std::thread::sleep(std::time::Duration::from_secs(3600)); // keep the PTY open
}

pub const BOX: [char; 11] = ['─', '│', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼'];
pub const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
pub const BRAILLE: [char; 8] = ['⣀', '⣤', '⣶', '⣿', '⡀', '⡄', '⡆', '⡇'];

/// w3: `frames` full-screen redraws (btop-like) at `fps`, each inside a synchronized-output
/// block, with "#F<frame> T<ns>#" in row 1.
pub fn w3(frames: usize, fps: f64, cols: usize, rows: usize) {
    let mut o = out();
    let mut rng = Rng(0x5eed_0003);
    let period = (1e9 / fps) as u64;
    write!(o, "\x1b[?1049h\x1b[?25l").unwrap();
    let t0 = now_ns() + 1_000_000_000; // let DIE start listening
    let mut buf = Vec::with_capacity(256 * 1024);
    for k in 0..frames {
        buf.clear();
        buf.extend_from_slice(b"\x1b[?2026h");
        for y in 1..rows {
            write!(buf, "\x1b[{};1H", y + 1).unwrap();
            let mut x = 0;
            while x < cols {
                let run = (4 + rng.below(12) as usize).min(cols - x);
                let (r, g, b) = (rng.below(256), rng.below(256), rng.below(256));
                write!(buf, "\x1b[38;2;{r};{g};{b}m").unwrap();
                if rng.below(4) == 0 {
                    write!(buf, "\x1b[48;5;{}m", 232 + rng.below(24)).unwrap();
                }
                for i in 0..run {
                    let ch = if y == 1 || y == rows - 1 {
                        BOX[0]
                    } else if x + i == 0 || x + i == cols - 1 {
                        BOX[1]
                    } else {
                        match rng.below(3) {
                            0 => BARS[rng.below(8) as usize],
                            1 => BRAILLE[rng.below(8) as usize],
                            _ => (b'0' + rng.below(10) as u8) as char,
                        }
                    };
                    let mut tmp = [0u8; 4];
                    buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                }
                buf.extend_from_slice(b"\x1b[0m");
                x += run;
            }
        }
        let due = t0 + k as u64 * period;
        loop {
            let left = due.saturating_sub(now_ns());
            if left == 0 {
                break;
            }
            if left > 2_000_000 {
                std::thread::sleep(std::time::Duration::from_nanos(left - 1_000_000));
            }
        }
        let mut head = Vec::new();
        write!(head, "\x1b[1;1H\x1b[0m#F{k:06} T{:020}#", now_ns()).unwrap();
        // The marker goes first inside the sync block so the frame shows it with the rest.
        let mut frame = Vec::with_capacity(buf.len() + 64);
        frame.extend_from_slice(b"\x1b[?2026h");
        frame.extend_from_slice(&head);
        frame.extend_from_slice(&buf[8..]);
        frame.extend_from_slice(b"\x1b[?2026l");
        o.write_all(&frame).unwrap();
    }
    title(&mut o, &format!("W3-DONE {}", now_ns()));
    std::thread::sleep(std::time::Duration::from_secs(3600));
}

pub fn w4_pixels(kind: &str) -> (u32, u32, Vec<u8>) {
    let (w, h) = if kind == "rgba4k" { (3840u32, 2160u32) } else { (1920, 1080) };
    let mut rng = Rng(0x5eed_0004);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let n = (rng.next() & 0x1f) as u8;
            px.extend_from_slice(&[(x * 255 / w) as u8 ^ n, (y * 255 / h) as u8, ((x ^ y) & 0xff) as u8, 255]);
        }
    }
    (w, h, px)
}

pub fn png_encode(w: u32, h: u32, px: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(px).unwrap();
    }
    out
}

/// Kitty graphics APC, chunked at 4096 base64 bytes like `kitten icat`.
pub fn kitty_apc(ctrl: &str, payload: &[u8], o: &mut impl Write) {
    let enc = crate::dterm::b64(payload);
    let chunks: Vec<&[u8]> = enc.as_bytes().chunks(4096).collect();
    for (i, c) in chunks.iter().enumerate() {
        let more = (i + 1 < chunks.len()) as u8;
        if i == 0 {
            write!(o, "\x1b_G{ctrl},m={more};").unwrap();
        } else {
            write!(o, "\x1b_Gm={more};").unwrap();
        }
        o.write_all(c).unwrap();
        o.write_all(b"\x1b\\").unwrap();
    }
}

/// w4: one image by direct transmission: `rgba4k` (raw RGBA) or `png1080` (PNG).
pub fn w4(kind: &str, id: u32) {
    let (w, h, px) = w4_pixels(kind);
    let payload = if kind == "rgba4k" { px } else { png_encode(w, h, &px) };
    let ctrl = if kind == "rgba4k" {
        format!("q=2,a=T,f=32,s={w},v={h},i={id},c=40,r=12")
    } else {
        format!("q=2,a=T,f=100,i={id},c=40,r=12")
    };
    let mut o = std::io::BufWriter::with_capacity(1 << 20, out());
    std::thread::sleep(std::time::Duration::from_secs(1)); // let DIE start listening
    write!(o, "\x1b[1;1H").unwrap();
    title(&mut o, &format!("W4-START {} {}", now_ns(), payload.len()));
    o.flush().unwrap();
    kitty_apc(&ctrl, &payload, &mut o);
    o.flush().unwrap();
    title(&mut o, &format!("W4-SENT {}", now_ns()));
    o.flush().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3600));
}

/// `n` numbered lines, then exit.
pub fn lines(n: usize, tag: &str) {
    let mut o = std::io::BufWriter::new(out());
    for i in 0..n {
        writeln!(o, "{tag} line {i:05} \x1b[3{}mcolour\x1b[0m lorem ipsum dolor sit amet", i % 8).unwrap();
    }
    o.flush().unwrap();
}

/// A small PNG shown with a direct placement, then exit.
pub fn img_small(id: u32) {
    let (w, h) = (64u32, 48u32);
    let px: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, (i / 7 % 256) as u8, 200, 255]).collect();
    let png = png_encode(w, h, &px);
    let mut o = std::io::BufWriter::new(out());
    kitty_apc(&format!("q=2,a=T,f=100,i={id},c=8,r=3"), &png, &mut o);
    writeln!(o).unwrap();
    o.flush().unwrap();
}

pub fn img_small_pixels() -> Vec<u8> {
    let (w, h) = (64u32, 48u32);
    (0..w * h).flat_map(|i| [(i % 251) as u8, (i / 7 % 256) as u8, 200, 255]).collect()
}
