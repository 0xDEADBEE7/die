//! Core's commands on the command line (contract §7): each asks with a prompt and acts on the
//! answer. To add one, register it in [`register_commands`] and give the prompt its candidates
//! with [`PromptSpec::candidates`].
use crate::dispatch::*;
use crate::ext::*;
use crate::prompt::PromptSpec;

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
}
