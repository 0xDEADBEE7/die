//! CLI for the S1 spike. See README.md for every command.

use s1::app::{act, with_loop, App, Opts, WinStyle};
use s1::gpu::RenderOpts;
use s1::stats::{now_s, summarize, write_result, J};
use s1::{bench, fidelity, font, interactive};
use std::path::PathBuf;
use std::process::Command;

const SCRATCH_FONTS: &str = "/private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/s0/s1/downloads/term";

fn spike_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")) }

fn term_files(explicit: Option<&str>) -> Option<[PathBuf; 4]> {
    let mut dirs: Vec<PathBuf> = vec![];
    if let Some(d) = explicit { dirs.push(d.into()); }
    if let Ok(d) = std::env::var("S1_FONT_DIR") { dirs.push(d.into()); }
    dirs.push(spike_dir().join("fonts"));
    dirs.push(SCRATCH_FONTS.into());
    dirs.into_iter().map(|d| font::font_files(&d, "IoskeleyMonoTerm")).find(|f| f.iter().all(|p| p.exists()))
}

fn installed_files() -> [PathBuf; 4] {
    font::font_files(&PathBuf::from(std::env::var("HOME").unwrap()).join("Library/Fonts"), "IoskeleyMono")
}

fn arg_val(args: &[String], k: &str) -> Option<String> { args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned()) }

fn opts(args: &[String]) -> Opts {
    let font_arg = arg_val(args, "--font");
    let (font_files, font_label) = match font_arg.as_deref() {
        Some("installed") => (installed_files(), "Ioskeley Mono (installed, non-Term)".to_string()),
        other => match term_files(other.filter(|o| *o != "term")) {
            Some(f) => (f, "Ioskeley Mono Term".to_string()),
            None => { eprintln!("[s1] WARNING: Ioskeley Mono Term not found (run ./fetch-font.sh); using the installed non-Term font"); (installed_files(), "Ioskeley Mono (installed; Term missing)".into()) }
        },
    };
    let file = arg_val(args, "--file").map(PathBuf::from).unwrap_or_else(|| {
        // a large Rust source file: winit's macOS window delegate, from the cargo registry
        let reg = PathBuf::from(std::env::var("HOME").unwrap()).join(".cargo/registry/src");
        std::fs::read_dir(&reg).ok().and_then(|mut d| d.next()).and_then(|e| e.ok())
            .map(|e| e.path().join("winit-0.30.13/src/platform_impl/macos/window_delegate.rs"))
            .filter(|p| p.exists()).unwrap_or_else(|| spike_dir().join("src/app.rs"))
    });
    Opts {
        style: if arg_val(args, "--style").as_deref() == Some("titlebar") { WinStyle::Titlebar } else { WinStyle::Borderless },
        font_files, font_label, pt: 16.0,
        render: RenderOpts { sprites: !args.iter().any(|a| a == "--no-sprites"), braille_sprite: args.iter().any(|a| a == "--braille-sprite") },
        guard: !args.iter().any(|a| a == "--no-occlusion-guard"), fix_keys: !args.iter().any(|a| a == "--no-key-fixes"),
        display_sync: arg_val(args, "--display-sync").as_deref() != Some("off"), drawables: arg_val(args, "--drawables").and_then(|s| s.parse().ok()).unwrap_or(3), pix_layer: args.iter().any(|a| a == "--pix-layer"), pix_full: args.iter().any(|a| a == "--pix-full"), pwt: args.iter().any(|a| a == "--pwt"),
        reconcile: !args.iter().any(|a| a == "--no-reconcile"), flush: !args.iter().any(|a| a == "--no-flush"),
        frame: arg_val(args, "--frame").map(|f| { let v: Vec<f64> = f.split(',').map(|x| x.parse().unwrap()).collect(); [v[0], v[1], v[2], v[3]] }),
        split: !args.iter().any(|a| a == "--no-split"), fullscreen: args.iter().any(|a| a == "--fullscreen"), activate: true, file,
        keys_target: arg_val(args, "--keys").and_then(|s| s.parse().ok()).unwrap_or(500),
    }
}

