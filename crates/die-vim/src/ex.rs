//! The `:` command line. K11a gives it its input; V2 and V5 fill in what it does.
use crate::grammar::{Kind, buffer_window, run_motion};
use crate::motions::goto_line;
use die_core::{Args, BufferId, Ctx, Error, PromptSpec, Result, ShowIn};
use std::path::{Path, PathBuf};

/// `:`: read a line and run it. `<BS>` on an empty line leaves it, as in your Evil.
pub fn open(ctx: &mut Ctx, _: &Args) -> Result {
    let spec = PromptSpec::new(":").history("ex").backspace_cancels();
    ctx.prompt(spec, |ctx, answer| match answer {
        Some(text) => run(ctx, &text),
        None => Ok(()),
    });
    Ok(())
}

/// Run an ex line: `:N`, one of die-vim's own, or a command's name with the rest of the line
/// as its [`Args::text`]. Any command in the registry can be called this way (contract §6).
pub fn run(ctx: &mut Ctx, text: &str) -> Result {
    let text = text.trim().trim_start_matches(':');
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
