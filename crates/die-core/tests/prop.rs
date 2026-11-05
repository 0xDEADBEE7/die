//! Criterion 9: across random sequences of edits (User and silent Extension, inserts and deletes
//! spanning hidden ranges), decoration changes, resizes, settings changes, undo/redo and motions:
//! - the cursor is never inside a hidden range (checked against a naive definition),
//! - the set of cursor positions matches that definition exactly,
//! - render, the motion queries and an independent naive layout oracle agree on every row and stop.
//!
//! `layout_agrees_with_the_naive_layout` is K4's check against `support/naive_layout.rs`, written
//! from contract §9 alone: every row, cell, cursor position, number and query, after each of a run
//! of edits and decoration changes on text full of clusters and control characters. Laying every
//! row out after each edit would hide what lazy rows get wrong, so a second property makes only
//! small queries between edits and compares at the end, and a third runs texts long enough to cross
//! the rope's chunks. S4's oracle wraps by character, so with `Wrap::Word` the sequences above read
//! their rows from that layout.
#![allow(clippy::nonminimal_bool)] // S4's test as ported (K1)

use die_test_cap as _;

use die_core::*;
use proptest::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

mod support;
use support::naive_layout as naive;

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
        nums: u8,
        maxw: Option<u8>,
        center: bool,
        wrap: u8,
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
        1 => (0u8..4, prop::option::of(2u8..30), any::<bool>(), 0u8..5, 1u8..9)
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
                        replacement: repl.clone().map(Into::into),
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
            t.decorate(p..p, Deco::Virtual(s.clone().into())).unwrap();
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
            s.line_numbers = match nums {
                0 => LineNumbers::Off,
                1 => LineNumbers::Absolute,
                2 => LineNumbers::Relative,
                _ => LineNumbers::Visual,
            };
            s.max_width = maxw.map(|m| m as usize);
            s.center = *center;
            s.wrap = match wrap {
                0 => Wrap::Off,
                1 | 2 => Wrap::Char,
                _ => Wrap::Word,
            };
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
                Ok(_) => "undo: ok",
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

type HideD = (usize, usize, Option<String>, u64);

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
    virt: &[(usize, String, u64)],
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
                // A line end stays on its row, even a full one (WRAP-2).
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
    let (cols, rows) = ctx.window_size(w).unwrap();
    let frame = ctx.compose(
        cols,
        rows + 2,
        &[WindowPlacement {
            window: w,
            col: 0,
            row: 0,
        }],
    );
    let t = ctx.text(b).unwrap();
    let s = t.rope().to_string();
    let hides: Vec<HideD> = t
        .decorations()
        .filter_map(|(_, d)| match &d.deco {
            Deco::Hide { replacement, .. } => Some((
                d.range.start,
                d.range.end,
                replacement.as_ref().map(|r| r.text.clone()),
                d.seq,
            )),
            _ => None,
        })
        .collect();
    let virt: Vec<(usize, String, u64)> = t
        .decorations()
        .filter_map(|(_, d)| match &d.deco {
            Deco::Virtual(v) => Some((d.range.start, v.text.clone(), d.seq)),
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
    let wrap = t.settings().wrap;
    if wrap != Wrap::Off && t.viewport(w).unwrap().rows.iter().any(|r| r.row > 0) {
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
    let width = (wrap != Wrap::Off).then_some(g.text_w);
    let (orows, ostops) = if wrap == Wrap::Word {
        word_oracle(t, w)
    } else {
        oracle(&s, &hides, &virt, width, t.settings().tab_width)
    };
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
        let have: String = frame
            .row(i)
            .iter()
            .skip(g.text_x)
            .take(g.text_w)
            .map(|c| c.glyph.to_string())
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
                frame.position(want.0, want.1).map(|(_, p)| p),
                Some(st.pos),
                "step {step}: render put {} elsewhere",
                st.pos
            );
        }
    }
    let cursor = frame.cursors.first().map(|c| (c.row, c.col, c.past_end));
    let want = t.screen_pos(w, cur);
    assert!(want.is_some(), "step {step}: cursor scrolled out of view");
    // Past a full row, the cursor goes in the right padding, or else on the row's last cell
    // (WRAP-2). M0 has no horizontal scrolling: a cursor past an unwrapped window's edge isn't
    // drawn.
    let want = want
        .map(|(r, c)| {
            let past = width.is_some() && c == g.text_x + g.text_w && c >= cols;
            (r, c - past as usize, past)
        })
        .filter(|&(_, c, _)| c < cols);
    assert_eq!(cursor, want, "step {step}: the cursor in {s:?}");
}

// ---------------------------------------------------------------- the property against the naive layout (K4)

/// `support/naive_layout.rs`, written from contract §9 alone, on what this buffer holds.
fn naive_input(t: &TextBuf, w: WindowId, text_w: usize) -> naive::Input {
    let s = t.settings();
    let mut decos: Vec<&Decoration> = t.decorations().map(|(_, d)| d).collect();
    decos.sort_by_key(|d| d.seq);
    let mut input = naive::Input::new(&t.rope().to_string(), text_w)
        .tab_width(s.tab_width)
        .wrap(match s.wrap {
            Wrap::Off => naive::Wrap::Off,
            Wrap::Word => naive::Wrap::Word,
            Wrap::Char => naive::Wrap::Char,
        })
        .numbers(naive_numbers(s.line_numbers), t.cursor(w).unwrap());
    for d in decos {
        match &d.deco {
            Deco::Hide { replacement, .. } => {
                let r = replacement.as_ref().map(|r| r.text.as_str());
                input = input.hide(d.range.clone(), r);
            }
            Deco::Virtual(v) => input = input.virtual_text(d.range.start, &v.text),
            _ => {}
        }
    }
    input
}

fn naive_numbers(n: LineNumbers) -> naive::LineNumbers {
    match n {
        LineNumbers::Off => naive::LineNumbers::Off,
        LineNumbers::Absolute => naive::LineNumbers::Absolute,
        LineNumbers::Relative => naive::LineNumbers::Relative,
        LineNumbers::Visual => naive::LineNumbers::Visual,
    }
}

/// The naive layout's rows and positions, in the shape of S4's oracle, for word wrap.
fn word_oracle(t: &TextBuf, w: WindowId) -> (Vec<ORow>, Vec<OStop>) {
    let g = t.geometry(w).unwrap();
    let l = naive::layout(&naive_input(t, w, g.text_w));
    let mut starts = vec![];
    let mut rows = vec![];
    for (i, r) in l.rows.iter().enumerate() {
        if i == 0 || l.rows[i - 1].display_line != r.display_line {
            starts.push(l.line_starts[r.display_line]);
        }
        let first = rows
            .iter()
            .rposition(|x: &ORow| x.dl_start != l.line_starts[r.display_line]);
        rows.push(ORow {
            dl_start: l.line_starts[r.display_line],
            row: first.map_or(rows.len(), |f| rows.len() - f - 1),
            cells: r.cells.iter().map(naive_cell).collect(),
        });
    }
    let stops = l
        .positions
        .iter()
        .map(|p| OStop {
            pos: p.pos,
            row: p.row,
            col: p.col,
            has_cell: l.rows[p.row].cells.len() > p.col,
        })
        .collect();
    (rows, stops)
}

fn naive_cell(c: &naive::Cell) -> String {
    match c {
        naive::Cell::Cluster(s) => s.clone(),
        naive::Cell::Continue => String::new(),
        naive::Cell::Tab | naive::Cell::Filler => " ".into(),
        naive::Cell::Control(c) => c.to_string(),
    }
}

