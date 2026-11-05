//! The one layout function (C16). Render and motions both read it, through one cache. Contract §9
//! is the rule, from "How text becomes rows" on.
//!
//! A display line is never laid out whole unless something asks for its end. `Walker` steps along
//! it item by item from any row's start, `Src::place` makes one row, and `Line` keeps where each
//! row starts, so a row's cells are made again when they're drawn and hold no memory. An edit keeps
//! the rows it can't have changed. Cursor positions need no layout at all: they read the rope around
//! them, so they cost the same whatever the line's length.
#![warn(missing_docs)]

use crate::Registry;
use crate::error::*;
use crate::ids::*;
use crate::scene::Glyph;
use crate::text::*;
use ropey::RopeSlice;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::{Range, RangeInclusive};
use std::rc::Rc;
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete, UnicodeSegmentation};
use unicode_width::UnicodeWidthStr;

/// A direction along the text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// Toward the end of the text.
    Forward,
    /// Toward the start of the text.
    Backward,
}

/// What a laid-out cell shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CellKind {
    /// A cell of the buffer's text, or of a control character's drawing.
    Text,
    /// A cell of a tab.
    Tab,
    /// A cell of a Hide's replacement.
    Replacement,
    /// A cell of virtual text.
    Virtual,
    /// Blank, where a wider item moved to the next row. It has no position.
    Filler,
}

/// One cell of a laid-out row. Rows are made again when they're drawn, so these aren't kept.
#[derive(Clone, Debug)]
pub(crate) struct LayoutCell {
    /// What it draws.
    pub glyph: Glyph,
    /// The position it shows, or for virtual text the position it stands before.
    pub pos: usize,
    /// The display column of the item it belongs to.
    pub vcol: usize,
    /// What it shows.
    pub kind: CellKind,
    /// The face its `Styled` text gives it.
    pub face: Option<&'static str>,
}

/// A position the cursor can occupy, and where it is drawn. `vcol` counts cells from the
/// display line's start; `row` and `col` place it for the width the line was laid out at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stop {
    /// The buffer position, in bytes.
    pub pos: usize,
    /// Display column within the unwrapped display line (tabs, wide chars, replacements).
    pub vcol: usize,
    /// The row, counted from the display line's first.
    pub row: usize,
    /// The column on that row, counted from the text area's left edge.
    pub col: usize,
}

/// One display line: a logical line, joined with the lines after it when a Hide covers its newline.
#[derive(Clone, Debug)]
pub struct DisplayLine {
    start: usize,
    end: usize,
    lines: RangeInclusive<usize>,
    eof: bool,
    stops: Vec<Stop>,
}

impl DisplayLine {
    /// Where it starts in the buffer, in bytes.
    pub fn start(&self) -> usize {
        self.start
    }
    /// Where the next display line starts, or the end of the text for the last one.
    pub fn end(&self) -> usize {
        self.end
    }
    /// The logical lines it holds, from 0.
    pub fn lines(&self) -> RangeInclusive<usize> {
        self.lines.clone()
    }
    /// Whether this is the last display line of the text.
    pub fn eof(&self) -> bool {
        self.eof
    }
    /// Its cursor positions, in order, unwrapped: each on row 0 at its display column. There's
    /// always at least one.
    pub fn stops(&self) -> &[Stop] {
        &self.stops
    }
}

// ---------------------------------------------------------------- the items of a display line

/// `Walk::virt` once a position's virtual text is behind it.
const PAST: u32 = u32::MAX;

/// Where layout stands on a display line: everything it needs to carry on from here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Walk {
    pos: usize,
    /// The display column of the item here.
    vcol: usize,
    /// Which of the virtual texts at `pos` comes next.
    virt: u32,
    /// How far into that virtual text, or into the replacement at `pos`, in bytes.
    off: u32,
    /// The cells of the item here that earlier rows hold: a split tab or control character.
    skip: u32,
}

impl Walk {
    fn at(pos: usize, vcol: usize) -> Walk {
        let (virt, off, skip) = (0, 0, 0);
        Walk {
            pos,
            vcol,
            virt,
            off,
            skip,
        }
    }
    /// Whether the first cell of what's at `pos` is behind it.
    fn past_first(&self) -> bool {
        self.virt == PAST && (self.off > 0 || self.skip > 0)
    }
    fn moved(self, from: usize, to: usize) -> Walk {
        let pos = self.pos - from + to;
        Walk { pos, ..self }
    }
}

/// One item (contract §9, "Items"), with what layout needs to know about it.
struct Item {
    /// Its first cell.
    first: LayoutCell,
    /// Whether that's a cursor position.
    stop: bool,
    /// All its cells.
    width: usize,
    whitespace: bool,
    /// A control character, drawn as a cell per character. A tab's kind says so instead.
    control: Option<char>,
    /// Where it started: to place it again from, on another row.
    at: Walk,
}

