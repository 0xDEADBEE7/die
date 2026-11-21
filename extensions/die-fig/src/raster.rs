//! A scene to pixels: tiny-skia for paths, gradients, clips and opacity, die-draw for text, into
//! premultiplied RGBA8 within a damage box; the measure over die-draw's fonts; and PNG export.
//!
//! Everything is drawn at the scene's scale, its logical px times `scale` physical. What the
//! raster decides on its own:
//!
//! - **Damage.** Only the damage box is cleared and drawn, and nothing outside it is written.
//!   Paths are laid out over the whole buffer and drawn through a mask, the box's or, for a
//!   whole redraw, the whole buffer's: tiny-skia cuts a path at the edges of what it draws
//!   into, and one cut at the box's edges would antialias differently there; and without a
//!   mask it lays an opaque paint's edges down by another formula, a level off. So a box
//!   redrawn is exactly what a whole redraw puts in it. What lies wholly outside the box isn't
//!   rasterised at all. A buffer whose rows have bytes past them is drawn into a packed copy
//!   and the box copied across.
//! - **Hairlines** are `max(1, ⌊scale⌋)` physical px wide, as die-markdown's rules are: one px at
//!   1 and 1.5, two at 2. Each line's centre goes to the middle of a pixel for an odd width
//!   (`⌊v⌋ + ½`) and to a pixel's edge for an even one (`round(v)`). A hairline made only of
//!   horizontal and vertical lines is filled as whole-pixel rectangles, its ends rounded to
//!   whole pixels and its corners filled, so it's never blurred; one with a slant or a curve is
//!   stroked at that width. Its dashes are found by their number along each line, from where
//!   the pattern stands at the line's start, in f64, and only those that can show are made, so
//!   a line of any length at any scale costs what the picture does. A pattern shorter than a
//!   physical pixel can't be shown in whole pixels and is drawn solid.
//! - **Clips** are rectangles rounded to the nearest physical pixel; nested ones intersect.
//! - **Opacity** draws its group into a layer of its own and lays it down at the opacity once,
//!   so overlapping marks in a faded group don't darken where they cross. Only the pixels the
//!   group drew into are laid down, so a faded series costs about what its line does.
//! - **Shadows** are black, the rounded rectangle's coverage blurred by three box blurs that
//!   stand in for a Gaussian of σ = `blur / 2` (CSS's reading of a blur radius), cut off
//!   `2 × blur` past the rectangle, the sums in whole numbers so a box redrawn gets what a
//!   whole redraw does. The blur is at most [`MAX_BLUR`]; a shadow whose blurred area would
//!   pass 2²⁶ pixels isn't drawn.
//! - **Text** needs a [`die_draw::Book`]: without one, none is drawn. A tab or a line break is
//!   drawn and measured as a space, and the other control characters, U+FFFE and U+FFFF not at
//!   all, as the SVG writes them.

use crate::scene::{
    Anchor, Cap, Item, Join, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec, Verb, Weight,
};
use crate::theme::{Color, draw_rgb};
use die_draw::{Book, Clip, Pass, Pixels, TextLook};
use std::borrow::Cow;
use tiny_skia as sk;

/// The most a shadow's blur is, in logical px: a larger one is drawn at this. Past it a shadow
/// is a haze no one sees, and its cost grows with the square of the blur.
pub const MAX_BLUR: f32 = 100.0;

/// The text measure over die-draw's fonts, at the scene's scale.
pub struct BookMeasure<'a> {
    /// The fonts.
    pub book: &'a mut die_draw::Book,
    /// Physical px to a logical px.
    pub scale: f32,
}

/// A scale to draw at: a positive finite one as it is, anything else as 1.
fn usable(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// A text as it's drawn, measured and written to SVG: a tab or a line break as a space, so
/// the words on either side stay apart, and without the other control characters, U+FFFE and
/// U+FFFF, which XML can't hold and the fonts have no glyph for.
pub(crate) fn plain(text: &str) -> Cow<'_, str> {
    let odd = |c: char| c.is_control() || c == '\u{FFFE}' || c == '\u{FFFF}';
    if !text.chars().any(odd) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .filter_map(|c| match c {
                '\t' | '\n' | '\r' => Some(' '),
                c if odd(c) => None,
                c => Some(c),
            })
            .collect(),
    )
}

/// How die-draw sets a text of this spec.
pub fn look(spec: TextSpec) -> TextLook {
    let weight = match spec.weight {
        Weight::Regular => die_draw::Weight::Regular,
        Weight::Medium => die_draw::Weight::Medium,
        Weight::Semibold => die_draw::Weight::Semibold,
    };
    let mut look = TextLook::new(weight, spec.size);
    if spec.tabular {
        look = look.tabular();
    }
    if spec.mono {
        look = look.mono();
    }
    look
}

impl Measure for BookMeasure<'_> {
    fn width(&mut self, text: &str, spec: TextSpec) -> f32 {
        let s = usable(self.scale);
        self.book.pass().extent(&plain(text), look(spec), s).width / s
    }

    fn ascent(&mut self, spec: TextSpec) -> f32 {
        let s = usable(self.scale);
        self.book.pass().extent("", look(spec), s).ascent / s
    }

    fn descent(&mut self, spec: TextSpec) -> f32 {
        let s = usable(self.scale);
        self.book.pass().extent("", look(spec), s).descent / s
    }

    fn cap(&mut self, spec: TextSpec) -> f32 {
        let s = usable(self.scale);
        self.book.pass().extent("", look(spec), s).cap / s
    }

    /// The whole text if it fits; else the longest prefix that fits with '…' after it; else,
    /// when not even '…' fits, nothing. The raster cuts a text with a `max_width` the same way.
    /// The text comes back as it's drawn: a tab or a line break as a space.
    fn fit(&mut self, text: &str, spec: TextSpec, max: f32) -> String {
        let s = usable(self.scale);
        let text = plain(text);
        fit(&mut self.book.pass(), &text, look(spec), s, max * s).into_owned()
    }
}

