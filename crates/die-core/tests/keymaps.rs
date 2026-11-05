//! K3: keymap registration. A binding that is also a prefix is an error, a later binding
//! for the same keys shadows the earlier one, and the leader comes from config.

use die_test_cap as _;

mod support;
use die_core::*;
use std::cell::RefCell;
use std::rc::Rc;
use support::TermSurface;

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
    for (bind, a, b) in [
        (
            (|r: &mut Registry| {
                r.kind_keymap("text")
                    .bind(State::Normal, "g", "a")
                    .bind(State::Normal, "gg", "b");
            }) as fn(&mut Registry),
            "g",
            "gg",
        ),
        (
            |r| {
                r.kind_keymap("text")
                    .bind(State::Normal, "gg", "b")
                    .bind(State::Normal, "ge", "c")
                    .bind(State::Normal, "g", "a");
            },
            "g",
            "gg",
        ),
        (
            |r| {
                r.global_keymap().bind(State::Normal, "<char>", "a").bind(
                    State::Normal,
                    "<char>x",
                    "b",
                );
            },
            "<char>",
            "<char>x",
        ),
    ] {
        let errs = errors(vec![ext(&log, &["a", "b", "c"], bind)]);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("prefix"), "{errs:?}");
        for named in [format!("{a:?} (bound to a)"), format!("{b:?} (bound to b)")] {
            assert!(errs[0].contains(&named), "names both bindings: {errs:?}");
        }
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
            s[0].by.as_deref()
        ),
        (State::Normal, "x", "first", Some("second"))
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
fn the_leader_applies_to_every_keymap_and_the_last_one_set_wins() {
    let log = Log::default();
    let tool = ext(&log, &["global", "kind", "mode", "range"], |r| {
        r.set_leader(",");
        r.global_keymap().bind(State::Normal, "<leader>a", "global");
        r.kind_keymap("text")
            .bind(State::Normal, "<leader>b", "kind");
        r.mode::<Plain>("plain", || Plain)
            .bind(State::Normal, "<leader>c", "mode");
        r.keymap("button").bind(State::Normal, "<leader>d", "range");
    });
    let mut ctx = Ctx::new(vec![tool, Box::new(LeaderIs("<Space>"))]).unwrap();
    let b = ctx.create_text("t", "abc\n");
    ctx.open_window(b, 40, 5).unwrap();
    ctx.enable_mode::<Plain>(b).unwrap();
    ctx.text_mut(b)
        .unwrap()
        .decorate(0..2, Deco::Keymap("button".into()))
        .unwrap();
    ctx.type_keys(" a b c d,a");
    assert_eq!(*log.borrow(), ["global", "kind", "mode", "range"]);
    assert_eq!(ctx.leader(), parse_keys(" ").unwrap()[0].chord());
}

struct Plain;
impl Mode for Plain {}

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
    for bad in ["ab", "", "<char>", "<leader>", "<Shift>"] {
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
fn modifier_keys_alone_reach_only_passthrough() {
    let log = Log::default();
    let any = ext(&log, &["cancel"], |r| {
        r.kind_keymap("text").bind(State::Normal, "<any>", "cancel");
    });
    let mut ctx = typing(vec![any], "");
    let observed = Rc::new(RefCell::new(vec![]));
    let o = observed.clone();
    let _sub = ctx.observe_keys(move |ev, _| o.borrow_mut().push(ev.clone()));
    let shift = KeyEvent::press(Key::Modifier(ModifierKey::Shift));
    for m in [ModifierKey::Control, ModifierKey::CapsLock] {
        ctx.input(KeyEvent::press(Key::Modifier(m)));
    }
    ctx.input(shift.clone());
    ctx.run_until_idle();
    assert!(
        log.borrow().is_empty(),
        "Shift on its way to `$` doesn't cancel `d`"
    );
    assert!(
        observed.borrow().is_empty(),
        "nor is it recorded in a macro"
    );
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);

    let events = Rc::new(RefCell::new(vec![]));
    let t = ctx.create_surface(
        "term",
        TermSurface {
            events: events.clone(),
        },
    );
    ctx.open_window(t, 40, 5).unwrap();
    ctx.input(shift.clone());
    ctx.input(shift.clone().kind(KeyKind::Release));
    ctx.run_until_idle();
    assert_eq!(
        *events.borrow(),
        [
            Event::Key(shift.clone()),
            Event::Key(shift.kind(KeyKind::Release))
        ],
        "the terminal reports them"
    );
}

