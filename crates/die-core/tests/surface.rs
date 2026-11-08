//! What core offers a terminal's surface (contract §7 and §10, scope D18): styled cells over the
//! window's whole area, the surface's cursor and status segment, the events it hears, and
//! passthrough as insert state with the DIE layer kept for DIE.

use die_test_cap as _;

use die_core::*;
use std::cell::{Cell as StdCell, RefCell};
use std::rc::Rc;

#[derive(Default)]
struct Seen {
    events: Vec<Event>,
    /// Each render's frame: whether it was new, and its size.
    renders: Vec<(bool, usize, usize)>,
    infos: Vec<RenderInfo>,
}

/// A surface that puts its cells when its frame is new, and logs what it hears.
struct Grid {
    seen: Rc<RefCell<Seen>>,
    cells: Vec<(usize, usize, FrameCell)>,
    cursor: Option<SurfaceCursor>,
    status: Option<String>,
    takes_esc: Rc<StdCell<bool>>,
}

impl Surface for Grid {
    fn kind(&self) -> &'static str {
        "grid"
    }
    fn render(&mut self, frame: &mut Frame, info: &RenderInfo) {
        let mut seen = self.seen.borrow_mut();
        seen.renders
            .push((frame.is_new(), frame.cols(), frame.rows()));
        seen.infos.push(*info);
        if frame.is_new() {
            for (r, c, cell) in &self.cells {
                frame.put(*r, *c, cell.clone());
            }
        }
    }
    fn cursor(&self, _: &RenderInfo) -> Option<SurfaceCursor> {
        self.cursor
    }
    fn status(&self) -> Option<String> {
        self.status.clone()
    }
    fn event(&mut self, ev: &Event, _: &mut Ctx) -> bool {
        self.seen.borrow_mut().events.push(ev.clone());
        true
    }
    fn takes_esc(&self) -> bool {
        self.takes_esc.get()
    }
    fn initial_state(&self) -> State {
        State::Passthrough
    }
}

type Log = Rc<RefCell<Vec<&'static str>>>;

/// The DIE layer as the facade binds it, a key under the leader, and a terminal's normal state.
struct Toy(Log);

impl Extension for Toy {
    fn name(&self) -> &'static str {
        "toy"
    }
    fn register(&mut self, reg: &mut Registry) {
        for name in ["quit-ish", "leader-f", "send-esc"] {
            let log = self.0.clone();
            reg.command(name, "Log its name.", move |_, _| {
                log.borrow_mut().push(name);
                Ok(())
            });
        }
        reg.command("to-insert", "Back to insert.", |ctx, a| {
            ctx.set_state_of(a.buffer.unwrap(), State::Passthrough);
            Ok(())
        });
        let g = reg.global_keymap();
        g.bind(State::Passthrough, "<D-q>", "quit-ish")
            .bind(State::Passthrough, "<D-v>", "surface-paste")
            .bind(State::Normal, "<leader>f", "leader-f")
            .bind(State::Insert, "<C-c>", "quit-ish");
        reg.kind_keymap("grid")
            .bind(State::Normal, "<Esc>", "send-esc")
            .bind(State::Normal, "i", "to-insert")
            .bind(State::Normal, "I", "to-insert")
            .bind(State::Normal, "p", "surface-paste");
    }
}

struct Setup {
    h: Harness,
    log: Log,
    seen: Rc<RefCell<Seen>>,
    takes_esc: Rc<StdCell<bool>>,
    buf: BufferId,
    win: WindowId,
}

