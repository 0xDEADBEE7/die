//! K3: keymap registration. A binding that is also a prefix is an error, a later binding
//! for the same keys shadows the earlier one, and the leader comes from config.
use die_core::*;
use std::cell::RefCell;
use std::rc::Rc;

type Log = Rc<RefCell<Vec<&'static str>>>;

/// An extension that registers `cmds` (each logging its name) and then runs `bind`.
struct Ext {
    name: &'static str,
    log: Log,
    cmds: &'static [&'static str],
    bind: fn(&mut Registry),
}

impl Extension for Ext {
    fn name(&self) -> &'static str {
        self.name
    }
    fn register(&mut self, reg: &mut Registry) {
        for &c in self.cmds {
            let log = self.log.clone();
            reg.command(c, "", move |_, _| {
                log.borrow_mut().push(c);
                Ok(())
            });
        }
        (self.bind)(reg);
    }
}

fn ext(log: &Log, cmds: &'static [&'static str], bind: fn(&mut Registry)) -> Box<dyn Extension> {
    Box::new(Ext {
        name: "ext",
        log: log.clone(),
        cmds,
        bind,
    })
}

fn errors(exts: Vec<Box<dyn Extension>>) -> Vec<String> {
    match Ctx::new(exts) {
        Ok(_) => vec![],
        Err(e) => e,
    }
}

fn typing(exts: Vec<Box<dyn Extension>>, keys: &str) -> Ctx {
    let mut ctx = Ctx::new(exts).unwrap();
    let b = ctx.create_text("t", "abc\n");
    ctx.open_window(b, 40, 5).unwrap();
    ctx.type_keys(keys);
    ctx
}

#[test]
fn a_binding_that_is_also_a_prefix_is_an_error() {
    let log = Log::default();
    for bind in [
        (|r: &mut Registry| {
            r.kind_keymap("text")
                .bind(State::Normal, "g", "a")
                .bind(State::Normal, "gg", "b");
        }) as fn(&mut Registry),
        |r| {
            r.kind_keymap("text")
                .bind(State::Normal, "gg", "b")
                .bind(State::Normal, "g", "a");
        },
        |r| {
            r.global_keymap().bind(State::Normal, "<char>", "a").bind(
                State::Normal,
                "<char>x",
                "b",
            );
        },
    ] {
        let errs = errors(vec![ext(&log, &["a", "b"], bind)]);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("prefix"), "{errs:?}");
    }
    // Different states, different keymaps, and different edges don't conflict.
    for bind in [
        (|r: &mut Registry| {
            r.kind_keymap("text")
                .bind(State::Normal, "g", "a")
                .bind(State::Visual, "gg", "b");
        }) as fn(&mut Registry),
        |r| {
            r.global_keymap().bind(State::Normal, "g", "a");
            r.kind_keymap("text").bind(State::Normal, "gg", "b");
        },
        |r| {
            r.global_keymap()
                .bind(State::Normal, "x", "a")
                .bind(State::Normal, "<char>y", "b");
        },
    ] {
        assert_eq!(
            errors(vec![ext(&log, &["a", "b"], bind)]),
            Vec::<String>::new()
        );
    }
}

#[test]
fn a_later_binding_for_the_same_keys_shadows_the_earlier() {
    let mut km = Keymap::default();
    km.bind(State::Normal, "x", "first")
        .bind(State::Normal, "x", "second")
        .bind(State::Normal, "<CR>", "enter")
        .bind(State::Normal, "<Enter>", "return")
        .bind(State::Insert, "x", "insert-x");
    let x = parse_keys("x").unwrap();
    assert_eq!(
        km.lookup(State::Normal, &x),
        Lookup::Command("second".into())
    );
    assert_eq!(
        km.lookup(State::Insert, &x),
        Lookup::Command("insert-x".into())
    );
    let s = km.shadowed();
    assert_eq!(s.len(), 2, "{s:?}");
    assert_eq!(
        (
            s[0].state,
            s[0].keys.as_str(),
            s[0].cmd.as_str(),
            s[0].by.as_str()
        ),
        (State::Normal, "x", "first", "second")
    );
    assert_eq!(
        (s[1].keys.as_str(), s[1].cmd.as_str()),
        ("<CR>", "enter"),
        "same key, other name"
    );
}

