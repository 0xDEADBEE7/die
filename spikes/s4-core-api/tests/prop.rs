//! Criterion 9: across random sequences of edits (User and silent Extension, inserts and deletes
//! spanning hidden ranges), decoration changes, resizes, settings changes, undo/redo and motions:
//! - the cursor is never inside a hidden range (checked against a naive definition),
//! - the set of cursor positions matches that definition exactly,
//! - render, the motion queries and an independent naive layout oracle agree on every row and stop.
use proptest::prelude::*;
use s4core::*;
use std::cell::RefCell;
use std::rc::Rc;
use unicode_width::UnicodeWidthChar;

thread_local! {
    static STATS: RefCell<std::collections::BTreeMap<&'static str, usize>> = RefCell::default();
}
fn bump(k: &'static str) {
    STATS.with(|s| *s.borrow_mut().entry(k).or_default() += 1);
}

#[derive(Debug, Clone)]
enum Op {
    Ins {
        at: u16,
        s: String,
        user: bool,
        silent: bool,
    },
    Del {
        at: u16,
        len: u8,
        user: bool,
        silent: bool,
    },
    Hide {
        at: u16,
        len: u8,
        repl: Option<String>,
    },
    Unhide(u8),
    Virt {
        at: u16,
        s: String,
    },
    Ro {
        at: u16,
        len: u8,
    },
    Resize {
        cols: u8,
        rows: u8,
    },
    Set {
        nums: bool,
        maxw: Option<u8>,
        center: bool,
        wrap: bool,
        tab: u8,
    },
    J(bool),
    Gj(i8),
    H(bool),
    Cursor(u16),
    Undo,
    Redo,
    UndoRedo,
    Group(bool),
}

fn text_s(max: usize) -> impl Strategy<Value = String> {
    let ch = prop_oneof![5 => Just('a'), 2 => Just('b'), 2 => Just(' '), 2 => Just('\n'), 1 => Just('\t'), 1 => Just('中'), 1 => Just('é')];
    prop::collection::vec(ch, 0..max).prop_map(|v| v.into_iter().collect())
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (any::<u16>(), text_s(6), any::<bool>(), any::<bool>()).prop_map(|(at, s, user, silent)| Op::Ins { at, s, user, silent }),
        3 => (any::<u16>(), 0u8..14, any::<bool>(), any::<bool>()).prop_map(|(at, len, user, silent)| Op::Del { at, len, user, silent }),
        3 => (any::<u16>(), 0u8..20, prop::option::of(text_s(5))).prop_map(|(at, len, repl)| Op::Hide { at, len, repl }),
        1 => any::<u8>().prop_map(Op::Unhide),
        1 => (any::<u16>(), text_s(4)).prop_map(|(at, s)| Op::Virt { at, s }),
        1 => (any::<u16>(), 0u8..10).prop_map(|(at, len)| Op::Ro { at, len }),
        1 => (8u8..40, 1u8..8).prop_map(|(cols, rows)| Op::Resize { cols, rows }),
        1 => (any::<bool>(), prop::option::of(2u8..30), any::<bool>(), prop::bool::weighted(0.8), 1u8..9)
            .prop_map(|(nums, maxw, center, wrap, tab)| Op::Set { nums, maxw, center, wrap, tab }),
        4 => any::<bool>().prop_map(Op::J),
        4 => (-3i8..=3).prop_map(Op::Gj),
        2 => any::<bool>().prop_map(Op::H),
        2 => any::<u16>().prop_map(Op::Cursor),
        1 => Just(Op::Undo),
        1 => Just(Op::Redo),
        1 => Just(Op::UndoRedo),
        1 => any::<bool>().prop_map(Op::Group),
    ]
}

fn at(t: &TextBuf, at: u16) -> usize {
    let n = t.rope().len_chars();
    t.rope().char_to_byte(at as usize % (n + 1))
}
fn chars_after(t: &TextBuf, p: usize, k: usize) -> usize {
    let c = t.rope().byte_to_char(p);
    t.rope().char_to_byte((c + k).min(t.rope().len_chars()))
}

