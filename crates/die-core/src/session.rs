//! The session (K12, contract §14, scope §5.9): what a crash or a restart must not lose.
//!
//! **The file.** `session.json` in DIE's state directory: a version, then one record per buffer,
//! each read on its own and with defaults for what it lacks, so one that fails to load drops only
//! that buffer, with a message. A build reads any version's file, whichever wrote it, and ignores
//! what it doesn't know. A record holds its text when that text isn't on disk: unsaved changes,
//! a buffer with no file, and a file deleted under it. With unsaved changes it also holds the
//! file's fingerprint, so a file that changed while DIE was down makes `:w` refuse as it always
//! does. A clean buffer reads its file again.
//!
//! **Writing.** About 1 s after the last change, or about 5 s after the first change not yet
//! written if changes keep coming, and on restart and quit. The main thread takes a
//! snapshot, whose ropes share their text, and a thread of its own encodes and writes it: to a
//! temporary file, flushed, renamed over the last one, which is kept as `session.prev.json`.
//! A crash at any point leaves one or the other, and a load falls back on the previous copy.
//! A file or a record that can't be read is copied to `session.rejected-*.json` first, since
//! the next write replaces it and it may hold your text.
//!
//! **Who writes.** Only the instance holding the lock. So writes start paused, a host resumes
//! them once it holds the lock ([`Ctx::resume_session`]), and a restart pauses them
//! ([`Ctx::hand_over_session`]) until it's known to have failed.
//!
//! **Beside the buffers** (K13). The split tree with its design sizes, each window's selection and
//! scroll in every buffer it has shown, which window has the focus, the registers with their
//! kinds and the prompt histories. A window showing a buffer that didn't come back goes, and its
//! cells with it. A tree that can't be read is dropped with a message, and the focused buffer goes
//! on screen as a file with no tree does. When no window or focus can come back, because what had
//! the focus isn't kept or is gone, the buffer used last goes on screen instead. A window on
//! `*messages*` stays, on the one this instance has, as its text isn't saved. A selection in visual
//! state comes back in visual state. Other states don't come back, as what they wait for is gone,
//! and a cursor left where only they allow it goes where normal state rests. A restore leaves
//! insert and replace state as Escape does, moving the cursor by [`Ctx::set_insert_end`]'s rule
//! and changing no text.
//!
//! What die-vim holds itself isn't saved: `.`, `@@`, `;`, `n`, `gv`. Windows outside the tree aren't
//! either, since only a host that places its own windows has them.

use crate::ext::catch;
use crate::files::{FileState, Fingerprint, Uneditable};
use crate::ids::*;
use crate::keymap::State;
use crate::messages::MESSAGES;
use crate::prompt::HISTORY;
use crate::windows::{View, Window};
use crate::workspace::tree::Layout;
use crate::workspace::{RegKind, Register, ShowIn};
use crate::{Content, Ctx, Error, Origin, Result, Selection, Shape, Sub, TextBuf};
use ropey::Rope;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, DirBuilder, File};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, SystemTime};

/// The schema this build writes.
pub const VERSION: u32 = 1;
/// The session, in the state directory.
pub const FILE: &str = "session.json";
/// The copy before it.
pub const PREVIOUS: &str = "session.prev.json";
const TEMPORARY: &str = "session.tmp";

/// How often core looks for a change to write.
const LOOK_EVERY: Duration = Duration::from_millis(500);
/// How long nothing must change before a write.
const QUIET: Duration = Duration::from_secs(1);
/// How long a change waits for that quiet before it's written anyway.
const MOST_BEHIND: Duration = Duration::from_secs(5);

type Outcome = std::result::Result<(), String>;

/// A path as JSON: a string, or its bytes when it isn't UTF-8.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
enum PathRecord {
    Text(String),
    Bytes { bytes: Vec<u8> },
}

impl PathRecord {
    fn new(path: &Path) -> PathRecord {
        match path.to_str() {
            Some(s) => PathRecord::Text(s.to_owned()),
            None => PathRecord::Bytes {
                bytes: path.as_os_str().as_bytes().to_vec(),
            },
        }
    }

    fn path(&self) -> PathBuf {
        match self {
            PathRecord::Text(s) => PathBuf::from(s),
            PathRecord::Bytes { bytes } => PathBuf::from(OsStr::from_bytes(bytes)),
        }
    }
}

/// What a saved file was: its modification time from the epoch, floored to the second, then
/// nanoseconds on from there.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(default)]
struct FingerprintRecord {
    secs: i64,
    nanos: u32,
    len: u64,
    inode: u64,
}

