//! What a config built on this one adds through `Extra`: extensions, keys after these, faces in
//! mono-dark, and a greeting, which `greeted` shows on a fresh start with no file named. A fixture
//! extension of the test's own stands in for the owner's die-config, whose own tests hold its
//! extensions to this.

use die_test_cap as _;

use die::app::Launch;
use die::core::{Ctx, Extension, KeyAction, Registry, ShowIn, State, Style, Theme, parse_keys};
use die_dev::{Extra, Greeting, config, config_with, greeted, theme};
use std::path::PathBuf;

/// The fixture's buffer.
const HELLO: &str = "*hello*";

/// An extension with one command, which shows `*hello*` in this window, and one face.
struct Hello;

impl Extension for Hello {
    fn name(&self) -> &'static str {
        "hello"
    }

    fn register(&mut self, reg: &mut Registry) {
        reg.face("hello-face", None, Style::new());
        reg.command("hello", "Show *hello* in this window.", |ctx, _| {
            let buf = match ctx.find_buffer(HELLO) {
                Some(buf) => buf,
                None => ctx.create_text(HELLO, "hello\n"),
            };
            ctx.show(buf, ShowIn::Current)?;
            Ok(())
        });
    }
}

fn greeting() -> Greeting {
    Greeting {
        command: "hello".into(),
        buffer: HELLO.into(),
    }
}

/// The fixture, its keys (one of them over this config's `\z`), its face, and its greeting.
fn extra() -> Extra {
    Extra {
        extensions: vec![Box::new(Hello)],
        bindings: Some(Box::new(|reg: &mut Registry| {
            let keys = reg.global_keymap();
            keys.bind(State::Normal, "<leader>j", "hello");
            keys.bind(State::Normal, "<leader>z", "hello");
        })),
        theme: Some(Box::new(|t: Theme| {
            t.face("hello-face", Style::new().fg(theme::RED))
        })),
        greeting: Some(greeting()),
    }
}

fn command(ctx: &Ctx, keys: &str) -> Option<String> {
    let found = ctx.describe_key(&parse_keys(keys).unwrap())?;
    match found.action {
        KeyAction::Command { name, .. } => Some(name.to_string()),
        _ => None,
    }
}

fn names(ctx: &Ctx) -> Vec<String> {
    let mut names: Vec<String> = ctx
        .buffer_list()
        .into_iter()
        .filter_map(|b| ctx.buffer_name(b).map(String::from))
        .filter(|n| !n.starts_with("*messages"))
        .collect();
    names.sort();
    names
}

fn unknown_faces(ctx: &Ctx) -> Vec<&String> {
    let said = ctx.messages.iter();
    said.filter(|m| m.contains("no one declared")).collect()
}

#[test]
fn an_extra_adds_extensions_keys_and_faces_on_top_of_this_config() {
    let ctx = die::start(config_with(vec![], extra())).unwrap();
    assert_eq!(command(&ctx, "\\j").as_deref(), Some("hello"));
    assert_eq!(
        command(&ctx, "\\z").as_deref(),
        Some("hello"),
        "a key bound in both is the extra's"
    );
    assert_eq!(command(&ctx, "\\f").as_deref(), Some("find-file"));
    assert_eq!(ctx.command_doc("hello").map(|d| d.1), Some("hello"));
    let face = ctx.face_style("hello-face").unwrap();
    assert_eq!(face.fg, Some(theme::RED));
    let ground = ctx.face_style("default").unwrap().bg;
    assert_eq!(ground, Some(theme::BG), "mono-dark under it");
    assert!(unknown_faces(&ctx).is_empty(), "{:?}", ctx.messages);
    assert_eq!(names(&ctx), ["*scratch*"], "no greeting outside `greeted`");
}

/// With no extension here, mono-dark styles nothing that no one declared, so a DIE with no
/// config of its own says nothing about faces at its start.
#[test]
fn this_config_alone_has_no_extension_and_no_face_unknown() {
    let config = config(vec![]);
    assert!(config.extensions.is_empty());
    let ctx = die::start(config).unwrap();
    assert!(unknown_faces(&ctx).is_empty(), "{:?}", ctx.messages);
    assert_eq!(command(&ctx, "\\z").as_deref(), Some("toggle-max-width"));
    assert_eq!(command(&ctx, "\\j"), None);
}

#[test]
fn a_fresh_start_with_no_file_opens_on_the_greeting_in_the_directory_die_started_in() {
    let fresh = Launch::default();
    let greeted_with = |files: Vec<PathBuf>, launch: &Launch, greeting| {
        let mut extra = extra();
        extra.greeting = None;
        die::start(greeted(config_with(files, extra), launch, greeting)).unwrap()
    };
    let ctx = greeted_with(vec![], &fresh, Some(greeting()));
    let shown = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.buffer_name(shown), Some(HELLO));
    let started = die::started_dir();
    assert_eq!(ctx.directory(shown), Some(started.as_path()));

    // None to show: the config as it was.
    let ctx = greeted_with(vec![], &fresh, None);
    assert_eq!(names(&ctx), ["*scratch*"]);

    // A file named: the file is on screen.
    let dir = std::env::temp_dir().join(format!("die-dev-extra-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("named.txt");
    std::fs::write(&file, "named\n").unwrap();
    let ctx = greeted_with(vec![file], &fresh, Some(greeting()));
    assert_eq!(names(&ctx), ["named.txt"]);
    std::fs::remove_dir_all(&dir).unwrap();

    // A restart's new instance shows what was there, and the greeting where nothing came back,
    // as `*scratch*` would.
    let restart = Launch {
        handover: Some("/nowhere/handover.sock".into()),
        ..Launch::default()
    };
    let state = std::env::temp_dir().join(format!("die-dev-extra-state-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&state).unwrap();
    let mut plain = config(vec![]);
    plain.core.session = Some(state.clone());
    let mut ctx = die::start(plain).unwrap();
    let scratch = ctx.focused_buffer().unwrap();
    let notes = ctx.create_text("*notes*", "unsaved words");
    ctx.show(notes, ShowIn::Current).unwrap();
    ctx.kill(scratch);
    ctx.save_session().unwrap();
    drop(ctx);
    let mut extra = extra();
    extra.greeting = None;
    let mut restarted = greeted(config_with(vec![], extra), &restart, Some(greeting()));
    restarted.core.session = Some(state.clone());
    let ctx = die::start(restarted).unwrap();
    assert_eq!(names(&ctx), ["*notes*"]);
    std::fs::remove_dir_all(&state).unwrap();
    let ctx = greeted_with(vec![], &restart, Some(greeting()));
    assert_eq!(ctx.buffer_name(ctx.focused_buffer().unwrap()), Some(HELLO));
}
