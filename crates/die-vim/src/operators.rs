//! Operators (scope §10), and the commands that edit without waiting for a motion. Each
//! follows your Evil, except where deviations.json says otherwise.
//!
//! `d`, `c`, `y`, `<`, `>`, `g~`, `gU` and `gu` wait for a motion or text object. Pressing
//! the operator's own key again means whole lines. The range goes through two steps:
//!
//! 1. [`covered`] runs the motion from the cursor and turns where it lands into a
//!    [`Span`], as its [`Kind`] says. A signal of End of line or End of buffer doesn't
//!    stop it: the operator takes what the motion reached. Any other signal does.
//! 2. [`apply`] gives the span to the operator.
//!
//! A command that moves the cursor and edits makes its edit first, so `u` puts the cursor
//! back where the command started (m0-log's call 7).

use crate::Vim;
use crate::grammar::{Fail, Goal, Kind, Operator, Start, Target, buffer_window, forget_goal};
use crate::insert::{self, Entry, MAX_REPEAT};
use crate::motions;
use crate::objects::Object;
use crate::registers;
use crate::text::{
    Chars, blank_line, indent_end, last_line, line_end, next_stop, normal_pos, on_last_empty_line,
    pos_at_col, prev_stop, rest_pos,
};
use die_core::{Args, BufferId, Ctx, Error, Key, Origin, RegKind, Register, Result, State};
use die_core::{TextBuf, WindowId};

/// An operator waiting for its motion: core's state is `OperatorPending`.
#[derive(Clone, Copy, Debug)]
pub struct Pending {
    op: Operator,
    count: Option<usize>,
    register: Option<char>,
}

/// What an operator acts on: the bytes `beg..end`, and whether they're whole lines. A
/// span's end is where it ends, so whole lines end at the start of the next line, or at
/// the end of the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// The first byte.
    pub beg: usize,
    /// Just past the last byte.
    pub end: usize,
    /// Whole lines: yanked and pasted as lines.
    pub lines: bool,
}

impl Span {
    /// Characters, `beg..end`.
    pub fn chars(beg: usize, end: usize) -> Span {
        Span {
            beg,
            end,
            lines: false,
        }
    }
    /// Whole lines, already `beg..end`.
    pub fn whole_lines(beg: usize, end: usize) -> Span {
        Span {
            beg,
            end,
            lines: true,
        }
    }
}

/// One change in a transaction.
type Edit = (std::ops::Range<usize>, &'static str);

/// What [`apply`] needs to know about how it was reached.
#[derive(Clone, Copy, Default)]
struct How {
    /// `d` with a motion from the keys. It has two habits that `x`, `D` and `c` don't: it
    /// turns a range from a line's start to a later line's end into whole lines, and after
    /// taking whole lines it puts the cursor in the column it started in.
    typed: bool,
    /// But `dw` taking whole lines goes to the first non-blank (`Target::no_column`).
    no_column: bool,
    /// `~` leaves the cursor past what it changed, not at the start.
    past_end: bool,
}

/// `d`, `c` and so on. Normal state starts waiting; pressed again, it means whole lines,
/// and another operator's key cancels.
pub fn on_key(ctx: &mut Ctx, a: &Args, op: Operator) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    if ctx.state_of(b) != State::OperatorPending {
        let p = Pending {
            op,
            count: a.count,
            register: a.register,
        };
        ctx.with_ext(|v: &mut Vim, _| v.pending.insert(b, p))?;
        ctx.set_state_of(b, State::OperatorPending);
        return Ok(());
    }
    let Some(p) = take(ctx, b)? else {
        return Ok(());
    };
    if p.op != op {
        forget_goal(ctx.text_mut(b)?, w);
        return Ok(());
    }
    let span = this_line_and_more(ctx.text(b)?, w, counts(p.count, a.count));
    let how = How {
        typed: true,
        ..How::default()
    };
    apply(ctx, a, p.op, p.register, span, how)
}

/// A motion from the keys, with an operator waiting (`grammar::run_motion`).
pub fn on_motion(
    ctx: &mut Ctx,
    a: &Args,
    kind: Kind,
    arg: Option<char>,
    motion: impl Fn(&Start) -> std::result::Result<Target, Fail>,
) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let Some(p) = take(ctx, b)? else {
        return Ok(());
    };
    let t = ctx.text_mut(b)?;
    let (span, no_column) = covered(t, w, counts(p.count, a.count), p.op, kind, arg, motion)?;
    let how = How {
        typed: true,
        no_column,
        ..How::default()
    };
    apply(ctx, a, p.op, p.register, span, how)
}

