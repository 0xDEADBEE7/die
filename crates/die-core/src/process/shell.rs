//! `shell-command`, Emacs's `M-!`, and what die-vim's `:!`, `:{range}!` and `:r !` run on. A
//! command runs as Emacs's synchronous `shell-command` does, while it takes less than
//! [`WAIT`]: a key after it waits for it, so a macro of `:%!sort` and `j` works on the sorted
//! lines. One that takes longer goes on in the background, and its output comes when it's done.
//! Its output is cut at the limit, and stops it there, since nothing could show the rest.
use super::{Group, LIMIT, Output, run, shell};
use crate::error::{Error, Result};
use crate::ext::{Ctx, Registry};
use crate::project::here_dir;
use crate::prompt::PromptSpec;
use std::io;
use std::path::Path;
use std::process::ExitStatus;
use std::sync::{Arc, mpsc};
use std::time::Duration;

pub(crate) const SHELL_OUTPUT: &str = "*Shell Command Output*";
/// How long a key waits for a shell command before it goes on in the background.
const WAIT: Duration = Duration::from_millis(500);

/// What a shell command printed, its output and errors together, and how it ended.
pub struct Ran {
    /// What it printed, up to the limit.
    pub text: String,
    /// Its status, or why it didn't run.
    pub status: io::Result<ExitStatus>,
    /// It printed more than the limit, and was stopped there.
    pub cut: bool,
    /// Some of what it printed wasn't UTF-8, and shows as replacements in `text`.
    pub lossy: bool,
}

impl Ctx {
    /// Run `line` with your shell in `dir`, with `input` on its stdin or nothing, and give what it
    /// printed to `done`: before this returns if it takes less than half a second, else later.
    pub fn shell_output(
        &mut self,
        dir: &Path,
        line: &str,
        input: Option<String>,
        done: impl FnOnce(&mut Ctx, Ran) -> Result + 'static,
    ) -> Result {
        let (tx, rx) = mpsc::channel();
        let (out, group) = (Arc::new(Output::default()), Group::default());
        let (cmd, o, g) = (shell(line, dir), out.clone(), group.clone());
        std::thread::spawn(move || {
            let input = input.map(String::into_bytes);
            let _ = tx.send(run(cmd, input, o, &g, None, true));
        });
        let ran = move |status| Ran {
            text: out.take(true),
            status,
            cut: out.cut(),
            lossy: out.lossy(),
        };
        let gone = || Err(io::Error::other("the command's thread stopped"));
        match rx.recv_timeout(WAIT) {
            Ok(status) => done(self, ran(status)),
            Err(mpsc::RecvTimeoutError::Disconnected) => done(self, ran(gone())),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.messages.push(format!(
                    "`{line}` is still running; what it prints comes when it's done"
                ));
                // So that it ends when DIE does.
                self.procs.background.retain(Group::running);
                self.procs.background.push(group);
                let job = self.spawn(
                    move || rx.recv().unwrap_or_else(|_| gone()),
                    move |ctx, s| done(ctx, ran(s)),
                );
                job.detach();
                Ok(())
            }
        }
    }

    /// Emacs's `shell-command`: run `line` in this buffer's directory, and show what it prints in
    /// `*Shell Command Output*`, or on the command row instead of a window if it's one line and no
    /// window has the buffer. With nothing printed, say how it ended.
    pub fn shell_command(&mut self, line: &str) -> Result {
        let dir = here_dir(self);
        let shown = dir.clone();
        self.shell_output(&dir, line, None, move |ctx, ran| {
            ctx.show_shell_output(&shown, ran)
        })
    }

    fn show_shell_output(&mut self, dir: &Path, ran: Ran) -> Result {
        let status = ran.status.map_err(|e| Error::Msg(e.to_string()))?;
        let mut text = ran.text;
        if ran.cut {
            text += &format!(
                "\n[DIE stopped the command at {} MiB of output.]\n",
                LIMIT >> 20
            );
        }
        let buf = self.output_buffer(SHELL_OUTPUT, &text, dir)?;
        if text.is_empty() {
            let said = match (status.code(), super::ended(&status)) {
                (Some(0), _) => "(Shell command succeeded with no output)".to_string(),
                (Some(n), _) => format!("(Shell command failed with code {n} and no output)"),
                (None, how) => format!("(Shell command killed by signal {how})"),
            };
            self.messages.push(said);
            return Ok(());
        }
        let one = text.strip_suffix('\n').unwrap_or(&text);
        if !one.contains('\n') && self.windows_showing(buf).is_empty() {
            self.messages.push(one.to_string());
            return Ok(());
        }
        for w in self.windows_showing(buf) {
            self.text_mut(buf)?.set_cursor(w, 0);
        }
        self.show_results(buf)
    }
}

pub(super) fn register(reg: &mut Registry) {
    reg.command(
        "shell-command",
        "Run a shell command in this buffer's directory, and show what it prints: one line on the \
         command row, more in *Shell Command Output*. With a command after the name, run that; \
         else ask for one",
        |ctx, a| {
            if let Some(line) = a.text.as_deref().filter(|t| !t.trim().is_empty()) {
                return ctx.shell_command(line);
            }
            let spec = PromptSpec::new("Shell command: ").history("shell-command");
            ctx.prompt(spec, move |ctx, answer| {
                match answer.filter(|a| !a.trim().is_empty()) {
                    Some(line) => ctx.shell_command(&line),
                    None => Ok(()),
                }
            });
            Ok(())
        },
    );
}
