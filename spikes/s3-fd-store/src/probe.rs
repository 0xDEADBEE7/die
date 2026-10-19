//! Kernel facts the design depends on, measured on this Mac.
//! `die probe` prints one line per fact.

use crate::pty::{self, Read, Size, Spawn};
use std::io::Write as _;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::path::Path;
use std::time::{Duration, Instant};

fn poll_revents(fd: BorrowedFd<'_>, events: i16) -> i16 {
    let mut p = libc::pollfd { fd: fd.as_raw_fd(), events, revents: 0 };
    unsafe { libc::poll(&mut p, 1, 0) };
    p.revents
}

fn rev(r: i16) -> String {
    let mut v = vec![];
    for (b, n) in [(libc::POLLIN, "IN"), (libc::POLLOUT, "OUT"), (libc::POLLHUP, "HUP"), (libc::POLLERR, "ERR"), (libc::POLLNVAL, "NVAL")] {
        if r & b != 0 {
            v.push(n);
        }
    }
    if v.is_empty() { "0".into() } else { v.join("|") }
}

fn sh(dir: &Path, script: &str, size: Size) -> (std::os::fd::OwnedFd, std::process::Child) {
    let m = pty::open_master().unwrap();
    let c = pty::spawn(
        m.as_fd(),
        &Spawn { argv0: "s3probe-sh", prog: "/bin/sh", args: &["-c", script], env: &[("PATH", "/usr/bin:/bin")], cwd: dir, size },
    )
    .unwrap();
    (m, c)
}

