//! Host modes: each is one fake-DIE process that a check starts, kills or restarts.

use crate::client::{Error, Store};
use crate::host::{self, Ctx, Job, Term, attach, spawn_term};
use crate::json::{J, Raw};
use crate::jo;
use crate::proto::{self, Msg};
use crate::pty::{self, Read};
use std::io::Write as _;
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::time::{Duration, Instant};

pub struct Args(pub Vec<String>);

impl Args {
    pub fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().position(|a| a == k).and_then(|i| self.0.get(i + 1)).map(|s| s.as_str())
    }
    pub fn num(&self, k: &str, d: i64) -> i64 {
        self.get(k).and_then(|v| v.parse().ok()).unwrap_or(d)
    }
    pub fn has(&self, k: &str) -> bool {
        self.0.iter().any(|a| a == k)
    }
}

pub type R<T> = Result<T, String>;

pub fn e<E: std::fmt::Debug>(x: E) -> String {
    format!("{x:?}")
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect()
}

fn save(path: &Path, m: &Msg) {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, m.encode()).unwrap();
    std::fs::rename(tmp, path).unwrap();
}

fn load(path: &Path) -> R<Msg> {
    Msg::decode(&std::fs::read(path).map_err(e)?).map_err(e)
}

fn session_of(terms: &[Term]) -> Msg {
    let mut m = Msg::new("SESSION").with("saved_ms", pty::now_ms());
    for t in terms {
        m.items.push(vec![
            ("label".into(), t.label.clone()),
            ("pid".into(), t.meta.pid.to_string()),
            ("job".into(), t.meta.job.to_string()),
            ("last".into(), t.seq.last.to_string()),
            ("tail".into(), hex(&t.seq.tail)),
        ]);
    }
    m
}

fn restore(store: &mut Store, s: &Msg, nudge: bool) -> R<(Vec<Term>, bool)> {
    let mut terms = vec![];
    let mut pids_match = true;
    for it in &s.items {
        let label = proto::get(it, "label").unwrap_or("");
        let mut t = attach(store, label, nudge).map_err(e)?;
        pids_match &= proto::get(it, "pid") == Some(t.meta.pid.to_string().as_str());
        t.seq.last = proto::get(it, "last").and_then(|v| v.parse().ok()).unwrap_or(0);
        t.seq.tail = unhex(proto::get(it, "tail").unwrap_or(""));
        terms.push(t);
    }
    Ok((terms, pids_match))
}

/// Bytes the kernel buffered while no host was reading: one immediate read per terminal.
fn first_reads(terms: &mut [Term]) -> Vec<usize> {
    let mut out = vec![];
    let mut buf = vec![0u8; 65536];
    for t in terms.iter_mut() {
        let mut n = 0;
        while let Ok(Read::Data(k)) = pty::read_timeout(t.master.as_fd(), &mut buf, Duration::from_millis(5)) {
            t.seq.feed(&buf[..k]);
            n += k;
            if n > 1 << 20 {
                break;
            }
        }
        out.push(n);
    }
    out
}

fn status_fields(store: &mut Store) -> (String, String, String) {
    let s = store.status().unwrap_or_default();
    (s.get("fds").unwrap_or("").into(), s.get("held").unwrap_or("").into(), s.get("pid").unwrap_or("").into())
}

fn ppid() -> i32 {
    unsafe { libc::getppid() }
}

