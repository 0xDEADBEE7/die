//! PTYs (contract §12's `pty`): a program on a pseudo-terminal of its own, for die-term.
//!
//! **Starting it.** Giving a child a controlling terminal takes code in the child before `exec`,
//! which std offers only through `unsafe`. So DIE's own binary does it: started with
//! [`EXEC_ARG`] and the PTY as its stdin, stdout and stderr, it becomes a session leader, makes
//! the PTY its controlling terminal, and becomes the program, keeping its pid. A binary that opens
//! PTYs calls [`trampoline`] first thing in `main`, DIE's and a test's alike. Every fd DIE opens is
//! close-on-exec, the PTY's included, so a program holds no other terminal open. What frameworks
//! open without it, as Metal's shader cache, `die::other_mode` marks before this runs.
//!
//! **The environment** is DIE's, with the PATH your login shell sets ([`crate::process::path`]),
//! `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=DIE` and its version, and LANG
//! from your region in UTF-8 if DIE's names no locale, as DIE.app's from the Dock doesn't
//! ([`crate::sys::lang`]), less what a terminal DIE started from would have left that's wrong
//! here, then the command's own.
//!
//! **Its output** is read on a thread of its own and handed on from the top of the loop, at most
//! [`BATCH`] a turn per PTY, after which a waiting key goes (as timers do): a flood can't hold up
//! a key. With none waiting, batches go on for up to [`OUTPUT_TURN`], and then the frame does.
//! The reader keeps at most [`KEEP`] waiting, and past that the program waits in the kernel. Input is written from that thread too, so a program that doesn't read never
//! blocks DIE. The exit comes after the last output.
//!
//! **Closing** a [`Pty`], or dropping it, hangs the terminal up, as closing a terminal window
//! does, and kills what's still there [`HANGUP_WAIT`] later. Quitting DIE closes every one. In
//! D20's next round the holder keeps the PTY instead of DIE; nothing here shows which has it.
#![warn(missing_docs)]
use crate::error::*;
use crate::ext::*;
use crate::rt::Waker;
use crate::surface::CellPixels;
use rustix::event::{PollFd, PollFlags, poll};
use rustix::fs::{Mode, OFlags};
use rustix::io::Errno;
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions};
use rustix::termios::{InputModes, LocalModes, OptionalActions, Winsize};
use std::cell::RefCell;
use std::ffi::{OsStr, OsString};
use std::io::{self, PipeReader, PipeWriter, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The first argument that makes DIE's binary a PTY's trampoline.
pub const EXEC_ARG: &str = "--pty-exec";
/// How much of a PTY's output one batch hands on.
pub const BATCH: usize = 256 << 10;
/// How long batches of output may go on before the frame, while no key waits.
pub const OUTPUT_TURN: Duration = Duration::from_millis(4);
/// How much output the reader keeps waiting before it stops reading.
pub const KEEP: usize = 1 << 20;
/// One read's most.
const READ: usize = 64 << 10;
/// How long a program that has exited gets for its last output to come out.
const DRAIN: Duration = Duration::from_millis(200);
/// How long a hung-up program gets to end before it's killed.
pub const HANGUP_WAIT: Duration = Duration::from_millis(300);
/// What every program on a PTY is told of its terminal (scope §6).
const TERMINAL: [(&str, &str); 4] = [
    ("TERM", "xterm-256color"),
    ("COLORTERM", "truecolor"),
    ("TERM_PROGRAM", "DIE"),
    ("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION")),
];
/// The variables that name a locale. With none of them set, a program runs in C's, so `é` is
/// two unknown bytes to it.
const LOCALE: [&str; 3] = ["LC_ALL", "LC_CTYPE", "LANG"];
/// What the terminal DIE started from may have left in its environment that's wrong in DIE's:
/// its size, and its own terminfo, sessions and shell integration.
const NOT_OURS: &[&str] = &[
    "COLUMNS",
    "LINES",
    "TERMINFO",
    "TERM_SESSION_ID",
    "LC_TERMINAL",
    "LC_TERMINAL_VERSION",
    "INSIDE_EMACS",
    "WINDOWID",
];
/// The same, by prefix.
const NOT_OURS_BEFORE: &[&str] = &[
    "GHOSTTY_",
    "KITTY_",
    "ITERM_",
    "WEZTERM_",
    "ALACRITTY_",
    "VTE_",
];

