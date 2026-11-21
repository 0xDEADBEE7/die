//! What a figure that can't draw shows in the editor (contract-p2b §1), and `.fig` files opening
//! as figures (§2): the symbolic error figure, `!` and a click turning it over to the log and
//! back, `Esc` closing the log, the log's keys and wheel, `⏎` and a double click going to an
//! entry's line and column (in the data file for a data file's error), the stale badge over a
//! kept figure on a bad edit and gone on a good one, `*messages*` told once; a `.fig` opening as
//! its figure, `\v` both ways, one that fails on opening, `open_fig_as = Text`, and the session
//! reopening each file as you last looked at it.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Args, BufferId, Config, Harness, Origin, ShowIn};
use die_fig::scene::Hit;
use die_fig::{ErrorKind, Fig, OpenAs, Options};
use die_vim::Vim;
use jiff::tz::TimeZone;
use std::path::Path;
use std::time::Duration;

/// Replace the source's text, as a user's edit, and let the quiet time pass.
fn edit(s: &mut Setup, text: &str) {
    let t = s.h.text_mut(s.src).unwrap();
    let end = t.len();
    t.tx(Origin::User).replace(0..end, text).commit().unwrap();
    s.h.run_until_idle();
    s.h.advance(Duration::from_millis(200));
    s.h.run_until_idle();
    s.h.scene();
}

/// A point of the window's area whose hit is `want`'s, found as the pointer finds it.
fn point_on(s: &mut Setup, want: impl Fn(&Hit) -> bool) -> Option<(f32, f32)> {
    let st = state(s);
    let st = st.borrow();
    let p = &st.panes[&s.win];
    let (w, h) = p.size();
    let mut y = 1.0;
    while y < h {
        let mut x = 1.0;
        while x < w {
            if want(&p.hit((x, y))) {
                return Some((x, y));
            }
            x += 4.0;
        }
        y += 4.0;
    }
    None
}

/// The window's log, if it's up: its first line shown and its marked entry.
fn log(s: &mut Setup) -> Option<(usize, usize)> {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].log
}

fn base_gen(s: &mut Setup) -> u64 {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].base_gen
}

const BROKEN: &str = "[{\"x\": 1, \"y\": 2},\n {\"x\" 2}]\n";
const GOOD: &str = "[{\"x\": 1, \"y\": 2},\n {\"x\": 2, \"y\": 4},\n {\"x\": 3, \"y\": 3}]\n";

#[test]
fn a_figure_that_never_parsed_shows_the_symbolic_figure_and_bang_or_a_click_turns_it_over() {
    let mut s = setup_with("data.json", BROKEN);
    let st = state(&mut s);
    assert!(st.borrow().model.is_none());
    let first = st
        .borrow()
        .error
        .as_ref()
        .expect("a failure")
        .first()
        .clone();
    assert_eq!(first.kind, ErrorKind::Malformed);
    assert_eq!(first.at.map(|a| a.line), Some(2));
    drop(st);
    // The whole area is the symbolic figure, and the status line says why.
    let (w, h) = state(&mut s).borrow().panes[&s.win].size();
    let middle = (w / 2.0, h / 2.0);
    assert_eq!(
        state(&mut s).borrow().panes[&s.win].hit(middle),
        Hit::ErrorFigure
    );
    assert!(
        status(&mut s).contains("Error · line 2"),
        "{}",
        status(&mut s)
    );
    // `!` turns it over to the log, and back; the base is drawn again each time.
    let drawn = base_gen(&mut s);
    keys(&mut s.h, "!");
    assert_eq!(log(&mut s), Some((0, 0)));
    assert!(base_gen(&mut s) > drawn, "the log drawn");
    assert!(point_on(&mut s, |h| matches!(h, Hit::LogEntry(0))).is_some());
    keys(&mut s.h, "!");
    assert_eq!(log(&mut s), None);
    // A click on the symbolic figure turns it over; one on the log's header turns it back.
    click(&mut s, middle);
    assert_eq!(log(&mut s), Some((0, 0)));
    click(&mut s, (w / 2.0, 20.0));
    assert_eq!(log(&mut s), None, "the header turns it back");
    // Esc closes the log.
    keys(&mut s.h, "!");
    keys(&mut s.h, "<Esc>");
    assert_eq!(log(&mut s), None);
    // `*messages*` heard it.
    let said =
        s.h.messages
            .iter()
            .filter(|m| m.contains("fig: data.json: line 2"))
            .count();
    assert_eq!(said, 1, "{:?}", s.h.messages);
    assert_eq!(s.h.next_timer(), None, "nothing runs at rest");
}

