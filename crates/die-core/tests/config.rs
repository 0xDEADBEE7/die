//! K5: the config step (contract §4, step 3, and C8): applied after every extension has registered
//! and before any extension inits.
//!
//! - `the_steps_run_in_the_contracts_order`: register, config, init, then the startup hook.
//! - `config_bindings_win_in_the_keymap_they_name`: global and kind keymaps.
//! - `config_binds_in_a_modes_keymap`: `Registry::mode_keymap`, and None for no such mode.
//! - `config_frees_a_key_an_extension_uses_as_a_prefix`: unbind, then bind it alone (§7).
//! - `a_prefix_conflict_with_an_extension_is_still_a_startup_error`: bind without the unbind.
//! - `config_commands_belong_to_config`: `command_doc` and a panic's attribution.
//! - `a_command_name_config_takes_from_an_extension_is_a_startup_error`: C8's rule on duplicates.
//! - `config_can_declare_faces`: a theme can style them, with no message.
//! - `every_missing_name_is_reported_at_once_and_nothing_inits`: the extension's and config's.
//! - `the_leader_config_sets_applies_to_every_keymap`: including what extensions bound first.
//! - `a_panic_in_the_config_step_is_a_startup_error`: with its message, and nothing inits.
//! - `the_theme_applies_before_init`: init reads it, and it can style an extension's face.
//! - `a_theme_that_styles_an_undeclared_face_is_reported`: through the same check as `set_theme`.
//! - `buffer_defaults_apply_to_every_new_text_buffer`: from init on.
//! - `file_settings_start_from_the_buffer_defaults`: settings by path (K10a-8), and their panic.
//! - `the_startup_hook_can_use_the_editor`: it makes a buffer and queues a call, not nested.
//! - `an_error_or_panic_in_the_startup_hook_is_logged`: the editor still starts.
//!
//! Shutdown (step 8):
//! - `extensions_shut_down_in_reverse_order`.
//! - `shutdown_cancels_what_config_detached`: its timers, idle timers and hooks.
//! - `shutdown_cancels_what_config_sets_during_it`: an extension's `shutdown` running config's
//!   command, so config's handles go last.

use die_test_cap as _;

use die_core::*;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

type Log = Rc<RefCell<Vec<String>>>;

struct Ext<const N: usize>(Log);
impl<const N: usize> Extension for Ext<N> {
    fn name(&self) -> &'static str {
        ["a", "b"][N]
    }
    fn register(&mut self, reg: &mut Registry) {
        self.0
            .borrow_mut()
            .push(format!("register {}", self.name()));
        if N == 0 {
            let log = self.0.clone();
            reg.command("a-cmd", "", move |_, _| {
                log.borrow_mut().push("a-cmd".into());
                Ok(())
            });
            reg.global_keymap().bind(State::Normal, "x", "a-cmd");
            reg.kind_keymap("text").bind(State::Normal, "y", "a-cmd");
            reg.kind_keymap("text").bind(State::Normal, "gg", "a-cmd");
            reg.kind_keymap("text")
                .bind(State::Normal, "<leader>l", "a-cmd");
            reg.mode::<M>("m", || M).bind(State::Normal, "w", "a-cmd");
            reg.face("a.face", None, Style::new());
        }
    }
    fn init(&mut self, ctx: &mut Ctx) -> Result {
        let line = format!("init {}, theme {}", self.name(), ctx.theme().name);
        self.0.borrow_mut().push(line);
        Ok(())
    }
}

struct M;
impl Mode for M {}

fn exts(log: &Log) -> Vec<Box<dyn Extension>> {
    vec![
        Box::new(Ext::<0>(log.clone())),
        Box::new(Ext::<1>(log.clone())),
    ]
}

/// A config that also registers `config-cmd`, which notes itself in the log.
fn config(log: &Log, f: impl FnOnce(&mut Registry) + 'static) -> Config {
    let log = log.clone();
    Config {
        register: Some(Box::new(move |reg: &mut Registry| {
            reg.command("config-cmd", "from your config", move |_, _| {
                log.borrow_mut().push("config-cmd".into());
                Ok(())
            });
            f(reg);
        })),
        ..Default::default()
    }
}