/// Planned restart chain: attach (or spawn), verify, save, start the next host, exit.
pub fn chain(ctx: &Ctx, a: &Args) -> R<()> {
    let generation = a.num("--gen", 1);
    let total = a.num("--total", 20);
    // Simulated startup time of the new build; every 5th restart outlasts the PTY buffer.
    let startup_ms = if generation == 1 { 0 } else if generation % 5 == 0 { 3000 } else { 300 };
    std::thread::sleep(Duration::from_millis(startup_ms));
    let t0 = Instant::now();
    let mut store = ctx.store("host").map_err(e)?;
    let sess = ctx.dir.join("session.txt");
    let (mut terms, pids_match, saved_ms) = if generation == 1 {
        let mut v = vec![];
        for i in 0..a.num("--shells", 3) {
            v.push(spawn_term(ctx, &mut store, &format!("term/{i}"), Job::Writer).map_err(e)?);
        }
        (v, true, 0)
    } else {
        let s = load(&sess)?;
        let (v, ok) = restore(&mut store, &s, true)?;
        (v, ok, s.get("saved_ms").and_then(|v| v.parse::<u128>().ok()).unwrap_or(0))
    };
    let attach_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let gap_ms = if saved_ms > 0 { (pty::now_ms() - saved_ms) as i64 } else { -1 };
    let buffered = first_reads(&mut terms);
    for t in terms.iter_mut() {
        t.pump(Duration::from_millis(50));
    }
    let mut pings = vec![];
    for (i, t) in terms.iter_mut().enumerate() {
        pings.push(t.ping(&format!("g{generation}-{i}")).map(|d| d.as_secs_f64() * 1000.0));
    }
    let hup: usize = terms.iter().map(|t| ctx.hup_lines(&t.label)).sum();
    let (sfds, held, spid) = status_fields(&mut store);
    let line = jo! {
        "gen": generation, "pid": std::process::id() as i32, "ppid": ppid(), "startup_ms": startup_ms, "attach_ms": attach_ms, "gap_ms": gap_ms,
        "buffered_bytes": buffered, "pids_match": pids_match,
        "shells_alive": terms.iter().map(|t| pty::alive(t.meta.pid)).collect::<Vec<_>>(),
        "jobs_alive": terms.iter().map(|t| pty::alive(t.meta.job)).collect::<Vec<_>>(),
        "shell_pids": terms.iter().map(|t| t.meta.pid).collect::<Vec<_>>(),
        "ping_ms": pings, "seq_first": terms.iter().map(|t| t.seq.first).collect::<Vec<_>>(),
        "seq_last": terms.iter().map(|t| t.seq.last).collect::<Vec<_>>(),
        "gaps": terms.iter().map(|t| t.seq.gaps).collect::<Vec<_>>(), "missing": terms.iter().map(|t| t.seq.missing).collect::<Vec<_>>(),
        "hup_lines": hup, "host_fds": pty::count_fds(std::process::id() as i32).unwrap_or(0),
        "store_fds": Raw(sfds), "store_held": Raw(held), "store_pid": Raw(spid),
    };
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(ctx.dir.join("chain.jsonl")).map_err(e)?;
    writeln!(f, "{line}").map_err(e)?;
    save(&sess, &session_of(&terms));
    if generation < total {
        let log = std::fs::File::create(ctx.dir.join(format!("host-chain-{}.log", generation + 1))).map_err(e)?;
        std::process::Command::new(&ctx.exe)
            .args(["host", "chain", "--dir"])
            .arg(&ctx.dir)
            .args(["--run", &ctx.run, "--gen", &(generation + 1).to_string(), "--total", &total.to_string()])
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .map_err(e)?;
    } else {
        std::fs::write(ctx.dir.join("chain.done"), "done\n").map_err(e)?;
    }
    Ok(()) // exit: our copies close, the store's stay
}

fn write_progress(ctx: &Ctx, t: &Term) {
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(ctx.file("progress", &t.label)).unwrap();
    let rec = format!("{:<20} {:<200}\n", t.seq.last, hex(&t.seq.tail));
    unsafe { libc::pwrite(f.as_raw_fd(), rec.as_ptr().cast(), rec.len(), 0) };
}

fn read_progress(ctx: &Ctx, label: &str) -> (u64, Vec<u8>) {
    let s = std::fs::read_to_string(ctx.file("progress", label)).unwrap_or_default();
    let mut w = s.split_whitespace();
    (w.next().and_then(|v| v.parse().ok()).unwrap_or(0), unhex(w.next().unwrap_or("")))
}

