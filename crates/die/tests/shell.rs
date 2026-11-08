//! `:!`, `M-!`, `:{range}!` and `:r !` with `sh` commands: short output on the command row,
//! longer in `*Shell Command Output*`, filters in place, in the buffer's directory, with the
//! PATH a terminal has. A slow command doesn't hold up the editor, a runaway one is stopped, and
//! a filter whose text changed meanwhile leaves it alone.

use die_test_cap as _;

use die::core::{BufferId, Ctx, ShowIn};
use die::{Config, start};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const OUTPUT: &str = "*Shell Command Output*";

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("die-shell-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        Dir(d.canonicalize().unwrap())
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An editor showing `file`.
fn editor(file: &Path) -> (Ctx, BufferId) {
    let mut ctx = start(Config::default()).unwrap();
    let buf = ctx.open_file(file).unwrap();
    ctx.show(buf, ShowIn::Current).unwrap();
    (ctx, buf)
}

fn text(ctx: &Ctx, b: BufferId) -> String {
    ctx.text(b).unwrap().rope().to_string()
}

fn names(ctx: &Ctx) -> Vec<String> {
    let shown = ctx
        .window_list()
        .into_iter()
        .map(|w| ctx.window_buffer(w).unwrap());
    shown
        .map(|b| ctx.buffer_name(b).unwrap().to_owned())
        .collect()
}

fn last(ctx: &Ctx) -> &str {
    ctx.messages.last().map_or("", String::as_str)
}

/// Take turns, without waiting for jobs, until `f` or a few seconds.
fn until(ctx: &mut Ctx, what: &str, f: impl Fn(&Ctx) -> bool) {
    let start = Instant::now();
    while !f(ctx) {
        assert!(start.elapsed() < Duration::from_secs(5), "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
        while ctx.step() {}
    }
}

#[test]
fn one_line_goes_on_the_command_row_and_more_in_a_window_below() {
    let d = Dir::new("show");
    let (mut ctx, b) = editor(&d.write("a.txt", "one\ntwo\n"));
    ctx.type_keys("j$:!echo hi<CR>");
    assert_eq!(last(&ctx), "hi");
    assert_eq!(names(&ctx), ["a.txt"], "no window for one line");
    assert_eq!(text(&ctx, b), "one\ntwo\n");
    let t = ctx.text(b).unwrap();
    assert_eq!(
        t.cursor(ctx.focused().unwrap()),
        Some(4),
        "at the line's start, as Evil's"
    );

    ctx.type_keys(":!printf 'a\\nb\\n'<CR>");
    assert_eq!(names(&ctx), ["a.txt", OUTPUT]);
    assert_eq!(ctx.focused_buffer(), Some(b), "the focus stays");
    let out = ctx.find_buffer(OUTPUT).unwrap();
    assert_eq!(text(&ctx, out), "a\nb\n");
    ctx.type_keys(":!echo again<CR>");
    assert_eq!(
        text(&ctx, out),
        "again\n",
        "a window has it, so it shows there"
    );
    assert_ne!(last(&ctx), "again");
}

#[test]
fn nothing_printed_says_how_it_ended() {
    let d = Dir::new("quiet");
    let (mut ctx, _) = editor(&d.write("a.txt", "x\n"));
    ctx.type_keys(":!true<CR>");
    assert_eq!(last(&ctx), "(Shell command succeeded with no output)");
    ctx.type_keys(":!exit 3<CR>");
    assert_eq!(
        last(&ctx),
        "(Shell command failed with code 3 and no output)"
    );
    ctx.type_keys(":!<CR>");
    assert_eq!(last(&ctx), "No shell command");
    ctx.type_keys(":!!<CR>");
    assert_eq!(
        last(&ctx),
        "(Shell command failed with code 3 and no output)",
        "the last again"
    );
}

#[test]
fn meta_bang_asks_for_a_command_with_its_own_history() {
    let d = Dir::new("meta");
    let (mut ctx, _) = editor(&d.write("a.txt", "x\n"));
    ctx.type_keys("<A-!>");
    assert_eq!(ctx.prompt_text(), Some(""));
    ctx.type_keys("echo from M-!<CR>");
    assert_eq!(last(&ctx), "from M-!");
    assert_eq!(ctx.prompt_history("shell-command"), ["echo from M-!"]);
}

#[test]
fn it_runs_in_the_buffers_directory_with_its_file_for_percent() {
    let d = Dir::new("where");
    let file = d.write("sub/b.txt", "x\n");
    let (mut ctx, _) = editor(&file);
    ctx.type_keys(":!pwd<CR>");
    assert_eq!(last(&ctx), d.0.join("sub").display().to_string());
    ctx.type_keys(":!echo %<CR>");
    assert_eq!(last(&ctx), file.display().to_string());
    ctx.type_keys(":!echo \\%<CR>");
    assert_eq!(last(&ctx), "%");
}

#[test]
fn commands_get_the_path_a_login_shell_has_with_cargo_and_local_bin() {
    let d = Dir::new("path");
    let (mut ctx, _) = editor(&d.write("a.txt", "x\n"));
    // `%` is the file's name, as in your Evil, unless `\` comes before.
    ctx.type_keys(":!printf \\%s \"$PATH\"<CR>");
    let path = last(&ctx).to_string();
    let home = std::env::home_dir().unwrap();
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    for own in [".local/bin", ".cargo/bin"] {
        assert!(dirs.contains(&home.join(own)), "{own}: {path}");
    }
}

#[test]
fn a_filter_puts_what_it_prints_in_place_of_the_lines_as_one_change() {
    let d = Dir::new("filter");
    let (mut ctx, b) = editor(&d.write("a.txt", "c\nb\na\nz\n"));
    ctx.type_keys(":1,3!sort<CR>");
    assert_eq!(text(&ctx, b), "a\nb\nc\nz\n");
    ctx.type_keys("u");
    assert_eq!(text(&ctx, b), "c\nb\na\nz\n");
    ctx.type_keys(":$r !echo end<CR>");
    assert_eq!(text(&ctx, b), "c\nb\na\nz\nend\n");
    let other = d.write("sub/more.txt", "read\nin\n");
    ctx.type_keys(&format!(":1r {}<CR>", other.display()));
    assert_eq!(text(&ctx, b), "c\nread\nin\nb\na\nz\nend\n");
    let t = ctx.text(b).unwrap();
    assert_eq!(
        t.cursor(ctx.focused().unwrap()),
        Some(2),
        "on the first line read in"
    );
}

#[test]
fn a_slow_command_goes_on_without_holding_up_the_editor() {
    let d = Dir::new("slow");
    let (mut ctx, b) = editor(&d.write("a.txt", "x\n"));
    let start = Instant::now();
    ctx.feed_keys(":!sleep 1; echo late<CR>").unwrap();
    while ctx.step() {}
    assert!(
        start.elapsed() < Duration::from_millis(900),
        "{:?}",
        start.elapsed()
    );
    assert!(
        last(&ctx).contains("is still running"),
        "{:?}",
        ctx.messages
    );
    ctx.feed_keys("ix<Esc>").unwrap();
    while ctx.step() {}
    assert_eq!(text(&ctx, b), "xx\n", "keys work meanwhile");
    until(&mut ctx, "its output", |ctx| last(ctx) == "late");
}

#[test]
fn a_slow_filter_leaves_text_that_changed_meanwhile_alone() {
    let d = Dir::new("changed");
    let (mut ctx, b) = editor(&d.write("a.txt", "b\na\n"));
    ctx.feed_keys(":%!sleep 1; sort<CR>").unwrap();
    while ctx.step() {}
    ctx.feed_keys("Ostill mine<Esc>").unwrap();
    while ctx.step() {}
    until(&mut ctx, "it ends", |ctx| {
        last(ctx).contains("The text changed")
    });
    assert_eq!(text(&ctx, b), "still mine\nb\na\n");

    ctx.feed_keys(":%!sleep 0.7; sort<CR>").unwrap();
    while ctx.step() {}
    until(&mut ctx, "it ends", |ctx| {
        text(ctx, b) == "a\nb\nstill mine\n"
    });
}

#[test]
fn a_runaway_command_is_stopped_at_the_limit() {
    let d = Dir::new("runaway");
    let (mut ctx, b) = editor(&d.write("a.txt", "x\n"));
    ctx.type_keys(":!yes<CR>");
    until(&mut ctx, "it stops", |ctx| {
        ctx.find_buffer(OUTPUT).is_some()
    });
    let out = text(&ctx, ctx.find_buffer(OUTPUT).unwrap());
    assert!(out.len() < (8 << 20) + 1000, "{}", out.len());
    assert!(out.ends_with("\n[DIE stopped the command at 8 MiB of output.]\n"));
    ctx.type_keys(":%!yes<CR>");
    assert!(
        last(&ctx).contains("printed more than DIE keeps"),
        "{:?}",
        ctx.messages
    );
    assert_eq!(text(&ctx, b), "x\n");
}