impl Item {
    fn splits(&self) -> bool {
        self.first.kind == CellKind::Tab || self.control.is_some()
    }
    fn cell(&self, i: usize) -> LayoutCell {
        let glyph = match self.control {
            Some(c) => Glyph::Char(control_cells(c)[i]),
            None if self.first.kind == CellKind::Tab => Glyph::Char(' '),
            None if i == 0 => self.first.glyph.clone(),
            None => Glyph::Continuation,
        };
        let first = self.first.clone();
        LayoutCell { glyph, ..first }
    }
}

/// How a control character is drawn: Emacs's `^A`, `^?` and `\200`, a cell per character.
pub(crate) fn control_cells(c: char) -> Vec<char> {
    let code = c as u32;
    match code {
        0..=0x1F => vec!['^', char::from_u32(code + 0x40).unwrap_or('?')],
        0x7F => vec!['^', '?'],
        _ => format!("\\{code:03o}").chars().collect(),
    }
}

/// What a walk reads, gathered so it can run while the cache is borrowed.
struct Src<'a> {
    t: &'a TextBuf,
    h: Rc<Hidden>,
    tab: usize,
}

/// Steps along a display line, one item at a time.
struct Walker<'a> {
    src: &'a Src<'a>,
    w: Walk,
    /// The end of the visible run it's in: a newline, or a hidden range's start.
    run_end: usize,
    /// The rope's chunk holding `w.pos`, and where it starts.
    chunk: (&'a str, usize),
    /// How far along the text the last item depends on.
    read_to: usize,
}

impl Walker<'_> {
    /// The next item, or None at the line's end.
    fn next(&mut self) -> Option<Item> {
        loop {
            let p = self.w.pos;
            let span = self.src.h.starting_at(p);
            if let Some(s) = span.filter(|s| s.repl.is_none()) {
                self.w = Walk::at(s.end, self.w.vcol);
                continue;
            }
            if self.w.virt != PAST {
                let virt = &self.src.h.virt;
                let i = virt.partition_point(|x| x.0 < p) + self.w.virt as usize;
                if let Some(&(_, id)) = virt.get(i).filter(|x| x.0 == p) {
                    self.read_to = p;
                    if let Some(item) = self.styled_item(id, CellKind::Virtual) {
                        return Some(item);
                    }
                    (self.w.virt, self.w.off) = (self.w.virt + 1, 0);
                    continue;
                }
                self.w.virt = PAST;
            }
            if let Some(s) = span {
                self.read_to = s.end;
                let item = s
                    .repl
                    .and_then(|id| self.styled_item(id, CellKind::Replacement));
                if item.is_some() {
                    return item;
                }
                self.w = Walk::at(s.end, self.w.vcol);
                continue;
            }
            let t = self.src.t;
            if p >= self.run_end {
                if p >= t.len() || t.rope().byte(p) == b'\n' {
                    self.read_to = p;
                    return None;
                }
                self.run_end = t.run_bounds(&self.src.h, p).1;
            }
            return Some(self.text_item(p));
        }
    }

    fn item(&self, text: &str, kind: CellKind, face: Option<&'static str>) -> Item {
        let (c, at) = (text.chars().next().unwrap_or(' '), self.w);
        let tab = c == '\t' && kind == CellKind::Text;
        let control = (c.is_control() && !tab).then_some(c);
        let width = match control {
            Some(c) => control_cells(c).len(),
            None if tab => self.src.tab - at.vcol % self.src.tab,
            None => text.width().max(1),
        };
        let first = LayoutCell {
            glyph: Glyph::of(text),
            pos: at.pos,
            vcol: at.vcol,
            kind: if tab { CellKind::Tab } else { kind },
            face,
        };
        Item {
            first,
            stop: kind == CellKind::Text || (kind == CellKind::Replacement && at.off == 0),
            width,
            whitespace: tab || c == ' ',
            control,
            at,
        }
    }

    /// The next cluster of a virtual text or replacement, if any is left.
    fn styled_item(&mut self, id: DecorationId, kind: CellKind) -> Option<Item> {
        let s = match &self.src.t.decoration(id)?.deco {
            Deco::Virtual(s)
            | Deco::Hide {
                replacement: Some(s),
                ..
            } => s,
            _ => return None,
        };
        let off = self.w.off as usize;
        let text = s.text.get(off..)?.graphemes(true).next()?;
        let run = s.faces.iter().rev().find(|(r, _)| r.contains(&off));
        let item = self.item(text, kind, run.map(|(_, f)| *f));
        self.w.off += text.len() as u32;
        self.w.vcol += item.width;
        self.w.skip = 0;
        Some(item)
    }

    /// The cluster of visible text at `p`, which is a cluster's start.
    fn text_item(&mut self, p: usize) -> Item {
        let rope = self.src.t.rope();
        if p < self.chunk.1 || p - self.chunk.1 >= self.chunk.0.len() {
            (self.chunk.0, self.chunk.1, ..) = rope.chunk_at_byte(p);
        }
        let (i, b) = (p - self.chunk.1, self.chunk.0.as_bytes());
        // Two ASCII characters in a run are two clusters: only CR LF would join, and a run stops
        // at a newline.
        let ascii =
            b[i] < 0x80 && (p + 1 == self.run_end || b.get(i + 1).is_some_and(|c| *c < 0x80));
        let (e, text): (usize, Cow<str>) = if ascii {
            (p + 1, self.chunk.0[i..=i].into())
        } else {
            let run = rope.byte_slice(p..self.run_end);
            let e = p + grapheme(run, 0, Step::Next).unwrap_or(run.len_bytes());
            (e, rope.byte_slice(p..e).into())
        };
        let item = self.item(&text, CellKind::Text, None);
        self.w = Walk::at(e, self.w.vcol + item.width);
        self.read_to = e;
        item
    }
}

