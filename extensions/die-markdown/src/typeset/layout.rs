//! The layout: a document set on a page of one width, at one scale, in one palette. What comes
//! out is everything a paint needs and nothing it has to work out: rectangles, glyphs with their
//! places, pictures and drawn marks, in the order they go down, with the links' rectangles, the
//! headings, and which source bytes are at which height.
//!
//! Lengths are physical pixels from here on: the sheet's CSS pixels times the scale. Block
//! positions are kept as fractions and snapped where something is drawn, so rounding never
//! adds up down a page; only a line's height is made whole, so rows repeat evenly.
//!
//! Vertical margins collapse as in a browser, by one rule ([`Flow`]): a gap is the larger of
//! the margins that meet, and a container with no padding (a list, an item, a quote) lets its
//! first and last child's margins through.

use super::doc::{self, Alert, Align, Block, Doc, Fit, Inline, Item, Length, Note, Para, Table};
use super::fonts::{Inst, Pass, Style as Face, Weight};
use super::sheet;
use super::syntax::Highlighter;
use super::{Env, HeadingPos, LinkBox, Palette, Pictures, Rect, Rgb};
use std::path::PathBuf;

/// The page's rows a band of the index covers.
pub(super) const BAND: i32 = 256;

/// What a width may be passed by before something no longer fits: sums of fractions aren't
/// exact.
const SLACK: f32 = 0.01;

/// A placed glyph: where its pen is from the area's left edge, and which glyph of which face.
#[derive(Clone, Copy, Debug)]
pub(super) struct Glyph {
    pub x: f32,
    pub inst: Inst,
    pub id: u16,
}

/// A mark that's drawn, not set: each in a square box.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mark {
    Check,
    Note,
    Tip,
    Important,
    Warning,
    Caution,
    Picture,
    Open,
    Closed,
    Wrap,
}

/// One thing to draw. Edges are on whole pixels.
#[derive(Clone, Copy, Debug)]
pub(super) enum Op {
    /// A filled rectangle.
    Fill {
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        ink: Rgb,
    },
    /// A rounded rectangle: filled, or with `line` above zero an outline that thick.
    Round {
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        radius: f32,
        line: f32,
        ink: Rgb,
    },
    /// Glyphs on one baseline, at `px` pixels an em.
    Text {
        from: u32,
        to: u32,
        baseline: i32,
        px: f32,
        ink: Rgb,
    },
    /// A line under the glyphs `from..to` of a run on `baseline` at `px`, which it steps
    /// round where they hang through it.
    Under {
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        ink: Rgb,
        from: u32,
        to: u32,
        baseline: i32,
        px: f32,
    },
    /// A picture, by its place among the page's picture files.
    Picture {
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        file: u32,
    },
    /// A drawn mark in a box `px` pixels a side.
    Mark {
        kind: Mark,
        x: i32,
        y: i32,
        px: f32,
        ink: Rgb,
    },
}

impl Op {
    /// The rows it may touch.
    pub(super) fn rows(&self) -> (i32, i32) {
        match *self {
            Op::Fill { y0, y1, .. }
            | Op::Round { y0, y1, .. }
            | Op::Under { y0, y1, .. }
            | Op::Picture { y0, y1, .. } => (y0, y1),
            Op::Text { baseline, px, .. } => {
                let px = px.ceil() as i32;
                (baseline - px - px / 2, baseline + px / 2 + 2)
            }
            Op::Mark { y, px, .. } => (y - 1, y + px.ceil() as i32 + 1),
        }
    }
}

/// Source bytes and the rows that show them.
#[derive(Clone, Copy, Debug)]
pub(super) struct Span {
    pub src: usize,
    pub end: usize,
    pub y0: f32,
    pub y1: f32,
}

/// A laid-out page.
#[derive(Debug)]
pub(super) struct Laid {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    /// The parts of a pixel a glyph may start at.
    pub bins: u8,
    pub canvas: Rgb,
    pub ring: Rgb,
    pub ops: Vec<Op>,
    pub glyphs: Vec<Glyph>,
    /// The picture files the page shows.
    pub files: Vec<PathBuf>,
    /// For each [`BAND`] rows of the page, the ops that touch them, in drawing order.
    pub bands: Vec<Vec<u32>>,
    pub links: Vec<LinkBox>,
    pub headings: Vec<HeadingPos>,
    pub spans: Vec<Span>,
    pub wants: Vec<PathBuf>,
    /// Places with names that aren't headings: footnotes and their references.
    pub anchors: Vec<(String, f32)>,
}

/// Where the next block goes, and the margin waiting above it.
#[derive(Clone, Copy)]
struct Flow {
    y: f32,
    /// The largest of the margins that have met since the last box.
    pending: f32,
    /// Nothing placed yet in a page or a padded box: its first box has no margin above.
    fresh: bool,
    /// The next box's own margin above is nothing: a quote's first child.
    flat: bool,
}

impl Flow {
    /// A box starts: where its top is.
    fn open(&mut self, above: f32) -> f32 {
        let above = if self.flat { 0.0 } else { above };
        if !self.fresh {
            self.y += self.pending.max(above);
        }
        (self.pending, self.fresh, self.flat) = (0.0, false, false);
        self.y
    }

    /// A box that ended at `y` leaves a margin below.
    fn close(&mut self, y: f32, below: f32) {
        (self.y, self.pending) = (y, below);
    }

    /// A margin of a container that lets margins through meets the others.
    fn join(&mut self, margin: f32) {
        self.pending = self.pending.max(margin);
    }
}

/// The type a block's text is set in.
#[derive(Clone, Copy)]
struct Ctx {
    /// CSS pixels.
    size: f32,
    /// The line's height in CSS pixels.
    line: f32,
    weight: Weight,
    ink: Rgb,
    heading: bool,
    /// How many lists this is inside.
    lists: usize,
}

/// A line's measures.
#[derive(Clone, Copy)]
struct Base {
    height: f32,
    /// From the line's top to its baseline.
    above: f32,
    x_height: f32,
}

/// The box a run of text sits in.
#[derive(Clone, Copy, PartialEq)]
enum Boxed {
    No,
    /// Inline code.
    Pill,
    /// A key cap.
    Key,
}

/// How a run of text looks, everything resolved.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    face: Face,
    /// CSS pixels.
    size: f32,
    ink: Rgb,
    underline: bool,
    strike: bool,
    /// How far above the line's baseline its own is.
    rise: f32,
    boxed: Boxed,
    span: u32,
    /// A box's padding either side, and how far it reaches above and below the line's baseline.
    pad: f32,
    up: f32,
    down: f32,
    link: Option<u32>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Word,
    Space,
    Break,
    Inset(u32),
}

/// Something a line is made of.
#[derive(Clone, Copy)]
struct Piece {
    kind: Kind,
    look: u32,
    /// Its glyphs, each placed from the piece's start.
    from: u32,
    to: u32,
    width: f32,
    /// The kerning before it, which a line's start drops.
    lead: f32,
    src: usize,
    /// No line may end between the piece before and this.
    joined: bool,
}

/// What isn't text in a line.
enum Inset {
    Picture {
        file: u32,
        /// Its size on the page before the line's width limits it.
        w: f32,
        h: f32,
        /// A width the source gave as a share of the line.
        share: Option<f32>,
        link: u32,
    },
    /// A picture that isn't there: its words in a box, behind a mark if it never will be.
    Absent {
        from: u32,
        to: u32,
        /// The whole box's width.
        width: f32,
        ink: Rgb,
        mark: bool,
        size: f32,
        link: u32,
    },
}

/// A paragraph's text, shaped and cut into pieces, ready for any width.
#[derive(Default)]
struct Prepared {
    pieces: Vec<Piece>,
    glyphs: Vec<Glyph>,
    looks: Vec<Look>,
    insets: Vec<Inset>,
}

/// A piece, or part of one, placed on a line.
#[derive(Clone, Copy)]
struct Frag {
    piece: u32,
    from: u32,
    to: u32,
    x: f32,
    width: f32,
    /// A picture's height.
    height: f32,
}

struct Line {
    from: usize,
    to: usize,
    width: f32,
    above: f32,
    height: f32,
    top: f32,
}

#[derive(Default)]
struct Broken {
    frags: Vec<Frag>,
    lines: Vec<Line>,
    width: f32,
    height: f32,
}

/// What hangs in a list item's indent, waiting for the item's first line.
struct Marker {
    kind: MarkerKind,
    /// The item's text edge.
    x: f32,
    ink: Rgb,
    size: f32,
    weight: Weight,
    /// A name the line it lands on goes by.
    anchor: Option<String>,
}

enum MarkerKind {
    Disc,
    Ring,
    Square,
    Number(String),
    Task(bool),
}

/// A picture's state.
enum Pic {
    Here(u32, f32, f32),
    Coming,
    Never,
}

struct Lay<'a, 'f> {
    pass: Pass<'f>,
    env: &'a Env<'a>,
    pal: &'a Palette,
    pictures: &'a Pictures,
    doc: &'a Doc,
    /// Physical pixels to a CSS pixel.
    s: f32,
    hair: f32,
    /// The furthest right anything may reach: the area's edge less the page's padding, or the
    /// column's edge where the area is narrower than that.
    edge: f32,
    out: Laid,
    flow: Flow,
    markers: Vec<Marker>,
    /// Each of the document's links' rectangles, and the links in the order they first showed.
    rects: Vec<Vec<Rect>>,
    order: Vec<u32>,
}