fn bench_cmd(args: &[String], t_main: f64) {
    let all = ["keys", "clicks", "typing", "redraw", "producer", "scale", "resize", "idle", "occlusion"];
    let names: Vec<String> = args.iter().skip(2).take_while(|a| !a.starts_with("--")).cloned().collect();
    let names: Vec<String> = if names.is_empty() || names.iter().any(|n| n == "all") { all.iter().map(|s| s.to_string()).collect() } else { names };
    let quick = args.iter().any(|a| a == "--quick");
    let mut o = opts(args);
    o.fullscreen = !args.iter().any(|a| a == "--windowed");
    let app = with_loop(o, t_main, true, |a| {
        let mut script = vec![bench_wait(0.8)];
        for n in &names {
            match n.as_str() {
                "keys" => script.extend(bench::keymatrix()),
                "clicks" => script.extend(bench::clicks()),
                "typing" => { let k = if quick { 60 } else { a.opts.keys_target }; script.extend(bench::typing(k, false, "no_producer")); script.extend(bench::typing(k, true, "with_producer")); }
                "redraw" => script.extend(bench::redraw(if quick { 120 } else { 630 })),
                "producer" => script.extend(bench::producer(if quick { 5.0 } else { 60.0 })),
                "scale" => script.extend(bench::scale(5)),
                "resize" => script.extend(bench::resize(if quick { 2.0 } else { 10.0 }, arg_val(args, "--resize-hz").and_then(|s| s.parse().ok()).unwrap_or(120.0))),
                "idle" => script.extend(bench::idle(if quick { 5.0 } else { 60.0 })),
                "occlusion" => script.extend(bench::occlusion(if quick { 4 } else { 20 }, if quick { 1 } else { 5 })),
                other => panic!("unknown bench {other}"),
            }
        }
        script.push(act(|a, _| a.exit = true));
        a.script = script.into();
    });
    let label = arg_val(args, "--label").unwrap_or_else(|| names.join("-"));
    finish_bench(app, &label);
}

fn bench_wait(s: f64) -> s1::app::Act { s1::app::Act::Wait(s) }

fn finish_bench(mut app: App, label: &str) {
    s1::bench::write_frames_csv(&app, label);
    let gpu = app.gpu.as_ref().unwrap();
    let mut top = vec![("label".to_string(), J::s(label)), ("display_sync".into(), J::B(app.opts.display_sync)), ("drawables".into(), J::N(app.opts.drawables as f64)), ("pix_layer".into(), J::B(app.opts.pix_layer)), ("pix_full".into(), J::B(app.opts.pix_full)), ("fullscreen_sized".into(), J::B(app.opts.fullscreen)), ("presents_with_transaction".into(), J::B(app.opts.pwt)), ("frame".into(), J::s(format!("{:?}", app.opts.frame))), ("style".into(), J::s(app.opts.style_name())), ("font".into(), J::s(&app.opts.font_label)),
        ("drawable_px".into(), J::s(format!("{:?}", app.px))), ("grid".into(), J::s(format!("{}x{}", app.scene.grid.cols, app.scene.grid.rows))),
        ("cell".into(), J::s(format!("{:?}", gpu.cell()))), ("startup".into(), J::O(gpu.startup.iter().map(|(k, v)| (k.to_string(), J::N(*v))).collect())),
        ("counters".into(), J::obj(vec![("reconciled_before_resized", J::N(app.c.reconciled as f64)), ("rendered", J::N(app.c.rendered as f64)), ("skipped_occluded", J::N(app.c.skipped_occluded as f64)), ("skipped_inflight", J::N(app.c.skipped_inflight as f64)), ("no_drawable", J::N(app.c.no_drawable as f64))])),
        ("load_avg_at_end".into(), J::s(load_avg()))];
    top.append(&mut app.extra);
    write_result(&format!("bench-{label}"), &J::O(top));
}

