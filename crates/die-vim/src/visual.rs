//! Visual state, char, line and block (scope §10), following your Evil except where
//! deviations.json says otherwise.
//!
//! The selection is core's [`Selection`], so a host can draw it and a harness can set it up.
//! `head` is the cursor, and both ends are inside what's selected. A block's columns come
//! from its two corners, and `goal == Some(usize::MAX)` (`$`) takes each line to its end.
//! Unlike normal state, the cursor can rest on a newline.
//!
//! Motions move the head (`grammar::run_motion`). Operators, as in Evil, go to the
//! selection's upper left first, so `u` comes back there, and act on [`Take`]. `<` and `>`
//! always take whole lines, and text objects grow the selection.

use crate::Vim;
use crate::grammar::{Operator, Start, buffer_window};
use crate::insert::{self, Entry, MAX_REPEAT, RowAt, Rows};
use crate::objects::Object;
use crate::operators::{self, How, Span};
use crate::registers;
use crate::repeat::{self, Change, Extent};
use crate::text::{Chars, last_line, line_end, next_stop, pos_at_col, prev_stop, rest_pos};
use die_core::{Args, BufferId, Ctx, Error, Gravity, MarkerId, Origin, RegKind, Register, Result};
use die_core::{Selection, Shape, State, TextBuf, WindowId, key_notation};
use std::ops::Range;

/// What an operator takes from a selection.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Take {
    /// What's selected.
    Selection,
    /// `D`, `C`, `Y` and `X`: whole lines, but a block stays one, and all but `Y` takes each
    /// of its lines to the end.
    Lines,
    /// `S` and `R`: whole lines, whatever the selection.
    WholeLines,
}

/// The last selection, for `gv`, following the text as it changes.
#[derive(Clone, Copy)]
pub struct Last {
    anchor: MarkerId,
    head: MarkerId,
    shape: Shape,
    goal: Option<usize>,
}

pub fn is_on(ctx: &Ctx, b: BufferId) -> bool {
    ctx.state_of(b) == State::Visual
}

fn selection(t: &TextBuf, w: WindowId) -> Result<Selection> {
    t.selection(w).ok_or(Error::Msg("no cursor".into()))
}

/// What a command acts on: its buffer, its window and the selection there.
fn here(ctx: &Ctx, a: &Args) -> Result<(BufferId, WindowId, Selection)> {
    let (b, w) = buffer_window(ctx, a)?;
    Ok((b, w, selection(ctx.text(b)?, w)?))
}

fn select_ends(t: &mut TextBuf, w: WindowId, sel: Selection, anchor: usize, head: usize) {
    let new = Selection {
        anchor,
        head,
        ..sel
    };
    t.set_selection(w, new);
}

fn spaces(t: &mut TextBuf, at: usize, n: usize) -> Result {
    t.tx(Origin::User).insert(at, " ".repeat(n)).commit()?;
    Ok(())
}

fn ends(sel: Selection) -> (usize, usize) {
    (sel.anchor.min(sel.head), sel.anchor.max(sel.head))
}

/// `v`, `V` and `C-v`: start selecting, or change shape, or the same key again stops.
pub fn enter(ctx: &mut Ctx, a: &Args, shape: Shape) -> Result {
    let (b, w, sel) = here(ctx, a)?;
    if is_on(ctx, b) && sel.shape == shape {
        return escape(ctx, a);
    }
    let new = if is_on(ctx, b) {
        Selection { shape, ..sel }
    } else {
        Selection {
            shape,
            ..Selection::cursor(sel.head)
        }
    };
    ctx.text_mut(b)?.set_selection(w, new);
    ctx.set_state_of(b, State::Visual);
    Ok(())
}

/// Escape: the cursor stays where the head is.
pub fn escape(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w, sel) = here(ctx, a)?;
    leave(ctx, b, w, sel)?;
    let t = ctx.text_mut(b)?;
    t.set_cursor(w, rest_pos(t, sel.head));
    Ok(())
}

/// Stop selecting with the cursor at the upper left, as Evil's operators do first. `gv` gets
/// the selection that way round.
fn leave_at_start(ctx: &mut Ctx, b: BufferId, w: WindowId, sel: Selection) -> Result {
    let t = ctx.text(b)?;
    let (lo, hi) = ends(sel);
    let (anchor, head) = if sel.shape == Shape::Block {
        let blk = Block::of(t, sel, false);
        let last = pos_at_col(t, blk.last, blk.right.saturating_sub(1));
        (last.min(line_end(t, blk.last)), blk.corner(t))
    } else {
        (hi, lo)
    };
    leave(
        ctx,
        b,
        w,
        Selection {
            anchor,
            head,
            ..sel
        },
    )
}

