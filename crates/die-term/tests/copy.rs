//! Copy state over a real terminal's text (scope §5.5, D19), with no PTY: bytes fed to a [`Term`]
//! as a program's output would be, core's snapshot of it through [`CopyText`], and die-vim's
//! grammar over that, with the window showing the engine's own cells where the snapshot is.

use die_test_cap as _;

use die_core::*;
use die_term::{CopyText, Term};
use die_vim::Vim;
use std::time::{Duration, Instant};

/// A terminal surface with no PTY: what the integrator's surface does for copy state.
struct Headless {
    term: Term,
    copy: CopyText,
}

impl Surface for Headless {
    fn kind(&self) -> &'static str {
        "term"
    }
    fn render(&mut self, frame: &mut Frame, _: &RenderInfo) {
        if let Some(f) = self.term.frame(true) {
            for (row, cells) in f.changed {
                for (out, c) in frame.row_mut(usize::from(row)).iter_mut().zip(cells) {
                    *out = FrameCell::from(c);
                }
            }
        }
    }
    fn event(&mut self, ev: &Event, _: &mut Ctx) -> bool {
        if let Event::Resize { cols, rows, .. } = ev {
            self.term.resize(*cols as u16, *rows as u16, (18, 36));
        }
        true
    }
    fn initial_state(&self) -> State {
        State::Passthrough
    }
    fn text(&mut self, update: bool) -> Option<SurfaceText> {
        Some(self.copy.text(&mut self.term, update))
    }
    fn scroll_to(&mut self, top: Option<usize>) {
        self.copy.scroll(&mut self.term, top);
    }
}

/// die-term's keys in normal state, as the integrator binds them: `i` back to insert.
struct Keys;

impl Extension for Keys {
    fn name(&self) -> &'static str {
        "term-keys"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.command("term-insert", "Back to insert.", |ctx, a| {
            ctx.set_state_of(a.buffer.unwrap(), State::Passthrough);
            Ok(())
        });
        reg.kind_keymap("term")
            .bind(State::Normal, "i", "term-insert");
    }
}

fn open() -> (Harness, BufferId, WindowId) {
    let exts: Vec<Box<dyn Extension>> = vec![Box::new(Vim::default()), Box::new(Keys)];
    let mut ctx = Harness::new(exts).expect("die-vim registers");
    let term = Term::new(80, 24, (18, 36)).unwrap();
    let copy = CopyText::default();
    let buf = ctx.create_surface("term", Headless { term, copy });
    let w = ctx.show(buf, ShowIn::Current).unwrap().unwrap();
    ctx.run_until_idle();
    ctx.screen();
    (ctx, buf, w)
}

/// The program prints, and the surface says so, as on PTY output.
fn print(ctx: &mut Harness, buf: BufferId, s: &str) {
    ctx.with_surface(buf, |h: &mut Headless, _| {
        let mut b = s.as_bytes();
        while !b.is_empty() {
            b = &b[h.term.feed(b)..];
        }
    })
    .unwrap();
    ctx.surface_text_changed(buf);
    ctx.screen();
}

fn seq(from: usize, to: usize) -> String {
    (from..=to).map(|i| format!("{i}\r\n")).collect()
}

fn errors(ctx: &mut Harness, keys: &str) -> Vec<String> {
    let log = ctx.type_keys(keys).expect("keys parse");
    log.into_iter().flat_map(|k| k.errors).collect()
}

/// The cursor's line in the snapshot, and its text.
fn cursor(ctx: &Harness, buf: BufferId, w: WindowId) -> (usize, String) {
    let t = ctx.text(buf).unwrap();
    let line = t.line_of(t.cursor(w).unwrap());
    (line, t.rope().line(line).to_string().trim_end().into())
}

