//! Undo history. Groups span commands (insert sessions, `3dd`) and restore the cursor.
//! Silent edits aren't recorded, but history is rebased over them lazily, so undo still
//! removes exactly the user's text after an extension appended above it (contract §9).
use crate::error::*;

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
    ops: Vec<Op>,          // forward ops, in application order
    cursor: Option<usize>, // cursor in the text before this group
    cmd: u64,              // the top-level command that created it
    open: bool,            // an explicit group (insert session) still collecting
}

#[derive(Debug)]
enum Entry {
    Group(Group),
    Silent(Vec<Shift>),
}

pub(crate) struct Plan {
    pub ops: Vec<Op>,
    pub cursor: Option<usize>,
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
    pending_open: Option<Option<usize>>,
}

const MAX_ENTRIES: usize = 2000;

fn push_silent(stack: &mut Vec<Entry>, s: Shift) {
    if stack.is_empty() {
        return;
    }
    match stack.last_mut() {
        Some(Entry::Silent(v)) => v.push(s),
        _ => stack.push(Entry::Silent(vec![s])),
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
    pub fn record(&mut self, op: Op, cursor_before: Option<usize>, cmd: u64) {
        self.redo.clear();
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
                        g.cursor = g.cursor.map(|c| below.iter().fold(c, |c, s| s.map(c)));
                        self.undo.push(Entry::Silent(below));
                        self.undo.push(Entry::Group(g));
                    }
                    None => self.undo.clear(),
                }
            }
            if let Some(Entry::Group(g)) = self.undo.last_mut() {
                g.ops.push(op);
                return;
            }
        }
        let cursor = self.pending_open.take().unwrap_or(cursor_before);
        self.undo.push(Entry::Group(Group {
            ops: vec![op],
            cursor,
            cmd,
            open: self.depth > 0,
        }));
        if self.undo.len() > MAX_ENTRIES {
            self.undo.remove(0);
        }
    }

    pub fn silent(&mut self, s: Shift) {
        if let Some(Some(c)) = &mut self.pending_open {
            *c = s.map(*c);
        }
        push_silent(&mut self.undo, s);
        push_silent(&mut self.redo, s);
    }

    /// Open an explicit group. Edits until the matching `end` are one undo step.
    pub fn begin(&mut self, cursor: Option<usize>, cmd: u64) {
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
        self.pending_open = Some(cursor);
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
        let (cursor, other) = if redo {
            let before = g.cursor.map(|c| pending.iter().fold(c, |c, s| s.map(c)));
            let cur = ops.first().map(|o| o.pos);
            (
                cur,
                Group {
                    ops: ops.clone(),
                    cursor: before,
                    cmd: 0,
                    open: false,
                },
            )
        } else {
            let cur = g.cursor.map(|c| after.iter().fold(c, |c, s| s.map(c)));
            (
                cur,
                Group {
                    ops: invert_seq(&ops),
                    cursor: cur,
                    cmd: 0,
                    open: false,
                },
            )
        };
        Ok(Plan {
            ops,
            cursor,
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
