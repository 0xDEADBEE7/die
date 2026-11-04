//! The one layout function (C16). Render and motions both call `layout`, through one cache.
//! It handles soft wrap, centered maximum width, the gutter, tabs, wide characters,
//! stored Hide decorations (with or without replacement text) and virtual text.
#![warn(missing_docs)]
use crate::ids::*;
use crate::text::*;
use ropey::Rope;
use std::collections::HashMap;
use std::rc::Rc;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A direction along the text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// Toward the end of the text.
    Forward,
    /// Toward the start of the text.
    Backward,
}

/// A merged hidden range. With a replacement, `start` is a cursor position ("the fold line").
#[derive(Clone, Debug)]
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
    pub repl: Option<String>,
    pub seq: u64,
}

#[derive(Default, Debug)]
pub(crate) struct Hidden {
    spans: Vec<Span>, // sorted, non-overlapping
}

impl Hidden {
    /// Overlapping hides merge. The first-starting one wins, preferring one with a replacement,
    /// then the longest, then the one added first.
    pub fn build(mut v: Vec<Span>) -> Hidden {
        v.retain(|s| s.start < s.end);
        v.sort_by_key(|s| (s.start, s.repl.is_none(), std::cmp::Reverse(s.end), s.seq));
        let mut out: Vec<Span> = vec![];
        for s in v {
            match out.last_mut() {
                Some(l) if s.start < l.end => l.end = l.end.max(s.end),
                _ => out.push(s),
            }
        }
        Hidden { spans: out }
    }
    fn last_le(&self, q: usize) -> Option<&Span> {
        let i = self.spans.partition_point(|s| s.start <= q);
        i.checked_sub(1).map(|i| &self.spans[i])
    }
    /// Is the character at `q` hidden?
    pub fn covers(&self, q: usize) -> bool {
        self.last_le(q).is_some_and(|s| q < s.end)
    }
    fn starting_at(&self, p: usize) -> Option<&Span> {
        self.last_le(p).filter(|s| s.start == p)
    }
    fn next_start_after(&self, p: usize) -> Option<usize> {
        let i = self.spans.partition_point(|s| s.start <= p);
        self.spans.get(i).map(|s| s.start)
    }
    fn anchor_containing(&self, p: usize) -> Option<usize> {
        self.last_le(p)
            .filter(|s| s.repl.is_some() && s.start < p && p < s.end)
            .map(|s| s.start)
    }
}

/// What a laid-out cell shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellKind {
    /// A cell of the buffer's text.
    Text,
    /// A cell of a tab.
    Tab,
    /// A cell of a Hide's replacement.
    Replacement,
    /// A cell of virtual text.
    Virtual,
    /// Padding where a wide character didn't fit at the end of a row.
    Filler,
}

#[derive(Clone, Debug, PartialEq)]
/// One cell of a laid-out row: what layout and motions agree is on screen.
pub struct LayoutCell {
    /// The grapheme drawn here; "" for the second cell of a wide character.
    pub text: String,
    /// The buffer position this cell starts, if any.
    pub pos: Option<usize>,
    /// What the cell shows.
    pub kind: CellKind,
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
    /// Where it starts in the buffer, in bytes.
    pub start: usize,
    /// Where the next display line starts, or the end of the text for the last one.
    pub end: usize,
    /// The first logical line it holds, from 0.
    pub first_line: usize,
    /// The last logical line it holds, from 0.
    pub last_line: usize,
    /// Whether this is the last display line of the text.
    pub eof: bool,
    /// Its rows, for the width it was laid out at. Without wrapping, one row.
    pub rows: Vec<Vec<LayoutCell>>,
    /// Its cursor positions, in order. There's always at least one.
    pub stops: Vec<Stop>,
}

impl DisplayLine {
    /// The last stop at or before `pos` (or the first stop).
    pub fn stop_le(&self, pos: usize) -> Stop {
        let i = self.stops.partition_point(|s| s.pos <= pos);
        self.stops[i.saturating_sub(1)]
    }
    /// Whether `pos` is a cursor position on this line.
    pub fn is_stop(&self, pos: usize) -> bool {
        self.stops.binary_search_by_key(&pos, |s| s.pos).is_ok()
    }
}

