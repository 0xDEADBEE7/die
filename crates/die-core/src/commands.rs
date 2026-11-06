//! Core's commands on the command line (contract §7): each asks with a prompt and acts on the
//! answer. To add one, register it in [`register_commands`] and give the prompt its candidates
//! with [`PromptSpec::candidates`].
use crate::ShowIn;
use crate::dispatch::*;
use crate::error::Error;
use crate::ext::*;
use crate::prompt::PromptSpec;

impl Ctx {
    /// Every command's name, sorted.
    pub fn command_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.commands.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
}

pub(crate) fn register_commands(reg: &mut Registry) {
    reg.command(
        "command-palette",
        "Choose a command by name and run it, with this command's count and register",
        |ctx, a| {
            let args = Args {
                keys: vec![],
                ..a.clone()
            };
            let names: Vec<String> = ctx.command_names().iter().map(|n| n.to_string()).collect();
            let spec = PromptSpec::new("Command: ")
                .history("command")
                .candidates(names);
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
        "switch-buffer",
        "Show a buffer in this window, and make a new one for a name no buffer has",
        |ctx, _| {
            // As in Emacs, `<CR>` with nothing typed goes back to the last buffer.
            let mut names = buffer_names(ctx);
            names.rotate_left(1);
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
            let spec = PromptSpec::new("Kill buffer: ")
                .history("buffer")
                .candidates(buffer_names(ctx));
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

/// Every buffer's name, the focused buffer's first and then the one used last first.
fn buffer_names(ctx: &Ctx) -> Vec<String> {
    let mut bufs = ctx.buffer_list();
    bufs.sort_by_key(|b| Some(*b) != ctx.focused_buffer());
    let names = bufs.iter().filter_map(|b| ctx.buffer_name(*b));
    names.map(str::to_string).collect()
}
