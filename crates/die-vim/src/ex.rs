//! The `:` command line. K11a gives it its input; V2 and V5 fill in what it does.
use crate::grammar::{Kind, buffer_window, run_motion};
use crate::motions::goto_line;
use crate::{each, registers, substitute, visual, window};
use die_core::{
    Args, BufferId, Ctx, Error, FileTarget, Origin, PromptSpec, Result, ShowIn, Side, project_root,
    recent_first,
};
use std::ffi::OsStr;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// What an ex command does, given whether a `!` follows its name, and the rest of the line.
type ExFn = fn(&mut Ctx, bool, &str) -> Result;

/// Ex commands, as `w[rite]` gives the shortest name and the whole one, and what they run. `:`'s
/// list shows the whole names. `:s`, `:N`, `:$` and the window commands are read before these, and
/// a command read before them adds its names in [`ex_names`], so the list has it too.
#[rustfmt::skip]
const EX: &[(&str, &str, ExFn)] = &[
    ("w", "write", |c, bang, rest| file_command(c, "w", bang, rest)),
    ("q", "quit", |c, bang, rest| file_command(c, "q", bang, rest)),
    ("wq", "wq", |c, bang, rest| file_command(c, "wq", bang, rest)),
    ("x", "xit", |c, bang, rest| file_command(c, "wq", bang, rest)),
    ("up", "update", |c, bang, rest| file_command(c, "w", bang, rest)),
    ("wa", "wall", |c, bang, _| write_all(c, bang)),
    ("qa", "qall", |c, bang, _| window::quit_app(c, bang)),
    ("quita", "quitall", |c, bang, _| window::quit_app(c, bang)),
    ("wqa", "wqall", |c, bang, _| write_all(c, bang).and_then(|_| window::quit_app(c, false))),
    ("xa", "xall", |c, bang, _| write_all(c, bang).and_then(|_| window::quit_app(c, false))),
    ("sav", "saveas", |c, bang, rest| write(c, buffer_window(c, &Args::default())?.0, rest, bang, true)),
    ("f", "file", |c, _, _| show_file_info(c, &Args::default())),
    ("ls", "ls", |c, _, _| list_buffers(c, false)),
    ("buffers", "buffers", |c, _, _| list_buffers(c, false)),
    ("files", "files", |c, _, _| list_buffers(c, true)),
    ("reg", "registers", |c, _, rest| show_registers(c, rest)),
    ("di", "display", |c, _, rest| show_registers(c, rest)),
    ("bn", "bnext", |c, _, _| window::buffer_step(c, true)),
    ("bp", "bprevious", |c, _, _| window::buffer_step(c, false)),
    ("bN", "bNext", |c, _, _| window::buffer_step(c, false)),
    ("bd", "bdelete", |c, bang, _| window::delete_buffer(c, bang)),
    ("bw", "bwipeout", |c, bang, _| window::delete_buffer(c, bang)),
    ("u", "undo", |c, _, _| c.run("vim-undo", &Args::default())),
    ("e", "edit", |c, bang, rest| if bang { file_command(c, "e", true, rest) } else { edit(c, rest) }),
    ("b", "buffer", |c, _, rest| buffer(c, rest)),
    ("cn", "cnext", |c, _, _| c.run("next-error", &Args::default())),
    ("cp", "cprevious", |c, _, _| c.run("previous-error", &Args::default())),
    ("noh", "nohlsearch", |c, _, _| { c.set_search_highlight(None); Ok(()) }),
];

/// Whether `name` is the command with this shortest and whole name, as `wri` is `w[rite]`.
pub(crate) fn abbreviates(name: &str, (short, whole): (&str, &str)) -> bool {
    name.len() >= short.len() && whole.starts_with(name)
}

/// The table's command that `name` names, with a `!` after it or not.
fn ex_command(name: &str) -> Option<&'static (&'static str, &'static str, ExFn)> {
    let bare = name.strip_suffix('!').unwrap_or(name);
    EX.iter().find(|e| abbreviates(bare, (e.0, e.1)))
}

/// Every ex command's shortest and whole names, for `:`'s list: the table's, the window commands'
/// and `:s`'s. A command `run` reads before the table goes here too.
fn ex_names() -> impl Iterator<Item = (&'static str, &'static str)> {
    let (own, windows) = (EX.iter(), window::EX.iter());
    let windows = windows.map(|e| (e.0, e.1));
    own.map(|e| (e.0, e.1))
        .chain(windows)
        .chain([substitute::NAME])
        .chain(each::NAMES)
}

/// Where an ex line's command name is: after any `:`, blanks and range, up to what isn't a name's
/// letter, digit, `-` or `_`. A range is read loosely, as it's typed: `'` and the key after it
/// are a mark.
fn command_at(line: &str) -> Range<usize> {
    let mut at = 0;
    while let Some(c) = line[at..].chars().next() {
        at += match c {
            '\'' => 1 + line[at + 1..].chars().next().map_or(0, char::len_utf8),
            ':' | ' ' | '\t' | '0'..='9' | '.' | '$' | '%' | ',' | ';' | '+' | '-' => 1,
            _ => break,
        };
    }
    let name = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    at..line.len() - line[at..].trim_start_matches(name).len()
}

