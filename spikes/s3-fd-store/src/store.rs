//! `die --fd-store`: holds labelled fds so PTYs outlive any DIE process.
//!
//! Lifetime: one store per user (flock on store.lock). It never exits while it holds a
//! live fd. It exits once it has held no fd and had no client connected for `idle_exit`.
//! It never reads or writes the fds it holds.

use crate::proto::{self, Kv, Msg};
use std::io;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct Opts {
    pub dir: PathBuf,
    pub idle_exit: Duration,
    pub sweep: Duration,
    pub daemon: bool,
    pub takeover: bool,
    pub expect_uid: Option<u32>,
}

struct Entry {
    label: String,
    fd: Option<OwnedFd>, // None once swept: a tombstone that says the other side went away
    meta: Kv,
    exited_ms: u128,
}

struct Client {
    sock: UnixStream,
    pid: i32,
    hello: bool,
    /// A store of another protocol major taking over: frozen verbs only.
    restricted: bool,
}

fn log(s: &str) {
    eprintln!("{} store[{}] {s}", crate::pty::now_ms(), std::process::id());
}

pub fn main(o: Opts) -> i32 {
    // The directory must be ours and private: 0700, owned by this uid.
    match std::fs::metadata(&o.dir) {
        Ok(m) if m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0 => {}
        Ok(m) => {
            eprintln!("fd-store: refusing {}: owner {} mode {:o} (need 0700, owned by us)", o.dir.display(), m.uid(), m.mode() & 0o7777);
            return 2;
        }
        Err(e) => {
            eprintln!("fd-store: {}: {e}", o.dir.display());
            return 2;
        }
    }
    std::env::set_current_dir(&o.dir).unwrap();
    let mut entries: Vec<Entry> = vec![];
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(proto::LOCK).unwrap();
    if o.takeover {
        match take_over(&mut entries) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("fd-store: takeover failed: {e}");
                return 4;
            }
        }
        // The old store exits after replying; wait for its lock.
        let t = Instant::now();
        while unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            if t.elapsed() > Duration::from_secs(5) {
                eprintln!("fd-store: old store did not release the lock");
                return 4;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    } else if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let pid = std::fs::read_to_string(proto::LOCK).unwrap_or_default();
        eprintln!("fd-store: already running (pid {})", pid.trim());
        return 3;
    }
    let _ = std::fs::remove_file(proto::SOCK); // stale after a crash; we hold the lock
    let old = unsafe { libc::umask(0o077) };
    let listener = UnixListener::bind(proto::SOCK).unwrap();
    unsafe { libc::umask(old) };
    if o.daemon {
        daemonize();
    }
    let _ = std::fs::write(proto::LOCK, format!("{}\n", std::process::id()));
    log(&format!("up: proto {} build {} idle_exit {:?} sweep {:?} held {} nofile {:?}", proto::proto(), proto::BUILD, o.idle_exit, o.sweep, entries.len(), crate::pty::nofile()));
    let code = serve(&o, listener, &mut entries);
    drop(lock);
    code
}

/// Detach: new session, stdio to /dev/null and store.log, and the parent returns at once
/// so the spawning host knows the socket is already bound.
fn daemonize() {
    unsafe {
        match libc::fork() {
            -1 => panic!("fork: {}", io::Error::last_os_error()),
            0 => {}
            _ => libc::_exit(0),
        }
        libc::setsid();
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        let logf = libc::open(c"store.log".as_ptr(), libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT, 0o600);
        libc::dup2(null, 0);
        libc::dup2(null, 1);
        libc::dup2(logf, 2);
        libc::close(null);
        libc::close(logf);
    }
}