impl FingerprintRecord {
    fn new(f: &Fingerprint) -> FingerprintRecord {
        let (modified, len, inode) = f.parts();
        let (secs, nanos) = match modified.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => (d.as_secs() as i64, d.subsec_nanos()),
            Err(e) => {
                let d = e.duration();
                match d.subsec_nanos() {
                    0 => (-(d.as_secs() as i64), 0),
                    n => (-(d.as_secs() as i64) - 1, 1_000_000_000 - n),
                }
            }
        };
        FingerprintRecord {
            secs,
            nanos,
            len,
            inode,
        }
    }

    /// None for a time this system can't hold, which then shows as a change on disk.
    fn fingerprint(&self) -> Option<Fingerprint> {
        let secs = Duration::from_secs(self.secs.unsigned_abs());
        let epoch = SystemTime::UNIX_EPOCH;
        let whole = if self.secs < 0 {
            epoch.checked_sub(secs)?
        } else {
            epoch.checked_add(secs)?
        };
        let modified = whole.checked_add(Duration::from_nanos(u64::from(self.nanos)))?;
        Some(Fingerprint::from_parts(modified, self.len, self.inode))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct FileRecord {
    path: PathRecord,
    #[serde(default)]
    fingerprint: Option<FingerprintRecord>,
    #[serde(default)]
    bom: bool,
    #[serde(default)]
    uneditable: Option<Uneditable>,
    #[serde(default)]
    read_only_before: bool,
}

/// A window's selection and scroll in one buffer, shown now or waiting for it to come back.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
struct ViewRecord {
    /// Which of the file's `windows`.
    window: usize,
    anchor: usize,
    head: usize,
    shape: Shape,
    goal: Option<usize>,
    top: usize,
    top_row: usize,
}

impl ViewRecord {
    fn new(window: usize, v: &View) -> ViewRecord {
        ViewRecord {
            window,
            anchor: v.sel.anchor,
            head: v.sel.head,
            shape: v.sel.shape,
            goal: v.sel.goal,
            top: v.top,
            top_row: v.top_row,
        }
    }

    fn view(&self, t: &TextBuf) -> View {
        View {
            sel: Selection {
                anchor: fit(t, self.anchor),
                head: fit(t, self.head),
                shape: self.shape,
                goal: self.goal,
            },
            top: fit(t, self.top),
            top_row: self.top_row,
            cols: 0,
            rows: 0,
        }
    }
}

/// A window in the tree. The file lists them with the one used last at the end.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(default)]
struct WindowRecord {
    /// Which of the file's `buffers`. None for what a session doesn't keep.
    buffer: Option<usize>,
    /// It shows `*messages*`, whose text isn't kept but which every instance has.
    messages: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegisterRecord {
    name: char,
    kind: RegKind,
    text: String,
}

/// Every register, in order of name. Shared, so looking for changes doesn't copy their text.
pub(crate) type SavedRegisters = Arc<Vec<RegisterRecord>>;

/// A list whose unreadable items are skipped, as is a value that isn't a list: a new build's view
/// mustn't cost an old one a buffer.
fn readable<'de, D: serde::Deserializer<'de>, T: serde::de::DeserializeOwned>(
    d: D,
) -> std::result::Result<Vec<T>, D::Error> {
    let Value::Array(list) = Value::deserialize(d)? else {
        return Ok(vec![]);
    };
    let read = |v| serde_json::from_value(v).ok();
    Ok(list.into_iter().filter_map(read).collect())
}

/// One buffer in the file. Every field but a file's path has a default, so builds can differ.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
struct BufferRecord {
    name: String,
    directory: Option<PathRecord>,
    file: Option<FileRecord>,
    /// The text, when the file doesn't have it.
    text: Option<String>,
    modified: bool,
    /// None from a build that didn't save it, which leaves what the buffer opened with.
    read_only: Option<bool>,
    /// The view a window that has none of its own starts from.
    cursor: usize,
    top: usize,
    top_row: usize,
    #[serde(deserialize_with = "readable")]
    views: Vec<ViewRecord>,
    visual: bool,
    /// It was in insert or replace state, which a restore leaves.
    insert: bool,
}

/// A buffer as of a snapshot. Its text is shared, and read when it's encoded.
#[derive(Clone)]
struct Saved {
    record: BufferRecord,
    rev: u64,
    rope: Option<Rope>,
}

impl PartialEq for Saved {
    /// `rev` says whether the text changed.
    fn eq(&self, other: &Saved) -> bool {
        self.rev == other.rev && self.record == other.record
    }
}

/// The session at some moment, oldest buffer first, which is the order a restore makes them in.
#[derive(Clone, PartialEq)]
struct Snapshot {
    focus: Option<usize>,
    buffers: Vec<Saved>,
    layout: Option<Layout>,
    windows: Vec<WindowRecord>,
    focused_window: Option<usize>,
    registers: SavedRegisters,
    histories: BTreeMap<String, Vec<String>>,
}

#[derive(Serialize)]
struct SessionFile<'a> {
    version: u32,
    focus: Option<usize>,
    buffers: &'a [BufferRecord],
    layout: &'a Option<Layout>,
    windows: &'a [WindowRecord],
    focused_window: Option<usize>,
    registers: &'a [RegisterRecord],
    histories: &'a BTreeMap<String, Vec<String>>,
}

impl Snapshot {
    fn encode(&self) -> Vec<u8> {
        let buffers: Vec<BufferRecord> = self
            .buffers
            .iter()
            .map(|b| BufferRecord {
                text: b.rope.as_ref().map(Rope::to_string),
                ..b.record.clone()
            })
            .collect();
        let file = SessionFile {
            version: VERSION,
            focus: self.focus,
            buffers: &buffers,
            layout: &self.layout,
            windows: &self.windows,
            focused_window: self.focused_window,
            registers: &self.registers,
            histories: &self.histories,
        };
        // Strings, numbers and options: there's nothing here that can fail to encode.
        serde_json::to_vec_pretty(&file).expect("a session encodes")
    }
}

enum Job {
    Write(Snapshot),
    /// Write this and say how it went.
    WriteNow(Snapshot, Sender<Outcome>),
    /// Say when everything before this is done.
    Flush(Sender<()>),
}

/// A session being kept in a directory.
pub(crate) struct Session {
    jobs: Sender<Job>,
    outcomes: Receiver<Outcome>,
    /// Off until the host holds the lock, and during a handover.
    writing: bool,
    /// What the last look found, and when it first found it.
    seen: Option<(Snapshot, Duration)>,
    written: Option<Snapshot>,
    /// When a look first found something `written` doesn't have. Stale, it only writes early.
    unwritten: Option<Duration>,
    /// A failure was reported and no write has worked since.
    failing: bool,
    _look: Sub,
}

/// Write the bytes durably, keeping the file they replace.
fn store(dir: &Path, bytes: &[u8]) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let temporary = dir.join(TEMPORARY);
    let mut open = File::options();
    open.write(true).create(true).truncate(true).mode(0o600);
    let mut f = open.open(&temporary)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    let file = dir.join(FILE);
    match fs::rename(&file, dir.join(PREVIOUS)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    fs::rename(&temporary, &file)?;
    // The file is safe whatever this does. Not every file system can flush a directory.
    let _ = File::open(dir).and_then(|d| d.sync_all());
    Ok(())
}

fn writer(dir: PathBuf, jobs: Receiver<Job>, outcomes: Sender<Outcome>) {
    let write = |s: &Snapshot| store(&dir, &s.encode()).map_err(|e| e.to_string());
    for job in jobs {
        match job {
            Job::Write(s) => {
                let _ = outcomes.send(write(&s));
            }
            Job::WriteNow(s, done) => {
                let _ = done.send(write(&s));
            }
            Job::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}

/// The saved session, as read from one file.
struct Parsed {
    focus: Option<u64>,
    buffers: Vec<Value>,
    layout: Option<Value>,
    windows: Option<Value>,
    focused_window: Option<u64>,
    registers: Option<Value>,
    histories: Option<Value>,
}

fn parse(bytes: &[u8]) -> std::result::Result<Parsed, String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let top = v.as_object().ok_or("not a JSON object")?;
    let buffers = match top.get("buffers") {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(list)) => list.clone(),
        Some(_) => return Err("its buffers aren't a list".into()),
    };
    let some = |name: &str| top.get(name).filter(|v| !v.is_null()).cloned();
    let number = |name: &str| top.get(name).and_then(Value::as_u64);
    Ok(Parsed {
        focus: number("focus"),
        buffers,
        layout: some("layout"),
        windows: some("windows"),
        focused_window: number("focused_window"),
        registers: some("registers"),
        histories: some("histories"),
    })
}

/// Copy what can't be read, as the next write replaces it. Nothing can be done if this fails.
fn keep_rejected(from: &Path) -> Option<PathBuf> {
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH);
    let at = now.map_or(0, |d| d.as_secs());
    // Both copies can fail within a second, and neither replaces the other.
    let to = (0..100)
        .map(|n| from.with_file_name(format!("session.rejected-{at}-{n}.json")))
        .find(|to| !to.exists())?;
    fs::copy(from, &to).ok().map(|_| to)
}

