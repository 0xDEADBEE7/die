//! PTY creation (openpty) and spawning a program on it. Both shapes use this; in the helper
//! shape it runs inside the helper process.
use crate::util::{cvt, set_cloexec, set_nonblocking};
use std::io;
use std::os::fd::RawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

pub struct Pty {
    pub master: RawFd,
    pub child: i32,
}

pub fn winsize(cols: u16, rows: u16, cw: u32, ch: u32) -> libc::winsize {
    libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: (cols as u32 * cw) as u16, ws_ypixel: (rows as u32 * ch) as u16 }
}

pub fn set_size(master: RawFd, cols: u16, rows: u16, cw: u32, ch: u32) {
    let ws = winsize(cols, rows, cw, ch);
    // SAFETY: ioctl on our master fd with a valid winsize.
    unsafe { libc::ioctl(master, libc::TIOCSWINSZ, &ws) };
}

/// Spawn `argv` on a new PTY as a session leader with the PTY as controlling terminal.
pub fn spawn(argv: &[String], cols: u16, rows: u16, cw: u32, ch: u32, env: &[(&str, &str)]) -> io::Result<Pty> {
    let (mut m, mut s) = (0, 0);
    let mut ws = winsize(cols, rows, cw, ch);
    // SAFETY: valid out pointers; no name buffer; winsize given.
    cvt(unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null_mut(), &mut ws) })?;
    set_cloexec(m);
    set_cloexec(s);
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    cmd.env("TERM", "xterm-256color").env("COLORTERM", "truecolor").env("TERM_PROGRAM", "DIE");
    for (k, v) in env {
        cmd.env(k, v);
    }
    // SAFETY: from_raw_fd on dups of our slave fd; Command closes them in the parent.
    unsafe {
        use std::os::fd::FromRawFd;
        cmd.stdin(Stdio::from_raw_fd(libc::dup(s)))
            .stdout(Stdio::from_raw_fd(libc::dup(s)))
            .stderr(Stdio::from_raw_fd(libc::dup(s)));
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd.spawn()?;
    crate::util::close(s);
    set_nonblocking(m)?;
    Ok(Pty { master: m, child: child.id() as i32 })
}

/// Non-blocking reap. Returns Some(status) once the child has exited.
pub fn try_wait(pid: i32) -> Option<i32> {
    let mut st = 0;
    // SAFETY: waitpid on our child.
    let r = unsafe { libc::waitpid(pid, &mut st, libc::WNOHANG) };
    (r == pid).then_some(st)
}
