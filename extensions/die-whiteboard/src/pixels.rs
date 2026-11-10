//! The pixel renderer: the board drawn by tiny-skia (through [`crate::canvas`]) into one pixel
//! buffer the size of the area, placed under the area's glyphs (core's M3a: `Ctx::pixel_buffer`,
//! `Frame::place` at z −1), so ink is anti-aliased at the display's resolution, with round caps
//! and joins and the highlighter's alpha, and labels stay cells over it, in the editor's font.
//!
//! What it keeps: the **committed picture** (the board and the selection's outlines, no gesture)
//! on a canvas of its own, and the **shown buffer**, the producer's, which is the committed
//! picture with the gesture's live element drawn over it. A pointer sample during a pen stroke
//! draws only the segments after the ones drawn and presents their rectangle; any other live
//! thing (a shape, the highlighter, the marquee) is put back and drawn whole, which is small. A
//! committed change ([`Damage::Region`]) draws that region of the picture again; undo, a resize,
//! a zoom, a theme change or the selection draw it all; a pan by whole pixels moves the picture
//! and draws the strips it uncovers. Everything runs on the main thread, in the event's own turn
//! for a pointer event ([`Renderer::update`]) and in `render` for a command's change, so the
//! frame that follows carries the pixels; the producer thread the contract allows stays in
//! reserve.
//!
//! The surface picks it when the host shows pixels (`Ctx::pixels_supported()`) and your options
//! don't ask for cells; [`crate::cells`] draws otherwise (Linux until the wgpu ring).

use crate::canvas::{Canvas, Map, Target};
use crate::model::{Element, LIVE, Point, Rect, Shape, label_lines};
use crate::render::{Damage, Renderer, Scene};
use crate::svg::Look;
use die_core::{
    CellPixels, Color, Ctx, Frame, FrameCell, Glyph, PixelBufferId, PixelRect, Producer, RenderInfo,
};

/// Around a selected element's bounds, in board units: where its outline goes.
pub const SELECTION_PAD: f32 = 3.0;
/// The selection's outline's opacity.
const SELECTION_ALPHA: f32 = 0.55;
/// The marquee's opacity.
const MARQUEE_ALPHA: f32 = 0.8;
/// Pixels around anything drawn, for its anti-aliased edge and an outline's own width.
const AA_PAD: f32 = 2.0;
/// A board with this many elements or more is drawn whole in bands, one thread each: a path
/// costs tiny-skia about 0.1 ms with round joins, so a dense board's full redraw (zoom, resize,
/// a theme change) is split across the cores. Fewer draw on this thread.
const BANDS_FROM: usize = 64;

/// The renderer: the buffer, once the area is known, and what's drawn on it.
pub struct Pixels {
    /// The cells in pixels, as the last resize said.
    cell: CellPixels,
    /// The buffer and the committed picture, once the host gave one.
    buffer: Option<Buffer>,
    /// What the committed picture owes.
    owed: Damage,
    /// The map the committed picture was drawn through, so a pan can move it instead.
    drawn_at: Option<Map>,
    /// The gesture's drawing on the shown buffer.
    live: Option<Live>,
    /// The host refused a buffer, which is said once.
    refused: bool,
}

struct Buffer {
    producer: Producer,
    committed: Canvas,
    /// Nothing presented since it was made or resized: it's transparent, and an empty board
    /// needn't present a transparent buffer to show nothing.
    fresh: bool,
}

/// What the gesture's live element looks like, so a sample that changed nothing draws nothing.
#[derive(Clone, Copy, PartialEq, Debug)]
enum LiveKey {
    /// A pen stroke: its first point, how many points, its last.
    Stroke(Point, usize, Point),
    /// Anything drawn whole: its bounds.
    Whole(Rect),
}

/// What of a gesture is on the shown buffer.
struct Live {
    /// The pixels it covers, to put back from the committed picture.
    rect: PixelRect,
    key: LiveKey,
}

/// The gesture's thing to draw over the committed picture.
enum Thing<'a> {
    Element(&'a Element),
    Marquee(Rect),
}

impl Thing<'_> {
    fn key(&self, cell: crate::model::CellSize) -> LiveKey {
        match self {
            Thing::Element(el) => match &el.shape {
                Shape::Stroke {
                    points,
                    highlighter: false,
                } if !points.is_empty() => {
                    LiveKey::Stroke(points[0], points.len(), points[points.len() - 1])
                }
                _ => LiveKey::Whole(el.bounds(cell)),
            },
            Thing::Marquee(r) => LiveKey::Whole(*r),
        }
    }
}

impl Default for Pixels {
    fn default() -> Pixels {
        Pixels::new()
    }
}