/// A text object, with an operator waiting. One that finds nothing stops the operator.
pub fn on_object(ctx: &mut Ctx, a: &Args, object: Object) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let Some(p) = take(ctx, b)? else {
        return Ok(());
    };
    let t = ctx.text_mut(b)?;
    let sel = t.selection(w).ok_or(Error::Msg("no cursor".into()))?;
    let found = object(&Start {
        text: t,
        pos: sel.head,
        count: counts(p.count, a.count),
        goal: None,
        arg: None,
        op: Some(p.op),
    });
    let span = match found {
        Ok(span) if takes_lines(p.op) && !span.lines => lines(t, span.beg, span.end),
        Ok(span) => span,
        Err(e) => {
            forget_goal(t, w);
            return Err(Error::Msg(e.into()));
        }
    };
    let how = How {
        typed: true,
        ..How::default()
    };
    apply(ctx, a, p.op, p.register, span, how)
}

/// Escape, or any key an operator can't use: the operator, its counts and the goal
/// column go, and nothing else happens. This also serves `f<Esc>` in normal state.
pub fn cancel(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    take(ctx, b)?;
    forget_goal(ctx.text_mut(b)?, w);
    Ok(())
}

/// The operator waiting in `b`, which stops waiting.
fn take(ctx: &mut Ctx, b: BufferId) -> Result<Option<Pending>> {
    ctx.set_state_of(b, State::Normal);
    Ok(ctx.with_ext(|v: &mut Vim, _| v.pending.remove(&b))?)
}

/// The counts on both sides of an operator multiply, or are none if neither was typed.
fn counts(op: Option<usize>, motion: Option<usize>) -> Option<usize> {
    match (op, motion) {
        (None, None) => None,
        _ => Some(op.unwrap_or(1).saturating_mul(motion.unwrap_or(1))),
    }
}

/// Shifts always take whole lines, whatever the motion.
fn takes_lines(op: Operator) -> bool {
    matches!(op, Operator::ShiftLeft | Operator::ShiftRight)
}

fn cursor(t: &TextBuf, w: WindowId) -> Result<usize> {
    t.cursor(w).ok_or(Error::Msg("no cursor".into()))
}

/// What a motion from the cursor covers, for operator `op`, as Evil's `evil-motion-range`
/// and its types do. `.1` says the motion was `w` or `W` (`Target::no_column`). The goal
/// column stays when the motion that stopped the operator keeps it.
fn covered(
    t: &mut TextBuf,
    w: WindowId,
    count: Option<usize>,
    op: Operator,
    kind: Kind,
    arg: Option<char>,
    motion: impl Fn(&Start) -> std::result::Result<Target, Fail>,
) -> Result<(Span, bool)> {
    let sel = t.selection(w).ok_or(Error::Msg("no cursor".into()))?;
    let start = Start {
        text: t,
        pos: sel.head,
        count,
        goal: Goal::decode(sel.goal),
        arg,
        op: Some(op),
    };
    let goes_on = |e: Fail| e == "End of line" || e == "End of buffer";
    let to = match motion(&start) {
        Ok(to) => to,
        Err(e) if goes_on(e) => Target::at(sel.head),
        Err(e) => {
            forget_goal(t, w);
            return Err(Error::Msg(e.into()));
        }
    };
    if let Some(e) = to.short.filter(|&e| !goes_on(e)) {
        return Err(Error::Msg(e.into()));
    }
    let (beg, end) = (sel.head.min(to.pos), sel.head.max(to.pos));
    let span = if takes_lines(op) {
        lines(t, beg, end)
    } else {
        expand(t, beg, end, to.kind.unwrap_or(kind))
    };
    Ok((span, to.no_column))
}

/// Whole lines, from `beg`'s line through `end`'s.
fn lines(t: &TextBuf, beg: usize, end: usize) -> Span {
    Span::whole_lines(t.line_start(t.line_of(beg)), after_line(t, t.line_of(end)))
}

