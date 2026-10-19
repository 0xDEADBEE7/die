//! Renderer-drawn glyphs that must join across cells: box drawing, blocks,
//! braille, powerline, plus the five underline styles and strikethrough.
//! Each returns a cell-sized 8-bit coverage bitmap.

/// Private-use code points the renderer uses for decorations.
pub const UL_BASE: u32 = 0xF8F0; // + Underline as u32 (1..=5)
pub const STRIKE: char = '\u{F8FA}';

pub fn is_sprite(ch: char) -> bool {
    matches!(ch as u32, 0x2500..=0x259F | 0x2800..=0x28FF | 0xE0B0..=0xE0B4 | 0xE0B6)
}

struct Canvas { w: i32, h: i32, px: Vec<u8> }

impl Canvas {
    fn new(w: u32, h: u32) -> Canvas { Canvas { w: w as i32, h: h as i32, px: vec![0; (w * h) as usize] } }
    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, v: u8) {
        for y in y0.max(0)..y1.min(self.h) { for x in x0.max(0)..x1.min(self.w) { let p = &mut self.px[(y * self.w + x) as usize]; *p = (*p).max(v); } }
    }
    /// 4x4 supersampled fill of an implicit shape given in pixel coordinates.
    fn shape(&mut self, f: impl Fn(f32, f32) -> bool) {
        for y in 0..self.h { for x in 0..self.w {
            let mut n = 0;
            for sy in 0..4 { for sx in 0..4 { if f(x as f32 + (sx as f32 + 0.5) / 4.0, y as f32 + (sy as f32 + 0.5) / 4.0) { n += 1; } } }
            if n > 0 { let p = &mut self.px[(y * self.w + x) as usize]; *p = (*p).max((n * 255 / 16) as u8); }
        } }
    }
}

// Arms per box-drawing char U+2500..=U+257F, as 4 nibbles (up, right, down, left):
// 0 none, 1 light, 2 heavy, 3 double. Special: 0xD... dashed, 0xA... arc, 0xE... diagonal.
const BOX: [u16; 128] = [
    0x0101, 0x0202, 0x1010, 0x2020, 0xD311, 0xD321, 0xD310, 0xD320, 0xD411, 0xD421, 0xD410, 0xD420, 0x0110, 0x0210, 0x0120, 0x0220,
    0x0011, 0x0012, 0x0021, 0x0022, 0x1100, 0x1200, 0x2100, 0x2200, 0x1001, 0x1002, 0x2001, 0x2002, 0x1110, 0x1210, 0x2110, 0x1120,
    0x2120, 0x2210, 0x1220, 0x2220, 0x1011, 0x1012, 0x2011, 0x1021, 0x2021, 0x2012, 0x1022, 0x2022, 0x0111, 0x0112, 0x0211, 0x0212,
    0x0121, 0x0122, 0x0221, 0x0222, 0x1101, 0x1102, 0x1201, 0x1202, 0x2101, 0x2102, 0x2201, 0x2202, 0x1111, 0x1112, 0x1211, 0x1212,
    0x2111, 0x1121, 0x2121, 0x2112, 0x2211, 0x1122, 0x1221, 0x2212, 0x1222, 0x2122, 0x2221, 0x2222, 0xD211, 0xD221, 0xD210, 0xD220,
    0x0303, 0x3030, 0x0310, 0x0130, 0x0330, 0x0013, 0x0031, 0x0033, 0x1300, 0x3100, 0x3300, 0x1003, 0x3001, 0x3003, 0x1310, 0x3130,
    0x3330, 0x1013, 0x3031, 0x3033, 0x0313, 0x0131, 0x0333, 0x1303, 0x3101, 0x3303, 0x1313, 0x3131, 0x3333, 0xA110, 0xA011, 0xA101,
    0xA100, 0xE001, 0xE002, 0xE003, 0x0001, 0x1000, 0x0100, 0x0010, 0x0002, 0x2000, 0x0200, 0x0020, 0x0201, 0x1020, 0x0102, 0x2010,
];

