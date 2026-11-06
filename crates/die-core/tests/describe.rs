//! K16: `describe-key` and `describe-command` through the harness: keys typed, `*help*` read.

use die_test_cap as _;

mod support;
use die_core::*;
use std::cell::RefCell;
use std::rc::Rc;
use support::TermSurface;

type Log = Rc<RefCell<Vec<&'static str>>>;

#[derive(Default)]
struct Fold;
impl Mode for Fold {}

/// Commands that log their names, the same keys bound at each level of the lookup order, and
/// `h` for `describe-key`, `H` for `describe-command`.
struct Toy {
    log: Log,
}

impl Extension for Toy {
    fn name(&self) -> &'static str {
        "toy"
    }
    fn register(&mut self, reg: &mut Registry) {
        for c in [
            "first",
            "second",
            "in-global",
            "in-kind",
            "in-mode",
            "in-range",
        ] {
            let log = self.log.clone();
            reg.command(c, "Log its name.", move |_, _| {
                log.borrow_mut().push(c);
                Ok(())
            });
        }
        reg.mode::<Fold>("fold", Fold::default);
        let g = reg.global_keymap();
        g.bind(State::Normal, "h", "describe-key");
        g.bind(State::Normal, "H", "describe-command");
        g.bind(State::Normal, "x", "first");
        g.bind(State::Normal, "x", "second");
        g.bind(State::Normal, "y", "in-global");
        g.bind(State::Normal, "z", "in-global");
        g.bind(State::Normal, "gg", "first");
        g.bind(State::Insert, "y", "in-global");
        reg.kind_keymap("text").bind(State::Normal, "y", "in-kind");
        let fold = reg.mode_keymap("fold").unwrap();
        fold.bind(State::Normal, "y", "in-mode");
        reg.keymap("button").bind(State::Normal, "y", "in-range");
    }
}

/// Your config frees `z`, which the extension bound, and binds `gx` in `kind text`.
struct Config;
impl Extension for Config {
    fn name(&self) -> &'static str {
        "config"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.global_keymap().unbind(State::Normal, "z");
        reg.kind_keymap("text").bind(State::Normal, "gx", "second");
    }
}

fn harness() -> (Harness, BufferId, WindowId, Log) {
    let log = Log::default();
    let toy = Toy { log: log.clone() };
    let mut h = Harness::new(vec![Box::new(toy), Box::new(Config)]).unwrap();
    let (b, w) = h.open_text("t", "abc\n").unwrap();
    (h, b, w, log)
}

fn typed(h: &mut Harness, keys: &str) {
    let log = h.type_keys(keys).unwrap();
    assert!(log.iter().all(|k| k.errors.is_empty()), "{log:?}");
}

fn help(h: &Harness) -> String {
    let b = h.find_buffer("*help*").expect("*help* exists");
    h.text(b).unwrap().rope().to_string()
}

#[test]
fn a_shadowed_binding_is_listed_with_what_hid_it_and_the_key_doesnt_run() {
    let (mut h, _, w, log) = harness();
    typed(&mut h, "hx");
    assert_eq!(
        help(&h),
        "x in Normal runs second, found in global.\n\
         second is from toy.\n\nLog its name.\n\n\
         Shadowed:\n  x to first in global: hidden by second\n"
    );
    assert!(log.borrow().is_empty(), "describing runs nothing");
    assert_eq!(h.focused(), Some(w), "the focus stays where it was");
    assert_eq!(h.window_list().len(), 2, "*help* has a window");

    typed(&mut h, "x");
    assert_eq!(*log.borrow(), ["second"], "keys are back to normal after");
}

#[test]
fn it_follows_the_lookup_order_and_lists_the_keymaps_below() {
    let (mut h, b, w, _) = harness();
    let shadowed = |d: &KeyDescription| -> Vec<(String, String)> {
        let s = d.shadowed.iter();
        s.map(|(map, s)| (map.clone(), s.cmd.clone())).collect()
    };
    let keymap = |d: &KeyDescription| match &d.action {
        KeyAction::Command { keymap, name, .. } => format!("{keymap}: {name}"),
        other => panic!("{other:?}"),
    };
    let y = parse_keys("y").unwrap();

    let d = h.describe_key(&y).unwrap();
    assert_eq!(keymap(&d), "kind text: in-kind");
    let below = vec![("global".to_owned(), "in-global".to_owned())];
    assert_eq!(shadowed(&d), below);

    h.enable_mode::<Fold>(b).unwrap();
    h.run_until_idle();
    let d = h.describe_key(&y).unwrap();
    assert_eq!(keymap(&d), "mode fold: in-mode");
    assert_eq!(shadowed(&d)[0].0, "kind text");
    assert_eq!(shadowed(&d).len(), 2);

    let t = h.text_mut(b).unwrap();
    t.decorate(0..2, Deco::Keymap("button".into())).unwrap();
    t.set_cursor(w, 0);
    let d = h.describe_key(&y).unwrap();
    assert_eq!(keymap(&d), "keymap button: in-range");
    assert_eq!(shadowed(&d).len(), 3);
    assert_eq!(h.state(), State::Normal);

    h.set_state(State::Insert);
    let d = h.describe_key(&y).unwrap();
    assert_eq!(
        keymap(&d),
        "global: in-global",
        "the state chooses the bindings"
    );
}

