//! Undo history. Groups span commands (insert sessions, `3dd`) and restore the selection.
//! Silent edits aren't recorded, but history is rebased over them lazily, so undo still
//! removes exactly the user's text after an extension appended above it (contract §9).
//! The history is capped per buffer: the oldest steps go first.
use crate::error::*;
use crate::text::Selection;

/// A recorded change: `removed` at `pos` was replaced by `inserted`.
#[derive(Clone, Debug)]
pub(crate) struct Op {
    pub pos: usize,
    pub removed: String,
    pub inserted: String,
}

/// A change reduced to positions, used for silent edits.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shift {
    pub pos: usize,
    pub del: usize,
    pub ins: usize,
}

impl Op {
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

pub(crate) struct Plan {
    pub ops: Vec<Op>,
    /// The selection an undo restores. Redo leaves it to the caller, which knows the range.
    pub sel: Option<Selection>,
    redo: bool,
    keep: usize,
    pending: Vec<Shift>,
    other: Group,
}

#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    depth: u32,
    pending_open: Option<Option<Selection>>,
}

/// How much history a buffer keeps: at most `steps` steps holding at most `bytes` of text.
#[derive(Clone, Copy)]
pub(crate) struct Limit {
    pub steps: usize,
    pub bytes: usize,
}

fn push_silent(stack: &mut Vec<Entry>, s: Shift) {
    match stack.last_mut() {
        None => {}
        // Streamed text: an insertion at the end of the last one extends it, which moves
        // every position and every change the same way.
        Some(Entry::Silent(v)) => match v.last_mut() {
            Some(l) if l.del == 0 && s.del == 0 && s.pos == l.pos + l.ins => l.ins += s.ins,
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
                    None => self.undo.clear(),
                }
            }
            if let Some(Entry::Group(g)) = self.undo.last_mut() {
                g.ops.push(op);
                g.bytes += bytes;
                self.trim(limit);
                return;
            }
        }
        let sel = self.pending_open.take().unwrap_or(sel_before);
        self.undo.push(Entry::Group(Group {
            ops: vec![op],
            sel,
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
        self.pending_open = None;
    }

    pub fn silent(&mut self, s: Shift) {
        if let Some(Some(c)) = &mut self.pending_open {
            *c = map_sel(*c, &[s]);
        }
        push_silent(&mut self.undo, s);
        push_silent(&mut self.redo, s);
    }

    /// Open an explicit group. Edits until the matching `end` are one undo step.
    pub fn begin(&mut self, sel: Option<Selection>, cmd: u64) {
        self.depth += 1;
        if self.depth > 1 {
            return;
        }
        // An edit made earlier in the same command (die-vim's `o`) joins the session.
        if let (Some(i), _) = top_group(&self.undo) {
            if let Entry::Group(g) = &mut self.undo[i] {
                if g.cmd == cmd {
                    g.open = true;
                    return;
                }
            }
        }
        self.pending_open = Some(sel);
    }

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
            self.pending_open = None;
        }
    }

    /// Work out an undo (or redo) step without changing anything.
    pub fn plan(&self, redo: bool) -> Result<Plan> {
        let stack = if redo { &self.redo } else { &self.undo };
        let (Some(i), pending) = top_group(stack) else {
            return Err(Error::NothingToUndo);
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
                    cmd: 0,
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
                    cmd: 0,
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
        self.depth = 0;
        self.pending_open = None;
    }

    pub fn truncate(&mut self, redo: bool) {
        if redo {
            self.redo.clear()
        } else {
            self.undo.clear()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