/// What to ask a grapheme cursor.
#[derive(Clone, Copy)]
pub(crate) enum Step {
    Next,
    Prev,
    Is,
}

/// The next or previous cluster boundary from `at` in a run, or `at` if it is one.
pub(crate) fn grapheme(run: RopeSlice, at: usize, step: Step) -> Option<usize> {
    let (mut chunk, mut chunk_at, ..) = run.chunk_at_byte(at);
    let mut gc = GraphemeCursor::new(at, run.len_bytes(), true);
    loop {
        let r = match step {
            Step::Next => gc.next_boundary(chunk, chunk_at),
            Step::Prev => gc.prev_boundary(chunk, chunk_at),
            Step::Is => gc.is_boundary(chunk, chunk_at).map(|b| b.then_some(at)),
        };
        match r {
            Ok(b) => return b,
            Err(GraphemeIncomplete::NextChunk) => {
                chunk_at += chunk.len();
                chunk = run.chunk_at_byte(chunk_at).0;
            }
            Err(GraphemeIncomplete::PrevChunk) => {
                (chunk, chunk_at, ..) = run.chunk_at_byte(chunk_at - 1);
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (c, s, ..) = run.chunk_at_byte(n - 1);
                gc.provide_context(c, s);
            }
            Err(GraphemeIncomplete::InvalidOffset) => return None,
        }
    }
}

// ---------------------------------------------------------------- one row

/// How a display line breaks into rows: whether at word boundaries, and how wide they are.
type Mode = (bool, usize);
/// Rows at this width find a display column or an unwrapped position in a few steps, whatever the
/// wrap setting: a line's columns don't depend on the width they're found at.
const CHECKPOINTS: Mode = (false, 256);
/// One row, however long.
const UNBROKEN: Mode = (false, usize::MAX);

/// What `Src::place` reports about a row.
struct RowOut {
    row: usize,
    want_stops: bool,
    want_cells: bool,
    cells: Vec<LayoutCell>,
    stops: Vec<Stop>,
    line_end: Option<Stop>,
}

impl RowOut {
    fn new(row: usize, want_stops: bool, want_cells: bool) -> Self {
        let (cells, stops, line_end) = (vec![], vec![], None);
        RowOut {
            row,
            want_stops,
            want_cells,
            cells,
            stops,
            line_end,
        }
    }
    fn stop(&mut self, pos: usize, vcol: usize, col: usize) -> Stop {
        let row = self.row;
        let s = Stop {
            pos,
            vcol,
            row,
            col,
        };
        if self.want_stops {
            self.stops.push(s);
        }
        s
    }
    fn put(&mut self, it: &Item, cells: Range<usize>, col: usize) {
        if cells.start == 0 && it.stop && !cells.is_empty() {
            self.stop(it.first.pos, it.at.vcol, col);
        }
        if self.want_cells {
            self.cells.extend(cells.map(|i| it.cell(i)));
        }
    }
    fn filler(&mut self, it: &Item, n: usize) {
        if self.want_cells {
            let mut cell = it.cell(0);
            (cell.glyph, cell.kind, cell.face) = (Glyph::Char(' '), CellKind::Filler, None);
            self.cells.resize(self.cells.len() + n, cell);
        }
    }
    /// Take back everything from column `col` on.
    fn rewind(&mut self, col: usize) {
        self.cells.truncate(col);
        self.stops
            .truncate(self.stops.partition_point(|s| s.col < col));
    }
}