/// The whole of `text` if it's at most `max` physical px wide; else its longest prefix that is
/// with '…' after it; else nothing.
fn fit<'t>(
    pass: &mut Pass<'_>,
    text: &'t str,
    look: TextLook,
    scale: f32,
    max: f32,
) -> Cow<'t, str> {
    if pass.extent(text, look, scale).width <= max {
        return Cow::Borrowed(text);
    }
    let starts: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    let cut = |k: usize| format!("{}…", &text[..starts[k]]);
    let fits = |k: usize, pass: &mut Pass<'_>| pass.extent(&cut(k), look, scale).width <= max;
    if starts.is_empty() || !fits(0, pass) {
        return Cow::Borrowed("");
    }
    // The longest prefix of k chars, k < n, that fits with its '…': widths grow with k.
    let (mut lo, mut hi) = (0, starts.len() - 1);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(mid, pass) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    Cow::Owned(cut(lo))
}

/// A box of physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelBox {
    /// The left column.
    pub x: u32,
    /// The top row.
    pub y: u32,
    /// The width.
    pub w: u32,
    /// The height.
    pub h: u32,
}

impl PixelBox {
    /// Whether it holds no pixel.
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// What both hold; empty, at the far corner of the nearer, if nothing.
    pub fn intersect(&self, o: &PixelBox) -> PixelBox {
        let (x, y) = (self.x.max(o.x), self.y.max(o.y));
        let r = self.x.saturating_add(self.w).min(o.x.saturating_add(o.w));
        let b = self.y.saturating_add(self.h).min(o.y.saturating_add(o.h));
        PixelBox {
            x,
            y,
            w: r.saturating_sub(x),
            h: b.saturating_sub(y),
        }
    }

    /// The smallest box holding both; an empty one adds nothing.
    pub fn union(&self, o: &PixelBox) -> PixelBox {
        match (self.is_empty(), o.is_empty()) {
            (true, _) => *o,
            (_, true) => *self,
            _ => {
                let (x, y) = (self.x.min(o.x), self.y.min(o.y));
                let r = self.x.saturating_add(self.w).max(o.x.saturating_add(o.w));
                let b = self.y.saturating_add(self.h).max(o.y.saturating_add(o.h));
                PixelBox {
                    x,
                    y,
                    w: r - x,
                    h: b - y,
                }
            }
        }
    }
}

impl From<PixelBox> for die_core::PixelRect {
    fn from(b: PixelBox) -> die_core::PixelRect {
        die_core::PixelRect::new(b.x, b.y, b.w, b.h)
    }
}

/// Draws `scene` into premultiplied RGBA8 `pixels` (`stride` bytes a row, physical size
/// `width × height`), only inside `damage` if given (physical px), clearing `damage` (or the
/// whole buffer) first to `clear` (None = transparent). tiny-skia for paths, die-draw for text.
/// Nothing outside the damage is written, and nothing at all if `pixels` can't hold
/// `width × height` at `stride`.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    scene: &Scene,
    pixels: &mut [u8],
    width: u32,
    height: u32,
    stride: usize,
    damage: Option<PixelBox>,
    clear: Option<Color>,
    book: Option<&mut die_draw::Book>,
) {
    let row = width as usize * 4;
    if width == 0 || height == 0 || stride < row {
        return;
    }
    if pixels.len() < stride * (height as usize - 1) + row {
        return;
    }
    let whole = PixelBox {
        x: 0,
        y: 0,
        w: width,
        h: height,
    };
    let area = damage.map_or(whole, |d| d.intersect(&whole));
    if area.is_empty() {
        return;
    }
    let mut pass = book.map(Book::pass);
    if stride == row {
        let bytes = &mut pixels[..row * height as usize];
        paint(scene, bytes, (width, height), area, clear, pass.as_mut());
    } else {
        // A row with bytes past it: drawn into a packed picture of the whole, and the damage
        // copied across.
        let mut packed = vec![0u8; row * height as usize];
        paint(
            scene,
            &mut packed,
            (width, height),
            area,
            clear,
            pass.as_mut(),
        );
        let n = area.w as usize * 4;
        for y in area.y as usize..(area.y + area.h) as usize {
            let from = y * row + area.x as usize * 4;
            let at = y * stride + area.x as usize * 4;
            pixels[at..at + n].copy_from_slice(&packed[from..from + n]);
        }
    }
}

/// A logical rectangle's physical pixels at `scale`: rounded out, clipped to `width × height`.
/// A rectangle with no area, or not finite, has none.
pub fn to_physical(r: Rect, scale: f32, width: u32, height: u32) -> PixelBox {
    let s = usable(scale);
    let finite = [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite());
    if !finite || r.is_empty() {
        return PixelBox {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        };
    }
    let side = |v: f32, most: u32| v.clamp(0.0, most as f32) as u32;
    let (x0, y0) = (
        side((r.x * s).floor(), width),
        side((r.y * s).floor(), height),
    );
    let (x1, y1) = (
        side((r.right() * s).ceil(), width),
        side((r.bottom() * s).ceil(), height),
    );
    PixelBox {
        x: x0,
        y: y0,
        w: x1.saturating_sub(x0),
        h: y1.saturating_sub(y0),
    }
}

/// A PNG of the scene, opaque on `background`, at the scene's scale.
pub fn png(
    scene: &Scene,
    background: Color,
    book: Option<&mut die_draw::Book>,
) -> Result<Vec<u8>, String> {
    let s = usable(scene.scale);
    let side = |v: f32| -> Result<u32, String> {
        let n = (v * s).ceil();
        if n.is_nan() || n < 1.0 {
            return Err(format!("a figure {v} px across has no pixels"));
        }
        if n > die_core::PIXEL_MAX_SIDE as f32 {
            return Err(format!(
                "{n} px is more than {} a side",
                die_core::PIXEL_MAX_SIDE
            ));
        }
        Ok(n as u32)
    };
    let (w, h) = (side(scene.width)?, side(scene.height)?);
    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    draw(
        scene,
        &mut rgba,
        w,
        h,
        w as usize * 4,
        None,
        Some(background),
        book,
    );
    // Opaque everywhere, so premultiplied is straight; demultiply leaves it as it is.
    die_draw::image::demultiply(&mut rgba);
    die_draw::image::encode(w, h, &rgba)
}

// ------------------------------------------------------------------ the painter

