//! `*messages*` (contract §15): the log as a buffer, and its newest line on the command row.
//!
//! Everything that logs pushes to [`Ctx::messages`], which doesn't reach for a buffer, so each
//! frame and each key bring the buffer up to date. The buffer is made with the first message and
//! made again if it's killed. It's read-only, has no undo and never counts as modified. It's
//! last in the buffer list until it's shown, and a window never falls back on it.
use crate::ext::*;
use crate::ids::*;
use crate::text::*;

const MESSAGES: &str = "*messages*";
/// How many lines the log and the buffer keep, as Emacs's `message-log-max`.
const KEEP: usize = 1000;

#[derive(Default)]
pub(crate) struct Log {
    /// How much of `messages` the buffer has.
    logged: usize,
    /// The command row's line, until the next key.
    pub(crate) echo: Option<String>,
}

impl Ctx {
    /// Bring `*messages*` and the command row up to date with the log.
    pub(crate) fn sync_messages(&mut self) {
        let logged = self.log.logged;
        if self.messages.len() <= logged {
            self.log.logged = self.messages.len(); // a test may have emptied it
            return;
        }
        let new: String = self.messages[logged..]
            .iter()
            .flat_map(|m| [m.as_str(), "\n"])
            .collect();
        let newest = self.messages.last().and_then(|m| m.lines().next());
        self.log.echo = newest.map(str::to_owned);
        let buf = match self.find_buffer(MESSAGES) {
            Some(b) => b,
            None => self.create_messages(),
        };
        if let Ok(t) = self.text_mut(buf) {
            let end = t.len();
            let _ = t.tx(Origin::Extension).silent().insert(end, new).commit();
            let extra = t.line_count().saturating_sub(KEEP + 1);
            if extra > 0 {
                let to = t.line_start(extra);
                let _ = t.tx(Origin::Extension).silent().delete(0..to).commit();
            }
            t.set_modified(false);
        }
        let over = self.messages.len().saturating_sub(KEEP);
        self.messages.drain(..over);
        self.log.logged = self.messages.len();
    }

    fn create_messages(&mut self) -> BufferId {
        let buf = self.create_text(MESSAGES, "");
        self.buffers[buf].used = 0;
        if let Ok(t) = self.text_mut(buf) {
            t.settings_mut().read_only = true;
        }
        buf
    }

    /// The buffers a window can fall back on, the one shown or made last first.
    pub(crate) fn fallback_buffers(&self) -> Vec<BufferId> {
        let log = self.find_buffer(MESSAGES);
        let mut all = self.buffer_list();
        all.retain(|b| Some(*b) != log);
        all
    }
}
