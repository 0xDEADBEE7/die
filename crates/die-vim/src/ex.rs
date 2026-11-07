//! The `:` command line. K11a gives it its input; V2 and V5 fill in what it does.
use crate::grammar::{Kind, buffer_window, run_motion};
use crate::motions::goto_line;
use crate::{substitute, visual, window};
use die_core::{Args, BufferId, Ctx, Error, FileTarget, PromptSpec, Result, ShowIn, project_root};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// `:`: read a line and run it, with your Evil's line keys. `<BS>` on an empty line leaves it,
/// as your Evil's `evil-ex-delete-backward-char` does. From a selection, the selection ends and
/// the line starts with its range.
pub fn open(ctx: &mut Ctx, a: &Args) -> Result {
    let mut spec = PromptSpec::new(":")
        .history("ex")
        .backspace_cancels()
        .evil_keys();
    if ctx.focused_buffer().is_some_and(|b| visual::is_on(ctx, b)) {
        visual::escape(ctx, a)?;
        spec = spec.initial(substitute::VISUAL);
    }
    ctx.prompt(spec, |ctx, answer| match answer {
        Some(text) => run(ctx, &text),
        None => Ok(()),
    });
    Ok(())
}

/// Run an ex line: `:s` with its range, `:N`, one of die-vim's own, or a command's name with the
/// rest of the line as its [`Args::text`]. Any command in the registry can be called this way
/// (contract §6), and from a selection: the others take no range, so it's ignored.
pub fn run(ctx: &mut Ctx, text: &str) -> Result {
    let text = text.trim().trim_start_matches(':').trim_start();
    if let Some((range, rest)) = substitute::parse(text) {
        // Your Evil gives `:s` `:jump`, and runs it with the hook a key's command gets. `:N` has
        // no command, so it isn't a jump.
        crate::marks::before(ctx, &Args::default(), true);
        return substitute::run(ctx, range, rest);
    }
    let text = without_range(text)?;
    if let Some(done) = window::ex(ctx, text) {
        return done;
    }
    let (name, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let rest = rest.trim();
    if name.is_empty() {
        return Ok(());
    }
    // Only digits: `parse` would take `+5` for line 5.
    let digits = name.bytes().all(|c| c.is_ascii_digit());
    if let Some(n) = name.parse::<usize>().ok().filter(|_| digits) {
        return run_motion(ctx, &Args::default(), Kind::Linewise, None, |s| {
            goto_line(s, n)
        });
    }
    match name {
        "w" | "w!" | "q" | "q!" | "quit" | "quit!" | "wq" | "wq!" | "e!" => {
            file_command(ctx, name, rest)
        }
        "x" | "xit" => file_command(ctx, "wq", rest),
        "x!" | "xit!" => file_command(ctx, "wq!", rest),
        "wa" | "wall" | "wa!" | "wall!" => write_all(ctx, name.ends_with('!')),
        "qa" | "qall" | "quitall" => window::quit_app(ctx, false),
        "qa!" | "qall!" | "quitall!" => window::quit_app(ctx, true),
        "wqa" | "wqall" | "xa" | "xall" | "wqa!" | "wqall!" | "xa!" | "xall!" => {
            write_all(ctx, name.ends_with('!'))?;
            window::quit_app(ctx, false)
        }
        "e" | "edit" => edit(ctx, rest),
        "b" | "buffer" => buffer(ctx, rest),
        "cn" | "cne" | "cnex" | "cnext" => ctx.run("next-error", &Args::default()),
        "cp" | "cpr" | "cpre" | "cprev" | "cprevi" | "cprevio" | "cpreviou" | "cprevious" => {
            ctx.run("previous-error", &Args::default())
        }
        "noh" | "nohl" | "nohlsearch" => {
            ctx.set_search_highlight(None);
            Ok(())
        }
        _ => {
            let args = Args {
                text: (!rest.is_empty()).then(|| rest.to_string()),
                ..Default::default()
            };
            ctx.run(name, &args)
        }
    }
}

/// The line without the range that only `:s` reads. `:'<,'>w` writes only those lines in your
/// Evil, and that isn't built, so it's refused: writing the whole file would be wrong.
fn without_range(text: &str) -> Result<&str> {
    let Some(rest) = text.strip_prefix(substitute::VISUAL) else {
        return Ok(text.strip_prefix('%').map_or(text, str::trim_start));
    };
    let rest = rest.trim_start();
    match rest.split_whitespace().next() {
        Some(name @ ("w" | "w!" | "wq" | "wq!")) => Err(Error::Msg(format!(
            ":{}{name} isn't built",
            substitute::VISUAL
        ))),
        _ => Ok(rest),
    }
}

/// `:w`, `:q`, `:wq` and `:e!`. In `:w!` and `:wq!`, `!` writes over a file that changed. In `:q!`
/// it drops unsaved changes. `:wq!` still refuses to leave another file's, as your Evil's does.
fn file_command(ctx: &mut Ctx, name: &str, rest: &str) -> Result {
    if !rest.is_empty() {
        return Err(Error::Msg(format!(":{name} with an argument isn't built")));
    }
    let (buf, w) = buffer_window(ctx, &Args::default())?;
    let force = name.ends_with('!');
    if name.starts_with('w') {
        if force {
            ctx.save_force(buf)?;
        } else {
            ctx.save(buf)?;
        }
    }
    match name.trim_end_matches('!') {
        "w" => Ok(()),
        "e" => ctx.revert_file(buf),
        name => window::close_or_quit(ctx, w, force && name != "wq"),
    }
}

/// `:wa`: every file with unsaved changes, on past one that's refused, whose error comes back.
fn write_all(ctx: &mut Ctx, force: bool) -> Result {
    let mut first = Ok(());
    for b in ctx.buffer_list() {
        let unsaved = ctx.file(b).is_some() && ctx.text(b).is_ok_and(|t| t.modified());
        if unsaved {
            let saved = if force {
                ctx.save_force(b)
            } else {
                ctx.save(b)
            };
            first = first.and(saved);
        }
    }
    first
}

/// `:e path`: a path is taken from the buffer's directory, as your Evil's is, or else from
/// where DIE started. `~/` is the home directory.
fn edit(ctx: &mut Ctx, path: &str) -> Result {
    if path.is_empty() {
        return Err(Error::Msg("No file name".into()));
    }
    let (b, _) = buffer_window(ctx, &Args::default())?;
    let path = absolute(ctx.directory(b), path);
    let file = ctx.open_file(&path)?;
    ctx.show(file, ShowIn::Current)?;
    Ok(())
}

pub(crate) fn absolute(dir: Option<&Path>, path: &str) -> PathBuf {
    if let (Some(rest), Some(home)) = (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        return Path::new(&home).join(rest);
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    dir.unwrap_or(&cwd).join(path)
}

/// `:b name`: the buffer of that name, or the one whose name alone contains it. `:b` alone is
/// the buffer used last, as your Evil's is.
fn buffer(ctx: &mut Ctx, name: &str) -> Result {
    let (here, _) = buffer_window(ctx, &Args::default())?;
    let others = |ctx: &Ctx| -> Vec<BufferId> {
        let not_log = |b: &BufferId| ctx.buffer_name(*b) != Some("*messages*");
        ctx.buffer_list().into_iter().filter(not_log).collect()
    };
    let found = if name.is_empty() {
        let last = others(ctx).into_iter().find(|b| *b != here);
        last.ok_or(Error::Msg("No other buffer".into()))?
    } else if let Some(b) = ctx.find_buffer(name) {
        b
    } else {
        let has = |b: &BufferId| ctx.buffer_name(*b).is_some_and(|n| n.contains(name));
        let hits: Vec<BufferId> = others(ctx).into_iter().filter(has).collect();
        match hits[..] {
            [b] => b,
            [] => return Err(Error::Msg(format!("No buffer matching {name}"))),
            _ => return Err(Error::Msg(format!("More than one buffer matches {name}"))),
        }
    };
    ctx.show(found, ShowIn::Current)?;
    Ok(())
}

/// Your Evil's `evil-find-file-at-point-with-line`, which is its `gF` and here `gf` too: the file
/// named under the cursor, as ffap reads a name, from this buffer's directory and then from its
/// project. `:line`, `:line:col`, `(line)` and `(line,col)` after the name say where. A name that
/// isn't a file is an error, as with `gF`.
pub fn find_file_at_point(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text(b)?;
    let cursor = t.cursor(w).unwrap_or(0);
    let start = t.line_start(t.line_of(cursor));
    let line = t.rope().line(t.line_of(cursor)).to_string();
    let missing = || Error::Msg("File does not exist.".into());
    let (name, after) = name_at(&line, cursor - start).ok_or_else(missing)?;
    let dir = absolute(ctx.directory(b), "");
    let name = match (name.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest).into_os_string(),
        _ => name.into(),
    };
    let tries = [dir.clone(), project_root(&dir)];
    let mut targets = tries
        .iter()
        .map(|base| FileTarget::parse(OsStr::new(&name), base));
    let mut target = targets.find(|t| t.path.is_file()).ok_or_else(missing)?;
    if let (None, Some((line, col))) = (target.line, in_parens(after)) {
        (target.line, target.col) = (Some(line), col);
    }
    ctx.open_target(&target).map(drop)
}

/// Your Evil's `evil-find-file-at-point-visual`: the selection is the whole name, with no position
/// read from it, tried from the same places as `gf`. Failing, the selection stays.
pub fn find_file_selected(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let name = visual::region_text(ctx, a)?;
    let dir = absolute(ctx.directory(b), "");
    let tries = [dir.clone(), project_root(&dir)].map(|base| absolute(Some(&base), &name));
    let Some(path) = tries.into_iter().find(|p| p.is_file()) else {
        return Err(Error::Msg(format!("Can't find file \"{name}\" in path")));
    };
    visual::escape(ctx, a)?;
    let (line, col) = (None, None);
    ctx.open_target(&FileTarget { path, line, col }).map(drop)
}

/// The file name around byte `at` of `line`, and what comes after it, with the characters ffap
/// takes for a file (`ffap-string-at-point-mode-alist`), and without those it strips from the ends.
fn name_at(line: &str, at: usize) -> Option<(&str, &str)> {
    let names =
        |c: char| c.is_alphabetic() || c.is_ascii_digit() || "-./:\\${}+<>@_~*?#".contains(c);
    let back = line[..at].rfind(|c| !names(c));
    let start = back.map_or(0, |i| {
        i + line[i..].chars().next().map_or(1, char::len_utf8)
    });
    let end = line[at..]
        .find(|c| !names(c))
        .map_or(line.len(), |i| at + i);
    let name = line[start..end].trim_start_matches(['{', '<', '@']);
    let name = name.trim_end_matches(['@', '>', ';', '.', ',', '!', ':', '}']);
    (!name.is_empty()).then_some((name, &line[end..]))
}

/// `(line)` or `(line,col)` at the start of this text.
fn in_parens(text: &str) -> Option<(usize, Option<usize>)> {
    let inner = text.strip_prefix('(')?.split_once(')')?.0;
    match inner.split_once(',') {
        Some((line, col)) => Some((line.parse().ok()?, Some(col.parse().ok()?))),
        None => Some((inner.parse().ok()?, None)),
    }
}