/// A host that runs until killed. Generation 0 spawns; later ones reattach. With
/// --redeposit it notices a dead store, starts a new one and deposits its fds again.
pub fn serve(ctx: &Ctx, a: &Args) -> R<()> {
    let generation = a.num("--gen", 0);
    let t0 = Instant::now();
    let mut store = ctx.store("host").map_err(e)?;
    let sess = ctx.dir.join("serve-session.txt");
    let (mut terms, pids_match) = if generation == 0 {
        let mut v = vec![];
        for i in 0..a.num("--shells", 2) {
            v.push(spawn_term(ctx, &mut store, &format!("term/{i}"), Job::Writer).map_err(e)?);
        }
        save(&sess, &session_of(&v));
        (v, true)
    } else {
        let (mut v, ok) = restore(&mut store, &load(&sess)?, true)?;
        for t in v.iter_mut() {
            (t.seq.last, t.seq.tail) = read_progress(ctx, &t.label);
        }
        (v, ok)
    };
    let attach_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let buffered = first_reads(&mut terms);
    let mut pings = vec![];
    for (i, t) in terms.iter_mut().enumerate() {
        pings.push(t.ping(&format!("s{generation}-{i}")).map(|d| d.as_secs_f64() * 1000.0));
        write_progress(ctx, t);
    }
    let hup: usize = terms.iter().map(|t| ctx.hup_lines(&t.label)).sum();
    let (sfds, held, spid) = status_fields(&mut store);
    let ready = jo! {
        "gen": generation, "pid": std::process::id() as i32, "ppid": ppid(), "attach_ms": attach_ms, "pids_match": pids_match,
        "shell_pids": terms.iter().map(|t| t.meta.pid).collect::<Vec<_>>(),
        "shells_alive": terms.iter().map(|t| pty::alive(t.meta.pid)).collect::<Vec<_>>(),
        "jobs_alive": terms.iter().map(|t| pty::alive(t.meta.job)).collect::<Vec<_>>(),
        "buffered_bytes": buffered, "ping_ms": pings,
        "seq_first": terms.iter().map(|t| t.seq.first).collect::<Vec<_>>(),
        "gaps": terms.iter().map(|t| t.seq.gaps).collect::<Vec<_>>(), "missing": terms.iter().map(|t| t.seq.missing).collect::<Vec<_>>(),
        "hup_lines": hup, "store_fds": Raw(sfds), "store_held": Raw(held), "store_pid": Raw(spid),
    };
    std::fs::write(ctx.dir.join(format!("serve-{generation}.json")), ready).map_err(e)?;
    loop {
        for t in terms.iter_mut() {
            t.pump(Duration::from_millis(20));
            write_progress(ctx, t);
        }
        if a.has("--redeposit") {
            let mut p = libc::pollfd { fd: store.as_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 };
            unsafe { libc::poll(&mut p, 1, 0) };
            if p.revents != 0 {
                let t = Instant::now();
                store = ctx.store("host").map_err(e)?;
                for term in &terms {
                    store.put(&term.label, term.master.as_fd(), &term.meta.to_kv(), true).map_err(e)?;
                }
                let ev = jo! {"event": "store-lost", "new_store_pid": store.pid, "redeposited": terms.len(), "ms": t.elapsed().as_secs_f64() * 1000.0};
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(ctx.dir.join(format!("serve-{generation}.events"))).map_err(e)?;
                writeln!(f, "{ev}").map_err(e)?;
            }
        }
    }
}