/// An opacity pushed and not yet popped.
enum Group {
    /// Opaque: drawn straight through.
    Through,
    /// Transparent: nothing in it is drawn.
    Hidden,
    /// Drawn into its own layer, laid down at its opacity when popped.
    Layer {
        /// The layer's pixels, the picture's size, transparent where nothing was drawn.
        px: Vec<u8>,
        alpha: f32,
        /// A box holding every pixel drawn into the layer: only it is laid down and cleared.
        ink: Option<Clip>,
    },
}

/// One scene's drawing into the damage of a picture.
struct Painter<'p, 'f, 'b> {
    /// The picture's pixels, packed: `w × 4` bytes a row.
    bytes: &'p mut [u8],
    w: u32,
    h: u32,
    /// The damage: nothing is drawn outside it.
    area: Clip,
    /// Physical px a logical px.
    scale: f32,
    /// Logical to physical px.
    ts: sk::Transform,
    /// The clip after each push: None where it's empty.
    clips: Vec<Option<Clip>>,
    groups: Vec<Group>,
    /// How many of `groups` are hidden.
    hidden: usize,
    /// Layers laid down and cleared, for the next group.
    spare: Vec<Vec<u8>>,
    /// The mask of the last clip drawn through: 255 inside it, 0 out.
    mask: Option<(Clip, sk::Mask)>,
    pass: Option<&'f mut Pass<'b>>,
}

/// The pixels being drawn into: the innermost layer's, else the box's.
fn surface<'x>(bytes: &'x mut [u8], groups: &'x mut [Group]) -> &'x mut [u8] {
    let layer = groups.iter_mut().rev().find_map(|g| match g {
        Group::Layer { px, .. } => Some(px),
        _ => None,
    });
    match layer {
        Some(px) => px,
        None => bytes,
    }
}

/// The smallest box holding both.
fn join(a: Clip, b: Clip) -> Clip {
    Clip {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    }
}

/// `c` grown by `d` px on every side.
fn grow(c: Clip, d: i32) -> Clip {
    Clip {
        x0: c.x0.saturating_sub(d),
        y0: c.y0.saturating_sub(d),
        x1: c.x1.saturating_add(d),
        y1: c.y1.saturating_add(d),
    }
}

/// Lays a run of a layer's pixels down on `under`'s at `k` of 255, and clears them: a group's
/// pixels at its opacity over what's under them. Four pixels with nothing are passed over at
/// once, since a faded line leaves most of its box empty.
fn lay(layer: &mut [u8], under: &mut [u8], k: u32) {
    let part = |v: u8| (u32::from(v) * k + 127) / 255;
    let one = |src: &mut [u8], dst: &mut [u8]| {
        if src[3] == 0 {
            return;
        }
        let rest = 255 - part(src[3]);
        for (d, &s) in dst.iter_mut().zip(&*src) {
            *d = (part(s) + (u32::from(*d) * rest + 127) / 255).min(255) as u8;
        }
        src.fill(0);
    };
    let mut src = layer.chunks_exact_mut(16);
    let mut dst = under.chunks_exact_mut(16);
    for (s, d) in (&mut src).zip(&mut dst) {
        if u128::from_ne_bytes((&*s).try_into().unwrap_or([0; 16])) == 0 {
            continue;
        }
        for (s, d) in s.chunks_exact_mut(4).zip(d.chunks_exact_mut(4)) {
            one(s, d);
        }
    }
    let (s, d) = (src.into_remainder(), dst.into_remainder());
    for (s, d) in s.chunks_exact_mut(4).zip(d.chunks_exact_mut(4)) {
        one(s, d);
    }
}

/// Moves a mask that's 255 over `old` and 0 elsewhere to 255 over `new`, touching only the
/// pixels that change: most clips a chart moves between hold one another.
fn shift(m: &mut sk::Mask, old: Clip, new: Clip) {
    let (w, h) = (m.width() as usize, m.height() as usize);
    let within = |c: Clip| {
        let c = c.meet(Clip {
            x0: 0,
            y0: 0,
            x1: w as i32,
            y1: h as i32,
        });
        (!c.is_empty()).then_some(c)
    };
    let (old, new) = (within(old), within(new));
    // Each box's columns on row `y`, if it's on the row.
    let span = |c: Option<Clip>, y: i32| {
        c.filter(|c| y >= c.y0 && y < c.y1)
            .map(|c| (c.x0 as usize, c.x1 as usize))
    };
    let rows = |c: Option<Clip>| c.map_or((h as i32, 0), |c| (c.y0, c.y1));
    let ((o0, o1), (n0, n1)) = (rows(old), rows(new));
    let data = m.data_mut();
    for y in o0.min(n0)..o1.max(n1) {
        let row = &mut data[y as usize * w..(y as usize + 1) * w];
        let (o, n) = (span(old, y), span(new, y));
        // Where one is and the other isn't.
        let set = |row: &mut [u8], a: Option<(usize, usize)>, b: Option<(usize, usize)>, v| {
            let Some((a0, a1)) = a else { return };
            match b {
                None => row[a0..a1].fill(v),
                Some((b0, b1)) => {
                    row[a0..a1.min(b0).max(a0)].fill(v);
                    row[b1.max(a0).min(a1)..a1].fill(v);
                }
            }
        };
        set(row, o, n, 0);
        set(row, n, o, 255);
    }
}

/// Draws `scene` into `area` of `bytes`, a packed picture `w × h`, cleared first. Everything
/// is laid out over the whole picture and masked to the area, so what the area gets is what a
/// whole redraw would put there: tiny-skia cuts a path at the edges of what it draws into,
/// and a path cut at the area's edges would antialias differently there. A whole redraw is
/// masked too, to the whole picture, since tiny-skia unmasked blends an opaque paint's edges
/// by another formula.
fn paint(
    scene: &Scene,
    bytes: &mut [u8],
    (w, h): (u32, u32),
    area: PixelBox,
    clear: Option<Color>,
    pass: Option<&mut Pass<'_>>,
) {
    let ground = clear.map_or([0; 4], |c| [c.0, c.1, c.2, 255]);
    let n = area.w as usize * 4;
    for y in area.y as usize..(area.y + area.h) as usize {
        let at = (y * w as usize + area.x as usize) * 4;
        for px in bytes[at..at + n].chunks_exact_mut(4) {
            px.copy_from_slice(&ground);
        }
    }
    let scale = usable(scene.scale);
    let mut p = Painter {
        bytes,
        w,
        h,
        area: Clip {
            x0: area.x as i32,
            y0: area.y as i32,
            x1: (area.x + area.w) as i32,
            y1: (area.y + area.h) as i32,
        },
        scale,
        ts: sk::Transform::from_scale(scale, scale),
        clips: Vec::new(),
        groups: Vec::new(),
        hidden: 0,
        spare: Vec::new(),
        mask: None,
        pass,
    };
    for item in &scene.items {
        p.item(item);
    }
    while !p.groups.is_empty() {
        p.pop_group();
    }
}

