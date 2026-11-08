//! `die --wait`: the clients waiting until you're done with their files, as Emacs's server keeps
//! `emacsclient`'s (server.el's `server-visit-files`, `server-buffer-done` and
//! `server-switch-buffer`). You're done with a buffer when [`Ctx::client_done`] says so, as `:wq`
//! and `:q` do there, or when it's killed. A client is told once it has none left.

use crate::files::normalize;
use crate::{BufferId, Ctx, FileTarget, Result, ShowIn};
use std::collections::HashSet;

/// The clients waiting, the first to come first, and the buffers made for them.
#[derive(Default)]
pub(crate) struct Clients {
    waiting: Vec<Client>,
    /// Buffers that weren't open until a client asked, which go when you're done with them, as
    /// `server-kill-new-buffers` has it. Only ones a client still waits on.
    made: HashSet<BufferId>,
}

struct Client {
    /// Its buffers, the last named first, as `server-visit-files` keeps them.
    buffers: Vec<BufferId>,
    done: Box<dyn FnOnce()>,
}

impl Ctx {
    /// Open `targets` as [`Ctx::open_target`] does, for a client that waits until you're done
    /// with them. `done` runs once each buffer that opened is done or killed, or at once if none
    /// did. One that won't open is logged. A buffer that was open with no unsaved changes is read
    /// again first, as `server-visit-files` reverts it, since git has just written the file.
    pub fn open_for_client(&mut self, targets: &[FileTarget], done: impl FnOnce() + 'static) {
        let mut buffers = vec![];
        for target in targets {
            let open = self.find_file_buffer(&normalize(&target.path));
            let saved = open.filter(|b| self.text(*b).is_ok_and(|t| !t.modified()));
            if let Some(Err(e)) = saved.map(|b| self.revert_file(b)) {
                self.messages.push(e.to_string());
            }
            match self.open_target(target) {
                Ok(buf) => {
                    if open.is_none() {
                        self.clients.made.insert(buf);
                    }
                    buffers.retain(|b| *b != buf);
                    buffers.insert(0, buf);
                }
                Err(e) => self.messages.push(e.to_string()),
            }
        }
        if buffers.is_empty() {
            return done();
        }
        let done = Box::new(done);
        self.clients.waiting.push(Client { buffers, done });
    }

    /// Whether a client waits on this buffer.
    pub fn is_client_buffer(&self, buf: BufferId) -> bool {
        self.clients
            .waiting
            .iter()
            .any(|c| c.buffers.contains(&buf))
    }

    /// You're done with `buf`, as Emacs's `server-edit` takes it, saved or not: no client waits on
    /// it now, and one left with none is told. One made for a client is killed, unsaved text and
    /// all, and its windows go back to what they showed. One open before goes last in the buffer
    /// list and stays on screen. Then the next buffer a client waits on comes up: the one after
    /// this in its client's list, or else the first client's latest, in a window showing it or
    /// this one.
    pub fn client_done(&mut self, buf: BufferId) -> Result {
        let made = self.clients.made.contains(&buf);
        let next = self.forget_client_buffer(buf);
        if made {
            self.kill(buf);
        } else if let Some(b) = self.buffers.get_mut(buf) {
            b.used = 0;
        }
        let first = || self.clients.waiting.iter().find_map(|c| c.buffers.first());
        match next.or_else(|| first().copied()) {
            Some(next) => match self.window_used_last(next) {
                Some(w) => self.focus(w),
                None => drop(self.show(next, ShowIn::Current)?),
            },
            None if !made => self
                .messages
                .push("No server buffers remain to edit".into()),
            None => {}
        }
        Ok(())
    }

    /// `buf` is done or dead, so no client waits on it, and each with nothing left is told. What's
    /// next: the buffer after it in the first client's list that had it.
    pub(crate) fn forget_client_buffer(&mut self, buf: BufferId) -> Option<BufferId> {
        self.clients.made.remove(&buf);
        let mut next = None;
        for client in &mut self.clients.waiting {
            if let Some(at) = client.buffers.iter().position(|b| *b == buf) {
                client.buffers.remove(at);
                next = next.or(client.buffers.get(at).copied());
            }
        }
        let all = std::mem::take(&mut self.clients.waiting).into_iter();
        let (finished, waiting): (Vec<_>, _) = all.partition(|c| c.buffers.is_empty());
        self.clients.waiting = waiting;
        for client in finished {
            (client.done)();
        }
        next
    }
}
