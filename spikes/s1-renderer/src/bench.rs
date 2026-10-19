//! Automated, app-side benches. Input is injected with NSApplication
//! postEvent (no CGEventPost, no permission prompts), so input numbers are
//! "app-side": they start at the event's timestamp inside this process.

use crate::app::{act, Act, App};
use crate::keys::{self, Action, MATRIX};
use crate::platform::{self, Inject};
use crate::scene;
use crate::stats::{now_s, proc_cpu_s, summarize, Lcg, J};
use std::sync::atomic::Ordering;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::window::Window;

pub const LETTERS: [(char, u16); 24] = [('a', 0), ('b', 11), ('c', 8), ('d', 2), ('e', 14), ('f', 3), ('g', 5), ('h', 4), ('i', 34), ('j', 38), ('k', 40), ('l', 37),
    ('m', 46), ('n', 45), ('o', 31), ('p', 35), ('r', 15), ('s', 1), ('t', 17), ('u', 32), ('v', 9), ('x', 7), ('y', 16), ('z', 6)];

fn frames_json(a: &mut App, tag: &str, skip: usize) -> (J, Vec<crate::gpu::FrameRec>) {
    let log = a.gpu().log.lock().unwrap();
    let fr: Vec<_> = log.frames.iter().filter(|f| f.tag == tag).skip(skip).cloned().collect();
    drop(log);
    let col = |f: &dyn Fn(&crate::gpu::FrameRec) -> f64| fr.iter().map(f).collect::<Vec<f64>>();
    let presented = fr.iter().filter(|f| f.presented > 0.0).count();
    let j = J::obj(vec![("frames", J::N(fr.len() as f64)), ("presented", J::N(presented as f64)),
        ("dropped", J::N(fr.iter().filter(|f| f.presented < 0.0).count() as f64)),
        ("cpu_ms", summarize(&col(&|f| f.cpu_ms)).json()), ("wall_ms", summarize(&col(&|f| f.wall_ms)).json()),
        ("acquire_ms", summarize(&col(&|f| f.acquire_ms)).json()), ("gpu_ms", summarize(&col(&|f| f.gpu_ms)).json()),
        ("quads", summarize(&col(&|f| f.quads as f64)).json())]);
    (j, fr)
}

fn push(a: &mut App, name: &str, j: J) { eprintln!("[s1] {name}: done"); a.extra.push((name.to_string(), j)); }

// ---------------------------------------------------------------- check 1
pub fn typing(n: usize, producer: bool, label: &'static str) -> Vec<Act> {
    let mut v = vec![act(move |a, _| {
        a.typing = true;
        if producer { a.start_producer(); } else { a.stop_producer(); }
        a.typed.clear(); a.draw_typing(); a.set_status(format!("bench typing {label}"));
    }), Act::Wait(1.0), act(|a, _| { a.kv.insert("typing_seq0".into(), a.key_seq as f64); a.kv.insert("typing_lost_focus".into(), 0.0); a.kv.insert("typing_occl0".into(), a.c.skipped_occluded as f64); })];
    let mut rng = Lcg(7 + producer as u64);
    for i in 0..n {
        let (c, code) = LETTERS[i % LETTERS.len()];
        v.push(act(move |a, _| {
            if !platform::is_key(&a.win().ns) { *a.kv.entry("typing_lost_focus".into()).or_default() += 1.0; }
            let s = c.to_string();
            a.post(Inject::Key { down: true, code, chars: &s, ign: &s, repeat: false }, 0);
        }));
        v.push(Act::Wait(0.012));
        v.push(act(move |a, _| { let s = c.to_string(); a.post(Inject::Key { down: false, code, chars: &s, ign: &s, repeat: false }, 0); }));
        v.push(Act::Wait(rng.range(0.035, 0.085)));
    }
    v.push(Act::Wait(0.6));
    v.push(act(move |a, _| {
        let seq0 = a.kv["typing_seq0"] as u64;
        let j = typing_stats(a, seq0, u64::MAX, n, producer, label);
        push(a, &format!("typing_{label}"), j);
        if producer { a.stop_producer(); }
    }));
    v
}

