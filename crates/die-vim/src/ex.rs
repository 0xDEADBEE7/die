//! The `:`, `/` and `?` command lines. K11a gives each its input; V2 and V5 fill in what they do.
use die_core::{Args, Ctx, Error, PromptSpec, Result};

/// A command line's prompt: `<BS>` on an empty line leaves it, as in your Evil.
fn line(label: &str, history: &str) -> PromptSpec {
    PromptSpec::new(label).history(history).backspace_cancels()
}

/// `:`: read a line and run it.
pub fn open(ctx: &mut Ctx, _: &Args) -> Result {
    ctx.prompt(line(":", "ex"), |ctx, answer| match answer {
        Some(text) => run(ctx, &text),
        None => Ok(()),
    });
    Ok(())
}

/// Run an ex line: a command's name, then the rest of the line as its [`Args::text`]. Any
/// command in the registry can be called this way (contract §6).
pub fn run(ctx: &mut Ctx, text: &str) -> Result {
    let text = text.trim().trim_start_matches(':');
    let (name, rest) = text.split_once(' ').unwrap_or((text, ""));
    if name.is_empty() {
        return Ok(());
    }
    let args = Args {
        text: (!rest.is_empty()).then(|| rest.trim().to_string()),
        ..Default::default()
    };
    ctx.run(name, &args)
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
