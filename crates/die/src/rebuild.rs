//! `rebuild`: `cargo xtask app` from inside DIE, with what cargo says in `*rebuild*`, then a
//! restart into the new build if it built. A failed build leaves this DIE running and installs
//! nothing, since `xtask app` puts a bundle in place only once it's whole, and a new build that
//! dies at its start leaves this one running too, as any `restart` does.

use die_core::{BufferId, Ctx, Error, Origin, Registry, ShowIn, Side, Sub, TextBuf, WindowId};
use std::cell::{Cell, RefCell};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// The buffer cargo's output goes in.
pub const REBUILD: &str = "*rebuild*";
/// How often what cargo has said so far goes into the buffer.
const SHOW_EVERY: Duration = Duration::from_millis(100);
/// How long, once cargo has exited, what it wrote last has to come out of the pipe. A process
/// it left behind holding the pipe isn't waited for.
const DRAIN: Duration = Duration::from_millis(500);

/// The workspace `rebuild` builds: the DIE checkout `dir` is in, so that editing a checkout
/// builds that checkout, or else the one named in `last`, the last one built.
pub fn root_for(dir: Option<&Path>, last: &Path) -> Option<PathBuf> {
    let here = dir.map(die_core::project_root).filter(|r| is_die(r));
    let kept = || {
        let kept = std::fs::read_to_string(last).ok()?;
        Some(PathBuf::from(kept.trim_end_matches('\n')))
    };
    here.or_else(|| kept().filter(|r| is_die(r)))
}

/// Whether `root` is DIE's workspace, so that `cargo xtask app` there is the one meant.
fn is_die(root: &Path) -> bool {
    root.join("crates/die/Cargo.toml").is_file() && root.join("xtask").is_dir()
}

/// The home `cargo xtask app --home` replaces `bundle` under, if it's an `Applications/DIE.app`.
pub fn home_of(bundle: &Path) -> Option<PathBuf> {
    let apps = bundle.parent().filter(|_| bundle.ends_with("DIE.app"))?;
    let home = apps.parent().filter(|_| apps.ends_with("Applications"))?;
    Some(home.to_path_buf())
}

/// `cargo xtask app --home HOME` in `root`, which replaces `HOME/Applications/DIE.app`. A DIE
/// started from the Dock has launchd's PATH, which hasn't got rustup's `~/.cargo/bin`, so that
/// goes first.
pub fn xtask_app(root: &Path, home: &Path) -> Command {
    let bin = std::env::home_dir().map(|h| h.join(".cargo/bin"));
    let cargo = bin.as_ref().map(|b| b.join("cargo")).filter(|c| c.exists());
    let mut cmd = Command::new(cargo.unwrap_or_else(|| "cargo".into()));
    if let Some(bin) = bin {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let dirs = std::iter::once(bin).chain(std::env::split_paths(&path));
        if let Ok(path) = std::env::join_paths(dirs) {
            cmd.env("PATH", path);
        }
    }
    cmd.current_dir(root)
        .args(["xtask", "app", "--home"])
        .arg(home);
    cmd
}

/// The command for a workspace, or why there's none.
pub type CommandFn = Box<dyn Fn(&Path) -> die_core::Result<Command>>;

/// What `rebuild` needs from the instance it runs in.
pub struct Setup {
    /// The command for a workspace, or why there's none.
    pub command: CommandFn,
    /// Ask for a restart.
    pub restart: Box<dyn Fn()>,
    /// The file the last workspace built is kept in.
    pub last: PathBuf,
}

/// What lasts from one rebuild to the next.
#[derive(Default)]
struct Shared {
    running: Cell<bool>,
    /// The window a rebuild opened for the output, closed when a build works.
    opened: Cell<Option<WindowId>>,
}

/// One build's output so far, and the timer that shows it.
struct Run {
    buf: BufferId,
    said: Arc<Mutex<Vec<u8>>>,
    tick: Option<Sub>,
}