/// Where a position may be in this text.
fn fit(t: &TextBuf, pos: usize) -> usize {
    let mut p = pos.min(t.len());
    while !t.is_boundary(p) {
        p -= 1;
    }
    p
}

impl Ctx {
    fn snapshot(&self) -> Snapshot {
        let mut order = self.buffer_list();
        order.reverse();
        let mut windows = self.tree.windows();
        windows.sort_by_key(|w| self.windows.get(*w).map(|w| w.used));
        let mut buffers = vec![];
        let mut kept = vec![];
        let mut focus = None;
        let focused = self.focused_buffer();
        for id in order {
            let Some(saved) = self.saved_buffer(id, &windows) else {
                continue;
            };
            if focused == Some(id) {
                focus = Some(buffers.len());
            }
            kept.push(id);
            buffers.push(saved);
        }
        let number = |w: WindowId| windows.iter().position(|o| *o == w);
        let in_file = |b: BufferId| kept.iter().position(|k| *k == b);
        let log = self.find_buffer(MESSAGES);
        Snapshot {
            focus,
            buffers,
            layout: self.tree.layout(&|w| number(w).unwrap_or_default()),
            windows: windows
                .iter()
                .map(|w| WindowRecord {
                    buffer: self.window_buffer(*w).and_then(in_file),
                    messages: log.is_some() && self.window_buffer(*w) == log,
                })
                .collect(),
            focused_window: self.focused.and_then(number),
            registers: self.saved_registers(),
            histories: self
                .histories
                .iter()
                .map(|(name, past)| (name.clone(), past.clone()))
                .collect(),
        }
    }