#[test]
fn enter_or_a_double_click_on_a_log_entry_goes_to_its_line_and_column() {
    let mut s = setup_with("data.json", BROKEN);
    // ⏎ on the symbolic figure: to the first error.
    keys(&mut s.h, "<CR>");
    assert_eq!(s.h.window_buffer(s.win), Some(s.src));
    let t = s.h.text(s.src).unwrap();
    let at = t.cursor(s.win).unwrap();
    assert_eq!(t.line_of(at), 1, "the second line");
    let col = at - t.line_start(1);
    assert_eq!(col, 6, "column 7: the 2 where a ':' should be");
    // Back to the figure; the log; a double click on its entry.
    s.h.show(s.buf, ShowIn::Current).unwrap();
    s.h.run_until_idle();
    keys(&mut s.h, "!");
    let on = point_on(&mut s, |h| matches!(h, Hit::LogEntry(0))).expect("an entry");
    // A double click: its second press goes, and the window shows the source before its
    // release, which is the text's.
    click(&mut s, on);
    let left = die_core::PointerKind::Press(die_core::Button::Left);
    pointer(&mut s, left, on, die_core::Mods::default(), 2);
    s.h.run_until_idle();
    assert_eq!(s.h.window_buffer(s.win), Some(s.src), "to the source");
    let t = s.h.text(s.src).unwrap();
    assert_eq!(t.line_of(t.cursor(s.win).unwrap()), 1);
}

/// A spec whose layers name eight columns its data hasn't: a long log.
fn many_missing() -> String {
    let layers: Vec<String> = (0..8)
        .map(|i| format!("{{\"mark\": \"line\", \"x\": \"x\", \"y\": \"gone{i}\"}}"))
        .collect();
    format!(
        "{{\"type\": \"chart\",\n \"data\": {{\"values\": [{{\"x\": 1, \"a\": 2}}, {{\"x\": 2, \"a\": 3}}]}},\n \"layers\": [\n  {}\n ]\n}}\n",
        layers.join(",\n  ")
    )
}

#[test]
fn the_log_moves_with_j_k_gg_g_ctrl_d_ctrl_u_and_the_wheel() {
    let mut s = setup_with("many.json", &many_missing());
    let st = state(&mut s);
    let n = st.borrow().error.as_ref().expect("failed").errors.len();
    assert_eq!(n, 8, "every missing column: {:?}", st.borrow().error);
    let lines = st.borrow().log_layout();
    drop(st);
    keys(&mut s.h, "!");
    keys(&mut s.h, "j");
    assert_eq!(log(&mut s).map(|l| l.1), Some(1), "j: the next entry");
    keys(&mut s.h, "3j");
    assert_eq!(log(&mut s).map(|l| l.1), Some(4), "a count");
    keys(&mut s.h, "k");
    assert_eq!(log(&mut s).map(|l| l.1), Some(3), "k: the one before");
    keys(&mut s.h, "G");
    let (top, mark) = log(&mut s).unwrap();
    assert_eq!(mark, 7, "G: the last");
    assert!(
        top > 0 && top + 1 > lines.starts[7] - 10,
        "scrolled to it: {top}"
    );
    keys(&mut s.h, "gg");
    assert_eq!(log(&mut s), Some((0, 0)), "gg: the first, at the top");
    keys(&mut s.h, "<C-d>");
    let (down, mark) = log(&mut s).unwrap();
    assert!(down > 0, "C-d: half a page down");
    assert!(
        lines.starts[mark] + lines.lens[mark] > down,
        "the mark kept on the page"
    );
    keys(&mut s.h, "<C-u>");
    assert_eq!(log(&mut s).map(|l| l.0), Some(0), "C-u: back up");
    // The wheel scrolls by lines; the pointer over an entry marks it.
    let (w, h) = state(&mut s).borrow().panes[&s.win].size();
    scroll(
        &mut s,
        (w / 2.0, h / 2.0),
        (0.0, 3.0),
        die_core::Mods::default(),
    );
    let (top, _) = log(&mut s).unwrap();
    assert!(top > 0, "the wheel scrolled: {top}");
    let on = point_on(&mut s, |h| matches!(h, Hit::LogEntry(i) if *i > 0)).expect("an entry");
    mv(&mut s, on);
    let Hit::LogEntry(i) = state(&mut s).borrow().panes[&s.win].hit(on) else {
        panic!("an entry");
    };
    assert_eq!(
        log(&mut s).map(|l| l.1),
        Some(i as usize),
        "the hovered entry marked"
    );
}

