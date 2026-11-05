//! K7: the public harness of contract §16, with one small extension.

use die_test_cap as _;

use die_core::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

const SECOND: Duration = Duration::from_secs(1);

/// `x` logs news, `f` fails, `w` puts a kill on the clipboard, `o` reads it, `j` moves down a
/// line, and `t` sets a one-second timer that fails.
struct Toy {
    pasted: Rc<Cell<bool>>,
}

impl Extension for Toy {
    fn name(&self) -> &'static str {
        "toy"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.command("x", "News.", |ctx, _| {
            ctx.messages.push("Wrote foo".into());
            Ok(())
        });
        reg.command("f", "Fail.", |_, _| Err(Error::Msg("toy failed".into())));
        reg.command("w", "Write the clipboard.", |ctx, _| {
            ctx.write_clipboard(&Register {
                kind: RegKind::Line,
                text: "killed\n".into(),
            });
            Ok(())
        });
        let pasted = self.pasted.clone();
        reg.command("o", "Read the clipboard.", move |ctx, _| {
            pasted.set(ctx.clipboard().is_some_and(|r| r.text == "outside"));
            Ok(())
        });
        reg.command("j", "Down.", |ctx, _| {
            let w = ctx.focused().unwrap();
            let b = ctx.window_buffer(w).unwrap();
            let t = ctx.text_mut(b)?;
            let line = t.line_of(t.cursor(w).unwrap());
            let to = t.line_start(line + 1);
            t.set_cursor(w, to);
            Ok(())
        });
        reg.command("t", "Fail in a second.", |ctx, _| {
            ctx.after(SECOND, |_| Err(Error::Msg("timer failed".into())))
                .detach();
            Ok(())
        });
        for key in ["x", "f", "w", "o", "j", "t"] {
            reg.global_keymap().bind(State::Normal, key, key);
        }
    }
}

fn harness() -> (Harness, BufferId, WindowId) {
    let toy = Toy {
        pasted: Rc::default(),
    };
    let mut h = Harness::new(vec![Box::new(toy)]).unwrap();
    let text: String = (0..20).map(|i| format!("line {i}\n")).collect();
    let (b, w) = h.open_text("t", &text).unwrap();
    (h, b, w)
}

#[test]
fn each_key_logs_its_own_errors_and_a_failure_doesnt_drop_the_rest() {
    let (mut h, ..) = harness();
    let log = h.type_keys("xfq").unwrap();
    let keys: Vec<&str> = log.iter().map(|k| k.key.as_str()).collect();
    assert_eq!(keys, ["x", "f", "q"]);
    assert_eq!(log[0].errors, Vec::<String>::new());
    assert!(log[1].errors[0].contains("toy failed"), "{:?}", log[1]);
    assert_eq!(log[2].errors, ["q is undefined"]);
    assert!(h.type_keys("<nonsense>").is_err());
}

#[test]
fn news_in_the_log_isnt_an_error() {
    let (mut h, ..) = harness();
    assert_eq!(h.type_keys("x").unwrap()[0].errors, Vec::<String>::new());
    assert_eq!(h.messages, ["Wrote foo"]);
}

/// Pointer input takes its turn: a press restarts an idle timer, and a bare move doesn't.
#[test]
fn pointer_events_take_their_turn() {
    let (mut h, ..) = harness();
    let ran = Rc::new(Cell::new(0));
    let r = ran.clone();
    let _sub = h.idle(SECOND, move |_| {
        r.set(r.get() + 1);
        Ok(())
    });
    h.advance(SECOND / 2);
    assert_eq!(h.pointer(PointerKind::Move, 1, 1), Vec::<String>::new());
    let press = PointerKind::Press(Button::Left);
    assert_eq!(h.pointer(press, 1, 1), Vec::<String>::new());
    h.advance(SECOND * 3 / 4);
    assert_eq!(ran.get(), 0, "the press started the wait again");
    h.advance(SECOND / 4);
    assert_eq!(ran.get(), 1);
}

#[test]
fn a_real_clock_gives_way_and_a_timer_runs_when_the_clock_is_moved() {
    let toy = Toy {
        pasted: Rc::default(),
    };
    let config = Config {
        clock: Clock::real(),
        ..Config::default()
    };
    let mut h = Harness::with_config(vec![Box::new(toy)], config).unwrap();
    h.open_text("t", "").unwrap();
    h.type_keys("t").unwrap();
    assert_eq!(h.now(), Duration::ZERO);
    let errors = h.advance(SECOND);
    assert!(errors[0].contains("timer failed"), "{errors:?}");
    assert_eq!(h.now(), SECOND);
}

#[test]
fn the_clipboard_is_fake_both_ways() {
    let pasted = Rc::new(Cell::new(false));
    let toy = Toy {
        pasted: pasted.clone(),
    };
    let mut h = Harness::new(vec![Box::new(toy)]).unwrap();
    h.open_text("t", "").unwrap();
    h.fake_clipboard().copy_from_outside("outside");
    h.type_keys("o").unwrap();
    assert!(pasted.get(), "core reads what another app copied");
    h.type_keys("w").unwrap();
    let writes = h.fake_clipboard().take_writes();
    assert_eq!(writes[0].text, "killed\n");
}

