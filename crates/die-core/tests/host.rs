//! K9: the calls die-render drives core through, headless. Test plan: `workspace.rs`, and here:
//! - `handle` keeps its order, with keys, a resize and the pointer running in the turn they came.
//! - `run_ready` says whether a new frame is needed, never waits for a job, and stops at its turn
//!   limit, with one message if only queued calls took those turns, and `next_deadline` is then
//!   zero so the host comes back.
//! - `next_deadline`: None when nothing's due, the time left, and zero when overdue or ready.
//! - `scene` draws the split tree at the frame's size, the same as `compose` with `placements`.
//! - `Input::Focus` hollows every cursor, and a surface hears it.
//! - `cell_at` agrees with the scene's positions over a split, and names the part of the window.
//! - Pointer input counts as activity for idle timers, and a bare move doesn't.
//! - The seam R3 is built against: `step`, `next_timer`, `input`, `set_waker`, `compose`.

use die_test_cap as _;

use die_core::*;
use std::cell::{Cell as StdCell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

const MS: Duration = Duration::from_millis(1);
/// What `run_ready` logs when queued calls alone reach its limit.
const STOPPED: &str = "so the frame went first";

fn key(c: char) -> Input {
    Input::Key(parse_keys(&c.to_string()).unwrap().pop().unwrap())
}

fn pointer(kind: PointerKind, row: usize, col: usize) -> Input {
    Input::Pointer(PointerEvent {
        kind,
        row,
        col,
        clicks: 0,
        mods: Mods::default(),
        px: None,
    })
}

struct Keys(Rc<RefCell<Vec<String>>>);

impl Extension for Keys {
    fn name(&self) -> &'static str {
        "keys"
    }
    fn register(&mut self, reg: &mut Registry) {
        let log = self.0.clone();
        reg.command(
            "note",
            "Note the key and the frame's size.",
            move |ctx, a| {
                let (cols, rows) = ctx.frame_size();
                log.borrow_mut()
                    .push(format!("{} {cols}x{rows}", a.char().unwrap_or('?')));
                Ok(())
            },
        );
        reg.global_keymap().bind(State::Normal, "<char>", "note");
    }
}

fn world(cols: usize, rows: usize) -> (Ctx, BufferId, WindowId, Rc<RefCell<Vec<String>>>) {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut c = Ctx::new(vec![Box::new(Keys(log.clone()))]).unwrap();
    c.handle(Input::Resize { cols, rows });
    assert!(c.run_ready());
    let b = c.create_text("t", "hello\nworld\n");
    let w = c.show(b, ShowIn::Current).unwrap().unwrap();
    (c, b, w, log)
}

// ------------------------------------------------------------------ handle and the turn loop

#[test]
fn input_runs_in_its_turn_and_in_the_order_it_came() {
    let (mut c, _, _, log) = world(80, 24);
    c.handle(key('a'));
    c.handle(Input::Resize { cols: 40, rows: 10 });
    c.handle(key('b'));
    assert!(log.borrow().is_empty(), "handle runs nothing");
    assert_eq!(c.frame_size(), (80, 24), "a resize waits its turn too");
    assert!(c.run_ready());
    assert_eq!(*log.borrow(), ["a 80x24", "b 40x10"]);
}

#[test]
fn fed_keys_go_ahead_of_what_the_host_handed_in() {
    let (mut c, _, _, log) = world(80, 24);
    c.handle(key('t'));
    c.feed_keys("f").unwrap();
    c.run_ready();
    assert_eq!(*log.borrow(), ["f 80x24", "t 80x24"]);
}

#[test]
fn run_ready_says_whether_anything_ran() {
    let (mut c, _, _, _) = world(80, 24);
    assert!(!c.run_ready(), "nothing to do");
    c.handle(key('a'));
    assert!(c.run_ready());
    assert!(!c.run_ready());
    c.defer(|_| Ok(()));
    assert!(c.run_ready());
    let _t = c.after(MS, |_| Ok(()));
    assert!(!c.run_ready(), "not yet");
}