/// Every row, cursor position, number and query against the naive layout, with the window made
/// tall enough to show them all. The window's size and scroll are put back after.
fn check_naive(ctx: &mut Ctx, b: BufferId, w: WindowId, step: usize) {
    let (cols, rows) = ctx.window_size(w).unwrap();
    let t = ctx.text(b).unwrap();
    let scroll = t.scroll(w).unwrap();
    let g = t.geometry(w).unwrap();
    let s = t.rope().to_string();
    let input = naive_input(t, w, g.text_w);
    let l = naive::layout(&input);
    let n = t.settings();
    let ng = naive::geometry(&s, input.numbers, cols, n.max_width, n.center);
    let got = (g.gutter, g.text_w, g.pad, g.text_x);
    assert_eq!(
        got,
        (ng.gutter, ng.text_w, ng.pad, ng.text_x),
        "step {step}: geometry of {s:?}"
    );

    // Every position's answers, whether or not it's a cursor position.
    for q in s.char_indices().map(|(i, _)| i).chain([s.len()]) {
        let at = l.positions.iter().find(|p| p.pos == q);
        assert_eq!(
            t.is_stop(q),
            at.is_some(),
            "step {step}: is {q} a stop in {input:?}"
        );
        let le = l
            .positions
            .iter()
            .rev()
            .find(|p| p.pos <= q && l.rows[p.row].display_line == naive_line(&l, q));
        let answer = le.or(l.positions.iter().find(|p| p.pos >= q)).unwrap();
        assert_eq!(
            t.display_col(q),
            answer.display_col,
            "step {step}: display_col({q}) in {input:?}"
        );
        let moved = naive::moved_cursor(q, &naive::hidden_ranges(&input.hides), &l.positions);
        assert_eq!(
            t.snap(q, None),
            moved,
            "step {step}: snap({q}) in {input:?}"
        );
        let on_line = |p: &&naive::CursorPos| l.rows[p.row].display_line == naive_line(&l, q);
        let before = l.positions.iter().filter(on_line).rfind(|p| p.pos <= q);
        let after = l.positions.iter().filter(on_line).find(|p| p.pos >= q);
        assert_eq!(
            t.snap(q, Some(Dir::Backward)),
            before.or(after).unwrap().pos,
            "step {step}: snap({q}, Backward) in {input:?}"
        );
        assert_eq!(
            t.snap(q, Some(Dir::Forward)),
            after.or(before).unwrap().pos,
            "step {step}: snap({q}, Forward) in {input:?}"
        );
    }
    for (d, start) in l.line_starts.iter().enumerate() {
        let dl = t.display_line(*start);
        let last = d + 1 == l.line_starts.len();
        let end = l.line_starts.get(d + 1).copied().unwrap_or(s.len());
        let lines =
            s[..*start].matches('\n').count()..=s[..end - usize::from(!last)].matches('\n').count();
        assert_eq!(
            (dl.start(), dl.end(), dl.eof(), dl.lines()),
            (*start, end, last, lines),
            "step {step}: display line {d} of {input:?}"
        );
        let got: Vec<(usize, usize, usize, usize)> = dl
            .stops()
            .iter()
            .map(|p| (p.pos, p.vcol, p.row, p.col))
            .collect();
        let want: Vec<(usize, usize, usize, usize)> = l
            .positions
            .iter()
            .filter(|p| l.rows[p.row].display_line == d)
            .map(|p| (p.pos, p.display_col, 0, p.display_col))
            .collect();
        assert_eq!(
            got, want,
            "step {step}: display line {d}'s stops in {input:?}"
        );
    }
    for q in 0..s.len() {
        if !s.is_char_boundary(q) {
            assert!(
                !t.is_stop(q),
                "step {step}: {q} is inside a character of {s:?}"
            );
            for dir in [None, Some(Dir::Forward), Some(Dir::Backward)] {
                let p = t.snap(q, dir);
                assert!(
                    t.is_stop(p),
                    "step {step}: snap({q}, {dir:?}) gave {p} in {s:?}"
                );
            }
            let _ = (t.display_col(q), t.screen_pos(w, q), t.display_line(q));
        }
    }

    ctx.resize(w, cols, l.rows.len().max(1)).unwrap();
    let t = ctx.text_mut(b).unwrap();
    t.set_scroll(w, 0, 0).unwrap();
    let frame = ctx.compose(
        cols,
        l.rows.len() + 2,
        &[WindowPlacement {
            window: w,
            col: 0,
            row: 0,
        }],
    );
    let t = ctx.text(b).unwrap();
    let vp = t.viewport(w).unwrap();
    assert_eq!(
        vp.rows.len(),
        l.rows.len(),
        "step {step}: the rows of {input:?}"
    );
    let room = cols.saturating_sub(g.text_x);
    for (i, (have, want)) in vp.rows.iter().zip(&l.rows).enumerate() {
        assert_eq!(
            have.number, want.number,
            "step {step}: row {i}'s number in {input:?}"
        );
        assert_eq!(
            have.dline_start, l.line_starts[want.display_line],
            "step {step}: row {i}'s line in {input:?}"
        );
        // The text cells, as far as the window reaches. A newline's cell is blank past them.
        let cell = |x: usize| frame.row(i)[g.text_x + x].glyph.clone();
        for x in 0..room.min(want.cells.len()) {
            let glyph = match &want.cells[x] {
                naive::Cell::Cluster(c) if c.chars().count() == 1 => {
                    Glyph::Char(c.chars().next().unwrap())
                }
                naive::Cell::Cluster(c) => Glyph::Cluster(c.as_str().into()),
                naive::Cell::Continue => Glyph::Continuation,
                naive::Cell::Tab | naive::Cell::Filler => Glyph::Char(' '),
                naive::Cell::Control(c) => Glyph::Char(*c),
            };
            assert_eq!(
                cell(x),
                glyph,
                "step {step}: row {i}, cell {x} of {input:?}"
            );
        }
        for x in want.cells.len()..room {
            assert_eq!(
                cell(x),
                Glyph::Char(' '),
                "step {step}: row {i}, cell {x} is past its end in {input:?}"
            );
        }
        // The gutter's cells.
        let gutter: String = frame.row(i)[g.pad.min(cols)..(g.pad + g.gutter).min(cols)]
            .iter()
            .map(|c| c.glyph.to_string())
            .collect();
        let shown = &want.gutter[..gutter.len().min(want.gutter.len())];
        assert_eq!(gutter, shown, "step {step}: row {i}'s gutter in {input:?}");
        // This row's cursor positions, as far as an unwrapped row reports them.
        let stops: Vec<(usize, usize, usize)> = have
            .stops()
            .iter()
            .map(|p| (p.pos, p.col, p.vcol))
            .collect();
        let mine: Vec<(usize, usize, usize)> = l
            .positions
            .iter()
            .filter(|p| p.row == i)
            .map(|p| (p.pos, p.col, p.display_col))
            .collect();
        assert_eq!(
            stops[..],
            mine[..stops.len().min(mine.len())],
            "step {step}: row {i}'s stops in {input:?}"
        );
        if n_is_wrapped(t) {
            assert_eq!(stops, mine, "step {step}: row {i}'s stops in {input:?}");
        }
    }
    for p in &l.positions {
        assert_eq!(
            t.screen_pos(w, p.pos),
            Some((p.row, g.text_x + p.col)),
            "step {step}: screen_pos({}) in {input:?}",
            p.pos
        );
    }
    check_moves(t, w, &l, &input, step);
    ctx.resize(w, cols, rows).unwrap();
    let t = ctx.text_mut(b).unwrap();
    t.set_scroll(w, scroll.0, scroll.1).unwrap();
}

fn n_is_wrapped(t: &TextBuf) -> bool {
    t.settings().wrap != Wrap::Off
}

