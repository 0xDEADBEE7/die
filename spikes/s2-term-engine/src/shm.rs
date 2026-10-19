//! Shared-memory images and fd passing (SCM_RIGHTS) for the helper shape.
use crate::util::{cvt, set_cloexec};
use std::collections::VecDeque;
use std::io;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// A mapping of an shm object. In the helper it keeps the fd (to send it again on
/// reattach); in DIE it is read-only and the fd is closed right after mmap.
#[derive(Debug)]
pub struct ShmMap {
    ptr: *mut u8,
    len: usize,
    map_len: usize,
    pub fd: Option<RawFd>,
}
// SAFETY: the mapping is plain memory; writes happen before it is shared.
unsafe impl Send for ShmMap {}
unsafe impl Sync for ShmMap {}

static SEQ: AtomicU64 = AtomicU64::new(0);

impl ShmMap {
    /// Create an anonymous shm object of `len` bytes (name unlinked at once), mapped RW.
    pub fn create(len: usize) -> io::Result<ShmMap> {
        let name = format!("/s2.{}.{}\0", crate::util::pid(), SEQ.fetch_add(1, Relaxed));
        // SAFETY: NUL-terminated name; variadic mode arg.
        let fd = cvt(unsafe {
            libc::shm_open(name.as_ptr().cast(), libc::O_CREAT | libc::O_EXCL | libc::O_RDWR, 0o600 as libc::c_uint)
        })?;
        // SAFETY: same name.
        unsafe { libc::shm_unlink(name.as_ptr().cast()) };
        set_cloexec(fd);
        let map_len = len.max(1);
        // SAFETY: fd is ours.
        if unsafe { libc::ftruncate(fd, map_len as libc::off_t) } != 0 {
            let e = io::Error::last_os_error();
            crate::util::close(fd);
            return Err(e);
        }
        let ptr = map(fd, map_len, libc::PROT_READ | libc::PROT_WRITE)?;
        Ok(ShmMap { ptr, len, map_len, fd: Some(fd) })
    }

    /// Map a received fd read-only and close it.
    pub fn map_received(fd: RawFd, len: usize) -> io::Result<ShmMap> {
        let map_len = len.max(1);
        let r = map(fd, map_len, libc::PROT_READ);
        crate::util::close(fd);
        Ok(ShmMap { ptr: r?, len, map_len, fd: None })
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: ptr maps at least len bytes for our lifetime.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Writable view; only used by the creator before the fd is shared.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: created RW by `create`.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

fn map(fd: RawFd, len: usize, prot: libc::c_int) -> io::Result<*mut u8> {
    // SAFETY: mapping a valid fd.
    let p = unsafe { libc::mmap(std::ptr::null_mut(), len, prot, libc::MAP_SHARED, fd, 0) };
    if p == libc::MAP_FAILED { Err(io::Error::last_os_error()) } else { Ok(p.cast()) }
}

impl Drop for ShmMap {
    fn drop(&mut self) {
        // SAFETY: our mapping and fd.
        unsafe { libc::munmap(self.ptr.cast(), self.map_len) };
        if let Some(fd) = self.fd {
            crate::util::close(fd);
        }
    }
}

/// sendmsg with an optional fd attached (SCM_RIGHTS). Non-blocking socket assumed.
pub fn send_with_fd(sock: RawFd, data: &[u8], fd: Option<RawFd>) -> io::Result<usize> {
    let mut iov = libc::iovec { iov_base: data.as_ptr() as *mut _, iov_len: data.len() };
    let mut cbuf = [0u64; 4]; // 32 bytes, enough for one fd, aligned
    // SAFETY: zeroed msghdr filled with valid pointers below.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    if let Some(fd) = fd {
        // SAFETY: CMSG macros on a buffer we own of sufficient size.
        unsafe {
            let space = libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32);
            msg.msg_control = cbuf.as_mut_ptr().cast();
            msg.msg_controllen = space as _;
            let c = libc::CMSG_FIRSTHDR(&msg);
            (*c).cmsg_level = libc::SOL_SOCKET;
            (*c).cmsg_type = libc::SCM_RIGHTS;
            (*c).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
            std::ptr::write_unaligned(libc::CMSG_DATA(c).cast::<RawFd>(), fd);
        }
    }
    // SAFETY: msg is fully initialized.
    let n = unsafe { libc::sendmsg(sock, &msg, 0) };
    if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
}

/// recvmsg into `buf`, pushing any received fds onto `fds` in arrival order.
pub fn recv_with_fds(sock: RawFd, buf: &mut [u8], fds: &mut VecDeque<RawFd>) -> io::Result<usize> {
    let mut iov = libc::iovec { iov_base: buf.as_mut_ptr().cast(), iov_len: buf.len() };
    let mut cbuf = [0u64; 32]; // room for several fds
    // SAFETY: zeroed msghdr filled with valid pointers.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cbuf.as_mut_ptr().cast();
    msg.msg_controllen = std::mem::size_of_val(&cbuf) as _;
    // SAFETY: as above.
    let n = unsafe { libc::recvmsg(sock, &mut msg, 0) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: walking the control buffer the kernel filled.
    unsafe {
        let mut c = libc::CMSG_FIRSTHDR(&msg);
        while !c.is_null() {
            if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                let data = libc::CMSG_DATA(c);
                let n_fds = ((*c).cmsg_len as usize - libc::CMSG_LEN(0) as usize) / std::mem::size_of::<RawFd>();
                for k in 0..n_fds {
                    let fd = std::ptr::read_unaligned(data.cast::<RawFd>().add(k));
                    set_cloexec(fd);
                    fds.push_back(fd);
                }
            }
            c = libc::CMSG_NXTHDR(&msg, c);
        }
    }
    Ok(n as usize)
}

pub fn set_sock_bufs(sock: RawFd, bytes: i32) {
    for opt in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
        // SAFETY: setsockopt with a valid int.
        unsafe {
            libc::setsockopt(sock, libc::SOL_SOCKET, opt, (&bytes as *const i32).cast(), 4);
        }
    }
}
