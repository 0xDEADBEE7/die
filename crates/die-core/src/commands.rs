//! Core's commands on the command line (contract §7): each asks with a prompt and acts on the
//! answer. To add one, register it in [`register_commands`] and give the prompt its candidates
//! with [`PromptSpec::candidates`].
use crate::ShowIn;
use crate::dispatch::*;
use crate::error::Error;
use crate::ext::*;
use crate::files::normalize;
use crate::prompt::{PromptSpec, recent_first};
use std::path::{Path, PathBuf};

mod describe;
pub use describe::{CommandDescription, KeyAction, KeyDescription};

impl Ctx {
    /// Every command's name, sorted.
    pub fn command_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.commands.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
}

pub(crate) fn register_commands(reg: &mut Registry) {
    describe::register(reg);
    reg.command(
        "command-palette",
        "Choose a command by name and run it, with this command's count and register",
        |ctx, a| {
            let args = Args {
                keys: vec![],
                ..a.clone()
            };
            let names = ctx.command_names().iter().map(|n| n.to_string()).collect();
            let spec = PromptSpec::new("Command: ")
                .history("command")
                .require_match()
                .candidates(recent_first(ctx.prompt_history("command"), names));
            ctx.prompt(spec, move |ctx, name| {
                match name.filter(|n| !n.is_empty()) {
                    Some(name) => ctx.run(&name, &args),
                    None => Ok(()),
                }
            });
            Ok(())
        },
    );
    reg.command(
        "find-file",
        "Open a file, starting from this buffer's directory, going down through directories on the way",
        |ctx, _| {
            let cwd = std::env::current_dir().unwrap_or_else(|_| "/".into());
            let here = ctx.focused_buffer().and_then(|b| ctx.directory(b));
            let dir = here.map_or(cwd, Path::to_path_buf);
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let line = line_for(&dir, home.as_deref());
            ask_for_file(ctx, dir, home, &line);
            Ok(())
        },
    );
    reg.command(
        "switch-buffer",
        "Show a buffer in this window, and make a new one for a name no buffer has",
        |ctx, _| {
            // As in Emacs, `<CR>` with nothing typed goes back to the last buffer.
            let (here, others) = buffer_names(ctx);
            let last = others.first().cloned();
            let mut names = recent_first(ctx.prompt_history("buffer"), others);
            if let Some(at) = names.iter().position(|n| Some(n) == last.as_ref()) {
                names[..=at].rotate_right(1);
            }
            names.extend(here);
            let spec = PromptSpec::new("Switch to buffer: ")
                .history("buffer")
                .candidates(names);
            ctx.prompt(spec, |ctx, name| {
                let Some(name) = name.filter(|n| !n.is_empty()) else {
                    return Ok(());
                };
                let buf = match ctx.find_buffer(&name) {
                    Some(buf) => buf,
                    None => ctx.create_text(&name, ""),
                };
                ctx.show(buf, ShowIn::Current).map(drop)
            });
            Ok(())
        },
    );
    reg.command(
        "kill-buffer",
        "Kill a buffer, by default this one, asking first if it has unsaved changes to a file",
        |ctx, _| {
            let (here, others) = buffer_names(ctx);
            let others = recent_first(ctx.prompt_history("buffer"), others);
            let spec = PromptSpec::new("Kill buffer: ")
                .history("buffer")
                .candidates(here.into_iter().chain(others));
            ctx.prompt(spec, |ctx, name| {
                let Some(name) = name.filter(|n| !n.is_empty()) else {
                    return Ok(());
                };
                let buf = ctx
                    .find_buffer(&name)
                    .ok_or_else(|| Error::Msg(format!("No buffer named {name}")))?;
                let unsaved = ctx.file(buf).is_some() && ctx.text(buf).is_ok_and(|t| t.modified());
                if !unsaved {
                    ctx.kill(buf);
                    return Ok(());
                }
                let label = format!("Buffer {name} modified; kill anyway? (yes or no) ");
                ctx.prompt(PromptSpec::new(&label), move |ctx, answer| {
                    if answer.as_deref() == Some("yes") {
                        ctx.kill(buf);
                    }
                    Ok(())
                });
                Ok(())
            });
            Ok(())
        },
    );
}