/// Latency of keys with sequence numbers in (seq0, seq1]: event timestamp to presentedTime.
pub fn typing_stats(a: &mut App, seq0: u64, seq1: u64, posted: usize, producer: bool, label: &str) -> J {
    let log = a.gpu().log.lock().unwrap();
    let (mut lat, mut disp, mut commit, mut missing, mut keys) = (vec![], vec![], vec![], 0, 0);
    for f in log.frames.iter() {
        for &(seq, ts, th) in &f.keys {
            if seq <= seq0 || seq > seq1 { continue; }
            keys += 1;
            if f.presented > 0.0 { lat.push((f.presented - ts) * 1e3); disp.push((th - ts) * 1e3); commit.push((f.t_commit - ts) * 1e3); } else { missing += 1; }
        }
    }
    drop(log);
    let s = summarize(&lat);
    eprintln!("[s1] typing {label}: latency {} (lost focus {}, occluded skips {})", s.line("ms"), a.kv.get("typing_lost_focus").unwrap_or(&0.0), a.c.skipped_occluded as f64 - a.kv.get("typing_occl0").copied().unwrap_or(0.0));
    J::obj(vec![("keys_posted", J::N(posted as f64)), ("keys_rendered", J::N(keys as f64)), ("keys_not_presented", J::N(missing as f64)),
        ("producer", J::B(producer)), ("lost_focus_posts", J::N(*a.kv.get("typing_lost_focus").unwrap_or(&0.0))),
        ("renders_skipped_occluded", J::N(a.c.skipped_occluded as f64 - a.kv.get("typing_occl0").copied().unwrap_or(0.0))),
        ("valid", J::B(*a.kv.get("typing_lost_focus").unwrap_or(&0.0) == 0.0 && a.c.skipped_occluded as f64 == a.kv.get("typing_occl0").copied().unwrap_or(0.0))),
        ("latency_ms", s.json()), ("event_to_handler_ms", summarize(&disp).json()), ("event_to_commit_ms", summarize(&commit).json()), ("latency_samples_ms", J::nums(&lat))])
}

// ---------------------------------------------------------------- check 2
pub fn redraw(frames: usize) -> Vec<Act> {
    let mut v = vec![act(|a, _| { a.stop_producer(); a.opts.split = false; a.layout(); a.set_status("bench redraw"); }), Act::Wait(0.5)];
    for i in 0..frames {
        v.push(act(move |a, _| {
            let t = now_s();
            let (cols, rows) = (a.scene.grid.cols, a.scene.grid.rows);
            let top = (i * 37) % a.lines.len().max(1);
            scene::fill_file_view(&mut a.scene.grid, &a.lines, top, cols, rows - 2, 1 + (i % 2) as u32);
            a.typed = format!("redraw frame {i} {}", if i % 2 == 0 { "even" } else { "odd" });
            a.draw_typing();
            a.status = format!("bench redraw {i}");
            a.draw_status();
            a.samples.entry("scene_fill_ms".into()).or_default().push((now_s() - t) * 1e3);
            a.render("redraw");
        }));
        v.push(Act::Wait(1.0 / 120.0));
    }
    v.push(Act::Wait(0.5));
    v.push(act(|a, _| {
        let (mut j, fr) = frames_json(a, "redraw", 30);
        let cells = a.scene.grid.cols * a.scene.grid.rows;
        if let J::O(o) = &mut j {
            o.push(("grid".into(), J::s(format!("{}x{}", a.scene.grid.cols, a.scene.grid.rows))));
            o.push(("cells".into(), J::N(cells as f64)));
            o.push(("drawable".into(), J::s(format!("{:?}", fr.first().map(|f| f.drawable)))));
            o.push(("scene_fill_ms".into(), summarize(&a.samples["scene_fill_ms"]).json()));
            o.push(("skipped_inflight".into(), J::N(a.c.skipped_inflight as f64)));
        }
        eprintln!("[s1] redraw: cpu {}", summarize(&fr.iter().map(|f| f.cpu_ms).collect::<Vec<_>>()).line("ms"));
        eprintln!("[s1] redraw: gpu {}", summarize(&fr.iter().map(|f| f.gpu_ms).collect::<Vec<_>>()).line("ms"));
        push(a, "redraw", j);
        a.opts.split = true; a.layout();
    }));
    v
}