pub fn run(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let out = &mut std::io::stdout();

    // P0: a master whose slave was never opened by anyone.
    let m = pty::open_master().unwrap();
    writeln!(out, "P0 master, slave never opened: poll(0)={} poll(IN)={}", rev(poll_revents(m.as_fd(), 0)), rev(poll_revents(m.as_fd(), libc::POLLIN))).unwrap();
    drop(m);

    // P1: a process exiting with unread output on a PTY nobody reads.
    p1(dir, out, None, false);
    p1(dir, out, Some(1), false);
    p1(dir, out, None, true);

    // P2: kqueue NOTE_EXIT | NOTE_EXITSTATUS on a process we are not the parent of.
    let o = std::process::Command::new("/bin/sh").arg("-c").arg("/bin/sleep 0.4 >/dev/null 2>&1 & echo $!").output().unwrap();
    let pid: i32 = String::from_utf8_lossy(&o.stdout).trim().parse().unwrap();
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), std::mem::size_of::<libc::proc_bsdinfo>() as i32) };
    let (fired, data, ms) = note_exit(pid, || {});
    writeln!(out, "P2 non-child pid {pid} (ppid {}): NOTE_EXIT fired={fired} after {ms:.0} ms, data={data:#x} (exit status if NOTE_EXITSTATUS works for non-children; sleep exits 0)", info.pbi_ppid).unwrap();
    let o = std::process::Command::new("/bin/sh").arg("-c").arg("/bin/sleep 30 >/dev/null 2>&1 & echo $!").output().unwrap();
    let pid: i32 = String::from_utf8_lossy(&o.stdout).trim().parse().unwrap();
    let (fired, data, ms) = note_exit(pid, || unsafe {
        libc::kill(pid, libc::SIGTERM);
    });
    writeln!(out, "P2b non-child killed with SIGTERM: fired={fired} after {ms:.2} ms, data={data:#x} (wait status: signal 15 = 0xf)").unwrap();

    // P3: does the kernel drop a same-size SIGWINCH? And a pixel-only change?
    let f = dir.join("winch.txt");
    let _ = std::fs::remove_file(&f);
    let script = format!("trap 'echo W >> {}' WINCH; while :; do /bin/sleep 0.02; done", f.display());
    let (m, mut c) = sh(dir, &script, Size::DEFAULT);
    std::thread::sleep(Duration::from_millis(300));
    let count = || std::fs::read_to_string(&f).map(|s| s.lines().count()).unwrap_or(0);
    let mut steps = vec![];
    let s0 = Size::DEFAULT;
    for (name, s) in [
        ("same", s0),
        ("same", s0),
        ("same", s0),
        ("rows+1", Size { rows: s0.rows + 1, ..s0 }),
        ("rows", s0),
        ("xpix+1", Size { xpix: s0.xpix + 1, ..s0 }),
        ("xpix", s0),
    ] {
        let before = count();
        pty::set_size(m.as_fd(), s).unwrap();
        std::thread::sleep(Duration::from_millis(250));
        steps.push(format!("{name}:{}", count() - before));
    }
    writeln!(out, "P3 SIGWINCH delivered per TIOCSWINSZ on the master: {}", steps.join(" ")).unwrap();
    let _ = c.kill();
    let _ = c.wait();
    drop(m);

    // P4: PTY output buffer with nobody reading, at several output speeds.
    for speed in [None, Some(libc::B9600), Some(libc::B38400), Some(libc::B230400)] {
        p4(dir, out, speed);
    }

    // P5: POLLHUP on a unix socket whose peer closed (the store may hold sockets too).
    let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
    let before = poll_revents(a.as_fd(), 0);
    drop(b);
    writeln!(out, "P5 socketpair: poll(0) before peer close={} after={}; poll(IN) after={}", rev(before), rev(poll_revents(a.as_fd(), 0)), rev(poll_revents(a.as_fd(), libc::POLLIN))).unwrap();

    // P6: the store's sweep case: the child exits with nothing unread.
    let (m, mut c) = sh(dir, "exit 0", Size::DEFAULT);
    let t = Instant::now();
    let st = c.wait().unwrap();
    let waited = t.elapsed().as_millis();
    let mut p = libc::pollfd { fd: m.as_raw_fd(), events: 0, revents: 0 };
    let t = Instant::now();
    let r = unsafe { libc::poll(&mut p, 1, 1000) };
    writeln!(out, "P6 child exited with no unread output ({st}, wait {waited} ms): blocking poll(0) -> {r} revents={} after {:.2} ms; poll(IN)={}", rev(p.revents), t.elapsed().as_secs_f64() * 1000.0, rev(poll_revents(m.as_fd(), libc::POLLIN))).unwrap();
    drop(m);
    // P6b: a blocking poll(0) wakes up when the child exits later.
    let (m, mut c) = sh(dir, "/bin/sleep 0.3; exit 0", Size::DEFAULT);
    let mut p = libc::pollfd { fd: m.as_raw_fd(), events: 0, revents: 0 };
    let t = Instant::now();
    let r = unsafe { libc::poll(&mut p, 1, 2000) };
    let woke = t.elapsed().as_millis();
    let _ = c.wait();
    // P7: kqueue EVFILT_READ with a huge NOTE_LOWAT: does it report only EOF?
    for (name, script) in [("no output", "/bin/sleep 0.3; exit 0"), ("200 bytes then exit", "/bin/sleep 0.1; printf '%0200d' 0; /bin/sleep 0.2; exit 0")] {
        let (m, mut c) = sh(dir, script, Size::DEFAULT);
        let mut evs = vec![];
        unsafe {
            let kq = libc::kqueue();
            let mut ev: libc::kevent = std::mem::zeroed();
            ev.ident = m.as_raw_fd() as usize;
            ev.filter = libc::EVFILT_READ;
            ev.flags = libc::EV_ADD | libc::EV_CLEAR;
            ev.fflags = libc::NOTE_LOWAT;
            ev.data = 1 << 30;
            let r = libc::kevent(kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null());
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(1200) {
                let mut o: libc::kevent = std::mem::zeroed();
                let ts = libc::timespec { tv_sec: 0, tv_nsec: 100_000_000 };
                if libc::kevent(kq, std::ptr::null(), 0, &mut o, 1, &ts) == 1 {
                    let (d, f) = (o.data, o.flags); evs.push(format!("{} ms data={} eof={}", t.elapsed().as_millis(), d, f & libc::EV_EOF != 0));
                    if o.flags & libc::EV_EOF != 0 || evs.len() > 5 {
                        break;
                    }
                }
            }
            libc::close(kq);
            writeln!(out, "P7 kqueue READ lowat=2^30 ({name}): register={r} events: [{}]", evs.join("; ")).unwrap();
        }
        drop(m);
        let _ = c.wait();
    }
    writeln!(out, "P6b blocking poll(0) on the master while the child runs 300 ms: -> {r} revents={} after {woke} ms", rev(p.revents)).unwrap();
    drop(m);
}