/// Register `rebuild`.
pub fn register(reg: &mut Registry, setup: Setup) {
    let (setup, shared) = (Rc::new(setup), Rc::new(Shared::default()));
    let doc = "Build DIE.app again with `cargo xtask app`, in the DIE checkout this buffer is in or \
               else the last one built, and restart into it if it builds";
    reg.command("rebuild", doc, move |ctx, _| {
        if shared.running.get() {
            return Err(Error::Msg("A rebuild is already running".into()));
        }
        let dir = ctx.focused_buffer().and_then(|b| ctx.directory(b));
        let Some(root) = root_for(dir, &setup.last) else {
            let why = "Open a file in a DIE checkout first, since none has been rebuilt before";
            return Err(Error::Msg(why.into()));
        };
        let cmd = (setup.command)(&root)?;
        let _ = std::fs::write(&setup.last, format!("{}\n", root.display()));
        let buf = output_buffer(ctx, &root, &cmd)?;
        show(ctx, buf, &shared.opened);
        let said = Arc::new(Mutex::new(Vec::new()));
        let shown = said.clone();
        let tick = ctx.every(SHOW_EVERY, move |ctx| {
            append(ctx, buf, &take(&shown, false));
            Ok(())
        });
        let run = RefCell::new(Run {
            buf,
            said: said.clone(),
            tick: Some(tick),
        });
        shared.running.set(true);
        let (setup, shared) = (setup.clone(), shared.clone());
        let job = ctx.spawn(
            move || build(cmd, said),
            move |ctx, status| ended(ctx, &mut run.borrow_mut(), &shared, status, &*setup.restart),
        );
        job.detach();
        ctx.messages
            .push(format!("Rebuilding in {}", root.display()));
        Ok(())
    });
}

/// `*rebuild*`, emptied, with what runs where at its top, and file names in cargo's errors
/// taken from `root`, so that `gf` opens them.
fn output_buffer(ctx: &mut Ctx, root: &Path, cmd: &Command) -> die_core::Result<BufferId> {
    let buf = match ctx.find_buffer(REBUILD) {
        Some(buf) => buf,
        None => ctx.create_text(REBUILD, ""),
    };
    let t = ctx.text_mut(buf)?;
    // Each time, since one a session brought back has your text defaults.
    let settings = t.settings_mut();
    settings.read_only = true;
    settings.max_width = None;
    settings.center = false;
    let all = 0..t.len();
    let header = format!("{} in {}\n\n", shown(cmd), root.display());
    t.tx(Origin::Extension)
        .silent()
        .replace(all, header)
        .commit()?;
    t.set_modified(false);
    ctx.set_directory(buf, root);
    let end = last_line_start(ctx.text(buf)?);
    for w in showing(ctx, buf) {
        ctx.text_mut(buf)?.set_cursor(w, end);
    }
    Ok(buf)
}

/// The command as a terminal would show it.
fn shown(cmd: &Command) -> String {
    let program = Path::new(cmd.get_program()).file_name().unwrap_or_default();
    let words = std::iter::once(program).chain(cmd.get_args());
    let words: Vec<_> = words.map(|w| w.to_string_lossy()).collect();
    words.join(" ")
}

fn showing(ctx: &Ctx, buf: BufferId) -> Vec<WindowId> {
    let all = ctx.window_list().into_iter();
    all.filter(|w| ctx.window_buffer(*w) == Some(buf)).collect()
}

/// Show the output below, with the focus staying, unless a window has it already. A window
/// opened for it is kept in `opened`, across rebuilds while it shows the output.
fn show(ctx: &mut Ctx, buf: BufferId, opened: &Cell<Option<WindowId>>) {
    opened.set(opened.get().filter(|w| ctx.window_buffer(*w) == Some(buf)));
    if !showing(ctx, buf).is_empty() {
        return;
    }
    let here = ctx.focused();
    // Too small to split, the messages still say how it went.
    if let Ok(Some(w)) = ctx.show(buf, ShowIn::Split(Side::Below)) {
        opened.set(Some(w));
        if let Ok(t) = ctx.text_mut(buf) {
            let end = last_line_start(t);
            t.set_cursor(w, end);
        }
    }
    if let Some(w) = here {
        ctx.focus(w);
    }
}