/// If this process was started as a PTY's trampoline, become its program, and if that fails, say
/// why on the terminal and give the exit code, as a shell's 127 or 126. None if it wasn't, which
/// is every other run. Call it first in `main`.
pub fn trampoline() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != EXEC_ARG {
        return None;
    }
    let (Some(arg0), Some(program)) = (args.next(), args.next()) else {
        eprintln!("die: {EXEC_ARG} needs a name and a program");
        return Some(2);
    };
    // stdin is the PTY: a new session takes it as its controlling terminal.
    let ctty =
        rustix::process::setsid().and_then(|_| rustix::process::ioctl_tiocsctty(io::stdin()));
    if let Err(e) = ctty {
        eprintln!("die: no controlling terminal: {e}");
        return Some(126);
    }
    let e = Command::new(&program).arg0(arg0).args(args).exec();
    eprintln!("die: {}: {e}", program.to_string_lossy());
    Some(if e.kind() == io::ErrorKind::NotFound {
        127
    } else {
        126
    })
}

/// A terminal's size: cells, and the pixels of the whole area (TIOCSWINSZ).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PtySize {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
    /// The area's width in physical pixels, or 0 for not known.
    pub width: u16,
    /// The area's height in physical pixels, or 0 for not known.
    pub height: u16,
}

impl PtySize {
    /// `cols` × `rows` cells of `cell`'s size, as [`Event::Resize`] gives them.
    pub fn new(cols: usize, rows: usize, cell: CellPixels) -> PtySize {
        let n = |v: usize| u16::try_from(v).unwrap_or(u16::MAX);
        PtySize {
            cols: n(cols),
            rows: n(rows),
            width: n(cols.saturating_mul(cell.width as usize)),
            height: n(rows.saturating_mul(cell.height as usize)),
        }
    }
    fn winsize(self) -> Winsize {
        Winsize {
            ws_row: self.rows,
            ws_col: self.cols,
            ws_xpixel: self.width,
            ws_ypixel: self.height,
        }
    }
}

type OutputFn = Box<dyn FnMut(&mut Ctx, &[u8]) -> Result>;
type ExitFn = Box<dyn FnOnce(&mut Ctx, io::Result<ExitStatus>) -> Result>;

/// What runs on a PTY, where, and what hears from it: [`Ctx::pty`]'s builder.
pub struct PtyCommand {
    spec: Spec,
    on_output: Option<OutputFn>,
    on_exit: Option<ExitFn>,
}

/// What the PTY's thread needs to start the program.
struct Spec {
    program: OsString,
    arg0: Option<OsString>,
    args: Vec<OsString>,
    dir: Option<PathBuf>,
    env: Vec<(OsString, Option<OsString>)>,
}

impl PtyCommand {
    /// Run `program`, found on the PATH it gets unless it's a path.
    pub fn new(program: impl AsRef<OsStr>) -> PtyCommand {
        PtyCommand {
            spec: Spec {
                program: program.as_ref().into(),
                arg0: None,
                args: vec![],
                dir: None,
                env: vec![],
            },
            on_output: None,
            on_exit: None,
        }
    }
    /// One argument more.
    pub fn arg(mut self, a: impl AsRef<OsStr>) -> PtyCommand {
        self.spec.args.push(a.as_ref().into());
        self
    }
    /// Arguments more.
    pub fn args(mut self, a: impl IntoIterator<Item = impl AsRef<OsStr>>) -> PtyCommand {
        self.spec
            .args
            .extend(a.into_iter().map(|a| a.as_ref().into()));
        self
    }
    /// The name the program sees itself run as, its `argv[0]`: `-zsh` makes a login shell.
    pub fn arg0(mut self, a: impl AsRef<OsStr>) -> PtyCommand {
        self.spec.arg0 = Some(a.as_ref().into());
        self
    }
    /// Where it starts. Without this, where DIE is.
    pub fn current_dir(mut self, d: impl AsRef<Path>) -> PtyCommand {
        self.spec.dir = Some(d.as_ref().into());
        self
    }
    /// Set a variable, after DIE's own.
    pub fn env(mut self, k: impl AsRef<OsStr>, v: impl AsRef<OsStr>) -> PtyCommand {
        self.spec
            .env
            .push((k.as_ref().into(), Some(v.as_ref().into())));
        self
    }
    /// Leave a variable out.
    pub fn env_remove(mut self, k: impl AsRef<OsStr>) -> PtyCommand {
        self.spec.env.push((k.as_ref().into(), None));
        self
    }
    /// What it prints comes here, in order, from the top of the loop (C3).
    pub fn on_output(mut self, f: impl FnMut(&mut Ctx, &[u8]) -> Result + 'static) -> PtyCommand {
        self.on_output = Some(Box::new(f));
        self
    }
    /// How it ended comes here, after its last output, or why it didn't start.
    pub fn on_exit(
        mut self,
        f: impl FnOnce(&mut Ctx, io::Result<ExitStatus>) -> Result + 'static,
    ) -> PtyCommand {
        self.on_exit = Some(Box::new(f));
        self
    }
}

