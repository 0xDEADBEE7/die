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
//!
//! **Client or instance.** LaunchServices starts every instance, so a run whose parent is launchd
//! (pid 1) is one, whether from the Dock, Finder, `open` or a restart. Any other run came from a
//! terminal and is a client: it sends its files to the instance that holds the lock, or starts
//! one through `open` with those files, and exits, so that no window lives as a terminal's child.
//! A launch that finds the lock held sends on any files it was given, and otherwise goes.

use crate::{Config, launch, rebuild, started_dir};
use die_core::instance::{
    Handover, Instance, OPEN_SOCKET, OpenListener, OpenRequest, Sent, Successor, send_open,
};
use die_core::sys::state_dir;
use die_core::{Ctx, FileTarget};
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
/// How long a new instance gives a client that's only looking at the lock.
const LOCK_LOOK: Duration = Duration::from_millis(20);
/// How long `die file` keeps trying while an instance starts or hands over.
const SEND_PATIENCE: Duration = Duration::from_secs(2);
/// How long `die file` waits to try `open` again, which LaunchServices may refuse for a moment.
const OPEN_AGAIN: Duration = Duration::from_millis(100);
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
    /// `--background`: open the files without coming to the front.
    pub background: bool,
}

/// The app bundle this process runs from, if it does, seen through a link such as
/// `~/.local/bin/die`.
pub fn bundle() -> Option<PathBuf> {
    die_render::bundle_of(&std::env::current_exe().ok()?.canonicalize().ok()?)
}

/// Whether this run came from a terminal rather than LaunchServices, which makes it a client.
/// A restart's new instance always says what it is.
pub fn is_client(asked: &Launch) -> bool {
    asked.handover.is_none() && std::os::unix::process::parent_id() != 1
}

/// `die file...` from a terminal: `files` go to the running instance, or a new one is started
/// with them. The exit code is 0, 1 if nobody answered, or 2 if the names make no sense.
pub fn client(bundle: &Path, files: Vec<PathBuf>, background: bool) -> i32 {
    let Some(dir) = state_dir() else {
        eprintln!("die: no place for DIE's state: HOME isn't set");
        return 2;
    };
    let names = files.into_iter().map(PathBuf::into_os_string);
    let targets = match FileTarget::parse_args(names, &started_dir()) {
        Ok(targets) => targets,
        Err(e) => {
            eprintln!("die: {e}");
            return 2;
        }
    };
    let request = OpenRequest {
        front: !background,
        targets,
    };
    let deadline = Instant::now() + SEND_PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match send_open(&dir, &request, left) {
            Ok(Sent::Delivered) => return 0,
            Ok(Sent::NoInstance) => match start_instance(bundle, &request) {
                Ok(()) => return 0,
                Err(e) if left.is_zero() => {
                    eprintln!("die: {e}");
                    return 1;
                }
                Err(_) => thread::sleep(OPEN_AGAIN),
            },
            Err(e) => {
                eprintln!("die: {e}");
                return 1;
            }
        }
    }
}