/// Stop selecting, and keep the selection for `gv`.
fn leave(ctx: &mut Ctx, b: BufferId, w: WindowId, sel: Selection) -> Result {
    ctx.set_state_of(b, State::Normal);
    let t = ctx.text_mut(b)?;
    let last = Last {
        anchor: t.add_marker(sel.anchor, Gravity::Left)?,
        head: t.add_marker(sel.head, Gravity::Left)?,
        shape: sel.shape,
        goal: sel.goal,
    };
    // Off the selection, so the window draws a plain cursor whatever happens next.
    t.set_selection(w, Selection::cursor(sel.head).with_goal(sel.goal));
    let old = ctx.with_ext(|v: &mut Vim, _| v.last_visual.insert(b, last))?;
    if let Some(old) = old {
        let t = ctx.text_mut(b)?;
        t.remove_marker(old.anchor);
        t.remove_marker(old.head);
    }
    Ok(())
}

/// `gv`: the last selection again. With none, or in visual state, it does nothing, as in Evil.
pub fn restore(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    if is_on(ctx, b) {
        return Ok(());
    }
    let last = ctx.with_ext(|v: &mut Vim, _| v.last_visual.get(&b).copied())?;
    let Some(l) = last else { return Ok(()) };
    let t = ctx.text_mut(b)?;
    let here = selection(t, w)?.head;
    let sel = Selection {
        anchor: t.marker(l.anchor).unwrap_or(here),
        head: t.marker(l.head).unwrap_or(here),
        shape: l.shape,
        goal: l.goal,
    };
    t.set_selection(w, sel);
    ctx.set_state_of(b, State::Visual);
    Ok(())
}

