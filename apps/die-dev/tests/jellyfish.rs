//! Your jellyfish as the buffer DIE starts on, through `die_dev::greeted` and `die::start`, which
//! is everything a real run does before its window opens: when it shows, when it stays away, and
//! what a session keeps of it. No window opens, and each session lives in a temporary directory.

use die_test_cap as _;

use die::app::Launch;
use die::core::{Ctx, ShowIn, Side};
use die_dev::{config, greeted};
use die_jellyfish::NAME;
use std::path::{Path, PathBuf};

/// A directory of one test's own, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir =
            std::env::temp_dir().join(format!("die-dev-jellyfish-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The three launches that are restarts: `:restart` and `:rebuild`'s, and xtask's two.
fn restarts() -> [Launch; 3] {
    let plain = Launch::default;
    [
        Launch {
            handover: Some("/nowhere/handover.sock".into()),
            ..plain()
        },
        Launch {
            restart_loop: true,
            ..plain()
        },
        Launch {
            restart_after_frame: true,
            ..plain()
        },
    ]
}

/// DIE as `main` starts it for a run you look at, with no file named, keeping its session in
/// `dir`.
fn start_over(dir: &Path, launch: &Launch) -> Ctx {
    let mut config = greeted(config(vec![]), launch);
    config.core.session = Some(dir.to_path_buf());
    die::start(config).unwrap()
}

/// Where the cursor is in `*notes*`.
const CURSOR: usize = 8;

/// The focused window's cursor, in the buffer it shows.
fn cursor(ctx: &Ctx) -> Option<usize> {
    let (w, buf) = (ctx.focused()?, ctx.focused_buffer()?);
    ctx.text(buf).ok()?.cursor(w)
}

/// A state directory whose session is a DIE you quit with unsaved `*notes*` on screen: in its
/// one window, or with `split`, in the left of two with `*other*` in the right.
fn session(name: &str, split: bool) -> Dir {
    let dir = Dir::new(name);
    let mut plain = config(vec![]);
    plain.core.session = Some(dir.0.clone());
    let mut ctx = die::start(plain).unwrap();
    let scratch = ctx.focused_buffer().unwrap();
    let notes = ctx.create_text("*notes*", "unsaved words");
    let w = ctx.show(notes, ShowIn::Current).unwrap().unwrap();
    ctx.text_mut(notes).unwrap().set_cursor(w, CURSOR);
    ctx.kill(scratch);
    if split {
        let right = ctx.split_window(ctx.focused().unwrap(), Side::Right);
        let other = ctx.create_text("*other*", "beside them");
        ctx.focus(right.unwrap());
        ctx.show(other, ShowIn::Current).unwrap();
    }
    ctx.save_session().unwrap();
    dir
}

fn on_screen(ctx: &Ctx) -> String {
    let buf = ctx.focused_buffer().unwrap();
    ctx.buffer_name(buf).unwrap().to_owned()
}

/// Every buffer's name, in name order.
fn names(ctx: &Ctx) -> Vec<String> {
    let all = ctx.buffer_list().into_iter();
    let mut names: Vec<String> = all
        .filter_map(|b| ctx.buffer_name(b).map(String::from))
        .collect();
    names.sort();
    names
}

/// What each window shows, left to right.
fn windows(ctx: &Ctx) -> Vec<String> {
    let shown = |w| ctx.buffer_name(ctx.window_buffer(w)?).map(String::from);
    ctx.window_list().into_iter().filter_map(shown).collect()
}

#[test]
fn the_jellyfish_a_launch_opens_on_is_where_die_started_and_not_where_what_it_covers_is() {
    // A DIE you quit on a buffer somewhere else, as in your notes' directory.
    let dir = Dir::new("where");
    let mut plain = config(vec![]);
    plain.core.session = Some(dir.0.clone());
    let mut ctx = die::start(plain).unwrap();
    let scratch = ctx.focused_buffer().unwrap();
    ctx.set_directory(scratch, &dir.0);
    ctx.save_session().unwrap();
    drop(ctx);

    let mut ctx = start_over(&dir.0, &Launch::default());
    assert_eq!(on_screen(&ctx), NAME);
    let jellyfish = ctx.focused_buffer().unwrap();
    let started = die::started_dir();
    assert_ne!(started, dir.0);
    assert_eq!(ctx.directory(jellyfish), Some(started.as_path()));
    // What it covers is as the session had it: `die::app::at_launch` is what moves `*scratch*`.
    ctx.type_keys("q");
    let under = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.directory(under), Some(dir.0.as_path()));
    // One of yours, asked for there, is there, as any new buffer is.
    ctx.type_keys(":jellyfish<CR>");
    assert_eq!(on_screen(&ctx), NAME);
    let asked = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.directory(asked), Some(dir.0.as_path()));
}

