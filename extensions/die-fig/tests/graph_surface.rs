//! A graph in the editor (contract-p3 §4): `\v` on fig's graph JSON and on a CSV with an id and
//! a parent column, the keys (h j k l, ] [, gg G, zoom and fit), hover and its dwell, a click, ⇧-
//! click and the path, a node's drag with its pin, undo and Esc mid-drag, a background pan, the
//! wheel's pan and ⌘-wheel zoom, double-clicks for the detail and for fit, the detail panel's keys
//! and pointer (on a chart too), search as it's typed with Esc back and n N, `t` for both kinds,
//! and the session's pins, layout and panel across a restart.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Args, Button, Config, Harness, Input, Mods, PointerKind, ShowIn};
use die_fig::Fig;
use die_fig::detail::Item;
use die_fig::graph::Target;
use die_fig::interact::Cursor;
use die_fig::scene::{Hit, Rect};
use die_vim::Vim;
use jiff::tz::TimeZone;
use std::time::Duration;

/// A fixture's text, read at run time.
fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/data/graphs")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

/// `name` holding `text`, shown as its figure in a window 1000 × 490 logical px.
fn graph_with(name: &str, text: &str, cols: usize, rows: usize) -> Setup {
    let mut h = harness();
    h.set_window_size(cols, rows);
    h.run_until_idle();
    let (src, win) = h.open_text(name, text).unwrap();
    h.run_until_idle();
    let errors = run(&mut h, "fig-toggle");
    assert!(errors.is_empty(), "{errors:?}");
    let buf = h.window_buffer(win).unwrap();
    let mut s = Setup { h, src, buf, win };
    let st = state(&mut s);
    assert!(
        st.borrow().graph.is_some(),
        "a graph: {:?}",
        st.borrow().error
    );
    s
}

/// services.json, the m3 fixture, charted.
fn services() -> Setup {
    graph_with("services.json", &fixture("services.json"), 200, 50)
}

/// Let every ease finish, every burst end and every dwell come.
fn settle(s: &mut Setup) {
    s.h.advance(Duration::from_millis(400));
}

/// Where a node is on screen, as last drawn.
fn node_at(s: &mut Setup, id: &str) -> (f32, f32) {
    let st = state(s);
    let st = st.borrow();
    let g = st.graph.as_ref().expect("a graph");
    let i = g.index_of(id).expect("the node");
    let l = st.panes[&s.win].graph.as_ref().expect("drawn");
    l.node(i).expect("shown").at
}

fn click_node(s: &mut Setup, id: &str) {
    let at = node_at(s, id);
    click(s, at);
}

fn shift_node(s: &mut Setup, id: &str) {
    let at = node_at(s, id);
    click_with(s, at, SHIFT);
}

fn double_node(s: &mut Setup, id: &str) {
    let at = node_at(s, id);
    double_click(s, at);
}

fn node(id: &str) -> Cursor {
    Cursor::Node(id.into())
}

/// The window's hover target.
fn target(s: &mut Setup) -> Option<Target> {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].target
}

/// A place in the graph's region with no node or edge near it.
fn empty_spot(s: &mut Setup) -> (f32, f32) {
    let st = state(s);
    let st = st.borrow();
    let g = st.graph.as_ref().unwrap();
    let l = st.panes[&s.win].graph.as_ref().unwrap();
    let r = l.region;
    let mut best: Option<((f32, f32), f32)> = None;
    for i in 1..40 {
        for j in 1..20 {
            let p = (r.x + r.w * i as f32 / 40.0, r.y + r.h * j as f32 / 20.0);
            if die_fig::graph::hover(g, l, p.0, p.1).is_some()
                || l.hits.nearest_segment(p.0, p.1, 16.0).is_some()
            {
                continue;
            }
            let d = (l.nodes.iter().flatten())
                .map(|n| (n.at.0 - p.0).hypot(n.at.1 - p.1))
                .fold(f32::INFINITY, f32::min);
            if best.is_none_or(|(_, b)| d > b) {
                best = Some((p, d));
            }
        }
    }
    best.expect("an empty spot").0
}

/// The middle of an edge between two nodes, where the pointer finds that edge, and its index.
fn edge_spot(s: &mut Setup, a: &str, b: &str) -> ((f32, f32), u32) {
    let st = state(s);
    let st = st.borrow();
    let g = st.graph.as_ref().unwrap();
    let (ia, ib) = (g.index_of(a).unwrap() as u32, g.index_of(b).unwrap() as u32);
    let k = (g.ends.iter())
        .position(|&e| e == (ia, ib) || e == (ib, ia))
        .unwrap();
    let l = st.panes[&s.win].graph.as_ref().unwrap();
    let e = l.edges[k].as_ref().unwrap();
    for t in [0.5, 0.4, 0.6, 0.3, 0.7] {
        let n = e.pts.len() - 1;
        let f = t * n as f32;
        let (i, u) = (f.floor() as usize, f.fract());
        let (p, q) = (e.pts[i], e.pts[(i + 1).min(n)]);
        let at = (p.0 + (q.0 - p.0) * u, p.1 + (q.1 - p.1) * u);
        if die_fig::graph::hover(g, l, at.0, at.1) == Some(Target::Edge(k as u32)) {
            return (at, k as u32);
        }
    }
    panic!("no spot on {a}–{b} finds it");
}

