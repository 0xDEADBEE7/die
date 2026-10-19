//! DIE's "main thread" for the measurements: an event loop over one or more terminals that
//! never blocks on them, ingests messages into each Surface, and returns frame credits at a
//! simulated 120 Hz vsync. Workloads w1–w5 and the RSS probe drive it.
use crate::client::TermClient;
use crate::engine::Cfg;
use crate::helper::HelperClient;
use crate::inproc::InProc;
use crate::proto::{Effect, ToEngine};
use crate::surface::{Applied, Surface};
use crate::util::{Summary, now_ns, pfd, poll, summarize_ns, usage};
use std::fmt::Write as _;

pub const VSYNC_NS: u64 = 8_333_333;

pub struct Term {
    pub client: Box<dyn TermClient>,
    pub surf: Surface,
    pub lost: Option<String>,
}

#[derive(Default)]
pub struct Rec {
    /// Main-thread ingest time per vsync interval that had any work.
    pub vsync_ingest: Vec<u64>,
    /// Main-thread ingest time per wakeup with work.
    pub wake_ingest: Vec<u64>,
    /// Wakeups caused by terminal fds (not timers).
    pub fd_wakeups: u64,
    pub timer_wakeups: u64,
    pub msgs: u64,
}

pub struct Loop {
    pub terms: Vec<Term>,
    pub rec: Rec,
    next_vsync: Option<u64>,
    acc: u64,
    epoch: u64,
}

pub fn make(shape: &str, name: &str, argv: &[String], cfg: Cfg, run_dir: &str) -> Box<dyn TermClient> {
    match shape {
        "inproc" => Box::new(InProc::spawn(argv, cfg, &[]).expect("spawn inproc")),
        "helper" => Box::new(
            HelperClient::spawn(name, argv, cfg, true, &format!("{run_dir}/{name}.log")).expect("spawn helper"),
        ),
        s => panic!("unknown shape {s}"),
    }
}

impl Loop {
    pub fn new(clients: Vec<Box<dyn TermClient>>) -> Loop {
        Loop {
            terms: clients.into_iter().map(|client| Term { client, surf: Surface::default(), lost: None }).collect(),
            rec: Rec::default(),
            next_vsync: None,
            acc: 0,
            epoch: now_ns(),
        }
    }

