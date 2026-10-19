//! `s2`: one binary, several modes. `--term-helper` is the helper process (DIE's own binary
//! in a second mode); `gen` runs a workload generator on a PTY; `bench` and the demos act as
//! DIE's main thread.
use s2_term_engine::{bench, demo, engine::Cfg, helper, workgen};
use std::collections::HashMap;

fn opts(args: &[String]) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(k) = args[i].strip_prefix("--") {
            if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                m.insert(k.to_string(), args[i + 1].clone());
                i += 2;
                continue;
            }
            m.insert(k.to_string(), "1".to_string());
        }
        i += 1;
    }
    m
}

fn chdir_run(o: &HashMap<String, String>) -> String {
    let dir = o.get("run-dir").cloned().unwrap_or_else(|| ".".into());
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::env::current_dir().unwrap().to_string_lossy().into_owned()
}

fn main() {
    s2_term_engine::touch_frameworks();
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("");
    match mode {
        "--term-helper" => {
            let dash = args.iter().position(|a| a == "--").unwrap_or(args.len());
            let o = opts(&args[2..dash]);
            let n = |k: &str, d: u32| o.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
            helper::helper_main(helper::HelperArgs {
                name: o.get("name").cloned().unwrap_or_else(|| "t".into()),
                cfg: Cfg { cols: n("cols", 80) as u16, rows: n("rows", 24) as u16, cell_w: n("cw", 18), cell_h: n("ch", 36) },
                inherited_fd: o.get("fd").and_then(|v| v.parse().ok()),
                exit_on_detach: o.contains_key("exit-on-detach"),
                adopt: o.get("adopt-fd").and_then(|v| v.parse().ok()).map(|fd| (fd, n("child", 0) as i32)),
                argv: args[(dash + 1).min(args.len())..].to_vec(),
            });
        }
        "gen" => {
            let what = args.get(2).map(String::as_str).unwrap_or("");
            let o = opts(&args[3.min(args.len())..]);
            let g = |k: &str, d: &str| o.get(k).cloned().unwrap_or_else(|| d.into());
            match what {
                "w2" => workgen::w2(g("bytes", "209715200").parse().unwrap()),
                "w3" => workgen::w3(
                    g("frames", "1000").parse().unwrap(),
                    g("fps", "60").parse().unwrap(),
                    g("cols", "250").parse().unwrap(),
                    g("rows", "70").parse().unwrap(),
                ),
                "w4" => workgen::w4(&g("kind", "rgba4k"), 1),
                "lines" => workgen::lines(args[3].parse().unwrap(), args.get(4).map_or("x", |s| s.as_str())),
                "img-small" => workgen::img_small(args[3].parse().unwrap()),
                w => panic!("unknown generator {w}"),
            }
        }
        "bench" => {
            let o = opts(&args[2..]);
            let run_dir = chdir_run(&o);
            let shape = o.get("shape").cloned().unwrap_or_else(|| "inproc".into());
            let wl = o.get("workload").cloned().unwrap_or_else(|| "w1".into());
            let line = bench::run(&shape, &wl, &run_dir, &o);
            println!("{line}");
            if let Some(out) = o.get("out") {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(out).unwrap();
                writeln!(f, "{line}").unwrap();
            }
        }
        "feed" => {
            // Debug: replay a captured byte stream into one Engine and print what it produced.
            let data = std::fs::read(&args[2]).unwrap();
            let mut e = s2_term_engine::engine::Engine::new(Cfg { cols: 250, rows: 70, cell_w: 18, cell_h: 36 });
            for c in data.chunks(65536) {
                e.write(c);
            }
            let fx = std::mem::take(&mut e.out.borrow_mut().effects);
            println!("effects={fx:?} held={} title={:?}", e.held, e.term.title());
        }
        "demo-kill" => {
            let o = opts(&args[2..]);
            let d = chdir_run(&o);
            std::process::exit(if demo::kill_helper(&d) { 0 } else { 1 });
        }
        "demo-version" => {
            let o = opts(&args[2..]);
            let d = chdir_run(&o);
            std::process::exit(if demo::version(&d) { 0 } else { 1 });
        }
        "session" => {
            let o = opts(&args[2..]);
            let d = chdir_run(&o);
            if o.contains_key("phase1") {
                demo::session_phase1(&d);
            } else {
                std::process::exit(if demo::session_phase2(&d, !o.contains_key("keep")) { 0 } else { 1 });
            }
        }
        _ => {
            eprintln!(
                "usage: s2 bench --shape inproc|helper --workload w1|w2|w3|w4|w5|rss|scrollback --run-dir D [--out F]\n       s2 demo-kill|demo-version --run-dir D\n       s2 session --run-dir D --phase1|--phase2\n       s2 gen w2|w3|w4|lines|img-small ...\n       s2 --term-helper --name N ... -- argv"
            );
            std::process::exit(2);
        }
    }
}