fn window(ctx: &mut Ctx) -> BufferId {
    let b = ctx.create_text("t", "text");
    ctx.open_window(b, 80, 24).unwrap();
    b
}

fn dark_theme() -> Theme {
    Theme::new("dark", Theme::default().ansi)
}

#[test]
fn the_steps_run_in_the_contracts_order() {
    let log: Log = Rc::default();
    let (l1, l2) = (log.clone(), log.clone());
    let cfg = Config {
        register: Some(Box::new(move |_| l1.borrow_mut().push("config".into()))),
        startup: Some(Box::new(move |_| {
            l2.borrow_mut().push("startup".into());
            Ok(())
        })),
        ..Default::default()
    };
    Ctx::with_config(exts(&log), cfg).unwrap();
    assert_eq!(
        *log.borrow(),
        [
            "register a",
            "register b",
            "config",
            "init a, theme default",
            "init b, theme default",
            "startup"
        ]
    );
}

#[test]
fn config_bindings_win_in_the_keymap_they_name() {
    let log: Log = Rc::default();
    let cfg = config(&log, |reg| {
        reg.global_keymap().bind(State::Normal, "x", "config-cmd");
        reg.kind_keymap("text")
            .bind(State::Normal, "y", "config-cmd");
    });
    let mut ctx = Ctx::with_config(exts(&log), cfg).unwrap();
    window(&mut ctx);
    log.borrow_mut().clear();
    ctx.type_keys("xy");
    assert_eq!(*log.borrow(), ["config-cmd", "config-cmd"]);
}

#[test]
fn config_binds_in_a_modes_keymap() {
    let log: Log = Rc::default();
    let cfg = config(&log, |reg| {
        reg.mode_keymap("m")
            .unwrap()
            .bind(State::Normal, "w", "config-cmd");
        assert!(reg.mode_keymap("no-such-mode").is_none());
    });
    let mut ctx = Ctx::with_config(exts(&log), cfg).unwrap();
    let b = window(&mut ctx);
    ctx.enable_mode::<M>(b).unwrap();
    log.borrow_mut().clear();
    ctx.type_keys("w");
    assert_eq!(*log.borrow(), ["config-cmd"]);
}

#[test]
fn config_frees_a_key_an_extension_uses_as_a_prefix() {
    let log: Log = Rc::default();
    let cfg = config(&log, |reg| {
        reg.kind_keymap("text")
            .unbind(State::Normal, "g")
            .bind(State::Normal, "g", "config-cmd");
    });
    let mut ctx = Ctx::with_config(exts(&log), cfg).unwrap();
    window(&mut ctx);
    log.borrow_mut().clear();
    ctx.type_keys("g");
    assert_eq!(*log.borrow(), ["config-cmd"]);
}

#[test]
fn a_prefix_conflict_with_an_extension_is_still_a_startup_error() {
    let log: Log = Rc::default();
    let cfg = config(&log, |reg| {
        reg.kind_keymap("text")
            .bind(State::Normal, "g", "config-cmd");
    });
    let errs = Ctx::with_config(exts(&log), cfg).err().unwrap();
    assert!(errs.iter().any(|e| e.contains("prefix")), "{errs:?}");
}

#[test]
fn config_commands_belong_to_config() {
    let log: Log = Rc::default();
    let cfg = config(&log, |reg| {
        reg.command("config-boom", "", |_, _| panic!("config bug"))
    });
    let mut ctx = Ctx::with_config(exts(&log), cfg).unwrap();
    assert_eq!(
        ctx.command_doc("config-cmd"),
        Some(("from your config", "config"))
    );
    assert_eq!(ctx.command_doc("a-cmd").map(|d| d.1), Some("a"));
    let r = ctx.run("config-boom", &Args::default());
    assert!(
        matches!(r, Err(Error::Panicked { ext: "config", .. })),
        "{r:?}"
    );
}

