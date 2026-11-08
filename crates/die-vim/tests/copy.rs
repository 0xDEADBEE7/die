//! Copy state (scope §5.5, D19): normal state on a surface is die-vim's grammar over a read-only
//! snapshot of its text, here a shell-like surface's lines. The window keeps the surface's own
//! cells, scrolled to where the snapshot is, with die-vim's cursor and selection over them, and
//! follows new output while the cursor is at the bottom.

use die_test_cap as _;

use die_core::*;
use die_vim::Vim;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

const PROMPT: &str = "$ ";
const RED: Rgb = Rgb(0xff, 0x40, 0x40);

/// What the shell has printed and heard, shared with the test.
#[derive(Default)]
struct Out {
    /// Every line, the prompt's last, the oldest gone past `cap`.
    lines: Vec<String>,
    /// Lines gone from the start so far.
    first: usize,
    cap: usize,
    /// The tops copy state asked for, when they changed.
    tops: Vec<Option<usize>>,
    /// What reached `event()`.
    heard: Vec<Event>,
    /// Whole texts read, and updates.
    reads: (usize, usize),
}

impl Out {
    /// Print a line above the prompt.
    fn print(&mut self, line: &str) {
        let at = self.lines.len().saturating_sub(1);
        self.lines.insert(at, line.into());
        let over = self.lines.len().saturating_sub(self.cap.max(1));
        self.lines.drain(..over);
        self.first += over;
    }
}

/// A terminal-like surface: its lines from the live view's top, or from where copy state says,
/// lines starting with `error:` in red, and the cursor after the prompt.
struct Shell {
    out: Rc<RefCell<Out>>,
    rows: usize,
    top: Option<usize>,
    /// The first line's number and the live view's, at the last read.
    read: (usize, usize),
}

impl Shell {
    fn live_top(&self, out: &Out) -> usize {
        out.lines.len().saturating_sub(self.rows)
    }
}

/// The cursor's row keeps its blanks through the cursor, as die-term's text does.
fn joined(lines: &[String], cursor_row: Option<usize>) -> String {
    let mut s = String::new();
    for (i, l) in lines.iter().enumerate() {
        s.push_str(l);
        if Some(i) == cursor_row {
            s.push(' ');
        }
        s.push('\n');
    }
    s
}

impl Surface for Shell {
    fn kind(&self) -> &'static str {
        "shell"
    }
    fn render(&mut self, frame: &mut Frame, _: &RenderInfo) {
        let out = self.out.borrow();
        let top = self.top.unwrap_or(self.live_top(&out));
        frame.clear();
        for r in 0..frame.rows() {
            let Some(line) = out.lines.get(top + r) else {
                break;
            };
            let mut style = FrameCell::blank();
            if line.starts_with("error:") {
                style.fg = Color::Rgb(RED);
            }
            frame.text(r, 0, line, &style);
        }
    }
    fn event(&mut self, ev: &Event, _: &mut Ctx) -> bool {
        if let Event::Resize { rows, .. } = ev {
            self.rows = *rows;
        }
        self.out.borrow_mut().heard.push(ev.clone());
        true
    }
    fn initial_state(&self) -> State {
        State::Passthrough
    }
    fn text(&mut self, update: bool) -> Option<SurfaceText> {
        let mut out = self.out.borrow_mut();
        let live = self.live_top(&out);
        let (first, seen) = (out.first, self.read);
        let last = out.lines.len() - 1;
        let mut st = match seen.1.checked_sub(first).filter(|_| update) {
            Some(from) => {
                out.reads.1 += 1;
                let mut st = SurfaceText::new(joined(&out.lines[from..], Some(last - from)));
                (st.dropped, st.from) = (first - seen.0, from);
                st
            }
            None => {
                out.reads.0 += 1;
                SurfaceText::new(joined(&out.lines, Some(last)))
            }
        };
        st.cursor = Some((last, PROMPT.len()));
        st.live_top = live;
        self.read = (first, first + live);
        Some(st)
    }
    fn scroll_to(&mut self, top: Option<usize>) {
        if top != self.top {
            self.top = top;
            self.out.borrow_mut().tops.push(top);
        }
    }
}

