//! The `:` command line. K11a gives it its input; V2 and V5 fill in what it does.
use crate::grammar::{Kind, buffer_window, run_motion};
use crate::motions::goto_line;
use crate::{substitute, visual};
use die_core::{Args, BufferId, Ctx, Error, PromptSpec, Result, ShowIn};
use std::path::{Path, PathBuf};

/// `:`: read a line and run it. `<BS>` on an empty line leaves it, as in your Evil. From a
/// selection, the selection ends and the line starts with its range.
pub fn open(ctx: &mut Ctx, a: &Args) -> Result {
    let mut spec = PromptSpec::new(":").history("ex").backspace_cancels();
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
/// (contract §6).
pub fn run(ctx: &mut Ctx, text: &str) -> Result {
    let text = text.trim().trim_start_matches(':');
    if let Some((range, rest)) = substitute::parse(text) {
        return substitute::run(ctx, range, rest);
    }
    let (name, rest) = text.split_once(' ').unwrap_or((text, ""));
    let rest = rest.trim();
    if name.is_empty() {
        return Ok(());
    }
    if let Ok(n) = name.parse::<usize>() {
        return run_motion(ctx, &Args::default(), Kind::Linewise, None, |s| {
            goto_line(s, n)
        });
    }
    if let Some(&(_, write, then)) = FILE_COMMANDS.iter().find(|f| f.0 == name) {
        if rest.is_empty() {
            return file_command(ctx, write, then);
        }
        return Err(Error::Msg(format!(":{name} with an argument isn't built")));
    }
    match name {
        "e" | "edit" => edit(ctx, rest),
        "b" | "buffer" => buffer(ctx, rest),
        _ => {
            let args = Args {
                text: (!rest.is_empty()).then(|| rest.to_string()),
                ..Default::default()
            };
            ctx.run(name, &args)
        }
    }
}

#[derive(Clone, Copy)]
enum Write {
    No,
    Save,
    Force,
}

/// What a file command does after its write.
#[derive(Clone, Copy)]
enum Then {
    Nothing,
    Revert,
    Quit { force: bool },
}

const FILE_COMMANDS: &[(&str, Write, Then)] = &[
    ("w", Write::Save, Then::Nothing),
    ("w!", Write::Force, Then::Nothing),
    ("q", Write::No, Then::Quit { force: false }),
    ("q!", Write::No, Then::Quit { force: true }),
    ("wq", Write::Save, Then::Quit { force: false }),
    ("wq!", Write::Force, Then::Quit { force: true }),
    ("e!", Write::No, Then::Revert),
];

fn file_command(ctx: &mut Ctx, write: Write, then: Then) -> Result {
    let no_buffer = || Error::Msg("No buffer here".into());
    let buf = ctx.focused_buffer().ok_or_else(no_buffer)?;
    match write {
        Write::No => {}
        Write::Save => ctx.save(buf)?,
        Write::Force => ctx.save_force(buf)?,
    }
    match then {
        Then::Nothing => Ok(()),
        Then::Revert => ctx.revert_file(buf),
        Then::Quit { force } => quit(ctx, force),
    }
}

/// `:q`: close the window, and from the only one, quit. That refuses while a file has unsaved
/// changes, unless `force`.
fn quit(ctx: &mut Ctx, force: bool) -> Result {
    if let Some(w) = ctx.focused()
        && ctx.close_window(w).is_ok()
    {
        return Ok(());
    }
    let unsaved = ctx
        .buffer_list()
        .into_iter()
        .find(|&b| ctx.file(b).is_some() && ctx.text(b).is_ok_and(|t| t.modified()));
    if let Some(b) = unsaved.filter(|_| !force) {
        let name = ctx.buffer_name(b).unwrap_or_default();
        return Err(Error::Msg(format!(
            "{name} has unsaved changes; :w saves them, :q! drops them"
        )));
    }
    ctx.quit();
    Ok(())
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

fn absolute(dir: Option<&Path>, path: &str) -> PathBuf {
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