#[test]
fn run_ready_runs_a_timer_only_when_the_clock_says() {
    let ran = Rc::new(StdCell::new(0));
    let mut c = Ctx::new(vec![]).unwrap();
    let r = ran.clone();
    let _t = c.after(10 * MS, move |_| {
        r.set(r.get() + 1);
        Ok(())
    });
    assert!(!c.run_ready());
    assert_eq!(ran.get(), 0);
    c.advance(10 * MS);
    assert_eq!(ran.get(), 1);
    assert!(!c.run_ready());
}

#[test]
fn run_ready_doesnt_wait_for_a_job() {
    let mut c = Ctx::new(vec![]).unwrap();
    let (tx, rx) = mpsc::channel::<()>();
    let done = Rc::new(StdCell::new(false));
    let d = done.clone();
    let _job = c.spawn(
        move || rx.recv().unwrap(),
        move |_, _| {
            d.set(true);
            Ok(())
        },
    );
    assert!(!c.run_ready(), "the job is still out");
    assert_eq!(c.next_deadline(), None, "and the waker tells the host");
    tx.send(()).unwrap();
    while !done.get() {
        // It wakes the host when it ends, and that's when a real host looks.
        c.run_ready();
        std::thread::yield_now();
    }
}

#[test]
fn a_call_that_keeps_queueing_another_stops_for_the_frame() {
    fn again(ctx: &mut Ctx) -> Result {
        ctx.defer(again);
        Ok(())
    }
    let mut c = Ctx::new(vec![]).unwrap();
    c.defer(again);
    assert!(c.run_ready());
    assert_eq!(c.next_deadline(), Some(Duration::ZERO), "there's more");
    let said = |c: &Ctx| c.messages.iter().filter(|m| m.contains(STOPPED)).count();
    assert_eq!(said(&c), 1);
    assert!(c.run_ready());
    assert_eq!(said(&c), 1, "once");
}

#[test]
fn a_long_paste_or_macro_yields_for_the_frame_and_says_nothing() {
    let said = |c: &Ctx| c.messages.iter().any(|m| m.contains(STOPPED));
    let n = TURN_LIMIT + 5;

    let (mut c, _, _, _) = world(40, 10);
    for _ in 0..n {
        c.handle(key('a'));
    }
    assert!(c.run_ready());
    assert!(!said(&c), "typed keys");
    assert_eq!(c.next_deadline(), Some(Duration::ZERO), "there's more");
    assert!(c.run_ready());
    assert_eq!(c.next_deadline(), None, "and it's done");
    assert!(!said(&c));

    let (mut c, _, _, _) = world(40, 10);
    c.feed_keys(&"a".repeat(n)).unwrap();
    assert!(c.run_ready());
    assert!(!said(&c), "fed keys");
    assert_eq!(c.next_deadline(), Some(Duration::ZERO), "there's more");
}

#[test]
fn a_call_that_starves_waiting_keys_is_reported() {
    fn again(ctx: &mut Ctx) -> Result {
        ctx.defer(again);
        Ok(())
    }
    let (mut c, _, _, _) = world(40, 10);
    c.defer(again);
    c.handle(key('a'));
    assert!(c.run_ready());
    let said = c.messages.iter().filter(|m| m.contains(STOPPED)).count();
    assert_eq!(said, 1);
    assert_eq!(c.next_deadline(), Some(Duration::ZERO));
}

#[test]
fn turn_limit_is_how_far_one_call_runs() {
    let n = Rc::new(StdCell::new(0usize));
    let count = n.clone();
    fn queue(ctx: &mut Ctx, n: Rc<StdCell<usize>>) -> Result {
        n.set(n.get() + 1);
        ctx.defer(move |ctx| queue(ctx, n));
        Ok(())
    }
    let mut c = Ctx::new(vec![]).unwrap();
    c.defer(move |ctx| queue(ctx, count));
    c.run_ready();
    assert_eq!(n.get(), TURN_LIMIT);
}

// ------------------------------------------------------------------ next_deadline

