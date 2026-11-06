//! `/ ? n N * #`: incremental, every match highlighted, smartcase. A search is an exclusive
//! motion, so it goes through [`run_motion`] like any other: it moves the cursor, moves a
//! selection's head, or gives a waiting operator its range.
//!
//! While a `/` or `?` is open, each change searches from where it began and moves the view
//! there. The view goes back before the answer runs, so an answer or a cancel starts from
//! the same place.

use crate::Vim;
use crate::grammar::{Fail, Kind, Start, Target, buffer_window, run_motion};
use crate::operators;
use crate::text::{Chars, Class, class, normal_pos, same_word};
use crate::visual;
use die_core::{Args, Ctx, Dir, Error, Key, KeyEvent, Pattern, PromptSpec, Result, Selection};
use die_core::{State, TextBuf};
use std::collections::HashMap;

/// What `n` and `N` repeat.
#[derive(Clone)]
pub struct Last {
    pat: Pattern,
    forward: bool,
}

pub const NONE_YET: &str = "No previous search";

/// Smartcase: a pattern with no capital in it ignores case. What follows a backslash isn't
/// text, so `\S` has no capital.
pub fn ignores_case(src: &str) -> bool {
    let mut chars = src.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            chars.next();
        } else if c.is_uppercase() {
            return false;
        }
    }
    true
}

fn compile(src: &str) -> Result<Pattern> {
    Pattern::new(src, ignores_case(src))
}

fn is_word(c: char) -> bool {
    class(c, false) == Class::Word
}

/// One landing from `pos`. Evil takes an empty match at the cursor on a typed `/` or `?`, on
/// any search backward, and going forward from the end of the text, where there's no next
/// character to start from. So `/^` from a line's start stays, and `?^` then `n` stays too.
/// With `rests`, the cursor can't rest on a newline, so an `n` whose match is on the newline
/// after it (`/$`) would leave it where it is: that one is passed over.
fn step(
    t: &TextBuf,
    pat: &Pattern,
    pos: usize,
    forward: bool,
    rests: bool,
    typed: bool,
) -> Option<usize> {
    let dir = if forward { Dir::Forward } else { Dir::Backward };
    if (typed || !forward || pos == t.len()) && t.empty_match_at(pat, pos, dir) {
        return Some(pos);
    }
    let to = t.search(pat, pos, dir)?.range.start;
    if rests && !typed && to != pos && normal_pos(t, to) == pos {
        return Some(t.search(pat, to, dir)?.range.start);
    }
    Some(to)
}

/// Where `n` searches in a row land from `from`, the first of them `typed` into a prompt. Once
/// a landing repeats the search has gone all the way round, so what's left of the count is
/// taken modulo that round: a huge count costs one pass over the text.
fn find(
    t: &TextBuf,
    pat: &Pattern,
    from: usize,
    n: usize,
    forward: bool,
    rests: bool,
    typed: bool,
) -> Option<usize> {
    let mut pos = step(t, pat, from, forward, rests, typed)?;
    let mut left = n.max(1) - 1;
    let mut seen = HashMap::from([(pos, left)]);
    while left > 0 {
        pos = step(t, pat, pos, forward, rests, false)?;
        left -= 1;
        if let Some(was) = seen.insert(pos, left) {
            for _ in 0..left % (was - left) {
                pos = step(t, pat, pos, forward, rests, false)?;
            }
            break;
        }
    }
    Some(pos)
}

fn motion(
    s: &Start,
    pat: &Pattern,
    forward: bool,
    typed: bool,
) -> std::result::Result<Target, Fail> {
    let rests = s.op.is_none() && !s.visual;
    let to = find(s.text, pat, s.pos, s.n(), forward, rests, typed).ok_or("Search failed")?;
    Ok(Target {
        to_last_line: true,
        ..Target::at(to)
    })
}

fn remember(ctx: &mut Ctx, last: Last) -> Result {
    ctx.with_ext(|v: &mut Vim, _| v.search = Some(last))?;
    Ok(())
}

fn last(ctx: &mut Ctx) -> Result<Option<Last>> {
    Ok(ctx.with_ext(|v: &mut Vim, _| v.search.clone())?)
}

/// `:s` leaves its pattern for `n` and keeps the way searches go. Before any search that's
/// backward, as `isearch-forward` starts out nil in your Evil.
pub fn remember_pattern(ctx: &mut Ctx, pat: Pattern) -> Result {
    let forward = last(ctx)?.is_some_and(|l| l.forward);
    remember(ctx, Last { pat, forward })
}

/// What an empty `:s` pattern means.
pub fn last_pattern(ctx: &mut Ctx) -> Result<Option<Pattern>> {
    Ok(last(ctx)?.map(|l| l.pat))
}

