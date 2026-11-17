//! The paint: the part of a laid-out page a pixel buffer shows, drawn into its bytes. Only what
//! touches the rows asked for is looked at, through the layout's index of bands, so a page half
//! a million rows tall paints a screenful as fast as a short one.
//!
//! Everything goes down source-over on the stored bytes with coverage as it is: that's how DIE
//! draws its own text, so a page's letters weigh what the editor's beside it do. The buffer is
//! filled with the canvas first, so it stays opaque whatever is drawn.

use super::fonts::{self, Mask, Pass};
use super::layout::{Laid, Mark, Op};
use super::{Pictures, Rect, Rgb, sheet};
use std::ops::Range;
use tiny_skia::{FillRule, LineCap, LineJoin, PathBuilder, Stroke};

/// The rows of a buffer being painted, and where the page is behind them.
struct Target<'a> {
    out: &'a mut [u8],
    width: i32,
    stride: usize,
    /// The page's row at the buffer's row 0.
    top: i32,
    /// The buffer's rows that may be written.
    rows: Range<i32>,
}

/// One pixel: `ink` over what's there, `a` of 255 of it.
fn blend(px: &mut [u8], ink: Rgb, a: u32) {
    let rest = 255 - a;
    for (to, from) in px.iter_mut().zip([ink.0, ink.1, ink.2, 255]) {
        *to = ((u32::from(from) * a + u32::from(*to) * rest + 127) / 255) as u8;
    }
}

impl Target<'_> {
    /// The buffer's rows a span of the page's rows shows in.
    fn clip(&self, y0: i32, y1: i32) -> Range<i32> {
        let (a, b) = (y0.saturating_sub(self.top), y1.saturating_sub(self.top));
        a.max(self.rows.start)..b.min(self.rows.end)
    }

    /// A stretch of one of the buffer's rows.
    fn span(&mut self, row: i32, x0: i32, x1: i32) -> &mut [u8] {
        let (x0, x1) = (x0.clamp(0, self.width), x1.clamp(0, self.width));
        let at = row as usize * self.stride;
        &mut self.out[at + x0 as usize * 4..at + x1.max(x0) as usize * 4]
    }

    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, ink: Rgb) {
        for row in self.clip(y0, y1) {
            for px in self.span(row, x0, x1).chunks_exact_mut(4) {
                px.copy_from_slice(&[ink.0, ink.1, ink.2, 255]);
            }
        }
    }

    /// A rounded rectangle, filled, or with `line` above zero its outline that thick. Edges
    /// are on whole pixels, so only the corners are anything but full or empty.
    #[allow(clippy::too_many_arguments)]
    fn round(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, radius: f32, line: f32, ink: Rgb) {
        let (w, h) = ((x1 - x0) as f32, (y1 - y0) as f32);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let radius = radius.min(w / 2.0).min(h / 2.0).max(0.0);
        let (cx, cy) = (x0 as f32 + w / 2.0, y0 as f32 + h / 2.0);
        // How much of a pixel is inside the rectangle drawn `inset` in, with corners of `r`:
        // by its centre's distance from the edge.
        let cover = |x: i32, y: i32, inset: f32, r: f32| {
            let dx = (x as f32 + 0.5 - cx).abs() - (w / 2.0 - inset - r);
            let dy = (y as f32 + 0.5 - cy).abs() - (h / 2.0 - inset - r);
            let outside = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r;
            (0.5 - outside).clamp(0.0, 1.0)
        };
        let amount = |x: i32, y: i32| {
            let all = cover(x, y, 0.0, radius);
            if line > 0.0 {
                all - cover(x, y, line, (radius - line).max(0.0)).min(all)
            } else {
                all
            }
        };
        // The rows and columns within a corner's reach of an edge.
        let reach = radius.max(line).ceil() as i32 + 1;
        let top = self.top;
        for row in self.clip(y0, y1) {
            let y = row + top;
            let corner = y < y0 + reach || y >= y1 - reach;
            let soft = |target: &mut Self, from: i32, to: i32| {
                let (from, to) = (from.clamp(0, target.width), to.clamp(0, target.width));
                for (x, px) in (from..to).zip(target.span(row, from, to).chunks_exact_mut(4)) {
                    let a = (amount(x, y) * 255.0 + 0.5) as u32;
                    if a > 0 {
                        blend(px, ink, a);
                    }
                }
            };
            if corner && (line > 0.0 || x1 - x0 <= 2 * reach) {
                soft(self, x0, x1);
                continue;
            }
            soft(self, x0, x0 + reach);
            soft(self, (x1 - reach).max(x0 + reach), x1);
            if line <= 0.0 {
                for px in self.span(row, x0 + reach, x1 - reach).chunks_exact_mut(4) {
                    px.copy_from_slice(&[ink.0, ink.1, ink.2, 255]);
                }
            }
        }
    }

    /// Coverage in a colour, its origin at a pixel of the page.
    fn mask(&mut self, x: i32, y: i32, mask: &Mask, ink: Rgb) {
        let (x0, y0) = (x + mask.left, y + mask.top);
        let (w, h) = (mask.width as i32, mask.height as i32);
        let (c0, c1) = ((-x0).max(0), (self.width - x0).min(w));
        if c0 >= c1 {
            return;
        }
        for row in self.clip(y0, y0 + h) {
            let line = (row + self.top - y0) * w;
            let cov = &mask.cov[(line + c0) as usize..(line + c1) as usize];
            let out = self.span(row, x0 + c0, x0 + c1);
            for (px, a) in out.chunks_exact_mut(4).zip(cov) {
                match *a {
                    0 => {}
                    255 => px.copy_from_slice(&[ink.0, ink.1, ink.2, 255]),
                    a => blend(px, ink, u32::from(a)),
                }
            }
        }
    }

    /// Premultiplied pixels, `w` a row, over what's there.
    fn picture(&mut self, x0: i32, y0: i32, w: i32, h: i32, pixels: &[u8]) {
        let (c0, c1) = ((-x0).max(0), (self.width - x0).min(w));
        if c0 >= c1 || pixels.len() < (w * h) as usize * 4 {
            return;
        }
        for row in self.clip(y0, y0 + h) {
            let line = ((row + self.top - y0) * w) as usize * 4;
            let source = &pixels[line + c0 as usize * 4..line + c1 as usize * 4];
            let out = self.span(row, x0 + c0, x0 + c1);
            for (px, from) in out.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
                let rest = 255 - u32::from(from[3]);
                for (to, value) in px.iter_mut().zip(from) {
                    *to = (u32::from(*value) + (u32::from(*to) * rest + 127) / 255).min(255) as u8;
                }
                px[3] = 255;
            }
        }
    }
}