/// The shell's keys in normal state, as die-term binds a terminal's: Esc to the program, `i` and
/// `a` back to insert, `p` to paste into it.
struct ShellKeys;

impl Extension for ShellKeys {
    fn name(&self) -> &'static str {
        "shell-keys"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.command("shell-esc", "Send Esc to the program.", |ctx, a| {
            let buf = a.buffer.unwrap();
            let esc = Event::Key(KeyEvent::press(Key::Esc));
            ctx.with_surface(buf, |s: &mut Shell, ctx| s.event(&esc, ctx))?;
            Ok(())
        });
        reg.command("shell-insert", "Back to insert.", |ctx, a| {
            ctx.set_state_of(a.buffer.unwrap(), State::Passthrough);
            Ok(())
        });
        reg.kind_keymap("shell")
            .bind(State::Normal, "<Esc>", "shell-esc")
            .bind(State::Normal, "i", "shell-insert")
            .bind(State::Normal, "a", "shell-insert")
            .bind(State::Normal, "p", "surface-paste");
    }
}

struct Term {
    ctx: Harness,
    buf: BufferId,
    w: WindowId,
    out: Rc<RefCell<Out>>,
}

impl Term {
    /// A shell that printed `lines`, in passthrough, in a window of 24 rows.
    fn new(lines: impl IntoIterator<Item = String>) -> Term {
        let exts: Vec<Box<dyn Extension>> = vec![Box::new(Vim::default()), Box::new(ShellKeys)];
        let mut ctx = Harness::new(exts).expect("die-vim registers");
        let mut lines: Vec<String> = lines.into_iter().collect();
        lines.push(PROMPT.into());
        let cap = 100_000;
        let out = Rc::new(RefCell::new(Out {
            lines,
            cap,
            ..Out::default()
        }));
        let shell = Shell {
            out: out.clone(),
            rows: 24,
            top: None,
            read: (0, 0),
        };
        let buf = ctx.create_surface("shell", shell);
        let w = ctx.show(buf, ShowIn::Current).unwrap().unwrap();
        ctx.run_until_idle();
        ctx.screen();
        Term { ctx, buf, w, out }
    }
    fn keys(&mut self, keys: &str) -> Vec<String> {
        let log = self.ctx.type_keys(keys).expect("keys parse");
        log.into_iter().flat_map(|k| k.errors).collect()
    }
    /// The program prints, and says so as die-term's output does.
    fn print(&mut self, line: &str) {
        self.out.borrow_mut().print(line);
        self.ctx.surface_text_changed(self.buf);
        self.ctx.screen();
    }
    fn text(&self) -> String {
        self.ctx.text(self.buf).unwrap().rope().to_string()
    }
    /// The cursor's line, and the line it shows.
    fn cursor(&self) -> (usize, String) {
        let t = self.ctx.text(self.buf).unwrap();
        let line = t.line_of(t.cursor(self.w).unwrap());
        (line, t.rope().line(line).to_string().trim_end().into())
    }
    fn top(&mut self) -> Option<usize> {
        self.ctx.top_line(self.w)
    }
    fn yanked(&self) -> String {
        self.ctx
            .register('"')
            .map(|r| r.text.clone())
            .unwrap_or_default()
    }
}

fn numbers(n: usize) -> impl Iterator<Item = String> {
    (1..=n).map(|i| i.to_string())
}

