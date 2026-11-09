//! The session browser, `*orchid-sessions*` (the Emacs client's `orchid-session-browser-mode`, drawn
//! by seek): a read-only buffer of sessions, newest first, in the selected window, killed on
//! select and on quit. The browser's builder fills this in, with `seek.rs`. What it has to work
//! with:
//!
//! - rows: [`crate::refresh_sessions`] reads every `metadata.json` into the registry
//!   ([`crate::Orchid::sessions`]); [`crate::parse::relative_time`],
//!   [`crate::parse::workspace_name`], [`crate::parse::agent_name`] and
//!   [`crate::parse::truncate_width`] format them, and [`crate::Orchid::chat_buffer`] says ACTIVE;
//! - live redraws: [`crate::Orchid::on_status_change`], and [`crate::poll`] for the 2 s look at
//!   what changed elsewhere, both held in the buffer's mode so they go with it;
//! - `n`: [`crate::read_agent`] then [`crate::chat::new_session`]; `RET`: [`crate::chat::open`];
//!   marks: [`crate::yes_or_no`] and [`crate::cli::Cli::run_all`];
//! - a restart: [`crate::Orchid::track`] with [`crate::Kind::Browser`], then `restore`.

use crate::Orchid;
use die_core::session::Value;
use die_core::{BufferId, Ctx, Registry, Result};

/// The buffer's name.
pub const BUFFER: &str = "*orchid-sessions*";

/// Its modes, keys and commands.
pub(crate) fn register(_reg: &mut Registry) {}

/// `orchid`: show the browser in the selected window, with a fresh list of sessions.
pub fn show(ctx: &mut Ctx) -> Result {
    ctx.messages
        .push("orchid: the session browser isn't built yet".into());
    Ok(())
}

/// Bring the browser back in `buf`, which the session restored as plain text, from the state it
/// was tracked with. It runs with the extension checked out (`orchid`), so it can track `buf` again and
/// subscribe; what needs `Ctx::with_ext::<Orchid>` waits for a `defer`.
pub(crate) fn restore(
    _orchid: &mut Orchid,
    _ctx: &mut Ctx,
    _buf: BufferId,
    _state: Value,
) -> Result {
    Ok(())
}
