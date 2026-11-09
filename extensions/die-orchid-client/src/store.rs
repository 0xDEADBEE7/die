//! orchid 0.2.2's store as a client reads it (orchid's `src/model.rs` and `src/store.rs`): a
//! session is `<root>/sessions/<id>/`, with `metadata.json`, rewritten whole by a rename, and
//! `events.jsonl`, appended a line at a time. Nothing here writes, and nothing takes orchid's
//! lock: readers never do.
//!
//! - [`Metadata`] is a session's `metadata.json`, and [`scan`] reads every session's, skipping
//!   what it can't read, as a browser must when a session is half made or half archived.
//! - [`Event`] is one line of `events.jsonl`. Types orchid 0.2 doesn't write are [`EventKind::Other`],
//!   with the line itself.
//! - [`Tail`] follows `events.jsonl` by byte offset, a whole line at a time, and [`restore`] reads
//!   the end of it for a chat's history.

use jiff::Timestamp;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ paths

/// `<root>/sessions`, where every live session's directory is.
pub fn sessions_dir(root: &Path) -> PathBuf {
    root.join("sessions")
}

/// A session's directory.
pub fn session_dir(root: &Path, id: &str) -> PathBuf {
    sessions_dir(root).join(id)
}

/// A session's `metadata.json`.
pub fn metadata_path(root: &Path, id: &str) -> PathBuf {
    session_dir(root, id).join("metadata.json")
}

/// A session's `events.jsonl`, its transcript.
pub fn events_path(root: &Path, id: &str) -> PathBuf {
    session_dir(root, id).join("events.jsonl")
}

/// A session's `logs.jsonl`, orchid's own log of it (`/logs` opens it).
pub fn logs_path(root: &Path, id: &str) -> PathBuf {
    session_dir(root, id).join("logs.jsonl")
}

// ------------------------------------------------------------------ metadata

/// Where a session is (orchid's `Status`). orchid read its old `hookrunning` as idle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Waiting for a message.
    #[default]
    #[serde(alias = "hookrunning")]
    Idle,
    /// A worker is taking a turn.
    Running,
    /// The provider or the worker failed.
    Failed,
    /// `orchid stop` ended the turn.
    Cancelled,
    /// The token cap or a hook ended it.
    Terminated,
}

impl Status {
    /// Its name in `metadata.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Running => "running",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
            Status::Terminated => "terminated",
        }
    }
    /// A worker is taking a turn.
    pub fn is_running(self) -> bool {
        self == Status::Running
    }
}

/// The token counts orchid keeps on a session, and copies onto each event as it's appended.
/// The aliases are the names before 0.2.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct TokenUsage {
    /// What the next request would send: the stubs' `[NNNk]`.
    #[serde(alias = "context_estimate")]
    pub marginal_input: u64,
    /// Every request's input so far: the indicator's and the restored line's `[NNNNNk]`.
    #[serde(alias = "input_total")]
    pub cumulative_input: u64,
    /// Every reply's output so far.
    #[serde(alias = "output_total")]
    pub cumulative_output: u64,
    /// Input the provider had cached.
    #[serde(alias = "cached_input_total")]
    pub cumulative_cached_input: u64,
    /// How many requests.
    pub requests: u64,
    /// `provider_reported` or `local_tokenizer`.
    pub method: String,
}

/// A session's `metadata.json`, the fields a client reads. The session's copy of its agent isn't
/// among them: its name is `agent_name`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Metadata {
    /// Its id, a UUID.
    pub id: String,
    /// What `--label` or `set --label` named it.
    #[serde(default)]
    pub label: Option<String>,
    /// Where its tools run, canonical.
    #[serde(default)]
    pub working_dir: Option<String>,
    /// When it was made.
    #[serde(default, deserialize_with = "timestamp")]
    pub created_at: Option<Timestamp>,
    /// When it last changed: the browser's UPDATED and its order.
    #[serde(default, deserialize_with = "timestamp")]
    pub updated_at: Option<Timestamp>,
    /// The session that made it, if any.
    #[serde(default)]
    pub parent: Option<String>,
    /// The agent it was made from. Sessions from before 0.2 have none.
    #[serde(default)]
    pub agent_name: Option<String>,
    /// Where it is.
    pub status: Status,
    /// Its worker's process, while one runs.
    #[serde(default)]
    pub pid: Option<u32>,
    /// orchid's estimate of the context, before any provider reported.
    #[serde(default)]
    pub token_estimate: u64,
    /// Its token counts.
    #[serde(default)]
    pub token_usage: TokenUsage,
    /// Why its last turn ended, when it didn't end well.
    #[serde(default)]
    pub termination_reason: Option<String>,
}