#[test]
fn a_command_name_config_takes_from_an_extension_is_a_startup_error() {
    let log: Log = Rc::default();
    let cfg = config(&log, |reg| reg.command("a-cmd", "", |_, _| Ok(())));
    let errs = Ctx::with_config(exts(&log), cfg).err().unwrap();
    assert_eq!(errs, ["command a-cmd registered twice"]);
}

#[test]
fn config_can_declare_faces() {
    let log: Log = Rc::default();
    let red = Rgb::hex("#ff0000");
    let mut cfg = config(&log, |reg| {
        reg.face("my.face", None, Style::new());
    });
    cfg.theme = Some(dark_theme().face("my.face", Style::new().fg(red)));
    let ctx = Ctx::with_config(exts(&log), cfg).unwrap();
    assert_eq!(ctx.face_style("my.face").unwrap().fg, Some(red));
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
}

#[test]
fn every_missing_name_is_reported_at_once_and_nothing_inits() {
    struct Binds;
    impl Extension for Binds {
        fn name(&self) -> &'static str {
            "binds"
        }
        fn register(&mut self, reg: &mut Registry) {
            reg.global_keymap()
                .bind(State::Normal, "q", "from-the-extension");
        }
        fn init(&mut self, _: &mut Ctx) -> Result {
            panic!("init must not run");
        }
    }
    let cfg = Config {
        register: Some(Box::new(|reg: &mut Registry| {
            reg.global_keymap().bind(State::Normal, "z", "from-config");
        })),
        ..Default::default()
    };
    let errs = Ctx::with_config(vec![Box::new(Binds)], cfg).err().unwrap();
    assert_eq!(errs.len(), 2, "{errs:?}");
    assert!(
        errs.iter().any(|e| e.contains("from-the-extension")),
        "{errs:?}"
    );
    assert!(errs.iter().any(|e| e.contains("from-config")), "{errs:?}");
}

#[test]
fn the_leader_config_sets_applies_to_every_keymap() {
    let log: Log = Rc::default();
    let cfg = config(&log, |reg| reg.set_leader(","));
    let mut ctx = Ctx::with_config(exts(&log), cfg).unwrap();
    window(&mut ctx);
    log.borrow_mut().clear();
    ctx.type_keys(",l");
    assert_eq!(*log.borrow(), ["a-cmd"]);
    assert_eq!(ctx.leader(), KeyEvent::press(Key::Char(',')).chord());
}

#[test]
fn a_panic_in_the_config_step_is_a_startup_error() {
    let log: Log = Rc::default();
    let cfg = Config {
        register: Some(Box::new(|_| panic!("config bug"))),
        ..Default::default()
    };
    let errs = Ctx::with_config(exts(&log), cfg).err().unwrap();
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].starts_with("config panicked"), "{errs:?}");
    assert!(errs[0].contains("config bug"), "{errs:?}");
    assert!(log.borrow().iter().all(|l| !l.starts_with("init")));
}

#[test]
fn the_theme_applies_before_init() {
    let log: Log = Rc::default();
    let red = Rgb::hex("#ff0000");
    let cfg = Config {
        theme: Some(dark_theme().face("a.face", Style::new().fg(red))),
        ..Default::default()
    };
    let ctx = Ctx::with_config(exts(&log), cfg).unwrap();
    assert!(log.borrow().contains(&"init a, theme dark".to_string()));
    assert_eq!(ctx.face_style("a.face").unwrap().fg, Some(red));
    assert!(
        ctx.messages.is_empty(),
        "a.face is declared: {:?}",
        ctx.messages
    );
}

#[test]
fn a_theme_that_styles_an_undeclared_face_is_reported() {
    let log: Log = Rc::default();
    let cfg = Config {
        theme: Some(dark_theme().face("no.such.face", Style::new())),
        ..Default::default()
    };
    let ctx = Ctx::with_config(exts(&log), cfg).unwrap();
    assert_eq!(ctx.messages.len(), 1, "{:?}", ctx.messages);
    assert!(
        ctx.messages[0].contains("no.such.face"),
        "{:?}",
        ctx.messages
    );
}