/// A clipboard in the config gives way, even a fake one, and `fake_clipboard` is the one in use.
#[test]
fn fake_clipboard_stays_the_one_core_is_using() {
    let toy = Toy {
        pasted: Rc::default(),
    };
    let mine = FakeClipboard::default();
    let config = Config {
        clipboard: Some(Box::new(mine.clone())),
        ..Config::default()
    };
    let mut h = Harness::with_config(vec![Box::new(toy)], config).unwrap();
    h.open_text("t", "").unwrap();
    h.type_keys("w").unwrap();
    assert_eq!(mine.take_writes().len(), 0);
    assert_eq!(h.fake_clipboard().take_writes().len(), 1);

    let other = FakeClipboard::default();
    h.set_clipboard(other.clone());
    h.type_keys("w").unwrap();
    assert_eq!(other.held().unwrap().text, "killed\n");
    assert_eq!(h.fake_clipboard().take_writes().len(), 1);
}

/// A panic core catches in your config's code, or a runaway loop, counts as a key's error.
#[test]
fn panics_and_runaways_that_only_said_so_in_messages_are_errors() {
    let (mut h, ..) = harness();
    h.set_status_text(|_, _| panic!("boom"));
    let errors = h.type_keys("x").unwrap().remove(0).errors;
    assert!(
        errors[0].starts_with("status text panicked: boom"),
        "{errors:?}"
    );

    h.set_file_settings(|_, _| panic!("boom"));
    h.open_file(&std::env::temp_dir().join("die-k7-no-such-file"))
        .unwrap();
    let errors = h.take_errors();
    assert!(errors[0].contains("file settings panicked"), "{errors:?}");

    fn again(ctx: &mut Ctx) -> Result {
        ctx.defer(again);
        Ok(())
    }
    h.defer(again);
    let errors = h.type_keys("x").unwrap().remove(0).errors;
    assert!(errors[0].contains("never went idle"), "{errors:?}");
}

/// §12's rule for a job that outlasts its wait: 5 s for the first key, not 5 s for each.
#[test]
fn a_job_that_outlasts_its_wait_doesnt_hold_up_the_rest_of_the_keys() {
    let (mut h, ..) = harness();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let _job = h.spawn(move || rx.recv().ok(), |_, _| Ok(()));
    let start = std::time::Instant::now();
    let log = h.type_keys("qq").unwrap();
    assert!(start.elapsed() < SECOND * 9, "{:?}", start.elapsed());
    assert_eq!(log.len(), 2);
    assert_eq!(log[1].errors, ["q is undefined"]);
    drop(tx);
}

/// Errors from a macro, outside any harness input, wait to be read and aren't the next key's.
#[test]
fn errors_logged_outside_an_input_are_kept_apart_from_the_next_keys() {
    let (mut h, ..) = harness();
    h.feed_keys("q").unwrap();
    h.run_until_idle();
    let log = h.type_keys("x").unwrap();
    assert_eq!(log[0].errors, Vec::<String>::new());
    assert_eq!(h.take_errors(), ["q is undefined"]);
    assert_eq!(h.take_errors(), Vec::<String>::new());
}

#[test]
fn the_window_has_the_size_asked_for_and_the_frame_fits_it() {
    let (mut h, _, w) = harness();
    assert_eq!(h.window_size(w), Some((80, 24)));
    h.set_window_size(20, 5);
    assert_eq!(h.window_size(w), Some((20, 5)));
    assert_eq!(h.frame_size(), (20, 7));
    assert_eq!(
        h.screen().split('\n').count(),
        7,
        "text, status, command line"
    );
}

#[test]
fn the_top_line_can_be_set_and_follows_the_cursor_after_each_key() {
    let (mut h, b, w) = harness();
    h.set_window_size(20, 3);
    let on_line = h.text(b).unwrap().line_start(5);
    h.text_mut(b).unwrap().set_cursor(w, on_line);
    h.set_top_line(w, 4).unwrap();
    assert_eq!(h.top_line(w), Some(4));
    assert!(h.screen().starts_with("line 4\nline 5\nline 6\n"));
    h.type_keys("jj").unwrap();
    assert_eq!(h.top_line(w), Some(5), "the second j scrolled");
    h.set_top_line(w, 0).unwrap();
    assert_eq!(
        h.top_line(w),
        Some(5),
        "a frame goes back to the cursor (C5)"
    );
    h.set_top_line(w, 99).unwrap();
    h.text_mut(b).unwrap().set_cursor(w, 0);
    assert_eq!(h.top_line(w), Some(0));
}

#[test]
fn the_style_dump_shows_what_the_screen_does_and_its_styles() {
    let (mut h, ..) = harness();
    h.set_window_size(10, 1);
    let dump = h.style_dump();
    assert!(dump.starts_with("scene 10x3\ntext\n|line 0"), "{dump}");
    assert!(
        dump.contains("\nstyle\n") && dump.contains("\nlegend\n"),
        "{dump}"
    );
    assert!(h.screen().starts_with("line 0\n"));
}
