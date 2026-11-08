//! The workspace (K6, contract §5): buffers with their names and directories, what shows them,
//! and the registers with the system clipboard. Windows and the split tree are in `windows`.
#![warn(missing_docs)]
use crate::error::*;
use crate::ext::*;
use crate::ids::*;
use crate::keymap::*;
use crate::rt::*;
use crate::text::*;
use std::any::TypeId;
use std::path::{Path, PathBuf};

pub(crate) mod tree;
pub use tree::Side;

/// Where [`Ctx::show`] shows a buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowIn {
    /// The window a running command acts for, or else the focused one, and it takes the focus.
    /// With no window yet, a first one that fills the frame.
    Current,
    /// A new window at this side of the current one, and it takes the focus.
    Split(Side),
    /// A window already showing it, which takes the focus: the current one if it does, or else
    /// the one used last. With none, as `Current`.
    Existing,
    /// Nowhere. The buffer stays alive, not shown.
    Background,
}

/// A register's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RegKind {
    /// Characters.
    Char,
    /// Whole lines.
    Line,
    /// A rectangle.
    Block,
    /// Keys `q` recorded, in notation. Any other register holds plain text, which `@` types.
    Macro,
}

/// What a register holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Register {
    /// Its shape.
    pub kind: RegKind,
    /// Its text.
    pub text: String,
}

/// The system clipboard (contract §5). R5 puts macOS's pasteboard behind this, and the harness a
/// [`crate::FakeClipboard`]. Core calls it on the main thread, and it runs unguarded: it's the host's
/// code, not an extension's. Core puts an error, which says what failed, in `*messages*`.
pub trait Clipboard {
    /// What it holds: charwise for what another app copied, as written for what DIE did, and None
    /// if that isn't text. On macOS this is what can bring up the pasteboard privacy alert, so core
    /// calls it only for a paste, and only once [`Clipboard::change_count`] says there's cause.
    fn read(&mut self) -> Result<Option<Register>, String>;
    /// Replace what it holds.
    fn write(&mut self, r: &Register) -> Result<(), String>;
    /// A number that changes when what it holds changes, and so when another app copies:
    /// NSPasteboard's `changeCount` on macOS, a count kept by comparing on Linux.
    fn change_count(&mut self) -> Result<u64, String>;
}

/// The clipboard, and what DIE already knows of it.
pub(crate) struct Clip {
    pub(crate) board: Box<dyn Clipboard>,
    /// The change count when core last looked. None before the first look, so what another app
    /// left there before DIE started counts as new, as it does in Emacs.
    seen_count: Option<u64>,
    seen_text: Option<String>,
}

impl Clip {
    pub(crate) fn new(board: Box<dyn Clipboard>) -> Clip {
        Clip {
            board,
            seen_count: None,
            seen_text: None,
        }
    }
}

impl Ctx {
    // -------------------------------------------------------------- buffers

