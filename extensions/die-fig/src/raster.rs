//! A scene to pixels: tiny-skia for paths, gradients, clips and opacity, die-draw for text, into
//! premultiplied RGBA8 within a damage box; the measure over die-draw's fonts; and PNG export.
//!
//! Everything is drawn at the scene's scale, its logical px times `scale` physical. What the
//! raster decides on its own:
//!
//! - **Damage.** Only the damage box is cleared and drawn, and nothing outside it is written.
//!   Paths are laid out over the whole buffer and masked to the box: tiny-skia cuts a path at
//!   the edges of what it draws into, and one cut at the box's edges would antialias
//!   differently there, so a box redrawn is exactly what a whole redraw puts in it. What lies
//!   wholly outside the box isn't rasterised at all. A buffer whose rows have bytes past them
//!   is drawn into a packed copy and the box copied across.
//! - **Hairlines** are `max(1, ⌊scale⌋)` physical px wide, as die-markdown's rules are: one px at
//!   1 and 1.5, two at 2. Each line's centre goes to the middle of a pixel for an odd width
//!   (`⌊v⌋ + ½`) and to a pixel's edge for an even one (`round(v)`). A hairline made only of
//!   horizontal and vertical lines is filled as whole-pixel rectangles, its ends rounded to
//!   whole pixels and its corners filled, so it's never blurred; one with a slant or a curve is
//!   stroked at that width.
//! - **Clips** are rectangles rounded to the nearest physical pixel; nested ones intersect.
//! - **Opacity** draws its group into a layer of its own and lays it down at the opacity once,
//!   so overlapping marks in a faded group don't darken where they cross.
//! - **Shadows** are black, the rounded rectangle's coverage blurred by three box blurs that
//!   stand in for a Gaussian of σ = `blur / 2` (CSS's reading of a blur radius), cut off
//!   `2 × blur` past the rectangle.
//! - **Text** needs a [`die_draw::Book`]: without one, none is drawn.

use crate::scene::{
    Anchor, Cap, Item, Join, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec, Verb, Weight,
};
use crate::theme::{Color, draw_rgb};
use die_draw::{Book, Clip, Pass, Pixels, TextLook};
use std::borrow::Cow;
use tiny_skia as sk;

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
        self.book.pass().extent(text, look(spec), s).width / s
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
    fn fit(&mut self, text: &str, spec: TextSpec, max: f32) -> String {
        let s = usable(self.scale);
        fit(&mut self.book.pass(), text, look(spec), s, max * s).into_owned()
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
    Layer(Vec<u8>, f32),
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
    /// The mask of the last clip drawn through, when it isn't the whole box.
    mask: Option<(Clip, sk::Mask)>,
    pass: Option<&'f mut Pass<'b>>,
}

/// The pixels being drawn into: the innermost layer's, else the box's.
fn surface<'x>(bytes: &'x mut [u8], groups: &'x mut [Group]) -> &'x mut [u8] {
    let layer = groups.iter_mut().rev().find_map(|g| match g {
        Group::Layer(px, _) => Some(px),
        _ => None,
    });
    match layer {
        Some(px) => px,
        None => bytes,
    }
}