    /// Wait for work until `until` (ns) at the latest; returns what was applied, with the
    /// receipt time (when the batch was ingested). Never blocks on an engine.
    pub fn step(&mut self, until: u64) -> Vec<(usize, Applied, u64)> {
        let mut events = Vec::new();
        let now = now_ns();
        let deadline = self.next_vsync.map_or(until, |v| v.min(until));
        let timeout = if deadline <= now { 0 } else { ((deadline - now) / 1_000_000) as i32 + 1 };
        let mut fds: Vec<_> = self
            .terms
            .iter()
            .map(|t| {
                if t.lost.is_some() {
                    pfd(-1, 0)
                } else {
                    pfd(t.client.fd(), libc::POLLIN | if t.client.wants_write() { libc::POLLOUT } else { 0 })
                }
            })
            .collect();
        let n = poll(&mut fds, timeout.min(i32::MAX as u64 as i32));
        if n > 0 {
            self.rec.fd_wakeups += 1;
        } else {
            self.rec.timer_wakeups += 1;
        }
        let t0 = now_ns();
        let mut work = false;
        let mut msgs = Vec::new();
        for (i, f) in fds.iter().enumerate() {
            if f.revents == 0 || self.terms[i].lost.is_some() {
                continue;
            }
            let t = &mut self.terms[i];
            if f.revents & libc::POLLOUT != 0 {
                t.client.on_writable();
            }
            if f.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                msgs.clear();
                let r = t.client.drain(&mut msgs);
                for m in msgs.drain(..) {
                    if std::env::var_os("S2_DEBUG").is_some() {
                        match &m {
                            crate::proto::ToDie::Frame(f) => eprintln!("[die] frame seq={} rows={} full={}", f.seq, f.dirty.len(), f.full),
                            crate::proto::ToDie::Image(i) => eprintln!("[die] image {} {}x{}", i.image_id, i.width, i.height),
                            m => eprintln!("[die] {m:?}"),
                        }
                    }
                    work = true;
                    self.rec.msgs += 1;
                    let a = t.surf.apply(m);
                    if std::env::var_os("S2_DEBUG").is_some() && matches!(a, Applied::Exited(_)) {
                        eprintln!("[die] screen at exit:\n{}", t.surf.screen_text().trim_end());
                    }
                    events.push((i, a));
                }
                if let Err(e) = r {
                    t.lost = Some(e);
                    work = true;
                }
            }
        }
        let t1 = now_ns();
        if work {
            self.acc += t1 - t0;
            self.rec.wake_ingest.push(t1 - t0);
            if self.next_vsync.is_none() {
                let k = (t1 - self.epoch) / VSYNC_NS + 1;
                self.next_vsync = Some(self.epoch + k * VSYNC_NS);
            }
        }
        // vsync: "present", then return each terminal's frame credit.
        if let Some(v) = self.next_vsync
            && t1 >= v
        {
            self.rec.vsync_ingest.push(self.acc);
            self.acc = 0;
            self.next_vsync = None;
            for t in self.terms.iter_mut() {
                if let Some(seq) = t.surf.need_ack.take()
                    && t.lost.is_none()
                {
                    t.client.send(ToEngine::Ack { seq });
                }
            }
        }
        events.into_iter().map(|(i, a)| (i, a, t1)).collect()
    }

    /// Step until `pred` holds or `timeout_ns` passes. Returns whether pred held.
    pub fn until(&mut self, timeout_ns: u64, mut pred: impl FnMut(&mut Loop, &[(usize, Applied, u64)]) -> bool) -> bool {
        let end = now_ns() + timeout_ns;
        loop {
            let ev = self.step(end);
            if pred(self, &ev) {
                return true;
            }
            if now_ns() >= end {
                return false;
            }
        }
    }

    /// Step until no message arrives for `quiet_ns` (or `max_ns` passes).
    pub fn settle(&mut self, quiet_ns: u64, max_ns: u64) {
        let end = now_ns() + max_ns;
        let mut last = now_ns();
        while now_ns() < end {
            let ev = self.step((last + quiet_ns).min(end));
            if !ev.is_empty() {
                last = now_ns();
            } else if now_ns() >= last + quiet_ns {
                return;
            }
        }
    }

    pub fn t(&mut self, i: usize) -> &mut Term {
        &mut self.terms[i]
    }

    /// Wait for a Stats reply from terminal i.
    pub fn stats(&mut self, i: usize) -> Option<crate::proto::Stats> {
        self.terms[i].client.send(ToEngine::GetStats);
        let mut got = None;
        self.until(5_000_000_000, |_, ev| {
            for (j, a, _) in ev {
                if *j == i
                    && let Applied::Stats(s) = a
                {
                    got = Some(s.clone());
                }
            }
            got.is_some()
        });
        got
    }

    pub fn scrollback(&mut self, i: usize) -> Option<(u32, Vec<u8>, u64)> {
        let t0 = now_ns();
        self.terms[i].client.send(ToEngine::FetchScrollback { req: 1 });
        let mut got = None;
        self.until(10_000_000_000, |_, ev| {
            for (j, a, t) in ev {
                if *j == i
                    && let Applied::Scrollback { lines, text, .. } = a
                {
                    got = Some((*lines, text.clone(), t - t0));
                }
            }
            got.is_some()
        });
        got
    }

    /// Type `s` as raw input (tests; DIE would send Key events).
    pub fn type_text(&mut self, i: usize, s: &str) {
        self.terms[i].client.send(ToEngine::Text(s.as_bytes().to_vec()));
    }

    pub fn screen_has(&self, i: usize, needle: &str) -> bool {
        self.terms[i].surf.screen_text().contains(needle)
    }
}

