//! The `:`, `/` and `?` command lines. K11a gives each its input; V2 and V5 fill in what they do.
use crate::{substitute, visual};
use die_core::{Args, Ctx, Error, PromptSpec, Result};

/// A command line's prompt: `<BS>` on an empty line leaves it, as in your Evil.
fn line(label: &str, history: &str) -> PromptSpec {
    PromptSpec::new(label).history(history).backspace_cancels()
}

/// `:`: read a line and run it. From a selection, the selection ends and the line starts with
/// its range.
pub fn open(ctx: &mut Ctx, a: &Args) -> Result {
    let mut spec = line(":", "ex");
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

/// Run an ex line: `:s` with its range, a file command, or a command's name, then the rest of
/// the line as its [`Args::text`]. Any command in the registry can be called this way (contract
/// §6).
pub fn run(ctx: &mut Ctx, text: &str) -> Result {
    let text = text.trim().trim_start_matches(':');
    if let Some((range, rest)) = substitute::parse(text) {
        return substitute::run(ctx, range, rest);
    }
    let (name, rest) = text.split_once(' ').unwrap_or((text, ""));
    if name.is_empty() {
        return Ok(());
    }
    if let Some(&(_, write, then)) = FILE_COMMANDS.iter().find(|f| f.0 == name) {
        if rest.trim().is_empty() {
            return file_command(ctx, write, then);
        }
        return Err(Error::Msg(format!(":{name} with an argument isn't built")));
    }
    let args = Args {
        text: (!rest.is_empty()).then(|| rest.trim().to_string()),
        ..Default::default()
    };
    ctx.run(name, &args)
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

/// `/`: read a pattern, forward.
pub fn search_forward(ctx: &mut Ctx, _: &Args) -> Result {
    ctx.prompt(line("/", "search"), |ctx, answer| search(ctx, answer, true));
    Ok(())
}

/// `?`: read a pattern, backward.
pub fn search_backward(ctx: &mut Ctx, _: &Args) -> Result {
    ctx.prompt(line("?", "search"), |ctx, answer| {
        search(ctx, answer, false)
    });
    Ok(())
}

/// V2 makes this search.
fn search(_: &mut Ctx, answer: Option<String>, _forward: bool) -> Result {
    match answer {
        Some(_) => Err(Error::Msg("search isn't built yet".into())),
        None => Ok(()),
    }
}
