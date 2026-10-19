//! Helper-shape demonstrations: helper crash, DIE crash + reattach, version handshake, and
//! the PTY-holder layout (a holder's copy of the master keeps the shell alive).
use crate::bench::{Loop, exe};
use crate::client::TermClient;
use crate::engine::Cfg;
use crate::helper::HelperClient;
use crate::proto::ToEngine;
use crate::surface::Applied;
use crate::util::{fnv64, now_ns};

const CFG: Cfg = Cfg { cols: 120, rows: 40, cell_w: 18, cell_h: 36 };

fn bash() -> Vec<String> {
    ["/bin/bash", "--noprofile", "--norc", "-i"].iter().map(|s| s.to_string()).collect()
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks existence.
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}

fn say(s: &str) {
    println!("{s}");
}

/// Wait for `needle` on terminal i's screen.
fn wait_text(l: &mut Loop, i: usize, needle: &str, secs: u64) -> bool {
    l.until(secs * 1_000_000_000, |l, _| l.screen_has(i, needle))
}

fn echo_check(l: &mut Loop, i: usize, tag: &str) -> bool {
    let n = (now_ns() % 100_000) as u32;
    l.type_text(i, &format!("echo {tag}-$(( {n} + 1 ))\n"));
    wait_text(l, i, &format!("{tag}-{}", n + 1), 5)
}

/// (i) kill -9 of one helper: DIE keeps running, reports the lost terminal, the other
/// terminal keeps working. Plus the holder layout: a third terminal whose PTY master DIE
/// (standing in for the holder) also holds; its shell survives the helper's death and a new
/// helper adopts the same PTY.
pub fn kill_helper(run_dir: &str) -> bool {
    let mut ok = true;
    let t1 = HelperClient::spawn("k1", &bash(), CFG, false, &format!("{run_dir}/k1.log")).unwrap();
    let t2 = HelperClient::spawn("k2", &bash(), CFG, false, &format!("{run_dir}/k2.log")).unwrap();
    // Holder layout: DIE creates the PTY, keeps the master (as die --fd-store would), and
    // hands a copy to the helper.
    let pty = crate::pty::spawn(&bash(), CFG.cols, CFG.rows, CFG.cell_w, CFG.cell_h, &[]).unwrap();
    let t3 = HelperClient::spawn_with("k3", &[], CFG, false, &format!("{run_dir}/k3.log"), Some((pty.master, pty.child))).unwrap();
    let mut l = Loop::new(vec![Box::new(t1), Box::new(t2), Box::new(t3)]);
    l.settle(800_000_000, 10_000_000_000);
    for i in 0..3 {
        let e = echo_check(&mut l, i, &format!("t{}-before", i + 1));
        say(&format!("terminal t{} echo before kill: {}", i + 1, if e { "ok" } else { "FAILED" }));
        ok &= e;
    }
    let h1 = l.t(0).client.engine_pid();
    let sh1 = l.t(0).client.child_pid();
    let h3 = l.t(2).client.engine_pid();
    say(&format!("t1 helper pid {h1}, shell pid {sh1}; t3 helper pid {h3}, shell pid {} (master also held by DIE)", pty.child));
    let t_kill = now_ns();
    // SAFETY: killing our own helper processes.
    unsafe {
        libc::kill(h1, libc::SIGKILL);
        libc::kill(h3, libc::SIGKILL);
    }
    let lost = l.until(5_000_000_000, |l, _| l.terms[0].lost.is_some() && l.terms[2].lost.is_some());
    let dt = (now_ns() - t_kill) as f64 / 1e6;
    for i in [0usize, 2] {
        let pid = l.t(i).client.engine_pid();
        let mut st = 0;
        // SAFETY: reaping our own child helper.
        let r = unsafe { libc::waitpid(pid, &mut st, 0) };
        let sig = if r == pid && libc::WIFSIGNALED(st) { libc::WTERMSIG(st) } else { -1 };
        say(&format!(
            "DIE reports: terminal t{} lost ({}); helper {pid} ended by signal {sig}; noticed after {dt:.1} ms",
            i + 1,
            l.t(i).lost.clone().unwrap_or_default()
        ));
    }
    ok &= lost;
    std::thread::sleep(std::time::Duration::from_millis(300));
    let sh1_alive = alive(sh1);
    say(&format!("t1 shell {sh1} after its helper died: {}", if sh1_alive { "alive (unexpected)" } else { "gone (SIGHUP: the helper held the only master)" }));
    let e2 = echo_check(&mut l, 1, "t2-after");
    say(&format!("terminal t2 after t1's helper was killed: {}", if e2 { "still works" } else { "FAILED" }));
    ok &= e2 && !sh1_alive;
    let sh3_alive = alive(pty.child);
    say(&format!("t3 shell {} after its helper died: {}", pty.child, if sh3_alive { "alive (the holder's master kept the PTY open)" } else { "gone" }));
    ok &= sh3_alive;
    // A new helper adopts the held PTY; scrollback is lost, the shell and its state are not.
    let t3b = HelperClient::spawn_with("k3b", &[], CFG, false, &format!("{run_dir}/k3b.log"), Some((pty.master, pty.child))).unwrap();
    l.terms.push(crate::bench::Term { client: Box::new(t3b), surf: Default::default(), lost: None });
    l.settle(300_000_000, 5_000_000_000);
    let e3 = echo_check(&mut l, 3, "t3-readopted");
    say(&format!("t3 re-adopted by a new helper: {}", if e3 { "works (screen restarts empty; the shell survived)" } else { "FAILED" }));
    ok &= e3;
    for i in [1usize, 3] {
        l.t(i).client.send(ToEngine::Shutdown);
    }
    l.settle(200_000_000, 2_000_000_000);
    for i in [1usize, 3] {
        let pid = l.t(i).client.engine_pid();
        let mut st = 0;
        unsafe { libc::waitpid(pid, &mut st, 0) };
    }
    crate::util::close(pty.master);
    say(&format!("RESULT kill-helper: {}", if ok { "PASS" } else { "FAIL" }));
    ok
}