/// A harness whose text buffers have margins, a width and line numbers, with a text window on
/// the left and the surface on the right, focused.
fn setup(cells: Vec<(usize, usize, FrameCell)>) -> Setup {
    let log: Log = Rc::default();
    let config = Config {
        text_defaults: Settings {
            max_width: Some(20),
            center: true,
            line_numbers: LineNumbers::Absolute,
            ..Settings::default()
        },
        ..Config::default()
    };
    let mut h = Harness::with_config(vec![Box::new(Toy(log.clone()))], config).unwrap();
    h.open_text("t", "text\n").unwrap();
    let (seen, takes_esc) = (Rc::<RefCell<Seen>>::default(), Rc::new(StdCell::new(false)));
    let grid = Grid {
        seen: seen.clone(),
        cells,
        cursor: None,
        status: None,
        takes_esc: takes_esc.clone(),
    };
    let buf = h.create_surface("grid", grid);
    let win = h.show(buf, ShowIn::Split(Side::Right)).unwrap().unwrap();
    h.send(Input::Resize { cols: 80, rows: 26 });
    Setup {
        h,
        log,
        seen,
        takes_esc,
        buf,
        win,
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(Rgb(r, g, b))
}

fn cell(c: char) -> FrameCell {
    FrameCell::new(Glyph::Char(c), Color::Default, Color::Default)
}

fn origin(h: &mut Harness, w: WindowId) -> (usize, usize) {
    let p = h.placements().into_iter().find(|p| p.window == w).unwrap();
    (p.row, p.col)
}

fn events(s: &Setup) -> Vec<Event> {
    std::mem::take(&mut s.seen.borrow_mut().events)
}

fn keys(s: &Setup) -> Vec<KeyEvent> {
    let evs = events(s).into_iter();
    evs.filter_map(|e| match e {
        Event::Key(k) => Some(k),
        _ => None,
    })
    .collect()
}

fn press(key: Key, mods: Mods) -> KeyEvent {
    KeyEvent::press(key).mods(mods)
}

const CMD: Mods = Mods {
    ctrl: false,
    alt: false,
    shift: false,
    super_: true,
};

#[test]
fn cells_resolve_over_the_whole_area_as_the_contract_says() {
    let white_on_black = |c| FrameCell::new(Glyph::Char(c), rgb(255, 255, 255), rgb(0, 0, 0));
    let mut b = FrameCell::new(Glyph::Char('b'), rgb(200, 0, 0), rgb(0, 0, 200));
    (b.bold, b.italic) = (true, true);
    let mut c = cell('c');
    c.reverse = true;
    let mut d = white_on_black('d');
    d.faint = true;
    let mut e = white_on_black('e');
    (e.faint, e.reverse) = (true, true);
    let mut f = cell('f');
    (f.invisible, f.underline, f.strike) = (true, Underline::Curly, true);
    let mut g = cell('g');
    g.blink = true;
    let mut u = FrameCell::new(Glyph::Char('u'), rgb(1, 2, 3), Color::Default);
    u.underline = Underline::Double;
    let mut v = cell('v');
    (v.underline, v.underline_color) = (Underline::Dashed, rgb(0, 255, 0));
    let wide = FrameCell::new(Glyph::Char('中'), rgb(9, 9, 9), Color::Default);
    let rest = FrameCell::new(Glyph::Continuation, rgb(9, 9, 9), Color::Default);
    let row: Vec<FrameCell> = vec![cell('a'), b, c, d, e, f, g, u, v, wide, rest];
    let mut cells: Vec<_> = row
        .into_iter()
        .enumerate()
        .map(|(i, c)| (0, i, c))
        .collect();
    cells.push((23, 39, cell('z')));
    let mut done = Cell::new(Glyph::Char('q'), Rgb(1, 1, 1), Rgb(2, 2, 2));
    (done.italic, done.underline) = (true, Underline::Dotted);
    cells.push((1, 0, FrameCell::from(done.clone())));
    let mut s = setup(cells);
    let (r0, c0) = origin(&mut s.h, s.win);
    let scene = s.h.scene();
    let at = |col: usize| scene.row(r0)[c0 + col].clone();
    let blank = scene.row(r0 + 2)[c0].clone();
    let (fg, bg) = (blank.fg, blank.bg);
    assert_eq!(
        (at(0).glyph, at(0).fg, at(0).bg),
        (Glyph::Char('a'), fg, bg),
        "the default face's"
    );
    assert_eq!((at(1).fg, at(1).bg), (Rgb(200, 0, 0), Rgb(0, 0, 200)));
    assert!(at(1).bold && at(1).italic && !at(1).strike);
    assert_eq!(
        (at(2).fg, at(2).bg),
        (bg, fg),
        "reverse swaps the defaults too"
    );
    assert_eq!(
        (at(3).fg, at(3).bg),
        (Rgb(165, 165, 165), Rgb(0, 0, 0)),
        "65 % of the fg, 35 % of the bg"
    );
    assert_eq!(
        (at(4).fg, at(4).bg),
        (Rgb(0, 0, 0), Rgb(165, 165, 165)),
        "faint, then the swap"
    );
    assert_eq!(at(5).glyph, Glyph::Char(' '), "invisible draws a space");
    assert_eq!(
        (at(5).underline, at(5).strike),
        (Underline::None, false),
        "with no line"
    );
    assert_eq!(
        at(6),
        Cell::new(Glyph::Char('g'), fg, bg),
        "blinking draws steadily"
    );
    assert_eq!(
        (at(7).underline, at(7).underline_color),
        (Underline::Double, Rgb(1, 2, 3))
    );
    assert_eq!(
        (at(8).underline, at(8).underline_color),
        (Underline::Dashed, Rgb(0, 255, 0))
    );
    assert_eq!(
        (at(9).glyph.clone(), at(10).glyph),
        (Glyph::Char('中'), Glyph::Continuation)
    );
    assert_eq!(
        scene.row(r0 + 23)[c0 + 39].glyph,
        Glyph::Char('z'),
        "its last cell is its own"
    );
    assert_eq!(
        scene.row(r0 + 1)[c0],
        done,
        "a resolved cell, as die-term's engine gives one, draws as it is"
    );
    assert_eq!(
        scene.position(r0, c0),
        None,
        "a surface's cells map to no text"
    );
    assert_eq!(c0, 40, "no margin before it, though text buffers have one");
    let text_row = scene.row_text(r0);
    assert!(
        text_row.ends_with("│abcd ") || text_row.contains("│abcde"),
        "{text_row}"
    );
}

#[test]
fn the_frame_keeps_what_it_had_until_its_size_changes() {
    let mut s = setup(vec![(0, 0, cell('k'))]);
    s.h.scene();
    s.h.scene();
    let (r0, c0) = origin(&mut s.h, s.win);
    assert_eq!(
        s.h.scene().row(r0)[c0].glyph,
        Glyph::Char('k'),
        "kept, not rewritten"
    );
    s.h.send(Input::Resize { cols: 60, rows: 20 });
    s.h.scene();
    let renders = std::mem::take(&mut s.seen.borrow_mut().renders);
    let firsts: Vec<_> = renders.iter().map(|r| r.0).collect();
    assert!(firsts[0], "new at first");
    assert!(
        !firsts[1..firsts.len() - 2].iter().any(|n| *n),
        "{renders:?}"
    );
    let last = renders.last().unwrap();
    assert_eq!(*last, (false, 30, 18), "{renders:?}");
    assert!(renders.contains(&(true, 30, 18)), "new at the new size");
    let w = s.win;
    s.h.focus(w);
    let infos = std::mem::take(&mut s.seen.borrow_mut().infos);
    let info = infos.last().unwrap();
    assert_eq!((info.window, info.state), (w, State::Passthrough));
}

#[test]
fn after_a_render_panics_the_next_good_one_writes_every_row() {
    // As a terminal renders: every row when the frame is new, and otherwise only those that
    // changed, which here is none.
    struct Rows(Rc<StdCell<bool>>, Rc<RefCell<Vec<bool>>>);
    impl Surface for Rows {
        fn kind(&self) -> &'static str {
            "rows"
        }
        fn render(&mut self, frame: &mut Frame, _: &RenderInfo) {
            self.1.borrow_mut().push(frame.is_new());
            if self.0.get() {
                panic!("render bug");
            }
            let new = frame.is_new();
            for r in (0..frame.rows()).filter(|_| new) {
                frame.text(r, 0, &format!("row {r}"), &FrameCell::blank());
            }
        }
    }
    let (bug, news) = (Rc::new(StdCell::new(false)), Rc::default());
    let mut h = Harness::new(vec![]).unwrap();
    h.open_text("t", "").unwrap();
    let buf = h.create_surface("rows", Rows(bug.clone(), Rc::clone(&news)));
    let w = h.show(buf, ShowIn::Current).unwrap().unwrap();
    let (r0, c0) = origin(&mut h, w);
    let rows = |h: &mut Harness| {
        let scene = h.scene();
        let row = |r| scene.row_text(r0 + r).chars().skip(c0).collect::<String>();
        (row(0), row(1))
    };
    rows(&mut h);
    bug.set(true);
    assert!(rows(&mut h).0.contains("render panicked"));
    bug.set(false);
    assert_eq!(rows(&mut h), ("row 0".into(), "row 1".into()));
    assert_eq!(rows(&mut h), ("row 0".into(), "row 1".into()), "and kept");
    assert_eq!(*news.borrow(), [true, false, true, false]);
}

