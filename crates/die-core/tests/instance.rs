//! `die file`'s socket: a request reaches the instance that holds the lock, waits out one that
//! isn't listening yet, and finds out when there's nobody.

use die_test_cap as _;

use die_core::FileTarget;
use die_core::instance::{Instance, OPEN_SOCKET, OpenListener, OpenRequest, Sent, send_open};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::Duration;

/// A fresh directory for one test, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let name = format!("die-open-{}-{name}", std::process::id());
        let dir = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request() -> OpenRequest {
    let target = |path: &str, line, col| FileTarget {
        path: PathBuf::from(path),
        line,
        col,
    };
    OpenRequest {
        front: false,
        targets: vec![
            target("/a b/c:9", Some(12), Some(3)),
            target("/plain", None, None),
        ],
    }
}

fn listen(dir: &Dir) -> (OpenListener, Receiver<OpenRequest>) {
    let (tx, rx) = channel();
    let listener = OpenListener::start(&dir.0, move |r| tx.send(r).is_ok()).unwrap();
    (listener, rx)
}

const NO_PATIENCE: Duration = Duration::ZERO;
const PATIENCE: Duration = Duration::from_secs(2);

#[test]
fn a_request_arrives_whole_and_is_answered() {
    let dir = Dir::new("whole");
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let (_listener, got) = listen(&dir);
    assert_eq!(
        send_open(&dir.0, &request(), PATIENCE).unwrap(),
        Sent::Delivered
    );
    assert_eq!(got.recv().unwrap(), request());
    let empty = OpenRequest {
        front: true,
        targets: vec![],
    };
    send_open(&dir.0, &empty, PATIENCE).unwrap();
    assert_eq!(got.recv().unwrap(), empty);
}

/// A crash leaves the socket's file behind, and that's no instance.
#[test]
fn with_the_lock_free_there_is_no_instance_whatever_file_is_there() {
    let dir = Dir::new("free");
    assert_eq!(
        send_open(&dir.0, &request(), PATIENCE).unwrap(),
        Sent::NoInstance
    );
    let stale = std::os::unix::net::UnixListener::bind(dir.0.join(OPEN_SOCKET)).unwrap();
    drop(stale);
    assert!(dir.0.join(OPEN_SOCKET).exists());
    assert_eq!(
        send_open(&dir.0, &request(), PATIENCE).unwrap(),
        Sent::NoInstance
    );
    // And looking leaves the lock file's holder alone.
    let held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let pid = fs::read_to_string(dir.0.join("lock")).unwrap();
    let _ = send_open(&dir.0, &request(), NO_PATIENCE);
    assert_eq!(fs::read_to_string(dir.0.join("lock")).unwrap(), pid);
    drop(held);
}

/// A restart: the old listener has stopped and the lock is held. The new one starts listening
/// while the sender waits.
#[test]
fn a_sender_waits_for_the_next_listener_and_gives_up_after_its_patience() {
    let dir = Dir::new("handover");
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let (old, _) = listen(&dir);
    drop(old);
    assert!(send_open(&dir.0, &request(), NO_PATIENCE).is_err());
    let path = dir.0.clone();
    let sender = thread::spawn(move || send_open(&path, &request(), PATIENCE).unwrap());
    thread::sleep(Duration::from_millis(50));
    let (_new, got) = listen(&dir);
    assert_eq!(sender.join().unwrap(), Sent::Delivered);
    assert_eq!(got.recv().unwrap(), request());
}

/// An instance whose loop has gone says nothing, so the sender doesn't think it's done.
#[test]
fn a_request_the_instance_can_not_carry_out_is_not_delivered() {
    let dir = Dir::new("refused");
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let _listener = OpenListener::start(&dir.0, |_| false).unwrap();
    assert!(send_open(&dir.0, &request(), Duration::from_millis(50)).is_err());
}
