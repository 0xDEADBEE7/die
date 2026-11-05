//! Scrolling (scope §10): `C-d C-f C-b C-e C-y`, `zz zt zb`, and the recentring your Emacs does
//! when a cursor leaves the window. Each follows your Evil's command of the same name; all count
//! screen rows, so a wrapped line takes as many as it shows.

use crate::grammar::{Fail, Goal, Kind, Target, buffer_window, forget_goal, run_motion};
use crate::motions::{goal_at, goal_pos, row_pos};
use crate::text::{last_line, line_end};
use die_core::{Args, Ctx, Error, Result, Selection, TextBuf, WindowId};

/// `next-screen-context-lines`: the rows a page keeps from the page before.
const PAGE_OVERLAP: usize = 2;

/// After a command: a cursor that left the window brings it back with the cursor in the middle,
/// as your Emacs's redisplay does, with `scroll-conservatively` at 0. That's why `20G` recentres.
pub fn reveal(ctx: &mut Ctx, a: &Args) {
    let Ok((b, w)) = buffer_window(ctx, a) else {
        return;
    };
    let Ok(t) = ctx.text(b) else { return };
    let Some(head) = t.cursor(w) else { return };
    if t.screen_pos(w, head).is_some() {
        return;
    }
    let Some(rows) = t.viewport(w).map(|v| v.geometry.rows) else {
        return;
    };
    if let Ok(t) = ctx.text_mut(b) {
        t.recenter(w, rows / 2);
    }
}

/// What a scrolling command starts from.
fn view(ctx: &mut Ctx, a: &Args) -> Result<(die_core::BufferId, WindowId, Selection, usize)> {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text(b)?;
    let sel = t.selection(w).ok_or(Error::Msg("no cursor".into()))?;
    let rows = t.viewport(w).ok_or(Error::Msg("no window".into()))?;
    Ok((b, w, sel, rows.geometry.rows))
}

/// Put the cursor at `pos`, in visual state moving the selection's head.
fn land(ctx: &mut Ctx, a: &Args, pos: usize, goal: Option<Goal>) -> Result {
    run_motion(ctx, a, Kind::Exclusive, None, move |_| {
        Ok(Target {
            goal,
            ..Target::at(pos)
        })
    })
}

fn fail(t: &mut TextBuf, w: WindowId, e: Fail) -> Result {
    forget_goal(t, w);
    Err(Error::Msg(e.into()))
}

/// Scroll down, keeping off the empty row after a final newline, which a cursor can't follow.
/// Returns the rows it scrolled.
fn scroll_down(t: &mut TextBuf, w: WindowId, n: usize) -> usize {
    let n = isize::try_from(n).unwrap_or(isize::MAX);
    let mut moved = t.scroll_rows(w, n).unwrap_or(0);
    let top = t.scroll(w).map_or(0, |(top, _)| top);
    if moved > 0 && t.line_of(top) > last_line(t) {
        t.scroll_rows(w, -1);
        moved -= 1;
    }
    moved
}

/// A cursor the view left behind comes to the row it left by.
fn pushed(t: &TextBuf, w: WindowId, head: usize, goal: Goal) -> usize {
    if t.screen_pos(w, head).is_some() {
        return head;
    }
    let Some(v) = t.viewport(w) else { return head };
    // `v.top` is the display line's start, which a wrapped line's later rows share.
    let first = v.rows.first().and_then(|r| r.stops().first());
    let row = if first.is_some_and(|st| head < st.pos) {
        v.rows.first()
    } else {
        v.rows.last()
    };
    row.map_or(head, |r| row_pos(t, r, goal))
}

