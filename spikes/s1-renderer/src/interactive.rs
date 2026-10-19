//! Interactive modes for checks that need real hardware input. Each one
//! guides the user on screen and writes results/interactive-<name>.{json,md}.

use crate::app::{with_loop, App, Opts, WinStyle};
use crate::bench::{eval_chord, typing_stats};
use crate::keys::{self, Action, MATRIX};
use crate::platform;
use crate::stats::{now_s, results_dir, summarize, write_result, J};
use objc2_app_kit::NSApplication;
use std::sync::atomic::Ordering;

fn save(app: &App, name: &str, j: J, md: String) {
    let name = &if SELFTEST.with(|s| s.get()) { format!("{name}-selftest") } else { name.to_string() };
    crate::bench::write_frames_csv(app, &format!("interactive-{name}"));
    write_result(&format!("interactive-{name}"), &j);
    std::fs::write(results_dir().join(format!("interactive-{name}.md")), md).unwrap();
    eprintln!("[s1] wrote results/interactive-{name}.md");
}

fn get(j: &J, path: &[&str]) -> f64 {
    let mut cur = j;
    for p in path { if let J::O(o) = cur { if let Some((_, v)) = o.iter().find(|(k, _)| k == p) { cur = v; continue; } } return f64::NAN; }
    if let J::N(x) = cur { *x } else { f64::NAN }
}

