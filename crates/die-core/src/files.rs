//! Files: opening one into a text buffer (K10a), then saving and reloading it (K10b).
//!
//! Only UTF-8 text is editable. A file that isn't UTF-8, holds NUL bytes or has
//! CRLF line ends opens read-only with a message, so a save can't change its
//! bytes behind your back. A UTF-8 byte-order mark is kept out of the text and
//! remembered for the save.
//!
//! **Saving.** A save writes the text and nothing else: no final newline is added
//! and no line end changes. It goes through symlinks, a dangling one included, to
//! the file they name. That file is replaced by a new one renamed over it, with
//! its mode and owner, so a crash leaves the old bytes or the new ones. It's
//! written in place when replacing it would change what it is: when it has other
//! hard links, when its owner can't be kept, or when its directory can't be
//! written. In place, a crash can leave part of each. The new file starts as a copy
//! of the old, so on macOS extended attributes and ACLs survive. `fs::copy` doesn't
//! carry them on Linux, where they're lost.
//!
//! **Refusing.** [`Ctx::save`], `:w`, refuses when the file changed since DIE last
//! read or wrote it, and when you can't write to it. [`Ctx::save_force`], `:w!`,
//! does neither. Both refuse a file that opened read-only and anything that isn't
//! a regular file. With no unsaved changes `:w` writes nothing and refuses nothing,
//! and a file that's gone is simply written again, as in Emacs.
//!
//! **Reloading.** Once a second [`Ctx::check_files`] compares each file with its
//! fingerprint, and reloads a buffer with no unsaved changes. A reload is one undo
//! step, and text it didn't change keeps its cursors and markers. With the same
//! bytes it changes nothing. It waits while an undo group is open, as during an
//! insert, and while the file's time is within 200 ms of now, because `rustfmt` and
//! others write in place and an empty file would lose the cursor. Unsaved changes
//! and files over 16 MiB are never reloaded by the poll: a message says the file
//! changed, once, and [`Ctx::revert_file`], `:e!`, reloads on request. A deleted file
//! leaves the buffer as it is, with a message.
//!
//! A reload into or out of a read-only state is no undo step, so `u` can't bring back
//! text that doesn't match the file, which `:w` would then store. `:e!` refuses to
//! replace unsaved changes with a file that can't be edited. The byte-order mark is
//! the file's as of the last read: undoing past a reload doesn't bring one back.
//!
//! Settings by path are a stopgap until file-type modes (M1), which extensions
//! will declare at Register.

use crate::ext::catch;
use crate::{BufferId, Ctx, Error, Origin, Result, Settings, Sub};
use ropey::Rope;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Seek, Write};
use std::ops::Range;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, fchown};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How often [`Ctx::check_files`] runs while a file is open. A `stat` took about 80 µs at
/// background priority on a loaded machine. Emacs's own 5 s interval is mostly hidden by its
/// file notifications, so 5 s here would feel slower than your Emacs.
const POLL_EVERY: Duration = Duration::from_secs(1);

/// How long after a file's modification time the poll leaves it alone: the tool may be between
/// truncating and writing. A time in the future isn't settling, or a skewed clock would stop
/// reloads.
const SETTLING: Duration = Duration::from_millis(200);

/// The largest file the poll reads by itself. A change means reading all of it on the main
/// thread, so this bounds what a growing log costs each second.
const POLL_READS_UP_TO: u64 = 16 << 20;

/// Past this much differing text, a reload is one change and doesn't look for lines to anchor.
/// Hashing grows with it, and so do the changes to apply and undo: 2 MB with 50,000 changed lines
/// took 1.9 s.
const ANCHORS_UP_TO: usize = 1 << 20;

/// How many symlinks a save follows before giving up, as the kernel's own limit does.
const MAX_LINKS: usize = 40;

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// What `:w` compares to tell whether a file changed on disk since DIE last read
/// or wrote it: its modification time, size and inode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    modified: SystemTime,
    len: u64,
    inode: u64,
}

impl Fingerprint {
    /// Modification time, size and inode, for the session.
    pub(crate) fn parts(&self) -> (SystemTime, u64, u64) {
        (self.modified, self.len, self.inode)
    }