/// `C-d`: the window and the cursor go down half a window, or count rows. A count doesn't stick.
/// With the text's end in view only the cursor moves, and once the scroll brings the end into
/// view the end goes to the window's bottom.
pub fn half_down(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w, sel, rows) = view(ctx, a)?;
    let t = ctx.text_mut(b)?;
    if t.line_of(sel.head) >= last_line(t) {
        return fail(t, w, "End of buffer");
    }
    let n = a.count.filter(|&n| n > 0).unwrap_or(rows / 2);
    let goal = goal_at(Goal::decode(sel.goal), t, sel.head);
    let at_row = t.screen_pos(w, sel.head).map_or(0, |(r, _)| r);
    let no_view = || Error::Msg("no window".into());
    let target = if t.screen_pos(w, line_end(t, last_line(t))).is_some() {
        let v = t.viewport(w).ok_or_else(no_view)?;
        let row = v.rows.get(at_row.saturating_add(n)).or(v.rows.last());
        row.map_or(sel.head, |r| row_pos(t, r, goal))
    } else {
        let wanted = isize::try_from(n).unwrap_or(isize::MAX);
        let moved = t.scroll_rows(w, wanted).unwrap_or(0);
        let v = t.viewport(w).ok_or_else(no_view)?;
        let row = v.rows.get(at_row).filter(|_| moved == n);
        let target = row.or(v.rows.last()).map_or(sel.head, |r| row_pos(t, r, goal));
        if let Some(short) = rows.checked_sub(v.rows.len()).filter(|&s| s > 0) {
            t.scroll_rows(w, -isize::try_from(short).unwrap_or(0));
        }
        target
    };
    land(ctx, a, target, Some(goal))
}

/// `C-f` and `C-b`: count windows, less two rows. Past the text's end or start the cursor goes
/// there and the view stays; only a cursor already there fails.
pub fn page(ctx: &mut Ctx, a: &Args, down: bool) -> Result {
    let (b, w, sel, rows) = view(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let goal = goal_at(Goal::decode(sel.goal), t, sel.head);
    let step = rows.saturating_sub(PAGE_OVERLAP).max(1);
    let mut end = None;
    for i in 0..a.count.unwrap_or(1).max(1) {
        let moved = if down {
            scroll_down(t, w, step)
        } else {
            t.scroll_rows(w, -isize::try_from(step).unwrap_or(isize::MAX))
                .unwrap_or(0)
        };
        if down && moved == step || !down && moved > 0 {
            continue;
        }
        if down {
            t.scroll_rows(w, -isize::try_from(moved).unwrap_or(0));
        }
        let (line, there, err) = if down {
            let line = last_line(t);
            (line, t.line_of(sel.head) >= line, "End of buffer")
        } else {
            (0, sel.head == 0, "Beginning of buffer")
        };
        if i == 0 && there {
            return fail(t, w, err);
        }
        end = Some(goal_pos(t, line, goal));
        break;
    }
    let target = end.unwrap_or_else(|| pushed(t, w, sel.head, goal));
    land(ctx, a, target, Some(goal))
}

/// `C-e` and `C-y`: count rows, the cursor staying unless the view leaves it behind. Failing at
/// the text's end or start, they forget the goal column, as any command but a vertical motion
/// does.
pub fn rows(ctx: &mut Ctx, a: &Args, down: bool) -> Result {
    let (b, w, sel, _) = view(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let n = a.count.unwrap_or(1).max(1);
    let moved = if down {
        scroll_down(t, w, n)
    } else {
        t.scroll_rows(w, -isize::try_from(n).unwrap_or(isize::MAX))
            .unwrap_or(0)
    };
    if moved == 0 {
        let e = if down {
            "End of buffer"
        } else {
            "Beginning of buffer"
        };
        return fail(t, w, e);
    }
    let goal = goal_at(Goal::decode(sel.goal), t, sel.head);
    let target = pushed(t, w, sel.head, goal);
    land(ctx, a, target, None)
}

/// `zz`, `zt` and `zb`: the cursor's line, or line count at the same column, goes to the window's
/// row `which(rows)`. The cursor's row on a wrapped line counts, as `recenter` does.
pub fn put_line(ctx: &mut Ctx, a: &Args, which: fn(usize) -> usize) -> Result {
    let (b, w, sel, rows) = view(ctx, a)?;
    let t = ctx.text_mut(b)?;
    let target = match a.count {
        Some(n) => {
            let line = n.saturating_sub(1).min(last_line(t));
            goal_pos(t, line, Goal::Col(t.display_col(sel.head)))
        }
        None => sel.head,
    };
    land(ctx, a, target, None)?;
    let t = ctx.text_mut(b)?;
    t.recenter(w, which(rows));
    Ok(())
}