struct Builder {
    width: Option<usize>,
    tab: usize,
    rows: Vec<Vec<LayoutCell>>,
    col: usize,
    vcol: usize,
    stops: Vec<Stop>,
}

impl Builder {
    fn fits(&self, n: usize) -> bool {
        self.width.is_none_or(|w| self.col + n <= w)
    }
    fn newrow(&mut self) {
        self.rows.push(vec![]);
        self.col = 0;
    }
    fn push(&mut self, text: &str, pos: Option<usize>, kind: CellKind) {
        if !self.fits(1) {
            self.newrow();
        }
        self.rows.last_mut().unwrap().push(LayoutCell {
            text: text.into(),
            pos,
            kind,
        });
        self.col += 1;
    }
    /// Place one grapheme. Returns the (row, col) of its first cell.
    fn put(&mut self, g: &str, pos: Option<usize>, kind: CellKind) -> (usize, usize) {
        if g == "\t" && kind == CellKind::Text {
            let n = self.tab - self.vcol % self.tab;
            if !self.fits(1) {
                self.newrow();
            }
            let at = (self.rows.len() - 1, self.col);
            for i in 0..n {
                self.push(" ", if i == 0 { pos } else { None }, CellKind::Tab);
            }
            self.vcol += n;
            return at;
        }
        let (g, w) = if g.chars().any(char::is_control) {
            ("?", 1)
        } else {
            (g, g.width().max(1))
        };
        if w > 1 && !self.fits(w) && self.col > 0 {
            while self.fits(1) {
                self.push(" ", None, CellKind::Filler);
            }
        }
        if !self.fits(1) {
            self.newrow();
        }
        let at = (self.rows.len() - 1, self.col);
        self.push(g, pos, kind);
        for _ in 1..w {
            self.rows.last_mut().unwrap().push(LayoutCell {
                text: String::new(),
                pos: None,
                kind,
            });
            self.col += 1;
        }
        self.vcol += w;
        at
    }
    fn stop(&mut self, pos: usize, vcol: usize, at: (usize, usize)) {
        self.stops.push(Stop {
            pos,
            vcol,
            row: at.0,
            col: at.1,
        });
    }
    /// A stop with no cell of its own: end of line or end of buffer.
    fn stop_here(&mut self, pos: usize) {
        if !self.fits(1) {
            self.newrow();
        }
        let at = (self.rows.len() - 1, self.col);
        self.stop(pos, self.vcol, at);
    }
}