    pub(crate) fn from_parts(modified: SystemTime, len: u64, inode: u64) -> Fingerprint {
        Fingerprint {
            modified,
            len,
            inode,
        }
    }

    fn from_metadata(m: &fs::Metadata) -> io::Result<Fingerprint> {
        Ok(Fingerprint {
            modified: m.modified()?,
            len: m.len(),
            inode: m.ino(),
        })
    }
}

/// Why a file can't be edited safely, so it opened read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Uneditable {
    /// Not valid UTF-8. The text shows U+FFFD where bytes are invalid.
    NotUtf8,
    /// NUL bytes, as in a binary file.
    NulBytes,
    /// CRLF line ends, which DIE doesn't edit yet.
    Crlf,
}

impl Uneditable {
    fn describe(self) -> &'static str {
        match self {
            Uneditable::NotUtf8 => "not UTF-8",
            Uneditable::NulBytes => "holds NUL bytes",
            Uneditable::Crlf => "has CRLF line ends",
        }
    }
}

/// The file a buffer visits.
#[derive(Clone, Debug, PartialEq)]
pub struct FileInfo {
    /// Absolute, with `.` and `..` resolved by name, as Emacs's `expand-file-name`
    /// does: `link/..` is the directory holding `link`, even when `link` is a
    /// symlink to a directory elsewhere. Symlinks are kept as given.
    pub path: PathBuf,
    /// As of the last read or write, or the last check that found the file gone. None while
    /// the file doesn't exist.
    pub fingerprint: Option<Fingerprint>,
    /// The file starts with a UTF-8 byte-order mark, which isn't in the text.
    pub bom: bool,
    /// Why the file can't be edited safely, if it can't. A save must refuse while
    /// this is set: the buffer's [`Settings::read_only`] can be turned off, and
    /// blocks only `User` edits.
    pub read_only: Option<Uneditable>,
    /// [`Settings::read_only`] before a file reason turned it on, to put back when that ends.
    read_only_before: bool,
    /// A message has said the file changed under unsaved text.
    told: bool,
}

impl FileInfo {
    pub(crate) fn read_only_before(&self) -> bool {
        self.read_only_before
    }
}

/// What the session remembers of a file, beyond its path and text.
pub(crate) struct FileState {
    pub(crate) fingerprint: Option<Fingerprint>,
    pub(crate) bom: bool,
    pub(crate) read_only: Option<Uneditable>,
    pub(crate) read_only_before: bool,
}

/// Settings by path, as [`crate::Config`] and [`Ctx::set_file_settings`] take them.
pub(crate) type SettingsFn = Box<dyn Fn(&Path, &mut Settings)>;
/// What a status line says, from the buffer's name and the file it visits.
pub type StatusTextFn = Box<dyn Fn(&str, Option<&FileInfo>) -> String>;

/// Which buffers visit files, and the config's settings by path.
#[derive(Default)]
pub(crate) struct Files {
    by_buffer: HashMap<BufferId, FileInfo>,
    pub(crate) settings: Option<SettingsFn>,
    /// The repeating [`Ctx::check_files`], while any buffer visits a file.
    poll: Option<Sub>,
    status_text: Option<StatusTextFn>,
}

impl Files {
    pub(crate) fn forget(&mut self, buf: BufferId) {
        self.by_buffer.remove(&buf);
        if self.by_buffer.is_empty() {
            self.poll = None;
        }
    }
}