#[test]
fn twenty_thousand_lines_take_motions_search_and_yank() {
    let mut t = Term::new(numbers(20_000));
    assert!(t.ctx.text(t.buf).is_err(), "no snapshot in insert");
    let took = Instant::now();
    assert_eq!(t.keys("<Esc>"), Vec::<String>::new());
    let took = took.elapsed();
    eprintln!("Esc over 20,000 lines, with its frame: {took:?}");
    assert_eq!(t.ctx.state(), State::Normal);
    assert_eq!(t.cursor(), (20_000, "$".into()), "on the shell's cursor");
    assert_eq!(t.top(), Some(20_001 - 24), "the live view");
    t.keys("gg");
    assert_eq!((t.cursor().0, t.top()), (0, Some(0)));
    assert_eq!(t.out.borrow().tops.last(), Some(&Some(0)));
    assert_eq!(t.keys("/19999<CR>"), Vec::<String>::new());
    assert_eq!(t.cursor(), (19_998, "19999".into()));
    t.keys("yy");
    assert_eq!(t.yanked(), "19999\n");
    t.keys("G");
    assert_eq!(t.cursor(), (20_000, "$".into()), "back at the bottom");
    assert_eq!(t.top(), Some(20_001 - 24));
    t.keys("2k0vey");
    assert_eq!(t.yanked(), "19999");
    t.keys("Gk");
    assert_eq!(t.cursor().1, "20000");
    t.keys("viwy");
    assert_eq!(t.yanked(), "20000");
    assert_eq!(t.ctx.state(), State::Normal);
}

#[test]
fn a_ten_thousand_line_snapshot_is_quick() {
    let mut t = Term::new(numbers(10_000).map(|n| format!("{n:>8} {}", "x".repeat(60))));
    let took = Instant::now();
    t.ctx.set_state_of(t.buf, State::Normal);
    let took = took.elapsed();
    assert_eq!(t.cursor().0, 10_000);
    eprintln!("10,000 lines of 69 columns taken in {took:?}");
    assert!(took.as_millis() < 500, "{took:?}");
}

#[test]
fn new_output_is_followed_at_the_bottom_and_not_after_k() {
    let mut t = Term::new(numbers(30));
    t.keys("<Esc>");
    t.print("thirty-one");
    assert!(t.text().contains("thirty-one"));
    assert_eq!(t.cursor(), (31, "$".into()), "still on the shell's cursor");
    assert_eq!(t.top(), Some(32 - 24), "showing the live view");
    // On the cursor's line, it follows the line and keeps its column.
    t.keys("0");
    t.print("thirty-two");
    assert_eq!(t.cursor(), (32, "$".into()));
    assert_eq!(
        t.ctx
            .text(t.buf)
            .unwrap()
            .display_col(t.ctx.text(t.buf).unwrap().cursor(t.w).unwrap()),
        0
    );
    t.keys("k");
    assert_eq!(t.cursor(), (31, "thirty-two".into()));
    t.print("thirty-three");
    assert!(!t.text().contains("thirty-three"), "frozen after k");
    assert_eq!(t.cursor(), (31, "thirty-two".into()));
    assert_eq!(
        t.out.borrow().tops.last(),
        Some(&Some(33 - 24)),
        "pinned where it was"
    );
    t.keys("G");
    assert!(
        t.text().contains("thirty-three"),
        "G comes back to the bottom"
    );
    assert_eq!(t.cursor(), (33, "$".into()));
    assert_eq!(t.top(), Some(34 - 24));
    let (whole, updates) = t.out.borrow().reads;
    assert_eq!(whole, 1, "updates read only what changed");
    assert!(updates >= 2);
}

#[test]
fn a_selection_waits_for_its_operator_before_following() {
    let mut t = Term::new(numbers(5));
    t.keys("<Esc>V");
    t.print("six");
    assert!(!t.text().contains("six"), "visual state doesn't follow");
    t.keys("<Esc>");
    assert!(t.text().contains("six"));
    assert_eq!(t.cursor(), (6, "$".into()));
}

#[test]
fn output_that_drops_the_oldest_lines_keeps_a_frozen_cursor_on_its_line() {
    let mut t = Term::new(numbers(50));
    t.out.borrow_mut().cap = 51;
    t.keys("<Esc>");
    t.print("fifty-one");
    assert_eq!(t.cursor(), (50, "$".into()));
    assert!(!t.text().starts_with("1\n"), "the oldest line went");
    t.keys("10k");
    assert_eq!(t.cursor().1, "42");
    t.print("fifty-two");
    assert_eq!(t.cursor().1, "42", "frozen, on the same line");
    t.keys("G");
    assert_eq!(t.cursor(), (50, "$".into()));
    assert!(t.text().starts_with("3\n"));
}