#[test]
fn bindings_that_could_never_match_are_errors() {
    let log = Log::default();
    for bind in [
        (|r: &mut Registry| {
            r.global_keymap().bind(State::Normal, "", "a");
        }) as fn(&mut Registry),
        |r| {
            r.global_keymap().bind(State::Normal, "<Shift>", "a");
        },
        |r| {
            r.global_keymap().bind(State::Passthrough, "<D-Super>", "a");
        },
        |r| {
            r.global_keymap().unbind(State::Normal, "zz");
        },
    ] {
        let errs = errors(vec![ext(&log, &["a"], bind)]);
        assert_eq!(errs.len(), 1, "{errs:?}");
    }
    let errs = errors(vec![ext(&log, &["a", "b"], |r| {
        r.global_keymap()
            .bind(State::Normal, "", "a")
            .bind(State::Normal, "x", "b");
    })]);
    assert_eq!(errs.len(), 1, "an empty binding isn't a prefix: {errs:?}");
    assert!(errs[0].contains("no keys"), "{errs:?}");
}

#[test]
fn your_config_unbinds_a_key_to_free_it() {
    let log = Log::default();
    // die-vim binds `\` alone and `g` as a prefix; your config wants `\f` and `g` alone.
    let vim = || {
        ext(
            &log,
            &["emacs-state", "top", "end", "find-file", "g"],
            |r| {
                r.kind_keymap("text")
                    .bind(State::Normal, "\\", "emacs-state")
                    .bind(State::Normal, "gg", "top")
                    .bind(State::Normal, "ge", "end");
            },
        )
    };
    let errs = errors(vec![
        vim(),
        Box::new(Config(|r| {
            r.kind_keymap("text")
                .bind(State::Normal, "<leader>f", "find-file")
                .bind(State::Normal, "g", "g");
        })),
    ]);
    assert_eq!(errs.len(), 2, "{errs:?}");
    let config = Config(|r| {
        r.kind_keymap("text")
            .unbind(State::Normal, "\\")
            .bind(State::Normal, "<leader>f", "find-file")
            .unbind(State::Normal, "g")
            .bind(State::Normal, "g", "g");
    });
    typing(vec![vim(), Box::new(config)], "\\fg");
    assert_eq!(*log.borrow(), ["find-file", "g"]);
}

/// Your config, registered last.
struct Config(fn(&mut Registry));
impl Extension for Config {
    fn name(&self) -> &'static str {
        "config"
    }
    fn register(&mut self, reg: &mut Registry) {
        (self.0)(reg);
    }
}

#[test]
fn unbinding_removes_longer_bindings_and_lists_them() {
    let mut km = Keymap::default();
    km.bind(State::Normal, "gg", "top")
        .bind(State::Normal, "ge", "end")
        .bind(State::Normal, "<any>", "cancel")
        .unbind(State::Normal, "gg");
    let g = parse_keys("g").unwrap();
    assert_eq!(km.lookup(State::Normal, &g), Lookup::Prefix, "ge is left");
    km.unbind(State::Normal, "g");
    assert_eq!(
        km.lookup(State::Normal, &g),
        Lookup::Command("cancel".into()),
        "g isn't a prefix any more"
    );
    let gone: Vec<_> = km
        .shadowed()
        .iter()
        .map(|s| (s.keys.as_str(), s.cmd.as_str(), s.by.clone()))
        .collect();
    assert_eq!(gone, [("gg", "top", None), ("ge", "end", None)]);
    assert_eq!(km.bindings().count(), 3, "unbinding keeps the record");
}

