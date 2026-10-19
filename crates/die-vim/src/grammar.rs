//! die-vim's grammar core: how keys become motions and edits.
//!
//! Normal state reads `[count] command`. A count is a prefix that core holds for
//! the next command (D9: [`Ctx::set_prefix`], delivered in [`Args::count`]); so
//! is a register (`"x`, V3). A command is a motion, an operator or an action:
//!
//! - A **motion** ([`Motion`]) is a plain function from where it starts
//!   ([`Start`]) to where it lands ([`Target`]). It reads the text, layout queries
//!   and its count, never other editor state, so M2's copy state can reuse it on
//!   a terminal's text. Its [`Kind`] says how an operator treats the range.
//! - An **operator** (V3) waits for a motion or text object: core's state is
//!   `OperatorPending`, and die-vim keeps the operator with its count and
//!   register. [`run_motion`] is where a motion meets it: with an operator
//!   pending, the range from the cursor to the motion's target goes to the
//!   operator instead of moving the cursor, and the two counts multiply.
//! - An **action** is anything else: entering insert state, undo. Insert sessions
//!   are one undo step each and record their keys, so a count (and `.`, V4) can
//!   replay them (`insert.rs`).
//!
//! In normal state the cursor rests on a character, never on a line's newline
//! unless the line is empty, and never past the last line (`text::normal_pos`).

use crate::text::normal_pos;
use die_core::{Args, BufferId, Ctx, Error, Prefix, Result, Selection, TextBuf, WindowId};

/// The column vertical motions aim for, kept across a run of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// A display column.
    Col(usize),
    /// The end of each line, after `$`.
    Eol,
}

impl Goal {
    /// As stored in a [`Selection`]'s goal column.
    pub fn encode(g: Option<Goal>) -> Option<usize> {
        g.map(|g| match g {
            Goal::Col(c) => c,
            Goal::Eol => usize::MAX,
        })
    }
    /// Back from a [`Selection`]'s goal column.
    pub fn decode(c: Option<usize>) -> Option<Goal> {
        c.map(|c| {
            if c == usize::MAX {
                Goal::Eol
            } else {
                Goal::Col(c)
            }
        })
    }
}

/// How an operator treats the range a motion covers (V3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The range stops before the target (`w`, `b`, `h`).
    Exclusive,
    /// The range includes the target (`e`, `f`, `$`).
    Inclusive,
    /// Whole lines, from the start's line to the target's (`j`, `k`, `G`).
    Linewise,
}

/// Where a motion starts, and what it may read.
pub struct Start<'a> {
    /// The text, with its layout queries.
    pub text: &'a TextBuf,
    /// The cursor.
    pub pos: usize,
    /// The count typed before the motion, if any.
    pub count: Option<usize>,
    /// The goal column a run of vertical motions keeps.
    pub goal: Option<Goal>,
    /// The character argument of `f`, `F`, `t` and `T`.
    pub arg: Option<char>,
}

impl Start<'_> {
    /// The count, 1 if none was typed.
    pub fn n(&self) -> usize {
        self.count.unwrap_or(1).max(1)
    }
}

/// Where a motion lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    /// The new cursor. [`run_motion`] moves it to where normal state allows.
    pub pos: usize,
    /// The goal to keep for the next vertical motion; None takes the new column.
    pub goal: Option<Goal>,
    /// The motion moved, but fewer times than its count: like Evil, it signals this.
    pub short: Option<&'static str>,
}

impl Target {
    /// `pos`, taking the new column as the goal, with nothing to signal.
    pub fn at(pos: usize) -> Target {
        Target {
            pos,
            goal: None,
            short: None,
        }
    }
}

/// A motion moved nowhere; the message says why (Evil signals an error).
pub type Fail = &'static str;

/// A motion as normal state binds it.
pub struct Motion {
    /// Its command's name.
    pub name: &'static str,
    /// Its keys in normal state.
    pub keys: &'static str,
    /// How an operator takes its range.
    pub kind: Kind,
    /// Its command's doc string.
    pub doc: &'static str,
    /// Where it lands.
    pub run: fn(&Start) -> std::result::Result<Target, Fail>,
}

/// The buffer and window a command acts on: the ones it was invoked in, else the
/// focused ones.
pub fn buffer_window(ctx: &Ctx, a: &Args) -> Result<(BufferId, WindowId)> {
    let w = a
        .window
        .or(ctx.focused())
        .ok_or(Error::Msg("no window".into()))?;
    let b = a
        .buffer
        .or(ctx.window_buffer(w))
        .ok_or(Error::Msg("no buffer".into()))?;
    Ok((b, w))
}

/// A count digit: `1`-`9`, and `0` after another digit. The count stays core's
/// prefix for the next command, keeping any register already typed.
pub fn count_digit(ctx: &mut Ctx, a: &Args, d: usize) -> Result {
    ctx.set_prefix(Prefix {
        count: Some(a.count.unwrap_or(0).saturating_mul(10).saturating_add(d)),
        register: a.register,
    });
    Ok(())
}

/// Run a motion from the cursor with the command's count. `arg` is the argument
/// of `f`, `F`, `t` and `T`. V3: a pending operator takes the range from the
/// cursor to the target here, treated as `kind` says, instead of the cursor moving.
pub fn run_motion(
    ctx: &mut Ctx,
    a: &Args,
    _kind: Kind,
    arg: Option<char>,
    motion: impl Fn(&Start) -> std::result::Result<Target, Fail>,
) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let sel = t.selection(w).ok_or(Error::Msg("no cursor".into()))?;
    let start = Start {
        text: t,
        pos: sel.head,
        count: a.count,
        goal: Goal::decode(sel.goal),
        arg,
    };
    let to = motion(&start).map_err(|e| Error::Msg(e.into()))?;
    let pos = normal_pos(t, to.pos);
    t.set_selection(w, Selection::cursor(pos).with_goal(Goal::encode(to.goal)));
    match to.short {
        Some(e) => Err(Error::Msg(e.into())),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::Goal;

    #[test]
    fn goals_round_trip_through_a_selection() {
        for g in [
            None,
            Some(Goal::Col(0)),
            Some(Goal::Col(7)),
            Some(Goal::Eol),
        ] {
            assert_eq!(Goal::decode(Goal::encode(g)), g);
        }
    }
}