/// An RFC 3339 time, or None if it's missing or doesn't parse.
fn timestamp<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Timestamp>, D::Error> {
    let s: Option<String> = Option::deserialize(d)?;
    Ok(s.and_then(|s| s.parse().ok()))
}

impl Metadata {
    /// Read a `metadata.json` from its text. An error says why it isn't one.
    pub fn parse(text: &str) -> Result<Metadata, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
}

/// Read one session's `metadata.json`. A file that isn't one is `InvalidData`.
pub fn read_metadata(root: &Path, id: &str) -> io::Result<Metadata> {
    let text = std::fs::read_to_string(metadata_path(root, id))?;
    Metadata::parse(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Every session under `root` whose `metadata.json` reads, in no order. One that doesn't, or
/// whose id isn't its directory's name, is skipped: orchid writes it by a rename, but a session
/// being made or archived can be caught between steps. No `sessions` directory is no sessions.
pub fn scan(root: &Path) -> Vec<Metadata> {
    let Ok(dirs) = std::fs::read_dir(sessions_dir(root)) else {
        return vec![];
    };
    let read = |entry: std::fs::DirEntry| {
        let name = entry.file_name().into_string().ok()?;
        let meta = read_metadata(root, &name).ok()?;
        (meta.id == name).then_some(meta)
    };
    dirs.filter_map(|e| read(e.ok()?)).collect()
}

// ------------------------------------------------------------------ events

/// One line of `events.jsonl` (orchid's `Event`).
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// What happened.
    pub kind: EventKind,
    /// Its id, which a chat shows each event under once. The Emacs client also took `event-id`
    /// and `uuid`.
    pub event_id: Option<String>,
    /// When it was appended.
    pub timestamp: Option<Timestamp>,
    /// The session's token counts when it was appended, if the line has them.
    pub token_usage: Option<TokenUsage>,
}

/// What an event says, by its `type`.
#[derive(Clone, Debug, PartialEq)]
pub enum EventKind {
    /// A message: `role` is `user` or `assistant`.
    Message {
        /// `user` or `assistant`.
        role: String,
        /// Its text.
        content: String,
    },
    /// The model's tool calls; orchid writes one per event.
    ToolCall {
        /// The calls.
        calls: Vec<ToolCall>,
    },
    /// A tool's result: a string, or an object as each tool makes them.
    ToolResult {
        /// The call it answers.
        call_id: String,
        /// The result.
        content: Value,
    },
    /// The token cap, a hook or `orchid stop` ended the turn.
    Termination {
        /// Why.
        reason: String,
    },
    /// The provider, the setup or the worker failed.
    Failure {
        /// What went wrong.
        message: String,
    },
    /// Any other type, as the `usage` and `reasoning` lines sessions from before 0.2 keep, or a
    /// line with no type: the type, if any, and the line as it was.
    Other {
        /// Its `type`.
        kind: Option<String>,
        /// The line.
        raw: String,
    },
}

/// One tool call (orchid's `ToolCall`). Its `input` is as the model gave it: [`ToolCall::tool`]
/// reads the three tools orchid has.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ToolCall {
    /// orchid's id for it, which its result names. The Emacs client read `id`.
    #[serde(default, alias = "id")]
    pub call_id: String,
    /// `bash`, `fs_read`, `fs_edit`, or another the model asked for.
    #[serde(default)]
    pub name: String,
    /// Its arguments.
    #[serde(default)]
    pub input: Value,
}