#[test]
fn os_fields_dont_change_what_a_key_matches() {
    let log = Log::default();
    let e = ext(&log, &["c-a", "c-s-a"], |r| {
        r.kind_keymap("text")
            .bind(State::Normal, "<C-a>", "c-a")
            .bind(State::Normal, "<C-S-a>", "c-s-a");
    });
    let mut ctx = typing(vec![e], "");
    let ctrl = |c, shift| {
        KeyEvent::press(Key::Char(c)).mods(Mods {
            ctrl: true,
            shift,
            ..Mods::default()
        })
    };
    let mut ev = ctrl('a', false);
    ev.right_mods.ctrl = true;
    ev.code = Some("KeyA".into());
    ev.unshifted = Some('a');
    ctx.input(ev);
    // Caps Lock gives a capital, but Ctrl-a is still `<C-a>`.
    let mut caps = ctrl('A', false);
    caps.caps_lock = true;
    ctx.input(caps);
    // Ctrl-Shift-a is `<C-S-a>`, whether or not the OS applied Shift to the letter.
    ctx.input(ctrl('A', true));
    ctx.input(ctrl('a', true));
    ctx.run_until_idle();
    assert_eq!(*log.borrow(), ["c-a", "c-a", "c-s-a", "c-s-a"]);
}

/// Emacs's `undefined` resets the prefix argument, so a typo can't arm the next command.
#[test]
fn an_undefined_key_clears_the_prefix_argument() {
    let log = Log::default();
    let bind = |r: &mut Registry| {
        r.kind_keymap("text").bind(State::Normal, "ga", "a");
    };
    let mut ctx = typing(vec![ext(&log, &["a"], bind)], "");
    ctx.set_prefix(Prefix {
        count: Some(3),
        register: Some('x'),
    });
    ctx.type_keys("gz");
    assert_eq!(ctx.prefix(), Prefix::default());
    assert_eq!(ctx.messages, ["gz is undefined"]);
    // A prefix of a longer binding isn't undefined.
    ctx.set_prefix(Prefix {
        count: Some(3),
        register: None,
    });
    ctx.type_keys("g");
    assert_eq!(ctx.prefix().count, Some(3));
}

fn plain_command(ctx: &mut Ctx, keys: &[KeyEvent]) {
    for k in keys {
        ctx.input(k.clone());
    }
    ctx.run_until_idle();
}

/// A key whose OS text is a control character doesn't match `<char>`, or type it.
#[test]
fn a_ctrl_chord_with_os_text_doesnt_match_char() {
    let log = Log::default();
    let bind = |r: &mut Registry| {
        r.kind_keymap("text").bind(State::Normal, "<char>", "a");
    };
    let mut ctx = typing(vec![ext(&log, &["a"], bind)], "");
    let ctrl_a = KeyEvent {
        text: Some("\u{1}".into()),
        ..KeyEvent::press(Key::Char('a')).mods(Mods {
            ctrl: true,
            ..Mods::default()
        })
    };
    plain_command(&mut ctx, &[ctrl_a]);
    assert!(log.borrow().is_empty());
    assert_eq!(ctx.messages, ["<C-a> is undefined"]);
}

#[test]
fn ctrl_bracket_runs_what_escape_is_bound_to() {
    let log = Log::default();
    let bind = |r: &mut Registry| {
        r.kind_keymap("text")
            .bind(State::Normal, "<Esc>", "a")
            .bind(State::Normal, "<C-i>", "b")
            .bind(State::Normal, "<C-m>", "c");
    };
    let ctx = typing(
        vec![ext(&log, &["a", "b", "c"], bind)],
        "<C-[><Tab><CR><Esc><C-i><C-m>",
    );
    assert_eq!(*log.borrow(), ["a", "b", "c", "a", "b", "c"]);
    assert_eq!(ctx.messages, Vec::<String>::new());
}

