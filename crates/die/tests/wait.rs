//! `die --wait` from the socket to the editor and back, with no window: the instance's side
//! (`die::app::carry_out`) opens the file for a client, which returns once `:wq` says you're done.
//! Each test has a lock and a socket of its own, in a directory of its own.

use die_test_cap as _;

use die::app::carry_out;
use die::core::FileTarget;
use die::core::instance::{Asked, Instance, OpenListener, OpenRequest, Sent, send_open};
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
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Send `die [--wait] path` from a thread of its own: what came of it, once it's over. For one
/// that waits, whether you were done.
fn client(dir: &Path, path: &Path, wait: bool) -> Receiver<Option<bool>> {
    let request = OpenRequest {
        front: false,
        wait,
        targets: vec![FileTarget {
            path: path.to_path_buf(),
            line: None,
            col: None,
        }],
    };
    let (dir, (tx, rx)) = (dir.to_path_buf(), channel());
    thread::spawn(move || match send_open(&dir, &request, PATIENCE).unwrap() {
        Sent::Delivered(waiting) => tx.send(waiting.map(|w| w.wait())).unwrap(),
        Sent::NoInstance => panic!("the test holds the lock"),
    });
    rx
}

#[test]
fn a_client_that_waits_returns_once_wq_writes_its_file() {
    let dir = Dir::new("wq");
    let file = dir.0.join("COMMIT_EDITMSG");
    std::fs::write(&file, "\n# Please enter the commit message\n").unwrap();
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let (tx, asked) = channel::<Asked>();
    let _listener = OpenListener::start(&dir.0, move |a| tx.send(a).unwrap()).unwrap();
    let mut ctx = start(Config::default()).unwrap();
    let before = ctx.focused_buffer().unwrap();

    let over = client(&dir.0, &file, true);
    carry_out(&mut ctx, asked.recv_timeout(PATIENCE).unwrap());
    let shown = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.file(shown).unwrap().path, file);
    assert!(
        over.recv_timeout(Duration::from_millis(200)).is_err(),
        "it waits"
    );

    ctx.type_keys("ccFix the wait<Esc>:wq<CR>");
    ctx.run_until_idle();
    assert_eq!(over.recv_timeout(PATIENCE), Ok(Some(true)));
    let written = std::fs::read_to_string(&file).unwrap();
    assert!(written.starts_with("Fix the wait\n"), "{written}");
    assert_eq!(ctx.focused_buffer(), Some(before), "the window goes back");
    assert!(!ctx.quit_wanted());

    // Plain `die file` returns as it opens.
    let plain = client(&dir.0, &file, false);
    carry_out(&mut ctx, asked.recv_timeout(PATIENCE).unwrap());
    assert_eq!(plain.recv_timeout(PATIENCE), Ok(None));
    let shown = ctx.focused_buffer().unwrap();
    assert!(!ctx.is_client_buffer(shown));
}

/// An editor that goes with the client waiting, as DIE does when it quits or restarts.
#[test]
fn a_client_that_waits_returns_when_the_editor_goes() {
    let dir = Dir::new("gone");
    let file = dir.0.join("git-rebase-todo");
    std::fs::write(&file, "pick 1234 one\n").unwrap();
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let (tx, asked) = channel::<Asked>();
    let _listener = OpenListener::start(&dir.0, move |a| tx.send(a).unwrap()).unwrap();
    let mut ctx = start(Config::default()).unwrap();
    let over = client(&dir.0, &file, true);
    carry_out(&mut ctx, asked.recv_timeout(PATIENCE).unwrap());
    assert!(over.recv_timeout(Duration::from_millis(200)).is_err());
    drop(ctx);
    assert_eq!(over.recv_timeout(PATIENCE), Ok(Some(false)));
}