/// A tool call's arguments, read for the tools orchid 0.2 has (`src/provider/defs.rs`).
#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    /// `bash {cmd}`.
    Bash {
        /// The command line.
        cmd: Option<String>,
    },
    /// `fs_read {paths}`. A single `path`, as before July 2026, is one path.
    Read {
        /// The files.
        paths: Vec<String>,
    },
    /// `fs_edit {path, edits}`. A single `old_string` and `new_string`, as before, is one edit.
    Edit {
        /// The file.
        path: Option<String>,
        /// Its replacements, in order.
        edits: Vec<Edit>,
    },
    /// Any other tool.
    Other,
}

/// One of `fs_edit`'s replacements.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Edit {
    /// The text replaced.
    pub old_string: Option<String>,
    /// What replaces it.
    pub new_string: Option<String>,
    /// Every occurrence, not just one.
    pub replace_all: bool,
}

impl ToolCall {
    /// Its arguments, read for its tool.
    pub fn tool(&self) -> Tool {
        let s = |k: &str| self.input.get(k).and_then(Value::as_str).map(String::from);
        match self.name.as_str() {
            "bash" => Tool::Bash { cmd: s("cmd") },
            "fs_read" => {
                let listed = self.input.get("paths").and_then(Value::as_array);
                let paths = listed.map(|p| p.iter().filter_map(Value::as_str).map(String::from));
                let paths: Vec<String> =
                    paths.map_or_else(|| s("path").into_iter().collect(), |p| p.collect());
                Tool::Read { paths }
            }
            "fs_edit" => {
                let listed = self.input.get("edits").cloned();
                let edits = listed.and_then(|e| serde_json::from_value(e).ok());
                let single = || Edit {
                    old_string: s("old_string"),
                    new_string: s("new_string"),
                    replace_all: false,
                };
                let edits = edits.unwrap_or_else(|| {
                    let one = single();
                    if one == Edit::default() {
                        vec![]
                    } else {
                        vec![one]
                    }
                });
                Tool::Edit {
                    path: s("path"),
                    edits,
                }
            }
            _ => Tool::Other,
        }
    }
}

impl Event {
    /// Read one line of `events.jsonl`. None if it isn't a JSON object, as the Emacs client
    /// dropped a line it couldn't parse.
    pub fn parse(line: &str) -> Option<Event> {
        let v: Value = serde_json::from_str(line).ok()?;
        let o = v.as_object()?;
        let text = |k: &str| o.get(k).and_then(Value::as_str);
        let owned = |k: &str| text(k).unwrap_or_default().to_string();
        let kind = match text("type") {
            Some("message") => EventKind::Message {
                role: owned("role"),
                content: owned("content"),
            },
            Some("tool_call") => EventKind::ToolCall {
                calls: o
                    .get("calls")
                    .and_then(|c| serde_json::from_value(c.clone()).ok())
                    .unwrap_or_default(),
            },
            Some("tool_result") => EventKind::ToolResult {
                call_id: owned("call_id"),
                content: o.get("content").cloned().unwrap_or(Value::Null),
            },
            Some("termination") => EventKind::Termination {
                reason: owned("reason"),
            },
            Some("failure") => EventKind::Failure {
                message: owned("message"),
            },
            other => EventKind::Other {
                kind: other.map(String::from),
                raw: line.to_string(),
            },
        };
        let id = ["event_id", "event-id", "uuid"]
            .iter()
            .find_map(|k| text(k));
        Some(Event {
            kind,
            event_id: id.map(String::from),
            timestamp: text("timestamp").and_then(|t| t.parse().ok()),
            token_usage: o
                .get("token_usage")
                .and_then(|t| serde_json::from_value(t.clone()).ok()),
        })
    }

    /// The session's marginal input when it was appended: its stub's `[NNNk]`.
    pub fn marginal_input(&self) -> Option<u64> {
        self.token_usage.as_ref().map(|t| t.marginal_input)
    }
}

