//! What core offers a terminal's surface (contract §7 and §10, scope D18): styled cells over the
//! window's area, which a maximum width narrows and centers as it does a file's text, the
//! surface's cursor and status segment, the events it hears, and passthrough as insert state with
//! the DIE layer kept for DIE.

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

/// A surface that puts its cells when its frame is new, and logs what it hears. With text, it
/// has copy state.
struct Grid {
    seen: Rc<RefCell<Seen>>,
    cells: Vec<(usize, usize, FrameCell)>,
    cursor: Option<SurfaceCursor>,
    status: Option<String>,
    takes_esc: Rc<StdCell<bool>>,
    /// As die-term in copy state: it declares every key and takes none.
    refuses_keys: Rc<StdCell<bool>>,
    text: Option<String>,
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
        !(self.refuses_keys.get() && matches!(ev, Event::Key(_)))
    }
    fn handles(&self, _: &KeyEvent) -> bool {
        self.refuses_keys.get()
    }
    fn takes_esc(&self) -> bool {
        self.takes_esc.get()
    }
    fn initial_state(&self) -> State {
        State::Passthrough
    }
    fn text(&mut self, _: bool) -> Option<SurfaceText> {
        self.text.clone().map(SurfaceText::new)
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
        for name in ["quit-ish", "leader-f", "send-esc", "take-x"] {
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
            .bind(State::Normal, "<leader>z", "toggle-max-width")
            .bind(State::Insert, "<C-c>", "quit-ish");
        reg.kind_keymap("grid")
            .bind(State::Normal, "<Esc>", "send-esc")
            .bind(State::Normal, "i", "to-insert")
            .bind(State::Normal, "I", "to-insert")
            .bind(State::Normal, "x", "take-x")
            .bind(State::Normal, "p", "surface-paste");
    }
}

struct Setup {
    h: Harness,
    log: Log,
    seen: Rc<RefCell<Seen>>,
    takes_esc: Rc<StdCell<bool>>,
    refuses_keys: Rc<StdCell<bool>>,
    buf: BufferId,
    win: WindowId,
}

/// A harness whose text buffers have line numbers but no maximum width, with a text window on
/// the left and the surface on the right, focused.
fn setup(cells: Vec<(usize, usize, FrameCell)>) -> Setup {
    let numbers = Settings {
        line_numbers: LineNumbers::Absolute,
        ..Settings::default()
    };
    setup_with(cells, None, numbers, (ShowIn::Split(Side::Right), 80))
}

