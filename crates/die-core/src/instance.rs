//! One DIE at a time, and the handover from one to the next. A lock file keeps the one
//! instance; the OS drops the lock when its holder dies, however it dies. On a restart the
//! old instance listens on a Unix socket and the new one connects as soon as it starts,
//! says when its first frame is up, and takes the lock once the old one has gone. If the
//! connection closes with nothing said, the new one died, and the old one carries on. A new
//! one that was too late finds the lock still held, and goes.
//!
//! A `die file` from a terminal reaches the holder over a second socket, `open.sock`. Only the
//! lock's holder binds it, once its first frame is up, and it stops listening before a restart
//! writes the session, so that a file can't land in what's being handed over. Nobody listening
//! with the lock held is an instance starting or handing over, and the sender tries again. Nobody
//! listening with the lock free is no instance, whatever file a crash left behind.
//!
//! A `die --wait` keeps its connection after the answer, and the instance says on it how the wait
//! ended: `done` once you're done with the files, or `failed`. A restart says `moved`, with what
//! the client still waits on, and the client asks the next instance to wait on that `again`. If
//! the instance goes first, quitting or crashing, the connection closes with nothing said.

use crate::clients::{Outcome, Waited};
use crate::files::FileTarget;
use std::ffi::{OsStr, OsString};
use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Seek, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle, sleep};
use std::time::{Duration, Instant};

const FIRST_FRAME: &str = "first-frame\n";

/// The socket a `die file` reaches the running instance on, in the state directory.
pub const OPEN_SOCKET: &str = "open.sock";
const OPENED: &[u8] = b"opened\n";
const DONE: &[u8] = b"done\n";
const FAILED: &[u8] = b"failed\n";
/// Then [`AGAIN`]'s request, which the client sends the next instance.
const MOVED: &[u8] = b"moved\n";
/// The mode of a request to wait again on files a restart moved.
const AGAIN: &[u8] = b"again";
const MADE: &[u8] = b"made";
const KEPT: &[u8] = b"kept";
const FRONT: &str = "front";
const BACKGROUND: &str = "background";
/// After the mode, in its field, for a request that waits. An instance that doesn't know it
/// refuses the request, rather than open a file of that name.
const WAIT: &str = " wait";
/// More than any command line, so a stray writer can't fill the instance's memory.
const MOST_REQUEST: u64 = 16 << 20;
/// How long the listener waits for a client to finish saying what it wants.
const REQUEST_LIMIT: Duration = Duration::from_secs(1);
/// How often a sender tries again while an instance holds the lock and isn't listening.
const SEND_POLL: Duration = Duration::from_millis(10);

/// How often `Handover::wait` looks for the connection.
const POLL: Duration = Duration::from_millis(1);

fn make_dir(dir: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

fn lock_file(dir: &Path) -> io::Result<File> {
    make_dir(dir)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("lock"))
}

/// Holds the lock while it lives. The file says which process does.
#[derive(Debug)]
pub struct Instance {
    _file: File,
}

impl Instance {
    /// Take the lock in `dir`. None if another instance has it.
    pub fn try_lock(dir: &Path) -> io::Result<Option<Instance>> {
        let file = lock_file(dir)?;
        match file.try_lock() {
            Ok(()) => Instance::held(file).map(Some),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }

    /// Take the lock in `dir` from an old instance whose handover has closed. Its lock goes
    /// when it does, so one still held after `patience` says it chose to carry on, and None
    /// says this instance should go.
    pub fn take_over(dir: &Path, patience: Duration) -> io::Result<Option<Instance>> {
        let deadline = Instant::now() + patience;
        loop {
            match Instance::try_lock(dir)? {
                None if Instant::now() < deadline => sleep(POLL),
                result => return Ok(result),
            }
        }
    }

    fn held(mut file: File) -> io::Result<Instance> {
        file.set_len(0)?;
        file.rewind()?;
        writeln!(file, "{}", std::process::id())?;
        Ok(Instance { _file: file })
    }
}

/// The old instance's side of a restart. The new one can tell it has gone when this drops.
#[derive(Debug)]
pub struct Handover {
    listener: UnixListener,
    path: PathBuf,
    successor: Option<UnixStream>,
}

impl Handover {
    /// Listen on a socket in `dir`, for this process alone.
    pub fn bind(dir: &Path) -> io::Result<Handover> {
        make_dir(dir)?;
        let path = dir.join(format!("handover-{}.sock", std::process::id()));
        // One left by a process of this pid that died, or a restart that failed.
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        Ok(Handover {
            listener,
            path,
            successor: None,
        })
    }