// ------------------------------------------------------------------ following events.jsonl

/// A follower of an append-only file by byte offset, as the Emacs client's poll was: each
/// [`Tail::read`] gives the whole lines added since the last, and keeps a last line that has no
/// newline yet for the next. If the file shrank, it starts again from the top, and says so: the
/// caller shows each event id once, so the lines it has seen come back harmlessly.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tail {
    /// Bytes read so far, the partial line's too.
    read: u64,
    /// The last line read, still without its newline.
    partial: Vec<u8>,
}

/// What a [`Tail::read`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lines {
    /// The new whole lines, without their newlines, in order. A line that isn't UTF-8 has its
    /// bad bytes replaced.
    pub lines: Vec<String>,
    /// The file had shrunk, so these are from its top.
    pub restarted: bool,
}

impl Tail {
    /// Follow from `offset`: the end of what [`restore`] read, or 0 for the whole file. It must
    /// be at a line's start.
    pub fn at(offset: u64) -> Tail {
        Tail {
            read: offset,
            partial: vec![],
        }
    }

    /// Where the next whole line starts: the offset to follow from again.
    pub fn offset(&self) -> u64 {
        self.read - self.partial.len() as u64
    }

    /// The lines added to `path` since the last read. A missing file is `NotFound`, as after
    /// `orchid delete` archives the session.
    pub fn read(&mut self, path: &Path) -> io::Result<Lines> {
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        let restarted = size < self.read;
        if restarted {
            *self = Tail::default();
        }
        if size == self.read {
            return Ok(Lines {
                lines: vec![],
                restarted,
            });
        }
        file.seek(SeekFrom::Start(self.read))?;
        let mut more = vec![];
        file.take(size - self.read).read_to_end(&mut more)?;
        self.read += more.len() as u64;
        self.partial.extend_from_slice(&more);
        let whole = self
            .partial
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |i| i + 1);
        let rest = self.partial.split_off(whole);
        let done = std::mem::replace(&mut self.partial, rest);
        Ok(Lines {
            lines: split_lines(&done),
            restarted,
        })
    }
}

/// Whole lines, each newline-terminated in `bytes`, as text.
fn split_lines(bytes: &[u8]) -> Vec<String> {
    let mut lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    lines.pop(); // what follows the last newline: nothing
    lines
        .into_iter()
        .map(|l| String::from_utf8_lossy(l).into_owned())
        .collect()
}

// ------------------------------------------------------------------ restoring a chat

/// What [`restore`] read: a chat's history, and where to follow from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Restored {
    /// The events, oldest first.
    pub events: Vec<Event>,
    /// The ids among them, for the follower's own check that it shows each event once.
    pub seen: HashSet<String>,
    /// Where the last whole line read ends: [`Tail::at`] this, so nothing appended between the
    /// restore and the follow is lost, nor shown twice (the Emacs client's B9).
    pub end: u64,
}

