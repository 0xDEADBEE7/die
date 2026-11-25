//! The surface, through die-core's harness and its pixel fake: `\V` on a `.fig` file's buffer
//! shows its figure and `\V` there goes back; the pointer over a mark shows its card and the
//! status line says it; `Tab` and `h`/`l` move the focus and the cursor; a changed spec is read
//! again within a second; `:mini-fig PATH` works from anywhere, and `\V` from die-fig's figure
//! of a file. With die-vim, whose commands the figure's window keys run. Skipped without the
//! fonts.

use die_test_cap as _;

use die_core::{
    Args, BufferId, CellPixels, Config, Harness, Input, Mods, PointerEvent, PointerKind, Registry,
    ShowIn, State, WindowId,
};
use die_draw::Book;
use die_mini_fig::MiniFig;
use die_mini_fig::hit::Shape;
use die_mini_fig::view::View;
use die_vim::Vim;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 10 × 20 physical pixels a cell on a 2x display: 5 × 10 logical px.
const CELL: CellPixels = CellPixels {
    width: 10,
    height: 20,
    scale: 2.0,
};

/// A bar chart of inline rows beside a line chart of a CSV.
fn spec(title: &str) -> String {
    let bars = r#"{"type": "bar", "title": "Sold", "x": {"field": "k", "type": "category"},
        "y": "n", "data": {"values": [{"k": "a", "n": 3}, {"k": "b", "n": 5}, {"k": "c", "n": 2}]}}"#;
    let line = r#"{"type": "line", "title": "Price", "x": "t", "y": "p", "data": "rates"}"#;
    format!(
        r#"{{"type": "dashboard", "title": "{title}", "data": {{"rates": "rates.csv"}},
            "rows": [{{"height": 1, "panels": [{bars}, {line}]}}]}}"#
    )
}

/// A folder of a test's own, removed when it drops.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("mini-fig-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir.canonicalize().unwrap())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Setup {
    h: Harness,
    dir: TempDir,
    /// The spec's text buffer, and the window.
    src: BufferId,
    win: WindowId,
    /// The figure's buffer.
    buf: BufferId,
}

/// A harness with mini-fig, `\V` bound as die-dev binds it, a 100 × 30 window, the cells known.
fn harness() -> Option<Harness> {
    if let Err(why) = Book::load(None) {
        eprintln!("skipped: no fonts: {why}");
        return None;
    }
    let config = Config {
        register: Some(Box::new(|reg: &mut Registry| {
            reg.set_leader("\\");
            let keys = reg.global_keymap();
            keys.bind(State::Normal, "<leader>V", "mini-fig-toggle");
        })),
        ..Config::default()
    };
    let exts: Vec<Box<dyn die_core::Extension>> =
        vec![Box::new(Vim::default()), Box::new(MiniFig::default())];
    let mut h = Harness::with_config(exts, config).unwrap();
    h.set_window_size(100, 30);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    Some(h)
}

/// The spec written in a folder of its own, visited, and shown by `\V`.
fn setup(name: &str) -> Option<Setup> {
    let mut h = harness()?;
    let dir = TempDir::new(name);
    std::fs::write(dir.0.join("prices.fig"), spec("Prices")).unwrap();
    std::fs::write(dir.0.join("rates.csv"), "t,p\n1,10\n2,12\n3,11\n").unwrap();
    let (_, win) = h.open_text("scratch", "").unwrap();
    let src = h.open_file(&dir.0.join("prices.fig")).unwrap();
    h.show(src, ShowIn::Current).unwrap();
    keys(&mut h, "<leader>V");
    let buf = h.window_buffer(win).unwrap();
    assert_ne!(buf, src, "the figure shows in the window");
    Some(Setup {
        h,
        dir,
        src,
        win,
        buf,
    })
}

/// Keys typed, with no errors.
fn keys(h: &mut Harness, keys: &str) {
    for k in h.type_keys(keys).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
    h.run_until_idle();
}

