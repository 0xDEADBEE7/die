//! The figure's keys (the design's Controls table, contract-p2 §4): the cursor's motions with
//! counts, the zooms and scrolls, hide/only/show-all, undo and redo, search, the table, the
//! settings, export, the reference, the key card, Esc, ⏎, and the keys a textless surface keeps.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Input, KeyAction, parse_keys};
use die_fig::data::SeriesKey;
use die_fig::interact::Cursor;
use std::time::Duration;

fn point(series: &str, row: u32) -> Cursor {
    Cursor::Point {
        series: SeriesKey::new(series, 0),
        row,
    }
}

fn settle(s: &mut Setup) {
    s.h.advance(Duration::from_millis(400));
}

fn row(s: &mut Setup) -> u32 {
    match cursor(s) {
        Some(Cursor::Point { row, .. }) => row,
        other => panic!("no cursor on a point: {other:?}"),
    }
}

fn undoable(s: &mut Setup) -> usize {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].undo.undoable()
}

#[test]
fn h_and_l_place_the_cursor_and_step_with_a_count() {
    let mut s = setup();
    assert_eq!(cursor(&mut s), None, "none while a figure opens");
    keys(&mut s.h, "l");
    assert!(cursor(&mut s).is_some(), "the first l places it");
    let r = row(&mut s);
    keys(&mut s.h, "l");
    assert_eq!(row(&mut s), r + 1);
    keys(&mut s.h, "3l");
    assert_eq!(row(&mut s), r + 4);
    keys(&mut s.h, "h");
    assert_eq!(row(&mut s), r + 3);
    // 0 after a count is a digit: 10 steps.
    keys(&mut s.h, "gg10l");
    assert_eq!(row(&mut s), 10);
    assert_eq!(undoable(&mut s), 0, "the cursor isn't undone");
}

#[test]
fn gg_and_g_go_to_the_first_and_last_point() {
    let mut s = setup();
    keys(&mut s.h, "lG");
    assert_eq!(row(&mut s), 20);
    keys(&mut s.h, "gg");
    assert_eq!(row(&mut s), 0);
}

#[test]
fn j_and_k_go_to_the_series_below_and_above() {
    let mut s = setup();
    // At x = 15, a is 15 and b is 5: b is below a.
    keys(&mut s.h, "gg15l");
    let on = cursor(&mut s);
    let start = match on {
        Some(Cursor::Point { series, .. }) => series.name.to_string(),
        _ => panic!(),
    };
    if start == "b" {
        keys(&mut s.h, "k");
    }
    assert_eq!(cursor(&mut s), Some(point("a", 15)));
    keys(&mut s.h, "j");
    assert_eq!(cursor(&mut s), Some(point("b", 15)));
    keys(&mut s.h, "k");
    assert_eq!(cursor(&mut s), Some(point("a", 15)));
}

#[test]
fn w_and_b_go_to_the_next_and_previous_peak() {
    let mut s = setup_with("peaks.csv", "x,v\n0,0\n1,1\n2,5\n3,1\n4,0\n5,1\n6,6\n7,1\n");
    keys(&mut s.h, "lgg");
    assert_eq!(row(&mut s), 0);
    keys(&mut s.h, "w");
    assert_eq!(row(&mut s), 2, "the peak at 5");
    keys(&mut s.h, "w");
    assert_eq!(row(&mut s), 6, "the peak at 6");
    keys(&mut s.h, "b");
    assert_eq!(row(&mut s), 2);
    // With a count, past the first.
    keys(&mut s.h, "gg2w");
    assert_eq!(row(&mut s), 6);
}

#[test]
fn h_m_l_go_to_the_views_edges_and_middle() {
    let mut s = setup();
    keys(&mut s.h, "++++");
    settle(&mut s);
    let (a, b) = view(&mut s).x.unwrap();
    let x_of = |s: &mut Setup| {
        let st = state(s);
        let st = st.borrow();
        let m = st.model.as_ref().unwrap();
        let Some(Cursor::Point { series, row }) = st.panes[&s.win].cursor.clone() else {
            panic!()
        };
        let info = m.series.iter().find(|i| i.key == series).unwrap();
        info.xs[row as usize]
    };
    // The samples are a unit apart: H and L take the first and last inside the view, and the
    // view stays as it was, with no scroll to put them a tenth inside.
    keys(&mut s.h, "H");
    let x = x_of(&mut s);
    assert!(x >= a && x < a + 1.0, "{x} at the left edge {a}");
    assert_eq!(view(&mut s).x, Some((a, b)), "H scrolls nothing");
    keys(&mut s.h, "L");
    let x = x_of(&mut s);
    assert!(x <= b && x > b - 1.0, "{x} at the right edge {b}");
    assert_eq!(view(&mut s).x, Some((a, b)), "L scrolls nothing");
    keys(&mut s.h, "M");
    let x = x_of(&mut s);
    assert!((x - (a + b) / 2.0).abs() <= 0.5, "{x} in the middle");
    assert_eq!(view(&mut s).x, Some((a, b)));
    // The first l puts the cursor at the view's left edge, and scrolls nothing either.
    let mut s = setup();
    keys(&mut s.h, "++++");
    settle(&mut s);
    let v = view(&mut s).x;
    keys(&mut s.h, "l");
    assert!(cursor(&mut s).is_some());
    assert_eq!(view(&mut s).x, v);
    // A step past the edge scrolls to keep it inside.
    keys(&mut s.h, "H");
    keys(&mut s.h, "h");
    settle(&mut s);
    assert_ne!(view(&mut s).x, v, "the view follows the cursor out");
}