// ---------------------------------------------------------------- check 3
pub fn producer(secs: f64) -> Vec<Act> {
    vec![act(|a, _| { a.opts.split = true; a.layout(); a.start_producer(); a.set_status("bench producer 60 Hz"); }), Act::Wait(2.0),
        act(|a, _| {
            let sh = &a.pix.as_ref().unwrap().shared;
            let (p0, m0, pr0) = (sh.produced.load(Ordering::SeqCst), sh.main_memcpy_bytes.load(Ordering::SeqCst), sh.producer_memcpy_bytes.load(Ordering::SeqCst));
            let (s0, w0) = (sh.superseded.load(Ordering::SeqCst), sh.ring_waits.load(Ordering::SeqCst));
            for (k, x) in [("p0", p0 as f64), ("m0", m0 as f64), ("pr0", pr0 as f64), ("s0", s0 as f64), ("w0", w0 as f64), ("t0", now_s())] { a.kv.insert(k.into(), x); }
        }),
        Act::Wait(secs),
        act(|a, _| { let sh = &a.pix.as_ref().unwrap().shared; a.kv.insert("p1".into(), sh.produced.load(Ordering::SeqCst) as f64); a.kv.insert("t1".into(), now_s()); }),
        Act::Wait(0.5),
        act(|a, _| {
            let (p0, p1, t0, t1) = (a.kv["p0"] as u64, a.kv["p1"] as u64, a.kv["t0"], a.kv["t1"]);
            let sh = a.pix.as_ref().unwrap().shared.clone();
            let log = a.gpu().log.lock().unwrap();
            let mut shown = std::collections::BTreeSet::new();
            let (mut cpu, mut wall, mut acq, mut gpu, mut blit) = (vec![], vec![], vec![], vec![], 0u64);
            for f in log.frames.iter().filter(|f| f.t0 >= t0 && f.t0 <= t1 + 0.2) {
                if f.pix_frame > p0 && f.pix_frame <= p1 && f.presented > 0.0 { shown.insert(f.pix_frame); }
                cpu.push(f.cpu_ms); wall.push(f.wall_ms); acq.push(f.acquire_ms); gpu.push(f.gpu_ms); blit += f.blit_bytes;
            }
            drop(log);
            let produced = p1 - p0;
            let copy = sh.copy_ms.lock().unwrap().clone();
            let draw = a.stop_producer();
            let ratio = shown.len() as f64 / produced.max(1) as f64;
            eprintln!("[s1] producer: produced {produced} presented {} ({:.2}%) main memcpy {} B", shown.len(), ratio * 100.0, sh.main_memcpy_bytes.load(Ordering::SeqCst) as f64 - a.kv["m0"]);
            let j = J::obj(vec![("seconds", J::N(t1 - t0)), ("produced", J::N(produced as f64)), ("presented", J::N(shown.len() as f64)), ("presented_ratio", J::N(ratio)),
                ("superseded", J::N(sh.superseded.load(Ordering::SeqCst) as f64 - a.kv["s0"])), ("ring_waits", J::N(sh.ring_waits.load(Ordering::SeqCst) as f64 - a.kv["w0"])),
                ("main_thread_pixel_memcpy_bytes", J::N(sh.main_memcpy_bytes.load(Ordering::SeqCst) as f64 - a.kv["m0"])),
                ("producer_memcpy_bytes", J::N(sh.producer_memcpy_bytes.load(Ordering::SeqCst) as f64 - a.kv["pr0"])),
                ("gpu_blit_bytes", J::N(blit as f64)), ("pix_size", J::s(format!("{:?}", a.pix_size()))),
                ("producer_copy_ms", summarize(&copy).json()), ("producer_draw_ms", summarize(&draw).json()),
                ("main_frame_cpu_ms", summarize(&cpu).json()), ("main_frame_wall_ms", summarize(&wall).json()), ("acquire_ms", summarize(&acq).json()), ("gpu_ms", summarize(&gpu).json())]);
            push(a, "producer", j);
        })]
}