#[test]
fn the_cursor_is_the_surfaces_and_hollow_without_the_keys() {
    let mut s = setup(vec![]);
    let (r0, c0) = origin(&mut s.h, s.win);
    let set = |h: &mut Harness, buf, c: Option<SurfaceCursor>| {
        h.with_surface(buf, |g: &mut Grid, _| g.cursor = c).unwrap();
    };
    let drawn = |h: &mut Harness| {
        let scene = h.scene();
        scene
            .cursors
            .iter()
            .map(|c| (c.row, c.col, c.shape))
            .collect::<Vec<_>>()
    };
    let text_cursor = |h: &mut Harness| drawn(h).into_iter().find(|c| c.1 < 40).unwrap();
    assert_eq!(
        drawn(&mut s.h).len(),
        1,
        "no cursor of its own: only the text's, hollow"
    );
    let bar = SurfaceCursor {
        shape: CursorShape::Bar,
        ..SurfaceCursor::at(1, 2)
    };
    set(&mut s.h, s.buf, Some(bar));
    assert_eq!(
        drawn(&mut s.h)[0],
        (r0 + 1, c0 + 2, CursorShape::Bar),
        "first, solid"
    );
    assert_eq!(text_cursor(&mut s.h).2, CursorShape::Hollow);
    let under = SurfaceCursor {
        shape: CursorShape::Underline,
        blinking: true,
        ..SurfaceCursor::at(3, 4)
    };
    set(&mut s.h, s.buf, Some(under));
    assert_eq!(
        drawn(&mut s.h)[0],
        (r0 + 3, c0 + 4, CursorShape::Underline),
        "steady"
    );
    s.h.send(Input::Focus(false));
    let lost = drawn(&mut s.h);
    assert!(
        lost.contains(&(r0 + 3, c0 + 4, CursorShape::Hollow)),
        "DIE lost the keyboard"
    );
    assert!(!s.seen.borrow().infos.last().unwrap().focused);
    s.h.send(Input::Focus(true));
    s.h.prompt(":", |_, _| Ok(()));
    let shapes = drawn(&mut s.h);
    assert_eq!(shapes[0].2, CursorShape::Bar, "the prompt's comes first");
    assert!(
        shapes.contains(&(r0 + 3, c0 + 4, CursorShape::Hollow)),
        "{shapes:?}"
    );
    s.h.type_keys("<Esc>").unwrap();
    let text = s.h.window_list()[0];
    s.h.focus(text);
    assert!(drawn(&mut s.h).contains(&(r0 + 3, c0 + 4, CursorShape::Hollow)));
    s.h.focus(s.win);
    let hidden = SurfaceCursor {
        visible: false,
        ..SurfaceCursor::at(0, 0)
    };
    set(&mut s.h, s.buf, Some(hidden));
    assert_eq!(drawn(&mut s.h).len(), 1, "a hidden cursor isn't drawn");
    let infos = std::mem::take(&mut s.seen.borrow_mut().infos);
    assert!(infos.last().unwrap().focused);
    assert_eq!(CursorShape::for_state(State::Passthrough), CursorShape::Bar);
    assert_eq!(CursorShape::for_state(State::Normal), CursorShape::Block);
}