fn apply(ctx: &mut Ctx, b: BufferId, w: WindowId, op: &Op, hides: &mut Vec<DecorationId>) {
    let origin = |user: bool| {
        if user {
            Origin::User
        } else {
            Origin::Extension
        }
    };
    if let Op::Resize { cols, rows } = op {
        ctx.resize(w, *cols as usize, *rows as usize).unwrap();
        return;
    }
    let t = ctx.text_mut(b).unwrap();
    let before = (t.rope().to_string(), t.rev());
    let edit = |t: &mut TextBuf, r: std::ops::Range<usize>, s: &str, user: bool, silent: bool| {
        let mut tx = t.tx(origin(user)).replace(r, s);
        if silent {
            tx = tx.silent();
        }
        let r = tx.commit();
        bump(if r.is_ok() {
            if silent {
                "edit: silent ok"
            } else {
                "edit: recorded ok"
            }
        } else {
            "edit: rejected (read-only)"
        });
        if r.is_err() {
            assert_eq!(
                (t.rope().to_string(), t.rev()),
                before,
                "a failed transaction changed nothing"
            );
        }
    };
    match op {
        Op::Ins {
            at: a,
            s,
            user,
            silent,
        } => {
            let p = at(t, *a);
            edit(t, p..p, s, *user, *silent);
        }
        Op::Del {
            at: a,
            len,
            user,
            silent,
        } => {
            let p = at(t, *a);
            let q = chars_after(t, p, *len as usize);
            edit(t, p..q, "", *user, *silent);
        }
        Op::Hide { at: a, len, repl } => {
            let p = at(t, *a);
            let q = chars_after(t, p, *len as usize);
            hides.push(
                t.decorate(
                    p..q,
                    Deco::Hide {
                        replacement: repl.clone(),
                        reveal: Reveal::Remove,
                    },
                )
                .unwrap(),
            );
        }
        Op::Unhide(i) => {
            if !hides.is_empty() {
                let id = hides.remove(*i as usize % hides.len());
                t.undecorate(id);
            }
        }
        Op::Virt { at: a, s } => {
            let p = at(t, *a);
            t.decorate(p..p, Deco::Virtual(s.clone())).unwrap();
        }
        Op::Ro { at: a, len } => {
            let p = at(t, *a);
            let q = chars_after(t, p, *len as usize);
            t.decorate(p..q, Deco::ReadOnly).unwrap();
        }
        Op::Set {
            nums,
            maxw,
            center,
            wrap,
            tab,
        } => {
            let s = t.settings_mut();
            s.line_numbers = if *nums {
                LineNumbers::Absolute
            } else {
                LineNumbers::Off
            };
            s.max_width = maxw.map(|m| m as usize);
            s.center = *center;
            s.wrap = *wrap;
            s.tab_width = *tab as usize;
        }
        Op::J(down) => {
            let sel = t.selection(w).unwrap();
            let goal = sel.goal.unwrap_or_else(|| t.display_col(sel.head));
            let dir = if *down { Dir::Forward } else { Dir::Backward };
            if let Some(l) = t.next_visible(t.line_of(sel.head), dir) {
                let p = t.pos_at_vcol(l, goal);
                assert!(t.is_stop(p), "pos_at_vcol returned a hidden position");
                t.set_selection(w, Selection::cursor(p).with_goal(Some(goal)));
            }
        }
        Op::Gj(n) => {
            let c = t.cursor(w).unwrap();
            let (p, _) = t.move_visual(w, c, *n as isize, None).unwrap();
            if p != c && t.line_of(p) == t.line_of(c) {
                bump("gj/gk within one wrapped line");
            }
            assert!(t.is_stop(p), "move_visual returned a hidden position");
            t.set_cursor(w, p);
        }
        Op::H(right) => {
            let c = t.cursor(w).unwrap();
            let (dir, k) = if *right {
                (Dir::Forward, 1)
            } else {
                (Dir::Backward, 0)
            };
            let ci = t.rope().byte_to_char(c);
            let nc = if *right { ci + k } else { ci.saturating_sub(1) };
            let p = t.rope().char_to_byte(nc.min(t.rope().len_chars()));
            let q = t.snap(p, Some(dir));
            t.set_cursor(w, q);
        }
        Op::Cursor(a) => {
            let p = at(t, *a);
            t.set_cursor(w, p);
        }
        Op::Undo => {
            bump(match t.undo() {
                Ok(()) => "undo: ok",
                Err(Error::UndoConflict) => "undo: conflict (truncated)",
                Err(Error::ReadOnly { .. }) => "undo: blocked by read-only",
                Err(_) => "undo: nothing",
            });
        }
        Op::Redo => {
            bump(if t.redo().is_ok() {
                "redo: ok"
            } else {
                "redo: none/conflict"
            });
        }
        Op::UndoRedo => {
            if t.undo().is_ok() {
                bump("undo+redo round trip");
                t.redo().expect("redo right after a successful undo");
                assert_eq!(
                    t.rope().to_string(),
                    before.0,
                    "undo then redo is the identity"
                );
            }
        }
        Op::Group(open) => {
            if *open {
                t.begin_undo_group()
            } else {
                t.end_undo_group()
            }
        }
        Op::Resize { .. } => unreachable!(),
    }
}

