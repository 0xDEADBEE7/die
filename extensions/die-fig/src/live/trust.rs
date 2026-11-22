//! Your yeses (ruling 13): a spec's `run` and `stream` commands, and a dashboard's `file`
//! panels outside the spec's directory, run or open only once you've said yes, asked once for
//! each, and remembered across restarts by the spec's absolute path and the exact command (or
//! the file's absolute path) in what the session keeps (`Kept.state`, through the surface).
//!
//! The model says what needs asking ([`Trust::command`], [`Trust::file`]); the surface asks
//! (`Run "./errors.sh" every 30s for health.fig? (y/n)`) and says what you answered. A no is
//! kept for this session only, so a figure that's rebuilt doesn't ask again at every edit; `R`
//! forgets the noes ([`Trust::ask_again`]) and asks again. A command in a spec with no file (a
//! buffer never saved) is remembered for this session only, there being no path to keep it by.
//! `:fig !cmd`, which you type, asks nothing, and nothing here is asked of it.

use crate::error::{ErrorKind, FigError};
use crate::source::lexical;
use die_core::session::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What the store says of a command or a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// You said yes (or it needs no yes: a file inside its spec's directory).
    Yes,
    /// You said no, this session.
    No,
    /// It needs asking.
    Ask,
}

/// The yeses and noes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trust {
    /// Commands said yes to, by spec and command: kept.
    commands: BTreeSet<(PathBuf, String)>,
    /// Files said yes to, by spec and file: kept.
    files: BTreeSet<(PathBuf, PathBuf)>,
    /// Commands said yes to in a spec with no file: this session only.
    unsaved: BTreeSet<String>,
    /// Commands said no to: this session only.
    no_commands: BTreeSet<(Option<PathBuf>, String)>,
    /// Files said no to: this session only.
    no_files: BTreeSet<(PathBuf, PathBuf)>,
}

impl Trust {
    /// No yeses yet.
    pub fn new() -> Trust {
        Trust::default()
    }

    /// Whether `cmd`, named by the spec at `spec` (None for one with no file), may run.
    pub fn command(&self, spec: Option<&Path>, cmd: &str) -> Answer {
        let spec = spec.map(lexical);
        let yes = match &spec {
            Some(p) => self.commands.contains(&(p.clone(), cmd.to_string())),
            None => self.unsaved.contains(cmd),
        };
        if yes {
            Answer::Yes
        } else if self.no_commands.contains(&(spec, cmd.to_string())) {
            Answer::No
        } else {
            Answer::Ask
        }
    }

    /// You said yes to `cmd` for `spec`.
    pub fn allow_command(&mut self, spec: Option<&Path>, cmd: &str) {
        let spec = spec.map(lexical);
        self.no_commands.remove(&(spec.clone(), cmd.to_string()));
        match spec {
            Some(p) => self.commands.insert((p, cmd.to_string())),
            None => self.unsaved.insert(cmd.to_string()),
        };
    }

    /// You said no to `cmd` for `spec`: not asked again this session until [`Trust::ask_again`].
    pub fn decline_command(&mut self, spec: Option<&Path>, cmd: &str) {
        self.no_commands
            .insert((spec.map(lexical), cmd.to_string()));
    }

    /// Whether the `file` panel `file` of the spec at `spec` may open: yes without asking when
    /// it's inside the spec's directory (on disk, links followed, where both are there).
    pub fn file(&self, spec: &Path, file: &Path) -> Answer {
        if inside(spec, file) {
            return Answer::Yes;
        }
        let key = (lexical(spec), lexical(file));
        if self.files.contains(&key) {
            Answer::Yes
        } else if self.no_files.contains(&key) {
            Answer::No
        } else {
            Answer::Ask
        }
    }

    /// You said yes to opening `file` for `spec`.
    pub fn allow_file(&mut self, spec: &Path, file: &Path) {
        let key = (lexical(spec), lexical(file));
        self.no_files.remove(&key);
        self.files.insert(key);
    }

    /// You said no to opening `file` for `spec`, this session.
    pub fn decline_file(&mut self, spec: &Path, file: &Path) {
        self.no_files.insert((lexical(spec), lexical(file)));
    }

    /// `R`: every no forgotten, so each is asked again.
    pub fn ask_again(&mut self) {
        self.no_commands.clear();
        self.no_files.clear();
    }

    /// `R` in one figure: the noes to its own specs forgotten (its commands', and its file
    /// panels'), so only its questions are asked again; another figure's noes hold.
    pub fn ask_again_for(&mut self, specs: &[Option<PathBuf>]) {
        let specs: Vec<Option<PathBuf>> = specs.iter().map(|s| s.as_deref().map(lexical)).collect();
        self.no_commands.retain(|(s, _)| !specs.contains(s));
        self.no_files
            .retain(|(s, _)| !specs.contains(&Some(s.clone())));
    }