impl Spec {
    /// The trampoline's command, on the PTY's other side.
    fn command(self, exe: PathBuf, slave: OwnedFd) -> io::Result<Command> {
        let mut cmd = Command::new(exe);
        cmd.arg(EXEC_ARG)
            .arg(self.arg0.as_ref().unwrap_or(&self.program))
            .arg(&self.program)
            .args(&self.args);
        for (k, _) in std::env::vars_os() {
            let name = k.to_string_lossy();
            let theirs = NOT_OURS_BEFORE.iter().any(|p| name.starts_with(p));
            if theirs || NOT_OURS.contains(&&*name) {
                cmd.env_remove(&k);
            }
        }
        cmd.envs(TERMINAL).env("PATH", crate::process::path());
        let named = |k| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        if !LOCALE.into_iter().any(named) {
            cmd.env("LANG", crate::sys::lang());
        }
        for (k, v) in &self.env {
            match v {
                Some(v) => cmd.env(k, v),
                None => cmd.env_remove(k),
            };
        }
        if let Some(dir) = &self.dir {
            cmd.current_dir(dir);
        }
        let (a, b) = (slave.try_clone()?, slave.try_clone()?);
        cmd.stdin(Stdio::from(a))
            .stdout(Stdio::from(b))
            .stderr(Stdio::from(slave));
        Ok(cmd)
    }
}

/// A program on a PTY, from [`Ctx::pty`]. Dropping it closes it, and no callback runs after (C4).
pub struct Pty {
    shared: Arc<Shared>,
}

impl Pty {
    /// Send bytes to the program, as typed. It never waits: they go when it reads them. After
    /// the end, nothing happens.
    pub fn write(&self, bytes: &[u8]) {
        let over = || self.shared.closed.load(Ordering::Acquire) || lock(&self.shared.out).ended;
        if bytes.is_empty() || over() {
            return;
        }
        lock(&self.shared.input).extend_from_slice(bytes);
        self.shared.nudge();
    }
    /// Give the terminal a new size, which the program hears as SIGWINCH if it changed.
    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        let master = lock(&self.shared.master).clone();
        let master = master.ok_or_else(|| io::Error::other("the PTY is closed"))?;
        Ok(rustix::termios::tcsetwinsize(&*master, size.winsize())?)
    }
    /// The program's pid, once it has started.
    pub fn pid(&self) -> Option<u32> {
        lock(&self.shared.proc).pid.map(|p| p.as_raw_pid() as u32)
    }
    /// The terminal's foreground process group: the program's own pid while it's at a prompt,
    /// and a job's while one runs, so a kill can be confirmed. None while the terminal has no
    /// session: before the program has made it its terminal, and once it has ended.
    pub fn foreground_group(&self) -> Option<u32> {
        let master = lock(&self.shared.master).clone()?;
        // macOS reads the group of a terminal with no session as 0, which rustix would make a
        // pid of, so the session first. Only an exit between the two can still give the 0.
        rustix::termios::tcgetsid(&*master).ok()?;
        let group = rustix::termios::tcgetpgrp(&*master).ok()?;
        Some(group.as_raw_pid() as u32)
    }
    /// Whether the terminal echoes what's typed: false at a password prompt.
    pub fn echoes(&self) -> Option<bool> {
        let modes = self.modes()?;
        Some(modes.local_modes.contains(LocalModes::ECHO))
    }
    /// Whether the terminal hands input on a line at a time (ICANON), as a shell leaves it for
    /// a job that doesn't read keys, so what's typed waits for the line's end.
    pub fn line_mode(&self) -> Option<bool> {
        let modes = self.modes()?;
        Some(modes.local_modes.contains(LocalModes::ICANON))
    }
    fn modes(&self) -> Option<rustix::termios::Termios> {
        let master = lock(&self.shared.master).clone()?;
        rustix::termios::tcgetattr(&*master).ok()
    }
    /// Hang the terminal up, as closing a terminal window does, and kill what's left of the
    /// program [`HANGUP_WAIT`] later. Its callbacks don't run after this.
    pub fn close(&self) {
        self.shared.close();
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.shared.close();
    }
}

