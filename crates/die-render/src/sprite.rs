//! Glyphs the renderer draws itself, so they join across cells as kitty's and
//! Ghostty's do (scope §5.11): box drawing, blocks, braille and powerline, and the
//! underlines and strike-through. Each is a cell-sized coverage bitmap, from the
//! cell's size in pixels and nothing else, so its tests need no font and no GPU.

use die_core::Underline;
use std::f32::consts::TAU;

const DOUBLE: u8 = 3;

/// The arms of U+2500..=U+257F, as nibbles: up, right, down, left. 0 is none, 1
/// light, 2 heavy, 3 double. Dashed glyphs have the arms of their solid line. Arcs
/// and diagonals aren't drawn from arms, and are 0.
const BOX: [u16; 128] = [
    0x0101, 0x0202, 0x1010, 0x2020, 0x0101, 0x0202, 0x1010, 0x2020, 0x0101, 0x0202, 0x1010, 0x2020,
    0x0110, 0x0210, 0x0120, 0x0220, 0x0011, 0x0012, 0x0021, 0x0022, 0x1100, 0x1200, 0x2100, 0x2200,
    0x1001, 0x1002, 0x2001, 0x2002, 0x1110, 0x1210, 0x2110, 0x1120, 0x2120, 0x2210, 0x1220, 0x2220,
    0x1011, 0x1012, 0x2011, 0x1021, 0x2021, 0x2012, 0x1022, 0x2022, 0x0111, 0x0112, 0x0211, 0x0212,
    0x0121, 0x0122, 0x0221, 0x0222, 0x1101, 0x1102, 0x1201, 0x1202, 0x2101, 0x2102, 0x2201, 0x2202,
    0x1111, 0x1112, 0x1211, 0x1212, 0x2111, 0x1121, 0x2121, 0x2112, 0x2211, 0x1122, 0x1221, 0x2212,
    0x1222, 0x2122, 0x2221, 0x2222, 0x0101, 0x0202, 0x1010, 0x2020, 0x0303, 0x3030, 0x0310, 0x0130,
    0x0330, 0x0013, 0x0031, 0x0033, 0x1300, 0x3100, 0x3300, 0x1003, 0x3001, 0x3003, 0x1310, 0x3130,
    0x3330, 0x1013, 0x3031, 0x3033, 0x0313, 0x0131, 0x0333, 0x1303, 0x3101, 0x3303, 0x1313, 0x3131,
    0x3333, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0001, 0x1000, 0x0100, 0x0010,
    0x0002, 0x2000, 0x0200, 0x0020, 0x0201, 0x1020, 0x0102, 0x2010,
];

/// The cell's pixels, written as coverage. `rect` keeps the most covered value.
struct Canvas {
    w: i32,
    h: i32,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: u32, h: u32) -> Canvas {
        let px = vec![0; w as usize * h as usize];
        let (w, h) = (w as i32, h as i32);
        Canvas { w, h, px }
    }

    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, v: u8) {
        for y in y0.max(0)..y1.min(self.h) {
            for x in x0.max(0)..x1.min(self.w) {
                self.px[(y * self.w + x) as usize] = v;
            }
        }
    }

    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.fill(x0, y0, x1, y1, 255);
    }

    fn clear(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.fill(x0, y0, x1, y1, 0);
    }

    /// Fill a shape given in pixel coordinates, taking 4×4 samples per pixel. A
    /// shape whose edge is on a pixel boundary comes out with no blur.
    fn shape(&mut self, inside: impl Fn(f32, f32) -> bool) {
        for y in 0..self.h {
            for x in 0..self.w {
                let mut n = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let (fx, fy) = (sx as f32 + 0.5, sy as f32 + 0.5);
                        n += u32::from(inside(x as f32 + fx / 4.0, y as f32 + fy / 4.0));
                    }
                }
                let p = &mut self.px[(y * self.w + x) as usize];
                *p = (*p).max((n * 255 / 16) as u8);
            }
        }
    }
}

/// A band `k` thick, centered in `n`, as every line in a cell is. Using one rule
/// everywhere keeps a double line's rails around the place a light line sits.
fn band(n: i32, k: i32) -> (i32, i32) {
    let start = (n - k) / 2;
    (start, start + k)
}

