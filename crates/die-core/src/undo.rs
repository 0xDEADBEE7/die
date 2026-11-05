//! Undo history. Groups span commands (insert sessions, `3dd`) and restore the selection.
//! Silent edits aren't recorded, but history is rebased over them lazily, so undo still
//! removes exactly the user's text after an extension appended above it (contract §9).
//! The history is capped per buffer: the oldest steps go first.
use crate::error::*;
use crate::text::Selection;

/// A recorded change: `removed` at `pos` was replaced by `inserted`.
#[derive(Clone, Debug)]
pub(crate) struct Op {
    /// Where it happened, in bytes, in the text just before it.
    pub pos: usize,
    /// The text it took out.
    pub removed: String,
    /// The text it put in.
    pub inserted: String,
}

/// A change reduced to positions, used for silent edits.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shift {
    /// Where it happened, in bytes, in the text just before it.
    pub pos: usize,
    /// How many bytes it took out.
    pub del: usize,
    /// How many bytes it put in.
    pub ins: usize,
}

impl Op {
    /// This change, reduced to positions.
    pub fn shift(&self) -> Shift {
        Shift {
            pos: self.pos,
            del: self.removed.len(),
            ins: self.inserted.len(),
        }
    }
    fn inverse(&self) -> Op {
        Op {
            pos: self.pos,
            removed: self.inserted.clone(),
            inserted: self.removed.clone(),
        }
    }
}

impl Shift {
    /// Where a position moves through this change (right gravity at an insertion point).
    pub fn map(&self, p: usize) -> usize {
        crate::text::map_pos(
            p,
            self.pos,
            self.pos + self.del,
            self.pos + self.ins,
            crate::text::Gravity::Right,
        )
    }
}

/// A selection moved through silent changes, in order.
fn map_sel(sel: Selection, shifts: &[Shift]) -> Selection {
    let m = |p| shifts.iter().fold(p, |p, s| s.map(p));
    Selection {
        anchor: m(sel.anchor),
        head: m(sel.head),
        ..sel
    }
}

fn invert_seq(ops: &[Op]) -> Vec<Op> {
    ops.iter().rev().map(Op::inverse).collect()
}

/// Transform two changes made against the same text. Returns `o` as it applies after `p`,
/// and `p` as it applies after `o`; `None` if they overlap.
fn transform(o: Shift, p: Shift) -> Option<(Shift, Shift)> {
    if p.pos + p.del <= o.pos {
        Some((
            Shift {
                pos: o.pos - p.del + p.ins,
                ..o
            },
            p,
        ))
    } else if p.pos >= o.pos + o.del {
        Some((
            o,
            Shift {
                pos: p.pos - o.del + o.ins,
                ..p
            },
        ))
    } else {
        None
    }
}

/// Rebase a sequence of ops over a sequence of silent shifts, both starting from the same text.
/// Returns the ops as they apply after the shifts, and the shifts as they apply after the ops.
fn rebase(ops: &[Op], pending: &[Shift]) -> Option<(Vec<Op>, Vec<Shift>)> {
    let mut pend = pending.to_vec();
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        let mut o = op.shift();
        for p in pend.iter_mut() {
            let (o2, p2) = transform(o, *p)?;
            o = o2;
            *p = p2;
        }
        out.push(Op {
            pos: o.pos,
            ..op.clone()
        });
    }
    Some((out, pend))
}

#[derive(Clone, Debug)]
struct Group {
    ops: Vec<Op>,           // forward ops, in application order
    sel: Option<Selection>, // the actor's selection in the text before this group
    cmd: u64,               // the top-level command that created it
    open: bool,             // an explicit group (insert session) still collecting
    bytes: usize,           // text held by its ops, for the cap
}

/// No command has this number, so a group closed by a boundary takes no more edits.
const CLOSED: u64 = u64::MAX;

#[derive(Debug)]
enum Entry {
    Group(Group),
    Silent(Vec<Shift>),
}