#[test]
fn next_deadline_is_how_long_the_host_can_sleep() {
    let mut c = Ctx::new(vec![]).unwrap();
    assert_eq!(c.next_deadline(), None, "nothing is due");
    let _t = c.after(30 * MS, |_| Ok(()));
    assert_eq!(c.next_deadline(), Some(30 * MS));
    let _u = c.after(10 * MS, |_| Ok(()));
    assert_eq!(c.next_deadline(), Some(10 * MS));
    c.advance(4 * MS);
    assert_eq!(c.next_deadline(), Some(6 * MS), "what's left, not when");
    c.advance(6 * MS);
    assert_eq!(c.next_deadline(), Some(20 * MS));
    c.advance(20 * MS);
    assert_eq!(c.next_deadline(), None);
}

#[test]
fn next_deadline_is_zero_with_work_ready() {
    let (mut c, _, _, _) = world(80, 24);
    assert_eq!(c.next_deadline(), None);
    c.handle(key('a'));
    assert_eq!(c.next_deadline(), Some(Duration::ZERO), "a key");
    c.run_ready();
    c.defer(|_| Ok(()));
    assert_eq!(c.next_deadline(), Some(Duration::ZERO), "a call");
    c.run_ready();
    c.feed_keys("a").unwrap();
    assert_eq!(c.next_deadline(), Some(Duration::ZERO), "a fed key");
    c.run_ready();
    assert_eq!(c.next_deadline(), None);
}

#[test]
fn a_repeat_that_fell_behind_is_overdue_not_hidden() {
    let cfg = Config {
        clock: Clock::real(),
        ..Default::default()
    };
    let mut c = Ctx::with_config(vec![], cfg).unwrap();
    let _slow = c.every(MS, |_| {
        std::thread::sleep(Duration::from_millis(5));
        Ok(())
    });
    std::thread::sleep(Duration::from_millis(2));
    c.run_ready();
    assert_eq!(c.next_deadline(), Some(Duration::ZERO));
}

#[test]
fn a_cancelled_timer_isnt_a_deadline() {
    let mut c = Ctx::new(vec![]).unwrap();
    let t = c.after(10 * MS, |_| Ok(()));
    drop(t);
    assert_eq!(c.next_deadline(), None);
}

#[test]
fn an_idle_timer_is_a_deadline_until_it_fires_and_a_key_starts_it_again() {
    let (mut c, _, _, _) = world(80, 24);
    let _t = c.idle(20 * MS, |_| Ok(()));
    assert_eq!(c.next_deadline(), Some(20 * MS));
    c.advance(20 * MS);
    assert_eq!(c.next_deadline(), None, "once per idle period");
    c.handle(key('a'));
    c.run_ready();
    assert_eq!(c.next_deadline(), Some(20 * MS));
}

// ------------------------------------------------------------------ the pointer and idle

#[test]
fn pointer_input_counts_as_activity_and_a_bare_move_doesnt() {
    let (mut c, _, _, _) = world(80, 24);
    let _t = c.idle(20 * MS, |_| Ok(()));
    c.advance(15 * MS);
    assert_eq!(c.next_deadline(), Some(5 * MS));
    c.handle(pointer(PointerKind::Move, 0, 0));
    c.run_ready();
    assert_eq!(c.next_deadline(), Some(5 * MS), "a bare move");
    for kind in [
        PointerKind::Press(Button::Left),
        PointerKind::Release(Button::Left),
        PointerKind::Drag(Button::Right),
        PointerKind::Scroll { dx: 0.0, dy: 1.5 },
    ] {
        c.advance(MS);
        c.handle(pointer(kind, 0, 0));
        c.run_ready();
        assert_eq!(c.next_deadline(), Some(20 * MS), "{kind:?}");
    }
}

// ------------------------------------------------------------------ the pointer on text

fn press(c: &mut Ctx, kind: PointerKind, row: usize, col: usize) {
    c.handle(pointer(kind, row, col));
    c.run_ready();
    c.scene();
}

const LEFT: PointerKind = PointerKind::Press(Button::Left);
const DRAG: PointerKind = PointerKind::Drag(Button::Left);

