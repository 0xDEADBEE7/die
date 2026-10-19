//! Helper shape: DIE's own binary in a second mode (`s2 --term-helper`) owns one terminal's
//! PTY and engine. It talks to DIE over a Unix stream socket; image pixels go through shm
//! fds passed with SCM_RIGHTS, never through the socket. It outlives DIE: a new DIE process
//! reattaches through the socket path and gets the full screen, scrollback and images back.
use crate::client::TermClient;
use crate::engine::{Cfg, Engine};
use crate::host::Host;
use crate::proto::{self, Pixels, ToDie, ToEngine};
use crate::shm::{ShmMap, recv_with_fds, send_with_fd, set_sock_bufs};
use crate::util::{close, cvt, pfd, poll, set_cloexec, set_nonblocking};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::os::fd::RawFd;
use std::sync::Arc;

const SOCK_BUF: i32 = 4 << 20;

// ------------------------------------------------------------------ socket plumbing

fn sockaddr(path: &str) -> (libc::sockaddr_un, libc::socklen_t) {
    // SAFETY: zeroed POD.
    let mut a: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    a.sun_family = libc::AF_UNIX as _;
    assert!(path.len() < a.sun_path.len(), "socket path too long: {path}");
    for (d, s) in a.sun_path.iter_mut().zip(path.bytes()) {
        *d = s as _;
    }
    (a, std::mem::size_of::<libc::sockaddr_un>() as _)
}

pub fn listen(path: &str) -> io::Result<RawFd> {
    let _ = std::fs::remove_file(path);
    // SAFETY: socket syscalls with valid args.
    unsafe {
        let fd = cvt(libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0))?;
        set_cloexec(fd);
        let (a, l) = sockaddr(path);
        cvt(libc::bind(fd, (&a as *const libc::sockaddr_un).cast(), l))?;
        cvt(libc::listen(fd, 4))?;
        set_nonblocking(fd)?;
        Ok(fd)
    }
}

pub fn connect(path: &str) -> io::Result<RawFd> {
    // SAFETY: socket syscalls with valid args.
    unsafe {
        let fd = cvt(libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0))?;
        set_cloexec(fd);
        let (a, l) = sockaddr(path);
        if let Err(e) = cvt(libc::connect(fd, (&a as *const libc::sockaddr_un).cast(), l)) {
            close(fd);
            return Err(e);
        }
        Ok(fd)
    }
}