thread_local! { static SELFTEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

/// --selftest drives the mode with app-side input (postEvent / setFrame /
/// the scale path) so the plumbing and result files can be checked without a person.
fn selftest(a: &mut App, kind: &str) {
    if !SELFTEST.with(|s| s.get()) { return; }
    use crate::app::{act, Act};
    use crate::platform::Inject;
    let mut v: Vec<Act> = vec![Act::Wait(0.5)];
    let key = |down: bool, code: u16, ch: &'static str, rep: bool| act(move |a, _| { a.post(Inject::Key { down, code, chars: ch, ign: ch, repeat: rep }, 0); });
    match kind {
        "typing" => for i in 0..a.opts.keys_target { let (c, code) = crate::bench::LETTERS[i % 24]; let s: &'static str = Box::leak(c.to_string().into_boxed_str()); v.push(key(true, code, s, false)); v.push(Act::Wait(0.012)); v.push(key(false, code, s, false)); v.push(Act::Wait(0.05)); },
        "keys" => {
            for c in MATRIX.iter() {
                let c: &'static keys::Chord = c;
                v.push(act(move |a, _| {
                    let mut flags = 0usize;
                    for &m in c.mods { flags |= platform::mod_flag(m) | platform::side_bits(m); a.post(Inject::Flags { code: m }, flags); }
                    if keys::is_modifier_code(c.code) && c.code != 57 { let b = platform::mod_flag(c.code) | platform::side_bits(c.code); a.post(Inject::Flags { code: c.code }, flags | b); a.post(Inject::Flags { code: c.code }, flags); }
                    else if c.code == 57 { a.post(Inject::Flags { code: 57 }, 1 << 16); a.post(Inject::Flags { code: 57 }, 0); }
                    else { a.post(Inject::Key { down: true, code: c.code, chars: c.chars, ign: c.ign, repeat: false }, flags);
                        for _ in 0..c.repeats.min(4) { a.post(Inject::Key { down: true, code: c.code, chars: c.chars, ign: c.ign, repeat: true }, flags); }
                        a.post(Inject::Key { down: false, code: c.code, chars: c.chars, ign: c.ign, repeat: false }, flags); }
                    for &m in c.mods.iter().rev() { flags &= !(platform::mod_flag(m) | platform::side_bits(m)); a.post(Inject::Flags { code: m }, flags); }
                }));
                v.push(Act::Wait(0.08));
            }
            for _ in 0..2 { v.push(key(true, 45, "n", false)); v.push(key(false, 45, "n", false)); v.push(Act::Wait(0.1)); }
        }
        "occlusion" => for _ in 0..21 { v.push(act(|a, _| a.win().ns.orderOut(None))); v.push(Act::Wait(0.15)); v.push(act(|a, _| a.win().ns.orderFront(None))); v.push(Act::Wait(0.15)); },
        "resize" => {
            v.push(act(|a, _| { a.resize_sync = true; let f = a.win().ns.frame(); a.kv.insert("fw".into(), f.size.width); a.kv.insert("fx".into(), f.origin.x); a.kv.insert("fy".into(), f.origin.y); a.kv.insert("fh".into(), f.size.height); }));
            for i in 0..700 { v.push(act(move |a, _| { let w = a.kv["fw"] * (0.7 + 0.3 * ((i as f64) / 40.0).cos().abs()); platform::set_frame(&a.win().ns, a.kv["fx"], a.kv["fy"], w.round(), a.kv["fh"]); })); v.push(Act::Wait(1.0 / 60.0)); }
        }
        "scale" => { for _ in 0..2 { v.push(act(|a, _| { a.on_scale(1.0, "scale"); })); v.push(Act::Wait(0.3)); v.push(act(|a, _| { a.on_scale(2.0, "scale"); })); v.push(Act::Wait(0.3)); } v.push(act(|a, _| a.exit = true)); }
        _ => {}
    }
    a.script = v.into();
}

pub fn run(args: &[String], mut o: Opts, t_main: f64) {
    SELFTEST.with(|s| s.set(args.iter().any(|a| a == "--selftest")));
    if SELFTEST.with(|s| s.get()) && !args.iter().any(|a| a == "--keys") { o.keys_target = 40; }
    match args.get(2).map(|s| s.as_str()).unwrap_or("") {
        "typing" => typing(o, t_main),
        "keys" => keyboard(o, t_main),
        "occlusion" => occlusion(o, t_main),
        "resize" => { o.style = WinStyle::Titlebar; resize(o, t_main) }
        "scale" => { o.style = WinStyle::Titlebar; scale(o, t_main) }
        k => eprintln!("unknown interactive mode '{k}': use typing, keys, occlusion, resize or scale"),
    }
}

fn typing(o: Opts, t_main: f64) {
    let n = o.keys_target as u64;
    let help = format!("# Interactive typing latency (check 1)\n\nType normally into this window: any letters, spaces, words.\nThe insert line at the bottom shows what you type.\n\nPhase 1: {} keys with nothing else running.\nPhase 2: {} more keys while the pixel buffer (right) animates at 60 Hz.\n\nThe count is on the status line. Cmd-Q stops early and still saves.\nLatency = the key's hardware timestamp to the frame's presentedTime.", n / 2, n - n / 2);
    let mut app = with_loop(o, t_main, true, move |a| {
        a.on_ready = Some(Box::new(move |a| { a.opts.split = true; a.layout(); a.show_text(&help); a.set_status(format!("typing phase 1: 0/{n}")); selftest(a, "typing"); }));
        a.tick = Some(Box::new(move |a, _| {
            if a.kv.get("last").copied() != Some(a.key_seq as f64) {
                a.kv.insert("last".into(), a.key_seq as f64);
                let phase = if a.kv.contains_key("half") { 2 } else { 1 };
                a.set_status(format!("typing phase {phase}: {}/{n}", a.key_seq));
            }
            if !a.kv.contains_key("half") && a.key_seq >= n / 2 {
                a.kv.insert("half".into(), a.key_seq as f64);
                a.start_producer();
                a.show_text("# Phase 2\n\nKeep typing. The pixel buffer on the right now animates at 60 Hz.");
            }
            if a.key_seq >= n && !a.kv.contains_key("done") { a.kv.insert("done".into(), now_s()); }
            if let Some(&t) = a.kv.get("done") { if now_s() - t > 0.5 { a.exit = true; } else { a.deadline = Some(a.deadline.map_or(t + 0.55, |d| d.min(t + 0.55))); } }
        }));
    });
    let half = app.kv.get("half").copied().unwrap_or(f64::MAX) as u64;
    let seq = app.key_seq;
    let p1 = typing_stats(&mut app, 0, half, half.min(seq) as usize, false, "interactive_no_producer");
    let p2 = typing_stats(&mut app, half, u64::MAX, seq.saturating_sub(half) as usize, true, "interactive_with_producer");
    let (a50, a99, b99) = (get(&p1, &["latency_ms", "p50"]), get(&p1, &["latency_ms", "p99"]), get(&p2, &["latency_ms", "p99"]));
    let md = format!("# Interactive typing (check 1, real keys)\n\n| phase | keys | p50 ms | p99 ms | max ms |\n|---|---|---|---|---|\n| no producer | {} | {:.2} | {:.2} | {:.2} |\n| 60 Hz producer | {} | {:.2} | {:.2} | {:.2} |\n\nTarget: p50 ≤ 12 ms, p99 ≤ 17 ms, p99 rise with producer ≤ 1 ms.\nResult: p50 {} , p99 {} , rise {:.2} ms {}.\n",
        get(&p1, &["latency_ms", "n"]), a50, a99, get(&p1, &["latency_ms", "max"]), get(&p2, &["latency_ms", "n"]), get(&p2, &["latency_ms", "p50"]), b99, get(&p2, &["latency_ms", "max"]),
        if a50 <= 12.0 { "pass" } else { "FAIL" }, if a99 <= 17.0 { "pass" } else { "FAIL" }, b99 - a99, if b99 - a99 <= 1.0 { "pass" } else { "FAIL" });
    save(&app, "typing", J::obj(vec![("no_producer", p1), ("with_producer", p2)]), md);
}

fn chord_help(i: usize) -> String {
    let c = &MATRIX[i];
    let how = if c.repeats > 0 { format!("HOLD the key for about 1 second, then release") } else if c.code == 57 { "press Caps Lock, then press it again to turn it off".into() } else { "press and release".into() };
    let mods: Vec<&str> = c.mods.iter().map(|&m| match m { 56 => "left Shift", 60 => "right Shift", 59 => "left Control", 62 => "right Control", 58 => "left Option", 61 => "right Option", 55 => "left Command", 54 => "right Command", _ => "?" }).collect();
    format!("# Keyboard matrix (check 10): chord {} of {}\n\n    {}\n\nHold: {}\nThen: {}\n\nF-keys may need the fn key. Cmd-. (period) skips this chord. Cmd-Q stops and saves.\nIME is on (as in insert state); press-and-hold is disabled for this app.\nListen for a beep and watch for an accent popup; you'll be asked at the end.",
        i + 1, MATRIX.len(), c.name, if mods.is_empty() { "nothing".into() } else { mods.join(" + ") }, how)
}

fn keyboard(o: Opts, t_main: f64) {
    let mut app = with_loop(o, t_main, true, |a| {
        a.on_ready = Some(Box::new(|a| {
            a.typing = false;
            a.win().window.set_ime_allowed(true);
            a.kv.insert("ci".into(), 0.0);
            start_chord(a, 0);
            selftest(a, "keys");
        }));
        a.on_key = Some(Box::new(|a, k| {
            if let Some(&q) = a.kv.get("question") {
                if k.action != Action::Press || !(k.code == "KeyY" || k.code == "KeyN") { return; }
                let yes = k.code == "KeyY";
                if q == 0.0 { a.kv.insert("beep_heard".into(), yes as u8 as f64); a.kv.insert("question".into(), 1.0); a.show_text("# Last question\n\nDid an accent popup (a small character picker) appear at any point?\n\nPress y or n."); }
                else { a.kv.insert("popup_seen".into(), yes as u8 as f64); a.exit = true; }
                return;
            }
            let ci = a.kv["ci"] as usize;
            if ci >= MATRIX.len() { return; }
            let c = &MATRIX[ci];
            if k.code == "Period" && k.mods & keys::m::SUPER != 0 && k.action == Action::Press { a.matrix_rows.push(J::obj(vec![("chord", J::s(c.name)), ("skipped", J::B(true))])); next(a); return; }
            if c.code != 57 && k.code == c.w3c && k.action == Action::Release {
                let r = eval_chord(a, ci, false);
                a.matrix_rows.push(r);
                next(a);
            }
        }));
        a.tick = Some(Box::new(|a, _| {
            let ci = a.kv["ci"] as usize;
            if ci < MATRIX.len() && MATRIX[ci].code == 57 {
                let k0 = a.kv["k0"] as usize;
                if a.keys[k0..].iter().filter(|k| k.code == "CapsLock").count() >= 2 {
                    let mut r = eval_chord(a, ci, false);
                    if let J::O(o) = &mut r { o.push(("note".into(), J::s("Caps Lock events come from the NSEvent monitor; winit 0.30.13 drops them"))); }
                    a.matrix_rows.push(r);
                    next(a);
                }
            }
        }));
    });
    let rows = std::mem::take(&mut app.matrix_rows);
    let pass = rows.iter().filter(|r| get_b(r, "pass")).count();
    let skipped = rows.iter().filter(|r| get_b(r, "skipped")).count();
    let fails: Vec<String> = rows.iter().filter(|r| !get_b(r, "pass") && !get_b(r, "skipped")).map(|r| format!("{} {}", get_s(r, "chord"), get_s(r, "actions"))).collect();
    let beep = app.kv.get("beep_heard").copied();
    let popup = app.kv.get("popup_seen").copied();
    let md = format!("# Interactive keyboard matrix (check 10, real keys)\n\nChords: {} recorded, {pass} pass, {skipped} skipped.\nWould-beep count (noResponderFor:): {}.\nYou heard a beep: {}. You saw an accent popup: {}.\n\nFailures:\n{}\n",
        rows.len(), platform::WOULD_BEEP.load(Ordering::SeqCst), yn(beep), yn(popup), if fails.is_empty() { "- none".into() } else { fails.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n") });
    let keys_json = J::A(app.keys.iter().map(|k| k.json()).collect());
    save(&app, "keys", J::obj(vec![("pass", J::N(pass as f64)), ("total", J::N(rows.len() as f64)), ("skipped", J::N(skipped as f64)),
        ("would_beep", J::N(platform::WOULD_BEEP.load(Ordering::SeqCst) as f64)), ("beep_heard", beep.map(|b| J::B(b > 0.0)).unwrap_or(J::Null)),
        ("popup_seen", popup.map(|b| J::B(b > 0.0)).unwrap_or(J::Null)), ("chords", J::A(rows)), ("events", keys_json)]), md);
}

fn yn(x: Option<f64>) -> &'static str { match x { Some(v) if v > 0.0 => "yes", Some(_) => "no", None => "not answered" } }
fn get_b(j: &J, k: &str) -> bool { matches!(j, J::O(o) if o.iter().any(|(n, v)| n == k && matches!(v, J::B(true)))) }
fn get_s(j: &J, k: &str) -> String { if let J::O(o) = j { for (n, v) in o { if n == k { if let J::S(s) = v { return s.clone(); } } } } String::new() }

fn start_chord(a: &mut App, i: usize) {
    for (k, x) in [("k0", a.keys.len() as f64), ("r0", a.raw.len() as f64), ("b0", platform::WOULD_BEEP.load(Ordering::SeqCst) as f64)] { a.kv.insert(k.into(), x); }
    a.show_text(&chord_help(i));
    a.set_status(format!("keys {}/{}", i + 1, MATRIX.len()));
}

fn next(a: &mut App) {
    let ci = a.kv["ci"] as usize + 1;
    a.kv.insert("ci".into(), ci as f64);
    if ci < MATRIX.len() { start_chord(a, ci); }
    else { a.kv.insert("question".into(), 0.0); a.show_text("# Almost done\n\nDid you hear a system beep at any point during the chords?\n\nPress y or n."); }
}

fn occlusion(o: Opts, t_main: f64) {
    let help = "# Occlusion (check 5)\n\nThe pixel buffer animates while you do this 20 times:\n\n  1. Cmd-Tab to another app whose window covers this one completely\n     (or press Cmd-H here to hide this window),\n  2. wait about a second,\n  3. Cmd-Tab back.\n\nThe cycle count is on the status line. Cmd-Q stops early and saves.";
    let mut app = with_loop(o, t_main, true, move |a| {
        a.on_ready = Some(Box::new(move |a| { a.opts.split = true; a.layout(); a.show_text(help); a.start_producer(); a.start_ping(5); a.kv.insert("o_t0".into(), now_s()); selftest(a, "occlusion"); }));
        a.on_key = Some(Box::new(|a, k| { if k.code == "KeyH" && k.mods & keys::m::SUPER != 0 && k.action == Action::Press { NSApplication::sharedApplication(a.mtm()).hide(None); } }));
        a.tick = Some(Box::new(|a, _| {
            let cycles = a.c.occluded_events / 2;
            if a.kv.get("cyc").copied() != Some(cycles as f64) { a.kv.insert("cyc".into(), cycles as f64); a.set_status(format!("occlusion cycles {cycles}/20")); }
            if cycles >= 20 { a.exit = true; }
        }));
    });
    let t0 = app.kv["o_t0"];
    let ping = app.ping.lock().unwrap().clone();
    let fr: Vec<_> = app.gpu().log.lock().unwrap().frames.iter().filter(|f| f.t0 >= t0).cloned().collect();
    let acq = summarize(&fr.iter().map(|f| f.acquire_ms).collect::<Vec<_>>());
    let sp = summarize(&ping);
    let md = format!("# Interactive occlusion (check 5, real Cmd-Tab)\n\nCycles: {}. Main-thread ping latency: p99 {:.2} ms, max {:.2} ms. Max nextDrawable wait: {:.2} ms.\nTarget: no main-thread stall over 50 ms. Result: {}.\n",
        app.c.occluded_events / 2, sp.p99, sp.max, acq.max, if sp.max <= 50.0 && acq.max <= 50.0 { "pass" } else { "FAIL" });
    save(&app, "occlusion", J::obj(vec![("cycles", J::N((app.c.occluded_events / 2) as f64)), ("ping_latency_ms", sp.json()), ("acquire_ms", acq.json()),
        ("frame_wall_ms", summarize(&fr.iter().map(|f| f.wall_ms).collect::<Vec<_>>()).json()), ("skipped_occluded", J::N(app.c.skipped_occluded as f64)), ("no_drawable", J::N(app.c.no_drawable as f64))]), md);
    app.stop_producer();
}

fn resize(o: Opts, t_main: f64) {
    let help = "# Live resize (check 7)\n\nIf AeroSpace tiles this window, make it floating first\n(AeroSpace: layout floating). Then drag a window edge or corner around\ncontinuously for about 10 seconds. The test ends 10 s after your first\ndrag. Watch for stretched or smeared text while you drag.\n\nCmd-Q stops early and saves.";
    let mut app = with_loop(o, t_main, true, move |a| {
        a.on_ready = Some(Box::new(move |a| { a.opts.split = true; a.layout(); a.show_text(help); a.start_producer(); selftest(a, "resize"); }));
        a.tick = Some(Box::new(|a, _| {
            if !a.kv.contains_key("r_t0") && !a.resize_log.is_empty() { a.kv.insert("r_t0".into(), now_s()); }
            if let Some(&t) = a.kv.get("r_t0") { if now_s() - t > 10.0 { a.exit = true; } else { let n = now_s() + 0.25; a.deadline = Some(a.deadline.map_or(n, |d| d.min(n))); } }
        }));
    });
    let fr: Vec<_> = app.gpu.as_ref().unwrap().log.lock().unwrap().frames.iter().filter(|f| f.tag == "resize").cloned().collect();
    let live = app.resize_log.len();
    let no_frame = app.resize_log.iter().filter(|j| matches!(j, J::O(o) if o.iter().any(|(k, v)| k == "frame" && matches!(v, J::Null)))).count();
    let wall = summarize(&fr.iter().map(|f| f.wall_ms).collect::<Vec<_>>());
    let mism = fr.iter().filter(|f| f.drawable != f.expected).count();
    let md = format!("# Interactive live resize (check 7, real drag)\n\nResize events: {live}; without a synchronous frame: {no_frame}; drawable size mismatches: {mism}.\nResize frame wall time: {}.\nTarget: no stretched frames; frame p99 ≤ 8 ms. Result: {}.\nAlso note what you saw: any stretched or smeared text while dragging?\n",
        wall.line("ms"), if no_frame == 0 && mism == 0 && wall.p99 <= 8.0 { "pass" } else { "FAIL" });
    let events = std::mem::take(&mut app.resize_log);
    save(&app, "resize", J::obj(vec![("resize_events", J::N(live as f64)), ("events_without_sync_frame", J::N(no_frame as f64)), ("drawable_mismatches", J::N(mism as f64)),
        ("frame_wall_ms", wall.json()), ("frame_cpu_ms", summarize(&fr.iter().map(|f| f.cpu_ms).collect::<Vec<_>>()).json()), ("events", J::A(events))]), md);
}

fn scale(o: Opts, t_main: f64) {
    let help = "# Backing scale change (check 6)\n\nThis needs a second display with a different scale (for example a 1x\nexternal monitor). Drag this window (by its background) onto the other\ndisplay and back, twice (make it floating in AeroSpace first, or move it\nwith your AeroSpace move-to-monitor binding). Then press Cmd-Q.\n\nWatch for blurry or wrongly sized text after each move.";
    let app = with_loop(o, t_main, true, move |a| {
        a.on_ready = Some(Box::new(move |a| { a.show_text(help); a.start_producer(); selftest(a, "scale"); }));
    });
    let log: Vec<_> = app.gpu.as_ref().unwrap().log.lock().unwrap().frames.clone();
    let mut rows = vec![];
    let mut ok = 0;
    for &(t, id, px, gen, cell) in &app.scale_log {
        let f = log.iter().find(|f| f.id == id);
        let good = f.is_some_and(|f| f.drawable == px && f.presented > 0.0);
        ok += good as usize;
        rows.push(J::obj(vec![("t", J::N(t)), ("frame", J::N(id as f64)), ("expected", J::s(format!("{px:?}"))), ("drawable", J::s(format!("{:?}", f.map(|f| f.drawable)))),
            ("presented", J::B(f.is_some_and(|f| f.presented > 0.0))), ("atlas_gen", J::N(gen as f64)), ("cell", J::s(format!("{cell:?}")))]));
    }
    let md = format!("# Interactive scale change (check 6, real display move)\n\nScale changes seen: {}; first frame correct: {ok}.\n{}\n", app.scale_log.len(),
        if app.scale_log.is_empty() { "No scale change happened (one display, or the window never crossed to a display with another scale)." } else if ok == app.scale_log.len() { "Result: pass." } else { "Result: FAIL." });
    save(&app, "scale", J::obj(vec![("changes", J::N(app.scale_log.len() as f64)), ("first_frame_correct", J::N(ok as f64)), ("events", J::A(rows))]), md);
}