/// Which display line the naive layout puts position `q` on.
fn naive_line(l: &naive::Layout, q: usize) -> usize {
    l.line_starts.partition_point(|s| *s <= q) - 1
}

/// `move_visual` and `pos_at_vcol` by their sentences in §9, over the naive layout's positions.
fn check_moves(t: &TextBuf, w: WindowId, l: &naive::Layout, input: &naive::Input, step: usize) {
    let on_row = |r: usize| l.positions.iter().filter(move |p| p.row == r);
    let wrapped = n_is_wrapped(t);
    // With no wrapping, a display line is one row and a column is a display column.
    let col_of = |p: &naive::CursorPos| if wrapped { p.col } else { p.display_col };
    let rows_with_stop: Vec<usize> = (0..l.rows.len())
        .filter(|&r| on_row(r).next().is_some())
        .collect();
    for p in &l.positions {
        for dy in [-2isize, -1, 1, 3] {
            for goal in [None, Some(0), Some(4)] {
                let i = rows_with_stop.iter().position(|&r| r == p.row).unwrap() as isize;
                let to = (i + dy).clamp(0, rows_with_stop.len() as isize - 1);
                let used = goal.unwrap_or(col_of(p));
                let want = if to == i {
                    p.pos
                } else {
                    let row = rows_with_stop[to as usize];
                    let first = on_row(row).next().unwrap();
                    on_row(row)
                        .rfind(|s| col_of(s) <= used)
                        .unwrap_or(first)
                        .pos
                };
                assert_eq!(
                    t.move_visual(w, p.pos, dy, goal),
                    Some((want, used)),
                    "step {step}: move_visual({}, {dy}, {goal:?}) in {input:?}",
                    p.pos
                );
            }
        }
    }
    for line in 0..t.line_count() {
        let start = t.line_start(line);
        let d = naive_line(l, start);
        let stops: Vec<&naive::CursorPos> = l
            .positions
            .iter()
            .filter(|p| l.rows[p.row].display_line == d)
            .collect();
        for vcol in [0, 1, 3, 8, 9, 40] {
            let want = stops
                .iter()
                .find(|p| p.display_col >= vcol)
                .unwrap_or(stops.last().unwrap());
            assert_eq!(
                t.pos_at_vcol(line, vcol),
                want.pos,
                "step {step}: pos_at_vcol({line}, {vcol}) in {input:?}"
            );
        }
    }
}

/// Text with what the naive layout has rules for: clusters, marks, control characters, wide
/// characters, and whitespace with marks on it.
fn rich_text(max: usize) -> impl Strategy<Value = String> {
    let ch = prop_oneof![
        8 => Just('a'), 3 => Just('b'), 5 => Just(' '), 3 => Just('\n'), 2 => Just('\t'),
        2 => Just('中'), 1 => Just('é'), 1 => Just('\u{301}'), 1 => Just('\u{903}'),
        1 => Just('\u{1}'), 1 => Just('\r'), 1 => Just('\u{7f}'), 1 => Just('\u{85}'),
        1 => Just('👍'), 1 => Just('\u{1f3fd}'), 1 => Just('\u{1f1e6}'), 1 => Just('\u{200b}'),
        1 => Just('\u{200d}'), 1 => Just('\u{a0}'), 1 => Just('-'), 1 => Just('\u{fe0e}'),
        1 => Just('\u{1100}'), 1 => Just('\u{1161}'),
    ];
    prop::collection::vec(ch, 0..max).prop_map(|v| v.into_iter().collect())
}

#[derive(Debug, Clone)]
enum N {
    Ins(u16, String),
    Del(u16, u8),
    Replace(u16, u8, String),
    Hide(u16, u8, Option<String>),
    Unhide(u8),
    Virt(u16, String),
    Size(u8),
    Set(u8, Option<u8>, bool, u8, u8),
    Cursor(u16),
}

fn n_step() -> impl Strategy<Value = N> {
    prop_oneof![
        3 => (any::<u16>(), rich_text(5)).prop_map(|(a, s)| N::Ins(a, s)),
        3 => (any::<u16>(), 0u8..8).prop_map(|(a, n)| N::Del(a, n)),
        3 => (any::<u16>(), 0u8..8, rich_text(5)).prop_map(|(a, n, s)| N::Replace(a, n, s)),
        2 => (any::<u16>(), 0u8..12, prop::option::of(rich_text(5))).prop_map(|(a, n, r)| N::Hide(a, n, r)),
        1 => any::<u8>().prop_map(N::Unhide),
        2 => (any::<u16>(), rich_text(5)).prop_map(|(a, s)| N::Virt(a, s)),
        1 => (2u8..30).prop_map(N::Size),
        1 => (0u8..4, prop::option::of(2u8..20), any::<bool>(), 0u8..5, 0u8..9).prop_map(|(n, m, c, wr, t)| N::Set(n, m, c, wr, t)),
        2 => any::<u16>().prop_map(N::Cursor),
    ]
}

/// One step on the text. A resize is the caller's.
fn apply_n(t: &mut TextBuf, w: WindowId, step: &N, hides: &mut Vec<DecorationId>) {
    match step {
        N::Ins(a, s) => {
            let p = at(t, *a);
            t.tx(Origin::Extension)
                .insert(p, s.as_str())
                .commit()
                .unwrap();
        }
        N::Del(a, n) => {
            let p = at(t, *a);
            let q = chars_after(t, p, *n as usize);
            t.tx(Origin::Extension).delete(p..q).commit().unwrap();
        }
        N::Replace(a, n, s) => {
            let p = at(t, *a);
            let q = chars_after(t, p, *n as usize);
            t.tx(Origin::Extension)
                .replace(p..q, s.as_str())
                .commit()
                .unwrap();
        }
        N::Hide(a, n, r) => {
            let p = at(t, *a);
            let q = chars_after(t, p, *n as usize);
            let deco = Deco::Hide {
                replacement: r.clone().map(Into::into),
                reveal: Reveal::Remove,
            };
            hides.push(t.decorate(p..q, deco).unwrap());
        }
        N::Unhide(k) => {
            if !hides.is_empty() {
                t.undecorate(hides.remove(*k as usize % hides.len()));
            }
        }
        N::Virt(a, s) => {
            let p = at(t, *a);
            t.decorate(p..p, Deco::Virtual(s.clone().into())).unwrap();
        }
        N::Size(_) => {}
        N::Set(nums, maxw, center, wrap, tab) => {
            let s = t.settings_mut();
            s.line_numbers = [
                LineNumbers::Off,
                LineNumbers::Absolute,
                LineNumbers::Relative,
                LineNumbers::Visual,
            ][*nums as usize];
            s.max_width = maxw.map(|m| m as usize);
            s.center = *center;
            s.wrap = [Wrap::Off, Wrap::Char, Wrap::Word, Wrap::Word, Wrap::Word][*wrap as usize];
            s.tab_width = *tab as usize;
        }
        N::Cursor(a) => {
            let p = at(t, *a);
            t.set_cursor(w, p);
        }
    }
}

fn run_naive(init: String, width: u8, steps: Vec<N>) {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("p", &init);
    let w = ctx.open_window(b, width as usize, 4).unwrap();
    check_naive(&mut ctx, b, w, 0);
    let mut hides = vec![];
    for (i, step) in steps.iter().enumerate() {
        if let N::Size(cols) = step {
            ctx.resize(w, *cols as usize, 4).unwrap();
        }
        apply_n(ctx.text_mut(b).unwrap(), w, step, &mut hides);
        check_naive(&mut ctx, b, w, i + 1);
    }
}