/// `o`: the cursor goes to the other end. `O`: on a block, to the other corner on its line.
pub fn swap(ctx: &mut Ctx, a: &Args, same_line: bool) -> Result {
    let (b, w, sel) = here(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let (anchor, head) = if same_line && sel.shape == Shape::Block {
        let (ca, ch) = (t.display_col(sel.anchor), t.display_col(sel.head));
        (
            pos_at_col(t, t.line_of(sel.anchor), ch),
            pos_at_col(t, t.line_of(sel.head), ca),
        )
    } else {
        (sel.head, sel.anchor)
    };
    select_ends(t, w, sel, anchor, head);
    Ok(())
}

/// A text object: the selection takes it in. If it has already, the selection takes in the
/// next thing it comes to in the direction it goes, as Evil's objects do. A pair of brackets
/// has one on either side, so the next that makes the selection bigger is an outer pair.
pub fn object(ctx: &mut Ctx, a: &Args, object: Object) -> Result {
    let (b, w, sel) = here(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let (lo, hi) = ends(sel);
    let single = lo == hi && sel.shape == Shape::Char;
    // Whole lines count up to the last one's newline, since a word stops there.
    let (beg, past) = if sel.shape == Shape::Line {
        let span = operators::lines(t, lo, hi);
        (span.beg, line_end(t, t.line_of(hi)))
    } else {
        (lo, next_stop(t, hi).unwrap_or(hi))
    };
    let forward = sel.head >= sel.anchor;
    let at = |pos| {
        object(&Start {
            text: t,
            pos,
            count: a.count,
            goal: None,
            arg: None,
            op: None,
            visual: false,
        })
    };
    let mut span = at(sel.head).map_err(|e| Error::Msg(e.into()))?;
    let grows = |s: &Span| s.beg < beg || s.end > past;
    if !single && !grows(&span) {
        let mut from = if forward {
            Some(past)
        } else {
            prev_stop(t, beg)
        };
        for _ in 0..2 {
            let Some(p) = from else { break };
            match at(p) {
                // Whole lines hold all the words they could reach, as Evil's stay on a line.
                Ok(s) if grows(&s) && (sel.shape != Shape::Line || s.lines) => {
                    span = s;
                    break;
                }
                _ => {}
            }
            from = if forward {
                next_stop(t, p)
            } else {
                prev_stop(t, p)
            };
        }
    }
    let (first, end) = if single {
        (span.beg, span.end)
    } else {
        (beg.min(span.beg), past.max(span.end))
    };
    let last = prev_stop(t, end).filter(|&p| p >= first).unwrap_or(first);
    let (anchor, head) = if forward {
        (first, last)
    } else {
        (last, first)
    };
    let shape = if span.lines { Shape::Line } else { Shape::Char };
    let new = Selection {
        shape,
        goal: None,
        ..sel
    };
    select_ends(t, w, new, anchor, head);
    Ok(())
}

/// What `.` needs in order to do this again.
fn change_of(t: &TextBuf, sel: Selection, a: &Args) -> Change {
    let (lo, hi) = ends(sel);
    let lines = t.line_of(hi) - t.line_of(lo);
    let chars_to = |from: usize| t.rope().byte_to_char(hi) - t.rope().byte_to_char(from);
    let extent = match sel.shape {
        Shape::Char if lines == 0 => Extent::Char {
            lines,
            chars: chars_to(lo),
        },
        Shape::Char => Extent::Char {
            lines,
            chars: chars_to(t.line_start(t.line_of(hi))),
        },
        Shape::Line => Extent::Line { lines },
        Shape::Block => Extent::Block {
            lines,
            cols: t.display_col(sel.anchor).abs_diff(t.display_col(sel.head)),
        },
    };
    Change {
        count: a.count,
        register: a.register,
        keys: key_notation(&a.keys),
        visual: Some(extent),
    }
}

/// For `.`: select as much as `e` says from the cursor. A block on a line too short for it
/// makes the line long enough, as Evil's does.
pub fn select(ctx: &mut Ctx, b: BufferId, w: WindowId, e: Extent) -> Result {
    let t = ctx.text_mut(b)?;
    let p = selection(t, w)?.head;
    let line = t.line_of(p);
    let down = |t: &TextBuf, n: usize| line.saturating_add(n).min(last_line(t).max(line));
    let chars_from = |t: &TextBuf, from: usize, n: usize| {
        let r = t.rope();
        let c = r.byte_to_char(from).saturating_add(n).min(r.len_chars());
        r.char_to_byte(c)
    };
    let (head, shape) = match e {
        Extent::Char { lines: 0, chars } => (chars_from(t, p, chars), Shape::Char),
        Extent::Char { lines, chars } => {
            let start = t.line_start(down(t, lines));
            (chars_from(t, start, chars), Shape::Char)
        }
        Extent::Line { lines } => (t.line_start(down(t, lines)), Shape::Line),
        Extent::Block { lines, cols } => {
            let (l, col) = (down(t, lines), t.display_col(p).saturating_add(cols));
            let (eol, have) = (line_end(t, l), t.display_col(line_end(t, l)));
            if have < col {
                spaces(t, eol, col - have)?;
            }
            (pos_at_col(t, l, col), Shape::Block)
        }
    };
    let new = Selection {
        shape,
        ..Selection::cursor(p)
    };
    select_ends(t, w, new, p, head);
    ctx.set_state_of(b, State::Visual);
    Ok(())
}

/// The lines and columns a block selection covers. `right` is just past the last column.
pub struct Block {
    first: usize,
    last: usize,
    left: usize,
    right: usize,
}

impl Block {
    fn of(t: &TextBuf, sel: Selection, to_eol: bool) -> Block {
        let (lo, hi) = ends(sel);
        let (first, last) = (t.line_of(lo), t.line_of(hi));
        let (ca, ch) = (t.display_col(sel.anchor), t.display_col(sel.head));
        // As Evil's block type has it: the corner further right takes in its character,
        // unless it's on a line's end, and with both in one column, the lower one does.
        let at_end = |p: usize| p >= line_end(t, t.line_of(p));
        let past = |p: usize, col: usize| {
            if at_end(p) {
                col
            } else {
                next_stop(t, p).map_or(col + 1, |q| t.display_col(q).max(col + 1))
            }
        };
        let (cl, ch2) = (t.display_col(lo), t.display_col(hi));
        let mut right = match cl.cmp(&ch2) {
            std::cmp::Ordering::Less => past(hi, ch2),
            std::cmp::Ordering::Greater => past(lo, cl),
            std::cmp::Ordering::Equal if at_end(hi) => past(lo, cl),
            std::cmp::Ordering::Equal => past(hi, ch2),
        };
        if to_eol || sel.goal == Some(usize::MAX) {
            let ends = (first..=last).map(|l| t.display_col(line_end(t, l)));
            right = ends.fold(ca.max(ch), usize::max);
        }
        Block {
            first,
            last,
            left: ca.min(ch),
            right,
        }
    }

    fn lines(&self) -> usize {
        self.last - self.first + 1
    }

    /// Where the upper left corner is.
    fn corner(&self, t: &TextBuf) -> usize {
        pos_at_col(t, self.first, self.left).min(line_end(t, self.first))
    }

    /// What's in the block on this line, for a delete or a case change. A tab it touches
    /// goes whole, as Evil's `evil-apply-on-block` has it.
    fn row(&self, t: &TextBuf, line: usize) -> Range<usize> {
        let eol = line_end(t, line);
        let at = pos_at_col(t, line, self.left).min(eol);
        let beg = if t.display_col(at) > self.left {
            prev_stop(t, at).unwrap_or(at)
        } else {
            at
        };
        // Under a Hide, a stop can be off this line.
        let beg = beg.max(t.line_start(line));
        beg..pos_at_col(t, line, self.right).min(eol).max(beg)
    }

    fn rows(&self, t: &TextBuf) -> Vec<Range<usize>> {
        (self.first..=self.last).map(|l| self.row(t, l)).collect()
    }

    /// What a register holds of this line, as Emacs's `extract-rectangle-line` has it. Every
    /// line is as wide as the block: short ones gain spaces, and tabs become spaces.
    fn text(&self, t: &TextBuf, line: usize) -> String {
        let eol = line_end(t, line);
        let start = pos_at_col(t, line, self.left).min(eol);
        let end = pos_at_col(t, line, self.right).min(eol).max(start);
        let (scol, ecol) = (t.display_col(start), t.display_col(end));
        let width = self.right.saturating_sub(self.left);
        let before = scol.saturating_sub(self.left).min(width);
        let mut s = " ".repeat(before);
        let mut c = Chars::at(t, start);
        while c.pos() < end {
            let Some(ch) = c.ch() else { break };
            let from = t.display_col(c.pos());
            c.fwd();
            if ch == '\t' {
                s.push_str(&" ".repeat(t.display_col(c.pos()).saturating_sub(from)));
            } else {
                s.push(ch);
            }
        }
        let have = before + ecol.saturating_sub(scol);
        s + &" ".repeat(width.saturating_sub(have))
    }

    fn register(&self, t: &TextBuf) -> Register {
        let rows: Vec<String> = (self.first..=self.last).map(|l| self.text(t, l)).collect();
        Register {
            kind: RegKind::Block,
            text: rows.join("\n"),
        }
    }
}

/// An operator on the selection.
pub fn operate(ctx: &mut Ctx, a: &Args, op: Operator, take: Take) -> Result {
    let (b, w, sel) = here(ctx, a)?;
    let t = ctx.text(b)?;
    let change = change_of(t, sel, a);
    let col = t.display_col(sel.head);
    let (lo, hi) = ends(sel);
    let whole = take == Take::WholeLines || operators::takes_lines(op);
    if sel.shape == Shape::Block && !whole {
        let blk = Block::of(t, sel, take == Take::Lines && op != Operator::Yank);
        let corner = blk.corner(t);
        leave_at_start(ctx, b, w, sel)?;
        let t = ctx.text_mut(b)?;
        t.set_cursor(w, corner);
        return on_block(ctx, a, op, &blk, corner, change);
    }
    let span = if whole || take == Take::Lines || sel.shape == Shape::Line {
        operators::lines(t, lo, hi)
    } else {
        Span::chars(lo, next_stop(t, hi).unwrap_or(hi))
    };
    leave_at_start(ctx, b, w, sel)?;
    let t = ctx.text_mut(b)?;
    t.set_selection(w, Selection::cursor(rest_pos(t, lo)).with_goal(sel.goal));
    operators::apply(ctx, a, op, a.register, span, How::from_col(col), change)?;
    // Evil's case operators don't move it again, and over whole lines that isn't the span's start.
    if matches!(
        op,
        Operator::Upcase | Operator::Downcase | Operator::InvertCase
    ) {
        let t = ctx.text_mut(b)?;
        t.set_cursor(w, rest_pos(t, lo));
    }
    Ok(())
}

fn on_block(
    ctx: &mut Ctx,
    a: &Args,
    op: Operator,
    blk: &Block,
    corner: usize,
    change: Change,
) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text(b)?;
    let rows = blk.rows(t);
    let saved = blk.register(t);
    match op {
        Operator::Yank => {
            registers::save(ctx, a.register, saved, true)?;
            let t = ctx.text_mut(b)?;
            t.set_cursor(w, rest_pos(t, corner));
            return Ok(());
        }
        Operator::Delete | Operator::Change => {
            if a.register.is_none() && rows.len() == 1 {
                let plain = t.rope().byte_slice(rows[0].clone()).to_string();
                registers::small_delete(ctx, &plain);
            }
            registers::save(ctx, a.register, saved, false)?;
            let t = ctx.text_mut(b)?;
            let mut tx = t.tx(Origin::User);
            for r in rows {
                tx = tx.delete(r);
            }
            tx.commit()?;
        }
        _ => {
            let t = ctx.text_mut(b)?;
            let new: Vec<String> = rows
                .iter()
                .map(|r| operators::cased(op, &t.rope().byte_slice(r.clone()).to_string()))
                .collect();
            let mut tx = t.tx(Origin::User);
            for (r, s) in rows.into_iter().zip(new) {
                tx = tx.replace(r, s);
            }
            tx.commit()?;
        }
    }
    if op == Operator::Change {
        let t = ctx.text_mut(b)?;
        t.set_cursor(w, corner);
        let at = RowAt::Col(t.display_col(corner));
        let rows = Rows {
            line: blk.first,
            at,
            n: blk.lines(),
            skip_short: false,
        };
        return begin_session(ctx, a, b, None, Entry::Insert, rows, change);
    }
    let t = ctx.text_mut(b)?;
    t.set_cursor(w, rest_pos(t, corner));
    repeat::commit(ctx, change)
}

fn begin_session(
    ctx: &mut Ctx,
    a: &Args,
    b: BufferId,
    count: Option<usize>,
    entry: Entry,
    rows: Rows,
    change: Change,
) -> Result {
    let once = Args { count, ..a.clone() };
    let mut s = insert::begin(ctx, &once, entry)?;
    s.rows = Some(rows);
    s.repeat = Some(change);
    ctx.with_ext(|v: &mut Vim, _| v.inserts.insert(b, s))?;
    Ok(())
}

/// `I` and `A`: on characters, an insert at the selection's start or after its end. On lines
/// and blocks, what's typed goes in on each line.
pub fn insert(ctx: &mut Ctx, a: &Args, append: bool) -> Result {
    let (b, w, sel) = here(ctx, a)?;
    let t = ctx.text(b)?;
    let change = change_of(t, sel, a);
    let (lo, hi) = ends(sel);
    let blk = Block::of(t, sel, false);
    let first = t.line_of(lo);
    let n = t.line_of(hi) - first + 1;
    let row = |at, skip_short| Rows {
        line: first,
        at,
        n,
        skip_short,
    };
    let corner = blk.corner(t);
    let (pos, entry, rows, pad) = match (sel.shape, append) {
        (Shape::Char, false) => (lo, Entry::Insert, row(RowAt::Col(0), false), false),
        (Shape::Char, true) => (hi, Entry::Append, row(RowAt::Col(0), false), false),
        (Shape::Line, false) => (
            lo,
            Entry::InsertLine,
            row(RowAt::FirstNonBlank, false),
            false,
        ),
        (Shape::Line, true) => (lo, Entry::AppendLine, row(RowAt::LineEnd, false), false),
        (Shape::Block, false) => {
            let col = t.display_col(corner);
            (corner, Entry::Insert, row(RowAt::Col(col), true), false)
        }
        (Shape::Block, true) if sel.goal == Some(usize::MAX) => {
            (corner, Entry::AppendLine, row(RowAt::LineEnd, false), false)
        }
        (Shape::Block, true) => {
            let at = pos_at_col(t, first, blk.right).min(line_end(t, first));
            (at, Entry::Insert, row(RowAt::Col(blk.right), false), true)
        }
    };
    let rows = if sel.shape == Shape::Char {
        Rows { n: 1, ..rows }
    } else {
        rows
    };
    if append && sel.shape == Shape::Char {
        leave(ctx, b, w, sel)?;
    } else {
        leave_at_start(ctx, b, w, sel)?;
    }
    let t = ctx.text_mut(b)?;
    t.set_cursor(w, pos);
    if pad {
        let col = t.display_col(pos);
        if col < blk.right {
            spaces(t, pos, blk.right - col)?;
            t.set_cursor(w, pos + blk.right - col);
        }
    }
    begin_session(ctx, a, b, a.count, entry, rows, change)
}

/// `J`: join the selected lines.
pub fn join(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w, sel) = here(ctx, a)?;
    let t = ctx.text(b)?;
    let change = change_of(t, sel, a);
    let (lo, hi) = ends(sel);
    let n = t.line_of(hi) - t.line_of(lo) + 1;
    leave_at_start(ctx, b, w, sel)?;
    let t = ctx.text_mut(b)?;
    t.set_cursor(w, rest_pos(t, lo));
    let lines = Args {
        count: Some(n),
        ..a.clone()
    };
    operators::join(ctx, &lines)?;
    repeat::commit(ctx, change)
}