/// What the main thread and a PTY's threads share.
struct Shared {
    /// The handle's copy of the master, for its size and modes. The reader has its own.
    master: Mutex<Option<Arc<OwnedFd>>>,
    /// A byte here wakes the reader; closing it stops it.
    nudge: Mutex<Option<PipeWriter>>,
    out: Mutex<Out>,
    ended: Condvar,
    input: Mutex<Vec<u8>>,
    proc: Mutex<Proc>,
    exiting: Condvar,
    /// There's output or an exit the main thread hasn't seen.
    fresh: AtomicBool,
    closed: AtomicBool,
    waker: Waker,
}

#[derive(Default)]
struct Out {
    bytes: Vec<u8>,
    /// Every slave fd is closed: nothing more will come.
    ended: bool,
}

#[derive(Default)]
struct Proc {
    pid: Option<Pid>,
    /// It has exited and is about to be reaped, so its pid is no longer one to kill.
    exiting: bool,
    status: Option<io::Result<ExitStatus>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    fn nudge(&self) {
        if let Some(w) = &*lock(&self.nudge) {
            let _ = (&*w).write(&[1]); // nonblocking: a full pipe has a nudge already
        }
    }
    fn news(&self) {
        self.fresh.store(true, Ordering::Release);
        self.waker.wake();
    }
    fn put_output(&self, bytes: &[u8]) {
        lock(&self.out).bytes.extend_from_slice(bytes);
        self.news();
    }
    fn end_output(&self) {
        lock(&self.out).ended = true;
        self.ended.notify_all();
        self.news();
    }
    fn waiting(&self) -> usize {
        lock(&self.out).bytes.len()
    }
    /// Up to `most` of the output, and the exit once every byte before it is taken. A reader
    /// that was full gets a turn to read what the kernel still holds before the exit goes.
    fn take(&self, most: usize) -> (Vec<u8>, Option<io::Result<ExitStatus>>) {
        let mut out = lock(&self.out);
        let was_full = out.bytes.len() >= KEEP;
        let n = out.bytes.len().min(most);
        let bytes: Vec<u8> = out.bytes.drain(..n).collect();
        let (left, ended) = (out.bytes.len(), out.ended);
        drop(out);
        let resumed = was_full && left < KEEP;
        if resumed {
            self.nudge();
        }
        if left > 0 || resumed && !ended {
            self.fresh.store(true, Ordering::Release);
            return (bytes, None);
        }
        (bytes, lock(&self.proc).status.take())
    }
    fn exited(&self, status: io::Result<ExitStatus>) {
        lock(&self.proc).status = Some(status);
        self.news();
    }
    fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        lock(&self.master).take();
        lock(&self.nudge).take(); // the reader stops, and its copy of the master goes
    }
    /// Wait until the program exits, for as long as `until`; else kill its process group.
    fn kill_unless_exited(&self, until: Instant) {
        let p = lock(&self.proc);
        let wait = until.saturating_duration_since(Instant::now());
        let (p, _) = (self.exiting)
            .wait_timeout_while(p, wait, |p| !p.exiting && p.status.is_none())
            .unwrap_or_else(|e| e.into_inner());
        // Under the lock, so the waiter can't reap it and free its pid first.
        if let Some(pid) = p.pid.filter(|_| !p.exiting) {
            let _ = rustix::process::kill_process_group(pid, Signal::KILL);
            let _ = rustix::process::kill_process(pid, Signal::KILL);
        }
    }
}

/// A PTY with its master and the slave the program gets, at `size`.
fn open(size: PtySize) -> io::Result<(OwnedFd, OwnedFd)> {
    // posix_openpt is this open on macOS and Linux, and rustix's has no close-on-exec on macOS.
    let flags = OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC;
    let master = rustix::fs::open("/dev/ptmx", flags, Mode::empty())?;
    rustix::pty::grantpt(&master)?;
    rustix::pty::unlockpt(&master)?;
    let name = rustix::pty::ptsname(&master, Vec::new())?;
    let slave = rustix::fs::open(name.as_c_str(), flags, Mode::empty())?;
    // Only once the slave is open (S3).
    rustix::termios::tcsetwinsize(&master, size.winsize())?;
    // A line's erase takes a whole UTF-8 character, as vterm's `stty iutf8` has it.
    let mut modes = rustix::termios::tcgetattr(&slave)?;
    modes.input_modes |= InputModes::IUTF8;
    rustix::termios::tcsetattr(&slave, OptionalActions::Now, &modes)?;
    rustix::fs::fcntl_setfl(&master, OFlags::NONBLOCK)?;
    Ok((master, slave))
}

