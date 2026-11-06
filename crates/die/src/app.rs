//! DIE.app: one instance, its session, and `restart`. The new instance comes up through
//! LaunchServices, with `open -g` so that it takes no focus, unless the old one was the active app,
//! when it comes up in front and takes the keyboard at its first frame, as a restart should feel
//! like nothing happened. Whether it's up is said over a Unix socket (`die_core::instance`), since
//! LaunchServices can't say that.
//!
//! A restart writes the session and stops writing, starts the new instance, which restores what
//! was written, and exits at its first frame. If the new one dies or takes too long, the old
//! one writes again and carries on, and a new one that was only slow finds the lock held and
//! goes. Only the lock's holder writes the session.

use crate::{Config, launch};
use die_core::instance::{Handover, Instance, Successor};
use die_core::sys::state_dir;
use die_render::window::Remote;
use std::fs::{self, OpenOptions};
use std::hash::{DefaultHasher, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

/// How long the old instance waits for the new one's first frame.
const RESTART_LIMIT: Duration = Duration::from_secs(10);
/// How long the old instance's lock may outlast its handover.
const LOCK_PATIENCE: Duration = Duration::from_secs(2);
/// Set, a restart's new instance panics before its first frame: how a test gets a bad build.
pub const PANIC_ON_RESTART: &str = "DIE_PANIC_ON_RESTART";
/// Each restart's time in ms goes in this file in the state directory, a line each.
pub const RESTART_LOG: &str = "restart.log";
/// Under `--restart-loop`, each restore's session digest goes in this file, a line each, and the
/// session it's a digest of beside it, as `restart-loop-<digest>.json`.
pub const RESTART_LOOP_DIGESTS: &str = "restart-loop.digests";
/// Under `--restart-loop`, this file appearing in the state directory says to restart now.
pub const RESTART_LOOP_GO: &str = "restart-loop.go";
/// How often `--restart-loop` looks for its go-ahead.
const LOOK_FOR_GO: Duration = Duration::from_millis(5);

/// What a launch was asked to do.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct Launch {
    /// `--handover socket`: this is a restart, and the old instance listens there.
    pub handover: Option<PathBuf>,
    /// `--restart-after-frame`: run `restart` once the first frame is up.
    pub restart_after_frame: bool,
    /// `--frontmost`: the old instance had the keyboard, so this one takes it at its first frame.
    pub frontmost: bool,
    /// `--restart-loop`: write a digest of the restored session at the first frame, restart at
    /// the go-ahead, and pass this on. `cargo xtask restart-loop` drives it.
    pub restart_loop: bool,
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
    if successor.is_some() && std::env::var_os(PANIC_ON_RESTART).is_some() {
        panic!("{PANIC_ON_RESTART} is set");
    }
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
    config.core.session = Some(dir.clone());
    let (restart, wanted) = channel();
    if asked.restart_after_frame {
        let _ = restart.send(());
    }
    let again = asked.restart_loop.then(|| restart.clone());
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
    let frontmost = asked.frontmost;
    let first_frame = Box::new(move |remote: &Remote| {
        let remote = remote.clone();
        thread::spawn(move || {
            let Some(held) = settle(successor, instance, &dir) else {
                remote.exit(0);
                return;
            };
            let _ = remote.call(move |shell, _| {
                if frontmost {
                    shell.take_focus();
                }
                shell.ctx().resume_session();
            });
            if let Some(again) = again {
                digest_then_wait_for_go(&remote, &dir, again);
            }
            serve(&wanted, &bundle, &dir, &remote, asked.restart_loop);
            drop(held);
        });
    });
    launch(config, None, Some(first_frame))
}

/// The first frame is up, so end up holding the lock, after any old instance has gone. None if
/// an old instance kept it, as it does when it gave up on this one, or a third took it.
fn settle(
    successor: Option<Successor>,
    instance: Option<Instance>,
    dir: &Path,
) -> Option<Instance> {
    let Some(successor) = successor else {
        return instance;
    };
    // However this ends, the handover has closed, and the lock says what that means.
    let _ = successor.first_frame();
    match Instance::take_over(dir, LOCK_PATIENCE) {
        Ok(Some(held)) => Some(held),
        Ok(None) => {
            eprintln!("die: another instance has the lock, so this one goes");
            None
        }
        Err(e) => {
            eprintln!("die: taking over: {e}");
            None
        }
    }
}

