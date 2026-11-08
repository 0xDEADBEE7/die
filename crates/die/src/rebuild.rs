//! `rebuild`: `cargo xtask app` from inside DIE, with what cargo says in `*rebuild*`, then a
//! restart into the new build if it built. A failed build leaves this DIE running and installs
//! nothing, since `xtask app` puts a bundle in place only once it's whole, and a new build that
//! dies at its start leaves this one running too, as any `restart` does.

use die_core::{Ctx, Error, Origin, Registry, ShowIn, Side, Sub, project_root};
use std::cell::{Cell, RefCell};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The buffer cargo's output goes in.
pub const REBUILD: &str = "*rebuild*";
/// How often what cargo has said so far goes into the buffer.
const SHOW_EVERY: Duration = Duration::from_millis(100);

/// The workspace `rebuild` builds: the project `dir` is in, if that's DIE's, so that editing a
/// checkout builds that checkout, or else the one this DIE was built from.
pub fn root_for(dir: Option<&Path>) -> PathBuf {
    let here = dir.map(project_root).filter(|r| is_die(r));
    here.unwrap_or_else(built_from)
}

/// Whether `root` is DIE's workspace, so that `cargo xtask app` there is the one meant.
fn is_die(root: &Path) -> bool {
    root.join("crates/die/Cargo.toml").is_file() && root.join("xtask").is_dir()
}

/// Where this DIE's workspace was when it was built.
pub fn built_from() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or(manifest)
        .to_path_buf()
}

/// `cargo xtask app` in `root`. A DIE started from the Dock has launchd's PATH, which hasn't
/// got rustup's `~/.cargo/bin`, so that goes first.
pub fn xtask_app(root: &Path) -> Command {
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
    cmd.current_dir(root).args(["xtask", "app"]);
    cmd
}

/// What one build has said, and the timer that shows it.
struct Run {
    buf: die_core::BufferId,
    said: Arc<Mutex<Vec<u8>>>,
    tick: Option<Sub>,
    /// The window this build opened for its output, closed again if it builds.
    opened: Option<die_core::WindowId>,
}

/// Register `rebuild`, which runs `command` for the workspace and calls `restart` once it has
/// exited 0.
pub fn register(
    reg: &mut Registry,
    command: impl Fn(&Path) -> Command + 'static,
    restart: impl Fn() + 'static,
) {
    let running = Rc::new(Cell::new(false));
    let (command, restart) = (Rc::new(command), Rc::new(restart));
    let doc = "Build DIE.app again with `cargo xtask app`, in the DIE workspace this buffer is in \
               or else the one this DIE was built from, and restart into it if it builds";
    reg.command("rebuild", doc, move |ctx, _| {
        if running.get() {
            return Err(Error::Msg("A rebuild is already running".into()));
        }
        let dir = ctx.focused_buffer().and_then(|b| ctx.directory(b));
        let root = root_for(dir);
        let cmd = command(&root);
        let buf = output_buffer(ctx, &root, &cmd)?;
        let opened = show(ctx, buf)?;
        let said = Arc::new(Mutex::new(Vec::new()));
        let shown = said.clone();
        let tick = ctx.every(SHOW_EVERY, move |ctx| {
            append(ctx, buf, take(&shown, false));
            Ok(())
        });
        let run = Rc::new(RefCell::new(Run {
            buf,
            said: said.clone(),
            tick: Some(tick),
            opened,
        }));
        running.set(true);
        let (running, restart) = (running.clone(), restart.clone());
        let job = ctx.spawn(
            move || build(cmd, &said),
            move |ctx, status| {
                running.set(false);
                ended(ctx, &mut run.borrow_mut(), status, &*restart)
            },
        );
        job.detach();
        ctx.messages
            .push(format!("Rebuilding in {}", root.display()));
        Ok(())
    });
}

/// `*rebuild*`, emptied, with what runs where at its top, and file names in cargo's errors
/// taken from `root`, so that `gf` opens them.
fn output_buffer(
    ctx: &mut Ctx,
    root: &Path,
    cmd: &Command,
) -> die_core::Result<die_core::BufferId> {
    let buf = match ctx.find_buffer(REBUILD) {
        Some(buf) => buf,
        None => {
            let buf = ctx.create_text(REBUILD, "");
            let settings = ctx.text_mut(buf)?.settings_mut();
            settings.read_only = true;
            settings.max_width = None;
            settings.center = false;
            buf
        }
    };
    let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
    let program = Path::new(cmd.get_program()).file_name().unwrap_or_default();
    let header = format!(
        "{} {} in {}\n\n",
        program.to_string_lossy(),
        args.join(" "),
        root.display()
    );
    let t = ctx.text_mut(buf)?;
    let all = 0..t.len();
    t.tx(Origin::Extension)
        .silent()
        .replace(all, header)
        .commit()?;
    t.set_modified(false);
    ctx.set_directory(buf, root);
    Ok(buf)
}

/// Show the output below, with the focus staying, unless a window has it already. The window
/// made for it comes back.
fn show(ctx: &mut Ctx, buf: die_core::BufferId) -> die_core::Result<Option<die_core::WindowId>> {
    let windows = ctx.window_list();
    if windows.iter().any(|w| ctx.window_buffer(*w) == Some(buf)) {
        return Ok(None);
    }
    let here = ctx.focused();
    let Ok(opened) = ctx.show(buf, ShowIn::Split(Side::Below)) else {
        return Ok(None); // Too small to split: the messages still say how it went.
    };
    if let Some(w) = here {
        ctx.focus(w);
    }
    Ok(opened)
}

/// Run `cmd` with its output and errors together in `said`, as a terminal would show them.
fn build(mut cmd: Command, said: &Mutex<Vec<u8>>) -> io::Result<ExitStatus> {
    let (mut reader, writer) = io::pipe()?;
    cmd.stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer);
    let mut child = cmd.spawn()?;
    drop(cmd); // its copies of the pipe's writing end, or the reading never ends
    let mut chunk = [0; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => said
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend(&chunk[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        }
    }
    child.wait()
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

fn append(ctx: &mut Ctx, buf: die_core::BufferId, text: String) {
    if text.is_empty() {
        return;
    }
    let Ok(t) = ctx.text_mut(buf) else { return }; // killed: the build goes on unseen
    let end = t.len();
    let _ = t.tx(Origin::Extension).silent().insert(end, text).commit();
    t.set_modified(false);
}

/// The build is over: show the rest, then restart if it built, or say why not.
fn ended(
    ctx: &mut Ctx,
    run: &mut Run,
    status: io::Result<ExitStatus>,
    restart: &dyn Fn(),
) -> die_core::Result {
    run.tick = None;
    let rest = take(&run.said, true);
    append(ctx, run.buf, rest);
    match status {
        Ok(s) if s.success() => {
            append(ctx, run.buf, "\nBuilt. Restarting.\n".into());
            let still = run
                .opened
                .filter(|w| ctx.window_buffer(*w) == Some(run.buf));
            if let Some(w) = still {
                let _ = ctx.close_window(w);
            }
            restart();
            Ok(())
        }
        Ok(s) => {
            append(ctx, run.buf, format!("\nThe build failed: {s}.\n"));
            let why = format!("The rebuild failed ({s}), so this DIE carries on. See {REBUILD}");
            Err(Error::Msg(why))
        }
        Err(e) => {
            append(ctx, run.buf, format!("\nIt didn't run: {e}.\n"));
            Err(Error::Msg(format!("The rebuild didn't run: {e}")))
        }
    }
}