    /// What the new instance connects to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Wait for the new instance's first frame. An error says that it died or ran out of
    /// time, and this instance should carry on.
    pub fn wait(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let late = || "the new instance had no first frame in time".to_owned();
        let died = || "the new instance died before its first frame".to_owned();
        let stream = loop {
            match self.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(late());
                    }
                    sleep(POLL);
                }
                Err(e) => return Err(e.to_string()),
            }
        };
        let left = deadline.saturating_duration_since(Instant::now());
        let setup = stream
            .set_nonblocking(false)
            .and_then(|()| stream.set_read_timeout(Some(left.max(POLL))));
        // macOS refuses these with EINVAL on a socket whose other end has already closed.
        setup.map_err(|_| died())?;
        let mut line = String::new();
        match BufReader::new(&stream).read_line(&mut line) {
            Ok(_) if line == FIRST_FRAME => {
                // This process won't drop the handover: it exits holding it. The connection
                // stands without the name, so each restart doesn't leave a file behind.
                let _ = fs::remove_file(&self.path);
                self.successor = Some(stream);
                Ok(())
            }
            Ok(_) => Err(died()),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                Err(late())
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

impl Drop for Handover {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// The new instance's side of a restart.
#[derive(Debug)]
pub struct Successor {
    stream: UnixStream,
}

impl Successor {
    /// Connect to the old instance, which is listening at `path`. Do it early, so that dying
    /// before the first frame closes the connection.
    pub fn connect(path: &Path) -> io::Result<Successor> {
        Ok(Successor {
            stream: UnixStream::connect(path)?,
        })
    }

    /// Say the first frame is up, and wait until the old instance has gone.
    pub fn first_frame(mut self) -> io::Result<()> {
        self.stream.write_all(FIRST_FRAME.as_bytes())?;
        let mut rest = vec![];
        self.stream.read_to_end(&mut rest)?;
        Ok(())
    }
}

/// What a `die file` asks the running instance to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OpenRequest {
    /// Come to the front: everything but `--background`.
    pub front: bool,
    /// Wait until you're done with the files: `die --wait`.
    pub wait: bool,
    /// The files, whose paths are absolute.
    pub targets: Vec<FileTarget>,
}

impl OpenRequest {
    /// `front` or `background`, with [`WAIT`] for one that waits, then the targets' arguments,
    /// each ended by a NUL.
    fn to_bytes(&self) -> Vec<u8> {
        let mut mode = OsString::from(if self.front { FRONT } else { BACKGROUND });
        if self.wait {
            mode.push(WAIT);
        }
        let args = self.targets.iter().flat_map(FileTarget::to_args);
        let mut bytes = vec![];
        for arg in std::iter::once(mode).chain(args) {
            bytes.extend_from_slice(arg.as_bytes());
            bytes.push(0);
        }
        bytes
    }

    fn from_bytes(bytes: &[u8]) -> Option<OpenRequest> {
        let mut fields = bytes.strip_suffix(b"\0")?.split(|c| *c == 0);
        let mode = OsStr::from_bytes(fields.next()?).to_str()?;
        let (mode, wait) = mode.strip_suffix(WAIT).map_or((mode, false), |m| (m, true));
        let front = match mode {
            FRONT => true,
            BACKGROUND => false,
            _ => return None,
        };
        let args = fields.map(|f| OsString::from_vec(f.to_vec()));
        let targets = FileTarget::parse_args(args, Path::new("/")).ok()?;
        Some(OpenRequest {
            front,
            wait,
            targets,
        })
    }
}

/// A request as the instance got it, with the connection to answer on. Dropped unanswered, it
/// tells the sender it wasn't carried out.
#[derive(Debug)]
pub struct Asked {
    /// What it asks.
    pub request: OpenRequest,
    /// What a client a restart moved here waits on: then `request` waits, and has no targets.
    pub again: Option<Vec<Waited>>,
    stream: UnixStream,
}

impl Asked {
    /// Say it was carried out. A request that waits keeps the connection, to say when you're done.
    pub fn answer(mut self) -> Option<Client> {
        let said = self.stream.write_all(OPENED).is_ok();
        (said && self.request.wait).then_some(Client(self.stream))
    }
}

/// A `die --wait` waiting on this instance. Dropped untold, as when the instance goes, it hears
/// nothing more.
#[derive(Debug)]
pub struct Client(UnixStream);

impl Client {
    /// Tell it how its wait ended. One that has gone isn't told.
    pub fn tell(mut self, outcome: &Outcome) {
        let said = match outcome {
            Outcome::Done => DONE.to_vec(),
            Outcome::Failed => FAILED.to_vec(),
            Outcome::Moved(waited) => [MOVED, &again_bytes(waited)].concat(),
        };
        let _ = self.0.write_all(&said);
    }
}

/// [`AGAIN`], then each file's path after `made` or `kept`, each ended by a NUL.
fn again_bytes(waited: &[Waited]) -> Vec<u8> {
    let mut bytes = [AGAIN, b"\0"].concat();
    for w in waited {
        let made = if w.made { MADE } else { KEPT };
        bytes.extend([made, w.path.as_os_str().as_bytes(), b"\0"].concat());
    }
    bytes
}

fn again_from(bytes: &[u8]) -> Option<Vec<Waited>> {
    let mut fields = bytes.strip_suffix(b"\0")?.split(|c| *c == 0);
    (fields.next()? == AGAIN).then_some(())?;
    let waited = |field: &[u8]| {
        let (made, path) = match (field.strip_prefix(MADE), field.strip_prefix(KEPT)) {
            (Some(path), _) => (true, path),
            (_, Some(path)) => (false, path),
            _ => return None,
        };
        let path = PathBuf::from(OsStr::from_bytes(path));
        path.is_absolute().then_some(Waited { path, made })
    };
    fields.map(waited).collect()
}

/// The instance's side of `die file`. It stops listening when it drops.
#[derive(Debug)]
pub struct OpenListener {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OpenListener {
    /// Listen in `dir`, on a thread of its own, and pass each request to `handle`, which answers
    /// it once it's carried out. Hold the lock before calling this: it replaces any socket there.
    pub fn start(
        dir: &Path,
        mut handle: impl FnMut(Asked) + Send + 'static,
    ) -> io::Result<OpenListener> {
        make_dir(dir)?;
        let path = dir.join(OPEN_SOCKET);
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    return;
                }
                // A client that's gone can try again, and opening a file twice is harmless.
                if let Ok(Some(asked)) = stream.and_then(read_request) {
                    handle(asked);
                }
            }
        });
        Ok(OpenListener {
            path,
            stop,
            thread: Some(thread),
        })
    }
}