/// `r` on a selection: every character in it becomes `with`, newlines aside. A block stays
/// put, and anything else ends on the last one, as Evil's does. A key that isn't text only
/// ends the selection.
pub fn replace(ctx: &mut Ctx, a: &Args, with: Option<&str>, enter: bool) -> Result {
    let (b, w, sel) = here(ctx, a)?;
    let t = ctx.text(b)?;
    let change = change_of(t, sel, a);
    let (lo, hi) = ends(sel);
    if sel.shape == Shape::Block {
        let blk = Block::of(t, sel, false);
        let corner = blk.corner(t);
        let rows = blk.rows(t);
        leave_at_start(ctx, b, w, sel)?;
        let t = ctx.text_mut(b)?;
        t.set_cursor(w, rest_pos(t, corner));
        let Some(with) = with else { return Ok(()) };
        let new: Vec<(Range<usize>, String)> = rows
            .into_iter()
            .map(|r| {
                let cols = t.display_col(r.end).saturating_sub(t.display_col(r.start));
                let s = with.repeat(cols);
                (r, s)
            })
            .collect();
        let mut tx = t.tx(Origin::User);
        for (r, s) in new {
            tx = tx.replace(r, s);
        }
        tx.commit()?;
        t.set_cursor(w, rest_pos(t, corner));
        return repeat::commit(ctx, change);
    }
    let span = if sel.shape == Shape::Line {
        operators::lines(t, lo, hi)
    } else {
        Span::chars(lo, next_stop(t, hi).unwrap_or(hi))
    };
    leave_at_start(ctx, b, w, sel)?;
    let t = ctx.text_mut(b)?;
    t.set_cursor(w, rest_pos(t, lo));
    if with.is_none() && !enter {
        return Ok(());
    }
    operators::replace_span(t, w, span, with, enter)?;
    repeat::commit(ctx, change)
}

