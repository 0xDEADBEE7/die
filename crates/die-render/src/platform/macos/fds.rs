//! The fds a program on a PTY inherits (contract §12).

use rustix::io::{FdFlags, fcntl_getfd, fcntl_setfd};
use std::os::fd::BorrowedFd;

/// Mark every fd but stdin, stdout and stderr close-on-exec, for a PTY's trampoline before it
/// `exec`s the program. Metal opens its shader cache read-write without close-on-exec, and a
/// spawn passes such an fd on, so without this every program on a PTY, and whatever it leaves
/// running, would hold the cache open. An fd that closed meanwhile is passed over.
pub fn close_on_exec_inherited() -> std::io::Result<()> {
    for entry in std::fs::read_dir("/dev/fd")? {
        let name = entry?.file_name();
        let Some(fd) = name
            .to_str()
            .and_then(|n| n.parse().ok())
            .filter(|fd| *fd > 2)
        else {
            continue;
        };
        // SAFETY: /dev/fd lists open fds, and this runs first in the trampoline, where no other
        // thread of DIE's has started to close one, so `fd` stays open while it's borrowed. The
        // listing's own fd is open until the loop ends.
        let fd = unsafe { BorrowedFd::borrow_raw(fd) };
        if let Ok(flags) = fcntl_getfd(fd) {
            let _ = fcntl_setfd(fd, flags | FdFlags::CLOEXEC);
        }
    }
    Ok(())
}