fn serve(o: &Opts, listener: UnixListener, entries: &mut Vec<Entry>) -> i32 {
    let started = Instant::now();
    let mut clients: Vec<Client> = vec![];
    let mut idle_since: Option<Instant> = None;
    let mut next_sweep = Instant::now() + o.sweep;
    let me = unsafe { libc::geteuid() };
    loop {
        let live = entries.iter().filter(|e| e.fd.is_some()).count();
        // Timeout: the next sweep while we hold fds, else the idle deadline, else forever.
        let now = Instant::now();
        let timeout = if live > 0 {
            idle_since = None;
            Some(next_sweep.saturating_duration_since(now))
        } else if clients.is_empty() {
            let since = *idle_since.get_or_insert(now);
            let left = (since + o.idle_exit).saturating_duration_since(now);
            if left.is_zero() {
                log("idle: no fds and no clients; exiting");
                let _ = std::fs::remove_file(proto::SOCK);
                return 0;
            }
            Some(left)
        } else {
            idle_since = None;
            None
        };
        let mut pfds = vec![libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        pfds.extend(clients.iter().map(|c| libc::pollfd { fd: c.sock.as_raw_fd(), events: libc::POLLIN, revents: 0 }));
        let ms = timeout.map(|d| d.as_millis().clamp(1, i32::MAX as u128) as i32).unwrap_or(-1);
        let n = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as _, ms) };
        if n < 0 {
            continue; // EINTR
        }
        if Instant::now() >= next_sweep {
            sweep(entries);
            next_sweep = Instant::now() + o.sweep;
        }
        // Clients first (indices shift when we remove), then new connections.
        for i in (0..clients.len()).rev() {
            if pfds[i + 1].revents == 0 {
                continue;
            }
            let next = match proto::recv(clients[i].sock.as_fd()) {
                Ok(Some((m, fd))) => handle(&mut clients[i], m, fd, entries, started),
                Ok(None) => Next::Close,
                Err(e) => {
                    log(&format!("client pid {}: {e}", clients[i].pid));
                    Next::Close
                }
            };
            match next {
                Next::Keep => {}
                Next::Close => {
                    clients.remove(i);
                }
                Next::Exit { unlink } => {
                    log("EXIT: closing all fds");
                    entries.clear();
                    if unlink {
                        let _ = std::fs::remove_file(proto::SOCK);
                    }
                    return 0;
                }
            }
        }
        if pfds[0].revents != 0 {
            if let Ok((sock, _)) = listener.accept() {
                let (uid, pid) = proto::peer(sock.as_fd());
                if uid != o.expect_uid.unwrap_or(me) {
                    log(&format!("refused pid {pid}: uid {uid}"));
                    continue; // dropped without a reply
                }
                let _ = rustix::io::fcntl_setfd(&sock, rustix::io::FdFlags::CLOEXEC);
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = sock.set_write_timeout(Some(Duration::from_secs(2)));
                clients.push(Client { sock, pid, hello: false, restricted: false });
            }
        }
    }
}

enum Next {
    Keep,
    Close,
    Exit { unlink: bool },
}

/// Drop every fd whose other side is gone. macOS poll() reports POLLHUP on a PTY master
/// only when POLLIN is requested too, so the sweep polls with POLLIN and a zero timeout.
fn sweep(entries: &mut [Entry]) {
    for e in entries.iter_mut() {
        let Some(fd) = &e.fd else { continue };
        let mut p = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        unsafe { libc::poll(&mut p, 1, 0) };
        if p.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            log(&format!("sweep: {} hung up; dropping", e.label));
            e.fd = None;
            e.exited_ms = crate::pty::now_ms();
        }
    }
}

