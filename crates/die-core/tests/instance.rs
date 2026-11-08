//! `die file`'s socket: a request reaches the instance that holds the lock, waits out one that
//! isn't listening yet, and finds out when there's nobody. `die --wait` hears how its wait ended,
//! or that the instance went, and a restart moves it to the next.

use die_test_cap as _;

use die_core::instance::{
    Asked, Client, Instance, OPEN_SOCKET, OpenListener, OpenRequest, Sent, send_again, send_open,
};
use die_core::{FileTarget, Outcome, Waited};
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
        wait: false,
        targets: vec![
            target("/a b/c:9", Some(12), Some(3)),
            target("/plain", None, None),
        ],
    }
}

/// What a listener got: the request, and the client if it waits.
type Got = Receiver<(OpenRequest, Option<Client>)>;

/// A listener that answers each request at once and passes it on.
fn listen(dir: &Dir) -> (OpenListener, Got) {
    let (tx, rx) = channel();
    let listener = OpenListener::start(&dir.0, move |asked: Asked| {
        let request = asked.request.clone();
        let _ = tx.send((request, asked.answer()));
    });
    (listener.unwrap(), rx)
}

fn delivered(sent: Sent) -> bool {
    matches!(sent, Sent::Delivered(None))
}

const NO_PATIENCE: Duration = Duration::ZERO;
const PATIENCE: Duration = Duration::from_secs(2);

#[test]
fn a_request_arrives_whole_and_is_answered() {
    let dir = Dir::new("whole");
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let (_listener, got) = listen(&dir);
    assert!(delivered(send_open(&dir.0, &request(), PATIENCE).unwrap()));
    assert_eq!(got.recv().unwrap().0, request());
    let empty = OpenRequest {
        front: true,
        wait: false,
        targets: vec![],
    };
    send_open(&dir.0, &empty, PATIENCE).unwrap();
    assert_eq!(got.recv().unwrap().0, empty);
}

/// A crash leaves the socket's file behind, and that's no instance.
#[test]
fn with_the_lock_free_there_is_no_instance_whatever_file_is_there() {
    let dir = Dir::new("free");
    let sent = send_open(&dir.0, &request(), PATIENCE).unwrap();
    assert!(matches!(sent, Sent::NoInstance));
    let stale = std::os::unix::net::UnixListener::bind(dir.0.join(OPEN_SOCKET)).unwrap();
    drop(stale);
    assert!(dir.0.join(OPEN_SOCKET).exists());
    let sent = send_open(&dir.0, &request(), PATIENCE).unwrap();
    assert!(matches!(sent, Sent::NoInstance));
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
    assert!(delivered(sender.join().unwrap()));
    assert_eq!(got.recv().unwrap().0, request());
}

/// An instance whose loop has gone says nothing, so the sender doesn't think it's done.
#[test]
fn a_request_the_instance_can_not_carry_out_is_not_delivered() {
    let dir = Dir::new("refused");
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let _listener = OpenListener::start(&dir.0, drop).unwrap();
    assert!(send_open(&dir.0, &request(), Duration::from_millis(50)).is_err());
}

/// Something took the socket's file away, so nothing can wake the listener's thread: a restart,
/// which stops listening first, mustn't wait for it for ever.
#[test]
fn stopping_does_not_hang_when_the_sockets_file_has_gone() {
    let dir = Dir::new("gone");
    let (listener, _got) = listen(&dir);
    fs::remove_file(dir.0.join(OPEN_SOCKET)).unwrap();
    let (done, stopped) = channel();
    thread::spawn(move || {
        drop(listener);
        let _ = done.send(());
    });
    assert!(stopped.recv_timeout(Duration::from_secs(5)).is_ok());
}

/// A stuck instance reads nothing, and a long command line doesn't fit the socket's buffer: the
/// sender still gives up in its time, rather than hang in the owner's terminal.
#[test]
fn a_sender_gives_up_on_a_stuck_instance_even_with_a_long_request() {
    let dir = Dir::new("stuck");
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let (release, stuck) = channel::<()>();
    let _listener = OpenListener::start(&dir.0, move |asked: Asked| {
        if stuck.recv().is_ok() {
            asked.answer();
        }
    })
    .unwrap();
    let limit = Duration::from_millis(50);
    // This one is taken, and holds the listener's only thread.
    assert!(send_open(&dir.0, &request(), limit).is_err());
    let file = |i| FileTarget {
        path: PathBuf::from(format!("/long/{i}")),
        line: None,
        col: None,
    };
    let long = OpenRequest {
        front: false,
        wait: false,
        targets: (0..20_000).map(file).collect(),
    };
    let path = dir.0.clone();
    let (done, answer) = channel();
    thread::spawn(move || {
        let _ = done.send(send_open(&path, &long, limit).is_err());
    });
    let gave_up = answer.recv_timeout(Duration::from_secs(5));
    drop(release);
    assert_eq!(gave_up, Ok(true));
}