/// Lines of weight 0, 1 or 2 from each arm's edge to the center. An arm goes as
/// far as the far side of the thickest line across it, so corners are square. With
/// no line across it, it stops at the center, where the opposite arm starts.
fn solid_arms(c: &mut Canvas, [up, right, down, left]: [u8; 4], t: i32) {
    let thick = |a: u8| i32::from(a) * t;
    let (vl, vr) = band(c.w, thick(up).max(thick(down)));
    let (ht, hb) = band(c.h, thick(left).max(thick(right)));
    let (w, h) = (c.w, c.h);
    if left > 0 {
        let (y0, y1) = band(h, thick(left));
        c.rect(0, y0, vr, y1);
    }
    if right > 0 {
        let (y0, y1) = band(h, thick(right));
        c.rect(vl, y0, w, y1);
    }
    if up > 0 {
        let (x0, x1) = band(w, thick(up));
        c.rect(x0, 0, x1, hb);
    }
    if down > 0 {
        let (x0, x1) = band(w, thick(down));
        c.rect(x0, ht, x1, h);
    }
}

/// A double line's four edges across `n`: rail, gap, rail, each `t` thick.
fn rails(n: i32, t: i32) -> [i32; 4] {
    let (a, _) = band(n, 3 * t);
    [a, a + t, a + 2 * t, a + 3 * t]
}

/// Both axes double: `═` and `║` meet, as in `╬`. Each arm is a solid band with its
/// gap cut from the edge up to the far side of the center, which leaves the rails
/// open where they should be: `╬` has four corners, and `╦` an unbroken top rail.
fn double_cross(c: &mut Canvas, [up, right, down, left]: [u8; 4], t: i32) {
    let [x0, x1, x2, x3] = rails(c.w, t);
    let [y0, y1, y2, y3] = rails(c.h, t);
    let (w, h) = (c.w, c.h);
    let bands = [
        (right, (x0, y0, w, y3), (x1, y1, w, y2)),
        (left, (0, y0, x3, y3), (0, y1, x2, y2)),
        (down, (x0, y0, x3, h), (x1, y1, x2, h)),
        (up, (x0, 0, x3, y3), (x1, 0, x2, y2)),
    ];
    for (arm, (a, b, e, f), _) in bands {
        if arm > 0 {
            c.rect(a, b, e, f);
        }
    }
    for (arm, _, (a, b, e, f)) in bands {
        if arm > 0 {
            c.clear(a, b, e, f);
        }
    }
}

/// One axis double, the other light or absent. Written for a double horizontal, as
/// in `╤`, with the light line across it; `swap` draws the other axis's, as in `╥`.
/// The rails stop at the light line, which reaches one rail or runs through both.
fn double_line(c: &mut Canvas, arms: [u8; 4], t: i32, swap: bool) {
    let [up, right, down, left] = arms;
    let (before, after, across_a, across_b) = if swap {
        (up, down, left, right)
    } else {
        (left, right, up, down)
    };
    let (long, short) = if swap { (c.h, c.w) } else { (c.w, c.h) };
    let mut put = |x0: i32, y0: i32, x1: i32, y1: i32, v: u8| {
        if swap {
            c.fill(y0, x0, y1, x1, v);
        } else {
            c.fill(x0, y0, x1, y1, v);
        }
    };
    let [y0, y1, y2, y3] = rails(short, t);
    let (lx0, lx1) = band(long, t);
    let across = across_a > 0 || across_b > 0;
    if before > 0 {
        put(0, y0, if across { lx1 } else { long / 2 }, y3, 255);
    }
    if after > 0 {
        put(if across { lx0 } else { long / 2 }, y0, long, y3, 255);
    }
    // With rails both ways and a line on one side only, the far rail is unbroken.
    let both = before > 0 && after > 0;
    if !across || (both && !(across_a > 0 && across_b > 0)) {
        put(0, y1, long, y2, 0);
    } else {
        if before > 0 {
            put(0, y1, lx0, y2, 0);
        }
        if after > 0 {
            put(lx1, y1, long, y2, 0);
        }
    }
    match (across_a > 0, across_b > 0) {
        (true, true) => put(lx0, 0, lx1, short, 255),
        (true, false) => put(lx0, 0, lx1, if both { y1 } else { y3 }, 255),
        (false, true) => put(lx0, if both { y2 } else { y0 }, lx1, short, 255),
        (false, false) => {}
    }
}

/// A light arc, centered on the light lines, so it meets them at the cell's edge.
/// The arc is a quarter circle, with straight runs where the cell is longer than
/// its radius.
fn arc(c: &mut Canvas, cp: u32, t: i32) {
    let half = t as f32 / 2.0;
    let cx = band(c.w, t).0 as f32 + half;
    let cy = band(c.h, t).0 as f32 + half;
    let (sx, sy) = match cp {
        0x256D => (1.0, 1.0),
        0x256E => (-1.0, 1.0),
        0x256F => (-1.0, -1.0),
        _ => (1.0, -1.0),
    };
    let r = cx.min(c.w as f32 - cx).min(cy).min(c.h as f32 - cy);
    c.shape(|x, y| {
        let (dx, dy) = ((x - cx) * sx, (y - cy) * sy);
        let dist = if dx < r && dy < r {
            ((dx - r).hypot(dy - r) - r).abs()
        } else if dy >= r {
            dx.abs()
        } else {
            dy.abs()
        };
        dist <= half
    });
}