#[test]
fn its_status_line_is_its_own_and_says_insert_in_passthrough() {
    let mut s = setup(vec![]);
    let (r0, _) = origin(&mut s.h, s.win);
    let status = |h: &mut Harness| h.scene().row_text(r0 + 24);
    let line = status(&mut s.h);
    assert!(
        line.contains("\u{2502} grid "),
        "with none of its own, the status text: {line:?}"
    );
    let title = "vim \u{2014} main.rs".to_string();
    s.h.with_surface(s.buf, |g: &mut Grid, _| g.status = Some(title))
        .unwrap();
    let line = status(&mut s.h);
    assert!(line.contains("\u{2502} vim \u{2014} main.rs  "), "{line:?}");
    assert!(
        !line.contains("grid"),
        "its own status stands in for the name: {line:?}"
    );
    assert!(line.ends_with("<I>"), "{line:?}");
    s.h.type_keys("<Esc>").unwrap();
    assert!(status(&mut s.h).ends_with("<N>"));
    assert_eq!(s.h.state(), State::Normal);
    let text = s.h.scene().row_text(r0 + 24);
    assert!(
        !text[..text.find('│').unwrap()].contains('<'),
        "text windows have no tag: {text}"
    );
}

#[test]
fn it_hears_its_size_in_cells_and_pixels() {
    let mut s = setup(vec![]);
    events(&s);
    let cell = CellPixels {
        width: 18,
        height: 36,
        scale: 2.0,
    };
    s.h.set_cell_pixels(cell);
    s.h.run_until_idle();
    let want = Event::Resize {
        window: s.win,
        cols: 40,
        rows: 24,
        cell,
    };
    assert_eq!(events(&s), [want]);
    assert_eq!(s.h.cell_pixels(), cell);
    assert_eq!(
        PtySize::new(40, 24, cell),
        PtySize {
            cols: 40,
            rows: 24,
            width: 720,
            height: 864
        },
        "as TIOCSWINSZ wants it"
    );
}

