//! Events as a chat shows them, laid out as the Emacs client's parsers laid them out
//! (`parsers/orchid-parser-*.el`), for orchid 0.2.2's events.
//!
//! An event shows as nothing, as raw text, or as collapsible sections ([`shown`]): a stub line,
//! 80 columns when it has tokens and a time, `label | [preview, 47 wide] | [NNNk] | [HH:MM:SS]`
//! ([`stub_line`]), over a detail that's hidden until you expand it ([`detail`]). A lazy section's
//! detail is made on its first expand, as Emacs's were. Where the Emacs client was wrong for 0.2
//! (its 1-client-map §7 A7 to A10), this follows 0.2: `call_id`, `fs_edit`'s `edits`,
//! `fs_read`'s `paths`, `failure` and `termination` events, and results that aren't a file map.
//!
//! Times are local, in the zone the caller passes, so tests pass a fixed one. Widths are
//! columns, as DIE draws them ([`width`]).

use crate::store::{Event, EventKind, Metadata, Tool, ToolCall};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde_json::Value;
use std::path::Path;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A chat's lines: the stubs, the separators, the indicator.
pub const WIDTH: usize = 80;
/// The stub's label column, with its brackets and the spaces after.
const LABEL_WIDTH: usize = 10;
/// The stub's preview column, with its brackets.
const PREVIEW_WIDTH: usize = 49;
/// A message's preview before the stub cuts it: longer ones keep 47 characters.
const PREVIEW_CUT: usize = 50;

/// The faces sections and details use, which the extension declares (`crate::FACES`).
pub mod face {
    /// A stub's: assistant replies, tools, results (Emacs's `orchid-collapsible-stub-face`).
    pub const STUB: &str = "orchid-collapsible-stub";
    /// Your own messages' stubs (`orchid-collapsible-user-stub-face`).
    pub const USER_STUB: &str = "orchid-collapsible-user-stub";
    /// A failure's stub, which Emacs showed as raw text.
    pub const FAILURE_STUB: &str = "orchid-collapsible-failure-stub";
    /// `Assistant: ` before a reply's text (`orchid-chat-assistant-face`).
    pub const ASSISTANT: &str = "orchid-chat-assistant";
    /// A diff's removed lines (`orchid-diff-removed-face`).
    pub const DIFF_REMOVED: &str = "orchid-diff-removed";
    /// A diff's added lines (`orchid-diff-added-face`).
    pub const DIFF_ADDED: &str = "orchid-diff-added";
    /// A diff's hunk headers and context (`orchid-diff-context-face`).
    pub const DIFF_CONTEXT: &str = "orchid-diff-context";
}

// ------------------------------------------------------------------ widths, tokens, times

/// The columns a tab counts for, as Emacs's `string-width` counts one (`tab-width`'s default).
/// DIE draws a tab to the next tab stop, which is never further.
const TAB: usize = 8;

/// The columns `s` takes, as DIE's layout draws it, so a stub is never wider than 80: each
/// grapheme cluster as wide as Unicode says and one at least (`⚠️` two, `é` one), a control
/// character as `^M` or `\200` (two or four), and a tab eight, as Emacs's `string-width` counted
/// it, which DIE's tab stops never pass.
pub fn width(s: &str) -> usize {
    s.graphemes(true).map(cluster_width).sum()
}

fn cluster_width(g: &str) -> usize {
    match g.chars().next() {
        Some('\t') => TAB,
        Some(c) if c < ' ' || c == '\x7f' => 2,
        Some(c) if c.is_control() => 4,
        _ => g.width().max(1),
    }
}

/// `s` cut to `cols` columns, with `ellipsis` in the last of them when it's cut, as Emacs's
/// `truncate-string-to-width` with an ellipsis. A cluster that would cross the edge goes.
pub fn truncate_width(s: &str, cols: usize, ellipsis: &str) -> String {
    if width(s) <= cols {
        return s.to_string();
    }
    let room = cols.saturating_sub(width(ellipsis));
    let mut out = String::new();
    let mut used = 0;
    for g in s.graphemes(true) {
        let w = cluster_width(g);
        if used + w > room {
            break;
        }
        used += w;
        out.push_str(g);
    }
    out + ellipsis
}

/// `s` with spaces after it to `cols` columns.
fn pad(s: &str, cols: usize) -> String {
    format!("{s}{}", " ".repeat(cols.saturating_sub(width(s))))
}

/// A path cut to `max` characters from the left, starting with `…` when it's cut.
pub fn truncate_path_left(path: &str, max: usize) -> String {
    let n = path.chars().count();
    if n <= max {
        return path.to_string();
    }
    let keep: String = path.chars().skip(n - (max - 1)).collect();
    format!("…{keep}")
}

/// A marginal input count in 4 columns: `NNNk` below a million, `N.Nm` below ten, else `NNm`.
pub fn format_tokens(tokens: u64) -> String {
    let count = if tokens < 1_000_000 {
        format!("{}k", tokens / 1000)
    } else if tokens < 10_000_000 {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    } else {
        format!("{}m", tokens / 1_000_000)
    };
    format!("{:>4}", truncate_width(&count, 4, ""))
}

/// A cumulative input count in 6 columns, as the indicator and the restored line show it:
/// `NNNNNk` below a hundred million, else `NNNm`.
pub fn format_cumulative(tokens: u64) -> String {
    let count = if tokens < 100_000_000 {
        format!("{}k", tokens / 1000)
    } else {
        format!("{}m", tokens / 1_000_000)
    };
    format!("{:>6}", truncate_width(&count, 6, ""))
}

/// The time of day in `zone`, `HH:MM:SS`.
pub fn local_time(t: Timestamp, zone: &TimeZone) -> String {
    t.to_zoned(zone.clone()).strftime("%H:%M:%S").to_string()
}