/// Paints the rows `rows` of a buffer whose row 0 shows the page's row `top`.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    laid: &Laid,
    mut pass: Pass<'_>,
    pictures: &Pictures,
    out: &mut [u8],
    width: u32,
    height: u32,
    top: i32,
    rows: Range<u32>,
    focus: Option<&[Rect]>,
) {
    let stride = width as usize * 4;
    if stride == 0 {
        return;
    }
    // Never past what the buffer holds, whatever it's said to.
    let held = (out.len() / stride)
        .min(height as usize)
        .min(i32::MAX as usize / 2) as u32;
    let rows = rows.start.min(held) as i32..rows.end.min(held) as i32;
    if rows.is_empty() {
        return;
    }
    let mut target = Target {
        out,
        width: width.min(i32::MAX as u32 / 8) as i32,
        stride,
        top,
        rows: rows.clone(),
    };
    // The canvas: one row made, the rest copied from it.
    let first = rows.start as usize * stride;
    let ink = laid.canvas;
    for px in target.out[first..first + stride].chunks_exact_mut(4) {
        px.copy_from_slice(&[ink.0, ink.1, ink.2, 255]);
    }
    for row in rows.start as usize + 1..rows.end as usize {
        target.out.copy_within(first..first + stride, row * stride);
    }
    // The page's rows asked for, and the ops in the bands they're in.
    let (from, to) = (top.saturating_add(rows.start), top.saturating_add(rows.end));
    if to <= 0 || laid.bands.is_empty() {
        return;
    }
    let band = |y: i32| (y.max(0) / super::layout::BAND) as usize;
    let last = band(to - 1).min(laid.bands.len() - 1);
    let mut ops: Vec<u32> = Vec::new();
    for band in &laid.bands[band(from).min(last)..=last] {
        ops.extend(band);
    }
    // An op in two bands is there twice; in order of their numbers they're in drawing order.
    ops.sort_unstable();
    ops.dedup();
    let bins = laid.bins.max(1);
    let place = |x: f32| {
        let parts = (x * f32::from(bins)).round() as i32;
        (
            parts.div_euclid(i32::from(bins)),
            parts.rem_euclid(i32::from(bins)) as u8,
        )
    };
    let mut gaps: Vec<bool> = Vec::new();
    for op in ops.iter().filter_map(|i| laid.ops.get(*i as usize)) {
        let (y0, y1) = op.rows();
        if y1 <= from || y0 >= to {
            continue;
        }
        match *op {
            Op::Fill {
                x0,
                y0,
                x1,
                y1,
                ink,
            } => target.fill(x0, y0, x1, y1, ink),
            Op::Round {
                x0,
                y0,
                x1,
                y1,
                radius,
                line,
                ink,
            } => target.round(x0, y0, x1, y1, radius, line, ink),
            Op::Text {
                from,
                to,
                baseline,
                px,
                ink,
            } => {
                let glyphs = laid.glyphs.get(from as usize..to as usize).unwrap_or(&[]);
                for glyph in glyphs {
                    let (x, bin) = place(glyph.x);
                    if let Some(mask) = pass.mask(glyph.inst, glyph.id, px, bin, bins) {
                        target.mask(x, baseline, mask, ink);
                    }
                }
            }
            Op::Under {
                x0,
                y0,
                x1,
                y1,
                ink,
                from,
                to,
                baseline,
                px,
            } => {
                // The line steps round what hangs through it, as a browser's does: where a
                // glyph has ink in the line's rows, the line stops a little short either side.
                gaps.clear();
                gaps.resize((x1 - x0).max(0) as usize, false);
                let clear = (laid.scale * 0.75).round().max(1.0) as i32;
                let glyphs = laid.glyphs.get(from as usize..to as usize).unwrap_or(&[]);
                for glyph in glyphs {
                    let (x, bin) = place(glyph.x);
                    let Some(mask) = pass.mask(glyph.inst, glyph.id, px, bin, bins) else {
                        continue;
                    };
                    let (w, h) = (mask.width as i32, mask.height as i32);
                    let (r0, r1) = (y0 - baseline - mask.top, y1 - baseline - mask.top);
                    for column in 0..w {
                        let inked = (r0.max(0)..r1.min(h))
                            .any(|r| mask.cov[(r * w + column) as usize] > 48);
                        if inked {
                            let at = x + mask.left + column - x0;
                            for gap in (at - clear).max(0)..(at + clear + 1).min(x1 - x0) {
                                gaps[gap as usize] = true;
                            }
                        }
                    }
                }
                let mut at = 0;
                while at < gaps.len() {
                    let run = gaps[at..]
                        .iter()
                        .take_while(|gap| **gap == gaps[at])
                        .count();
                    if !gaps[at] {
                        target.fill(x0 + at as i32, y0, x0 + (at + run) as i32, y1, ink);
                    }
                    at += run;
                }
            }
            Op::Picture {
                x0,
                y0,
                x1,
                y1,
                file,
            } => {
                let Some(path) = laid.files.get(file as usize) else {
                    continue;
                };
                let (w, h) = ((x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32);
                if let Some(pixels) = pictures.at_size(path, w, h) {
                    target.picture(x0, y0, w as i32, h as i32, &pixels);
                }
            }
            Op::Mark {
                kind,
                x,
                y,
                px,
                ink,
            } => {
                if let Some(mask) = pass.shape(kind as u16, px, || mark(kind, px)) {
                    target.mask(x, y, mask, ink);
                }
            }
        }
    }
    // The ring round the link the keyboard is on.
    let s = laid.scale;
    let out = (sheet::RING_OUT * s).round();
    for rect in focus.unwrap_or(&[]) {
        let (x0, y0) = ((rect.x - out).floor() as i32, (rect.y - out).floor() as i32);
        let (x1, y1) = (
            (rect.x + rect.w + out).ceil() as i32,
            (rect.y + rect.h + out).ceil() as i32,
        );
        let line = (sheet::RING * s).round().max(1.0);
        target.round(x0, y0, x1, y1, sheet::RING_RADIUS * s, line, laid.ring);
    }
}

/// A drawn mark's coverage in a box `px` pixels a side: stand-ins for GitHub's icons, a few
/// strokes each.
fn mark(kind: Mark, px: f32) -> Option<Mask> {
    let mut lines = PathBuilder::new();
    let mut solid = PathBuilder::new();
    let (mut side, mut width) = (16.0, 1.5);
    let mut path = |points: &[(f32, f32)], closed: bool| {
        for (i, (x, y)) in points.iter().enumerate() {
            match i {
                0 => lines.move_to(*x, *y),
                _ => lines.line_to(*x, *y),
            }
        }
        if closed {
            lines.close();
        }
    };
    // An exclamation mark or an `i`: a bar and a dot.
    let mut dot = |x: f32, y: f32, r: f32| solid.push_circle(x, y, r);
    match kind {
        Mark::Check => {
            (side, width) = (13.0, 1.75);
            path(&[(3.0, 6.75), (5.5, 9.25), (10.0, 4.0)], false);
        }
        Mark::Note => {
            path(&[(8.0, 7.5), (8.0, 11.5)], false);
            dot(8.0, 5.0, 1.0);
        }
        Mark::Tip => {
            path(&[(6.0, 12.5), (10.0, 12.5)], false);
            path(&[(6.5, 14.5), (9.5, 14.5)], false);
        }
        Mark::Important => {
            path(&[(8.0, 3.75), (8.0, 6.75)], false);
            dot(8.0, 8.75, 1.0);
        }
        Mark::Warning => {
            path(&[(8.0, 1.5), (15.0, 14.0), (1.0, 14.0)], true);
            path(&[(8.0, 6.0), (8.0, 9.5)], false);
            dot(8.0, 11.75, 1.0);
        }
        Mark::Caution => {
            let corners = [
                (5.0, 1.0),
                (11.0, 1.0),
                (15.0, 5.0),
                (15.0, 11.0),
                (11.0, 15.0),
                (5.0, 15.0),
                (1.0, 11.0),
                (1.0, 5.0),
            ];
            path(&corners, true);
            path(&[(8.0, 4.5), (8.0, 8.5)], false);
            dot(8.0, 11.0, 1.0);
        }
        Mark::Picture => {
            width = 1.25;
            path(&[(1.5, 2.5), (14.5, 2.5), (14.5, 13.5), (1.5, 13.5)], true);
            path(
                &[
                    (2.0, 12.0),
                    (6.0, 8.0),
                    (9.0, 11.0),
                    (11.5, 8.5),
                    (14.0, 11.0),
                ],
                false,
            );
            dot(5.0, 6.0, 1.25);
        }
        Mark::Wrap => {
            // An arrow that turns back to the right: this row goes on from the one above.
            width = 1.25;
            path(&[(10.0, 7.5), (12.5, 10.0), (10.0, 12.5)], false);
        }
        Mark::Open => {}
    }
    match kind {
        Mark::Note => lines.push_circle(8.0, 8.0, 7.0),
        Mark::Tip => lines.push_circle(8.0, 6.0, 4.5),
        Mark::Important => {
            // A speech bubble: a rounded box with a tail.
            lines.move_to(3.0, 1.5);
            lines.line_to(13.0, 1.5);
            lines.quad_to(14.5, 1.5, 14.5, 3.0);
            lines.line_to(14.5, 9.5);
            lines.quad_to(14.5, 11.0, 13.0, 11.0);
            lines.line_to(7.5, 11.0);
            lines.line_to(4.0, 14.5);
            lines.line_to(4.0, 11.0);
            lines.line_to(3.0, 11.0);
            lines.quad_to(1.5, 11.0, 1.5, 9.5);
            lines.line_to(1.5, 3.0);
            lines.quad_to(1.5, 1.5, 3.0, 1.5);
            lines.close();
        }
        Mark::Wrap => {
            lines.move_to(4.5, 3.5);
            lines.line_to(4.5, 7.5);
            lines.quad_to(4.5, 10.0, 7.0, 10.0);
            lines.line_to(12.5, 10.0);
        }
        Mark::Open => {
            // The triangle of an open `<details>`.
            solid.move_to(3.5, 5.75);
            solid.line_to(12.0, 5.75);
            solid.line_to(7.75, 11.25);
            solid.close();
        }
        _ => {}
    }
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    let scale = px / side;
    if let Some(stroked) = lines
        .finish()
        .and_then(|l| l.stroke(&stroke, scale.max(1.0) * 2.0))
    {
        solid.push_path(&stroked);
    }
    fonts::fill(&solid.finish()?, scale, 0.0, FillRule::Winding)
}