    fn saved_registers(&self) -> SavedRegisters {
        let all = || {
            let mut list: Vec<_> = self
                .registers
                .iter()
                .map(|(name, r)| RegisterRecord {
                    name: *name,
                    kind: r.kind,
                    text: r.text.clone(),
                })
                .collect();
            list.sort_by_key(|r| r.name);
            Arc::new(list)
        };
        self.registers_saved.get_or_init(all).clone()
    }

    fn saved_buffer(&self, id: BufferId, windows: &[WindowId]) -> Option<Saved> {
        let b = self.buffers.get(id)?;
        let Content::Text(t) = &b.content else {
            return None; // Surfaces save themselves, from M1.
        };
        if b.name == MESSAGES {
            return None;
        }
        let window = self.window_used_last(id);
        let view = window.and_then(|w| t.views.get(&w)).or(t.rest.as_ref());
        let file = self.file(id);
        let on_disk = file.is_some_and(|f| f.fingerprint.is_some());
        let keep_text = t.modified() || !on_disk;
        Some(Saved {
            record: BufferRecord {
                name: b.name.clone(),
                directory: Some(PathRecord::new(&b.directory)),
                file: file.map(|f| FileRecord {
                    path: PathRecord::new(&f.path),
                    fingerprint: f.fingerprint.as_ref().map(FingerprintRecord::new),
                    bom: f.bom,
                    uneditable: f.read_only,
                    read_only_before: f.read_only_before(),
                }),
                text: None,
                modified: t.modified(),
                read_only: Some(t.settings().read_only),
                cursor: view.map_or(0, |v| v.sel.head),
                top: view.map_or(0, |v| v.top),
                top_row: view.map_or(0, |v| v.top_row),
                views: windows
                    .iter()
                    .enumerate()
                    .filter_map(|(i, w)| {
                        let v = t.views.get(w).or_else(|| t.parked.get(w))?;
                        Some(ViewRecord::new(i, v))
                    })
                    .collect(),
                visual: b.state == State::Visual,
                insert: matches!(b.state, State::Insert | State::Replace),
            },
            rev: t.rev(),
            rope: keep_text.then(|| t.rope().clone()),
        })
    }

    /// The session as JSON, as core would write it now.
    pub fn session_json(&self) -> String {
        String::from_utf8_lossy(&self.snapshot().encode()).into_owned()
    }

    /// Restore a session from JSON into this workspace (contract §16's "restore it into a fresh
    /// workspace"). A record that can't be restored is dropped with a message. It's an error,
    /// with nothing restored, when the whole of it can't be read.
    pub fn restore_session(&mut self, json: &str) -> Result {
        let parsed = parse(json.as_bytes()).map_err(Error::Msg)?;
        self.restore_parsed(parsed);
        Ok(())
    }