/// How long ago, as the browser's UPDATED column says it: `just now`, `N minute(s) ago`,
/// `N hour(s) ago`, `N day(s) ago`, or `N/A` without a time.
pub fn relative_time(t: Option<Timestamp>, now: Timestamp) -> String {
    let Some(t) = t else {
        return "N/A".into();
    };
    let secs = now.duration_since(t).as_secs_f64().floor() as i64;
    let ago = |n: i64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match secs {
        ..60 => "just now".into(),
        60..3600 => ago(secs / 60, "minute"),
        3600..86400 => ago(secs / 3600, "hour"),
        _ => ago(secs / 86400, "day"),
    }
}

/// The last part of a working directory, as the browser's labels and the chat's buffer names and
/// separators use it, or `N/A` without one.
pub fn workspace_name(working_dir: Option<&str>) -> String {
    let Some(dir) = working_dir else {
        return "N/A".into();
    };
    let trimmed = dir.trim_end_matches('/');
    let name = Path::new(trimmed).file_name().map(|n| n.to_string_lossy());
    name.map_or_else(|| trimmed.to_string(), |n| n.into_owned())
}

/// The agent a session was made from, or `default` for one from before 0.2.
pub fn agent_name(meta: &Metadata) -> &str {
    meta.agent_name.as_deref().unwrap_or("default")
}

/// `left`, then spaces, then `right` at the end of 80 columns, with a space at least: the
/// indicator's line and the restored line.
pub fn right_aligned(left: &str, right: &str) -> String {
    let gap = WIDTH.saturating_sub(width(left) + width(right)).max(1);
    format!("{left}{}{right}", " ".repeat(gap))
}

// ------------------------------------------------------------------ stubs

/// A stub line, laid out as the Emacs client's `orchid-parser--format-stub-with-timestamp`:
/// `stub` is `[label] [preview]`. The label is padded to 10 columns, or `[event]` if it's wider
/// or `stub` isn't two bracketed parts. The preview's text goes in 47 columns between its
/// brackets, cut without an ellipsis and with a trailing `...` dropped. Then four spaces and
/// `[NNNk]` and `[HH:MM:SS]`, for those it has.
pub fn stub_line(
    stub: &str,
    time: Option<Timestamp>,
    tokens: Option<u64>,
    zone: &TimeZone,
) -> String {
    let (label, preview) = match split_stub(stub) {
        Some((label, preview)) if width(label) <= LABEL_WIDTH => (label, preview),
        Some((_, preview)) => ("[event]", preview),
        None => ("[event]", ""),
    };
    let preview = match preview.strip_suffix("...]") {
        Some(cut) => format!("{cut}]"),
        None => preview.to_string(),
    };
    let bracketed =
        preview.chars().count() > 1 && preview.starts_with('[') && preview.ends_with(']');
    let content = if bracketed {
        &preview[1..preview.len() - 1]
    } else {
        &preview[..]
    };
    let content = truncate_width(content, PREVIEW_WIDTH - 2, "");
    let mut line = format!(
        "{}[{}]",
        pad(label, LABEL_WIDTH),
        pad(&content, PREVIEW_WIDTH - 2)
    );
    let tokens = tokens.map(format_tokens);
    let time = time.map(|t| local_time(t, zone));
    if tokens.is_some() || time.is_some() {
        line += "    ";
    }
    if let Some(t) = &tokens {
        line += &format!("[{t}]");
    }
    if let Some(t) = &time {
        line += &format!("{}[{t}]", if tokens.is_some() { " " } else { "" });
    }
    line
}

/// `[label]` and `[preview]`, as the Emacs regexp `\`\(\[[^]]+\]\)\s-*\(\[.*\]\)\'` splits them.
fn split_stub(s: &str) -> Option<(&str, &str)> {
    let close = s.strip_prefix('[')?.find(']')?;
    if close == 0 {
        return None;
    }
    let (label, rest) = s.split_at(close + 2);
    let preview = rest.trim_start_matches([' ', '\t', '\n', '\r', '\x0c']);
    let whole = preview.len() >= 2 && preview.starts_with('[') && preview.ends_with(']');
    (whole && !preview.contains('\n')).then_some((label, preview))
}

/// A message's text on one line, cut to 47 characters and `...` past 50, for a stub's preview.
pub fn preview(content: &str) -> String {
    let flat = content.replace('\n', " ");
    if flat.chars().count() > PREVIEW_CUT {
        let cut: String = flat.chars().take(PREVIEW_CUT - 3).collect();
        format!("{cut}...")
    } else {
        flat
    }
}

/// Your message's stub, as a send draws it before orchid has it: `[user] [preview]` with the
/// session's marginal input and the time.
pub fn user_stub(content: &str, time: Timestamp, tokens: Option<u64>, zone: &TimeZone) -> String {
    stub_line(
        &format!("[user] [{}]", preview(content)),
        Some(time),
        tokens,
        zone,
    )
}

// ------------------------------------------------------------------ events

/// How an event shows in a chat.
#[derive(Clone, Debug, PartialEq)]
pub enum Shown {
    /// Not at all: an empty message, a result with nothing in it, your own message as it streams
    /// in (a send drew its stub already), and the `usage` and `reasoning` lines orchid 0.2
    /// ignores.
    Nothing,
    /// Unstyled text, not collapsible, as the Emacs client showed an event it had no parser for.
    Raw(String),
    /// One collapsible section a stub, joined by newlines: one per tool call.
    Sections(Vec<Section>),
}