    pub(crate) fn unique_name(&self, name: &str) -> String {
        let taken = |n: &str| self.buffers.values().any(|b| b.name == n);
        if !taken(name) {
            return name.into();
        }
        (2..)
            .map(|i| format!("{name}<{i}>"))
            .find(|n| !taken(n))
            .unwrap()
    }
    /// A new buffer's directory: the focused buffer's, as in Emacs, or else where DIE started.
    fn new_directory(&self) -> PathBuf {
        let focused = self.focused_buffer().and_then(|b| self.directory(b));
        focused.map_or_else(
            || std::env::current_dir().unwrap_or_else(|_| "/".into()),
            Path::to_path_buf,
        )
    }
    fn add_buffer(&mut self, name: &str, content: Content, state: State) -> BufferId {
        let b = Buffer {
            name: self.unique_name(name),
            directory: self.new_directory(),
            content,
            modes: vec![],
            local: Keymap::with_leader(self.leader),
            state,
            used: self.next_tick(),
        };
        self.buffers.insert(b)
    }
    /// Create a text buffer. Its name is made unique, as in Emacs (`term<2>`).
    pub fn create_text(&mut self, name: &str, text: &str) -> BufferId {
        let mut t = Box::new(TextBuf::new(text));
        *t.settings_mut() = self.text_defaults.clone();
        self.add_buffer(name, Content::Text(t), State::Normal)
    }
    /// Create a buffer showing a custom surface. Its name is made unique.
    pub fn create_surface<S: Surface>(&mut self, name: &str, s: S) -> BufferId {
        let (kind, state) = (s.kind(), s.initial_state());
        let content = Content::Custom {
            type_id: TypeId::of::<S>(),
            kind,
            owner: self.cur_ext(),
            obj: Some(Box::new(s)),
        };
        self.add_buffer(name, content, state)
    }
    /// The buffer with this name.
    pub fn find_buffer(&self, name: &str) -> Option<BufferId> {
        self.buffers
            .iter()
            .find(|(_, b)| b.name == name)
            .map(|(id, _)| id)
    }
    /// Every live buffer, the one shown or made last first.
    pub fn buffer_list(&self) -> Vec<BufferId> {
        let mut all: Vec<_> = self.buffers.iter().map(|(id, b)| (b.used, id)).collect();
        all.sort_by_key(|(used, _)| std::cmp::Reverse(*used));
        all.into_iter().map(|(_, id)| id).collect()
    }
    /// A buffer's name. None if it's dead.
    pub fn buffer_name(&self, buf: BufferId) -> Option<&str> {
        self.buffers.get(buf).map(|b| b.name.as_str())
    }
    /// The directory a buffer's relative paths start from, Emacs's `default-directory`: the
    /// directory of the file it visits, or else the one the focused buffer had when it was made.
    pub fn directory(&self, buf: BufferId) -> Option<&Path> {
        self.buffers.get(buf).map(|b| b.directory.as_path())
    }
    /// Set a buffer's directory. A dead buffer is ignored.
    pub fn set_directory(&mut self, buf: BufferId, dir: impl Into<PathBuf>) {
        if let Some(b) = self.buffers.get_mut(buf) {
            b.directory = dir.into();
        }
    }
    /// Whether a buffer is still alive.
    pub fn is_live(&self, buf: BufferId) -> bool {
        self.buffers.contains_key(buf)
    }
    /// Kill a buffer. It is dead at once; a checked-out surface or mode drops at check-in.
    /// Dropping its modes drops the `Sub` handles they hold, which cancels them (C4). Every window
    /// showing it goes on showing another buffer: the one used last that no window shows, or
    /// else the one used last, or else a new `*scratch*`. A `die --wait` client waits on it no
    /// more, and is told if it was the last it waited on.
    pub fn kill(&mut self, buf: BufferId) -> bool {
        let Some(b) = self.buffers.remove(buf) else {
            return false;
        };
        self.cleanup(|_| drop(b));
        self.files.forget(buf);
        self.forget_client_buffer(buf);
        self.stop_ticking_if_done();
        let showing: Vec<WindowId> = self.windows_showing(buf);
        for w in showing {
            let next = self.other_buffer();
            self.set_window_buffer(w, next);
        }
        let hooks: Vec<_> = self
            .subs
            .iter()
            .filter_map(|e| match &e.kind {
                SubKind::Killed(f) if e.inner.alive.get() => Some((e.inner.clone(), f.clone())),
                _ => None,
            })
            .collect();
        for (inner, f) in hooks {
            let ext = inner.ext;
            self.queue.push_back(Queued {
                ext,
                alive: Some(inner),
                keyed: false,
                f: Box::new(move |ctx| (f.borrow_mut())(ctx, buf)),
            });
        }
        true
    }
    /// The buffer a window goes to when its own dies.
    fn other_buffer(&mut self) -> BufferId {
        let all = self.fallback_buffers();
        let unseen = all.iter().find(|b| self.windows_showing(**b).is_empty());
        match unseen.or(all.first()) {
            Some(b) => *b,
            None => self.create_text("*scratch*", ""),
        }
    }

