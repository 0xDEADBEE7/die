//! A line of text set once, then measured and drawn: die-markdown's way of setting a label
//! (a run, the pen moved by each character's kerning then its advance) and of painting one (the
//! pen on a fraction of a pixel, the baseline on a whole one, the glyphs grown as a Mac's font
//! smoothing grows them), for any caller.

use crate::Rgb;
use crate::blit::{Clip, Pixels, smoothing};
use crate::fonts::{Inst, Pass, Style, Weight};

/// How a run is set. `size` is in logical px (CSS px, points): optical size and tracking are
/// looked up at it. The physical size is `size × scale`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TextLook {
    /// The face.
    pub style: Style,
    /// The size in logical px.
    pub size: f32,
    /// Digits as tabular figures.
    pub tabular: bool,
}

impl TextLook {
    /// The proportional face at a weight and a size.
    pub fn new(weight: Weight, size: f32) -> TextLook {
        TextLook {
            style: Style {
                weight,
                ..Style::default()
            },
            size,
            tabular: false,
        }
    }

    /// The same with tabular figures.
    pub fn tabular(self) -> TextLook {
        TextLook {
            tabular: true,
            ..self
        }
    }

    /// The same in the monospace.
    pub fn mono(self) -> TextLook {
        TextLook {
            style: Style {
                mono: true,
                ..self.style
            },
            ..self
        }
    }
}

/// A line's measures in physical px: its advance, ascent and descent (both positive), and its
/// cap height, all from the primary face.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Extent {
    /// How far the pen moves over the line.
    pub width: f32,
    /// How far the face rises above the baseline.
    pub ascent: f32,
    /// How far it drops below, positive.
    pub descent: f32,
    /// How tall its capitals and digits are.
    pub cap: f32,
}

impl Extent {
    /// Ascent and descent.
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }

    /// The baseline that centres the line in a box `height` tall from `top`, as die-markdown
    /// centres a line of text in its line height.
    pub fn baseline_in(&self, top: f32, height: f32) -> f32 {
        top + (height - self.height()) / 2.0 + self.ascent
    }
}

/// A glyph placed on a line: `x` from the line's pen origin, in physical px.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    /// Where it starts.
    pub x: f32,
    /// Its face.
    pub inst: Inst,
    /// Its glyph.
    pub id: u16,
}

/// A line set: its glyphs, its measures, its size in physical px and its subpixel bins.
#[derive(Clone, Debug, Default)]
pub struct Line {
    /// The glyphs, left to right.
    pub glyphs: Vec<Placed>,
    /// Its measures.
    pub extent: Extent,
    /// The size it was set at, in physical px.
    pub px: f32,
    /// Subpixel positions per pixel it's drawn at.
    pub bins: u8,
}

/// Subpixel positions per pixel at a scale: four under 1.5, else two.
pub fn bins(scale: f32) -> u8 {
    if scale < 1.5 { 4 } else { 2 }
}