/// Ask for a file, from this line. There's no directory editor to open, so answering with a
/// directory, as `<CR>` on one does, asks again from there.
fn ask_for_file(ctx: &mut Ctx, dir: PathBuf, home: Option<PathBuf>, line: &str) {
    let spec = PromptSpec::new("Find file: ")
        .initial(line)
        .history("file")
        .descends()
        .candidates_from(listing(dir.clone(), home.clone()));
    ctx.prompt(spec, move |ctx, answer| {
        let Some(answer) = answer.filter(|a| !a.is_empty()) else {
            return Ok(());
        };
        let path = resolve(&dir, home.as_deref(), &answer);
        if path.is_dir() {
            let slash = if answer.ends_with('/') { "" } else { "/" };
            ask_for_file(ctx, dir, home, &format!("{answer}{slash}"));
            return Ok(());
        }
        let file = ctx.open_file(&path)?;
        ctx.show(file, ShowIn::Current)?;
        Ok(())
    });
}

/// The prompt's starting line for a directory: ending in `/`, and with `~` for the home directory
/// as Emacs abbreviates it.
fn line_for(dir: &Path, home: Option<&Path>) -> String {
    let shown = home
        .and_then(|h| dir.strip_prefix(h).ok())
        .map(|rest| Path::new("~").join(rest));
    let line = shown.as_deref().unwrap_or(dir).to_string_lossy();
    format!("{}/", line.trim_end_matches('/'))
}

/// What a line means: only what's from its last restart counts, `~` is the home directory, a
/// relative path starts from `dir`, and `..` goes by name, as [`Ctx::open_file`] takes it.
fn resolve(dir: &Path, home: Option<&Path>, line: &str) -> PathBuf {
    let line = restarted(line);
    normalize(&match (line.strip_prefix('~'), home) {
        (Some(""), Some(home)) => home.to_path_buf(),
        (Some(rest), Some(home)) if rest.starts_with('/') => home.join(&rest[1..]),
        _ => dir.join(line),
    })
}

/// The line from where it last starts again, as Emacs's `substitute-in-file-name` has it: the
/// second slash of a `//`, or the `~` of a `/~/`. The start line is a full path, so typing over
/// it is how to go elsewhere. A `/~` at the end counts too, and a `~name` is only a name.
fn restarted(line: &str) -> &str {
    let b = line.as_bytes();
    let starts = |&i: &usize| {
        b[i - 1] == b'/'
            && (b[i] == b'/' || b[i] == b'~' && matches!(b.get(i + 1), None | Some(b'/')))
    };
    (1..b.len()).rev().find(starts).map_or(line, |i| &line[i..])
}

/// The candidates for the line: the entries of the directory it names up to its last `/`. That
/// directory's listing is kept until the line names another.
fn listing(dir: PathBuf, home: Option<PathBuf>) -> impl FnMut(&str) -> (usize, Vec<String>) {
    let mut kept: Option<(String, Vec<String>)> = None;
    move |line| {
        let base = line.rfind('/').map_or(0, |i| i + 1);
        let named = &line[..base];
        match &kept {
            Some((name, entries)) if name == named => (base, entries.clone()),
            _ => {
                let entries = entries_of(&resolve(&dir, home.as_deref(), named));
                kept = Some((named.to_string(), entries.clone()));
                (base, entries)
            }
        }
    }
}

/// A directory's entries by name, hidden ones last, each directory ending in `/`. That includes a
/// symlink to one. A name that isn't UTF-8 can't go on the line, so it isn't offered.
fn entries_of(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut entries: Vec<String> = read
        .flatten()
        .filter_map(|e| {
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir())
                || e.file_type().is_ok_and(|t| t.is_symlink()) && e.path().is_dir();
            let name = e.file_name().into_string().ok()?;
            Some(if is_dir { name + "/" } else { name })
        })
        .collect();
    entries.sort_unstable_by(|a, b| (a.starts_with('.'), a).cmp(&(b.starts_with('.'), b)));
    entries
}

/// The focused buffer's name, and every other buffer's, the one used last first.
fn buffer_names(ctx: &Ctx) -> (Option<String>, Vec<String>) {
    let name = |b| ctx.buffer_name(b).map(str::to_string);
    let here = ctx.focused_buffer();
    let others = ctx.buffer_list().into_iter().filter(|b| Some(*b) != here);
    (here.and_then(name), others.filter_map(name).collect())
}