impl Ctx {
    /// Open the file at `path`, which must be absolute, into a text buffer named
    /// after it, or return the buffer already visiting it (through a symlink too).
    /// `..` is resolved as [`FileInfo::path`] says. A missing file opens as an
    /// empty buffer that a save will create, with its directory, which a message
    /// says if it's missing. Anything else that isn't a readable regular file is
    /// an error. The settings from
    /// [`Ctx::set_file_settings`] apply, then read-only for a file that can't be
    /// edited safely, with a message.
    pub fn open_file(&mut self, path: &Path) -> Result<BufferId> {
        if !path.is_absolute() {
            return Err(Error::Msg(format!(
                "{}: not an absolute path",
                path.display()
            )));
        }
        let path = normalize(path);
        if let Some(b) = self.find_file_buffer(&path) {
            return Ok(b);
        }
        let (text, fingerprint, bom, read_only) = match read(&path) {
            Ok((bytes, fp)) => {
                let (text, bom, why) = decode(bytes);
                (text, Some(fp), bom, why)
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                if let Some(dir) = path.parent().filter(|d| !d.exists()) {
                    let line =
                        format!("Directory {} doesn't exist; a save makes it", dir.display());
                    self.messages.push(line);
                }
                (String::new(), None, false, None)
            }
            Err(e) => return Err(Error::Msg(format!("{}: {e}", path.display()))),
        };
        let state = FileState {
            fingerprint,
            bom,
            read_only,
            read_only_before: false,
        };
        self.attach_file(path, &text, state)
    }

    /// A new buffer with this text, visiting `path`: a file just read, or one from the session,
    /// whose text may differ from the file's. It's for the session to say it's unsaved. The
    /// settings by path apply, then read-only for a file that can't be edited safely.
    pub(crate) fn attach_file(
        &mut self,
        path: PathBuf,
        text: &str,
        state: FileState,
    ) -> Result<BufferId> {
        if !path.is_absolute() {
            let line = format!("{}: not an absolute path", path.display());
            return Err(Error::Msg(line));
        }
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let mut settings = self.text_defaults.clone();
        if let Some(f) = &self.files.settings
            && let Err(c) = catch(|| f(&path, &mut settings))
        {
            settings = self.text_defaults.clone();
            self.log_error(format!(
                "{name}: file settings panicked ({}); opened with the defaults",
                c.msg
            ));
        }
        let read_only_before = settings.read_only || state.read_only_before;
        if let Some(why) = state.read_only {
            settings.read_only = true;
            self.messages
                .push(format!("{name} {}; opened read-only", why.describe()));
        }
        let buf = self.create_text(&name, text);
        *self.text_mut(buf)?.settings_mut() = settings;
        if let Some(dir) = path.parent() {
            self.set_directory(buf, dir);
        }
        self.files.by_buffer.insert(
            buf,
            FileInfo {
                path,
                fingerprint: state.fingerprint,
                bom: state.bom,
                read_only: state.read_only,
                read_only_before,
                told: false,
            },
        );
        if self.files.poll.is_none() {
            let poll = self.every(POLL_EVERY, |ctx| {
                ctx.check_files();
                Ok(())
            });
            self.files.poll = Some(poll);
        }
        Ok(buf)
    }

    /// The file `buf` visits, if any.
    pub fn file(&self, buf: BufferId) -> Option<&FileInfo> {
        self.files.by_buffer.get(&buf)
    }

    /// Write `buf` to its file, as `:w`. The module's docs have the rules. With no unsaved
    /// changes and the file there, this writes nothing. Errors, with the file as it was: the
    /// file changed on disk, you can't write to it, it opened read-only ([`FileInfo::read_only`])
    /// or isn't a regular file, the buffer visits no file, or the write failed.
    pub fn save(&mut self, buf: BufferId) -> Result {
        self.write_file(buf, false)
    }

    /// Write `buf` to its file whatever is there, as `:w!`. It still refuses a file that opened
    /// read-only, whose bytes the text doesn't hold, and anything that isn't a regular file.
    pub fn save_force(&mut self, buf: BufferId) -> Result {
        self.write_file(buf, true)
    }

