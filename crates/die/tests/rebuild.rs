//! `rebuild`, with `sh` standing in for cargo: what it says goes in `*rebuild*` as it says it,
//! a build that works restarts and closes the window it opened, one that fails restarts nothing
//! and leaves its errors on screen, and the workspace is the DIE checkout you're in.

use die_test_cap as _;

use die::core::{Args, Ctx, Registry};
use die::rebuild::{REBUILD, built_from, register, root_for};
use die::{Config, start};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

fn sh(script: &'static str) -> impl Fn(&Path) -> Command {
    move |root| {
        let mut cmd = Command::new("/bin/sh");
        cmd.current_dir(root).args(["-c", script]);
        cmd
    }
}

/// An editor whose `rebuild` runs `script`, and how many times it has asked for a restart.
fn editor(script: &'static str) -> (Ctx, Rc<Cell<usize>>) {
    let restarts = Rc::new(Cell::new(0));
    let counted = restarts.clone();
    let mut config = Config::default();
    config.core.register = Some(Box::new(move |reg: &mut Registry| {
        register(reg, sh(script), move || counted.set(counted.get() + 1));
    }));
    (start(config).unwrap(), restarts)
}

fn output(ctx: &Ctx) -> String {
    let buf = ctx.find_buffer(REBUILD).expect("*rebuild* is there");
    ctx.text(buf).unwrap().rope().to_string()
}

/// Start a rebuild without waiting for it, as `type_keys` would.
fn start_rebuild(ctx: &mut Ctx) {
    ctx.run("rebuild", &Args::default()).unwrap();
}

fn focused_name(ctx: &Ctx) -> String {
    ctx.buffer_name(ctx.focused_buffer().unwrap())
        .unwrap()
        .to_owned()
}

#[test]
fn a_build_that_works_shows_what_it_said_then_restarts_and_closes_its_window() {
    let (mut ctx, restarts) =
        editor("sleep 0.2; echo Compiling die; echo warning: unused >&2; echo Finished");
    start_rebuild(&mut ctx);
    assert_eq!(ctx.window_list().len(), 2, "{:?}", ctx.messages);
    assert_eq!(focused_name(&ctx), "*scratch*", "the focus stays");
    ctx.run_until_idle();
    let said = output(&ctx);
    for part in [
        "sh -c",
        "Compiling die\n",
        "warning: unused\n",
        "Finished\n",
        "Built.",
    ] {
        assert!(said.contains(part), "{part}: {said}");
    }
    assert_eq!(restarts.get(), 1);
    assert_eq!(ctx.window_list().len(), 1, "the window it opened has gone");
}

#[test]
fn a_build_that_fails_restarts_nothing_and_leaves_its_errors_where_gf_finds_them() {
    let (mut ctx, restarts) = editor(
        "echo 'error[E0425]: cannot find value `x`' >&2; echo ' --> crates/die/src/lib.rs:3:5' >&2; exit 101",
    );
    ctx.type_keys(":rebuild<CR>");
    ctx.run_until_idle();
    let said = output(&ctx);
    for part in ["error[E0425]", "--> crates/die/src/lib.rs:3:5", "101"] {
        assert!(said.contains(part), "{part}: {said}");
    }
    assert_eq!(restarts.get(), 0);
    assert_eq!(ctx.window_list().len(), 2, "the errors stay on screen");
    assert_eq!(focused_name(&ctx), "*scratch*");
    let last = ctx.messages.last().unwrap();
    assert!(last.contains("The rebuild failed"), "{:?}", ctx.messages);
    let buf = ctx.find_buffer(REBUILD).unwrap();
    assert_eq!(ctx.directory(buf), Some(built_from().as_path()));
}

#[test]
fn what_the_build_says_shows_while_it_runs() {
    let (mut ctx, restarts) = editor("echo first; sleep 3; echo second");
    start_rebuild(&mut ctx);
    let start = Instant::now();
    while !output(&ctx).contains("first\n") && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(10));
        while ctx.step() {}
    }
    assert!(output(&ctx).contains("first\n"), "{}", output(&ctx));
    assert!(!output(&ctx).contains("second\n"), "{}", output(&ctx));
    assert_eq!(restarts.get(), 0, "still building");
    ctx.run_until_idle();
    assert_eq!(restarts.get(), 1);
}

#[test]
fn one_rebuild_runs_at_a_time() {
    let (mut ctx, restarts) = editor("sleep 0.3");
    ctx.type_keys(":rebuild<CR>:rebuild<CR>");
    ctx.run_until_idle();
    let refused = "A rebuild is already running";
    assert!(
        ctx.messages.iter().any(|m| m == refused),
        "{:?}",
        ctx.messages
    );
    assert_eq!(restarts.get(), 1);
    ctx.type_keys(":rebuild<CR>");
    ctx.run_until_idle();
    assert_eq!(restarts.get(), 2, "and another once it's done");
}

#[test]
fn the_workspace_is_the_die_checkout_youre_in_or_else_the_one_this_was_built_from() {
    let dir = std::env::temp_dir().join(format!("die-rebuild-root-{}", std::process::id()));
    let checkout = dir.join("checkout");
    for d in [".git", "crates/die/src", "xtask"] {
        std::fs::create_dir_all(checkout.join(d)).unwrap();
    }
    std::fs::write(checkout.join("crates/die/Cargo.toml"), "").unwrap();
    let other = dir.join("other");
    std::fs::create_dir_all(other.join(".git")).unwrap();

    assert_eq!(root_for(Some(&checkout.join("crates/die/src"))), checkout);
    assert_eq!(root_for(Some(&other)), built_from(), "not DIE's");
    assert_eq!(root_for(None), built_from());
    let here: PathBuf = built_from();
    assert!(
        here.join("crates/die/Cargo.toml").is_file(),
        "{}",
        here.display()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