/// (ii) phase 1: start a helper terminal, fill scrollback and show an image, print what DIE
/// saw, then wait to be killed with SIGKILL.
pub fn session_phase1(run_dir: &str) {
    let c = HelperClient::spawn("rt", &bash(), CFG, false, &format!("{run_dir}/rt.log")).unwrap();
    let mut l = Loop::new(vec![Box::new(c)]);
    l.settle(800_000_000, 10_000_000_000);
    let e = exe();
    l.type_text(0, &format!("{e} gen lines 3000 early; {e} gen img-small 7; (sleep 3; echo AFTER-DIE-DIED; {e} gen lines 200 late) &\n"));
    let got_img = l.until(20_000_000_000, |l, _| l.terms[0].surf.images.contains_key(&7));
    l.settle(500_000_000, 10_000_000_000);
    let (lines, text, _) = l.scrollback(0).unwrap();
    let img = l.t(0).surf.images.get(&7).map(|i| fnv64(i.pixels.bytes())).unwrap_or(0);
    println!(
        "{{\"phase\":1,\"die_pid\":{},\"helper_pid\":{},\"shell_pid\":{},\"image\":{got_img},\"image_fnv\":{img},\"expected_image_fnv\":{},\"scrollback_lines\":{lines},\"scrollback_has_early_00000\":{},\"placements\":{}}}",
        crate::util::pid(),
        l.t(0).client.engine_pid(),
        l.t(0).client.child_pid(),
        fnv64(&crate::workgen::img_small_pixels()),
        String::from_utf8_lossy(&text).contains("early line 00000"),
        l.t(0).surf.placements.len()
    );
    use std::io::Write;
    std::io::stdout().flush().unwrap();
    loop {
        l.step(now_ns() + 1_000_000_000);
    }
}

/// (ii) phase 2: a new DIE process attaches by socket path and checks that the screen,
/// scrollback (including output produced while no DIE existed) and the image are intact.
pub fn session_phase2(run_dir: &str, shutdown: bool) -> bool {
    let _ = run_dir;
    let t0 = now_ns();
    let c = match HelperClient::attach("rt.sock") {
        Ok(c) => c,
        Err(e) => {
            println!("{{\"phase\":2,\"error\":\"attach: {e}\"}}");
            return false;
        }
    };
    let mut l = Loop::new(vec![Box::new(c)]);
    let mut first_frame = 0u64;
    let mut img_t = 0u64;
    l.until(10_000_000_000, |l, ev| {
        for (_, a, t) in ev {
            match a {
                Applied::Frame { full: true, .. } if first_frame == 0 => first_frame = *t,
                Applied::Image { id: 7, .. } => img_t = *t,
                _ => {}
            }
        }
        first_frame > 0 && img_t > 0 && !l.terms[0].surf.placements.is_empty()
    });
    let attach_ms = (first_frame.max(img_t) - t0) as f64 / 1e6;
    let (lines, text, fetch) = l.scrollback(0).unwrap_or_default();
    let text = String::from_utf8_lossy(&text).into_owned();
    let img = l.t(0).surf.images.get(&7).map(|i| fnv64(i.pixels.bytes())).unwrap_or(0);
    let want = fnv64(&crate::workgen::img_small_pixels());
    let alive = echo_check(&mut l, 0, "still-alive");
    let r = l.t(0).client.engine_pid();
    // The helper copied the image into shm once (phase 1); the reattach resent the same fd.
    let copies = l.stats(0).map(|s| s.images_copied).unwrap_or(99);
    let ok = copies == 1 && img == want && text.contains("early line 00000") && text.contains("AFTER-DIE-DIED") && text.contains("late line 00199") && alive;
    println!(
        "{{\"phase\":2,\"die_pid\":{},\"helper_pid\":{r},\"attach_to_full_state_ms\":{attach_ms:.2},\"scrollback_lines\":{lines},\"scrollback_fetch_ms\":{:.2},\"has_early_00000\":{},\"has_output_made_while_die_was_dead\":{},\"has_late_00199\":{},\"image_fnv\":{img},\"image_matches\":{},\"placements\":{},\"shell_answers\":{alive},\"helper_image_copies_total\":{copies},\"result\":\"{}\"}}",
        crate::util::pid(),
        fetch as f64 / 1e6,
        text.contains("early line 00000"),
        text.contains("AFTER-DIE-DIED"),
        text.contains("late line 00199"),
        img == want,
        l.t(0).surf.placements.len(),
        if ok { "PASS" } else { "FAIL" }
    );
    if shutdown {
        l.t(0).client.send(ToEngine::Shutdown);
        l.settle(200_000_000, 2_000_000_000);
    }
    ok
}