/// Where the line after `line` starts, or the end of the text.
fn after_line(t: &TextBuf, line: usize) -> usize {
    if line + 1 < t.line_count() {
        t.line_start(line + 1)
    } else {
        t.len()
    }
}

/// The current line and count-1 lines below it, as far as there are. It can't fail.
fn this_line_and_more(t: &TextBuf, w: WindowId, n: Option<usize>) -> Span {
    let here = t.line_of(t.cursor(w).unwrap_or(0));
    let more = n.unwrap_or(1).max(1) - 1;
    let line = here.saturating_add(more).min(last_line(t).max(here));
    Span::whole_lines(t.line_start(here), after_line(t, line))
}

/// Turn the range between a motion's two ends into what an operator takes.
fn expand(t: &TextBuf, beg: usize, end: usize, kind: Kind) -> Span {
    let back = |p: usize| {
        let mut c = Chars::at(t, p);
        c.back();
        c.pos().max(beg)
    };
    let past = |p: usize| next_stop(t, p).unwrap_or(p);
    match kind {
        Kind::Linewise => lines(t, beg, end),
        Kind::Inclusive => {
            // A target on a newline stays short of it, unless that's where it started.
            let on_newline = t.rope().get_byte(end) == Some(b'\n');
            Span::chars(beg, past(if on_newline { back(end) } else { end }))
        }
        // An exclusive range that ends at the start of a line ends on the line before.
        // From at or before that line's first non-blank, it's whole lines.
        Kind::Exclusive if beg != end && t.line_start(t.line_of(end)) == end => {
            let newline = back(end);
            if indent_end(t, t.line_of(beg)) >= beg {
                lines(t, beg, newline)
            } else {
                Span::chars(beg, past(back(newline)))
            }
        }
        Kind::Exclusive => Span::chars(beg, end),
    }
}

/// Give a span to an operator.
fn apply(
    ctx: &mut Ctx,
    a: &Args,
    op: Operator,
    register: Option<char>,
    span: Span,
    how: How,
) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let head = cursor(ctx.text(b)?, w)?;
    let done = match op {
        Operator::Delete => delete(ctx, b, w, register, span, how),
        Operator::Change => change(ctx, a, b, w, register, span),
        Operator::Yank => yank(ctx, b, w, register, span),
        Operator::ShiftLeft => shift(ctx.text_mut(b)?, w, span, false),
        Operator::ShiftRight => shift(ctx.text_mut(b)?, w, span, true),
        Operator::InvertCase | Operator::Upcase | Operator::Downcase => {
            recase(ctx.text_mut(b)?, w, op, span, how.past_end)
        }
    };
    // Evil's operators go to the range's start before they act, except `y` and `>`, so that's
    // where one that fails leaves the cursor. Only one that fails: otherwise the edit comes
    // first (call 7).
    if done.is_err() {
        let stays = matches!(op, Operator::Yank | Operator::ShiftRight);
        let t = ctx.text_mut(b)?;
        t.set_cursor(w, rest_pos(t, if stays { head } else { span.beg }));
    }
    done
}

/// The text of a span, as a register holds it: whole lines end in a newline.
fn taken(t: &TextBuf, span: Span) -> Register {
    let mut text = t.rope().byte_slice(span.beg..span.end).to_string();
    if span.lines && !text.ends_with('\n') {
        text.push('\n');
    }
    Register {
        kind: if span.lines {
            RegKind::Line
        } else {
            RegKind::Char
        },
        text,
    }
}

/// What `d`, `x` and `c` share, Evil's `evil-delete`: keep the text, then delete it. The
/// cursor doesn't move, so `u` restores it. Returns where the cursor belongs: the span's
/// start, or the newline before it if whole lines at the end of the text had to take it.
fn cut(ctx: &mut Ctx, b: BufferId, register: Option<char>, span: Span) -> Result<usize> {
    let t = ctx.text(b)?;
    let plain = t.rope().byte_slice(span.beg..span.end).to_string();
    let saved = taken(t, span);
    let unfinished = span.beg == span.end || t.rope().byte(span.end - 1) != b'\n';
    let newline_before = span.lines
        && span.end == t.len()
        && unfinished
        && span.beg > 0
        && t.rope().byte(span.beg - 1) == b'\n';
    if register.is_none() && !plain.contains('\n') {
        registers::small_delete(ctx, &plain);
    }
    registers::save(ctx, register, saved, false)?;
    let start = span.beg - usize::from(newline_before);
    let t = ctx.text_mut(b)?;
    t.tx(Origin::User).delete(start..span.end).commit()?;
    Ok(start)
}