/// 100 open/kill-buffer cycles in one host, counting fds and processes.
pub fn cycles(ctx: &Ctx, a: &Args) -> R<()> {
    let n = a.num("--n", 100);
    let mut store = ctx.store("host").map_err(e)?;
    let me = std::process::id() as i32;
    let host0 = pty::count_fds(me).unwrap_or(0);
    let (s0, _, _) = status_fields(&mut store);
    let mut samples = vec![];
    let (mut hup_ok, mut jobs_gone, mut fails) = (0, 0, vec![]);
    let mut kill_ms = vec![];
    let t0 = Instant::now();
    for i in 0..n {
        let label = format!("cyc/{i}");
        let t = match spawn_term(ctx, &mut store, &label, Job::Sleep) {
            Ok(t) => t,
            Err(x) => {
                fails.push(format!("{i}: spawn {x}"));
                continue;
            }
        };
        let job = t.meta.job;
        match t.kill_buffer(&mut store) {
            Ok(d) => kill_ms.push(d.as_secs_f64() * 1000.0),
            Err(x) => fails.push(format!("{i}: {x}")),
        }
        let w = Instant::now();
        while ctx.hup_lines(&label) == 0 && w.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(2));
        }
        hup_ok += (ctx.hup_lines(&label) == 1) as usize;
        jobs_gone += host::wait_gone(job, Duration::from_secs(2)) as usize;
        if i % 10 == 9 {
            let (sf, held, _) = status_fields(&mut store);
            samples.push(jo! {"cycle": i + 1, "host_fds": pty::count_fds(me).unwrap_or(0), "store_fds": Raw(sf), "store_held": Raw(held)});
        }
    }
    let (s1, held, spid) = status_fields(&mut store);
    let tomb = store.status().ok().and_then(|s| s.get("tombstones").map(String::from)).unwrap_or_default();
    std::thread::sleep(Duration::from_millis(300));
    kill_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let out = jo! {
        "cycles": n, "secs": t0.elapsed().as_secs_f64(), "hup_delivered": hup_ok, "jobs_gone": jobs_gone, "failures": fails,
        "kill_buffer_ms_p50": kill_ms.get(kill_ms.len() / 2).copied(), "kill_buffer_ms_max": kill_ms.last().copied(),
        "host_fds_before": host0, "host_fds_after": pty::count_fds(me).unwrap_or(0),
        "store_fds_before": Raw(s0), "store_fds_after": Raw(s1), "store_held_after": Raw(held), "store_tombstones": Raw(tomb), "store_pid": Raw(spid),
        "marked_procs_left": host::marked_procs(&ctx.run), "samples": samples.into_iter().map(Raw).collect::<Vec<_>>(),
    };
    std::fs::write(ctx.dir.join("cycles.json"), out).map_err(e)?;
    Ok(())
}

/// Spawn shells, deposit them, exit without reading again.
pub fn spawn_only(ctx: &Ctx, a: &Args) -> R<()> {
    let mut store = ctx.store("host").map_err(e)?;
    let prefix = a.get("--prefix").unwrap_or("t");
    let job = match a.get("--job") {
        Some("writer") => || Job::Writer,
        Some("sleep") => || Job::Sleep,
        _ => || Job::None,
    };
    let mut out = vec![];
    for i in 0..a.num("--n", 1) {
        let label = a.get("--label").map(String::from).unwrap_or(format!("{prefix}/{i}"));
        let t = spawn_term(ctx, &mut store, &label, job()).map_err(e)?;
        out.push(jo! {"label": t.label.clone(), "pid": t.meta.pid, "job": t.meta.job});
    }
    std::fs::write(ctx.dir.join(format!("spawned-{prefix}.json")), out.into_iter().map(Raw).collect::<Vec<_>>().j()).map_err(e)?;
    Ok(())
}

fn decode_status(st: i64) -> String {
    let st = st as i32;
    if libc::WIFEXITED(st) {
        format!("exit {}", libc::WEXITSTATUS(st))
    } else if libc::WIFSIGNALED(st) {
        format!("signal {}", libc::WTERMSIG(st))
    } else {
        format!("raw {st:#x}")
    }
}

