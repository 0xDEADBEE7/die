//! Your DIE config while DIE is being built, drafted from your Emacs `init.el`, which it
//! only reads, with no extension: DIE's own keys, theme and status line. Bindings grow with each
//! checkpoint, as their commands land: `\f \] \/ \x` come with K6 and K11a.
//!
//! Extensions aren't part of DIE (scope D14): each is a repository of its own, and a config
//! built on this one installs them with [`main_with`] and an [`Extra`], as the owner's
//! die-config does.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

pub mod theme;

use die::app::Launch;
use die::core::{Ctx, Extension, FileInfo, LineNumbers, Registry, Settings, State, Theme, Wrap};
use die::render::check;
use die::render::driver::Config as Font;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The files your `init.el` turns line numbers on for: python-mode, c-mode, c++-mode,
/// emacs-lisp-mode, js-mode, mhtml-mode and css-mode, and the modes derived from them, by
/// the extensions `emacs -Q` gives those modes. Lower case: Emacs folds case when nothing
/// matches exactly, and `.C` and `.H`, which do match, are in these modes too.
const LINE_NUMBERS: &[&str] = &[
    "py", "pyi", "pyw", "c", "h", "i", "lex", "y", "cc", "cpp", "cxx", "c++", "hh", "hpp", "hxx",
    "h++", "ii", "el", "js", "jsx", "html", "htm", "xhtml", "shtml", "shtm", "css", "scss", "less",
];

/// What the command line asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Args {
    /// `--check a,b`: run these rather than wait for you.
    pub check: Option<Vec<String>>,
    /// The files named.
    pub files: Vec<PathBuf>,
    /// What a launch of DIE.app was asked to do.
    pub launch: Launch,
}

/// `die [--check a,b] [--handover socket] [--restart-after-frame] [--restart-loop] [--frontmost]
/// [--background] [--wait] [--] [file...]`, where in DIE.app a file is `path[:line[:col]]` or
/// `+line[:col] path`. `-w` is `--wait`.
pub fn parse_args(args: impl IntoIterator<Item = impl Into<OsString>>) -> Result<Args, String> {
    let mut parsed = Args {
        check: None,
        files: vec![],
        launch: Launch::default(),
    };
    let mut args = args.into_iter().map(Into::into);
    let mut only_files = false;
    while let Some(arg) = args.next() {
        let name = arg.to_string_lossy();
        if !only_files && name == "--" {
            only_files = true;
        } else if !only_files && name == "--check" {
            let list = args.next().ok_or("--check needs a list of checks")?;
            let list = list.to_string_lossy();
            let names: Vec<String> = list.split(',').map(str::to_owned).collect();
            if let Some(bad) = names.iter().find(|n| !check::NAMES.contains(&n.as_str())) {
                return Err(format!(
                    "no check named {bad}; there's {}",
                    check::NAMES.join(", ")
                ));
            }
            parsed.check = Some(names);
        } else if !only_files && name == "--handover" {
            let socket = args.next().ok_or("--handover needs a socket")?;
            parsed.launch.handover = Some(PathBuf::from(socket));
        } else if !only_files && name == "--restart-after-frame" {
            parsed.launch.restart_after_frame = true;
        } else if !only_files && name == "--frontmost" {
            parsed.launch.frontmost = true;
        } else if !only_files && name == "--restart-loop" {
            parsed.launch.restart_loop = true;
        } else if !only_files && name == "--background" {
            parsed.launch.background = true;
        } else if !only_files && (name == "--wait" || name == "-w") {
            parsed.launch.wait = true;
        } else if !only_files && name.starts_with('-') && name != "-" {
            return Err(format!("no option {name}"));
        } else {
            parsed.files.push(PathBuf::from(arg));
        }
    }
    Ok(parsed)
}

/// Keys and commands a config adds, declared on the registry after this config's own.
pub type BindingsFn = Box<dyn FnOnce(&mut Registry)>;

/// The theme a config uses, given mono-dark.
pub type ThemeFn = Box<dyn FnOnce(Theme) -> Theme>;