    /// How many records it dropped for being unreadable.
    fn restore_parsed(&mut self, parsed: Parsed) -> usize {
        let mut dropped = 0;
        let mut focus = None;
        let mut ids = vec![];
        let before = self.buffer_list();
        let mut views = vec![];
        let mut left_insert = vec![];
        for (i, value) in parsed.buffers.into_iter().enumerate() {
            let hint = value.get("name").and_then(Value::as_str);
            let name = hint.unwrap_or("a buffer").to_owned();
            let mut its_views = vec![];
            let mut in_insert = None;
            let restored = match serde_json::from_value::<BufferRecord>(value) {
                Ok(mut record) => {
                    its_views = std::mem::take(&mut record.views);
                    in_insert = record.insert.then_some(record.cursor);
                    catch(|| self.restore_buffer(record, &before))
                        .unwrap_or_else(|c| Err(format!("panicked: {}", c.msg)))
                }
                Err(e) => Err(e.to_string()),
            };
            ids.push(restored.as_ref().ok().copied().flatten());
            match restored {
                Ok(None) => {}
                Ok(Some(id)) => {
                    views.push((id, its_views));
                    left_insert.extend(in_insert.map(|cursor| (id, cursor)));
                    if parsed.focus == Some(i as u64) {
                        focus = Some(id);
                    }
                }
                Err(e) => {
                    dropped += 1;
                    self.messages.push(format!(
                        "session: dropped {name}, which failed to load: {e}"
                    ));
                }
            }
        }
        let registers: Option<Vec<Value>> =
            self.read_part(parsed.registers, "registers", &mut dropped);
        dropped += self.restore_registers(registers.unwrap_or_default());
        let histories: Option<BTreeMap<String, Vec<String>>> =
            self.read_part(parsed.histories, "histories", &mut dropped);
        for (name, mut past) in histories.into_iter().flatten() {
            past.drain(..past.len().saturating_sub(HISTORY));
            self.histories.insert(name, past);
        }
        let layout: Option<Layout> = self.read_part(parsed.layout, "windows", &mut dropped);
        let windows: Option<Vec<WindowRecord>> =
            self.read_part(parsed.windows, "windows", &mut dropped);
        let windows = windows.unwrap_or_default();
        let placed = layout.is_some_and(|layout| {
            let focused = parsed.focused_window.map(|f| f as usize);
            let placing = catch(|| self.restore_windows(&layout, &windows, focused, &ids, views));
            let why = match placing {
                Ok(Ok(placed)) => return placed,
                Ok(Err(why)) => why,
                Err(c) => c.msg,
            };
            self.messages
                .push(format!("session: dropped the windows: {why}"));
            dropped += 1;
            false
        });
        // The file has the buffer used last at the end.
        let last_used = ids.iter().flatten().last().copied();
        if let (false, Some(buf)) = (placed, focus.or(last_used)) {
            self.show_restored(buf);
        }
        for (buf, cursor) in left_insert {
            self.leave_insert(buf, cursor);
        }
        dropped
    }

    /// Insert state didn't come back: the window used last goes from where its cursor was saved to
    /// where Escape would have put it.
    fn leave_insert(&mut self, buf: BufferId, cursor: usize) {
        let window = self.window_used_last(buf);
        let end = self.insert_end;
        let Ok(t) = self.text_mut(buf) else { return };
        if let (Some(end), Some(w)) = (end, window) {
            let p = end(t, w, fit(t, cursor));
            t.set_cursor(w, p);
        }
    }

    /// Part of the file, if it can be read, and otherwise a message and one more dropped.
    fn read_part<T: serde::de::DeserializeOwned>(
        &mut self,
        value: Option<Value>,
        what: &str,
        dropped: &mut usize,
    ) -> Option<T> {
        serde_json::from_value(value?)
            .inspect_err(|e| {
                self.messages
                    .push(format!("session: dropped the {what}: {e}"));
                *dropped += 1;
            })
            .ok()
    }

    /// How many it dropped.
    fn restore_registers(&mut self, records: Vec<Value>) -> usize {
        let mut dropped = 0;
        for value in records {
            match serde_json::from_value::<RegisterRecord>(value) {
                Ok(r) => self.set_register(
                    r.name,
                    Register {
                        kind: r.kind,
                        text: r.text,
                    },
                ),
                Err(e) => {
                    dropped += 1;
                    self.messages
                        .push(format!("session: dropped a register: {e}"));
                }
            }
        }
        self.clipboard_known_from_registers();
        dropped
    }