#[test]
fn plus_equals_and_k_zoom_in_minus_and_j_zoom_out_eased_one_step_a_run() {
    let mut s = setup();
    keys(&mut s.h, "+");
    let st = state(&mut s);
    assert!(st.borrow().panes[&s.win].easing(), "a key's zoom eases");
    settle(&mut s);
    let one = view(&mut s).x.unwrap();
    assert!(one.1 - one.0 < 20.0);
    assert_eq!(s.h.next_timer(), None, "the ease's timer stops with it");
    keys(&mut s.h, "=K");
    settle(&mut s);
    let three = view(&mut s).x.unwrap();
    assert!(three.1 - three.0 < one.1 - one.0);
    assert_eq!(undoable(&mut s), 2, "a step each burst");
    keys(&mut s.h, "-J");
    settle(&mut s);
    let out = view(&mut s).x.unwrap();
    assert!(out.1 - out.0 > three.1 - three.0);
    // Zooming out stops at the data: no further than fit.
    keys(&mut s.h, "------");
    settle(&mut s);
    assert_eq!(view(&mut s).x, None, "back to fit, not past it");
    // Keys within a scroll burst of each other, as typed at once, are one step.
    let steps = undoable(&mut s);
    keys(&mut s.h, "+++");
    assert_eq!(undoable(&mut s), steps + 1);
    keys(&mut s.h, "u");
    settle(&mut s);
    assert_eq!(view(&mut s).x, None, "the run was one step");
}

#[test]
fn a_count_of_thousands_zooms_as_far_as_the_data_lets_it() {
    // 1.25 to the 4000th is no number: the count stops where the zoom does.
    let mut s = setup();
    keys(&mut s.h, "4000+");
    settle(&mut s);
    let (a, b) = view(&mut s).x.expect("zoomed in");
    assert!(b - a <= 2.0, "to the limit: {a}..{b}");
    keys(&mut s.h, "4000-");
    settle(&mut s);
    assert_eq!(view(&mut s).x, None, "and out to the fit");
}

#[test]
fn zero_resets_the_view() {
    let mut s = setup();
    keys(&mut s.h, "++");
    settle(&mut s);
    assert!(view(&mut s).x.is_some());
    keys(&mut s.h, "0");
    settle(&mut s);
    assert_eq!(view(&mut s).x, None);
}

#[test]
fn zh_zl_zh_zl_scroll_and_zz_centres() {
    let mut s = setup();
    keys(&mut s.h, "++++");
    settle(&mut s);
    let start = view(&mut s).x.unwrap();
    keys(&mut s.h, "zl");
    settle(&mut s);
    let right = view(&mut s).x.unwrap();
    assert!(right.0 > start.0, "zl: later");
    keys(&mut s.h, "zh");
    settle(&mut s);
    assert!((view(&mut s).x.unwrap().0 - start.0).abs() < 1e-6);
    keys(&mut s.h, "zL");
    settle(&mut s);
    let half = view(&mut s).x.unwrap();
    assert!(
        half.0 - start.0 > right.0 - start.0,
        "half a plot is more than an eighth"
    );
    keys(&mut s.h, "zH");
    settle(&mut s);
    keys(&mut s.h, "gg10lzz");
    settle(&mut s);
    let (a, b) = view(&mut s).x.unwrap();
    assert!(
        ((a + b) / 2.0 - 10.0).abs() < 0.5,
        "centred on x = 10: {a}..{b}"
    );
    // zj and zk are bound, and do nothing wrong on a chart whose y fits.
    keys(&mut s.h, "zjzk");
}

