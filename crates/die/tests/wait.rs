//! `die --wait` from the socket to the editor and back, with no window: the client
//! (`die::app::client_in`) sends its file, the instance's side (`die::app::carry_out`) opens it for
//! the client, and the client's exit code says how its wait ended. Each test has a lock and a
//! socket of its own, in a directory of its own, and stands in for the instance itself.

use die_test_cap as _;

use die::app::{Launch, carry_out, client_in};
use die::core::instance::{Asked, Instance, OpenListener, OpenRequest, Sent, send_open};
use die::core::{Ctx, FileTarget};
use die::{Config, start};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::Duration;

const PATIENCE: Duration = Duration::from_secs(5);

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("die-wait-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An instance's side: the lock, and a listener that hands on what it's asked.
struct Listening {
    _held: Instance,
    _listener: OpenListener,
    asked: Receiver<Asked>,
}

/// A client that finds nobody listening takes the lock for a moment to see if it's held.
fn lock(dir: &Path) -> Instance {
    Instance::take_over(dir, PATIENCE).unwrap().unwrap()
}

fn listen(dir: &Path, held: Instance) -> Listening {
    let (tx, asked) = channel::<Asked>();
    let listener = OpenListener::start(dir, move |a| drop(tx.send(a))).unwrap();
    Listening {
        _held: held,
        _listener: listener,
        asked,
    }
}

fn instance(dir: &Path) -> Listening {
    listen(dir, lock(dir))
}

/// `die --background --wait path` on a thread of its own: its exit code, once it returns.
fn die_wait(dir: &Path, path: &Path) -> Receiver<i32> {
    let asked = Launch {
        background: true,
        wait: true,
        ..Launch::default()
    };
    let (dir, path, (tx, rx)) = (dir.to_path_buf(), path.to_path_buf(), channel());
    // The test holds the lock, so the client never starts an instance from this bundle.
    let bundle = PathBuf::from("/nonexistent/DIE.app");
    thread::spawn(move || {
        tx.send(client_in(&dir, &bundle, vec![path], &asked))
            .unwrap()
    });
    rx
}

fn editor() -> Ctx {
    start(Config::default()).unwrap()
}

fn keys(ctx: &mut Ctx, keys: &str) {
    ctx.type_keys(keys);
    ctx.run_until_idle();
}

fn still_waits(over: &Receiver<i32>) -> bool {
    over.recv_timeout(Duration::from_millis(200)).is_err()
}

#[test]
fn a_client_that_waits_returns_0_once_wq_writes_its_file() {
    let dir = Dir::new("wq");
    let file = dir.file("COMMIT_EDITMSG", "\n# Please enter the commit message\n");
    let die = instance(&dir.0);
    let mut ctx = editor();
    let before = ctx.focused_buffer().unwrap();

    let over = die_wait(&dir.0, &file);
    carry_out(&mut ctx, die.asked.recv_timeout(PATIENCE).unwrap());
    let shown = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.file(shown).unwrap().path, file);
    assert!(still_waits(&over));

    keys(&mut ctx, "ccFix the wait<Esc>:wq<CR>");
    assert_eq!(over.recv_timeout(PATIENCE), Ok(0));
    let written = std::fs::read_to_string(&file).unwrap();
    assert!(written.starts_with("Fix the wait\n"), "{written}");
    assert_eq!(ctx.focused_buffer(), Some(before), "the window goes back");
    assert!(!ctx.quit_wanted());

    // Plain `die file` returns as it opens.
    let request = OpenRequest {
        front: false,
        wait: false,
        targets: vec![FileTarget {
            path: file.clone(),
            line: None,
            col: None,
        }],
    };
    let plain = thread::spawn({
        let dir = dir.0.clone();
        move || {
            matches!(
                send_open(&dir, &request, PATIENCE),
                Ok(Sent::Delivered(None))
            )
        }
    });
    carry_out(&mut ctx, die.asked.recv_timeout(PATIENCE).unwrap());
    assert!(plain.join().unwrap());
    let shown = ctx.focused_buffer().unwrap();
    assert!(!ctx.is_client_buffer(shown));
}