#[test]
fn the_pointer_reaches_it_in_its_own_cells_and_pixels() {
    let mut s = setup(vec![]);
    let (text, w) = (s.h.window_list()[0], s.win);
    s.h.focus(text);
    s.h.set_cell_pixels(CellPixels {
        width: 10,
        height: 20,
        scale: 2.0,
    });
    s.h.run_until_idle();
    events(&s);
    let (r0, c0) = origin(&mut s.h, w);
    let mods = Mods {
        shift: true,
        ..Mods::default()
    };
    let at = |kind, row, col, px| {
        Input::Pointer(PointerEvent {
            kind,
            row,
            col,
            clicks: 2,
            mods,
            px,
        })
    };
    let px = |row: usize, col: usize| Some((col as f32 * 10.0 + 3.0, row as f32 * 20.0 + 5.0));
    let press = PointerKind::Press(Button::Left);
    s.h.send(at(press, r0 + 2, c0 + 3, px(r0 + 2, c0 + 3)));
    assert_eq!(s.h.focused(), Some(w), "a press focuses its window");
    let got = events(&s);
    let ptr = |e: &Event| match e {
        Event::Pointer(p) => *p,
        other => panic!("{other:?}"),
    };
    assert_eq!(got[1], Event::Focus(true), "after the press, queued");
    let p = ptr(&got[0]);
    assert_eq!((p.kind, p.window, p.row, p.col), (press, w, 2, 3));
    assert_eq!((p.x, p.y, p.clicks, p.mods), (33.0, 45.0, 2, mods));
    // A drag out of the area stays with it, held inside.
    let drag = PointerKind::Drag(Button::Left);
    s.h.send(at(drag, r0 + 30, 2, px(r0 + 30, 2)));
    let p = ptr(&events(&s)[0]);
    assert_eq!((p.kind, p.row, p.col, p.x), (drag, 23, 0, 0.0));
    assert_eq!(p.y, 24.0 * 20.0 - 1.0);
    let release = PointerKind::Release(Button::Left);
    s.h.send(at(release, r0, 2, None));
    assert_eq!(ptr(&events(&s)[0]).kind, release);
    // Motion and precise scrolls over it, and nothing over the text.
    let scroll = PointerKind::Scroll { dx: 0.25, dy: -1.5 };
    s.h.send(at(PointerKind::Move, r0 + 1, c0 + 1, None));
    s.h.send(at(scroll, r0 + 1, c0 + 1, None));
    let got: Vec<_> = events(&s).iter().map(|e| (ptr(e).kind, ptr(e).x)).collect();
    assert_eq!(
        got,
        [(PointerKind::Move, 10.0), (scroll, 10.0)],
        "the cell's corner"
    );
    s.h.send(at(press, r0, 1, None));
    s.h.send(at(release, r0, 1, None));
    assert_eq!(
        events(&s),
        [Event::Focus(false)],
        "the text window took that"
    );
    assert_eq!(s.h.focused(), Some(text));
    // A drag from a press on the text, and its release, are the text's over the area too.
    s.h.send(at(press, r0, 1, None));
    s.h.send(at(drag, r0 + 1, c0 + 2, None));
    s.h.send(at(release, r0 + 1, c0 + 3, None));
    assert!(events(&s).is_empty(), "it never had the press");
    s.h.prompt(":", |_, _| Ok(()));
    s.h.send(at(press, r0 + 2, c0 + 3, None));
    assert!(events(&s).is_empty(), "the prompt has the keys");
}

