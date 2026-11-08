//! `rebuild`, with `sh` standing in for cargo: what it says goes in `*rebuild*` as it says it,
//! a build that works restarts and leaves nothing behind, one that fails restarts nothing and
//! leaves its first error on screen, where `gf` and `next-error` find it, and the workspace is the
//! checkout the bundle names, whatever file you're in.

use die_test_cap as _;

use die::core::{Args, BufferId, Ctx, Registry, ShowIn};
use die::rebuild::{REBUILD, Setup, home_of, register, root_for};

use die::{Config, start};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("die-rebuild-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d.canonicalize().unwrap())
    }
    /// A DIE checkout, as far as `rebuild` looks, and the one the bundle names.
    fn checkout(&self, name: &str) -> PathBuf {
        let root = self.0.join(name);
        for d in [".git", "crates/die/src", "xtask"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("crates/die/Cargo.toml"), "").unwrap();
        std::fs::write(self.named(), format!("{}\n", root.display())).unwrap();
        root
    }
    /// What stands for the bundle's `Contents/Resources/checkout`.
    fn named(&self) -> PathBuf {
        self.0.join("named-checkout")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An editor whose `rebuild` runs `script` with `sh`, and how many restarts it has asked for.
/// The checkout it builds is named in `dir`.
fn editor(dir: &Dir, script: &str) -> (Ctx, Rc<Cell<usize>>) {
    let script = script.to_owned();
    editor_with(dir, move |root| {
        let mut cmd = Command::new("/bin/sh");
        cmd.current_dir(root).args(["-c", &script]);
        cmd
    })
}

fn editor_with(dir: &Dir, command: impl Fn(&Path) -> Command + 'static) -> (Ctx, Rc<Cell<usize>>) {
    let restarts = Rc::new(Cell::new(0));
    let counted = restarts.clone();
    let checkout = dir.named();
    let mut config = Config::default();
    config.core.register = Some(Box::new(move |reg: &mut Registry| {
        let setup = Setup {
            command: Box::new(move |root| Ok(command(root))),
            restart: Box::new(move || counted.set(counted.get() + 1)),
            checkout,
        };
        register(reg, setup);
    }));
    (start(config).unwrap(), restarts)
}

/// Focus a new file in `root`.
fn edit_in(ctx: &mut Ctx, root: &Path) -> BufferId {
    let buf = ctx.open_file(&root.join("crates/die/src/lib.rs")).unwrap();
    ctx.show(buf, ShowIn::Current).unwrap();
    buf
}

/// Start a rebuild without waiting for it, as `type_keys` would.
fn start_rebuild(ctx: &mut Ctx) {
    ctx.run("rebuild", &Args::default()).unwrap();
}

fn output(ctx: &Ctx) -> String {
    let buf = ctx.find_buffer(REBUILD).expect("*rebuild* is there");
    ctx.text(buf).unwrap().rope().to_string()
}

fn names(ctx: &Ctx) -> Vec<String> {
    let windows = ctx.window_list().into_iter();
    let shown = windows.map(|w| ctx.window_buffer(w).unwrap());
    shown
        .map(|b| ctx.buffer_name(b).unwrap().to_owned())
        .collect()
}

#[test]
fn a_build_that_works_restarts_and_leaves_no_window_or_log_behind() {
    let d = Dir::new("works");
    let root = d.checkout("die");
    let (mut ctx, restarts) = editor(&d, "sleep 0.2; echo Compiling die; echo Finished");
    let here = edit_in(&mut ctx, &root);
    start_rebuild(&mut ctx);
    assert_eq!(names(&ctx), ["lib.rs", REBUILD], "{:?}", ctx.messages);
    assert_eq!(ctx.focused_buffer(), Some(here), "the focus stays");
    assert!(output(&ctx).starts_with("sh -c sleep"), "{}", output(&ctx));
    assert!(output(&ctx).contains(&format!(" in {}\n", root.display())));
    ctx.run_until_idle();
    assert_eq!(restarts.get(), 1, "{:?}", ctx.messages);
    assert_eq!(names(&ctx), ["lib.rs"]);
    assert_eq!(
        ctx.find_buffer(REBUILD),
        None,
        "a restart won't bring it back"
    );
}

#[test]
fn a_build_that_fails_restarts_nothing_and_shows_its_first_error_where_gf_finds_it() {
    let d = Dir::new("fails");
    let root = d.checkout("die");
    let script = "echo Compiling die; echo 'error[E0425]: cannot find value `x`' >&2; \
                  echo ' --> crates/die/src/lib.rs:3:5' >&2; echo 'error: could not compile' >&2; \
                  exit 101";
    let (mut ctx, restarts) = editor(&d, script);
    std::fs::write(root.join("crates/die/src/lib.rs"), "a\nb\nlet x = y;\n").unwrap();
    let here = edit_in(&mut ctx, &root);
    start_rebuild(&mut ctx);
    ctx.run_until_idle();
    assert_eq!(restarts.get(), 0);
    let said = output(&ctx);
    for part in ["error[E0425]", "--> crates/die/src/lib.rs:3:5", "101"] {
        assert!(said.contains(part), "{part}: {said}");
    }
    assert_eq!(
        names(&ctx),
        ["lib.rs", REBUILD],
        "the errors stay on screen"
    );
    assert_eq!(ctx.focused_buffer(), Some(here));
    let last = ctx.messages.last().unwrap();
    assert!(last.contains("The rebuild failed"), "{:?}", ctx.messages);
    assert!(last.contains(REBUILD), "{last}");
    let buf = ctx.find_buffer(REBUILD).unwrap();
    assert_eq!(ctx.directory(buf), Some(root.as_path()), "for gf");
    let w = ctx.window_list()[1];
    let at = ctx.text(buf).unwrap().cursor(w).unwrap();
    assert!(said[at..].starts_with("error[E0425]"), "{}", &said[at..]);

    // `next-error` goes through it as through `*compilation*`.
    ctx.type_keys(":cn<CR>");
    let (b, w) = (ctx.focused_buffer().unwrap(), ctx.focused().unwrap());
    assert_eq!(b, here, "{:?}", ctx.messages);
    assert_eq!(
        ctx.text(b).unwrap().cursor(w),
        Some(4 + 4),
        "line 3, column 5"
    );
}

#[test]
fn after_a_failure_the_build_that_works_closes_the_window_the_first_one_opened() {
    let d = Dir::new("again");
    let root = d.checkout("die");
    let fixed = d.0.join("fixed");
    let script = format!(
        "test -e '{}' || {{ echo 'error: no' >&2; exit 1; }}",
        fixed.display()
    );
    let (mut ctx, restarts) = editor(&d, &script);
    edit_in(&mut ctx, &root);
    ctx.type_keys(":rebuild<CR>");
    assert_eq!(names(&ctx), ["lib.rs", REBUILD]);
    std::fs::write(&fixed, "").unwrap();
    ctx.type_keys(":rebuild<CR>");
    assert_eq!(restarts.get(), 1, "{:?}", ctx.messages);
    assert_eq!(names(&ctx), ["lib.rs"]);
}

#[test]
fn what_the_build_says_shows_while_it_runs_and_the_window_follows_it() {
    let d = Dir::new("streams");
    let root = d.checkout("die");
    let (mut ctx, restarts) = editor(&d, "for i in $(seq 1 40); do echo line $i; done; sleep 3");
    edit_in(&mut ctx, &root);
    start_rebuild(&mut ctx);
    let start = Instant::now();
    while !output(&ctx).contains("line 40\n") && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(10));
        while ctx.step() {}
    }
    let said = output(&ctx);
    assert!(said.contains("line 40\n"), "{said}");
    assert_eq!(restarts.get(), 0, "still building");
    let (buf, w) = (ctx.find_buffer(REBUILD).unwrap(), ctx.window_list()[1]);
    let at = ctx.text(buf).unwrap().cursor(w).unwrap();
    assert_eq!(&said[at..], "line 40\n", "the cursor is on the last line");
    ctx.run_until_idle();
    assert_eq!(restarts.get(), 1);
}