/// What a config built on this one adds, so it doesn't copy this one: the owner's die-config
/// is a binary crate whose `main` is `die_dev::main_with(extra)`. Every field is optional, and
/// `Extra::default()` is this config alone.
#[derive(Default)]
pub struct Extra {
    /// The extensions, after DIE's own, in this order.
    pub extensions: Vec<Box<dyn Extension>>,
    /// Keys and commands, declared after this config's own on the same registry, so the
    /// extensions' commands are there to bind, and a key bound twice is the later one's.
    pub bindings: Option<BindingsFn>,
    /// The theme: given mono-dark, which styles only core's faces, mono-dark with the
    /// extensions' faces styled too. DIE reports a face the theme styles that no one declared.
    pub theme: Option<ThemeFn>,
    /// What a run you look at opens on when no file is named ([`greeted`] says when).
    pub greeting: Option<Greeting>,
}

/// What a run you look at opens on: a command an extension registered, and the buffer it
/// shows, which takes the directory DIE started in.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Greeting {
    /// The command, run with no arguments.
    pub command: String,
    /// The name of the buffer it shows.
    pub buffer: String,
}

/// Your config, for these files: DIE's own, with no extension.
pub fn config(files: Vec<PathBuf>) -> die::Config {
    config_with(files, Extra::default())
}

/// `config` with `extra` on top: its extensions, its bindings after these, and its theme. Its
/// greeting isn't in it: `main_with` hands that to [`greeted`] for the two real runs only.
pub fn config_with(files: Vec<PathBuf>, extra: Extra) -> die::Config {
    let started = die::started_dir();
    let home = std::env::home_dir();
    let Extra {
        extensions,
        bindings: more,
        theme: faces,
        greeting: _,
    } = extra;
    let register: BindingsFn = match more {
        Some(more) => Box::new(move |reg: &mut Registry| {
            bindings(reg);
            more(reg);
        }),
        None => Box::new(bindings),
    };
    let core = die::core::Config {
        register: Some(register),
        theme: Some(match faces {
            Some(faces) => faces(theme::mono_dark()),
            None => theme::mono_dark(),
        }),
        text_defaults: text_defaults(),
        file_settings: Some(Box::new(file_settings)),
        startup: Some(Box::new(startup)),
        ..die::core::Config::default()
    };
    die::Config {
        core,
        extensions,
        status_text: Some(Box::new(status_text(&started, home.as_deref()))),
        font: Font::term(16.0, 1.0),
        files,
    }
}

/// `config`, for a run you look at, which opens on `greeting`. `main_with` calls this for the
/// two real runs only, so `config` itself stays what `--check` and the tests know: a still text
/// buffer and no timer. With no greeting, it's `config` as it was.
///
/// Your startup hook runs first, and an error of its own is what comes back, the greeting's
/// only when yours had none. Then the greeting's command runs when all of these hold:
/// - no file was named, as `config.files` stands when this is called;
/// - at most one window exists, so it never stands over a session's splits, where it would cost
///   you one at the next restore, a window on it not coming back;
/// - `launch` is a fresh one (no `--handover`, `--restart-loop` or `--restart-after-frame`), or
///   nothing has the focus, as after a restart that brought nothing back, where it stands in for
///   `*scratch*`.
///
/// So a cold start is the greeting alone, and over a session that restored one window it covers
/// that window until it's let go.
///
/// The greeting's buffer is in the directory DIE started in, your home for DIE.app, and not the
/// directory of the buffer it covers, which the command run by hand takes as any new buffer does:
/// DIE starts where Emacs does, whatever the session brought back behind it.
pub fn greeted(
    mut config: die::Config,
    launch: &Launch,
    greeting: Option<Greeting>,
) -> die::Config {
    let Some(greeting) = greeting else {
        return config;
    };
    let no_files = config.files.is_empty();
    let fresh = launch.fresh();
    let started = die::started_dir();
    let before = config.core.startup.take();
    config.core.startup = Some(Box::new(move |ctx| {
        let done = before.map_or(Ok(()), |hook| hook(ctx));
        if no_files && ctx.window_list().len() <= 1 && (fresh || ctx.focused().is_none()) {
            let shown = ctx.run(&greeting.command, &die::core::Args::default());
            if let Some(buf) = ctx.find_buffer(&greeting.buffer) {
                ctx.set_directory(buf, started);
            }
            return done.and(shown);
        }
        done
    }));
    config
}