/// One exit-detection trial on a shell we are not the parent of: time from the trigger to
/// EIO on the master and to kqueue NOTE_EXIT on the pid.
fn exit_trial(t: Term, store: &mut Store, kind: &str, i: usize, read_delay: Duration) -> String {
    let pid = t.meta.pid;
    let kq = unsafe { libc::kqueue() };
    let mut ev: libc::kevent = unsafe { std::mem::zeroed() };
    ev.ident = pid as usize;
    ev.filter = libc::EVFILT_PROC;
    ev.flags = libc::EV_ADD | libc::EV_ONESHOT;
    ev.fflags = libc::NOTE_EXIT | libc::NOTE_EXITSTATUS;
    let reg = unsafe { libc::kevent(kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    let token = format!("bye-{i}");
    let t0 = Instant::now();
    match kind {
        "sigkill" => unsafe {
            libc::kill(pid, libc::SIGKILL);
        },
        "exit7" => t.send("exit 7\n").unwrap(),
        "bye-exit3" => t.send(&format!("echo \"bye\"\"-{i}\"; exit 3\n")).unwrap(),
        "bg-holder" => t.send("( trap '' HUP; exec /bin/sleep 2 ) &\nexit 0\n").unwrap(),
        _ => unreachable!(),
    }
    let (note, eio, saw) = std::thread::scope(|s| {
        let h = s.spawn(|| {
            let mut o: libc::kevent = unsafe { std::mem::zeroed() };
            let ts = libc::timespec { tv_sec: 5, tv_nsec: 0 };
            let n = unsafe { libc::kevent(kq, std::ptr::null(), 0, &mut o, 1, &ts) };
            (n == 1).then(|| (t0.elapsed().as_secs_f64() * 1000.0, o.data as i64))
        });
        std::thread::sleep(read_delay);
        let mut acc = vec![];
        let mut buf = [0u8; 4096];
        let mut eio = None;
        while t0.elapsed() < Duration::from_secs(5) {
            match pty::read_timeout(t.master.as_fd(), &mut buf, Duration::from_millis(100)) {
                Ok(Read::Data(n)) => acc.extend_from_slice(&buf[..n]),
                Ok(Read::Timeout) => {}
                Ok(Read::End) | Err(_) => {
                    eio = Some(t0.elapsed().as_secs_f64() * 1000.0);
                    break;
                }
            }
        }
        (h.join().unwrap(), eio, pty::contains(&acc, token.as_bytes()))
    });
    unsafe { libc::close(kq) };
    let state = store.list().ok().and_then(|l| l.into_iter().find(|it| proto::get(it, "label") == Some(t.label.as_str())).and_then(|it| proto::get(&it, "state").map(String::from)));
    let _ = store.drop_label(&t.label);
    jo! {
        "trial": i, "kind": kind, "read_delay_ms": read_delay.as_millis(), "kevent_register": reg,
        "note_exit_ms": note.map(|n| n.0), "status": note.map(|n| decode_status(n.1)),
        "eio_ms": eio, "final_output_read": if kind == "bye-exit3" { Some(saw) } else { None },
        "store_state_after": state,
    }
}

pub fn exit_detect(ctx: &Ctx, a: &Args) -> R<()> {
    let mut store = ctx.store("host").map_err(e)?;
    let prefix = a.get("--prefix").unwrap_or("ex");
    let mut out = vec![];
    for i in 0..a.num("--n", 21) as usize {
        let t = attach(&mut store, &format!("{prefix}/{i}"), false).map_err(e)?;
        if pty::read_timeout(t.master.as_fd(), &mut [0u8; 4096], Duration::from_millis(20)).is_err() {
            return Err("read".into());
        }
        let (kind, delay) = match i {
            0..=9 => (["sigkill", "exit7"][i % 2], 0),
            10..=14 => ("bye-exit3", 0),
            15..=17 => ("bye-exit3", 500),
            _ => ("bg-holder", 0),
        };
        out.push(Raw(exit_trial(t, &mut store, kind, i, Duration::from_millis(delay))));
    }
    std::fs::write(ctx.dir.join("exit.json"), out.j()).map_err(e)?;
    Ok(())
}

/// Version checks: report what this build sees in the store, optionally add a terminal.
pub fn ver(ctx: &Ctx, a: &Args) -> R<()> {
    let tag = a.get("--tag").unwrap_or("x").to_string();
    let path = ctx.dir.join(format!("ver-{tag}.json"));
    let spawn = a.has("--spawn");
    let r = if spawn { ctx.store("host") } else { Store::connect(&ctx.dir, "host") };
    let mut store = match r {
        Ok(s) => s,
        Err(Error::Refused { code, store_proto, store_build, msg }) => {
            let o = jo! {"client_build": proto::BUILD, "client_proto": proto::proto(), "refused": true, "code": code, "store_proto": store_proto, "store_build": store_build, "msg": msg};
            std::fs::write(&path, o).map_err(e)?;
            std::process::exit(3);
        }
        Err(x) => return Err(e(x)),
    };
    let mut entries = vec![];
    for it in store.list().map_err(e)? {
        if proto::get(&it, "state") != Some("live") {
            continue;
        }
        let label = proto::get(&it, "label").unwrap_or("").to_string();
        let mut t = attach(&mut store, &label, false).map_err(e)?;
        let raw_keys: Vec<String> = it.iter().map(|(k, _)| k.clone()).collect();
        #[cfg(feature = "proto-n1")]
        let cwd = Some(t.meta.cwd.clone());
        #[cfg(not(feature = "proto-n1"))]
        let cwd: Option<String> = None;
        let ping = t.ping(&format!("{tag}-{label}")).map(|d| d.as_secs_f64() * 1000.0);
        entries.push(Raw(jo! {"label": label, "pid": t.meta.pid, "alive": pty::alive(t.meta.pid), "raw_keys": raw_keys, "parsed_cwd": cwd, "ping_ms": ping}));
    }
    if let Some(label) = a.get("--add") {
        spawn_term(ctx, &mut store, label, Job::None).map_err(e)?;
    }
    let st = store.status().map_err(e)?;
    let o = jo! {
        "client_build": proto::BUILD, "client_proto": proto::proto(), "refused": false,
        "store_build": store.build.clone(), "store_proto": store.proto.clone(), "store_pid": store.pid,
        "status_build": st.get("build").unwrap_or(""), "entries": entries, "added": a.get("--add"),
    };
    std::fs::write(&path, o).map_err(e)?;
    Ok(())
}

/// Kill-buffer every live terminal in the store; clear tombstones.
pub fn kill_all(ctx: &Ctx, _a: &Args) -> R<()> {
    let mut store = match Store::connect(&ctx.dir, "host") {
        Ok(s) => s,
        Err(x) => {
            std::fs::write(ctx.dir.join("kill-all.json"), jo! {"error": e(x)}).map_err(e)?;
            return Ok(());
        }
    };
    let mut res = vec![];
    for it in store.list().map_err(e)? {
        let label = proto::get(&it, "label").unwrap_or("").to_string();
        if proto::get(&it, "state") != Some("live") {
            let _ = store.drop_label(&label);
            continue;
        }
        let before = ctx.hup_lines(&label);
        let t = attach(&mut store, &label, false).map_err(e)?;
        let job = t.meta.job;
        let r = t.kill_buffer(&mut store);
        let w = Instant::now();
        while ctx.hup_lines(&label) == before && w.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(2));
        }
        res.push(Raw(jo! {"label": label, "ms": r.as_ref().ok().map(|d| d.as_secs_f64() * 1000.0), "error": r.err(), "hup_added": ctx.hup_lines(&label) - before, "job_gone": job == 0 || host::wait_gone(job, Duration::from_secs(2))}));
    }
    std::thread::sleep(Duration::from_millis(200));
    let o = jo! {"killed": res, "marked_procs_left": host::marked_procs(&ctx.run)};
    std::fs::write(ctx.dir.join("kill-all.json"), o).map_err(e)?;
    Ok(())
}