impl Pass<'_> {
    /// The primary face's measures for a look at a scale, with no width.
    fn measures(&mut self, look: TextLook, scale: f32) -> Extent {
        let inst = self.primary(look.style, look.size);
        let m = self.metrics(inst);
        let unit = look.size * scale / m.upem;
        Extent {
            width: 0.0,
            ascent: m.ascent * unit,
            descent: m.descent * unit,
            cap: m.cap_height * unit,
        }
    }

    /// A line's measures without setting it: the same as [`Pass::set`]'s, with nothing kept.
    pub fn extent(&mut self, text: &str, look: TextLook, scale: f32) -> Extent {
        let mut run = self.run(look.style, look.size, scale).tabular(look.tabular);
        let mut pen = 0.0;
        for c in text.chars() {
            let s = self.next(&mut run, c);
            pen += s.kern;
            pen += s.advance;
        }
        Extent {
            width: pen,
            ..self.measures(look, scale)
        }
    }

    /// A line set in a look at a scale: each glyph where the pen is after its kerning.
    pub fn set(&mut self, text: &str, look: TextLook, scale: f32) -> Line {
        self.set_ends(text, look, scale, None)
    }

    /// [`Pass::set`], also saying in `ends` where the pen is after each character.
    fn set_ends(
        &mut self,
        text: &str,
        look: TextLook,
        scale: f32,
        mut ends: Option<&mut Vec<f32>>,
    ) -> Line {
        let mut run = self.run(look.style, look.size, scale).tabular(look.tabular);
        let mut glyphs = Vec::with_capacity(text.len());
        let mut pen = 0.0;
        for c in text.chars() {
            let s = self.next(&mut run, c);
            pen += s.kern;
            glyphs.push(Placed {
                x: pen,
                inst: s.inst,
                id: s.id,
            });
            pen += s.advance;
            if let Some(ends) = ends.as_deref_mut() {
                ends.push(pen);
            }
        }
        Line {
            glyphs,
            extent: Extent {
                width: pen,
                ..self.measures(look, scale)
            },
            px: look.size * scale,
            bins: bins(scale),
        }
    }

    /// A line no wider than `max` physical px: the whole of it if it fits, else the longest
    /// start of it that fits with '…' after, spaces before the '…' dropped; only the '…' when
    /// not even one character fits with it.
    pub fn set_within(&mut self, text: &str, look: TextLook, scale: f32, max: f32) -> Line {
        let mut ends = Vec::new();
        let mut line = self.set_ends(text, look, scale, Some(&mut ends));
        if line.extent.width <= max {
            return line;
        }
        let dots = self.set("…", look, scale);
        let room = max - dots.extent.width;
        let mut keep = ends.iter().take_while(|end| **end <= room).count();
        let mut chars = text.chars().take(keep).collect::<Vec<char>>();
        while chars.pop_if(|c| c.is_whitespace()).is_some() {
            keep -= 1;
        }
        let pen = keep.checked_sub(1).map_or(0.0, |last| ends[last]);
        line.glyphs.truncate(keep);
        let after = dots.glyphs.iter().map(|g| Placed { x: pen + g.x, ..*g });
        line.glyphs.extend(after);
        line.extent.width = pen + dots.extent.width;
        line
    }

    /// Draws a line with its pen at `x` and its baseline at `baseline` (physical px; the pen
    /// lands on the nearest of the line's bins, the baseline on a whole pixel), in `ink` at
    /// `alpha` of 255, grown by [`smoothing`] and clipped to `clip`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        line: &Line,
        to: &mut Pixels<'_>,
        x: f32,
        baseline: f32,
        ink: Rgb,
        alpha: u8,
        clip: Clip,
    ) {
        if alpha == 0 {
            return;
        }
        let bins = line.bins.max(1);
        let grow = smoothing(line.px, ink);
        let y = baseline.round() as i32;
        for g in &line.glyphs {
            let parts = ((x + g.x) * f32::from(bins)).round() as i32;
            let (gx, bin) = (
                parts.div_euclid(i32::from(bins)),
                parts.rem_euclid(i32::from(bins)) as u8,
            );
            if let Some(mask) = self.mask(g.inst, g.id, line.px, bin, bins, grow) {
                to.coverage(gx, y, mask, ink, alpha, clip);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Book;

    #[test]
    fn a_line_cut_to_fit_is_its_start_and_an_ellipsis_within_the_room() {
        // DIE's own fonts are there in a checkout, whatever the machine has.
        let Ok(mut book) = Book::load(None) else {
            eprintln!("skipped: no font on this machine");
            return;
        };
        let mut pass = book.pass();
        let look = TextLook::new(Weight::Regular, 12.0);
        let text = "Requests per second by region";
        let whole = pass.set(text, look, 2.0);
        assert_eq!(whole.glyphs.len(), text.chars().count());
        assert_eq!((whole.px, whole.bins), (24.0, 2));
        assert_eq!(pass.extent(text, look, 2.0), whole.extent);
        let e = whole.extent;
        assert!(e.width > 0.0 && e.cap > 0.0 && e.cap < e.ascent && e.descent > 0.0);
        assert_eq!(e.baseline_in(10.0, e.height() + 4.0), 12.0 + e.ascent);
        // Room enough: all of it.
        let fits = pass.set_within(text, look, 2.0, e.width);
        assert_eq!(fits.glyphs.len(), whole.glyphs.len());
        let dots = pass.set("…", look, 2.0);
        let dots_id = dots.glyphs[0].id;
        for max in [e.width - 0.5, e.width / 2.0, 3.0 * dots.extent.width] {
            let cut = pass.set_within(text, look, 2.0, max);
            assert!(cut.extent.width <= max, "{} in {max}", cut.extent.width);
            let (last, kept) = cut.glyphs.split_last().unwrap();
            assert_eq!(last.id, dots_id);
            assert!(!kept.is_empty() && kept.len() < whole.glyphs.len());
            for (a, b) in kept.iter().zip(&whole.glyphs) {
                assert_eq!((a.x, a.inst, a.id), (b.x, b.inst, b.id));
            }
            // The longest start that fits: one character more, past any spaces, wouldn't.
            let spaces = text
                .chars()
                .skip(kept.len())
                .take_while(|c| *c == ' ')
                .count();
            let more: String = text.chars().take(kept.len() + spaces + 1).collect();
            let wider = pass.extent(&more, look, 2.0).width + dots.extent.width;
            assert!(wider > max, "{more:?} and '…' fit {max}");
        }
        // A cut after a space drops the space.
        let start = pass.extent("Requests ", look, 2.0).width;
        let cut = pass.set_within(text, look, 2.0, start + dots.extent.width + 0.01);
        assert_eq!(cut.glyphs.len(), "Requests…".chars().count());
        let words = pass.extent("Requests", look, 2.0).width;
        assert_eq!(cut.extent.width, words + dots.extent.width);
        // Not one character fits beside it: only the ellipsis.
        let none = pass.set_within(text, look, 2.0, 1.0);
        assert_eq!(none.glyphs.len(), 1);
        assert_eq!(none.glyphs[0].id, dots_id);
    }

    #[test]
    fn a_line_draws_in_its_ink_at_its_alpha_only_inside_its_clip() {
        let Ok(mut book) = Book::load(None) else {
            eprintln!("skipped: no font on this machine");
            return;
        };
        let mut pass = book.pass();
        let look = TextLook::new(Weight::Semibold, 16.0).tabular();
        let line = pass.set("Hi 1,024", look, 2.0);
        let (w, h) = (200u32, 60u32);
        let baseline = line.extent.baseline_in(0.0, h as f32);
        let ink = Rgb(255, 255, 255);
        let mut draw = |alpha: u8, clip: Option<Clip>| {
            let mut bytes = vec![0u8; (w * h * 4) as usize];
            let mut to = Pixels::new(&mut bytes, w, h).unwrap();
            let clip = clip.unwrap_or(to.whole());
            pass.draw(&line, &mut to, 10.25, baseline, ink, alpha, clip);
            bytes
        };
        // Ink where the line is, between its ascent and descent, white and premultiplied.
        let full = draw(255, None);
        let mut inked = 0;
        for (i, px) in full.chunks_exact(4).enumerate() {
            let (x, y) = ((i as u32 % w) as f32, (i as u32 / w) as f32);
            if px[3] == 0 {
                continue;
            }
            inked += 1;
            assert!(px[..3].iter().all(|c| *c == px[3]), "{px:?}");
            assert!(x >= 9.0 && x <= 11.0 + line.extent.width, "column {x}");
            let (top, bottom) = (
                baseline - line.extent.ascent,
                baseline + line.extent.descent,
            );
            assert!(y >= top.floor() - 1.0 && y <= bottom.ceil(), "row {y}");
        }
        assert!(inked > 100);
        assert!(full.chunks_exact(4).any(|px| px == [255; 4]));
        // At half alpha the most any pixel takes is half; at none, nothing.
        let half = draw(128, None);
        assert_eq!(half.chunks_exact(4).map(|px| px[3]).max(), Some(128));
        assert!(draw(0, None).iter().all(|b| *b == 0));
        // Clipped to the left half, the right half is untouched and the left as before.
        let left = draw(255, Some(Clip::new(0, 0, 50, h as i32)));
        for (i, (a, b)) in left.chunks_exact(4).zip(full.chunks_exact(4)).enumerate() {
            let want = if i as u32 % w < 50 { b } else { &[0; 4] };
            assert_eq!(a, want, "pixel {i}");
        }
    }
}
