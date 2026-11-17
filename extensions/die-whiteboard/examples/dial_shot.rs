//! dial_shot: the tool dial as your screen would show it, written as PNGs. For looking at the
//! dial's design without DIE:
//!
//! ```sh
//! cargo run --release --offline -p die-whiteboard --example dial_shot -- OUT_DIR
//! ```
//!
//! A board of strokes, shapes, an arrow and a label in several inks, drawn through the harness
//! and its fake pixel sink under mono-dark's colours (die-dev's, copied here, since an extension
//! doesn't depend on your config) with cells of 19 × 40 pixels on a 2x display, as your 16 pt
//! font has them. Each shot opens the dial, moves the pointer, and composites the frame as the
//! host does: each cell's background, the board's pixel buffer over it, then each cell's glyph
//! in DIE's IoskeleyMonoTerm. It writes the whole frame and a crop around the dial at 1:1, for
//! stage one with nothing hovered and hovering arrow, stage two on the pen (its first band, then
//! ink 2 thick and thin), on text and on the ellipse, and the cell renderer's stage one and two;
//! and prints what a move with the dial open
//! costs, and what drawing its line art alone does, which a change of what's hovered pays.

use die_test_cap as _;

use die_core::{
    Args, CellPixels, Config, Glyph, Harness, Input, Key, KeyEvent, KeyKind, Mods, PointerEvent,
    PointerKind, Rgb, Style, Theme,
};
use die_vim::Vim;
use die_whiteboard::canvas::{Canvas, DialColours, Map, Raster};
use die_whiteboard::dial::{Armed, Dial, DialLook, Opener, RING, toward};
use die_whiteboard::export::encode_png;
use die_whiteboard::fonts::{Fonts, font_dir};
use die_whiteboard::model::{CellSize, Point, Shape};
use die_whiteboard::svg::Look;
use die_whiteboard::{Options, Whiteboard};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Your font's cells at 16 pt on a 2x display.
const CELL: CellPixels = CellPixels {
    width: 19,
    height: 40,
    scale: 2.0,
};
/// A cell in board units at 1:1.
const CW: f32 = 9.5;
const CH: f32 = 20.0;

fn main() {
    if let Err(wrong) = run() {
        eprintln!("dial_shot: {wrong}");
        std::process::exit(1);
    }
}

/// mono-dark's colours that a board and its dial use: die-dev's `theme.rs`.
fn mono_dark() -> Theme {
    let ansi = [
        "#1a1a1a", "#cc4444", "#55aa55", "#b8963e", "#5588aa", "#886688", "#4a8a8a", "#aaaaaa",
        "#444444", "#dd6666", "#77cc77", "#ccaa55", "#7799bb", "#aa88aa", "#66aaaa", "#eeeeee",
    ];
    let s = Style::new;
    let h = Rgb::hex;
    Theme::new("mono-dark", ansi.map(h))
        .face("default", s().fg(h("#cccccc")).bg(h("#1a1a1a")))
        .face("shadow", s().fg(h("#555555")))
        .face("status-line", s().fg(h("#cccccc")).bg(h("#252525")))
        .face(
            "status-line-inactive",
            s().fg(h("#555555")).bg(h("#1a1a1a")),
        )
        .face("whiteboard-dial", s().fg(h("#888888")))
        .face("whiteboard-dial-accent", s().fg(h("#cc4444")))
}

struct Shot {
    h: Harness,
    fonts: Fonts,
    out: PathBuf,
    _dir: TempDir,
}