/// What the lazy property does between edits. Each query asks about one position or one small
/// window, as a keystroke does, so a line stays laid out only as far as they reached.
#[derive(Debug, Clone)]
enum Lazy {
    N(N),
    Undo,
    Redo,
    Boundary,
    Sticky(u8, bool, bool),
    TwoInserts(u16, u16, String),
    Silent(u16, String),
    Query(u16),
    Frame,
    Top(u16, u8),
    OtherSize(u8),
}

fn lazy_step() -> impl Strategy<Value = Lazy> {
    prop_oneof![
        12 => n_step().prop_map(Lazy::N),
        3 => Just(Lazy::Undo),
        2 => Just(Lazy::Redo),
        1 => Just(Lazy::Boundary),
        2 => (any::<u8>(), any::<bool>(), any::<bool>()).prop_map(|(k, a, b)| Lazy::Sticky(k, a, b)),
        2 => (any::<u16>(), any::<u16>(), rich_text(3)).prop_map(|(a, b, s)| Lazy::TwoInserts(a, b, s)),
        1 => (any::<u16>(), rich_text(3)).prop_map(|(a, s)| Lazy::Silent(a, s)),
        4 => any::<u16>().prop_map(Lazy::Query),
        3 => Just(Lazy::Frame),
        1 => (any::<u16>(), any::<u8>()).prop_map(|(a, r)| Lazy::Top(a, r)),
        1 => (2u8..30).prop_map(Lazy::OtherSize),
    ]
}

/// A cache's rows are only ever half laid out in use. So: edits with small queries between them,
/// in two windows of different widths, and only then every row against the naive layout.
fn run_naive_lazy(init: String, width: u8, steps: Vec<Lazy>) {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("p", &init);
    let w = ctx.open_window(b, width as usize, 2).unwrap();
    let other = ctx
        .open_window(b, (width as usize / 2 + 3).max(2), 2)
        .unwrap();
    ctx.focus(w);
    let mut hides = vec![];
    for step in &steps {
        match step {
            Lazy::N(N::Size(cols)) => ctx.resize(w, *cols as usize, 2).unwrap(),
            Lazy::OtherSize(cols) => ctx.resize(other, *cols as usize, 2).unwrap(),
            Lazy::Frame => {
                let (cols, rows) = ctx.window_size(w).unwrap();
                let at = WindowPlacement {
                    window: w,
                    col: 0,
                    row: 0,
                };
                ctx.compose(cols, rows + 2, &[at]);
            }
            _ => {}
        }
        let t = ctx.text_mut(b).unwrap();
        match step {
            Lazy::N(n) => apply_n(t, w, n, &mut hides),
            Lazy::Undo => {
                let _ = t.undo();
            }
            Lazy::Redo => {
                let _ = t.redo();
            }
            Lazy::Boundary => t.undo_boundary(),
            Lazy::Sticky(k, start, end) => {
                if !hides.is_empty() {
                    let g = |left| if left { Gravity::Left } else { Gravity::Right };
                    t.set_sticky(hides[*k as usize % hides.len()], g(*start), g(*end));
                }
            }
            Lazy::TwoInserts(a, c, s) => {
                let (p, q) = (at(t, *a), at(t, *c));
                if p != q {
                    let (p, q) = (p.min(q), p.max(q));
                    let tx = t.tx(Origin::Extension).insert(p, s.as_str()).insert(q, "z");
                    tx.commit().unwrap();
                }
            }
            Lazy::Silent(a, s) => {
                let p = at(t, *a);
                let tx = t.tx(Origin::Extension).silent().insert(p, s.as_str());
                tx.commit().unwrap();
            }
            Lazy::Query(a) => {
                let p = at(t, *a);
                let _ = (t.screen_pos(w, p), t.display_col(p));
                let _ = t.move_visual(other, p, 1, None);
            }
            Lazy::Top(a, r) => {
                let p = at(t, *a);
                t.set_scroll(w, p, *r as usize).unwrap();
            }
            Lazy::Frame | Lazy::OtherSize(_) => {}
        }
    }
    check_naive(&mut ctx, b, w, steps.len());
    check_naive(&mut ctx, b, other, steps.len());
}

/// The random texts are too short to reach a gutter's next digit.
#[test]
fn the_gutter_agrees_with_the_naive_layout_where_it_grows() {
    for lines in [0, 1, 99, 100, 999, 1000, 9999, 10_000] {
        for end in ["", "\n", "x", "x\n"] {
            let text = format!("{}{end}", "y\n".repeat(lines));
            let mut ctx = Ctx::new(vec![]).unwrap();
            let b = ctx.create_text("g", &text);
            let w = ctx.open_window(b, 40, 4).unwrap();
            for style in [LineNumbers::Off, LineNumbers::Relative] {
                let t = ctx.text_mut(b).unwrap();
                t.settings_mut().line_numbers = style;
                let g = t.geometry(w).unwrap().gutter;
                let want = naive::gutter_width(&text, naive_numbers(style));
                assert_eq!(g, want, "{style:?}, {lines} lines and {end:?}");
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: naive_layout_cases(), max_shrink_iters: 20_000, ..ProptestConfig::default() })]
    /// K4's second check: rows, positions, numbers and queries match the naive layout, across edits
    /// and decoration changes, so a stale cache shows too.
    #[test]
    fn layout_agrees_with_the_naive_layout(
        init in rich_text(60),
        width in 2u8..24,
        steps in prop::collection::vec(n_step(), 0..12),
    ) {
        run_naive(init, width, steps);
    }

    /// The same check when an edit finds a line half laid out, which is how a keystroke finds it.
    #[test]
    fn layout_agrees_with_the_naive_layout_after_lazy_edits(
        init in rich_text(60),
        width in 2u8..24,
        steps in prop::collection::vec(lazy_step(), 0..24),
    ) {
        run_naive_lazy(init, width, steps);
    }
}

/// A rope's chunks hold about 1 KB, so the short texts above never cross one. A cluster that does
/// sends `grapheme` to the next chunk, the previous one and the chunks before for context.
#[test]
fn a_cluster_across_a_ropes_chunks_agrees_with_the_naive_layout() {
    for unit in [
        "e\u{301}",
        "\u{1f1e6}",
        "👍\u{1f3fd}",
        "👍\u{200d}👍",
        "\u{1100}\u{1161}",
    ] {
        let mut crossed = 0;
        // Moving the text along puts a chunk's edge in each place inside a cluster in turn.
        for shift in 0..unit.len() {
            let text = "a".repeat(shift) + &unit.repeat(1300 / unit.len());
            let clusters: Vec<usize> = text.grapheme_indices(true).map(|(i, _)| i).collect();
            let mut ctx = Ctx::new(vec![]).unwrap();
            let b = ctx.create_text("p", &text);
            let w = ctx.open_window(b, 17, 4).unwrap();
            let mut edge = 0;
            let inside = ctx.text(b).unwrap().rope().chunks().any(|c| {
                edge += c.len();
                edge < text.len() && clusters.binary_search(&edge).is_err()
            });
            if inside {
                crossed += 1;
                check_naive(&mut ctx, b, w, 0);
            }
        }
        assert!(crossed > 0, "no chunk's edge falls inside {unit:?}");
    }
}

/// Mixed text across a rope's chunks, with few newlines, so one display line crosses several.
fn long_rich_text(max: usize) -> impl Strategy<Value = String> {
    let ch = prop_oneof![
        6 => Just('a'), 2 => Just(' '), 2 => Just('\t'),
        4 => Just('中'), 2 => Just('é'), 3 => Just('\u{301}'), 1 => Just('\u{903}'),
        1 => Just('\u{1}'), 1 => Just('\r'), 1 => Just('\u{85}'),
        3 => Just('👍'), 2 => Just('\u{1f3fd}'), 4 => Just('\u{1f1e6}'), 1 => Just('\u{200b}'),
        2 => Just('\u{200d}'), 1 => Just('\u{fe0e}'),
        1 => Just('\u{1100}'), 1 => Just('\u{1161}'),
        1 => Just('\n'),
    ];
    prop::collection::vec(ch, max / 2..max).prop_map(|v| v.into_iter().collect())
}