/// Run `cmd` with its output and errors together in `said`, as a terminal would show them, until
/// it exits.
fn build(mut cmd: Command, said: Arc<Mutex<Vec<u8>>>) -> io::Result<ExitStatus> {
    let (mut reader, writer) = io::pipe()?;
    cmd.stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer);
    let spawned = cmd.spawn();
    let dir = cmd.get_current_dir().map(Path::to_path_buf);
    let what = format!("{} in {}", shown(&cmd), dir.unwrap_or_default().display());
    drop(cmd); // its copies of the pipe's writing end, or the reading never ends
    let mut child = spawned.map_err(|e| io::Error::new(e.kind(), format!("{what}: {e}")))?;
    let (read_all, all_read) = mpsc::channel();
    std::thread::spawn(move || {
        let mut chunk = [0; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => said
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .extend(&chunk[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = read_all.send(());
    });
    let status = child.wait();
    let _ = all_read.recv_timeout(DRAIN);
    status
}

/// What `said` has whole lines of, or with `all`, everything.
fn take(said: &Mutex<Vec<u8>>, all: bool) -> String {
    let mut said = said.lock().unwrap_or_else(|e| e.into_inner());
    let end = match all {
        true => said.len(),
        false => said
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |at| at + 1),
    };
    let taken: Vec<u8> = said.drain(..end).collect();
    String::from_utf8_lossy(&taken).into_owned()
}

/// Where the last line starts, not counting the empty one after a final newline.
fn last_line_start(t: &TextBuf) -> usize {
    let lines = t.line_count();
    let ends_in_newline = !t.is_empty() && t.line_start(lines - 1) == t.len();
    t.line_start(lines - 1 - usize::from(ends_in_newline))
}

/// Add to the output. A window whose cursor is on the last line follows it, as `*messages*`
/// does.
fn append(ctx: &mut Ctx, buf: BufferId, text: &str) {
    if text.is_empty() {
        return;
    }
    let windows = showing(ctx, buf);
    let Ok(t) = ctx.text_mut(buf) else { return }; // killed: the build goes on unseen
    let last = last_line_start(t);
    let following: Vec<_> = windows
        .into_iter()
        .filter(|w| t.cursor(*w).is_some_and(|at| at >= last))
        .collect();
    let end = t.len();
    let _ = t.tx(Origin::Extension).silent().insert(end, text).commit();
    t.set_modified(false);
    let last = last_line_start(t);
    for w in following {
        t.set_cursor(w, last);
    }
}

/// Put every window showing the output on its first error, as Emacs's
/// `compilation-scroll-output` set to `first-error` does.
fn to_first_error(ctx: &mut Ctx, buf: BufferId) {
    let windows = showing(ctx, buf);
    let Ok(t) = ctx.text_mut(buf) else { return };
    let text = t.rope().to_string();
    let at = text.find("\nerror").map(|at| at + 1);
    for w in windows {
        t.set_cursor(w, at.unwrap_or_else(|| last_line_start(t)));
    }
}

/// The build is over: show the rest, then restart if it built, or say why not.
fn ended(
    ctx: &mut Ctx,
    run: &mut Run,
    shared: &Shared,
    status: io::Result<ExitStatus>,
    restart: &dyn Fn(),
) -> die_core::Result {
    run.tick = None;
    shared.running.set(false);
    append(ctx, run.buf, &take(&run.said, true));
    let see = match ctx.text(run.buf) {
        Ok(_) => format!(". See {REBUILD}"),
        Err(_) => String::new(),
    };
    match status {
        Ok(s) if s.success() => {
            let opened = shared.opened.take();
            if let Some(w) = opened.filter(|w| ctx.window_buffer(*w) == Some(run.buf)) {
                let _ = ctx.close_window(w);
            }
            // Its log has done its job, and the restart shouldn't bring it back.
            ctx.kill(run.buf);
            ctx.messages.push("Built. Restarting".into());
            restart();
            Ok(())
        }
        Ok(s) => {
            append(ctx, run.buf, &format!("\nThe build failed: {s}.\n"));
            to_first_error(ctx, run.buf);
            let why = format!("The rebuild failed ({s}), so this DIE carries on{see}");
            Err(Error::Msg(why))
        }
        Err(e) => {
            append(ctx, run.buf, &format!("\nIt didn't run: {e}.\n"));
            Err(Error::Msg(format!("The rebuild didn't run: {e}")))
        }
    }
}