impl Src<'_> {
    /// Place one row from `start`, by "Where soft wrap breaks". Returns where the next row starts
    /// (None if the line ends on this one) and how far along the text that decision read.
    fn place(
        &self,
        start: Walk,
        (word, width): Mode,
        limit: usize,
        out: &mut RowOut,
    ) -> (Option<Walk>, usize) {
        let mut wk = Walker {
            src: self,
            w: start,
            run_end: 0,
            chunk: ("", 0),
            read_to: start.pos,
        };
        let (mut col, mut after_ws, mut brk) = (0, false, None);
        loop {
            let Some(it) = wk.next() else {
                out.line_end = Some(out.stop(wk.w.pos, wk.w.vcol, col));
                return (None, wk.read_to);
            };
            if col >= limit {
                return (None, wk.read_to);
            }
            let skip = it.at.skip as usize;
            if word && after_ws && !it.whitespace && col > 0 {
                brk = Some((it.at, col));
            }
            if col + it.width - skip <= width {
                out.put(&it, skip..it.width, col);
                col += it.width - skip;
                after_ws = it.whitespace;
            } else if let Some((at, c)) = brk {
                out.rewind(c);
                return (Some(at), wk.read_to);
            } else if it.splits() {
                let done = skip + width - col;
                out.put(&it, skip..done, col);
                let skip = done as u32;
                return (Some(Walk { skip, ..it.at }), wk.read_to);
            } else if col == 0 {
                // Wider than a row: it stays, and the row counts as full.
                out.put(&it, 0..width, 0);
                (col, after_ws) = (width, it.whitespace);
            } else {
                out.filler(&it, width - col);
                return (Some(it.at), wk.read_to);
            }
        }
    }
}

// ---------------------------------------------------------------- the cache

/// A display line's rows, as far as they've been asked for. Each row is where it starts and how far
/// along the text its end was decided from: an edit before there can change it. Positions count
/// from the line's start, so a line moved along by an edit before it stays good.
#[derive(Debug)]
struct Line {
    rows: Vec<(Walk, usize)>,
    /// Where the line ends, once its rows reach there.
    end: Option<usize>,
}

/// How many rows start at or before the first cell at `rel`.
fn rows_to_stop(rows: &[(Walk, usize)], rel: usize) -> usize {
    rows.partition_point(|(w, _)| w.pos < rel || (w.pos == rel && !w.past_first()))
}

/// Past this many display lines, the cache starts again. An edit moves every line after it, so a
/// bound on them bounds the edit's cost.
const CACHED_LINES: usize = 512;

/// The per-line cache. Text edits keep what they can (`edited`). A change to a Hide or virtual
/// text empties it, and so does a new tab width.
#[derive(Default)]
pub(crate) struct LayoutCache {
    tab: usize,
    pub(crate) hidden: Option<Rc<Hidden>>,
    lines: BTreeMap<(usize, Mode), Line>,
}

impl LayoutCache {
    /// Bytes `a..b` were replaced by text ending at `c`.
    pub(crate) fn edited(&mut self, a: usize, b: usize, c: usize) {
        self.hidden = None;
        let tail = self.lines.split_off(&(b + 1, (false, 0)));
        self.lines.retain(|&(s, _), l| {
            if s <= a && l.end.is_none_or(|e| s + e >= a) {
                let i = l.rows.partition_point(|(_, read_to)| *read_to < a - s);
                l.rows.truncate(i + 1);
                l.end = None;
            }
            s <= a
        });
        self.lines
            .extend(tail.into_iter().map(|((s, m), l)| ((s - b + c, m), l)));
    }
    /// A Hide or virtual text changed.
    pub(crate) fn clear(&mut self) {
        *self = LayoutCache::default();
    }
}