const TIOCSDRAINWAIT: libc::c_ulong = 0x8004_7457;
const TIOCGDRAINWAIT: libc::c_ulong = 0x4004_7456;

fn ps_state(pid: i32) -> String {
    let o = std::process::Command::new("/bin/ps").args(["-o", "stat=,wchan=", "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&o.stdout).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Child prints and exits while nobody reads the master. Optionally set a drain wait,
/// or SIGKILL the stuck child. Never blocks for more than about 8 s.
fn p1(dir: &Path, out: &mut std::io::Stdout, drainwait: Option<i32>, sigkill: bool) {
    let m = pty::open_master().unwrap();
    let mut dw: i32 = -1;
    unsafe { libc::ioctl(m.as_raw_fd(), TIOCGDRAINWAIT, &mut dw) };
    let mut set = String::from("-");
    if let Some(v) = drainwait {
        let r = unsafe { libc::ioctl(m.as_raw_fd(), TIOCSDRAINWAIT, &v) };
        set = if r < 0 { format!("set {v}s failed: {}", std::io::Error::last_os_error()) } else { format!("set {v}s ok") };
    }
    let mut c = pty::spawn(
        m.as_fd(),
        &Spawn { argv0: "s3probe-sh", prog: "/bin/sh", args: &["-c", "echo final-words; exit 3"], env: &[("PATH", "/usr/bin:/bin")], cwd: dir, size: Size::DEFAULT },
    )
    .unwrap();
    let pid = c.id() as i32;
    let t = Instant::now();
    std::thread::sleep(Duration::from_millis(300));
    let state = ps_state(pid);
    let hup = rev(poll_revents(m.as_fd(), 0));
    if sigkill {
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    let mut exited = None;
    while t.elapsed() < Duration::from_secs(8) {
        if let Some(st) = c.try_wait().unwrap() {
            exited = Some((t.elapsed().as_millis(), st));
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let after = rev(poll_revents(m.as_fd(), 0));
    let mut buf = [0u8; 4096];
    let mut reads = vec![];
    for _ in 0..2 {
        match pty::read_timeout(m.as_fd(), &mut buf, Duration::from_millis(200)).unwrap() {
            Read::Data(n) => reads.push(format!("{:?}", String::from_utf8_lossy(&buf[..n]))),
            Read::Timeout => reads.push("timeout".into()),
            Read::End => reads.push("EIO".into()),
        }
    }
    let late = if exited.is_none() {
        let t2 = Instant::now();
        let st = c.wait().unwrap();
        format!("; after the master was read the child exited in {} ms ({st})", t2.elapsed().as_millis())
    } else {
        String::new()
    };
    writeln!(
        out,
        "P1 drainwait default={dw} {set} sigkill={sigkill}: at 300 ms child state '{state}', master poll(0)={hup}; exited within 8 s: {:?}; poll(0) then={after}; reads: {}{late}",
        exited.map(|(ms, st)| format!("{ms} ms {st}")),
        reads.join(" / ")
    )
    .unwrap();
}

/// Flood a PTY nobody reads; stop the writer, then drain to see what the kernel buffered.
fn p4(dir: &Path, out: &mut std::io::Stdout, speed: Option<libc::speed_t>) {
    let prog = dir.join("flood.progress");
    let _ = std::fs::remove_file(&prog);
    let exe = std::env::current_exe().unwrap();
    let script = format!("exec '{}' flood '{}'", exe.display(), prog.display());
    let (m, mut c) = sh(dir, &script, Size::DEFAULT);
    let mut tio: libc::termios = unsafe { std::mem::zeroed() };
    unsafe { libc::tcgetattr(m.as_raw_fd(), &mut tio) };
    let before = unsafe { libc::cfgetospeed(&tio) };
    if let Some(sp) = speed {
        unsafe {
            libc::cfsetspeed(&mut tio, sp);
            libc::tcsetattr(m.as_raw_fd(), libc::TCSANOW, &tio);
        }
    }
    std::thread::sleep(Duration::from_millis(1000));
    let p1 = std::fs::read_to_string(&prog).unwrap_or_default();
    std::thread::sleep(Duration::from_millis(1000));
    let p2 = std::fs::read_to_string(&prog).unwrap_or_default();
    let state = ps_state(c.id() as i32);
    unsafe { libc::kill(c.id() as i32, libc::SIGSTOP) };
    std::thread::sleep(Duration::from_millis(50));
    let mut acc = Vec::new();
    let mut buf = vec![0u8; 65536];
    while let Read::Data(n) = pty::read_timeout(m.as_fd(), &mut buf, Duration::from_millis(100)).unwrap() {
        acc.extend_from_slice(&buf[..n]);
    }
    let text = String::from_utf8_lossy(&acc);
    let nums: Vec<u64> = text.lines().filter_map(|l| l.trim().strip_prefix('L')?.parse().ok()).collect();
    let gaps = nums.windows(2).filter(|w| w[1] != w[0] + 1).count();
    writeln!(
        out,
        "P4 speed {before}->{:?}: writes done after 1 s = {}, after 2 s = {}, writer state '{state}'; buffered {} bytes = lines {:?}..{:?}, gaps {gaps}",
        speed,
        p1.trim(),
        p2.trim(),
        acc.len(),
        nums.first(),
        nums.last()
    )
    .unwrap();
    drop(m); // hang up first, or the killed writer would wait in exit for a reader
    unsafe {
        libc::kill(c.id() as i32, libc::SIGKILL);
        libc::kill(c.id() as i32, libc::SIGCONT);
    }
    let _ = c.wait();
}

/// Register NOTE_EXIT on `pid`, run `trigger`, wait up to 2 s. Returns (fired, data, ms).
pub fn note_exit(pid: i32, trigger: impl FnOnce()) -> (bool, i64, f64) {
    unsafe {
        let kq = libc::kqueue();
        let mut ev: libc::kevent = std::mem::zeroed();
        ev.ident = pid as usize;
        ev.filter = libc::EVFILT_PROC;
        ev.flags = libc::EV_ADD | libc::EV_ONESHOT;
        ev.fflags = libc::NOTE_EXIT | libc::NOTE_EXITSTATUS;
        let r = libc::kevent(kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null());
        if r < 0 {
            libc::close(kq);
            return (false, -1, 0.0);
        }
        let t = Instant::now();
        trigger();
        let mut out: libc::kevent = std::mem::zeroed();
        let ts = libc::timespec { tv_sec: 2, tv_nsec: 0 };
        let n = libc::kevent(kq, std::ptr::null(), 0, &mut out, 1, &ts);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        libc::close(kq);
        (n == 1, out.data as i64, ms)
    }
}

/// `die flood <progress>`: write numbered lines to stdout as fast as possible,
/// recording how many writes completed.
pub fn flood(progress: &Path) {
    let f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(progress).unwrap();
    let mut i: u64 = 0;
    loop {
        i += 1;
        let line = format!("L{i:08}\n");
        if pty::write_all(unsafe { BorrowedFd::borrow_raw(1) }, line.as_bytes()).is_err() {
            return;
        }
        let rec = format!("{i:<20}\n");
        unsafe { libc::pwrite(f.as_raw_fd(), rec.as_ptr().cast(), rec.len(), 0) };
    }
}

/// `die race DIR`: one thread receives fds from the store in a loop (they arrive without
/// FD_CLOEXEC until we set it), another spawns children. Count children that inherited
/// an extra fd, with std::process::Command and with posix_spawn + CLOEXEC_DEFAULT.
pub fn race(dir: &Path) {
    use std::sync::atomic::{AtomicBool, Ordering};
    std::env::set_current_dir(dir).unwrap();
    let exe = std::env::current_exe().unwrap();
    let mut store = crate::client::Store::connect_or_spawn(dir, "host", &exe, &["--idle-exit", "2"]).unwrap();
    let m = pty::open_master().unwrap();
    store.put("race/0", m.as_fd(), &vec![], true).unwrap();
    drop(m);
    for variant in ["std-command", "posix_spawn-cloexec-default"] {
        let stop = AtomicBool::new(false);
        let (mut leaked, mut total) = (0, 0);
        let mut kinds: Vec<String> = vec![];
        std::thread::scope(|s| {
            s.spawn(|| {
                let mut st = crate::client::Store::connect(dir, "host").unwrap();
                while !stop.load(Ordering::Relaxed) {
                    let _ = st.get("race/0");
                }
            });
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(10) {
                let pid = if variant == "std-command" {
                    let c = std::process::Command::new("/bin/sleep").arg("0.2").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
                    c.id() as i32
                } else {
                    spawn_cloexec_default(&["/bin/sleep", "0.2"])
                };
                std::thread::sleep(Duration::from_millis(30));
                let n = pty::count_fds(pid).unwrap_or(0);
                total += 1;
                if n > 3 {
                    leaked += 1;
                    kinds.push(fd_kinds(pid));
                }
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                    libc::waitpid(pid, std::ptr::null_mut(), 0);
                }
            }
            stop.store(true, Ordering::Relaxed);
        });
        println!("RACE {variant}: {leaked} of {total} children inherited an extra fd; fds (fd:type, 1=vnode 2=socket): {kinds:?}");
    }
    let _ = store.drop_label("race/0");
}

fn spawn_cloexec_default(argv: &[&str]) -> i32 {
    unsafe {
        let mut attr: libc::posix_spawnattr_t = std::mem::zeroed();
        libc::posix_spawnattr_init(&mut attr);
        libc::posix_spawnattr_setflags(&mut attr, libc::POSIX_SPAWN_CLOEXEC_DEFAULT as libc::c_short);
        let mut fa: libc::posix_spawn_file_actions_t = std::mem::zeroed();
        libc::posix_spawn_file_actions_init(&mut fa);
        for fd in 0..3 {
            libc::posix_spawn_file_actions_addopen(&mut fa, fd, c"/dev/null".as_ptr(), libc::O_RDWR, 0);
        }
        let cs: Vec<std::ffi::CString> = argv.iter().map(|a| std::ffi::CString::new(*a).unwrap()).collect();
        let mut ptrs: Vec<*mut libc::c_char> = cs.iter().map(|c| c.as_ptr() as *mut _).collect();
        ptrs.push(std::ptr::null_mut());
        let env: [*mut libc::c_char; 1] = [std::ptr::null_mut()];
        let mut pid = 0;
        let r = libc::posix_spawn(&mut pid, cs[0].as_ptr(), &fa, &attr, ptrs.as_ptr(), env.as_ptr());
        assert_eq!(r, 0, "posix_spawn");
        libc::posix_spawn_file_actions_destroy(&mut fa);
        libc::posix_spawnattr_destroy(&mut attr);
        pid
    }
}

fn fd_kinds(pid: i32) -> String {
    let mut buf = [[0u32; 2]; 64];
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, buf.as_mut_ptr().cast(), std::mem::size_of_val(&buf) as i32) };
    (0..(n.max(0) as usize / 8)).map(|i| format!("{}:{}", buf[i][0], buf[i][1])).collect::<Vec<_>>().join(",")
}

/// `die ctty DIR`: spawn a shell with posix_spawn(SETSID | CLOEXEC_DEFAULT) and the slave
/// opened by a file action. Does it get a controlling terminal, and SIGHUP on hangup?
pub fn ctty(dir: &Path) {
    let m = pty::open_master().unwrap();
    // TIOCSWINSZ on a master fails with ENOTTY until a slave has been opened.
    let early = pty::set_size(m.as_fd(), Size::DEFAULT).err().map(|e| e.to_string());
    println!("CTTY TIOCSWINSZ on a fresh master before the slave is opened: {early:?}");
    let name = rustix::pty::ptsname(&m, Vec::new()).unwrap();
    let hup = dir.join("ctty-hup.txt");
    let _ = std::fs::remove_file(&hup);
    let script = format!("trap 'echo HUP >> {}; exit 0' HUP; tty; ps -o pid=,tt=,stat= -p $$; echo fds=$(ls /dev/fd | wc -l); while :; do /bin/sleep 0.05; done", hup.display());
    let pid = unsafe {
        let mut attr: libc::posix_spawnattr_t = std::mem::zeroed();
        libc::posix_spawnattr_init(&mut attr);
        libc::posix_spawnattr_setflags(&mut attr, (libc::POSIX_SPAWN_CLOEXEC_DEFAULT | 0x0400 /* POSIX_SPAWN_SETSID, sys/spawn.h */) as libc::c_short);
        let mut fa: libc::posix_spawn_file_actions_t = std::mem::zeroed();
        libc::posix_spawn_file_actions_init(&mut fa);
        libc::posix_spawn_file_actions_addopen(&mut fa, 0, name.as_ptr(), libc::O_RDWR, 0);
        libc::posix_spawn_file_actions_adddup2(&mut fa, 0, 1);
        libc::posix_spawn_file_actions_adddup2(&mut fa, 0, 2);
        let argv: Vec<std::ffi::CString> = ["/bin/sh", "-c", &script].iter().map(|a| std::ffi::CString::new(*a).unwrap()).collect();
        let mut ptrs: Vec<*mut libc::c_char> = argv.iter().map(|c| c.as_ptr() as *mut _).collect();
        ptrs.push(std::ptr::null_mut());
        let path = std::ffi::CString::new("PATH=/usr/bin:/bin").unwrap();
        let env = [path.as_ptr() as *mut libc::c_char, std::ptr::null_mut()];
        let mut pid = 0;
        let r = libc::posix_spawn(&mut pid, argv[0].as_ptr(), &fa, &attr, ptrs.as_ptr(), env.as_ptr());
        assert_eq!(r, 0);
        pid
    };
    let (_, out) = pty::read_until(m.as_fd(), "fds=", Duration::from_secs(3)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let mut rest = [0u8; 256];
    let n = match pty::read_timeout(m.as_fd(), &mut rest, Duration::from_millis(100)).unwrap() { Read::Data(n) => n, _ => 0 };
    let t = Instant::now();
    drop(m);
    let mut st = 0;
    unsafe { libc::waitpid(pid, &mut st, 0) };
    println!(
        "CTTY posix_spawn(SETSID|CLOEXEC_DEFAULT)+addopen(slave): output {:?}; after closing the master: hup file {:?}, exited after {} ms",
        String::from_utf8_lossy(&[out, rest[..n].to_vec()].concat()).replace("\r\n", " | "),
        std::fs::read_to_string(&hup).ok(),
        t.elapsed().as_millis()
    );
}
