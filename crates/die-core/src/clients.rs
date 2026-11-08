//! `die --wait`: the clients waiting until you're done with their files, as Emacs's server keeps
//! `emacsclient`'s (server.el's `server-visit-files`, `server-buffer-done` and
//! `server-switch-buffer`). You're done with a buffer when [`Ctx::client_done`] says so, as `:wq`
//! and `:q` do there, or when it's killed. A client is told once it has none left. `:cq` gives up
//! on one ([`Ctx::client_failed`]), as `server-edit-abort` does, and a restart hands them all to
//! the next instance ([`Ctx::hand_over_clients`]), which waits on them again.

use crate::files::normalize;
use crate::{BufferId, Ctx, FileTarget, Result, ShowIn};
use std::collections::HashSet;
use std::path::PathBuf;

/// How a client's wait ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// You're done with its files.
    Done,
    /// `:cq` gave up on its files, as `server-edit-abort` has it, or none would open.
    Failed,
    /// A restart hands it to the next instance, which it asks to wait on these again.
    Moved(Vec<Waited>),
}

/// A file a client still waits on, as a restart hands it over.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Waited {
    /// Its path, which is absolute.
    pub path: PathBuf,
    /// Its buffer was made for the client, so it goes once you're done with it.
    pub made: bool,
}

/// What tells a client how its wait ended.
type Tell = Box<dyn FnOnce(Outcome)>;

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
    tell: Tell,
}

impl Ctx {
    /// Open `files` as [`Ctx::open_target`] does, for a client that waits until you're done with
    /// them. `tell` is told once each buffer that opened is done or killed, or at once, as failed,
    /// if none did. One that won't open is logged. A buffer that was open with no unsaved changes
    /// is read again first, as `server-visit-files` reverts it, since git has just written the
    /// file.
    pub fn open_for_client(&mut self, files: &[FileTarget], tell: impl FnOnce(Outcome) + 'static) {
        let mut buffers = vec![];
        for target in files {
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
        self.wait_on(buffers, Box::new(tell));
    }

    /// Wait on these again for a client that a restart moved here. They're open already, from the
    /// session, so nothing is read again and nothing moves on screen. One that isn't is opened,
    /// as made for the client.
    pub fn wait_again(&mut self, waited: Vec<Waited>, tell: impl FnOnce(Outcome) + 'static) {
        let mut buffers = vec![];
        for w in waited {
            let open = self.find_file_buffer(&normalize(&w.path)).is_some();
            match self.open_file(&w.path) {
                Ok(buf) => {
                    if w.made || !open {
                        self.clients.made.insert(buf);
                    }
                    buffers.push(buf);
                }
                Err(e) => self.messages.push(e.to_string()),
            }
        }
        self.wait_on(buffers, Box::new(tell));
    }

    fn wait_on(&mut self, buffers: Vec<BufferId>, tell: Tell) {
        if buffers.is_empty() {
            return tell(Outcome::Failed);
        }
        self.clients.waiting.push(Client { buffers, tell });
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
    /// all, and its windows show what [`Ctx::kill`] gives them. One open before goes last in the
    /// buffer list and stays on screen. Then the next buffer a client waits on comes up: the one
    /// after this in its client's list, or else the first client's latest, in a window showing it
    /// or this one. With `force`, as your Evil's `:q!` runs only `server-buffer-done`, nothing
    /// comes up and nothing is said.
    pub fn client_done(&mut self, buf: BufferId, force: bool) -> Result {
        let made = self.clients.made.contains(&buf);
        let next = self.forget_client_buffer(buf);
        if made {
            self.kill(buf);
        } else if let Some(b) = self.buffers.get_mut(buf) {
            b.used = 0;
        }
        if force {
            return Ok(());
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

    /// `:cq` on `buf`: each client waiting on it fails, as `server-edit-abort` has `emacsclient`
    /// fail, and waits on nothing more. Its other buffers made for it go if they have no unsaved
    /// changes, as `server-delete-client` has it, and `buf` is done as `:q!` has it.
    pub fn client_failed(&mut self, buf: BufferId) -> Result {
        let all = std::mem::take(&mut self.clients.waiting).into_iter();
        let (failed, waiting): (Vec<_>, _) = all.partition(|c| c.buffers.contains(&buf));
        self.clients.waiting = waiting;
        for client in failed {
            for b in client.buffers.into_iter().filter(|b| *b != buf) {
                let unsaved = self.text(b).is_ok_and(|t| t.modified());
                if !self.is_client_buffer(b) && self.clients.made.remove(&b) && !unsaved {
                    self.kill(b);
                }
            }
            (client.tell)(Outcome::Failed);
        }
        self.client_done(buf, true)
    }

    /// A restart is handing over to the next instance: tell each client what it waits on, so
    /// that it asks that one to wait on them again, and forget them all.
    pub fn hand_over_clients(&mut self) {
        for client in std::mem::take(&mut self.clients.waiting) {
            let waited = client.buffers.iter().filter_map(|b| {
                let made = self.clients.made.contains(b);
                let path = self.file(*b)?.path.clone();
                Some(Waited { path, made })
            });
            (client.tell)(Outcome::Moved(waited.collect()));
        }
        self.clients.made.clear();
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
            (client.tell)(Outcome::Done);
        }
        next
    }
}
