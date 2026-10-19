//! Criteria 10, 11, 12: keymap precedence and passthrough; the prefix argument; feed_keys and a
//! key observer (q/@ round trip).
#![allow(clippy::type_complexity)] // S4's test as ported (K1)
mod support;
use die_core::*;
use std::cell::RefCell;
use std::rc::Rc;
use support::*;

type Log = Rc<RefCell<Vec<String>>>;

fn logger(reg: &mut Registry, log: &Log, name: &'static str) {
    let log = log.clone();
    reg.command(name, "", move |_, a| {
        log.borrow_mut().push(format!(
            "{name}{}",
            a.typed().map(|t| format!(":{t}")).unwrap_or_default()
        ));
        Ok(())
    });
}

struct Ext {
    log: Log,
}
impl Extension for Ext {
    fn name(&self) -> &'static str {
        "keys"
    }
    fn register(&mut self, reg: &mut Registry) {
        for n in [
            "cmd-x",
            "cmd-char",
            "cmd-any",
            "button",
            "paste",
            "esc-normal",
        ] {
            logger(reg, &self.log, n);
        }
        reg.kind_keymap("text")
            .bind(State::Normal, "x", "cmd-x")
            .bind(State::Normal, "<char>", "cmd-char")
            .bind(State::Normal, "<any>", "cmd-any");
        reg.keymap("button").bind(State::Normal, "<CR>", "button");
        reg.global_keymap()
            .bind(State::Passthrough, "<D-v>", "paste");
        reg.kind_keymap("term")
            .bind(State::Normal, "<Esc>", "esc-normal");
    }
}

fn setup(text: &str) -> (Ctx, Log, BufferId, WindowId) {
    let log: Log = Rc::default();
    let mut ctx = Ctx::new(vec![Box::new(Ext { log: log.clone() })]).unwrap();
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, 40, 5).unwrap();
    (ctx, log, b, w)
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

#[test]
fn exact_beats_char_beats_any_within_one_keymap() {
    let (mut ctx, log, _, _) = setup("abc");
    ctx.type_keys("xy<Esc><C-a>");
    ctx.input(KeyEvent::committed("é")); // a dead key's committed text
    ctx.run_until_idle();
    assert_eq!(
        take(&log),
        ["cmd-x:x", "cmd-char:y", "cmd-any", "cmd-any", "cmd-char:é"]
    );
}

#[test]
fn release_events_never_reach_keymaps() {
    let (mut ctx, log, _, _) = setup("abc");
    ctx.input(KeyEvent::press(Key::Char('x')).kind(KeyKind::Release));
    ctx.input(KeyEvent::press(Key::Esc).kind(KeyKind::Release));
    ctx.input(KeyEvent::press(Key::Char('x')).kind(KeyKind::Repeat));
    ctx.run_until_idle();
    assert_eq!(take(&log), ["cmd-x:x"], "only the repeat matched");
}

#[test]
fn range_keymap_comes_first() {
    let (mut ctx, log, b, w) = setup("[ok] rest");
    ctx.text_mut(b)
        .unwrap()
        .decorate(0..4, Deco::Keymap("button".into()))
        .unwrap();
    ctx.type_keys("<CR>");
    ctx.text_mut(b).unwrap().set_cursor(w, 6);
    ctx.type_keys("<CR>");
    assert_eq!(take(&log), ["button", "cmd-any"]);
}

#[derive(Default)]
struct Search;
impl Mode for Search {}
struct SearchExt {
    log: Log,
}
impl Extension for SearchExt {
    fn name(&self) -> &'static str {
        "seek"
    }
    fn register(&mut self, reg: &mut Registry) {
        logger(reg, &self.log, "search-char");
        reg.mode::<Search>("seek-search", || Search)
            .bind(State::Normal, "<char>", "search-char");
    }
}

#[test]
fn a_modes_char_beats_the_kind_keymaps_j() {
    let log: Log = Rc::default();
    let mut ctx = Ctx::new(vec![
        Box::new(Vim::default()),
        Box::new(SearchExt { log: log.clone() }),
    ])
    .unwrap();
    let b = ctx.create_text("browser", "row0\nrow1\nrow2\n");
    let w = ctx.open_window(b, 40, 5).unwrap();
    ctx.enable_mode::<Search>(b).unwrap();
    ctx.type_keys("jk");
    assert_eq!(take(&log), ["search-char:j", "search-char:k"]);
    assert_eq!(lc(&ctx, b, w), (0, 0), "die-vim's j never ran");
    ctx.disable_mode::<Search>(b).unwrap();
    ctx.type_keys("j");
    assert_eq!(lc(&ctx, b, w), (1, 0));
}