/// `thin` is the light line thickness in pixels.
pub fn draw(ch: char, w: u32, h: u32, thin: u32) -> Option<Vec<u8>> {
    let mut c = Canvas::new(w, h);
    let (wi, hi, t) = (w as i32, h as i32, thin.max(1) as i32);
    let cp = ch as u32;
    match cp {
        0x2500..=0x257F => {
            let code = BOX[(cp - 0x2500) as usize];
            let kind = code >> 12;
            if kind == 0xE {
                let (wf, hf, tf) = (w as f32, h as f32, t as f32 * 0.75);
                let d1 = move |x: f32, y: f32| ((x / wf) - (1.0 - y / hf)).abs() * wf.min(hf) < tf; // ╱
                let d2 = move |x: f32, y: f32| ((x / wf) - (y / hf)).abs() * wf.min(hf) < tf;       // ╲
                let k = code & 0xF;
                c.shape(|x, y| (k & 1 != 0 && d1(x, y)) || (k & 2 != 0 && d2(x, y)));
                return Some(c.px);
            }
            // dashed: 0xD<n><weight><horizontal>; arcs are drawn as light corners
            let dashes = if kind == 0xD { ((code >> 8) & 0xF) as i32 } else { 0 };
            let (up, right, down, left) = match kind {
                0xD => { let wgt = ((code >> 4) & 0xF) as u8; if code & 0xF == 1 { (0, wgt, 0, wgt) } else { (wgt, 0, wgt, 0) } }
                0xA => match cp { 0x256D => (0, 1, 1, 0), 0x256E => (0, 0, 1, 1), 0x256F => (1, 0, 0, 1), _ => (1, 1, 0, 0) },
                _ => ((code >> 12 & 0xF) as u8, (code >> 8 & 0xF) as u8, (code >> 4 & 0xF) as u8, (code & 0xF) as u8),
            };
            let th = |wgt: u8| if wgt == 2 { 2 * t } else { t };
            let (cx, cy) = (wi / 2, hi / 2);
            let vmax = [up, down].iter().map(|&a| if a == 3 { 3 * t } else if a > 0 { th(a) } else { 0 }).max().unwrap();
            let hmax = [left, right].iter().map(|&a| if a == 3 { 3 * t } else if a > 0 { th(a) } else { 0 }).max().unwrap();
            let hline = |c: &mut Canvas, x0: i32, x1: i32, wgt: u8| {
                if wgt == 3 { c.rect(x0, cy - 3 * t / 2, x1, cy - 3 * t / 2 + t, 255); c.rect(x0, cy + t / 2, x1, cy + t / 2 + t, 255); }
                else if wgt > 0 { let k = th(wgt); c.rect(x0, cy - k / 2, x1, cy - k / 2 + k, 255); }
            };
            let vline = |c: &mut Canvas, y0: i32, y1: i32, wgt: u8| {
                if wgt == 3 { c.rect(cx - 3 * t / 2, y0, cx - 3 * t / 2 + t, y1, 255); c.rect(cx + t / 2, y0, cx + t / 2 + t, y1, 255); }
                else if wgt > 0 { let k = th(wgt); c.rect(cx - k / 2, y0, cx - k / 2 + k, y1, 255); }
            };
            if dashes > 0 {
                let n = dashes;
                for i in 0..n {
                    if left > 0 { let seg = wi as f32 / n as f32; hline(&mut c, (i as f32 * seg + seg * 0.2) as i32, (i as f32 * seg + seg * 0.8) as i32, left); }
                    if up > 0 { let seg = hi as f32 / n as f32; vline(&mut c, (i as f32 * seg + seg * 0.2) as i32, (i as f32 * seg + seg * 0.8) as i32, up); }
                }
                return Some(c.px);
            }
            hline(&mut c, 0, cx + (vmax + 1) / 2, left);
            hline(&mut c, cx - vmax / 2, wi, right);
            vline(&mut c, 0, cy + (hmax + 1) / 2, up);
            vline(&mut c, cy - hmax / 2, hi, down);
        }
        0x2580..=0x259F => {
            let e = |n: i32, of: i32| (of * n + 4) / 8;
            match cp {
                0x2580 => c.rect(0, 0, wi, hi / 2, 255),
                0x2581..=0x2588 => { let n = (cp - 0x2580) as i32; c.rect(0, hi - e(n, hi), wi, hi, 255) }
                0x2589..=0x258F => { let n = (0x2590 - cp) as i32; c.rect(0, 0, e(n, wi), hi, 255) }
                0x2590 => c.rect(wi / 2, 0, wi, hi, 255),
                0x2591..=0x2593 => c.rect(0, 0, wi, hi, [64, 128, 192][(cp - 0x2591) as usize]),
                0x2594 => c.rect(0, 0, wi, e(1, hi), 255),
                0x2595 => c.rect(wi - e(1, wi), 0, wi, hi, 255),
                _ => {
                    let q = [4u8, 8, 1, 13, 9, 7, 11, 2, 6, 14][(cp - 0x2596) as usize];
                    if q & 1 != 0 { c.rect(0, 0, wi / 2, hi / 2, 255); }
                    if q & 2 != 0 { c.rect(wi / 2, 0, wi, hi / 2, 255); }
                    if q & 4 != 0 { c.rect(0, hi / 2, wi / 2, hi, 255); }
                    if q & 8 != 0 { c.rect(wi / 2, hi / 2, wi, hi, 255); }
                }
            }
        }
        0x2800..=0x28FF => {
            let bits = cp - 0x2800;
            let (sw, sh) = (w as f32 / 2.0, h as f32 / 4.0);
            let r = (sw.min(sh) * 0.28).max(1.0);
            let dots: [(u32, f32, f32); 8] = [(0, 0., 0.), (1, 0., 1.), (2, 0., 2.), (3, 1., 0.), (4, 1., 1.), (5, 1., 2.), (6, 0., 3.), (7, 1., 3.)];
            c.shape(|x, y| dots.iter().any(|&(b, dx, dy)| bits & (1 << b) != 0 && { let (ox, oy) = ((dx + 0.5) * sw, (dy + 0.5) * sh); (x - ox).powi(2) + (y - oy).powi(2) <= r * r }));
        }
        0xE0B0..=0xE0B6 => {
            let (wf, hf) = (w as f32, h as f32);
            let tf = t as f32;
            match cp {
                0xE0B0 => c.shape(|x, y| x <= wf * (1.0 - (2.0 * y / hf - 1.0).abs())),
                0xE0B2 => c.shape(|x, y| wf - x <= wf * (1.0 - (2.0 * y / hf - 1.0).abs())),
                0xE0B1 => c.shape(|x, y| (x - wf * (1.0 - (2.0 * y / hf - 1.0).abs())).abs() < tf),
                0xE0B3 => c.shape(|x, y| ((wf - x) - wf * (1.0 - (2.0 * y / hf - 1.0).abs())).abs() < tf),
                0xE0B4 => c.shape(|x, y| (x / wf).powi(2) + ((y - hf / 2.0) / (hf / 2.0)).powi(2) <= 1.0),
                0xE0B6 => c.shape(|x, y| ((wf - x) / wf).powi(2) + ((y - hf / 2.0) / (hf / 2.0)).powi(2) <= 1.0),
                _ => return None,
            }
        }
        _ => return None,
    }
    Some(c.px)
}