// ---------------------------------------------------------------- check 4
pub fn idle(secs: f64) -> Vec<Act> {
    vec![act(|a, _| { a.stop_producer(); a.ping_stop.store(true, Ordering::Relaxed); a.set_status("bench idle: nothing should draw"); }), Act::Wait(1.5),
        act(|a, _| { for (k, x) in [("r0", a.c.rendered as f64), ("c0", proc_cpu_s()), ("w0", a.c.wakeups as f64), ("t0", now_s())] { a.kv.insert(k.into(), x); } }),
        Act::Wait(secs),
        act(|a, _| {
            let dt = now_s() - a.kv["t0"];
            let frames = a.c.rendered as f64 - a.kv["r0"];
            let cpu = (proc_cpu_s() - a.kv["c0"]) / dt * 100.0;
            eprintln!("[s1] idle: {frames} frames, {cpu:.3}% CPU over {dt:.1}s");
            let j = J::obj(vec![("seconds", J::N(dt)), ("frames_drawn", J::N(frames)), ("cpu_percent", J::N(cpu)), ("wakeups", J::N(a.c.wakeups as f64 - a.kv["w0"]))]);
            push(a, "idle", j);
        })]
}

// ---------------------------------------------------------------- check 5
fn occl_cycle(v: &mut Vec<Act>, n: usize, cover: bool) {
    for _ in 0..n {
        v.push(act(move |a, _| if cover { a.cover.as_ref().unwrap().orderFrontRegardless() } else { a.win().ns.orderOut(None) }));
        v.push(Act::Wait(1.0));
        v.push(act(move |a, _| if cover { a.cover.as_ref().unwrap().orderOut(None) } else { a.win().ns.orderFront(None) }));
        v.push(Act::Wait(1.0));
    }
}

fn occl_collect(label: &'static str, cycles: usize) -> Act {
    act(move |a, _| {
        let t0 = a.kv["o_t0"];
        let ping = std::mem::take(&mut *a.ping.lock().unwrap());
        let log = a.gpu().log.lock().unwrap();
        let fr: Vec<_> = log.frames.iter().filter(|f| f.t0 >= t0).cloned().collect();
        drop(log);
        let acq: Vec<f64> = fr.iter().map(|f| f.acquire_ms).collect();
        let wall: Vec<f64> = fr.iter().map(|f| f.wall_ms).collect();
        let sp = summarize(&ping);
        eprintln!("[s1] occlusion {label}: main-thread ping latency {} ; max acquire {:.1} ms", sp.line("ms"), summarize(&acq).max);
        let j = J::obj(vec![("cycles", J::N(cycles as f64)), ("guard", J::B(a.opts.guard)), ("ping_latency_ms", sp.json()), ("frame_acquire_ms", summarize(&acq).json()),
            ("frame_wall_ms", summarize(&wall).json()), ("frames", J::N(fr.len() as f64)), ("occluded_events", J::N(a.c.occluded_events as f64 - a.kv["o_ev0"])),
            ("skipped_occluded", J::N(a.c.skipped_occluded as f64 - a.kv["o_sk0"])), ("no_drawable", J::N(a.c.no_drawable as f64 - a.kv["o_nd0"]))]);
        push(a, &format!("occlusion_{label}"), j);
    })
}

fn occl_mark() -> Act {
    act(|a, _| { a.ping.lock().unwrap().clear(); for (k, x) in [("o_t0", now_s()), ("o_ev0", a.c.occluded_events as f64), ("o_sk0", a.c.skipped_occluded as f64), ("o_nd0", a.c.no_drawable as f64)] { a.kv.insert(k.into(), x); } })
}