impl TextBuf {
    fn src(&self) -> Src<'_> {
        let tab = self.settings.tab_width.max(1);
        Src {
            t: self,
            h: self.hidden(),
            tab,
        }
    }

    /// Run `f` on a display line's rows, laid out until `enough` says that will do.
    fn with_rows<R>(
        &self,
        s: usize,
        mode: Mode,
        enough: impl Fn(&[(Walk, usize)]) -> bool,
        f: impl FnOnce(&Line) -> R,
    ) -> R {
        let src = self.src();
        let mut c = self.cache.borrow_mut();
        if c.tab != self.settings.tab_width {
            c.tab = self.settings.tab_width;
            c.lines.clear();
        }
        if c.lines.len() >= CACHED_LINES && !c.lines.contains_key(&(s, mode)) {
            c.lines.clear();
        }
        let line = c.lines.entry((s, mode)).or_insert_with(|| Line {
            rows: vec![(Walk::at(0, 0), usize::MAX)],
            end: None,
        });
        while line.end.is_none() && !enough(&line.rows) {
            let last = line.rows.len() - 1;
            let from = line.rows[last].0.moved(0, s);
            match src.place(from, mode, usize::MAX, &mut RowOut::new(0, false, false)) {
                (Some(n), read_to) => {
                    line.rows[last].1 = read_to - s;
                    line.rows.push((n.moved(s, 0), usize::MAX));
                }
                (None, read_to) => line.end = Some(read_to - s),
            }
        }
        f(line)
    }
    /// Where row `r` starts, if the display line has that many.
    fn row_walk(&self, s: usize, mode: Mode, r: usize) -> Option<Walk> {
        let found = |l: &Line| l.rows.get(r).map(|x| x.0.moved(0, s));
        self.with_rows(s, mode, |rows| rows.len() > r, found)
    }
    /// The cursor positions on a row, and whether another row follows it.
    fn stops_of(&self, s: usize, mode: Mode, r: usize) -> (Vec<Stop>, bool) {
        let mut out = RowOut::new(r, true, false);
        let from = self.row_walk(s, mode, r);
        let next = from.and_then(|w| self.src().place(w, mode, usize::MAX, &mut out).0);
        (out.stops, next.is_some())
    }

    // ---- cursor positions: none of these lay anything out

    /// Is `pos` a position the cursor can occupy (contract §9, "Cursor positions")?
    pub fn is_stop(&self, pos: usize) -> bool {
        if !self.is_boundary(pos) {
            return false;
        }
        let h = self.hidden();
        match h.covering(pos) {
            Some(s) => s.start == pos && s.repl.is_some(),
            None => pos == self.len() || self.rope().byte(pos) == b'\n' || self.is_cluster(&h, pos),
        }
    }
    fn is_cluster(&self, h: &Hidden, pos: usize) -> bool {
        let (rs, re) = self.run_bounds(h, pos);
        let run = self.rope().byte_slice(rs..re);
        pos == rs || grapheme(run, pos - rs, Step::Is).is_some()
    }
    /// The first cursor position at or after `p`, which is on a character boundary. There's
    /// always one on `p`'s display line.
    fn stop_after(&self, h: &Hidden, mut p: usize) -> usize {
        loop {
            if let Some(s) = h.covering(p) {
                if s.start == p && s.repl.is_some() {
                    return p;
                }
                p = s.end;
            } else if p == self.len() || self.rope().byte(p) == b'\n' || self.is_cluster(h, p) {
                return p;
            } else {
                let (rs, re) = self.run_bounds(h, p);
                let run = self.rope().byte_slice(rs..re);
                p = rs + grapheme(run, p - rs, Step::Next).unwrap_or(re - rs);
            }
        }
    }
    /// The last cursor position before `c` on its display line, if there is one.
    pub(crate) fn stop_before(&self, h: &Hidden, mut c: usize) -> Option<usize> {
        let first = self.dline_start(c);
        while c > first {
            match h.last_le(c - 1).filter(|s| c <= s.end) {
                Some(s) if s.repl.is_some() => return Some(s.start),
                Some(s) => c = s.start,
                None => {
                    let (rs, re) = self.run_bounds(h, c - 1);
                    let run = self.rope().byte_slice(rs..re);
                    return grapheme(run, c - rs, Step::Prev).map(|b| rs + b);
                }
            }
        }
        None
    }
    /// The nearest cursor position. With no direction: the fold line if inside a Hide with a
    /// replacement, else the next visible position. Going back, it stays on the position's display
    /// line, and if that has none before it, takes its first.
    pub fn snap(&self, pos: usize, dir: Option<Dir>) -> usize {
        let pos = pos.min(self.len());
        if self.is_stop(pos) {
            return pos;
        }
        // The next character boundary: `pos` may be inside a character.
        let r = self.rope();
        let c = r.byte_to_char(pos);
        let c = r.char_to_byte(c + usize::from(r.char_to_byte(c) != pos));
        let h = self.hidden();
        match dir {
            Some(Dir::Backward) => self.stop_before(&h, c),
            Some(Dir::Forward) => None,
            None => h
                .covering(pos)
                .filter(|s| s.repl.is_some())
                .map(|s| s.start),
        }
        .unwrap_or_else(|| self.stop_after(&h, c))
    }
    /// The next line that isn't hidden: the first logical line of the next (or previous) display line.
    pub fn next_visible(&self, line: usize, dir: Dir) -> Option<usize> {
        let s = self.dline_start(self.line_start(line));
        let e = self.line_end_of(&self.hidden(), s);
        match dir {
            Dir::Forward => (e < self.len()).then(|| self.line_of(e + 1)),
            Dir::Backward => (s > 0).then(|| self.line_of(self.dline_start(s - 1))),
        }
    }

    // ---- queries

    /// The display line containing `pos`, unwrapped. Getting its cursor positions walks all of it,
    /// so `is_stop`, `snap`, `display_col` and `pos_at_vcol` are cheaper for what they answer.
    pub fn display_line(&self, pos: usize) -> DisplayLine {
        let start = self.dline_start(pos);
        let e = self.line_end_of(&self.hidden(), start);
        let mut out = RowOut::new(0, true, false);
        self.src()
            .place(Walk::at(start, 0), UNBROKEN, usize::MAX, &mut out);
        DisplayLine {
            start,
            end: e + usize::from(e < self.len()),
            lines: self.line_of(start)..=self.line_of(e),
            eof: e == self.len(),
            stops: out.stops,
        }
    }
    /// Where the cursor position at or before `pos` is, in this mode: its row and stop.
    fn locate(&self, pos: usize, m: Option<Mode>) -> (Row, Stop) {
        let pos = self.snap(pos, Some(Dir::Backward));
        let s = self.dline_start(pos);
        let mode = m.unwrap_or(CHECKPOINTS);
        let (r, w) = self.with_rows(
            s,
            mode,
            |rows| rows_to_stop(rows, pos - s) < rows.len(),
            |l| {
                let r = rows_to_stop(&l.rows, pos - s).saturating_sub(1);
                (r, l.rows[r].0.moved(0, s))
            },
        );
        let mut out = RowOut::new(r, true, false);
        self.src().place(w, mode, usize::MAX, &mut out);
        let i = out
            .stops
            .partition_point(|st| st.pos <= pos)
            .saturating_sub(1);
        let st = out.stops.get(i).copied();
        let st = st.unwrap_or_else(|| out.stop(pos, w.vcol, 0));
        match m {
            Some(_) => ((s, r), st),
            None => (
                (s, 0),
                Stop {
                    row: 0,
                    col: st.vcol,
                    ..st
                },
            ),
        }
    }
    /// The display column of `pos`, independent of wrapping (for `j`/`k` goals and block selections).
    pub fn display_col(&self, pos: usize) -> usize {
        self.locate(pos, None).1.vcol
    }
    /// The first cursor position at or after display column `vcol` on the display line holding
    /// `line`, or else that display line's last. A column inside a tab, a wide character or a
    /// replacement lands after it, as Emacs's `move-to-column` does for Evil's `j` and `k`.
    pub fn pos_at_vcol(&self, line: usize, vcol: usize) -> usize {
        self.first_stop_from(self.dline_start(self.line_start(line)), vcol)
    }
    fn first_stop_from(&self, s: usize, vcol: usize) -> usize {
        let past = |rows: &[(Walk, usize)]| rows.partition_point(|(w, _)| w.vcol <= vcol);
        let mut r = self.with_rows(
            s,
            CHECKPOINTS,
            |rows| past(rows) < rows.len(),
            |l| past(&l.rows).saturating_sub(1),
        );
        loop {
            let (stops, more) = self.stops_of(s, CHECKPOINTS, r);
            if let Some(st) = stops.iter().find(|st| st.vcol >= vcol) {
                return st.pos;
            }
            if !more {
                return stops.last().map_or(s, |st| st.pos);
            }
            r += 1;
        }
    }

    /// Where window `w`'s text goes, or None if `w` doesn't show this buffer.
    pub fn geometry(&self, w: WindowId) -> Option<Geometry> {
        let v = self.views.get(&w)?;
        let empty_last = self.is_empty() || self.rope().byte(self.len() - 1) == b'\n';
        let numbered = self.line_count() - usize::from(empty_last);
        let gutter = match self.settings.line_numbers {
            LineNumbers::Off => 0,
            _ => numbered.to_string().len().max(3) + 1,
        };
        let avail = v.cols.saturating_sub(gutter).max(2);
        let text_w = self.settings.max_width.map_or(avail, |m| m.clamp(2, avail));
        let pad = if self.settings.center {
            (avail - text_w) / 2
        } else {
            0
        };
        Some(Geometry {
            cols: v.cols,
            rows: v.rows,
            pad,
            gutter,
            text_x: pad + gutter,
            text_w,
        })
    }
    fn mode(&self, g: &Geometry) -> Option<Mode> {
        let wrap = self.settings.wrap;
        (wrap != Wrap::Off).then_some((wrap == Wrap::Word, g.text_w))
    }
    fn next_row(&self, (s, r): Row, m: Option<Mode>) -> Option<Row> {
        if m.is_some_and(|m| self.row_walk(s, m, r + 1).is_some()) {
            return Some((s, r + 1));
        }
        let e = self.line_end_of(&self.hidden(), s);
        (e < self.len()).then_some((e + 1, 0))
    }
    fn prev_row(&self, (s, r): Row, m: Option<Mode>) -> Option<Row> {
        if r > 0 {
            return Some((s, r - 1));
        }
        let p = self.dline_start(s.checked_sub(1)?);
        let last = |mode| self.with_rows(p, mode, |_| false, |l| l.rows.len() - 1);
        Some((p, m.map_or(0, last)))
    }
    fn top(&self, w: WindowId, m: Option<Mode>) -> Row {
        let v = self.views[&w];
        let want = v.top_row;
        let last = |l: &Line| want.min(l.rows.len() - 1);
        let r = m.map_or(0, |mode| {
            self.with_rows(v.top, mode, |r| r.len() > want, last)
        });
        (v.top, r)
    }
    /// Row `b`'s distance below row `a`, if it isn't above it and is within `limit`.
    fn rows_between(&self, a: Row, b: Row, m: Option<Mode>, limit: usize) -> Option<usize> {
        if b < a {
            return None;
        }
        if a.0 == b.0 {
            return Some(b.1 - a.1);
        }
        let (mut cur, mut n) = (a, 0);
        while cur != b && n <= limit {
            cur = self.next_row(cur, m)?;
            n += 1;
        }
        Some(n)
    }

    /// Move by screen rows, for `gj`/`gk`. Returns the new position and the goal column used.
    pub fn move_visual(
        &self,
        w: WindowId,
        pos: usize,
        dy: isize,
        goal: Option<usize>,
    ) -> Option<(usize, usize)> {
        let g = self.geometry(w)?;
        let m = self.mode(&g);
        let (mut cur, st) = self.locate(pos, m);
        let goal = goal.unwrap_or(st.col);
        let (mut best, mut n) = (None, dy.unsigned_abs());
        while n > 0 {
            let next = if dy > 0 {
                self.next_row(cur, m)
            } else {
                self.prev_row(cur, m)
            };
            let Some(r) = next else { break };
            cur = r;
            // Rows with no cursor position (a wrapped replacement's tail) are skipped, not counted.
            if m.is_none_or(|mode| !self.stops_of(cur.0, mode, cur.1).0.is_empty()) {
                best = Some(cur);
                n -= 1;
            }
        }
        let Some((s, r)) = best else {
            return Some((pos, goal));
        };
        let Some(mode) = m else {
            // The stop before the first one past the goal, if it's past.
            let q = self.first_stop_from(s, goal + 1);
            let left = (self.display_col(q) > goal).then(|| self.stop_before(&self.hidden(), q));
            return Some((left.flatten().unwrap_or(q), goal));
        };
        let (stops, _) = self.stops_of(s, mode, r);
        let st = stops
            .iter()
            .rev()
            .find(|st| st.col <= goal)
            .or(stops.first());
        Some((st.map_or(pos, |st| st.pos), goal))
    }

    /// Screen (row, col) of `pos` in a window, or None if it is scrolled out of view.
    pub fn screen_pos(&self, w: WindowId, pos: usize) -> Option<(usize, usize)> {
        let g = self.geometry(w)?;
        let m = self.mode(&g);
        let (row, st) = self.locate(pos, m);
        let n = self.rows_between(self.top(w, m), row, m, g.rows)?;
        (n < g.rows).then_some((n, g.text_x + st.col))
    }

    /// The rows a window shows: what render draws, and what H/M/L and scrolling read.
    pub fn viewport(&self, w: WindowId) -> Option<Viewport> {
        let g = self.geometry(w)?;
        let m = self.mode(&g);
        let (top, src) = (self.top(w, m), self.src());
        let (mut cur, mut rows) = (top, vec![]);
        loop {
            let (s, r) = cur;
            let (mode, limit, from) = match m {
                Some(mode) => (mode, usize::MAX, self.row_walk(s, mode, r)),
                None => (UNBROKEN, g.cols, Some(Walk::at(s, 0))),
            };
            let mut out = RowOut::new(r, true, true);
            if let Some(from) = from {
                src.place(from, mode, limit, &mut out);
            }
            rows.push(ViewRow {
                dline_start: s,
                row: r,
                first_line: self.line_of(s),
                number: None,
                current: false,
                cells: out.cells,
                line_end: out.line_end,
                stops: out.stops,
            });
            match self.next_row(cur, m).filter(|_| rows.len() < g.rows) {
                Some(n) => cur = n,
                None => break,
            }
        }
        self.number_rows(w, m, top, &mut rows);
        let (top, top_row) = top;
        Some(Viewport {
            geometry: g,
            top,
            top_row,
            rows,
        })
    }

    /// Contract §9, "Line numbers". The cursor's distances are counted from the first row shown,
    /// so a cursor off-screen costs a walk to it.
    fn number_rows(&self, w: WindowId, m: Option<Mode>, top: Row, rows: &mut [ViewRow]) {
        use LineNumbers::*;
        let style = self.settings.line_numbers;
        if style == Off {
            return;
        }
        let (cur, _) = self.locate(self.views[&w].sel.head, m);
        let cur_row = match self.rows_between(top, cur, m, usize::MAX) {
            Some(n) => n as isize,
            None => -(self.rows_between(cur, top, m, usize::MAX).unwrap_or(0) as isize),
        };
        // Display lines from the top one to the cursor's, negative if it's above.
        let (lo, hi) = (top.0.min(cur.0), top.0.max(cur.0));
        let (mut p, mut cur_line) = (lo, 0isize);
        while style == Relative && p < hi {
            p = self.line_end_of(&self.hidden(), p) + 1;
            cur_line += if cur.0 > top.0 { 1 } else { -1 };
        }
        let mut line = 0isize;
        for (i, vr) in rows.iter_mut().enumerate() {
            line += isize::from(i > 0 && vr.row == 0);
            let (own, on_cursor) = (vr.first_line + 1, i as isize == cur_row);
            vr.current = vr.dline_start == cur.0 && (style != Visual || on_cursor);
            vr.number = match style {
                _ if vr.dline_start == self.len() => None,
                Off => None,
                Absolute => (vr.row == 0).then_some(own),
                Relative if line == cur_line => (vr.row == 0).then_some(own),
                Relative => (vr.row == 0).then_some(line.abs_diff(cur_line)),
                Visual if on_cursor => Some(own),
                Visual => Some((i as isize).abs_diff(cur_row)),
            };
        }
    }

    /// Where the display line at a window's top starts, and which of its rows is first on screen.
    /// None if `w` doesn't show this buffer.
    pub fn scroll(&self, w: WindowId) -> Option<(usize, usize)> {
        let g = self.geometry(w)?;
        Some(self.top(w, self.mode(&g)))
    }
    /// Show `top`'s display line from its row `row` first: `top` moves to its display line's start,
    /// and `row` stops at the line's last row. Core scrolls back to the cursor before the next
    /// render (C5), so a command that scrolls also leaves the cursor on a visible row. None if `w`
    /// doesn't show this buffer.
    pub fn set_scroll(&mut self, w: WindowId, top: usize, row: usize) -> Option<()> {
        let top = self.dline_start(top);
        let v = self.views.get_mut(&w)?;
        (v.top, v.top_row) = (top, row);
        self.views.get_mut(&w)?.top_row = self.scroll(w)?.1;
        Some(())
    }

    /// Scroll so the cursor is visible. Core does this before every render (C5).
    pub(crate) fn scroll_to_cursor(&mut self, w: WindowId) {
        let Some(g) = self.geometry(w) else { return };
        let m = self.mode(&g);
        let (cur, _) = self.locate(self.views[&w].sel.head, m);
        let top = self.top(w, m);
        let new_top = match self.rows_between(top, cur, m, g.rows) {
            None => cur,
            Some(n) if n >= g.rows => {
                let up = |t: Row, _| self.prev_row(t, m).unwrap_or(t);
                (1..g.rows.max(1)).fold(cur, up)
            }
            Some(_) => top,
        };
        if let Some(v) = self.views.get_mut(&w) {
            (v.top, v.top_row) = new_top;
        }
    }
}