/// DIE quitting or crashing with the client waiting fails it, so git stops rather than go on with
/// a message or a rebase's list saved half done. So does `:cq`, and a file that won't open.
#[test]
fn a_client_that_waits_returns_1_if_die_goes_first_or_you_give_up() {
    let dir = Dir::new("gone");
    let file = dir.file("git-rebase-todo", "pick 1234 one\n");
    let die = instance(&dir.0);
    let mut ctx = editor();
    let over = die_wait(&dir.0, &file);
    carry_out(&mut ctx, die.asked.recv_timeout(PATIENCE).unwrap());
    keys(&mut ctx, "ccdrop 1234 one<Esc>:w<CR>");
    assert!(still_waits(&over));
    drop(ctx);
    assert_eq!(over.recv_timeout(PATIENCE), Ok(1));

    let mut ctx = editor();
    let over = die_wait(&dir.0, &file);
    carry_out(&mut ctx, die.asked.recv_timeout(PATIENCE).unwrap());
    keys(&mut ctx, ":cq<CR>");
    assert_eq!(over.recv_timeout(PATIENCE), Ok(1));
    assert!(!ctx.quit_wanted());

    let over = die_wait(&dir.0, &dir.0);
    carry_out(&mut ctx, die.asked.recv_timeout(PATIENCE).unwrap());
    assert_eq!(over.recv_timeout(PATIENCE), Ok(1), "a directory won't open");
}

/// A restart: the old instance stops listening, tells the client what it waits on, and goes, and
/// for a moment nobody holds the lock. The client asks the next instance, which has the file open
/// from the session, to wait on it again, and `:wq` there returns it with 0.
#[test]
fn a_restart_hands_a_waiting_client_to_the_next_instance() {
    let dir = Dir::new("restart");
    let file = dir.file("COMMIT_EDITMSG", "\n");
    let old = instance(&dir.0);
    let mut ctx = editor();
    let over = die_wait(&dir.0, &file);
    carry_out(&mut ctx, old.asked.recv_timeout(PATIENCE).unwrap());
    let Listening {
        _held: held,
        _listener: listener,
        ..
    } = old;
    drop(listener);
    ctx.hand_over_clients();
    drop((ctx, held));
    thread::sleep(Duration::from_millis(300));
    assert!(still_waits(&over));

    let mut next = editor();
    let restored = next.open_file(&file).unwrap();
    let before = next.focused_buffer().unwrap();
    let die = instance(&dir.0);
    carry_out(&mut next, die.asked.recv_timeout(PATIENCE).unwrap());
    assert!(next.is_client_buffer(restored));
    assert_eq!(
        next.focused_buffer(),
        Some(before),
        "nothing moves on screen"
    );
    assert!(still_waits(&over));
    next.show(restored, die::core::ShowIn::Current).unwrap();
    keys(&mut next, "ccRestarted<Esc>:wq<CR>");
    assert_eq!(over.recv_timeout(PATIENCE), Ok(0));
    assert!(!next.is_live(restored), "made for the client, it goes");
}

/// An instance that holds the lock and isn't listening yet, as one starting with a big session
/// or a restart handing over, is waited out for longer than `die file`'s 2 s, so that git isn't
/// told the editor failed.
#[test]
fn a_client_that_waits_waits_out_an_instance_that_is_not_listening_yet() {
    let dir = Dir::new("slow");
    let file = dir.file("msg", "\n");
    let held = lock(&dir.0);
    let over = die_wait(&dir.0, &file);
    thread::sleep(Duration::from_secs(3));
    assert!(still_waits(&over), "still trying");
    let die = listen(&dir.0, held);
    let mut ctx = editor();
    carry_out(&mut ctx, die.asked.recv_timeout(PATIENCE).unwrap());
    keys(&mut ctx, ":wq<CR>");
    assert_eq!(over.recv_timeout(PATIENCE), Ok(0));
}