fn undoable(s: &mut Setup) -> usize {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].undo.undoable()
}

#[test]
fn backslash_v_shows_fig_graph_json_and_a_csv_tree_as_graphs() {
    let mut s = services();
    let (base, over) = buffers(&mut s);
    let (w, h, px) = s.h.pixels(base).expect("drawn");
    let inked = (px.chunks_exact(4))
        .filter(|p| p[..3] != [0x1a, 0x1a, 0x1a])
        .count();
    assert!(inked > 2000, "{inked} px of graph on {w}×{h}");
    assert!(status(&mut s).contains("18 nodes"), "{}", status(&mut s));
    assert_ne!(base, over);
    assert_eq!(s.h.next_timer(), None, "at rest, nothing runs");
    // fig's own graph shape, with its fig blocks.
    let mut f = graph_with("fig-graph.json", &fixture("fig-graph.json"), 200, 50);
    assert!(
        state(&mut f).borrow().model.is_none(),
        "a graph, not a chart"
    );
    // A table with an id and a parent column: a tree.
    let mut t = graph_with("org.csv", &fixture("org.csv"), 200, 50);
    let st = state(&mut t);
    let layout = st.borrow().graph.as_ref().unwrap().layout.clone();
    assert!(
        matches!(layout, die_fig::spec::Layout::Tree { .. }),
        "{layout:?}"
    );
    // And back to the text.
    keys(&mut s.h, "\\v");
    assert_eq!(s.h.window_buffer(s.win), Some(s.src));
}

#[test]
fn hjkl_move_by_direction_and_square_brackets_turn_round_the_anchor() {
    let mut s = services();
    keys(&mut s.h, "l");
    let first = cursor(&mut s).expect("placed by the first move");
    assert_eq!(undoable(&mut s), 0, "the cursor isn't an undo step");
    // From worker, l goes to a node to its right.
    click_node(&mut s, "worker");
    assert_eq!(cursor(&mut s), Some(node("worker")));
    let (wx, wy) = node_at(&mut s, "worker");
    keys(&mut s.h, "l");
    let Some(Cursor::Node(right)) = cursor(&mut s) else {
        panic!("moved");
    };
    assert!(
        node_at(&mut s, &right).0 > wx,
        "{right} lies right of worker"
    );
    keys(&mut s.h, "h");
    let left = cursor_id(&mut s);
    assert!(node_at(&mut s, &left).0 < node_at(&mut s, &right).0);
    keys(&mut s.h, "j");
    let below = cursor_id(&mut s);
    let _ = (first, wy, below);
    // ] and [ go round worker's neighbours, the anchor kept while they repeat.
    click_node(&mut s, "worker");
    let neighbours = ["api", "queue", "scheduler", "cache", "db", "metrics"];
    let mut seen = vec![];
    for _ in 0..neighbours.len() {
        keys(&mut s.h, "]");
        seen.push(cursor_id(&mut s));
    }
    let mut sorted = seen.clone();
    sorted.sort();
    let mut want: Vec<String> = neighbours.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(sorted, want, "every neighbour once, round worker: {seen:?}");
    keys(&mut s.h, "]");
    assert_eq!(cursor_id(&mut s), seen[0], "round again");
    keys(&mut s.h, "[");
    assert_eq!(&cursor_id(&mut s), seen.last().unwrap(), "and back");
    // gg and G: the first and last node in the file.
    keys(&mut s.h, "gg");
    assert_eq!(cursor(&mut s), Some(node("api")));
    keys(&mut s.h, "G");
    assert_eq!(cursor(&mut s), Some(node("metrics")));
}