#[test]
fn colors_stay_and_the_selection_and_cursor_draw_over_them() {
    let mut t = Term::new(["ok".to_string(), "error: no".to_string()]);
    t.keys("<Esc>");
    let s = t.ctx.scene();
    assert_eq!(s.row_text(1), "error: no");
    assert_eq!(s.row(1)[0].fg, RED, "the surface's own colors");
    assert_eq!(
        (s.cursors[0].row, s.cursors[0].col),
        (2, 2),
        "on the prompt's cursor"
    );
    assert_eq!(s.cursors[0].shape, CursorShape::Block);
    t.keys("k0v$");
    let s = t.ctx.scene();
    let selection = t.ctx.face_style("selection").and_then(|s| s.bg).unwrap();
    assert_eq!(s.row(1)[0].bg, selection);
    assert_eq!(s.row(1)[0].fg, RED, "the selection keeps the text's color");
    assert_ne!(s.row(0)[0].bg, selection);
    assert_eq!(s.row(1)[9].bg, selection, "and the line's end");
    assert_eq!((s.cursors[0].row, s.cursors[0].col), (1, 9));
}

#[test]
fn edits_say_the_buffer_is_read_only() {
    let mut t = Term::new(numbers(3));
    t.keys("<Esc>k0");
    let before = t.text();
    for keys in [
        "x", "dd", "dw", "cw", "cc", "C", "s", "S", "o", "O", "R", "J", "~", "rx", ">>", "gUU",
        "gi", "I", "A", "vd", "vc", "VjJ", "<C-v>I", "u",
    ] {
        let errors = t.keys(keys);
        let said = errors
            .iter()
            .any(|e| e.contains("read-only") || e.contains("nothing to"));
        assert!(said, "{keys}: {errors:?}");
        assert_eq!(t.text(), before, "{keys}");
        assert_eq!(t.ctx.state(), State::Normal, "{keys}");
        t.keys("<Esc>");
        if t.ctx.state() == State::Passthrough {
            t.keys("<Esc>");
        }
        t.keys("Gk0");
    }
}

#[test]
fn the_kinds_keys_win_esc_goes_to_the_program_and_i_shows_the_live_screen() {
    let mut t = Term::new(numbers(40));
    t.keys("<Esc>");
    t.out.borrow_mut().heard.clear();
    t.keys("<Esc>");
    let esc = Event::Key(KeyEvent::press(Key::Esc));
    assert_eq!(t.out.borrow().heard, [esc]);
    assert_eq!(t.ctx.state(), State::Normal, "die-vim's Escape didn't run");
    t.keys("gg");
    t.keys("i");
    assert_eq!(t.ctx.state(), State::Passthrough);
    assert!(t.ctx.text(t.buf).is_err(), "no snapshot");
    t.ctx.screen();
    assert_eq!(t.out.borrow().tops.last(), Some(&None), "the live view");
    t.keys("<Esc>");
    assert_eq!(t.cursor().0, 40, "a new snapshot, on the cursor again");
    let d = t
        .ctx
        .describe_key(&[KeyEvent::press(Key::Char('j'))])
        .unwrap();
    assert!(d.to_string().contains("kind text"), "{d}");
}