fn delete(
    ctx: &mut Ctx,
    b: BufferId,
    w: WindowId,
    register: Option<char>,
    mut span: Span,
    how: How,
) -> Result {
    let t = ctx.text(b)?;
    let sel = t.selection(w).ok_or(Error::Msg("no cursor".into()))?;
    let (col, goal) = (t.display_col(sel.head), Goal::decode(sel.goal));
    if how.typed && !span.lines {
        let (first, last) = (t.line_of(span.beg), t.line_of(span.end));
        if last > first && t.line_start(first) == span.beg && line_end(t, last) == span.end {
            span = lines(t, span.beg, span.end);
        }
    }
    let at = cut(ctx, b, register, span)?;
    let t = ctx.text_mut(b)?;
    let pos = if span.lines {
        // The new last line, if whole lines at the end went (rule 1).
        let line = t.line_of(at).min(last_line(t));
        let to = match goal {
            _ if how.no_column => indent_end(t, line),
            Some(Goal::Eol) => line_end(t, line),
            _ => pos_at_col(t, line, col),
        };
        normal_pos(t, to)
    } else {
        rest_pos(t, at)
    };
    t.set_cursor(w, pos);
    Ok(())
}

/// `c`, as Evil's `evil-change`: delete, then insert. On whole lines it opens a line
/// where they were, indented like the line above. The session's undo step takes in the
/// delete, because core joins an edit from the same command (`begin_undo_group`).
fn change(
    ctx: &mut Ctx,
    a: &Args,
    b: BufferId,
    w: WindowId,
    register: Option<char>,
    span: Span,
) -> Result {
    let at = cut(ctx, b, register, span)?;
    ctx.text_mut(b)?.set_cursor(w, at);
    let entry = match span.lines {
        false => Entry::Insert,
        true if at == span.beg => Entry::OpenAbove,
        true => Entry::OpenBelow,
    };
    let once = Args {
        count: None,
        ..a.clone()
    };
    let session = insert::begin(ctx, &once, entry)?;
    ctx.with_ext(|v: &mut Vim, _| v.inserts.insert(b, session))?;
    Ok(())
}

/// `y`: whole lines leave the cursor where it is, and anything else moves it to the start.
fn yank(ctx: &mut Ctx, b: BufferId, w: WindowId, register: Option<char>, span: Span) -> Result {
    let saved = taken(ctx.text(b)?, span);
    registers::save(ctx, register, saved, true)?;
    let t = ctx.text_mut(b)?;
    if span.lines {
        forget_goal(t, w);
    } else {
        t.set_cursor(w, rest_pos(t, span.beg));
    }
    Ok(())
}

/// Spaces in a shift (scope §10).
const SHIFT_WIDTH: usize = 4;

/// `>` and `<`: each line moves to the next multiple of 4 columns that way. Lines of
/// nothing but spaces end up empty, and empty lines stay empty. The cursor stays in its
/// column, or at the line's end after `$`. It stays on its line with `>`, and with `<`
/// goes to the first line shifted, as in Evil, where only `>` says not to move.
fn shift(t: &mut TextBuf, w: WindowId, span: Span, right: bool) -> Result {
    let sel = t.selection(w).ok_or(Error::Msg("no cursor".into()))?;
    let here = t.line_of(if right { sel.head } else { span.beg });
    let (col, goal) = (t.display_col(sel.head), Goal::decode(sel.goal));
    let mut tx_ops: Vec<(std::ops::Range<usize>, String)> = vec![];
    let mut line = t.line_of(span.beg);
    while line < t.line_count() && t.line_start(line) < span.end {
        let (start, end) = (t.line_start(line), indent_end(t, line));
        let has_text = end < line_end(t, line);
        let indent = t.display_col(end);
        let new = if right {
            (indent / SHIFT_WIDTH + 1) * SHIFT_WIDTH
        } else {
            indent.div_ceil(SHIFT_WIDTH).saturating_sub(1) * SHIFT_WIDTH
        };
        tx_ops.push((start..end, " ".repeat(if has_text { new } else { 0 })));
        line += 1;
    }
    // The empty last line, or an empty text, has no line to shift, so `>` indents it.
    if span.beg == span.end && right {
        tx_ops.push((span.beg..span.beg, " ".repeat(SHIFT_WIDTH)));
    }
    // A first change that only takes away a line's indentation, with the cursor at its start:
    // Evil's `u` goes to the end of the indentation it puts back.
    let first = tx_ops.iter().find(|(r, s)| !(r.is_empty() && s.is_empty()));
    if let Some((range, spaces)) = first
        && sel.head == range.start
        && spaces.is_empty()
    {
        let to = rest_pos(t, range.end);
        t.set_cursor(w, to);
    }
    let mut tx = t.tx(Origin::User);
    for (range, spaces) in tx_ops {
        tx = tx.replace(range, spaces);
    }
    tx.commit()?;
    let to = match goal {
        Some(Goal::Eol) => line_end(t, here),
        _ => pos_at_col(t, here, col),
    };
    t.set_cursor(w, rest_pos(t, to));
    Ok(())
}

