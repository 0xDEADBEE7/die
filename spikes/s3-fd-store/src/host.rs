//! The fake DIE host: spawns shells on PTYs, deposits the masters, reattaches after a
//! restart, and tracks the output stream so lost bytes would show up as gaps.

use crate::client::{Error, Store};
use crate::proto::{self, Kv};
use crate::pty::{self, Read, Size, Spawn};
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

pub struct Ctx {
    pub dir: PathBuf,
    pub run: String,
    pub exe: PathBuf,
}

impl Ctx {
    pub fn file(&self, kind: &str, label: &str) -> PathBuf {
        self.dir.join(format!("{kind}-{}.txt", label.replace('/', "_")))
    }
    pub fn store(&self, role: &str) -> Result<Store, Error> {
        Store::connect_or_spawn(&self.dir, role, &self.exe, &["--idle-exit", "3", "--sweep", "1"])
    }
    pub fn hup_lines(&self, label: &str) -> usize {
        std::fs::read_to_string(self.file("hup", label)).map(|s| s.lines().count()).unwrap_or(0)
    }
}

/// What a terminal's metadata looks like to this build. Missing fields take defaults and
/// unknown fields are ignored, which is how build N and build N+1 share a store.
#[derive(Clone, Debug, Default)]
pub struct TermMeta {
    pub pid: i32,
    pub size: Option<Size>,
    pub job: i32,
    #[cfg(feature = "proto-n1")]
    pub cwd: String,
}

impl TermMeta {
    pub fn to_kv(&self) -> Kv {
        let s = self.size.unwrap_or(Size::DEFAULT);
        #[allow(unused_mut)]
        let mut kv: Kv = [("pid", self.pid.to_string()), ("rows", s.rows.to_string()), ("cols", s.cols.to_string()), ("xpix", s.xpix.to_string()), ("ypix", s.ypix.to_string()), ("job", self.job.to_string())]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        #[cfg(feature = "proto-n1")]
        kv.push(("cwd".into(), self.cwd.clone()));
        kv
    }
    pub fn from_kv(kv: &Kv) -> TermMeta {
        let n = |k: &str| proto::get(kv, k).and_then(|v| v.parse::<i64>().ok());
        let size = match (n("rows"), n("cols")) {
            (Some(r), Some(c)) => Some(Size { rows: r as u16, cols: c as u16, xpix: n("xpix").unwrap_or(0) as u16, ypix: n("ypix").unwrap_or(0) as u16 }),
            _ => None,
        };
        TermMeta {
            pid: n("pid").unwrap_or(0) as i32,
            size,
            job: n("job").unwrap_or(0) as i32,
            #[cfg(feature = "proto-n1")]
            cwd: proto::get(kv, "cwd").unwrap_or("").to_string(),
        }
    }
}

/// Tracks `seq N` lines from the writer job. A gap means output was lost.
#[derive(Default, Debug, Clone)]
pub struct Seq {
    pub first: Option<u64>,
    pub last: u64,
    pub gaps: u64,
    pub missing: u64,
    pub bytes: u64,
    pub tail: Vec<u8>,
    pub window: Vec<u8>,
}

impl Seq {
    pub fn feed(&mut self, b: &[u8]) {
        self.bytes += b.len() as u64;
        self.window.extend_from_slice(b);
        if self.window.len() > 65536 {
            self.window.drain(..self.window.len() - 32768);
        }
        self.tail.extend_from_slice(b);
        while let Some(i) = self.tail.iter().position(|&c| c == b'\n') {
            let line: Vec<u8> = self.tail.drain(..=i).collect();
            let s = String::from_utf8_lossy(&line);
            if let Some(n) = s.trim().strip_prefix("seq ").and_then(|v| v.parse::<u64>().ok()) {
                if self.first.is_none() {
                    self.first = Some(n);
                }
                if self.last != 0 && n != self.last + 1 {
                    self.gaps += 1;
                    self.missing += n.saturating_sub(self.last + 1);
                }
                self.last = n;
            }
        }
    }
}

pub enum Job {
    None,
    Writer,
    Sleep,
}

pub struct Term {
    pub label: String,
    pub master: OwnedFd,
    pub meta: TermMeta,
    pub child: Option<Child>,
    pub seq: Seq,
    pub ended: bool,
}

pub fn shell_env(ctx: &Ctx) -> Vec<(&'static str, String)> {
    vec![
        ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin".into()),
        ("HOME", ctx.dir.display().to_string()),
        ("TERM", "xterm-256color".into()),
        ("PS1", "".into()),
        ("PS2", "".into()),
        ("HISTFILE", "/dev/null".into()),
        ("BASH_SILENCE_DEPRECATION_WARNING", "1".into()),
        ("LANG", "C".into()),
    ]
}