/// `p` and `P` over a selection: it goes, and the register's text takes its place. `P` keeps
/// the register as it is: the selection goes nowhere.
pub fn paste(ctx: &mut Ctx, a: &Args, keep: bool) -> Result {
    let (b, w, sel) = here(ctx, a)?;
    let r = registers::read(ctx, a.register)?;
    let n = a.count.unwrap_or(1).max(1);
    if n > 1 && r.text.len().saturating_mul(n) > MAX_REPEAT {
        return Err(Error::Msg("Count too large".into()));
    }
    let t = ctx.text(b)?;
    let change = change_of(t, sel, a);
    let (lo, hi) = ends(sel);
    let blk = Block::of(t, sel, false);
    let corner = blk.corner(t);
    let kill = keep.then_some('_');
    leave_at_start(ctx, b, w, sel)?;
    let t = ctx.text_mut(b)?;
    let rotated = if sel.shape == Shape::Block {
        corner
    } else {
        lo
    };
    t.set_cursor(w, rest_pos(t, rotated));
    match sel.shape {
        Shape::Block => over_block(ctx, b, w, &blk, kill, &r, n)?,
        Shape::Line => over_lines(ctx, b, w, lo, hi, kill, &r, n)?,
        Shape::Char => {
            let t = ctx.text(b)?;
            let span = Span::chars(lo, next_stop(t, hi).unwrap_or(hi));
            let at = operators::cut(ctx, b, kill, span)?;
            let t = ctx.text_mut(b)?;
            match r.kind {
                RegKind::Line => put_lines_at(t, w, at, &r.text.repeat(n))?,
                RegKind::Block => {
                    t.set_cursor(w, at);
                    operators::paste_block(t, w, &r.text, n, false)?;
                }
                _ => put_chars(t, w, at, &r.text.repeat(n))?,
            }
        }
    }
    repeat::commit(ctx, change)
}