/// Draws `scene` into `area` of `bytes`, a packed picture `w × h`, cleared first. Everything
/// is laid out over the whole picture and masked to the area, so what the area gets is what a
/// whole redraw would put there: tiny-skia cuts a path at the edges of what it draws into,
/// and a path cut at the area's edges would antialias differently there.
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
    /// All of the picture.
    fn whole(&self) -> Clip {
        Clip {
            x0: 0,
            y0: 0,
            x1: self.w as i32,
            y1: self.h as i32,
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
                if self.outward(b.inflate(reach + 1.0)).meet(clip).is_empty() {
                    return;
                }
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
            Group::Layer(vec![0; self.bytes.len()], a)
        };
        self.groups.push(g);
    }

    /// Ends the innermost group: a layer is laid down at its opacity on what's under it.
    fn pop_group(&mut self) {
        match self.groups.pop() {
            None | Some(Group::Through) => {}
            Some(Group::Hidden) => self.hidden -= 1,
            Some(Group::Layer(layer, a)) => {
                let (w, area) = (self.w as usize, self.area);
                let under = surface(self.bytes, &mut self.groups);
                let k = (a * 255.0 + 0.5) as u32;
                let part = |v: u8| (u32::from(v) * k + 127) / 255;
                for y in area.y0 as usize..area.y1 as usize {
                    let (from, to) = (y * w + area.x0 as usize, y * w + area.x1 as usize);
                    let src = layer[from * 4..to * 4].chunks_exact(4);
                    for (dst, src) in under[from * 4..to * 4].chunks_exact_mut(4).zip(src) {
                        if src[3] == 0 {
                            continue;
                        }
                        let rest = 255 - part(src[3]);
                        for (d, &s) in dst.iter_mut().zip(src) {
                            *d = (part(s) + (u32::from(*d) * rest + 127) / 255).min(255) as u8;
                        }
                    }
                }
            }
        }
    }

    /// Draws through tiny-skia into the current surface, masked to `clip` unless it's the
    /// whole picture.
    fn with(&mut self, clip: Clip, f: impl FnOnce(&mut sk::PixmapMut<'_>, Option<&sk::Mask>)) {
        let whole = clip == self.whole();
        if !whole && self.mask.as_ref().is_none_or(|(c, _)| *c != clip) {
            self.mask = rect_mask(clip, self.w, self.h).map(|m| (clip, m));
        }
        let mask = match &self.mask {
            Some((c, m)) if !whole && *c == clip => Some(m),
            _ if whole => None,
            _ => return,
        };
        let (w, h) = (self.w, self.h);
        let px = surface(self.bytes, &mut self.groups);
        if let Some(mut pm) = sk::PixmapMut::from_bytes(px, w, h) {
            f(&mut pm, mask);
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
        let dash = s
            .dash
            .map(|(on, off)| (on * self.scale, off * self.scale))
            .filter(|&(on, off)| on > 0.0 && off >= 0.0 && (on + off).is_finite());
        let mut pb = sk::PathBuilder::new();
        for (pts, closed) in &subpaths {
            hair_rects(pts, *closed, w, s.cap, dash, &mut pb);
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
                let cut = fit(pass, text, look, s, m * s);
                if cut.is_empty() {
                    return;
                }
                pass.set(&cut, look, s)
            }
            None => pass.set(text, look, s),
        };
        let width = line.extent.width;
        let x = at.0 * s
            - match anchor {
                Anchor::Start => 0.0,
                Anchor::Middle => width / 2.0,
                Anchor::End => width,
            };
        let baseline = at.1 * s;
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
        let blur = if blur.is_finite() { blur.max(0.0) } else { 0.0 };
        if a <= 0.0 || rect.is_empty() || !dy.is_finite() {
            return;
        }
        let r = sk::Rect::from_xywh(rect.x * s, (rect.y + dy) * s, rect.w * s, rect.h * s);
        let Some(r) = r else { return };
        let reach = (2.0 * blur * s).ceil();
        let region = Clip {
            x0: (r.left() - reach).floor() as i32,
            y0: (r.top() - reach).floor() as i32,
            x1: (r.right() + reach).ceil() as i32,
            y1: (r.bottom() + reach).ceil() as i32,
        };
        let seen = region.meet(clip);
        if seen.is_empty() {
            return;
        }
        let Some(cov) = shadow_coverage(r, radius * s, blur * s / 2.0, region, seen) else {
            return;
        };
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

/// A clip's mask over a box `w × h`: 255 inside, 0 out.
fn rect_mask(c: Clip, w: u32, h: u32) -> Option<sk::Mask> {
    let mut m = sk::Mask::new(w, h)?;
    let c = c.meet(Clip {
        x0: 0,
        y0: 0,
        x1: w as i32,
        y1: h as i32,
    });
    if !c.is_empty() {
        let data = m.data_mut();
        for y in c.y0..c.y1 {
            let at = y as usize * w as usize;
            data[at + c.x0 as usize..at + c.x1 as usize].fill(255);
        }
    }
    Some(m)
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
/// fills, and past each end by half the width for a round or square cap. Dashed, each dash is
/// a piece with its caps, the pattern running on round corners.
fn hair_rects(
    pts: &[(f32, f32)],
    closed: bool,
    w: f32,
    cap: Cap,
    dash: Option<(f32, f32)>,
    pb: &mut sk::PathBuilder,
) {
    let odd = (w as i32) % 2 == 1;
    let band = |c: f32| {
        let mid = if odd { c.floor() + 0.5 } else { c.round() };
        (mid - w / 2.0, mid + w / 2.0)
    };
    let edge = |v: f32| (v + 0.5).floor();
    let capped = if cap == Cap::Butt { 0.0 } else { w / 2.0 };
    let segs: Vec<_> = segments(pts, closed).collect();
    let n = segs.len();
    let mut phase = 0.0f32;
    for (j, &(a, b)) in segs.iter().enumerate() {
        let across = level(a, b);
        // Along the line: from `s` to `e`; across it at `c`.
        let (s, e, c) = if across {
            (a.0, b.0, a.1)
        } else {
            (a.1, b.1, a.0)
        };
        let (lo, hi) = band(c);
        let mut put = |p: f32, q: f32| {
            let (p, q) = (edge(p), edge(q));
            if q <= p {
                return;
            }
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
        let len = (e - s).abs();
        match dash {
            None => {
                let first = if closed || j > 0 { w / 2.0 } else { capped };
                let last = if closed || j + 1 < n { w / 2.0 } else { capped };
                let (p, q) = (s - dir * first, e + dir * last);
                put(p.min(q), p.max(q));
            }
            Some((on, off)) => {
                let period = on + off;
                let mut t = 0.0f32;
                while t < len {
                    let into = phase % period;
                    let (step, lit) = if into < on {
                        (on - into, true)
                    } else {
                        (period - into, false)
                    };
                    let step = step.min(len - t);
                    if lit {
                        let (p, q) = (s + dir * (t - capped), s + dir * (t + step + capped));
                        put(p.min(q), p.max(q));
                    }
                    t += step;
                    phase += step;
                }
            }
        }
    }
}

// ------------------------------------------------------------------ shadows

/// A shadow's coverage, 0 to 1, over `seen` (rows of its width): the rounded rectangle `r`'s
/// over `region`, blurred to σ = `sigma`. Only the rows and columns `seen` needs are blurred.
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
    let mut cov: Vec<f32> = mask.data().iter().map(|&v| f32::from(v) / 255.0).collect();
    let radii = boxes(sigma);
    let margin: usize = radii.iter().sum();
    // The rows and columns of `seen`, from the region's corner.
    let (sx0, sx1) = (
        (seen.x0 - region.x0) as usize,
        (seen.x1 - region.x0) as usize,
    );
    let (sy0, sy1) = (
        (seen.y0 - region.y0) as usize,
        (seen.y1 - region.y0) as usize,
    );
    let rows = sy0.saturating_sub(margin)..(sy1 + margin).min(rh);
    let mut tmp = Vec::new();
    if margin > 0 {
        for y in rows.clone() {
            let line = &mut cov[y * rw..(y + 1) * rw];
            for &k in &radii {
                blur_line(line, &mut tmp, k);
            }
        }
        let mut col = Vec::with_capacity(rows.len());
        for x in sx0..sx1 {
            col.clear();
            col.extend(rows.clone().map(|y| cov[y * rw + x]));
            for &k in &radii {
                blur_line(&mut col, &mut tmp, k);
            }
            for (i, y) in rows.clone().enumerate() {
                cov[y * rw + x] = col[i];
            }
        }
    }
    let mut out = Vec::with_capacity((sx1 - sx0) * (sy1 - sy0));
    for y in sy0..sy1 {
        out.extend(
            cov[y * rw + sx0..y * rw + sx1]
                .iter()
                .map(|v| v.clamp(0.0, 1.0)),
        );
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

/// A box blur of radius `r` along a line, in place, with nothing past its ends.
fn blur_line(line: &mut [f32], tmp: &mut Vec<f32>, r: usize) {
    let n = line.len();
    if r == 0 || n == 0 {
        return;
    }
    tmp.clear();
    tmp.extend_from_slice(line);
    let k = 1.0 / (2 * r + 1) as f32;
    let mut acc: f32 = tmp[..(r + 1).min(n)].iter().sum();
    for i in 0..n {
        line[i] = acc * k;
        if i + r + 1 < n {
            acc += tmp[i + r + 1];
        }
        if i >= r {
            acc -= tmp[i - r];
        }
    }
}
