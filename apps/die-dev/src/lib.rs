//! Your DIE config while DIE is being built, drafted from your Emacs `init.el`, which it
//! only reads. Bindings grow with each checkpoint, as their commands land: `\f \] \/ \x`
//! come with K6 and K11a.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

pub mod theme;

use die::app::Launch;
use die::core::{Ctx, FileInfo, LineNumbers, Registry, Settings, State, Wrap};
use die::render::check;
use die::render::driver::Config as Font;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

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

/// Your config, for these files.
pub fn config(files: Vec<PathBuf>) -> die::Config {
    let started = die::started_dir();
    let home = std::env::home_dir();
    let core = die::core::Config {
        register: Some(Box::new(bindings)),
        theme: Some(theme::mono_dark()),
        text_defaults: text_defaults(),
        file_settings: Some(Box::new(file_settings)),
        startup: Some(Box::new(startup)),
        ..die::core::Config::default()
    };
    die::Config {
        core,
        extensions: extensions(),
        status_text: Some(Box::new(status_text(&started, home.as_deref()))),
        font: Font::term(16.0, 1.0),
        files,
    }
}

/// `config`, for a run you look at, which opens on your jellyfish. `main` calls this for the two
/// real runs only, so `config` itself stays what `--check` and the tests know: a still text
/// buffer and no timer.
///
/// Your startup hook runs first, and an error of its own is what comes back, the jellyfish's only
/// when yours had none. Then the `jellyfish` command runs when all of these hold:
/// - no file was named, as `config.files` stands when this is called;
/// - at most one window exists, so it never stands over a session's splits, where it would cost
///   you one at the next restore, a window on it not coming back;
/// - `launch` is a fresh one (no `--handover`, `--restart-loop` or `--restart-after-frame`), or
///   nothing has the focus, as after a restart that brought nothing back, where it stands in for
///   `*scratch*`.
///
/// So a cold start is the jellyfish alone, over a session that restored one window it covers
/// that window until `q`, and `:rebuild` shows what was there.
///
/// The jellyfish this shows is in the directory DIE started in, your home for DIE.app, and not
/// the directory of the buffer it covers, which a `:jellyfish` of yours takes as any new buffer
/// does: DIE starts where Emacs does, whatever the session brought back behind it.
pub fn greeted(mut config: die::Config, launch: &Launch) -> die::Config {
    let no_files = config.files.is_empty();
    let fresh = launch.fresh();
    let started = die::started_dir();
    let before = config.core.startup.take();
    config.core.startup = Some(Box::new(move |ctx| {
        let done = before.map_or(Ok(()), |hook| hook(ctx));
        if no_files && ctx.window_list().len() <= 1 && (fresh || ctx.focused().is_none()) {
            let shown = ctx.run("jellyfish", &die::core::Args::default());
            if let Some(buf) = ctx.find_buffer(die_jellyfish::NAME) {
                ctx.set_directory(buf, started);
            }
            return done.and(shown);
        }
        done
    }));
    config
}

/// The packages your `init.el` loads, as DIE has them: emacs-orchid-client's port, with its
/// defaults, as you set none of its options. It finds `orchid` on your login shell's PATH, which
/// has `~/.local/bin`, as `init.el` adds it to `exec-path`. And the whiteboard, with its defaults:
/// PNGs at 2x on the theme's background, the fonts where DIE's are. And your jellyfish, with its
/// defaults: the water's dust and twinkles but no shoal, the window's whole width, and at rest
/// after five minutes without a key. Registering it shows nothing: `greeted` does that. And the
/// markdown view, with its defaults: the mode on `.md` files and markdown-mode's other
/// extensions, front matter hidden as your preview's export strips it, links that aren't
/// files DIE opens going to `open`, and the rendered view a page drawn as GitHub draws one, in
/// mono-dark's colours. A file opens raw, as it always has: `\m` renders it.
pub fn extensions() -> Vec<Box<dyn die::core::Extension>> {
    vec![
        Box::new(die_orchid_client::Orchid::default()),
        Box::new(die_whiteboard::Whiteboard::default()),
        Box::new(die_jellyfish::Jellyfish::default()),
        Box::new(die_markdown::Markdown::default()),
    ]
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
    // emacs-orchid-client's session browser, in normal state, as `init.el` binds it.
    keys.bind(State::Normal, "<leader>o", "orchid");
    // The whiteboard: a board in this window, or the one used last.
    keys.bind(State::Normal, "<leader>w", "whiteboard");
    // A markdown file's two views, rendered and raw. It's in the mode's keymap, so `\m` is a key
    // only where there's markdown to render, and in the page's, which is a buffer of its own
    // with no mode: `\m` there goes back to the file. Both only where the extension is.
    if let Some(keys) = reg.mode_keymap(die_markdown::MODE) {
        keys.bind(State::Normal, "<leader>m", "markdown-toggle");
        let keys = reg.kind_keymap(die_markdown::PAGE);
        keys.bind(State::Normal, "<leader>m", "markdown-toggle");
    }
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