/// The log as it's drawn, with the figure's name (each entry's `-->` line says it), and its page.
fn drawn_log(s: &mut Setup) -> (die_fig::plot::LogLayout, usize) {
    let st = state(s);
    let st = st.borrow();
    let (_, lines) = die_fig::plot::errors::log_entries(&st.log_errors(), &st.text, &st.name);
    (lines, die_fig::plot::log_page(st.panes[&s.win].size()))
}

/// Whether entry `i` of the drawn log is on the page whole.
fn on_page(
    lines: &die_fig::plot::LogLayout,
    page: usize,
    (top, _): (usize, usize),
    i: usize,
) -> bool {
    let top = top.min(lines.max_top(page));
    lines.starts[i] >= top && lines.starts[i] + lines.lens[i] <= top + page
}

#[test]
fn the_log_s_keys_and_wheel_reach_every_entry_of_the_log_as_drawn() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/data/errors/many-errors.fig");
    let mut h = harness();
    let src = open(&mut h, &path);
    let mut s = setup_on(h, src);
    assert_ne!(s.buf, src, "the .fig opened as its figure");
    keys(&mut s.h, "!");
    let (lines, page) = drawn_log(&mut s);
    let n = lines.starts.len();
    assert!(
        n >= 8 && lines.total > page,
        "a long log: {n} entries, {} lines",
        lines.total
    );
    // The figure's own entries say its name, so the keys' layout is the drawn one.
    assert_eq!(state(&mut s).borrow().log_layout(), lines);
    // j to each entry brings it whole onto the page.
    for i in 1..n {
        keys(&mut s.h, "j");
        let at = log(&mut s).unwrap();
        assert_eq!(at.1, i);
        assert!(
            on_page(&lines, page, at, i),
            "j to entry {i}: {at:?}, {lines:?}"
        );
    }
    // G lights the last, on the page, where the pointer finds it.
    keys(&mut s.h, "gg");
    keys(&mut s.h, "G");
    let at = log(&mut s).unwrap();
    assert_eq!(at.1, n - 1);
    assert!(on_page(&lines, page, at, n - 1), "G: {at:?}");
    let last = (n - 1) as u32;
    assert!(
        point_on(&mut s, |h| *h == Hit::LogEntry(last)).is_some(),
        "the last entry under the pointer"
    );
    // The wheel goes as far as the drawn log's end, and no further.
    keys(&mut s.h, "gg");
    let (w, hh) = state(&mut s).borrow().panes[&s.win].size();
    for _ in 0..60 {
        scroll(
            &mut s,
            (w / 2.0, hh / 2.0),
            (0.0, 5.0),
            die_core::Mods::default(),
        );
    }
    assert_eq!(
        log(&mut s).unwrap().0,
        lines.max_top(page),
        "the wheel to the end"
    );
    // C-d as far as it goes: the end too.
    keys(&mut s.h, "gg");
    keys(&mut s.h, "20<C-d>");
    assert_eq!(
        log(&mut s).unwrap().0,
        lines.max_top(page),
        "C-d to the end"
    );
}

