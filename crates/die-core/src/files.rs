//! Files, read side (K10a): opening a file into a text buffer, the UTF-8 check,
//! the fingerprint `:w` compares before writing, and settings by path.
//!
//! Only UTF-8 text is editable. A file that isn't UTF-8, holds NUL bytes or has
//! CRLF line ends opens read-only with a message, so a save can't change its
//! bytes behind your back. A UTF-8 byte-order mark is kept out of the text and
//! remembered for the save. The write side (saving, the `:w` refusal,
//! auto-revert) is K10b.

use crate::{BufferId, Ctx, Error, Result, Settings};
use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

/// What `:w` compares to tell whether a file changed on disk since DIE last read
/// or wrote it: its modification time, size and identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub modified: SystemTime,
    pub len: u64,
    pub dev: u64,
    pub inode: u64,
}

impl Fingerprint {
    /// The fingerprint of the file at `path` now, following symlinks.
    pub fn of(path: &Path) -> std::io::Result<Fingerprint> {
        Fingerprint::from_metadata(&fs::metadata(path)?)
    }

    fn from_metadata(m: &fs::Metadata) -> std::io::Result<Fingerprint> {
        Ok(Fingerprint {
            modified: m.modified()?,
            len: m.len(),
            dev: m.dev(),
            inode: m.ino(),
        })
    }
}

/// Why a file opened read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOnly {
    /// Not valid UTF-8. The text shows U+FFFD where bytes are invalid.
    NotUtf8,
    NulBytes,
    /// CRLF line ends, which DIE doesn't edit yet.
    Crlf,
}

impl ReadOnly {
    fn describe(self) -> &'static str {
        match self {
            ReadOnly::NotUtf8 => "not UTF-8",
            ReadOnly::NulBytes => "holds NUL bytes",
            ReadOnly::Crlf => "has CRLF line ends",
        }
    }
}

/// The file a buffer visits.
#[derive(Clone, Debug, PartialEq)]
pub struct FileInfo {
    /// Absolute, with `.` and `..` resolved. Symlinks are kept as given.
    pub path: PathBuf,
    /// As of the last read or write. None while the file doesn't exist.
    pub fingerprint: Option<Fingerprint>,
    /// The file starts with a UTF-8 byte-order mark, which isn't in the text.
    pub bom: bool,
    pub read_only: Option<ReadOnly>,
}

type SettingsFn = Rc<dyn Fn(&Path, &mut Settings)>;

/// Which buffers visit files, and the config's settings by path.
#[derive(Default)]
pub(crate) struct Files {
    by_buffer: HashMap<BufferId, FileInfo>,
    settings: Option<SettingsFn>,
}

impl Files {
    pub(crate) fn forget(&mut self, buf: BufferId) {
        self.by_buffer.remove(&buf);
    }
}

impl Ctx {
    /// Open the file at `path`, which must be absolute, into a text buffer named
    /// after it, or return the buffer already visiting it (through a symlink too).
    /// A missing file opens as an empty buffer that a save will create. The
    /// settings from [`Ctx::set_file_settings`] apply, then read-only for a file
    /// that can't be edited safely, with a message.
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
                let (text, bom, ro) = decode(bytes);
                (text, Some(fp), bom, ro)
            }
            Err(e) if e.kind() == ErrorKind::NotFound => (String::new(), None, false, None),
            Err(e) => return Err(Error::Msg(format!("{}: {e}", path.display()))),
        };
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let buf = self.create_text(&name, &text);
        let config = self.files.settings.clone();
        let settings = self.text_mut(buf)?.settings_mut();
        if let Some(f) = config {
            f(&path, settings);
        }
        if let Some(ro) = read_only {
            settings.read_only = true;
            self.messages
                .push(format!("{name} {}; opened read-only", ro.describe()));
        }
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

    /// The buffer visiting `path`: the same path, or the same file reached another way.
    pub fn find_file_buffer(&self, path: &Path) -> Option<BufferId> {
        let path = normalize(path);
        let id = |p: &Path| fs::metadata(p).ok().map(|m| (m.dev(), m.ino()));
        let want = id(&path);
        let files = &self.files.by_buffer;
        files
            .iter()
            .find(|(_, f)| f.path == path || want.is_some() && id(&f.path) == want)
            .map(|(b, _)| *b)
    }

    /// Settings by path, from your config: called with each file's path and its
    /// buffer's settings when the file opens. Until file-type modes (M1), this is
    /// where wrap, width and line numbers are set per kind of file.
    pub fn set_file_settings(&mut self, f: impl Fn(&Path, &mut Settings) + 'static) {
        self.files.settings = Some(Rc::new(f));
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

/// The file's bytes and its fingerprint, read so they agree: if the file
/// changes while it's read, it's read again.
fn read(path: &Path) -> std::io::Result<(Vec<u8>, Fingerprint)> {
    for _ in 0..3 {
        let mut f = fs::File::open(path)?;
        let before = f.metadata()?;
        if before.is_dir() {
            return Err(std::io::Error::new(
                ErrorKind::IsADirectory,
                "is a directory",
            ));
        }
        let mut bytes = Vec::with_capacity(before.len() as usize);
        f.read_to_end(&mut bytes)?;
        let (fp, after) = (
            Fingerprint::from_metadata(&before)?,
            Fingerprint::from_metadata(&f.metadata()?)?,
        );
        if fp == after {
            return Ok((bytes, fp));
        }
    }
    Err(std::io::Error::other(
        "the file kept changing while it was read",
    ))
}

/// The text, whether it had a byte-order mark, and why it's read-only, if it is.
fn decode(mut bytes: Vec<u8>) -> (String, bool, Option<ReadOnly>) {
    let bom = bytes.starts_with(b"\xEF\xBB\xBF");
    if bom {
        bytes.drain(..3);
    }
    match String::from_utf8(bytes) {
        Ok(s) if s.contains('\0') => (s, bom, Some(ReadOnly::NulBytes)),
        Ok(s) if s.contains("\r\n") => (s, bom, Some(ReadOnly::Crlf)),
        Ok(s) => (s, bom, None),
        Err(e) => (
            String::from_utf8_lossy(e.as_bytes()).into_owned(),
            bom,
            Some(ReadOnly::NotUtf8),
        ),
    }
}