/// Lays a document out.
pub(super) fn lay(doc: &Doc, env: &Env<'_>, pass: Pass<'_>, pictures: &Pictures) -> Laid {
    let s = if env.scale.is_finite() {
        env.scale.clamp(0.5, 8.0)
    } else {
        1.0
    };
    let width = env.width.min(1 << 16) as f32;
    let pal = env.palette;
    let mut lay = Lay {
        pass,
        env,
        pal,
        pictures,
        doc,
        s,
        hair: sheet::hairline(s),
        edge: width,
        out: Laid {
            width,
            height: 0.0,
            scale: s,
            // Quarter pixels where a pixel is a CSS pixel; halves where it's half of one.
            bins: if s < 1.5 { 4 } else { 2 },
            canvas: pal.canvas,
            ring: pal.link,
            ops: Vec::new(),
            glyphs: Vec::new(),
            files: Vec::new(),
            bands: Vec::new(),
            links: Vec::new(),
            headings: Vec::new(),
            spans: Vec::new(),
            wants: Vec::new(),
            anchors: Vec::new(),
        },
        flow: Flow {
            y: 0.0,
            pending: 0.0,
            fresh: true,
            flat: false,
        },
        markers: Vec::new(),
        rects: vec![Vec::new(); doc.links.len()],
        order: Vec::new(),
    };
    // The column: centred, at most the sheet's width, never laid out narrower than its least,
    // however narrow the area (what's past the area's edge isn't painted).
    let column = width.clamp(sheet::COLUMN_MIN * s, sheet::COLUMN * s);
    let narrow = width < sheet::NARROW * s;
    let pad = if narrow {
        sheet::PAD_NARROW
    } else {
        sheet::PAD
    };
    let pad = (pad * s).round();
    let left = ((width - column) / 2.0).max(0.0).round();
    let (x, w) = (left + pad, (column - 2.0 * pad).max(s));
    lay.edge = (width - pad).max(x + w);
    lay.flow.y = pad;
    let ctx = Ctx {
        size: sheet::TEXT,
        line: sheet::TEXT * sheet::TEXT_LINE,
        weight: Weight::Regular,
        ink: pal.text,
        heading: false,
        lists: 0,
    };
    lay.blocks(&doc.blocks, x, w, &ctx);
    lay.out.height = (lay.flow.y + pad).ceil();
    lay.finish()
}

impl Lay<'_, '_> {
    // ------------------------------------------------------------ small things

    fn px(&self, css: f32) -> f32 {
        css * self.s
    }

    fn fill(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, ink: Rgb) {
        let (x0, y0) = (x0.round() as i32, y0.round() as i32);
        let (x1, y1) = (x1.round() as i32, y1.round() as i32);
        if x1 > x0 && y1 > y0 {
            self.out.ops.push(Op::Fill {
                x0,
                y0,
                x1,
                y1,
                ink,
            });
        }
    }

    /// A line as thick as the display's thinnest, from `y` down.
    fn hairline(&mut self, x0: f32, x1: f32, y: f32, ink: Rgb) {
        let y = y.round();
        self.fill(x0, y, x1, y + self.hair, ink);
    }

    #[allow(clippy::too_many_arguments)]
    fn round(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, line: f32, ink: Rgb) {
        let (x0, y0) = (x0.round() as i32, y0.round() as i32);
        let (x1, y1) = (x1.round() as i32, y1.round() as i32);
        if x1 > x0 && y1 > y0 {
            let op = Op::Round {
                x0,
                y0,
                x1,
                y1,
                radius,
                line,
                ink,
            };
            self.out.ops.push(op);
        }
    }

    /// A colour a palette gives an element, unless it gives it the text's: then the element has
    /// none of its own, and takes what's round it, as it would from GitHub's sheet.
    fn own(&self, role: Rgb, around: Rgb) -> Rgb {
        if role == self.pal.text { around } else { role }
    }

    fn span(&mut self, src: usize, end: usize, y0: f32, y1: f32) {
        let end = end.max(src);
        self.out.spans.push(Span { src, end, y0, y1 });
    }

    fn base(&mut self, ctx: &Ctx) -> Base {
        let face = Face {
            weight: ctx.weight,
            ..Face::default()
        };
        let inst = self.pass.primary(face, ctx.size);
        let m = self.pass.metrics(inst);
        let unit = self.px(ctx.size) / m.upem;
        let (ascent, descent) = (m.ascent * unit, m.descent * unit);
        let height = self.px(ctx.line).round().max(1.0);
        Base {
            height,
            above: (height - (ascent + descent)) / 2.0 + ascent,
            x_height: m.x_height * unit,
        }
    }

    /// Sets a string on a baseline, from `x` on or ending at it, and says how wide it is.
    #[allow(clippy::too_many_arguments)]
    fn label(
        &mut self,
        text: &str,
        face: Face,
        size: f32,
        ink: Rgb,
        x: f32,
        baseline: f32,
        ends: bool,
    ) -> f32 {
        let mut run = self.pass.run(face, size, self.s);
        let from = self.out.glyphs.len();
        let mut pen = 0.0;
        for c in text.chars() {
            let shaped = self.pass.next(&mut run, c);
            pen += shaped.kern;
            self.out.glyphs.push(Glyph {
                x: pen,
                inst: shaped.inst,
                id: shaped.id,
            });
            pen += shaped.advance;
        }
        let left = if ends { x - pen } else { x };
        for glyph in &mut self.out.glyphs[from..] {
            glyph.x += left;
        }
        self.out.ops.push(Op::Text {
            from: from as u32,
            to: self.out.glyphs.len() as u32,
            baseline: baseline.round() as i32,
            px: self.px(size),
            ink,
        });
        pen
    }

    // ------------------------------------------------------------ blocks

    fn blocks(&mut self, blocks: &[Block], x: f32, w: f32, ctx: &Ctx) {
        for block in blocks {
            match block {
                Block::Para(para) => self.para(para, x, w, ctx),
                Block::Heading { level, slug, para } => self.heading(*level, slug, para, x, w, ctx),
                Block::Code {
                    text,
                    lines,
                    lang,
                    src,
                } => self.code(text, lines, src.end, lang, x, w, ctx),
                Block::Quote {
                    alert: None,
                    blocks,
                } => self.quote(blocks, x, w, ctx),
                Block::Quote {
                    alert: Some(kind),
                    blocks,
                } => self.alert(*kind, blocks, x, w, ctx),
                Block::List { start, items } => self.list(*start, items, x, w, ctx),
                Block::Table(table) => self.table(table, x, w, ctx),
                Block::Rule { src } => {
                    let y = self.flow.open(self.px(sheet::RULE_GAP));
                    let bottom = y.round() + self.px(sheet::RULE).round();
                    self.fill(x, y, x + w, bottom, self.pal.border);
                    self.span(src.start, src.end, y, bottom);
                    self.flow
                        .close(y + self.px(sheet::RULE), self.px(sheet::RULE_GAP));
                }
                Block::Details {
                    summary,
                    blocks,
                    open,
                    at,
                    link,
                } => self.details(summary, blocks, (*open, *at, *link), x, w, ctx),
                Block::Notes(notes) => self.notes(notes, x, w, ctx),
            }
        }
    }

    fn para(&mut self, para: &Para, x: f32, w: f32, ctx: &Ctx) {
        let gap = self.px(sheet::GAP);
        let (above, below) = match para.fit {
            Fit::Block => (0.0, gap),
            Fit::Loose => (gap, gap),
            Fit::Tight => (0.0, 0.0),
        };
        let base = self.base(ctx);
        let prepared = self.prepare(&para.inlines, ctx, &base);
        let broken = break_lines(&prepared, w, &base, true);
        let y = self.flow.open(above);
        self.emit(&prepared, &broken, x, y, w, para.align, para.src.end);
        self.flow.close(y + broken.height, below);
    }

    fn heading(&mut self, level: u8, slug: &str, para: &Para, x: f32, w: f32, ctx: &Ctx) {
        let at = usize::from(level.clamp(1, 6)) - 1;
        let size = sheet::HEADING[at];
        let own = Ctx {
            size,
            line: size * sheet::HEADING_LINE,
            weight: Weight::Semibold,
            ink: self.own(self.pal.headings[at], ctx.ink),
            heading: true,
            lists: ctx.lists,
        };
        let base = self.base(&own);
        let prepared = self.prepare(&para.inlines, &own, &base);
        let broken = break_lines(&prepared, w, &base, true);
        let y = self.flow.open(self.px(sheet::HEADING_ABOVE));
        self.emit(&prepared, &broken, x, y, w, para.align, para.src.end);
        // A heading with no text still takes its line.
        let mut bottom = y + broken.height.max(base.height);
        if level <= 2 {
            bottom += self.px(size * sheet::HEADING_RULE_PAD);
            self.hairline(x, x + w, bottom, self.pal.hairline);
            bottom += self.hair;
        }
        self.out.headings.push(HeadingPos {
            y: y.round(),
            level: level.clamp(1, 6),
            slug: slug.to_owned(),
            source: para.src.clone(),
        });
        self.flow.close(bottom, self.px(sheet::GAP));
    }