    /// Replace the split tree with the saved one. False if there's none to put on screen, and an
    /// error if what's saved can't be one.
    fn restore_windows(
        &mut self,
        layout: &Layout,
        windows: &[WindowRecord],
        focused: Option<usize>,
        ids: &[Option<BufferId>],
        views: Vec<(BufferId, Vec<ViewRecord>)>,
    ) -> std::result::Result<bool, String> {
        let saved = |r: &WindowRecord| r.buffer.and_then(|i| ids.get(i).copied().flatten());
        if !windows.iter().any(|r| r.messages || saved(r).is_some()) {
            return Ok(false);
        }
        if !layout.valid(windows.len()) {
            return Err("they don't make a tree".into());
        }
        let log = windows.iter().any(|r| r.messages).then(|| {
            let found = self.find_buffer(MESSAGES);
            found.unwrap_or_else(|| self.create_messages())
        });
        let shows = |r: &WindowRecord| saved(r).or(log.filter(|_| r.messages));
        let Some(stand_in) = windows.iter().find_map(shows) else {
            return Ok(false);
        };
        let old = self.tree.windows();
        let mut made = vec![];
        for r in windows {
            let used = self.next_tick();
            made.push(self.windows.insert(Window {
                buffer: shows(r).unwrap_or(stand_in),
                cols: 0,
                rows: 0,
                rect: None,
                used,
            }));
        }
        self.tree.set_layout(layout, &made);
        if self.tree.frame.0 == 0 {
            self.tree.frame = (80, 26);
        }
        // Taking a window out works in cells, which a layout's sizes aren't until it's tiled.
        self.tree.tile();
        // Taking a window out through the tree leaves it a tree whatever else went.
        for (r, w) in windows.iter().zip(&made) {
            if shows(r).is_none() {
                self.tree.remove(*w);
                self.drop_window(*w);
            }
        }
        for w in old {
            self.drop_window(w);
        }
        for (buf, list) in views {
            let shown: Vec<_> = made.iter().map(|w| self.window_buffer(*w)).collect();
            let list: Vec<_> = list.into_iter().map(|v| self.at_rest(buf, v)).collect();
            let Ok(t) = self.text_mut(buf) else { continue };
            for v in list {
                match shown.get(v.window) {
                    Some(Some(b)) if *b == buf => {
                        let view = v.view(t);
                        t.views.insert(made[v.window], view);
                    }
                    Some(Some(_)) => {
                        let view = v.view(t);
                        t.parked.insert(made[v.window], view);
                    }
                    _ => {}
                }
            }
            t.normalize_views();
        }
        let alive: Vec<WindowId> = made
            .iter()
            .copied()
            .filter(|w| self.windows.contains_key(*w))
            .collect();
        for &w in &alive {
            let buf = self.windows[w].buffer;
            if let Ok(t) = self.text_mut(buf)
                && !t.views.contains_key(&w)
            {
                t.unpark_view(w, None, 0, 0);
            }
        }
        self.retile();
        for &w in &alive {
            let buf = self.windows[w].buffer;
            if let Ok(t) = self.text_mut(buf)
                && let Some(v) = t.views.get(&w).copied()
            {
                t.set_scroll(w, v.top, v.top_row);
            }
        }
        let wanted = focused.and_then(|f| made.get(f)).copied();
        let to_focus = wanted
            .filter(|w| alive.contains(w))
            .or(alive.last().copied());
        if let Some(w) = to_focus {
            self.focus(w);
        }
        // Focusing marks things used. The file says in what order they were.
        for w in alive {
            self.windows[w].used = self.next_tick();
        }
        for id in ids.iter().flatten() {
            let used = self.next_tick();
            if let Some(b) = self.buffers.get_mut(*id) {
                b.used = used;
            }
        }
        Ok(true)
    }

    /// A saved view, with its positions where the buffer's state lets a cursor rest.
    fn at_rest(&mut self, buf: BufferId, v: ViewRecord) -> ViewRecord {
        let Ok(t) = self.text(buf) else { return v };
        let (anchor, head) = (fit(t, v.anchor), fit(t, v.head));
        ViewRecord {
            anchor: self.rested(buf, anchor),
            head: self.rested(buf, head),
            ..v
        }
    }