#[test]
fn x_hides_o_shows_only_x_shows_all_each_a_step() {
    let mut s = setup();
    let (a, b) = (SeriesKey::new("a", 0), SeriesKey::new("b", 0));
    keys(&mut s.h, "gg15l");
    if cursor(&mut s) != Some(point("a", 15)) {
        keys(&mut s.h, "k");
    }
    keys(&mut s.h, "o");
    assert_eq!(view(&mut s).only, Some(a.clone()));
    keys(&mut s.h, "o");
    assert_eq!(view(&mut s).only, None);
    keys(&mut s.h, "x");
    assert!(view(&mut s).hidden.contains(&a));
    assert_eq!(cursor(&mut s), None, "the cursor goes with its series");
    keys(&mut s.h, "lx");
    assert!(view(&mut s).hidden.contains(&b), "{:?}", view(&mut s));
    keys(&mut s.h, "X");
    assert!(view(&mut s).hidden.is_empty());
    assert_eq!(undoable(&mut s), 5);
    keys(&mut s.h, "u");
    assert_eq!(view(&mut s).hidden.len(), 2);
    keys(&mut s.h, "<C-r>");
    assert!(view(&mut s).hidden.is_empty());
}

#[test]
fn u_and_c_r_and_cmd_z_undo_and_redo_view_changes_eased() {
    let mut s = setup();
    keys(&mut s.h, "+");
    settle(&mut s);
    let zoomed = view(&mut s).x;
    keys(&mut s.h, "u");
    let st = state(&mut s);
    assert!(st.borrow().panes[&s.win].easing(), "undo eases");
    settle(&mut s);
    assert_eq!(view(&mut s).x, None);
    keys(&mut s.h, "<C-r>");
    settle(&mut s);
    assert_eq!(view(&mut s).x, zoomed);
    keys(&mut s.h, "<D-z>");
    settle(&mut s);
    assert_eq!(view(&mut s).x, None);
    keys(&mut s.h, "<D-S-z>");
    settle(&mut s);
    assert_eq!(view(&mut s).x, zoomed);
    // Nothing more to redo: said, not an error.
    let errors = keys_errors(&mut s.h, "<C-r>");
    assert!(errors.iter().any(|e| e.contains("redo")), "{errors:?}");
    assert_eq!(s.h.next_timer(), None);
}

#[test]
fn slash_finds_a_series_n_and_cap_n_go_on_and_esc_lets_its_emphasis_go() {
    let mut s = setup();
    keys(&mut s.h, "/b<CR>");
    let b = SeriesKey::new("b", 0);
    match cursor(&mut s) {
        Some(Cursor::Point { series, .. }) => assert_eq!(series, b),
        other => panic!("{other:?}"),
    }
    let st = state(&mut s);
    assert_eq!(
        st.borrow().panes[&s.win].found,
        Some(b.clone()),
        "brought forward"
    );
    assert_eq!(st.borrow().panes[&s.win].overlay().emphasis, Some(b));
    keys(&mut s.h, "n");
    assert!(cursor(&mut s).is_some());
    keys(&mut s.h, "N");
    keys(&mut s.h, "<Esc>");
    assert_eq!(st.borrow().panes[&s.win].found, None);
}

#[test]
fn t_shows_the_data_as_tsv_beside_the_figure() {
    let mut s = setup();
    keys(&mut s.h, "t");
    let table = s.h.find_buffer("*fig table: data.csv*").expect("the table");
    let text = s.h.text(table).unwrap().rope().to_string();
    assert!(text.contains('\t') && text.lines().count() > 20, "{text}");
    assert_eq!(s.h.window_list().len(), 2, "beside, in a split");
    assert_eq!(s.h.window_buffer(s.win), Some(s.buf), "the figure stays");
    // A column a series: a hidden one is left out.
    let header = |t: &str| {
        t.lines()
            .next()
            .unwrap_or("")
            .split('\t')
            .map(String::from)
            .collect::<Vec<_>>()
    };
    let before = header(&text);
    assert!(before.contains(&"b".to_string()), "{before:?}");
    s.h.focus(s.win);
    keys(&mut s.h, "/b<CR>");
    assert!(
        matches!(cursor(&mut s), Some(Cursor::Point { ref series, .. }) if &*series.name == "b")
    );
    keys(&mut s.h, "x");
    s.h.focus(s.win);
    keys(&mut s.h, "t");
    let text = s.h.text(table).unwrap().rope().to_string();
    let after = header(&text);
    assert_eq!(after.len(), before.len() - 1, "{after:?}");
    assert!(!after.contains(&"b".to_string()), "{after:?}");
}