pub fn occlusion(cycles: usize, unguarded: usize) -> Vec<Act> {
    let mut v = vec![act(|a, el| {
        a.opts.split = true; a.layout(); a.start_producer(); a.start_ping(5); a.set_status("bench occlusion");
        let size = a.win().window.outer_size().to_logical::<f64>(a.scale);
        let w = el.create_window(Window::default_attributes().with_decorations(false).with_inner_size(LogicalSize::new(size.width, size.height))
            .with_position(LogicalPosition::new(0.0, 0.0)).with_active(false).with_visible(false)).unwrap();
        let ns = platform::ns_window(&platform::ns_view(&w));
        ns.orderOut(None);
        a.cover = Some(ns);
        a.cover_win = Some(w);
    }), Act::Wait(1.0)];
    v.push(occl_mark()); occl_cycle(&mut v, cycles, true); v.push(occl_collect("cover_guarded", cycles));
    v.push(occl_mark()); occl_cycle(&mut v, cycles / 2, false); v.push(occl_collect("orderout_guarded", cycles / 2));
    if unguarded > 0 {
        v.push(act(|a, _| a.opts.guard = false));
        v.push(occl_mark()); occl_cycle(&mut v, unguarded, true); v.push(occl_collect("cover_unguarded", unguarded));
        v.push(occl_mark()); occl_cycle(&mut v, unguarded, false); v.push(occl_collect("orderout_unguarded", unguarded));
        v.push(act(|a, _| a.opts.guard = true));
    }
    v.push(act(|a, _| { a.ping_stop.store(true, Ordering::Relaxed); a.stop_producer(); a.win().ns.makeKeyAndOrderFront(None); a.cover = None; a.cover_win = None; }));
    v.push(Act::Wait(0.5));
    v
}

// ---------------------------------------------------------------- check 6
pub fn scale(cycles: usize) -> Vec<Act> {
    let mut v = vec![act(|a, _| { a.opts.split = true; a.layout(); a.start_producer(); a.set_status("bench scale (simulated)"); a.scale_log.clear(); }), Act::Wait(0.5)];
    for _ in 0..cycles {
        v.push(act(|a, _| { a.on_scale(1.0, "scale"); }));
        v.push(Act::Wait(0.4));
        v.push(act(|a, _| { let s = a.pix.as_ref().map(|p| p.shared.size()).unwrap_or_default(); a.samples.entry("pix_w_at_1x".into()).or_default().push(s.0 as f64); }));
        v.push(act(|a, _| { a.on_scale(2.0, "scale"); }));
        v.push(Act::Wait(0.4));
        v.push(act(|a, _| { let s = a.pix.as_ref().map(|p| p.shared.size()).unwrap_or_default(); a.samples.entry("pix_w_at_2x".into()).or_default().push(s.0 as f64); }));
    }
    v.push(act(|a, _| {
        let log: Vec<_> = a.gpu().log.lock().unwrap().frames.clone();
        let mut rows = vec![];
        let (mut ok, mut lat) = (0, vec![]);
        let mut prev_gen = 0;
        for &(t, id, px, gen, cell) in &a.scale_log {
            let f = log.iter().find(|f| f.id == id);
            let good = f.is_some_and(|f| f.drawable == px && f.presented > 0.0) && gen != prev_gen;
            if good { ok += 1; lat.push((f.unwrap().presented - t) * 1e3); }
            prev_gen = gen;
            rows.push(J::obj(vec![("frame", J::N(id as f64)), ("expected", J::s(format!("{px:?}"))), ("drawable", J::s(format!("{:?}", f.map(|f| f.drawable)))),
                ("presented", J::B(f.is_some_and(|f| f.presented > 0.0))), ("atlas_gen", J::N(gen as f64)), ("cell", J::s(format!("{cell:?}")))]));
        }
        let (p1, p2) = (a.samples.get("pix_w_at_1x").cloned().unwrap_or_default(), a.samples.get("pix_w_at_2x").cloned().unwrap_or_default());
        let j = J::obj(vec![("method", J::s("simulated: our ScaleFactorChanged path called directly; AppKit/winit notification not exercised")),
            ("changes", J::N(a.scale_log.len() as f64)), ("first_frame_correct", J::N(ok as f64)), ("change_to_present_ms", summarize(&lat).json()),
            ("pixbuf_width_at_1x", J::nums(&p1)), ("pixbuf_width_at_2x", J::nums(&p2)), ("events", J::A(rows))]);
        eprintln!("[s1] scale: {ok}/{} changes had a correct first frame", a.scale_log.len());
        push(a, "scale", j);
        a.stop_producer();
    }));
    v
}