/// The S2 shape: an engine helper takes the PTY from the store and deposits its own
/// channel (one end of a socketpair) so a restarted host can find it.
pub fn helper(ctx: &Ctx, _a: &Args) -> R<()> {
    let mut store = Store::connect(&ctx.dir, "helper").map_err(e)?;
    let mut t = attach(&mut store, "term/1/pty", false).map_err(e)?;
    let (mine, theirs) = std::os::unix::net::UnixStream::pair().map_err(e)?;
    store.put("term/1/engine", theirs.as_fd(), &vec![("helper_pid".into(), std::process::id().to_string())], true).map_err(e)?;
    drop(theirs);
    mine.set_nonblocking(true).map_err(e)?;
    std::fs::write(ctx.dir.join("helper.ready"), std::process::id().to_string()).map_err(e)?;
    let mut buf = [0u8; 256];
    loop {
        t.pump(Duration::from_millis(20));
        if let Ok(n) = rustix::io::read(&mine, &mut buf) {
            if n == 0 {
                continue;
            }
            let req = String::from_utf8_lossy(&buf[..n]).trim().to_string();
            let _ = rustix::io::write(&mine, format!("PONG {req} seq={}\n", t.seq.last).as_bytes());
        }
    }
}

/// A restarted host talks to the engine helper through the channel it left in the store.
pub fn helper_client(ctx: &Ctx, a: &Args) -> R<()> {
    let mut store = Store::connect(&ctx.dir, "host").map_err(e)?;
    let tag = a.get("--tag").unwrap_or("x");
    let r = match store.get("term/1/engine") {
        Ok((fd, meta)) => {
            let s = std::os::unix::net::UnixStream::from(fd);
            s.set_read_timeout(Some(Duration::from_secs(2))).map_err(e)?;
            rustix::io::write(&s, format!("PING {tag}\n").as_bytes()).map_err(e)?;
            let mut buf = [0u8; 256];
            let n = rustix::io::read(&s, &mut buf).unwrap_or(0);
            jo! {"tag": tag, "helper_pid": meta.get("helper_pid").unwrap_or(""), "reply": String::from_utf8_lossy(&buf[..n]).trim().to_string()}
        }
        Err(x) => jo! {"tag": tag, "error": e(x)},
    };
    std::fs::write(ctx.dir.join(format!("helper-client-{tag}.json")), r).map_err(e)?;
    Ok(())
}