/// A directory of the example's own, removed when it drops.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run() -> Result<(), String> {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("dial_shot <out-dir>")?;
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let fonts = || Fonts::load(&font_dir(None));
    let centre = Point::new(500.0, 300.0);

    let mut s = Shot::new(true, fonts()?, &out)?;
    s.space(KeyKind::Press, centre);
    s.write("stage1-idle", centre)?;
    s.to(centre + toward(135.0) * (1.9 * CH));
    s.write("stage1-arrow", centre)?;
    s.to(centre + toward(0.0) * (1.6 * CH));
    s.write("stage1-pen", centre)?;
    // Out past the ring, in the first band: the arc open, nothing picked. Then on past the
    // guide marked 2: ink 1 is armed, so straight out keeps it; then round to ink 2's segment
    // (10° to 30°) and out to the thick band.
    s.to(centre + toward(0.0) * (3.6 * CH));
    s.write("stage2-pen-first-band", centre)?;
    s.to(centre + toward(0.0) * (4.8 * CH));
    s.write("stage2-pen-straight", centre)?;
    s.to(centre + toward(20.0) * (7.0 * CH));
    s.write("stage2-pen-ink2-thick", centre)?;
    s.to(centre + toward(20.0) * (4.8 * CH));
    s.write("stage2-pen-ink2-thin", centre)?;
    s.space(KeyKind::Release, centre + toward(20.0) * (4.8 * CH));
    s.space(KeyKind::Press, centre);
    s.to(centre + toward(225.0) * (1.8 * CH));
    s.to(centre + toward(225.0) * (4.5 * CH));
    // With ink 2 armed now, text's arc starts at 195°: ink 5's segment is 275° to 295°.
    s.to(centre + toward(285.0) * (5.0 * CH));
    s.write("stage2-text", centre)?;
    s.space(KeyKind::Release, centre + toward(285.0) * (5.0 * CH));
    // East, the ellipse: its chip runs out along the centre's row past the arc.
    s.space(KeyKind::Press, centre);
    s.to(centre + toward(90.0) * (1.8 * CH));
    s.to(centre + toward(100.0) * (6.0 * CH));
    s.write("stage2-ellipse", centre)?;
    s.space(KeyKind::Release, centre);
    // Near the area's edge: the dial is cut there as the area is.
    let corner = Point::new(60.0, 60.0);
    s.space(KeyKind::Press, corner);
    s.to(corner + toward(90.0) * (1.8 * CH));
    s.write("stage1-corner", corner)?;
    s.space(KeyKind::Release, corner);
    // What a move costs: the needle and the crosshair follow every one. Sweeps of 200 moves,
    // the frame rendered after each, crossing two sectors in stage one and five inks in two.
    s.space(KeyKind::Press, centre);
    s.to(centre + toward(0.0) * (1.6 * CH));
    let n = 200;
    let t = Instant::now();
    for i in 0..n {
        let deg = 300.0 + 120.0 * i as f32 / n as f32;
        s.to(centre + toward(deg) * (1.8 * CH));
    }
    let one = t.elapsed().as_secs_f64() * 1e3 / f64::from(n);
    s.to(centre + toward(0.0) * (4.0 * CH));
    let t = Instant::now();
    for i in 0..n {
        let deg = -10.0 + 120.0 * i as f32 / n as f32;
        s.to(centre + toward(deg) * (6.5 * CH));
    }
    let two = t.elapsed().as_secs_f64() * 1e3 / f64::from(n);
    println!(
        "a move with the dial open, through the harness, frame and all: {one:.2} ms in stage one, {two:.2} ms in stage two"
    );
    s.space(KeyKind::Release, centre);
    draw_cost(centre);

    let mut s = Shot::new(false, fonts()?, &out)?;
    s.space(KeyKind::Press, centre);
    s.to(centre + toward(135.0) * (1.9 * CH));
    s.write("cells-stage1-arrow", centre)?;
    s.to(centre + toward(0.0) * (1.6 * CH));
    s.to(centre + toward(20.0) * (7.0 * CH));
    s.write("cells-stage2-pen", centre)?;
    s.space(KeyKind::Release, centre);
    Ok(())
}