// ---------------------------------------------------------------- check 7
pub fn resize(secs: f64, hz: f64) -> Vec<Act> {
    let mut v = vec![act(|a, _| {
        a.opts.split = true; a.layout(); a.start_producer(); a.set_status("bench resize (programmatic)");
        let f = a.win().ns.frame();
        for (k, x) in [("rx", f.origin.x), ("ry", f.origin.y), ("rw", f.size.width), ("rh", f.size.height)] { a.kv.insert(k.into(), x); }
    }), Act::Wait(0.5)];
    let steps = (secs * hz) as usize;
    for i in 0..steps {
        v.push(act(move |a, _| {
            let (x, y, w0, h0) = (a.kv["rx"], a.kv["ry"], a.kv["rw"], a.kv["rh"]);
            let k = 0.5 + 0.5 * (i as f64 * std::f64::consts::TAU / 240.0).cos();
            let (w, h) = ((w0 * (0.55 + 0.45 * k)).round(), (h0 * (0.7 + 0.3 * k)).round());
            let t = now_s();
            platform::set_frame(&a.win().ns, x, y + (h0 - h), w, h);
            let changed = platform::backing_px(&a.win().view, a.scale) != a.px;
            let r = a.on_resize(true, "resize");
            let ms = (now_s() - t) * 1e3;
            if changed {
                a.samples.entry("resize_step_ms".into()).or_default().push(ms);
                if r.is_none() { *a.kv.entry("resize_no_sync_frame".into()).or_default() += 1.0; }
            }
        }));
        v.push(Act::Wait(1.0 / hz));
    }
    v.push(act(|a, _| { let (x, y, w, h) = (a.kv["rx"], a.kv["ry"], a.kv["rw"], a.kv["rh"]); platform::set_frame(&a.win().ns, x, y, w, h); a.on_resize(true, "main"); }));
    v.push(Act::Wait(0.5));
    v.push(act(move |a, _| {
        let (mut j, fr) = frames_json(a, "resize", 0);
        let mismatched = fr.iter().filter(|f| f.drawable != f.expected).count();
        let steps = a.samples.get("resize_step_ms").cloned().unwrap_or_default();
        if let J::O(o) = &mut j {
            o.push(("method".into(), J::s(format!("programmatic: NSWindow setFrame every 1/{hz} s, then a synchronous render with presentsWithTransaction; not a user drag"))));
            o.push(("steps".into(), J::N(steps.len() as f64)));
            o.push(("steps_without_sync_frame".into(), J::N(*a.kv.get("resize_no_sync_frame").unwrap_or(&0.0))));
            o.push(("drawable_size_mismatches".into(), J::N(mismatched as f64)));
            o.push(("step_ms_setframe_plus_render".into(), summarize(&steps).json()));
        }
        eprintln!("[s1] resize: step {} ; frame wall {}", summarize(&steps).line("ms"), summarize(&fr.iter().map(|f| f.wall_ms).collect::<Vec<_>>()).line("ms"));
        push(a, "resize", j);
        a.stop_producer();
    }));
    v
}

// ---------------------------------------------------------------- check 10
fn mod_bit(code: u16) -> u16 {
    use keys::m::*;
    match code { 56 => LSHIFT, 60 => RSHIFT, 59 => LCTRL, 62 => RCTRL, 58 => LALT, 61 => RALT, 55 => LSUPER, 54 => RSUPER, _ => 0 }
}

