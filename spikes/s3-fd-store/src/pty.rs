//! PTY spawn, window size and small fd utilities.
//! This is the part of die-core's `pty()` that the fd store depends on.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub rows: u16,
    pub cols: u16,
    pub xpix: u16,
    pub ypix: u16,
}

impl Size {
    pub const DEFAULT: Size = Size { rows: 24, cols: 80, xpix: 1280, ypix: 768 };
    fn ws(self) -> libc::winsize {
        libc::winsize { ws_row: self.rows, ws_col: self.cols, ws_xpixel: self.xpix, ws_ypixel: self.ypix }
    }
}

/// posix_openpt with O_CLOEXEC set atomically (rustix has no CLOEXEC flag on macOS).
pub fn open_master() -> io::Result<OwnedFd> {
    let raw = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let m = unsafe { OwnedFd::from_raw_fd(raw) };
    rustix::pty::grantpt(&m)?;
    rustix::pty::unlockpt(&m)?;
    Ok(m)
}

pub struct Spawn<'a> {
    pub argv0: &'a str,
    pub prog: &'a str,
    pub args: &'a [&'a str],
    pub env: &'a [(&'a str, &'a str)],
    pub cwd: &'a Path,
    pub size: Size,
}

/// Spawn a program as a session leader with the PTY's slave as its controlling terminal.
pub fn spawn(master: BorrowedFd<'_>, s: &Spawn<'_>) -> io::Result<Child> {
    let name = rustix::pty::ptsname(master, Vec::new())?;
    let slave = rustix::fs::open(
        name.as_c_str(),
        rustix::fs::OFlags::RDWR | rustix::fs::OFlags::NOCTTY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    set_size(master, s.size)?;
    let s_raw = slave.as_raw_fd();
    let mut cmd = Command::new(s.prog);
    cmd.arg0(s.argv0).args(s.args).env_clear().current_dir(s.cwd);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for (k, v) in s.env {
        cmd.env(k, v);
    }
    unsafe {
        cmd.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::ioctl(s_raw, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            for t in 0..3 {
                if libc::dup2(s_raw, t) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let child = cmd.spawn()?;
    drop(slave);
    Ok(child)
}

pub fn set_size(fd: BorrowedFd<'_>, s: Size) -> io::Result<()> {
    let ws = s.ws();
    if unsafe { libc::ioctl(fd.as_raw_fd(), libc::TIOCSWINSZ, &ws) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn get_size(fd: BorrowedFd<'_>) -> io::Result<Size> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(fd.as_raw_fd(), libc::TIOCGWINSZ, &mut ws) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Size { rows: ws.ws_row, cols: ws.ws_col, xpix: ws.ws_xpixel, ypix: ws.ws_ypixel })
}

/// Reattach redraw: the kernel only sends SIGWINCH when the winsize changes,
/// so change the row count by one and back.
pub fn redraw_nudge(fd: BorrowedFd<'_>, gap: Duration) -> io::Result<()> {
    let s = get_size(fd)?;
    set_size(fd, Size { rows: s.rows + 1, ..s })?;
    if !gap.is_zero() {
        std::thread::sleep(gap);
    }
    set_size(fd, s)
}

/// Adaptive reattach redraw: one row fewer, wait until the program starts repainting
/// (output becomes readable) or `max` passes, then restore. Returns the wait.
/// Shrinking keeps the program from drawing below the engine's grid.
pub fn redraw_nudge_on_output(fd: BorrowedFd<'_>, max: Duration) -> io::Result<Duration> {
    let s = get_size(fd)?;
    set_size(fd, Size { rows: s.rows.saturating_sub(1).max(1), ..s })?;
    let t = Instant::now();
    let mut p = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut p, 1, max.as_millis() as i32) };
    let waited = t.elapsed();
    set_size(fd, s)?;
    Ok(waited)
}

pub enum Read {
    Data(usize),
    Timeout,
    /// EIO or EOF: every slave fd is closed.
    End,
}

/// Wait up to `timeout` for data, then read once.
pub fn read_timeout(fd: BorrowedFd<'_>, buf: &mut [u8], timeout: Duration) -> io::Result<Read> {
    let mut p = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    let r = unsafe { libc::poll(&mut p, 1, ms) };
    if r < 0 {
        let e = io::Error::last_os_error();
        return if e.kind() == io::ErrorKind::Interrupted { Ok(Read::Timeout) } else { Err(e) };
    }
    if r == 0 {
        return Ok(Read::Timeout);
    }
    let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
    if n < 0 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::EIO) => Ok(Read::End),
            Some(libc::EAGAIN) | Some(libc::EINTR) => Ok(Read::Timeout),
            _ => Err(e),
        };
    }
    Ok(if n == 0 { Read::End } else { Read::Data(n as usize) })
}

pub fn write_all(fd: BorrowedFd<'_>, mut b: &[u8]) -> io::Result<()> {
    while !b.is_empty() {
        let n = unsafe { libc::write(fd.as_raw_fd(), b.as_ptr().cast(), b.len()) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        b = &b[n as usize..];
    }
    Ok(())
}

/// Read until `needle` appears or the deadline passes. Returns everything read.
pub fn read_until(fd: BorrowedFd<'_>, needle: &str, timeout: Duration) -> io::Result<(bool, Vec<u8>)> {
    let end = Instant::now() + timeout;
    let mut acc = Vec::new();
    let mut buf = [0u8; 16384];
    loop {
        if contains(&acc, needle.as_bytes()) {
            return Ok((true, acc));
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok((false, acc));
        }
        match read_timeout(fd, &mut buf, left)? {
            Read::Data(n) => acc.extend_from_slice(&buf[..n]),
            Read::Timeout => {}
            Read::End => return Ok((false, acc)),
        }
    }
}

pub fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Number of open fds in a process (PROC_PIDLISTFDS), so leak checks need no lsof.
pub fn count_fds(pid: i32) -> io::Result<usize> {
    #[repr(C)]
    struct ProcFdInfo {
        fd: i32,
        kind: u32,
    }
    let mut buf: Vec<ProcFdInfo> = Vec::with_capacity(1024);
    let cap = (buf.capacity() * std::mem::size_of::<ProcFdInfo>()) as i32;
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, buf.as_mut_ptr().cast(), cap) };
    if n <= 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(n as usize / std::mem::size_of::<ProcFdInfo>())
}

pub fn alive(pid: i32) -> bool {
    pid > 0 && (unsafe { libc::kill(pid, 0) } == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

pub fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
}

/// Soft and hard RLIMIT_NOFILE: each held terminal costs the store one fd.
pub fn nofile() -> (u64, u64) {
    let mut r: libc::rlimit = unsafe { std::mem::zeroed() };
    unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut r) };
    (r.rlim_cur, r.rlim_max)
}