/// Characters at `at`, and the cursor on the last of them, as `P` does.
fn put_chars(t: &mut TextBuf, w: WindowId, at: usize, text: &str) -> Result {
    t.tx(Origin::User).insert(at, text).commit()?;
    let last = prev_stop(t, at + text.len()).unwrap_or(at);
    t.set_cursor(w, rest_pos(t, if text.is_empty() { at } else { last }));
    Ok(())
}

/// Lines where a charwise selection was: they split the line there, as Evil's do. At the end
/// of the text there's nothing to split, and they go above the line.
fn put_lines_at(t: &mut TextBuf, w: WindowId, at: usize, text: &str) -> Result {
    let (pos, new) = if at >= t.len() {
        (t.line_start(t.line_of(at)), text.to_owned())
    } else {
        (at, format!("\n{text}"))
    };
    let line = t.line_of(pos) + usize::from(at < t.len());
    t.tx(Origin::User).insert(pos, new).commit()?;
    t.set_cursor(w, rest_pos(t, crate::text::indent_end(t, line)));
    Ok(())
}

/// Over whole lines. Lines replace them, and the final newline stays (S5 row 18). Otherwise
/// the last line's newline stays, for the text to go on a line of its own.
#[allow(clippy::too_many_arguments)]
fn over_lines(
    ctx: &mut Ctx,
    b: BufferId,
    w: WindowId,
    lo: usize,
    hi: usize,
    kill: Option<char>,
    r: &Register,
    n: usize,
) -> Result {
    let t = ctx.text(b)?;
    let span = operators::lines(t, lo, hi);
    let ends_line = span.end > span.beg && t.rope().byte(span.end - 1) == b'\n';
    if r.kind == RegKind::Line {
        let saved = operators::taken(t, span);
        let plain = t.rope().byte_slice(span.beg..span.end).to_string();
        if kill.is_none() && !plain.contains('\n') {
            registers::small_delete(ctx, &plain);
        }
        registers::save(ctx, kill, saved, false)?;
        let text = r.text.repeat(n);
        let body = if ends_line {
            text.as_str()
        } else {
            text.strip_suffix('\n').unwrap_or(&text)
        };
        let t = ctx.text_mut(b)?;
        t.tx(Origin::User)
            .replace(span.beg..span.end, body)
            .commit()?;
        let line = t.line_of(span.beg);
        t.set_cursor(w, rest_pos(t, crate::text::indent_end(t, line)));
        return Ok(());
    }
    let inner = Span {
        end: span.end - usize::from(ends_line),
        ..span
    };
    let at = operators::cut(ctx, b, kill, inner)?;
    let t = ctx.text_mut(b)?;
    if r.kind == RegKind::Block {
        t.set_cursor(w, at);
        return operators::paste_block(t, w, &r.text, n, false);
    }
    put_chars(t, w, at, &r.text.repeat(n))
}