pub fn keymatrix() -> Vec<Act> {
    let mut v = vec![act(|a, _| { a.stop_producer(); a.typing = false; a.set_status("bench keyboard matrix"); }), Act::Wait(0.3)];
    for (ime, fix) in [(false, false), (false, true), (true, false), (true, true)] {
        v.push(act(move |a, _| { a.win().window.set_ime_allowed(ime); platform::FIX_KEYS.with(|f| f.set(fix)); a.kv.insert("fix".into(), fix as u8 as f64); a.matrix_rows.clear(); }));
        v.push(Act::Wait(0.1));
        for ci in 0..MATRIX.len() {
            v.push(act(move |a, _| {
                let c = &MATRIX[ci];
                for (k, x) in [("k0", a.keys.len() as f64), ("r0", a.raw.len() as f64), ("b0", platform::WOULD_BEEP.load(Ordering::SeqCst) as f64)] { a.kv.insert(k.into(), x); }
                let mut flags = 0usize;
                for &m in c.mods { flags |= platform::mod_flag(m) | platform::side_bits(m); a.post(Inject::Flags { code: m }, flags); }
                if keys::is_modifier_code(c.code) {
                    let bits = platform::mod_flag(c.code) | platform::side_bits(c.code);
                    a.post(Inject::Flags { code: c.code }, flags | bits);
                    a.post(Inject::Flags { code: c.code }, flags);
                } else {
                    a.post(Inject::Key { down: true, code: c.code, chars: c.chars, ign: c.ign, repeat: false }, flags);
                    for _ in 0..c.repeats { a.post(Inject::Key { down: true, code: c.code, chars: c.chars, ign: c.ign, repeat: true }, flags); }
                    a.post(Inject::Key { down: false, code: c.code, chars: c.chars, ign: c.ign, repeat: false }, flags);
                }
                for &m in c.mods.iter().rev() { flags &= !(platform::mod_flag(m) | platform::side_bits(m)); a.post(Inject::Flags { code: m }, flags); }
            }));
            v.push(Act::Wait(0.06));
            v.push(act(move |a, _| { let r = eval_chord(a, ci, true); a.matrix_rows.push(r); }));
        }
        v.push(act(move |a, _| {
            let rows = std::mem::take(&mut a.matrix_rows);
            let pass = rows.iter().filter(|r| matches!(r, J::O(o) if o.iter().any(|(k, v)| k == "pass" && matches!(v, J::B(true))))).count();
            eprintln!("[s1] keys ime={ime} fix={fix}: {pass}/{} chords pass", rows.len());
            push(a, &format!("keys_ime{}_fix{}", ime as u8, fix as u8), J::obj(vec![("ime_allowed", J::B(ime)), ("monitor_fixes", J::B(fix)), ("pass", J::N(pass as f64)), ("total", J::N(rows.len() as f64)), ("chords", J::A(rows))]));
        }));
    }
    v.push(act(|a, _| { a.win().window.set_ime_allowed(false); platform::FIX_KEYS.with(|f| f.set(a.opts.fix_keys)); a.typing = true; }));
    v
}