/// A chat's history, as the Emacs client restored it: the last `max_bytes` of `events.jsonl`
/// (dropping the partial line it starts in), read from the end back, keeping up to `max_events`
/// events that `shown` says show, each id once, then put oldest first. A last line still being
/// written isn't read. A missing file is `NotFound`.
pub fn restore(
    path: &Path,
    max_bytes: u64,
    max_events: usize,
    shown: impl Fn(&Event) -> bool,
) -> io::Result<Restored> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let start = size.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![];
    file.by_ref().take(size - start).read_to_end(&mut bytes)?;
    let mut from = 0;
    if start > 0 {
        // The line the window starts in is partial, unless the byte before it ended one.
        let mut before = [0u8];
        file.seek(SeekFrom::Start(start - 1))?;
        file.read_exact(&mut before)?;
        if before[0] != b'\n' {
            from = bytes
                .iter()
                .position(|b| *b == b'\n')
                .map_or(bytes.len(), |i| i + 1);
        }
    }
    let whole = bytes
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(from, |i| i + 1)
        .max(from);
    let mut restored = Restored {
        end: start + whole as u64,
        ..Restored::default()
    };
    for line in split_lines(&bytes[from..whole]).iter().rev() {
        if restored.events.len() >= max_events {
            break;
        }
        let Some(event) = Event::parse(line) else {
            continue;
        };
        let seen = event
            .event_id
            .as_ref()
            .is_some_and(|id| restored.seen.contains(id));
        if seen || !shown(&event) {
            continue;
        }
        restored.seen.extend(event.event_id.clone());
        restored.events.push(event);
    }
    restored.events.reverse();
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("orchid-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A `metadata.json` in orchid 0.2.2's shape (`model.rs`'s `Metadata`), written by hand.
    fn metadata(id: &str, status: &str, updated: &str) -> String {
        format!(
            r#"{{
  "id": "{id}",
  "label": "default-die",
  "working_dir": "/tmp/die",
  "created_at": "2026-11-01T09:00:00.000000Z",
  "agent_name": "default",
  "agent": {{"prompt": "default", "connection": "echo", "tools": [], "max_tokens": 120000}},
  "status": "{status}",
  "pid": null,
  "updated_at": "{updated}",
  "token_estimate": 512,
  "token_usage": {{"marginal_input": 63000, "cumulative_input": 4200000, "cumulative_output": 900, "cumulative_cached_input": 0, "requests": 3, "method": "local_tokenizer"}},
  "termination_reason": null
}}"#
        )
    }

    fn line(id: &str, kind: &str) -> String {
        format!(
            r#"{{"type":"message","role":"{kind}","content":"hi {id}","event_id":"{id}","timestamp":"2026-11-01T09:00:00Z","connection":"echo","token_usage":{{"marginal_input":1000,"cumulative_input":0,"cumulative_output":0,"cumulative_cached_input":0,"requests":0,"method":"local_tokenizer"}}}}"#
        )
    }

    #[test]
    fn metadata_reads_orchid_0_2s_shape() {
        let m = Metadata::parse(&metadata("s1", "running", "2026-11-01T09:05:00.5Z")).unwrap();
        assert_eq!(m.id, "s1");
        assert_eq!(m.status, Status::Running);
        assert_eq!(m.agent_name.as_deref(), Some("default"));
        assert_eq!(m.working_dir.as_deref(), Some("/tmp/die"));
        assert_eq!(m.token_usage.cumulative_input, 4_200_000);
        assert_eq!(m.token_usage.marginal_input, 63_000);
        assert_eq!(
            m.updated_at.unwrap(),
            "2026-11-01T09:05:00.5Z".parse::<Timestamp>().unwrap()
        );
        // Every status, and the old `hookrunning`.
        for (s, want) in [
            ("idle", Status::Idle),
            ("failed", Status::Failed),
            ("cancelled", Status::Cancelled),
            ("terminated", Status::Terminated),
            ("hookrunning", Status::Idle),
        ] {
            let m = Metadata::parse(&metadata("s", s, "2026-11-01T09:05:00Z")).unwrap();
            assert_eq!(m.status, want);
            assert!(!m.status.is_running());
        }
        assert!(Metadata::parse(r#"{"id":"s","status":"weird"}"#).is_err());
        assert!(Metadata::parse(r#"{"id":"s""#).is_err(), "a partial file");
    }

    #[test]
    fn scan_skips_what_it_cant_read() {
        let root = dir("scan");
        for (id, text) in [
            ("a", metadata("a", "idle", "2026-11-01T09:00:00Z")),
            ("b", metadata("b", "running", "2026-11-01T10:00:00Z")),
            ("partial", r#"{"id":"partial","sta"#.to_string()),
            (
                "other",
                metadata("not-other", "idle", "2026-11-01T09:00:00Z"),
            ),
        ] {
            std::fs::create_dir_all(session_dir(&root, id)).unwrap();
            std::fs::write(metadata_path(&root, id), text).unwrap();
        }
        std::fs::create_dir_all(session_dir(&root, "empty")).unwrap();
        let mut ids: Vec<String> = scan(&root).into_iter().map(|m| m.id).collect();
        ids.sort();
        assert_eq!(ids, ["a", "b"]);
        assert!(scan(&root.join("nowhere")).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn events_read_every_kind_orchid_writes() {
        let call = r#"{"type":"tool_call","calls":[{"call_id":"c1","name":"fs_edit","input":{"path":"src/a.rs","edits":[{"old_string":"a","new_string":"b"},{"old_string":"c","new_string":"d","replace_all":true}]}}],"event_id":"e2","timestamp":"2026-11-01T09:00:01Z","connection":null,"token_usage":{"marginal_input":2000}}"#;
        let e = Event::parse(call).unwrap();
        let EventKind::ToolCall { calls } = &e.kind else {
            panic!("{e:?}")
        };
        assert_eq!(calls[0].call_id, "c1");
        assert_eq!(
            calls[0].tool(),
            Tool::Edit {
                path: Some("src/a.rs".into()),
                edits: vec![
                    Edit {
                        old_string: Some("a".into()),
                        new_string: Some("b".into()),
                        replace_all: false
                    },
                    Edit {
                        old_string: Some("c".into()),
                        new_string: Some("d".into()),
                        replace_all: true
                    },
                ],
            }
        );
        assert_eq!(e.marginal_input(), Some(2000));
        assert_eq!(e.event_id.as_deref(), Some("e2"));
        let read = r#"{"type":"tool_call","calls":[{"call_id":"c2","name":"fs_read","input":{"paths":["a","b"]}}],"event_id":"e3","timestamp":"2026-11-01T09:00:02Z"}"#;
        let EventKind::ToolCall { calls } = Event::parse(read).unwrap().kind else {
            panic!()
        };
        assert_eq!(
            calls[0].tool(),
            Tool::Read {
                paths: vec!["a".into(), "b".into()]
            }
        );
        let result = r#"{"type":"tool_result","call_id":"c2","content":{"a":"text","b":{"error":"Is a directory"}},"event_id":"e4","timestamp":"2026-11-01T09:00:03Z"}"#;
        let EventKind::ToolResult { call_id, content } = Event::parse(result).unwrap().kind else {
            panic!()
        };
        assert_eq!(call_id, "c2");
        assert_eq!(content["b"]["error"], "Is a directory");
        let end = r#"{"type":"termination","reason":"token cap reached","event_id":"e5","timestamp":"2026-11-01T09:00:04Z"}"#;
        assert_eq!(
            Event::parse(end).unwrap().kind,
            EventKind::Termination {
                reason: "token cap reached".into()
            }
        );
        let failed = r#"{"type":"failure","message":"no connection","event_id":"e6","timestamp":"2026-11-01T09:00:05Z"}"#;
        assert_eq!(
            Event::parse(failed).unwrap().kind,
            EventKind::Failure {
                message: "no connection".into()
            }
        );
        let old =
            r#"{"type":"usage","input":5,"event_id":"e7","timestamp":"2026-11-01T09:00:06Z"}"#;
        assert_eq!(
            Event::parse(old).unwrap().kind,
            EventKind::Other {
                kind: Some("usage".into()),
                raw: old.into()
            }
        );
        let untyped = r#"{"content":"no type here"}"#;
        assert_eq!(
            Event::parse(untyped).unwrap().kind,
            EventKind::Other {
                kind: None,
                raw: untyped.into()
            }
        );
        assert_eq!(Event::parse("not json at all"), None);
        assert_eq!(Event::parse("[1,2]"), None);
        // The Emacs client's other names for the id.
        let uuid = Event::parse(r#"{"type":"message","role":"user","content":"x","uuid":"u1"}"#);
        assert_eq!(uuid.unwrap().event_id.as_deref(), Some("u1"));
    }

    #[test]
    fn tools_from_before_0_2_read_as_one_path_or_one_edit() {
        let call = |name: &str, input: Value| ToolCall {
            call_id: "c".into(),
            name: name.into(),
            input,
        };
        let read = call("fs_read", serde_json::json!({"path": "/short/path"}));
        assert_eq!(
            read.tool(),
            Tool::Read {
                paths: vec!["/short/path".into()]
            }
        );
        let edit = call(
            "fs_edit",
            serde_json::json!({"path": "p", "old_string": "a", "new_string": "b"}),
        );
        let Tool::Edit { edits, .. } = edit.tool() else {
            panic!()
        };
        assert_eq!(edits.len(), 1);
        assert_eq!(
            call("bash", serde_json::json!({"cmd": "ls"})).tool(),
            Tool::Bash {
                cmd: Some("ls".into())
            }
        );
        assert_eq!(call("example_tool", Value::Null).tool(), Tool::Other);
    }

    #[test]
    fn the_tail_gives_whole_lines_keeps_a_partial_one_and_starts_again_when_the_file_shrinks() {
        let d = dir("tail");
        let path = d.join("events.jsonl");
        let mut f = File::create(&path).unwrap();
        let mut tail = Tail::default();
        assert_eq!(tail.read(&path).unwrap(), Lines::default());
        f.write_all(b"one\ntw").unwrap();
        let got = tail.read(&path).unwrap();
        assert_eq!(got.lines, ["one"]);
        assert_eq!(
            tail.offset(),
            4,
            "the next whole line starts after one's newline"
        );
        assert_eq!(tail.read(&path).unwrap().lines, Vec::<String>::new());
        f.write_all(b"o\nthree\n").unwrap();
        assert_eq!(tail.read(&path).unwrap().lines, ["two", "three"]);
        assert_eq!(tail.offset(), 14);
        // Rewritten shorter: from the top again.
        std::fs::write(&path, "new\n").unwrap();
        let got = tail.read(&path).unwrap();
        assert!(got.restarted);
        assert_eq!(got.lines, ["new"]);
        // Followed from an offset, as after a restore.
        std::fs::write(&path, "a\nb\nc\n").unwrap();
        assert_eq!(Tail::at(2).read(&path).unwrap().lines, ["b", "c"]);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            tail.read(&path).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn restore_takes_the_newest_unique_events_oldest_first() {
        let d = dir("restore");
        let path = d.join("events.jsonl");
        let mut text = String::new();
        for i in 0..10 {
            text += &line(&format!("e{i}"), "assistant");
            text.push('\n');
        }
        text += &line("e9", "assistant"); // a duplicate id
        text += "\nnot json\n";
        text += &line("e10", "assistant")[..20]; // still being written
        std::fs::write(&path, &text).unwrap();
        let ids = |r: &Restored| -> Vec<String> {
            r.events
                .iter()
                .map(|e| e.event_id.clone().unwrap())
                .collect()
        };
        let all = |_: &Event| true;
        let r = restore(&path, 5 << 20, 3, all).unwrap();
        assert_eq!(ids(&r), ["e7", "e8", "e9"]);
        assert_eq!(
            r.seen,
            HashSet::from(["e7".into(), "e8".into(), "e9".into()])
        );
        assert_eq!(r.end as usize, text.len() - 20, "up to the partial line");
        // Only what shows counts.
        let odd = |e: &Event| {
            e.event_id
                .as_deref()
                .is_some_and(|id| id.ends_with(['1', '3', '5']))
        };
        assert_eq!(
            ids(&restore(&path, 5 << 20, 250, odd).unwrap()),
            ["e1", "e3", "e5"]
        );
        // The last bytes only, without the line they start in: here e8's.
        let one = line("e0", "assistant").len() + 1;
        let r = restore(&path, (text.len() - 8 * one - 4) as u64, 250, all).unwrap();
        assert_eq!(ids(&r), ["e9"]);
        // Exactly at a line's start, that line is whole.
        let r = restore(&path, (text.len() - 8 * one) as u64, 250, all).unwrap();
        assert_eq!(ids(&r), ["e8", "e9"]);
        assert_eq!(
            restore(&d.join("gone"), 1, 1, all).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        std::fs::remove_dir_all(&d).unwrap();
    }
}
