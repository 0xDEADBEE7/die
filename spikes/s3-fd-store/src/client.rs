//! Client API for any DIE-owned process: the editor, a per-terminal engine helper (S2),
//! or a newer store taking over. Labels are free-form, namespaced by convention
//! (`term/<id>/pty`, `term/<id>/engine`). A GET returns a duplicate: the store keeps
//! its copy until DROP, and never reads from it.

use crate::proto::{self, Kv, Msg};
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The store answered HELLO with ERR (for example code=incompatible-major).
    Refused { code: String, store_proto: String, store_build: String, msg: String },
    Missing,
    Exited,
    Exists,
    #[allow(dead_code)] // shown through Debug
    Other(String),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub struct Store {
    sock: UnixStream,
    pub pid: i32,
    pub proto: String,
    pub build: String,
}

/// macOS limits socket paths to 104 bytes. Long store dirs are reached through a
/// relative path, so the caller's cwd must be the store dir.
fn sock_path(dir: &Path) -> io::Result<PathBuf> {
    let abs = dir.join(proto::SOCK);
    if abs.as_os_str().len() < 100 {
        return Ok(abs);
    }
    if std::env::current_dir()?.canonicalize()? == dir.canonicalize()? {
        return Ok(PathBuf::from(proto::SOCK));
    }
    Err(io::Error::other("store socket path is over 104 bytes; chdir to the store dir first"))
}

impl Store {
    pub fn connect(dir: &Path, role: &str) -> Result<Store, Error> {
        let sock = UnixStream::connect(sock_path(dir)?)?;
        sock.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut s = Store { sock, pid: 0, proto: String::new(), build: String::new() };
        let hello = Msg::new("HELLO").with("proto", proto::proto()).with("role", role).with("build", proto::BUILD).with("pid", std::process::id());
        let (r, _) = s.raw(hello, None)?;
        let field = |k: &str| r.get(k).unwrap_or("").to_string();
        if r.verb != "OK" {
            return Err(Error::Refused { code: field("code"), store_proto: field("proto"), store_build: field("build"), msg: field("msg") });
        }
        s.pid = field("pid").parse().unwrap_or(0);
        s.proto = field("proto");
        s.build = field("build");
        Ok(s)
    }

    /// Connect, starting a store first if none is running. `extra` goes to the store's argv.
    pub fn connect_or_spawn(dir: &Path, role: &str, exe: &Path, extra: &[&str]) -> Result<Store, Error> {
        match Store::connect(dir, role) {
            Err(Error::Io(e)) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {}
            r => return r,
        }
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--fd-store").arg("--dir").arg(dir).arg("--daemon").args(extra);
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null());
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        // The first process daemonizes after binding and exits, so wait() is short.
        let _ = cmd.spawn()?.wait();
        let t = Instant::now();
        loop {
            match Store::connect(dir, role) {
                Err(Error::Io(e)) if t.elapsed() < Duration::from_secs(3) => {
                    let _ = e;
                    std::thread::sleep(Duration::from_millis(10));
                }
                r => return r,
            }
        }
    }

    fn raw(&mut self, m: Msg, fd: Option<BorrowedFd<'_>>) -> Result<(Msg, Option<OwnedFd>), Error> {
        proto::send(self.sock.as_fd(), &m, fd)?;
        match proto::recv(self.sock.as_fd())? {
            Some(r) => Ok(r),
            None => Err(Error::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "store closed the connection"))),
        }
    }

    fn call(&mut self, m: Msg, fd: Option<BorrowedFd<'_>>) -> Result<(Msg, Option<OwnedFd>), Error> {
        let (r, f) = self.raw(m, fd)?;
        if r.verb == "OK" {
            return Ok((r, f));
        }
        Err(match r.get("code") {
            Some("missing") => Error::Missing,
            Some("exited") => Error::Exited,
            Some("exists") => Error::Exists,
            c => Error::Other(format!("{c:?}: {}", r.get("msg").unwrap_or(""))),
        })
    }

    /// Deposit a duplicate of `fd` under `label`, with opaque metadata the store keeps verbatim.
    pub fn put(&mut self, label: &str, fd: BorrowedFd<'_>, meta: &Kv, replace: bool) -> Result<(), Error> {
        let mut m = Msg::new("PUT").with("label", label);
        if replace {
            m.set("replace", "1");
        }
        m.kv.extend(meta.iter().cloned());
        self.call(m, Some(fd)).map(|_| ())
    }

    /// A duplicate of the fd plus its metadata.
    pub fn get(&mut self, label: &str) -> Result<(OwnedFd, Msg), Error> {
        let (r, f) = self.call(Msg::new("GET").with("label", label), None)?;
        Ok((f.ok_or_else(|| Error::Other("GET reply carried no fd".into()))?, r))
    }

    /// Every entry: label, state (live or exited) and metadata.
    pub fn list(&mut self) -> Result<Vec<Kv>, Error> {
        Ok(self.call(Msg::new("LIST"), None)?.0.items)
    }

    #[allow(dead_code)] // part of the API shape; the spike's hosts never resize
    pub fn update(&mut self, label: &str, meta: &Kv) -> Result<(), Error> {
        let mut m = Msg::new("UPDATE").with("label", label);
        m.kv.extend(meta.iter().cloned());
        self.call(m, None).map(|_| ())
    }

    /// Close the store's copy. With no other copy left, the shell gets SIGHUP.
    pub fn drop_label(&mut self, label: &str) -> Result<bool, Error> {
        Ok(self.call(Msg::new("DROP").with("label", label), None)?.0.get("existed") == Some("1"))
    }

    pub fn status(&mut self) -> Result<Msg, Error> {
        Ok(self.call(Msg::new("STATUS"), None)?.0)
    }

    /// Ask the store to close everything and exit. `expect` guards a takeover.
    pub fn exit(&mut self, expect: Option<usize>, unlink: bool) -> Result<(), Error> {
        let mut m = Msg::new("EXIT").with("unlink", unlink as u8);
        if let Some(n) = expect {
            m.set("expect", n);
        }
        self.call(m, None).map(|_| ())
    }
}

impl AsFd for Store {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.sock.as_fd()
    }
}