/// Underline (style 1..=5) or strikethrough sprite for one cell.
/// `pos` is the line's top y in pixels from the cell top.
pub fn decoration(ch: char, w: u32, h: u32, pos: i32, thick: u32) -> Option<Vec<u8>> {
    let mut c = Canvas::new(w, h);
    let (wi, t) = (w as i32, thick.max(1) as i32);
    if ch == STRIKE { let y = h as i32 * 11 / 20; c.rect(0, y, wi, y + t, 255); return Some(c.px); }
    match ch as u32 - UL_BASE {
        1 => c.rect(0, pos, wi, pos + t, 255),
        2 => { c.rect(0, pos - t, wi, pos, 255); c.rect(0, pos + t, wi, pos + 2 * t, 255); }
        3 => { let (wf, tf, pf) = (w as f32, t as f32, pos as f32 + t as f32 / 2.0); let amp = (tf * 1.5).max(2.0); c.shape(|x, y| (y - (pf + amp * (x / wf * std::f32::consts::TAU).sin())).abs() < tf * 0.6); }
        4 => { let d = t.max(2); let mut x = 0; while x < wi { c.rect(x, pos, x + d, pos + t, 255); x += 2 * d; } }
        5 => { c.rect(0, pos, wi * 3 / 8, pos + t, 255); c.rect(wi * 5 / 8, pos, wi, pos + t, 255); }
        _ => return None,
    }
    Some(c.px)
}