/// The PTY's thread: start the program, then read and write until it ends or the PTY closes.
fn run(
    shared: Arc<Shared>,
    master: Arc<OwnedFd>,
    nudged: PipeReader,
    start: (Spec, PathBuf, OwnedFd),
) {
    let (spec, exe, slave) = start;
    let child = spec.command(exe, slave).and_then(|mut cmd| cmd.spawn());
    let child = match child {
        Ok(child) => child,
        Err(e) => {
            shared.end_output();
            return shared.exited(Err(e));
        }
    };
    lock(&shared.proc).pid = Some(Pid::from_child(&child));
    let s = shared.clone();
    let waiter = std::thread::Builder::new().name("die-pty-wait".into());
    if let Err(e) = waiter.spawn(move || wait(s, child)) {
        shared.end_output();
        return shared.exited(Err(e));
    }
    pump(&shared, &master, &nudged);
    drop(master);
    if shared.closed.load(Ordering::Acquire) {
        shared.kill_unless_exited(Instant::now() + HANGUP_WAIT);
    }
}

/// Read output and write input until every slave closes or the nudge pipe does.
fn pump(shared: &Shared, master: &OwnedFd, nudged: &PipeReader) {
    let (mut input, mut buf) = (Vec::new(), vec![0; READ]);
    loop {
        let mut want = PollFlags::empty();
        if shared.waiting() < KEEP {
            want |= PollFlags::IN;
        }
        if !input.is_empty() {
            want |= PollFlags::OUT;
        }
        let mut fds = vec![PollFd::new(nudged, PollFlags::IN)];
        // Polled for nothing, a hung-up master would still wake this at once on Linux.
        if !want.is_empty() {
            fds.push(PollFd::new(master, want));
        }
        match poll(&mut fds, None) {
            Ok(_) | Err(Errno::INTR) => {}
            Err(_) => return,
        }
        let woke = fds[0].revents();
        let ready = fds.get(1).map_or(PollFlags::empty(), PollFd::revents);
        if !woke.is_empty() {
            let mut b = [0; 64];
            if matches!((&*nudged).read(&mut b), Ok(0)) {
                return; // closed
            }
            input.append(&mut lock(&shared.input));
        }
        if ready.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR) {
            match rustix::io::read(master, &mut buf[..]) {
                Ok(n) if n > 0 => shared.put_output(&buf[..n]),
                Err(Errno::AGAIN | Errno::INTR) => {}
                // EOF, or EIO once every slave is closed.
                _ => return shared.end_output(),
            }
        }
        if ready.contains(PollFlags::OUT) {
            match rustix::io::write(master, &input) {
                Ok(n) => drop(input.drain(..n)),
                Err(Errno::AGAIN | Errno::INTR) => {}
                Err(_) => input.clear(),
            }
        }
    }
}

/// Wait for the program to exit, then for its last output, and say how it ended.
fn wait(shared: Arc<Shared>, mut child: Child) {
    let pid = Pid::from_child(&child);
    let exited = WaitIdOptions::EXITED | WaitIdOptions::NOWAIT;
    while let Err(Errno::INTR) = rustix::process::waitid(WaitId::Pid(pid), exited) {}
    lock(&shared.proc).exiting = true;
    shared.exiting.notify_all();
    let status = child.wait();
    let out = lock(&shared.out);
    let wait = shared.ended.wait_timeout_while(out, DRAIN, |o| !o.ended);
    drop(wait);
    shared.exited(status);
}

/// The PTYs open, as the loop hands on what they say.
#[derive(Default)]
pub(crate) struct Ptys {
    live: Vec<Entry>,
    /// Their output had a batch since the last key or frame.
    pub(crate) ran: bool,
    /// When the first of those was.
    pub(crate) since: Option<Instant>,
}

impl Ptys {
    /// Whether output can have another batch before the frame: [`OUTPUT_TURN`] isn't spent.
    pub(crate) fn more(&self) -> bool {
        self.since.is_some_and(|t| t.elapsed() < OUTPUT_TURN)
    }
}

struct Entry {
    shared: Arc<Shared>,
    owner: ExtIdx,
    on_output: Rc<RefCell<Option<OutputFn>>>,
    on_exit: Option<ExitFn>,
}