    fn write_file(&mut self, buf: BufferId, force: bool) -> Result {
        let f = self.visit(buf)?;
        let (path, bom) = (f.path.clone(), f.bom);
        let (known, why) = (f.fingerprint, f.read_only);
        let name = self.buffer_name(buf).unwrap_or_default().to_string();
        if let Some(why) = why {
            return Err(Error::Msg(format!("{name} {}; not saved", why.describe())));
        }
        let failed = |e: io::Error| Error::Msg(format!("{name}: {e}; not saved"));
        let disk = on_disk(&path).map_err(failed)?;
        if !force && disk.is_some() {
            if !self.unsaved(buf)? {
                self.messages.push("(No changes need to be saved)".into());
                return Ok(());
            }
            if disk != known {
                return Err(Error::Msg(format!(
                    "{name} changed on disk; :w! overwrites it, :e! reloads it"
                )));
            }
            if let Err(e) = File::options().write(true).open(&path)
                && e.kind() == ErrorKind::PermissionDenied
            {
                return Err(Error::Msg(format!("{name}: {e}; :w! tries anyway")));
            }
        }
        let t = self.text(buf)?;
        let fingerprint = store(&path, t.rope(), bom).map_err(failed)?;
        self.settle(buf, fingerprint)?;
        self.messages.push(format!("Wrote {}", path.display()));
        Ok(())
    }

    /// Read `buf`'s file again and replace the text, as `:e!`, unsaved changes included. It's
    /// one undo step, so `u` brings them back. There's no step when the buffer was read-only for
    /// the file's sake, whose text a save refused anyway. It's an error, and the text stays, with
    /// the file gone or unreadable, or with unsaved changes and a file that can't be edited, where
    /// `u` and `:w` couldn't get them back. Text the file didn't change keeps its cursors and
    /// markers.
    pub fn revert_file(&mut self, buf: BufferId) -> Result {
        self.visit(buf)?;
        self.reload(buf)
    }

    /// Compare every visited file with its fingerprint now, and reload the buffers with no
    /// unsaved changes. It runs by itself every second while a file is open. A host may also
    /// call it when its window gets the focus back.
    pub fn check_files(&mut self) {
        let bufs: Vec<BufferId> = self.files.by_buffer.keys().copied().collect();
        for buf in bufs {
            self.check_file(buf);
        }
    }

    /// One file of [`Ctx::check_files`]. What can't be read now, or may be mid-write, is looked
    /// at again next time, and what's only half written after that shows as a change again.
    fn check_file(&mut self, buf: BufferId) -> Option<()> {
        let f = self.files.by_buffer.get(&buf)?;
        let known = f.fingerprint;
        let disk = on_disk(&f.path).ok()?;
        if disk == known {
            return Some(());
        }
        let name = self.buffer_name(buf)?.to_string();
        if disk.is_none() {
            let f = self.files.by_buffer.get_mut(&buf)?;
            f.fingerprint = None;
            f.told = false;
            let line = format!("{name} was deleted on disk; the text stays, and :w writes it");
            self.messages.push(line);
        } else if self.unsaved(buf).ok()? {
            let f = self.files.by_buffer.get_mut(&buf)?;
            if !std::mem::replace(&mut f.told, true) {
                self.messages.push(format!(
                    "{name} changed on disk; :w refuses, :w! overwrites it, :e! reloads it"
                ));
            }
        } else if disk.is_some_and(|d| d.len > POLL_READS_UP_TO) {
            let f = self.files.by_buffer.get_mut(&buf)?;
            if !std::mem::replace(&mut f.told, true) {
                let line = format!("{name} changed on disk and is large; :e! reloads it");
                self.messages.push(line);
            }
        } else if !self.text(buf).ok()?.undo_group_open() && !disk.is_some_and(settling) {
            self.reload(buf).ok()?;
        }
        Some(())
    }

    /// The file `buf` visits, or the error for a buffer with none.
    fn visit(&self, buf: BufferId) -> Result<&FileInfo> {
        self.files.by_buffer.get(&buf).ok_or_else(|| {
            let name = self.buffer_name(buf).unwrap_or("this buffer");
            Error::Msg(format!("{name} visits no file"))
        })
    }

    /// Whether the text differs from what the file last held, as far as this buffer knows.
    fn unsaved(&self, buf: BufferId) -> Result<bool> {
        Ok(self.text(buf)?.modified())
    }

    /// The text and the file now agree, and `fingerprint` is what the file looks like.
    fn settle(&mut self, buf: BufferId, fingerprint: Fingerprint) -> Result {
        self.text_mut(buf)?.set_modified(false);
        if let Some(f) = self.files.by_buffer.get_mut(&buf) {
            f.fingerprint = Some(fingerprint);
            f.told = false;
        }
        Ok(())
    }