proptest! {
    // About a second a case, so a fixed few however many the others run.
    #![proptest_config(ProptestConfig { cases: 5, max_shrink_iters: 200, ..ProptestConfig::default() })]
    #[test]
    fn long_text_across_a_ropes_chunks_agrees_with_the_naive_layout(
        init in long_rich_text(1600),
        width in 2u8..40,
        steps in prop::collection::vec(n_step(), 0..3),
    ) {
        run_naive(init, width, steps);
    }
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
    prop_cases().unwrap_or(10_000)
}

/// The naive layout's cases cost 20 to 40 times the others. At 10,000 they took about 8 minutes
/// of each `cargo xtask ci` at background priority, against about 2 at this count. K4 passed
/// 40,000: run `DIE_PROP_CASES=40000` after changing the layout.
fn naive_layout_cases() -> u32 {
    prop_cases().unwrap_or(2_000)
}

fn prop_cases() -> Option<u32> {
    std::env::var("DIE_PROP_CASES").ok()?.parse().ok()
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
    Silent {
        at: u16,
        ins: String,
        del: u8,
    },
    /// Rewrite what the last silent edit inserted, as a status line redrawn in place is.
    Redraw(String),
}

fn u_step() -> impl Strategy<Value = U> {
    let c = prop_oneof![Just('x'), Just('y'), Just('中'), Just('\n')];
    prop_oneof![3 => c.prop_map(U::Type), 2 => (any::<u16>(), text_s(4), 0u8..4).prop_map(|(at, ins, del)| U::Silent { at, ins, del }), 2 => text_s(4).prop_map(U::Redraw)]
}

/// Undo across silent edits, against a model. The user types a session (or deletes a range) at
/// p; silent Extension edits land anywhere that doesn't overlap the user's text (touching is
/// allowed). Undo must then succeed, remove exactly the user's text (or put back exactly what
/// was deleted, where it was), restore the cursor, and redo must reverse it.
/// K2: undo also reports the range it changed, and restores the selection's shape.
fn run_undo(init: String, start: u16, del_len: u8, deletion: bool, shape: u8, steps: Vec<U>) {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("u", &init);
    let w = ctx.open_window(b, 30, 5).unwrap();
    let t = ctx.text_mut(b).unwrap();
    let mut p = at(t, start);
    let shape = [Shape::Char, Shape::Line, Shape::Block][shape as usize % 3];
    t.set_selection(
        w,
        Selection {
            shape,
            ..Selection::cursor(p)
        },
    );
    let mut len = 0usize; // the user's text is [p, p + len) (insertion mode)
    let mut deleted = String::new(); // what the user deleted at p (deletion mode)
    let mut last: Option<(usize, usize)> = None; // what the last silent edit inserted
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
        // An empty deletion, at the end of the text, is no change and no undo step.
        if q == p {
            return;
        }
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
                last = None;
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
                last = Some((s, n));
            }
            U::Redraw(ins) => {
                let Some((s, n)) = last else { continue };
                let e = s + n;
                let ok = if deletion {
                    e <= p || s >= p
                } else {
                    e <= p || s > p + len
                };
                if !ok || (deletion && s < p && p < e) {
                    continue;
                }
                let m = ins.len();
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
                    p = p - n + m;
                }
                last = Some((s, m));
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
    let (undone, redone) = if deletion {
        (p..p + deleted.len(), p..p)
    } else {
        (p..p, p..p + len)
    };
    let t = ctx.text_mut(b).unwrap();
    assert_eq!(
        t.undo(),
        Ok(undone),
        "undo refused (the model says it can't conflict), or reported the wrong range"
    );
    assert_eq!(t.rope().to_string(), want, "undo restored the wrong text");
    assert_eq!(t.cursor(w), Some(t.snap(p, None)), "cursor not restored");
    assert_eq!(
        t.selection(w).map(|s| s.shape),
        Some(shape),
        "shape not restored"
    );
    assert_eq!(t.redo(), Ok(redone));
    assert_eq!(t.rope().to_string(), before, "redo didn't reverse undo");
}

fn text_of(ctx: &Ctx, b: BufferId) -> String {
    ctx.text(b).unwrap().rope().to_string()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), max_shrink_iters: 20_000, ..ProptestConfig::default() })]
    #[test]
    fn undo_is_exact_around_silent_edits(init in text_s(30), start in any::<u16>(), del_len in 0u8..5, deletion in any::<bool>(),
                                         shape in 0u8..3, steps in prop::collection::vec(u_step(), 1..20)) {
        run_undo(init, start, del_len, deletion, shape, steps);
    }

    #[test]
    fn undo_history_matches_a_model(init in text_s(20), steps_cap in 0usize..6, bytes_cap in 1usize..40,
                                    hs in prop::collection::vec(h_step(), 1..30)) {
        run_history(init, steps_cap, bytes_cap, hs);
    }
}

// ---------------------------------------------------------------- undo history model (K2)

#[derive(Debug, Clone)]
enum H {
    /// One command inserting text.
    Ins(u16, String),
    /// One command deleting characters.
    Del(u16, u8),
    /// An explicit group: a cursor move in a unit of its own when there is one, then
    /// characters typed one command each, with an undo boundary before the `split`th one when
    /// that falls inside.
    Unit(u16, Vec<char>, u8, Option<u16>),
    /// An explicit group with an undo inside it: the first `k` characters, an undo, then the
    /// rest. The group stays open, so the rest is one step (DR-14).
    UnitUndo(u16, Vec<char>, u8),
    /// One command that inserts and then starts a group, as die-vim's `o` does, and the
    /// characters typed in it.
    Open(u16, String, Vec<char>),
    /// One command: an insert, an undo boundary, another insert right after the first.
    Split(u16, String, String),
    /// One transaction with two changes, which it records last one first: an insert,
    /// and a replacement of the character at or after it.
    Multi(u16, u16, String, String),
    /// One command with two transactions: one step.
    Two(u16, String, String),
    /// A change that changes nothing: an empty insert, an empty delete, or text replaced by
    /// itself, which is still a step (TEXT-1).
    Noop(u16, u8, u8),
    /// A silent Extension edit "above your text": at offset 0, so every step moves by its length.
    Stream(String),
    /// Move the focus, and so the acting window, between the buffer's two windows.
    Focus(bool),
    /// Change the caps mid-run. They apply at the next edit, undo or redo.
    Caps(usize, usize),
    Undo,
    Redo,
    Sel(u16, u16, u8, Option<u8>),
}

fn h_step() -> impl Strategy<Value = H> {
    let s = || text_s(4).prop_filter("an insert inserts something", |s| !s.is_empty());
    let c = || prop_oneof![Just('x'), Just('中'), Just('\n')];
    let chars = move || prop::collection::vec(c(), 1..5);
    prop_oneof![
        3 => (any::<u16>(), s()).prop_map(|(a, s)| H::Ins(a, s)),
        2 => (any::<u16>(), 1u8..4).prop_map(|(a, n)| H::Del(a, n)),
        2 => (any::<u16>(), chars(), any::<u8>(), prop::option::of(any::<u16>())).prop_map(|(a, v, k, m)| H::Unit(a, v, k, m)),
        1 => (any::<u16>(), chars(), any::<u8>()).prop_map(|(a, v, k)| H::UnitUndo(a, v, k)),
        1 => (any::<u16>(), s(), chars()).prop_map(|(a, x, v)| H::Open(a, x, v)),
        1 => (any::<u16>(), s(), s()).prop_map(|(a, x, y)| H::Split(a, x, y)),
        1 => (any::<u16>(), any::<u16>(), s(), s()).prop_map(|(a, c, x, y)| H::Multi(a, c, x, y)),
        1 => (any::<u16>(), s(), s()).prop_map(|(a, x, y)| H::Two(a, x, y)),
        1 => (any::<u16>(), 1u8..4, 0u8..3).prop_map(|(a, n, k)| H::Noop(a, n, k)),
        2 => s().prop_map(H::Stream),
        1 => any::<bool>().prop_map(H::Focus),
        1 => (0usize..6, 1usize..40).prop_map(|(s, b)| H::Caps(s, b)),
        3 => Just(H::Undo),
        2 => Just(H::Redo),
        1 => (any::<u16>(), any::<u16>(), 0u8..3, prop::option::of(any::<u8>())).prop_map(|(a, h, k, g)| H::Sel(a, h, k, g)),
    ]
}