#[test]
fn the_leader_comes_from_config_registered_last() {
    let log = Log::default();
    let tool = ext(&log, &["find-file"], |r| {
        r.global_keymap()
            .bind(State::Normal, "<leader>f", "find-file");
    });
    let mut ctx = typing(vec![tool, Box::new(LeaderIs("<Space>"))], " f\\f");
    assert_eq!(*log.borrow(), ["find-file"], "only Space-f ran it");
    assert_eq!(ctx.leader(), parse_keys(" ").unwrap()[0].chord());
    ctx.feed_keys("<leader>f").unwrap();
    ctx.run_until_idle();
    assert_eq!(log.borrow().len(), 2, "fed keys resolve the leader too");

    let log = Log::default();
    let tool = ext(&log, &["find-file"], |r| {
        r.global_keymap()
            .bind(State::Normal, "<leader>f", "find-file");
    });
    let ctx = typing(vec![tool], "\\f");
    assert_eq!(*log.borrow(), ["find-file"], "the default leader is \\");
    assert_eq!(ctx.leader(), parse_keys("\\").unwrap()[0].chord());
}

#[test]
fn prefix_conflicts_are_checked_with_the_configured_leader() {
    let log = Log::default();
    let tool = ext(&log, &["a", "b"], |r| {
        r.global_keymap()
            .bind(State::Normal, " ", "a")
            .bind(State::Normal, "<leader>f", "b");
    });
    assert_eq!(
        errors(vec![tool]),
        Vec::<String>::new(),
        "with \\ as leader they don't meet"
    );
    let tool = ext(&log, &["a", "b"], |r| {
        r.global_keymap()
            .bind(State::Normal, " ", "a")
            .bind(State::Normal, "<leader>f", "b");
    });
    let errs = errors(vec![tool, Box::new(LeaderIs("<Space>"))]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("prefix"), "{errs:?}");
}

#[test]
fn a_leader_must_be_one_key() {
    for bad in ["ab", "", "<char>", "<leader>"] {
        let errs = errors(vec![Box::new(LeaderIs(bad))]);
        assert_eq!(errs.len(), 1, "{bad:?}: {errs:?}");
        assert!(errs[0].contains("leader"), "{errs:?}");
    }
}

/// Your config, registered last, setting the leader.
struct LeaderIs(&'static str);
impl Extension for LeaderIs {
    fn name(&self) -> &'static str {
        "leader"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.set_leader(self.0);
    }
}

#[test]
fn modifier_keys_alone_never_reach_keymaps() {
    let log = Log::default();
    let any = ext(&log, &["cancel"], |r| {
        r.kind_keymap("text").bind(State::Normal, "<any>", "cancel");
    });
    let mut ctx = typing(vec![any], "");
    for m in [
        ModifierKey::Shift,
        ModifierKey::Control,
        ModifierKey::CapsLock,
    ] {
        ctx.input(KeyEvent::press(Key::Modifier(m)));
    }
    ctx.run_until_idle();
    assert!(
        log.borrow().is_empty(),
        "Shift on its way to `$` doesn't cancel `d`"
    );
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

#[test]
fn os_fields_dont_change_what_a_key_matches() {
    let log = Log::default();
    let e = ext(&log, &["c-a"], |r| {
        r.kind_keymap("text").bind(State::Normal, "<C-a>", "c-a");
    });
    let mut ctx = typing(vec![e], "");
    let mut ev = KeyEvent::press(Key::Char('a'));
    ev.mods.ctrl = true;
    ev.mods.right.ctrl = true;
    ev.mods.caps_lock = true;
    ev.code = Some("KeyA".into());
    ev.unshifted = Some('a');
    ctx.input(ev);
    ctx.run_until_idle();
    assert_eq!(*log.borrow(), ["c-a"]);
}