/// The .app host: spawn and serve until killed, or attach, ping and exit normally.
pub fn app(ctx: &Ctx, a: &Args) -> R<()> {
    let phase = a.get("--phase").unwrap_or("spawn").to_string();
    let mut store = ctx.store("host").map_err(e)?;
    let mut terms = vec![];
    if phase == "spawn" {
        for i in 0..2 {
            terms.push(spawn_term(ctx, &mut store, &format!("term/{i}"), Job::Writer).map_err(e)?);
        }
        save(&ctx.dir.join("app-session.txt"), &session_of(&terms));
    } else {
        terms = restore(&mut store, &load(&ctx.dir.join("app-session.txt"))?, true)?.0;
    }
    let mut pings = vec![];
    for (i, t) in terms.iter_mut().enumerate() {
        pings.push(t.ping(&format!("{phase}-{i}")).map(|d| d.as_secs_f64() * 1000.0));
    }
    let o = jo! {
        "phase": phase.clone(), "pid": std::process::id() as i32, "ppid": ppid(), "exe": ctx.exe.display().to_string(),
        "nofile_soft_hard": vec![pty::nofile().0, pty::nofile().1],
        "store_pid": store.pid, "shell_pids": terms.iter().map(|t| t.meta.pid).collect::<Vec<_>>(), "ping_ms": pings,
        "hup_lines": terms.iter().map(|t| ctx.hup_lines(&t.label)).sum::<usize>(),
    };
    std::fs::write(ctx.dir.join(format!("app-{phase}.json")), o).map_err(e)?;
    if phase == "spawn" {
        loop {
            for t in terms.iter_mut() {
                t.pump(Duration::from_millis(20));
            }
        }
    }
    Ok(())
}