fn letter_key(i: usize) -> (i32, char) {
    use libghostty_vt::key::Key as K;
    const KEYS: [K; 26] = [
        K::A, K::B, K::C, K::D, K::E, K::F, K::G, K::H, K::I, K::J, K::K, K::L, K::M, K::N, K::O, K::P, K::Q, K::R,
        K::S, K::T, K::U, K::V, K::W, K::X, K::Y, K::Z,
    ];
    (i32::from(KEYS[i % 26]), (b'a' + (i % 26) as u8) as char)
}

fn key_event(i: usize, ctrl: bool) -> ToEngine {
    use libghostty_vt::key::{Action, Mods};
    let (k, c) = letter_key(i);
    ToEngine::Key {
        key: k,
        mods: if ctrl { Mods::CTRL.bits() } else { 0 },
        action: i32::from(Action::Press),
        text: if ctrl { String::new() } else { c.to_string() },
        unshifted: c as u32,
        t_sent: now_ns(),
    }
}

pub fn exe() -> String {
    std::env::current_exe().unwrap().to_string_lossy().into_owned()
}

fn bash() -> Vec<String> {
    ["/bin/bash", "--noprofile", "--norc", "-i"].iter().map(|s| s.to_string()).collect()
}

fn raw(v: &[u64]) -> String {
    let parts: Vec<String> = v.iter().map(|x| x.to_string()).collect();
    format!("[{}]", parts.join(","))
}

fn j(s: &Summary) -> String {
    s.to_string()
}

fn ingest_json(rec: &Rec) -> String {
    let v = summarize_ns(&rec.vsync_ingest);
    let over = rec.vsync_ingest.iter().filter(|&&x| x > 1_000_000).count();
    format!(
        "{{\"per_vsync\":{},\"vsyncs_over_1ms\":{},\"per_wake\":{},\"fd_wakeups\":{},\"msgs\":{}}}",
        j(&v),
        over,
        j(&summarize_ns(&rec.wake_ingest)),
        rec.fd_wakeups,
        rec.msgs
    )
}