    /// A code block: its lines in the code's face, each glyph in its piece's colour where the
    /// fence names a language the highlighter knows.
    #[allow(clippy::too_many_arguments)]
    fn code(
        &mut self,
        text: &str,
        starts: &[usize],
        end: usize,
        lang: &str,
        x: f32,
        w: f32,
        ctx: &Ctx,
    ) {
        let y = self.flow.open(0.0);
        let top = y.round();
        let pad = self.px(sheet::CODE_PAD).round();
        let size = ctx.size * sheet::CODE;
        let line = self.px(size * sheet::CODE_LINE).round().max(1.0);
        let face = Face {
            mono: true,
            ..Face::default()
        };
        let inst = self.pass.primary(face, size);
        let m = self.pass.metrics(inst);
        let unit = self.px(size) / m.upem;
        let above = (line - (m.ascent + m.descent) * unit) / 2.0 + m.ascent * unit;
        let ink = self.own(self.pal.code_block, ctx.ink);
        let room = (w - 2.0 * pad).max(self.px(size));
        let panel = self.out.ops.len();
        self.round(
            x,
            top,
            x + w,
            top + 1.0,
            self.px(sheet::RADIUS),
            0.0,
            self.pal.panel,
        );
        let has_panel = self.out.ops.len() > panel;
        let left = x.round() + pad;
        let mut row = top + pad;
        let tab = self.env.tab_width.clamp(1, 64);
        let body = text.strip_suffix('\n').unwrap_or(text);
        let lines = if text.is_empty() { None } else { Some(body) };
        let mut lit = Highlighter::new(lang);
        let mut runs = Vec::new();
        for (n, source) in lines.iter().flat_map(|b| b.split('\n')).enumerate() {
            let first = row;
            let mut run = self.pass.run(face, size, self.s);
            let (mut from, mut pen, mut column) = (self.out.glyphs.len(), 0.0f32, 0usize);
            let flush = |lay: &mut Self, from: usize, row: f32, ink: Rgb| {
                if lay.out.glyphs.len() > from {
                    lay.out.ops.push(Op::Text {
                        from: from as u32,
                        to: lay.out.glyphs.len() as u32,
                        baseline: (row + above).round() as i32,
                        px: lay.px(size),
                        ink,
                    });
                }
            };
            // The line's pieces, and the one the glyph at hand is in. A space takes whatever
            // colour is going, so a run of glyphs is only cut where the ink changes.
            match &mut lit {
                Some(lit) => lit.line(source, &mut runs),
                None => runs.clear(),
            }
            // Where the glyphs of the colour at hand start, and of the row: the row is wrapped
            // only once it has one.
            let (mut piece, mut now, mut row_from) = (0usize, ink, from);
            for (at, c) in source.char_indices() {
                if !runs.is_empty() && !c.is_whitespace() {
                    while runs.get(piece + 1).is_some_and(|r| r.0 <= at) {
                        piece += 1;
                    }
                    let want = runs[piece].1.ink(&self.pal.syntax, ink);
                    if want != now {
                        flush(self, from, row, now);
                        (from, now) = (self.out.glyphs.len(), want);
                    }
                }
                if c == '\t' {
                    // A tab reaches the next stop, counted in columns of the code's own face.
                    let stop = tab - column % tab;
                    pen += self.pass.next(&mut run, ' ').advance * stop as f32;
                    column += stop;
                    continue;
                }
                if c.is_control() || unseen(c) {
                    continue;
                }
                let shaped = self.pass.next(&mut run, c);
                if pen + shaped.kern + shaped.advance > room + SLACK
                    && self.out.glyphs.len() > row_from
                {
                    // The line is longer than the panel: it goes on below, behind a quiet mark
                    // in the padding, so nobody takes the row for a line of its own.
                    flush(self, from, row, now);
                    row += line;
                    pen = 0.0;
                    let side = self.px(size).round();
                    self.out.ops.push(Op::Mark {
                        kind: Mark::Wrap,
                        x: (left - (pad + side) / 2.0).round() as i32,
                        y: (row + (line - side) / 2.0).round() as i32,
                        px: side,
                        ink: self.pal.subtle,
                    });
                    (from, row_from) = (self.out.glyphs.len(), self.out.glyphs.len());
                }
                self.out.glyphs.push(Glyph {
                    x: left + pen + shaped.kern,
                    inst: shaped.inst,
                    id: shaped.id,
                });
                pen += shaped.kern + shaped.advance;
                column += 1;
            }
            flush(self, from, row, now);
            row += line;
            let start = starts.get(n).copied().unwrap_or(end);
            let next = starts.get(n + 1).copied().unwrap_or(end);
            self.span(start, next, first, row);
        }
        let bottom = row + pad;
        if has_panel && let Op::Round { y1, .. } = &mut self.out.ops[panel] {
            *y1 = bottom as i32;
        }
        self.place_markers(top + pad + above, top);
        self.flow.close(y + (bottom - top), self.px(sheet::GAP));
    }

    fn quote(&mut self, blocks: &[Block], x: f32, w: f32, ctx: &Ctx) {
        // Its first child's margin above and its last child's below are nothing, and with no
        // padding of its own the quote starts and ends where they do.
        let top = self.flow.y
            + if self.flow.fresh {
                0.0
            } else {
                self.flow.pending
            };
        self.flow.flat = true;
        let inner = Ctx {
            ink: self.pal.quote,
            ..*ctx
        };
        let bar = self.px(sheet::QUOTE_BAR);
        let pad = self.px(sheet::QUOTE_PAD);
        self.blocks(
            blocks,
            x + bar + pad,
            (w - bar - 2.0 * pad).max(pad),
            &inner,
        );
        self.flow.flat = false;
        let bottom = self.flow.y;
        self.fill(x, top, x.round() + bar.round(), bottom, self.pal.border);
        if bottom > top || !self.flow.fresh {
            self.flow.pending = self.px(sheet::GAP);
        }
    }

    fn alert(&mut self, kind: Alert, blocks: &[Block], x: f32, w: f32, ctx: &Ctx) {
        let ink = self.pal.alerts[kind as usize];
        let (word, mark) = match kind {
            Alert::Note => ("Note", Mark::Note),
            Alert::Tip => ("Tip", Mark::Tip),
            Alert::Important => ("Important", Mark::Important),
            Alert::Warning => ("Warning", Mark::Warning),
            Alert::Caution => ("Caution", Mark::Caution),
        };
        let top = self.flow.open(0.0);
        let bar = self.px(sheet::QUOTE_BAR);
        let (pad, inner_x) = (
            self.px(sheet::ALERT_PAD),
            x + bar + self.px(sheet::QUOTE_PAD),
        );
        // The title: an icon and a word on a line as tall as its letters.
        let title = Ctx {
            line: ctx.size,
            weight: Weight::Medium,
            ..*ctx
        };
        let base = self.base(&title);
        let line = (top + pad).round();
        let icon = self.px(sheet::ICON);
        self.out.ops.push(Op::Mark {
            kind: mark,
            x: inner_x.round() as i32,
            y: (line + (base.height - icon) / 2.0).round() as i32,
            px: icon,
            ink: ink.title,
        });
        let face = Face {
            weight: Weight::Medium,
            ..Face::default()
        };
        let text_x = inner_x.round() + icon + self.px(sheet::ICON_GAP);
        self.label(
            word,
            face,
            ctx.size,
            ink.title,
            text_x,
            line + base.above,
            false,
        );
        // The title is the alert's first line: a list item's marker hangs beside it.
        self.place_markers(line + base.above, line);
        // Padded, so the children's margins stay inside: a gap under the title, none at the end.
        let outer = self.flow;
        self.flow = Flow {
            y: line + base.height,
            pending: self.px(sheet::GAP),
            fresh: false,
            flat: false,
        };
        let inner_w = (w - bar - 2.0 * self.px(sheet::QUOTE_PAD)).max(pad);
        self.blocks(blocks, inner_x, inner_w, ctx);
        let bottom = self.flow.y + pad;
        self.flow = outer;
        self.fill(x, top, x.round() + bar.round(), bottom, ink.bar);
        self.flow.close(bottom, self.px(sheet::GAP));
    }