impl Pixels {
    /// A renderer with no buffer yet: the first resize makes one.
    pub fn new() -> Pixels {
        Pixels {
            cell: CellPixels::default(),
            buffer: None,
            owed: Damage::All,
            drawn_at: None,
            live: None,
            refused: false,
        }
    }

    /// The buffer it shows, once it has one.
    pub fn buffer(&self) -> Option<PixelBufferId> {
        self.buffer.as_ref().map(|b| b.producer.id())
    }

    /// Bring the shown buffer up to date with the scene and present what changed.
    fn sync(&mut self, scene: &Scene<'_>) {
        let Some(buf) = &mut self.buffer else {
            return;
        };
        if buf.fresh && scene.board.is_empty() && scene.live.is_none() && scene.marquee.is_none() {
            return;
        }
        let (w, h) = buf.producer.size();
        let whole = PixelRect::whole(w, h);
        let map = Map {
            scale: scene.view.factor(self.cell.scale),
            tx: scene.view.pan.0,
            ty: scene.view.pan.1,
        };
        let look = Look {
            inks: scene.inks.map(|c| match c {
                Color::Default => scene.fg,
                Color::Rgb(c) => c,
            }),
            bg: None,
            cell: scene.cell,
        };
        let pad = SELECTION_PAD * map.scale + AA_PAD;
        let picture = Picture {
            scene,
            look: &look,
            map,
            whole: (w, h),
        };

        // 1. The committed picture: what changed on it, if anything.
        let owed = std::mem::take(&mut self.owed);
        let moved = self
            .drawn_at
            .filter(|old| *old != map)
            .and_then(|old| shift_between(old, map, w, h));
        let changed: Option<PixelRect> = match (self.drawn_at == Some(map), moved, owed) {
            (true, _, Damage::None | Damage::View) => None,
            (true, _, Damage::Region(r)) => {
                let pr = map.pixels(&r, pad, w, h);
                (!pr.is_empty()).then(|| {
                    picture.redraw(&mut buf.committed, pr);
                    pr
                })
            }
            (false, Some((dx, dy)), owed) if owed != Damage::All => {
                buf.committed.shift(dx, dy);
                for strip in buf.committed.shift_exposes(dx, dy) {
                    picture.redraw(&mut buf.committed, strip);
                }
                if let Damage::Region(r) = owed {
                    let pr = map.pixels(&r, pad, w, h);
                    if !pr.is_empty() {
                        picture.redraw(&mut buf.committed, pr);
                    }
                }
                Some(whole)
            }
            _ => {
                buf.committed.clear();
                let drawn = picture.draw_all(&mut buf.committed);
                // A fresh buffer is transparent, as the picture was: only what's drawn changed.
                if buf.fresh { drawn } else { Some(whole) }
            }
        };
        self.drawn_at = Some(map);

        // 2. The shown buffer: the committed picture where it changed and under the old live
        //    drawing, then the live drawing.
        let thing = match (scene.live, scene.marquee) {
            (Some(el), _) => Some(Thing::Element(el)),
            (None, Some(m)) => Some(Thing::Marquee(m)),
            (None, None) => None,
        };
        let key = thing.as_ref().map(|t| t.key(scene.cell));
        let was = self.live.take();
        if changed.is_none() && key == was.as_ref().map(|l| l.key) {
            self.live = was;
            return;
        }
        let grown = match (&thing, &was, changed) {
            (Some(Thing::Element(el)), Some(was), None) => stroke_grown(el, was),
            _ => None,
        };
        let present =
            if let (Some(drawn), Some(Thing::Element(el)), Some(was)) = (grown, &thing, &was) {
                // The pen: only the segments after the ones drawn.
                let Shape::Stroke { points, .. } = &el.shape else {
                    unreachable!("stroke_grown says so")
                };
                let mut target = Target::Bytes {
                    bytes: buf.producer.pixels_mut(),
                    width: w,
                    height: h,
                };
                let seg = Element {
                    id: LIVE,
                    ink: el.ink,
                    width: el.width,
                    shape: Shape::Stroke {
                        points: points[drawn - 1..].to_vec(),
                        highlighter: false,
                    },
                };
                let r = map.pixels(&seg.bounds(scene.cell), AA_PAD, w, h);
                target.draw(&seg, &look, map);
                self.live = Some(Live {
                    rect: was.rect.union(r),
                    key: key.expect("a stroke"),
                });
                r
            } else {
                let restore = union(changed, was.map(|l| l.rect));
                if let Some(r) = restore {
                    buf.committed.copy_to(buf.producer.pixels_mut(), r);
                }
                let mut target = Target::Bytes {
                    bytes: buf.producer.pixels_mut(),
                    width: w,
                    height: h,
                };
                let drawn = thing.as_ref().map(|t| match t {
                    Thing::Element(el) => {
                        target.draw(el, &look, map);
                        map.pixels(&el.bounds(scene.cell), AA_PAD, w, h)
                    }
                    Thing::Marquee(m) => {
                        target.outline(m, map, scene.fg, MARQUEE_ALPHA, true);
                        map.pixels(m, AA_PAD, w, h)
                    }
                });
                self.live = drawn.zip(key).map(|(rect, key)| Live { rect, key });
                match union(restore, drawn) {
                    Some(r) => r,
                    None => return,
                }
            };
        if !present.is_empty() {
            buf.fresh = false;
            buf.producer.present(present);
        }
    }
}

