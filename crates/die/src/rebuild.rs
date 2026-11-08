//! `rebuild`: `cargo xtask app` from inside DIE, with what cargo says in `*rebuild*`, then a
//! restart into the new build if it built. A failed build leaves this DIE running and installs
//! nothing, since `xtask app` puts a bundle in place only once it's whole, and a new build that
//! dies at its start leaves this one running too, as any `restart` does.

use die_core::process::{last_line_start, shown};
use die_core::{BufferId, Ctx, Error, Registry, ShowIn, Side, WindowId};
use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::rc::Rc;

/// The buffer cargo's output goes in.
pub const REBUILD: &str = "*rebuild*";

/// The workspace `rebuild` builds: the DIE checkout the file `named` names, which
/// `cargo xtask app` writes in the bundle it makes, so that `rebuild` builds where this DIE came
/// from, whatever file you're in.
pub fn root_for(named: &Path) -> Result<PathBuf, String> {
    let Ok(text) = std::fs::read_to_string(named) else {
        return Err(
            "This DIE.app doesn't say which checkout it came from. Run `cargo xtask app` \
                    in your DIE checkout once, and `rebuild` builds that one from then on"
                .into(),
        );
    };
    let root = PathBuf::from(text.trim_end_matches('\n'));
    match is_die(&root) {
        true => Ok(root),
        false => Err(format!(
            "This DIE.app came from {}, which isn't a DIE checkout any more. Run `cargo xtask \
             app` in yours once",
            root.display()
        )),
    }
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
/// started from the Dock has launchd's PATH, so cargo is found on the PATH every command gets
/// (`die_core::process::path`).
pub fn xtask_app(root: &Path, home: &Path) -> Command {
    let mut cmd = Command::new("cargo");
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
    /// The file naming the checkout to build: the running bundle's `Contents/Resources/checkout`.
    pub checkout: PathBuf,
}

/// What lasts from one rebuild to the next.
#[derive(Default)]
struct Shared {
    running: Cell<bool>,
    /// The window a rebuild opened for the output, closed when a build works.
    opened: Cell<Option<WindowId>>,
}

/// Register `rebuild`.
pub fn register(reg: &mut Registry, setup: Setup) {
    let (setup, shared) = (Rc::new(setup), Rc::new(Shared::default()));
    let doc = "Build DIE.app again with `cargo xtask app`, in the checkout this DIE came from, and \
               restart into it if it builds";
    reg.command("rebuild", doc, move |ctx, _| {
        if shared.running.get() {
            return Err(Error::Msg("A rebuild is already running".into()));
        }
        let (setup, shared) = (setup.clone(), shared.clone());
        let ending = crate::terminals::Ending::Rebuild;
        crate::terminals::unless_busy(ctx, ending, move |ctx| start(ctx, &setup, &shared))
    });
}

/// Run the build, with its output in `*rebuild*`.
fn start(ctx: &mut Ctx, setup: &Rc<Setup>, shared: &Rc<Shared>) -> die_core::Result {
    let root = root_for(&setup.checkout).map_err(Error::Msg)?;
    let cmd = (setup.command)(&root)?;
    // File names in cargo's errors are from `root`, so that `gf` and `<CR>` open them.
    let header = format!("{} in {}\n\n", shown(&cmd), root.display());
    let buf = ctx.output_buffer(REBUILD, &header, &root)?;
    let end = last_line_start(ctx.text(buf)?);
    for w in showing(ctx, buf) {
        ctx.text_mut(buf)?.set_cursor(w, end);
    }
    show(ctx, buf, &shared.opened);
    shared.running.set(true);
    let (setup, shared) = (setup.clone(), shared.clone());
    ctx.stream(buf, cmd, move |ctx, f| {
        ended(ctx, buf, &shared, f.status, &*setup.restart)
    });
    ctx.messages
        .push(format!("Rebuilding in {}", root.display()));
    Ok(())
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

/// The build is over: restart if it built, or say why not.
fn ended(
    ctx: &mut Ctx,
    buf: BufferId,
    shared: &Shared,
    status: io::Result<ExitStatus>,
    restart: &dyn Fn(),
) -> die_core::Result {
    shared.running.set(false);
    let see = match ctx.text(buf) {
        Ok(_) => format!(". See {REBUILD}"),
        Err(_) => String::new(),
    };
    match status {
        Ok(s) if s.success() => {
            let opened = shared.opened.take();
            if let Some(w) = opened.filter(|w| ctx.window_buffer(*w) == Some(buf)) {
                let _ = ctx.close_window(w);
            }
            // Its log has done its job, and the restart shouldn't bring it back.
            ctx.kill(buf);
            ctx.messages.push("Built. Restarting".into());
            restart();
            Ok(())
        }
        Ok(s) => {
            ctx.append_output(buf, &format!("\nThe build failed: {s}.\n"));
            to_first_error(ctx, buf);
            let why = format!("The rebuild failed ({s}), so this DIE carries on{see}");
            Err(Error::Msg(why))
        }
        Err(e) => {
            ctx.append_output(buf, &format!("\nIt didn't run: {e}.\n"));
            Err(Error::Msg(format!("The rebuild didn't run: {e}")))
        }
    }
}