fn handle(c: &mut Client, m: Msg, fd: Option<OwnedFd>, entries: &mut Vec<Entry>, started: Instant) -> Next {
    let reply = |c: &Client, r: Msg, fd: Option<std::os::fd::BorrowedFd<'_>>| {
        if proto::send(c.sock.as_fd(), &r, fd).is_ok() { Next::Keep } else { Next::Close }
    };
    if m.verb == "HELLO" {
        let theirs = m.get("proto").unwrap_or("0.0");
        let major: u32 = theirs.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let mine = Msg::new("OK").with("proto", proto::proto()).with("build", proto::BUILD).with("pid", std::process::id());
        if major != proto::MAJOR && m.get("role") == Some("store") {
            // Escape hatch for a future major bump: a newer store may still empty us.
            log(&format!("takeover by pid {} with proto {theirs}: frozen verbs only", c.pid));
            c.hello = true;
            c.restricted = true;
            return reply(c, mine.with("restricted", 1), None);
        }
        if major != proto::MAJOR {
            log(&format!("refused pid {}: proto {theirs} (store {})", c.pid, proto::proto()));
            let mut r = mine;
            r.verb = "ERR".into();
            r.set("code", "incompatible-major");
            r.set("msg", format!("store speaks protocol {}, client {theirs}", proto::proto()));
            reply(c, r, None);
            return Next::Close;
        }
        c.hello = true;
        log(&format!("hello pid {} role {} proto {theirs} build {}", c.pid, m.get("role").unwrap_or("?"), m.get("build").unwrap_or("?")));
        let held = entries.iter().filter(|e| e.fd.is_some()).count();
        return reply(c, mine.with("held", held), None);
    }
    if !c.hello {
        reply(c, Msg::err("hello-first", "send HELLO first"), None);
        return Next::Close;
    }
    if c.restricted && !matches!(m.verb.as_str(), "LIST" | "GET" | "EXIT") {
        return reply(c, Msg::err("restricted", "only LIST, GET and EXIT across protocol majors"), None);
    }
    if matches!(m.verb.as_str(), "LIST" | "GET" | "STATUS") {
        sweep(entries); // answer with fresh state
    }
    let label = m.get("label").unwrap_or("").to_string();
    let idx = entries.iter().position(|e| e.label == label);
    match m.verb.as_str() {
        "PUT" => {
            let Some(fd) = fd else { return reply(c, Msg::err("no-fd", "PUT needs an fd"), None) };
            if label.is_empty() {
                return reply(c, Msg::err("no-label", "PUT needs a label"), None);
            }
            if let Some(i) = idx {
                if entries[i].fd.is_some() && m.get("replace") != Some("1") {
                    return reply(c, Msg::err("exists", "label is live; pass replace=1"), None);
                }
                entries.remove(i);
            }
            let meta = m.kv.iter().filter(|(k, _)| k != "label" && k != "replace").cloned().collect();
            log(&format!("put {label} from pid {}", c.pid));
            entries.push(Entry { label, fd: Some(fd), meta, exited_ms: 0 });
            reply(c, Msg::new("OK"), None)
        }
        "GET" => match idx.map(|i| &entries[i]) {
            Some(Entry { fd: Some(fd), meta, .. }) => {
                let mut r = Msg::new("OK").with("label", &label);
                r.kv.extend(meta.iter().cloned());
                reply(c, r, Some(fd.as_fd()))
            }
            Some(e) => reply(c, Msg::err("exited", "the other side hung up").with("exited_ms", e.exited_ms), None),
            None => reply(c, Msg::err("missing", "no such label"), None),
        },
        "LIST" => {
            let mut r = Msg::new("OK").with("n", entries.len());
            for e in entries.iter() {
                let mut it: Kv = vec![("label".into(), e.label.clone()), ("state".into(), if e.fd.is_some() { "live" } else { "exited" }.into())];
                if e.fd.is_none() {
                    it.push(("exited_ms".into(), e.exited_ms.to_string()));
                }
                it.extend(e.meta.iter().cloned());
                r.items.push(it);
            }
            reply(c, r, None)
        }
        "UPDATE" => match idx {
            Some(i) => {
                for (k, v) in m.kv.iter().filter(|(k, _)| k != "label") {
                    proto::set(&mut entries[i].meta, k, v.clone());
                }
                reply(c, Msg::new("OK"), None)
            }
            None => reply(c, Msg::err("missing", "no such label"), None),
        },
        "DROP" => {
            if let Some(i) = idx {
                entries.remove(i);
                log(&format!("drop {label} by pid {}", c.pid));
            }
            reply(c, Msg::new("OK").with("existed", idx.is_some() as u8), None)
        }
        "STATUS" => {
            let held = entries.iter().filter(|e| e.fd.is_some()).count();
            let r = Msg::new("OK")
                .with("pid", std::process::id())
                .with("build", proto::BUILD)
                .with("proto", proto::proto())
                .with("held", held)
                .with("tombstones", entries.len() - held)
                .with("fds", crate::pty::count_fds(std::process::id() as i32).unwrap_or(0))
                .with("uptime_ms", started.elapsed().as_millis());
            reply(c, r, None)
        }
        "EXIT" => {
            let held = entries.iter().filter(|e| e.fd.is_some()).count();
            if let Some(n) = m.get("expect") {
                if n != held.to_string() {
                    return reply(c, Msg::err("changed", "held count changed"), None);
                }
            }
            reply(c, Msg::new("OK"), None);
            Next::Exit { unlink: m.get("unlink") != Some("0") }
        }
        v => reply(c, Msg::err("unknown-verb", v), None),
    }
}

/// Store upgrade: copy every live fd out of the running store, then ask it to exit.
/// The fds are never without a holder.
fn take_over(entries: &mut Vec<Entry>) -> io::Result<()> {
    let mut old = crate::client::Store::connect(std::path::Path::new("."), "store").map_err(|e| io::Error::other(format!("{e:?}")))?;
    let list = old.list().map_err(|e| io::Error::other(format!("{e:?}")))?;
    for it in list.iter().filter(|it| proto::get(it, "state") == Some("live")) {
        let label = proto::get(it, "label").unwrap_or("").to_string();
        let (fd, meta) = old.get(&label).map_err(|e| io::Error::other(format!("{e:?}")))?;
        let meta = meta.kv.into_iter().filter(|(k, _)| k != "label").collect();
        entries.push(Entry { label, fd: Some(fd), meta, exited_ms: 0 });
    }
    old.exit(Some(entries.len()), false).map_err(|e| io::Error::other(format!("{e:?}")))?;
    log(&format!("took over {} fds from store pid {}", entries.len(), old.pid));
    Ok(())
}