    /// Make the text what the file holds, in as few changes as it can.
    fn reload(&mut self, buf: BufferId) -> Result {
        let f = self.visit(buf)?;
        let was = f.read_only;
        let path = f.path.clone();
        let name = self.buffer_name(buf).unwrap_or_default().to_string();
        let (bytes, fingerprint) =
            read(&path).map_err(|e| Error::Msg(format!("{name}: {e}; not reloaded")))?;
        let (text, bom, why) = decode(bytes);
        if let Some(why) = why
            && self.unsaved(buf)?
        {
            return Err(Error::Msg(format!(
                "{name} {} now, so :e! would lose your changes; not reloaded",
                why.describe()
            )));
        }
        let t = self.text_mut(buf)?;
        let changes = line_changes(&t.rope().to_string(), &text);
        if !changes.is_empty() {
            t.undo_boundary();
            let mut tx = t.tx(Origin::Extension);
            for (r, s) in changes {
                tx = tx.replace(r, s);
            }
            if was.is_some() || why.is_some() {
                tx = tx.silent();
            }
            tx.commit()?;
            t.undo_boundary();
            self.messages.push(format!("Reverted {name}"));
        }
        match (was, why) {
            (None, Some(why)) => {
                let settings = self.text_mut(buf)?.settings_mut();
                let before = std::mem::replace(&mut settings.read_only, true);
                if let Some(f) = self.files.by_buffer.get_mut(&buf) {
                    f.read_only_before = before;
                }
                let line = format!("{name} {}; now read-only", why.describe());
                self.messages.push(line);
            }
            (Some(_), None) => {
                let before = self.visit(buf)?.read_only_before;
                self.text_mut(buf)?.settings_mut().read_only = before;
            }
            _ => {}
        }
        if let Some(f) = self.files.by_buffer.get_mut(&buf) {
            f.bom = bom;
            f.read_only = why;
        }
        self.settle(buf, fingerprint)
    }

    /// The buffer visiting `path`, which is absolute and normalized: the one with
    /// that path, or else one reaching the same file another way.
    pub(crate) fn find_file_buffer(&self, path: &Path) -> Option<BufferId> {
        let files = &self.files.by_buffer;
        let id = |p: &Path| fs::metadata(p).ok().map(|m| (m.dev(), m.ino()));
        let same_file = || {
            let want = id(path)?;
            files.iter().find(|(_, f)| id(&f.path) == Some(want))
        };
        files
            .iter()
            .find(|(_, f)| f.path == path)
            .or_else(same_file)
            .map(|(b, _)| *b)
    }

    /// Settings by path, from your config: called with each file's path and its
    /// buffer's settings when the file opens. Until file-type modes (M1), this is
    /// where wrap, width and line numbers are set per kind of file. Set it once,
    /// before any file opens, the session's included: a second call replaces the
    /// first. If `f` panics, the file opens with `Config::text_defaults` and a message.
    pub fn set_file_settings(&mut self, f: impl Fn(&Path, &mut Settings) + 'static) {
        self.files.settings = Some(Box::new(f));
    }

    /// What status lines show in place of the buffer's name: `f` gets the name and the file the
    /// buffer visits. A second call replaces the first. If `f` panics, it's logged once and
    /// status lines go back to the name.
    pub fn set_status_text(&mut self, f: impl Fn(&str, Option<&FileInfo>) -> String + 'static) {
        self.files.status_text = Some(Box::new(f));
    }

    /// What `buf`'s status line says.
    pub(crate) fn status_text(&mut self, buf: BufferId) -> String {
        let name = self.buffers.get(buf).map_or("", |b| &b.name);
        let Some(f) = &self.files.status_text else {
            return name.to_owned();
        };
        match catch(|| f(name, self.files.by_buffer.get(&buf))) {
            Ok(text) => text,
            Err(c) => {
                let (msg, name) = (c.msg, name.to_owned());
                self.log_error(format!(
                    "status text panicked: {msg}. Status lines show the buffer's name now."
                ));
                self.files.status_text = None;
                name
            }
        }
    }
}