#[test]
fn a_sequence_is_read_whole_and_an_undefined_one_is_said_to_be() {
    let (mut h, _, _, log) = harness();
    typed(&mut h, "hg");
    assert_eq!(h.find_buffer("*help*"), None, "g is only a prefix");
    typed(&mut h, "x");
    assert!(help(&h).starts_with("gx in Normal runs second, found in kind text."));
    assert!(log.borrow().is_empty());

    // An undefined key is an answer here, so it clears nothing and fails nothing.
    h.set_prefix(Prefix {
        count: Some(3),
        register: None,
    });
    typed(&mut h, "hq");
    assert_eq!(help(&h), "q in Normal is undefined.\n");
    assert_eq!(h.prefix().count, None, "`h` took the count, as any command");
    assert_eq!(h.window_list().len(), 2, "*help* keeps its one window");
}

#[test]
fn a_key_your_config_unbound_is_listed_as_unbound() {
    let (mut h, ..) = harness();
    typed(&mut h, "hz");
    assert_eq!(
        help(&h),
        "z in Normal is undefined.\n\n\
         Shadowed:\n  z to in-global in global: unbound\n"
    );
}

/// Doesn't declare `q`; declares every other key.
struct Declares(TermSurface);
impl Surface for Declares {
    fn kind(&self) -> &'static str {
        "term"
    }
    fn render(&mut self, cols: usize, rows: usize, out: &mut Vec<String>) {
        self.0.render(cols, rows, out);
    }
    fn event(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        self.0.event(ev, ctx)
    }
    fn handles(&self, key: &KeyEvent) -> bool {
        key.printable().as_deref() != Some("q")
    }
    fn initial_state(&self) -> State {
        State::Passthrough
    }
}

#[test]
fn in_passthrough_it_reports_the_surface_and_keeps_the_key_from_it() {
    let (mut h, ..) = harness();
    let events: Rc<RefCell<Vec<Event>>> = Default::default();
    let term = TermSurface {
        events: events.clone(),
    };
    let b = h.create_surface("term", Declares(term));
    let w = h.show(b, ShowIn::Current).unwrap().unwrap();
    h.run_until_idle();
    events.borrow_mut().clear();

    h.run("describe-key", &Args::default()).unwrap();
    typed(&mut h, "a");
    assert_eq!(
        help(&h),
        "a in Passthrough goes to the term surface, which handles it.\n"
    );
    assert_eq!(h.focused(), Some(w));
    let ev = KeyEvent::press(Key::Char('a'));
    h.send(Input::Key(KeyEvent {
        kind: KeyKind::Release,
        ..ev
    }));
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|e| matches!(e, Event::Key(_)))
            .count(),
        0,
        "the program sees neither the press nor its release"
    );

    h.run("describe-key", &Args::default()).unwrap();
    typed(&mut h, "q");
    assert!(help(&h).contains("doesn't declare it"), "{}", help(&h));

    let esc = KeyEvent {
        mods: Mods {
            super_: true,
            ..Mods::default()
        },
        ..KeyEvent::press(Key::Esc)
    };
    h.run("describe-key", &Args::default()).unwrap();
    h.send(Input::Key(esc));
    assert_eq!(help(&h), "<D-Esc> leaves passthrough.\n");
    assert_eq!(h.state(), State::Passthrough);
}

#[test]
fn describe_command_shows_doc_owner_and_every_key_bound_in_force() {
    let (mut h, ..) = harness();
    typed(&mut h, "Hfirst<CR>");
    assert_eq!(
        help(&h),
        "first is from toy.\n\nLog its name.\n\n\
         gg in Normal, in global\n"
    );
    typed(&mut h, "Hin-global<CR>");
    assert_eq!(
        help(&h),
        "in-global is from toy.\n\nLog its name.\n\n\
         y in Normal, in global\n\
         y in Insert, in global\n"
    );
    let core = h.describe_command("command-palette").unwrap();
    assert_eq!((core.owner, core.keys.len()), ("core", 0));
    assert!(core.to_string().contains("No key is bound to it."));
    assert_eq!(h.describe_command("nope"), None);
}
