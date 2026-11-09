//! Sessions for tests, written by hand in the shapes orchid 0.2.2's `src/model.rs` defines, never
//! copied from real data, and only under a root a test made ([`TempRoot`]). With
//! [`crate::Orchid::for_tests`] and a [`crate::cli::FakeRunner`], a test never reaches your
//! `~/.config/orchid` nor runs your orchid.
//!
//! A chat is driven as orchid drives it: append lines to `events.jsonl` ([`append`]) and rewrite
//! `metadata.json` ([`Meta::write`], by a rename as orchid does), then move the harness's clock.

use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// When every fixture event happened unless it says otherwise: 11:16:46 in [`crate::test_zone`].
pub const AT: &str = "2026-07-22T01:16:46Z";
/// The marginal input every fixture event carries unless it says otherwise: `[ 63k]`.
pub const MARGINAL: u64 = 63_000;

/// A directory of a test's own under the temp directory, empty, and removed when it drops.
pub struct TempRoot(PathBuf);

impl TempRoot {
    /// A new one, named for `name`, the process and a count, so tests running at once don't share.
    pub fn new(name: &str) -> TempRoot {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("orchid-test-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temporary root");
        TempRoot(dir.canonicalize().expect("a temporary root"))
    }

    /// Where it is.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A session's `metadata.json`, with what a test sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    /// Its id.
    pub id: String,
    /// Its label.
    pub label: Option<String>,
    /// Its working directory.
    pub working_dir: Option<String>,
    /// Its agent.
    pub agent_name: Option<String>,
    /// `idle`, `running`, `failed`, `cancelled` or `terminated`.
    pub status: String,
    /// Its worker, while running.
    pub pid: Option<u32>,
    /// When it was made.
    pub created_at: String,
    /// When it last changed.
    pub updated_at: String,
    /// orchid's estimate, before a provider reported.
    pub token_estimate: u64,
    /// `token_usage.marginal_input`.
    pub marginal_input: u64,
    /// `token_usage.cumulative_input`.
    pub cumulative_input: u64,
    /// Why its last turn ended badly.
    pub termination_reason: Option<String>,
}

impl Meta {
    /// An idle session of the default agent, labelled `default-die` and working in `/tmp/die`,
    /// made and changed at [`AT`].
    pub fn new(id: &str) -> Meta {
        Meta {
            id: id.into(),
            label: Some("default-die".into()),
            working_dir: Some("/tmp/die".into()),
            agent_name: Some("default".into()),
            status: "idle".into(),
            pid: None,
            created_at: AT.into(),
            updated_at: AT.into(),
            token_estimate: 512,
            marginal_input: MARGINAL,
            cumulative_input: 4_200_000,
            termination_reason: None,
        }
    }

    /// The file's text, pretty as orchid writes it.
    pub fn json(&self) -> String {
        let v = json!({
            "id": self.id,
            "label": self.label,
            "working_dir": self.working_dir,
            "created_at": self.created_at,
            "agent_name": self.agent_name,
            "agent": {"prompt": "default", "connection": "echo", "tools": ["bash", "fs_read", "fs_edit"], "max_tokens": 120000},
            "status": self.status,
            "pid": self.pid,
            "updated_at": self.updated_at,
            "token_estimate": self.token_estimate,
            "token_usage": {
                "marginal_input": self.marginal_input,
                "cumulative_input": self.cumulative_input,
                "cumulative_output": 900,
                "cumulative_cached_input": 0,
                "requests": 3,
                "method": "local_tokenizer"
            },
            "termination_reason": self.termination_reason,
        });
        serde_json::to_string_pretty(&v).expect("JSON")
    }

    /// Write it under `root`, making the session's directory, as orchid writes it: to
    /// `metadata.json.tmp`, then renamed, so a reader never sees half of it.
    pub fn write(&self, root: &Path) {
        let dir = crate::store::session_dir(root, &self.id);
        std::fs::create_dir_all(&dir).expect("the session's directory");
        let tmp = dir.join("metadata.json.tmp");
        std::fs::write(&tmp, self.json()).expect("metadata.json.tmp");
        std::fs::rename(&tmp, dir.join("metadata.json")).expect("metadata.json");
    }
}

/// Append `lines` to a session's `events.jsonl`, each with its newline, as orchid appends.
pub fn append(root: &Path, id: &str, lines: &[String]) {
    let path = crate::store::events_path(root, id);
    std::fs::create_dir_all(path.parent().expect("a directory")).expect("the session's directory");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("events.jsonl");
    for l in lines {
        writeln!(f, "{l}").expect("a line");
    }
}

/// One `events.jsonl` line: `fields` (`type` and its kind's) with what orchid adds to every event.
pub fn line(event_id: &str, timestamp: &str, marginal_input: u64, fields: Value) -> String {
    let mut v = fields;
    let common = json!({
        "event_id": event_id,
        "timestamp": timestamp,
        "connection": "echo",
        "token_usage": {
            "marginal_input": marginal_input,
            "cumulative_input": 4_200_000,
            "cumulative_output": 900,
            "cumulative_cached_input": 0,
            "requests": 3,
            "method": "local_tokenizer"
        },
    });
    if let (Value::Object(v), Value::Object(c)) = (&mut v, common) {
        v.extend(c);
    }
    v.to_string()
}

/// A message, `user` or `assistant`.
pub fn message(event_id: &str, role: &str, content: &str) -> String {
    line(
        event_id,
        AT,
        MARGINAL,
        json!({"type": "message", "role": role, "content": content}),
    )
}

/// One tool call, as orchid writes one per event.
pub fn tool_call(event_id: &str, call_id: &str, name: &str, input: Value) -> String {
    let calls = json!([{"call_id": call_id, "name": name, "input": input}]);
    line(
        event_id,
        AT,
        MARGINAL,
        json!({"type": "tool_call", "calls": calls}),
    )
}

/// A tool's result.
pub fn tool_result(event_id: &str, call_id: &str, content: Value) -> String {
    line(
        event_id,
        AT,
        MARGINAL,
        json!({"type": "tool_result", "call_id": call_id, "content": content}),
    )
}

/// A failure.
pub fn failure(event_id: &str, message: &str) -> String {
    line(
        event_id,
        AT,
        MARGINAL,
        json!({"type": "failure", "message": message}),
    )
}

/// A termination.
pub fn termination(event_id: &str, reason: &str) -> String {
    line(
        event_id,
        AT,
        MARGINAL,
        json!({"type": "termination", "reason": reason}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{self, Event, EventKind, Status};

    #[test]
    fn fixtures_read_back_as_orchid_wrote_them() {
        let root = TempRoot::new("fixtures");
        let mut m = Meta::new("s1");
        m.status = "running".into();
        m.write(root.path());
        let read = store::read_metadata(root.path(), "s1").unwrap();
        assert_eq!(
            (read.status, read.token_usage.cumulative_input),
            (Status::Running, 4_200_000)
        );
        append(
            root.path(),
            "s1",
            &[message("e1", "user", "hi"), failure("e2", "no")],
        );
        append(root.path(), "s1", &[termination("e3", "cap")]);
        let text = std::fs::read_to_string(store::events_path(root.path(), "s1")).unwrap();
        let events: Vec<Event> = text.lines().filter_map(Event::parse).collect();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].marginal_input(), Some(MARGINAL));
        assert_eq!(
            events[1].kind,
            EventKind::Failure {
                message: "no".into()
            }
        );
        let path = root.path().to_path_buf();
        drop(root);
        assert!(!path.exists());
    }
}