    /// One buffer. None if there's nothing to bring back, which isn't a failure. `before` is what
    /// was there before this restore began.
    fn restore_buffer(
        &mut self,
        record: BufferRecord,
        before: &[BufferId],
    ) -> std::result::Result<Option<BufferId>, String> {
        let name = record.name.as_str();
        let id = match (&record.file, &record.text) {
            (Some(f), None) => {
                let path = f.path.path();
                if f.fingerprint.is_some() && !path.exists() {
                    self.messages
                        .push(format!("session: {name} is gone from disk"));
                    return Ok(None);
                }
                self.open_file(&path).map_err(|e| e.to_string())?
            }
            (Some(f), Some(text)) => self.restore_file(name, f, text)?,
            (None, text) => {
                // An extension's own buffer, made again at init, takes the saved text and stays
                // the buffer the extension holds.
                let own = self.find_buffer(name).filter(|b| {
                    before.contains(b) && self.file(*b).is_none() && self.text(*b).is_ok()
                });
                match (own, text) {
                    (Some(b), None) => b,
                    (Some(b), Some(text)) => {
                        let t = self.text_mut(b).map_err(|e| e.to_string())?;
                        let all = 0..t.len();
                        let tx = t.tx(Origin::Extension).silent();
                        let replaced = tx.replace(all, text.as_str()).commit();
                        replaced.map_err(|e| e.to_string())?;
                        b
                    }
                    (None, text) => self.create_text(name, text.as_deref().unwrap_or("")),
                }
            }
        };
        if let Some(dir) = &record.directory {
            self.set_directory(id, dir.path());
        }
        if record.visual {
            self.set_state_of(id, State::Visual);
        }
        // A file that can't be edited now and could when it was saved stays read-only.
        let was_editable = record.file.as_ref().is_none_or(|f| f.uneditable.is_none());
        let forced = was_editable && self.file(id).is_some_and(|f| f.read_only.is_some());
        let t = self.text(id).map_err(|e| e.to_string())?;
        let (cursor, top) = (fit(t, record.cursor), fit(t, record.top));
        let cursor = self.rested(id, cursor);
        let t = self.text_mut(id).map_err(|e| e.to_string())?;
        if record.text.is_some() {
            t.set_modified(record.modified);
        }
        if let Some(read_only) = record.read_only {
            t.settings_mut().read_only = read_only || forced;
        }
        t.rest = Some(View {
            sel: Selection::cursor(cursor),
            top,
            top_row: record.top_row,
            cols: 0,
            rows: 0,
        });
        t.normalize_views();
        Ok(Some(id))
    }

    /// A file's buffer with text the file doesn't have.
    fn restore_file(
        &mut self,
        name: &str,
        f: &FileRecord,
        text: &str,
    ) -> std::result::Result<BufferId, String> {
        let path = f.path.path();
        if self.find_file_buffer(&path).is_some() {
            // Two buffers can't visit one file. The text still mustn't go.
            let line = format!(
                "session: {} is already open, so {name} keeps its text without the file",
                path.display()
            );
            self.messages.push(line);
            return Ok(self.create_text(name, text));
        }
        let fingerprint = f
            .fingerprint
            .as_ref()
            .and_then(FingerprintRecord::fingerprint);
        let state = FileState {
            fingerprint,
            bom: f.bom,
            read_only: f.uneditable,
            read_only_before: f.read_only_before,
        };
        self.attach_file(path, text, state)
            .map_err(|e| e.to_string())
    }

    /// Put a restored buffer on screen, where it was.
    fn show_restored(&mut self, buf: BufferId) {
        let shown = if self.focused().is_some() {
            self.show(buf, ShowIn::Current).ok().flatten()
        } else {
            let (cols, rows) = match self.frame_size() {
                (0, _) => (80, 24),
                (cols, rows) => (cols, rows.saturating_sub(2)),
            };
            self.open_window(buf, cols, rows).ok()
        };
        let Some(w) = shown else { return };
        if let Ok(t) = self.text_mut(buf)
            && let Some(v) = t.rest
        {
            t.set_cursor(w, v.sel.head);
            t.set_scroll(w, v.top, v.top_row);
        }
    }

    /// At startup: restore what's in `dir` and keep it from now on, once resumed. The previous
    /// copy stands in for a file that's missing or can't be read.
    pub(crate) fn attach_session(&mut self, dir: PathBuf) {
        self.load_session(&dir);
        let (jobs, job_rx) = channel();
        let (outcome_tx, outcomes) = channel();
        let named = thread::Builder::new().name("die-session".into());
        if let Err(e) = named.spawn(move || writer(dir, job_rx, outcome_tx)) {
            self.messages
                .push(format!("session: won't be saved, with no thread: {e}"));
            return;
        }
        let look = self.every(LOOK_EVERY, |ctx| {
            ctx.look_at_session();
            Ok(())
        });
        self.session = Some(Session {
            jobs,
            outcomes,
            writing: false,
            seen: None,
            written: None,
            unwritten: None,
            failing: false,
            _look: look,
        });
    }