// ---------------------------------------------------------------- the naive definitions

type HideD = (usize, usize, Option<String>, DecorationId);

fn naive_inside(hides: &[HideD], p: usize) -> bool {
    let strict = hides.iter().any(|h| h.0 < p && p < h.1);
    let start_plain = hides.iter().any(|h| h.0 == p && p < h.1 && h.2.is_none());
    let start_repl = hides.iter().any(|h| h.0 == p && p < h.1 && h.2.is_some());
    strict || (start_plain && !start_repl)
}

struct ORow {
    dl_start: usize,
    row: usize,
    cells: Vec<String>,
}
struct OStop {
    pos: usize,
    row: usize, // index into the oracle's rows
    col: usize,
    has_cell: bool,
}

/// An independent, char-by-char layout of the whole buffer, written from the rules, not the code.
fn oracle(
    s: &str,
    hides: &[HideD],
    virt: &[(usize, String, DecorationId)],
    width: Option<usize>,
    tab: usize,
) -> (Vec<ORow>, Vec<OStop>) {
    let mut hs: Vec<HideD> = hides.iter().filter(|h| h.0 < h.1).cloned().collect();
    hs.sort_by_key(|h| (h.0, h.2.is_none(), std::cmp::Reverse(h.1), h.3));
    let mut merged: Vec<HideD> = vec![];
    for h in hs {
        match merged.last_mut() {
            Some(m) if h.0 < m.1 => m.1 = m.1.max(h.1),
            _ => merged.push(h),
        }
    }
    let mut vs = virt.to_vec();
    vs.sort_by_key(|v| (v.0, v.2));
    let full = |col: usize| width.is_some_and(|w| col >= w);
    let (mut rows, mut stops) = (vec![], vec![]);
    let mut p = 0;
    loop {
        let dl_start = p;
        let mut cur: Vec<Vec<String>> = vec![vec![]];
        let (mut col, mut vcol) = (0usize, 0usize);
        let base = rows.len();
        let place = |cur: &mut Vec<Vec<String>>,
                     col: &mut usize,
                     vcol: &mut usize,
                     c: char|
         -> (usize, usize) {
            if c == '\t' {
                let n = tab - *vcol % tab;
                let mut at = None;
                for _ in 0..n {
                    if full(*col) {
                        cur.push(vec![]);
                        *col = 0;
                    }
                    at.get_or_insert((cur.len() - 1, *col));
                    cur.last_mut().unwrap().push(" ".into());
                    *col += 1;
                }
                *vcol += n;
                return at.unwrap();
            }
            let cw = c.width().unwrap_or(1).max(1);
            if cw > 1 && width.is_some_and(|w| *col + cw > w) && *col > 0 {
                while !full(*col) {
                    cur.last_mut().unwrap().push(" ".into());
                    *col += 1;
                }
            }
            if full(*col) {
                cur.push(vec![]);
                *col = 0;
            }
            let at = (cur.len() - 1, *col);
            cur.last_mut().unwrap().push(c.to_string());
            for _ in 1..cw {
                cur.last_mut().unwrap().push(String::new());
            }
            *col += cw;
            *vcol += cw;
            at
        };
        let end_line;
        loop {
            let span = merged.iter().find(|m| m.0 == p).cloned();
            let shown = span.as_ref().is_none_or(|m| m.2.is_some());
            if shown {
                for v in vs.iter().filter(|v| v.0 == p) {
                    for c in v.1.chars() {
                        place(&mut cur, &mut col, &mut vcol, c);
                    }
                }
            }
            if let Some(m) = span {
                if let Some(r) = &m.2 {
                    let mut first = None;
                    for c in r.chars() {
                        let at = place(&mut cur, &mut col, &mut vcol, c);
                        first.get_or_insert(at);
                    }
                    let (row, col) = first.unwrap();
                    stops.push(OStop {
                        pos: p,
                        row: base + row,
                        col,
                        has_cell: true,
                    });
                }
                p = m.1;
                continue;
            }
            let c = s[p..].chars().next();
            if c.is_none() || c == Some('\n') {
                if full(col) {
                    cur.push(vec![]);
                    col = 0;
                }
                stops.push(OStop {
                    pos: p,
                    row: base + cur.len() - 1,
                    col,
                    has_cell: false,
                });
                end_line = c.is_none();
                if !end_line {
                    p += 1;
                }
                break;
            }
            let c = c.unwrap();
            let (row, col0) = place(&mut cur, &mut col, &mut vcol, c);
            stops.push(OStop {
                pos: p,
                row: base + row,
                col: col0,
                has_cell: true,
            });
            p += c.len_utf8();
        }
        for (row, cells) in cur.into_iter().enumerate() {
            rows.push(ORow {
                dl_start,
                row,
                cells,
            });
        }
        if end_line {
            return (rows, stops);
        }
    }
}