/// `path` with `.` and `..` resolved without touching the disk.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// A regular file's bytes, and its fingerprint from just before they were read.
/// A change during the read then shows as a change on disk: `:w` refuses, and
/// auto-revert reloads. Other files aren't read: opening a FIFO waits for a
/// writer, and a device can be endless.
fn read(path: &Path) -> io::Result<(Vec<u8>, Fingerprint)> {
    if !fs::metadata(path)?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    let mut f = File::open(path)?;
    let m = f.metadata()?;
    let mut bytes = Vec::with_capacity(m.len() as usize);
    f.read_to_end(&mut bytes)?;
    Ok((bytes, Fingerprint::from_metadata(&m)?))
}

/// The text, whether it had a byte-order mark, and why it's read-only, if it is.
fn decode(mut bytes: Vec<u8>) -> (String, bool, Option<Uneditable>) {
    let bom = bytes.starts_with(BOM);
    if bom {
        bytes.drain(..3);
    }
    match String::from_utf8(bytes) {
        Ok(s) if s.contains('\0') => (s, bom, Some(Uneditable::NulBytes)),
        Ok(s) if s.contains("\r\n") => (s, bom, Some(Uneditable::Crlf)),
        Ok(s) => (s, bom, None),
        Err(e) => (
            String::from_utf8_lossy(e.as_bytes()).into_owned(),
            bom,
            Some(Uneditable::NotUtf8),
        ),
    }
}