/// [`setup`] with these text defaults, the surface giving `text`, shown `how` in a frame `cols`
/// wide.
fn setup_with(
    cells: Vec<(usize, usize, FrameCell)>,
    text: Option<&str>,
    text_defaults: Settings,
    (how, cols): (ShowIn, usize),
) -> Setup {
    let log: Log = Rc::default();
    let config = Config {
        text_defaults,
        ..Config::default()
    };
    let mut h = Harness::with_config(vec![Box::new(Toy(log.clone()))], config).unwrap();
    h.open_text("t", "text\n").unwrap();
    let (seen, takes_esc) = (Rc::<RefCell<Seen>>::default(), Rc::new(StdCell::new(false)));
    let refuses_keys = Rc::new(StdCell::new(false));
    let grid = Grid {
        seen: seen.clone(),
        cells,
        cursor: None,
        status: None,
        takes_esc: takes_esc.clone(),
        refuses_keys: refuses_keys.clone(),
        text: text.map(String::from),
    };
    let buf = h.create_surface("grid", grid);
    let win = h.show(buf, how).unwrap().unwrap();
    h.send(Input::Resize { cols, rows: 26 });
    Setup {
        h,
        log,
        seen,
        takes_esc,
        refuses_keys,
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
    assert_eq!(c0, 40, "no gutter before it, though text buffers have one");
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
    // The first button down holds the pointer until it comes up: a right click mid-drag is
    // passed on, and the drag goes on to the left button's release, out of the area too.
    let (rpress, rrelease) = (
        PointerKind::Press(Button::Right),
        PointerKind::Release(Button::Right),
    );
    s.h.send(at(press, r0 + 2, c0 + 3, None));
    s.h.send(at(drag, r0 + 3, c0 + 3, None));
    s.h.send(at(rpress, r0 + 3, c0 + 3, None));
    s.h.send(at(rrelease, r0 + 3, c0 + 3, None));
    s.h.send(at(drag, r0 + 30, 2, None));
    s.h.send(at(release, r0 + 30, 2, None));
    let kinds: Vec<_> = events(&s).iter().map(|e| ptr(e).kind).collect();
    assert_eq!(kinds, [press, drag, rpress, rrelease, drag, release]);
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

#[test]
fn in_normal_state_a_release_goes_to_the_surface_whose_event_took_the_press() {
    let mut s = setup(vec![]);
    s.h.type_keys("<Esc>").unwrap();
    events(&s);
    // `j` is no keymap's: the surface takes its press, so its repeat and release reach it too,
    // as they do in passthrough.
    let j = KeyEvent::press(Key::Char('j'));
    for k in [
        j.clone(),
        j.clone().kind(KeyKind::Repeat),
        j.clone().kind(KeyKind::Release),
    ] {
        s.h.send(Input::Key(k));
    }
    assert_eq!(
        keys(&s),
        [
            j.clone(),
            j.clone().kind(KeyKind::Repeat),
            j.clone().kind(KeyKind::Release)
        ]
    );
    // `x` is the kind keymap's: the command runs, and the release has no surface to go to.
    let x = KeyEvent::press(Key::Char('x'));
    s.h.send(Input::Key(x.clone()));
    s.h.send(Input::Key(x.kind(KeyKind::Release)));
    assert_eq!(*s.log.borrow(), ["take-x"]);
    assert!(keys(&s).is_empty(), "a key the keymap took has no owner");
    // The focus moves between the press and the release: the release still goes to the
    // surface that had the press, not to the text window that has the keys now.
    s.h.send(Input::Key(j.clone()));
    let left = s.h.window_list()[0];
    s.h.focus(left);
    s.h.send(Input::Key(j.clone().kind(KeyKind::Release)));
    assert_eq!(keys(&s), [j.clone(), j.kind(KeyKind::Release)]);
    assert_eq!(s.h.state_of(s.buf), State::Normal, "and nothing else moved");
    // A key the surface declares (`handles`) but doesn't take went nowhere, so its release goes
    // nowhere either: die-term declares every key for passthrough and takes none in copy state,
    // and a program never sees a release without its press.
    s.h.focus(s.win);
    s.refuses_keys.set(true);
    events(&s);
    let f12 = KeyEvent::press(Key::F(12));
    let errors = s.h.send(Input::Key(f12.clone()));
    assert_eq!(errors, ["<F12> is undefined"]);
    s.h.send(Input::Key(f12.clone().kind(KeyKind::Release)));
    assert_eq!(keys(&s), [f12], "the press was offered, the release not");
}

/// Your config's text defaults: 80 columns, centered, with line numbers a surface doesn't get.
fn centered() -> Settings {
    Settings {
        max_width: Some(80),
        center: true,
        line_numbers: LineNumbers::Absolute,
        ..Settings::default()
    }
}

/// The areas it was told of, by window, since the last look.
fn resizes(s: &Setup) -> Vec<(WindowId, usize, usize)> {
    let evs = events(s).into_iter();
    evs.filter_map(|e| match e {
        Event::Resize {
            window, cols, rows, ..
        } => Some((window, cols, rows)),
        _ => None,
    })
    .collect()
}

const BG: Rgb = Rgb(0x10, 0x10, 0x10);
const SELECTION: Rgb = Rgb(0x40, 0x40, 0x40);

#[test]
fn a_max_width_gives_it_a_files_centered_area_and_toggles_with_it() {
    let cells = vec![(0, 0, cell('a')), (0, 79, cell('z')), (23, 79, cell('y'))];
    let mut s = setup_with(cells, None, centered(), (ShowIn::Current, 120));
    let w = s.win;
    assert_eq!(s.h.window_size(w), Some((120, 24)));
    let g = s.h.surface_area(w).unwrap();
    assert_eq!(
        (g.pad, g.gutter, g.text_x, g.text_w),
        (20, 0, 20, 80),
        "no gutter"
    );
    assert_eq!(resizes(&s).last(), Some(&(w, 80, 24)), "it hears its area");
    assert_eq!(
        s.h.settings(s.buf).unwrap().max_width,
        Some(80),
        "the defaults'"
    );
    let scene = s.h.scene();
    let glyph = |r: usize, c: usize| scene.row(r)[c].glyph.clone();
    assert_eq!(
        (glyph(0, 20), glyph(0, 99), glyph(23, 99)),
        (Glyph::Char('a'), Glyph::Char('z'), Glyph::Char('y'))
    );
    let margins = (0..20).chain(100..120);
    for c in margins {
        let m = &scene.row(5)[c];
        assert_eq!(
            (&m.glyph, m.bg),
            (&Glyph::Char(' '), BG),
            "blank, the default face"
        );
    }
    let status = scene.row(24);
    assert_eq!(
        (status[0].bg, status[119].bg),
        (Rgb(0x28, 0x28, 0x28), Rgb(0x28, 0x28, 0x28))
    );
    assert!(
        scene.row_text(24).starts_with(" grid"),
        "the status line spans the window"
    );
    let frame = |s: &Setup| s.seen.borrow().renders.last().map(|r| (r.1, r.2));
    assert_eq!(frame(&s), Some((80, 24)), "its frame is the area");
    s.h.with_surface(s.buf, |g: &mut Grid, _| {
        g.cursor = Some(SurfaceCursor::at(1, 2))
    })
    .unwrap();
    let cursor = s.h.scene().cursors[0];
    assert_eq!(
        (cursor.row, cursor.col),
        (1, 22),
        "its cursor is in the area"
    );
    // In insert state, `toggle-max-width` widens it to the window, and back.
    s.h.run("toggle-max-width", &Args::default()).unwrap();
    s.h.run_until_idle();
    assert_eq!(resizes(&s), [(w, 120, 24)]);
    let scene = s.h.scene();
    assert_eq!(scene.row(0)[0].glyph, Glyph::Char('a'));
    assert_eq!(frame(&s), Some((120, 24)));
    assert_eq!(s.h.settings(s.buf).unwrap().max_width, None);
    s.h.run("toggle-max-width", &Args::default()).unwrap();
    s.h.run_until_idle();
    assert_eq!(resizes(&s), [(w, 80, 24)]);
    assert_eq!(s.h.scene().row(0)[20].glyph, Glyph::Char('a'));
    // Anything else that changes the width or centering does it too.
    s.h.change_settings(s.buf, |s| s.center = false).unwrap();
    s.h.run_until_idle();
    assert_eq!(resizes(&s), [(w, 80, 24)], "told again, at the same size");
    assert_eq!(s.h.scene().row(0)[0].glyph, Glyph::Char('a'), "at the left");
    s.h.change_settings(s.buf, |s| s.max_width = Some(100))
        .unwrap();
    s.h.run_until_idle();
    assert_eq!(resizes(&s), [(w, 100, 24)]);
}

#[test]
fn the_pointer_is_in_the_areas_cells_and_a_margin_isnt_the_programs() {
    let mut s = setup_with(vec![], None, centered(), (ShowIn::Current, 120));
    s.h.set_cell_pixels(CellPixels {
        width: 10,
        height: 20,
        scale: 2.0,
    });
    s.h.run_until_idle();
    events(&s);
    let at = |kind, row, col: usize| {
        Input::Pointer(PointerEvent {
            kind,
            row,
            col,
            clicks: 1,
            mods: Mods::default(),
            px: Some((col as f32 * 10.0 + 3.0, row as f32 * 20.0 + 5.0)),
        })
    };
    let pointers = |s: &Setup| {
        let evs = events(s).into_iter();
        let ps = evs.filter_map(|e| match e {
            Event::Pointer(p) => Some((p.kind, p.row, p.col, p.x, p.y)),
            _ => None,
        });
        ps.collect::<Vec<_>>()
    };
    let (press, release) = (
        PointerKind::Press(Button::Left),
        PointerKind::Release(Button::Left),
    );
    let drag = PointerKind::Drag(Button::Left);
    s.h.send(at(press, 2, 23));
    assert_eq!(
        pointers(&s),
        [(press, 2, 3, 33.0, 45.0)],
        "from the area's edge"
    );
    s.h.send(at(drag, 3, 5));
    s.h.send(at(drag, 3, 115));
    s.h.send(at(release, 3, 115));
    assert_eq!(
        pointers(&s),
        [
            (drag, 3, 0, 0.0, 65.0),
            (drag, 3, 79, 799.0, 65.0),
            (release, 3, 79, 799.0, 65.0)
        ],
        "drags held inside it"
    );
    for col in [5, 19, 100, 119] {
        s.h.send(at(press, 2, col));
        s.h.send(at(PointerKind::Move, 2, col));
        s.h.send(at(release, 2, col));
    }
    assert_eq!(
        pointers(&s),
        [],
        "a margin is no part of the program's grid"
    );
    let scroll = PointerKind::Scroll { dx: 0.0, dy: 1.0 };
    s.h.send(at(scroll, 2, 5));
    assert_eq!(
        pointers(&s),
        [(scroll, 2, 0, 0.0, 45.0)],
        "the wheel scrolls it from a margin, as a file's text"
    );
}

#[test]
fn copy_states_cursor_and_selection_are_drawn_on_its_cells_at_the_new_offset() {
    let long = "x".repeat(100);
    let text = format!("one\n{long}\ntwo\n");
    let mut s = setup_with(vec![], Some(&text), centered(), (ShowIn::Current, 120));
    let (w, buf) = (s.win, s.buf);
    s.h.type_keys("<Esc>").unwrap();
    assert_eq!(s.h.state(), State::Normal);
    let cursor = |h: &mut Harness| {
        let c = h.scene().cursors[0];
        (c.row, c.col)
    };
    assert_eq!(
        cursor(&mut s.h),
        (2, 20),
        "on the last line, at the area's edge"
    );
    let vp = s.h.text(buf).unwrap().viewport(w).unwrap();
    let firsts: Vec<_> = vp.rows.iter().map(|r| r.first_line).collect();
    assert_eq!(
        firsts,
        [0, 1, 2, 3],
        "a line wider than the area doesn't wrap"
    );
    // A click in a margin is a click in a file's margin: the row's start, or its end.
    let click = |h: &mut Harness, col| {
        for kind in [
            PointerKind::Press(Button::Left),
            PointerKind::Release(Button::Left),
        ] {
            let ptr = PointerEvent {
                kind,
                row: 0,
                col,
                clicks: 1,
                mods: Mods::default(),
                px: None,
            };
            h.send(Input::Pointer(ptr));
        }
        h.text(buf).unwrap().cursor(w)
    };
    assert_eq!(click(&mut s.h, 5), Some(0));
    assert_eq!(click(&mut s.h, 110), Some(3));
    assert_eq!(click(&mut s.h, 21), Some(1));
    assert!(
        events(&s).iter().all(|e| !matches!(e, Event::Pointer(_))),
        "the program hears none of it"
    );
    let sel = Selection {
        anchor: 0,
        head: 2,
        shape: Shape::Char,
        goal: None,
    };
    s.h.text_mut(buf).unwrap().set_selection(w, sel);
    s.h.set_state_of(buf, State::Visual);
    let selected = |h: &mut Harness| {
        let scene = h.scene();
        let row = scene.row(0);
        (0..120)
            .filter(|&c| row[c].bg == SELECTION)
            .collect::<Vec<_>>()
    };
    assert_eq!(selected(&mut s.h), [20, 21, 22]);
    assert_eq!(cursor(&mut s.h), (0, 22));
    events(&s);
    s.h.run("toggle-max-width", &Args::default()).unwrap();
    s.h.run_until_idle();
    assert_eq!(resizes(&s), [(w, 120, 24)], "the surface hears it");
    assert_eq!(selected(&mut s.h), [0, 1, 2], "at the new offset");
    assert_eq!(cursor(&mut s.h), (0, 2));
    s.h.set_state_of(buf, State::Normal);
    s.h.type_keys("\\z").unwrap();
    s.h.run_until_idle();
    assert_eq!(resizes(&s), [(w, 80, 24)], "`\\z` in normal state");
    assert_eq!(cursor(&mut s.h), (0, 22));
    assert_eq!(
        s.h.text(buf).unwrap().settings().max_width,
        Some(80),
        "the snapshot's too"
    );
}

#[test]
fn a_window_opened_on_it_tells_it_a_narrower_area() {
    let mut s = setup_with(vec![], None, centered(), (ShowIn::Current, 120));
    events(&s);
    let w = s.h.open_window(s.buf, 100, 10).unwrap();
    s.h.run_until_idle();
    assert_eq!(resizes(&s), [(w, 80, 10)]);
    let w = s.h.open_window(s.buf, 60, 10).unwrap();
    s.h.run_until_idle();
    assert_eq!(resizes(&s), [], "its caller gave the size: {w:?}");
}

#[test]
fn two_windows_on_it_each_get_their_own_area() {
    let mut s = setup_with(
        vec![(0, 0, cell('a'))],
        None,
        centered(),
        (ShowIn::Current, 172),
    );
    let a = s.win;
    let b = s.h.split_window(a, Side::Right).unwrap();
    let rows = s.h.window_size(b).unwrap().1;
    s.h.resize(b, 50, rows).unwrap();
    s.h.run_until_idle();
    let (ca, cb) = (s.h.window_size(a).unwrap().0, s.h.window_size(b).unwrap().0);
    assert_eq!((ca, cb), (121, 50));
    let got = resizes(&s);
    assert_eq!(got.iter().rfind(|r| r.0 == a), Some(&(a, 80, 24)));
    assert_eq!(
        got.iter().rfind(|r| r.0 == b),
        Some(&(b, 50, 24)),
        "narrower than the width"
    );
    let (pa, pb) = (s.h.surface_area(a).unwrap(), s.h.surface_area(b).unwrap());
    assert_eq!(
        (pa.text_x, pa.text_w, pb.text_x, pb.text_w),
        (20, 80, 0, 50)
    );
    let (_, ac) = origin(&mut s.h, a);
    let (_, bc) = origin(&mut s.h, b);
    let scene = s.h.scene();
    assert_eq!(scene.row(0)[ac + 20].glyph, Glyph::Char('a'));
    assert_eq!(scene.row(0)[bc].glyph, Glyph::Char('a'));
    assert_eq!(scene.row(0)[ac].glyph, Glyph::Char(' '));
}

/// A surface whose lines of output wrap at its width, as a terminal's do: its text is their
/// rows, saying which run on into the next, and its last Resize reflows them.
struct Flow {
    lines: Vec<String>,
    cols: usize,
}

impl Surface for Flow {
    fn kind(&self) -> &'static str {
        "grid"
    }
    fn render(&mut self, _: &mut Frame, _: &RenderInfo) {}
    fn event(&mut self, ev: &Event, _: &mut Ctx) -> bool {
        if let Event::Resize { cols, .. } = ev {
            self.cols = *cols;
        }
        true
    }
    fn initial_state(&self) -> State {
        State::Passthrough
    }
    fn text(&mut self, _: bool) -> Option<SurfaceText> {
        let mut st = SurfaceText::new(String::new());
        let mut row = 0;
        for l in &self.lines {
            let chars: Vec<char> = l.chars().collect();
            let rows: Vec<&[char]> = chars.chunks(self.cols).collect();
            for (i, r) in rows.iter().enumerate() {
                if i + 1 < rows.len() {
                    st.wrapped.push((row, self.cols));
                }
                st.text.extend(r.iter());
                st.text.push('\n');
                row += 1;
            }
        }
        st.live_top = row;
        Some(st)
    }
}

#[test]
fn a_reflow_keeps_each_window_in_its_line_of_output() {
    let config = Config {
        text_defaults: centered(),
        ..Config::default()
    };
    let mut h = Harness::with_config(vec![Box::new(Toy(Log::default()))], config).unwrap();
    h.open_text("t", "text\n").unwrap();
    let lines = (0..10).map(|i| format!("L{i}-{}", "abcdefghij".repeat(10)));
    let flow = Flow {
        lines: lines.collect(),
        cols: 120,
    };
    let buf = h.create_surface("grid", flow);
    let a = h.show(buf, ShowIn::Current).unwrap().unwrap();
    h.send(Input::Resize {
        cols: 120,
        rows: 26,
    });
    let b = h.split_window(a, Side::Below).unwrap();
    h.focus(a);
    h.type_keys("<Esc>").unwrap();
    assert_eq!(h.state(), State::Normal);
    // At 80 columns each 103-character line is two rows: L3's second is line 7.
    let at = |h: &mut Harness, line: usize, col: usize| h.text(buf).unwrap().line_start(line) + col;
    let place = |h: &mut Harness, w: WindowId| {
        let t = h.text(buf).unwrap();
        let (sel, top) = (t.selection(w).unwrap(), t.scroll(w).unwrap().0);
        let at = |p: usize| (t.line_of(p), t.display_col(p));
        (at(sel.anchor), at(sel.head), t.line_of(top))
    };
    let set = |h: &mut Harness, w: WindowId, anchor: usize, head: usize, top: usize| {
        let sel = Selection {
            anchor,
            head,
            shape: Shape::Char,
            goal: None,
        };
        let t = h.text_mut(buf).unwrap();
        t.set_selection(w, sel);
        let top = t.line_start(top);
        t.set_scroll(w, top, 0);
    };
    let (l3, l1) = (at(&mut h, 7, 10), at(&mut h, 2, 5));
    set(&mut h, a, l3, l3, 4);
    set(&mut h, b, l1, l1, 2);
    assert_eq!(place(&mut h, a), ((7, 10), (7, 10), 4));
    h.type_keys("\\z").unwrap();
    h.run_until_idle();
    assert_eq!(h.text(buf).unwrap().line_count(), 11, "a row a line now");
    assert_eq!(
        place(&mut h, a),
        ((3, 90), (3, 90), 2),
        "L3 at its column, under L2"
    );
    assert_eq!(place(&mut h, b), ((1, 5), (1, 5), 1), "L1");
    // And back, with a selection from L1's start.
    let (from, to) = (at(&mut h, 1, 0), at(&mut h, 3, 90));
    set(&mut h, a, from, to, 1);
    h.set_state_of(buf, State::Visual);
    h.run("toggle-max-width", &Args::default()).unwrap();
    h.run_until_idle();
    assert_eq!(h.text(buf).unwrap().line_count(), 21);
    assert_eq!(
        place(&mut h, a),
        ((2, 0), (7, 10), 2),
        "both ends kept, the top too"
    );
    assert_eq!(place(&mut h, b), ((2, 5), (2, 5), 2));
}