    /// A buffer's text, for reading (C2).
    pub fn text(&self, buf: BufferId) -> Result<&TextBuf> {
        match &self.buffers.get(buf).ok_or(Checkout::Gone)?.content {
            Content::Text(t) => Ok(t),
            _ => Err(Error::NotText),
        }
    }
    /// Core-owned text is borrowed by id, never checked out (C2).
    pub fn text_mut(&mut self, buf: BufferId) -> Result<&mut TextBuf> {
        let actor = self
            .cur_window
            .or(self.focused)
            .filter(|w| self.windows.get(*w).is_some_and(|w| w.buffer == buf));
        let cmd = self.cmd_seq;
        match &mut self.buffers.get_mut(buf).ok_or(Checkout::Gone)?.content {
            Content::Text(t) => {
                t.cmd = cmd;
                t.actor = actor;
                Ok(t)
            }
            _ => Err(Error::NotText),
        }
    }

    /// Show a buffer, and return the window that does, or None for `Background`.
    pub fn show(&mut self, buf: BufferId, how: ShowIn) -> Result<Option<WindowId>> {
        self.buffers.get(buf).ok_or(Checkout::Gone)?;
        let current = self.current_window();
        let w = match how {
            ShowIn::Background => return Ok(None),
            ShowIn::Current => current,
            ShowIn::Split(side) => {
                let w = current.ok_or_else(|| Error::Msg("no window to split".into()))?;
                Some(self.split_window(w, side)?)
            }
            ShowIn::Existing => {
                let here = current.filter(|w| self.windows[*w].buffer == buf);
                here.or(self.window_used_last(buf)).or(current)
            }
        };
        let Some(w) = w else {
            let (cols, rows) = self.tree.frame;
            let opened = self.open_window(buf, cols, rows.saturating_sub(2));
            return opened.map(Some);
        };
        self.set_window_buffer(w, buf);
        self.focus(w);
        Ok(Some(w))
    }

    // -------------------------------------------------------------- registers

    /// What a register holds.
    pub fn register(&self, c: char) -> Option<&Register> {
        self.registers.get(&c)
    }
    /// Set a register.
    pub fn set_register(&mut self, c: char, r: Register) {
        self.registers.insert(c, r);
        self.registers_saved.take();
    }

    /// Use this clipboard from now on, in place of the one in memory every workspace starts with.
    pub fn set_clipboard(&mut self, board: impl Clipboard + 'static) {
        self.clip = Clip::new(Box::new(board));
    }
    /// What the system clipboard holds, if it's text. A failure goes to `*messages*`.
    pub fn clipboard(&mut self) -> Option<Register> {
        self.clip.board.read().unwrap_or_else(|e| {
            self.messages.push(e);
            None
        })
    }
    /// Put this on the system clipboard. It isn't news to [`Ctx::clipboard_news`]. A failure goes to
    /// `*messages*`, and the caller's copy is the one that counts, so what's there now isn't news
    /// either: a later paste doesn't put an older copy above this.
    pub fn write_clipboard(&mut self, r: &Register) {
        let written = self.clip.board.write(r);
        let count = self.clip.board.change_count();
        match (written, &count) {
            (Err(e), _) => self.messages.push(e),
            (Ok(()), Err(e)) => self.messages.push(e.clone()),
            (Ok(()), Ok(_)) => {}
        }
        if let Ok(count) = count {
            self.clip.seen_count = Some(count);
        }
        self.clip.seen_text = Some(r.text.clone());
    }
    /// Registers came back from a session, and the newest kill is what DIE last put on the
    /// clipboard or took from it, so that text still there isn't news.
    pub(crate) fn clipboard_known_from_registers(&mut self) {
        self.clip.seen_text = self.registers.get(&'"').map(|r| r.text.clone());
    }
    /// What another app copied since core last wrote to the clipboard or asked this, once, as
    /// Emacs's `interprogram-paste-function` answers. Copying the text DIE last knew of isn't news.
    pub fn clipboard_news(&mut self) -> Option<Register> {
        let count = match self.clip.board.change_count() {
            Ok(count) => count,
            Err(e) => {
                self.messages.push(e);
                return None;
            }
        };
        if self.clip.seen_count == Some(count) {
            return None;
        }
        // A read that fails leaves the count, so the next paste looks again and the copy isn't lost.
        let read = self.clip.board.read().map_err(|e| self.messages.push(e));
        let r = read.ok()?;
        self.clip.seen_count = Some(count);
        let r = r?;
        if self.clip.seen_text.as_ref() == Some(&r.text) {
            return None;
        }
        self.clip.seen_text = Some(r.text.clone());
        Some(r)
    }
}
