//! `compile`, `recompile`, `kill-compilation` and `:make`, with `sh` scripts standing in for
//! cargo: what a command prints streams into `*compilation*` under Emacs's header, its errors are
//! where `next-error`, `<CR>` and `gf` go, from the directory it ran in or its project, and one
//! runs at a time.

use die_test_cap as _;

use die::core::{Args, BufferId, Ctx};
use die::{Config, start};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const COMPILATION: &str = "*compilation*";

struct Dir(PathBuf);

impl Dir {
    /// A project, with a `.git`, and its `src/lib.rs` and `src/main.rs`.
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("die-compile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::create_dir_all(d.join("src")).unwrap();
        let d = Dir(d.canonicalize().unwrap());
        d.write("src/lib.rs", "one\ntwo two\nthree\nfour\n");
        d.write("src/main.rs", "a\nb\nc\n");
        d
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }
    /// A script, and the command that runs it.
    fn script(&self, name: &str, body: &str) -> String {
        format!("sh {}", self.write(name, body).display())
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
    ctx.show(buf, die::core::ShowIn::Current).unwrap();
    (ctx, buf)
}

fn text(s: &str) -> Args {
    Args {
        text: Some(s.into()),
        ..Args::default()
    }
}

fn output(ctx: &Ctx) -> String {
    let buf = ctx
        .find_buffer(COMPILATION)
        .expect("*compilation* is there");
    ctx.text(buf).unwrap().rope().to_string()
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

/// Take turns, without waiting for jobs, until `f` or a few seconds.
fn until(ctx: &mut Ctx, what: &str, f: impl Fn(&Ctx) -> bool) {
    let start = Instant::now();
    while !f(ctx) {
        assert!(start.elapsed() < Duration::from_secs(5), "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
        while ctx.step() {}
    }
}

/// The focused buffer's name, and its cursor's line and column from 1.
fn at(ctx: &Ctx) -> (String, usize, usize) {
    let (b, w) = (ctx.focused_buffer().unwrap(), ctx.focused().unwrap());
    let t = ctx.text(b).unwrap();
    let p = t.cursor(w).unwrap();
    let line = t.line_of(p);
    let name = ctx.buffer_name(b).unwrap().to_owned();
    (name, line + 1, p - t.line_start(line) + 1)
}

/// The frame as text.
fn screen(ctx: &mut Ctx) -> String {
    let s = ctx.scene();
    let rows: Vec<String> = (0..s.rows).map(|r| s.row_text(r)).collect();
    rows.join("\n")
}

/// The window showing `*compilation*`, focused.
fn into_output(ctx: &mut Ctx) {
    let buf = ctx.find_buffer(COMPILATION).unwrap();
    let w = ctx
        .window_list()
        .into_iter()
        .find(|w| ctx.window_buffer(*w) == Some(buf));
    ctx.focus(w.unwrap());
}

#[test]
fn what_it_prints_streams_in_below_with_emacss_header_and_last_line() {
    let d = Dir::new("streams");
    let (mut ctx, here) = editor(&d.0.join("src/lib.rs"));
    let cmd = d.script("build.sh", "echo one; sleep 0.4; echo two");
    ctx.run("compile", &text(&cmd)).unwrap();
    assert_eq!(names(&ctx), ["lib.rs", COMPILATION]);
    assert_eq!(ctx.focused_buffer(), Some(here), "the focus stays");
    until(&mut ctx, "the first line", |ctx| {
        output(ctx).contains("\none\n")
    });
    let said = output(&ctx);
    assert!(!said.contains("two"), "{said}");
    let dir = d.0.join("src");
    let head = format!(
        "-*- mode: compilation; default-directory: \"{}/\" -*-\nCompilation started at ",
        dir.display()
    );
    assert!(said.starts_with(&head), "{said}");
    assert!(said.contains(&format!("\n\n{cmd}\none\n")), "{said}");
    ctx.run_until_idle();
    let said = output(&ctx);
    let last = said.lines().last().unwrap();
    assert!(said.contains("two\n\nCompilation finished at "), "{said}");
    assert!(
        last.contains(", duration 0.") && last.ends_with(" s"),
        "{last}"
    );
    assert_eq!(ctx.messages.last().unwrap(), "Compilation finished");
    let (buf, w) = (ctx.find_buffer(COMPILATION).unwrap(), ctx.window_list()[1]);
    assert_eq!(
        ctx.text(buf).unwrap().cursor(w),
        Some(0),
        "at the top, as Emacs leaves it"
    );
}

#[test]
fn next_error_goes_through_rustcs_cargos_and_gnus_places_but_not_notes() {
    let d = Dir::new("errors");
    d.write("src/here.rs", "x\ny\n");
    let script = "echo '   Compiling p v0.1.0'
echo 'error[E0425]: cannot find value `x` in this scope'
echo ' --> src/lib.rs:2:5'
echo '  |'
echo '2 |     x'
echo 'note: the lint level is defined here'
echo ' --> src/lib.rs:1:2'
echo 'src/main.rs:3:1: warning: unused'
echo 'here.rs:2: error: from where it ran'
echo \"thread 'tests::t' (5678) panicked at src/lib.rs:3:2:\"
echo 'assertion failed'
exit 101";
    let (mut ctx, _) = editor(&d.0.join("src/lib.rs"));
    let cmd = d.script("errors.sh", script);
    ctx.type_keys(&format!(":make {cmd}<CR>"));
    let said = output(&ctx);
    assert!(
        said.contains("\nCompilation exited abnormally with code 101 at "),
        "{said}"
    );
    let last = ctx.messages.last().unwrap();
    assert_eq!(last, "Compilation exited abnormally with code 101");
    let steps = [
        ("lib.rs", 2, 5),
        ("main.rs", 3, 1),
        ("here.rs", 2, 1),
        ("lib.rs", 3, 2),
    ];
    for want in steps {
        ctx.type_keys(":cn<CR>");
        assert_eq!(
            at(&ctx),
            (want.0.into(), want.1, want.2),
            "{:?}",
            ctx.messages
        );
    }
    ctx.type_keys(":cn<CR>");
    assert_eq!(ctx.messages.last().unwrap(), "Moved past last error");
    ctx.type_keys(":cp<CR>");
    assert_eq!(at(&ctx), ("here.rs".into(), 2, 1));

    // In the output, `<CR>` opens a note too, and `gf` reads the name under the cursor.
    into_output(&mut ctx);
    let buf = ctx.find_buffer(COMPILATION).unwrap();
    let note = output(&ctx).find(" --> src/lib.rs:1:2").unwrap();
    let w = ctx.focused().unwrap();
    ctx.text_mut(buf).unwrap().set_cursor(w, note);
    ctx.type_keys("<CR>");
    assert_eq!(at(&ctx), ("lib.rs".into(), 1, 2));
    into_output(&mut ctx);
    let w = ctx.focused().unwrap();
    ctx.text_mut(buf).unwrap().set_cursor(w, note + 6);
    ctx.type_keys("gf");
    assert_eq!(at(&ctx), ("lib.rs".into(), 1, 2), "{:?}", ctx.messages);

    // `gj` and `gk` move between errors in the output, past the note. `gf` opened the file in
    // the output's window, as your Evil's does.
    ctx.show(buf, die::core::ShowIn::Current).unwrap();
    let w = ctx.focused().unwrap();
    ctx.text_mut(buf).unwrap().set_cursor(w, 0);
    ctx.type_keys("gj");
    assert_eq!(at(&ctx), (COMPILATION.into(), 7, 1));
    ctx.type_keys("gj");
    assert_eq!(at(&ctx), (COMPILATION.into(), 12, 1), "{}", output(&ctx));
    ctx.type_keys("gk");
    assert_eq!(at(&ctx), (COMPILATION.into(), 7, 1));
}

#[test]
fn c_c_c_k_stops_it_and_says_so() {
    let d = Dir::new("kill");
    let (mut ctx, _) = editor(&d.0.join("src/lib.rs"));
    ctx.type_keys(":make true<CR>");
    ctx.type_keys(":kill-compilation<CR>");
    let last = ctx.messages.last().unwrap();
    assert_eq!(last, "The compilation process is not running");
    let cmd = d.script("long.sh", "echo started; sleep 10");
    let start = Instant::now();
    ctx.run("compile", &text(&cmd)).unwrap();
    until(&mut ctx, "it starts", |ctx| {
        output(ctx).contains("started\n")
    });
    into_output(&mut ctx);
    ctx.feed_keys("<C-c><C-k>").unwrap();
    until(&mut ctx, "it stops", |ctx| {
        output(ctx).contains("\nCompilation interrupt at ")
    });
    assert!(start.elapsed() < Duration::from_secs(5));
    assert_eq!(ctx.messages.last().unwrap(), "Compilation interrupt");
}

#[test]
fn one_runs_at_a_time_and_a_new_one_stops_the_old_if_you_say_so() {
    let d = Dir::new("one");
    let (mut ctx, _) = editor(&d.0.join("src/lib.rs"));
    let slow = d.script("slow.sh", "echo slow; sleep 10");
    ctx.run("compile", &text(&slow)).unwrap();
    until(&mut ctx, "it starts", |ctx| output(ctx).contains("slow\n"));
    ctx.run("compile", &text("echo quick")).unwrap();
    let asked = "A compilation process is running; kill it? (yes or no) ";
    assert!(
        screen(&mut ctx).contains(asked.trim_end()),
        "{}",
        screen(&mut ctx)
    );
    ctx.feed_keys("no<CR>").unwrap();
    while ctx.step() {}
    let refused = "Cannot have two processes in `*compilation*' at once";
    assert_eq!(ctx.messages.last().unwrap(), refused);
    assert!(output(&ctx).contains("slow\n"), "the first runs on");

    ctx.run("compile", &text("echo quick")).unwrap();
    ctx.feed_keys("yes<CR>").unwrap();
    until(&mut ctx, "the second ends", |ctx| {
        output(ctx).contains("Compilation finished")
    });
    let said = output(&ctx);
    assert!(said.contains("\necho quick\nquick\n"), "{said}");
    assert!(!said.contains("slow"), "{said}");
}

#[test]
fn gr_and_a_bare_make_run_it_again_where_it_ran() {
    let d = Dir::new("again");
    let (mut ctx, _) = editor(&d.0.join("src/lib.rs"));
    let cmd = d.script("count.sh", "echo run >> count; wc -l < count | tr -d ' '");
    ctx.type_keys(&format!(":make {cmd}<CR>"));
    assert!(output(&ctx).contains("\n1\n"), "{}", output(&ctx));
    into_output(&mut ctx);
    ctx.type_keys("gr");
    assert!(output(&ctx).contains("\n2\n"), "{}", output(&ctx));
    let elsewhere = ctx.open_file(&d.0.join("src/main.rs")).unwrap();
    ctx.show(elsewhere, die::core::ShowIn::Current).unwrap();
    ctx.type_keys(":make<CR>");
    assert!(output(&ctx).contains("\n3\n"), "{}", output(&ctx));
    let count = std::fs::read_to_string(d.0.join("src/count")).unwrap();
    assert_eq!(
        count.lines().count(),
        3,
        "all in the directory it first ran in"
    );
}

#[test]
fn the_prompt_offers_the_last_command_or_emacss() {
    let d = Dir::new("prompt");
    let (mut ctx, _) = editor(&d.0.join("src/lib.rs"));
    ctx.type_keys("<A-x>compile<CR>");
    assert!(screen(&mut ctx).contains("Compile command: make -k"));
    assert_eq!(ctx.prompt_text(), Some("make -k "));
    ctx.type_keys("<C-u>echo first<CR>");
    assert!(output(&ctx).contains("\nfirst\n"));
    ctx.run("compile", &Args::default()).unwrap();
    assert_eq!(ctx.prompt_text(), Some("echo first"));
    ctx.type_keys("<Esc>");
    ctx.type_keys(":make echo second<CR>");
    ctx.run("compile", &Args::default()).unwrap();
    assert_eq!(ctx.prompt_text(), Some("echo second"));
    assert_eq!(ctx.prompt_history("compile"), ["echo first", "echo second"]);
}

#[test]
fn unsaved_files_are_offered_for_saving_first() {
    let d = Dir::new("save");
    let (mut ctx, buf) = editor(&d.0.join("src/lib.rs"));
    ctx.type_keys("ihello <Esc>");
    ctx.type_keys(":make echo built<CR>");
    let path = d.0.join("src/lib.rs");
    // The label is wider than the frame, so the command row shows its end.
    let asked = "/src/lib.rs? (y, n, !, q)";
    assert!(screen(&mut ctx).contains(asked), "{}", screen(&mut ctx));
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.find_buffer(COMPILATION), None, "a cancel runs nothing");
    ctx.type_keys(":make echo built<CR>n<CR>");
    assert!(output(&ctx).contains("\nbuilt\n"));
    assert!(ctx.text(buf).unwrap().modified());
    ctx.type_keys(":make echo built<CR>y<CR>");
    assert!(!ctx.text(buf).unwrap().modified());
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.starts_with("hello one"), "{on_disk}");
}

#[test]
fn output_past_the_limit_is_cut_with_a_note() {
    let d = Dir::new("limit");
    let (mut ctx, _) = editor(&d.0.join("src/lib.rs"));
    ctx.type_keys(":make yes | head -c 9000000<CR>");
    let said = output(&ctx);
    assert!(said.len() < (8 << 20) + 1000, "{}", said.len());
    assert!(said.contains("\n[DIE kept the first 8 MiB of this.]\n"));
    assert!(said.contains("\nCompilation finished at "));
}