#[test]
fn the_scroll_keys_go_over_the_scrollback_and_stop_at_the_live_view() {
    let mut t = Term::new(numbers(100));
    t.keys("<Esc>");
    let live = Some(101 - 24);
    assert_eq!(t.top(), live);
    t.keys("<C-e>");
    assert_eq!(t.top(), live, "nothing below the live view");
    t.keys("zt");
    assert_eq!(t.top(), live);
    t.keys("<C-b>");
    assert_eq!(t.top(), Some(101 - 24 - 22));
    assert_eq!(t.out.borrow().tops.last(), Some(&Some(101 - 24 - 22)));
    t.keys("<C-y><C-y>");
    assert_eq!(t.top(), Some(101 - 24 - 24));
    t.keys("<C-d>");
    assert_eq!(t.top(), Some(101 - 24 - 12));
    t.keys("<C-f><C-f>");
    assert_eq!(t.top(), live);
    t.keys("gg");
    assert_eq!(t.top(), Some(0));
    t.keys("50Gzz");
    assert_eq!(t.top(), Some(49 - 12));
    t.keys("zb");
    assert_eq!(t.top(), Some(49 - 23));
    t.keys("G");
    assert_eq!(t.top(), live);
    let scrolled = t.ctx.send(Input::Pointer(PointerEvent {
        kind: PointerKind::Scroll { dx: 0.0, dy: -3.0 },
        row: 5,
        col: 5,
        clicks: 0,
        mods: Mods::default(),
        px: None,
    }));
    assert_eq!(scrolled, Vec::<String>::new());
    assert!(
        t.top() < live,
        "the wheel scrolls the snapshot: {:?}",
        t.top()
    );
    assert!(
        t.out
            .borrow()
            .heard
            .iter()
            .all(|e| !matches!(e, Event::Pointer(_)))
    );
}

#[test]
fn p_pastes_into_the_program_and_a_split_has_its_own_place() {
    let mut t = Term::new(numbers(60));
    t.keys("<Esc>kyy");
    t.keys("p");
    assert_eq!(
        t.out.borrow().heard.last(),
        Some(&Event::Paste("60\n".into()))
    );
    t.keys("<C-w>s");
    let other = t.ctx.focused().unwrap();
    assert_ne!(other, t.w);
    t.keys("gg");
    let text = t.ctx.text(t.buf).unwrap();
    assert_eq!(text.line_of(text.cursor(other).unwrap()), 0);
    assert_eq!(
        text.line_of(text.cursor(t.w).unwrap()),
        59,
        "the first window kept its place"
    );
}

#[test]
fn a_click_moves_the_cursor_and_a_drag_selects_as_on_text() {
    let mut t = Term::new(numbers(5).map(|n| format!("line {n}")));
    t.keys("<Esc>");
    t.out.borrow_mut().heard.clear();
    t.ctx.pointer(PointerKind::Press(Button::Left), 1, 5);
    t.ctx.pointer(PointerKind::Release(Button::Left), 1, 5);
    assert_eq!(t.cursor(), (1, "line 2".into()));
    t.ctx.pointer(PointerKind::Press(Button::Left), 2, 0);
    t.ctx.pointer(PointerKind::Drag(Button::Left), 3, 3);
    t.ctx.pointer(PointerKind::Release(Button::Left), 3, 3);
    assert_eq!(t.ctx.state(), State::Visual);
    t.keys("y");
    assert_eq!(t.yanked(), "line 3\nline");
    assert!(
        t.out.borrow().heard.is_empty(),
        "{:?}",
        t.out.borrow().heard
    );
}

#[test]
fn what_die_vim_kept_of_an_old_snapshot_does_no_harm_in_a_new_one() {
    let mut t = Term::new(numbers(300));
    t.keys("<Esc>ggmajjvjjyGmb50%");
    t.keys("i");
    t.out.borrow_mut().cap = 20;
    for i in 0..40 {
        t.out.borrow_mut().print(&format!("new {i}"));
    }
    t.keys("<Esc>");
    for keys in [
        "gv<Esc>", "`a", "'b", "<C-o>", "<Tab>", "g;", "g,", "``", "''", "`[", "`.", "n", "*",
    ] {
        let errors = t.keys(keys);
        assert!(
            errors.iter().all(|e| !e.contains("panicked")),
            "{keys}: {errors:?}"
        );
        let text = t.ctx.text(t.buf).unwrap();
        assert!(text.cursor(t.w).unwrap() <= text.len(), "{keys}");
    }
}