#[test]
fn s_turns_a_setting_and_each_is_an_undo_step() {
    let mut s = setup();
    keys(&mut s.h, "s");
    let candidates = s.h.prompt_candidates();
    assert!(candidates.contains(&"grid: on"), "{candidates:?}");
    keys(&mut s.h, "grid<CR>");
    let st = state(&mut s);
    assert_eq!(st.borrow().toggles.grid, Some(false));
    assert!(
        s.h.messages.iter().any(|m| m == "settings: grid off"),
        "{:?}",
        s.h.messages
    );
    assert_eq!(undoable(&mut s), 1);
    let built = st.borrow().model_gen;
    keys(&mut s.h, "u");
    assert_eq!(st.borrow().toggles.grid, None, "undone");
    assert!(st.borrow().model_gen > built, "the model built again");
}

#[test]
fn e_exports_a_png_beside_the_file_and_an_svg_by_its_name() {
    let dir = TempDir::new("export");
    let path = dir.path().join("rates.csv");
    std::fs::write(&path, crossing()).unwrap();
    let mut h = harness();
    let src = h.open_file(&path).unwrap();
    h.show(src, die_core::ShowIn::Current).unwrap();
    h.run_until_idle();
    h.run("fig-toggle", &die_core::Args::default()).unwrap();
    h.run_until_idle();
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    let mut s = Setup { h, src, buf, win };
    assert!(state(&mut s).borrow().model.is_some());
    keys(&mut s.h, "E");
    let line = s.h.prompt_text().unwrap_or_default();
    assert!(line.ends_with("rates.png"), "{line}");
    keys(&mut s.h, "<CR>");
    let png = dir.path().join("rates.png");
    let bytes = std::fs::read(&png).expect("written");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert!(
        s.h.messages.iter().any(|m| m.starts_with("Wrote ")),
        "{:?}",
        s.h.messages
    );
    // There already: asked first.
    keys(&mut s.h, "E<CR>");
    assert!(s.h.prompt_text().is_some(), "the overwrite question");
    keys(&mut s.h, "n<CR>");
    // An SVG by its name.
    keys(&mut s.h, "E<BS><BS><BS>svg<CR>");
    let svg = std::fs::read_to_string(dir.path().join("rates.svg")).expect("the svg");
    assert!(svg.starts_with("<svg") || svg.contains("<svg"), "{svg}");
    // Neither: refused.
    let errors = keys_errors(&mut s.h, "E<BS><BS><BS>txt<CR>");
    assert!(
        errors.iter().any(|e| e.contains(".png or an .svg")),
        "{errors:?}"
    );
}

#[test]
fn c_is_bound_to_copy_the_picture() {
    // Not run: it would put a picture on this machine's pasteboard.
    let mut s = setup();
    let events = parse_keys("C").unwrap();
    s.h.focus(s.win);
    assert!(matches!(
        s.h.describe_key(&events).unwrap().action,
        KeyAction::Command { ref name, .. } if name == "fig-copy-image"
    ));
}

#[test]
fn m_sets_the_reference_and_backtick_returns_to_it() {
    let mut s = setup();
    let errors = keys_errors(&mut s.h, "m");
    assert!(errors.iter().any(|e| e.contains("no cursor")), "{errors:?}");
    keys(&mut s.h, "gg4l");
    let at = cursor(&mut s);
    keys(&mut s.h, "m");
    assert_eq!(view(&mut s).reference, at);
    assert_eq!(undoable(&mut s), 0, "the reference is no undo step");
    keys(&mut s.h, "6l`");
    assert_eq!(cursor(&mut s), at);
    keys(&mut s.h, "m");
    assert_eq!(view(&mut s).reference, None, "m on it clears it");
}

#[test]
fn question_mark_shows_the_key_card_until_any_key() {
    let mut s = setup();
    keys(&mut s.h, "?");
    let st = state(&mut s);
    assert!(st.borrow().panes[&s.win].help);
    assert!(
        st.borrow().panes[&s.win].overlay_bounds().is_some(),
        "drawn"
    );
    keys(&mut s.h, "l");
    assert!(!st.borrow().panes[&s.win].help, "a bound key closes it");
    keys(&mut s.h, "?");
    // A key no keymap takes closes it too, quietly.
    raw_key(
        &mut s.h,
        die_core::KeyEvent::press(die_core::Key::Char(' ')),
    );
    assert!(!st.borrow().panes[&s.win].help);
    assert_eq!(
        st.borrow().panes[&s.win].overlay_bounds().is_some(),
        cursor(&mut s).is_some()
    );
}