#[test]
fn a_click_focuses_the_window_and_places_the_cursor() {
    let (mut c, b, a, _) = world(23, 10);
    let r = c.split_window(a, Side::Right).unwrap();
    c.scene();
    c.focus(r);
    press(&mut c, LEFT, 1, 2);
    assert_eq!(c.focused(), Some(a));
    let t = c.text(b).unwrap();
    assert_eq!((t.cursor(a), t.cursor(r)), (Some(8), Some(0)));

    press(&mut c, LEFT, 8, 15);
    assert_eq!(c.focused(), Some(r), "a status line focuses its window");
    assert_eq!(c.text(b).unwrap().cursor(r), Some(0), "and places nothing");

    press(&mut c, PointerKind::Press(Button::Right), 1, 2);
    assert_eq!(c.focused(), Some(r), "only the left button");
}

#[test]
fn a_click_past_a_rows_end_lands_on_its_end_of_line_position() {
    let (mut c, b, w, _) = world(8, 5);
    let t = c.text_mut(b).unwrap();
    t.tx(Origin::User).insert(0, "abcd\n").commit().unwrap();
    let s = t.settings_mut();
    s.max_width = Some(4);
    s.center = true;
    c.scene();
    press(&mut c, LEFT, 0, 7);
    assert_eq!(
        c.text(b).unwrap().cursor(w),
        Some(4),
        "a full row's padding"
    );
    press(&mut c, LEFT, 1, 7);
    assert_eq!(
        c.text(b).unwrap().cursor(w),
        Some(8),
        "a wrapped row: its last"
    );
    press(&mut c, LEFT, 2, 7);
    assert_eq!(
        c.text(b).unwrap().cursor(w),
        Some(10),
        "a short row: its newline"
    );
}

#[test]
fn a_drag_selects_in_visual_state_from_where_it_was_pressed() {
    let (mut c, b, w, _) = world(23, 10);
    c.scene();
    press(&mut c, LEFT, 0, 1);
    press(&mut c, DRAG, 0, 1);
    assert_eq!(c.state(), State::Normal, "it hasn't left the cell");
    press(&mut c, DRAG, 1, 2);
    press(&mut c, PointerKind::Release(Button::Left), 1, 2);
    press(&mut c, DRAG, 0, 4);
    assert_eq!(c.state(), State::Visual);
    let sel = c.text(b).unwrap().selection(w).unwrap();
    assert_eq!((sel.anchor, sel.head, sel.shape), (1, 8, Shape::Char));

    press(&mut c, LEFT, 0, 3);
    assert_eq!(c.state(), State::Normal, "with no keymap too");
    assert_eq!(c.text(b).unwrap().selection(w), Some(Selection::cursor(3)));
}

#[test]
fn the_trackpad_scrolls_the_window_under_it_and_brings_the_cursor_along() {
    let (mut c, _, a, _) = world(23, 6);
    let b = c.create_text("long", &"0123\n".repeat(20));
    c.show(b, ShowIn::Current).unwrap();
    let r = c.split_window(a, Side::Right).unwrap();
    c.scene();
    press(&mut c, LEFT, 0, 3);
    let scroll = |c: &mut Ctx, dy| press(c, PointerKind::Scroll { dx: 0.0, dy }, 0, 15);
    let view = |c: &Ctx, w| {
        let t = c.text(b).unwrap();
        (t.scroll(w).unwrap().0, t.cursor(w).unwrap())
    };

    scroll(&mut c, 0.6);
    assert_eq!(view(&c, r).0, 0, "short of a row");
    scroll(&mut c, 0.6);
    assert_eq!(view(&c, r), (5, 5), "the cursor kept to the top row");
    scroll(&mut c, 2.0);
    assert_eq!(view(&c, r), (15, 15));
    assert_eq!(c.focused(), Some(a), "the focus stays");
    assert_eq!(view(&c, a), (0, 3), "and so does the other window");

    scroll(&mut c, -2.2);
    assert_eq!(view(&c, r).0, 5, "up, keeping the 0.2 over from before");
    c.text_mut(b).unwrap().set_cursor(r, 5);
    scroll(&mut c, 1e6);
    assert_eq!(view(&c, r), (100, 100), "the last row, and no further");
    c.focus(r);
    scroll(&mut c, -1e6);
    assert_eq!(view(&c, r).0, 0);
    assert_eq!(view(&c, r).1, 15, "the bottom row of 3, at its column");
}

