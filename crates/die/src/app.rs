//! DIE.app: one instance, and a stub `restart` that K12 fills in. The new instance comes up
//! through LaunchServices, with `open -g`, so it takes no focus. Whether it's up is said over
//! a Unix socket (`die_core::instance`), since LaunchServices can't say that.

use crate::{Config, launch};
use die_core::instance::{Handover, Instance, Successor};
use die_core::sys::state_dir;
use die_render::window::Remote;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::{Duration, Instant};

/// How long the old instance waits for the new one's first frame.
const RESTART_LIMIT: Duration = Duration::from_secs(10);
/// Each restart's time in ms goes in this file in the state directory, a line each.
pub const RESTART_LOG: &str = "restart.log";

/// What a launch was asked to do.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct Launch {
    /// `--handover socket`: this is a restart, and the old instance listens there.
    pub handover: Option<PathBuf>,
    /// `--restart-after-frame`: run `restart` once the first frame is up.
    pub restart_after_frame: bool,
}

/// The app bundle this process runs from, if it does.
pub fn bundle() -> Option<PathBuf> {
    die_render::bundle_of(&std::env::current_exe().ok()?)
}

/// Run as DIE.app until the window closes or a restart hands over. If another instance has
/// the lock this one says so and exits, with nothing opened. The exit code is `die::run`'s.
pub fn run(mut config: Config, bundle: PathBuf, asked: Launch) -> i32 {
    let Some(dir) = state_dir() else {
        eprintln!("die: no place for DIE's state: HOME isn't set");
        return 2;
    };
    // An old instance that won't take the connection has gone, so this is a plain launch.
    let successor = asked.handover.and_then(|s| Successor::connect(&s).ok());
    let instance = match successor {
        Some(_) => None,
        None => match Instance::try_lock(&dir) {
            Ok(Some(instance)) => Some(instance),
            Ok(None) => {
                eprintln!("die: DIE is already running");
                return 0;
            }
            Err(e) => {
                eprintln!("die: {}: {e}", dir.display());
                return 2;
            }
        },
    };
    let (restart, wanted) = channel();
    if asked.restart_after_frame {
        let _ = restart.send(());
    }
    let register = config.core.register.take();
    config.core.register = Some(Box::new(move |reg| {
        reg.command("restart", "Start DIE again.", move |_, _| {
            let _ = restart.send(());
            Ok(())
        });
        if let Some(register) = register {
            register(reg);
        }
    }));
    let first_frame = Box::new(move |remote: &Remote| {
        let remote = remote.clone();
        thread::spawn(move || {
            let held = settle(successor, instance, &dir);
            serve(&wanted, &bundle, &dir, &remote);
            drop(held);
        });
    });
    launch(config, None, Some(first_frame))
}

/// The first frame is up, so end up holding the lock, after any old instance has gone.
fn settle(
    successor: Option<Successor>,
    instance: Option<Instance>,
    dir: &Path,
) -> Option<Instance> {
    let Some(successor) = successor else {
        return instance;
    };
    let locked = successor.first_frame().and_then(|()| Instance::lock(dir));
    locked
        .inspect_err(|e| eprintln!("die: taking over: {e}"))
        .ok()
}

/// Run each `restart` as it's asked for, until one takes. The handover stays open, and the
/// caller keeps the lock, until this process exits: the new instance waits for both.
fn serve(wanted: &Receiver<()>, bundle: &Path, dir: &Path, remote: &Remote) {
    for () in wanted {
        match restart(bundle, dir) {
            Ok((handover, took)) => {
                if let Err(e) = log_restart(dir, took) {
                    eprintln!("die: {RESTART_LOG}: {e}");
                }
                remote.exit(0);
                let _handover = handover;
                loop {
                    thread::park();
                }
            }
            Err(e) => eprintln!("die: restart: {e}"),
        }
    }
}

/// Launch the new instance and wait for its first frame, which is how long it took.
fn restart(bundle: &Path, dir: &Path) -> Result<(Handover, Duration), String> {
    let start = Instant::now();
    let mut handover = Handover::bind(dir).map_err(|e| e.to_string())?;
    let mut open = Command::new("open");
    open.args(["-g", "-n"]);
    if let Some(dir) = std::env::var_os("DIE_STATE_DIR") {
        let mut env = std::ffi::OsString::from("DIE_STATE_DIR=");
        env.push(dir);
        open.arg("--env").arg(env);
    }
    open.arg(bundle).args(["--args", "--handover"]);
    let status = open.arg(handover.path()).status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => return Err(format!("open: {status}")),
        Err(e) => return Err(format!("open: {e}")),
    }
    handover.wait(RESTART_LIMIT)?;
    Ok((handover, start.elapsed()))
}

fn log_restart(dir: &Path, took: Duration) -> io::Result<()> {
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(RESTART_LOG))?;
    writeln!(log, "{:.1}", took.as_secs_f64() * 1e3)
}