#[test]
fn a_paste_comes_as_one_event() {
    let mut s = setup(vec![]);
    events(&s);
    s.h.fake_clipboard().copy_from_outside("one\ntwo");
    s.h.send(Input::Key(press(Key::Char('v'), CMD)));
    assert_eq!(events(&s), [Event::Paste("one\ntwo".into())], "Cmd-V");
    s.h.set_register(
        'a',
        Register {
            kind: RegKind::Line,
            text: "ls\n".into(),
        },
    );
    let args = Args {
        register: Some('a'),
        buffer: Some(s.buf),
        ..Args::default()
    };
    s.h.run("surface-paste", &args).unwrap();
    assert_eq!(
        events(&s),
        [Event::Paste("ls\n".into())],
        "a register, as `p` would"
    );
    let thrice = Args {
        count: Some(3),
        ..args.clone()
    };
    s.h.run("surface-paste", &thrice).unwrap();
    assert_eq!(
        events(&s),
        [Event::Paste("ls\nls\nls\n".into())],
        "`3p`, in one piece"
    );
    let e = (s.h.run(
        "surface-paste",
        &Args {
            count: Some(usize::MAX),
            ..args
        },
    ))
    .unwrap_err();
    assert!(e.to_string().contains("64 MiB"), "{e}");
    let t = s.h.find_buffer("t").unwrap();
    let on_text = Args {
        buffer: Some(t),
        ..Args::default()
    };
    let e = s.h.run("surface-paste", &on_text).unwrap_err();
    assert!(e.to_string().contains("no surface"), "{e}");
}

#[test]
fn in_passthrough_every_key_but_the_die_layers_reaches_it_with_repeats_and_releases() {
    let mut s = setup(vec![]);
    events(&s);
    let none = Mods::default();
    let ctrl = Mods { ctrl: true, ..none };
    let shift = Mods {
        shift: true,
        ..none
    };
    let a = press(Key::Char('a'), none);
    let typed = [
        a.clone(),
        a.clone().kind(KeyKind::Repeat),
        a.clone().kind(KeyKind::Release),
        press(Key::Char('\\'), none),
        press(Key::Char('c'), ctrl),
        press(Key::Char('z'), ctrl),
        press(Key::Char('g'), ctrl),
        press(Key::Modifier(ModifierKey::Shift), shift),
        press(Key::Modifier(ModifierKey::Shift), none).kind(KeyKind::Release),
        press(Key::Char('x'), CMD),
        press(Key::Esc, ctrl),
        press(Key::Esc, shift),
    ];
    for k in &typed {
        s.h.send(Input::Key(k.clone()));
    }
    assert_eq!(
        keys(&s),
        typed,
        "the leader, C-c and an unbound Cmd chord among them"
    );
    assert!(
        s.log.borrow().is_empty(),
        "nothing bound outside passthrough ran"
    );
    // The DIE layer's bindings are DIE's, and swallow their repeat and release.
    let q = press(Key::Char('q'), CMD);
    for k in [
        q.clone(),
        q.clone().kind(KeyKind::Repeat),
        q.kind(KeyKind::Release),
    ] {
        s.h.send(Input::Key(k));
    }
    assert!(keys(&s).is_empty());
    assert_eq!(*s.log.borrow(), ["quit-ish"]);
    // Esc goes to normal, unless the surface takes it now; Cmd-Esc goes whatever it says.
    s.takes_esc.set(true);
    let esc = press(Key::Esc, none);
    s.h.send(Input::Key(esc.clone()));
    s.h.send(Input::Key(esc.clone().kind(KeyKind::Release)));
    assert_eq!(keys(&s), [esc.clone(), esc.clone().kind(KeyKind::Release)]);
    assert_eq!(s.h.state(), State::Passthrough);
    s.h.send(Input::Key(press(Key::Esc, CMD)));
    assert_eq!(s.h.state(), State::Normal, "Cmd-Esc leaves, and isn't sent");
    s.h.type_keys("i").unwrap();
    s.takes_esc.set(false);
    s.h.send(Input::Key(esc.clone()));
    s.h.send(Input::Key(esc.clone().kind(KeyKind::Repeat)));
    s.h.send(Input::Key(esc.clone().kind(KeyKind::Release)));
    assert_eq!(s.h.state(), State::Normal, "a plain Esc leaves");
    assert!(keys(&s).is_empty(), "with its repeat and release");
}