    fn load_session(&mut self, dir: &Path) {
        for name in [FILE, PREVIOUS] {
            let path = dir.join(name);
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => {
                    self.messages.push(format!("session: {name}: {e}"));
                    continue;
                }
            };
            match parse(&bytes) {
                Ok(parsed) => {
                    if self.restore_parsed(parsed) > 0 {
                        self.say_rejected(&path);
                    }
                    return;
                }
                Err(e) => {
                    self.messages
                        .push(format!("session: {name} can't be read: {e}"));
                    self.say_rejected(&path);
                }
            }
        }
    }

    fn say_rejected(&mut self, path: &Path) {
        if let Some(to) = keep_rejected(path) {
            self.messages
                .push(format!("session: what was rejected is in {}", to.display()));
        }
    }

    fn look_at_session(&mut self) {
        self.note_session_outcomes();
        if !self.session.as_ref().is_some_and(|s| s.writing) {
            return;
        }
        let now = self.now();
        let snapshot = self.snapshot();
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let since = match &s.seen {
            Some((seen, since)) if *seen == snapshot => *since,
            _ => {
                s.seen = Some((snapshot.clone(), now));
                now
            }
        };
        if s.written.as_ref() == Some(&snapshot) {
            s.unwritten = None;
            return;
        }
        let unwritten = *s.unwritten.get_or_insert(now);
        if now.saturating_sub(since) < QUIET && now.saturating_sub(unwritten) < MOST_BEHIND {
            return;
        }
        s.unwritten = None;
        s.written = Some(snapshot.clone());
        if s.jobs.send(Job::Write(snapshot)).is_err() {
            s.written = None;
            self.messages.push("session: the writer has stopped".into());
        }
    }

    /// Say a failed write once, until one works, and try again at the next look.
    fn note_session_outcomes(&mut self) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let mut line = None;
        while let Ok(outcome) = s.outcomes.try_recv() {
            match outcome {
                Ok(()) => s.failing = false,
                Err(e) => {
                    s.written = None;
                    if !std::mem::replace(&mut s.failing, true) {
                        line = Some(format!("session: not saved: {e}"));
                    }
                }
            }
        }
        self.messages.extend(line);
    }

    /// The lock is held: writes may start. The first comes after the quiet time, whether or not
    /// anything changed. Without a session this does nothing.
    pub fn resume_session(&mut self) {
        if let Some(s) = &mut self.session {
            s.writing = true;
            s.seen = None;
            s.written = None;
        }
    }

    /// Write the session now, and wait until it's on disk. Without a session this does nothing.
    pub fn save_session(&mut self) -> Result {
        let snapshot = self.snapshot();
        let Some(s) = &mut self.session else {
            return Ok(());
        };
        let (done, wait) = channel();
        let failed = |e: String| Error::Msg(format!("session: not saved: {e}"));
        let sent = s.jobs.send(Job::WriteNow(snapshot.clone(), done));
        sent.map_err(|_| failed("the writer has stopped".into()))?;
        wait.recv()
            .map_err(|_| failed("the writer has stopped".into()))?
            .map_err(failed)?;
        s.written = Some(snapshot);
        Ok(())
    }

    /// A restart is starting: write the session, and write no more until
    /// [`Ctx::resume_session`] says the restart failed. If the write fails, nothing is paused and
    /// the restart shouldn't go on.
    pub fn hand_over_session(&mut self) -> Result {
        self.save_session()?;
        if let Some(s) = &mut self.session {
            s.writing = false;
        }
        Ok(())
    }

    /// Wait until the writer's thread has done all it was given, and report what failed.
    pub fn flush_session(&mut self) {
        if let Some(s) = &self.session {
            let (done, wait) = channel();
            if s.jobs.send(Job::Flush(done)).is_ok() {
                let _ = wait.recv();
            }
        }
        self.note_session_outcomes();
    }

    /// At quit: the session as it stands, unless this instance isn't the one writing.
    pub(crate) fn save_session_at_quit(&mut self) {
        if !self.session.as_ref().is_some_and(|s| s.writing) {
            return;
        }
        if let Err(e) = self.save_session() {
            self.messages.push(e.to_string());
        }
    }
}