fn set_ver(major: u16, minor: u16) {
    // SAFETY: single-threaded at this point in the demo; only our own variables.
    unsafe {
        std::env::set_var("S2_PROTO_MAJOR", major.to_string());
        std::env::set_var("S2_PROTO_MINOR", minor.to_string());
    }
}

/// (iii) version handshake: same major attaches at the lower minor; another major is refused
/// cleanly on both sides and the helper and its shell keep running.
pub fn version(run_dir: &str) -> bool {
    let mut ok = true;
    set_ver(1, 1);
    let a = HelperClient::spawn("vt", &bash(), CFG, false, &format!("{run_dir}/vt.log")).unwrap();
    let helper = a.spawned_pid.unwrap();
    let mut l = Loop::new(vec![Box::new(a)]);
    l.settle(800_000_000, 10_000_000_000);
    let e = echo_check(&mut l, 0, "v11");
    say(&format!("helper built as 1.1, DIE 1.1: attached, echo {}", if e { "ok" } else { "FAILED" }));
    ok &= e;
    drop(l);
    for (maj, min, expect_ok, expect_minor) in [(1u16, 2u16, true, 1u16), (2, 0, false, 0), (0, 9, false, 0), (1, 0, true, 0)] {
        set_ver(maj, min);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let c = HelperClient::attach("vt.sock").unwrap();
        let mut l = Loop::new(vec![Box::new(c)]);
        l.until(3_000_000_000, |l, _| l.terms[0].lost.is_some() || !l.terms[0].surf.grid.is_empty());
        let lost = l.t(0).lost.clone();
        let reply = {
            // The concrete client is behind the trait; ask through the pid/minor surface.
            l.t(0).client.engine_pid()
        };
        let line = match &lost {
            Some(reason) => format!("DIE {maj}.{min}: refused cleanly: \"{reason}\""),
            None => {
                let e = echo_check(&mut l, 0, &format!("v{maj}{min}"));
                format!("DIE {maj}.{min}: attached (helper pid {reply}), echo {}", if e { "ok" } else { "FAILED" })
            }
        };
        let good = lost.is_none() == expect_ok;
        say(&format!("{line}{}", if good { "" } else { "  <-- UNEXPECTED" }));
        ok &= good;
        let _ = expect_minor;
        drop(l);
        let h_alive = alive(helper);
        say(&format!("  helper {helper} still running: {h_alive}"));
        ok &= h_alive;
    }
    // Negotiated minor is visible in the helper log; check it there.
    set_ver(1, 1);
    let c = HelperClient::attach("vt.sock").unwrap();
    let mut l = Loop::new(vec![Box::new(c)]);
    l.settle(300_000_000, 3_000_000_000);
    l.t(0).client.send(ToEngine::Shutdown);
    l.settle(200_000_000, 2_000_000_000);
    let mut st = 0;
    unsafe { libc::waitpid(helper, &mut st, 0) };
    let log = std::fs::read_to_string(format!("{run_dir}/vt.log")).unwrap_or_default();
    for line in log.lines() {
        say(&format!("  helper log: {line}"));
    }
    ok &= log.contains("speaks 1.2; helper is") && log.contains("using 1.1") && log.contains("using 1.0") && log.contains("refused DIE");
    say(&format!("RESULT version: {}", if ok { "PASS" } else { "FAIL" }));
    ok
}

pub fn _unused(_c: &dyn TermClient) {}
