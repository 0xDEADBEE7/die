//! One DIE at a time, and the handover from one to the next. A lock file keeps the one
//! instance; the OS drops the lock when its holder dies, however it dies. On a restart the
//! old instance listens on a Unix socket and the new one connects as soon as it starts,
//! says when its first frame is up, and takes the lock once the old one has gone. If the
//! connection closes with nothing said, the new one died, and the old one carries on.

use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Seek, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

const FIRST_FRAME: &str = "first-frame\n";

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

    /// Take the lock in `dir`, waiting for whoever has it.
    pub fn lock(dir: &Path) -> io::Result<Instance> {
        let file = lock_file(dir)?;
        file.lock()?;
        Instance::held(file)
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
        setup.map_err(|e| e.to_string())?;
        let mut line = String::new();
        match BufReader::new(&stream).read_line(&mut line) {
            Ok(_) if line == FIRST_FRAME => {
                self.successor = Some(stream);
                Ok(())
            }
            Ok(_) => Err("the new instance died before its first frame".to_owned()),
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