fn check(ctx: &mut Ctx, b: BufferId, w: WindowId, step: usize) {
    let frame = ctx.render_window(w);
    let t = ctx.text(b).unwrap();
    let s = t.rope().to_string();
    let hides: Vec<HideD> = t
        .decorations()
        .filter_map(|(id, d)| match &d.deco {
            Deco::Hide { replacement, .. } => {
                Some((d.range.start, d.range.end, replacement.clone(), id))
            }
            _ => None,
        })
        .collect();
    let virt: Vec<(usize, String, DecorationId)> = t
        .decorations()
        .filter_map(|(id, d)| match &d.deco {
            Deco::Virtual(v) => Some((d.range.start, v.clone(), id)),
            _ => None,
        })
        .collect();
    let cur = t.cursor(w).unwrap();
    bump("checks");
    if hides.iter().any(|h| h.0 < h.1) {
        bump("checks with a live hide");
    }
    if hides
        .iter()
        .any(|h| h.0 < h.1 && h.2.is_some() && h.0 == cur)
    {
        bump("cursor on a replacement (fold line)");
    }
    if hides.iter().any(|h| h.0 < h.1 && h.1 == cur) {
        bump("cursor right after hidden text");
    }
    if hides
        .iter()
        .any(|h| h.0 < h.1 && s[h.0..h.1].contains('\n'))
    {
        bump("hide joining lines");
    }
    if t.settings().wrap && t.viewport(w).unwrap().rows.iter().any(|r| r.row > 0) {
        bump("wrapped rows on screen");
    }
    if t.viewport(w).unwrap().top > 0 {
        bump("scrolled");
    }
    if !virt.is_empty() {
        bump("virtual text present");
    }
    assert!(
        !naive_inside(&hides, cur),
        "step {step}: cursor {cur} inside a hidden range"
    );
    let sel = t.selection(w).unwrap();
    assert!(
        !naive_inside(&hides, sel.anchor),
        "step {step}: selection anchor hidden"
    );
    for (q, _) in s.char_indices().chain([(s.len(), ' ')]) {
        assert_eq!(
            t.is_stop(q),
            !naive_inside(&hides, q),
            "step {step}: stop set differs at {q} in {s:?}"
        );
    }
    let g = t.geometry(w).unwrap();
    let width = t.settings().wrap.then_some(g.text_w);
    let (orows, ostops) = oracle(&s, &hides, &virt, width, t.settings().tab_width);
    let vp = t.viewport(w).unwrap();
    let top = orows
        .iter()
        .position(|r| (r.dl_start, r.row) == (vp.top, vp.top_row))
        .expect("viewport top is a row");
    for i in 0..g.rows {
        // Without wrap the spike has no horizontal scroll: a window shows the first text_w columns.
        let want: String = orows
            .get(top + i)
            .map(|r| r.cells.iter().take(g.text_w).cloned().collect())
            .unwrap_or_default();
        let have: String = frame.cells[i]
            .iter()
            .skip(g.text_x)
            .take(g.text_w)
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(
            have.trim_end(),
            want.trim_end(),
            "step {step}: screen row {i} of {s:?}"
        );
    }
    for st in &ostops {
        if st.row < top || st.row >= top + g.rows {
            assert!(
                t.screen_pos(w, st.pos).is_none() || !width.is_some(),
                "step {step}: off-screen stop has a screen position"
            );
            continue;
        }
        let want = (st.row - top, g.text_x + st.col);
        assert_eq!(
            t.screen_pos(w, st.pos),
            Some(want),
            "step {step}: screen_pos({}) in {s:?}",
            st.pos
        );
        if st.has_cell && st.col < g.text_w {
            assert_eq!(
                frame.cells[want.0][want.1].pos,
                Some(st.pos),
                "step {step}: render put {} elsewhere",
                st.pos
            );
        }
    }
    assert!(
        frame.cursor.is_some(),
        "step {step}: cursor scrolled out of view"
    );
    assert_eq!(frame.cursor, t.screen_pos(w, cur));
}