/// Run one workload on one shape; returns a JSON object (one line).
pub fn run(shape: &str, workload: &str, run_dir: &str, opt: &std::collections::HashMap<String, String>) -> String {
    let get = |k: &str, d: &str| opt.get(k).cloned().unwrap_or_else(|| d.to_string());
    let cfg80 = Cfg { cols: 120, rows: 40, cell_w: 18, cell_h: 36 };
    let mut o = String::new();
    write!(
        o,
        "{{\"shape\":\"{shape}\",\"workload\":\"{workload}\",\"rep\":\"{}\",\"variant\":\"{}\",\"t\":{}",
        get("rep", "1"),
        get("variant", "plain"),
        now_ns()
    )
    .unwrap();
    match workload {
        "w1" => {
            let n: usize = get("n", "500").parse().unwrap();
            let mut l = Loop::new(vec![make(shape, "w1", &bash(), cfg80, run_dir)]);
            l.settle(500_000_000, 10_000_000_000);
            let (mut e2e, mut pipe, mut build) = (Vec::new(), Vec::new(), Vec::new());
            let mut misses = 0;
            for i in 0..n {
                if i > 0 && i % 50 == 0 {
                    l.t(0).client.send(key_event(20, true)); // ctrl-u clears the line
                    l.settle(30_000_000, 2_000_000_000);
                }
                let cx0 = l.t(0).surf.cursor.map(|c| c.0);
                let t_send = now_ns();
                l.t(0).client.send(key_event(i, false));
                let mut got = None;
                l.until(1_000_000_000, |l, ev| {
                    for (_, a, t) in ev {
                        if let Applied::Frame { t_read, t_built, .. } = a
                            && l.terms[0].surf.cursor.map(|c| c.0) != cx0
                        {
                            got = Some((*t, *t_read, *t_built));
                        }
                    }
                    got.is_some()
                });
                match got {
                    Some((t, tr, tb)) => {
                        e2e.push(t - t_send);
                        pipe.push(t - tr);
                        build.push(tb - tr);
                    }
                    None => misses += 1,
                }
                let next = now_ns() + 15_000_000;
                while now_ns() < next {
                    l.step(next);
                }
            }
            write!(
                o,
                ",\"samples\":{},\"misses\":{},\"key_to_frame\":{},\"pty_read_to_frame_available\":{},\"pty_read_to_frame_built\":{},\"ingest\":{}",
                e2e.len(),
                misses,
                j(&summarize_ns(&e2e)),
                j(&summarize_ns(&pipe)),
                j(&summarize_ns(&build)),
                ingest_json(&l.rec)
            )
            .unwrap();
            write!(o, ",\"raw_key_to_frame_ns\":{},\"raw_pty_read_to_available_ns\":{}", raw(&e2e), raw(&pipe)).unwrap();
            l.t(0).client.send(ToEngine::Shutdown);
            l.settle(100_000_000, 1_000_000_000);
        }
        "w2" => {
            let bytes: usize = get("bytes", "209715200").parse().unwrap();
            let argv = vec![exe(), "gen".into(), "w2".into(), "--bytes".into(), bytes.to_string()];
            let mut l = Loop::new(vec![make(shape, "w2", &argv, cfg80, run_dir)]);
            let (mut t_start_gen, mut t_done_gen, mut t_done_recv, mut t_first) = (0u64, 0u64, 0u64, 0u64);
            let mut frames_during = 0u64;
            let mut rows_during = 0usize;
            l.rec = Rec::default();
            let ok = l.until(600_000_000_000, |_, ev| {
                for (_, a, t) in ev {
                    match a {
                        Applied::Effect(Effect::Title(s)) if s.starts_with("W2-START") => {
                            t_start_gen = s[9..].trim().parse().unwrap_or(0);
                            t_first = *t;
                        }
                        Applied::Effect(Effect::Title(s)) if s.starts_with("W2-DONE") => {
                            t_done_gen = s[8..].trim().parse().unwrap_or(0);
                            t_done_recv = *t;
                        }
                        Applied::Frame { rows, .. } if t_first > 0 && t_done_recv == 0 => {
                            frames_during += 1;
                            rows_during += rows;
                        }
                        _ => {}
                    }
                }
                t_done_recv > 0
            });
            l.settle(200_000_000, 5_000_000_000);
            let st = l.stats(0).unwrap_or_default();
            let secs = (t_done_recv - t_start_gen) as f64 / 1e9;
            write!(
                o,
                ",\"ok\":{ok},\"bytes\":{bytes},\"gen_write_s\":{:.3},\"to_die_s\":{:.3},\"mb_per_s\":{:.1},\"frames\":{frames_during},\"frames_per_s\":{:.1},\"avg_rows_per_frame\":{:.1},\"engine_bytes_parsed\":{},\"engine_frames\":{},\"transport_bytes_in\":{},\"ingest\":{}",
                (t_done_gen - t_start_gen) as f64 / 1e9,
                secs,
                bytes as f64 / 1048576.0 / secs,
                frames_during as f64 / secs,
                rows_during as f64 / frames_during.max(1) as f64,
                st.bytes_parsed,
                st.frames,
                l.t(0).client.bytes_in(),
                ingest_json(&l.rec)
            )
            .unwrap();
            write!(o, ",\"raw_vsync_ingest_ns\":{}", raw(&l.rec.vsync_ingest)).unwrap();
            l.t(0).client.send(ToEngine::Shutdown);
            l.settle(100_000_000, 1_000_000_000);
        }
        "w3" => {
            let frames: usize = get("frames", "1000").parse().unwrap();
            let fps = get("fps", "60");
            let (cols, rows) = (250u16, 70u16);
            let cfg = Cfg { cols, rows, cell_w: 18, cell_h: 36 };
            let argv = vec![
                exe(), "gen".into(), "w3".into(), "--frames".into(), frames.to_string(), "--fps".into(), fps.clone(),
                "--cols".into(), cols.to_string(), "--rows".into(), rows.to_string(),
            ];
            let mut l = Loop::new(vec![make(shape, "w3", &argv, cfg, run_dir)]);
            l.rec = Rec::default();
            let mut lat = Vec::new();
            let mut pipe = Vec::new();
            let mut last_k: i64 = -1;
            let mut coalesced = 0i64;
            let mut done = false;
            let mut bad = 0;
            l.until(600_000_000_000, |l, ev| {
                for (_, a, t) in ev {
                    match a {
                        Applied::Frame { t_read, .. } => {
                            let r0 = l.terms[0].surf.row_text(0);
                            if let Some(p) = r0.find("#F")
                                && r0.len() >= p + 30
                            {
                                let k: i64 = r0[p + 2..p + 8].parse().unwrap_or(-1);
                                let tw: u64 = r0[p + 10..p + 30].parse().unwrap_or(0);
                                if k > last_k && tw > 0 {
                                    coalesced += k - last_k - 1;
                                    last_k = k;
                                    lat.push(t - tw);
                                    pipe.push(t - t_read);
                                } else if k < last_k {
                                    bad += 1;
                                }
                            }
                        }
                        Applied::Effect(Effect::Title(s)) if s.starts_with("W3-DONE") => done = true,
                        _ => {}
                    }
                }
                done
            });
            l.settle(200_000_000, 2_000_000_000);
            write!(
                o,
                ",\"frames_sent\":{frames},\"fps\":{fps},\"frames_seen\":{},\"coalesced\":{coalesced},\"out_of_order\":{bad},\"pty_write_to_frame_available\":{},\"pty_read_to_frame_available\":{},\"transport_bytes_in\":{},\"ingest\":{}",
                lat.len(),
                j(&summarize_ns(&lat)),
                j(&summarize_ns(&pipe)),
                l.t(0).client.bytes_in(),
                ingest_json(&l.rec)
            )
            .unwrap();
            write!(o, ",\"raw_write_to_available_ns\":{},\"raw_vsync_ingest_ns\":{}", raw(&lat), raw(&l.rec.vsync_ingest)).unwrap();
            l.t(0).client.send(ToEngine::Shutdown);
            l.settle(100_000_000, 1_000_000_000);
        }
        "w4" => {
            let kind = get("kind", "rgba4k");
            let argv = vec![exe(), "gen".into(), "w4".into(), "--kind".into(), kind.clone()];
            let mut l = Loop::new(vec![make(shape, "w4", &argv, cfg80, run_dir)]);
            let st0 = l.stats(0).unwrap_or_default(); // the generator waits 1 s before writing
            let b0 = l.t(0).client.bytes_in();
            l.rec = Rec::default();
            let (mut t_start, mut payload, mut t_sent_gen) = (0u64, 0u64, 0u64);
            let mut img = None;
            let mut img_wake_ingest = 0u64;
            l.until(300_000_000_000, |l, ev| {
                for (_, a, t) in ev {
                    match a {
                        Applied::Effect(Effect::Title(s)) if s.starts_with("W4-START") => {
                            let mut it = s[9..].split(' ');
                            t_start = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                            payload = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                        }
                        Applied::Effect(Effect::Title(s)) if s.starts_with("W4-SENT") => {
                            t_sent_gen = s[8..].trim().parse().unwrap_or(0)
                        }
                        Applied::Image { id, t_ready, t_sent, bytes } => {
                            img = Some((*id, *t_ready, *t_sent, *bytes, *t));
                            img_wake_ingest = *l.rec.wake_ingest.last().unwrap_or(&0);
                        }
                        _ => {}
                    }
                }
                img.is_some() && t_sent_gen > 0
            });
            let b1 = l.t(0).client.bytes_in();
            l.settle(200_000_000, 2_000_000_000);
            let st1 = l.stats(0).unwrap_or_default();
            let (id, t_ready, t_sent, bytes, t_avail) = img.unwrap_or_default();
            // Content check, outside the timing.
            let (w, h, want) = crate::workgen::w4_pixels(&kind);
            let got = l.t(0).surf.images.get(&id).map(|i| crate::util::fnv64(i.pixels.bytes()));
            let pixels_ok = got == Some(crate::util::fnv64(&want));
            let kind_of = l.t(0).surf.images.get(&id).map(|i| match i.pixels {
                crate::proto::Pixels::Heap(_) => "heap Arc (in-process)",
                crate::proto::Pixels::Shm(_) => "shm mapping (fd via SCM_RIGHTS)",
            });
            write!(
                o,
                ",\"kind\":\"{kind}\",\"w\":{w},\"h\":{h},\"rgba_bytes\":{bytes},\"escape_payload_bytes\":{payload},\"pixels_ok\":{pixels_ok},\"pixel_memory\":\"{}\",\"gen_start_to_ready_ms\":{:.2},\"gen_sent_to_ready_ms\":{:.2},\"engine_copy_ms\":{:.3},\"transport_ms\":{:.3},\"handoff_ready_to_available_ms\":{:.3},\"main_thread_ingest_for_image_wake_us\":{:.1},\"png_decode_ms\":{:.2},\"engine_copies\":{},\"transport_bytes_during_image\":{},\"ingest\":{}",
                kind_of.unwrap_or("none"),
                (t_ready.saturating_sub(t_start)) as f64 / 1e6,
                (t_ready as f64 - t_sent_gen as f64) / 1e6,
                (t_sent - t_ready) as f64 / 1e6,
                (t_avail - t_sent) as f64 / 1e6,
                (t_avail - t_ready) as f64 / 1e6,
                img_wake_ingest as f64 / 1e3,
                (st1.png_decode_ns - st0.png_decode_ns) as f64 / 1e6,
                st1.images_copied - st0.images_copied,
                b1 - b0,
                ingest_json(&l.rec)
            )
            .unwrap();
            l.t(0).client.send(ToEngine::Shutdown);
            l.settle(100_000_000, 1_000_000_000);
        }
        "w5" => {
            let secs: u64 = get("secs", "60").parse().unwrap();
            let mut l = Loop::new(vec![make(shape, "w5", &bash(), cfg80, run_dir)]);
            l.settle(1_000_000_000, 10_000_000_000);
            let me = crate::util::pid();
            let eng = l.t(0).client.engine_pid();
            let child = l.t(0).client.child_pid();
            let s0 = l.stats(0).unwrap_or_default();
            l.settle(200_000_000, 1_000_000_000);
            let (ud0, ue0, uc0) = (usage(me), usage(eng), usage(child));
            let (fd0, tm0) = (l.rec.fd_wakeups, l.rec.timer_wakeups);
            let end = now_ns() + secs * 1_000_000_000;
            let mut events = 0;
            while now_ns() < end {
                events += l.step(end).len();
            }
            let (ud1, ue1, uc1) = (usage(me), usage(eng), usage(child));
            let (fd1, tm1) = (l.rec.fd_wakeups, l.rec.timer_wakeups);
            let s1 = l.stats(0).unwrap_or_default();
            let d = |a: Option<crate::util::Usage>, b: Option<crate::util::Usage>| match (a, b) {
                (Some(a), Some(b)) => format!(
                    "{{\"cpu_ms\":{:.3},\"pkg_idle_wkups\":{},\"interrupt_wkups\":{}}}",
                    ((b.user_ns + b.system_ns) - (a.user_ns + a.system_ns)) as f64 / 1e6,
                    b.pkg_idle_wkups - a.pkg_idle_wkups,
                    b.interrupt_wkups - a.interrupt_wkups
                ),
                _ => "null".into(),
            };
            write!(
                o,
                ",\"secs\":{secs},\"events_in_window\":{events},\"die_main_fd_wakeups\":{},\"die_main_timer_wakeups\":{},\"engine_loop_wakeups\":{},\"die_process\":{},\"engine_process\":{},\"same_process\":{},\"shell\":{}",
                fd1 - fd0,
                tm1 - tm0,
                (s1.loop_wakeups - s0.loop_wakeups).saturating_sub(1),
                d(ud0, ud1),
                d(ue0, ue1),
                me == eng,
                d(uc0, uc1)
            )
            .unwrap();
            l.t(0).client.send(ToEngine::Shutdown);
            l.settle(100_000_000, 1_000_000_000);
        }
        "rss" => {
            let n: usize = get("n", "8").parse().unwrap();
            let me = crate::util::pid();
            let before = usage(me).unwrap();
            let clients: Vec<_> = (0..n).map(|i| make(shape, &format!("rss{i}"), &bash(), cfg80, run_dir)).collect();
            let mut l = Loop::new(clients);
            l.settle(1_500_000_000, 20_000_000_000);
            let measure = |l: &Loop| {
                let die = usage(me).unwrap();
                let engines: Vec<_> = if shape == "helper" {
                    l.terms.iter().filter_map(|t| usage(t.client.engine_pid())).collect()
                } else {
                    Vec::new()
                };
                let avg = |f: &dyn Fn(&crate::util::Usage) -> u64| {
                    engines.iter().map(f).sum::<u64>() as f64 / engines.len().max(1) as f64 / 1048576.0
                };
                format!(
                    "{{\"die_footprint_delta_per_term_mb\":{:.2},\"die_resident_delta_per_term_mb\":{:.2},\"helper_footprint_mb\":{:.2},\"helper_resident_mb\":{:.2}}}",
                    (die.phys_footprint as f64 - before.phys_footprint as f64) / n as f64 / 1048576.0,
                    (die.resident as f64 - before.resident as f64) / n as f64 / 1048576.0,
                    avg(&|u| u.phys_footprint),
                    avg(&|u| u.resident)
                )
            };
            let empty = measure(&l);
            for i in 0..n {
                l.type_text(i, &format!("{} gen lines 10000 sb\n", exe()));
            }
            l.settle(1_500_000_000, 60_000_000_000);
            let full = measure(&l);
            write!(o, ",\"n\":{n},\"empty_scrollback\":{empty},\"scrollback_10k_lines\":{full},\"die_footprint_before_mb\":{:.2}", before.phys_footprint as f64 / 1048576.0).unwrap();
            for i in 0..n {
                l.t(i).client.send(ToEngine::Shutdown);
            }
            l.settle(200_000_000, 2_000_000_000);
        }
        "scrollback" => {
            // Cost of fetching full scrollback (copy state / reattach) through each shape.
            let mut l = Loop::new(vec![make(shape, "sb", &bash(), cfg80, run_dir)]);
            l.settle(500_000_000, 10_000_000_000);
            l.type_text(0, &format!("{} gen lines 10000 sb\n", exe()));
            l.settle(1_000_000_000, 60_000_000_000);
            let mut times = Vec::new();
            let mut lines = 0;
            let mut size = 0;
            for _ in 0..20 {
                if let Some((n, text, dt)) = l.scrollback(0) {
                    times.push(dt);
                    lines = n;
                    size = text.len();
                }
            }
            write!(o, ",\"lines\":{lines},\"bytes\":{size},\"fetch_round_trip\":{}", j(&summarize_ns(&times))).unwrap();
            l.t(0).client.send(ToEngine::Shutdown);
            l.settle(100_000_000, 1_000_000_000);
        }
        w => panic!("unknown workload {w}"),
    }
    o.push('}');
    o
}