#[test]
fn a_scroll_over_a_prompts_list_goes_through_it_and_not_the_window_it_hides() {
    let (mut c, _, a, _) = world(20, 24);
    let b = c.create_text("long", &"0123\n".repeat(50));
    c.show(b, ShowIn::Current).unwrap();
    let items = (0..40).map(|i| format!("item{i}"));
    c.prompt(PromptSpec::new("? ").candidates(items), |_, _| Ok(()));
    let s = c.scene();
    assert!(s.row_text(20).starts_with("item"), "{}", s.row_text(20));
    assert_eq!(c.cell_at(20, 2), None);
    press(&mut c, PointerKind::Scroll { dx: 0.0, dy: 3.0 }, 20, 2);
    assert_eq!(c.prompt_chosen(), Some("item3"));
    let t = c.text(b).unwrap();
    assert_eq!((t.scroll(a).unwrap().0, t.cursor(a)), (0, Some(0)));

    press(&mut c, PointerKind::Scroll { dx: 0.0, dy: 1.0 }, 0, 2);
    assert_eq!(c.prompt_chosen(), Some("item3"), "above the list");
    assert_eq!(c.text(b).unwrap().scroll(a).unwrap().0, 5, "the window");
}

#[test]
fn a_click_on_virtual_text_at_a_rows_edge_stays_on_its_row() {
    let (mut c, b, w, _) = world(10, 6);
    let t = c.text_mut(b).unwrap();
    t.decorate(11..11, Deco::Virtual("v".repeat(5).into()))
        .unwrap();
    c.scene();
    press(&mut c, LEFT, 1, 9);
    assert_eq!(c.text(b).unwrap().cursor(w), Some(10), "its last character");
}

#[test]
fn a_click_in_a_frame_composed_narrower_than_its_window_lands_in_it() {
    let (mut c, b, w, _) = world(20, 10);
    let placed = c.placements();
    c.compose(10, 5, &placed);
    c.handle(pointer(LEFT, 4, 5));
    c.run_ready();
    assert_eq!(c.text(b).unwrap().cursor(w), Some(12), "the text's end");
}

// ------------------------------------------------------------------ scene

#[test]
fn scene_draws_the_tree_at_the_frames_size() {
    let (mut c, b, w, _) = world(20, 8);
    let s = c.scene();
    assert_eq!((s.cols, s.rows), (20, 8));
    assert_eq!(s.row_text(0), "hello");
    assert_eq!(s.row_text(6), " t", "the status line");
    assert_eq!(s.cursors.len(), 1);
    assert_eq!(s.cursors[0].shape, CursorShape::Block);

    let right = c.split_window(w, Side::Right).unwrap();
    let other = c.create_text("other", "other text");
    c.focus(right);
    c.show(other, ShowIn::Current).unwrap();
    let s = c.scene();
    assert_eq!(s.row_text(0), "hello    │other text");
    assert_eq!(s.cursors.len(), 2);
    assert_eq!(s.cursors[0].shape, CursorShape::Block, "the focused window");
    assert_eq!(s.cursors[1].shape, CursorShape::Hollow);
    let _ = b;
}

#[test]
fn scene_is_compose_with_the_placements() {
    let (mut c, _, w, _) = world(60, 20);
    let below = c.split_window(w, Side::Below).unwrap();
    let _right = c.split_window(below, Side::Right).unwrap();
    let placed = c.placements();
    assert_eq!(placed.len(), 3);
    let (cols, rows) = c.frame_size();
    let by_hand = c.compose(cols, rows, &placed);
    assert_eq!(c.scene(), by_hand);
}

