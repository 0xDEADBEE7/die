//! The pictures a person would see: your config, real keys, and the renderer's offscreen path,
//! with no window. `DIE_SHOTS_DIR=dir cargo test -p die-dev --test looks -- --ignored` writes a
//! PNG of each scene there to look at.

use die_test_cap as _;

use die::core::{Ctx, Input};
use die::render::driver::{Config as Font, Renderer};
use die::render::frame::Image;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const COLS: usize = 110;
const ROWS: usize = 38;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A PNG with no compression, so this needs no dependency for it.
fn png(img: &Image, path: &Path) {
    fn crc(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = vec![];
    for y in 0..img.h {
        raw.push(0);
        for x in 0..img.w {
            raw.extend_from_slice(&img.rgb(x, y));
        }
    }
    let mut z = vec![0x78, 1];
    let blocks = raw.chunks(65535);
    let count = blocks.len();
    for (i, block) in blocks.enumerate() {
        z.push(u8::from(i + 1 == count));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + u32::from(v)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = vec![];
    ihdr.extend_from_slice(&img.w.to_be_bytes());
    ihdr.extend_from_slice(&img.h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out).unwrap();
}

struct Shots {
    dir: PathBuf,
    font: Font,
    renderer: Renderer,
}

impl Shots {
    /// Your DIE on `files`, at the owner's size.
    fn open(&self, files: &[PathBuf]) -> Ctx {
        let mut config = die_dev::config(files.to_vec());
        config.font = self.font.clone();
        let mut ctx = die::start(config).unwrap();
        ctx.handle(Input::Resize {
            cols: COLS,
            rows: ROWS,
        });
        settle(&mut ctx);
        ctx
    }

    fn shot(&mut self, ctx: &mut Ctx, name: &str) -> Image {
        settle(ctx);
        let scene = ctx.scene();
        let (w, h) = self.renderer.cell().grid(scene.cols, scene.rows);
        let img = self.renderer.render(&scene, w, h).unwrap();
        png(&img, &self.dir.join(format!("{name}.png")));
        img
    }
}

/// Whether a picture has this color from mono-dark anywhere.
fn has(img: &Image, [r, g, b]: [u8; 3]) -> bool {
    img.bgra.chunks_exact(4).any(|p| p[..3] == [b, g, r])
}

/// Run until the keys are done and a background job has had time to answer.
fn settle(ctx: &mut Ctx) {
    let until = Instant::now() + Duration::from_millis(400);
    while Instant::now() < until {
        if !ctx.run_ready() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn keys(ctx: &mut Ctx, keys: &str) {
    ctx.feed_keys(keys).unwrap();
    settle(ctx);
}

#[test]
#[ignore = "writes pictures: set DIE_SHOTS_DIR"]
fn every_scene_to_a_png() {
    let Some(dir) = std::env::var_os("DIE_SHOTS_DIR").map(PathBuf::from) else {
        return;
    };
    let font = Font::term(16.0, 2.0);
    let Ok(renderer) = Renderer::new(&font) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Shots {
        dir: dir.clone(),
        font,
        renderer,
    };
    let source = repo().join("crates/die-core/src/prompt/draw.rs");
    let src = source.canonicalize().unwrap();

    // One file of DIE's Rust source.
    let mut c = s.open(std::slice::from_ref(&src));
    let img = s.shot(&mut c, "source");
    assert!(has(&img, [0x1a; 3]), "bg");
    assert!(has(&img, [0x25; 3]), "mode-line");
    assert!(has(&img, [0xcc; 3]), "cursor");

    // Cursors, on the same file.
    keys(&mut c, "20Gw");
    s.shot(&mut c, "cursor-normal");
    keys(&mut c, "i");
    s.shot(&mut c, "cursor-insert");
    keys(&mut c, "<Esc>R");
    s.shot(&mut c, "cursor-replace");
    keys(&mut c, "<Esc>");
    c.handle(Input::Focus(false));
    s.shot(&mut c, "cursor-unfocused");
    c.handle(Input::Focus(true));

    // Visual.
    keys(&mut c, "vjjll");
    s.shot(&mut c, "visual-char");
    keys(&mut c, "<Esc>Vjj");
    let img = s.shot(&mut c, "visual-line");
    assert!(has(&img, [0x2e; 3]), "region");
    keys(&mut c, "<Esc><C-v>jjjlll");
    s.shot(&mut c, "visual-block");
    keys(&mut c, "<Esc>");

    // Search.
    keys(&mut c, "gg/self");
    s.shot(&mut c, "search-typing");
    keys(&mut c, "<CR>n");
    s.shot(&mut c, "search-current");
    keys(&mut c, ":noh<CR>");

    // Splits.
    keys(&mut c, ":vs<CR>");
    s.shot(&mut c, "split-two");
    keys(&mut c, "<C-w>l:sp<CR>");
    s.shot(&mut c, "split-three");
    keys(&mut c, ":on<CR>");

    // Prompts.
    keys(&mut c, "\\f");
    s.shot(&mut c, "find-file");
    keys(&mut c, "cr");
    s.shot(&mut c, "find-file-typed");
    keys(&mut c, "<Esc><Esc>");
    keys(&mut c, "\\p");
    s.shot(&mut c, "find-file-project");
    keys(&mut c, "<Esc><Esc>");
    keys(&mut c, "\\pprmdrw");
    s.shot(&mut c, "find-file-matches");
    keys(&mut c, "<Esc><Esc>");
    keys(&mut c, "\\x");
    s.shot(&mut c, "palette");
    keys(&mut c, "<Esc><Esc>");
    keys(&mut c, "d");
    s.shot(&mut c, "cursor-operator");
    keys(&mut c, "<Esc>:%s/self/this");
    s.shot(&mut c, "ex-prompt");
    keys(&mut c, "<Esc><Esc>:vs<CR>:vs<CR>");
    s.shot(&mut c, "split-three-across");
    let scene = c.scene();
    let (w, h) = s.renderer.cell().grid(scene.cols, scene.rows);
    let img = s.renderer.render(&scene, w + 13, h + 27).unwrap();
    png(&img, &dir.join("window-between-cells.png"));
    keys(&mut c, ":on<CR>");
    keys(&mut c, "\\hkx");
    s.shot(&mut c, "help");
    keys(&mut c, ":on<CR>");

    // Buffers of DIE's own.
    keys(&mut c, "\\rshape_for<CR>");
    std::thread::sleep(Duration::from_secs(1));
    s.shot(&mut c, "grep");
    keys(&mut c, "<C-w>j");
    s.shot(&mut c, "grep-focused");
    keys(&mut c, "<C-w>k:on<CR>fZ:s/nothing-like-this/x/<CR>");
    keys(&mut c, ":b *messages*<CR>");
    s.shot(&mut c, "messages");

    // A bracket's pair, under a block cursor.
    let mut c = s.open(std::slice::from_ref(&src));
    keys(&mut c, "/draw_prompt_list<CR>f(");
    s.shot(&mut c, "bracket");

    // Text that's hard to draw.
    let tmp = dir.join("input");
    std::fs::create_dir_all(&tmp).unwrap();
    let wrap = tmp.join("wrapped.txt");
    let words = "A long line of plain words that goes on past the eightieth column and wraps at a word, as global visual line mode has it. ".repeat(6);
    let unbroken = "x".repeat(200);
    std::fs::write(&wrap, format!("before\n{words}\nnext\n{unbroken}\nend\n")).unwrap();
    let mut c = s.open(&[wrap]);
    keys(&mut c, "jfwj");
    s.shot(&mut c, "wrapped");
    keys(&mut c, "/xxx<CR>$");
    s.shot(&mut c, "wrapped-line-end");
    keys(&mut c, "0gj");
    keys(&mut c, "Ax<Esc>");
    keys(&mut c, "k$a");
    s.shot(&mut c, "wrapped-insert-at-end");

    let wide = tmp.join("wide.txt");
    std::fs::write(
        &wide,
        "ascii then 日本語のテキスト and 한국어 and 中文 mixed\n\
         emoji 😀 🎉 👩‍👩‍👧‍👦 🇯🇵 ❤️ ✅ end of line\n\
         combining e\u{301} a\u{308} n\u{303} z\u{335} and ligature-free -> => != fi fl\n\
         box ─│┌┐└┘├┤┬┴┼ █▓▒░ ▀▄ braille ⠿⠇ powerline \u{e0b0}\u{e0b2}\n\
         greek αβγδ math ∀∃∑∫√ arrows ←↑→↓ quotes “ ” ‘ ’ — … •\n\
         tab\there\tand\tthere, control \u{1}\u{7f} nbsp\u{a0}x\n\
         日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語\n\
         \n\
         gjpqy|_ baseline descenders GJPQY ()[]{} @#$%&*~^`'\"\n\
         **bold** nothing here, 0O1lI\n",
    )
    .unwrap();
    let mut c = s.open(&[wide]);
    s.shot(&mut c, "wide");
    keys(&mut c, "ggf本");
    s.shot(&mut c, "wide-cursor");
    keys(&mut c, "i");
    s.shot(&mut c, "wide-bar");
    keys(&mut c, "<Esc>jfj3l");
    s.shot(&mut c, "emoji-cursor");
    keys(&mut c, "ggf本v2ljj");
    s.shot(&mut c, "wide-visual");
    keys(&mut c, "<Esc>");

    let num = tmp.join("numbers.py");
    let py: String = (1..=140)
        .map(|i| format!("def function_{i}(self, value):\n    return value + {i}\n"))
        .collect();
    std::fs::write(&num, py).unwrap();
    let mut c = s.open(&[num]);
    keys(&mut c, "130Gzz");
    s.shot(&mut c, "numbers");
    keys(&mut c, ":vs<CR>");
    s.shot(&mut c, "numbers-split");

    // A display that isn't 2x.
    let font = Font::term(16.0, 1.0);
    s.renderer = Renderer::new(&font).unwrap();
    s.font = font;
    let mut c = s.open(std::slice::from_ref(&src));
    keys(&mut c, "/self<CR>");
    s.shot(&mut c, "source-1x");
}
