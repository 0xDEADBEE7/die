//! The loop's decisions, with no window: how long to sleep, how many turns a pump may
//! take, the grid for a drawable, and what a frame shows. These run on every platform.

use die_test_cap as _;

use die_core::{
    Button, Clock, Config, Ctx, Input, Key, KeyEvent, Mods, PointerEvent, PointerKind, Result,
    Scene, ShowIn, Side,
};
use die_render::font::{CellMetrics, FaceMetrics};
use die_render::latency::Latency;
use die_render::window::{
    Host, RETRY_DRAW, Sleep, TURN_LIMIT, grid, holds_frame, sleep_for, sleep_or_retry,
};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn host(text: &str) -> Host {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let buf = ctx.create_text("a.txt", text);
    ctx.open_window(buf, 80, 24).unwrap();
    Host::new(ctx)
}

fn row_text(s: &Scene, r: usize) -> String {
    s.row(r).iter().map(|c| c.glyph.to_string()).collect()
}

/// The 19×40 cell at 2x.
fn cell() -> CellMetrics {
    let m = FaceMetrics {
        ascent: 30.8799,
        descent: 7.04,
        leading: 2.0801,
        underline_position: -3.2,
        underline_thickness: 1.6,
        x_height: 17.0,
    };
    CellMetrics::new(m, 19.2, 2.0)
}

#[test]
fn with_no_timer_the_loop_sleeps_until_something_wakes_it() {
    assert_eq!(sleep_for(None, ms(5)), Sleep::Forever);
}

#[test]
fn it_sleeps_exactly_until_the_next_timer() {
    assert_eq!(sleep_for(Some(ms(30)), ms(5)), Sleep::For(ms(25)));
}

#[test]
fn a_timer_already_due_means_no_sleep() {
    assert_eq!(sleep_for(Some(ms(5)), ms(5)), Sleep::No);
    assert_eq!(sleep_for(Some(ms(4)), ms(5)), Sleep::No);
}

#[test]
fn a_pump_runs_everything_ready_and_says_so() {
    let mut h = host("x");
    h.ctx.defer(|ctx| {
        ctx.defer(|_| Ok(()));
        Ok(())
    });
    let p = h.pump();
    assert_eq!(p.ran, 2);
    assert!(!p.more);
    let p = h.pump();
    assert_eq!((p.ran, p.more), (0, false), "nothing was left to run");
}

/// A call that queues itself again forever.
fn again(ctx: &mut Ctx) -> Result {
    ctx.defer(again);
    Ok(())
}

#[test]
fn a_call_that_keeps_queueing_another_cant_hold_the_loop() {
    let mut h = host("x");
    h.ctx.defer(again);
    let p = h.pump();
    assert_eq!(p.ran, TURN_LIMIT);
    assert!(p.more, "there's more, so the loop doesn't sleep");
    assert_eq!(h.sleep(p.more), Sleep::No);
}

#[test]
fn a_pump_that_stopped_short_stops_the_loop_sleeping_whatever_the_timers_say() {
    let mut h = host("x");
    h.ctx.after(ms(1000), |_| Ok(())).detach();
    assert_eq!(h.sleep(true), Sleep::No);
    // The fake clock stands still, so the wait is exact.
    assert_eq!(h.sleep(false), Sleep::For(ms(1000)));
}

#[test]
fn the_grid_is_whole_cells_of_the_drawable() {
    let c = cell();
    assert_eq!((c.w, c.h), (19, 40), "the cell these sizes use");
    assert_eq!(grid((1900, 800), &c), (100, 20));
    assert_eq!(grid((1918, 839), &c), (100, 20), "a part cell is no cell");
    assert_eq!(grid((1919, 840), &c), (101, 21));
}

#[test]
fn a_very_small_drawable_still_has_a_grid_core_can_compose() {
    let c = cell();
    let small = grid((1, 1), &c);
    assert_eq!(small, (1, 3), "a text row, a status line, a command line");
    let mut h = host("hello");
    h.set_grid(small);
    h.pump();
    let s = h.scene();
    assert_eq!((s.cols, s.rows), (1, 3));
}

#[test]
fn one_window_fills_the_frame_above_its_status_line_and_the_command_line() {
    let mut h = host("hello\nworld\n");
    h.set_grid((30, 6));
    h.pump();
    let w = h.ctx.focused().unwrap();
    assert_eq!(h.ctx.window_size(w), Some((30, 4)));
    let s = h.scene();
    assert_eq!((s.cols, s.rows), (30, 6));
    assert!(row_text(&s, 0).starts_with("hello"));
    assert!(row_text(&s, 1).starts_with("world"));
    assert!(row_text(&s, 4).contains("a.txt"), "the status line");
    assert_eq!(row_text(&s, 5).trim(), "", "the command line");
}

