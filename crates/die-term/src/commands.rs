//! die-term as an extension: opening terminals, and the commands and keys a terminal's normal
//! state has (scope §6, D18).
//!
//! `term` opens your `$SHELL` as a login shell in the buffer's directory, in this window, in
//! insert state, and `:term cmd` runs `cmd` through it; more are `*terminal*<2>` and so on.
//! `term-switch` goes to `*terminal*` and opens it if it isn't there, as ghostel does. In normal
//! state, Esc goes to the program, as your `vterm-send-escape` binding has it, `i`, `a`, `I` and
//! `A` go back to insert at the live screen, `p` and `P` paste, `G` goes to the terminal's
//! cursor, and `C-c C-r` changes where insert state's Esc goes, as evil-ghostel's
//! `evil-ghostel-toggle-send-escape` does. The program's exit closes the buffer.
use crate::Term;
use crate::surface::{CopyFn, Escape, KIND, TermSurface};
use die_core::{
    Args, BufferId, Ctx, Error, Extension, Key, KeyEvent, PtyCommand, Register, Registry, Result,
    Rgb, ShowIn, State,
};
use std::ffi::OsString;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

/// The first terminal's name. The next is `*terminal*<2>`.
pub const NAME: &str = "*terminal*";

/// die-term, registered by the `die` facade.
pub struct Terminals {
    /// The shell `term` starts, as a login shell. None is `$SHELL`, or `/bin/sh` without one.
    shell: Option<PathBuf>,
    /// What a program's OSC 52 write does with its text. The default puts it on the system
    /// clipboard; the facade makes it a kill as well, as Emacs's `kill-new` does.
    on_copy: CopyFn,
    /// Insert state's Esc went to a program on its alternate screen, and said so once.
    hinted: bool,
}

impl Default for Terminals {
    fn default() -> Terminals {
        Terminals {
            shell: None,
            on_copy: to_clipboard,
            hinted: false,
        }
    }
}

impl Terminals {
    /// With OSC 52 writes going to `f`.
    pub fn copying_with(self, f: CopyFn) -> Terminals {
        Terminals { on_copy: f, ..self }
    }
    /// With `term` starting `shell` rather than `$SHELL`.
    pub fn with_shell(self, shell: impl Into<PathBuf>) -> Terminals {
        let shell = Some(shell.into());
        Terminals { shell, ..self }
    }
}

/// Put an OSC 52 write on the system clipboard.
pub fn to_clipboard(ctx: &mut Ctx, r: Register) {
    ctx.write_clipboard(&r);
}

/// What runs in a new terminal ([`open`]).
#[derive(Clone, Debug)]
pub struct Spec {
    /// The buffer's name, made unique as `*terminal*<2>` is.
    pub name: String,
    /// The program, found on the login shell's PATH unless it's a path.
    pub program: OsString,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Start it as a login shell (`argv[0]` is `-zsh`), and count it busy only while a job runs.
    pub login: bool,
    /// Where it starts. None is the new buffer's directory: the focused buffer's.
    pub dir: Option<PathBuf>,
    /// Variables for it, after DIE's (`die_core::pty`'s).
    pub env: Vec<(OsString, OsString)>,
}

impl Spec {
    /// `program` with no arguments, in a buffer named `name`.
    pub fn new(name: &str, program: impl Into<OsString>) -> Spec {
        Spec {
            name: name.into(),
            program: program.into(),
            args: vec![],
            login: false,
            dir: None,
            env: vec![],
        }
    }

    /// `shell` as a login shell, in `*terminal*`.
    pub fn shell(shell: &Path) -> Spec {
        Spec {
            login: true,
            ..Spec::new(NAME, shell)
        }
    }
}

/// Open a terminal running `spec`, shown `how`, in insert state. Err if there's no PTY to be had
/// or no such directory; a program that won't start says so on the terminal, and the buffer
/// closes when it exits.
pub fn open(ctx: &mut Ctx, spec: Spec, how: ShowIn) -> Result<BufferId> {
    let copy = ctx.with_ext(|t: &mut Terminals, _| t.on_copy);
    open_with(ctx, spec, how, copy.unwrap_or(to_clipboard))
}