/// One turn of `--restart-loop`. It's past a frame, as scroll follows the cursor at draw.
fn digest_then_wait_for_go(remote: &Remote, dir: &Path, again: Sender<()>) {
    let Ok(session) = remote.call(|shell, _| shell.ctx().session_json()) else {
        return;
    };
    if let Err(e) = write_digest(dir, &session) {
        report(remote, format!("{RESTART_LOOP_DIGESTS}: {e}"));
    }
    let go = dir.join(RESTART_LOOP_GO);
    thread::spawn(move || {
        while fs::remove_file(&go).is_err() {
            thread::sleep(LOOK_FOR_GO);
        }
        let _ = again.send(());
    });
}

fn write_digest(dir: &Path, session: &str) -> io::Result<()> {
    // Its keys are fixed, so builds from one toolchain agree.
    let mut hasher = DefaultHasher::new();
    hasher.write(session.as_bytes());
    let digest = format!("{:016x}", hasher.finish());
    fs::write(dir.join(format!("restart-loop-{digest}.json")), session)?;
    append_line(&dir.join(RESTART_LOOP_DIGESTS), &digest)
}

/// Run each `restart` as it's asked for, until one takes. The handover stays open, and the
/// caller keeps the lock, until this process exits: the new instance waits for both.
fn serve(wanted: &Receiver<()>, bundle: &Path, dir: &Path, remote: &Remote, restart_loop: bool) {
    for () in wanted {
        // Text that can't be saved stays in this instance, which keeps running.
        let saved =
            remote.call(|shell, _| shell.ctx().hand_over_session().map_err(|e| e.to_string()));
        match saved {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                report(remote, e);
                continue;
            }
            Err(_) => return,
        }
        let frontmost = remote.call(|shell, _| shell.app_active()).unwrap_or(false);
        match restart(bundle, dir, frontmost, restart_loop) {
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
            Err(e) => {
                let _ = remote.call(move |shell, _| {
                    // In front, the new instance took the focus as it started, and it's gone.
                    if frontmost {
                        shell.take_focus();
                    }
                    shell.ctx().resume_session();
                });
                report(remote, e);
            }
        }
    }
}

/// A restart didn't happen: say why on stderr and in `*messages*`.
fn report(remote: &Remote, e: String) {
    eprintln!("die: restart: {e}");
    let _ = remote.call(move |shell, _| shell.ctx().messages.push(format!("restart: {e}")));
}

/// Launch the new instance and wait for its first frame, which is how long it took.
fn restart(
    bundle: &Path,
    dir: &Path,
    frontmost: bool,
    restart_loop: bool,
) -> Result<(Handover, Duration), String> {
    let start = Instant::now();
    let mut handover = Handover::bind(dir).map_err(|e| e.to_string())?;
    let mut open = Command::new("open");
    open.args(open_options(frontmost));
    for name in ["DIE_STATE_DIR", PANIC_ON_RESTART] {
        if let Some(value) = std::env::var_os(name) {
            let mut env = std::ffi::OsString::from(name);
            env.push("=");
            env.push(value);
            open.arg("--env").arg(env);
        }
    }
    open.arg(bundle).args(["--args", "--handover"]);
    open.arg(handover.path());
    if frontmost {
        open.arg("--frontmost");
    }
    if restart_loop {
        open.arg("--restart-loop");
    }
    let status = open.status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => return Err(format!("open: {status}")),
        Err(e) => return Err(format!("open: {e}")),
    }
    handover.wait(RESTART_LIMIT)?;
    Ok((handover, start.elapsed()))
}

/// How `open` starts the new instance.
fn open_options(frontmost: bool) -> &'static [&'static str] {
    if frontmost { &["-n"] } else { &["-g", "-n"] }
}

fn log_restart(dir: &Path, took: Duration) -> io::Result<()> {
    let line = format!("{:.1}", took.as_secs_f64() * 1e3);
    append_line(&dir.join(RESTART_LOG), &line)
}

fn append_line(path: &Path, line: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

#[cfg(test)]
mod tests {
    use super::open_options;

    /// A restart keeps the focus DIE had, and takes none it didn't have.
    #[test]
    fn only_a_frontmost_instance_starts_the_new_one_in_front() {
        assert_eq!(open_options(false), ["-g", "-n"]);
        assert_eq!(open_options(true), ["-n"]);
    }
}