fn waiting() -> OpenRequest {
    OpenRequest {
        wait: true,
        ..request()
    }
}

/// Send a request that waits, from a thread of its own, and wait on it there: what came of the
/// wait comes back once it's over.
fn send_and_wait(dir: &Dir) -> Receiver<Option<Outcome>> {
    let (path, (done, over)) = (dir.0.clone(), channel());
    thread::spawn(move || {
        let Sent::Delivered(Some(waiting)) = send_open(&path, &waiting(), PATIENCE).unwrap() else {
            panic!("a request that waits gets the connection to wait on");
        };
        let _ = done.send(waiting.wait());
    });
    over
}

/// `die --wait`: the answer comes as the files open, and the client waits on until the instance
/// says you're done with them.
#[test]
fn a_waiting_client_returns_once_told_it_is_done() {
    let dir = Dir::new("wait");
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let (_listener, got) = listen(&dir);
    let over = send_and_wait(&dir);
    let (asked, client) = got.recv().unwrap();
    assert_eq!(asked, waiting(), "it arrives whole, wait and all");
    let still = over.recv_timeout(Duration::from_millis(100));
    assert!(still.is_err(), "it waits");
    client
        .expect("a request that waits keeps its client")
        .tell(&Outcome::Done);
    assert_eq!(
        over.recv_timeout(Duration::from_secs(5)),
        Ok(Some(Outcome::Done))
    );
}

/// The instance quits or crashes with the client waiting: it returns, and hears nothing.
#[test]
fn a_waiting_client_returns_when_the_instance_goes_untold() {
    let dir = Dir::new("gone-untold");
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let (listener, got) = listen(&dir);
    let over = send_and_wait(&dir);
    let (_, client) = got.recv().unwrap();
    assert!(over.recv_timeout(Duration::from_millis(100)).is_err());
    drop((client, listener));
    assert_eq!(over.recv_timeout(Duration::from_secs(5)), Ok(None));
}

/// Several wait at once, and each is told for itself.
#[test]
fn clients_wait_side_by_side_and_each_hears_only_its_own_done() {
    let dir = Dir::new("several");
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let (_listener, got) = listen(&dir);
    let first = send_and_wait(&dir);
    let (_, one) = got.recv().unwrap();
    let second = send_and_wait(&dir);
    let (_, two) = got.recv().unwrap();
    two.unwrap().tell(&Outcome::Failed);
    let told = second.recv_timeout(Duration::from_secs(5));
    assert_eq!(told, Ok(Some(Outcome::Failed)));
    assert!(first.recv_timeout(Duration::from_millis(100)).is_err());
    one.unwrap().tell(&Outcome::Done);
    let told = first.recv_timeout(Duration::from_secs(5));
    assert_eq!(told, Ok(Some(Outcome::Done)));
}

/// A restart tells the client what it waits on, and the client asks the next instance to wait on
/// that again: the request arrives as it was told, waiting, with paths that are odd as can be.
#[test]
fn a_restart_moves_a_waiting_client_to_the_next_instance() {
    let dir = Dir::new("moved");
    let _held = Instance::try_lock(&dir.0).unwrap().unwrap();
    let (old, got) = listen(&dir);
    let over = send_and_wait(&dir);
    let (_, client) = got.recv().unwrap();
    let waited = vec![
        Waited {
            path: PathBuf::from("/a b/COMMIT_EDITMSG"),
            made: true,
        },
        Waited {
            path: PathBuf::from("/made/kept:12\nagain"),
            made: false,
        },
    ];
    drop(old);
    client.unwrap().tell(&Outcome::Moved(waited.clone()));
    let told = over.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(told, Some(Outcome::Moved(waited.clone())));

    let (tx, again) = channel();
    let _new = OpenListener::start(&dir.0, move |asked: Asked| {
        let _ = tx.send((asked.request.clone(), asked.again.clone(), asked.answer()));
    })
    .unwrap();
    let Sent::Delivered(Some(waiting)) = send_again(&dir.0, &waited, PATIENCE).unwrap() else {
        panic!("asking again waits");
    };
    let (request, asked_again, client) = again.recv().unwrap();
    assert_eq!(asked_again, Some(waited));
    assert!(request.wait && !request.front && request.targets.is_empty());
    client.unwrap().tell(&Outcome::Done);
    assert_eq!(waiting.wait(), Some(Outcome::Done));
}