fn open_with(ctx: &mut Ctx, mut spec: Spec, how: ShowIn, copy: CopyFn) -> Result<BufferId> {
    crate::shell::integrate(&mut spec);
    let window = ctx.focused();
    let (cols, rows) = window.and_then(|w| ctx.window_size(w)).unwrap_or((80, 24));
    let cell = ctx.cell_pixels();
    let to_u16 = |n: usize| u16::try_from(n.max(1)).unwrap_or(u16::MAX);
    let mut term =
        Term::new(to_u16(cols), to_u16(rows), (cell.width, cell.height)).map_err(Error::Msg)?;
    let default = ctx.face_style("default").unwrap_or_default();
    let fg = default.fg.unwrap_or(Rgb(0xd0, 0xd0, 0xd0));
    let bg = default.bg.unwrap_or(Rgb(0x10, 0x10, 0x10));
    let cursor = ctx.face_style("cursor").and_then(|s| s.bg);
    let ansi = ctx.theme().ansi;
    term.set_colors(fg, bg, cursor, &ansi);
    let program = Path::new(&spec.program);
    let short = program.file_name().unwrap_or(program.as_os_str());
    let short = short.to_string_lossy().into_owned();
    let s = TermSurface::new(term, &spec.name, &short, Path::new("/"), copy);
    let buf = ctx.create_surface(&spec.name, s);
    if let Some(dir) = &spec.dir {
        ctx.set_directory(buf, dir.clone());
    }
    let dir = ctx
        .directory(buf)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let name = ctx.buffer_name(buf).unwrap_or(NAME).to_string();
    let mut cmd = PtyCommand::new(&spec.program)
        .args(&spec.args)
        .current_dir(&dir)
        .on_output(move |ctx, bytes| {
            ctx.with_surface(buf, |s: &mut TermSurface, ctx| s.output(ctx, bytes))?;
            Ok(())
        })
        .on_exit(move |ctx, status| exited(ctx, buf, status));
    if spec.login {
        cmd = cmd.arg0(format!("-{short}"));
    }
    for (k, v) in &spec.env {
        cmd = cmd.env(k, v);
    }
    let size = die_core::PtySize::new(cols.max(1), rows.max(1), cell);
    let pty = match ctx.pty(cmd, size) {
        Ok(pty) => pty,
        Err(e) => {
            ctx.kill(buf);
            return Err(e);
        }
    };
    let login = spec.login;
    ctx.with_surface(buf, move |s: &mut TermSurface, _| {
        s.buf = buf;
        s.name = name;
        s.dir = dir;
        s.shell = login;
        s.pty = Some(pty);
        s.cell = cell;
    })?;
    if let Err(e) = ctx.show(buf, how) {
        ctx.kill(buf);
        return Err(e);
    }
    Ok(buf)
}

/// The program ended: close its buffer, and say so if it failed.
fn exited(ctx: &mut Ctx, buf: BufferId, status: io::Result<ExitStatus>) -> Result {
    let program = ctx.with_surface(buf, |s: &mut TermSurface, _| s.program.clone());
    let (Ok(program), Some(name)) = (program, ctx.buffer_name(buf)) else {
        return Ok(());
    };
    let why = match status {
        Ok(s) if s.success() => None,
        Ok(s) => Some(match (s.code(), s.signal()) {
            (Some(code), _) => format!("{program} exited with {code}"),
            (_, Some(signal)) => format!("{program} was killed by signal {signal}"),
            _ => format!("{program} ended: {s}"),
        }),
        Err(e) => Some(format!("{program} didn't start: {e}")),
    };
    if let Some(why) = why {
        ctx.messages.push(format!("{name}: {why}"));
    }
    ctx.kill(buf);
    Ok(())
}