fn read_request(mut stream: UnixStream) -> io::Result<Option<Asked>> {
    stream.set_read_timeout(Some(REQUEST_LIMIT))?;
    // A client that reads nothing can't hold the main thread, which answers.
    stream.set_write_timeout(Some(REQUEST_LIMIT))?;
    let mut bytes = vec![];
    Read::by_ref(&mut stream)
        .take(MOST_REQUEST)
        .read_to_end(&mut bytes)?;
    let again = again_from(&bytes);
    let request = match again {
        Some(_) => Some(OpenRequest {
            front: false,
            wait: true,
            targets: vec![],
        }),
        None => OpenRequest::from_bytes(&bytes),
    };
    Ok(request.map(|request| Asked {
        request,
        again,
        stream,
    }))
}

impl Drop for OpenListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the thread from its `accept`. If the socket's file has gone, nothing can, and the
        // thread stays there with nobody able to reach it.
        if UnixStream::connect(&self.path).is_ok()
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

/// What came of [`send_open`].
#[derive(Debug)]
pub enum Sent {
    /// The running instance carried it out. A request that waits has the connection to wait on.
    Delivered(Option<Waiting>),
    /// Nothing holds the lock, so there's no instance to ask.
    NoInstance,
}

/// A `die --wait` whose files are open, waiting on the instance.
#[derive(Debug)]
pub struct Waiting(UnixStream);