impl Ctx {
    /// Start `cmd` on a new PTY of `size` (contract §12). Its output and exit come to `cmd`'s
    /// callbacks from the top of the loop. Err if there's no PTY to be had, or no such
    /// directory; any other failure to start comes to `on_exit`, and an `exec` that fails says
    /// why on the terminal and exits 127 or 126, as a shell does.
    pub fn pty(&mut self, cmd: PtyCommand, size: PtySize) -> Result<Pty> {
        let PtyCommand {
            spec,
            on_output,
            on_exit,
        } = cmd;
        if let Some(dir) = spec.dir.as_ref().filter(|d| !d.is_dir()) {
            return Err(Error::Msg(format!("{}: no such directory", dir.display())));
        }
        let failed = |e: io::Error| Error::Msg(format!("No PTY: {e}"));
        let exe = crate::sys::own_exe().map_err(failed)?;
        let (master, slave) = open(size).map_err(failed)?;
        let master = Arc::new(master);
        let (nudged, nudge) = io::pipe().map_err(failed)?;
        rustix::io::ioctl_fionbio(&nudge, true).map_err(|e| failed(e.into()))?;
        let shared = Arc::new(Shared {
            master: Mutex::new(Some(master.clone())),
            nudge: Mutex::new(Some(nudge)),
            out: Mutex::default(),
            ended: Condvar::new(),
            input: Mutex::default(),
            proc: Mutex::default(),
            exiting: Condvar::new(),
            fresh: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            waker: self.waker(),
        });
        let s = shared.clone();
        let reader = std::thread::Builder::new().name("die-pty".into());
        reader
            .spawn(move || run(s, master, nudged, (spec, exe, slave)))
            .map_err(failed)?;
        self.ptys.live.push(Entry {
            shared: shared.clone(),
            owner: self.cur_ext(),
            on_output: Rc::new(RefCell::new(on_output)),
            on_exit,
        });
        Ok(Pty { shared })
    }

    /// Whether a PTY has output or an exit waiting for its turn, so the host mustn't sleep.
    pub fn pty_output_waiting(&self) -> bool {
        let waiting =
            |s: &Shared| s.fresh.load(Ordering::Acquire) && !s.closed.load(Ordering::Acquire);
        self.ptys.live.iter().any(|e| waiting(&e.shared))
    }

    /// Queue each PTY's next batch of output, and an exit after the last of it. Whether any was.
    pub(crate) fn ptys_take(&mut self) -> bool {
        let mut queued = false;
        let mut i = 0;
        while let Some(e) = self.ptys.live.get(i) {
            let shared = e.shared.clone();
            if shared.closed.load(Ordering::Acquire) {
                let gone = self.ptys.live.remove(i);
                self.cleanup(|_| drop(gone));
                continue;
            }
            if !shared.fresh.swap(false, Ordering::AcqRel) {
                i += 1;
                continue;
            }
            let (bytes, status) = shared.take(BATCH);
            let (owner, f) = (e.owner, e.on_output.clone());
            if !bytes.is_empty() {
                let s = shared.clone();
                self.defer_as(owner, move |ctx| match &mut *f.borrow_mut() {
                    Some(f) if !s.closed.load(Ordering::Acquire) => f(ctx, &bytes),
                    _ => Ok(()),
                });
                queued = true;
            }
            let Some(status) = status else {
                i += 1;
                continue;
            };
            let gone = self.ptys.live.remove(i);
            if let Some(f) = gone.on_exit {
                self.defer_as(owner, move |ctx| {
                    match shared.closed.load(Ordering::Acquire) {
                        false => f(ctx, status),
                        true => Ok(()),
                    }
                });
                queued = true;
            }
            self.cleanup(|_| drop(gone.on_output));
        }
        queued
    }

    /// At quit: hang up every PTY, and kill what's left of each program [`HANGUP_WAIT`] later.
    pub(crate) fn stop_ptys(&mut self) {
        let live: Vec<Entry> = self.ptys.live.drain(..).collect();
        live.iter().for_each(|e| e.shared.close());
        let until = Instant::now() + HANGUP_WAIT;
        for e in &live {
            e.shared.kill_unless_exited(until);
        }
        self.cleanup(|_| drop(live));
    }

    /// Harness: take turns until `done` says so, or `wait` has passed, for what comes from
    /// another thread with no job to wait on, as a PTY's output does. Whether `done` said so.
    pub fn run_until(&mut self, wait: Duration, mut done: impl FnMut(&mut Ctx) -> bool) -> bool {
        let start = Instant::now();
        loop {
            self.drain(false);
            if done(self) {
                return true;
            }
            if start.elapsed() > wait {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