fn diagonals(c: &mut Canvas, cp: u32, t: i32) {
    let (w, h) = (c.w as f32, c.h as f32);
    let near = 0.75 * t as f32;
    let rising = |x: f32, y: f32| (x / w - (1.0 - y / h)).abs() * w.min(h) < near;
    let falling = |x: f32, y: f32| (x / w - y / h).abs() * w.min(h) < near;
    c.shape(|x, y| (cp != 0x2572 && rising(x, y)) || (cp != 0x2571 && falling(x, y)));
}

/// How many dashes a dashed glyph has in a cell; 0 for the rest.
fn dashes(cp: u32) -> i32 {
    match cp {
        0x2504..=0x2507 => 3,
        0x2508..=0x250B => 4,
        0x254C..=0x254F => 2,
        _ => 0,
    }
}

/// A dashed line: `n` dashes, each with a gap on both sides, so the gaps stay even
/// across a cell's edge.
fn dashed(c: &mut Canvas, [up, right, _, _]: [u8; 4], n: i32, t: i32) {
    let horizontal = right > 0;
    let weight = i32::from(right.max(up));
    let long = if horizontal { c.w } else { c.h };
    let (a, b) = band(if horizontal { c.h } else { c.w }, weight * t);
    for i in 0..n {
        let at = |k: f32| ((i as f32 + k) * long as f32 / n as f32).round() as i32;
        if horizontal {
            c.rect(at(0.2), a, at(0.8), b);
        } else {
            c.rect(a, at(0.2), b, at(0.8));
        }
    }
}

fn box_glyph(c: &mut Canvas, cp: u32, t: i32) {
    let code = BOX[(cp - 0x2500) as usize];
    let arms = [12, 8, 4, 0].map(|s| ((code >> s) & 0xF) as u8);
    let [up, right, down, left] = arms;
    let (horizontal, vertical) = (right.max(left), up.max(down));
    match cp {
        0x256D..=0x2570 => arc(c, cp, t),
        0x2571..=0x2573 => diagonals(c, cp, t),
        _ if dashes(cp) > 0 => dashed(c, arms, dashes(cp), t),
        _ if horizontal == DOUBLE && vertical == DOUBLE => double_cross(c, arms, t),
        _ if horizontal == DOUBLE => double_line(c, arms, t, false),
        _ if vertical == DOUBLE => double_line(c, arms, t, true),
        _ => solid_arms(c, arms, t),
    }
}

/// Blocks split a cell at one place, so `▀` and `▄`, `▌` and `▐`, and the quadrants
/// each fill the cell exactly, with no gap or overlap, even at an odd size.
fn block(c: &mut Canvas, cp: u32) {
    let (w, h) = (c.w, c.h);
    let eighth = |n: i32, of: i32| (of * n + 4) / 8;
    let (mx, my) = (eighth(4, w), eighth(4, h));
    match cp {
        0x2580 => c.rect(0, 0, w, h - my),
        0x2581..=0x2588 => c.rect(0, h - eighth((cp - 0x2580) as i32, h), w, h),
        0x2589..=0x258F => c.rect(0, 0, eighth((0x2590 - cp) as i32, w), h),
        0x2590 => c.rect(mx, 0, w, h),
        0x2591..=0x2593 => c.fill(0, 0, w, h, [64, 128, 192][(cp - 0x2591) as usize]),
        0x2594 => c.rect(0, 0, w, eighth(1, h)),
        0x2595 => c.rect(w - eighth(1, w), 0, w, h),
        _ => {
            // U+2596..=U+259F, as bits: top left, top right, bottom left, bottom right.
            let q = [4, 8, 1, 13, 9, 7, 11, 2, 6, 14][(cp - 0x2596) as usize];
            for (bit, x0, y0, x1, y1) in [
                (1, 0, 0, mx, h - my),
                (2, mx, 0, w, h - my),
                (4, 0, h - my, mx, h),
                (8, mx, h - my, w, h),
            ] {
                if q & bit != 0 {
                    c.rect(x0, y0, x1, y1);
                }
            }
        }
    }
}