#[test]
fn a_frame_too_small_for_its_windows_draws_what_fits() {
    let (mut c, _, w, _) = world(60, 20);
    let below = c.split_window(w, Side::Below).unwrap();
    let right = c.split_window(below, Side::Right).unwrap();
    for (cols, rows) in [(1, 1), (2, 2), (3, 2), (1, 9), (9, 1), (0, 0), (4, 3)] {
        c.handle(Input::Resize { cols, rows });
        c.run_ready();
        let s = c.scene();
        assert_eq!((s.cols, s.rows), (cols, rows));
        let _ = c.window_in_direction(right, Side::Left);
        let b = c.window_buffer(w).unwrap();
        c.text_mut(b).unwrap().set_cursor(w, 2);
        assert!(c.placements().iter().all(|p| {
            let (pc, pr) = c.window_size(p.window).unwrap();
            p.col + pc <= cols && p.row + pr < rows.max(1)
        }));
    }
    c.handle(Input::Resize { cols: 60, rows: 20 });
    c.run_ready();
    assert_eq!(c.placements().len(), 3, "all of them come back");
}

// ------------------------------------------------------------------ the host's focus

#[test]
fn without_the_keyboard_focus_every_cursor_is_hollow() {
    let (mut c, _, w, _) = world(40, 10);
    c.split_window(w, Side::Right).unwrap();
    c.handle(Input::Focus(false));
    c.run_ready();
    let s = c.scene();
    assert_eq!(s.cursors.len(), 2);
    assert!(s.cursors.iter().all(|k| k.shape == CursorShape::Hollow));
    c.handle(Input::Focus(true));
    c.run_ready();
    assert_eq!(c.scene().cursors[0].shape, CursorShape::Block);
}

#[test]
fn the_prompts_cursor_is_hollow_too_without_the_keyboard_focus() {
    let (mut c, _, _, _) = world(40, 10);
    c.prompt("name", |_, _| Ok(()));
    c.handle(Input::Focus(false));
    c.run_ready();
    let s = c.scene();
    assert!(s.cursors.iter().all(|k| k.shape == CursorShape::Hollow));
}

#[derive(Default)]
struct Plain(Rc<RefCell<Vec<Event>>>);

impl Surface for Plain {
    fn kind(&self) -> &'static str {
        "plain"
    }
    fn render(&mut self, _: &mut Frame, _: &RenderInfo) {}
    fn event(&mut self, ev: &Event, _: &mut Ctx) -> bool {
        self.0.borrow_mut().push(ev.clone());
        false
    }
}

#[test]
fn the_focused_surface_hears_the_hosts_focus_once() {
    let (mut c, _, _, _) = world(40, 10);
    let log: Rc<RefCell<Vec<Event>>> = Rc::default();
    let s = c.create_surface("s", Plain(log.clone()));
    c.show(s, ShowIn::Current).unwrap();
    c.run_ready();
    log.borrow_mut().clear();
    c.handle(Input::Focus(false));
    c.handle(Input::Focus(false));
    c.handle(Input::Focus(true));
    c.run_ready();
    assert_eq!(*log.borrow(), [Event::Focus(false), Event::Focus(true)]);
}

#[test]
fn a_surface_hears_nothing_while_the_app_is_unfocused() {
    // It gains the window's focus while DIE's is away, and hears of it once DIE has it back.
    let (mut c, _, a, _) = world(40, 10);
    let log: Rc<RefCell<Vec<Event>>> = Rc::default();
    let s = c.create_surface("s", Plain(log.clone()));
    let b = c.split_window(a, Side::Right).unwrap();
    c.focus(b);
    c.show(s, ShowIn::Current).unwrap();
    c.focus(a);
    c.run_ready();
    log.borrow_mut().clear();
    c.handle(Input::Focus(false));
    c.run_ready();
    c.focus(b);
    c.run_ready();
    assert_eq!(*log.borrow(), [], "DIE doesn't have the focus to give");
    c.handle(Input::Focus(true));
    c.run_ready();
    assert_eq!(*log.borrow(), [Event::Focus(true)]);

    // It loses the window's focus while DIE's is away. It heard that already.
    log.borrow_mut().clear();
    c.handle(Input::Focus(false));
    c.run_ready();
    c.focus(a);
    c.run_ready();
    assert_eq!(*log.borrow(), [Event::Focus(false)], "once");
    log.borrow_mut().clear();
    c.handle(Input::Focus(true));
    c.run_ready();
    assert_eq!(*log.borrow(), [], "the text window has it now");
}