/// `:`'s list: the ex commands by their whole names and every other command, those `:` ran last
/// first, as [`recent_first`] orders them.
fn names(ctx: &Ctx) -> Vec<String> {
    let all = ex_names().map(|n| n.1).chain(ctx.command_names());
    let mut all: Vec<String> = all.map(String::from).collect();
    all.sort_unstable();
    all.dedup();
    let run = ctx.prompt_history("ex").iter().filter_map(|line| {
        let name = &line[command_at(line)];
        let ex = ex_names().find(|&n| abbreviates(name, n));
        (!name.is_empty()).then(|| ex.map_or(name, |n| n.1).to_string())
    });
    recent_first(&run.collect::<Vec<_>>(), all)
}

/// `:`: read a line and run it, with your Evil's line keys. `<BS>` on an empty line leaves it,
/// as your Evil's `evil-ex-delete-backward-char` does. From a selection, the selection ends and
/// the line starts with its range. While the line is a command's name, after any range, a list
/// of commands matches it, as the command palette's does.
pub fn open(ctx: &mut Ctx, a: &Args) -> Result {
    let mut spec = PromptSpec::new(":")
        .history("ex")
        .backspace_cancels()
        .evil_keys()
        .candidates(names(ctx))
        .matches_part(|line| Some(command_at(line)).filter(|r| r.end == line.len()));
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

/// Run an ex line: `:s`, `:norm` or `:g` with its range, `:N`, one of die-vim's own, or a
/// command's name with the rest of the line as its [`Args::text`]. Any command in the registry can
/// be called this way (contract §6), and from a selection: the others take no range, so it's
/// ignored.
pub fn run(ctx: &mut Ctx, text: &str) -> Result {
    let line = text.trim_start().trim_start_matches(':').trim_start();
    let text = line.trim_end();
    if let Some((range, rest)) = substitute::parse(text) {
        // Your Evil gives `:s` `:jump`, and runs it with the hook a key's command gets. `:N` has
        // no command, so it isn't a jump.
        crate::marks::before(ctx, &Args::default(), true);
        return substitute::run(ctx, range, rest);
    }
    // `:norm`'s keys keep the spaces at their end, as your Evil's do.
    let (lines, rest) = substitute::range(line);
    if let Some(done) = each::ex(ctx, &lines, rest) {
        return done;
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
    let line = name.parse::<usize>().ok().filter(|_| digits);
    if let Some(n) = line.or((name == "$").then_some(usize::MAX)) {
        return run_motion(ctx, &Args::default(), Kind::Linewise, None, |s| {
            goto_line(s, n)
        });
    }
    if let Some(&(.., f)) = ex_command(name) {
        return f(ctx, name.ends_with('!'), rest);
    }
    let args = Args {
        text: (!rest.is_empty()).then(|| rest.to_string()),
        ..Default::default()
    };
    ctx.run(name, &args)
}

/// The line without the range that only `:s` reads. `:'<,'>w` writes only those lines in your
/// Evil, and that isn't built, so it's refused: writing the whole file would be wrong.
fn without_range(text: &str) -> Result<&str> {
    let Some(rest) = text.strip_prefix(substitute::VISUAL) else {
        return Ok(text.strip_prefix('%').map_or(text, str::trim_start));
    };
    let rest = rest.trim_start();
    let name = rest.split_whitespace().next().unwrap_or_default();
    match ex_command(name).map(|e| e.1) {
        Some("write" | "wq") => Err(Error::Msg(format!(
            ":{}{name} isn't built",
            substitute::VISUAL
        ))),
        _ => Ok(rest),
    }
}

/// `:w`, `:q`, `:wq` and `:e!`. In `:w!` and `:wq!`, `!` writes over a file that changed, or
/// over the one named. In `:q!` it drops unsaved changes. `:wq!` still refuses to leave another
/// file's, as your Evil's does.
fn file_command(ctx: &mut Ctx, name: &str, force: bool, rest: &str) -> Result {
    let writes = name.starts_with('w');
    if !rest.is_empty() && !writes {
        return Err(Error::Msg(format!(":{name} with an argument isn't built")));
    }
    let (buf, w) = buffer_window(ctx, &Args::default())?;
    if writes {
        write(ctx, buf, rest, force, false)?;
    }
    match name {
        "w" => Ok(()),
        "e" => ctx.revert_file(buf),
        name => window::close_or_quit(ctx, w, force && name != "wq"),
    }
}

/// `:w path` and `:saveas path` as your Evil's, with a path taken as `:e` takes it. A buffer with
/// no file then visits it, as it does after `:saveas`, and `:w` from one with a file writes a copy
/// and leaves it as it was, unsaved changes and all. Only `!` writes over what's there. The
/// buffer's own file is `:w`. A read-only buffer with no file, as `*grep*`, writes a copy.
fn write(ctx: &mut Ctx, buf: BufferId, path: &str, force: bool, saveas: bool) -> Result {
    if path.starts_with(['>', '!']) {
        return Err(Error::Msg(format!(":w {path} isn't built")));
    }
    let has_file = ctx.file(buf).is_some();
    let visit = if has_file {
        saveas
    } else {
        !ctx.text(buf)?.settings().read_only
    };
    match (path, has_file, force) {
        ("", false, _) => Err(Error::Msg(
            "Please specify a file name for the buffer".into(),
        )),
        ("", true, false) => ctx.save(buf),
        ("", true, true) => ctx.save_force(buf),
        _ => ctx.save_to(buf, &absolute(ctx.directory(buf), path), force, visit),
    }
}

/// `:f` and `C-g`, as your Evil's `evil-show-file-info`: the file, its lines, and how far down
/// the cursor's line is. An argument is ignored, as there.
pub fn show_file_info(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let t = ctx.text(b)?;
    let rows = t.line_count();
    let lines = rows - usize::from(t.line_start(rows - 1) == t.len());
    let at = t.line_of(t.cursor(w).unwrap_or(0)) + 1;
    let percent = match lines {
        0 => "No lines in buffer".to_string(),
        n => format!("{}%", (at as f64 / n as f64 * 100.0) as u64),
    };
    let line = match ctx.file(b) {
        None => format!("{lines} lines --{percent}--"),
        Some(f) => {
            let locked = std::fs::metadata(&f.path).is_ok_and(|m| m.permissions().readonly());
            let ro = if locked || f.read_only.is_some() {
                "[readonly] "
            } else {
                ""
            };
            format!("\"{}\" {lines} {ro}lines --{percent}--", f.path.display())
        }
    };
    ctx.messages.push(line);
    Ok(())
}

/// `:ls`: a line in `*messages*` for each buffer, the one used last first, which the window
/// then shows, as your Evil's `buffer-menu` takes it. `.` is this one, `%` read-only, `*` unsaved.
/// `:files` lists only those with a file.
fn list_buffers(ctx: &mut Ctx, files_only: bool) -> Result {
    let here = ctx.focused_buffer();
    let mut lines = vec![];
    for b in ctx.buffer_list() {
        let file = ctx.file(b).map(|f| f.path.display().to_string());
        let t = ctx.text(b).ok();
        let mark = |on: bool, c: char| if on { c } else { ' ' };
        let ro = mark(t.is_some_and(|t| t.settings().read_only), '%');
        let unsaved = mark(file.is_some() && t.is_some_and(|t| t.modified()), '*');
        let name = ctx.buffer_name(b).unwrap_or_default();
        let line = format!("{}{ro}{unsaved} {name:<20} ", mark(here == Some(b), '.'));
        if file.is_some() || !files_only {
            lines.push(line + &file.unwrap_or_default());
        }
    }
    ctx.messages
        .extend(lines.iter().map(|l| l.trim_end().into()));
    let log = ctx.messages_buffer();
    if here != Some(log) {
        ctx.show(log, ShowIn::Current)?;
    }
    Ok(())
}

/// `:reg` and `:di`, as your Evil's `evil-show-registers`: `*registers*` below, with a row for
/// each register and what it holds, or for those `names` names. A newline in text shows as `^J`,
/// and a recording as its keys, in the notation `@` reads. Your Evil's table has the same columns,
/// with its header on a line of its own.
fn show_registers(ctx: &mut Ctx, names: &str) -> Result {
    let mut text = format!("{:<10} Value\n", "Register");
    for (c, held) in registers::listing(ctx) {
        if names.is_empty() || names.contains(c) {
            text += &format!("{c:<10} {}\n", held.replace('\n', "^J"));
        }
    }
    let name = "*registers*";
    let b = match ctx.find_buffer(name) {
        Some(b) => b,
        None => ctx.create_text(name, ""),
    };
    let t = ctx.text_mut(b)?;
    t.settings_mut().read_only = true;
    let all = 0..t.len();
    t.tx(Origin::Extension)
        .silent()
        .replace(all, text)
        .commit()?;
    t.set_modified(false);
    let shown = ctx
        .window_list()
        .into_iter()
        .any(|w| ctx.window_buffer(w) == Some(b));
    let how = if shown {
        ShowIn::Existing
    } else {
        ShowIn::Split(Side::Below)
    };
    if let Some(w) = ctx.show(b, how)? {
        ctx.text_mut(b)?.set_cursor(w, 0);
    }
    Ok(())
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
    let home = path.strip_prefix("~/").or((path == "~").then_some(""));
    if let (Some(rest), Some(home)) = (home, std::env::var_os("HOME")) {
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
    let start = line[..at].trim_end_matches(names).len();
    let end = line.len() - line[at..].trim_start_matches(names).len();
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