fn defaults() -> Settings {
    Settings {
        tab_width: 3,
        ..Default::default()
    }
}

#[test]
fn buffer_defaults_apply_to_every_new_text_buffer() {
    struct Reads(Rc<RefCell<Option<usize>>>);
    impl Extension for Reads {
        fn name(&self) -> &'static str {
            "reads"
        }
        fn register(&mut self, _: &mut Registry) {}
        fn init(&mut self, ctx: &mut Ctx) -> Result {
            let b = ctx.create_text("from-init", "");
            *self.0.borrow_mut() = Some(ctx.text(b)?.settings().tab_width);
            Ok(())
        }
    }
    let seen = Rc::default();
    let cfg = Config {
        text_defaults: defaults(),
        ..Default::default()
    };
    let mut ctx = Ctx::with_config(vec![Box::new(Reads(Rc::clone(&seen)))], cfg).unwrap();
    assert_eq!(*seen.borrow(), Some(3));
    let b = ctx.create_text("later", "");
    assert_eq!(ctx.text(b).unwrap().settings().tab_width, 3);
    let mut plain = Ctx::new(vec![]).unwrap();
    let b = plain.create_text("t", "");
    assert_eq!(
        plain.text(b).unwrap().settings().tab_width,
        Settings::default().tab_width,
        "no config, no change"
    );
}

struct TempFile(std::path::PathBuf);
impl TempFile {
    fn new(name: &str) -> TempFile {
        let dir = std::env::temp_dir().join(format!("die-k5-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, "text\n").unwrap();
        TempFile(path)
    }
}
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn file_settings_start_from_the_buffer_defaults() {
    let (rs, txt) = (TempFile::new("a.rs"), TempFile::new("b.txt"));
    let cfg = Config {
        text_defaults: defaults(),
        file_settings: Some(Box::new(|path: &Path, s: &mut Settings| {
            assert_eq!(s.tab_width, 3, "it starts from the defaults");
            if path.extension().is_some_and(|e| e == "rs") {
                s.max_width = Some(80);
            }
        })),
        ..Default::default()
    };
    let mut ctx = Ctx::with_config(vec![], cfg).unwrap();
    let a = ctx.open_file(&rs.0).unwrap();
    let b = ctx.open_file(&txt.0).unwrap();
    let settings = |ctx: &Ctx, b| ctx.text(b).unwrap().settings().clone();
    assert_eq!(settings(&ctx, a).max_width, Some(80));
    assert_eq!(settings(&ctx, a).tab_width, 3);
    assert_eq!(settings(&ctx, b).max_width, None);
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);

    // A panic in it opens the file with the defaults: your config's.
    let bad = TempFile::new("c.rs");
    let cfg = Config {
        text_defaults: defaults(),
        file_settings: Some(Box::new(|_: &Path, s: &mut Settings| {
            s.tab_width = 9;
            panic!("settings bug");
        })),
        ..Default::default()
    };
    let mut ctx = Ctx::with_config(vec![], cfg).unwrap();
    let c = ctx.open_file(&bad.0).unwrap();
    assert_eq!(settings(&ctx, c).tab_width, 3);
    assert!(
        ctx.messages[0].contains("settings bug"),
        "{:?}",
        ctx.messages
    );
    ctx.run_until_idle();
    assert_eq!(ctx.messages.len(), 1, "logged once: {:?}", ctx.messages);
}

#[test]
fn the_startup_hook_can_use_the_editor() {
    let log: Log = Rc::default();
    let l = log.clone();
    let cfg = Config {
        startup: Some(Box::new(move |ctx: &mut Ctx| {
            let depth = ctx.nesting();
            l.borrow_mut().push(format!("startup@{depth}"));
            let b = ctx.create_text("scratch", "");
            ctx.open_window(b, 80, 24)?;
            let l = l.clone();
            ctx.defer(move |ctx| {
                l.borrow_mut().push(format!("queued@{}", ctx.nesting()));
                Ok(())
            });
            Ok(())
        })),
        ..Default::default()
    };
    let mut ctx = Ctx::with_config(vec![], cfg).unwrap();
    assert_eq!(
        *log.borrow(),
        ["startup@1"],
        "queued calls wait for the loop"
    );
    assert!(ctx.find_buffer("scratch").is_some());
    ctx.run_until_idle();
    assert_eq!(*log.borrow(), ["startup@1", "queued@1"]);
}

#[test]
fn an_error_or_panic_in_the_startup_hook_is_logged() {
    let cfg = Config {
        startup: Some(Box::new(|_| Err(Error::Msg("startup failed".into())))),
        ..Default::default()
    };
    let ctx = Ctx::with_config(vec![], cfg).unwrap();
    assert!(
        ctx.messages[0].contains("startup failed"),
        "{:?}",
        ctx.messages
    );

    let cfg = Config {
        startup: Some(Box::new(|_| panic!("startup bug"))),
        ..Default::default()
    };
    let ctx = Ctx::with_config(vec![], cfg).unwrap();
    assert!(
        ctx.messages[0].starts_with("config panicked: startup bug"),
        "{:?}",
        ctx.messages
    );
}

#[test]
fn extensions_shut_down_in_reverse_order() {
    struct Down<const N: usize>(Log);
    impl<const N: usize> Extension for Down<N> {
        fn name(&self) -> &'static str {
            ["a", "b"][N]
        }
        fn register(&mut self, _: &mut Registry) {}
        fn shutdown(&mut self, _: &mut Ctx) {
            self.0.borrow_mut().push(self.name().into());
        }
    }
    let log: Log = Rc::default();
    let exts: Vec<Box<dyn Extension>> = vec![
        Box::new(Down::<0>(log.clone())),
        Box::new(Down::<1>(log.clone())),
    ];
    Ctx::new(exts).unwrap().shutdown();
    assert_eq!(*log.borrow(), ["b", "a"]);
}