impl Shot {
    /// A board drawn in pixels, or in cells, with some of everything on it, open in a window of
    /// 110 × 32 cells.
    fn new(pixels: bool, fonts: Fonts, out: &Path) -> Result<Shot, String> {
        let exts: Vec<Box<dyn die_core::Extension>> = vec![
            Box::new(Vim::default()),
            Box::new(Whiteboard::new(Options {
                pixels,
                ..Options::default()
            })),
        ];
        let config = Config {
            theme: Some(mono_dark()),
            ..Config::default()
        };
        let mut h = Harness::with_config(exts, config).map_err(|e| e.join("; "))?;
        h.open_text("notes", "").map_err(|e| e.to_string())?;
        h.set_window_size(110, 32);
        h.set_cell_pixels(CELL);
        h.run_until_idle();
        let dir = std::env::temp_dir().join(format!("dial-shot-{}-{pixels}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let args = Args {
            text: Some(dir.join("board.svg").display().to_string()),
            ..Args::default()
        };
        h.run("whiteboard", &args).map_err(|e| e.to_string())?;
        h.run_until_idle();
        let buf = h.focused_buffer().ok_or("no board")?;
        let shared = h
            .with_ext(|wb: &mut Whiteboard, _| wb.board(buf))
            .map_err(|e| format!("{e:?}"))?
            .ok_or("no board")?;
        {
            let mut st = shared.borrow_mut();
            let wave = |x0: f32, y0: f32, len: f32, amp: f32, k: f32| {
                (0..=60)
                    .map(|i| {
                        let t = i as f32 / 60.0;
                        Point::new(x0 + t * len, y0 + amp * (t * k).sin())
                    })
                    .collect::<Vec<_>>()
            };
            let stroke = |points| Shape::Stroke {
                points,
                highlighter: false,
            };
            st.board
                .add(stroke(wave(340.0, 210.0, 320.0, 18.0, 9.0)), 1, 1);
            st.board
                .add(stroke(wave(300.0, 330.0, 380.0, 24.0, 6.0)), 2, 2);
            st.board
                .add(stroke(wave(420.0, 400.0, 200.0, 12.0, 12.0)), 4, 0);
            st.board
                .add(stroke(wave(80.0, 520.0, 300.0, 30.0, 5.0)), 5, 1);
            st.board
                .add(stroke(wave(700.0, 120.0, 260.0, 20.0, 7.0)), 6, 1);
            let p = Point::new;
            st.board.add(
                Shape::Rect {
                    a: p(560.0, 240.0),
                    b: p(760.0, 330.0),
                },
                1,
                1,
            );
            st.board.add(
                Shape::Ellipse {
                    a: p(250.0, 150.0),
                    b: p(420.0, 260.0),
                },
                4,
                1,
            );
            st.board.add(
                Shape::Line {
                    a: p(380.0, 440.0),
                    b: p(620.0, 380.0),
                    arrow: true,
                    from: None,
                    to: None,
                },
                2,
                1,
            );
            st.board.add(
                Shape::Label {
                    at: p(600.0, 260.0),
                    text: "the dial".into(),
                },
                3,
                1,
            );
            st.board.add(
                Shape::Label {
                    at: p(130.0, 470.0),
                    text: "notes".into(),
                },
                1,
                1,
            );
            st.redraw();
        }
        h.scene();
        Ok(Shot {
            h,
            fonts,
            out: out.to_path_buf(),
            _dir: TempDir(dir),
        })
    }

    /// The pointer's place in physical pixels of the frame, for a board point at 1:1.
    fn frame_px(&mut self, p: Point) -> (f32, f32, usize, usize) {
        let w = self.h.focused().expect("a window");
        let pl = self
            .h
            .placements()
            .into_iter()
            .find(|pl| pl.window == w)
            .expect("placed");
        let g = self.h.surface_area(w).expect("a surface");
        let (r0, c0) = (pl.row, pl.col + g.text_x);
        let (x, y) = (p.x * CELL.scale as f32, p.y * CELL.scale as f32);
        let px = (
            x + c0 as f32 * CELL.width as f32,
            y + r0 as f32 * CELL.height as f32,
        );
        let row = r0 + (y / CELL.height as f32).floor().max(0.0) as usize;
        let col = c0 + (x / CELL.width as f32).floor().max(0.0) as usize;
        (px.0, px.1, row, col)
    }

    fn to(&mut self, p: Point) {
        let (x, y, row, col) = self.frame_px(p);
        let errors = self.h.send(Input::Pointer(PointerEvent {
            kind: PointerKind::Move,
            row,
            col,
            clicks: 0,
            mods: Mods::default(),
            px: Some((x, y)),
        }));
        assert!(errors.is_empty(), "{errors:?}");
        self.h.scene();
    }

    fn space(&mut self, kind: KeyKind, at: Point) {
        self.to(at);
        let errors = self
            .h
            .send(Input::Key(KeyEvent::press(Key::Char(' ')).kind(kind)));
        assert!(errors.is_empty(), "{errors:?}");
        self.h.scene();
    }

    /// The frame as the host draws it, and a crop around the dial at `centre`.
    fn write(&mut self, name: &str, centre: Point) -> Result<(), String> {
        let frame = self.compose();
        let path = self.out.join(format!("{name}.png"));
        let png = encode_png(&frame)?;
        std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))?;
        let (x, y, ..) = self.frame_px(centre);
        let reach = (RING + 7.5) * CH * CELL.scale as f32;
        let (w, h) = ((2.0 * reach * 1.3) as u32, (2.0 * reach) as u32);
        let x0 = (x - w as f32 / 2.0).max(0.0) as u32;
        let y0 = (y - h as f32 / 2.0).max(0.0) as u32;
        let crop = crop(&frame, x0, y0, w, h);
        let path = self.out.join(format!("{name}-crop.png"));
        std::fs::write(&path, encode_png(&crop)?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {}", path.display());
        Ok(())
    }