/// Eight even dots, none crossing the cell's edge. Dots 1-3 and 7 are the left
/// column, 4-6 and 8 the right, as Unicode numbers them.
fn braille(c: &mut Canvas, cp: u32) {
    const DOTS: [(f32, f32); 8] = [
        (0.0, 0.0),
        (0.0, 1.0),
        (0.0, 2.0),
        (1.0, 0.0),
        (1.0, 1.0),
        (1.0, 2.0),
        (0.0, 3.0),
        (1.0, 3.0),
    ];
    let bits = cp - 0x2800;
    let (sw, sh) = (c.w as f32 / 2.0, c.h as f32 / 4.0);
    let r = (sw.min(sh) * 0.28).max(1.0);
    c.shape(|x, y| {
        DOTS.iter().enumerate().any(|(i, &(dx, dy))| {
            let (ox, oy) = ((dx + 0.5) * sw, (dy + 0.5) * sh);
            bits & (1 << i) != 0 && (x - ox).hypot(y - oy) <= r
        })
    });
}

fn powerline(c: &mut Canvas, cp: u32, t: i32) {
    let (w, h, t) = (c.w as f32, c.h as f32, t as f32);
    let tip = move |y: f32| w * (1.0 - (2.0 * y / h - 1.0).abs());
    let round = move |x: f32, y: f32| (x / w).powi(2) + ((y - h / 2.0) / (h / 2.0)).powi(2) <= 1.0;
    match cp {
        0xE0B0 => c.shape(|x, y| x <= tip(y)),
        0xE0B1 => c.shape(|x, y| (x - tip(y)).abs() < t),
        0xE0B2 => c.shape(|x, y| w - x <= tip(y)),
        0xE0B3 => c.shape(|x, y| (w - x - tip(y)).abs() < t),
        0xE0B4 => c.shape(round),
        _ => c.shape(|x, y| round(w - x, y)),
    }
}

/// `ch` drawn in a `w` × `h` pixel cell, with light lines `thin` pixels thick, as
/// `w * h` coverage bytes, top row first. `None` if the renderer doesn't draw it.
pub fn draw(ch: char, w: u32, h: u32, thin: u32) -> Option<Vec<u8>> {
    let cp = ch as u32;
    let t = thin.max(1) as i32;
    let mut c = Canvas::new(w, h);
    match cp {
        0x2500..=0x257F => box_glyph(&mut c, cp, t),
        0x2580..=0x259F => block(&mut c, cp),
        0x2800..=0x28FF => braille(&mut c, cp),
        0xE0B0..=0xE0B4 | 0xE0B6 => powerline(&mut c, cp, t),
        _ => return None,
    }
    Some(c.px)
}

/// A line drawn over a cell's glyph.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Line {
    /// An underline. `Underline::None` draws nothing.
    Under(Underline),
    /// The strike-through.
    Strike,
}

/// `line` in a `w` × `h` cell, `thick` pixels thick, with its top at `y`, as
/// `w * h` coverage bytes. Each style repeats at the cell's width, so a run of
/// cells draws one unbroken line.
pub fn line(line: Line, w: u32, h: u32, y: i32, thick: u32) -> Vec<u8> {
    let mut c = Canvas::new(w, h);
    let (wi, hi, t) = (w as i32, h as i32, thick.max(1) as i32);
    let y = y.min(hi - t).max(0);
    match line {
        Line::Strike | Line::Under(Underline::Single) => c.rect(0, y, wi, y + t),
        Line::Under(Underline::Double) => {
            // Two lines with a gap, kept inside the cell.
            let top = y.min(hi - 3 * t).max(0);
            c.rect(0, top, wi, top + t);
            c.rect(0, top + 2 * t, wi, top + 3 * t);
        }
        Line::Under(Underline::Curly) => {
            let (wf, half) = (w as f32, t as f32 * 0.6);
            let amp = (t as f32 * 1.5).max(2.0);
            let mid = (y as f32 + t as f32 / 2.0)
                .min(h as f32 - amp - half)
                .max(amp + half);
            c.shape(|x, py| (py - (mid + amp * (x / wf * TAU).sin())).abs() < half);
        }
        Line::Under(Underline::Dotted) => {
            let dot = t.max(2);
            let n = (wi / (2 * dot)).max(1);
            for i in 0..n {
                let x = i * wi / n;
                c.rect(x, y, x + dot, y + t);
            }
        }
        Line::Under(Underline::Dashed) => {
            c.rect(0, y, wi * 3 / 8, y + t);
            c.rect(wi * 5 / 8, y, wi, y + t);
        }
        Line::Under(Underline::None) => {}
    }
    c.px
}
