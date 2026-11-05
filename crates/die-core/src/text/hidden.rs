//! What a buffer's decorations do to its text before any layout: hidden ranges and display-line
//! edges (contract §9). None of this lays anything out, so it costs the same whatever a line's
//! length: it reads the rope around a position.
use super::*;

/// A merged hidden range. With a replacement, `start` is a cursor position ("the fold line").
#[derive(Clone, Debug)]
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
    pub repl: Option<DecorationId>,
    pub seq: u64,
}

/// The decorations that shape rows, in the order layout reads them.
#[derive(Default, Debug)]
pub(crate) struct Hidden {
    spans: Vec<Span>, // sorted, non-overlapping
    /// Virtual text, by position and then in the order it was added.
    pub virt: Vec<(usize, DecorationId)>,
}

impl Hidden {
    /// Overlapping hides merge. The first-starting one wins, preferring one with a replacement,
    /// then the longest, then the one added first.
    fn merge(mut v: Vec<Span>) -> Vec<Span> {
        v.retain(|s| s.start < s.end);
        v.sort_by_key(|s| (s.start, s.repl.is_none(), std::cmp::Reverse(s.end), s.seq));
        let mut out: Vec<Span> = vec![];
        for s in v {
            match out.last_mut() {
                Some(l) if s.start < l.end => l.end = l.end.max(s.end),
                _ => out.push(s),
            }
        }
        out
    }
    pub fn last_le(&self, q: usize) -> Option<&Span> {
        let i = self.spans.partition_point(|s| s.start <= q);
        i.checked_sub(1).map(|i| &self.spans[i])
    }
    /// The hidden range the character at `q` is in.
    pub fn covering(&self, q: usize) -> Option<&Span> {
        self.last_le(q).filter(|s| q < s.end)
    }
    pub fn starting_at(&self, p: usize) -> Option<&Span> {
        self.last_le(p).filter(|s| s.start == p)
    }
}

impl TextBuf {
    pub(crate) fn hidden(&self) -> Rc<Hidden> {
        if let Some(h) = &self.cache.borrow().hidden {
            return h.clone();
        }
        let (mut spans, mut virt) = (vec![], vec![]);
        for (id, d) in self.decorations() {
            match &d.deco {
                Deco::Hide { replacement, .. } => spans.push(Span {
                    start: d.range.start,
                    end: d.range.end,
                    repl: replacement.as_ref().map(|_| id),
                    seq: d.seq,
                }),
                Deco::Virtual(_) => virt.push((d.seq, d.range.start, id)),
                _ => {}
            }
        }
        virt.sort_by_key(|x| (x.1, x.0));
        let virt = virt.into_iter().map(|x| (x.1, x.2)).collect();
        let spans = Hidden::merge(spans);
        let h = Rc::new(Hidden { spans, virt });
        self.cache.borrow_mut().hidden = Some(h.clone());
        h
    }
    /// The visible run around `pos`, which must not be hidden: from the line's start or the last
    /// hidden range's end, to its newline or the next hidden range's start.
    pub(crate) fn run_bounds(&self, h: &Hidden, pos: usize) -> (usize, usize) {
        let after = h.last_le(pos).map_or(0, |s| s.end);
        let i = h.spans.partition_point(|s| s.start <= pos);
        let end = h.spans.get(i).map_or(usize::MAX, |s| s.start);
        let start = self.line_start(self.line_of(pos));
        (start.max(after), self.newline_from(pos).min(end))
    }
    /// The first newline at or after `p`, or the end of the text.
    fn newline_from(&self, p: usize) -> usize {
        let next = self.line_of(p) + 1;
        self.line_start(next) - usize::from(next < self.line_count())
    }
    /// Where the display line starting at `s` stops: its first newline that isn't hidden, or the
    /// end of the text.
    pub(crate) fn line_end_of(&self, h: &Hidden, s: usize) -> usize {
        let mut e = self.newline_from(s);
        while let Some(sp) = h.covering(e) {
            e = self.newline_from(sp.end);
        }
        e
    }
    /// The start of the display line containing `pos`.
    pub(crate) fn dline_start(&self, pos: usize) -> usize {
        let h = self.hidden();
        let mut p = pos.min(self.len());
        loop {
            let ls = self.line_start(self.line_of(p));
            match ls.checked_sub(1).and_then(|nl| h.covering(nl)) {
                Some(s) => p = s.start,
                None => return ls,
            }
        }
    }
}