#[test]
fn a_release_or_repeat_reaches_it_only_after_its_press_did() {
    let mut s = setup(vec![]);
    events(&s);
    // As the OS gives them, with the physical key: a release's character is what the modifiers
    // held then make it, so Shift let go before I comes up as i.
    let code = |k: KeyEvent, c: &str| KeyEvent {
        code: Some(c.into()),
        ..k
    };
    let none = Mods::default();
    let shift = Mods {
        shift: true,
        ..none
    };
    let a = code(press(Key::Char('a'), none), "KeyA");
    let sh = code(press(Key::Modifier(ModifierKey::Shift), shift), "ShiftLeft");
    let big_i = code(press(Key::Char('I'), shift), "KeyI");
    let esc = code(press(Key::Esc, none), "Escape");
    for k in [
        a.clone(),
        a.clone().kind(KeyKind::Release),
        esc.clone(),
        esc.clone().kind(KeyKind::Release),
        sh.clone(),
        big_i.clone(),
        big_i.clone().kind(KeyKind::Repeat),
        sh.clone().mods(none).kind(KeyKind::Release),
        code(press(Key::Char('i'), none), "KeyI").kind(KeyKind::Release),
        a.clone(),
        a.clone().kind(KeyKind::Repeat),
        a.clone().kind(KeyKind::Release),
    ] {
        s.h.send(Input::Key(k));
    }
    assert_eq!(s.h.state(), State::Passthrough, "I went back to insert");
    assert_eq!(
        keys(&s),
        [
            a.clone(),
            a.clone().kind(KeyKind::Release),
            a.clone(),
            a.clone().kind(KeyKind::Repeat),
            a.kind(KeyKind::Release)
        ],
        "not I's repeat or release, nor Shift's, whose presses normal state took"
    );
}

#[test]
fn a_prompt_puts_back_its_own_buffers_state_and_no_other() {
    let mut s = setup(vec![]);
    let t = s.h.find_buffer("t").unwrap();
    // Focus moves while it's open: the answer is the surface's, not the text's.
    s.h.prompt("Paste it? (y or n) ", |_, _| Ok(()));
    assert_eq!(s.h.state_of(s.buf), State::CommandLine);
    let left = s.h.window_list()[0];
    s.h.focus(left);
    s.h.type_keys("y<CR>").unwrap();
    assert_eq!(s.h.state_of(s.buf), State::Passthrough);
    assert_eq!(s.h.state_of(t), State::Normal);
    // The program ends under the prompt, and its window falls back to the text.
    s.h.focus(s.win);
    s.h.prompt("Paste it? (y or n) ", |_, _| Ok(()));
    s.h.kill(s.buf);
    s.h.type_keys("y<CR>").unwrap();
    assert_eq!(s.h.prompt_text(), None);
    assert_eq!(
        s.h.state_of(t),
        State::Normal,
        "a text buffer isn't left in passthrough, where keys go nowhere"
    );
}

#[test]
fn in_normal_state_its_kinds_keymap_is_looked_up_as_usual() {
    let mut s = setup(vec![]);
    s.h.type_keys("<Esc>").unwrap();
    events(&s);
    s.h.type_keys("<Esc>").unwrap();
    assert_eq!(
        *s.log.borrow(),
        ["send-esc"],
        "as vterm-send-escape is bound"
    );
    s.h.type_keys("\\f").unwrap();
    assert_eq!(
        *s.log.borrow(),
        ["send-esc", "leader-f"],
        "the global keymap's keys, under the leader too"
    );
    s.h.type_keys("j").unwrap();
    assert_eq!(
        keys(&s),
        [KeyEvent::press(Key::Char('j'))],
        "a key no keymap took"
    );
    s.h.fake_clipboard().copy_from_outside("echo");
    s.h.type_keys("p").unwrap();
    assert_eq!(events(&s), [Event::Paste("echo".into())]);
    let i = KeyEvent::press(Key::Char('i'));
    s.h.send(Input::Key(i.clone()));
    s.h.send(Input::Key(i.clone().kind(KeyKind::Repeat)));
    s.h.send(Input::Key(i.kind(KeyKind::Release)));
    assert_eq!(s.h.state(), State::Passthrough);
    assert!(events(&s).is_empty(), "the program never had the i's press");
    let d =
        s.h.describe_key(&[KeyEvent::press(Key::Char('j'))])
            .unwrap();
    assert!(
        d.to_string()
            .starts_with("j in Insert goes to the grid surface"),
        "{d}"
    );
}
