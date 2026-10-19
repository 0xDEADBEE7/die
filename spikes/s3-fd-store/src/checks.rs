//! Check drivers: each starts, kills and restarts host processes, then aggregates.
//! `die check <name> --base DIR` writes DIR/<name>/summary.json and prints it.

use crate::host::{self, store_pid};
use crate::jo;
use crate::json::{J, Raw};
use crate::modes::{Args, R, e};
use crate::pty::{self, Read, Size, Spawn};
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The raw JSON text of `"key":value` in one of our own JSON lines (no nested objects).
pub fn jget<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let i = s.find(&pat)? + pat.len();
    let rest = &s[i..];
    let end = match rest.as_bytes().first()? {
        b'[' => rest.find(']')? + 1,
        b'"' => rest[1..].find('"')? + 2,
        _ => rest.find([',', '}']).unwrap_or(rest.len()),
    };
    Some(&rest[..end])
}

fn jlist(s: &str, key: &str) -> Vec<String> {
    jget(s, key).map(|v| v.trim_matches(['[', ']']).split(',').filter(|x| !x.is_empty()).map(String::from).collect()).unwrap_or_default()
}

fn jnum(s: &str, key: &str) -> Option<f64> {
    jget(s, key)?.trim_matches('"').parse().ok()
}

fn stats(mut v: Vec<f64>) -> String {
    if v.is_empty() {
        return "null".into();
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    jo! {"n": v.len(), "min": v[0], "p50": v[v.len() / 2], "max": v[v.len() - 1]}
}

fn fresh(base: &Path, name: &str) -> PathBuf {
    let d = base.join(name);
    if d.exists() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::env::set_current_dir(&d).unwrap();
    d
}

fn host(exe: &Path, dir: &Path, run: &str, mode: &str, extra: &[&str]) -> Child {
    let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{mode}.log"))).unwrap();
    Command::new(exe)
        .args(["host", mode, "--dir"])
        .arg(dir)
        .args(["--run", run])
        .args(extra)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap()
}

fn host_wait(exe: &Path, dir: &Path, run: &str, mode: &str, extra: &[&str]) -> Option<i32> {
    host(exe, dir, run, mode, extra).wait().ok().and_then(|s| s.code())
}

fn wait_file(p: &Path, d: Duration) -> Option<String> {
    let t = Instant::now();
    while t.elapsed() < d {
        if let Ok(s) = std::fs::read_to_string(p) {
            if !s.is_empty() {
                return Some(s);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn kill9(pid: i32) {
    unsafe { libc::kill(pid, libc::SIGKILL) };
}

fn ps_field(pid: i32, field: &str) -> String {
    let o = Command::new("/bin/ps").args(["-o", &format!("{field}="), "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Kill-buffer everything left, wait for the store's idle exit, and list leftovers.
fn cleanup(exe: &Path, dir: &Path, run: &str) -> String {
    let spid = store_pid(dir);
    host_wait(exe, dir, run, "kill-all", &[]);
    let ka = read(&dir.join("kill-all.json"));
    let exited = host::wait_gone(spid, Duration::from_secs(8));
    if !exited {
        unsafe { libc::kill(spid, libc::SIGTERM) };
    }
    std::thread::sleep(Duration::from_millis(200));
    let left = host::marked_procs(run);
    for l in &left {
        if let Some(p) = l.split_whitespace().next().and_then(|p| p.parse::<i32>().ok()) {
            kill9(p);
        }
    }
    jo! {"kill_all": Raw(ka), "store_pid": spid, "store_idle_exit": exited, "socket_removed": !dir.join("store.sock").exists(), "leftovers_killed": left}
}

fn all_true(lines: &[String], key: &str) -> bool {
    lines.iter().all(|l| jlist(l, key).iter().all(|v| v == "true"))
}

fn sum(lines: &[String], key: &str) -> f64 {
    lines.iter().map(|l| jlist(l, key).iter().filter_map(|v| v.parse::<f64>().ok()).sum::<f64>()).sum()
}

fn flat(lines: &[String], key: &str) -> Vec<f64> {
    lines.iter().flat_map(|l| jlist(l, key)).filter_map(|v| v.parse().ok()).collect()
}

fn distinct(lines: &[String], key: &str) -> Vec<String> {
    let mut v: Vec<String> = lines.iter().filter_map(|l| jget(l, key).map(String::from)).collect();
    v.sort();
    v.dedup();
    v
}

pub fn restart(base: &Path, exe: &Path) -> R<String> {
    let dir = fresh(base, "restart");
    let run = "rst";
    host_wait(exe, &dir, run, "chain", &["--gen", "1", "--total", "21", "--shells", "3"]);
    let done = wait_file(&dir.join("chain.done"), Duration::from_secs(180)).is_some();
    let lines: Vec<String> = read(&dir.join("chain.jsonl")).lines().map(String::from).collect();
    let pids: Vec<String> = distinct(&lines, "shell_pids");
    let pings = flat(&lines, "ping_ms");
    let ok_pings = lines.iter().flat_map(|l| jlist(l, "ping_ms")).filter(|v| v != "null").count();
    let gaps_ms: Vec<f64> = lines.iter().filter_map(|l| jnum(l, "gap_ms")).filter(|v| *v >= 0.0).collect();
    let hup: f64 = lines.iter().filter_map(|l| jnum(l, "hup_lines")).sum();
    let mut agg = jo! {
        "hosts": lines.len(), "restarts": lines.len().saturating_sub(1), "chain_done": done,
        "shell_pid_sets": pids.clone(), "pids_match_all": lines.iter().all(|l| jget(l, "pids_match") == Some("true")),
        "shells_alive_all": all_true(&lines, "shells_alive"), "jobs_alive_all": all_true(&lines, "jobs_alive"),
        "hup_lines_total": hup, "seq_gaps_total": sum(&lines, "gaps"), "seq_missing_total": sum(&lines, "missing"),
        "pings_ok": ok_pings, "ping_ms": Raw(stats(pings)), "gap_ms": Raw(stats(gaps_ms)),
        "buffered_bytes": Raw(stats(flat(&lines, "buffered_bytes"))),
        "buffered_bytes_after_3s_gaps": lines.iter().filter(|l| jget(l, "startup_ms") == Some("3000")).flat_map(|l| jlist(l, "buffered_bytes")).collect::<Vec<_>>(),
        "attach_ms": Raw(stats(lines.iter().filter_map(|l| jnum(l, "attach_ms")).collect())),
        "host_fds": distinct(&lines, "host_fds"), "store_fds": distinct(&lines, "store_fds"), "store_pids": distinct(&lines, "store_pid"),
    };
    let pass = done && lines.len() == 21 && pids.len() == 1 && hup == 0.0 && sum(&lines, "missing") == 0.0 && ok_pings == 63 && agg.contains("\"shells_alive_all\":true") && agg.contains("\"jobs_alive_all\":true");
    agg = format!("{},\"cleanup\":{}}}", &agg[..agg.len() - 1], cleanup(exe, &dir, run));
    Ok(jo! {"check": "restart", "pass": pass, "result": Raw(agg)})
}

pub fn kill9_check(base: &Path, exe: &Path) -> R<String> {
    let dir = fresh(base, "kill9");
    let run = "k9";
    let mut h = host(exe, &dir, run, "serve", &["--gen", "0", "--shells", "2"]);
    let first = wait_file(&dir.join("serve-0.json"), Duration::from_secs(20)).ok_or("serve-0 never ready")?;
    let shells: Vec<i32> = jlist(&first, "shell_pids").iter().filter_map(|v| v.parse().ok()).collect();
    let mut after_kill = vec![];
    let mut gens = vec![];
    for k in 1..=5 {
        std::thread::sleep(Duration::from_millis(400));
        kill9(h.id() as i32);
        let _ = h.wait();
        std::thread::sleep(Duration::from_millis(200));
        let hup: usize = (0..2).map(|i| host::Ctx { dir: dir.clone(), run: run.into(), exe: exe.into() }.hup_lines(&format!("term/{i}"))).sum();
        after_kill.push(Raw(jo! {"kill": k, "shells_alive": shells.iter().map(|p| pty::alive(*p)).collect::<Vec<_>>(), "hup_lines": hup, "store_alive": pty::alive(store_pid(&dir))}));
        h = host(exe, &dir, run, "serve", &["--gen", &k.to_string()]);
        let s = wait_file(&dir.join(format!("serve-{k}.json")), Duration::from_secs(20)).ok_or(format!("serve-{k} never ready"))?;
        gens.push(s);
    }
    kill9(h.id() as i32);
    let _ = h.wait();
    let ok_pings = gens.iter().flat_map(|l| jlist(l, "ping_ms")).filter(|v| v != "null").count();
    let survived = after_kill.iter().all(|r| r.0.contains("\"shells_alive\":[true,true]") && r.0.contains("\"hup_lines\":0"));
    let pass = survived && gens.len() == 5 && ok_pings == 10 && all_true(&gens, "shells_alive") && gens.iter().all(|g| jget(g, "pids_match") == Some("true"));
    let r = jo! {
        "check": "kill9", "pass": pass, "after_each_kill": after_kill,
        "reattached": gens.len(), "pings_ok": ok_pings, "ping_ms": Raw(stats(flat(&gens, "ping_ms"))),
        "pids_match_all": gens.iter().all(|g| jget(g, "pids_match") == Some("true")), "jobs_alive_all": all_true(&gens, "jobs_alive"),
        "seq_missing_per_kill": flat(&gens, "missing"), "buffered_bytes": flat(&gens, "buffered_bytes"),
        "attach_ms": Raw(stats(gens.iter().filter_map(|g| jnum(g, "attach_ms")).collect())),
        "cleanup": Raw(cleanup(exe, &dir, run)),
    };
    Ok(r)
}

pub fn cycles(base: &Path, exe: &Path) -> R<String> {
    let dir = fresh(base, "cycles");
    let run = "cyc";
    host_wait(exe, &dir, run, "cycles", &["--n", "100"]);
    let c = read(&dir.join("cycles.json"));
    let spid = jnum(&c, "store_pid").unwrap_or(0.0) as i32;
    let store_exit = host::wait_gone(spid, Duration::from_secs(8));
    let left = host::marked_procs(run);
    let pass = jget(&c, "hup_delivered") == Some("100")
        && jget(&c, "jobs_gone") == Some("100")
        && jget(&c, "host_fds_before") == jget(&c, "host_fds_after")
        && jget(&c, "store_fds_before") == jget(&c, "store_fds_after")
        && jget(&c, "store_held_after") == Some("0")
        && jget(&c, "marked_procs_left") == Some("[]")
        && left.is_empty();
    Ok(jo! {"check": "cycles", "pass": pass, "result": Raw(c), "store_idle_exit_after": store_exit, "leftovers_after_exit": left})
}

pub fn exit_check(base: &Path, exe: &Path) -> R<String> {
    let dir = fresh(base, "exit");
    let run = "ex";
    host_wait(exe, &dir, run, "spawn-only", &["--n", "21", "--prefix", "ex"]);
    host_wait(exe, &dir, run, "exit-detect", &["--n", "21", "--prefix", "ex"]);
    let trials = read(&dir.join("exit.json"));
    // The store's periodic sweep with no host attached: kill shells, read the store log.
    host_wait(exe, &dir, run, "spawn-only", &["--n", "3", "--prefix", "sw"]);
    let spawned = read(&dir.join("spawned-sw.json"));
    let pids: Vec<i32> = spawned.match_indices("\"pid\":").filter_map(|(i, p)| spawned[i + p.len()..].split([',', '}']).next()?.parse().ok()).collect();
    let killed_ms = pty::now_ms();
    for p in &pids {
        kill9(*p);
    }
    std::thread::sleep(Duration::from_millis(2500));
    let log = read(&dir.join("store.log"));
    let sweep_ms: Vec<f64> = log
        .lines()
        .filter(|l| l.contains("sweep: sw/"))
        .filter_map(|l| l.split_whitespace().next()?.parse::<u128>().ok())
        .map(|t| t.saturating_sub(killed_ms) as f64)
        .collect();
    let split = |kind: &str, key: &str| -> Vec<f64> {
        trials.split("{\"trial\"").filter(|t| t.contains(&format!("\"kind\":\"{kind}\"")) && t.contains("\"read_delay_ms\":0")).filter_map(|t| jnum(t, key)).collect()
    };
    let note_fast: Vec<f64> = ["sigkill", "exit7", "bye-exit3"].iter().flat_map(|k| split(k, "note_exit_ms")).collect();
    let eio_fast: Vec<f64> = ["sigkill", "exit7", "bye-exit3"].iter().flat_map(|k| split(k, "eio_ms")).collect();
    let pass = note_fast.len() == 15 && eio_fast.len() == 15 && note_fast.iter().chain(&eio_fast).all(|v| *v <= 100.0);
    let r = jo! {
        "check": "exit", "pass": pass, "note_exit_ms": Raw(stats(note_fast)), "eio_ms": Raw(stats(eio_fast)),
        "trials": Raw(trials), "store_sweep_after_kill_ms": sweep_ms, "store_sweep_interval_s": 1,
        "cleanup": Raw(cleanup(exe, &dir, run)),
    };
    Ok(r)
}

pub fn versions(base: &Path, exe: &Path, n1: &Path, m2: &Path) -> R<String> {
    let run = "ver";
    let dir = fresh(base, "versions");
    let step = |bin: &Path, tag: &str, extra: &[&str]| -> String {
        let mut a = vec!["--tag", tag];
        a.extend_from_slice(extra);
        let code = host_wait(bin, &dir, run, "ver", &a);
        format!("{{\"exit_code\":{},\"out\":{}}}", code.unwrap_or(-1), Raw(read(&dir.join(format!("ver-{tag}.json")))).j())
    };
    let s1 = step(exe, "n-a", &["--spawn", "--add", "term/a"]);
    let s2 = step(exe, "n-b", &["--add", "term/b"]);
    let store_n = store_pid(&dir);
    let s3 = step(n1, "n1", &["--add", "term/c"]);
    let s4 = step(exe, "n-after-n1", &[]);
    let s4b = step(n1, "n1-reads-back", &[]);
    let s5 = step(m2, "m2", &[]);
    let s6 = step(exe, "n-after-m2", &[]);
    let store_alive_after_m2 = pty::alive(store_n);
    // Store upgrade: build N+1's store takes every fd from build N's store.
    let to = Command::new(n1).args(["--fd-store", "--dir"]).arg(&dir).args(["--daemon", "--takeover", "--idle-exit", "3", "--sweep", "1"]).status().map(|s| s.code());
    std::thread::sleep(Duration::from_millis(100));
    let s7 = step(exe, "n-after-takeover", &[]);
    let old_gone = !pty::alive(store_n);
    let hup: usize = ["term/a", "term/b", "term/c"].iter().map(|l| host::Ctx { dir: dir.clone(), run: run.into(), exe: exe.into() }.hup_lines(l)).sum();
    let c1 = cleanup(exe, &dir, run);
    // Reverse direction: an M2 store refuses a v1 host; an N+1 store serves an N host.
    let dir2 = fresh(base, "versions-m2store");
    let _ = Command::new(m2).args(["--fd-store", "--dir"]).arg(&dir2).args(["--daemon", "--idle-exit", "3"]).status();
    let r1 = {
        let code = host_wait(exe, &dir2, run, "ver", &["--tag", "n-vs-m2", "--spawn"]);
        format!("{{\"exit_code\":{},\"out\":{}}}", code.unwrap_or(-1), Raw(read(&dir2.join("ver-n-vs-m2.json"))).j())
    };
    let m2pid = store_pid(&dir2);
    let m2_idle_exit = host::wait_gone(m2pid, Duration::from_secs(6));
    let dir3 = fresh(base, "versions-n1store");
    let r2a = {
        let code = host_wait(n1, &dir3, run, "ver", &["--tag", "n1-spawn", "--spawn", "--add", "term/x"]);
        format!("{{\"exit_code\":{},\"out\":{}}}", code.unwrap_or(-1), Raw(read(&dir3.join("ver-n1-spawn.json"))).j())
    };
    let r2b = {
        let code = host_wait(exe, &dir3, run, "ver", &["--tag", "n-vs-n1store"]);
        format!("{{\"exit_code\":{},\"out\":{}}}", code.unwrap_or(-1), Raw(read(&dir3.join("ver-n-vs-n1store.json"))).j())
    };
    let c3 = cleanup(exe, &dir3, run);
    // A future major bump: an M2 store empties an N store through the frozen verbs.
    let dir4 = fresh(base, "versions-major-takeover");
    let n_put = {
        let code = host_wait(exe, &dir4, run, "ver", &["--tag", "n-put", "--spawn", "--add", "term/m"]);
        format!("{{\"exit_code\":{},\"out\":{}}}", code.unwrap_or(-1), Raw(read(&dir4.join("ver-n-put.json"))).j())
    };
    let n_store = store_pid(&dir4);
    let to_m2 = Command::new(m2).args(["--fd-store", "--dir"]).arg(&dir4).args(["--daemon", "--takeover", "--idle-exit", "3", "--sweep", "1"]).status().map(|s| s.code());
    std::thread::sleep(Duration::from_millis(100));
    let m2_after = {
        let code = host_wait(m2, &dir4, run, "ver", &["--tag", "m2-after-takeover"]);
        format!("{{\"exit_code\":{},\"out\":{}}}", code.unwrap_or(-1), Raw(read(&dir4.join("ver-m2-after-takeover.json"))).j())
    };
    let n_store_gone = !pty::alive(n_store);
    let hup_major = host::Ctx { dir: dir4.clone(), run: run.into(), exe: exe.into() }.hup_lines("term/m");
    let c4 = cleanup(m2, &dir4, run);
    let pass = s3.contains("\"store_build\":\"N\"")
        && s3.contains("\"parsed_cwd\":\"\"")
        && s4.contains("cwd")
        && s4b.contains(&format!("\"parsed_cwd\":\"{}\"", dir.display()))
        && s5.contains("\"refused\":true")
        && s5.contains("incompatible-major")
        && store_alive_after_m2
        && s6.contains("\"refused\":false")
        && s7.contains("\"store_build\":\"N+1\"")
        && old_gone
        && hup == 0
        && r1.contains("\"refused\":true")
        && r2b.contains("\"refused\":false")
        && m2_after.contains("\"store_build\":\"M2\"")
        && m2_after.contains("\"alive\":true")
        && n_store_gone
        && hup_major == 0;
    Ok(jo! {
        "check": "versions", "pass": pass,
        "n_spawns_store": Raw(s1), "n_second": Raw(s2), "n1_on_n_store": Raw(s3), "n_reads_n1_entry": Raw(s4),
        "n1_reads_own_field_back_through_n_store": Raw(s4b), "m2_on_n_store": Raw(s5), "n_after_m2": Raw(s6), "store_alive_after_m2": store_alive_after_m2,
        "takeover_exit": format!("{to:?}"), "n_after_takeover": Raw(s7), "old_store_gone": old_gone, "hup_lines": hup,
        "cleanup": Raw(c1), "n_host_vs_m2_store": Raw(r1), "m2_store_idle_exit": m2_idle_exit,
        "n1_store_spawned": Raw(r2a), "n_host_vs_n1_store": Raw(r2b), "cleanup_n1store": Raw(c3),
        "major_n_put": Raw(n_put), "major_takeover_exit": format!("{to_m2:?}"), "major_m2_host_after": Raw(m2_after),
        "major_n_store_gone": n_store_gone, "major_hup_lines": hup_major, "major_cleanup": Raw(c4),
    })
}

pub fn lifetime(base: &Path, exe: &Path) -> R<String> {
    let run = "life";
    let dir = fresh(base, "lifetime");
    let start = |d: &Path, extra: &[&str]| -> (Option<i32>, String) {
        let o = Command::new(exe).args(["--fd-store", "--dir"]).arg(d).arg("--daemon").args(extra).output().unwrap();
        (o.status.code(), String::from_utf8_lossy(&o.stderr).trim().to_string())
    };
    let first = start(&dir, &["--idle-exit", "2", "--sweep", "1"]);
    std::thread::sleep(Duration::from_millis(100));
    let spid = store_pid(&dir);
    let second = start(&dir, &["--idle-exit", "2"]);
    let ppid = ps_field(spid, "ppid");
    host_wait(exe, &dir, run, "spawn-only", &["--n", "1", "--prefix", "life"]);
    let cpu0 = ps_field(spid, "time");
    std::thread::sleep(Duration::from_millis(3500));
    let alive_holding = pty::alive(spid);
    let cpu1 = ps_field(spid, "time");
    let rss_kb = ps_field(spid, "rss");
    let t = Instant::now();
    host_wait(exe, &dir, run, "kill-all", &[]);
    let exited = host::wait_gone(spid, Duration::from_secs(6));
    let exit_after_drop_ms = t.elapsed().as_millis();
    let sock_gone = !dir.join("store.sock").exists();
    // Crash leaves a stale socket; the next client starts a fresh store anyway.
    let _ = start(&dir, &["--idle-exit", "2"]);
    std::thread::sleep(Duration::from_millis(100));
    let crashed = store_pid(&dir);
    kill9(crashed);
    std::thread::sleep(Duration::from_millis(100));
    let stale = dir.join("store.sock").exists();
    host_wait(exe, &dir, run, "spawn-only", &["--n", "1", "--prefix", "stale"]);
    let after_stale = store_pid(&dir);
    let stale_ok = after_stale != crashed && pty::alive(after_stale);
    let c1 = cleanup(exe, &dir, run);
    // A directory that isn't private is refused.
    let open_dir = base.join("lifetime-0755");
    let _ = std::fs::remove_dir_all(&open_dir);
    std::fs::create_dir_all(&open_dir).unwrap();
    std::fs::set_permissions(&open_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let perm = start(&open_dir, &[]);
    // Peer uid check, simulated: the store is told to expect uid 0, so our uid is refused.
    let dir_uid = fresh(base, "lifetime-uid");
    let _ = start(&dir_uid, &["--expect-uid", "0", "--idle-exit", "2"]);
    std::thread::sleep(Duration::from_millis(100));
    let upid = store_pid(&dir_uid);
    let conn = crate::client::Store::connect(&dir_uid, "host").map(|_| ()).map_err(e);
    let log_has_refusal = read(&dir_uid.join("store.log")).contains("refused pid");
    unsafe { libc::kill(upid, libc::SIGTERM) };
    let pass = first.0 == Some(0) && second.0 == Some(3) && alive_holding && exited && sock_gone && stale && stale_ok && perm.0 == Some(2) && conn.is_err() && log_has_refusal;
    Ok(jo! {
        "check": "lifetime", "pass": pass, "first_start": format!("{first:?}"), "second_start_refused": format!("{second:?}"),
        "store_ppid_after_daemonize": ppid, "alive_while_holding_after_3500ms": alive_holding,
        "cpu_time_before_after": vec![cpu0, cpu1], "rss_kb": rss_kb,
        "exited_after_last_drop": exited, "exit_ms_after_kill_all_started": exit_after_drop_ms, "socket_removed": sock_gone,
        "stale_socket_left_by_kill9": stale, "new_store_after_stale": stale_ok, "cleanup": Raw(c1),
        "dir_0755": format!("{perm:?}"), "uid_mismatch_connect": format!("{conn:?}"), "uid_refusal_logged": log_has_refusal,
    })
}

pub fn store_crash(base: &Path, exe: &Path) -> R<String> {
    let run = "sc";
    let dir = fresh(base, "storecrash");
    let ctx = host::Ctx { dir: dir.clone(), run: run.into(), exe: exe.into() };
    // (a) Store killed while a host is attached: the host still holds the masters.
    let mut h = host(exe, &dir, run, "serve", &["--gen", "0", "--shells", "2", "--redeposit"]);
    let s0 = wait_file(&dir.join("serve-0.json"), Duration::from_secs(20)).ok_or("serve-0")?;
    let shells: Vec<i32> = jlist(&s0, "shell_pids").iter().filter_map(|v| v.parse().ok()).collect();
    let old = store_pid(&dir);
    kill9(old);
    let ev = wait_file(&dir.join("serve-0.events"), Duration::from_secs(5)).unwrap_or_default();
    let alive_a = shells.iter().all(|p| pty::alive(*p));
    let new = store_pid(&dir);
    std::thread::sleep(Duration::from_millis(300));
    kill9(h.id() as i32);
    let _ = h.wait();
    h = host(exe, &dir, run, "serve", &["--gen", "1"]);
    let s1 = wait_file(&dir.join("serve-1.json"), Duration::from_secs(20)).unwrap_or_default();
    kill9(h.id() as i32);
    let _ = h.wait();
    let hup_a: usize = (0..2).map(|i| ctx.hup_lines(&format!("term/{i}"))).sum();
    let c1 = cleanup(exe, &dir, run);
    // (b) Store killed while no host is attached: nothing else holds the masters.
    let dir = fresh(base, "storecrash-nohost");
    let ctx = host::Ctx { dir: dir.clone(), run: format!("{run}b"), exe: exe.into() };
    host_wait(exe, &dir, &ctx.run, "spawn-only", &["--n", "2", "--prefix", "nh", "--job", "writer"]);
    let spawned = read(&dir.join("spawned-nh.json"));
    let pids: Vec<i32> = spawned.match_indices("\"pid\":").filter_map(|(i, p)| spawned[i + p.len()..].split([',', '}']).next()?.parse().ok()).collect();
    let t = Instant::now();
    kill9(store_pid(&dir));
    let mut hup_ms = None;
    while t.elapsed() < Duration::from_secs(3) {
        if (0..2).all(|i| ctx.hup_lines(&format!("nh/{i}")) > 0) {
            hup_ms = Some(t.elapsed().as_secs_f64() * 1000.0);
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    std::thread::sleep(Duration::from_millis(300));
    let alive_b: Vec<bool> = pids.iter().map(|p| pty::alive(*p)).collect();
    let left = host::marked_procs(&ctx.run);
    for l in &left {
        if let Some(p) = l.split_whitespace().next().and_then(|p| p.parse::<i32>().ok()) {
            kill9(p);
        }
    }
    let pass = alive_a && ev.contains("store-lost") && new != old && hup_a == 0 && s1.contains("\"pids_match\":true") && jlist(&s1, "ping_ms").iter().all(|v| v != "null");
    Ok(jo! {
        "check": "store-crash", "pass_attached_case": pass,
        "a_shells_alive_after_store_kill": alive_a, "a_host_event": ev.trim().to_string(), "a_old_store": old, "a_new_store": new,
        "a_reattach_after_host_kill9": Raw(s1), "a_hup_lines": hup_a, "a_cleanup": Raw(c1),
        "b_hup_after_ms": hup_ms, "b_shells_alive_after": alive_b, "b_leftovers": left,
    })
}

/// Same-size SIGWINCH, and whether real full-screen programs redraw after the nudge.
pub fn winch(base: &Path, _exe: &Path) -> R<String> {
    let dir = fresh(base, "winch");
    let f = dir.join("winch.txt");
    let script = format!("trap 'echo W >> {}' WINCH; while :; do /bin/sleep 0.02; done", f.display());
    let env = [("PATH", "/usr/bin:/bin"), ("HOME", dir.to_str().unwrap()), ("TERM", "xterm-256color"), ("LESSHISTFILE", "-"), ("LANG", "C")];
    let mk = |argv0: &str, prog: &str, args: &[&str]| {
        let m = pty::open_master().unwrap();
        let c = pty::spawn(m.as_fd(), &Spawn { argv0, prog, args, env: &env, cwd: &dir, size: Size::DEFAULT }).unwrap();
        (m, c)
    };
    let (m, mut c) = mk("s3sh-winch", "/bin/sh", &["-c", &script]);
    std::thread::sleep(Duration::from_millis(300));
    let count = || read(&f).lines().count();
    let s0 = Size::DEFAULT;
    let mut steps = vec![];
    for _round in 0..3 {
        for (name, s) in [("same", s0), ("rows+1", Size { rows: s0.rows + 1, ..s0 }), ("rows", s0), ("xpix+1", Size { xpix: s0.xpix + 1, ..s0 }), ("xpix", s0)] {
            let b = count();
            pty::set_size(m.as_fd(), s).unwrap();
            std::thread::sleep(Duration::from_millis(150));
            steps.push(format!("{name}:{}", count() - b));
        }
    }
    drop(m);
    let _ = c.kill();
    let _ = c.wait();
    // Real programs: does each nudge make them repaint the screen?
    let file = dir.join("lines.txt");
    std::fs::write(&file, (1..=300).map(|i| format!("line-{i:03} the quick brown fox\n")).collect::<String>()).unwrap();
    let fpath = file.to_str().unwrap();
    let mut progs = vec![];
    for (name, prog, args, needle) in [
        ("less", "/usr/bin/less", vec![fpath], "line-0"),
        ("vim", "/usr/bin/vim", vec!["-u", "NONE", "-i", "NONE", "-n", "-N", "-R", "--noplugin", fpath], "line-0"),
        ("pico", "/usr/bin/pico", vec!["-v", fpath], "line-0"),
        ("top", "/usr/bin/top", vec!["-s", "30"], "PID"),
    ] {
        let (m, mut c) = mk(&format!("s3sh-winch-{name}"), prog, &args);
        let drain = |m: &std::os::fd::OwnedFd, d: Duration| -> Vec<u8> {
            let mut acc = vec![];
            let mut buf = [0u8; 65536];
            let t = Instant::now();
            while t.elapsed() < d {
                if let Ok(Read::Data(n)) = pty::read_timeout(m.as_fd(), &mut buf, Duration::from_millis(20)) {
                    acc.extend_from_slice(&buf[..n]);
                }
            }
            acc
        };
        let initial = drain(&m, Duration::from_millis(1500));
        let mut res = vec![("initial".to_string(), initial.len(), pty::contains(&initial, needle.as_bytes()))];
        let acts = ["same-size", "rows+1-0ms", "rows+1-10ms", "rows+1-20ms", "rows+1-50ms", "xpix-0ms", "rows-1-on-output"];
        for act in acts {
            let mut waited = None;
            match act {
                "same-size" => pty::set_size(m.as_fd(), s0).unwrap(),
                "xpix-0ms" => {
                    pty::set_size(m.as_fd(), Size { xpix: s0.xpix + 1, ..s0 }).unwrap();
                    pty::set_size(m.as_fd(), s0).unwrap();
                }
                "rows-1-on-output" => waited = Some(pty::redraw_nudge_on_output(m.as_fd(), Duration::from_millis(200)).unwrap().as_secs_f64() * 1000.0),
                a => {
                    let ms: u64 = a.trim_start_matches("rows+1-").trim_end_matches("ms").parse().unwrap();
                    pty::redraw_nudge(m.as_fd(), Duration::from_millis(ms)).unwrap();
                }
            }
            let out = drain(&m, Duration::from_millis(700));
            res.push((format!("{act}{}", waited.map(|w| format!(" (waited {w:.1} ms)")).unwrap_or_default()), out.len(), pty::contains(&out, needle.as_bytes())));
        }
        drop(m);
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(2) && c.try_wait().ok().flatten().is_none() {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = c.kill();
        let _ = c.wait();
        progs.push(Raw(jo! {"program": name, "steps": res.iter().map(|(a, n, l)| Raw(jo!{"action": a.clone(), "bytes": *n, "repainted_text": *l})).collect::<Vec<_>>()}));
    }
    let same_dropped = steps.iter().filter(|s| s.starts_with("same:")).all(|s| s == "same:0");
    Ok(jo! {"check": "winch", "same_size_sigwinch_dropped": same_dropped, "sh_trap_steps": steps, "programs": progs, "leftovers": host::marked_procs("winch")})
}

pub fn buffer(base: &Path, exe: &Path) -> R<String> {
    let dir = fresh(base, "buffer");
    let run = "buf";
    host_wait(exe, &dir, run, "flood-spawn", &[]);
    std::thread::sleep(Duration::from_millis(1000));
    let p1 = read(&dir.join("flood.progress")).trim().to_string();
    std::thread::sleep(Duration::from_millis(1000));
    let p2 = read(&dir.join("flood.progress")).trim().to_string();
    let pid: i32 = read(&dir.join("flood.pid")).trim().parse().unwrap_or(0);
    let state = ps_field(pid, "stat");
    host_wait(exe, &dir, run, "flood-attach", &[]);
    let a = read(&dir.join("flood-attach.json"));
    let c = cleanup(exe, &dir, run);
    Ok(jo! {"check": "buffer", "writes_done_1s": p1, "writes_done_2s": p2, "writer_state_while_detached": state, "attach": Raw(a), "line_bytes_on_master": 11, "cleanup": Raw(c)})
}

pub fn helper(base: &Path, exe: &Path) -> R<String> {
    let dir = fresh(base, "helper");
    let run = "hlp";
    host_wait(exe, &dir, run, "spawn-only", &["--n", "1", "--prefix", "term/1", "--job", "writer", "--label", "term/1/pty"]);
    let mut h = host(exe, &dir, run, "helper", &[]);
    wait_file(&dir.join("helper.ready"), Duration::from_secs(5)).ok_or("helper not ready")?;
    host_wait(exe, &dir, run, "helper-client", &["--tag", "r1"]);
    std::thread::sleep(Duration::from_millis(300));
    host_wait(exe, &dir, run, "helper-client", &["--tag", "r2"]);
    let r1 = read(&dir.join("helper-client-r1.json"));
    let r2 = read(&dir.join("helper-client-r2.json"));
    kill9(h.id() as i32);
    let _ = h.wait();
    std::thread::sleep(Duration::from_millis(1500));
    host_wait(exe, &dir, run, "list", &["--tag", "after-helper-kill"]);
    let l = read(&dir.join("list-after-helper-kill.json"));
    let _ = std::fs::remove_file(dir.join("helper.ready"));
    h = host(exe, &dir, run, "helper", &[]);
    wait_file(&dir.join("helper.ready"), Duration::from_secs(5)).ok_or("helper 2 not ready")?;
    host_wait(exe, &dir, run, "helper-client", &["--tag", "r3"]);
    let r3 = read(&dir.join("helper-client-r3.json"));
    kill9(h.id() as i32);
    let _ = h.wait();
    let c = cleanup(exe, &dir, run);
    let pass = r1.contains("PONG PING r1") && r2.contains("PONG PING r2") && l.contains("term/1/engine") && l.contains("exited") && r3.contains("PONG PING r3");
    Ok(jo! {"check": "helper", "pass": pass, "restart1": Raw(r1), "restart2": Raw(r2), "list_after_helper_kill9": Raw(l), "new_helper": Raw(r3), "cleanup": Raw(c)})
}

pub fn app(base: &Path, exe: &Path, app_dir: &Path) -> R<String> {
    // A minimal bundle around the host binary.
    let bundle = app_dir.join("S3Host.app");
    let _ = std::fs::remove_dir_all(&bundle);
    std::fs::create_dir_all(bundle.join("Contents/MacOS")).map_err(e)?;
    std::fs::copy(exe, bundle.join("Contents/MacOS/die")).map_err(e)?;
    std::fs::write(
        bundle.join("Contents/Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>die</string>
<key>CFBundleIdentifier</key><string>local.die.s3-spike</string>
<key>CFBundleName</key><string>S3Host</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSUIElement</key><true/>
</dict></plist>
"#,
    )
    .map_err(e)?;
    let dir = fresh(base, "app");
    let run = "app";
    let open = |phase: &str| {
        Command::new("/usr/bin/open").args(["-n", "-g"]).arg(&bundle).args(["--args", "host", "app", "--dir"]).arg(&dir).args(["--run", run, "--phase", phase]).status().map(|s| s.code())
    };
    let o1 = open("spawn");
    let sp = wait_file(&dir.join("app-spawn.json"), Duration::from_secs(20)).ok_or("app spawn phase never wrote its file")?;
    let app_pid = jnum(&sp, "pid").unwrap_or(0.0) as i32;
    let shells: Vec<i32> = jlist(&sp, "shell_pids").iter().filter_map(|v| v.parse().ok()).collect();
    let spid = store_pid(&dir);
    let store_ppid = ps_field(spid, "ppid");
    let app_comm = ps_field(app_pid, "comm");
    std::thread::sleep(Duration::from_millis(500));
    kill9(app_pid);
    std::thread::sleep(Duration::from_millis(500));
    let ctx = host::Ctx { dir: dir.clone(), run: run.into(), exe: exe.into() };
    let hup = |c: &host::Ctx| (0..2).map(|i| c.hup_lines(&format!("term/{i}"))).sum::<usize>();
    let after_kill = jo! {"app_gone": !pty::alive(app_pid), "store_alive": pty::alive(spid), "shells_alive": shells.iter().map(|p| pty::alive(*p)).collect::<Vec<_>>(), "hup_lines": hup(&ctx)};
    let o2 = open("attach");
    let at = wait_file(&dir.join("app-attach.json"), Duration::from_secs(20)).unwrap_or_default();
    let at_pid = jnum(&at, "pid").unwrap_or(0.0) as i32;
    let exited_normally = host::wait_gone(at_pid, Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(1000));
    let after_exit = jo! {"store_alive": pty::alive(spid), "shells_alive": shells.iter().map(|p| pty::alive(*p)).collect::<Vec<_>>(), "hup_lines": hup(&ctx)};
    let o3 = open("attach2");
    let at2 = wait_file(&dir.join("app-attach2.json"), Duration::from_secs(20)).unwrap_or_default();
    let c = cleanup(exe, &dir, run);
    let pass = after_kill.contains("\"shells_alive\":[true,true]") && after_kill.contains("\"hup_lines\":0") && after_kill.contains("\"store_alive\":true")
        && jlist(&at, "ping_ms").len() == 2 && jlist(&at, "ping_ms").iter().all(|v| v != "null")
        && after_exit.contains("\"shells_alive\":[true,true]") && jlist(&at2, "ping_ms").iter().all(|v| v != "null") && !at2.is_empty();
    Ok(jo! {
        "check": "app", "pass": pass, "bundle": bundle.display().to_string(), "open_exit": format!("{:?}", (o1, o2, o3)),
        "spawn": Raw(sp), "app_comm": app_comm, "store_pid": spid, "store_ppid": store_ppid,
        "after_app_kill9": Raw(after_kill), "relaunch_attach": Raw(at), "attach_app_exited_normally": exited_normally,
        "after_normal_app_exit": Raw(after_exit), "third_launch": Raw(at2), "cleanup": Raw(c),
    })
}

pub fn run(name: &str, a: &Args) -> R<String> {
    let base = PathBuf::from(a.get("--base").ok_or("--base")?);
    let exe = std::env::current_exe().map_err(e)?;
    let one = |n: &str| -> R<String> {
        let t = Instant::now();
        let r = match n {
            "restart" => restart(&base, &exe),
            "kill9" => kill9_check(&base, &exe),
            "cycles" => cycles(&base, &exe),
            "exit" => exit_check(&base, &exe),
            "versions" => versions(&base, &exe, Path::new(a.get("--n1").ok_or("--n1")?), Path::new(a.get("--m2").ok_or("--m2")?)),
            "lifetime" => lifetime(&base, &exe),
            "store-crash" => store_crash(&base, &exe),
            "winch" => winch(&base, &exe),
            "buffer" => buffer(&base, &exe),
            "helper" => helper(&base, &exe),
            "app" => app(&base, &exe, Path::new(a.get("--app-dir").ok_or("--app-dir")?)),
            _ => Err(format!("unknown check {n}")),
        };
        let s = match r {
            Ok(s) => format!("{},\"secs\":{:.1}}}", &s[..s.len() - 1], t.elapsed().as_secs_f64()),
            Err(x) => jo! {"check": n, "error": x},
        };
        std::fs::write(base.join(format!("summary-{n}.json")), &s).map_err(e)?;
        Ok(s)
    };
    if name == "all" {
        let names = ["restart", "kill9", "cycles", "exit", "versions", "lifetime", "store-crash", "winch", "buffer", "helper", "app"];
        let all: Vec<Raw> = names.iter().map(|n| Raw(one(n).unwrap_or_else(|x| jo! {"check": *n, "error": x}))).collect();
        let s = all.j();
        std::fs::write(base.join("summary-all.json"), &s).map_err(e)?;
        Ok(s)
    } else {
        one(name)
    }
}