#[test]
fn shutdown_cancels_what_config_detached() {
    let log: Log = Rc::default();
    let (l1, l2, l3) = (log.clone(), log.clone(), log.clone());
    let cfg = Config {
        startup: Some(Box::new(move |ctx| {
            ctx.every(Duration::from_millis(10), move |_| {
                l1.borrow_mut().push("timer".into());
                Ok(())
            })
            .detach();
            ctx.idle(Duration::from_millis(10), move |_| {
                l2.borrow_mut().push("idle".into());
                Ok(())
            })
            .detach();
            ctx.on_buffer_killed(move |_, _| {
                l3.borrow_mut().push("hook".into());
                Ok(())
            })
            .detach();
            Ok(())
        })),
        ..Default::default()
    };
    let mut ctx = Ctx::with_config(vec![], cfg).unwrap();
    assert!(ctx.next_timer().is_some());
    ctx.shutdown();
    assert_eq!(ctx.next_timer(), None);
    ctx.advance(Duration::from_millis(30));
    let b = ctx.create_text("t", "text");
    ctx.kill(b);
    ctx.run_until_idle();
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

#[test]
fn shutdown_cancels_what_config_sets_during_it() {
    struct RunsConfig;
    impl Extension for RunsConfig {
        fn name(&self) -> &'static str {
            "runs-config"
        }
        fn register(&mut self, _: &mut Registry) {}
        fn shutdown(&mut self, ctx: &mut Ctx) {
            ctx.run("set-a-timer", &Args::default()).unwrap();
        }
    }
    let cfg = Config {
        register: Some(Box::new(|reg: &mut Registry| {
            reg.command("set-a-timer", "", |ctx, _| {
                ctx.every(Duration::from_millis(10), |_| Ok(())).detach();
                Ok(())
            });
        })),
        ..Default::default()
    };
    let mut ctx = Ctx::with_config(vec![Box::new(RunsConfig)], cfg).unwrap();
    ctx.shutdown();
    assert_eq!(ctx.next_timer(), None);
}