/// Outgoing queue on a non-blocking stream socket; an item may carry one fd.
#[derive(Default)]
pub struct OutQ {
    q: VecDeque<(Vec<u8>, usize, Option<RawFd>)>,
    pub bytes_out: u64,
}
impl OutQ {
    pub fn push(&mut self, b: Vec<u8>, fd: Option<RawFd>) {
        self.q.push_back((b, 0, fd));
    }
    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }
    /// Err on a dead peer.
    pub fn flush(&mut self, sock: RawFd) -> io::Result<()> {
        while let Some((b, off, fd)) = self.q.front_mut() {
            match send_with_fd(sock, &b[*off..], *fd) {
                Ok(n) => {
                    *fd = None; // the fd went with the first byte sent
                    *off += n;
                    self.bytes_out += n as u64;
                    if *off == b.len() {
                        self.q.pop_front();
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::Interrupted => return Ok(()),
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ helper process

pub struct HelperArgs {
    pub name: String,
    pub cfg: Cfg,
    pub inherited_fd: Option<RawFd>,
    pub exit_on_detach: bool,
    /// Adopt an existing PTY master (fd, program pid) instead of spawning: the PTY-holder
    /// layout, where a holder keeps its own copy of the master.
    pub adopt: Option<(RawFd, i32)>,
    pub argv: Vec<String>,
}

enum ConnState {
    Hello,
    Ready,
}

struct Conn {
    fd: RawFd,
    inbuf: Vec<u8>,
    out: OutQ,
    state: ConnState,
}

fn log(name: &str, msg: &str) {
    eprintln!("[helper {name} pid {}] {msg}", crate::util::pid());
}

/// `s2 --term-helper ...`: runs until the program on the PTY exits.
pub fn helper_main(a: HelperArgs) -> ! {
    // Own session: no signals from DIE's terminal or process group.
    unsafe { libc::setsid() };
    let pty = match a.adopt {
        Some((fd, child)) => {
            set_nonblocking(fd).unwrap();
            set_cloexec(fd);
            crate::pty::Pty { master: fd, child }
        }
        None => crate::pty::spawn(&a.argv, a.cfg.cols, a.cfg.rows, a.cfg.cell_w, a.cfg.cell_h, &[])
            .unwrap_or_else(|e| panic!("spawn {:?}: {e}", a.argv)),
    };
    let sock_path = format!("{}.sock", a.name);
    let lfd = listen(&sock_path).expect("listen");
    let mut host = Host::new(Engine::new(a.cfg), pty.master, pty.child);
    host.credits = 0; // nothing is sent until a DIE completes the handshake
    let mine = proto::my_version();
    // Images DIE may hold, kept mapped so a reattach can resend the fds without copying.
    let mut kept: HashMap<u32, (u64, Arc<ShmMap>)> = HashMap::new();
    let mut conn: Option<Conn> = a.inherited_fd.map(|fd| new_conn(fd));
    let mut alloc = |n: usize| {
        let mut m = ShmMap::create(n).expect("shm");
        let p = m.bytes_mut().as_mut_ptr();
        (Pixels::Shm(Arc::new(m)), p)
    };
    let mut rbuf = vec![0u8; 256 * 1024];
    let mut fdq = VecDeque::new();
    loop {
        let pty_ev = libc::POLLIN | if host.pty_out.is_empty() { 0 } else { libc::POLLOUT };
        let mut fds = vec![pfd(host.pty, pty_ev), pfd(lfd, libc::POLLIN)];
        if let Some(c) = &conn {
            fds.push(pfd(c.fd, libc::POLLIN | if c.out.is_empty() { 0 } else { libc::POLLOUT }));
        }
        poll(&mut fds, host.timeout_ms());
        host.stats.loop_wakeups += 1;

        // New DIE: the newest connection wins (an older DIE is dead or hung).
        if fds[1].revents & libc::POLLIN != 0 {
            // SAFETY: accept on our listening socket.
            let fd = unsafe { libc::accept(lfd, std::ptr::null_mut(), std::ptr::null_mut()) };
            if fd >= 0 {
                if let Some(old) = conn.take() {
                    log(&a.name, "new connection replaces the old one");
                    close(old.fd);
                }
                conn = Some(new_conn(fd));
            }
        }

        // DIE -> helper
        let mut lost = false;
        if let Some(c) = conn.as_mut()
            && fds.len() > 2
            && fds[2].revents != 0
        {
            loop {
                match recv_with_fds(c.fd, &mut rbuf, &mut fdq) {
                    Ok(0) => {
                        lost = true;
                        break;
                    }
                    Ok(n) => c.inbuf.extend_from_slice(&rbuf[..n]),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        lost = true;
                        break;
                    }
                }
            }
            if matches!(c.state, ConnState::Hello) {
                match proto::parse_hello(&c.inbuf) {
                    Ok(Some((maj, min, build, used))) => {
                        c.inbuf.drain(..used);
                        match proto::negotiate(mine, (maj, min)) {
                            Ok(minor) => {
                                log(&a.name, &format!("attached: DIE {build} speaks {maj}.{min}; helper is {} speaking {}.{}; using {}.{minor}", proto::build_id(), mine.0, mine.1, mine.0));
                                let r = proto::hello_reply(true, mine.0, minor, crate::util::pid() as u32, host.child as u32, &proto::build_id());
                                c.out.push(r, None);
                                c.state = ConnState::Ready;
                                // Full resync: full frame, tables, and every image DIE should hold.
                                host.eng.reset_sync();
                                host.need_full = true;
                                host.credits = 1;
                                let live: Vec<_> = host.eng.placements().iter().map(|p| p.image_id).collect();
                                kept.retain(|id, _| live.contains(id));
                                for (id, (generation, map)) in &kept {
                                    let (w, h) = image_size(&mut host, *id);
                                    let up = proto::ImageUpload {
                                        image_id: *id,
                                        generation: *generation,
                                        width: w,
                                        height: h,
                                        pixels: Pixels::Shm(map.clone()),
                                        t_ready: crate::util::now_ns(),
                                        t_sent: crate::util::now_ns(),
                                    };
                                    c.out.push(proto::encode_to_die(&ToDie::Image(up)), map.fd);
                                    host.eng.images_sent.insert(*id, *generation);
                                }
                            }
                            Err(reason) => {
                                log(&a.name, &format!("refused DIE {build} ({maj}.{min}): {reason}"));
                                let r = proto::hello_reply(false, mine.0, mine.1, crate::util::pid() as u32, host.child as u32, &reason);
                                c.out.push(r, None);
                                let _ = c.out.flush(c.fd);
                                lost = true;
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        log(&a.name, &format!("bad hello: {e}"));
                        lost = true;
                    }
                }
            }
            if matches!(c.state, ConnState::Ready) {
                let (used, bodies) = proto::split_frames(&c.inbuf);
                let msgs: Vec<ToEngine> = bodies
                    .iter()
                    .filter_map(|&(o, n)| proto::decode_to_engine(&c.inbuf[o..o + n]).flatten())
                    .collect();
                c.inbuf.drain(..used);
                let out = &mut c.out;
                let mut send = |m: ToDie| out.push(proto::encode_to_die(&m), None);
                for m in msgs {
                    host.handle(m, &mut send);
                }
            }
        }
        if lost {
            if let Some(c) = conn.take() {
                close(c.fd);
            }
            host.credits = 0;
            for f in fdq.drain(..) {
                close(f);
            }
            log(&a.name, "DIE detached; terminal keeps running");
            if a.exit_on_detach {
                unsafe { libc::kill(host.child, libc::SIGKILL) };
                let _ = std::fs::remove_file(&sock_path);
                std::process::exit(0);
            }
        }

        // PTY -> engine, attached or not (the program must never block on a full PTY).
        if fds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            host.read_pty(1 << 20);
        }
        host.flush_pty();

        match conn.as_mut() {
            Some(c) if matches!(c.state, ConnState::Ready) => {
                let out = &mut c.out;
                let kept_ref = &mut kept;
                let mut send = |m: ToDie| match m {
                    ToDie::Image(up) => {
                        let Pixels::Shm(map) = &up.pixels else { unreachable!() };
                        kept_ref.insert(up.image_id, (up.generation, map.clone()));
                        out.push(proto::encode_to_die(&ToDie::Image(up.clone())), map.fd);
                    }
                    ToDie::ImageGone { image_id } => {
                        kept_ref.remove(&image_id);
                        out.push(proto::encode_to_die(&m), None);
                    }
                    m => out.push(proto::encode_to_die(&m), None),
                };
                host.produce(&mut alloc, &mut send);
                if host.eof {
                    send(ToDie::Exited { status: host.exit_status() });
                }
                if c.out.flush(c.fd).is_err() {
                    close(c.fd);
                    conn = None;
                    host.credits = 0;
                }
            }
            _ => {
                // Detached: effects are dropped; frames and images wait for the next DIE.
                host.eng.out.borrow_mut().effects.clear();
            }
        }
        if host.eof {
            if let Some(c) = conn.as_mut() {
                // Best effort: let DIE see Exited.
                for _ in 0..100 {
                    if c.out.is_empty() || c.out.flush(c.fd).is_err() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
            let _ = std::fs::remove_file(&sock_path);
            log(&a.name, "program exited; helper exits");
            std::process::exit(0);
        }
    }
}

fn image_size(host: &mut Host, id: u32) -> (u32, u32) {
    let g = host.eng.term.kitty_graphics().unwrap();
    g.image(id).map(|i| (i.width().unwrap_or(0), i.height().unwrap_or(0))).unwrap_or((0, 0))
}

fn new_conn(fd: RawFd) -> Conn {
    set_nonblocking(fd).unwrap();
    set_sock_bufs(fd, SOCK_BUF);
    Conn { fd, inbuf: Vec::new(), out: OutQ::default(), state: ConnState::Hello }
}

// ------------------------------------------------------------------ DIE side

enum CState {
    AwaitReply,
    Ready,
}

pub struct HelperClient {
    sock: RawFd,
    inbuf: Vec<u8>,
    rbuf: Vec<u8>,
    fds: VecDeque<RawFd>,
    out: OutQ,
    state: CState,
    pub reply: Option<proto::HelloReply>,
    /// Our own child when this DIE spawned the helper.
    pub spawned_pid: Option<i32>,
    bytes_in: u64,
    queued: Vec<ToEngine>,
}

impl HelperClient {
    /// Start `s2 --term-helper` (same binary) with one end of a socketpair as fd 3.
    pub fn spawn(name: &str, argv: &[String], cfg: Cfg, exit_on_detach: bool, log_path: &str) -> io::Result<HelperClient> {
        Self::spawn_with(name, argv, cfg, exit_on_detach, log_path, None)
    }

    /// Like `spawn`, but the helper adopts `master` (passed as fd 4) for program `child`.
    pub fn spawn_with(
        name: &str,
        argv: &[String],
        cfg: Cfg,
        exit_on_detach: bool,
        log_path: &str,
        adopt: Option<(RawFd, i32)>,
    ) -> io::Result<HelperClient> {
        let mut sv = [0; 2];
        // SAFETY: valid out array.
        cvt(unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, sv.as_mut_ptr()) })?;
        set_cloexec(sv[0]);
        let child_end = sv[1];
        let exe = std::env::current_exe()?;
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--term-helper")
            .args(["--name", name])
            .args(["--cols", &cfg.cols.to_string(), "--rows", &cfg.rows.to_string()])
            .args(["--cw", &cfg.cell_w.to_string(), "--ch", &cfg.cell_h.to_string()])
            .args(["--fd", "3"]);
        if exit_on_detach {
            cmd.arg("--exit-on-detach");
        }
        if let Some((_, child)) = adopt {
            cmd.args(["--adopt-fd", "4", "--child", &child.to_string()]);
        }
        cmd.arg("--").args(argv);
        let adopt_fd = adopt.map(|a| a.0);
        let log = std::fs::OpenOptions::new().create(true).append(true).open(log_path)?;
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(log);
        use std::os::unix::process::CommandExt;
        // SAFETY: dup2 in the child before exec.
        unsafe {
            cmd.pre_exec(move || {
                if libc::dup2(child_end, 3) < 0 {
                    return Err(io::Error::last_os_error());
                }
                if let Some(m) = adopt_fd
                    && libc::dup2(m, 4) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn()?;
        close(child_end);
        let mut c = HelperClient::from_fd(sv[0])?;
        c.spawned_pid = Some(child.id() as i32);
        Ok(c)
    }

    /// Attach to a running helper by its socket path.
    pub fn attach(path: &str) -> io::Result<HelperClient> {
        HelperClient::from_fd(connect(path)?)
    }

    fn from_fd(sock: RawFd) -> io::Result<HelperClient> {
        set_nonblocking(sock)?;
        set_sock_bufs(sock, SOCK_BUF);
        let mut c = HelperClient {
            sock,
            inbuf: Vec::with_capacity(1 << 20),
            rbuf: vec![0u8; 1 << 20],
            fds: VecDeque::new(),
            out: OutQ::default(),
            state: CState::AwaitReply,
            reply: None,
            spawned_pid: None,
            bytes_in: 0,
            queued: Vec::new(),
        };
        let (maj, min) = proto::my_version();
        c.out.push(proto::hello(maj, min), None);
        let _ = c.out.flush(sock);
        Ok(c)
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state, CState::Ready)
    }
}

impl TermClient for HelperClient {
    fn fd(&self) -> RawFd {
        self.sock
    }
    fn wants_write(&self) -> bool {
        !self.out.is_empty()
    }
    fn on_writable(&mut self) {
        let _ = self.out.flush(self.sock);
    }
    fn send(&mut self, m: ToEngine) {
        if !self.is_ready() {
            self.queued.push(m); // sent once the handshake completes
            return;
        }
        self.out.push(proto::encode_to_engine(&m), None);
        let _ = self.out.flush(self.sock);
    }
    fn drain(&mut self, out: &mut Vec<ToDie>) -> Result<(), String> {
        let mut eof = false;
        loop {
            match recv_with_fds(self.sock, &mut self.rbuf, &mut self.fds) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(n) => {
                    self.bytes_in += n as u64;
                    self.inbuf.extend_from_slice(&self.rbuf[..n]);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("helper connection lost: {e}")),
            }
        }
        if matches!(self.state, CState::AwaitReply) {
            match proto::parse_hello_reply(&self.inbuf) {
                Ok(Some((r, used))) => {
                    self.inbuf.drain(..used);
                    if !r.ok {
                        let msg = format!("helper refused this DIE: {}", r.text);
                        self.reply = Some(r);
                        return Err(msg);
                    }
                    self.reply = Some(r);
                    self.state = CState::Ready;
                    for m in std::mem::take(&mut self.queued) {
                        self.out.push(proto::encode_to_engine(&m), None);
                    }
                    let _ = self.out.flush(self.sock);
                }
                Ok(None) => {}
                Err(e) => return Err(format!("bad handshake: {e}")),
            }
        }
        if self.is_ready() {
            let (used, bodies) = proto::split_frames(&self.inbuf);
            let fds = &mut self.fds;
            let mut take = |len: usize| fds.pop_front().and_then(|fd| ShmMap::map_received(fd, len).ok().map(Arc::new));
            for (o, n) in bodies {
                if let Some(m) = proto::decode_to_die(&self.inbuf[o..o + n], &mut take).flatten() {
                    out.push(m);
                }
            }
            self.inbuf.drain(..used);
        }
        if eof {
            return Err("helper connection closed".into());
        }
        Ok(())
    }
    fn engine_pid(&self) -> i32 {
        self.reply.as_ref().map(|r| r.helper_pid as i32).or(self.spawned_pid).unwrap_or(-1)
    }
    fn child_pid(&self) -> i32 {
        self.reply.as_ref().map(|r| r.child_pid as i32).unwrap_or(-1)
    }
    fn bytes_in(&self) -> u64 {
        self.bytes_in
    }
    fn shape(&self) -> &'static str {
        "helper"
    }
}

impl Drop for HelperClient {
    fn drop(&mut self) {
        close(self.sock);
        for f in self.fds.drain(..) {
            close(f);
        }
    }
}