/// Over a block. A block goes in its place, and a line of text on each of its lines. Text of
/// several lines goes in once at its corner, and whole lines go below it.
fn over_block(
    ctx: &mut Ctx,
    b: BufferId,
    w: WindowId,
    blk: &Block,
    kill: Option<char>,
    r: &Register,
    n: usize,
) -> Result {
    let t = ctx.text(b)?;
    let rows = blk.rows(t);
    let saved = blk.register(t);
    registers::save(ctx, kill, saved, false)?;
    let text = r.text.repeat(n);
    let t = ctx.text_mut(b)?;
    let on_each = r.kind == RegKind::Char && !text.contains('\n');
    let corner = rows[0].start;
    let mut tx = t.tx(Origin::User);
    for row in rows {
        tx = tx.replace(row, if on_each { text.as_str() } else { "" });
    }
    tx.commit()?;
    t.set_cursor(w, rest_pos(t, corner));
    match r.kind {
        _ if on_each => Ok(()),
        RegKind::Block => {
            t.set_cursor(w, corner);
            operators::paste_block(t, w, &r.text, n, false)
        }
        RegKind::Line => {
            let at = line_end(t, blk.last);
            let body = text.strip_suffix('\n').unwrap_or(&text);
            t.tx(Origin::User)
                .insert(at, format!("\n{body}"))
                .commit()?;
            let line = blk.last + 1;
            t.set_cursor(w, rest_pos(t, crate::text::indent_end(t, line)));
            Ok(())
        }
        _ => put_chars(t, w, corner, &text),
    }
}