/// Start an instance with these files. `-n` makes it run even if one has just appeared, which
/// `open` alone would only bring forward with the files lost. That one finds the lock held and
/// sends them on.
fn start_instance(bundle: &Path, request: &OpenRequest) -> Result<(), String> {
    let mut open = open_bundle(bundle, request.front);
    if !request.front {
        open.arg("--background");
    }
    open.arg("--");
    let args = request.targets.iter().flat_map(FileTarget::to_args);
    let out = open.args(args).output().map_err(|e| format!("open: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let why = String::from_utf8_lossy(&out.stderr);
        Err(format!("open: {}", why.trim()))
    }
}

/// Run as DIE.app until the window closes or a restart hands over. If another instance has the
/// lock, this one sends it any files and exits, or says so and exits if it has none. The exit
/// code is `die::run`'s, or 1 if an instance holds the lock and nobody answered.
pub fn run(mut config: Config, bundle: PathBuf, asked: Launch) -> i32 {
    let Some(dir) = state_dir() else {
        eprintln!("die: no place for DIE's state: HOME isn't set");
        return 2;
    };
    let names = config.files.drain(..).map(PathBuf::into_os_string);
    let targets = match FileTarget::parse_args(names, &started_dir()) {
        Ok(targets) => targets,
        Err(e) => {
            eprintln!("die: {e}");
            return 2;
        }
    };
    // An old instance that won't take the connection has gone, so this is a plain launch.
    let successor = asked.handover.and_then(|s| Successor::connect(&s).ok());
    if successor.is_some() && std::env::var_os(PANIC_ON_RESTART).is_some() {
        panic!("{PANIC_ON_RESTART} is set");
    }
    let instance = match successor {
        Some(_) => None,
        None => match Instance::take_over(&dir, LOCK_LOOK) {
            Ok(Some(instance)) => Some(instance),
            Ok(None) if targets.is_empty() => {
                eprintln!("die: DIE is already running");
                return 0;
            }
            Ok(None) => {
                let request = OpenRequest {
                    front: !asked.background,
                    targets,
                };
                return match send_open(&dir, &request, SEND_PATIENCE) {
                    Ok(Sent::Delivered) => 0,
                    Ok(Sent::NoInstance) => {
                        eprintln!("die: the running DIE went before it could open these");
                        1
                    }
                    Err(e) => {
                        eprintln!("die: {e}");
                        1
                    }
                };
            }
            Err(e) => {
                eprintln!("die: {}: {e}", dir.display());
                return 2;
            }
        },
    };
    config.core.session = Some(dir.clone());
    // `start` puts them in the first window, so that one file has no frame of something else.
    config.files = targets.iter().map(|t| t.path.clone()).collect();
    let (restart, wanted) = channel();
    if asked.restart_after_frame {
        let _ = restart.send(());
    }
    let again = asked.restart_loop.then(|| restart.clone());
    let register = config.core.register.take();
    let rebuilt = restart.clone();
    let (installed, last) = (bundle.clone(), dir.join("rebuild-root"));
    config.core.register = Some(Box::new(move |reg| {
        reg.command("restart", "Start DIE again.", move |_, _| {
            let _ = restart.send(());
            Ok(())
        });
        let home = rebuild::home_of(&installed);
        let command = move |root: &Path| match &home {
            Some(home) => Ok(rebuild::xtask_app(root, home)),
            None => Err(die_core::Error::Msg(format!(
                "This DIE runs from {}, which `cargo xtask app` doesn't install",
                installed.display()
            ))),
        };
        let restart = Box::new(move || {
            let _ = rebuilt.send(());
        });
        let command = Box::new(command);
        rebuild::register(
            reg,
            rebuild::Setup {
                command,
                restart,
                last,
            },
        );
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
                // Launched in front, from the Dock or Finder, it gets the app but not the keyboard,
                // and the app may come just after this frame.
                if frontmost {
                    shell.take_focus();
                } else {
                    shell.take_focus_when_active();
                }
                shell.ctx().resume_session();
                open_targets(shell.ctx(), &targets);
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

/// Open each as `:e` would. One that won't open is logged, as at startup, and the rest go on.
fn open_targets(ctx: &mut Ctx, targets: &[FileTarget]) {
    for target in targets {
        if let Err(e) = ctx.open_target(target) {
            ctx.messages.push(e.to_string());
        }
    }
}

/// Listen for `die file`. Without the socket this instance still works, so that's only a message.
fn listen(dir: &Path, remote: &Remote) -> Option<OpenListener> {
    let caller = remote.clone();
    let opened = OpenListener::start(dir, move |request| {
        let done = caller.call(move |shell, _| {
            open_targets(shell.ctx(), &request.targets);
            if request.front {
                shell.come_to_front();
            }
        });
        done.is_ok()
    });
    match opened {
        Ok(listener) => Some(listener),
        Err(e) => {
            let e = format!("{}: {e}", dir.join(OPEN_SOCKET).display());
            eprintln!("die: {e}");
            let _ = remote.call(move |shell, _| shell.ctx().messages.push(e));
            None
        }
    }
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
/// caller keeps the lock, until this process exits: the new instance waits for both. `die file`
/// is listened for all along, except while a restart is under way: it waits for the new instance,
/// or for this one again if that fails.
fn serve(wanted: &Receiver<()>, bundle: &Path, dir: &Path, remote: &Remote, restart_loop: bool) {
    let mut listener = listen(dir, remote);
    for () in wanted {
        drop(listener.take());
        // Text that can't be saved stays in this instance, which keeps running.
        let saved =
            remote.call(|shell, _| shell.ctx().hand_over_session().map_err(|e| e.to_string()));
        match saved {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                report(remote, e);
                listener = listen(dir, remote);
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
                listener = listen(dir, remote);
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
    let mut open = open_bundle(bundle, frontmost);
    open.arg("--handover");
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

/// `open` for a new instance of the bundle, as far as `--args`.
fn open_bundle(bundle: &Path, frontmost: bool) -> Command {
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
    open.arg(bundle).arg("--args");
    open
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