/// The terminals with a program other than their shell running, by buffer name, as quitting
/// asks about (scope D18).
pub fn busy(ctx: &mut Ctx) -> Vec<String> {
    let mut names = vec![];
    for b in ctx.buffer_list() {
        if matches!(
            ctx.with_surface(b, |s: &mut TermSurface, _| s.busy()),
            Ok(true)
        ) {
            names.extend(ctx.buffer_name(b).map(String::from));
        }
    }
    names.sort();
    names
}

/// Say once a run that insert state's Esc went to a program on its alternate screen.
pub(crate) fn hint_once(ctx: &mut Ctx) {
    let first = ctx.with_ext(|t: &mut Terminals, _| !std::mem::replace(&mut t.hinted, true));
    if matches!(first, Ok(true)) {
        ctx.messages.push(
            "Esc went to the program, which is on its alternate screen. Cmd-Esc goes to normal \
             state, and C-c C-r there changes where Esc goes"
                .into(),
        );
    }
}

/// The terminal a command acts on: the buffer it was given, or the focused one.
fn this(ctx: &Ctx, a: &Args) -> Result<BufferId> {
    let buf = a.buffer.or_else(|| ctx.focused_buffer());
    buf.ok_or_else(|| Error::Msg("No terminal here".into()))
}

/// Act on this buffer's terminal, or say it isn't one.
fn with_term<R>(
    ctx: &mut Ctx,
    a: &Args,
    f: impl FnOnce(&mut TermSurface, &mut Ctx) -> R,
) -> Result<R> {
    let buf = this(ctx, a)?;
    ctx.with_surface(buf, f)
        .map_err(|_| Error::Msg("This buffer isn't a terminal".into()))
}

impl Terminals {
    /// What `term` runs: the shell as a login shell.
    fn shell_spec(&self) -> Spec {
        let shell = self.shell.clone().or_else(|| {
            let sh = std::env::var_os("SHELL").filter(|s| !s.is_empty());
            sh.map(PathBuf::from)
        });
        Spec::shell(&shell.unwrap_or_else(|| PathBuf::from("/bin/sh")))
    }

    /// `:term`, or with a command, `:term top`, that command through the shell, as Vim's
    /// `:terminal top` runs it, and the buffer closes when it ends.
    fn term(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let mut spec = self.shell_spec();
        if let Some(cmd) = a.text.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            spec.args = vec!["-c".into(), cmd.into()];
            spec.login = false;
        }
        open_with(ctx, spec, ShowIn::Current, self.on_copy).map(drop)
    }

    fn switch(&mut self, ctx: &mut Ctx, _: &Args) -> Result {
        let found = ctx.find_buffer(NAME);
        let live = found.filter(|b| ctx.with_surface(*b, |_: &mut TermSurface, _| ()).is_ok());
        match live {
            Some(b) => ctx.show(b, ShowIn::Current).map(drop),
            None => open_with(ctx, self.shell_spec(), ShowIn::Current, self.on_copy).map(drop),
        }
    }
}

/// Esc in normal state: to the program, and stay.
fn send_escape(ctx: &mut Ctx, a: &Args) -> Result {
    with_term(ctx, a, |s, _| {
        let bytes = s.term.key(&KeyEvent::press(Key::Esc));
        s.write(&bytes);
    })
}

/// `G` in normal state: with no count, die-vim's `G` and then on to the terminal's cursor, as
/// evil-ghostel's `evil-ghostel-goto-cursor` does, so the cursor follows it; with a count, or
/// on the alternate screen, die-vim's `G` alone.
fn goto_cursor(ctx: &mut Ctx, a: &Args) -> Result {
    ctx.run("vim-goto-line", a)?;
    let buf = this(ctx, a)?;
    let alt = with_term(ctx, a, |s, _| s.term.alt_screen())?;
    let (Some(w), Some(at)) = (a.window.or(ctx.focused()), ctx.surface_cursor(buf)) else {
        return Ok(());
    };
    if a.count.is_none() && !alt {
        ctx.text_mut(buf)?.set_cursor(w, at);
    }
    Ok(())
}