/// `die`'s `main`, for this config with `extra` on top: DIE's other modes first (a PTY's
/// trampoline), then the arguments, then DIE.app's instance, its client, `--check` or a plain
/// run. `extra` is called only once none of the other modes has taken the run, and only for a
/// run that needs the config, so a config's extensions are made after the trampoline.
pub fn main_with(extra: impl FnOnce() -> Extra) -> ExitCode {
    if let Some(code) = die::other_mode() {
        return ExitCode::from(code.clamp(0, 255) as u8);
    }
    die::render::latency::start_clock();
    let args = match parse_args(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("die: {e}");
            return ExitCode::from(2);
        }
    };
    let made = move |files| {
        let mut extra = extra();
        let greeting = extra.greeting.take();
        (config_with(files, extra), greeting)
    };
    // A run you look at opens on the greeting when no file is named (`greeted` says when). A
    // check takes the config as it is: its first buffer is still, and has text.
    let code = match (args.check, die::app::bundle()) {
        (None, Some(bundle)) if die::app::is_client(&args.launch) => {
            die::app::client(&bundle, args.files, &args.launch)
        }
        (Some(names), _) => die::check(made(args.files).0, names),
        (None, Some(bundle)) => {
            // LaunchServices starts DIE.app in `/`: it starts at home, as Emacs.app does.
            die::app::start_at_home();
            let (config, greeting) = made(args.files);
            let config = greeted(config, &args.launch, greeting);
            die::app::run(config, bundle, args.launch)
        }
        (None, None) => {
            let (config, greeting) = made(args.files);
            die::run(greeted(config, &args.launch, greeting))
        }
    };
    ExitCode::from(code.clamp(0, 255) as u8)
}

/// `init.el`'s leader, and the bindings whose commands exist so far.
fn bindings(reg: &mut Registry) {
    reg.set_leader("\\");
    let keys = reg.global_keymap();
    keys.bind(State::Normal, "<leader>z", "toggle-max-width");
    keys.bind(State::Normal, "<leader>x", "command-palette");
    keys.bind(State::Normal, "<leader>f", "find-file");
    keys.bind(State::Normal, "<leader>\\", "find-file-home");
    keys.bind(State::Normal, "<leader>p", "find-file-project");
    keys.bind(State::Normal, "<leader>r", "grep");
    keys.bind(State::Normal, "<leader>/", "switch-buffer");
    keys.bind(State::Normal, "<leader>hk", "describe-key");
    keys.bind(State::Normal, "<leader>hc", "describe-command");
    keys.bind(State::Normal, "<leader>]", "switch-buffer");
    keys.bind(State::Normal, "<leader>}", "buffer-menu");
    keys.bind(State::Normal, "<leader>?", "buffer-menu");
    // `termin-open` and `ghostel`: a new terminal, and the first one.
    keys.bind(State::Normal, "<leader>e", "term");
    keys.bind(State::Normal, "<leader>g", "term-switch");
}

/// `init.el`'s `ghostel-shell` and `vterm-shell`: `term` starts `/bin/bash`, interactive and not
/// a login shell, as vterm (and so termin, your `\e`) starts it, so it reads your `~/.bashrc`.
/// Only macOS has terminals (D10).
fn startup(ctx: &mut Ctx) -> die::core::Result {
    #[cfg(target_os = "macos")]
    ctx.with_ext(|t: &mut die::term::Terminals, _| t.set_shell("/bin/bash", false))?;
    #[cfg(not(target_os = "macos"))]
    let _ = ctx;
    Ok(())
}

/// `global-visual-line-mode` and `visual-fill-column`, centered at 80.
fn text_defaults() -> Settings {
    Settings {
        wrap: Wrap::Word,
        max_width: Some(80),
        center: true,
        ..Settings::default()
    }
}

/// Line numbers by file: `init.el`'s `display-line-numbers-type` is `visual`.
fn file_settings(path: &Path, settings: &mut Settings) {
    let extension = path.extension().and_then(|e| e.to_str());
    if extension.is_some_and(|e| LINE_NUMBERS.iter().any(|n| n.eq_ignore_ascii_case(e))) {
        settings.line_numbers = LineNumbers::Visual;
    }
}

/// `dir` as your mode line shows it: with `~` for `home`, and no trailing `/`.
pub fn directory(dir: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| dir.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    }
}

/// Your status line: `default-directory`, as your mode line has it. That's the file's
/// directory, or for a buffer visiting no file, `started`.
pub fn status_text(
    started: &Path,
    home: Option<&Path>,
) -> impl Fn(&str, Option<&FileInfo>) -> String + 'static {
    let (started, home) = (started.to_path_buf(), home.map(Path::to_path_buf));
    move |_, file| {
        let dir = file.and_then(|f| f.path.parent()).unwrap_or(&started);
        let shown = directory(dir, home.as_deref());
        if shown.ends_with('/') {
            shown
        } else {
            shown + "/"
        }
    }
}