/// The row the window draws the cursor on shows the cursor's line: the window shows the
/// engine's cells where the snapshot is.
fn shown_where_it_is(ctx: &mut Harness, buf: BufferId, w: WindowId) {
    let s = ctx.scene();
    let (row, col) = (s.cursors[0].row, s.cursors[0].col);
    let (line, text) = cursor(ctx, buf, w);
    assert_eq!(s.row_text(row).trim_end(), text, "line {line} on row {row}");
    let t = ctx.text(buf).unwrap();
    assert_eq!(col, t.display_col(t.cursor(w).unwrap()));
}

#[test]
fn seq_1_20000_then_search_yank_and_back_to_the_bottom() {
    let (mut ctx, buf, w) = open();
    print(&mut ctx, buf, &seq(1, 20_000));
    print(&mut ctx, buf, "$ ");
    let took = Instant::now();
    assert_eq!(errors(&mut ctx, "<Esc>"), Vec::<String>::new());
    eprintln!("Esc with its frame: {:?}", took.elapsed());
    let lines = ctx.text(buf).unwrap().line_count() - 1;
    assert!((9_000..=10_001).contains(&lines), "{lines} lines");
    assert_eq!(cursor(&ctx, buf, w).1, "$");
    shown_where_it_is(&mut ctx, buf, w);
    errors(&mut ctx, "gg");
    let first = ctx.text(buf).unwrap().rope().line(0).to_string();
    assert_eq!(
        ctx.scene().row_text(0),
        first.trim_end(),
        "the scrollback's top on screen"
    );
    shown_where_it_is(&mut ctx, buf, w);
    assert_eq!(errors(&mut ctx, "/19999<CR>"), Vec::<String>::new());
    assert_eq!(cursor(&ctx, buf, w).1, "19999");
    shown_where_it_is(&mut ctx, buf, w);
    errors(&mut ctx, "yy");
    assert_eq!(ctx.register('"').unwrap().text, "19999\n");
    errors(&mut ctx, "G");
    assert_eq!(cursor(&ctx, buf, w).1, "$", "back at the bottom");
    assert_eq!(ctx.scene().row_text(22), "20000", "the live screen");
    errors(&mut ctx, "k0vey");
    assert_eq!(ctx.register('"').unwrap().text, "20000");
}

#[test]
fn following_reads_only_what_changed_and_matches_a_whole_read() {
    let (mut ctx, buf, w) = open();
    print(&mut ctx, buf, &seq(1, 100));
    print(&mut ctx, buf, "$ ");
    errors(&mut ctx, "<Esc>");
    // Past the scrollback's end, so the oldest lines go while it follows.
    for k in 0..12 {
        print(
            &mut ctx,
            buf,
            &format!("\r\n{}$ ", seq(k * 1000, k * 1000 + 999)),
        );
        assert_eq!(cursor(&ctx, buf, w).1, "$", "after batch {k}");
    }
    shown_where_it_is(&mut ctx, buf, w);
    let followed = ctx.text(buf).unwrap().rope().to_string();
    let whole = ctx
        .with_surface(buf, |h: &mut Headless, _| {
            CopyText::default().text(&mut h.term, false).text
        })
        .unwrap();
    assert_eq!(followed, whole, "updates come to what reading it all gives");
    errors(&mut ctx, "5k");
    let held = cursor(&ctx, buf, w).1;
    print(&mut ctx, buf, "\r\nlater\r\n$ ");
    assert_eq!(cursor(&ctx, buf, w).1, held, "frozen on its line");
    assert!(
        ctx.text(buf).unwrap().rope().to_string().contains("later"),
        "with what's new read"
    );
    shown_where_it_is(&mut ctx, buf, w);
    errors(&mut ctx, "G");
    assert!(ctx.text(buf).unwrap().rope().to_string().contains("later"));
    shown_where_it_is(&mut ctx, buf, w);
}