/// Buffer check: a writer floods a PTY that nobody reads.
pub fn flood_spawn(ctx: &Ctx, _a: &Args) -> R<()> {
    let mut store = ctx.store("host").map_err(e)?;
    let m = pty::open_master().map_err(e)?;
    let prog = ctx.dir.join("flood.progress");
    let argv0 = format!("s3sh-{}", ctx.run);
    let child = pty::spawn(
        m.as_fd(),
        &pty::Spawn { argv0: &argv0, prog: ctx.exe.to_str().unwrap(), args: &["flood", prog.to_str().unwrap()], env: &[], cwd: &ctx.dir, size: pty::Size::DEFAULT },
    )
    .map_err(e)?;
    store.put("flood/0", m.as_fd(), &vec![("pid".into(), child.id().to_string())], false).map_err(e)?;
    std::fs::write(ctx.dir.join("flood.pid"), child.id().to_string()).map_err(e)?;
    Ok(())
}

pub fn flood_attach(ctx: &Ctx, _a: &Args) -> R<()> {
    let mut store = ctx.store("host").map_err(e)?;
    let progress_before: u64 = std::fs::read_to_string(ctx.dir.join("flood.progress")).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let (m, meta) = store.get("flood/0").map_err(e)?;
    let pid: i32 = meta.get("pid").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut acc = 0usize;
    let mut first_burst = 0usize;
    let mut nums: (Option<u64>, u64, u64) = (None, 0, 0); // first, last, gaps
    let mut tail = Vec::new();
    let mut buf = vec![0u8; 65536];
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(1000) {
        if let Ok(Read::Data(n)) = pty::read_timeout(m.as_fd(), &mut buf, Duration::from_millis(50)) {
            if acc == 0 {
                first_burst = n;
            }
            acc += n;
            tail.extend_from_slice(&buf[..n]);
            while let Some(i) = tail.iter().position(|&c| c == b'\n') {
                let line: Vec<u8> = tail.drain(..=i).collect();
                if let Some(v) = String::from_utf8_lossy(&line).trim().strip_prefix('L').and_then(|v| v.parse::<u64>().ok()) {
                    if nums.0.is_none() {
                        nums.0 = Some(v);
                    } else if v != nums.1 + 1 {
                        nums.2 += 1;
                    }
                    nums.1 = v;
                }
            }
        }
    }
    let _ = store.drop_label("flood/0");
    drop(m);
    let gone = host::wait_gone(pid, Duration::from_secs(2));
    let o = jo! {"writes_done_before_attach": progress_before, "first_read_bytes": first_burst, "bytes_read_1s": acc, "first_line": nums.0, "last_line": nums.1, "line_gaps": nums.2, "writer_gone_after_kill_buffer": gone};
    std::fs::write(ctx.dir.join("flood-attach.json"), o).map_err(e)?;
    Ok(())
}

/// Dump the store's LIST.
pub fn list(ctx: &Ctx, a: &Args) -> R<()> {
    let mut store = Store::connect(&ctx.dir, "host").map_err(e)?;
    let items: Vec<Raw> = store.list().map_err(e)?.iter().map(|kv| Raw(crate::json::obj(&kv.iter().map(|(k, v)| (k.as_str(), v.j())).collect::<Vec<_>>()))).collect();
    std::fs::write(ctx.dir.join(format!("list-{}.json", a.get("--tag").unwrap_or("x"))), items.j()).map_err(e)?;
    Ok(())
}