/// A collapsible section's stub. Every section starts collapsed.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// The stub line, without a newline.
    pub stub: String,
    /// The stub's face.
    pub face: &'static str,
    /// Its detail is made on its first expand, not with the stub, as Emacs's lazy sections were.
    pub lazy: bool,
}

/// A piece of a section's detail.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    /// Text, with a face or none.
    Text(String, Option<&'static str>),
    /// A section inside the detail, collapsed, as each file in a tool result's map is: its stub,
    /// then its own detail.
    Section(Section, Vec<Piece>),
}

/// Whether an event shows at all ([`shown`] isn't [`Shown::Nothing`]), without laying it out:
/// what a restore counts toward its limit. `restoring` shows your own messages too.
pub fn shows(event: &Event, restoring: bool) -> bool {
    match &event.kind {
        EventKind::Message { role, content } => match role.as_str() {
            "user" => restoring && !content.is_empty(),
            "assistant" => !content.is_empty(),
            _ => false,
        },
        EventKind::ToolCall { calls } => !calls.is_empty(),
        EventKind::ToolResult { content, .. } => !is_empty_result(content),
        EventKind::Termination { .. } | EventKind::Failure { .. } => true,
        EventKind::Other { kind, .. } => !matches!(kind.as_deref(), Some("usage" | "reasoning")),
    }
}