#[test]
fn passthrough_cmd_esc_first_and_other_keys_to_event() {
    let (mut ctx, log, _, _) = setup("");
    let events = Rc::new(RefCell::new(vec![]));
    let t = ctx.create_surface(
        "term",
        TermSurface {
            events: events.clone(),
        },
    );
    let w = ctx.open_window(t, 40, 5).unwrap();
    assert_eq!(ctx.state(), State::Passthrough);
    let cmd = Mods {
        cmd: true,
        ..Default::default()
    };
    let a = KeyEvent::press(Key::Char('a'));
    let esc = KeyEvent::press(Key::Esc);
    let cmd_v = KeyEvent::press(Key::Char('v')).mods(cmd);
    let cmd_esc = esc.clone().mods(cmd);
    for ev in [
        a.clone(),
        a.clone().kind(KeyKind::Release),
        cmd_v.clone(),
        cmd_v.clone().kind(KeyKind::Repeat),
        cmd_v.clone().kind(KeyKind::Release),
        esc.clone(), // ESC goes to the program (D18)
        cmd_esc.clone(),
        cmd_esc.clone().kind(KeyKind::Repeat),
        esc.clone().kind(KeyKind::Release), // Cmd may be up by now
    ] {
        ctx.input(ev);
    }
    ctx.run_until_idle();
    assert_eq!(ctx.state(), State::Normal, "Cmd-Esc left passthrough");
    let got: Vec<Event> = events.borrow().clone();
    assert_eq!(
        got,
        [
            Event::Key(a.clone()),
            Event::Key(a.kind(KeyKind::Release)),
            Event::Key(esc.clone())
        ]
    );
    assert_eq!(
        take(&log),
        ["paste"],
        "Cmd layer went to DIE's keymaps; its repeat and release were swallowed"
    );
    ctx.input(esc);
    ctx.run_until_idle();
    assert_eq!(
        take(&log),
        ["esc-normal"],
        "a fresh Esc reaches normal state's keymaps"
    );
    let _ = w;
}

// ---------------------------------------------------------------- criterion 11: prefix argument

struct ArgsExt {
    seen: Rc<RefCell<Vec<(Option<usize>, Option<char>)>>>,
}
impl Extension for ArgsExt {
    fn name(&self) -> &'static str {
        "args"
    }
    fn register(&mut self, reg: &mut Registry) {
        let seen = self.seen.clone();
        reg.command("show-args", "", move |_, a| {
            seen.borrow_mut().push((a.count, a.register));
            Ok(())
        });
        reg.kind_keymap("text")
            .bind(State::Normal, "S", "show-args");
    }
}

#[test]
fn prefix_argument_set_by_die_vim_commands_and_read_from_args() {
    let seen = Rc::new(RefCell::new(vec![]));
    let mut ctx = Ctx::new(vec![
        Box::new(Vim::default()),
        Box::new(ArgsExt { seen: seen.clone() }),
    ])
    .unwrap();
    let text: String = (0..20).map(|i| format!("line{i}\n")).collect();
    let b = ctx.create_text("t", &text);
    let w = ctx.open_window(b, 40, 10).unwrap();
    ctx.type_keys("\"b5S");
    ctx.type_keys("S");
    ctx.type_keys("5\"cS");
    ctx.type_keys("\"<Esc>S"); // <any> cancels the pending register
    assert_eq!(
        *seen.borrow(),
        [
            (Some(5), Some('b')),
            (None, None),
            (Some(5), Some('c')),
            (None, None)
        ]
    );
    ctx.type_keys("12j");
    assert_eq!(lc(&ctx, b, w), (12, 0));
    ctx.type_keys("ll0");
    assert_eq!(lc(&ctx, b, w), (12, 0), "0 with no count is a motion");
    ctx.type_keys("\"a2dd");
    assert_eq!(ctx.register('a').unwrap().text, "line12\nline13\n");
    assert!(ctx.register('"').is_none());
    assert_eq!(ctx.prefix(), Prefix::default());
}

// ---------------------------------------------------------------- criterion 12: feed_keys + observer

#[test]
fn macro_round_trip_with_feed_keys_and_a_key_observer() {
    let (mut ctx, b, w) = vim_ctx("abc\ndef\nghi\njkl\n", 40, 10);
    ctx.type_keys("qaxjq");
    assert_eq!(text(&ctx, b), "bc\ndef\nghi\njkl\n");
    assert_eq!(ctx.register('a').unwrap().text, "xj");
    ctx.type_keys("@a");
    assert_eq!(text(&ctx, b), "bc\nef\nghi\njkl\n");
    assert_eq!(lc(&ctx, b, w), (2, 0));
    // Keys fed by @a are flagged, so recording b stores "@a", as Vim does.
    ctx.type_keys("qb@aq");
    assert_eq!(ctx.register('b').unwrap().text, "@a");
    assert_eq!(text(&ctx, b), "bc\nef\nhi\njkl\n");
    ctx.type_keys("@b");
    assert_eq!(text(&ctx, b), "bc\nef\nhi\nkl\n");
    ctx.type_keys("k");
    ctx.feed_keys("x").unwrap();
    ctx.run_until_idle();
    assert_eq!(text(&ctx, b), "bc\nef\nhi\nl\n");
}

// ---------------------------------------------------------------- C8: startup check

struct Typo;
impl Extension for Typo {
    fn name(&self) -> &'static str {
        "typo"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.kind_keymap("text")
            .bind(State::Normal, "a", "no-such-cmd")
            .bind(State::Normal, "b", "also-missing")
            .bind(State::Normal, "<bad", "x");
    }
}

#[test]
fn missing_command_names_reported_at_once() {
    let errs = Ctx::new(vec![Box::new(Typo)]).err().unwrap();
    assert_eq!(errs.len(), 3, "{errs:?}");
    assert!(
        errs.iter().any(|e| e.contains("no-such-cmd"))
            && errs.iter().any(|e| e.contains("also-missing"))
    );
}