impl Waiting {
    /// Wait until the instance says how the wait ended. None if it went first, as when it quits
    /// or crashes.
    pub fn wait(mut self) -> Option<Outcome> {
        let mut said = vec![];
        // macOS refuses this with EINVAL once the instance has closed, which it may have done
        // already, and then there's nothing to wait for.
        let _ = self.0.set_read_timeout(None);
        self.0.read_to_end(&mut said).ok()?;
        match said.as_slice() {
            DONE => Some(Outcome::Done),
            FAILED => Some(Outcome::Failed),
            said => again_from(said.strip_prefix(MOVED)?).map(Outcome::Moved),
        }
    }
}

/// Ask the instance in `dir` to open these. While one holds the lock and doesn't answer, as it
/// doesn't while it starts or hands over to a restart, try again for `patience`. Then an error
/// says nobody answered.
pub fn send_open(dir: &Path, request: &OpenRequest, patience: Duration) -> io::Result<Sent> {
    send(dir, &request.to_bytes(), request.wait, patience)
}

/// Ask the instance in `dir` to wait on these again, for a client that a restart moved
/// ([`Outcome::Moved`]), as [`send_open`] asks.
pub fn send_again(dir: &Path, waited: &[Waited], patience: Duration) -> io::Result<Sent> {
    send(dir, &again_bytes(waited), true, patience)
}

fn send(dir: &Path, bytes: &[u8], wait: bool, patience: Duration) -> io::Result<Sent> {
    let deadline = Instant::now() + patience;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let socket = dir.join(OPEN_SOCKET);
        if let Ok(stream) = deliver(&socket, bytes, left.max(SEND_POLL)) {
            return Ok(Sent::Delivered(wait.then_some(Waiting(stream))));
        }
        if !lock_is_held(dir)? {
            return Ok(Sent::NoInstance);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                ErrorKind::TimedOut,
                "the running DIE didn't answer",
            ));
        }
        sleep(SEND_POLL);
    }
}

/// Send the request and read the answer. The connection, which a request that waits goes on with.
fn deliver(socket: &Path, bytes: &[u8], limit: Duration) -> io::Result<UnixStream> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(limit))?;
    // A long command line doesn't fit the socket's buffer, and a stuck instance reads nothing.
    stream.set_write_timeout(Some(limit))?;
    stream.write_all(bytes)?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut answer = [0; OPENED.len()];
    stream.read_exact(&mut answer)?;
    if answer == OPENED {
        Ok(stream)
    } else {
        Err(ErrorKind::ConnectionAborted.into())
    }
}

/// Whether an instance holds the lock. It takes the lock for a moment to find out and leaves the
/// file alone, so an instance starting just then needs [`Instance::take_over`]'s patience.
fn lock_is_held(dir: &Path) -> io::Result<bool> {
    match lock_file(dir)?.try_lock() {
        Ok(()) => Ok(false),
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(e)) => Err(e),
    }
}