/// The other case of one character, or itself if that takes more than one.
fn inverted(c: char) -> char {
    match single(c.to_uppercase()) {
        Some(up) if up != c => up,
        _ => single(c.to_lowercase()).unwrap_or(c),
    }
}

fn single(mut it: impl Iterator<Item = char>) -> Option<char> {
    let c = it.next()?;
    it.next().is_none().then_some(c)
}

/// `g~`, `gU` and `gu`. The cursor goes to the start, or with `~`, past what it changed.
fn recase(t: &mut TextBuf, w: WindowId, op: Operator, span: Span, past_end: bool) -> Result {
    let old = t.rope().byte_slice(span.beg..span.end).to_string();
    let new: String = match op {
        Operator::Upcase => old.to_uppercase(),
        Operator::Downcase => old.to_lowercase(),
        _ => old.chars().map(inverted).collect(),
    };
    let end = span.beg + new.len();
    // As a buffer's first change, Evil's `u` after `gU` and `gu` goes to the change's start.
    if op != Operator::InvertCase && t.rev() == 0 {
        t.set_cursor(w, span.beg);
    }
    // Text replaced by itself is still an undo step, as in Evil, so `u` takes it back.
    t.tx(Origin::User)
        .replace(span.beg..span.end, new)
        .commit()?;
    let pos = if past_end { end } else { span.beg };
    t.set_cursor(w, rest_pos(t, pos));
    Ok(())
}

// ---------------------------------------------------------------- commands with their own motion

/// The motion a command brings with it.
pub type Own = fn(&Start) -> std::result::Result<Target, Fail>;

/// `x`, `X`, `s`, `D`, `C`, `~`: an operator over a motion of its own, without `d`'s habits.
pub fn with_motion(ctx: &mut Ctx, a: &Args, op: Operator, kind: Kind, motion: Own) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let (span, _) = covered(t, w, a.count, op, kind, None, motion)?;
    let how = How {
        past_end: op == Operator::InvertCase,
        ..How::default()
    };
    apply(ctx, a, op, a.register, span, how)
}

/// `S` and `Y`: an operator over whole lines, count of them.
pub fn with_lines(ctx: &mut Ctx, a: &Args, op: Operator) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let span = this_line_and_more(ctx.text(b)?, w, a.count);
    apply(ctx, a, op, a.register, span, How::default())
}

/// `"x`: name a register for the next command. A count typed before it goes, as in Evil.
pub fn name_register(ctx: &mut Ctx, a: &Args) -> Result {
    if let Some(c) = a.char() {
        ctx.set_prefix(die_core::Prefix {
            count: None,
            register: Some(c),
        });
    }
    let (b, w) = buffer_window(ctx, a)?;
    forget_goal(ctx.text_mut(b)?, w);
    Ok(())
}