/// An undo or redo worked out but not yet done, so a read-only check can refuse it first.
pub(crate) struct Plan {
    /// The changes to apply, in order, in the coordinates of the text as it is now.
    pub ops: Vec<Op>,
    /// The selection an undo restores. Redo leaves it to the caller, which knows the range.
    pub sel: Option<Selection>,
    redo: bool,
    keep: usize,
    pending: Vec<Shift>,
    other: Group,
}

/// A buffer's undo and redo stacks, and how deep the explicit groups are nested.
#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    depth: u32,
    /// A conflict while a group was collecting dropped the history, and no undo has said so.
    dropped: bool,
}

/// How much history a buffer keeps: at most `steps` steps holding at most `bytes` of text.
#[derive(Clone, Copy)]
pub(crate) struct Limit {
    /// The most steps to keep.
    pub steps: usize,
    /// The most removed and inserted text to keep, in bytes, unless it's all one step.
    pub bytes: usize,
}

fn push_silent(stack: &mut Vec<Entry>, s: Shift) {
    match stack.last_mut() {
        None => {}
        // Streamed text: an insertion at the end of the last one extends it, which moves
        // every position and every change the same way.
        Some(Entry::Silent(v)) => match v.last_mut() {
            Some(l) if l.del == 0 && s.del == 0 && s.pos == l.pos + l.ins => l.ins += s.ins,
            // A rewrite of text the last shift inserted (a spinner, a status line redrawn in
            // place) folds into it. That's exact for positions and for rebasing any history
            // op, since those can't lie inside silent text. A pure insertion at its start is
            // left alone: gravity differs there.
            Some(l)
                if s.pos >= l.pos
                    && s.pos + s.del <= l.pos + l.ins
                    && (s.del > 0 || s.pos > l.pos) =>
            {
                l.ins = l.ins - s.del + s.ins
            }
            _ => v.push(s),
        },
        Some(Entry::Group(_)) => stack.push(Entry::Silent(vec![s])),
    }
}

/// Index of the topmost group and the silent shifts above it, oldest first.
fn top_group(stack: &[Entry]) -> (Option<usize>, Vec<Shift>) {
    let mut pending = vec![];
    for i in (0..stack.len()).rev() {
        match &stack[i] {
            Entry::Silent(v) => pending.splice(0..0, v.iter().copied()),
            Entry::Group(_) => return (Some(i), pending),
        };
    }
    (None, pending)
}

impl History {
    /// Record a change. `sel_before` is the acting window's selection just before it, which
    /// undo restores if this is the step's first change.
    pub fn record(&mut self, op: Op, sel_before: Option<Selection>, cmd: u64, limit: Limit) {
        self.redo.clear();
        let bytes = op.removed.len() + op.inserted.len();
        let (gi, pending) = top_group(&self.undo);
        let appendable =
            gi.is_some_and(|i| matches!(&self.undo[i], Entry::Group(g) if g.open || g.cmd == cmd));
        if let (true, Some(i)) = (appendable, gi) {
            if !pending.is_empty() {
                // Rebase the collecting group over the silent edits above it, then move them below.
                let Entry::Group(mut g) = self.undo.remove(i) else {
                    unreachable!()
                };
                self.undo.truncate(i);
                match rebase(&invert_seq(&g.ops), &pending) {
                    Some((inv, below)) => {
                        g.ops = invert_seq(&inv);
                        g.sel = g.sel.map(|s| map_sel(s, &below));
                        self.undo.push(Entry::Silent(below));
                        self.undo.push(Entry::Group(g));
                    }
                    None => {
                        self.undo.clear();
                        self.dropped = true;
                    }
                }
            }
            if let Some(Entry::Group(g)) = self.undo.last_mut() {
                g.ops.push(op);
                g.bytes += bytes;
                self.trim(limit);
                return;
            }
        }
        self.undo.push(Entry::Group(Group {
            ops: vec![op],
            sel: sel_before,
            cmd,
            open: self.depth > 0,
            bytes,
        }));
        self.trim(limit);
    }

