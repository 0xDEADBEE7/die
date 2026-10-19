//! Spike S3: `die --fd-store` keeps PTY masters open across DIE restarts and crashes.
//! One binary, several modes: the store, a fake DIE host, and the checks that drive them.

mod checks;
mod client;
mod host;
mod json;
mod modes;
mod probe;
mod proto;
mod pty;
mod store;

use modes::Args;
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let a = Args(std::env::args().collect());
    let secs = |k: &str, d: f64| Duration::from_secs_f64(a.get(k).and_then(|v| v.parse().ok()).unwrap_or(d));
    let code = match a.0.get(1).map(|s| s.as_str()) {
        Some("--fd-store") => store::main(store::Opts {
            dir: PathBuf::from(a.get("--dir").expect("--dir")),
            idle_exit: secs("--idle-exit", 300.0),
            sweep: secs("--sweep", 5.0),
            daemon: a.has("--daemon"),
            takeover: a.has("--takeover"),
            expect_uid: a.get("--expect-uid").and_then(|v| v.parse().ok()),
        }),
        Some("host") => {
            let dir = PathBuf::from(a.get("--dir").expect("--dir"));
            std::env::set_current_dir(&dir).expect("chdir"); // socket paths are relative (104-byte limit)
            let ctx = host::Ctx { dir, run: a.get("--run").unwrap_or("x").into(), exe: std::env::current_exe().unwrap() };
            let r = match a.0[2].as_str() {
                "chain" => modes::chain(&ctx, &a),
                "serve" => modes::serve(&ctx, &a),
                "cycles" => modes::cycles(&ctx, &a),
                "spawn-only" => modes::spawn_only(&ctx, &a),
                "exit-detect" => modes::exit_detect(&ctx, &a),
                "ver" => modes::ver(&ctx, &a),
                "kill-all" => modes::kill_all(&ctx, &a),
                "helper" => modes::helper(&ctx, &a),
                "helper-client" => modes::helper_client(&ctx, &a),
                "list" => modes::list(&ctx, &a),
                "app" => modes::app(&ctx, &a),
                "flood-spawn" => modes::flood_spawn(&ctx, &a),
                "flood-attach" => modes::flood_attach(&ctx, &a),
                m => Err(format!("unknown host mode {m}")),
            };
            match r {
                Ok(()) => 0,
                Err(x) => {
                    eprintln!("host {}: {x}", a.0[2]);
                    1
                }
            }
        }
        Some("check") => match checks::run(&a.0[2], &a) {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(x) => {
                eprintln!("check: {x}");
                1
            }
        },
        Some("probe") => {
            probe::run(std::path::Path::new(&a.0[2]));
            0
        }
        Some("ctty") => {
            probe::ctty(std::path::Path::new(&a.0[2]));
            0
        }
        Some("race") => {
            probe::race(std::path::Path::new(&a.0[2]));
            0
        }
        Some("flood") => {
            probe::flood(std::path::Path::new(&a.0[2]));
            0
        }
        _ => {
            eprintln!("usage: die --fd-store --dir D [--daemon] [--idle-exit S] [--sweep S] [--takeover]\n       die host MODE --dir D ...\n       die check NAME|all --base D [--n1 BIN --m2 BIN --app-dir D]\n       die probe D");
            2
        }
    };
    std::process::exit(code);
}