/// `r`: replace count characters with the next key, and leave the cursor on the last.
/// Enter splits the line instead. Escape or a key that isn't text does nothing.
pub fn replace(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let key = a.keys.last();
    let enter = key.is_some_and(|k| k.chord().key == Key::Enter);
    let tab = key.is_some_and(|k| k.chord().key == Key::Tab);
    let with = if tab {
        Some("\t".to_owned())
    } else {
        a.typed()
    };
    let t = ctx.text_mut(b)?;
    if !enter && with.is_none() {
        forget_goal(t, w);
        return Ok(());
    }
    // Any operator but `c` would do: only `w` tells them apart.
    let motion: Own = motions::forward_char;
    let (span, _) = covered(
        t,
        w,
        a.count,
        Operator::Delete,
        Kind::Exclusive,
        None,
        motion,
    )?;
    if enter {
        t.tx(Origin::User)
            .replace(span.beg..span.end, "\n")
            .commit()?;
        insert::indent_from_above(t, w, t.line_of(span.beg) + 1)?;
        let pos = rest_pos(t, cursor(t, w)?);
        t.set_cursor(w, pos);
        return Ok(());
    }
    let with = with.unwrap_or_default();
    let (mut n, mut p) = (0, span.beg);
    while p < span.end {
        p = next_stop(t, p).unwrap_or(span.end);
        n += 1;
    }
    let new = with.repeat(n);
    t.tx(Origin::User)
        .replace(span.beg..span.end, new)
        .commit()?;
    let last = span.beg + with.len() * n.saturating_sub(1);
    t.set_cursor(w, rest_pos(t, last));
    Ok(())
}

/// `J`: join count lines (at least two), as Emacs's `join-line` does. The whitespace at
/// the join becomes one space, or none next to a closing or opening bracket or an empty
/// line, and the cursor stays there. All of it is one edit, however many lines. On the
/// last line there's nothing to join: the final newline stays, where your Evil's would go
/// (rule 1). On the empty line after that newline it does nothing at all, as Evil's does.
pub fn join(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let start = cursor(t, w)?;
    let here = t.line_of(start);
    if on_last_empty_line(t, start) {
        forget_goal(t, w);
        return Ok(());
    }
    let pos = if here >= last_line(t) {
        if blank_line(t, here) {
            insert::indent_from_above(t, w, here)?;
            cursor(t, w)?
        } else {
            line_end(t, here)
        }
    } else {
        let joins = a.count.unwrap_or(1).clamp(2, last_line(t) + 1 - here) - 1;
        let bytes = after_line(t, here + joins) - t.line_start(here);
        if a.count.is_some_and(|n| n > 1) && bytes > MAX_REPEAT {
            return Err(Error::Msg("Count too large".into()));
        }
        let (edits, pos, was_blank) = joined(t, here, joins);
        let mut tx = t.tx(Origin::User);
        for (range, space) in edits {
            tx = tx.replace(range, space);
        }
        if let Err(e) = tx.commit() {
            // Evil's `J` goes to the next line to join it, and stays there if it can't.
            t.set_cursor(w, t.line_start(here + 1));
            return Err(e);
        }
        if was_blank {
            insert::indent_from_above(t, w, here)?;
            cursor(t, w)?
        } else {
            pos
        }
    };
    t.set_cursor(w, rest_pos(t, pos));
    Ok(())
}

/// The edits that join `joins` lines onto `here`, in the text as it is now. `.1` is where the
/// last join lands once they're made, and `.2` says the line was blank before that join.
fn joined(t: &TextBuf, here: usize, joins: usize) -> (Vec<Edit>, usize, bool) {
    let mut edits: Vec<Edit> = vec![];
    for line in here..here + joins {
        let newline = line_end(t, line);
        // Back over the spaces at the end of what's joined so far. A line of nothing but
        // spaces gets as far as the last join, which this one then takes in.
        let floor = edits.last().map_or(0, |e| e.0.end);
        let mut before = Chars::at(t, newline);
        while before.pos() > floor && matches!(before.prev_ch(), Some(' ' | '\t')) {
            before.back();
        }
        let from = match edits.last() {
            Some(e) if e.0.end == before.pos() => {
                let from = e.0.start;
                edits.pop();
                from
            }
            _ => before.pos(),
        };
        let mut after = Chars::at(t, newline + 1);
        while matches!(after.ch(), Some(' ' | '\t')) {
            after.fwd();
        }
        let before_join = Chars::at(t, from).prev_ch();
        let bare = matches!(after.ch(), None | Some('\n' | ')' | ']' | '}'))
            || matches!(before_join, None | Some('\n' | '(' | '[' | '{'));
        edits.push((from..after.pos(), if bare { "" } else { " " }));
    }
    let last = edits.last().map_or(0..0, |e| e.0.clone());
    let pos = edits
        .iter()
        .filter(|e| e.0.start < last.start)
        .fold(last.start, |p, e| p - e.0.len() + e.1.len());
    (edits, pos, last.start == t.line_start(here))
}

