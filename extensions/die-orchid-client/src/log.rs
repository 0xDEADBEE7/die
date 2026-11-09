//! `*Orchid Log*`: the client's own log, `[HH:MM:SS] what happened`, as the Emacs client's
//! `orchid-log` wrote it, but kept to its last [`crate::Options::log_lines`] lines (2,000 by
//! default), where Emacs's grew without end. It's an output buffer: read-only, unmodified, and
//! not kept over a restart, as Emacs's isn't. `orchid-log-show` shows it as `display-buffer`
//! does: in another window, or a new one below, with the focus staying.

use crate::Orchid;
use die_core::{Ctx, Origin, Result};
use jiff::Timestamp;

/// Its name.
pub const BUFFER: &str = "*Orchid Log*";

impl Orchid {
    /// Add a line to `*Orchid Log*`, making the buffer the first time, and drop its oldest lines
    /// past the limit.
    pub fn log(&self, ctx: &mut Ctx, what: &str) -> Result {
        let buf = match ctx.find_buffer(BUFFER) {
            Some(buf) => buf,
            None => ctx.output_buffer(BUFFER, "", self.root())?,
        };
        let time = crate::parse::local_time(Timestamp::now(), self.zone());
        ctx.append_output(buf, &format!("[{time}] {what}\n"));
        let t = ctx.text_mut(buf)?;
        // The line after the last newline is empty, and doesn't count.
        let lines = t.line_count().saturating_sub(1);
        if lines > self.options.log_lines {
            let cut = t.line_start(lines - self.options.log_lines);
            t.tx(Origin::Extension).silent().delete(0..cut).commit()?;
            t.set_modified(false);
        }
        Ok(())
    }
}

/// [`Orchid::log`], from outside the extension.
pub fn log(ctx: &mut Ctx, what: &str) -> Result {
    ctx.with_ext(|o: &mut Orchid, ctx| o.log(ctx, what))?
}

/// `orchid-log-show`: `*Orchid Log*` where `display-buffer` would put it ([`crate::display_buffer`]).
pub fn show(ctx: &mut Ctx) -> Result {
    let buf = match ctx.find_buffer(BUFFER) {
        Some(buf) => buf,
        None => {
            let root = ctx.with_ext(|o: &mut Orchid, _| o.root().to_path_buf())?;
            ctx.output_buffer(BUFFER, "", &root)?
        }
    };
    crate::display_buffer(ctx, buf)
}