// ------------------------------------------------------------------ cells

#[test]
fn a_cell_maps_to_a_window_and_a_position() {
    let (mut c, _, a, _) = world(23, 10);
    let b = c.split_window(a, Side::Right).unwrap();
    assert_eq!(c.cell_at(0, 0), None, "no frame yet");
    let s = c.scene();
    let hit = |window, part, pos| Some(CellHit { window, part, pos });
    assert_eq!(c.cell_at(0, 0), hit(a, WindowPart::Text, Some(0)));
    assert_eq!(c.cell_at(1, 2), hit(a, WindowPart::Text, Some(8)));
    assert_eq!(c.cell_at(0, 12), hit(b, WindowPart::Text, Some(0)));
    assert_eq!(c.cell_at(8, 3), hit(a, WindowPart::Status, None));
    assert_eq!(c.cell_at(8, 15), hit(b, WindowPart::Status, None));
    assert_eq!(c.cell_at(0, 11), hit(a, WindowPart::Divider, None));
    assert_eq!(c.cell_at(8, 11), hit(a, WindowPart::Divider, None));
    assert_eq!(
        c.cell_at(5, 3),
        hit(a, WindowPart::Text, None),
        "below the text"
    );
    assert_eq!(c.cell_at(9, 0), None, "the command line");
    assert_eq!(c.cell_at(99, 99), None);
    for r in 0..s.rows {
        for col in 0..s.cols {
            let from_scene = s.position(r, col);
            let from_core = c.cell_at(r, col).and_then(|h| Some((h.window, h.pos?)));
            assert_eq!(from_scene, from_core, "{r},{col}");
        }
    }
}

#[test]
fn a_cell_is_what_the_last_frame_showed() {
    let (mut c, b, w, _) = world(20, 8);
    let _ = c.scene();
    assert_eq!(c.cell_at(0, 0).unwrap().pos, Some(0));
    c.text_mut(b)
        .unwrap()
        .tx(Origin::Extension)
        .insert(0, "abc")
        .commit()
        .unwrap();
    assert_eq!(c.cell_at(0, 1).unwrap().pos, Some(1), "the frame on screen");
    let _ = c.scene();
    assert_eq!(c.cell_at(0, 3).unwrap().pos, Some(3));
    let _ = w;
}

#[test]
fn compose_keeps_the_cells_for_a_host_that_places_windows_itself() {
    let mut c = Ctx::new(vec![]).unwrap();
    let b = c.create_text("t", "hello");
    let w = c.open_window(b, 10, 3).unwrap();
    let placed = [WindowPlacement {
        window: w,
        col: 2,
        row: 1,
    }];
    c.compose(14, 7, &placed);
    assert_eq!(c.cell_at(1, 3).unwrap().pos, Some(1));
    assert_eq!(c.cell_at(0, 0), None);
    assert_eq!(c.cell_at(4, 3).unwrap().part, WindowPart::Status);
    assert_eq!(c.cell_at(1, 12).unwrap().part, WindowPart::Divider);
}

// ------------------------------------------------------------------ what R3 stands on