/// THE layout function. Lays out the display line that starts at `start`.
pub(crate) fn layout(
    rope: &Rope,
    hidden: &Hidden,
    virt: &[(usize, String)],
    start: usize,
    width: Option<usize>,
    tab: usize,
) -> DisplayLine {
    let len = rope.len_bytes();
    let mut b = Builder {
        width,
        tab: tab.max(1),
        rows: vec![vec![]],
        col: 0,
        vcol: 0,
        stops: vec![],
    };
    let mut vi = virt.partition_point(|v| v.0 < start);
    let mut p = start;
    let emit_virt = |b: &mut Builder, vi: &mut usize, p: usize, show: bool| {
        while *vi < virt.len() && virt[*vi].0 <= p {
            if show && virt[*vi].0 == p {
                for g in virt[*vi].1.graphemes(true) {
                    b.put(g, None, CellKind::Virtual);
                }
            }
            *vi += 1;
        }
    };
    let (end, eof) = loop {
        if let Some(sp) = hidden.starting_at(p) {
            if let Some(r) = &sp.repl {
                emit_virt(&mut b, &mut vi, p, true);
                let v = b.vcol;
                let mut first = None;
                for g in r.graphemes(true) {
                    let at = b.put(
                        g,
                        if first.is_none() { Some(p) } else { None },
                        CellKind::Replacement,
                    );
                    first.get_or_insert(at);
                }
                b.stop(p, v, first.unwrap());
            } else {
                emit_virt(&mut b, &mut vi, p, false);
            }
            p = sp.end;
            continue;
        }
        emit_virt(&mut b, &mut vi, p, true);
        if p >= len {
            b.stop_here(p);
            break (len, true);
        }
        if rope.byte(p) == b'\n' {
            b.stop_here(p);
            break (p + 1, false);
        }
        let line = rope.byte_to_line(p);
        let line_end = if line + 1 < rope.len_lines() {
            rope.line_to_byte(line + 1) - 1
        } else {
            len
        };
        let run_end = hidden
            .next_start_after(p)
            .map_or(line_end, |s| s.min(line_end));
        let run = rope.byte_slice(p..run_end).to_string();
        for (off, g) in run.grapheme_indices(true) {
            let q = p + off;
            if off > 0 {
                emit_virt(&mut b, &mut vi, q, true);
            }
            let v = b.vcol;
            let at = b.put(g, Some(q), CellKind::Text);
            b.stop(q, v, at);
        }
        p = run_end;
    };
    DisplayLine {
        start,
        end,
        first_line: rope.byte_to_line(start),
        last_line: rope.byte_to_line(if eof { len } else { end - 1 }),
        eof,
        rows: b.rows,
        stops: b.stops,
    }
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

pub(crate) fn geometry(s: &Settings, line_count: usize, cols: usize, rows: usize) -> Geometry {
    let gutter = match s.line_numbers {
        LineNumbers::Off => 0,
        _ => line_count.to_string().len().max(3) + 1,
    };
    let avail = cols.saturating_sub(gutter).max(2);
    let text_w = s.max_width.map_or(avail, |m| m.clamp(2, avail));
    let pad = if s.center { (avail - text_w) / 2 } else { 0 };
    Geometry {
        cols,
        rows,
        pad,
        gutter,
        text_x: pad + gutter,
        text_w,
    }
}

#[derive(Default)]
pub(crate) struct LayoutCache {
    key: (u64, u64, usize),
    hidden: Option<Rc<Hidden>>,
    virt: Option<Rc<Vec<(usize, String)>>>,
    lines: HashMap<(Option<usize>, usize), Rc<DisplayLine>>,
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
    /// The row's cells, left to right.
    pub cells: Vec<LayoutCell>,
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

type Row = (Rc<DisplayLine>, usize);

impl TextBuf {
    fn cache(&self) -> std::cell::RefMut<'_, LayoutCache> {
        let key = (self.rev(), self.deco_gen, self.settings.tab_width);
        let mut c = self.cache.borrow_mut();
        if c.key != key {
            *c = LayoutCache {
                key,
                ..Default::default()
            };
        }
        c
    }
    pub(crate) fn hidden(&self) -> Rc<Hidden> {
        if let Some(h) = &self.cache().hidden {
            return h.clone();
        }
        let spans = self
            .decorations()
            .filter_map(|(_, d)| match &d.deco {
                Deco::Hide { replacement, .. } => Some(Span {
                    start: d.range.start,
                    end: d.range.end,
                    repl: replacement.clone(),
                    seq: d.seq,
                }),
                _ => None,
            })
            .collect();
        let h = Rc::new(Hidden::build(spans));
        self.cache().hidden = Some(h.clone());
        h
    }
    fn virt(&self) -> Rc<Vec<(usize, String)>> {
        if let Some(v) = &self.cache().virt {
            return v.clone();
        }
        let mut v: Vec<(u64, usize, String)> = self
            .decorations()
            .filter_map(|(_, d)| match &d.deco {
                Deco::Virtual(t) => Some((d.seq, d.range.start, t.clone())),
                _ => None,
            })
            .collect();
        v.sort_by_key(|x| (x.1, x.0));
        let v = Rc::new(v.into_iter().map(|x| (x.1, x.2)).collect::<Vec<_>>());
        self.cache().virt = Some(v.clone());
        v
    }

    /// The start of the display line containing `pos`.
    pub(crate) fn dline_start(&self, pos: usize) -> usize {
        let h = self.hidden();
        let mut l = self.line_of(pos);
        loop {
            let ls = self.rope().line_to_byte(l);
            if l == 0 || !h.covers(ls - 1) {
                return ls;
            }
            l -= 1;
        }
    }
    fn dl_at(&self, start: usize, width: Option<usize>) -> Rc<DisplayLine> {
        if let Some(d) = self.cache().lines.get(&(width, start)) {
            return d.clone();
        }
        let (h, v) = (self.hidden(), self.virt());
        let d = Rc::new(layout(
            self.rope(),
            &h,
            &v,
            start,
            width,
            self.settings.tab_width,
        ));
        self.cache().lines.insert((width, start), d.clone());
        d
    }
    fn dl(&self, pos: usize, width: Option<usize>) -> Rc<DisplayLine> {
        self.dl_at(self.dline_start(pos.min(self.len())), width)
    }

    /// The unwrapped display line containing `pos` (width-independent).
    pub fn display_line(&self, pos: usize) -> Rc<DisplayLine> {
        self.dl(pos, None)
    }
    /// Is `pos` a position the cursor can occupy (not inside hidden text)?
    pub fn is_stop(&self, pos: usize) -> bool {
        pos <= self.len() && self.dl(pos, None).is_stop(pos)
    }
    /// The nearest cursor position. With no direction: the fold line if inside a Hide with a
    /// replacement, else the next visible position.
    pub fn snap(&self, pos: usize, dir: Option<Dir>) -> usize {
        let pos = pos.min(self.len());
        let dl = self.dl(pos, None);
        if dl.is_stop(pos) {
            return pos;
        }
        let i = dl.stops.partition_point(|s| s.pos < pos);
        let after = dl.stops.get(i).map(|s| s.pos);
        let before = i.checked_sub(1).map(|i| dl.stops[i].pos);
        let r = match dir {
            Some(Dir::Backward) => before.or(after),
            Some(Dir::Forward) => after.or(before),
            None => self.hidden().anchor_containing(pos).or(after).or(before),
        };
        r.unwrap_or(pos)
    }
    /// The next line that isn't hidden: the first logical line of the next (or previous) display line.
    pub fn next_visible(&self, line: usize, dir: Dir) -> Option<usize> {
        let dl = self.dl(self.line_start(line), None);
        match dir {
            Dir::Forward => (!dl.eof).then(|| self.line_of(dl.end)),
            Dir::Backward => (dl.start > 0).then(|| self.dl(dl.start - 1, None).first_line),
        }
    }
    /// The display column of `pos`, independent of wrapping (for `j`/`k` goals and block selections).
    pub fn display_col(&self, pos: usize) -> usize {
        self.dl(pos, None).stop_le(pos).vcol
    }
    /// The cursor position at a display column on `line`'s display line.
    pub fn pos_at_vcol(&self, line: usize, vcol: usize) -> usize {
        let dl = self.dl(self.line_start(line), None);
        let i = dl.stops.partition_point(|s| s.vcol <= vcol);
        dl.stops[i.saturating_sub(1)].pos
    }

    /// Where window `w`'s text goes, or None if `w` doesn't show this buffer.
    pub fn geometry(&self, w: WindowId) -> Option<Geometry> {
        let v = self.views.get(&w)?;
        Some(geometry(&self.settings, self.line_count(), v.cols, v.rows))
    }
    fn wrap(&self, g: &Geometry) -> Option<usize> {
        self.settings.wrap.then_some(g.text_w)
    }
    fn next_row(&self, (d, r): &Row, w: Option<usize>) -> Option<Row> {
        if r + 1 < d.rows.len() {
            Some((d.clone(), r + 1))
        } else if !d.eof {
            Some((self.dl_at(d.end, w), 0))
        } else {
            None
        }
    }
    fn prev_row(&self, (d, r): &Row, w: Option<usize>) -> Option<Row> {
        if *r > 0 {
            Some((d.clone(), r - 1))
        } else if d.start > 0 {
            let p = self.dl(d.start - 1, w);
            let n = p.rows.len() - 1;
            Some((p, n))
        } else {
            None
        }
    }
    fn top(&self, w: WindowId, width: Option<usize>) -> Row {
        let v = self.views[&w];
        let d = self.dl(v.top, width);
        let r = if d.start == v.top {
            v.top_row.min(d.rows.len() - 1)
        } else {
            0
        };
        (d, r)
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
        let width = self.wrap(&g);
        let d = self.dl(pos, width);
        let st = d.stop_le(pos);
        let goal = goal.unwrap_or(st.col);
        let mut cur: Row = (d, st.row);
        let mut best: Option<Row> = None;
        let mut n = dy.unsigned_abs();
        while n > 0 {
            let nx = if dy > 0 {
                self.next_row(&cur, width)
            } else {
                self.prev_row(&cur, width)
            };
            let Some(r) = nx else { break };
            cur = r;
            // Rows with no cursor position (a wrapped replacement's tail) are skipped, not counted.
            if cur.0.stops.iter().any(|s| s.row == cur.1) {
                best = Some(cur.clone());
                n -= 1;
            }
        }
        let Some((d, r)) = best else {
            return Some((pos, goal));
        };
        let in_row: Vec<&Stop> = d.stops.iter().filter(|s| s.row == r).collect();
        let s = in_row
            .iter()
            .rev()
            .find(|s| s.col <= goal)
            .unwrap_or(&in_row[0]);
        Some((s.pos, goal))
    }

    /// Screen (row, col) of `pos` in a window, or None if it is scrolled out of view.
    pub fn screen_pos(&self, w: WindowId, pos: usize) -> Option<(usize, usize)> {
        let g = self.geometry(w)?;
        let width = self.wrap(&g);
        let d = self.dl(pos, width);
        let st = d.stop_le(pos);
        let n = self.rows_between(self.top(w, width), (d, st.row), width, g.rows)?;
        (n < g.rows).then_some((n, g.text_x + st.col))
    }
    /// Screen rows from `a` down to `b`, if `b` is not above `a` and within `limit`.
    fn rows_between(&self, a: Row, b: Row, w: Option<usize>, limit: usize) -> Option<usize> {
        if (b.0.start, b.1) < (a.0.start, a.1) {
            return None;
        }
        let (mut cur, mut n) = (a, 0);
        while (cur.0.start, cur.1) != (b.0.start, b.1) {
            cur = self.next_row(&cur, w)?;
            n += 1;
            if n > limit {
                return Some(n);
            }
        }
        Some(n)
    }

    /// The rows a window shows: what render draws, and what H/M/L and scrolling read.
    pub fn viewport(&self, w: WindowId) -> Option<Viewport> {
        let g = self.geometry(w)?;
        let width = self.wrap(&g);
        let mut cur = self.top(w, width);
        let (top, top_row) = (cur.0.start, cur.1);
        let mut rows = vec![];
        loop {
            let (d, r) = &cur;
            rows.push(ViewRow {
                dline_start: d.start,
                row: *r,
                first_line: d.first_line,
                cells: d.rows[*r].clone(),
            });
            if rows.len() >= g.rows {
                break;
            }
            match self.next_row(&cur, width) {
                Some(n) => cur = n,
                None => break,
            }
        }
        Some(Viewport {
            geometry: g,
            top,
            top_row,
            rows,
        })
    }

    /// Scroll so the cursor is visible. Core does this before every render (C5).
    pub(crate) fn scroll_to_cursor(&mut self, w: WindowId) {
        let Some(g) = self.geometry(w) else { return };
        let width = self.wrap(&g);
        let head = self.views[&w].sel.head;
        let d = self.dl(head, width);
        let st = d.stop_le(head);
        let cur: Row = (d, st.row);
        let top = self.top(w, width);
        let new_top = match self.rows_between(top.clone(), cur.clone(), width, g.rows) {
            None => cur,
            Some(n) if n >= g.rows => {
                let mut t = cur;
                for _ in 1..g.rows.max(1) {
                    match self.prev_row(&t, width) {
                        Some(p) => t = p,
                        None => break,
                    }
                }
                t
            }
            Some(_) => top,
        };
        let v = self.views.get_mut(&w).unwrap();
        v.top = new_top.0.start;
        v.top_row = new_top.1;
    }
}