    /// Whether there's no yes to keep.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.files.is_empty()
    }

    /// What a restart keeps: the yeses with a spec's path, `{"commands": [[spec, cmd], …],
    /// "files": [[spec, file], …]}` in order. Null when there are none.
    pub fn to_value(&self) -> Value {
        if self.is_empty() {
            return Value::Null;
        }
        let commands: Vec<Value> = (self.commands.iter())
            .map(|(s, c)| json!([s.display().to_string(), c]))
            .collect();
        let files: Vec<Value> = (self.files.iter())
            .map(|(s, f)| json!([s.display().to_string(), f.display().to_string()]))
            .collect();
        json!({ "commands": commands, "files": files })
    }

    /// The yeses a restart kept, as [`Trust::to_value`] wrote them; what isn't one is let go.
    pub fn from_value(v: &Value) -> Trust {
        let pairs = |key: &str| -> Vec<(String, String)> {
            let items = v.get(key).and_then(Value::as_array);
            (items.into_iter().flatten())
                .filter_map(|p| {
                    let a = p.as_array()?;
                    let (x, y) = (a.first()?.as_str()?, a.get(1)?.as_str()?);
                    (a.len() == 2 && !x.is_empty() && !y.is_empty())
                        .then(|| (x.to_string(), y.to_string()))
                })
                .collect()
        };
        let mut t = Trust::new();
        for (s, c) in pairs("commands") {
            t.commands.insert((lexical(Path::new(&s)), c));
        }
        for (s, f) in pairs("files") {
            t.files
                .insert((lexical(Path::new(&s)), lexical(Path::new(&f))));
        }
        t
    }
}

/// What a source you said no to shows: an unreadable error, `not run: you said no`, naming the
/// command.
pub fn not_run(cmd: &str) -> FigError {
    FigError::new("not run: you said no")
        .with_kind(ErrorKind::Unreadable)
        .note(format!("{cmd} needs your yes to run; R asks again"))
}

/// Whether `file` is inside the directory of the spec at `spec` ([`inside_dir`]).
fn inside(spec: &Path, file: &Path) -> bool {
    spec.parent().is_some_and(|dir| inside_dir(dir, file))
}

/// Whether `file` is inside `dir`: by their paths on disk where both are there (so a link out of
/// it is out, and `..` goes where it goes), else by name once `.` and `..` are taken out. An
/// empty `dir` holds nothing.
pub fn inside_dir(dir: &Path, file: &Path) -> bool {
    if dir.as_os_str().is_empty() {
        return false;
    }
    match (std::fs::canonicalize(dir), std::fs::canonicalize(file)) {
        (Ok(d), Ok(f)) => f.starts_with(d),
        _ => lexical(file).starts_with(lexical(dir)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_yes_is_by_the_spec_and_the_exact_command() {
        let mut t = Trust::new();
        let spec = Path::new("/w/dash/health.fig");
        assert_eq!(t.command(Some(spec), "./errors.sh"), Answer::Ask);
        t.allow_command(Some(spec), "./errors.sh");
        assert_eq!(t.command(Some(spec), "./errors.sh"), Answer::Yes);
        assert_eq!(
            t.command(Some(Path::new("/w/dash/./health.fig")), "./errors.sh"),
            Answer::Yes,
            "the same path however it's spelt"
        );
        assert_eq!(t.command(Some(spec), "./errors.sh "), Answer::Ask);
        assert_eq!(
            t.command(Some(Path::new("/w/other.fig")), "./errors.sh"),
            Answer::Ask
        );
        assert_eq!(t.command(None, "./errors.sh"), Answer::Ask);
    }

    #[test]
    fn a_no_holds_for_the_session_until_r_and_isnt_kept() {
        let mut t = Trust::new();
        let spec = Path::new("/w/a.fig");
        t.decline_command(Some(spec), "./x.sh");
        assert_eq!(t.command(Some(spec), "./x.sh"), Answer::No);
        assert_eq!(t.to_value(), Value::Null, "a no isn't kept");
        t.ask_again();
        assert_eq!(t.command(Some(spec), "./x.sh"), Answer::Ask);
        t.decline_command(Some(spec), "./x.sh");
        t.allow_command(Some(spec), "./x.sh");
        assert_eq!(t.command(Some(spec), "./x.sh"), Answer::Yes);
    }

    #[test]
    fn the_yeses_round_trip_through_what_a_restart_keeps() {
        let mut t = Trust::new();
        t.allow_command(Some(Path::new("/w/a.fig")), "./x.sh --all");
        t.allow_command(None, "./unsaved.sh");
        t.allow_file(Path::new("/w/a.fig"), Path::new("/elsewhere/b.fig"));
        let v = t.to_value();
        assert_eq!(
            v,
            json!({
                "commands": [["/w/a.fig", "./x.sh --all"]],
                "files": [["/w/a.fig", "/elsewhere/b.fig"]],
            })
        );
        let back = Trust::from_value(&v);
        assert_eq!(
            back.command(Some(Path::new("/w/a.fig")), "./x.sh --all"),
            Answer::Yes
        );
        assert_eq!(back.command(None, "./unsaved.sh"), Answer::Ask);
        assert_eq!(
            back.file(Path::new("/w/a.fig"), Path::new("/elsewhere/b.fig")),
            Answer::Yes
        );
        let junk = json!({"commands": [["/w/a.fig"], 3, ["", "x"]], "files": "no"});
        assert!(Trust::from_value(&junk).is_empty());
    }

    #[test]
    fn a_file_inside_its_specs_directory_needs_no_yes() {
        let t = Trust::new();
        let spec = Path::new("/w/dash/health.fig");
        assert_eq!(t.file(spec, Path::new("/w/dash/panels/x.fig")), Answer::Yes);
        assert_eq!(
            t.file(spec, Path::new("/w/dash/../x.fig")),
            Answer::Ask,
            "out by `..`"
        );
        assert_eq!(t.file(spec, Path::new("/w/x.fig")), Answer::Ask);
        let e = not_run("./x.sh");
        assert_eq!(
            (e.msg.as_str(), e.kind),
            ("not run: you said no", ErrorKind::Unreadable)
        );
    }
}