#[test]
fn every_window_of_a_split_shows_and_a_click_reaches_the_one_it_is_over() {
    let mut h = host("hello\nworld\n");
    h.set_grid((30, 6));
    let left = h.ctx.focused().unwrap();
    let right = h.ctx.split_window(left, Side::Right).unwrap();
    let other = h.ctx.create_text("b.txt", "other\n");
    h.ctx.focus(right);
    h.ctx.show(other, ShowIn::Current).unwrap();
    h.ctx.focus(left);
    h.pump();
    let s = h.scene();
    assert!(row_text(&s, 0).starts_with("hello"), "{}", row_text(&s, 0));
    assert!(row_text(&s, 0).contains("│other"), "{}", row_text(&s, 0));

    h.ctx.handle(Input::Pointer(PointerEvent {
        kind: PointerKind::Press(Button::Left),
        row: 0,
        col: 18,
        clicks: 1,
        mods: Mods::default(),
        px: None,
    }));
    h.pump();
    assert_eq!(h.ctx.focused(), Some(right));
    assert_eq!(h.ctx.text(other).unwrap().cursor(right), Some(3));

    h.set_grid((40, 6));
    h.pump();
    assert_eq!(
        h.ctx.window_size(left).map(|s| s.0),
        Some(19),
        "they share it"
    );
}

#[test]
fn input_goes_to_core_and_runs_on_the_next_pump() {
    let mut h = host("hello");
    let seen = Rc::new(Cell::new(0));
    let s2 = seen.clone();
    h.ctx
        .observe_keys(move |_, _| s2.set(s2.get() + 1))
        .detach();
    h.ctx.input(KeyEvent::press(Key::Char('z')));
    assert_eq!(h.pump().ran, 1);
    assert_eq!(seen.get(), 1);
}

#[test]
fn a_host_with_no_window_composes_an_empty_frame() {
    let mut h = Host::new(Ctx::new(vec![]).unwrap());
    h.set_grid((10, 4));
    assert_eq!(h.pump().ran, 0);
    let s = h.scene();
    assert_eq!((s.cols, s.rows), (10, 4));
    assert!(s.cursors.is_empty());
}

#[test]
fn a_real_clock_fires_a_timer_through_the_host() {
    let config = Config {
        clock: Clock::real(),
        ..Config::default()
    };
    let mut h = Host::new(Ctx::with_config(vec![], config).unwrap());
    let fired = Rc::new(Cell::new(false));
    let f2 = fired.clone();
    h.ctx
        .after(ms(5), move |_| {
            f2.set(true);
            Ok(())
        })
        .detach();
    let Sleep::For(d) = h.sleep(false) else {
        panic!("a timer is pending")
    };
    std::thread::sleep(d);
    h.pump();
    assert!(fired.get());
    assert_eq!(h.sleep(false), Sleep::Forever);
}

#[test]
fn occlusion_holds_back_every_frame_but_the_first() {
    assert!(!holds_frame(true, 0, true), "the first frame always draws");
    assert!(holds_frame(true, 1, true));
    assert!(!holds_frame(true, 1, false));
    assert!(
        !holds_frame(false, 1, true),
        "with the guard off, nothing's held"
    );
}

#[test]
fn the_latency_log_has_a_line_for_each_stage_and_key_and_says_if_it_cannot_open() {
    let dir = std::env::temp_dir().join(format!("die-latency-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log");
    let mut log = Latency::open(&path).unwrap();
    assert!(log.logging());
    log.stage("window");
    log.key(1, ms(1), 0.5, 0.25);
    log.key(2, ms(2), 0.5, 0.25);
    log.shown(vec![(2, None)]);
    log.flush();
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].starts_with("startup stage=window ms="), "{text}");
    let key = "to_shown_ms=none cpu_ms=0.50 gpu_ms=0.25";
    assert_eq!(lines[1], format!("key to_frame_ms=2.00 {key}"), "{text}");
    assert_eq!(lines[2], format!("key to_frame_ms=1.00 {key}"), "{text}");

    let err = Latency::open(&dir.join("no/such/dir")).err().unwrap();
    assert!(err.contains("DIE_LATENCY_LOG"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pixels_pending_keep_the_loop_from_sleeping_until_a_draw_takes_them() {
    let mut h = host("");
    assert_eq!(h.sleep(false), Sleep::Forever);
    let pending = h.pending();
    assert!(!pending.is_set());
    // A present's thread raises it; a clone is the same flag.
    std::thread::spawn({
        let pending = pending.clone();
        move || pending.set()
    })
    .join()
    .unwrap();
    assert!(pending.is_set());
    assert_eq!(
        h.sleep(false),
        Sleep::No,
        "a frame is owed, as with PTY output"
    );
    assert!(pending.take(), "the draw takes it");
    assert!(!pending.take(), "once");
    assert_eq!(h.sleep(false), Sleep::Forever);
}

#[test]
fn a_frame_that_couldnt_draw_wakes_the_loop_again() {
    assert_eq!(sleep_or_retry(Sleep::Forever, true), Sleep::For(RETRY_DRAW));
    assert_eq!(sleep_or_retry(Sleep::For(ms(5)), true), Sleep::For(ms(5)));
    assert_eq!(
        sleep_or_retry(Sleep::For(ms(5000)), true),
        Sleep::For(RETRY_DRAW)
    );
    assert_eq!(sleep_or_retry(Sleep::No, true), Sleep::No);
    assert_eq!(sleep_or_retry(Sleep::Forever, false), Sleep::Forever);
}
