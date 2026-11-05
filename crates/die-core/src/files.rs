//! Files, read side (K10a): opening a file into a text buffer, the UTF-8 check,
//! the fingerprint `:w` compares before writing, and settings by path.
//!
//! Only UTF-8 text is editable. A file that isn't UTF-8, holds NUL bytes or has
//! CRLF line ends opens read-only with a message, so a save can't change its
//! bytes behind your back. A UTF-8 byte-order mark is kept out of the text and
//! remembered for the save. The write side (saving, the `:w` refusal,
//! auto-revert) is K10b.
//!
//! Settings by path are a stopgap until file-type modes (M1), which extensions
//! will declare at Register.

use crate::ext::catch;
use crate::{BufferId, Ctx, Error, Result, Settings};
use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

/// What `:w` compares to tell whether a file changed on disk since DIE last read
/// or wrote it: its modification time, size and inode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    modified: SystemTime,
    len: u64,
    inode: u64,
}

impl Fingerprint {
    fn from_metadata(m: &fs::Metadata) -> std::io::Result<Fingerprint> {
        Ok(Fingerprint {
            modified: m.modified()?,
            len: m.len(),
            inode: m.ino(),
        })
    }
}

/// Why a file can't be edited safely, so it opened read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// As of the last read or write. None while the file doesn't exist.
    pub fingerprint: Option<Fingerprint>,
    /// The file starts with a UTF-8 byte-order mark, which isn't in the text.
    pub bom: bool,
    /// Why the file can't be edited safely, if it can't. A save must refuse while
    /// this is set: the buffer's [`Settings::read_only`] can be turned off, and
    /// blocks only `User` edits.
    pub read_only: Option<Uneditable>,
}

/// Settings by path, as [`crate::Config`] and [`Ctx::set_file_settings`] take them.
pub(crate) type SettingsFn = Box<dyn Fn(&Path, &mut Settings)>;

/// Which buffers visit files, and the config's settings by path.
#[derive(Default)]
pub(crate) struct Files {
    by_buffer: HashMap<BufferId, FileInfo>,
    pub(crate) settings: Option<SettingsFn>,
}

impl Files {
    pub(crate) fn forget(&mut self, buf: BufferId) {
        self.by_buffer.remove(&buf);
    }
}

impl Ctx {
    /// Open the file at `path`, which must be absolute, into a text buffer named
    /// after it, or return the buffer already visiting it (through a symlink too).
    /// `..` is resolved as [`FileInfo::path`] says. A missing file opens as an
    /// empty buffer that a save will create, and anything else that isn't a
    /// readable regular file is an error. The settings from
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
            Err(e) if e.kind() == ErrorKind::NotFound => (String::new(), None, false, None),
            Err(e) => return Err(Error::Msg(format!("{}: {e}", path.display()))),
        };
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let mut settings = self.text_defaults.clone();
        if let Some(f) = &self.files.settings
            && let Err(c) = catch(|| f(&path, &mut settings))
        {
            settings = self.text_defaults.clone();
            self.messages.push(format!(
                "{name}: file settings panicked ({}); opened with the defaults",
                c.msg
            ));
        }
        if let Some(why) = read_only {
            settings.read_only = true;
            self.messages
                .push(format!("{name} {}; opened read-only", why.describe()));
        }
        let buf = self.create_text(&name, &text);
        *self.text_mut(buf)?.settings_mut() = settings;
        self.files.by_buffer.insert(
            buf,
            FileInfo {
                path,
                fingerprint,
                bom,
                read_only,
            },
        );
        Ok(buf)
    }

    /// The file `buf` visits, if any.
    pub fn file(&self, buf: BufferId) -> Option<&FileInfo> {
        self.files.by_buffer.get(&buf)
    }

    /// The buffer visiting `path`, which is absolute and normalized: the one with
    /// that path, or else one reaching the same file another way.
    fn find_file_buffer(&self, path: &Path) -> Option<BufferId> {
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
}

/// `path` with `.` and `..` resolved without touching the disk.
fn normalize(path: &Path) -> PathBuf {
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
fn read(path: &Path) -> std::io::Result<(Vec<u8>, Fingerprint)> {
    if !fs::metadata(path)?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    let mut f = fs::File::open(path)?;
    let m = f.metadata()?;
    let mut bytes = Vec::with_capacity(m.len() as usize);
    f.read_to_end(&mut bytes)?;
    Ok((bytes, Fingerprint::from_metadata(&m)?))
}

/// The text, whether it had a byte-order mark, and why it's read-only, if it is.
fn decode(mut bytes: Vec<u8>) -> (String, bool, Option<Uneditable>) {
    let bom = bytes.starts_with(b"\xEF\xBB\xBF");
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