#[test]
fn a_fresh_start_with_no_file_opens_on_the_jellyfish_and_a_file_a_check_or_a_restart_doesnt() {
    let fresh = Launch::default();
    let mut ctx = die::start(greeted(config(vec![]), &fresh)).unwrap();
    assert_eq!(on_screen(&ctx), NAME);
    assert_eq!(names(&ctx), [NAME], "alone: no scratch was made beside it");
    let w = ctx.focused().unwrap();
    assert_eq!(
        ctx.window_size(w),
        Some((80, 24)),
        "as the empty buffer's window was"
    );
    assert!(ctx.next_timer().is_some(), "and it swims");
    // q leaves DIE as it started before there was a jellyfish: on an empty buffer, at rest.
    ctx.type_keys("q");
    assert_eq!(names(&ctx), ["*scratch*"]);
    assert_eq!(ctx.next_timer(), None);

    // A file named: the file is on screen, and no jellyfish was made behind it.
    let dir = Dir::new("named");
    let file = dir.0.join("named.txt");
    std::fs::write(&file, "named\n").unwrap();
    let ctx = die::start(greeted(config(vec![file]), &fresh)).unwrap();
    assert_eq!(names(&ctx), ["named.txt"]);

    // `--check` takes `config` as it is, as `main` gives it and as every other test builds it.
    let mut ctx = die::start(config(vec![])).unwrap();
    assert_eq!(names(&ctx), ["*scratch*"]);
    assert_eq!(ctx.next_timer(), None, "nothing moves under a check");

    // A restart shows what was there, whichever kind it is.
    let kept = session("restart", false);
    for restart in restarts() {
        let ctx = start_over(&kept.0, &restart);
        assert_eq!(windows(&ctx), ["*notes*"], "{restart:?}");
        assert_eq!(names(&ctx), ["*notes*"], "{restart:?}");
    }
    // One that brought nothing back, as after a DIE that had the jellyfish alone, whose session
    // holds no buffer, gets it where the empty buffer would have been.
    let alone = Dir::new("alone");
    let mut ctx = start_over(&alone.0, &fresh);
    assert_eq!(names(&ctx), [NAME]);
    ctx.save_session().unwrap();
    drop(ctx);
    let ctx = start_over(&alone.0, &restarts()[0]);
    assert_eq!(windows(&ctx), [NAME]);
    assert_eq!(names(&ctx), [NAME]);
}

#[test]
fn over_a_session_it_covers_one_window_never_a_split_and_the_next_start_brings_the_buffer_back() {
    let fresh = Launch::default();
    let one = session("one", false);
    let mut ctx = start_over(&one.0, &fresh);
    assert_eq!(
        windows(&ctx),
        [NAME],
        "it took the one window, and opened none"
    );
    assert_eq!(
        names(&ctx),
        [NAME, "*notes*"],
        "what came back is all there"
    );
    // You quit, or `:rebuild`, with the jellyfish still up: that's the session the next DIE reads.
    ctx.save_session().unwrap();
    // q uncovers the buffer beneath, as it was.
    ctx.type_keys("q");
    assert_eq!(windows(&ctx), ["*notes*"]);
    let notes = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.text(notes).unwrap().rope().to_string(), "unsaved words");
    assert_eq!(cursor(&ctx), Some(CURSOR));
    drop(ctx);

    // The session kept nothing of the jellyfish, and the window it covered cost you nothing: a
    // restart is on your notes in one window, and the next fresh launch covers them again.
    let ctx = start_over(&one.0, &restarts()[0]);
    assert_eq!(windows(&ctx), ["*notes*"]);
    assert_eq!(names(&ctx), ["*notes*"]);
    assert_eq!(cursor(&ctx), Some(CURSOR), "where you were in them");
    drop(ctx);
    let ctx = start_over(&one.0, &fresh);
    assert_eq!(windows(&ctx), [NAME]);
    assert_eq!(names(&ctx), [NAME, "*notes*"]);

    // Splits come back as you left them, with no jellyfish made: it would take one of them, and a
    // window on it doesn't come back.
    let two = session("two", true);
    let ctx = start_over(&two.0, &fresh);
    assert_eq!(windows(&ctx), ["*notes*", "*other*"]);
    assert_eq!(names(&ctx), ["*notes*", "*other*"]);
}
