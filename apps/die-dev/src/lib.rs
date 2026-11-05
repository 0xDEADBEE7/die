//! Your DIE config while DIE is being built, drafted from `~/.emacs.d/init.el`, which it
//! only reads. Bindings grow with each checkpoint, as their commands land: `\f \] \/ \x`
//! come with K6 and K11a.

#[cfg(test)]
use die_test_cap as _;

pub mod theme;

use die::core::{FileInfo, LineNumbers, Registry, Settings, State, Wrap};
use die::render::check;
use die::render::driver::Config as Font;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The files your `init.el` turns line numbers on for: python-mode, c-mode, c++-mode,
/// emacs-lisp-mode, js-mode, mhtml-mode and css-mode, by their usual extensions.
const LINE_NUMBERS: &[&str] = &[
    "py", "c", "h", "cc", "cpp", "cxx", "hh", "hpp", "hxx", "el", "js", "mjs", "cjs", "html",
    "htm", "css",
];

/// What the command line asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Args {
    /// `--check a,b`: run these rather than wait for you.
    pub check: Option<Vec<String>>,
    /// The files named.
    pub files: Vec<PathBuf>,
}

/// `die [--check a,b] [--] [file...]`.
pub fn parse_args(args: impl IntoIterator<Item = impl Into<OsString>>) -> Result<Args, String> {
    let mut parsed = Args {
        check: None,
        files: vec![],
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
        } else if !only_files && name.starts_with("--") {
            return Err(format!("no option {name}"));
        } else {
            parsed.files.push(PathBuf::from(arg));
        }
    }
    Ok(parsed)
}

/// Your config, for these files.
pub fn config(files: Vec<PathBuf>) -> die::Config {
    let started = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let home = std::env::home_dir();
    let core = die::core::Config {
        register: Some(Box::new(bindings)),
        theme: Some(theme::mono_dark()),
        text_defaults: text_defaults(),
        file_settings: Some(Box::new(file_settings)),
        ..die::core::Config::default()
    };
    die::Config {
        core,
        status_text: Some(Box::new(status_text(&started, home.as_deref()))),
        font: Font::term(16.0, 1.0),
        files,
        ..die::Config::default()
    }
}

/// `init.el`'s leader, and the bindings whose commands exist so far.
fn bindings(reg: &mut Registry) {
    reg.set_leader("\\");
    let keys = reg.global_keymap();
    keys.bind(State::Normal, "<leader>z", "toggle-max-width");
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
    if extension.is_some_and(|e| LINE_NUMBERS.contains(&e)) {
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