/// As the OS sends them: `[` and `m` with Ctrl held. Notation has already folded them.
#[test]
fn ctrl_bracket_and_ctrl_m_answer_a_prompt() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", "");
    ctx.open_window(b, 40, 5).unwrap();
    let held = Mods {
        ctrl: true,
        ..Mods::default()
    };
    let answers = Rc::new(RefCell::new(vec![]));
    for key in ['m', '['] {
        let a = answers.clone();
        ctx.prompt("?", move |_, answer| {
            a.borrow_mut().push(answer);
            Ok(())
        });
        ctx.type_keys("ab");
        plain_command(&mut ctx, &[KeyEvent::press(Key::Char(key)).mods(held)]);
    }
    assert_eq!(*answers.borrow(), [Some("ab".to_string()), None]);
}

/// Keymaps never see a key with no name, not even through `<any>`. Passthrough forwards it.
#[test]
fn an_unnamed_key_never_reaches_a_keymap() {
    let log = Log::default();
    let bind = |r: &mut Registry| {
        r.kind_keymap("text").bind(State::Normal, "<any>", "a");
    };
    let mut ctx = typing(vec![ext(&log, &["a"], bind)], "");
    plain_command(&mut ctx, &[KeyEvent::press(Key::Unidentified)]);
    assert!(log.borrow().is_empty());
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);

    let mut ctx = Ctx::new(vec![]).unwrap();
    let events = Rc::new(RefCell::new(vec![]));
    let s = TermSurface {
        events: events.clone(),
    };
    let b = ctx.create_surface("term", s);
    ctx.open_window(b, 40, 5).unwrap();
    assert_eq!(ctx.state(), State::Passthrough);
    plain_command(&mut ctx, &[KeyEvent::press(Key::Unidentified)]);
    assert_eq!(events.borrow().len(), 1, "passthrough forwards it");
}

/// A surface in passthrough with `<any>` bound, as the DIE layer can bind it: it takes a key
/// with Super held from the surface, unless the key is one keymaps never see.
#[test]
fn with_super_held_passthrough_keeps_keys_keymaps_never_see() {
    let log = Log::default();
    let bind = |r: &mut Registry| {
        r.kind_keymap("term").bind(State::Passthrough, "<any>", "a");
    };
    let mut ctx = Ctx::new(vec![ext(&log, &["a"], bind)]).unwrap();
    let events = Rc::new(RefCell::new(vec![]));
    let s = TermSurface {
        events: events.clone(),
    };
    let b = ctx.create_surface("term", s);
    ctx.open_window(b, 40, 5).unwrap();
    let held = Mods {
        super_: true,
        ..Mods::default()
    };
    for (i, key) in [Key::Unidentified, Key::Modifier(ModifierKey::Super)]
        .into_iter()
        .enumerate()
    {
        plain_command(&mut ctx, &[KeyEvent::press(key).mods(held)]);
        assert!(log.borrow().is_empty(), "{key:?} reached a keymap");
        assert_eq!(events.borrow().len(), i + 1, "{key:?} went to the surface");
    }
    plain_command(&mut ctx, &[KeyEvent::press(Key::Char('a')).mods(held)]);
    assert_eq!(*log.borrow(), ["a"], "a named key still does");
}

#[test]
fn an_empty_commit_doesnt_run_a_space_binding() {
    let log = Log::default();
    let bind = |r: &mut Registry| {
        r.kind_keymap("text").bind(State::Normal, "<Space>", "a");
    };
    let mut ctx = typing(vec![ext(&log, &["a"], bind)], "");
    plain_command(&mut ctx, &[KeyEvent::committed("")]);
    assert!(log.borrow().is_empty());
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

/// `🇦🇺` as a binding could never fire, and typing it would arm a prefix that takes the next key.
#[test]
fn binding_a_cluster_of_several_characters_is_a_startup_error() {
    let log = Log::default();
    let bind = |r: &mut Registry| {
        r.kind_keymap("text")
            .bind(State::Normal, "\u{1f1e6}\u{1f1fa}", "a");
    };
    let errs = errors(vec![ext(&log, &["a"], bind)]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("<char>"), "{errs:?}");
}