#[test]
fn a_bad_edit_keeps_the_last_figure_with_the_stale_badge_until_a_good_one() {
    let mut s = setup_with("data.json", GOOD);
    let st = state(&mut s);
    let built = st.borrow().model_gen;
    assert!(point_on(&mut s, |h| *h == Hit::Badge).is_none(), "no badge");
    edit(&mut s, BROKEN);
    assert_eq!(st.borrow().model_gen, built, "the last figure kept");
    assert_eq!(st.borrow().badge().as_deref(), Some("Error · line 2"));
    let on = point_on(&mut s, |h| *h == Hit::Badge).expect("the badge");
    let (w, _) = st.borrow().panes[&s.win].size();
    assert!(on.0 > w * 0.6 && on.1 < 40.0, "top right: {on:?}");
    // The same failure again is said once.
    edit(&mut s, &BROKEN.replace("2}]", "2} ]"));
    let said =
        s.h.messages
            .iter()
            .filter(|m| m.contains("fig: data.json: line 2"))
            .count();
    assert_eq!(said, 1, "{:?}", s.h.messages);
    // A click on the badge brings the log up over the figure; `!` takes it away.
    click(&mut s, on);
    assert_eq!(log(&mut s), Some((0, 0)));
    assert!(
        state(&mut s).borrow().panes[&s.win].layout.is_none(),
        "no chart under the log"
    );
    keys(&mut s.h, "!");
    assert_eq!(log(&mut s), None);
    assert!(
        state(&mut s).borrow().panes[&s.win].layout.is_some(),
        "the chart again"
    );
    // A good edit: the badge goes, the figure follows.
    edit(&mut s, GOOD.replace("4}", "9}").as_str());
    assert_eq!(st.borrow().badge(), None);
    assert!(st.borrow().model_gen > built);
    assert!(
        point_on(&mut s, |h| *h == Hit::Badge).is_none(),
        "the badge gone"
    );
}