/// `i`, `a`, `I` and `A`: back to insert state at the live screen.
fn insert(ctx: &mut Ctx, a: &Args) -> Result {
    let buf = this(ctx, a)?;
    with_term(ctx, a, |s, _| s.insert())?;
    ctx.set_state_of(buf, State::Passthrough);
    Ok(())
}

/// evil-ghostel's `evil-ghostel-toggle-send-escape`: from auto to whichever way auto doesn't
/// send Esc now, and from either back to auto. A count of 1, 2 or 3 picks auto, the program or
/// normal state.
fn toggle_escape(ctx: &mut Ctx, a: &Args) -> Result {
    let said = with_term(ctx, a, |s, _| {
        let alt = s.term.alt_screen();
        s.escape = match (a.count, s.escape) {
            (Some(1), _) => Escape::Auto,
            (Some(2), _) => Escape::Program,
            (Some(3), _) => Escape::Normal,
            (Some(n), _) => {
                return Err(Error::Msg(format!(
                    "No Esc routing {n}: 1 is auto, 2 the program, 3 normal state"
                )));
            }
            (None, Escape::Auto) if alt => Escape::Normal,
            (None, Escape::Auto) => Escape::Program,
            (None, _) => Escape::Auto,
        };
        Ok(match (s.escape, alt) {
            (Escape::Auto, true) => "Esc in insert: auto (to the program now)",
            (Escape::Auto, false) => "Esc in insert: auto (to normal state now)",
            (Escape::Program, _) => "Esc in insert: to the program",
            (Escape::Normal, _) => "Esc in insert: to normal state",
        })
    })??;
    ctx.messages.push(said.into());
    Ok(())
}

impl Extension for Terminals {
    fn name(&self) -> &'static str {
        "die-term"
    }

    fn register(&mut self, reg: &mut Registry) {
        let doc = "Open a terminal: your $SHELL as a login shell, in this buffer's directory, in \
                   this window, in insert state, or with a command, as `:term top`, that command \
                   through it. Another is *terminal*<2>, and so on";
        reg.ext_command("term", doc, Terminals::term);
        reg.ext_command("terminal", doc, Terminals::term);
        reg.ext_command(
            "term-switch",
            "Go to *terminal*, and open it if it isn't there, as ghostel does",
            Terminals::switch,
        );
        reg.command(
            "term-send-escape",
            "Esc in a terminal's normal state: send Esc to the program, and stay in normal state",
            send_escape,
        );
        reg.command(
            "term-goto-cursor",
            "G in a terminal's normal state: to the terminal's cursor, or with a count to that \
             line, as evil-ghostel's G does",
            goto_cursor,
        );
        reg.command(
            "term-insert",
            "Back to a terminal's insert state, at the live screen",
            insert,
        );
        reg.command(
            "term-toggle-send-escape",
            "Change where a plain Esc in this terminal's insert state goes: from auto (to the \
             program on its alternate screen, else to normal state) to the other way, and back. \
             A count of 1, 2 or 3 picks auto, the program or normal state",
            toggle_escape,
        );
        let k = reg.kind_keymap(KIND);
        k.bind(State::Normal, "<Esc>", "term-send-escape")
            .bind(State::Normal, "<C-c><C-r>", "term-toggle-send-escape")
            .bind(State::Normal, "p", "surface-paste")
            .bind(State::Normal, "P", "surface-paste")
            .bind(State::Normal, "<D-v>", "surface-paste")
            .bind(State::Normal, "G", "term-goto-cursor");
        for key in ["i", "a", "I", "A"] {
            k.bind(State::Normal, key, "term-insert");
        }
    }

    /// The login shell's PATH, which a terminal's first program waits for (`die_core::process`).
    fn init(&mut self, _ctx: &mut Ctx) -> Result {
        let warm = std::thread::Builder::new().name("die-term-path".into());
        let _ = warm.spawn(|| {
            die_core::process::path();
        });
        Ok(())
    }
}