    /// Each cell's background, the pixel buffers placed under the glyphs, then the glyphs.
    fn compose(&mut self) -> Raster {
        let scene = self.h.scene();
        let (cw, ch) = (CELL.width as usize, CELL.height as usize);
        let (w, h) = (scene.cols * cw, scene.rows * ch);
        let mut rgb = vec![0f32; w * h * 3];
        for r in 0..scene.rows {
            for (c, cell) in scene.row(r).iter().enumerate() {
                let Rgb(cr, cg, cb) = cell.bg;
                for y in r * ch..(r + 1) * ch {
                    for x in c * cw..(c + 1) * cw {
                        let i = (y * w + x) * 3;
                        rgb[i..i + 3].copy_from_slice(&[
                            f32::from(cr),
                            f32::from(cg),
                            f32::from(cb),
                        ]);
                    }
                }
            }
        }
        for p in &scene.placements {
            let Some((bw, bh, px)) = self.h.pixels(p.buffer) else {
                continue;
            };
            let (x0, y0) = (p.col * cw, p.row * ch);
            let (pw, ph) = (
                (p.cols * cw).min(bw as usize),
                (p.rows * ch).min(bh as usize),
            );
            for y in 0..ph {
                for x in 0..pw {
                    let s = (y * bw as usize + x) * 4;
                    let a = f32::from(px[s + 3]) / 255.0;
                    if a == 0.0 {
                        continue;
                    }
                    let d = ((y0 + y) * w + x0 + x) * 3;
                    if d + 2 >= rgb.len() {
                        continue;
                    }
                    for k in 0..3 {
                        rgb[d + k] = f32::from(px[s + k]) + rgb[d + k] * (1.0 - a);
                    }
                }
            }
        }
        let size = ch as f32 * 0.8;
        let ascent = self.fonts.ascent(size);
        for r in 0..scene.rows {
            for (c, cell) in scene.row(r).iter().enumerate() {
                let ch_ = match &cell.glyph {
                    Glyph::Char(' ') | Glyph::Continuation => continue,
                    Glyph::Char(c) => *c,
                    Glyph::Cluster(s) => s.chars().next().unwrap_or(' '),
                };
                let (m, cov) = self.fonts.glyph(ch_, size);
                let left = (c * cw) as f32 + (cw as f32 - m.advance_width) / 2.0;
                let baseline = (r * ch) as f32 + (ch as f32 - size * 1.2) / 2.0 + ascent;
                let gx = (left + m.xmin as f32).round() as i64;
                let gy = (baseline - (m.ymin + m.height as i32) as f32).round() as i64;
                let Rgb(fr, fg, fb) = cell.fg;
                for j in 0..m.height {
                    for i in 0..m.width {
                        let a = f32::from(cov[j * m.width + i]) / 255.0;
                        let (x, y) = (gx + i as i64, gy + j as i64);
                        if a == 0.0 || x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                            continue;
                        }
                        let d = (y as usize * w + x as usize) * 3;
                        for (k, f) in [fr, fg, fb].into_iter().enumerate() {
                            rgb[d + k] = f32::from(f) * a + rgb[d + k] * (1.0 - a);
                        }
                    }
                }
            }
        }
        let rgba = rgb
            .chunks_exact(3)
            .flat_map(|p| {
                [p[0], p[1], p[2]]
                    .map(|v| v.round().clamp(0.0, 255.0) as u8)
                    .into_iter()
                    .chain([255])
            })
            .collect();
        Raster {
            width: w as u32,
            height: h as u32,
            rgba,
        }
    }
}

/// What drawing the dial's line art alone costs, into a canvas the size of the shots' area.
fn draw_cost(centre: Point) {
    let cell = CellSize { w: CW, h: CH };
    let mut canvas = Canvas::new(2090, 1280).expect("a canvas");
    let map = Map {
        scale: 2.0,
        tx: 0.0,
        ty: 0.0,
    };
    let look = DialLook::default();
    let inks = Look::default().inks;
    let colours = DialColours {
        look: &look,
        inks: &inks,
        fg: Rgb(0xcc, 0xcc, 0xcc),
    };
    for (stage, deg, r) in [
        ("one", 20.0, 1.8),
        ("two", 20.0, 6.5),
        ("two, east, the chip out along the row", 100.0, 6.5),
    ] {
        let mut d = Dial::open(centre, Opener::Space, cell, 1.0, Armed::default());
        d.hover(centre + toward(deg) * (1.8 * CH));
        d.hover(centre + toward(deg) * (r * CH));
        let layout = d.layout(Point::new(0.0, 0.0), (110, 32), false);
        let n = 100;
        let t = Instant::now();
        for _ in 0..n {
            canvas.target().dial(&d, &layout, map, colours);
        }
        let ms = t.elapsed().as_secs_f64() * 1e3 / f64::from(n);
        let b = map.pixels(&d.bounds(&layout), 2.0, 2090, 1280);
        println!(
            "the line art alone in stage {stage}: {ms:.2} ms over {}x{} px",
            b.width, b.height
        );
    }
}

/// The part of `r` from (x0, y0), `w` × `h`, cut at its edges.
fn crop(r: &Raster, x0: u32, y0: u32, w: u32, h: u32) -> Raster {
    let w = w.min(r.width.saturating_sub(x0));
    let h = h.min(r.height.saturating_sub(y0));
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        let at = ((y * r.width + x0) * 4) as usize;
        rgba.extend_from_slice(&r.rgba[at..at + (w * 4) as usize]);
    }
    Raster {
        width: w,
        height: h,
        rgba,
    }
}