fn cursor_id(s: &mut Setup) -> String {
    match cursor(s) {
        Some(Cursor::Node(id)) => id.to_string(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn zoom_keys_ease_and_fit_brings_the_whole_graph_back() {
    let mut s = services();
    click_node(&mut s, "db");
    let at = node_at(&mut s, "db");
    keys(&mut s.h, "++");
    let st = state(&mut s);
    assert!(st.borrow().panes[&s.win].easing(), "a key's zoom eases");
    settle(&mut s);
    assert!(!st.borrow().panes[&s.win].easing());
    let v = view(&mut s);
    assert!(v.x.is_some(), "zoomed: {v:?}");
    let now = node_at(&mut s, "db");
    assert!(
        (now.0 - at.0).abs() < 1.0 && (now.1 - at.1).abs() < 1.0,
        "about the cursor: {at:?} → {now:?}"
    );
    assert_eq!(undoable(&mut s), 1, "two keys in a burst, one step");
    keys(&mut s.h, "zl");
    settle(&mut s);
    keys(&mut s.h, "zz");
    settle(&mut s);
    keys(&mut s.h, "0");
    settle(&mut s);
    assert_eq!(view(&mut s).x, None, "fit");
    keys(&mut s.h, "u");
    settle(&mut s);
    assert!(view(&mut s).x.is_some(), "u undoes the fit");
    assert_eq!(s.h.next_timer(), None);
}

#[test]
fn hover_finds_a_node_then_an_edge_and_the_node_comes_into_focus_after_its_dwell() {
    let mut s = services();
    let (_, over) = buffers(&mut s);
    presents(&mut s.h);
    let at = node_at(&mut s, "worker");
    mv(&mut s, (at.0 + 1.0, at.1));
    let i = state(&mut s)
        .borrow()
        .graph
        .as_ref()
        .unwrap()
        .index_of("worker")
        .unwrap();
    assert_eq!(target(&mut s), Some(Target::Node(i as u32)), "no delay");
    assert_eq!(hover(&mut s), Some(node("worker")));
    let p = presents(&mut s.h);
    assert!(
        !p.is_empty() && p.iter().all(|(id, _)| *id == over),
        "{p:?}"
    );
    let st = state(&mut s);
    assert_eq!(st.borrow().panes[&s.win].dwell, None, "not yet in focus");
    s.h.advance(Duration::from_millis(120));
    assert_eq!(
        st.borrow().panes[&s.win].dwell,
        Some(node("worker")),
        "in focus after the dwell"
    );
    assert_eq!(
        st.borrow().panes[&s.win]
            .graph_overlay(st.borrow().graph.as_ref().unwrap())
            .emphasis,
        Some(node("worker"))
    );
    assert_eq!(s.h.next_timer(), None, "no timer once it's settled");
    // An edge, where no node is near.
    let (e, k) = edge_spot(&mut s, "warehouse", "analytics");
    mv(&mut s, e);
    assert_eq!(target(&mut s), Some(Target::Edge(k)));
    assert_eq!(hover(&mut s), None, "an edge is no cursor");
    s.h.advance(Duration::from_millis(120));
    assert_eq!(
        st.borrow().panes[&s.win].dwell,
        None,
        "let go after the dwell"
    );
    // The pointer leaving clears it all.
    mv(&mut s, at);
    s.h.send(Input::PointerLeft);
    assert_eq!(target(&mut s), None);
    s.h.advance(Duration::from_millis(120));
    assert_eq!(st.borrow().panes[&s.win].dwell, None);
}

#[test]
fn a_click_on_a_node_or_an_edge_puts_the_cursor_and_the_background_does_nothing() {
    let mut s = services();
    click_node(&mut s, "cache");
    assert_eq!(cursor(&mut s), Some(node("cache")));
    let (e, _) = edge_spot(&mut s, "warehouse", "analytics");
    let (wh, an) = (node_at(&mut s, "warehouse"), node_at(&mut s, "analytics"));
    click(&mut s, e);
    let nearer = if (wh.0 - e.0).hypot(wh.1 - e.1) < (an.0 - e.0).hypot(an.1 - e.1) {
        "warehouse"
    } else {
        "analytics"
    };
    assert_eq!(cursor(&mut s), Some(node(nearer)), "the nearer end");
    let spot = empty_spot(&mut s);
    click(&mut s, spot);
    assert_eq!(cursor(&mut s), Some(node(nearer)), "nothing");
    assert_eq!(undoable(&mut s), 0);
}

#[test]
fn shift_click_sets_the_reference_and_the_path_shows_to_the_cursor() {
    let mut s = services();
    shift_node(&mut s, "web");
    assert_eq!(view(&mut s).reference, Some(node("web")));
    assert_eq!(cursor(&mut s), None, "the cursor isn't moved");
    let before = state(&mut s).borrow().panes[&s.win].overlay_bounds();
    click_node(&mut s, "analytics");
    let st = state(&mut s);
    let path = {
        let st = st.borrow();
        let g = st.graph.as_ref().unwrap();
        g.path(g.index_of("web").unwrap(), g.index_of("analytics").unwrap())
    };
    assert!(path.is_some_and(|p| p.len() >= 4), "a path of hops");
    assert_ne!(st.borrow().panes[&s.win].overlay_bounds(), before, "drawn");
    // ⇧-click it again: cleared. And Esc clears it too.
    shift_node(&mut s, "web");
    assert_eq!(view(&mut s).reference, None);
    shift_node(&mut s, "web");
    keys(&mut s.h, "<Esc>");
    assert_eq!(view(&mut s).reference, None);
}

#[test]
fn a_node_dragged_moves_and_pins_as_one_undo_step_and_esc_mid_drag_puts_it_back() {
    let mut s = services();
    let from = node_at(&mut s, "mailer");
    let others = node_at(&mut s, "db");
    let to = (from.0 + 60.0, from.1 - 30.0);
    gesture(
        &mut s,
        &[
            (from.0 + 2.0, from.1 + 1.0),
            (from.0 + 20.0, from.1),
            (to.0 + 2.0, to.1 + 1.0),
        ],
    );
    let v = view(&mut s);
    assert!(v.pinned.contains_key("mailer"), "pinned: {v:?}");
    let now = node_at(&mut s, "mailer");
    assert!(
        (now.0 - to.0).abs() < 1.5 && (now.1 - to.1).abs() < 1.5,
        "the grab kept under the pointer: {now:?} against {to:?}"
    );
    let still = node_at(&mut s, "db");
    assert!(
        (still.0 - others.0).abs() < 0.5 && (still.1 - others.1).abs() < 0.5,
        "nothing else moves"
    );
    assert_eq!(undoable(&mut s), 1, "one step for the move and the pin");
    assert!(status(&mut s).contains("1 pinned"), "{}", status(&mut s));
    keys(&mut s.h, "u");
    settle(&mut s);
    assert!(view(&mut s).pinned.is_empty(), "u lets it go");
    let back = node_at(&mut s, "mailer");
    assert!((back.0 - from.0).abs() < 1.0 && (back.1 - from.1).abs() < 1.0);
    // Esc mid-drag: back where it was, unpinned, no step.
    let steps = undoable(&mut s);
    pointer(
        &mut s,
        PointerKind::Press(Button::Left),
        from,
        Mods::default(),
        1,
    );
    pointer(
        &mut s,
        PointerKind::Drag(Button::Left),
        (from.0 + 40.0, from.1),
        Mods::default(),
        0,
    );
    assert!(view(&mut s).pinned.contains_key("mailer"), "live");
    keys(&mut s.h, "<Esc>");
    pointer(
        &mut s,
        PointerKind::Release(Button::Left),
        (from.0 + 40.0, from.1),
        Mods::default(),
        1,
    );
    assert!(view(&mut s).pinned.is_empty(), "put back");
    assert_eq!(undoable(&mut s), steps);
    let back = node_at(&mut s, "mailer");
    assert!((back.0 - from.0).abs() < 1.0 && (back.1 - from.1).abs() < 1.0);
}

#[test]
fn a_drag_on_the_background_pans_one_to_one_and_is_one_undo_step() {
    let mut s = services();
    let spot = empty_spot(&mut s);
    let before = node_at(&mut s, "api");
    gesture(
        &mut s,
        &[
            spot,
            (spot.0 + 10.0, spot.1 + 4.0),
            (spot.0 + 30.0, spot.1 + 12.0),
        ],
    );
    let after = node_at(&mut s, "api");
    assert!(
        (after.0 - before.0 - 30.0).abs() < 1.0 && (after.1 - before.1 - 12.0).abs() < 1.0,
        "1:1: {before:?} → {after:?}"
    );
    assert_eq!(undoable(&mut s), 1);
    assert!(view(&mut s).pinned.is_empty(), "nothing pinned");
    // ⌥-drag on a node pans too, rather than moving it.
    let at = node_at(&mut s, "worker");
    gesture_with(
        &mut s,
        &[at, (at.0 + 8.0, at.1), (at.0 + 20.0, at.1)],
        ALT,
        Button::Left,
    );
    assert!(view(&mut s).pinned.is_empty());
    assert_eq!(undoable(&mut s), 2);
}

#[test]
fn the_wheel_pans_in_two_dimensions_and_cmd_wheel_zooms_about_the_pointer_one_step_a_burst() {
    let mut s = services();
    let spot = empty_spot(&mut s);
    let before = node_at(&mut s, "api");
    for _ in 0..3 {
        scroll(&mut s, spot, (1.0, 0.5), Mods::default());
    }
    let after = node_at(&mut s, "api");
    assert!(
        after.0 < before.0 && after.1 < before.1,
        "{before:?} → {after:?}"
    );
    settle(&mut s);
    assert_eq!(undoable(&mut s), 1, "a burst is one step");
    // ⌘-scroll zooms about the pointer: the node under it stays.
    let at = node_at(&mut s, "db");
    let span = view(&mut s).x.map(|x| x.1 - x.0).unwrap();
    for _ in 0..2 {
        scroll(&mut s, at, (0.0, -2.0), CMD);
    }
    let now = node_at(&mut s, "db");
    assert!(
        (now.0 - at.0).abs() < 1.0 && (now.1 - at.1).abs() < 1.0,
        "{at:?} → {now:?}"
    );
    let zoomed = view(&mut s).x.map(|x| x.1 - x.0).unwrap();
    assert!(zoomed < span, "zoomed in");
    assert!(
        !state(&mut s).borrow().panes[&s.win].easing(),
        "the wheel never eases"
    );
    settle(&mut s);
    assert_eq!(undoable(&mut s), 2);
}

#[test]
fn a_double_click_on_a_node_opens_its_detail_and_on_the_background_fits() {
    let mut s = services();
    double_node(&mut s, "worker");
    let st = state(&mut s);
    {
        let st = st.borrow();
        let p = &st.panes[&s.win];
        let panel = p.panel.as_ref().expect("the panel opened");
        assert_eq!(panel.tree.title, "worker");
        assert!(panel.tree.kind.contains("degree 6"), "{}", panel.tree.kind);
        assert_eq!(p.cursor, Some(node("worker")));
        // The figure lays out in what the panel leaves.
        let (fw, _) = p.figure_size();
        assert!(fw < p.size().0 * 0.75, "{fw} of {}", p.size().0);
        assert!(p.graph.as_ref().unwrap().size.0 <= fw);
    }
    // An edge's detail by a double click on it.
    let (e, _) = edge_spot(&mut s, "warehouse", "analytics");
    double_click(&mut s, e);
    let title = st.borrow().panes[&s.win]
        .panel
        .as_ref()
        .unwrap()
        .tree
        .title
        .clone();
    assert!(
        title.contains("warehouse") && title.contains("analytics"),
        "{title}"
    );
    keys(&mut s.h, "<Esc>");
    assert!(st.borrow().panes[&s.win].panel.is_none(), "Esc closes it");
    // On the background: fit, eased, one step.
    keys(&mut s.h, "++");
    settle(&mut s);
    let steps = undoable(&mut s);
    let spot = empty_spot(&mut s);
    double_click(&mut s, spot);
    settle(&mut s);
    assert_eq!(view(&mut s).x, None);
    assert_eq!(undoable(&mut s), steps + 1);
}

/// The window's panel's row and focus.
fn panel_at(s: &mut Setup) -> (String, usize, bool) {
    let st = state(s);
    let st = st.borrow();
    let p = st.panes[&s.win].panel.as_ref().expect("open");
    (p.tree.title.clone(), p.state.row, p.state.focused)
}

#[test]
fn the_panel_opens_on_return_takes_the_keys_on_tab_and_follows_the_cursor() {
    let mut s = services();
    let errors = keys_errors(&mut s.h, "<CR>");
    assert!(
        errors.iter().any(|m| m.contains("no cursor yet")),
        "{errors:?}"
    );
    click_node(&mut s, "worker");
    keys(&mut s.h, "<CR>");
    let (title, row, focused) = panel_at(&mut s);
    assert_eq!((title.as_str(), focused), ("worker", false));
    // j moves the figure's cursor while the panel hasn't the keys.
    keys(&mut s.h, "<Tab>");
    assert!(panel_at(&mut s).2, "Tab: the keys to the panel");
    keys(&mut s.h, "j");
    assert!(panel_at(&mut s).1 > row, "j moves the panel's cursor");
    assert_eq!(cursor(&mut s), Some(node("worker")), "not the figure's");
    keys(&mut s.h, "G");
    let last = panel_at(&mut s).1;
    keys(&mut s.h, "gg");
    assert!(panel_at(&mut s).1 < last);
    // To the neighbour api's row, y copies its id, ⏎ takes the cursor there and the panel
    // follows it.
    let api = {
        let st = state(&mut s);
        let st = st.borrow();
        let p = st.panes[&s.win].panel.as_ref().unwrap();
        (p.tree.rows.iter())
            .position(|r| matches!(&r.item, Some(Item::Node { id, .. }) if &**id == "api"))
            .expect("api among worker's neighbours")
    };
    for _ in 0..40 {
        if panel_at(&mut s).1 == api {
            break;
        }
        keys(&mut s.h, "j");
    }
    assert_eq!(panel_at(&mut s).1, api);
    keys(&mut s.h, "y");
    let r = s.h.register('"').map(|r| r.text.clone());
    assert_eq!(r.as_deref(), Some("api"), "y copies the neighbour's id");
    keys(&mut s.h, "Y");
    keys(&mut s.h, "<CR>");
    assert_eq!(cursor(&mut s), Some(node("api")), "⏎ jumps to it");
    let (title, _, focused) = panel_at(&mut s);
    assert_eq!(
        (title.as_str(), focused),
        ("api", true),
        "the panel follows"
    );
    // S-Tab gives the keys back; ⏎ closes the panel.
    keys(&mut s.h, "<S-Tab>");
    assert!(!panel_at(&mut s).2);
    keys(&mut s.h, "<CR>");
    assert!(state(&mut s).borrow().panes[&s.win].panel.is_none());
    let errors = keys_errors(&mut s.h, "<Tab>");
    assert!(
        errors.iter().any(|e| e.contains("no detail panel")),
        "{errors:?}"
    );
}

#[test]
fn the_panels_pointer_lights_a_rows_mark_moves_its_cursor_folds_and_scrolls() {
    // A short window, so worker's rows overflow the panel.
    let mut s = graph_with("services.json", &fixture("services.json"), 200, 26);
    click_node(&mut s, "worker");
    keys(&mut s.h, "<CR>");
    let st = state(&mut s);
    let (rect, rows) = {
        let st = st.borrow();
        let p = &st.panes[&s.win];
        (
            p.panel_rect().unwrap(),
            p.panel.as_ref().unwrap().tree.rows.clone(),
        )
    };
    // A row's place on screen, by the panel's regions.
    let place = |s: &mut Setup, want: &dyn Fn(&Hit) -> bool| -> Option<Rect> {
        let st = state(s);
        let st = st.borrow();
        let p = &st.panes[&s.win];
        for y in 0..(rect.h as i32) {
            for x in [rect.x + 30.0, rect.x + 8.0, rect.x + 22.0] {
                let at = (x, rect.y + y as f32);
                if want(&p.hit(at)) {
                    return Some(Rect::new(at.0, at.1, 1.0, 1.0));
                }
            }
        }
        None
    };
    let api = rows
        .iter()
        .position(|r| matches!(&r.item, Some(Item::Node { id, .. }) if &**id == "api"));
    let on_api = api.and_then(|i| place(&mut s, &|h| *h == Hit::Detail(i as u32)));
    if let (Some(i), Some(r)) = (api, on_api) {
        mv(&mut s, (r.x, r.y));
        let st = st.borrow();
        let p = &st.panes[&s.win];
        assert!(matches!(&p.row_hover, Some(Item::Node { id, .. }) if &**id == "api"));
        assert_eq!(
            p.graph_overlay(st.graph.as_ref().unwrap()).emphasis,
            Some(node("api")),
            "the row's node comes forward"
        );
        drop(st);
        click(&mut s, (r.x, r.y));
        assert_eq!(panel_at(&mut s).1, i, "a click moves the panel's cursor");
        assert!(panel_at(&mut s).2, "and gives it the keys");
        assert_eq!(cursor(&mut s), Some(node("worker")), "the figure's stays");
    }
    // The fold triangle on deploy: open, then shut.
    let deploy = rows.iter().position(|r| r.key == "deploy").expect("deploy");
    assert!(rows[deploy].folded);
    if let Some(r) = place(&mut s, &|h| *h == Hit::DetailFold(deploy as u32)) {
        click(&mut s, (r.x, r.y));
        let open = !st.borrow().panes[&s.win].panel.as_ref().unwrap().tree.rows[deploy].folded;
        assert!(open, "the triangle unfolds it");
    }
    // The wheel scrolls the panel, not the figure.
    let v = view(&mut s);
    let mid = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    for _ in 0..4 {
        scroll(&mut s, mid, (0.0, 2.0), Mods::default());
    }
    let top = st.borrow().panes[&s.win].panel.as_ref().unwrap().state.top;
    assert!(top > 0, "scrolled");
    assert_eq!(view(&mut s), v, "the figure didn't move");
    settle(&mut s);
    assert_eq!(undoable(&mut s), 0);
}

#[test]
fn the_panel_works_on_a_chart_too() {
    let mut s = setup();
    keys(&mut s.h, "gg7l<CR>");
    let st = state(&mut s);
    let (title, rows) = {
        let st = st.borrow();
        let p = st.panes[&s.win].panel.as_ref().expect("open on a chart");
        (p.tree.title.clone(), p.tree.rows.len())
    };
    assert!(!title.is_empty() && rows > 2, "{title}: {rows} rows");
    // The chart lays out in what's left.
    let plot = plot(&mut s);
    let (fw, _) = st.borrow().panes[&s.win].figure_size();
    assert!(plot.right() <= fw + 0.5, "{plot:?} within {fw}");
    // It follows the cursor.
    keys(&mut s.h, "l");
    let again = st.borrow().panes[&s.win]
        .panel
        .as_ref()
        .unwrap()
        .tree
        .title
        .clone();
    assert_ne!(again, title, "a new moment");
    keys(&mut s.h, "<CR>");
    assert!(st.borrow().panes[&s.win].panel.is_none());
}

#[test]
fn search_lights_matches_as_typed_esc_puts_the_cursor_back_and_n_cap_n_go_on() {
    let mut s = services();
    click_node(&mut s, "mobile");
    let v = view(&mut s);
    for log in s.h.type_keys("/a").unwrap() {
        assert!(log.errors.is_empty(), "{:?}", log.errors);
    }
    let st = state(&mut s);
    assert_eq!(cursor(&mut s), Some(node("api")), "the best match as typed");
    let lit = st.borrow().panes[&s.win].matches.len();
    assert!(lit >= 5, "the matches lit: {lit}");
    keys(&mut s.h, "<Esc>");
    assert_eq!(cursor(&mut s), Some(node("mobile")), "Esc puts it back");
    assert!(st.borrow().panes[&s.win].matches.is_empty());
    assert_eq!(st.borrow().panes[&s.win].found_node, None);
    settle(&mut s);
    assert_eq!(view(&mut s).x, v.x);
    // ⏎ keeps the find, brought forward until Esc.
    keys(&mut s.h, "/a<CR>");
    assert_eq!(cursor(&mut s), Some(node("api")));
    assert_eq!(st.borrow().panes[&s.win].found_node, Some(node("api")));
    keys(&mut s.h, "n");
    assert_eq!(cursor(&mut s), Some(node("auth")), "the next match");
    keys(&mut s.h, "N");
    assert_eq!(cursor(&mut s), Some(node("api")));
    keys(&mut s.h, "N");
    assert_ne!(cursor(&mut s), Some(node("api")), "round to the last");
    keys(&mut s.h, "<Esc>");
    assert_eq!(st.borrow().panes[&s.win].found_node, None);
}

#[test]
fn t_shows_a_graphs_node_and_edge_tables_and_a_charts_rows_at_the_cursor() {
    let mut s = services();
    click_node(&mut s, "worker");
    keys(&mut s.h, "t");
    let b =
        s.h.find_buffer("*fig table: services.json*")
            .expect("the tables");
    let text = s.h.text(b).unwrap().rope().to_string();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("id\tlabel\tgroup"), "{}", lines[0]);
    assert!(
        text.contains("\n\nfrom\tto\t"),
        "the edges after a blank line"
    );
    let w =
        s.h.window_list()
            .into_iter()
            .find(|w| s.h.window_buffer(*w) == Some(b))
            .unwrap();
    let t = s.h.text(b).unwrap();
    let line = t.line_of(t.cursor(w).unwrap());
    assert!(
        lines[line].starts_with("worker\t"),
        "on worker's row: {}",
        lines[line]
    );
    // A chart's: the cursor's row.
    let mut c = setup();
    keys(&mut c.h, "gg5l");
    keys(&mut c.h, "t");
    let b = c.h.find_buffer("*fig table: data.csv*").unwrap();
    let w =
        c.h.window_list()
            .into_iter()
            .find(|w| c.h.window_buffer(*w) == Some(b))
            .unwrap();
    let t = c.h.text(b).unwrap();
    let line = t.line_of(t.cursor(w).unwrap());
    let text = t.rope().to_string();
    assert!(
        text.lines().nth(line).is_some_and(|l| l.starts_with("5\t")),
        "on x = 5's row: line {line}"
    );
}

#[test]
fn x_o_and_cap_x_act_on_the_cursors_group() {
    let mut s = services();
    click_node(&mut s, "db");
    keys(&mut s.h, "x");
    let data = die_fig::data::SeriesKey::new("data", 0);
    assert!(view(&mut s).hidden.contains(&data));
    assert_eq!(cursor(&mut s), None, "the cursor goes with it");
    keys(&mut s.h, "X");
    assert!(view(&mut s).hidden.is_empty());
    settle(&mut s);
    click_node(&mut s, "api");
    keys(&mut s.h, "o");
    assert_eq!(
        view(&mut s).only,
        Some(die_fig::data::SeriesKey::new("platform", 0))
    );
    keys(&mut s.h, "u");
    assert_eq!(view(&mut s).only, None);
}

#[test]
fn s_turns_the_labels_and_the_layout_and_relayout_lets_the_pins_go() {
    let mut s = services();
    let from = node_at(&mut s, "cdn");
    gesture(
        &mut s,
        &[
            from,
            (from.0 + 10.0, from.1),
            (from.0 + 30.0, from.1 + 10.0),
        ],
    );
    assert_eq!(view(&mut s).pinned.len(), 1);
    keys(&mut s.h, "s");
    let c = s.h.prompt_candidates();
    assert!(c.iter().any(|c| c.starts_with("layout: force")), "{c:?}");
    keys(&mut s.h, "relayout<CR>");
    settle(&mut s);
    assert!(view(&mut s).pinned.is_empty());
    keys(&mut s.h, "s");
    keys(&mut s.h, "labels<CR>");
    let st = state(&mut s);
    assert_eq!(st.borrow().toggles.labels, Some(false));
    keys(&mut s.h, "s");
    keys(&mut s.h, "layout: layered<CR>");
    let layout = st.borrow().graph.as_ref().unwrap().layout.clone();
    assert_eq!(layout, die_fig::spec::Layout::Layered);
    keys(&mut s.h, "u");
    assert_eq!(st.borrow().toggles.layout, None, "undone");
}

#[test]
fn the_focusing_press_only_focuses_and_a_second_button_mid_drag_does_nothing() {
    let mut s = services();
    s.h.send(Input::Focus(false));
    s.h.send(Input::Focus(true));
    click_node(&mut s, "db");
    assert_eq!(cursor(&mut s), None, "the press that focused only focused");
    click_node(&mut s, "db");
    assert_eq!(cursor(&mut s), Some(node("db")));
    // A right press mid-drag ends nothing.
    let from = node_at(&mut s, "cdn");
    pointer(
        &mut s,
        PointerKind::Press(Button::Left),
        from,
        Mods::default(),
        1,
    );
    pointer(
        &mut s,
        PointerKind::Drag(Button::Left),
        (from.0 + 20.0, from.1),
        Mods::default(),
        0,
    );
    pointer(
        &mut s,
        PointerKind::Press(Button::Right),
        (from.0 + 20.0, from.1),
        Mods::default(),
        1,
    );
    pointer(
        &mut s,
        PointerKind::Release(Button::Right),
        (from.0 + 20.0, from.1),
        Mods::default(),
        1,
    );
    pointer(
        &mut s,
        PointerKind::Drag(Button::Left),
        (from.0 + 40.0, from.1),
        Mods::default(),
        0,
    );
    pointer(
        &mut s,
        PointerKind::Release(Button::Left),
        (from.0 + 40.0, from.1),
        Mods::default(),
        1,
    );
    let now = node_at(&mut s, "cdn");
    assert!((now.0 - from.0 - 40.0).abs() < 1.5, "{from:?} → {now:?}");
}

#[test]
fn following_the_source_keeps_the_pins_and_the_cursor_of_nodes_that_survive() {
    let mut s = services();
    let from = node_at(&mut s, "cdn");
    gesture(
        &mut s,
        &[
            from,
            (from.0 + 10.0, from.1),
            (from.0 + 30.0, from.1 + 10.0),
        ],
    );
    click_node(&mut s, "db");
    let pins = view(&mut s).pinned.clone();
    // Edit the source: a field changes; every id survives.
    let text = fixture("services.json").replace("\"port\": 8443", "\"port\": 9443");
    let len = s.h.text(s.src).unwrap().len();
    s.h.text_mut(s.src)
        .unwrap()
        .tx(die_core::Origin::User)
        .replace(0..len, &text)
        .commit()
        .unwrap();
    s.h.advance(Duration::from_millis(400));
    s.h.run_until_idle();
    assert_eq!(view(&mut s).pinned, pins);
    assert_eq!(cursor(&mut s), Some(node("db")));
}

fn up(state_dir: &TempDir) -> Harness {
    let exts: Vec<Box<dyn die_core::Extension>> = vec![
        Box::new(Vim::default()),
        Box::new(Fig::with_zone(TimeZone::UTC)),
    ];
    let config = Config {
        session: Some(state_dir.path().to_path_buf()),
        ..Config::default()
    };
    let mut h = Harness::with_config(exts, config).unwrap();
    h.resume_session();
    h.set_window_size(200, 50);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    h
}

#[test]
fn a_restart_keeps_the_pins_the_layout_the_labels_and_the_open_panel() {
    let state_dir = TempDir::new("graph-session-state");
    let dir = TempDir::new("graph-session-data");
    let path = dir.path().join("services.json");
    std::fs::write(&path, fixture("services.json")).unwrap();
    let mut h = up(&state_dir);
    let src = h.open_file(&path).unwrap();
    h.show(src, ShowIn::Current).unwrap();
    h.run_until_idle();
    h.run("fig-toggle", &Args::default()).unwrap();
    h.run_until_idle();
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    // Past the grace of the focus the start gave, so the first press isn't only focusing.
    h.advance(Duration::from_millis(200));
    let mut s = Setup { h, src, buf, win };
    let from = node_at(&mut s, "cdn");
    gesture(
        &mut s,
        &[
            from,
            (from.0 + 10.0, from.1),
            (from.0 + 30.0, from.1 + 10.0),
        ],
    );
    keys(&mut s.h, "s");
    keys(&mut s.h, "labels<CR>");
    click_node(&mut s, "worker");
    keys(&mut s.h, "<CR>");
    let pins = view(&mut s).pinned.clone();
    assert_eq!(pins.len(), 1);
    let mut h = s.h;
    h.hand_over_session().unwrap();
    drop(h);
    let h = up(&state_dir);
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    let mut s = Setup {
        h,
        src: buf,
        buf,
        win,
    };
    assert_eq!(s.h.buffer_name(buf), Some("services.json *fig*"));
    let st = state(&mut s);
    assert!(
        st.borrow().graph.is_some(),
        "built: {:?}",
        st.borrow().error
    );
    assert_eq!(view(&mut s).pinned, pins, "the pins");
    assert_eq!(st.borrow().toggles.labels, Some(false), "the labels off");
    assert_eq!(cursor(&mut s), Some(node("worker")));
    let panel = st.borrow().panes[&s.win]
        .panel
        .as_ref()
        .map(|p| p.tree.title.clone());
    assert_eq!(panel.as_deref(), Some("worker"), "the panel open again");
}