impl Painter<'_, '_, '_> {
    /// Notes that the pixels of `c` may be drawn into, for the innermost layer.
    fn inked(&mut self, c: Clip) {
        if c.is_empty() {
            return;
        }
        let layer = self.groups.iter_mut().rev().find_map(|g| match g {
            Group::Layer { ink, .. } => Some(ink),
            _ => None,
        });
        if let Some(ink) = layer {
            *ink = Some(ink.map_or(c, |i| join(i, c)));
        }
    }

    /// Where drawing lands now: the damage within the clip, or None when that's empty or a
    /// group is hidden.
    fn visible(&self) -> Option<Clip> {
        if self.hidden > 0 {
            return None;
        }
        match self.clips.last() {
            Some(c) => *c,
            None => Some(self.area),
        }
    }

    /// A logical rectangle in physical px, each edge at its nearest pixel.
    fn nearest(&self, r: Rect) -> Clip {
        let s = self.scale;
        let at = |v: f32| {
            let v = (v * s).round();
            if v.is_nan() { 0 } else { v as i32 }
        };
        Clip {
            x0: at(r.x),
            y0: at(r.y),
            x1: at(r.right()),
            y1: at(r.bottom()),
        }
    }

    /// A logical rectangle in physical px, rounded out.
    fn outward(&self, r: Rect) -> Clip {
        let s = self.scale;
        let at = |v: f32, up: bool| {
            let v = if up { (v * s).ceil() } else { (v * s).floor() };
            if v.is_nan() { 0 } else { v as i32 }
        };
        Clip {
            x0: at(r.x, false),
            y0: at(r.y, false),
            x1: at(r.right(), true),
            y1: at(r.bottom(), true),
        }
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::PushClip(r) => {
                let inner = self.clips.last().copied().unwrap_or(Some(self.area));
                let inner = inner
                    .map(|c| c.meet(self.nearest(*r)))
                    .filter(|c| !c.is_empty());
                self.clips.push(inner);
            }
            Item::PopClip => {
                self.clips.pop();
            }
            Item::PushOpacity(a) => self.push_group(*a),
            Item::PopOpacity => {
                if !self.groups.is_empty() {
                    self.pop_group();
                }
            }
            Item::Path { path, fill, stroke } => {
                let Some(clip) = self.visible() else { return };
                let Some(b) = path.bounds() else { return };
                let reach = stroke.as_ref().map_or(0.0, |s| reach(s, self.scale));
                // All it can touch: its points, past them as far as its stroke reaches, and
                // two px more for antialiasing and a hairline's snap to whole pixels.
                let touched = grow(self.outward(b.inflate(reach + 1.0)), 2).meet(clip);
                if touched.is_empty() {
                    return;
                }
                self.inked(touched);
                if let Some(f) = fill {
                    self.fill(path, f, clip);
                }
                if let Some(s) = stroke {
                    if s.hairline {
                        self.hairline(path, s, clip);
                    } else {
                        self.stroke(path, s, clip);
                    }
                }
            }
            Item::Text {
                text,
                x,
                y,
                spec,
                anchor,
                color,
                alpha,
                max_width,
            } => {
                let Some(clip) = self.visible() else { return };
                self.text(
                    text,
                    (*x, *y),
                    *spec,
                    *anchor,
                    *color,
                    *alpha,
                    *max_width,
                    clip,
                );
            }
            Item::Shadow {
                rect,
                radius,
                blur,
                dy,
                alpha,
            } => {
                let Some(clip) = self.visible() else { return };
                self.shadow(*rect, *radius, *blur, *dy, *alpha, clip);
            }
        }
    }

    fn push_group(&mut self, alpha: f32) {
        let a = if alpha.is_nan() {
            0.0
        } else {
            alpha.clamp(0.0, 1.0)
        };
        let g = if self.hidden > 0 || a >= 1.0 {
            Group::Through
        } else if a <= 0.0 {
            self.hidden += 1;
            Group::Hidden
        } else {
            // A fresh layer's pages are the system's zeroes, touched only where it's drawn.
            let px = self
                .spare
                .pop()
                .unwrap_or_else(|| vec![0; self.bytes.len()]);
            Group::Layer {
                px,
                alpha: a,
                ink: None,
            }
        };
        self.groups.push(g);
    }

    /// Ends the innermost group: a layer is laid down at its opacity on what's under it, where
    /// it was drawn into, and cleared there for the next group.
    fn pop_group(&mut self) {
        match self.groups.pop() {
            None | Some(Group::Through) => {}
            Some(Group::Hidden) => self.hidden -= 1,
            Some(Group::Layer {
                px: mut layer,
                alpha,
                ink,
            }) => {
                let ink = ink.map(|i| i.meet(self.area)).filter(|i| !i.is_empty());
                if let Some(ink) = ink {
                    let w = self.w as usize;
                    let under = surface(self.bytes, &mut self.groups);
                    let k = (alpha * 255.0 + 0.5) as u32;
                    for y in ink.y0 as usize..ink.y1 as usize {
                        let (from, to) =
                            ((y * w + ink.x0 as usize) * 4, (y * w + ink.x1 as usize) * 4);
                        lay(&mut layer[from..to], &mut under[from..to], k);
                    }
                    self.inked(ink);
                }
                self.spare.push(layer);
            }
        }
    }

    /// Draws through tiny-skia into the current surface, masked to `clip`. Always through a
    /// mask, the whole picture's too: unmasked, tiny-skia blends an opaque paint's edges by
    /// another formula, and a box redrawn would be a level off a whole redraw there. The one
    /// mask is moved from clip to clip, where they differ.
    fn with(&mut self, clip: Clip, f: impl FnOnce(&mut sk::PixmapMut<'_>, Option<&sk::Mask>)) {
        let (w, h) = (self.w, self.h);
        match &mut self.mask {
            Some((c, _)) if *c == clip => {}
            Some((c, m)) => {
                shift(m, *c, clip);
                *c = clip;
            }
            None => {
                let Some(mut m) = sk::Mask::new(w, h) else {
                    return;
                };
                let none = Clip {
                    x0: 0,
                    y0: 0,
                    x1: 0,
                    y1: 0,
                };
                shift(&mut m, none, clip);
                self.mask = Some((clip, m));
            }
        }
        let Some((_, mask)) = &self.mask else { return };
        let px = surface(self.bytes, &mut self.groups);
        if let Some(mut pm) = sk::PixmapMut::from_bytes(px, w, h) {
            f(&mut pm, Some(mask));
        }
    }

    fn fill(&mut self, path: &PathData, paint: &Paint, clip: Clip) {
        let (Some(p), Some(shader)) = (sk_path(path), shader(paint, sk::Transform::identity()))
        else {
            return;
        };
        let paint = sk_paint(shader);
        let ts = self.ts;
        self.with(clip, |pm, mask| {
            pm.fill_path(&p, &paint, sk::FillRule::Winding, ts, mask);
        });
    }

    fn stroke(&mut self, path: &PathData, s: &Stroke, clip: Clip) {
        if !s.width.is_finite() || s.width <= 0.0 {
            return;
        }
        let (Some(p), Some(shader)) = (sk_path(path), shader(&s.paint, sk::Transform::identity()))
        else {
            return;
        };
        let paint = sk_paint(shader);
        let stroke = sk_stroke(s, s.width, 1.0);
        let ts = self.ts;
        self.with(clip, |pm, mask| {
            pm.stroke_path(&p, &paint, &stroke, ts, mask)
        });
    }

    /// A hairline: whole-pixel rectangles where it runs straight across or down, else a stroke
    /// a hairline wide.
    fn hairline(&mut self, path: &PathData, s: &Stroke, clip: Clip) {
        let Some(shader) = shader(&s.paint, self.ts) else {
            return;
        };
        let paint = sk_paint(shader);
        let w = hair(self.scale);
        let ts = self.ts;
        let sc = self.scale;
        let map = |x: f32, y: f32| (x * sc, y * sc);
        let Some(subpaths) = subpaths(path, map) else {
            // A curve: stroked as it is, a hairline wide.
            let Some(p) = sk_path(path).and_then(|p| p.transform(ts)) else {
                return;
            };
            let stroke = sk_stroke(s, w, self.scale);
            self.with(clip, |pm, mask| {
                pm.stroke_path(&p, &paint, &stroke, sk::Transform::identity(), mask);
            });
            return;
        };
        let straight = subpaths
            .iter()
            .all(|(pts, closed)| segments(pts, *closed).all(|(a, b)| level(a, b) || plumb(a, b)));
        if !straight {
            let mut pb = sk::PathBuilder::new();
            for (pts, closed) in &subpaths {
                for (i, &(x, y)) in pts.iter().enumerate() {
                    if i == 0 {
                        pb.move_to(x, y);
                    } else {
                        pb.line_to(x, y);
                    }
                }
                if *closed {
                    pb.close();
                }
            }
            let Some(p) = pb.finish() else { return };
            let stroke = sk_stroke(s, w, self.scale);
            self.with(clip, |pm, mask| {
                pm.stroke_path(&p, &paint, &stroke, sk::Transform::identity(), mask);
            });
            return;
        }
        // In physical px. With nothing off, or a pattern under a pixel that whole pixels can't
        // show, it's solid.
        let px = f64::from(self.scale);
        let dash = s
            .dash
            .map(|(on, off)| (f64::from(on) * px, f64::from(off) * px))
            .filter(|&(on, off)| {
                on > 0.0 && off > 0.0 && on + off >= 1.0 && (on + off).is_finite()
            });
        let mut pb = sk::PathBuilder::new();
        for (pts, closed) in &subpaths {
            hair_rects(pts, *closed, w, s.cap, dash, clip, &mut pb);
        }
        let Some(p) = pb.finish() else { return };
        self.with(clip, |pm, mask| {
            pm.fill_path(
                &p,
                &paint,
                sk::FillRule::Winding,
                sk::Transform::identity(),
                mask,
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        text: &str,
        at: (f32, f32),
        spec: TextSpec,
        anchor: Anchor,
        color: Color,
        alpha: f32,
        max_width: Option<f32>,
        clip: Clip,
    ) {
        let a = if alpha.is_nan() {
            0
        } else {
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
        };
        let text = plain(text);
        if a == 0 || text.is_empty() || spec.size.is_nan() || spec.size <= 0.0 {
            return;
        }
        // Skip what can't reach the clip: an em a char, 1.2 em up and half an em down.
        let em = spec.size;
        let wide = max_width.map_or(f32::INFINITY, |m| m.max(0.0));
        let wide = (text.chars().count() as f32 * em).min(wide);
        let left = match anchor {
            Anchor::Start => at.0,
            Anchor::Middle => at.0 - wide / 2.0,
            Anchor::End => at.0 - wide,
        };
        let near = Rect::new(left, at.1 - 1.2 * em, wide, 1.7 * em).inflate(1.0);
        if self.outward(near).meet(clip).is_empty() {
            return;
        }
        let s = self.scale;
        let Some(pass) = self.pass.as_deref_mut() else {
            return;
        };
        let look = look(spec);
        let line = match max_width {
            Some(m) => {
                let cut = fit(pass, &text, look, s, m * s);
                if cut.is_empty() {
                    return;
                }
                pass.set(&cut, look, s)
            }
            None => pass.set(&text, look, s),
        };
        let width = line.extent.width;
        let x = at.0 * s
            - match anchor {
                Anchor::Start => 0.0,
                Anchor::Middle => width / 2.0,
                Anchor::End => width,
            };
        let baseline = at.1 * s;
        // Its ink, and an em past it each way for what a glyph hangs over its advance.
        let em = spec.size * s;
        let ink = Clip {
            x0: (x - em).floor() as i32,
            y0: (baseline - 2.0 * em).floor() as i32,
            x1: (x + width + em).ceil() as i32,
            y1: (baseline + em).ceil() as i32,
        };
        self.inked(ink.meet(clip));
        let Some(pass) = self.pass.as_deref_mut() else {
            return;
        };
        let (w, h) = (self.w, self.h);
        let px = surface(self.bytes, &mut self.groups);
        if let Some(mut to) = Pixels::new(px, w, h) {
            pass.draw(&line, &mut to, x, baseline, draw_rgb(color), a, clip);
        }
    }

    fn shadow(&mut self, rect: Rect, radius: f32, blur: f32, dy: f32, alpha: f32, clip: Clip) {
        let s = self.scale;
        let a = if alpha.is_nan() {
            0.0
        } else {
            alpha.clamp(0.0, 1.0)
        };
        let blur = if blur.is_nan() {
            0.0
        } else {
            blur.clamp(0.0, MAX_BLUR)
        };
        if a <= 0.0 || rect.is_empty() || !dy.is_finite() {
            return;
        }
        let r = sk::Rect::from_xywh(rect.x * s, (rect.y + dy) * s, rect.w * s, rect.h * s);
        let Some(r) = r else { return };
        // Where the blur spreads, in f64 so a rectangle or a reach past any picture can't
        // overflow; one past what can be held isn't drawn.
        let reach = f64::from((2.0 * blur * s).ceil());
        let (x0, y0) = (
            (f64::from(r.left()) - reach).floor(),
            (f64::from(r.top()) - reach).floor(),
        );
        let (x1, y1) = (
            (f64::from(r.right()) + reach).ceil(),
            (f64::from(r.bottom()) + reach).ceil(),
        );
        let meets = x0 < f64::from(clip.x1)
            && y0 < f64::from(clip.y1)
            && x1 > f64::from(clip.x0)
            && y1 > f64::from(clip.y0);
        if !meets || (x1 - x0) * (y1 - y0) > f64::from(1u32 << 26) {
            return;
        }
        // Within the clip's reach of 2²⁶ px now, so each side is an i32.
        let region = Clip {
            x0: x0 as i32,
            y0: y0 as i32,
            x1: x1 as i32,
            y1: y1 as i32,
        };
        let seen = region.meet(clip);
        if seen.is_empty() {
            return;
        }
        let Some(cov) = shadow_coverage(r, radius * s, blur * s / 2.0, region, seen) else {
            return;
        };
        self.inked(seen);
        let w = self.w as usize;
        let px = surface(self.bytes, &mut self.groups);
        let sw = (seen.x1 - seen.x0) as usize;
        for (j, row) in cov.chunks_exact(sw).enumerate() {
            let y = seen.y0 as usize + j;
            for (i, &c) in row.iter().enumerate() {
                let k = ((a * c * 255.0 + 0.5) as u32).min(255);
                if k > 0 {
                    let at = (y * w + seen.x0 as usize + i) * 4;
                    die_draw::blend(&mut px[at..at + 4], die_draw::Rgb(0, 0, 0), k);
                }
            }
        }
    }
}

/// How far a stroke reaches past its path in logical px: half its width, as far as a mitre
/// goes at tiny-skia's limit of 4, or a square cap's corner.
fn reach(s: &Stroke, scale: f32) -> f32 {
    let half = if s.hairline {
        hair(scale) / scale / 2.0
    } else {
        s.width / 2.0
    };
    match (s.join, s.cap) {
        (Join::Miter, _) => half * 4.0,
        (_, Cap::Square) => half * std::f32::consts::SQRT_2,
        _ => half,
    }
}

/// A hairline's width in physical px: a logical px, down to whole pixels and never under one.
fn hair(scale: f32) -> f32 {
    scale.floor().max(1.0)
}

/// The path in tiny-skia's terms. None for one with no points or one that isn't finite.
fn sk_path(p: &PathData) -> Option<sk::Path> {
    let mut b = sk::PathBuilder::with_capacity(p.verbs.len(), p.verbs.len());
    for v in &p.verbs {
        match *v {
            Verb::Move(x, y) => b.move_to(x, y),
            Verb::Line(x, y) => b.line_to(x, y),
            Verb::Quad(a, c, x, y) => b.quad_to(a, c, x, y),
            Verb::Cubic(a, c, d, e, x, y) => b.cubic_to(a, c, d, e, x, y),
            Verb::Close => b.close(),
        }
    }
    b.finish()
}

fn sk_paint(shader: sk::Shader<'static>) -> sk::Paint<'static> {
    sk::Paint {
        shader,
        anti_alias: true,
        ..sk::Paint::default()
    }
}

/// A stroke `width` wide in the units it's drawn in, with dashes at `dash_scale` to them.
fn sk_stroke(s: &Stroke, width: f32, dash_scale: f32) -> sk::Stroke {
    let dash = s.dash.and_then(|(on, off)| {
        let (on, off) = (on * dash_scale, off * dash_scale);
        (on >= 0.0 && off >= 0.0 && on + off > 0.0)
            .then(|| sk::StrokeDash::new(vec![on, off], 0.0))
            .flatten()
    });
    sk::Stroke {
        width,
        miter_limit: 4.0,
        line_cap: match s.cap {
            Cap::Butt => sk::LineCap::Butt,
            Cap::Round => sk::LineCap::Round,
            Cap::Square => sk::LineCap::Square,
        },
        line_join: match s.join {
            Join::Miter => sk::LineJoin::Miter,
            Join::Round => sk::LineJoin::Round,
            Join::Bevel => sk::LineJoin::Bevel,
        },
        dash,
    }
}

fn sk_color(c: Color, alpha: f32) -> Option<sk::Color> {
    let a = if alpha.is_nan() {
        0.0
    } else {
        alpha.clamp(0.0, 1.0)
    };
    sk::Color::from_rgba(
        f32::from(c.0) / 255.0,
        f32::from(c.1) / 255.0,
        f32::from(c.2) / 255.0,
        a,
    )
}

/// What a paint draws with, its gradient's points through `local`. None for a paint that
/// draws nothing: transparent, or a gradient with no stops.
fn shader(paint: &Paint, local: sk::Transform) -> Option<sk::Shader<'static>> {
    match paint {
        Paint::Solid { color, alpha } => {
            let c = sk_color(*color, *alpha)?;
            (c.alpha() > 0.0).then_some(sk::Shader::SolidColor(c))
        }
        Paint::Linear { from, to, stops } => {
            let mut stops: Vec<_> = stops
                .iter()
                .filter(|(o, _, a)| o.is_finite() && !a.is_nan())
                .copied()
                .collect();
            stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            if stops.iter().all(|(_, _, a)| *a <= 0.0) {
                return None;
            }
            let stops = stops
                .iter()
                .filter_map(|&(o, c, a)| Some(sk::GradientStop::new(o, sk_color(c, a)?)))
                .collect();
            sk::LinearGradient::new(
                sk::Point::from_xy(from.0, from.1),
                sk::Point::from_xy(to.0, to.1),
                stops,
                sk::SpreadMode::Pad,
                local,
            )
        }
    }
}

// ------------------------------------------------------------------ hairlines

/// How near a line must be to level or plumb, in physical px, to be drawn as one.
const STRAIGHT: f32 = 1e-3;

fn level(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.1 - b.1).abs() < STRAIGHT
}

fn plumb(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() < STRAIGHT
}

/// A subpath's points, and whether it's closed.
type Subpath = (Vec<(f32, f32)>, bool);

/// The path's subpaths through `map`. None if it has a curve.
fn subpaths(path: &PathData, map: impl Fn(f32, f32) -> (f32, f32)) -> Option<Vec<Subpath>> {
    let mut out: Vec<Subpath> = Vec::new();
    let mut open = false;
    for v in &path.verbs {
        match *v {
            Verb::Move(x, y) => {
                out.push((vec![map(x, y)], false));
                open = true;
            }
            Verb::Line(x, y) => {
                if !open {
                    // After a close, or with no move: from where the last subpath began.
                    let start = out.last().map_or(map(0.0, 0.0), |(pts, _)| pts[0]);
                    out.push((vec![start], false));
                    open = true;
                }
                if let Some((pts, _)) = out.last_mut() {
                    pts.push(map(x, y));
                }
            }
            Verb::Quad(..) | Verb::Cubic(..) => return None,
            Verb::Close => {
                if open && let Some((_, closed)) = out.last_mut() {
                    *closed = true;
                }
                open = false;
            }
        }
    }
    Some(out)
}

/// A subpath's segments, with the closing one if it's closed, leaving out any with no length.
fn segments(pts: &[(f32, f32)], closed: bool) -> impl Iterator<Item = ((f32, f32), (f32, f32))> {
    let back = (closed && pts.len() > 1).then(|| (pts[pts.len() - 1], pts[0]));
    pts.windows(2)
        .map(|w| (w[0], w[1]))
        .chain(back)
        .filter(|(a, b)| a != b)
}

/// An axis-aligned hairline subpath as whole-pixel rectangles, `w` px wide: across, centred
/// on the middle of a pixel for an odd width and on a pixel's edge for an even one; along, from
/// edge to edge rounded to whole pixels, past each corner by half the width so the corner
/// fills, and past each end by half the width for a round or square cap. Dashed (`on` and `off`
/// physical px), each dash is a piece with its caps, the pattern running on round corners and
/// a dash lit across a corner filling it.
///
/// Only what can show in `view` is made: each line is cut a few px past it, and of its dashes
/// only those in what's left, each found by its number from where the pattern stands at the
/// line's start. So a line far past the picture, or out to infinity, costs what the picture
/// does, and no dash is lost to rounding however long the line.
fn hair_rects(
    pts: &[(f32, f32)],
    closed: bool,
    w: f32,
    cap: Cap,
    dash: Option<(f64, f64)>,
    view: Clip,
    pb: &mut sk::PathBuilder,
) {
    let odd = (w as i32) % 2 == 1;
    let band = |c: f32| {
        let mid = if odd { c.floor() + 0.5 } else { c.round() };
        (mid - w / 2.0, mid + w / 2.0)
    };
    let edge = |v: f64| (v + 0.5).floor();
    let half = f64::from(w) / 2.0;
    let capped = if cap == Cap::Butt { 0.0 } else { half };
    // Past the view by more than an end or a corner reaches, and a whole number of px, so a
    // line cut there shows the same pixels in the view as one that isn't.
    let margin = f64::from(w) + 2.0;
    let segs: Vec<_> = segments(pts, closed).collect();
    let n = segs.len();
    // How far into the pattern the line starts, from 0 to a period.
    let mut phase = 0.0f64;
    for (j, &(a, b)) in segs.iter().enumerate() {
        let across = level(a, b);
        // Along the line: from `s` to `e`; across it at `c`.
        let (s, e, c) = if across {
            (a.0, b.0, a.1)
        } else {
            (a.1, b.1, a.0)
        };
        let (s, e) = (f64::from(s), f64::from(e));
        let len = (e - s).abs();
        let start = phase;
        if let Some((on, off)) = dash {
            phase = (phase + len).rem_euclid(on + off);
            if !phase.is_finite() {
                phase = 0.0;
            }
        }
        // A dashed line from infinity has no place to start its pattern; a line off the view
        // across shows nothing.
        if (dash.is_some() && !s.is_finite()) || !c.is_finite() {
            continue;
        }
        let (lo, hi) = band(c);
        let (v0, v1) = if across {
            (view.y0, view.y1)
        } else {
            (view.x0, view.x1)
        };
        if hi <= v0 as f32 || lo >= v1 as f32 {
            continue;
        }
        let (w0, w1) = if across {
            (view.x0, view.x1)
        } else {
            (view.y0, view.y1)
        };
        let (w0, w1) = (f64::from(w0) - margin, f64::from(w1) + margin);
        let mut put = |p: f64, q: f64| {
            let (p, q) = (edge(p.min(q).max(w0)), edge(p.max(q).min(w1)));
            if p.is_nan() || q.is_nan() || q <= p {
                return;
            }
            let (p, q) = (p as f32, q as f32);
            let r = if across {
                sk::Rect::from_ltrb(p, lo, q, hi)
            } else {
                sk::Rect::from_ltrb(lo, p, hi, q)
            };
            if let Some(r) = r {
                pb.push_rect(r);
            }
        };
        let dir = if e >= s { 1.0 } else { -1.0 };
        let first = if closed || j > 0 { half } else { capped };
        let last = if closed || j + 1 < n { half } else { capped };
        let Some((on, off)) = dash else {
            put(s - dir * first, e + dir * last);
            continue;
        };
        // The stretch of the line the view can see, as distances along it from `s`.
        let (t0, t1) = if dir > 0.0 {
            (w0 - s, w1 - s)
        } else {
            (s - w1, s - w0)
        };
        let (t0, t1) = (t0.max(0.0), t1.min(len));
        if t0.is_nan() || t1.is_nan() || t1 <= t0 {
            continue;
        }
        // Dash k is lit from k periods less `start` for `on`: the first that ends past `t0`
        // to the last that starts before `t1`.
        let period = on + off;
        let mut k = ((t0 + start - on) / period).floor().max(0.0);
        let most = ((t1 + start) / period).ceil();
        while k < most {
            let from = k * period - start;
            let (p, q) = (from.max(0.0), (from + on).min(len));
            if q > p {
                // A dash lit at the line's start or end is lit across the corner there.
                let back = if p <= 0.0 { first } else { capped };
                let on_past = if q >= len { last } else { capped };
                put(s + dir * (p - back), s + dir * (q + on_past));
            }
            k += 1.0;
        }
    }
}

// ------------------------------------------------------------------ shadows

/// A shadow's coverage, 0 to 1, over `seen` (rows of its width): the rounded rectangle `r`'s
/// over `region`, blurred to σ = `sigma`. Only the rows and columns `seen` needs are blurred,
/// and in whole numbers, so a pixel's coverage is the same whatever `seen` is: a box redrawn
/// gets what a whole redraw does. The region's size bounds the sums well within a u64.
fn shadow_coverage(
    r: sk::Rect,
    radius: f32,
    sigma: f32,
    region: Clip,
    seen: Clip,
) -> Option<Vec<f32>> {
    let (rw, rh) = (
        (region.x1 - region.x0) as u32,
        (region.y1 - region.y0) as u32,
    );
    if u64::from(rw) * u64::from(rh) > 1 << 26 {
        return None;
    }
    let mut mask = sk::Mask::new(rw, rh)?;
    let local = Rect::new(
        r.left() - region.x0 as f32,
        r.top() - region.y0 as f32,
        r.width(),
        r.height(),
    );
    let radius = if radius.is_finite() {
        radius.max(0.0)
    } else {
        0.0
    };
    let path = sk_path(&PathData::rounded(local, [radius; 4]))?;
    mask.fill_path(
        &path,
        sk::FillRule::Winding,
        true,
        sk::Transform::identity(),
    );
    let (rw, rh) = (rw as usize, rh as usize);
    let cov = mask.data();
    let radii = boxes(sigma);
    let margin: usize = radii.iter().sum();
    // What three box sums of these radii multiply a value by.
    let gain: u64 = radii.iter().map(|&k| 2 * k as u64 + 1).product();
    // The rows and columns of `seen`, from the region's corner.
    let (sx0, sx1) = (
        (seen.x0 - region.x0) as usize,
        (seen.x1 - region.x0) as usize,
    );
    let (sy0, sy1) = (
        (seen.y0 - region.y0) as usize,
        (seen.y1 - region.y0) as usize,
    );
    let sw = sx1 - sx0;
    let rows = sy0.saturating_sub(margin)..(sy1 + margin).min(rh);
    // Across each row: three box sums over the whole row, then `seen`'s columns as 256ths of
    // a level of coverage, rounded.
    let mut line = Vec::with_capacity(rw);
    let mut tmp = Vec::new();
    let mut across = Vec::with_capacity(rows.len() * sw);
    for y in rows.clone() {
        line.clear();
        line.extend(cov[y * rw..(y + 1) * rw].iter().map(|&v| u64::from(v)));
        for &k in &radii {
            box_sum(&mut line, &mut tmp, k);
        }
        across.extend(line[sx0..sx1].iter().map(|&v| (v * 256 + gain / 2) / gain));
    }
    // Down each of `seen`'s columns, the same.
    let whole = (gain * 256 * 255) as f64;
    let mut out = vec![0.0f32; sw * (sy1 - sy0)];
    let mut col = Vec::with_capacity(rows.len());
    for x in 0..sw {
        col.clear();
        col.extend((0..rows.len()).map(|i| across[i * sw + x]));
        for &k in &radii {
            box_sum(&mut col, &mut tmp, k);
        }
        for y in sy0..sy1 {
            let v = col[y - rows.start] as f64 / whole;
            out[(y - sy0) * sw + x] = v.clamp(0.0, 1.0) as f32;
        }
    }
    Some(out)
}

/// The radii of three box blurs that together stand in for a Gaussian of σ = `sigma`.
fn boxes(sigma: f32) -> [usize; 3] {
    if sigma.is_nan() || sigma <= 0.0 {
        return [0; 3];
    }
    let n = 3.0f32;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    // How many of the three take the narrower box.
    let l = wl as f32;
    let m = ((12.0 * sigma * sigma - n * l * l - 4.0 * n * l - 3.0 * n) / (-4.0 * l - 4.0)).round()
        as i32;
    let size = |i: i32| if i < m { wl } else { wu };
    [0, 1, 2].map(|i| ((size(i) - 1) / 2) as usize)
}

/// Box sums of radius `r` along a line, in place: each value the sum of those within `r` of
/// it, with nothing past the line's ends. In whole numbers, so each sum is exact wherever the
/// line starts.
fn box_sum(line: &mut [u64], tmp: &mut Vec<u64>, r: usize) {
    let n = line.len();
    if r == 0 || n == 0 {
        return;
    }
    tmp.clear();
    tmp.extend_from_slice(line);
    let mut acc: u64 = tmp[..(r + 1).min(n)].iter().sum();
    for i in 0..n {
        line[i] = acc;
        if i + r + 1 < n {
            acc += tmp[i + r + 1];
        }
        if i >= r {
            acc -= tmp[i - r];
        }
    }
}