#[test]
fn the_seam_r3_is_built_against_still_works() {
    let mut c = Ctx::with_config(
        vec![],
        Config {
            clock: Clock::real(),
            ..Default::default()
        },
    )
    .unwrap();
    let woken = Arc::new(AtomicUsize::new(0));
    let w2 = woken.clone();
    c.set_waker(move || {
        w2.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    let b = c.create_text("t", "hello");
    let w = c.open_window(b, 80, 22).unwrap();
    let _t = c.after(Duration::from_secs(3600), |_| Ok(()));
    c.input(parse_keys("x").unwrap().pop().unwrap());
    while c.step() {}
    assert!(c.next_timer().unwrap() > c.now());
    let s = c.compose(
        80,
        24,
        &[WindowPlacement {
            window: w,
            col: 0,
            row: 0,
        }],
    );
    assert_eq!(s, c.scene());
    c.waker().wake();
    assert_eq!(woken.load(Ordering::SeqCst), 1);
}

#[test]
fn the_host_loop_end_to_end() {
    let (mut c, _, _, log) = world(30, 10);
    let mut frames = 0;
    let ticks = Rc::new(StdCell::new(0));
    let t = ticks.clone();
    let _repeat = c.every(10 * MS, move |_| {
        t.set(t.get() + 1);
        Ok(())
    });
    for input in [key('a'), key('b')] {
        c.handle(input);
        if c.run_ready() {
            let s = c.scene();
            assert_eq!((s.cols, s.rows), (30, 10));
            frames += 1;
        }
        let sleep = c.next_deadline().expect("the repeat is due");
        c.advance(sleep);
    }
    assert_eq!(frames, 2);
    assert_eq!(ticks.get(), 2);
    assert_eq!(*log.borrow(), ["a 30x10", "b 30x10"]);
}

#[test]
fn a_cursor_on_a_wrapped_lines_first_row_comes_to_the_top_when_scrolled_off() {
    let (mut c, _, a, _) = world(10, 6);
    let b = c.create_text("long", &format!("{}\nend\n", "x".repeat(90)));
    c.show(b, ShowIn::Current).unwrap();
    c.scene();
    c.text_mut(b).unwrap().set_cursor(a, 0);
    press(&mut c, PointerKind::Scroll { dx: 0.0, dy: 2.0 }, 0, 5);
    let t = c.text(b).unwrap();
    assert_eq!(t.scroll(a).unwrap().0, 0, "still the same line");
    let (row, _) = t.screen_pos(a, t.cursor(a).unwrap()).unwrap();
    assert_eq!(row, 0, "the row it left by, not the last");
}

#[test]
fn a_click_in_the_gutter_or_below_the_text_lands_on_the_nearest_text() {
    let (mut c, b, w, _) = world(20, 10);
    c.text_mut(b).unwrap().settings_mut().line_numbers = LineNumbers::Absolute;
    c.scene();
    press(&mut c, LEFT, 1, 0);
    assert_eq!(
        c.text(b).unwrap().cursor(w),
        Some(6),
        "the gutter: its row's start"
    );
    press(&mut c, LEFT, 6, 5);
    assert_eq!(
        c.text(b).unwrap().cursor(w),
        Some(12),
        "below: the text's end"
    );
}

#[test]
fn keys_and_clicks_dont_count_while_a_restart_hands_over() {
    let (mut c, b, w, log) = world(20, 10);
    c.scene();
    c.hand_over_session().unwrap();
    c.handle(key('a'));
    c.run_ready();
    press(&mut c, LEFT, 1, 2);
    assert!(
        log.borrow().is_empty(),
        "the session can no longer hold them"
    );
    assert_eq!(c.text(b).unwrap().cursor(w), Some(0));

    c.resume_session();
    c.handle(key('b'));
    c.run_ready();
    press(&mut c, LEFT, 1, 2);
    assert_eq!(
        log.borrow().len(),
        1,
        "the restart failed, so they count again"
    );
    assert_eq!(c.text(b).unwrap().cursor(w), Some(8));
}

#[test]
fn a_cursor_scrolled_off_a_wrapped_line_keeps_its_column_on_the_screen() {
    let (mut c, _, a, _) = world(20, 6);
    let b = c.create_text("long", &format!("{}\nend\n", "x".repeat(200)));
    c.show(b, ShowIn::Current).unwrap();
    c.scene();
    c.text_mut(b).unwrap().set_cursor(a, 65);
    c.scene();
    press(&mut c, PointerKind::Scroll { dx: 0.0, dy: 8.0 }, 0, 5);
    let t = c.text(b).unwrap();
    let cursor = t.cursor(a).unwrap();
    assert_eq!(t.screen_pos(a, cursor), Some((0, 5)), "at {cursor}");
}