/// `text` in `dir/name`.
fn write(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

/// A file opened in the window, as `:e` opens it, and the queue run.
fn open(h: &mut Harness, path: &Path) -> BufferId {
    let b = h.open_file(path).unwrap();
    h.show(b, ShowIn::Current).unwrap();
    h.run_until_idle();
    h.scene();
    b
}

fn setup_on(h: Harness, src: BufferId) -> Setup {
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    Setup { h, src, buf, win }
}

const LATENCY: &str = "time,service,ms\n2026-11-21T06:00,api,42\n2026-11-21T06:05,api,45\n2026-11-21T06:10,api,timeout\n";

#[test]
fn a_data_file_s_error_goes_to_its_cell_in_the_data_file() {
    let dir = TempDir::new("errors-cell");
    write(dir.path(), "latency.csv", LATENCY);
    let spec = write(
        dir.path(),
        "latency.fig",
        "{\"type\": \"line\", \"data\": \"latency.csv\", \"x\": \"time\",\n \"y\": {\"field\": \"ms\", \"type\": \"number\"}}\n",
    );
    let mut h = harness();
    let src = open(&mut h, &spec);
    let mut s = setup_on(h, src);
    assert_ne!(s.buf, s.src, "the .fig opened as its figure");
    let st = state(&mut s);
    let first = st.borrow().error.as_ref().expect("failed").first().clone();
    assert_eq!(first.kind, ErrorKind::Mismatch);
    assert_eq!(first.file_name().as_deref(), Some("latency.csv"));
    assert_eq!(first.at.map(|a| (a.line, a.col)), Some((4, 22)));
    assert!(
        first
            .context
            .iter()
            .any(|c| c.contains("the spec says \"ms\" is a number"))
    );
    drop(st);
    keys(&mut s.h, "<CR>");
    let data = s.h.window_buffer(s.win).unwrap();
    assert_eq!(s.h.buffer_name(data), Some("latency.csv"), "the data file");
    let t = s.h.text(data).unwrap();
    let at = t.cursor(s.win).unwrap();
    assert_eq!(
        (t.line_of(at), at - t.line_start(3)),
        (3, 21),
        "on \"timeout\""
    );
}

const RATES_FIG: &str =
    "{\"type\": \"line\", \"data\": \"rates.csv\", \"x\": \"x\", \"y\": \"a\"}\n";

#[test]
fn a_fig_file_opens_as_its_figure_and_leader_v_flips_to_its_text_and_back() {
    let dir = TempDir::new("errors-open");
    write(dir.path(), "rates.csv", &crossing());
    let spec = write(dir.path(), "rates.fig", RATES_FIG);
    let mut h = harness();
    let src = open(&mut h, &spec);
    let mut s = setup_on(h, src);
    assert_eq!(s.h.buffer_name(s.buf), Some("rates.fig *fig*"));
    assert!(state(&mut s).borrow().model.is_some(), "drawn");
    assert_eq!(run(&mut s.h, "fig-toggle"), Vec::<String>::new());
    assert_eq!(s.h.window_buffer(s.win), Some(src), "\\v: its text");
    // It stays text after the command's settle: it doesn't open as the figure again.
    keys(&mut s.h, "l");
    assert_eq!(s.h.window_buffer(s.win), Some(src));
    assert_eq!(run(&mut s.h, "fig-toggle"), Vec::<String>::new());
    let fig = s.h.window_buffer(s.win).unwrap();
    assert_eq!(s.h.buffer_name(fig), Some("rates.fig *fig*"), "and back");
    // A .fig.json does too; a CSV opens as text.
    let json = write(dir.path(), "rates.fig.json", RATES_FIG);
    let b = open(&mut s.h, &json);
    assert_ne!(s.h.window_buffer(s.win), Some(b));
    let csv = open(&mut s.h, &dir.path().join("rates.csv"));
    assert_eq!(s.h.window_buffer(s.win), Some(csv), "a CSV is text");
}

/// `:CMD` typed, run to its end.
fn ex(h: &mut Harness, cmd: &str) {
    keys(h, &format!(":{cmd}<CR>"));
    h.run_until_idle();
    h.scene();
}

#[test]
fn a_fig_file_opened_again_opens_as_you_last_looked_at_it_and_its_own_text_stays_text() {
    let dir = TempDir::new("errors-again");
    write(dir.path(), "rates.csv", &crossing());
    let spec = write(dir.path(), "rates.fig", RATES_FIG);
    let notes = write(dir.path(), "notes.txt", "hello\n");
    let mut h = harness();
    h.open_text("scratch", "").unwrap();
    h.run_until_idle();
    ex(&mut h, &format!("e {}", spec.display()));
    assert_eq!(shown_name(&mut h), "rates.fig *fig*", ":e: the figure");
    // Elsewhere and back: the figure again, as it was left.
    ex(&mut h, &format!("e {}", notes.display()));
    assert_eq!(shown_name(&mut h), "notes.txt");
    ex(&mut h, &format!("e {}", spec.display()));
    assert_eq!(
        shown_name(&mut h),
        "rates.fig *fig*",
        ":e again: the figure"
    );
    ex(&mut h, "b notes.txt");
    ex(&mut h, "b rates.fig");
    assert_eq!(shown_name(&mut h), "rates.fig *fig*", ":b: the figure");
    // From its figure to its text by name, or back along the jumps: its text, which stays.
    ex(&mut h, "b rates.fig");
    assert_eq!(
        shown_name(&mut h),
        "rates.fig",
        "its text, asked for from its figure"
    );
    keys(&mut h, "l");
    assert_eq!(shown_name(&mut h), "rates.fig", "and it stays so");
    // Left as text: `:e` brings it back as text.
    ex(&mut h, &format!("e {}", notes.display()));
    ex(&mut h, &format!("e {}", spec.display()));
    assert_eq!(
        shown_name(&mut h),
        "rates.fig",
        "last looked at as text: text"
    );
    // \v to the figure, and `C-^` to the text behind it: text, not the figure again.
    h.run("fig-toggle", &Args::default()).unwrap();
    h.run_until_idle();
    assert_eq!(shown_name(&mut h), "rates.fig *fig*");
    keys(&mut h, "<C-^>");
    h.run_until_idle();
    assert_ne!(
        shown_name(&mut h),
        "rates.fig *fig*",
        "C-^ leaves the figure"
    );
    keys(&mut h, "l");
    assert_ne!(shown_name(&mut h), "rates.fig *fig*", "and stays gone");
}

#[test]
fn a_fig_that_fails_on_opening_shows_the_symbolic_figure_and_leader_v_its_text() {
    let dir = TempDir::new("errors-fail");
    let spec = write(
        dir.path(),
        "gone.fig",
        "{\"type\": \"line\",\n \"data\": \"gone.csv\", \"x\": \"x\", \"y\": \"a\"}\n",
    );
    let mut h = harness();
    let src = open(&mut h, &spec);
    let mut s = setup_on(h, src);
    assert_ne!(s.buf, src, "the figure, though it can't draw");
    let st = state(&mut s);
    let first = st.borrow().error.as_ref().expect("failed").first().clone();
    assert_eq!(first.kind, ErrorKind::Unreadable);
    assert_eq!(
        first.at.map(|a| (a.line, a.col)),
        Some((2, 10)),
        "at the path in the spec"
    );
    drop(st);
    let (w, h) = state(&mut s).borrow().panes[&s.win].size();
    assert_eq!(
        state(&mut s).borrow().panes[&s.win].hit((w / 2.0, h / 2.0)),
        Hit::ErrorFigure
    );
    run(&mut s.h, "fig-toggle");
    assert_eq!(s.h.window_buffer(s.win), Some(src), "\\v: its text");
}

#[test]
fn open_fig_as_text_opens_a_fig_file_as_text() {
    let dir = TempDir::new("errors-text");
    write(dir.path(), "rates.csv", &crossing());
    let spec = write(dir.path(), "rates.fig", RATES_FIG);
    let exts: Vec<Box<dyn die_core::Extension>> = vec![
        Box::new(Vim::default()),
        Box::new(Fig::new(Options {
            open_fig_as: OpenAs::Text,
            ..Options::default()
        })),
    ];
    let mut h = Harness::new(exts).unwrap();
    h.set_window_size(100, 30);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    let src = open(&mut h, &spec);
    assert_eq!(h.window_buffer(h.focused().unwrap()), Some(src), "text");
    h.run("fig-toggle", &Args::default()).unwrap();
    h.run_until_idle();
    let fig = h.window_buffer(h.focused().unwrap()).unwrap();
    assert_eq!(
        h.buffer_name(fig),
        Some("rates.fig *fig*"),
        "\\v shows the figure"
    );
}

/// DIE with the session in `state`, as tests/session.rs starts it.
fn up(state: &TempDir) -> Harness {
    let exts: Vec<Box<dyn die_core::Extension>> = vec![
        Box::new(Vim::default()),
        Box::new(Fig::with_zone(TimeZone::UTC)),
    ];
    let config = Config {
        session: Some(state.path().to_path_buf()),
        ..Config::default()
    };
    let mut h = Harness::with_config(exts, config).unwrap();
    h.resume_session();
    h.set_window_size(100, 30);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    h.scene();
    h
}

fn restart(mut h: Harness, state: &TempDir) -> Harness {
    h.hand_over_session().unwrap();
    drop(h);
    up(state)
}

fn shown_name(h: &mut Harness) -> String {
    let b = h.window_buffer(h.focused().unwrap()).unwrap();
    h.buffer_name(b).unwrap_or_default().to_string()
}

#[test]
fn a_restart_reopens_each_fig_file_as_you_last_looked_at_it() {
    let state_dir = TempDir::new("errors-session");
    let dir = TempDir::new("errors-session-data");
    write(dir.path(), "rates.csv", &crossing());
    let spec = write(dir.path(), "rates.fig", RATES_FIG);
    let mut h = up(&state_dir);
    open(&mut h, &spec);
    assert_eq!(shown_name(&mut h), "rates.fig *fig*");
    // As text, then a restart: text.
    h.run("fig-toggle", &Args::default()).unwrap();
    h.run_until_idle();
    assert_eq!(shown_name(&mut h), "rates.fig");
    let mut h = restart(h, &state_dir);
    assert_eq!(
        shown_name(&mut h),
        "rates.fig",
        "left as text, back as text"
    );
    keys(&mut h, "l");
    assert_eq!(shown_name(&mut h), "rates.fig", "and it stays so");
    // As its figure, then a restart: the figure.
    h.run("fig-toggle", &Args::default()).unwrap();
    h.run_until_idle();
    assert_eq!(shown_name(&mut h), "rates.fig *fig*");
    let mut h = restart(h, &state_dir);
    assert_eq!(
        shown_name(&mut h),
        "rates.fig *fig*",
        "left as the figure, back so"
    );
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    let st = h.with_ext(|f: &mut Fig, _| f.figure(buf)).unwrap().unwrap();
    assert!(
        st.borrow().model.is_some(),
        "drawn: {:?}",
        st.borrow().error
    );
}

#[test]
fn a_restart_reopens_a_fig_left_as_text_however_its_text_was_reached() {
    let state_dir = TempDir::new("errors-session-b");
    let dir = TempDir::new("errors-session-b-data");
    write(dir.path(), "rates.csv", &crossing());
    let a = write(dir.path(), "a.fig", RATES_FIG);
    let b = write(dir.path(), "b.fig", RATES_FIG);
    let mut h = up(&state_dir);
    open(&mut h, &a);
    assert_eq!(shown_name(&mut h), "a.fig *fig*");
    // Its text by `:b` from its figure: text is how you left it.
    ex(&mut h, "b a.fig");
    assert_eq!(shown_name(&mut h), "a.fig");
    let mut h = restart(h, &state_dir);
    assert_eq!(
        shown_name(&mut h),
        "a.fig",
        "left as text by :b, back as text"
    );
    keys(&mut h, "l");
    assert_eq!(shown_name(&mut h), "a.fig", "and it stays so");
    // b.fig left as its figure; after a restart its text reached from elsewhere opens its
    // figure, as it did before the restart, and from its figure stays text.
    open(&mut h, &b);
    assert_eq!(shown_name(&mut h), "b.fig *fig*");
    let mut h = restart(h, &state_dir);
    assert_eq!(
        shown_name(&mut h),
        "b.fig *fig*",
        "left as the figure, back so"
    );
    ex(&mut h, "b a.fig");
    assert_eq!(shown_name(&mut h), "a.fig", "a.fig still text");
    ex(&mut h, "b b.fig");
    assert_eq!(
        shown_name(&mut h),
        "b.fig *fig*",
        "b.fig from elsewhere: its figure"
    );
    ex(&mut h, "b b.fig");
    assert_eq!(shown_name(&mut h), "b.fig", "from its figure: its text");
    let mut h = restart(h, &state_dir);
    assert_eq!(shown_name(&mut h), "b.fig", "left as text, back as text");
}

#[test]
fn a_fig_saved_under_its_name_or_new_and_empty_stays_text_and_a_jump_to_its_error_too() {
    let dir = TempDir::new("errors-stay");
    write(dir.path(), "rates.csv", &crossing());
    let mut h = harness();
    // Written in a buffer of its own and saved as a .fig: you're writing it, so it stays text.
    let (b, win) = h.open_text("draft", "").unwrap();
    let t = h.text_mut(b).unwrap();
    t.tx(Origin::User).insert(0, RATES_FIG).commit().unwrap();
    h.run_until_idle();
    let path = dir.path().join("draft.fig");
    h.save_to(b, &path, false, true).unwrap();
    h.run_until_idle();
    assert_eq!(
        h.window_buffer(win),
        Some(b),
        "saved under the name: still text"
    );
    // A new .fig with nothing in it yet: text, to write.
    let new = open(&mut h, &dir.path().join("new.fig"));
    assert_eq!(
        h.window_buffer(h.focused().unwrap()),
        Some(new),
        "new and empty: text"
    );
    // `:fig FILE` on a .fig no buffer visits, then ⏎ to its error: its text, and it stays so.
    let broken = write(
        dir.path(),
        "broken.fig",
        "{\"type\": \"line\",\n \"data\": \"rates.csv\", \"x\": \"x\", \"y\": \"nope\"}\n",
    );
    let args = Args {
        text: Some(broken.display().to_string()),
        ..Args::default()
    };
    h.run("fig", &args).unwrap();
    h.run_until_idle();
    let fig = h.window_buffer(h.focused().unwrap()).unwrap();
    assert_eq!(h.buffer_name(fig), Some("broken.fig *fig*"));
    keys(&mut h, "<CR>");
    h.run_until_idle();
    let shown = h.window_buffer(h.focused().unwrap()).unwrap();
    assert_eq!(
        h.buffer_name(shown),
        Some("broken.fig"),
        "its text, at the error"
    );
    keys(&mut h, "l");
    assert_eq!(
        h.window_buffer(h.focused().unwrap()),
        Some(shown),
        "and it stays so"
    );
}

#[test]
fn data_with_only_its_header_draws_nothing_to_draw_until_a_row_comes() {
    let mut s = setup_with("data.csv", "x,a\n");
    let st = state(&mut s);
    assert!(st.borrow().model.is_none());
    let first = st.borrow().error.as_ref().expect("why").first().clone();
    assert_eq!(first.kind, ErrorKind::Empty);
    assert_eq!(st.borrow().badge().as_deref(), Some("Empty"));
    drop(st);
    let (w, h) = state(&mut s).borrow().panes[&s.win].size();
    assert_eq!(
        state(&mut s).borrow().panes[&s.win].hit((w / 2.0, h / 2.0)),
        Hit::ErrorFigure
    );
    edit(&mut s, "x,a\n1,2\n2,3\n");
    let st = state(&mut s);
    assert!(st.borrow().model.is_some(), "{:?}", st.borrow().error);
    assert_eq!(st.borrow().badge(), None);
}