/// Search, and show every match. A match in hidden text is revealed first, since a cursor
/// can't rest in hidden text (`search-invisible t`). A search that fails is an error, so it
/// ends a macro (contract §7).
fn go(ctx: &mut Ctx, a: &Args, pat: &Pattern, forward: bool, typed: bool) -> Result {
    ctx.set_search_highlight(Some(pat.clone()));
    let (b, w) = buffer_window(ctx, a)?;
    if ctx.state_of(b) != State::OperatorPending {
        let t = ctx.text(b)?;
        let head = t.selection(w).map_or(0, |s| s.head);
        let rests = !visual::is_on(ctx, b);
        let n = a.count.unwrap_or(1);
        if let Some(to) = find(ctx.text(b)?, pat, head, n, forward, rests, typed) {
            ctx.reveal(b, to)?;
        }
    }
    let run = |s: &Start| motion(s, pat, forward, typed);
    run_motion(ctx, a, Kind::Exclusive, None, run).map_err(|e| match e {
        Error::Msg(m) if m == "Search failed" => Error::Msg(format!("{m}: {}", pat.as_str())),
        e => e,
    })
}

/// `/` and `?`: read a pattern, moving the view to its first match as it's typed.
pub fn open(ctx: &mut Ctx, a: &Args, forward: bool) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text(b)?;
    let sel = t.selection(w).ok_or(Error::Msg("no cursor".into()))?;
    let top = t.scroll(w);
    let back = move |ctx: &mut Ctx| -> Result {
        let t = ctx.text_mut(b)?;
        t.set_selection(w, sel);
        if let Some((top, row)) = top {
            t.set_scroll(w, top, row);
        }
        Ok(())
    };
    let visual = visual::is_on(ctx, b);
    let n = a.count.unwrap_or(1);
    let spec = PromptSpec::new(if forward { "/" } else { "?" })
        .history("search")
        // `<BS>` on an empty line leaves it, as in Vim. Your Evil's `/` is isearch, which stays
        // (scope §10).
        .backspace_cancels()
        .evil_keys()
        .on_change(move |ctx, text| {
            back(ctx)?;
            let pat = compile(text).ok().filter(|_| !text.is_empty());
            ctx.set_search_highlight(pat.clone());
            let t = ctx.text_mut(b)?;
            if let Some(to) = pat.and_then(|p| find(t, &p, sel.head, n, forward, !visual, true)) {
                let anchor = if visual { sel.anchor } else { to };
                t.set_selection(
                    w,
                    Selection {
                        anchor,
                        head: to,
                        ..sel
                    },
                );
            }
            Ok(())
        });
    let a = a.clone();
    ctx.prompt(spec, move |ctx, answer| {
        back(ctx)?;
        answered(ctx, &a, answer, forward)
    });
    Ok(())
}

fn answered(ctx: &mut Ctx, a: &Args, answer: Option<String>, forward: bool) -> Result {
    let before = last(ctx)?;
    let Some(text) = answer else {
        ctx.set_search_highlight(before.map(|l| l.pat));
        return operators::cancel(ctx, a);
    };
    let pat = if text.is_empty() {
        let repeat = before.as_ref().map(|l| l.pat.clone());
        repeat.ok_or(Error::Msg(NONE_YET.into()))
    } else {
        compile(&text)
    };
    let pat = match pat {
        Ok(pat) => pat,
        Err(e) => {
            ctx.set_search_highlight(before.map(|l| l.pat));
            operators::cancel(ctx, a)?;
            return Err(e);
        }
    };
    remember(
        ctx,
        Last {
            pat: pat.clone(),
            forward,
        },
    )?;
    // What `.` replays has to say what was typed into the prompt too.
    let mut a = a.clone();
    let typed = text.chars().map(|c| KeyEvent::press(Key::Char(c)));
    a.keys.extend(typed.chain([KeyEvent::press(Key::Enter)]));
    go(ctx, &a, &pat, forward, true)
}

/// `n`, and `N` the other way, which doesn't change the way the next `n` goes.
pub fn again(ctx: &mut Ctx, a: &Args, reverse: bool) -> Result {
    let Some(l) = last(ctx)? else {
        operators::cancel(ctx, a)?;
        return Err(Error::Msg(NONE_YET.into()));
    };
    go(ctx, a, &l.pat, l.forward != reverse, false)
}

/// `*` and `#`: the word under the cursor, or the next one along the way they search. It
/// matches as Evil's `\\<word\\>` does, by die-vim's own test of a word character, so a run of
/// punctuation never matches and the search fails.
pub fn word(ctx: &mut Ctx, a: &Args, forward: bool) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text(b)?;
    let head = t.selection(w).map_or(0, |s| s.head);
    let Some(text) = word_at(t, head, forward) else {
        operators::cancel(ctx, a)?;
        return Err(Error::Msg("No word under the cursor".into()));
    };
    let pat = Pattern::word(&text, ignores_case(&text), is_word)?;
    remember(
        ctx,
        Last {
            pat: pat.clone(),
            forward,
        },
    )?;
    go(ctx, a, &pat, forward, false)
}

fn word_at(t: &TextBuf, pos: usize, forward: bool) -> Option<String> {
    let thing = |c| !matches!(class(c, false), Class::Blank | Class::Newline);
    let mut c = Chars::at(t, pos);
    while !c.ch().is_some_and(thing) {
        if !(if forward { c.fwd() } else { c.back() }) {
            return None;
        }
    }
    while let (Some(p), Some(h)) = (c.prev_ch(), c.ch()) {
        if !same_word(p, h, false) {
            break;
        }
        c.back();
    }
    let mut word = String::new();
    while let Some(h) = c.ch() {
        word.push(h);
        c.fwd();
        if !c.ch().is_some_and(|n| same_word(h, n, false)) {
            break;
        }
    }
    Some(word)
}