/// `p` and `P`, as Evil's `evil-paste-after` and `evil-paste-before`. Lines go below or
/// above this line, and the cursor goes to their first non-blank. Characters go after or
/// before the cursor, and it goes to the last one. Both are one edit, made first.
pub fn paste(ctx: &mut Ctx, a: &Args, before: bool) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    // Whatever happens next, the goal column goes, as after any command but `j` and `k`.
    forget_goal(ctx.text_mut(b)?, w);
    let r = registers::read(ctx, a.register)?;
    let n = a.count.unwrap_or(1).max(1);
    if n > 1 && r.text.len().saturating_mul(n) > MAX_REPEAT {
        return Err(Error::Msg("Count too large".into()));
    }
    let text = r.text.repeat(n);
    if text.is_empty() {
        return Ok(());
    }
    let t = ctx.text_mut(b)?;
    let p = cursor(t, w)?;
    let line = t.line_of(p);
    let pos = match (r.kind, before) {
        (RegKind::Line, true) => {
            let at = t.line_start(line);
            t.tx(Origin::User).insert(at, text).commit()?;
            indent_end(t, line)
        }
        (RegKind::Line, false) => {
            let at = line_end(t, line);
            let body = text.strip_suffix('\n').unwrap_or(&text);
            t.tx(Origin::User)
                .insert(at, format!("\n{body}"))
                .commit()?;
            indent_end(t, line + 1)
        }
        // A block register pastes as characters until V4 brings visual block.
        (_, _) => {
            let at = if before || p >= line_end(t, line) {
                p
            } else {
                next_stop(t, p).unwrap_or(p)
            };
            t.tx(Origin::User).insert(at, text.as_str()).commit()?;
            let end = at + text.len();
            // `p` doesn't step back over a newline it pasted, and `P` does.
            if !before && t.line_start(t.line_of(end)) == end {
                end
            } else {
                prev_stop(t, end).unwrap_or(end)
            }
        }
    };
    t.set_cursor(w, rest_pos(t, pos));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_multiply_and_saturate() {
        assert_eq!(counts(None, None), None);
        assert_eq!(counts(Some(2), None), Some(2));
        assert_eq!(counts(None, Some(3)), Some(3));
        assert_eq!(counts(Some(2), Some(3)), Some(6));
        assert_eq!(counts(Some(usize::MAX), Some(2)), Some(usize::MAX));
    }

    #[test]
    fn exclusive_ranges_ending_at_a_lines_start_step_back() {
        let t = TextBuf::new("ab\ncd\nef\n");
        let span = |beg, end| expand(&t, beg, end, Kind::Exclusive);
        assert_eq!(span(1, 2), Span::chars(1, 2), "not at a line's start");
        assert_eq!(span(1, 3), Span::chars(1, 2), "stops before the newline");
        assert_eq!(span(0, 3), Span::whole_lines(0, 3), "from the line's start");
        assert_eq!(span(0, 6), Span::whole_lines(0, 6));
        assert_eq!(span(1, 6), Span::chars(1, 5));
        assert_eq!(span(4, 4), Span::chars(4, 4), "empty stays empty");
    }

    #[test]
    fn inclusive_ranges_take_the_whole_grapheme() {
        let t = TextBuf::new("cafe\u{301} x\n");
        assert_eq!(
            expand(&t, 0, 3, Kind::Inclusive),
            Span::chars(0, 6),
            "the mark goes with its letter"
        );
    }

    #[test]
    fn inclusive_ranges_stop_short_of_a_newline() {
        let t = TextBuf::new("ab\n\ncd\n");
        assert_eq!(expand(&t, 0, 2, Kind::Inclusive), Span::chars(0, 2));
        assert_eq!(
            expand(&t, 3, 3, Kind::Inclusive),
            Span::chars(3, 4),
            "unless that's where it started"
        );
    }

    #[test]
    fn inverting_a_character_with_no_single_other_case_leaves_it() {
        assert_eq!(inverted('a'), 'A');
        assert_eq!(inverted('A'), 'a');
        assert_eq!(inverted('1'), '1');
        assert_eq!(inverted('ß'), 'ß');
        assert_eq!(inverted('é'), 'É');
    }
}