/// What a result shows nothing for: none, `""`, and an empty object or list, as Emacs read those
/// as nil.
fn is_empty_result(content: &Value) -> bool {
    match content {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Object(o) => o.is_empty(),
        Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

/// How an event shows: its stubs, in `zone`'s time. `restoring` shows your own messages, which
/// a live chat drew when you sent them.
pub fn shown(event: &Event, restoring: bool, zone: &TimeZone) -> Shown {
    if !shows(event, restoring) {
        return Shown::Nothing;
    }
    let line = |stub: &str| stub_line(stub, event.timestamp, event.marginal_input(), zone);
    let one = |stub: String, face, lazy| {
        Shown::Sections(vec![Section {
            stub: line(&stub),
            face,
            lazy,
        }])
    };
    match &event.kind {
        EventKind::Message { role, content } if role == "user" => one(
            format!("[user] [{}]", preview(content)),
            face::USER_STUB,
            false,
        ),
        EventKind::Message { content, .. } => {
            one(format!("[agnt] [{}]", preview(content)), face::STUB, true)
        }
        EventKind::ToolCall { calls } => Shown::Sections(
            calls
                .iter()
                .map(|c| Section {
                    stub: line(&call_stub(c)),
                    face: face::STUB,
                    lazy: true,
                })
                .collect(),
        ),
        EventKind::ToolResult { content, .. } => {
            let rule = "─".repeat(PREVIEW_WIDTH - 2);
            one(format!("[rslt] [{rule}]"), face::STUB, is_file_map(content))
        }
        EventKind::Termination { reason } => {
            one(format!("[term] [{}]", preview(reason)), face::STUB, false)
        }
        EventKind::Failure { message } => one(
            format!("[fail] [{}]", preview(message)),
            face::FAILURE_STUB,
            false,
        ),
        EventKind::Other {
            kind: Some(kind),
            raw,
        } => Shown::Raw(format!("[Unhandled event: {kind}]\n{raw}\n\n")),
        EventKind::Other { kind: None, raw } => {
            Shown::Raw(format!("[Event with no type]\n{raw}\n\n"))
        }
    }
}

/// A tool call's stub, before its columns: `[bash] [cmd]`, `[read] [path]` or `[read] [N files]`,
/// `[edit] [path]`, and `[tool] [name]` for a tool orchid doesn't have.
fn call_stub(call: &ToolCall) -> String {
    match call.tool() {
        Tool::Bash { cmd } => {
            let flat = cmd.unwrap_or_default().replace('\n', " ");
            let shown = if flat.chars().count() > 45 {
                format!("{}...", flat.chars().take(42).collect::<String>())
            } else {
                flat
            };
            format!("[bash] [{shown}]")
        }
        Tool::Read { paths } => match paths.as_slice() {
            [one] => format!("[read] [{}]", truncate_path_left(one, 45)),
            [] => "[read] []".into(),
            many => format!("[read] [{} files]", many.len()),
        },
        Tool::Edit { path, .. } => {
            format!(
                "[edit] [{}]",
                truncate_path_left(&path.unwrap_or_default(), 45)
            )
        }
        Tool::Other => format!("[tool] [{}]", call.name),
    }
}

/// A result that's a map of files to their text or `{"error"}`, as `fs_read`'s is: not one of the
/// other objects orchid's tools give, `{"error"}`, `{"status","error"}` or `{"path","edits_applied"}`.
fn is_file_map(content: &Value) -> bool {
    let Value::Object(o) = content else {
        return false;
    };
    let keys = |ks: &[&str]| o.len() == ks.len() && ks.iter().all(|k| o.contains_key(*k));
    !o.is_empty()
        && !keys(&["error"])
        && !keys(&["status", "error"])
        && !o.contains_key("edits_applied")
}

/// The detail under an event's `index`th section (its stub's, in [`shown`]'s order), without the
/// newline that ends the stub. Empty past the last section. An `fs_edit`'s diff runs `diff -u`,
/// as the Emacs client's did, so it's made when it's wanted.
pub fn detail(event: &Event, index: usize) -> Vec<Piece> {
    let text = |s: String| vec![Piece::Text(s, None)];
    match &event.kind {
        EventKind::Message { role, content } if role == "user" => text(content.clone()),
        EventKind::Message { content, .. } => vec![
            Piece::Text("Assistant: ".into(), Some(face::ASSISTANT)),
            Piece::Text(content.clone(), None),
        ],
        EventKind::ToolCall { calls } => calls.get(index).map_or_else(Vec::new, call_detail),
        EventKind::ToolResult { call_id, content } => result_detail(call_id, content),
        EventKind::Termination { reason } => text(format!("  Reason: {reason}")),
        EventKind::Failure { message } => text(format!("  Message: {message}")),
        EventKind::Other { .. } => vec![],
    }
}

/// `  Tool:`, `  ID:` and the arguments, as each tool reads them.
fn call_detail(call: &ToolCall) -> Vec<Piece> {
    let head = format!("  Tool: {}\n  ID: {}\n", call.name, call.call_id);
    let indented = |s: &str| {
        s.split('\n')
            .map(|l| format!("    {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let args = match call.tool() {
        Tool::Bash { cmd: Some(cmd) } if cmd.contains('\n') => {
            format!("  Command:\n{}", indented(&cmd))
        }
        Tool::Bash { cmd } => format!("  Command: {}", cmd.unwrap_or_default()),
        Tool::Read { paths } if !paths.is_empty() => {
            format!("  Paths:\n{}", indented(&paths.join("\n")))
        }
        Tool::Read { .. } => "  Path: ".into(),
        Tool::Edit { path, edits } => {
            let mut out = vec![Piece::Text(
                format!("{head}  Path: {}\n", path.unwrap_or_default()),
                None,
            )];
            for (i, edit) in edits.iter().enumerate() {
                let ends_line = out
                    .last()
                    .is_some_and(|p| matches!(p, Piece::Text(t, _) if t.ends_with('\n')));
                if i > 0 && !ends_line {
                    out.push(Piece::Text("\n".into(), None));
                }
                out.extend(edit_diff(
                    edit.old_string.as_deref(),
                    edit.new_string.as_deref(),
                ));
            }
            return out;
        }
        Tool::Other => format!("  Args: {}", call.input),
    };
    vec![Piece::Text(head + &args, None)]
}

/// A result's detail: a string's lines under `  Result:`, a file map's files as sections under
/// `  Results:`, and orchid's other objects said in a line.
fn result_detail(call_id: &str, content: &Value) -> Vec<Piece> {
    let head = format!("  Call ID: {call_id}\n");
    let lines = |s: &str| {
        s.split('\n')
            .map(|l| format!("  {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let error = |k: &str| {
        content
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let said = match content {
        Value::String(s) => lines(s),
        _ if is_file_map(content) => return file_map(head, content),
        Value::Object(o) if o.contains_key("edits_applied") => format!(
            "  {}: {} edits applied",
            error("path"),
            o.get("edits_applied")
                .map_or_else(String::new, Value::to_string)
        ),
        Value::Object(o) if o.contains_key("status") => {
            format!("  [{}] {}", error("status"), error("error"))
        }
        Value::Object(_) => format!("  [error] {}", error("error")),
        other => lines(&other.to_string()),
    };
    vec![Piece::Text(format!("{head}  Result:\n{said}"), None)]
}

/// `fs_read`'s map: a collapsed `[path]` section for each file, its text or `[error] why` inside.
fn file_map(head: String, content: &Value) -> Vec<Piece> {
    let mut out = vec![Piece::Text(format!("{head}  Results:\n"), None)];
    let Value::Object(files) = content else {
        return out;
    };
    for (i, (path, value)) in files.iter().enumerate() {
        if i > 0 {
            out.push(Piece::Text("\n".into(), None));
        }
        let text = match value {
            Value::String(s) => s.clone(),
            v => match v.get("error").and_then(Value::as_str) {
                Some(e) => format!("[error] {e}"),
                None => v.to_string(),
            },
        };
        let stub = Section {
            stub: format!("[{path}]"),
            face: face::STUB,
            lazy: false,
        };
        out.push(Piece::Section(
            stub,
            vec![Piece::Text(format!("{text}\n"), None)],
        ));
    }
    out
}

/// One edit as `diff -u` shows it, under `  Diff:`, each line indented and in a diff face, with
/// the file header left out; `  No changes` if there are none. Without both strings, or without
/// `diff`, the strings themselves.
pub fn edit_diff(old: Option<&str>, new: Option<&str>) -> Vec<Piece> {
    let diffed = match (old, new) {
        (Some(a), Some(b)) => run_diff(a, b),
        _ => None,
    };
    let Some(out) = diffed else {
        let block = |name: &str, s: &str| {
            let lines: Vec<String> = s.split('\n').map(|l| format!("    {l}")).collect();
            format!("  {name}:\n{}", lines.join("\n"))
        };
        let mut text = old.map(|s| block("Old string", s)).unwrap_or_default();
        if let Some(s) = new {
            text += &format!("\n{}", block("New string", s));
        }
        return vec![Piece::Text(text, None)];
    };
    if out.is_empty() {
        return vec![Piece::Text("  No changes".into(), None)];
    }
    let mut pieces = vec![Piece::Text("  Diff:\n".into(), None)];
    let mut in_hunk = false;
    for line in out.split('\n').filter(|l| !l.is_empty()) {
        in_hunk |= line.starts_with("@@");
        let face = match line.as_bytes().first() {
            _ if !in_hunk => continue, // `---` and `+++`, the files' names
            Some(b'-') => face::DIFF_REMOVED,
            Some(b'+') => face::DIFF_ADDED,
            _ => face::DIFF_CONTEXT,
        };
        pieces.push(Piece::Text(format!("    {line}\n"), Some(face)));
    }
    pieces
}

/// `diff -u` between two strings, through files of their own in the temp directory. None if
/// diff couldn't run.
fn run_diff(old: &str, new: &str) -> Option<String> {
    let a = TempFile::new("old", old)?;
    let b = TempFile::new("new", new)?;
    let diff = if Path::new("/usr/bin/diff").exists() {
        "/usr/bin/diff"
    } else {
        "diff"
    };
    let out = std::process::Command::new(diff)
        .arg("-u")
        .arg(&a.0)
        .arg(&b.0)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    // 0: the same, 1: different. 2 is trouble.
    (out.status.code().is_some_and(|c| c < 2))
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A file made new for one diff, gone when it drops.
struct TempFile(std::path::PathBuf);

impl TempFile {
    fn new(which: &str, text: &str) -> Option<TempFile> {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let name = format!("orchid-diff-{which}-{}-{n}", std::process::id());
            let path = std::env::temp_dir().join(name);
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path);
            match file {
                Ok(mut f) => {
                    let made = TempFile(path);
                    return f.write_all(text.as_bytes()).ok().map(|_| made);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    //! The Emacs client's parser and format tests (`test/orchid-parser-test.el`,
    //! `orchid-browser-format-test.el`, and the indicator's and restored line's), where they hold
    //! for orchid 0.2. Their times are in Australia/Sydney, which is UTC+10 in July: a fixed zone.
    use super::*;
    use crate::store::Event;
    use jiff::tz::Offset;

    fn sydney() -> TimeZone {
        TimeZone::fixed(Offset::constant(10))
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn event(json: &str) -> Event {
        Event::parse(json).unwrap()
    }

    fn sections(e: &Event, restoring: bool) -> Vec<Section> {
        match shown(e, restoring, &sydney()) {
            Shown::Sections(s) => s,
            other => panic!("{other:?}"),
        }
    }

    /// A detail's text, faces and nesting left out.
    fn flat(pieces: &[Piece]) -> String {
        pieces
            .iter()
            .map(|p| match p {
                Piece::Text(t, _) => t.clone(),
                Piece::Section(s, d) => format!("{}\n{}", s.stub, flat(d)),
            })
            .collect()
    }

    #[test]
    fn a_flat_message_shows_a_collapsed_lazy_reply() {
        let e = event(r#"{"type":"message","role":"assistant","content":"Hello world"}"#);
        let s = sections(&e, false);
        assert_eq!(s.len(), 1);
        assert!(
            s[0].stub.starts_with("[agnt]    [Hello world "),
            "{:?}",
            s[0].stub
        );
        assert!(s[0].lazy);
        assert_eq!(s[0].face, face::STUB);
        assert_eq!(flat(&detail(&e, 0)), "Assistant: Hello world");
        assert_eq!(
            detail(&e, 0)[0],
            Piece::Text("Assistant: ".into(), Some(face::ASSISTANT))
        );
        let empty = event(r#"{"type":"message","role":"assistant","content":""}"#);
        assert_eq!(shown(&empty, true, &sydney()), Shown::Nothing);
    }

    #[test]
    fn your_messages_show_only_when_restoring() {
        let e = event(r#"{"type":"message","role":"user","content":"line one\nline two"}"#);
        assert_eq!(shown(&e, false, &sydney()), Shown::Nothing);
        assert!(!shows(&e, false) && shows(&e, true));
        let s = sections(&e, true);
        assert!(
            s[0].stub.starts_with("[user]    [line one line two "),
            "{:?}",
            s[0].stub
        );
        assert_eq!((s[0].face, s[0].lazy), (face::USER_STUB, false));
        assert_eq!(flat(&detail(&e, 0)), "line one\nline two");
    }

    #[test]
    fn unknown_and_untyped_events_show_raw() {
        let line = r#"{"type":"orchid_unknown_xyz","data":"test"}"#;
        assert_eq!(
            shown(&event(line), false, &sydney()),
            Shown::Raw(format!("[Unhandled event: orchid_unknown_xyz]\n{line}\n\n"))
        );
        let line = r#"{"content":"no type here"}"#;
        assert_eq!(
            shown(&event(line), false, &sydney()),
            Shown::Raw(format!("[Event with no type]\n{line}\n\n"))
        );
        // What orchid 0.2 ignores shows nothing.
        for kind in ["usage", "reasoning"] {
            let e = event(&format!(r#"{{"type":"{kind}","input":3}}"#));
            assert_eq!(shown(&e, true, &sydney()), Shown::Nothing);
        }
    }

    #[test]
    fn truncate_path_left() {
        assert_eq!(super::truncate_path_left("/short/path", 35), "/short/path");
        let path = "/home/user/.config/orchid/conversations/abc123";
        let cut = super::truncate_path_left(path, 35);
        assert_eq!(cut.chars().count(), 35);
        assert!(cut.starts_with('…'));
        assert!(cut.ends_with(&path[path.len() - 34..]));
    }

    fn call(name: &str, input: &str) -> Event {
        event(&format!(
            r#"{{"type":"tool_call","calls":[{{"call_id":"c1","name":"{name}","input":{input}}}],"event_id":"e1","timestamp":"2026-07-22T01:16:46+10:00","token_usage":{{"marginal_input":102000}}}}"#
        ))
    }

    #[test]
    fn tool_call_stubs() {
        let unknown = sections(&call("example_tool", "{}"), false);
        assert!(
            unknown[0].stub.starts_with("[tool]    [example_tool "),
            "{:?}",
            unknown[0].stub
        );
        assert!(!unknown[0].stub.contains("tool call:"));
        let short = sections(&call("fs_read", r#"{"paths":["/short/path"]}"#), false);
        assert!(
            short[0].stub.starts_with("[read]    [/short/path "),
            "{:?}",
            short[0].stub
        );
        let long = "/home/user/.config/orchid/conversations/abc123/somefile.el";
        let cut = sections(
            &call("fs_read", &format!(r#"{{"paths":["{long}"]}}"#)),
            false,
        );
        assert!(cut[0].stub.starts_with("[read]    […"), "{:?}", cut[0].stub);
        assert!(!cut[0].stub.contains(long));
        let three = call("fs_read", r#"{"paths":["./foo.el","./bar.el","./baz.el"]}"#);
        assert!(
            sections(&three, false)[0]
                .stub
                .starts_with("[read]    [3 files ")
        );
        let d = flat(&detail(&three, 0));
        assert_eq!(
            d,
            "  Tool: fs_read\n  ID: c1\n  Paths:\n    ./foo.el\n    ./bar.el\n    ./baz.el"
        );
        let bash = call("bash", r#"{"cmd":"ls -la"}"#);
        let s = sections(&bash, false);
        assert!(
            s[0].stub.starts_with("[bash]    [ls -la "),
            "{:?}",
            s[0].stub
        );
        assert!(s[0].lazy);
        assert_eq!(
            flat(&detail(&bash, 0)),
            "  Tool: bash\n  ID: c1\n  Command: ls -la"
        );
        let multi = call("bash", r#"{"cmd":"cd x\nmake"}"#);
        assert_eq!(
            flat(&detail(&multi, 0)),
            "  Tool: bash\n  ID: c1\n  Command:\n    cd x\n    make"
        );
        let other = call("example_tool", r#"{"a":1}"#);
        assert_eq!(
            flat(&detail(&other, 0)),
            "  Tool: example_tool\n  ID: c1\n  Args: {\"a\":1}"
        );
        let none = event(r#"{"type":"tool_call","calls":[]}"#);
        assert_eq!(shown(&none, true, &sydney()), Shown::Nothing);
    }

    #[test]
    fn a_long_command_shows_its_first_42_characters() {
        let cmd = "a".repeat(50);
        let s = sections(&call("bash", &format!(r#"{{"cmd":"{cmd}"}}"#)), false);
        let want = format!("[bash]    [{}{}]", "a".repeat(42), " ".repeat(5));
        assert!(s[0].stub.starts_with(&want), "{:?}", s[0].stub);
    }

    #[test]
    fn an_edit_shows_a_diff_per_edit() {
        let e = call(
            "fs_edit",
            r#"{"path":"/foo/bar.el","edits":[{"old_string":"hello world","new_string":"hello emacs"},{"old_string":"x\ny","new_string":"x\nz"}]}"#,
        );
        assert!(
            sections(&e, false)[0]
                .stub
                .starts_with("[edit]    [/foo/bar.el ")
        );
        let pieces = detail(&e, 0);
        let d = flat(&pieces);
        assert!(
            d.starts_with("  Tool: fs_edit\n  ID: c1\n  Path: /foo/bar.el\n  Diff:\n"),
            "{d}"
        );
        assert!(d.contains("    -hello world\n"), "{d}");
        assert!(d.contains("    +hello emacs\n"), "{d}");
        assert!(
            d.contains("     x\n    -y\n    \\ No newline at end of file\n    +z\n"),
            "{d}"
        );
        assert_eq!(d.matches("  Diff:\n").count(), 2, "{d}");
        assert!(!d.contains("--- ") && !d.contains("+++ "), "{d}");
        assert!(pieces.contains(&Piece::Text(
            "    -hello world\n".into(),
            Some(face::DIFF_REMOVED)
        )));
        assert!(pieces.contains(&Piece::Text(
            "    +hello emacs\n".into(),
            Some(face::DIFF_ADDED)
        )));
        assert!(pieces.contains(&Piece::Text(
            "    @@ -1 +1 @@\n".into(),
            Some(face::DIFF_CONTEXT)
        )));
        // A removed line that starts with `--` is a line, not a header.
        let dashes = flat(&edit_diff(Some("-- a\n"), Some("b\n")));
        assert!(dashes.contains("    --- a\n"), "{dashes}");
        assert_eq!(flat(&edit_diff(Some("same"), Some("same"))), "  No changes");
        assert_eq!(
            flat(&edit_diff(Some("a\nb"), None)),
            "  Old string:\n    a\n    b"
        );
    }

    fn result(content: &str) -> Event {
        event(&format!(
            r#"{{"type":"tool_result","call_id":"cid1","content":{content},"event_id":"e1","timestamp":"2026-06-06T04:38:21.765384Z"}}"#
        ))
    }

    #[test]
    fn a_string_result_shows_its_lines() {
        let e = result(r#""applied successfully\nsecond""#);
        let s = sections(&e, false);
        assert!(!s[0].lazy);
        assert!(
            s[0].stub
                .starts_with(&format!("[rslt]    [{}]", "─".repeat(47)))
        );
        assert_eq!(
            flat(&detail(&e, 0)),
            "  Call ID: cid1\n  Result:\n  applied successfully\n  second"
        );
        for empty in ["\"\"", "null", "{}", "[]"] {
            assert_eq!(
                shown(&result(empty), true, &sydney()),
                Shown::Nothing,
                "{empty}"
            );
        }
    }

    #[test]
    fn a_file_map_result_shows_a_section_per_file() {
        let e = result(
            r#"{"./foo/bar.el":"line one\nline two","./baz/qux.el":{"error":"Is a directory"}}"#,
        );
        assert!(sections(&e, false)[0].lazy);
        let d = detail(&e, 0);
        assert_eq!(
            d[0],
            Piece::Text("  Call ID: cid1\n  Results:\n".into(), None)
        );
        let Piece::Section(s, inner) = &d[1] else {
            panic!("{d:?}")
        };
        assert_eq!(
            (s.stub.as_str(), s.face, s.lazy),
            ("[./baz/qux.el]", face::STUB, false)
        );
        assert_eq!(
            inner,
            &[Piece::Text("[error] Is a directory\n".into(), None)]
        );
        assert_eq!(d[2], Piece::Text("\n".into(), None));
        let Piece::Section(s, inner) = &d[3] else {
            panic!("{d:?}")
        };
        assert_eq!(s.stub, "[./foo/bar.el]");
        assert_eq!(inner, &[Piece::Text("line one\nline two\n".into(), None)]);
    }

    #[test]
    fn orchids_other_results_are_said_in_a_line() {
        let said = |c: &str| flat(&detail(&result(c), 0));
        assert_eq!(
            said(r#"{"error":"blocked by hook: no"}"#),
            "  Call ID: cid1\n  Result:\n  [error] blocked by hook: no"
        );
        assert_eq!(
            said(r#"{"status":"terminated","error":"cancelled by user"}"#),
            "  Call ID: cid1\n  Result:\n  [terminated] cancelled by user"
        );
        assert_eq!(
            said(r#"{"path":"src/a.rs","edits_applied":2}"#),
            "  Call ID: cid1\n  Result:\n  src/a.rs: 2 edits applied"
        );
        for c in [
            r#"{"error":"x"}"#,
            r#"{"status":"terminated","error":"x"}"#,
            r#"{"path":"p","edits_applied":1}"#,
        ] {
            assert!(!sections(&result(c), false)[0].lazy);
        }
    }

    #[test]
    fn failures_and_terminations_have_stubs() {
        let f = event(
            r#"{"type":"failure","message":"connection refused","timestamp":"2026-07-22T01:16:46+10:00"}"#,
        );
        let s = sections(&f, false);
        assert!(
            s[0].stub.starts_with("[fail]    [connection refused "),
            "{:?}",
            s[0].stub
        );
        assert!(s[0].stub.ends_with("    [01:16:46]"), "{:?}", s[0].stub);
        assert_eq!(s[0].face, face::FAILURE_STUB);
        assert_eq!(flat(&detail(&f, 0)), "  Message: connection refused");
        let t = event(r#"{"type":"termination","reason":"cancelled by user"}"#);
        assert!(
            sections(&t, false)[0]
                .stub
                .starts_with("[term]    [cancelled by user")
        );
        assert_eq!(flat(&detail(&t, 0)), "  Reason: cancelled by user");
    }

    #[test]
    fn stub_columns_align_tool_events() {
        let time = Some(ts("2026-07-22T01:16:46+10:00"));
        let bash = stub_line(
            "[bash] [# run formatter and focused compile check d]",
            time,
            Some(102_000),
            &sydney(),
        );
        let rule = format!("[rslt] [{}]", "─".repeat(47));
        let result = stub_line(&rule, time, Some(111_000), &sydney());
        assert!(bash.starts_with("[bash]    [# run formatter"));
        assert!(result.starts_with("[rslt]    [─"));
        assert!(bash.ends_with("    [102k] [01:16:46]"), "{bash}");
        assert!(result.ends_with("    [111k] [01:16:46]"), "{result}");
        assert_eq!(width(&bash), 80);
        assert_eq!(width(&result), 80);
        assert!(result.contains(&format!("[rslt]    [{}]", "─".repeat(47))));
        assert!(!result.contains("─ "));
        assert!(!bash.contains("2026-07-22"));
    }

    #[test]
    fn a_bracketed_preview_keeps_its_label() {
        let stub = stub_line(
            "[agnt] [Updated [parser] behavior]",
            Some(ts("2026-07-22T01:16:46+10:00")),
            Some(63_000),
            &sydney(),
        );
        assert!(stub.starts_with("[agnt]    [Updated [parser] behavior"));
        assert_eq!(width(&stub), 80);
        for role in ["user", "assistant"] {
            let e = event(&format!(
                r#"{{"type":"message","role":"{role}","content":"Please inspect [this] value","timestamp":"2026-07-22T01:16:46+10:00","token_usage":{{"marginal_input":63000}}}}"#
            ));
            let want = if role == "user" { "[user]" } else { "[agnt]" };
            assert!(sections(&e, true)[0].stub.starts_with(want));
        }
    }

    #[test]
    fn a_stub_that_isnt_two_parts_is_an_event() {
        let z = sydney();
        assert_eq!(
            stub_line("[example_tool]", None, None, &z),
            format!("[event]   [{}]", " ".repeat(47))
        );
        let wide = stub_line("[much too wide] [x]", None, None, &z);
        assert!(wide.starts_with("[event]   [x "), "{wide}");
        // No tokens: the time alone, after four spaces.
        let t = stub_line("[agnt] [x]", Some(ts("2026-07-22T01:16:46Z")), None, &z);
        assert!(
            t.ends_with(&format!("x{}]    [11:16:46]", " ".repeat(46))),
            "{t}"
        );
        // A trailing `...` goes, and wide characters fill whole columns.
        assert!(stub_line("[agnt] [abc...]", None, None, &z).starts_with("[agnt]    [abc "));
        let cjk = stub_line(&format!("[agnt] [{}]", "界".repeat(30)), None, None, &z);
        assert_eq!(width(&cjk), 59);
        assert!(cjk.ends_with(&format!("{} ]", "界".repeat(23))), "{cjk}");
    }

    #[test]
    fn timestamps_convert_to_local_time() {
        assert_eq!(
            local_time(ts("2026-07-22T01:16:46Z"), &sydney()),
            "11:16:46"
        );
        assert_eq!(
            local_time(ts("2026-07-22T01:16:46Z"), &TimeZone::UTC),
            "01:16:46"
        );
    }

    #[test]
    fn message_stubs_show_marginal_input_tokens() {
        let e = event(
            r#"{"type":"message","role":"assistant","content":"done","timestamp":"2026-07-22T01:16:46+10:00","token_usage":{"marginal_input":94000}}"#,
        );
        assert!(
            sections(&e, false)[0]
                .stub
                .ends_with("    [ 94k] [01:16:46]")
        );
        let at = ts("2026-07-22T01:16:46+10:00");
        assert!(user_stub("hi", at, Some(63_000), &sydney()).ends_with("    [ 63k] [01:16:46]"));
    }

    #[test]
    fn token_counts() {
        let cases = [
            (0, "  0k"),
            (999, "  0k"),
            (94_000, " 94k"),
            (999_999, "999k"),
            (1_000_000, "1.0m"),
            (4_250_000, "4.2m"),
            (9_990_000, "10.0"),
            (10_000_000, " 10m"),
            (1_234_000_000, "1234"),
        ];
        for (n, want) in cases {
            assert_eq!(format_tokens(n), want, "{n}");
        }
        assert_eq!(format_cumulative(4_200_000), " 4200k");
        assert_eq!(format_cumulative(99_999_999), "99999k");
        assert_eq!(format_cumulative(100_000_000), "  100m");
    }

    #[test]
    fn the_indicator_and_restored_lines_right_align_their_tokens() {
        let tokens = format!("[{}]", format_cumulative(4_200_000));
        let line = right_aligned("[Finished: 4s]", &tokens);
        assert_eq!(line, format!("[Finished: 4s]{}[ 4200k]", " ".repeat(58)));
        assert_eq!(width(&line), 80);
        let restored = right_aligned("[session restored]", &tokens);
        assert!(restored.starts_with("[session restored]") && restored.ends_with("[ 4200k]"));
        assert_eq!(width(&restored), 80);
        assert_eq!(
            right_aligned(&"x".repeat(80), "[y]"),
            format!("{} [y]", "x".repeat(80))
        );
    }

    #[test]
    fn relative_times() {
        let now = ts("2026-11-09T12:00:00Z");
        let ago = |s: i64| relative_time(Some(now - jiff::SignedDuration::from_secs(s)), now);
        assert_eq!(relative_time(None, now), "N/A");
        assert_eq!(ago(0), "just now");
        assert_eq!(ago(59), "just now");
        assert_eq!(ago(-30), "just now", "a clock ahead of yours");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(150), "2 minutes ago");
        assert_eq!(ago(3600), "1 hour ago");
        assert_eq!(ago(7300), "2 hours ago");
        assert_eq!(ago(86_400), "1 day ago");
        assert_eq!(ago(3 * 86_400 + 5), "3 days ago");
        // A metadata.json whose time doesn't parse has none.
        let m = Metadata::parse(r#"{"id":"s","status":"idle","updated_at":"not-a-date"}"#).unwrap();
        assert_eq!(relative_time(m.updated_at, now), "N/A");
        assert_eq!(
            relative_time(
                Metadata::parse(r#"{"id":"s","status":"idle","updated_at":""}"#)
                    .unwrap()
                    .updated_at,
                now
            ),
            "N/A"
        );
    }

    #[test]
    fn workspace_and_agent_names() {
        assert_eq!(workspace_name(Some("/home/user/projects/my-app")), "my-app");
        assert_eq!(
            workspace_name(Some("/home/user/projects/my-app/")),
            "my-app"
        );
        assert_eq!(workspace_name(None), "N/A");
        let m = Metadata::parse(r#"{"id":"s","status":"idle"}"#).unwrap();
        assert_eq!(agent_name(&m), "default");
        let m = Metadata::parse(r#"{"id":"s","status":"idle","agent_name":"developer"}"#).unwrap();
        assert_eq!(agent_name(&m), "developer");
    }

    #[test]
    fn labels_cut_with_two_dots_keep_their_column() {
        let label = truncate_width(&"x".repeat(40), 30, "..");
        assert_eq!(label, format!("{}..", "x".repeat(28)));
        assert_eq!(width(&label), 30);
        assert_eq!(truncate_width("short", 30, ".."), "short");
        assert_eq!(
            truncate_width(&"界".repeat(20), 30, ".."),
            format!("{}..", "界".repeat(14))
        );
    }

    #[test]
    fn widths_are_as_die_draws_them_and_tabs_as_emacs_counted() {
        // A tab is eight, as Emacs's `string-width` said: this stub is Emacs's byte for byte.
        let at: Timestamp = "2026-07-22T01:16:46Z".parse().unwrap();
        let zone = crate::test_zone();
        let tabbed = stub_line(
            "[agnt] [col1\tcol2\tcol3 and more text]",
            Some(at),
            Some(63_000),
            &zone,
        );
        assert_eq!(
            tabbed,
            "[agnt]    [col1\tcol2\tcol3 and more text     ]    [ 63k] [11:16:46]"
        );
        // A cluster is as wide as DIE draws it, and a control character as `^M` or `\200`.
        assert_eq!(width("⚠️ ok"), 5);
        assert_eq!(width("✔️"), 2);
        assert_eq!(width("e\u{301}"), 1);
        assert_eq!(width("a\rb\x7f"), 6);
        assert_eq!(width("\u{80}"), 4);
        assert_eq!(width("👨‍👩‍👧"), 2);
        for preview in [
            "⚠️ Two tests fail on main; ✔️ the rest pass",
            "crlf reply\r second line",
        ] {
            let stub = stub_line(
                &format!("[agnt] [{preview}]"),
                Some(at),
                Some(63_000),
                &zone,
            );
            assert_eq!(width(&stub), WIDTH, "{stub}");
        }
        // Cut between clusters, never inside one.
        assert_eq!(truncate_width("ab⚠️cd", 3, ""), "ab");
        assert_eq!(truncate_width("ab⚠️cd", 4, ""), "ab⚠️");
    }
}