/// One undo step, as the model sees it.
#[derive(Debug, Clone)]
struct MStep {
    before: String,
    after: String,
    /// The acting window's selection just before the step's first edit: what undo restores.
    sel: Selection,
    bytes: usize,
    undone: std::ops::Range<usize>,
    redone: std::ops::Range<usize>,
}

/// The history as the model sees it: stacks of whole-text snapshots, and the caps.
struct Model {
    undo: Vec<MStep>,
    redo: Vec<MStep>,
    steps_cap: usize,
    bytes_cap: usize,
}

impl Model {
    fn trim(&mut self) {
        while self.undo.len() > self.steps_cap
            || (self.undo.len() > 1
                && self.undo.iter().map(|s| s.bytes).sum::<usize>() > self.bytes_cap)
        {
            self.undo.remove(0);
        }
    }
    fn push(&mut self, step: MStep) {
        self.redo.clear();
        self.undo.push(step);
        self.trim();
    }
    /// A step that inserted `redone`'s worth of text at its start.
    fn push_insert(
        &mut self,
        before: String,
        ctx: &Ctx,
        b: BufferId,
        sel: Selection,
        r: std::ops::Range<usize>,
    ) {
        self.push(MStep {
            before,
            after: text_of(ctx, b),
            sel,
            bytes: r.len(),
            undone: r.start..r.start,
            redone: r,
        });
    }
    /// A silent insertion of `s` at offset 0: every snapshot and position moves past it.
    fn stream(&mut self, s: &str) {
        let n = s.len();
        for m in self.undo.iter_mut().chain(&mut self.redo) {
            m.before.insert_str(0, s);
            m.after.insert_str(0, s);
            m.sel.anchor += n;
            m.sel.head += n;
            m.undone = m.undone.start + n..m.undone.end + n;
            m.redone = m.redone.start + n..m.redone.end + n;
        }
    }
}

/// Run `f` as one top-level unit (A11), returning its value.
fn unit<R: 'static>(ctx: &mut Ctx, b: BufferId, f: impl FnOnce(&mut TextBuf) -> R + 'static) -> R {
    let out = Rc::new(RefCell::new(None));
    let o = out.clone();
    ctx.defer(move |ctx| {
        *o.borrow_mut() = Some(f(ctx.text_mut(b).unwrap()));
        Ok(())
    });
    ctx.run_until_idle();
    out.borrow_mut().take().expect("the unit ran")
}

fn type_char(ctx: &mut Ctx, b: BufferId, q: usize, c: char) -> usize {
    unit(ctx, b, move |t| {
        t.tx(Origin::User)
            .insert(q, c.to_string())
            .commit()
            .unwrap()
    });
    q + c.len_utf8()
}

fn undo_and_check(ctx: &mut Ctx, b: BufferId, w: WindowId, m: &mut Model) {
    let r = unit(ctx, b, |t| t.undo());
    m.trim();
    match m.undo.pop() {
        None => assert_eq!(
            r,
            Err(Error::NothingToUndo),
            "the model has nothing to undo"
        ),
        Some(s) => {
            assert_eq!(r, Ok(s.undone.clone()), "undo's range");
            assert_eq!(text_of(ctx, b), s.before, "undo's text");
            assert_eq!(
                ctx.text(b).unwrap().selection(w),
                Some(s.sel),
                "undo's selection, in the acting window"
            );
            m.redo.push(s);
        }
    }
}

fn redo_and_check(ctx: &mut Ctx, b: BufferId, w: WindowId, m: &mut Model) {
    let r = unit(ctx, b, |t| t.redo());
    m.trim();
    match m.redo.pop() {
        None => assert_eq!(
            r,
            Err(Error::NothingToUndo),
            "the model has nothing to redo"
        ),
        Some(s) => {
            assert_eq!(r, Ok(s.redone.clone()), "redo's range");
            assert_eq!(text_of(ctx, b), s.after, "redo's text");
            let cursor = Selection::cursor(s.redone.start);
            assert_eq!(
                ctx.text(b).unwrap().selection(w),
                Some(cursor),
                "redo's cursor, in the acting window"
            );
            m.undo.push(s);
        }
    }
}