fn load_avg() -> String {
    let mut l = [0f64; 3];
    unsafe { libc::getloadavg(l.as_mut_ptr(), 3) };
    format!("{:.2} {:.2} {:.2}", l[0], l[1], l[2])
}

fn firstframe_parent(args: &[String]) {
    let runs: usize = arg_val(args, "--runs").and_then(|s| s.parse().ok()).unwrap_or(12);
    let exe = std::env::current_exe().unwrap();
    let mut rows = vec![];
    let mut to_present = vec![];
    let mut to_commit = vec![];
    for i in 0..runs {
        let t = now_s();
        let out = Command::new(&exe).arg("firstframe-child").args(args.iter().skip(2)).env("S1_SPAWN_T", format!("{t:.9}")).output().unwrap();
        let line = String::from_utf8_lossy(&out.stdout).lines().find(|l| l.starts_with("FF ")).map(|l| l[3..].to_string()).unwrap_or_default();
        let v: Vec<f64> = line.split(',').filter_map(|s| s.parse().ok()).collect();
        eprintln!("[s1] firstframe run {i}: {line}");
        if v.len() >= 8 {
            let (spawn, main, win, gpu, commit, presented, nth) = (v[0], v[1], v[2], v[3], v[4], v[5], v[7]);
            if i > 0 { if presented > 0.0 { to_present.push((presented - spawn) * 1e3); } to_commit.push((commit - spawn) * 1e3); }
            rows.push(J::obj(vec![("run", J::N(i as f64)), ("warm", J::B(i > 0)), ("spawn_to_main_ms", J::N((main - spawn) * 1e3)), ("main_to_window_ms", J::N((win - main) * 1e3)),
                ("window_to_gpu_font_ready_ms", J::N((gpu - win) * 1e3)), ("spawn_to_commit_ms", J::N((commit - spawn) * 1e3)),
                ("spawn_to_presented_ms", if presented > 0.0 { J::N((presented - spawn) * 1e3) } else { J::Null }),
                ("first_presented_frame_index", J::N(nth)), ("startup_device_msl_font_ms", J::s(v[8..].iter().map(|x| format!("{x:.2}")).collect::<Vec<_>>().join(" ")))]));
        }
        std::thread::sleep(std::time::Duration::from_millis(700));
    }
    let sp = summarize(&to_present);
    eprintln!("[s1] firstframe warm spawn->presented {}", sp.line("ms"));
    let label = arg_val(args, "--label").map(|l| format!("firstframe-{l}")).unwrap_or_else(|| "firstframe".into());
    write_result(&label, &J::obj(vec![("runs", J::N(runs as f64)), ("note", J::s("run 0 is the first launch after a build (treated as cold-ish); warm = runs 1..")),
        ("warm_spawn_to_presented_ms", sp.json()), ("warm_spawn_to_commit_ms", summarize(&to_commit).json()), ("load_avg", J::s(load_avg())), ("rows", J::A(rows))]));
}

fn firstframe_child(args: &[String], t_main: f64) {
    let spawn: f64 = std::env::var("S1_SPAWN_T").ok().and_then(|s| s.parse().ok()).unwrap_or(t_main);
    let mut o = opts(args);
    o.split = false;
    o.activate = !args.iter().any(|a| a == "--no-activate");
    let activate = o.activate;
    with_loop(o, t_main, activate, move |a| {
        a.tick = Some(Box::new(move |a, _| {
            // keep drawing until a frame is actually on screen; report the first committed and first presented
            let frames = a.gpu.as_ref().map(|g| g.log.lock().unwrap().frames.clone()).unwrap_or_default();
            let timed_out = now_s() - a.t_main > 5.0;
            let first = frames.first().cloned();
            let shown = frames.iter().find(|f| f.presented > 0.0).cloned();
            let pending = frames.iter().any(|f| f.presented == 0.0);
            if let Some(f0) = first { if shown.is_some() || timed_out {
                let m = |k: &str| a.marks.iter().find(|(n, _)| *n == k).map(|x| x.1).unwrap_or(0.0);
                let st: Vec<String> = a.gpu.as_ref().unwrap().startup.iter().map(|x| format!("{:.3}", x.1)).collect();
                let (pt, pc, n) = shown.map(|f| (f.presented, f.t_commit, f.id)).unwrap_or((0.0, 0.0, 0));
                println!("FF {spawn:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{n},{}", a.t_main, m("window_created"), m("gpu_ready"), f0.t_commit, pt, pc, st.join(","));
                a.exit = true;
            } else if !pending { a.scene.grid.mark_all(); a.needs_render = true; } }
            if !a.exit { a.deadline = Some(now_s() + 0.002); }
        }));
    });
}

