//! The chat buffer, `*orchid-chat-<agent>-<wsdir>-<id5>*` (the Emacs client's `orchid-chat-mode`):
//! a header, the session's restored events, then each new one as it's appended, each a collapsed
//! 80-column stub, and your input at the end, sent with `RET` in insert state. The chat's builder
//! fills this in, with `sections.rs` (collapsibles) and `indicator.rs` (the processing line).
//! What it has to work with:
//!
//! - history: [`crate::store::restore`] with [`crate::parse::shows`], then [`crate::store::Tail`]
//!   from where it ended; each event laid out by [`crate::parse::shown`] and
//!   [`crate::parse::detail`], in the zone [`crate::Orchid::zone`] gives;
//! - following: a [`crate::poll::Polling`] in the mode, fast while the session runs or a send is
//!   out ([`crate::poll::Poller::set_fast`]), and [`crate::Orchid::update_session`] with each
//!   `metadata.json` it reads, so the browser hears;
//! - sending: [`crate::cli::Cli::run`] with [`crate::cli::Cmd::Send`], and
//!   [`crate::parse::user_stub`];
//! - a restart: [`crate::Orchid::track_with`] with [`crate::Kind::Chat`], the session's id under
//!   `id`, and a closure that adds the unsent input from the input marker; then `restore`.

use crate::Orchid;
use die_core::session::Value;
use die_core::{BufferId, Ctx, Registry, Result};
use std::path::Path;

/// Its modes, keys and commands.
pub(crate) fn register(_reg: &mut Registry) {}

/// Open a chat on `session_id` in the selected window, or go to the one open on it.
pub fn open(ctx: &mut Ctx, session_id: &str) -> Result {
    ctx.messages.push(format!(
        "orchid: chats aren't built yet, so {session_id} can't open"
    ));
    Ok(())
}

/// Create a session with `agent` working in `working_dir`, and open a chat on it in the selected
/// window.
pub fn new_session(ctx: &mut Ctx, agent: &str, working_dir: &Path) -> Result {
    let dir = working_dir.display();
    ctx.messages.push(format!(
        "orchid: chats aren't built yet, so no {agent} session in {dir}"
    ));
    Ok(())
}

/// Bring the chat back in `buf`, which the session restored as plain text, from what its tracking
/// kept: rebuilt from `events.jsonl`, its input kept, following again. It runs with the
/// extension checked out (`orchid`); what needs `Ctx::with_ext::<Orchid>` waits for a `defer`.
pub(crate) fn restore(
    _orchid: &mut Orchid,
    _ctx: &mut Ctx,
    _buf: BufferId,
    _state: Value,
) -> Result {
    Ok(())
}