#[test]
fn a_process_the_build_leaves_behind_isnt_waited_for() {
    let d = Dir::new("behind");
    let root = d.checkout("die");
    let (mut ctx, restarts) = editor(&d, "sleep 4 & echo done");
    edit_in(&mut ctx, &root);
    let start = Instant::now();
    ctx.type_keys(":rebuild<CR>");
    assert_eq!(restarts.get(), 1);
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn one_rebuild_runs_at_a_time() {
    let d = Dir::new("one");
    let root = d.checkout("die");
    let (mut ctx, restarts) = editor(&d, "sleep 0.3");
    edit_in(&mut ctx, &root);
    ctx.type_keys(":rebuild<CR>:rebuild<CR>");
    let refused = "A rebuild is already running";
    assert!(
        ctx.messages.iter().any(|m| m == refused),
        "{:?}",
        ctx.messages
    );
    assert_eq!(restarts.get(), 1);
    ctx.type_keys(":rebuild<CR>");
    assert_eq!(restarts.get(), 2, "and another once it's done");
}

#[test]
fn a_reused_log_loses_the_text_defaults_a_session_gave_it() {
    let d = Dir::new("reused");
    let root = d.checkout("die");
    let (mut ctx, _) = editor(&d, "exit 1");
    edit_in(&mut ctx, &root);
    let buf = ctx.create_text(REBUILD, "from the last session\n");
    let settings = ctx.text_mut(buf).unwrap().settings_mut();
    (settings.max_width, settings.center) = (Some(80), true);
    ctx.type_keys(":rebuild<CR>");
    let settings = ctx.text(buf).unwrap().settings();
    assert_eq!((settings.max_width, settings.center), (None, false));
    assert!(settings.read_only);
    assert!(!output(&ctx).contains("from the last session"));
}

#[test]
fn it_builds_the_checkout_the_bundle_names_whatever_file_youre_in() {
    let d = Dir::new("elsewhere");
    let root = d.checkout("die");
    let elsewhere = d.0.join("other-project/.git");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let (mut ctx, restarts) = editor(&d, "true");
    let notes = ctx.open_file(&elsewhere.with_file_name("app.rs")).unwrap();
    ctx.show(notes, ShowIn::Current).unwrap();
    ctx.type_keys(":rebuild<CR>");
    assert_eq!(restarts.get(), 1, "{:?}", ctx.messages);
    let built = ctx.messages.iter().find(|m| m.starts_with("Rebuilding in"));
    assert_eq!(built, Some(&format!("Rebuilding in {}", root.display())));
}

#[test]
fn a_bundle_that_names_no_checkout_says_how_to_make_one_that_does() {
    let d = Dir::new("unnamed");
    let (mut ctx, restarts) = editor(&d, "true");
    ctx.type_keys(":rebuild<CR>");
    assert_eq!(restarts.get(), 0);
    let last = ctx.messages.last().unwrap();
    assert!(last.contains("doesn't say which checkout"), "{last}");
    assert!(last.contains("cargo xtask app"), "{last}");
    assert_eq!(ctx.find_buffer(REBUILD), None);
}

#[test]
fn a_command_that_wont_start_says_what_and_where() {
    let d = Dir::new("wont");
    let root = d.checkout("die");
    let (mut ctx, restarts) = editor_with(&d, |root| {
        let mut cmd = Command::new("/no/such/cargo");
        cmd.current_dir(root);
        cmd
    });
    edit_in(&mut ctx, &root);
    ctx.type_keys(":rebuild<CR>");
    assert_eq!(restarts.get(), 0);
    let last = ctx.messages.last().unwrap();
    assert!(last.contains("cargo in"), "{last}");
    assert!(last.contains(&root.display().to_string()), "{last}");
}

#[test]
fn the_checkout_named_has_to_be_there_and_be_dies() {
    let d = Dir::new("root");
    assert!(root_for(&d.named()).is_err(), "none named");
    let checkout = d.checkout("checkout");
    assert_eq!(root_for(&d.named()), Ok(checkout.clone()));
    std::fs::remove_dir_all(checkout.join("xtask")).unwrap();
    let gone = root_for(&d.named()).unwrap_err();
    assert!(gone.contains(&checkout.display().to_string()), "{gone}");
    assert!(gone.contains("isn't a DIE checkout"), "{gone}");
}

#[test]
fn it_replaces_the_bundle_that_runs_or_none() {
    let home = |p: &str| home_of(Path::new(p));
    assert_eq!(
        home("/Users/me/Applications/DIE.app"),
        Some("/Users/me".into())
    );
    assert_eq!(home("/Applications/DIE.app"), Some("/".into()));
    assert_eq!(home("/Users/me/Downloads/DIE.app"), None);
    assert_eq!(home("/Users/me/Applications/Other.app"), None);
}