    /// Drop the oldest steps until at most `limit.steps` remain and they hold at most
    /// `limit.bytes`. The newest step stays whatever its size, unless no steps are kept.
    pub(crate) fn trim(&mut self, limit: Limit) {
        let (mut steps, mut bytes, mut cut) = (0, 0, 0);
        for (i, e) in self.undo.iter().enumerate().rev() {
            let Entry::Group(g) = e else { continue };
            if steps == limit.steps || (steps > 0 && bytes + g.bytes > limit.bytes) {
                cut = i + 1;
                break;
            }
            steps += 1;
            bytes += g.bytes;
        }
        self.undo.drain(..cut);
        // Silent shifts below the oldest step no longer matter.
        let first = self.undo.iter().position(|e| matches!(e, Entry::Group(_)));
        self.undo.drain(..first.unwrap_or(self.undo.len()));
    }

    /// End the current step: the next recorded edit starts a new one, even in the same
    /// command or inside an open group, which stays open.
    pub fn boundary(&mut self) {
        if let (Some(i), _) = top_group(&self.undo)
            && let Entry::Group(g) = &mut self.undo[i]
        {
            g.open = false;
            g.cmd = CLOSED;
        }
    }

    /// Note a silent edit, which later undos and redos rebase over.
    pub fn silent(&mut self, s: Shift) {
        push_silent(&mut self.undo, s);
        push_silent(&mut self.redo, s);
    }

    /// Open an explicit group. Edits until the matching `end` are one undo step. A step made
    /// earlier in the same command joins it.
    pub fn begin(&mut self, cmd: u64) {
        self.depth += 1;
        if self.depth > 1 {
            return;
        }
        // An edit made earlier in the same command (die-vim's `o`) joins the session.
        if let (Some(i), _) = top_group(&self.undo) {
            if let Entry::Group(g) = &mut self.undo[i] {
                if g.cmd == cmd {
                    g.open = true;
                }
            }
        }
    }

    /// Close an explicit group. Only the outermost `end` closes it.
    pub fn end(&mut self) {
        if self.depth == 0 {
            return;
        }
        self.depth -= 1;
        if self.depth == 0 {
            if let (Some(i), _) = top_group(&self.undo) {
                if let Entry::Group(g) = &mut self.undo[i] {
                    g.open = false;
                }
            }
        }
    }

    /// Work out an undo (or redo) step without changing anything.
    pub fn plan(&self, redo: bool) -> Result<Plan> {
        let stack = if redo { &self.redo } else { &self.undo };
        let (Some(i), pending) = top_group(stack) else {
            return Err(if !redo && self.dropped {
                Error::UndoConflict
            } else {
                Error::NothingToUndo
            });
        };
        let Entry::Group(g) = &stack[i] else {
            unreachable!()
        };
        let to_apply = if redo {
            g.ops.clone()
        } else {
            invert_seq(&g.ops)
        };
        let (ops, after) = rebase(&to_apply, &pending).ok_or(Error::UndoConflict)?;
        let (sel, other) = if redo {
            let before = g.sel.map(|s| map_sel(s, &pending));
            (
                None,
                Group {
                    ops: ops.clone(),
                    sel: before,
                    cmd: CLOSED,
                    open: false,
                    bytes: g.bytes,
                },
            )
        } else {
            let cur = g.sel.map(|s| map_sel(s, &after));
            (
                cur,
                Group {
                    ops: invert_seq(&ops),
                    sel: cur,
                    cmd: CLOSED,
                    open: false,
                    bytes: g.bytes,
                },
            )
        };
        Ok(Plan {
            ops,
            sel,
            redo,
            keep: i,
            pending: after,
            other,
        })
    }

    /// Do what `plan` worked out, once its changes are made.
    pub fn commit(&mut self, p: Plan) {
        let (from, to) = if p.redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        from.truncate(p.keep);
        if !p.pending.is_empty() && !from.is_empty() {
            from.push(Entry::Silent(p.pending));
        }
        to.push(Entry::Group(p.other));
        // An open group stays open: the next edit starts a new step inside it.
    }