fn run(init: String, ops: Vec<Op>) {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("p", &init);
    let w = ctx.open_window(b, 12, 5).unwrap();
    let hides = Rc::new(RefCell::new(vec![]));
    check(&mut ctx, b, w, 0);
    for (i, op) in ops.into_iter().enumerate() {
        // Each op runs as its own top-level unit, as a command or callback would.
        let h = hides.clone();
        ctx.defer(move |ctx| {
            apply(ctx, b, w, &op, &mut h.borrow_mut());
            Ok(())
        });
        ctx.run_until_idle();
        assert!(
            ctx.messages.is_empty(),
            "step {}: {:?}",
            i + 1,
            ctx.messages
        );
        check(&mut ctx, b, w, i + 1);
    }
}

fn cases() -> u32 {
    std::env::var("S4_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), max_shrink_iters: 20_000, ..ProptestConfig::default() })]
    #[test]
    fn cursor_never_hidden_and_render_agrees_with_motions(init in text_s(80), ops in prop::collection::vec(op(), 1..40)) {
        run(init, ops);
    }
}

/// Not a pass criterion: shows what the random sequences actually exercise.
#[test]
fn coverage_stats() {
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
    let cfg = Config {
        cases: 2000,
        failure_persistence: None,
        ..Config::default()
    };
    let mut runner =
        TestRunner::new_with_rng(cfg, TestRng::deterministic_rng(RngAlgorithm::ChaCha));
    runner
        .run(
            &(text_s(80), prop::collection::vec(op(), 1..40)),
            |(i, o)| {
                run(i, o);
                Ok(())
            },
        )
        .unwrap();
    STATS.with(|s| {
        for (k, v) in s.borrow().iter() {
            println!("{k:40} {v}");
        }
    });
}

// ---------------------------------------------------------------- undo model

#[derive(Debug, Clone)]
enum U {
    Type(char),
    Silent { at: u16, ins: String, del: u8 },
}

fn u_step() -> impl Strategy<Value = U> {
    let c = prop_oneof![Just('x'), Just('y'), Just('中'), Just('\n')];
    prop_oneof![3 => c.prop_map(U::Type), 2 => (any::<u16>(), text_s(4), 0u8..4).prop_map(|(at, ins, del)| U::Silent { at, ins, del })]
}

/// Undo across silent edits, against a model. The user types a session (or deletes a range) at
/// p; silent Extension edits land anywhere that doesn't overlap the user's text (touching is
/// allowed). Undo must then succeed, remove exactly the user's text (or put back exactly what
/// was deleted, where it was), restore the cursor, and redo must reverse it.
fn run_undo(init: String, start: u16, del_len: u8, deletion: bool, steps: Vec<U>) {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("u", &init);
    let w = ctx.open_window(b, 30, 5).unwrap();
    let t = ctx.text_mut(b).unwrap();
    let mut p = at(t, start);
    t.set_cursor(w, p);
    let mut len = 0usize; // the user's text is [p, p + len) (insertion mode)
    let mut deleted = String::new(); // what the user deleted at p (deletion mode)
    let call = |ctx: &mut Ctx, f: Box<dyn FnOnce(&mut TextBuf)>| {
        ctx.defer(move |ctx| {
            f(ctx.text_mut(b).unwrap());
            Ok(())
        });
        ctx.run_until_idle();
    };
    if deletion {
        let t = ctx.text_mut(b).unwrap();
        let q = chars_after(t, p, del_len as usize + 1);
        deleted = t.rope().byte_slice(p..q).to_string();
        call(
            &mut ctx,
            Box::new(move |t| {
                t.tx(Origin::User).delete(p..q).commit().unwrap();
            }),
        );
    } else {
        call(&mut ctx, Box::new(|t| t.begin_undo_group()));
    }
    for s in steps {
        match s {
            U::Type(c) if !deletion => {
                let at = p + len;
                call(
                    &mut ctx,
                    Box::new(move |t| {
                        t.tx(Origin::User)
                            .insert(at, c.to_string())
                            .commit()
                            .unwrap();
                        t.set_cursor(w, at + c.len_utf8());
                    }),
                );
                len += c.len_utf8();
            }
            U::Type(_) => {}
            U::Silent { at: a, ins, del } => {
                let t = ctx.text(b).unwrap();
                let s = at(t, a);
                let e = chars_after(t, s, del as usize);
                // Outside the user's text; inserting exactly at its end would split a live session.
                let ok = if deletion {
                    e <= p || s >= p
                } else {
                    e <= p || s > p + len
                };
                if !ok || (deletion && s < p && p < e) {
                    continue;
                }
                let n = ins.len();
                call(
                    &mut ctx,
                    Box::new(move |t| {
                        t.tx(Origin::Extension)
                            .silent()
                            .replace(s..e, ins)
                            .commit()
                            .unwrap();
                    }),
                );
                if e <= p {
                    p = p - (e - s) + n;
                }
            }
        }
    }
    if !deletion {
        call(&mut ctx, Box::new(|t| t.end_undo_group()));
        if len == 0 {
            return;
        }
    }
    let before = text_of(&ctx, b);
    let mut want = before.clone();
    if deletion {
        want.insert_str(p, &deleted);
    } else {
        want.replace_range(p..p + len, "");
    }
    let t = ctx.text_mut(b).unwrap();
    assert_eq!(
        t.undo(),
        Ok(()),
        "undo refused; model says it can't conflict"
    );
    assert_eq!(t.rope().to_string(), want, "undo restored the wrong text");
    assert_eq!(t.cursor(w), Some(t.snap(p, None)), "cursor not restored");
    assert_eq!(t.redo(), Ok(()));
    assert_eq!(t.rope().to_string(), before, "redo didn't reverse undo");
}

fn text_of(ctx: &Ctx, b: BufferId) -> String {
    ctx.text(b).unwrap().rope().to_string()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), max_shrink_iters: 20_000, ..ProptestConfig::default() })]
    #[test]
    fn undo_is_exact_around_silent_edits(init in text_s(30), start in any::<u16>(), del_len in 0u8..5, deletion in any::<bool>(),
                                         steps in prop::collection::vec(u_step(), 1..20)) {
        run_undo(init, start, del_len, deletion, steps);
    }
}