/// Spawn a throwaway interactive bash with a HUP trap that logs to a file, start an
/// optional background job, then deposit the master in the store.
pub fn spawn_term(ctx: &Ctx, store: &mut Store, label: &str, job: Job) -> io::Result<Term> {
    let master = pty::open_master()?;
    let env = shell_env(ctx);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let argv0 = format!("s3sh-{}", ctx.run);
    let child = pty::spawn(
        master.as_fd(),
        &Spawn { argv0: &argv0, prog: "/bin/bash", args: &["--norc", "--noprofile", "--noediting", "-i"], env: &env, cwd: &ctx.dir, size: Size::DEFAULT },
    )?;
    let (hup, jobf) = (ctx.file("hup", label), ctx.file("job", label));
    let _ = std::fs::remove_file(&jobf);
    let mut setup = format!("stty -echo\ntrap 'echo \"HUP $$\" >> \"{}\"; trap - HUP; kill -HUP $$' HUP\n", hup.display());
    match job {
        Job::None => {}
        Job::Writer => setup += &format!(
            "/bin/bash --norc --noprofile -c 'i=0; while :; do i=$((i+1)); echo \"seq $i\"; /bin/sleep 0.02; done' s3w-{} &\necho $! > \"{}\"\n",
            ctx.run,
            jobf.display()
        ),
        Job::Sleep => setup += &format!("(exec -a s3j-{} /bin/sleep 100000) &\necho $! > \"{}\"\n", ctx.run, jobf.display()),
    }
    setup += &format!("echo \"rea\"\"dy-{label}\"\n");
    pty::write_all(master.as_fd(), setup.as_bytes())?;
    let mut t = Term { label: label.into(), master, meta: TermMeta { pid: child.id() as i32, ..Default::default() }, child: Some(child), seq: Seq::default(), ended: false };
    if !t.read_for(&format!("ready-{label}"), Duration::from_secs(5)) {
        return Err(io::Error::other(format!("{label}: shell never became ready")));
    }
    if !matches!(job, Job::None) {
        t.meta.job = std::fs::read_to_string(&jobf).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    }
    t.meta.size = Some(Size::DEFAULT);
    #[cfg(feature = "proto-n1")]
    {
        t.meta.cwd = ctx.dir.display().to_string();
    }
    store.put(label, t.master.as_fd(), &t.meta.to_kv(), false).map_err(|e| io::Error::other(format!("put: {e:?}")))?;
    Ok(t)
}

/// Reattach: take a duplicate from the store, then nudge the size so programs redraw.
pub fn attach(store: &mut Store, label: &str, nudge: bool) -> Result<Term, Error> {
    let (master, msg) = store.get(label)?;
    let meta = TermMeta::from_kv(&msg.kv);
    if nudge {
        pty::redraw_nudge(master.as_fd(), Duration::ZERO)?;
    }
    Ok(Term { label: label.into(), master, meta, child: None, seq: Seq::default(), ended: false })
}

impl Term {
    pub fn send(&self, s: &str) -> io::Result<()> {
        pty::write_all(self.master.as_fd(), s.as_bytes())
    }

    /// Read until `needle` shows up in the output, feeding the seq tracker.
    pub fn read_for(&mut self, needle: &str, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        let start = self.seq.window.len();
        let mut buf = [0u8; 16384];
        loop {
            let from = start.min(self.seq.window.len()).saturating_sub(needle.len());
            if pty::contains(&self.seq.window[from..], needle.as_bytes()) {
                return true;
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() || self.ended {
                return false;
            }
            match pty::read_timeout(self.master.as_fd(), &mut buf, left) {
                Ok(Read::Data(n)) => self.seq.feed(&buf[..n]),
                Ok(Read::Timeout) => {}
                Ok(Read::End) | Err(_) => self.ended = true,
            }
        }
    }

    /// Read whatever arrives for `d`.
    pub fn pump(&mut self, d: Duration) {
        let _ = self.read_for("\u{0}never\u{0}", d);
    }

    /// Round trip through the shell. The command is split so its echo can't match.
    pub fn ping(&mut self, token: &str) -> Option<Duration> {
        let t = Instant::now();
        self.send(&format!("echo \"pong\"\"-{token}\"\n")).ok()?;
        self.read_for(&format!("pong-{token}"), Duration::from_secs(3)).then(|| t.elapsed())
    }

    /// Kill-buffer: the store drops its copy, we close ours, and the kernel hangs up the
    /// shell. Returns how long the shell took to go away.
    pub fn kill_buffer(mut self, store: &mut Store) -> Result<Duration, String> {
        store.drop_label(&self.label).map_err(|e| format!("drop: {e:?}"))?;
        let pid = self.meta.pid;
        let t = Instant::now();
        drop(self.master);
        while t.elapsed() < Duration::from_secs(5) {
            let gone = match &mut self.child {
                Some(c) => c.try_wait().map(|s| s.is_some()).unwrap_or(true),
                None => !pty::alive(pid),
            };
            if gone {
                return Ok(t.elapsed());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Err(format!("pid {pid} still alive 5 s after kill-buffer"))
    }
}

/// Processes carrying this run's marker: argv0 `s3sh-<run>…` or `s3j-<run>` (shells,
/// sleep jobs) or a last argument `s3w-<run>` (writer jobs).
pub fn marked_procs(run: &str) -> Vec<String> {
    let o = std::process::Command::new("/bin/ps").args(["-A", "-o", "pid=,stat=,command="]).output().unwrap();
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            let (argv0, last) = (w.get(2).copied().unwrap_or(""), w.last().copied().unwrap_or(""));
            argv0.starts_with(&format!("s3sh-{run}")) || argv0 == format!("s3j-{run}") || last == format!("s3w-{run}")
        })
        .map(|l| l.trim().to_string())
        .collect()
}

pub fn wait_gone(pid: i32, d: Duration) -> bool {
    let t = Instant::now();
    while t.elapsed() < d {
        if !pty::alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

pub fn store_pid(dir: &Path) -> i32 {
    std::fs::read_to_string(dir.join(proto::LOCK)).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}