    /// Drop the undo stack, or the redo stack, after a conflict. Reports any earlier drop.
    pub fn truncate(&mut self, redo: bool) {
        if redo {
            self.redo.clear()
        } else {
            self.undo.clear();
            self.dropped = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shift(pos: usize, del: usize, ins: usize) -> Shift {
        Shift { pos, del, ins }
    }

    /// `l` then `s`, against the one shift `push_silent` folds them into, for every small
    /// case: where each position goes, how each op rebases, and whether it conflicts.
    #[test]
    fn a_folded_rewrite_is_exact() {
        let mut folded = 0;
        for (lp, ld, li) in
            (0..7).flat_map(|p| (0..3).flat_map(move |d| (0..4).map(move |i| (p, d, i))))
        {
            for (sp, sd, si) in
                (0..7).flat_map(|p| (0..4).flat_map(move |d| (0..4).map(move |i| (p, d, i))))
            {
                let group = || {
                    Entry::Group(Group {
                        ops: vec![],
                        sel: None,
                        cmd: 0,
                        open: false,
                        bytes: 0,
                    })
                };
                let mut stack = vec![group()];
                push_silent(&mut stack, shift(lp, ld, li));
                push_silent(&mut stack, shift(sp, sd, si));
                let (_, pending) = top_group(&stack);
                if pending.len() != 1 {
                    continue;
                }
                folded += 1;
                let (l, s, f) = (shift(lp, ld, li), shift(sp, sd, si), pending[0]);
                for p in 0..16 {
                    assert_eq!(f.map(p), s.map(l.map(p)), "{l:?} {s:?} at {p}");
                }
                for (op, del, ins) in
                    (0..10).flat_map(|p| (0..4).flat_map(move |d| (0..3).map(move |i| (p, d, i))))
                {
                    let o = Op {
                        pos: op,
                        removed: "x".repeat(del),
                        inserted: "y".repeat(ins),
                    };
                    let by_two = rebase(std::slice::from_ref(&o), &[l, s]);
                    let by_one = rebase(std::slice::from_ref(&o), &[f]);
                    let key = |r: &Option<(Vec<Op>, Vec<Shift>)>| {
                        r.as_ref().map(|(ops, after)| {
                            let m = |p| after.iter().fold(p, |p, s| s.map(p));
                            let moved: Vec<usize> = (0..20).map(m).collect();
                            (ops[0].pos, moved)
                        })
                    };
                    assert_eq!(key(&by_one), key(&by_two), "{l:?} {s:?} over {o:?}");
                }
            }
        }
        assert!(folded > 1000, "only {folded} cases folded");
    }

    #[test]
    fn a_status_line_rewritten_in_place_stays_one_shift() {
        let limit = Limit {
            steps: 10,
            bytes: 100,
        };
        let typed = Op {
            pos: 0,
            removed: String::new(),
            inserted: "x".into(),
        };
        for rewrite in [shift(1, 10, 10), shift(4, 1, 1), shift(1, 1, 1)] {
            let mut h = History::default();
            h.record(typed.clone(), None, 1, limit);
            h.silent(shift(1, 0, 10));
            for _ in 0..1000 {
                h.silent(rewrite);
            }
            let (_, pending) = top_group(&h.undo);
            assert_eq!(pending.len(), 1, "{rewrite:?}");
            assert_eq!((pending[0].pos, pending[0].del, pending[0].ins), (1, 0, 10));
        }
    }

    #[test]
    fn a_pure_insertion_at_the_last_shifts_start_stays_apart() {
        let mut stack = vec![Entry::Silent(vec![shift(5, 0, 3)])];
        push_silent(&mut stack, shift(5, 0, 2));
        let (_, pending) = top_group(&stack);
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn streamed_silent_text_is_one_shift() {
        let mut h = History::default();
        let typed = Op {
            pos: 0,
            removed: String::new(),
            inserted: "x".into(),
        };
        let limit = Limit {
            steps: 10,
            bytes: 100,
        };
        h.record(typed, None, 1, limit);
        for i in 0..1000 {
            h.silent(Shift {
                pos: 1 + i,
                del: 0,
                ins: 1,
            });
        }
        let (_, pending) = top_group(&h.undo);
        assert_eq!(pending.len(), 1);
        assert_eq!((pending[0].pos, pending[0].ins), (1, 1000));
    }
}