/// The fingerprint of what's at `path` now, following symlinks, and None if nothing is there.
/// Anything but a regular file is an error.
fn on_disk(path: &Path) -> io::Result<Option<Fingerprint>> {
    match fs::metadata(path) {
        Ok(m) if m.is_file() => Fingerprint::from_metadata(&m).map(Some),
        Ok(_) => Err(io::Error::other("not a regular file")),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Whether the file may still be being written.
fn settling(f: Fingerprint) -> bool {
    let age = SystemTime::now().duration_since(f.modified);
    age.is_ok_and(|age| age < SETTLING)
}

/// `path` with its own symlinks followed, a dangling one too, so the result isn't a symlink.
/// `canonicalize` fails on a dangling one, and the system follows those in directories above.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    let mut p = path.to_path_buf();
    for _ in 0..MAX_LINKS {
        match fs::read_link(&p) {
            Ok(to) => p = p.parent().map_or_else(|| to.clone(), |dir| dir.join(&to)),
            Err(_) => return Ok(p),
        }
    }
    Err(io::Error::other("too many levels of symlinks"))
}

/// Write the text where `path` leads, and return the fingerprint of what's there now.
fn store(path: &Path, text: &Rope, bom: bool) -> io::Result<Fingerprint> {
    let real = resolve(path)?;
    if on_disk(&real)?.is_none() {
        // A new file: umask sets the mode, and a crash has nothing to lose. Emacs asks before it
        // makes the directory. A save can't ask yet, so the open said it would.
        if let Some(dir) = real.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut f = File::options().write(true).create_new(true).open(&real)?;
        put(&mut f, text, bom)?;
        return stamp(&f);
    }
    let old = fs::metadata(&real)?;
    if old.nlink() == 1
        && let Some(fingerprint) = replace(&real, &old, text, bom)?
    {
        return Ok(fingerprint);
    }
    let mut f = File::options().write(true).open(&real)?;
    put(&mut f, text, bom)?;
    let end = f.stream_position()?;
    f.set_len(end)?;
    stamp(&f)
}

/// Write a new file beside `real`, with `old`'s owner and mode, and rename it over `real`. None
/// if it can't be made there or can't have that owner, which leaves everything as it was.
fn replace(
    real: &Path,
    old: &fs::Metadata,
    text: &Rope,
    bom: bool,
) -> io::Result<Option<Fingerprint>> {
    let Some(tmp) = temp_beside(real)? else {
        return Ok(None);
    };
    let mut f = copy_for_its_attributes(real, &tmp)?;
    let mine = f.metadata()?;
    let same = (mine.uid(), mine.gid()) == (old.uid(), old.gid());
    if !same && fchown(&f, Some(old.uid()), Some(old.gid())).is_err() {
        fs::remove_file(&tmp)?;
        return Ok(None);
    }
    let done = put(&mut f, text, bom)
        .and_then(|()| f.set_permissions(old.permissions()))
        .and_then(|()| stamp(&f))
        .and_then(|fingerprint| fs::rename(&tmp, real).map(|()| Some(fingerprint)));
    if done.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    done
}

/// Make `tmp`, which exists and is empty, a copy of `real` emptied again, which on macOS keeps
/// the extended attributes and ACLs. `tmp` has to exist: for a new name, `fs::copy` makes a clone
/// on APFS, without the ACLs. A file that can't be copied, one DIE can't read say, gives an empty
/// `tmp` without them.
fn copy_for_its_attributes(real: &Path, tmp: &Path) -> io::Result<File> {
    let copied = fs::copy(real, tmp).and_then(|_| {
        fs::set_permissions(tmp, fs::Permissions::from_mode(0o600))?;
        File::options().write(true).truncate(true).open(tmp)
    });
    copied.or_else(|_| {
        let _ = fs::remove_file(tmp);
        let mut open = File::options();
        open.write(true).create_new(true).mode(0o600).open(tmp)
    })
}

/// A new empty file beside `real` that only its owner can read, until the save sets the mode.
/// None if the directory doesn't let DIE make one.
fn temp_beside(real: &Path) -> io::Result<Option<PathBuf>> {
    let dir = real.parent().unwrap_or(real);
    for n in 0..1024 {
        let tmp = dir.join(format!(".die-save-{}-{n}", std::process::id()));
        let mut open = File::options();
        match open.write(true).create_new(true).mode(0o600).open(&tmp) {
            Ok(_) => return Ok(Some(tmp)),
            Err(e) => match e.kind() {
                ErrorKind::AlreadyExists => {}
                ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => return Ok(None),
                _ => return Err(e),
            },
        }
    }
    Err(io::Error::other("can't name a temporary file"))
}

fn put(f: &mut File, text: &Rope, bom: bool) -> io::Result<()> {
    if bom {
        f.write_all(BOM)?;
    }
    text.chunks().try_for_each(|c| f.write_all(c.as_bytes()))
}

/// Flush `f`, and return its fingerprint as written. A rename doesn't change it.
fn stamp(f: &File) -> io::Result<Fingerprint> {
    f.sync_all()?;
    Fingerprint::from_metadata(&f.metadata()?)
}

/// The changes that turn `old` into `new`: byte ranges of `old`, in order, and what replaces
/// them. The lines the two share at each end are left out, so an append costs no hashing. In
/// between, lines that appear exactly once in each anchor the match, in the longest run that
/// keeps their order, as patience diff does. So `cargo fmt` changing two far apart places leaves
/// a cursor between them where it was. It takes O(n log n) whatever the files are, and at worst
/// it's one change, with what the two share at its ends left out, as Emacs's revert does.
fn line_changes(old: &str, new: &str) -> Vec<(Range<usize>, String)> {
    let start = shared_start(old, new);
    let end = shared_end(&old[start..], &new[start..]);
    let (old, new) = (&old[start..old.len() - end], &new[start..new.len() - end]);
    let (a, b) = (line_starts(old), line_starts(new));
    let mut out = vec![];
    let (mut i, mut j) = (0, 0);
    if old.len() + new.len() <= ANCHORS_UP_TO {
        for (ai, bj) in anchors(old, &a, new, &b) {
            push_change(&mut out, old, a[i]..a[ai], new, b[j]..b[bj]);
            (i, j) = (ai + 1, bj + 1);
        }
    }
    push_change(&mut out, old, a[i]..old.len(), new, b[j]..new.len());
    for (r, _) in &mut out {
        *r = r.start + start..r.end + start;
    }
    out
}

/// How many bytes of whole lines `old` and `new` share at their start.
fn shared_start(old: &str, new: &str) -> usize {
    const BLOCK: usize = 4096;
    let (x, y) = (old.as_bytes(), new.as_bytes());
    let same = x.chunks(BLOCK).zip(y.chunks(BLOCK));
    let blocks = same.take_while(|(p, q)| p == q).count() * BLOCK;
    let at = blocks.min(x.len()).min(y.len());
    let pairs = x[at..].iter().zip(&y[at..]);
    let n = at + pairs.take_while(|(p, q)| p == q).count();
    x[..n]
        .iter()
        .rposition(|c| *c == b'\n')
        .map_or(0, |i| i + 1)
}

/// How many bytes of whole lines `old` and `new` share at their end.
fn shared_end(old: &str, new: &str) -> usize {
    let (x, y) = (old.as_bytes(), new.as_bytes());
    let pairs = x.iter().rev().zip(y.iter().rev());
    let n = pairs.take_while(|(p, q)| p == q).count();
    let starts_a_line = |t: &[u8]| t.len() == n || t[t.len() - n - 1] == b'\n';
    if starts_a_line(x) && starts_a_line(y) {
        return n;
    }
    let tail = &x[x.len() - n..];
    tail.iter()
        .position(|c| *c == b'\n')
        .map_or(0, |i| n - i - 1)
}

/// Where each line starts, then the end of the text.
fn line_starts(s: &str) -> Vec<usize> {
    let mut v = vec![0];
    v.extend(s.match_indices('\n').map(|(i, _)| i + 1));
    if v.last() != Some(&s.len()) {
        v.push(s.len());
    }
    v
}

/// Pairs of equal lines, as indices into `a` and `b`, both rising.
fn anchors(old: &str, a: &[usize], new: &str, b: &[usize]) -> Vec<(usize, usize)> {
    #[derive(Default)]
    struct Seen {
        old: (u32, usize),
        new: (u32, usize),
    }
    let mut seen: HashMap<&str, Seen> = HashMap::new();
    for i in 0..a.len() - 1 {
        let e = seen.entry(&old[a[i]..a[i + 1]]).or_default();
        e.old = (e.old.0 + 1, i);
    }
    for j in 0..b.len() - 1 {
        let e = seen.entry(&new[b[j]..b[j + 1]]).or_default();
        e.new = (e.new.0 + 1, j);
    }
    let unique = seen.values().filter(|s| s.old.0 == 1 && s.new.0 == 1);
    let mut pairs: Vec<(usize, usize)> = unique.map(|s| (s.old.1, s.new.1)).collect();
    pairs.sort_unstable();
    // The longest rising run of the `new` indices: `tails[n]` ends the best run of length n + 1.
    let mut tails: Vec<usize> = vec![];
    let mut before = vec![None; pairs.len()];
    for k in 0..pairs.len() {
        let n = tails.partition_point(|t| pairs[*t].1 < pairs[k].1);
        before[k] = n.checked_sub(1).map(|n| tails[n]);
        if n == tails.len() {
            tails.push(k);
        } else {
            tails[n] = k;
        }
    }
    let mut run = vec![];
    let mut at = tails.last().copied();
    while let Some(k) = at {
        run.push(pairs[k]);
        at = before[k];
    }
    run.reverse();
    run
}

/// Add the change from `old[r]` to `new[s]`, without what they share at either end.
fn push_change(
    out: &mut Vec<(Range<usize>, String)>,
    old: &str,
    r: Range<usize>,
    new: &str,
    s: Range<usize>,
) {
    let (x, y) = (&old[r.clone()], &new[s]);
    let mut start = x.bytes().zip(y.bytes()).take_while(|(p, q)| p == q).count();
    while !x.is_char_boundary(start) {
        start -= 1;
    }
    let (x, y) = (&x[start..], &y[start..]);
    let same = |(p, q): &(u8, u8)| p == q;
    let mut end = x
        .bytes()
        .rev()
        .zip(y.bytes().rev())
        .take_while(same)
        .count();
    while !x.is_char_boundary(x.len() - end) {
        end -= 1;
    }
    if x.len() > end || y.len() > end {
        let at = r.start + start;
        out.push((at..at + x.len() - end, y[..y.len() - end].to_string()));
    }
}