/// Undo history against a model: a stack of whole-text snapshots. Steps are commands,
/// explicit groups and undo boundaries, in two windows with the focus moving between them, and
/// with silent text streaming in; the buffer's caps are small, random and change mid-run. Every
/// undo and redo must give the model's text, report the model's range, and restore the acting
/// window's selection from just before the step's first change (undo) or put a cursor at the
/// change's start (redo). After every step, the change log must replay to the same text.
fn run_history(init: String, steps_cap: usize, bytes_cap: usize, hs: Vec<H>) {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("h", &init);
    let first = ctx.open_window(b, 30, 5).unwrap();
    let second = ctx.open_window(b, 30, 5).unwrap();
    let s = ctx.text_mut(b).unwrap().settings_mut();
    s.undo_steps = steps_cap;
    s.undo_bytes = bytes_cap;
    let mut m = Model {
        undo: vec![],
        redo: vec![],
        steps_cap,
        bytes_cap,
    };
    for h in hs {
        let before = text_of(&ctx, b);
        let w = ctx.focused().unwrap();
        let selection = |ctx: &Ctx| ctx.text(b).unwrap().selection(w).unwrap();
        let sel = selection(&ctx);
        let t = ctx.text(b).unwrap();
        let rev = t.rev();
        let logged = match h {
            H::Stream(_) => (Origin::Extension, true),
            _ => (Origin::User, false),
        };
        match h {
            H::Ins(a, s) => {
                let p = at(t, a);
                let n = s.len();
                unit(&mut ctx, b, move |t| {
                    t.tx(Origin::User).insert(p, s).commit().unwrap()
                });
                m.push_insert(before.clone(), &ctx, b, sel, p..p + n);
            }
            H::Del(a, k) => {
                let p = at(t, a);
                let q = chars_after(t, p, k as usize);
                if q == p {
                    continue;
                }
                unit(&mut ctx, b, move |t| {
                    t.tx(Origin::User).delete(p..q).commit().unwrap()
                });
                let after = text_of(&ctx, b);
                m.push(MStep {
                    before: before.clone(),
                    after,
                    sel,
                    bytes: q - p,
                    undone: p..q,
                    redone: p..p,
                });
            }
            H::Unit(a, chars, split, moved) => {
                let p = at(t, a);
                let split = split as usize % (chars.len() + 1);
                let to = moved.map(|m| at(t, m));
                unit(&mut ctx, b, |t| t.begin_undo_group());
                if let Some(to) = to {
                    unit(&mut ctx, b, move |t| t.set_cursor(w, to));
                }
                let sel = selection(&ctx);
                let (mut q, mut start, mut first) = (p, p, (before.clone(), sel));
                for (i, c) in chars.into_iter().enumerate() {
                    if i == split && i > 0 {
                        let (before, sel) = first;
                        m.push_insert(before, &ctx, b, sel, start..q);
                        let sel = unit(&mut ctx, b, move |t| {
                            t.undo_boundary();
                            t.selection(w).unwrap()
                        });
                        first = (text_of(&ctx, b), sel);
                        start = q;
                    }
                    q = type_char(&mut ctx, b, q, c);
                }
                unit(&mut ctx, b, |t| t.end_undo_group());
                let (before, sel) = first;
                m.push_insert(before, &ctx, b, sel, start..q);
            }
            H::UnitUndo(a, chars, k) => {
                let p = at(t, a);
                let (head, tail) = chars.split_at(k as usize % (chars.len() + 1));
                unit(&mut ctx, b, |t| t.begin_undo_group());
                let mut q = p;
                for &c in head {
                    q = type_char(&mut ctx, b, q, c);
                }
                if !head.is_empty() {
                    m.push_insert(before.clone(), &ctx, b, sel, p..q);
                }
                undo_and_check(&mut ctx, b, w, &mut m);
                let mid = text_of(&ctx, b);
                let sel = selection(&ctx);
                let p = at(ctx.text(b).unwrap(), a);
                let mut q = p;
                for &c in tail {
                    q = type_char(&mut ctx, b, q, c);
                }
                unit(&mut ctx, b, |t| t.end_undo_group());
                if !tail.is_empty() {
                    m.push_insert(mid, &ctx, b, sel, p..q);
                }
            }
            H::Open(a, x, chars) => {
                let p = at(t, a);
                let n = x.len();
                unit(&mut ctx, b, move |t| {
                    t.tx(Origin::User).insert(p, x).commit().unwrap();
                    t.begin_undo_group();
                });
                let mut q = p + n;
                for c in chars {
                    q = type_char(&mut ctx, b, q, c);
                }
                unit(&mut ctx, b, |t| t.end_undo_group());
                m.push_insert(before.clone(), &ctx, b, sel, p..q);
            }
            H::Split(a, x, y) => {
                let p = at(t, a);
                let (nx, ny) = (x.len(), y.len());
                let mut mid = before.clone();
                mid.insert_str(p, &x);
                let mid_sel = unit(&mut ctx, b, move |t| {
                    t.tx(Origin::User).insert(p, x).commit().unwrap();
                    let s = t.selection(w).unwrap();
                    t.undo_boundary();
                    t.tx(Origin::User).insert(p + nx, y).commit().unwrap();
                    s
                });
                let after = text_of(&ctx, b);
                m.push(MStep {
                    before: before.clone(),
                    after: mid.clone(),
                    sel,
                    bytes: nx,
                    undone: p..p,
                    redone: p..p + nx,
                });
                let q = p + nx;
                m.push(MStep {
                    before: mid,
                    after,
                    sel: mid_sel,
                    bytes: ny,
                    undone: q..q,
                    redone: q..q + ny,
                });
            }
            H::Multi(a, c, x, y) => {
                let (p, q) = (at(t, a).min(at(t, c)), at(t, a).max(at(t, c)));
                let q1 = chars_after(t, q, 1);
                let mut want = before.clone();
                want.replace_range(q..q1, &y);
                want.insert_str(p, &x);
                let (nx, ny) = (x.len(), y.len());
                unit(&mut ctx, b, move |t| {
                    t.tx(Origin::User)
                        .insert(p, x)
                        .replace(q..q1, y)
                        .commit()
                        .unwrap()
                });
                let after = text_of(&ctx, b);
                assert_eq!(after, want, "the transaction's text");
                m.push(MStep {
                    before: before.clone(),
                    after,
                    sel,
                    bytes: nx + ny + (q1 - q),
                    undone: p..q1,
                    redone: p..q + nx + ny,
                });
            }
            H::Two(a, x, y) => {
                let p = at(t, a);
                let (nx, ny) = (x.len(), y.len());
                unit(&mut ctx, b, move |t| {
                    t.tx(Origin::User).insert(p, x).commit().unwrap();
                    t.tx(Origin::User).insert(p + nx, y).commit().unwrap();
                });
                m.push_insert(before.clone(), &ctx, b, sel, p..p + nx + ny);
            }
            H::Noop(a, n, kind) => {
                let p = at(t, a);
                let q = chars_after(t, p, n as usize);
                if kind == 2 && q > p {
                    let same = before[p..q].to_string();
                    unit(&mut ctx, b, move |t| {
                        t.tx(Origin::User).replace(p..q, same).commit().unwrap()
                    });
                    assert_ne!(ctx.text(b).unwrap().rev(), rev, "text replaced by itself");
                    m.push(MStep {
                        before: before.clone(),
                        after: before.clone(),
                        sel,
                        bytes: 2 * (q - p),
                        undone: p..q,
                        redone: p..q,
                    });
                } else {
                    let r = unit(&mut ctx, b, move |t| {
                        let tx = t.tx(Origin::User);
                        match kind {
                            0 => tx.insert(p, ""),
                            _ => tx.delete(p..p),
                        }
                        .commit()
                    });
                    assert_eq!(r, Ok(rev), "an empty change makes no revision");
                }
            }
            H::Stream(s) => {
                m.stream(&s);
                unit(&mut ctx, b, move |t| {
                    t.tx(Origin::Extension)
                        .silent()
                        .insert(0, s)
                        .commit()
                        .unwrap()
                });
            }
            H::Focus(one) => ctx.focus(if one { first } else { second }),
            H::Caps(steps, bytes) => {
                (m.steps_cap, m.bytes_cap) = (steps, bytes);
                let s = ctx.text_mut(b).unwrap().settings_mut();
                s.undo_steps = steps;
                s.undo_bytes = bytes;
            }
            H::Undo => undo_and_check(&mut ctx, b, w, &mut m),
            H::Redo => redo_and_check(&mut ctx, b, w, &mut m),
            H::Sel(a, h, k, goal) => {
                let sel = Selection {
                    anchor: at(t, a),
                    head: at(t, h),
                    shape: [Shape::Char, Shape::Line, Shape::Block][k as usize],
                    goal: goal.map(usize::from),
                };
                ctx.text_mut(b).unwrap().set_selection(w, sel);
            }
        }
        // Replaying the log gives the text: every change, in order, each in the coordinates
        // of the text after the one before it, undo's and redo's too.
        let t = ctx.text(b).unwrap();
        let mut replayed = before;
        for c in t.edits_since(rev).expect("the log reaches this far") {
            replayed.replace_range(c.start..c.old_end, &c.text);
            assert_eq!((c.origin, c.silent), logged, "how the change was logged");
        }
        assert_eq!(replayed, text_of(&ctx, b), "the log replays to the text");
    }
}

// ---------------------------------------------------------------- K6: windows and the modified flag

#[derive(Clone, Debug)]
enum WinOp {
    Split(u8, Side),
    Close(u8),
    Only(u8),
    Resize(u8, u8, u8),
    Frame(u8, u8),
    Balance(u8),
    Show(u8, u8),
    Focus(u8),
    Dir(Side),
}

fn side() -> impl Strategy<Value = Side> {
    prop_oneof![
        Just(Side::Left),
        Just(Side::Right),
        Just(Side::Above),
        Just(Side::Below)
    ]
}

fn win_op() -> impl Strategy<Value = WinOp> {
    prop_oneof![
        5 => (any::<u8>(), side()).prop_map(|(w, s)| WinOp::Split(w, s)),
        3 => any::<u8>().prop_map(WinOp::Close),
        1 => any::<u8>().prop_map(WinOp::Only),
        4 => (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(w, c, r)| WinOp::Resize(w, c, r)),
        2 => (0u8..120, 0u8..60).prop_map(|(c, r)| WinOp::Frame(c, r)),
        1 => any::<u8>().prop_map(WinOp::Balance),
        2 => (any::<u8>(), any::<u8>()).prop_map(|(w, b)| WinOp::Show(w, b)),
        2 => any::<u8>().prop_map(WinOp::Focus),
        2 => side().prop_map(WinOp::Dir),
    ]
}

