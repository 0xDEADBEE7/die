//! die-term in the facade: registered wherever its engine is, which is macOS until DIE's CI
//! builds a Linux archive (D10, D12), and what quitting and restarting ask while programs run in
//! terminals (scope D18). Terminals don't survive a restart until D20's holder and helper, so a
//! restart hangs their programs up, as a quit does, and the session brings back no terminal.
use die_core::{Ctx, Extension, PromptSpec, Registry, Result};

/// die-term, with an OSC 52 write made a kill, as Emacs's `kill-new` makes it.
#[cfg(target_os = "macos")]
pub(crate) fn extension() -> Option<Box<dyn Extension>> {
    let terminals = die_term::Terminals::default().copying_with(die_vim::kill_new);
    Some(Box::new(terminals))
}

/// No engine here, so no terminals (D10).
#[cfg(not(target_os = "macos"))]
pub(crate) fn extension() -> Option<Box<dyn Extension>> {
    None
}

/// Where there are no terminals, the commands that open one say why, so a config that binds them
/// still starts.
pub(crate) fn register(reg: &mut Registry) {
    if cfg!(target_os = "macos") {
        return;
    }
    for name in ["term", "terminal", "term-switch"] {
        reg.command(
            name,
            "Open a terminal, which this platform has none of yet",
            |_, _| {
                Err(die_core::Error::Msg(
                    "No terminals here yet: this platform has no engine archive (D10)".into(),
                ))
            },
        );
    }
}

/// The terminals with a program other than their shell running in them, by name.
#[cfg(target_os = "macos")]
pub fn busy(ctx: &mut Ctx) -> Vec<String> {
    die_term::busy(ctx)
}

/// No terminals, so none busy.
#[cfg(not(target_os = "macos"))]
pub fn busy(_: &mut Ctx) -> Vec<String> {
    vec![]
}

/// What's about to end the terminals' programs.
#[derive(Clone, Copy)]
pub enum Ending {
    /// Quitting.
    Quit,
    /// `restart`.
    Restart,
    /// `rebuild`, which restarts once it has built.
    Rebuild,
}

/// What die-vim asks before it quits (`Vim::asking_before_quit`): Cmd-Q's question, for `:q` in
/// the last window and `:qa` too, with `!` as well, since `!` speaks only for unsaved text.
pub(crate) fn before_quit(ctx: &mut Ctx, quit: Box<dyn FnOnce(&mut Ctx) -> Result>) -> Result {
    unless_busy(ctx, Ending::Quit, quit)
}

/// Do `then` now if no terminal has a program running, or else once you say yes (scope D18's
/// Cmd-Q). A shell at its prompt doesn't count.
pub fn unless_busy(
    ctx: &mut Ctx,
    ending: Ending,
    then: impl FnOnce(&mut Ctx) -> Result + 'static,
) -> Result {
    let names = busy(ctx);
    if names.is_empty() {
        return then(ctx);
    }
    let running = match names.len() {
        1 => format!("A program is running in {}", names[0]),
        _ => format!("Programs are running in {}", names.join(", ")),
    };
    let label = match ending {
        Ending::Quit => format!("{running}. Quit anyway? (y or n) "),
        Ending::Restart => format!(
            "{running}, and terminals don't survive a restart yet. Restart anyway? (y or n) "
        ),
        Ending::Rebuild => format!(
            "{running}, and terminals don't survive the restart after a rebuild yet. Rebuild \
             anyway? (y or n) "
        ),
    };
    ctx.prompt(PromptSpec::new(&label), move |ctx, answer| {
        match answer.as_deref() {
            Some("y" | "yes") => then(ctx),
            _ => Ok(()),
        }
    });
    Ok(())
}
