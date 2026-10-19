//! Small OS helpers shared by both shapes: clocks, fds, poll, rusage, stats.
use std::io;
use std::os::fd::RawFd;

/// System-wide monotonic clock in ns (CLOCK_UPTIME_RAW), comparable across processes.
pub fn now_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: valid out pointer.
    unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

pub fn cvt(r: libc::c_int) -> io::Result<libc::c_int> {
    if r < 0 { Err(io::Error::last_os_error()) } else { Ok(r) }
}

pub fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl on a caller-owned fd.
    unsafe {
        let fl = cvt(libc::fcntl(fd, libc::F_GETFL))?;
        cvt(libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK))?;
    }
    Ok(())
}

pub fn set_cloexec(fd: RawFd) {
    // SAFETY: fcntl on a caller-owned fd.
    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
}

/// A non-blocking self-pipe used as a wake source. Returns (read, write).
pub fn wake_pipe() -> (RawFd, RawFd) {
    let mut fds = [0; 2];
    // SAFETY: valid out array.
    cvt(unsafe { libc::pipe(fds.as_mut_ptr()) }).expect("pipe");
    for f in fds {
        set_nonblocking(f).unwrap();
        set_cloexec(f);
    }
    (fds[0], fds[1])
}

pub fn wake(fd: RawFd) {
    // SAFETY: writing one byte from a valid buffer; EAGAIN (pipe full) is fine.
    unsafe { libc::write(fd, b"!".as_ptr().cast(), 1) };
}

pub fn drain_pipe(fd: RawFd) {
    let mut b = [0u8; 256];
    // SAFETY: reading into a valid buffer.
    while unsafe { libc::read(fd, b.as_mut_ptr().cast(), b.len()) } > 0 {}
}

/// poll(2) wrapper. `timeout_ms < 0` blocks indefinitely. Retries EINTR.
pub fn poll(fds: &mut [libc::pollfd], timeout_ms: i32) -> usize {
    loop {
        // SAFETY: valid slice of pollfd.
        let r = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        if r >= 0 {
            return r as usize;
        }
        if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            panic!("poll: {}", io::Error::last_os_error());
        }
    }
}

pub fn pfd(fd: RawFd, events: i16) -> libc::pollfd {
    libc::pollfd { fd, events, revents: 0 }
}

pub fn close(fd: RawFd) {
    // SAFETY: caller owns fd.
    unsafe { libc::close(fd) };
}

/// Per-process counters from proc_pid_rusage(RUSAGE_INFO_V4).
#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub user_ns: u64,
    pub system_ns: u64,
    pub pkg_idle_wkups: u64,
    pub interrupt_wkups: u64,
    pub phys_footprint: u64,
    pub resident: u64,
}

#[allow(deprecated)]
pub fn usage(pid: i32) -> Option<Usage> {
    // SAFETY: zeroed POD out buffer of the right flavor.
    let mut ri: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let r = unsafe {
        libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, (&mut ri as *mut libc::rusage_info_v4).cast())
    };
    if r != 0 {
        return None;
    }
    // ri_user_time/system_time are in mach absolute time units.
    let mut tb = libc::mach_timebase_info { numer: 0, denom: 0 };
    #[allow(deprecated)]
    unsafe {
        libc::mach_timebase_info(&mut tb)
    };
    let t = |v: u64| v * tb.numer as u64 / tb.denom.max(1) as u64;
    Some(Usage {
        user_ns: t(ri.ri_user_time),
        system_ns: t(ri.ri_system_time),
        pkg_idle_wkups: ri.ri_pkg_idle_wkups,
        interrupt_wkups: ri.ri_interrupt_wkups,
        phys_footprint: ri.ri_phys_footprint,
        resident: ri.ri_resident_size,
    })
}

pub fn pid() -> i32 {
    // SAFETY: trivial syscall.
    unsafe { libc::getpid() }
}

/// Percentile summary in microseconds.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub n: usize,
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
}

pub fn summarize_ns(v: &[u64]) -> Summary {
    if v.is_empty() {
        return Summary::default();
    }
    let mut s = v.to_vec();
    s.sort_unstable();
    let q = |p: f64| s[((s.len() as f64 - 1.0) * p).round() as usize] as f64 / 1000.0;
    Summary {
        n: s.len(),
        p50: q(0.5),
        p90: q(0.9),
        p99: q(0.99),
        max: *s.last().unwrap() as f64 / 1000.0,
        mean: s.iter().sum::<u64>() as f64 / s.len() as f64 / 1000.0,
    }
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{\"n\":{},\"p50_us\":{:.1},\"p90_us\":{:.1},\"p99_us\":{:.1},\"max_us\":{:.1},\"mean_us\":{:.1}}}",
            self.n, self.p50, self.p90, self.p99, self.max, self.mean
        )
    }
}

/// FNV-1a 64, for content checks (not security).
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Deterministic xorshift PRNG for the workload generators.
pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}