#[test]
fn the_key_card_goes_with_any_key_bound_or_not_the_figures() {
    let mut s = setup();
    let st = state(&mut s);
    for k in [":<Esc>", "<C-w>v", "\\z"] {
        keys(&mut s.h, "?");
        let here = s.h.focused().unwrap();
        assert!(st.borrow().panes[&here].help, "{k}: up");
        keys(&mut s.h, k);
        assert!(
            st.borrow().panes.values().all(|p| !p.help),
            "{k}: gone from every window"
        );
    }
}

#[test]
fn zk_shows_higher_values_and_zj_lower_where_y_is_free() {
    let mut text = String::from("u,v\n");
    for i in 0..60 {
        text.push_str(&format!("{},{}\n", (i * 37) % 60, i));
    }
    let mut s = setup_with("points.csv", &text);
    keys(&mut s.h, "++");
    settle(&mut s);
    let (lo, hi) = view(&mut s).y.expect("a scatter zooms y too");
    keys(&mut s.h, "zk");
    settle(&mut s);
    let (a, b) = view(&mut s).y.unwrap();
    assert!(a > lo && b > hi, "up: {lo}..{hi} to {a}..{b}");
    keys(&mut s.h, "zjzj");
    settle(&mut s);
    let (c, d) = view(&mut s).y.unwrap();
    assert!(c < a && d < b, "down: {a}..{b} to {c}..{d}");
}

#[test]
fn esc_clears_the_key_card_then_the_reference() {
    let mut s = setup();
    keys(&mut s.h, "gg3lm?");
    let st = state(&mut s);
    keys(&mut s.h, "<Esc>");
    assert!(!st.borrow().panes[&s.win].help);
    assert!(view(&mut s).reference.is_some(), "one thing an Esc");
    keys(&mut s.h, "<Esc>");
    assert_eq!(view(&mut s).reference, None);
    // Hidden series stay hidden: that's X.
    keys(&mut s.h, "x<Esc>");
    assert_eq!(view(&mut s).hidden.len(), 1);
}

#[test]
fn return_opens_the_detail_panel_for_the_cursor_and_closes_it() {
    let mut s = setup();
    let errors = keys_errors(&mut s.h, "<CR>");
    assert!(
        errors.iter().any(|m| m.contains("no cursor yet")),
        "{errors:?}"
    );
    keys(&mut s.h, "gg7l<CR>");
    let st = state(&mut s);
    let rows = {
        let st = st.borrow();
        let p = st.panes[&s.win].panel.as_ref().expect("open");
        p.tree
            .rows
            .iter()
            .map(|r| r.value.clone())
            .collect::<Vec<_>>()
    };
    assert!(rows.iter().any(|v| v == "7"), "the cursor's x: {rows:?}");
    keys(&mut s.h, "<CR>");
    assert!(st.borrow().panes[&s.win].panel.is_none(), "closed again");
}

#[test]
fn the_textless_surface_keeps_the_window_keys_colon_and_leader_z() {
    let mut s = setup();
    for (keys_, cmd) in [
        (":", "vim-ex"),
        ("ZZ", "vim-quit"),
        ("ZQ", "vim-quit"),
        ("<C-^>", "vim-alternate-buffer"),
        ("<C-o>", "vim-jump-backward"),
        ("\\z", "toggle-max-width"),
        ("<C-w>v", "vim-window-vsplit"),
        ("<C-w><Esc>", "vim-cancel"),
        ("\\v", "fig-toggle"),
        ("K", "fig-zoom-in"),
        ("J", "fig-zoom-out"),
    ] {
        let events = parse_keys(keys_).unwrap();
        let action = s.h.describe_key(&events).unwrap().action;
        assert!(
            matches!(action, KeyAction::Command { ref name, .. } if name == cmd),
            "{keys_}: {action:?}"
        );
    }
    keys(&mut s.h, ":");
    assert_eq!(s.h.state(), die_core::State::CommandLine);
    keys(&mut s.h, "<Esc>");
    // \z gives the figure a file's width, and its whole width back.
    keys(&mut s.h, "\\z\\z");
}

#[test]
fn space_is_unbound_and_does_nothing() {
    let mut s = setup();
    let before = view(&mut s);
    let events = parse_keys("<Space>").unwrap();
    let action = s.h.describe_key(&events).unwrap().action;
    assert!(!matches!(action, KeyAction::Command { .. }), "{action:?}");
    s.h.send(Input::Key(die_core::KeyEvent::press(die_core::Key::Char(
        ' ',
    ))));
    assert_eq!(view(&mut s), before);
    assert_eq!(cursor(&mut s), None);
    let st = state(&mut s);
    assert!(st.borrow().panes[&s.win].overlay().brush.is_none());
}