#[test]
fn colors_are_the_engines_in_normal_state() {
    let (mut ctx, buf, w) = open();
    print(&mut ctx, buf, "plain \x1b[31mred\x1b[0m\r\n$ ");
    let before = ctx.scene();
    errors(&mut ctx, "<Esc>");
    let after = ctx.scene();
    assert_eq!(after.row(0), before.row(0), "the same cells");
    assert_ne!(after.row(0)[6].fg, after.row(0)[0].fg, "red stays red");
    errors(&mut ctx, "k0v$");
    let s = ctx.scene();
    let selection = ctx.face_style("selection").and_then(|s| s.bg).unwrap();
    assert_eq!((s.row(0)[0].bg, s.row(0)[6].bg), (selection, selection));
    assert_eq!(s.row(0)[6].fg, after.row(0)[6].fg);
    assert_eq!(cursor(&ctx, buf, w).1, "plain red");
}

#[test]
fn the_alternate_screen_is_its_rows_alone() {
    let (mut ctx, buf, w) = open();
    print(&mut ctx, buf, &seq(1, 50));
    print(&mut ctx, buf, "\x1b[?1049h\x1b[Hfirst\r\nsecond\x1b[5;3H");
    ctx.set_state_of(buf, State::Normal);
    ctx.screen();
    let t = ctx.text(buf).unwrap();
    assert_eq!(t.rope().line(0).to_string(), "first\n");
    assert_eq!(cursor(&ctx, buf, w).0, 4, "on the program's cursor");
    shown_where_it_is(&mut ctx, buf, w);
    errors(&mut ctx, "gg");
    assert_eq!(ctx.scene().row_text(0), "first");
    shown_where_it_is(&mut ctx, buf, w);
}

#[test]
fn i_goes_back_to_the_live_screen() {
    let (mut ctx, buf, _) = open();
    print(&mut ctx, buf, &seq(1, 200));
    print(&mut ctx, buf, "$ ");
    errors(&mut ctx, "<Esc>gg");
    assert_eq!(ctx.scene().row_text(0), "1");
    errors(&mut ctx, "i");
    assert_eq!(ctx.state(), State::Passthrough);
    assert_eq!(ctx.scene().row_text(23), "$", "the screen's bottom row");
    let at_bottom = ctx
        .with_surface(buf, |h: &mut Headless, _| h.term.viewport().at_bottom)
        .unwrap();
    assert!(at_bottom);
}

#[test]
fn a_ten_thousand_line_snapshot_is_quick_to_take() {
    let mut term = Term::new(80, 24, (18, 36)).unwrap();
    let line = format!(
        "{} the quick brown fox jumps over the lazy dog, again and again",
        "x"
    );
    let mut out = String::new();
    for i in 0..12_000 {
        out.push_str(&format!("{i:>6} {line}\r\n"));
    }
    let mut b = out.as_bytes();
    while !b.is_empty() {
        b = &b[term.feed(b)..];
    }
    let mut best = Duration::MAX;
    let mut lines = 0;
    for _ in 0..5 {
        let took = Instant::now();
        let st = CopyText::default().text(&mut term, false);
        best = best.min(took.elapsed());
        lines = st.text.lines().count();
    }
    eprintln!("a whole read of {lines} lines of 70 columns: {best:?}");
    assert!(lines >= 9_000, "{lines}");
    // Through core: the read, the rope, and the window's view, as Esc takes it.
    let exts: Vec<Box<dyn Extension>> = vec![Box::new(Vim::default())];
    let mut ctx = Harness::new(exts).unwrap();
    let buf = ctx.create_surface(
        "term",
        Headless {
            term,
            copy: CopyText::default(),
        },
    );
    ctx.show(buf, ShowIn::Current).unwrap();
    ctx.run_until_idle();
    let took = Instant::now();
    ctx.set_state_of(buf, State::Normal);
    let took = took.elapsed();
    eprintln!("copy state entered over them in {took:?}");
    assert!(ctx.text(buf).is_ok());
    assert!(took < Duration::from_millis(500), "{took:?}");
}