/// Core's layout commands.
pub(crate) fn register_commands(reg: &mut Registry) {
    let doc =
        "Turn the buffer's maximum text width off, or back on at the width it had (80 if none)";
    reg.command("toggle-max-width", doc, |ctx, a| {
        let w = a.window.or(ctx.focused());
        let b = a.buffer.or(w.and_then(|w| ctx.window_buffer(w)));
        let b = b.ok_or_else(|| Error::Msg("No buffer".into()))?;
        let t = ctx.text_mut(b)?;
        match t.settings.max_width.take() {
            Some(m) => t.max_width_off = Some(m),
            None => t.settings.max_width = Some(t.max_width_off.unwrap_or(80)),
        }
        Ok(())
    });
}

/// Where a window's text goes: centered maximum width, gutter, text area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    /// The window's width in cells.
    pub cols: usize,
    /// The window's height in rows.
    pub rows: usize,
    /// Blank cells to the left of the gutter, when the buffer is centered.
    pub pad: usize,
    /// The gutter's width in cells, its blank last cell included. 0 with line numbers off.
    pub gutter: usize,
    /// The column the text starts at: `pad + gutter`.
    pub text_x: usize,
    /// The text area's width in cells.
    pub text_w: usize,
}

/// One row a window shows.
#[derive(Clone, Debug)]
pub struct ViewRow {
    /// Where its display line starts in the buffer, in bytes.
    pub dline_start: usize,
    /// Which row of that display line it is, from 0.
    pub row: usize,
    /// That display line's first logical line, from 0.
    pub first_line: usize,
    /// The number its gutter draws (contract §9, "Line numbers"), whole: the gutter may show only
    /// its last digits.
    pub number: Option<usize>,
    pub(crate) current: bool,
    pub(crate) cells: Vec<LayoutCell>,
    pub(crate) line_end: Option<Stop>,
    stops: Vec<Stop>,
}

impl ViewRow {
    /// The cursor positions on this row, left to right, where they're drawn. An unwrapped row
    /// lists only those up to the window's edge.
    pub fn stops(&self) -> &[Stop] {
        &self.stops
    }
}

/// What a window shows: what render draws, and what `H`, `M`, `L` and scrolling read.
#[derive(Clone, Debug)]
pub struct Viewport {
    /// Where the window's text goes.
    pub geometry: Geometry,
    /// Where the display line at the window's top starts in the buffer, in bytes.
    pub top: usize,
    /// Which row of that display line is the window's top row, from 0.
    pub top_row: usize,
    /// The rows shown, top to bottom. It can be fewer than the window's height.
    pub rows: Vec<ViewRow>,
}

type Row = (usize, usize);