/// Whatever was done to them, the windows divide the frame above the command line: every cell is
/// in exactly one window's text, status line or divider, and the frame's right edge has no divider.
/// In a frame too small for them they can't all fit, and they still don't overlap.
fn check_tiling(ctx: &mut Ctx) -> Result<(), TestCaseError> {
    let (cols, rows) = ctx.frame_size();
    let area = rows.saturating_sub(1);
    let mut owner = vec![0u8; cols * area];
    let list = ctx.window_list();
    let placed = ctx.placements();
    for p in &placed {
        let (c, r) = ctx.window_size(p.window).unwrap();
        prop_assert!(c > 0 && r > 0);
        let reach = if p.col + c == cols { c } else { c + 1 };
        prop_assert!(p.col + reach <= cols, "{p:?} {c}x{r} in {cols}x{rows}");
        prop_assert!(p.row + r < area, "{p:?} {c}x{r} in {cols}x{rows}");
        for y in p.row..=p.row + r {
            for x in p.col..p.col + reach {
                owner[y * cols + x] += 1;
                prop_assert_eq!(owner[y * cols + x], 1, "({}, {}) twice", x, y);
            }
        }
    }
    // A frame with room for every window at its minimum is covered, with nothing left over.
    let room = list.len() * 2;
    if cols >= room && area >= room {
        prop_assert_eq!(placed.len(), list.len(), "every window is placed");
        prop_assert!(owner.iter().all(|n| *n == 1), "a cell has no window");
    }
    for w in &list {
        // Each view is the window's size.
        let b = ctx.window_buffer(*w).unwrap();
        let (c, r) = ctx.window_size(*w).unwrap();
        let g = ctx.text(b).unwrap().geometry(*w).unwrap();
        prop_assert_eq!((g.cols, g.rows), (c, r));
    }
    prop_assert!(
        list.contains(&ctx.focused().unwrap()),
        "the focus is on a live window"
    );
    let s = ctx.scene();
    prop_assert_eq!((s.cols, s.rows), (cols, rows));
    Ok(())
}

fn run_windows(ops: Vec<WinOp>) -> Result<(), TestCaseError> {
    let mut ctx = Ctx::new(vec![]).unwrap();
    ctx.set_frame_size(100, 40);
    let bufs: Vec<BufferId> = ["a\nb\nc\n", "", "long line here\n"]
        .iter()
        .map(|t| ctx.create_text("t", t))
        .collect();
    ctx.show(bufs[0], ShowIn::Current).unwrap();
    let pick = |ctx: &Ctx, i: u8| {
        let l = ctx.window_list();
        l[i as usize % l.len()]
    };
    for op in ops {
        match op {
            WinOp::Split(w, s) => {
                let w = pick(&ctx, w);
                let _ = ctx.split_window(w, s);
            }
            WinOp::Close(w) => {
                let w = pick(&ctx, w);
                let sole = ctx.window_list().len() == 1;
                prop_assert_eq!(ctx.close_window(w).is_err(), sole);
            }
            WinOp::Only(w) => {
                let w = pick(&ctx, w);
                ctx.only_window(w).unwrap();
                prop_assert_eq!(ctx.window_list(), [w]);
            }
            WinOp::Resize(w, c, r) => {
                let w = pick(&ctx, w);
                ctx.resize(w, c as usize, r as usize).unwrap();
            }
            WinOp::Frame(c, r) => ctx.set_frame_size(c as usize, r as usize),
            WinOp::Balance(w) => {
                let w = pick(&ctx, w);
                ctx.balance_windows(w);
            }
            WinOp::Show(w, b) => {
                let w = pick(&ctx, w);
                ctx.focus(w);
                ctx.show(bufs[b as usize % bufs.len()], ShowIn::Current)
                    .unwrap();
            }
            WinOp::Focus(w) => {
                let w = pick(&ctx, w);
                ctx.focus(w);
            }
            WinOp::Dir(s) => {
                if let Some(next) = ctx.focus_direction(s) {
                    prop_assert_eq!(ctx.focused(), Some(next));
                }
            }
        }
        check_tiling(&mut ctx)?;
    }
    Ok(())
}

#[derive(Clone, Debug)]
enum ModOp {
    Insert(u8, bool),
    Delete(u8),
    Silent(u8),
    Undo,
    Redo,
    Save,
    Boundary,
    Group(bool),
}

fn mod_op() -> impl Strategy<Value = ModOp> {
    prop_oneof![
        5 => (any::<u8>(), any::<bool>()).prop_map(|(a, s)| ModOp::Insert(a, s)),
        3 => any::<u8>().prop_map(ModOp::Delete),
        1 => any::<u8>().prop_map(ModOp::Silent),
        4 => Just(ModOp::Undo),
        3 => Just(ModOp::Redo),
        2 => Just(ModOp::Save),
        3 => Just(ModOp::Boundary),
        1 => any::<bool>().prop_map(ModOp::Group),
    ]
}

/// The flag is clear only when the text is what was saved. The other way round, an edit and its
/// inverse leave the same text with the flag set, as in Emacs.
fn run_modified(ops: Vec<ModOp>) -> Result<(), TestCaseError> {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "start");
    let w = ctx.open_window(b, 20, 5).unwrap();
    ctx.focus(w);
    let mut saved = "start".to_string();
    for op in ops {
        // Each op is a command of its own.
        ctx.run_until_idle();
        let t = ctx.text_mut(b).unwrap();
        match op {
            ModOp::Insert(at, end_step) => {
                let n = t.rope().len_chars();
                let at = t.rope().char_to_byte(at as usize % (n + 1));
                t.tx(Origin::User).insert(at, "xy").commit().unwrap();
                if end_step {
                    t.undo_boundary();
                }
            }
            ModOp::Delete(at) => {
                let n = t.rope().len_chars();
                if n > 0 {
                    let at = at as usize % n;
                    let (s, e) = (t.rope().char_to_byte(at), t.rope().char_to_byte(at + 1));
                    t.tx(Origin::User).delete(s..e).commit().unwrap();
                }
            }
            ModOp::Silent(at) => {
                let n = t.rope().len_chars();
                let at = t.rope().char_to_byte(at as usize % (n + 1));
                t.tx(Origin::Extension)
                    .silent()
                    .insert(at, "S")
                    .commit()
                    .unwrap();
            }
            ModOp::Undo => {
                let _ = t.undo();
            }
            ModOp::Redo => {
                let _ = t.redo();
            }
            ModOp::Save => {
                t.set_modified(false);
                saved = t.rope().to_string();
            }
            ModOp::Boundary => t.undo_boundary(),
            ModOp::Group(open) => {
                if open {
                    t.begin_undo_group();
                } else {
                    t.end_undo_group();
                }
            }
        }
        let t = ctx.text(b).unwrap();
        if !t.modified() {
            prop_assert_eq!(t.rope().to_string(), saved.clone(), "unmodified");
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: prop_cases().unwrap_or(2_000), max_shrink_iters: 5_000, ..ProptestConfig::default() })]
    #[test]
    fn windows_tile_the_frame(ops in prop::collection::vec(win_op(), 1..30)) {
        run_windows(ops)?;
    }
    #[test]
    fn unmodified_means_the_saved_text(ops in prop::collection::vec(mod_op(), 1..40)) {
        run_modified(ops)?;
    }
}