/// Something of the figure's view.
fn view<R>(s: &mut Setup, f: impl FnOnce(&View) -> R) -> R {
    let buf = s.buf;
    s.h.with_surface(buf, |v: &mut View, _| f(v)).unwrap()
}

/// The figure's pixels as last presented.
fn pixels(s: &mut Setup) -> (u32, u32, Vec<u8>) {
    let id = view(s, View::pixels).expect("a pixel buffer");
    s.h.pixels(id).expect("presented")
}

/// The status line's row.
fn status(s: &mut Setup) -> String {
    let screen = s.h.screen();
    let line = screen.lines().find(|l| l.contains("mini-fig"));
    line.unwrap_or_default().to_string()
}

/// The pointer moved to a logical point of the figure's area.
fn point(s: &mut Setup, (lx, ly): (f32, f32)) {
    let p =
        s.h.placements()
            .into_iter()
            .find(|p| p.window == s.win)
            .unwrap();
    let g = s.h.surface_area(s.win).unwrap();
    let (r0, c0) = (p.row, p.col + g.text_x);
    let (x, y) = (lx * CELL.scale as f32, ly * CELL.scale as f32);
    let px = (
        x + (c0 as u32 * CELL.width) as f32,
        y + (r0 as u32 * CELL.height) as f32,
    );
    let errors = s.h.send(Input::Pointer(PointerEvent {
        kind: PointerKind::Move,
        row: r0 + (y / CELL.height as f32) as usize,
        col: c0 + (x / CELL.width as f32) as usize,
        clicks: 0,
        mods: Mods::default(),
        px: Some(px),
    }));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn leader_v_on_a_fig_file_shows_its_figure_and_again_goes_back() {
    let Some(mut s) = setup("toggle") else {
        return;
    };
    let (frames, hits) = view(&mut s, |v| (v.drawn.frames.len(), v.drawn.hits.clone()));
    assert_eq!(frames, 2);
    for panel in 0..2 {
        assert!(hits.iter().any(|h| h.key.0 == panel), "panel {panel} drew");
    }
    let (w, h, px) = pixels(&mut s);
    assert_eq!((w, h), (1000, 600), "the area's cells in pixels");
    assert!(px.chunks(4).all(|p| p[3] == 255), "drawn all over");
    assert!(
        status(&mut s).contains("mini-fig: Prices"),
        "{}",
        status(&mut s)
    );
    keys(&mut s.h, "<leader>V");
    assert_eq!(s.h.window_buffer(s.win), Some(s.src));
    assert!(!s.h.is_live(s.buf), "the figure goes");
}

#[test]
fn the_pointer_over_a_mark_shows_its_card_and_off_it_takes_it_away() {
    let Some(mut s) = setup("pointer") else {
        return;
    };
    let hit = view(&mut s, |v| v.drawn.hits[1].clone());
    let Shape::Rect(r) = hit.shape else {
        panic!("a bar is a rectangle");
    };
    let before = pixels(&mut s).2;
    point(&mut s, (r.x + r.w / 2.0, r.y + r.h / 2.0));
    assert_eq!(view(&mut s, |v| v.on), Some(hit.key));
    assert!(
        status(&mut s).contains("Prices · k b, n 5"),
        "{}",
        status(&mut s)
    );
    assert_ne!(pixels(&mut s).2, before, "the card is drawn");
    point(&mut s, (2.0, 2.0));
    assert_eq!(view(&mut s, |v| v.on), None);
    assert_eq!(pixels(&mut s).2, before, "and taken away");
}

#[test]
fn tab_moves_the_focus_and_h_l_the_cursor_over_the_focused_panel() {
    let Some(mut s) = setup("keys") else {
        return;
    };
    keys(&mut s.h, "<Tab>");
    assert_eq!(view(&mut s, |v| (v.focus, v.on)), (Some(0), None));
    assert!(
        status(&mut s).contains("Prices · Sold"),
        "{}",
        status(&mut s)
    );
    keys(&mut s.h, "l");
    assert_eq!(view(&mut s, |v| v.on), Some((0, 0, 0)));
    keys(&mut s.h, "ll");
    assert_eq!(view(&mut s, |v| v.on), Some((0, 0, 2)));
    keys(&mut s.h, "l");
    assert_eq!(
        view(&mut s, |v| v.on),
        Some((0, 0, 2)),
        "it stays at the end"
    );
    keys(&mut s.h, "h");
    assert_eq!(view(&mut s, |v| v.on), Some((0, 0, 1)));
    assert!(status(&mut s).contains("k b, n 5"), "{}", status(&mut s));
    keys(&mut s.h, "<Tab>h");
    assert_eq!(
        view(&mut s, |v| (v.focus, v.on)),
        (Some(1), Some((1, 0, 2)))
    );
    keys(&mut s.h, "<S-Tab>");
    assert_eq!(view(&mut s, |v| (v.focus, v.on)), (Some(0), None));
    keys(&mut s.h, "l<Esc>");
    assert_eq!(view(&mut s, |v| (v.focus, v.on)), (Some(0), None));
    keys(&mut s.h, "<Esc>");
    assert_eq!(view(&mut s, |v| v.focus), None);
}

#[test]
fn a_changed_spec_or_data_file_is_read_again_within_a_second() {
    let Some(mut s) = setup("reload") else {
        return;
    };
    let later = SystemTime::now() + Duration::from_secs(10);
    let touch = |p: &Path, text: &str| {
        std::fs::write(p, text).unwrap();
        let f = std::fs::File::options().write(true).open(p).unwrap();
        f.set_modified(later).unwrap();
    };
    touch(&s.dir.0.join("prices.fig"), &spec("Costs"));
    s.h.advance(Duration::from_millis(1100));
    assert!(
        status(&mut s).contains("mini-fig: Costs"),
        "{}",
        status(&mut s)
    );
    touch(&s.dir.0.join("rates.csv"), "t,p\n1,10\n2,12\n3,11\n4,9\n");
    s.h.advance(Duration::from_millis(1100));
    let points = view(&mut s, |v| {
        v.drawn.hits.iter().filter(|h| h.key.0 == 1).count()
    });
    assert_eq!(points, 4, "the new row is drawn");
}

#[test]
fn mini_fig_path_works_from_anywhere_and_leader_v_from_die_figs_figure_of_a_file() {
    let Some(mut h) = harness() else {
        return;
    };
    let dir = TempDir::new("anywhere");
    std::fs::write(dir.0.join("prices.fig"), spec("Prices")).unwrap();
    std::fs::write(dir.0.join("rates.csv"), "t,p\n1,10\n2,12\n").unwrap();
    std::fs::write(dir.0.join("bad.fig"), "{").unwrap();
    let (scratch, win) = h.open_text("scratch", "").unwrap();
    h.set_directory(scratch, &dir.0);
    let args = |text: &str| Args {
        text: Some(text.into()),
        ..Args::default()
    };
    h.run("mini-fig", &args("prices.fig")).unwrap();
    h.run_until_idle();
    let fig = h.window_buffer(win).unwrap();
    assert!(
        h.with_surface(fig, |v: &mut View, _| v.from == Some(scratch))
            .unwrap()
    );
    h.run("mini-fig", &args("bad.fig")).unwrap();
    h.run_until_idle();
    assert!(h.screen().contains("mini-fig: not JSON"), "{}", h.screen());
    // die-fig names its figure of a file after the file, in the file's folder.
    let theirs = h.create_text("prices.fig *fig*", "");
    h.set_directory(theirs, &dir.0);
    h.show(theirs, ShowIn::Current).unwrap();
    keys(&mut h, "<leader>V");
    let ours = h.window_buffer(win).unwrap();
    let from = h.with_surface(ours, |v: &mut View, _| (v.from, v.path.clone()));
    assert_eq!(from.unwrap(), (Some(theirs), dir.0.join("prices.fig")));
    assert_eq!(ours, fig, "the figure already open");
    keys(&mut h, "<leader>V");
    assert_eq!(h.window_buffer(win), Some(theirs));
}
