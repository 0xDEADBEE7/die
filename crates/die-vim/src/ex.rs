//! The `:` command line. K11a gives it its input; V2 and V5 fill in what it does.
use crate::grammar::{Kind, buffer_window, run_motion};
use crate::motions::goto_line;
use crate::{insert_keys, substitute, visual, window};
use die_core::{Args, BufferId, Ctx, Error, PromptSpec, Result, ShowIn};
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
    let a = a.clone();
    ctx.prompt(spec, move |ctx, answer| {
        let r = match answer {
            Some(text) => run(ctx, &text),
            None => Ok(()),
        };
        insert_keys::after_command(ctx, &a, &r);
        r
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