fn rebuild_cmd(args: &[String]) {
    let runs: usize = arg_val(args, "--runs").and_then(|s| s.parse().ok()).unwrap_or(5);
    let sk = spike_dir().join("skeleton");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let build = |profile: &str| -> f64 {
        let t = now_s();
        let mut c = Command::new(&cargo);
        c.current_dir(&sk).arg("build").arg("-q").arg("-p").arg("die-dev");
        if profile == "release" { c.arg("--release"); }
        let st = c.status().unwrap();
        assert!(st.success(), "skeleton build failed");
        (now_s() - t) * 1e3
    };
    let mut out = vec![];
    for profile in ["dev", "release"] {
        let warm = build(profile);
        for leaf in ["die-vim", "die-render"] {
            let f = sk.join("crates").join(leaf).join("src/lib.rs");
            let mut v = vec![];
            for i in 0..runs {
                // a real code edit, not a touch: add or remove a public function
                let src = std::fs::read_to_string(&f).unwrap();
                let edited = if i % 2 == 0 { format!("{src}pub fn s1_edit_{i}(x: usize) -> usize {{ x * {} + 1 }}\n", i + 3) } else { src.lines().filter(|l| !l.starts_with("pub fn s1_edit_")).collect::<Vec<_>>().join("\n") + "\n" };
                std::fs::write(&f, edited).unwrap();
                v.push(build(profile));
            }
            let s = summarize(&v);
            eprintln!("[s1] rebuild {profile} edit {leaf}: {}", s.line("ms"));
            out.push((format!("{profile}_{leaf}"), J::obj(vec![("samples_ms", J::nums(&v)), ("summary", s.json())])));
        }
        out.push((format!("{profile}_noop_ms"), J::N(warm)));
    }
    out.push(("load_avg".into(), J::s(load_avg())));
    write_result("rebuild", &J::O(out));
}

fn main() {
    let t_main = now_s();
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("view");
    match cmd {
        "view" => { with_loop(opts(&args), t_main, true, |a| a.set_status("view: type to insert, PageUp/PageDown scroll, Cmd-Q quits")); }
        "bench" => bench_cmd(&args, t_main),
        "interactive" => interactive::run(&args, opts(&args), t_main),
        "fidelity" => {
            let mut fonts = vec![];
            if let Some(f) = term_files(arg_val(&args, "--font").as_deref()) { fonts.push(("term".to_string(), f)); }
            fonts.push(("installed".to_string(), installed_files()));
            let j = fidelity::run(&fonts);
            write_result("fidelity", &j);
        }
        "firstframe" => firstframe_parent(&args),
        "firstframe-child" => firstframe_child(&args, t_main),
        "rebuild" => rebuild_cmd(&args),
        "clocks" => {
            // NSEvent timestamps are "system uptime"; presentedTime is CACurrentMediaTime's clock.
            let up = objc2_foundation::NSProcessInfo::processInfo().systemUptime();
            let ca = now_s();
            println!("systemUptime {up:.6}  CACurrentMediaTime {ca:.6}  diff {:.3} ms", (up - ca) * 1e3);
        }
        _ => eprintln!("usage: s1 [view|bench [names..]|interactive <typing|keys|occlusion|resize|scale>|fidelity|firstframe|rebuild] [options]; see README.md"),
    }
}