/// The committed picture: how to draw any part of it.
struct Picture<'a, 'b> {
    scene: &'a Scene<'b>,
    look: &'a Look,
    map: Map,
    whole: (u32, u32),
}

impl Picture<'_, '_> {
    /// Draw the elements and the selection's outlines that reach `rect` (pixels of the whole
    /// picture) into `into`, a target whose top-left pixel is `rect`'s. The pixels of the whole
    /// picture they cover, if any did.
    fn draw(&self, into: &mut Target<'_>, rect: PixelRect) -> Option<PixelRect> {
        let local = self.map.into_rect(rect);
        let pad = SELECTION_PAD * self.map.scale + AA_PAD;
        let mut covered = None;
        for el in &self.scene.board.elements {
            let b = el.bounds(self.scene.cell);
            let px = self.map.pixels(&b, pad, self.whole.0, self.whole.1);
            if !intersects(px, rect) {
                continue;
            }
            covered = union(covered, Some(px));
            into.draw(el, self.look, local);
            if self.scene.selection.contains(&el.id) {
                let around = b.inflate(SELECTION_PAD);
                into.outline(&around, local, self.scene.fg, SELECTION_ALPHA, true);
            }
        }
        covered
    }

    /// Draw the whole picture on `committed`, which is clear: in bands on threads of their own
    /// for a dense board, else here. The pixels covered, if any.
    fn draw_all(&self, committed: &mut Canvas) -> Option<PixelRect> {
        let (w, h) = (committed.width(), committed.height());
        let whole = PixelRect::whole(w, h);
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let bands = threads.min(8).min(h as usize / 64);
        if self.scene.board.len() < BANDS_FROM || bands < 2 {
            return self.draw(&mut committed.target(), whole);
        }
        let step = h.div_ceil(bands as u32);
        let strips: Vec<PixelRect> = (0..h)
            .step_by(step as usize)
            .map(|y| PixelRect::new(0, y, w, step.min(h - y)))
            .collect();
        let drawn: Vec<(PixelRect, Option<Canvas>, Option<PixelRect>)> = std::thread::scope(|s| {
            let handles: Vec<_> = strips
                .iter()
                .map(|&strip| {
                    s.spawn(move || {
                        let mut part = Canvas::new(strip.width, strip.height);
                        let covered = part
                            .as_mut()
                            .and_then(|p| self.draw(&mut p.target(), strip));
                        (strip, part, covered)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|j| j.join().expect("a band's thread doesn't panic"))
                .collect()
        });
        let mut covered = None;
        for (strip, part, drew) in drawn {
            if let Some(part) = part {
                committed.paste(&part, strip.x, strip.y);
            }
            covered = union(covered, drew);
        }
        covered
    }

    /// Draw `rect` of the picture again on `committed`.
    fn redraw(&self, committed: &mut Canvas, rect: PixelRect) {
        let Some(mut part) = Canvas::new(rect.width, rect.height) else {
            return;
        };
        self.draw(&mut part.target(), rect);
        committed.paste(&part, rect.x, rect.y);
    }
}

/// How many of a pen stroke's points are drawn already, if `el` is the stroke `was` drew with
/// points added since, and nothing else changed.
fn stroke_grown(el: &Element, was: &Live) -> Option<usize> {
    let LiveKey::Stroke(first, drawn, _) = was.key else {
        return None;
    };
    match &el.shape {
        Shape::Stroke {
            points,
            highlighter: false,
        } if points.first() == Some(&first) && points.len() > drawn && drawn >= 1 => Some(drawn),
        _ => None,
    }
}

/// The whole-pixel move from `old` to `new`, if that's all that changed and it's less than the
/// picture.
fn shift_between(old: Map, new: Map, w: u32, h: u32) -> Option<(i32, i32)> {
    if old.scale != new.scale {
        return None;
    }
    let (dx, dy) = (new.tx - old.tx, new.ty - old.ty);
    let whole = |d: f32| (d - d.round()).abs() < 1e-3;
    if !whole(dx) || !whole(dy) || dx.abs() >= w as f32 || dy.abs() >= h as f32 {
        return None;
    }
    Some((dx.round() as i32, dy.round() as i32))
}

fn intersects(a: PixelRect, b: PixelRect) -> bool {
    !a.is_empty()
        && !b.is_empty()
        && a.x < b.x + b.width
        && b.x < a.x + a.width
        && a.y < b.y + b.height
        && b.y < a.y + a.height
}

fn union(a: Option<PixelRect>, b: Option<PixelRect>) -> Option<PixelRect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

impl Renderer for Pixels {
    fn name(&self) -> &'static str {
        "pixels"
    }

    fn resize(&mut self, cols: usize, rows: usize, cell: CellPixels, ctx: &mut Ctx) {
        self.cell = cell;
        let want = (cols as u32 * cell.width, rows as u32 * cell.height);
        if self
            .buffer
            .as_ref()
            .is_some_and(|b| b.producer.size() == want)
        {
            return;
        }
        self.live = None;
        self.drawn_at = None;
        self.owed = Damage::All;
        if want.0 == 0 || want.1 == 0 {
            self.buffer = None;
            return;
        }
        let Some(committed) = Canvas::new(want.0, want.1) else {
            self.buffer = None;
            return;
        };
        match self.buffer.take() {
            Some(mut b) => {
                if b.producer.resize(want.0, want.1).is_ok() {
                    b.committed = committed;
                    b.fresh = true;
                    self.buffer = Some(b);
                }
            }
            None => match ctx.pixel_buffer(want.0, want.1) {
                Ok((_, producer)) => {
                    self.buffer = Some(Buffer {
                        producer,
                        committed,
                        fresh: true,
                    });
                }
                Err(e) => {
                    if !self.refused {
                        ctx.messages
                            .push(format!("whiteboard: no pixel buffer: {e}"));
                    }
                    self.refused = true;
                }
            },
        }
    }

    fn board_changed(&mut self, damage: Damage) {
        self.owed = self.owed.with(damage);
    }

    fn update(&mut self, scene: &Scene<'_>) {
        self.sync(scene);
    }

    fn place(&mut self, frame: &mut Frame) {
        if let Some(b) = &self.buffer {
            frame.place(b.producer.id(), -1);
        }
    }

    /// The pixels, then the labels as cells over them, a selected one in reverse video.
    fn render(&mut self, frame: &mut Frame, scene: &Scene<'_>, _: &RenderInfo) {
        self.sync(scene);
        frame.clear();
        // The cells don't zoom: a label's text keeps its size where its corner goes.
        let (cw, ch) = if self.cell.width == 0 || self.cell.height == 0 {
            crate::cells::FALLBACK_CELL
        } else {
            (self.cell.width as f32, self.cell.height as f32)
        };
        for el in &scene.board.elements {
            let Shape::Label { at, text } = &el.shape else {
                continue;
            };
            let (x, y) = scene.view.to_physical(*at, self.cell.scale);
            let (c0, r0) = ((x / cw).floor() as i64, (y / ch).floor() as i64);
            let fg = scene.inks[usize::from(el.ink.clamp(1, 6)) - 1];
            let reverse = scene.selection.contains(&el.id);
            for (i, line) in label_lines(text).iter().enumerate() {
                let row = r0 + i as i64;
                if row < 0 {
                    continue;
                }
                for (j, ch) in line.chars().enumerate() {
                    let col = c0 + j as i64;
                    if col < 0 {
                        continue;
                    }
                    let mut cell = FrameCell::new(Glyph::Char(ch), fg, Color::Default);
                    cell.reverse = reverse;
                    frame.put(row as usize, col as usize, cell);
                }
            }
        }
    }

    fn zooms(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pan_by_whole_pixels_is_a_shift_and_anything_else_is_not() {
        let at = Map {
            scale: 2.0,
            tx: 10.5,
            ty: 0.0,
        };
        let moved = Map { tx: 30.5, ..at };
        assert_eq!(shift_between(at, moved, 100, 100), Some((20, 0)));
        let fraction = Map { tx: 11.0, ..at };
        assert_eq!(shift_between(at, fraction, 100, 100), None);
        let zoomed = Map {
            scale: 2.5,
            ..moved
        };
        assert_eq!(shift_between(at, zoomed, 100, 100), None);
        let far = Map { tx: 110.5, ..at };
        assert_eq!(shift_between(at, far, 100, 100), None, "past the picture");
    }

    #[test]
    fn rects_meet_or_not() {
        let a = PixelRect::new(0, 0, 10, 10);
        assert!(intersects(a, PixelRect::new(9, 9, 5, 5)));
        assert!(!intersects(a, PixelRect::new(10, 0, 5, 5)), "edge to edge");
        assert!(!intersects(a, PixelRect::new(0, 0, 0, 5)), "empty");
        assert_eq!(
            union(Some(a), Some(PixelRect::new(20, 20, 1, 1))),
            Some(PixelRect::new(0, 0, 21, 21))
        );
        assert_eq!(union(None, Some(a)), Some(a));
    }
}