pub fn eval_chord(a: &mut App, ci: usize, exact_repeats: bool) -> J {
    let c = &MATRIX[ci];
    let (k0, r0, b0) = (a.kv["k0"] as usize, a.kv["r0"] as usize, a.kv["b0"] as u64);
    let evs: Vec<_> = a.keys[k0..].to_vec();
    let raw = a.raw[r0.min(a.raw.len())..].to_vec();
    let beeps = platform::WOULD_BEEP.load(Ordering::SeqCst) - b0;
    let main: Vec<_> = evs.iter().filter(|k| k.code == c.w3c).collect();
    let actions: Vec<Action> = main.iter().map(|k| k.action).collect();
    let reps = if exact_repeats { c.repeats as usize } else if c.repeats > 0 { actions.iter().filter(|x| **x == Action::Repeat).count().max(3) } else { 0 };
    let mut expect = vec![Action::Press];
    expect.extend(std::iter::repeat_n(Action::Repeat, reps));
    expect.push(Action::Release);
    let order_ok = actions == expect;
    let want_mods: u16 = c.mods.iter().map(|&m| mod_bit(m)).fold(0, |x, y| x | y);
    let press = main.iter().find(|k| k.action == Action::Press);
    let mods_ok = keys::is_modifier_code(c.code) || press.is_some_and(|p| p.mods & !keys::m::CAPS == want_mods);
    let fields_ok = press.is_some_and(|p| !p.logical.is_empty() && p.logical != "Unidentified" && (c.base.is_none() || p.unshifted == c.base));
    let is_alt = c.mods.iter().any(|&m| m == 58 || m == 61) && !c.mods.iter().any(|&m| m == 59 || m == 55 || m == 54) && c.base.is_some();
    let alt_ok = !is_alt || press.is_some_and(|p| p.logical.to_lowercase() == c.base.unwrap().to_string() || p.logical == c.ign);
    let printable = c.base.is_some() && !c.mods.iter().any(|&m| m == 59 || m == 55 || m == 54);
    let text_ok = !printable || press.is_some_and(|p| p.text.is_some());
    let modifier_events_ok = c.mods.iter().all(|&m| evs.iter().any(|k| k.action == Action::Press && keys::mods_code(m) == k.code));
    let pass = order_ok && mods_ok && fields_ok && alt_ok && text_ok && modifier_events_ok && beeps == 0;
    J::obj(vec![("chord", J::s(c.name)), ("pass", J::B(pass)), ("actions", J::s(format!("{actions:?}"))), ("order_ok", J::B(order_ok)), ("mods_ok", J::B(mods_ok)),
        ("fields_ok", J::B(fields_ok)), ("option_as_alt_ok", J::B(alt_ok)), ("text_ok", J::B(text_ok)), ("modifier_key_events_ok", J::B(modifier_events_ok)), ("would_beep", J::N(beeps as f64)),
        ("press", press.map(|p| p.json()).unwrap_or(J::Null)), ("monitor_saw", J::N(raw.iter().filter(|r| r.code == c.code).count() as f64)),
        ("rerouted", J::B(raw.iter().any(|r| r.rerouted))), ("sources", J::s(format!("{:?}", main.iter().map(|k| k.source).collect::<Vec<_>>())))])
}

// ---------------------------------------------------------------- window style: clicks on the first text row
pub fn clicks() -> Vec<Act> {
    vec![act(|a, _| {
        a.mouse.clear();
        let h = a.win().view.bounds().size.height;
        for y in [h - 6.0, h - 22.0, h - 60.0] { a.post(Inject::Mouse { down: true, x: 300.0, y }, 0); a.post(Inject::Mouse { down: false, x: 300.0, y }, 0); }
    }), Act::Wait(0.4), act(|a, _| {
        let n = a.mouse.len();
        let j = J::obj(vec![("style", J::s(a.opts.style_name())), ("clicks_posted", J::N(3.0)), ("mouse_events_received", J::N(n as f64)),
            ("note", J::s("clicks at 6, 22 and 60 pt below the top edge; 2 events (down+up) expected per click that reaches the view"))]);
        eprintln!("[s1] clicks ({}): {n}/6 mouse events reached the view", a.opts.style_name());
        push(a, &format!("clicks_{}", a.opts.style_name()), j);
    })]
}

/// Raw frame log for a run: results/frames-<label>.csv
pub fn write_frames_csv(app: &App, label: &str) {
    let frames = app.gpu.as_ref().unwrap().log.lock().unwrap().frames.clone();
    let mut csv = String::from("id,tag,t0,t_commit,cpu_ms,wall_ms,acquire_ms,gpu_ms,presented,drawable_w,drawable_h,expected_w,expected_h,keys,pix_frame,quads\n");
    for f in &frames {
        csv.push_str(&format!("{},{},{:.6},{:.6},{:.4},{:.4},{:.4},{:.4},{:.6},{},{},{},{},{},{},{}\n", f.id, f.tag, f.t0, f.t_commit, f.cpu_ms, f.wall_ms, f.acquire_ms, f.gpu_ms, f.presented, f.drawable.0, f.drawable.1, f.expected.0, f.expected.1, f.keys.len(), f.pix_frame, f.quads));
    }
    std::fs::write(crate::stats::results_dir().join(format!("frames-{label}.csv")), csv).unwrap();
}