    fn list(&mut self, start: Option<u64>, items: &[Item], x: f32, w: f32, ctx: &Ctx) {
        let indent = self.px(sheet::INDENT);
        let (ix, iw) = (x + indent, (w - indent).max(indent));
        let inner = Ctx {
            lists: ctx.lists + 1,
            ..*ctx
        };
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.flow.join(self.px(sheet::ITEM_GAP));
            }
            let kind = match (item.task, start) {
                (Some(done), _) => MarkerKind::Task(done),
                (None, Some(first)) => {
                    MarkerKind::Number(numeral(first.saturating_add(i as u64), ctx.lists))
                }
                (None, None) => match ctx.lists {
                    0 => MarkerKind::Disc,
                    1 => MarkerKind::Ring,
                    _ => MarkerKind::Square,
                },
            };
            self.markers.push(Marker {
                kind,
                x: ix,
                ink: self.own(self.pal.marker, ctx.ink),
                size: ctx.size,
                weight: ctx.weight,
                anchor: None,
            });
            let waiting = self.markers.len();
            self.blocks(&item.blocks, ix, iw, &inner);
            if self.markers.len() >= waiting {
                self.marker_line(&inner, item.src.start);
            }
        }
        // A list in a list has no margin of its own; either way its last item's goes through.
        self.flow.join(if ctx.lists > 0 {
            0.0
        } else {
            self.px(sheet::GAP)
        });
    }

    /// An item with no line of its own still shows its marker, on a line's worth of room.
    fn marker_line(&mut self, ctx: &Ctx, src: usize) {
        let base = self.base(ctx);
        let y = self.flow.open(0.0);
        let top = y.round();
        self.place_markers(top + base.above, top);
        self.span(src, src, top, top + base.height);
        self.flow.close(y + base.height, 0.0);
    }

    /// A `<details>` block: `state` is whether its tag says open, where the tag is, and the
    /// link its summary line is. Closed, as a browser shows one unless the tag says otherwise,
    /// it's the summary line alone behind a triangle pointing right; open, the triangle points
    /// down and what it holds follows, with nothing between but the blocks' own margins. The
    /// reader turns one by listing it in the environment.
    fn details(
        &mut self,
        summary: &Para,
        blocks: &[Block],
        state: (bool, usize, u32),
        x: f32,
        w: f32,
        ctx: &Ctx,
    ) {
        let (open, at, link) = state;
        let open = open != self.env.opened.contains(&at);
        let base = self.base(ctx);
        let prepared = self.prepare(&summary.inlines, ctx, &base);
        let indent = self.px(18.0);
        let broken = break_lines(&prepared, (w - indent).max(indent), &base, true);
        let y = self.flow.open(0.0);
        let icon = self.px(sheet::ICON);
        self.out.ops.push(Op::Mark {
            kind: if open { Mark::Open } else { Mark::Closed },
            x: x.round() as i32,
            y: (y.round() + (base.height - icon) / 2.0).round() as i32,
            px: icon,
            ink: ctx.ink,
        });
        self.emit(
            &prepared,
            &broken,
            x + indent,
            y,
            w - indent,
            summary.align,
            summary.src.end,
        );
        // The whole summary line is the toggle, as in a browser.
        let height = broken.height.max(base.height);
        self.link_rect(
            link,
            Rect {
                x: x.round(),
                y: y.round(),
                w: w.ceil(),
                h: height.ceil(),
            },
        );
        self.flow.close(y + height, 0.0);
        if open {
            self.blocks(blocks, x, w, ctx);
        }
        self.flow.join(self.px(sheet::GAP));
    }

    fn notes(&mut self, notes: &[Note], x: f32, w: f32, ctx: &Ctx) {
        // Under a hairline, small and muted. The rule keeps the notes' margins inside, so the
        // first has its gap above it like the rest.
        let y = self.flow.open(0.0);
        self.hairline(x, x + w, y, self.pal.border);
        self.flow.close(y.round() + self.hair, self.px(sheet::GAP));
        let small = Ctx {
            size: sheet::NOTE,
            line: sheet::NOTE * sheet::TEXT_LINE,
            weight: Weight::Regular,
            ink: self.pal.muted,
            heading: false,
            lists: ctx.lists,
        };
        let indent = self.px(sheet::NOTE_INDENT);
        for note in notes {
            self.markers.push(Marker {
                kind: MarkerKind::Number(format!("{}.", note.number)),
                x: x + indent,
                ink: self.pal.muted,
                size: small.size,
                weight: small.weight,
                anchor: Some(format!("fn-{}", note.label)),
            });
            let waiting = self.markers.len();
            self.blocks(&note.blocks, x + indent, (w - indent).max(indent), &small);
            if self.markers.len() >= waiting {
                self.marker_line(&small, 0);
            }
        }
    }

    fn table(&mut self, table: &Table, x: f32, w: f32, ctx: &Ctx) {
        let columns = table
            .rows
            .iter()
            .map(Vec::len)
            .fold(table.head.len(), usize::max);
        if columns == 0 {
            return;
        }
        let header = Ctx {
            weight: Weight::Semibold,
            ink: self.own(self.pal.table_header, ctx.ink),
            ..*ctx
        };
        let (base, head_base) = (self.base(ctx), self.base(&header));
        let hair = self.hair;
        // A table of so many columns that their padding alone would fill the column gives
        // some of it up: at most half the width goes to padding.
        let spare = (w / 2.0 / (2.0 * columns as f32)).floor().max(1.0);
        let (mut pad_x, pad_y) = (
            self.px(sheet::CELL_X).round().min(spare),
            self.px(sheet::CELL_Y).round(),
        );
        // Every cell, shaped once: the header row first.
        let mut rows: Vec<Vec<Prepared>> = Vec::with_capacity(table.rows.len() + 1);
        let head: Vec<Prepared> = table
            .head
            .iter()
            .map(|c| self.prepare(&c.inlines, &header, &head_base))
            .collect();
        rows.push(head);
        for row in &table.rows {
            rows.push(
                row.iter()
                    .map(|c| self.prepare(&c.inlines, ctx, &base))
                    .collect(),
            );
        }
        // A column is as wide as its widest cell on one line, or as narrow as its longest word.
        let (mut widest, mut least) = (vec![0.0f32; columns], vec![0.0f32; columns]);
        for (r, row) in rows.iter().enumerate() {
            let base = if r == 0 { &head_base } else { &base };
            for (c, cell) in row.iter().enumerate() {
                widest[c] =
                    widest[c].max(break_lines(cell, f32::INFINITY, base, false).width.ceil());
                least[c] = least[c].max(break_lines(cell, 0.0, base, false).width.ceil());
            }
        }
        let lines = hair * (columns + 1) as f32;
        let frame = lines + 2.0 * pad_x * columns as f32;
        let room = (w - frame).max(0.0);
        // A table whose longest words don't fit the column may run past its edge, as far as
        // the area's less the page's padding: what GitHub's sideways scroll shows on a wide
        // screen. Past that it gives up its cells' padding, down to a little, before a word
        // is cut: that happens only when there's no room anywhere.
        let (all, tight): (f32, f32) = (widest.iter().sum(), least.iter().sum());
        let furthest = (self.edge - x).max(w) - lines;
        if tight + 2.0 * pad_x * columns as f32 > furthest {
            let fits = ((furthest - tight) / (2.0 * columns as f32)).floor();
            pad_x = fits.clamp(self.px(sheet::CELL_X_LEAST).round(), pad_x);
        }
        let beyond = (furthest - 2.0 * pad_x * columns as f32).max(room);
        let widths: Vec<f32> = if all <= room {
            widest.clone()
        } else if tight <= room {
            // Each column gives up the same share of what it could spare.
            let spare = (room - tight) / (all - tight).max(1.0);
            let between = |(most, least): (&f32, &f32)| (least + (most - least) * spare).floor();
            widest.iter().zip(&least).map(between).collect()
        } else if tight <= beyond {
            least.clone()
        } else {
            // Words break in the column with the longest, and only as far as needed, the
            // others whole; if every column is down to its floor, all shrink alike.
            let mut widths = least.clone();
            let mut over = tight - beyond;
            let floor = self.px(sheet::CELL_FLOOR * ctx.size);
            let mut by_least: Vec<usize> = (0..columns).collect();
            by_least.sort_by(|a, b| least[*b].total_cmp(&least[*a]));
            for c in by_least {
                if over <= 0.0 {
                    break;
                }
                let take = over.min((widths[c] - floor).max(0.0)).ceil();
                widths[c] -= take;
                over -= take;
            }
            if over > 0.0 {
                let sum: f32 = widths.iter().sum();
                let share = (beyond / sum.max(1.0)).min(1.0);
                widths = widths
                    .iter()
                    .map(|w| (w * share).floor().max(1.0))
                    .collect();
            }
            widths
        };
        let y = self.flow.open(0.0);
        let top = y.round();
        let left = x.round();
        let right = left
            + hair * (columns + 1) as f32
            + widths.iter().map(|c| c + 2.0 * pad_x).sum::<f32>();
        let mut edges = vec![top];
        let mut row_top = top;
        for (r, row) in rows.iter().enumerate() {
            let base = if r == 0 { &head_base } else { &base };
            let broken: Vec<Broken> = row
                .iter()
                .enumerate()
                .map(|(c, cell)| break_lines(cell, widths[c], base, true))
                .collect();
            let tallest = broken.iter().map(|b| b.height).fold(base.height, f32::max);
            let bottom = row_top + hair + pad_y + tallest + pad_y;
            // Every second row of the body is shaded.
            if r > 0 && r % 2 == 0 {
                self.fill(left, row_top + hair, right, bottom, self.pal.panel);
            }
            let mut cell_x = left + hair;
            for (c, cell) in row.iter().enumerate() {
                // A header cell with no alignment is centred, as a browser centres one.
                let align = match table.aligns.get(c).copied().unwrap_or_default() {
                    Align::Start if r == 0 => Align::Center,
                    align => align,
                };
                let end = if r == 0 {
                    table.head.get(c)
                } else {
                    table.rows[r - 1].get(c)
                };
                let end = end.map_or(table.src.end, |p| p.src.end);
                // Cells sit in the middle of their row.
                let cell_y = row_top + hair + pad_y + ((tallest - broken[c].height) / 2.0).round();
                self.emit(
                    cell,
                    &broken[c],
                    cell_x + pad_x,
                    cell_y,
                    widths[c],
                    align,
                    end,
                );
                cell_x += widths[c] + 2.0 * pad_x + hair;
            }
            row_top = bottom;
            edges.push(bottom);
        }
        // The grid, over the shading.
        let border = self.pal.border;
        for edge in &edges {
            self.fill(left, *edge, right, edge + hair, border);
        }
        let bottom = row_top + hair;
        let mut edge = left;
        for width in widths.iter().map(Some).chain([None]) {
            self.fill(edge, top, edge + hair, bottom, border);
            edge += hair + width.map_or(0.0, |w| w + 2.0 * pad_x);
        }
        self.place_markers(top + hair + pad_y + head_base.above, top);
        self.flow.close(y + (bottom - top), self.px(sheet::GAP));
    }

    // ------------------------------------------------------------ list markers

    /// Draws the markers waiting for a line, now that there is one.
    fn place_markers(&mut self, baseline: f32, top: f32) {
        let baseline = baseline.round();
        for marker in std::mem::take(&mut self.markers) {
            if let Some(name) = marker.anchor {
                self.out.anchors.push((name, top));
            }
            let face = Face {
                weight: marker.weight,
                ..Face::default()
            };
            let inst = self.pass.primary(face, marker.size);
            let m = self.pass.metrics(inst);
            // A browser sizes and places a bullet from the face's ascent in whole pixels: two
            // thirds of it across, a pixel over half that to a side.
            let ascent = (m.ascent * marker.size / m.upem).round();
            let two_thirds = (ascent * 2.0 / 3.0).floor();
            let side = self.px(((two_thirds + 1.0) / 2.0).floor()).round();
            let x0 = (marker.x - self.px(two_thirds + 7.0)).round();
            let down = (3.0 * (ascent - two_thirds) / 2.0).floor();
            let y0 = (baseline - self.px(ascent - down)).round();
            match marker.kind {
                MarkerKind::Disc => {
                    self.round(x0, y0, x0 + side, y0 + side, side / 2.0, 0.0, marker.ink)
                }
                MarkerKind::Ring => {
                    let line = sheet::stroke(self.s);
                    self.round(x0, y0, x0 + side, y0 + side, side / 2.0, line, marker.ink);
                }
                MarkerKind::Square => self.fill(x0, y0, x0 + side, y0 + side, marker.ink),
                MarkerKind::Number(text) => {
                    // Right-aligned a space before the text, so the periods line up.
                    let mut run = self.pass.run(face, marker.size, self.s);
                    let space = self.pass.next(&mut run, ' ').advance;
                    self.label(
                        &text,
                        face,
                        marker.size,
                        marker.ink,
                        marker.x - space,
                        baseline,
                        true,
                    );
                }
                MarkerKind::Task(done) => {
                    let side = self.px(sheet::CHECK).round();
                    let x0 = (marker.x - self.px(sheet::CHECK_GAP)).round() - side;
                    let y0 = baseline - side;
                    let radius = self.px(sheet::CHECK_RADIUS);
                    if done {
                        let fill = self.pal.check;
                        self.round(x0, y0, x0 + side, y0 + side, radius, 0.0, fill);
                        // The tick in whichever of the page and white shows better on the fill.
                        let (page, white) = (self.pal.canvas, Rgb(255, 255, 255));
                        let ink = if contrast(page, fill) > contrast(white, fill) {
                            page
                        } else {
                            white
                        };
                        self.out.ops.push(Op::Mark {
                            kind: Mark::Check,
                            x: x0 as i32,
                            y: y0 as i32,
                            px: side,
                            ink,
                        });
                    } else {
                        let line = sheet::stroke(self.s);
                        self.round(x0, y0, x0 + side, y0 + side, radius, line, marker.ink);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ text

    /// How a run looks: the document's style in this block's type and this palette.
    fn look(&mut self, style: &doc::Style, ctx: &Ctx, base: &Base) -> Look {
        let mut face = Face {
            mono: style.code || style.kbd,
            weight: if style.strong {
                Weight::Semibold
            } else {
                ctx.weight
            },
            italic: style.em,
        };
        let mut size = ctx.size;
        let (mut boxed, mut pad, mut up, mut down, mut rise) = (Boxed::No, 0.0, 0.0, 0.0, 0.0);
        if style.kbd {
            (face.weight, size, boxed) = (Weight::Regular, sheet::KBD, Boxed::Key);
        } else if style.code {
            boxed = Boxed::Pill;
            if !ctx.heading {
                size *= sheet::CODE;
            }
        }
        if style.script != 0 {
            size *= sheet::SCRIPT;
            let moved = if style.script > 0 {
                sheet::SUP_RISE
            } else {
                -sheet::SUB_DROP
            };
            rise = self.px(size * moved);
        }
        if boxed != Boxed::No {
            let inst = self.pass.primary(face, size);
            let m = self.pass.metrics(inst);
            let unit = self.px(size) / m.upem;
            let (ascent, descent) = (m.ascent * unit, m.descent * unit);
            if boxed == Boxed::Pill {
                let (across, along) = match ctx.heading {
                    true => (sheet::PILL_X_HEADING, 0.0),
                    false => (sheet::PILL_X, sheet::PILL_Y),
                };
                pad = self.px(size * across);
                (up, down) = (
                    rise + ascent + self.px(size * along),
                    descent + self.px(size * along) - rise,
                );
            } else {
                // A key cap's box is centred half an x-height above the baseline, and its
                // letters sit on a ten pixel line inside its padding and border.
                let height = self.px(sheet::KBD_HEIGHT).round();
                up = (base.x_height / 2.0 + height / 2.0).round();
                down = height - up;
                let inner = self.px(10.0);
                let from_top = self.px(4.0) + (inner - (ascent + descent)) / 2.0 + ascent;
                rise = up - from_top;
                pad = self.px(sheet::KBD_PAD) + self.hair;
            }
        }
        // A link is the link's colour whatever it holds; otherwise the nearest element with a
        // colour of its own gives it, and with none the block's does.
        let mut ink = ctx.ink;
        let mut roles = style.roles;
        while roles != 0 {
            let role = match roles & 15 {
                doc::STRONG => self.pal.strong,
                doc::EM => self.pal.em,
                doc::STRIKE => self.pal.del,
                _ => self.pal.code,
            };
            if role != self.pal.text {
                ink = role;
                break;
            }
            roles >>= 4;
        }
        if style.kbd {
            ink = ctx.ink;
        }
        if style.link.is_some() {
            ink = self.pal.link;
        }
        Look {
            face,
            size,
            ink,
            underline: style.link.is_some() && self.pal.link_underline,
            strike: style.strike,
            rise,
            boxed,
            span: style.span,
            pad,
            up,
            down,
            link: style.link,
        }
    }

    /// Shapes a paragraph's runs and cuts them into words, spaces and what isn't text.
    fn prepare(&mut self, inlines: &[Inline], ctx: &Ctx, base: &Base) -> Prepared {
        let mut out = Prepared::default();
        // Whether the last piece is a word that ended with its run, not at a space: the next
        // run's first word goes on from it.
        let mut open = false;
        for inline in inlines {
            match inline {
                Inline::Break => {
                    out.pieces.push(Piece {
                        kind: Kind::Break,
                        look: 0,
                        from: 0,
                        to: 0,
                        width: 0.0,
                        lead: 0.0,
                        src: 0,
                        joined: false,
                    });
                    open = false;
                }
                Inline::Text { text, style, src } => {
                    let look = self.look(style, ctx, base);
                    let at = match out.looks.iter().rposition(|l| *l == look) {
                        Some(at) => at,
                        None => {
                            out.looks.push(look);
                            out.looks.len() - 1
                        }
                    } as u32;
                    open = self.words(&mut out, text, *src, look, at, open);
                }
                Inline::Image {
                    dest,
                    alt,
                    link,
                    width,
                    height,
                    src,
                } => {
                    let inset = self.inset(&mut out, dest, alt, *link, *width, *height, ctx);
                    out.insets.push(inset);
                    out.pieces.push(Piece {
                        kind: Kind::Inset(out.insets.len() as u32 - 1),
                        look: 0,
                        from: 0,
                        to: 0,
                        width: 0.0,
                        lead: 0.0,
                        src: *src,
                        joined: open,
                    });
                    open = true;
                }
            }
        }
        if out.looks.is_empty() {
            let plain = self.look(&doc::Style::default(), ctx, base);
            out.looks.push(plain);
        }
        out
    }

    /// A run's text as words and spaces. Says whether it ended inside a word.
    fn words(
        &mut self,
        out: &mut Prepared,
        text: &str,
        src: usize,
        look: Look,
        at: u32,
        open: bool,
    ) -> bool {
        let mut run = self.pass.run(look.face, look.size, self.s);
        // The word being made: where its glyphs start, its pen, and the kerning before it.
        let mut word: Option<(u32, f32, f32, usize)> = None;
        let mut joined = open;
        let code = look.boxed == Boxed::Pill;
        let mut chars = text.char_indices().peekable();
        let mut before = None;
        while let Some((offset, c)) = chars.next() {
            if c.is_whitespace() && c != '\u{a0}' {
                if let Some(word) = word.take() {
                    word_piece(out, at, word, joined);
                }
                joined = false;
                // Spaces collapse to one, but in code, which keeps its own.
                let again = matches!(out.pieces.last(), Some(p) if p.kind == Kind::Space);
                if code || !again {
                    let shaped = self.pass.next(&mut run, ' ');
                    out.pieces.push(Piece {
                        kind: Kind::Space,
                        look: at,
                        from: 0,
                        to: 0,
                        width: shaped.advance + shaped.kern,
                        lead: 0.0,
                        src: src + offset,
                        joined: false,
                    });
                }
                before = Some(c);
                continue;
            }
            if c.is_control() || unseen(c) {
                continue;
            }
            // East Asian text has no spaces to break at: a line may end before an ideograph,
            // a kana or a hangul, unless it's a mark that can't start a line.
            if wide(c)
                && !no_start(c)
                && let Some(word) = word.take()
            {
                word_piece(out, at, word, joined);
                joined = false;
            }
            let shaped = self.pass.next(&mut run, c);
            let (_, pen, _, _) =
                word.get_or_insert((out.glyphs.len() as u32, 0.0, shaped.kern, src + offset));
            if *pen > 0.0 {
                *pen += shaped.kern;
            }
            out.glyphs.push(Glyph {
                x: *pen,
                inst: shaped.inst,
                id: shaped.id,
            });
            *pen += shaped.advance;
            // A line may end after a hyphen, a slash or a question mark inside a word, as in a
            // long address, and after an East Asian character that may end one.
            let next = chars.peek().map(|(_, c)| *c);
            let breaks = match c {
                '-' => {
                    before.is_some_and(char::is_alphabetic) && next.is_some_and(char::is_alphabetic)
                }
                '/' | '?' => {
                    before.is_some_and(|b| b != '/' && b != ':')
                        && next.is_some_and(char::is_alphanumeric)
                }
                '—' => next.is_some_and(char::is_alphanumeric),
                _ => wide(c) && !no_end(c) && next.is_some_and(|n| !no_start(n)),
            };
            if breaks && let Some(word) = word.take() {
                word_piece(out, at, word, joined);
                joined = false;
            }
            before = Some(c);
        }
        match word {
            Some(word) => {
                word_piece(out, at, word, joined);
                true
            }
            // A run of nothing changes nothing; one that ended at a space or a break point
            // leaves the next free.
            None => open && before.is_none(),
        }
    }

    /// A picture in a line: itself if it's here, else its words.
    #[allow(clippy::too_many_arguments)]
    fn inset(
        &mut self,
        out: &mut Prepared,
        dest: &str,
        alt: &str,
        link: u32,
        width: Option<Length>,
        height: Option<Length>,
        ctx: &Ctx,
    ) -> Inset {
        let state = self.picture(dest);
        if let Pic::Here(file, w, h) = state {
            // A picture's pixel is a CSS pixel, as in a browser; the source may give a size.
            let (mut pw, mut ph) = (self.px(w), self.px(h));
            let px = |l: Option<Length>| match l {
                Some(Length::Px(v)) if v > 0.0 => Some(self.px(v)),
                _ => None,
            };
            match (px(width), px(height)) {
                (Some(nw), Some(nh)) => (pw, ph) = (nw, nh),
                (Some(nw), None) => (pw, ph) = (nw, ph * nw / pw.max(1.0)),
                (None, Some(nh)) => (pw, ph) = (pw * nh / ph.max(1.0), nh),
                (None, None) => {}
            }
            let share = match width {
                Some(Length::Percent(p)) if p > 0.0 => Some(p / 100.0),
                _ => None,
            };
            return Inset::Picture {
                file,
                w: pw,
                h: ph,
                share,
                link,
            };
        }
        // Its alternative text, else the file's name.
        let name = dest.rsplit('/').next().unwrap_or("");
        let words = [alt.trim(), name, "picture"];
        let words = words.iter().find(|w| !w.is_empty()).copied().unwrap_or("");
        let mut short: String = words.chars().take(80).collect();
        if short.len() < words.len() {
            short.push('…');
        }
        let never = matches!(state, Pic::Never);
        let ink = if never {
            self.pal.muted
        } else {
            self.pal.subtle
        };
        let mut run = self.pass.run(Face::default(), ctx.size, self.s);
        let (from, mut pen) = (out.glyphs.len() as u32, 0.0);
        for c in short.chars().filter(|c| !c.is_control() && !unseen(*c)) {
            let shaped = self
                .pass
                .next(&mut run, if c.is_whitespace() { ' ' } else { c });
            pen += shaped.kern;
            out.glyphs.push(Glyph {
                x: pen,
                inst: shaped.inst,
                id: shaped.id,
            });
            pen += shaped.advance;
        }
        let icon = if never {
            self.px(sheet::ICON + sheet::ABSENT_GAP)
        } else {
            0.0
        };
        Inset::Absent {
            from,
            to: out.glyphs.len() as u32,
            width: 2.0 * self.px(sheet::ABSENT_PAD) + icon + pen,
            ink,
            mark: never,
            size: ctx.size,
            link,
        }
    }

    /// Whether a picture's file is here, coming or never will be.
    fn picture(&mut self, dest: &str) -> Pic {
        let plain = dest.split(['?', '#']).next().unwrap_or("");
        let local = plain.strip_prefix("file://").unwrap_or(plain);
        // Anything else with a scheme is somewhere a page doesn't fetch from.
        let scheme = local.split_once(':').is_some_and(|(s, _)| {
            s.len() > 1
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        });
        if local.is_empty() || scheme || local.starts_with("//") {
            return Pic::Never;
        }
        let local = PathBuf::from(decoded(local));
        let path = match (local.is_absolute(), self.env.dir) {
            (true, _) => local,
            (false, Some(dir)) => dir.join(local),
            (false, None) => return Pic::Never,
        };
        match self.pictures.known.get(&path) {
            Some(Some(picture)) if picture.width > 0 && picture.height > 0 => {
                let (w, h) = (picture.width as f32, picture.height as f32);
                let known = self.out.files.iter().position(|f| *f == path);
                let file = known.unwrap_or_else(|| {
                    self.out.files.push(path);
                    self.out.files.len() - 1
                });
                Pic::Here(file as u32, w, h)
            }
            Some(_) => Pic::Never,
            None => {
                // Only PNG is decoded here: a file that says it's something else isn't asked for.
                let kind = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let other = [
                    "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico", "tif", "tiff", "avif",
                    "heic", "pdf", "mp4", "mov", "webm",
                ];
                if other.contains(&kind.to_ascii_lowercase().as_str()) {
                    return Pic::Never;
                }
                if !self.out.wants.contains(&path) {
                    self.out.wants.push(path);
                }
                Pic::Coming
            }
        }
    }

    /// Puts a paragraph's lines on the page, its top at `y`.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        text: &Prepared,
        broken: &Broken,
        x: f32,
        y: f32,
        width: f32,
        align: Align,
        end: usize,
    ) {
        for (n, line) in broken.lines.iter().enumerate() {
            let top = (y + line.top).round();
            let baseline = (top + line.above).round();
            let spare = (width - line.width).max(0.0);
            let lx = x + match align {
                Align::Center => (spare / 2.0).round(),
                Align::Right => spare,
                Align::Start | Align::Left => 0.0,
            };
            if n == 0 {
                self.place_markers(baseline, top);
            }
            let frags = &broken.frags[line.from..line.to];
            let piece = |f: &Frag| &text.pieces[f.piece as usize];
            let look = |f: &Frag| &text.looks[piece(f).look as usize];
            let is_inset = |f: &Frag| matches!(piece(f).kind, Kind::Inset(_));
            // The boxes behind the text: a pill for each stretch of one code span.
            let mut at = 0;
            while at < frags.len() {
                let first = look(&frags[at]);
                if first.boxed == Boxed::No || is_inset(&frags[at]) {
                    at += 1;
                    continue;
                }
                let mut last = at;
                while last + 1 < frags.len() && !is_inset(&frags[last + 1]) && {
                    let next = look(&frags[last + 1]);
                    next.boxed == first.boxed && next.span == first.span
                } {
                    last += 1;
                }
                let x0 = lx + frags[at].x - first.pad;
                let x1 = lx + frags[last].x + frags[last].width + first.pad;
                let (y0, y1) = (
                    (baseline - first.up).round(),
                    (baseline + first.down).round(),
                );
                let radius = self.px(sheet::RADIUS);
                if first.boxed == Boxed::Pill {
                    self.round(x0, y0, x1, y1, radius, 0.0, self.pal.pill);
                } else {
                    // A key cap: a border, heavier along the bottom.
                    let hair = self.hair;
                    self.round(x0, y0, x1, y1, radius, 0.0, self.pal.border);
                    let (x0, x1) = (x0.round() + hair, x1.round() - hair);
                    self.round(
                        x0,
                        y0 + hair,
                        x1,
                        y1 - 2.0 * hair,
                        radius - hair,
                        0.0,
                        self.pal.panel,
                    );
                }
                at = last + 1;
            }
            // The glyphs, a run for each stretch of one colour, size and baseline.
            let mut run: Option<(Rgb, f32, i32, usize)> = None;
            let mut drawn = Vec::with_capacity(frags.len());
            for frag in frags {
                drawn.push(self.out.glyphs.len());
                let (p, l) = (piece(frag), look(frag));
                match p.kind {
                    Kind::Space | Kind::Break => {}
                    Kind::Word => {
                        let key = (l.ink, self.px(l.size), (baseline - l.rise).round() as i32);
                        if run.is_none_or(|(ink, px, base, _)| (ink, px, base) != key) {
                            self.end_run(run.take());
                            run = Some((key.0, key.1, key.2, self.out.glyphs.len()));
                        }
                        let glyphs = &text.glyphs[frag.from as usize..frag.to as usize];
                        let origin = glyphs.first().map_or(0.0, |g| g.x);
                        let placed = glyphs.iter().map(|g| Glyph {
                            x: lx + frag.x + g.x - origin,
                            ..*g
                        });
                        self.out.glyphs.extend(placed);
                    }
                    Kind::Inset(inset) => {
                        self.end_run(run.take());
                        self.put_inset(
                            text,
                            &text.insets[inset as usize],
                            lx + frag.x,
                            baseline,
                            frag,
                        );
                    }
                }
            }
            self.end_run(run.take());
            drawn.push(self.out.glyphs.len());
            // Lines under links and through struck text: one stretch for each link and each
            // struck run, straight through a pill inside it, padding and all, as a browser
            // draws one line; its place and weight are the text's where the stretch has any.
            let deco = |f: &Frag| {
                let l = look(f);
                let drawn = !is_inset(f) && (l.underline || l.strike);
                drawn.then_some((l.link.filter(|_| l.underline), l.strike))
            };
            let mut at = 0;
            while at < frags.len() {
                let Some(key) = deco(&frags[at]) else {
                    at += 1;
                    continue;
                };
                let mut last = at;
                while last + 1 < frags.len() && deco(&frags[last + 1]) == Some(key) {
                    last += 1;
                }
                let stretch = &frags[at..=last];
                let plain = stretch.iter().map(look).find(|l| l.boxed == Boxed::No);
                let first = *plain.unwrap_or(look(&frags[at]));
                let (x0, x1) = (
                    lx + frags[at].x - look(&frags[at]).pad,
                    lx + frags[last].x + frags[last].width + look(&frags[last]).pad,
                );
                let thick = (self.px(first.size) / 16.0).round().max(1.0);
                let own = baseline - first.rise;
                if first.underline {
                    // GitHub's offset under the baseline, whatever the text's size.
                    let y0 = (own + self.px(sheet::UNDERLINE)).round();
                    let (x0, x1) = (x0.round() as i32, x1.round() as i32);
                    if x1 > x0 {
                        self.out.ops.push(Op::Under {
                            x0,
                            y0: y0 as i32,
                            x1,
                            y1: (y0 + thick) as i32,
                            ink: first.ink,
                            from: drawn[at] as u32,
                            to: drawn[last + 1] as u32,
                            baseline: own.round() as i32,
                            px: self.px(first.size),
                        });
                    }
                }
                if first.strike {
                    let y0 = (own - self.px(first.size * 0.26) - thick / 2.0).round();
                    self.fill(x0, y0, x1, y0 + thick, first.ink);
                }
                at = last + 1;
            }
            // The links' rectangles: one for each link's stretch of this line.
            let link_of = |f: &Frag| match piece(f).kind {
                Kind::Inset(inset) => match text.insets[inset as usize] {
                    Inset::Picture { link, .. } | Inset::Absent { link, .. } => Some(link),
                },
                _ => look(f).link,
            };
            let mut at = 0;
            while at < frags.len() {
                let Some(link) = link_of(&frags[at]) else {
                    at += 1;
                    continue;
                };
                let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
                while at < frags.len() && link_of(&frags[at]) == Some(link) {
                    let frag = &frags[at];
                    let l = look(frag);
                    let (up, down, pad) = if let Kind::Inset(inset) = piece(frag).kind {
                        match text.insets[inset as usize] {
                            Inset::Picture { .. } => (frag.height, 0.0, 0.0),
                            Inset::Absent { size, .. } => {
                                let (up, down) = self.absent_box(size);
                                (up, down, 0.0)
                            }
                        }
                    } else if l.boxed != Boxed::No {
                        (l.up, l.down, l.pad)
                    } else {
                        (
                            l.rise + self.px(l.size) * 0.95,
                            self.px(l.size) * 0.28 - l.rise,
                            0.0,
                        )
                    };
                    x0 = x0.min(lx + frag.x - pad);
                    x1 = x1.max(lx + frag.x + frag.width + pad);
                    y0 = y0.min(baseline - up);
                    y1 = y1.max(baseline + down);
                    at += 1;
                }
                let rect = Rect {
                    x: x0.floor(),
                    y: y0.floor(),
                    w: (x1.ceil() - x0.floor()).max(0.0),
                    h: (y1.ceil() - y0.floor()).max(0.0),
                };
                self.link_rect(link, rect);
            }
            // The source this line shows: from its first piece to the next line's.
            let first_src = |l: &Line| {
                // A word cut over two lines is at its letters' places, a byte a letter.
                let first = broken.frags[l.from..l.to].first()?;
                let p = piece(first);
                Some(p.src + first.from.saturating_sub(p.from) as usize)
            };
            let src = first_src(line).unwrap_or(end);
            let next = broken.lines[n + 1..]
                .iter()
                .find_map(first_src)
                .unwrap_or(end);
            self.span(src, next, top, top + line.height);
        }
    }

    /// A rectangle a link covers. Its first is where the link enters the reading order, and
    /// where a name the link gives its place points.
    fn link_rect(&mut self, link: u32, rect: Rect) {
        let Some(rects) = self.rects.get_mut(link as usize) else {
            return;
        };
        if rects.is_empty() {
            self.order.push(link);
            if let Some(name) = &self.doc.links[link as usize].anchor {
                self.out.anchors.push((name.clone(), rect.y));
            }
        }
        rects.push(rect);
    }

    fn end_run(&mut self, run: Option<(Rgb, f32, i32, usize)>) {
        if let Some((ink, px, baseline, from)) = run
            && self.out.glyphs.len() > from
        {
            self.out.ops.push(Op::Text {
                from: from as u32,
                to: self.out.glyphs.len() as u32,
                baseline,
                px,
                ink,
            });
        }
    }

    /// Draws what isn't text in a line, its left edge at `x`.
    fn put_inset(&mut self, text: &Prepared, inset: &Inset, x: f32, baseline: f32, frag: &Frag) {
        match *inset {
            Inset::Picture { file, .. } => {
                let (x0, y1) = (x.round() as i32, baseline as i32);
                let (w, h) = (
                    frag.width.round().max(1.0) as i32,
                    frag.height.round().max(1.0) as i32,
                );
                self.out.ops.push(Op::Picture {
                    x0,
                    y0: y1 - h,
                    x1: x0 + w,
                    y1,
                    file,
                });
            }
            Inset::Absent {
                from,
                to,
                ink,
                mark,
                size,
                ..
            } => {
                // A bordered box round a small picture mark and the words.
                let (up, down) = self.absent_box(size);
                let (y0, height) = (baseline - up, up + down);
                let pad = self.px(sheet::ABSENT_PAD);
                let radius = self.px(sheet::RADIUS);
                self.round(
                    x,
                    y0,
                    x + frag.width,
                    y0 + height,
                    radius,
                    self.hair,
                    self.pal.border,
                );
                let mut left = x + pad;
                if mark {
                    let icon = self.px(sheet::ICON);
                    self.out.ops.push(Op::Mark {
                        kind: Mark::Picture,
                        x: left.round() as i32,
                        y: (y0 + (height - icon) / 2.0).round() as i32,
                        px: icon,
                        ink,
                    });
                    left += icon + self.px(sheet::ABSENT_GAP);
                }
                // The words that fit: a box too wide for its line loses its end.
                let room = x + frag.width - pad - left;
                let glyphs = &text.glyphs[from as usize..to as usize];
                let start = self.out.glyphs.len();
                for (i, glyph) in glyphs.iter().enumerate() {
                    let end = glyphs.get(i + 1).map_or(f32::MAX, |g| g.x);
                    if end > room + SLACK && i + 1 < glyphs.len() {
                        break;
                    }
                    self.out.glyphs.push(Glyph {
                        x: left + glyph.x,
                        ..*glyph
                    });
                }
                self.end_run(Some((ink, self.px(size), baseline as i32, start)));
            }
        }
    }

    /// How far the box of a picture that isn't there reaches above and below the baseline:
    /// centred on the middle of the text, so it sits in its line without growing it.
    fn absent_box(&self, size: f32) -> (f32, f32) {
        let height = self.px(sheet::ABSENT_HEIGHT).round();
        let up = (self.px(size) * 0.35 + height / 2.0).round();
        (up, height - up)
    }

    // ------------------------------------------------------------ the end

    fn finish(mut self) -> Laid {
        let (width, height) = (self.out.width, self.out.height);
        // The links, in the order they first showed, their rectangles inside the page.
        for link in std::mem::take(&mut self.order) {
            let Some(from) = self.doc.links.get(link as usize) else {
                continue;
            };
            let rects = std::mem::take(&mut self.rects[link as usize]);
            let inside = |r: Rect| {
                let (x0, y0) = (r.x.clamp(0.0, width), r.y.clamp(0.0, height));
                let (x1, y1) = (
                    (r.x + r.w).clamp(0.0, width),
                    (r.y + r.h).clamp(0.0, height),
                );
                Rect {
                    x: x0,
                    y: y0,
                    w: x1 - x0,
                    h: y1 - y0,
                }
            };
            self.out.links.push(LinkBox {
                rects: rects.into_iter().map(inside).collect(),
                dest: from.dest.clone(),
                title: from.title.clone(),
                image: from.image,
                source: from.src.clone(),
            });
        }
        // The source map goes one way only: down the page as the source goes on. What breaks
        // that (a second cell beside a taller first, footnotes gathered at the foot from the
        // middle of the text) is left out, and the block it's in answers for it.
        let mut spans: Vec<Span> = Vec::with_capacity(self.out.spans.len());
        for span in std::mem::take(&mut self.out.spans) {
            match spans.last_mut() {
                Some(last) if span.src <= last.src || span.y0 < last.y0 => {}
                Some(last) => {
                    last.end = last.end.min(span.src).max(last.src);
                    last.y1 = last.y1.min(span.y0).max(last.y0);
                    spans.push(span);
                }
                None => spans.push(span),
            }
        }
        self.out.spans = spans;
        // The index: which ops touch each band of rows.
        let bands = (height as i32 / BAND + 1).max(1) as usize;
        self.out.bands = vec![Vec::new(); bands];
        for (i, op) in self.out.ops.iter().enumerate() {
            let (y0, y1) = op.rows();
            if y1 <= y0 || y1 <= 0 {
                continue;
            }
            let first = (y0.max(0) / BAND) as usize;
            let last = ((y1 - 1).max(0) / BAND) as usize;
            for band in first..=last.min(bands - 1) {
                self.out.bands[band].push(i as u32);
            }
        }
        self.out
    }
}

/// A finished word: its glyphs are the last `from..` of those shaped so far.
fn word_piece(out: &mut Prepared, look: u32, word: (u32, f32, f32, usize), joined: bool) {
    let (from, width, lead, src) = word;
    out.pieces.push(Piece {
        kind: Kind::Word,
        look,
        from,
        to: out.glyphs.len() as u32,
        width,
        lead,
        src,
        joined,
    });
}

/// An ideograph, a kana, a hangul or the punctuation set with them: text a line may break
/// anywhere in, having no spaces.
fn wide(c: char) -> bool {
    matches!(
        u32::from(c),
        0x1100..=0x11ff
            | 0x2e80..=0x2fff
            | 0x3001..=0x303f
            | 0x3040..=0x30ff
            | 0x3100..=0x31ff
            | 0x3400..=0x4dbf
            | 0x4e00..=0x9fff
            | 0xa960..=0xa97f
            | 0xac00..=0xd7ff
            | 0xf900..=0xfaff
            | 0xfe30..=0xfe4f
            | 0xff01..=0xff60
            | 0xffe0..=0xffe6
            | 0x20000..=0x3134f
    )
}

/// East Asian marks a line can't start with: closing punctuation, small kana, the long vowel.
fn no_start(c: char) -> bool {
    matches!(
        c,
        '、' | '。'
            | '，'
            | '．'
            | '：'
            | '；'
            | '！'
            | '？'
            | '」'
            | '』'
            | '）'
            | '】'
            | '〉'
            | '》'
            | '〕'
            | '〗'
            | '〙'
            | '〛'
            | '］'
            | '｝'
            | 'ー'
            | '々'
            | 'ぁ'
            | 'ぃ'
            | 'ぅ'
            | 'ぇ'
            | 'ぉ'
            | 'っ'
            | 'ゃ'
            | 'ゅ'
            | 'ょ'
            | 'ゎ'
            | 'ァ'
            | 'ィ'
            | 'ゥ'
            | 'ェ'
            | 'ォ'
            | 'ッ'
            | 'ャ'
            | 'ュ'
            | 'ョ'
            | 'ヮ'
            | 'ヵ'
            | 'ヶ'
    )
}

/// East Asian marks a line can't end with: opening brackets.
fn no_end(c: char) -> bool {
    matches!(
        c,
        '「' | '『' | '（' | '【' | '〈' | '《' | '〔' | '〖' | '〘' | '〚' | '［' | '｛'
    )
}

/// Characters that take no room and draw nothing: joiners, selectors, marks of direction.
fn unseen(c: char) -> bool {
    matches!(c, '\u{ad}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{fe00}'..='\u{fe0f}' | '\u{feff}')
}

/// A path with its percent escapes read.
fn decoded(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (
            bytes[at],
            bytes.get(at + 1).copied().and_then(hex),
            bytes.get(at + 2).copied().and_then(hex),
        ) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                at += 3;
            }
            (byte, _, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An ordered item's marker: decimal at the top, roman a list down, letters below, as
/// GitHub's sheet has them.
fn numeral(n: u64, lists: usize) -> String {
    match lists {
        0 => format!("{n}."),
        1 if (1..4000).contains(&n) => {
            let table = [
                (1000, "m"),
                (900, "cm"),
                (500, "d"),
                (400, "cd"),
                (100, "c"),
                (90, "xc"),
                (50, "l"),
                (40, "xl"),
                (10, "x"),
                (9, "ix"),
                (5, "v"),
                (4, "iv"),
                (1, "i"),
            ];
            let (mut n, mut out) = (n, String::new());
            for (value, letters) in table {
                while n >= value {
                    out.push_str(letters);
                    n -= value;
                }
            }
            out + "."
        }
        _ if n >= 1 && lists >= 2 => {
            let (mut n, mut out) = (n, Vec::new());
            while n > 0 && out.len() < 16 {
                out.push(b'a' + ((n - 1) % 26) as u8);
                n = (n - 1) / 26;
            }
            out.reverse();
            String::from_utf8_lossy(&out).into_owned() + "."
        }
        _ => format!("{n}."),
    }
}

/// How well two colours tell apart: the ratio of their luminances, the lighter over the darker.
fn contrast(a: Rgb, b: Rgb) -> f32 {
    let light = |c: Rgb| {
        let part = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * part(c.0) + 0.7152 * part(c.1) + 0.0722 * part(c.2)
    };
    let (a, b) = (light(a) + 0.05, light(b) + 0.05);
    a.max(b) / a.min(b)
}

/// Breaks a paragraph into lines no wider than `width`: at spaces, at the break points inside
/// words, and with `split` inside a word too wide for a line of its own. Without `split` such
/// a word overhangs, which is how a column's least width is measured.
fn break_lines(text: &Prepared, width: f32, base: &Base, split: bool) -> Broken {
    /// A place the line could end: the pieces placed by then, and the piece the next line
    /// starts with.
    struct Stop {
        frags: usize,
        next: usize,
    }
    let mut out = Broken::default();
    let count = text.pieces.len();
    let look = |p: &Piece| &text.looks[p.look as usize];
    // The pill a piece is in, and its padding.
    let pill = |p: &Piece| {
        let l = look(p);
        (p.kind != Kind::Break && !matches!(p.kind, Kind::Inset(_)) && l.boxed != Boxed::No)
            .then_some((l.span, l.pad))
    };
    let (mut at, mut part, mut top) = (0usize, 0u32, 0.0f32);
    while at < count {
        let first = out.frags.len();
        // A line doesn't start with a space, but in code.
        while part == 0
            && at < count
            && text.pieces[at].kind == Kind::Space
            && pill(&text.pieces[at]).is_none()
        {
            at += 1;
        }
        // Spaces after the last break make no line of their own.
        if at >= count && !out.lines.is_empty() {
            break;
        }
        let (mut x, mut inside, mut stop) = (0.0f32, None::<(u32, f32)>, None::<Stop>);
        while at < count {
            let piece = &text.pieces[at];
            if piece.kind == Kind::Break {
                at += 1;
                break;
            }
            let own = pill(piece);
            // Leaving one pill and entering another, each with its padding.
            let crossing = if own.map(|p| p.0) != inside.map(|p| p.0) {
                inside.map_or(0.0, |p| p.1) + own.map_or(0.0, |p| p.1)
            } else {
                0.0
            };
            let closing = own.map_or(0.0, |p| p.1);
            let empty = out.frags.len() == first;
            let here = Stop {
                frags: out.frags.len(),
                next: at,
            };
            if piece.kind == Kind::Space {
                stop = Some(Stop {
                    next: at + 1,
                    ..here
                });
                x += crossing;
                inside = own;
                out.frags.push(Frag {
                    piece: at as u32,
                    from: 0,
                    to: 0,
                    x,
                    width: piece.width,
                    height: 0.0,
                });
                x += piece.width;
                at += 1;
                continue;
            }
            let after_space = out.frags[first..]
                .last()
                .is_some_and(|f| text.pieces[f.piece as usize].kind == Kind::Space);
            if !piece.joined && !empty && part == 0 && !after_space {
                stop = Some(here);
            }
            let lead = if empty || part > 0 { 0.0 } else { piece.lead };
            let (from, glyphs) = (
                piece.from + part,
                &text.glyphs[piece.from as usize..piece.to as usize],
            );
            let origin = glyphs.get(part as usize).map_or(0.0, |g| g.x);
            let (w, h) = match piece.kind {
                Kind::Inset(inset) => inset_size(&text.insets[inset as usize], width, base),
                _ => (piece.width - origin, 0.0),
            };
            let start = x + crossing + lead;
            if start + w + closing <= width + SLACK {
                inside = own;
                out.frags.push(Frag {
                    piece: at as u32,
                    from,
                    to: piece.to,
                    x: start,
                    width: w,
                    height: h,
                });
                x = start + w;
                (at, part) = (at + 1, 0);
                continue;
            }
            // It doesn't fit. The line ends at its last stop if it has one.
            if let Some(stop) = stop.take() {
                out.frags.truncate(stop.frags);
                (at, part) = (stop.next, 0);
                break;
            }
            if split && piece.kind == Kind::Word {
                // A word wider than the line breaks by letters: as many as fit, one at least.
                let room = width - start - closing;
                let rest = &glyphs[part as usize..];
                let ends = |i: usize| rest.get(i + 1).map_or(piece.width, |g| g.x) - origin;
                let mut fit = (0..rest.len())
                    .take_while(|i| ends(*i) <= room + SLACK)
                    .count();
                if fit == 0 && empty {
                    fit = 1;
                }
                if fit > 0 && fit < rest.len() {
                    out.frags.push(Frag {
                        piece: at as u32,
                        from,
                        to: from + fit as u32,
                        x: start,
                        width: ends(fit - 1),
                        height: 0.0,
                    });
                    part += fit as u32;
                    break;
                }
            }
            if empty || (!split && piece.joined) {
                // Nothing to do but let it overhang: the first thing on a line, or, when
                // words aren't cut, one that goes on from the word before it.
                inside = own;
                out.frags.push(Frag {
                    piece: at as u32,
                    from,
                    to: piece.to,
                    x: start,
                    width: w,
                    height: h,
                });
                x = start + w;
                (at, part) = (at + 1, 0);
                continue;
            }
            break;
        }
        // A line doesn't end with spaces either.
        while out.frags.len() > first
            && out
                .frags
                .last()
                .is_some_and(|f| text.pieces[f.piece as usize].kind == Kind::Space)
        {
            out.frags.pop();
        }
        let end = out.frags[first..].last().map_or(0.0, |f| {
            f.x + f.width + pill(&text.pieces[f.piece as usize]).map_or(0.0, |p| p.1)
        });
        // A picture taller than the text makes its line as tall as it needs.
        let tallest = out.frags[first..]
            .iter()
            .map(|f| f.height)
            .fold(0.0, f32::max);
        let above = base.above.max(tallest);
        let height = above + (base.height - base.above);
        out.lines.push(Line {
            from: first,
            to: out.frags.len(),
            width: end,
            above,
            height,
            top,
        });
        top += height;
        out.width = out.width.max(end);
    }
    out.height = top;
    out
}

/// How large something that isn't text is on a line `width` wide.
fn inset_size(inset: &Inset, width: f32, base: &Base) -> (f32, f32) {
    match *inset {
        Inset::Picture { w, h, share, .. } => {
            // Never wider than its line, the height keeping the proportion. Asked how narrow
            // it can go (a line of no width), it says a couple of lines' worth: it shrinks.
            let wanted = match share {
                Some(share) if width.is_finite() => width * share,
                _ => w,
            };
            let fitted = if width > 0.0 {
                wanted.min(width)
            } else {
                w.min(base.height * 2.0)
            };
            (fitted, (h * fitted / w.max(1.0)).max(1.0))
        }
        // A box of words gives up its end to fit its line, and never grows the line.
        Inset::Absent { width: full, .. } if width > 0.0 => (full.min(width), 0.0),
        Inset::Absent { width: full, .. } => (full.min(base.height * 2.0), 0.0),
    }
}
