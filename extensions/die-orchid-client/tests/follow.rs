//! A chat following its session, on the harness's fake clock, with die-vim: events streaming in
//! while you type, sending, `[SYSTEM]`, the processing indicator and each of orchid's ends, a
//! session already running when it opens (B2), the restore meeting the follow (B9), an archived
//! session (B8), a turn sent from elsewhere, a new session's pending chat, and a restart. The
//! Emacs client's `orchid-chat-send-test`, `orchid-chat-display-test`,
//! `orchid-processing-indicator-test` and `orchid-log-*-test` specs that hold for orchid 0.2 are
//! here. orchid is faked: a test plays its part by writing the session's files.

use die_test_cap as _;

use die_core::{BufferId, Deco, Harness, Side, State, WindowId, parse_keys};
use die_orchid_client::cli::{Call, FakeRunner, Reply};
use die_orchid_client::fixtures::{self, Meta, TempRoot};
use die_orchid_client::poll::{FAST, SLOW};
use die_orchid_client::store::{self, Status};
use die_orchid_client::{Kind, Orchid, chat, indicator, log, parse};
use die_vim::Vim;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

const ID: &str = "0f1e2d3c-aaaa-bbbb-cccc-123456789abc";
const NAME: &str = "*orchid-chat-default-die-89abc*";
const SECOND: Duration = Duration::from_secs(1);

fn harness(root: &TempRoot, fake: &FakeRunner) -> Harness {
    let orchid = Orchid::for_tests(root.path(), fake.clone());
    let mut h = Harness::new(vec![Box::new(Vim::default()), Box::new(orchid)]).unwrap();
    h.open_text("notes", "").unwrap();
    h.set_window_size(80, 40);
    h
}

fn meta(status: &str) -> Meta {
    let mut m = Meta::new(ID);
    m.status = status.into();
    m
}

/// orchid's part in a send: the session runs, and `send` says so.
fn sends(fake: &FakeRunner, root: &TempRoot) {
    let root = root.path().to_path_buf();
    fake.on("send", move |_: &Call| {
        meta("running").write(&root);
        Reply::ok(&json!({"id": ID, "status": "running", "pid": 4242}).to_string())
    });
}

/// A chat on a session in `status` with these events.
fn opened(h: &mut Harness, root: &TempRoot, m: Meta, events: &[String]) -> (BufferId, WindowId) {
    m.write(root.path());
    fixtures::append(root.path(), &m.id, events);
    chat::open(h, &m.id).unwrap();
    h.run_until_idle();
    assert!(h.take_errors().is_empty());
    (h.focused_buffer().unwrap(), h.focused().unwrap())
}

fn text(h: &Harness, b: BufferId) -> String {
    h.text(b).unwrap().rope().to_string()
}

/// What follows the last separator's blank line: your input.
fn input(h: &Harness, b: BufferId) -> String {
    let t = text(h, b);
    let sep = chat::separator(Some("die"));
    let after = t.rsplit_once(&format!("{sep}\n\n")).map(|x| x.1);
    after.unwrap_or_default().to_string()
}

fn rows(h: &mut Harness) -> Vec<String> {
    let screen = h.screen();
    let mut all: Vec<String> = screen.lines().map(String::from).collect();
    all.truncate(all.len().saturating_sub(2));
    while all.last().is_some_and(String::is_empty) {
        all.pop();
    }
    all
}

fn keys(h: &mut Harness, k: &str) -> Vec<String> {
    let log = h.type_keys(k).unwrap();
    log.into_iter().flat_map(|k| k.errors).collect()
}

/// Keys, each taken in turn without waiting for a job, as typed while one is out.
fn keys_now(h: &mut Harness, k: &str) {
    for ev in parse_keys(k).unwrap() {
        h.input(ev);
        while h.step() {}
    }
}

fn cursor(h: &Harness, b: BufferId, w: WindowId) -> usize {
    h.text(b).unwrap().cursor(w).unwrap()
}

/// The indicator's row, if it shows.
fn indicator_row(h: &mut Harness) -> Option<String> {
    let all = rows(h);
    all.into_iter()
        .find(|r| r.starts_with("[Processing: ") || r.starts_with("[Finished: "))
}

fn period(h: &mut Harness) -> Option<Duration> {
    h.with_ext(|o: &mut Orchid, _| o.poll.period()).unwrap()
}

fn log_lines(h: &Harness) -> Vec<String> {
    match h.find_buffer(log::BUFFER) {
        Some(b) => text(h, b).lines().map(|l| l[11..].to_string()).collect(),
        None => vec![],
    }
}

/// A line of `events.jsonl` that happened `ago` before now, on the wall clock.
fn message_ago(id: &str, role: &str, content: &str, ago: Duration) -> String {
    let at = jiff::Timestamp::now() - jiff::SignedDuration::try_from(ago).unwrap();
    let fields = json!({"type": "message", "role": role, "content": content});
    fixtures::line(id, &at.to_string(), fixtures::MARGINAL, fields)
}

// ------------------------------------------------------------------ streaming and typing

#[test]
fn what_you_type_stays_put_while_a_turns_events_stream_in() {
    let root = TempRoot::new("typing");
    let fake = FakeRunner::new();
    sends(&fake, &root);
    let mut h = harness(&root, &fake);
    let events = [
        fixtures::message("e1", "user", "hello there"),
        fixtures::message("e2", "assistant", "Hi!"),
    ];
    let (b, w) = opened(&mut h, &root, meta("idle"), &events);
    // Another window on the chat, reading the transcript.
    let other = h.split_window(w, Side::Below).unwrap();
    let reading = text(&h, b).find("[user]").unwrap();
    h.text_mut(b).unwrap().set_cursor(other, reading);
    h.focus(w);
    assert!(keys(&mut h, "ifirst<CR>").is_empty());
    assert_eq!(input(&h, b), "");
    assert!(keys(&mut h, "hel").is_empty());
    fixtures::append(
        root.path(),
        ID,
        &[
            fixtures::message("e3", "user", "first"),
            fixtures::message("e4", "assistant", "streamed reply"),
            fixtures::tool_result("e5", "c1", json!("─".repeat(60))),
        ],
    );
    assert!(h.advance(FAST).is_empty());
    assert!(keys(&mut h, "lo").is_empty());
    assert_eq!(input(&h, b), "hello");
    assert_eq!(
        cursor(&h, b, w),
        text(&h, b).len(),
        "still after what you typed"
    );
    assert_eq!(h.state_of(b), State::Insert);
    assert_eq!(
        cursor(&h, b, other),
        reading,
        "the other window keeps its place"
    );
    // Your own message isn't shown again when orchid has it; the turn's events go under it.
    let t = text(&h, b);
    assert_eq!(t.matches("[user]    [first ").count(), 1);
    let at = |s: &str| t.find(s).unwrap();
    assert!(at("[user]    [first ") < at("[agnt]    [streamed reply"));
    assert!(at("[agnt]    [streamed reply") < at("[rslt]"));
    assert!(at("[rslt]") < at("[Processing: "));
    assert!(at("[Processing: ") < at("\n\nhello"));
    // Esc leaves insert state, `.` is what you typed, and undo takes back only that.
    assert!(keys(&mut h, "<Esc>").is_empty());
    assert_eq!(h.state_of(b), State::Normal);
    assert_eq!(h.register('.').map(|r| r.text.as_str()), Some("hello"));
    let before = text(&h, b);
    assert!(keys(&mut h, "u").is_empty());
    assert_eq!(input(&h, b), "");
    assert_eq!(text(&h, b), before.trim_end_matches("hello"));
}

// ------------------------------------------------------------------ sending

#[test]
fn ret_in_insert_sends_the_trimmed_input_after_two_dashes() {
    let root = TempRoot::new("send");
    let fake = FakeRunner::new();
    sends(&fake, &root);
    let mut h = harness(&root, &fake);
    let (b, w) = opened(&mut h, &root, meta("idle"), &[]);
    assert!(keys(&mut h, "i  -x first line<S-CR>second  <CR>").is_empty());
    let root_arg = root.path().display().to_string();
    assert_eq!(
        fake.argvs(),
        [vec![
            "--config".to_string(),
            root_arg,
            "send".into(),
            "--id".into(),
            ID.into(),
            "--".into(),
            "-x first line\nsecond".into(),
        ]]
    );
    // Your stub, with the session's marginal input and the time now; then the indicator.
    let shown = rows(&mut h);
    let stub = shown.iter().find(|r| r.starts_with("[user]")).unwrap();
    assert!(
        stub.starts_with("[user]    [-x first line second "),
        "{stub}"
    );
    assert_eq!(parse::width(stub), 80);
    let time = &stub[stub.len() - 10..];
    assert!(stub[..70].ends_with("    [ 63k] "), "{stub}");
    let digits = time.bytes().filter(u8::is_ascii_digit).count();
    assert!(
        time.starts_with('[') && time.ends_with(']') && digits == 6,
        "{time}"
    );
    assert_eq!(
        indicator_row(&mut h).unwrap(),
        indicator::line(0, false, 4_200_000).trim_end()
    );
    assert_eq!(input(&h, b), "");
    assert_eq!(cursor(&h, b, w), text(&h, b).len());
    assert_eq!(h.state_of(b), State::Insert);
    // Its detail is all of it.
    let t = text(&h, b);
    assert!(t.contains("]\n-x first line\nsecond\n"), "{t}");
    // An empty input sends nothing (B3), and nor do spaces, which stay.
    keys(&mut h, "<CR>   <CR>");
    assert_eq!(fake.calls().len(), 1);
    assert_eq!(input(&h, b), "   ");
    // RET before the input is a newline; in normal state it's die-vim's.
    keys(&mut h, "<Esc>ggA<CR>");
    assert!(text(&h, b).starts_with("Workspace: /tmp/die\n\nAgent: "));
    keys(&mut h, "<Esc><CR>");
    assert_eq!(fake.calls().len(), 1);
    assert!(h.take_errors().is_empty());
}

#[test]
fn your_stub_takes_orchids_estimate_before_a_provider_said() {
    let root = TempRoot::new("estimate");
    let fake = FakeRunner::new();
    sends(&fake, &root);
    let mut h = harness(&root, &fake);
    let mut m = meta("idle");
    (m.marginal_input, m.token_estimate) = (0, 42_000);
    opened(&mut h, &root, m, &[]);
    keys(&mut h, "ihi<CR>");
    let shown = rows(&mut h);
    let stub = shown.iter().find(|r| r.starts_with("[user]")).unwrap();
    assert!(stub.contains("    [ 42k] ["), "{stub}");
}

#[test]
fn a_failed_send_says_so_above_the_input() {
    let root = TempRoot::new("failed");
    let fake = FakeRunner::new();
    fake.reply("send", Reply::error("Connection refused"));
    let mut h = harness(&root, &fake);
    let (b, _) = opened(&mut h, &root, meta("idle"), &[]);
    keys(&mut h, "ioops<CR>");
    h.run_until_idle();
    let t = text(&h, b);
    let rule = chat::rule();
    let said = format!("\n\n{rule}\n[SYSTEM] Send failed:\nConnection refused\n{rule}\n\n");
    assert_eq!(t.matches("[SYSTEM]").count(), 1, "{t}");
    assert!(t.ends_with(&said), "{t:?}");
    assert!(
        !t.contains("command_failed"),
        "orchid's message, not its error (A5)"
    );
    assert!(indicator_row(&mut h).is_none());
    let at = t.find("[SYSTEM]").unwrap();
    let warning = h
        .text(b)
        .unwrap()
        .decorations_at(at)
        .any(|(_, d)| matches!(d.deco, Deco::Face("warning")));
    assert!(warning);
    // What you type next is the input, under it.
    keys(&mut h, "next");
    assert!(text(&h, b).ends_with(&format!("{rule}\n\nnext")));
    assert!(log_lines(&h).contains(&"Send failed: Connection refused".to_string()));
}

// ------------------------------------------------------------------ the indicator

#[test]
fn the_indicator_ticks_while_a_turn_runs_and_finishes_on_each_of_orchids_ends() {
    for end in ["idle", "failed", "cancelled", "terminated"] {
        let root = TempRoot::new(&format!("ends-{end}"));
        let fake = FakeRunner::new();
        sends(&fake, &root);
        let mut h = harness(&root, &fake);
        opened(&mut h, &root, meta("idle"), &[]);
        assert_eq!(period(&mut h), Some(SLOW));
        keys(&mut h, "igo<CR>");
        assert_eq!(period(&mut h), Some(FAST));
        let line = |s, done, n| indicator::line(s, done, n).trim_end().to_string();
        assert_eq!(indicator_row(&mut h), Some(line(0, false, 4_200_000)));
        h.advance(SECOND);
        assert_eq!(indicator_row(&mut h), Some(line(1, false, 4_200_000)));
        h.advance(SECOND);
        assert_eq!(indicator_row(&mut h), Some(line(2, false, 4_200_000)));
        let running = h.with_ext(|o: &mut Orchid, _| o.session(ID).map(|s| s.status));
        assert_eq!(
            running.unwrap(),
            Some(Status::Running),
            "the registry heard"
        );
        let mut m = meta(end);
        m.cumulative_input = 5_100_000;
        m.write(root.path());
        assert!(h.advance(FAST).is_empty());
        let finished = line(2, true, 5_100_000);
        assert_eq!(
            finished,
            format!("[Finished: 2s]{}[ 5100k]", " ".repeat(58))
        );
        assert_eq!(indicator_row(&mut h), Some(finished.clone()), "{end}");
        // It stops ticking, and the poll slows.
        h.advance(3 * SECOND);
        assert_eq!(indicator_row(&mut h), Some(finished), "{end}");
        assert_eq!(period(&mut h), Some(SLOW), "{end}");
        let now = h.with_ext(|o: &mut Orchid, _| o.session(ID).map(|s| s.status.as_str()));
        assert_eq!(now.unwrap(), Some(end));
    }
}

#[test]
fn a_turn_over_before_it_was_seen_running_finishes_too() {
    let root = TempRoot::new("quick");
    let fake = FakeRunner::new();
    let at = root.path().to_path_buf();
    fake.on("send", move |_: &Call| {
        // An echo turn, done by the time anything looks.
        meta("idle").write(&at);
        let reply = fixtures::message("e2", "assistant", "echo: go");
        fixtures::append(&at, ID, &[fixtures::message("e1", "user", "go"), reply]);
        Reply::ok(&json!({"id": ID, "status": "running", "pid": 1}).to_string())
    });
    let mut h = harness(&root, &fake);
    let (b, _) = opened(&mut h, &root, meta("idle"), &[]);
    keys(&mut h, "igo<CR>");
    h.advance(FAST);
    let row = indicator_row(&mut h).unwrap();
    assert!(row.starts_with("[Finished: 0s]"), "{row}");
    let t = text(&h, b);
    assert!(t.find("[agnt]    [echo: go").unwrap() < t.find("[Finished").unwrap());
}

#[test]
fn a_status_that_doesnt_read_never_finishes_it() {
    let root = TempRoot::new("unreadable");
    let fake = FakeRunner::new();
    sends(&fake, &root);
    let mut h = harness(&root, &fake);
    opened(&mut h, &root, meta("idle"), &[]);
    keys(&mut h, "igo<CR>");
    std::fs::write(store::metadata_path(root.path(), ID), "{ half a file").unwrap();
    h.advance(2 * SECOND);
    let row = indicator_row(&mut h).unwrap();
    assert!(row.starts_with("[Processing: 2s]"), "{row}");
    let trouble: Vec<String> = log_lines(&h)
        .into_iter()
        .filter(|l| l.contains("metadata.json"))
        .collect();
    assert_eq!(trouble.len(), 1, "logged once, not each tick: {trouble:?}");
}

#[test]
fn a_session_running_when_it_opens_streams_above_its_indicator() {
    let root = TempRoot::new("running");
    let fake = FakeRunner::new();
    let mut h = harness(&root, &fake);
    let events = [message_ago("e1", "user", "go on", Duration::from_secs(5))];
    let (b, _) = opened(&mut h, &root, meta("running"), &events);
    assert_eq!(period(&mut h), Some(FAST));
    let shown = rows(&mut h);
    let at = shown
        .iter()
        .position(|r| r.starts_with("[session restored]"))
        .unwrap();
    // Counted from your message, as orchid keeps no start for a turn.
    let row = &shown[at + 2];
    assert!(
        row.starts_with("[Processing: 5s]") || row.starts_with("[Processing: 6s]"),
        "{row}"
    );
    assert_eq!(shown[at + 3], chat::separator(Some("die")));
    fixtures::append(
        root.path(),
        ID,
        &[fixtures::message("e2", "assistant", "late reply")],
    );
    h.advance(FAST);
    let t = text(&h, b);
    let restored = t.find("[session restored]").unwrap();
    let reply = t.find("[agnt]    [late reply").unwrap();
    assert!(
        restored < reply && reply < t.find("[Processing").unwrap(),
        "{t}"
    );
    assert_eq!(input(&h, b), "", "not in your input (B2)");
    meta("idle").write(root.path());
    h.advance(FAST);
    assert!(indicator_row(&mut h).unwrap().starts_with("[Finished: "));
}

// ------------------------------------------------------------------ following events.jsonl

#[test]
fn nothing_between_the_restore_and_the_follow_is_lost_or_shown_twice() {
    use std::io::Write;
    let root = TempRoot::new("seam");
    let fake = FakeRunner::new();
    let mut h = harness(&root, &fake);
    let events = [
        fixtures::message("e1", "user", "one"),
        fixtures::message("e2", "assistant", "two"),
    ];
    meta("idle").write(root.path());
    fixtures::append(root.path(), ID, &events);
    // orchid is halfway through writing a line.
    let line = fixtures::message("e3", "assistant", "three");
    let (first, rest) = line.split_at(line.len() / 2);
    let path = store::events_path(root.path(), ID);
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(first.as_bytes()).unwrap();
    chat::open(&mut h, ID).unwrap();
    let b = h.focused_buffer().unwrap();
    assert!(!text(&h, b).contains("[three"));
    writeln!(f, "{rest}").unwrap();
    h.advance(SLOW);
    assert_eq!(text(&h, b).matches("[agnt]    [three").count(), 1);
    // A line with an id it showed already isn't shown again.
    fixtures::append(
        root.path(),
        ID,
        &[
            events[1].clone(),
            fixtures::message("e4", "assistant", "four"),
        ],
    );
    h.advance(SLOW);
    let t = text(&h, b);
    assert_eq!(t.matches("[agnt]    [two").count(), 1);
    assert_eq!(t.matches("[agnt]    [four").count(), 1);
}

#[test]
fn an_archived_session_says_so_once_and_isnt_followed() {
    let root = TempRoot::new("archived");
    let fake = FakeRunner::new();
    let mut h = harness(&root, &fake);
    let (b, _) = opened(
        &mut h,
        &root,
        meta("idle"),
        &[fixtures::message("e1", "user", "x")],
    );
    std::fs::remove_dir_all(store::session_dir(root.path(), ID)).unwrap();
    h.advance(SLOW);
    h.advance(3 * SLOW);
    let t = text(&h, b);
    let said = format!(
        "[SYSTEM] Session {ID} is gone (archived or deleted), so this chat stopped following it"
    );
    assert_eq!(t.matches(&said).count(), 1, "{t}");
    assert_eq!(period(&mut h), None, "nothing left to poll");
    let archived: Vec<String> = log_lines(&h)
        .into_iter()
        .filter(|l| l.contains("archived"))
        .collect();
    assert_eq!(archived.len(), 1);
    let known = h
        .with_ext(|o: &mut Orchid, _| o.session(ID).is_some())
        .unwrap();
    assert!(!known);
}

#[test]
fn a_selection_stays_on_its_text_as_a_turn_goes_in_ticks_and_finishes() {
    let root = TempRoot::new("selection");
    let fake = FakeRunner::new();
    let mut h = harness(&root, &fake);
    let (b, w) = opened(
        &mut h,
        &root,
        meta("idle"),
        &[fixtures::message("e1", "user", "x")],
    );
    keys(&mut h, "Gihello world<Esc>0vllll");
    assert_eq!(h.state_of(b), State::Visual);
    let sel = |h: &Harness| h.text(b).unwrap().selection(w).unwrap();
    let selected = |h: &Harness| {
        let s = sel(h);
        let t = text(h, b);
        t[s.anchor.min(s.head)..=s.anchor.max(s.head)].to_string()
    };
    assert_eq!(selected(&h), "hello");
    // A turn from elsewhere goes in above the input: the selection stays on what it selected.
    meta("running").write(root.path());
    let sent = message_ago("e2", "user", "from emacs", Duration::ZERO);
    fixtures::append(root.path(), ID, &[sent]);
    h.advance(SLOW);
    assert!(indicator_row(&mut h).is_some());
    assert_eq!(selected(&h), "hello");
    assert_eq!(h.state_of(b), State::Visual);
    keys(&mut h, "d");
    assert_eq!(input(&h, b), " world\n", "the operator took the selection");
    // A line selection from the turn's stub down to the indicator keeps its shape and ends as
    // the line ticks.
    let t = text(&h, b);
    let stub = t.find("[user]    [from emacs").unwrap();
    let line = t.find("[Processing: ").unwrap();
    h.text_mut(b).unwrap().set_cursor(w, stub);
    keys(&mut h, "V");
    let on_line = die_core::Selection {
        head: line + 3,
        ..sel(&h)
    };
    h.text_mut(b).unwrap().set_selection(w, on_line);
    let before = sel(&h);
    assert_eq!(before.shape, die_core::Shape::Line);
    h.advance(2 * SECOND);
    assert!(
        indicator_row(&mut h)
            .unwrap()
            .starts_with("[Processing: 2s]")
    );
    assert_eq!(sel(&h), before, "both ends, the shape and the goal kept");
    assert_eq!(h.state_of(b), State::Visual);
    // The finish stretches a selection in the transcript to the end, as Emacs's
    // `(goto-char (point-max))` stretched the region.
    meta("idle").write(root.path());
    h.advance(FAST);
    assert!(indicator_row(&mut h).unwrap().starts_with("[Finished: "));
    let end = sel(&h);
    assert_eq!(h.state_of(b), State::Visual);
    assert_eq!((end.anchor, end.shape), (before.anchor, before.shape));
    let t = h.text(b).unwrap();
    assert_eq!(t.line_of(end.head), t.line_of(t.len()));
}

#[test]
fn a_gone_chat_sends_nothing_and_a_session_left_as_its_lock_is_gone() {
    let root = TempRoot::new("gone-send");
    let fake = FakeRunner::new();
    sends(&fake, &root);
    let mut h = harness(&root, &fake);
    let (b, _) = opened(
        &mut h,
        &root,
        meta("idle"),
        &[fixtures::message("e1", "user", "x")],
    );
    std::fs::remove_dir_all(store::session_dir(root.path(), ID)).unwrap();
    h.advance(SLOW);
    assert!(text(&h, b).contains("is gone"));
    let refused = keys(&mut h, "Gihello<CR>");
    assert_eq!(
        refused,
        [format!(
            "Session {ID} is gone (archived or deleted); nothing was sent"
        )]
    );
    assert!(
        fake.calls().is_empty(),
        "orchid isn't asked: {:?}",
        fake.argvs()
    );
    assert!(
        !store::session_dir(root.path(), ID).exists(),
        "so its lock doesn't make the directory again"
    );
    assert!(!text(&h, b).contains("[user]    [hello"));
    // A running session whose directory is left holding only orchid's lock, as a send or its
    // worker leaves it after a delete: gone, said once, and the log isn't filled.
    let root = TempRoot::new("gone-flock");
    let fake = FakeRunner::new();
    let mut h = harness(&root, &fake);
    let (b, _) = opened(
        &mut h,
        &root,
        meta("running"),
        &[fixtures::message("e1", "user", "x")],
    );
    let dir = store::session_dir(root.path(), ID);
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".flock"), "").unwrap();
    for _ in 0..20 {
        h.advance(FAST);
    }
    let t = text(&h, b);
    assert_eq!(t.matches("is gone (archived or deleted)").count(), 1, "{t}");
    assert_eq!(period(&mut h), None, "nothing left to poll");
    let about: Vec<String> = log_lines(&h)
        .into_iter()
        .filter(|l| l.starts_with(ID))
        .collect();
    assert_eq!(about.len(), 1, "{about:?}");
}

#[test]
fn trouble_reading_each_file_is_logged_once() {
    let root = TempRoot::new("trouble");
    let fake = FakeRunner::new();
    let mut h = harness(&root, &fake);
    opened(
        &mut h,
        &root,
        meta("running"),
        &[fixtures::message("e1", "user", "x")],
    );
    // Neither file reads: metadata.json isn't JSON, and events.jsonl is a directory.
    std::fs::write(store::metadata_path(root.path(), ID), "{").unwrap();
    let events = store::events_path(root.path(), ID);
    std::fs::remove_file(&events).unwrap();
    std::fs::create_dir(&events).unwrap();
    for _ in 0..10 {
        h.advance(FAST);
    }
    let about: Vec<String> = log_lines(&h)
        .into_iter()
        .filter(|l| l.starts_with(ID))
        .collect();
    assert_eq!(about.len(), 2, "{about:?}");
    assert!(about[0].contains("metadata.json") && about[1].contains("events.jsonl"));
}

#[test]
fn log_show_shows_the_events_a_chat_has_read_and_keeps_them_coming() {
    let root = TempRoot::new("log-show");
    let fake = FakeRunner::new();
    let mut h = harness(&root, &fake);
    let notes = h.focused_buffer().unwrap();
    let events = [
        fixtures::message("e1", "user", "hello"),
        fixtures::message("e2", "assistant", "Hi!"),
    ];
    let (chat, _) = opened(&mut h, &root, meta("idle"), &events);
    h.show(notes, die_core::ShowIn::Current).unwrap();
    // Not followed: Emacs's error.
    let args = |id: &str| die_core::Args {
        text: Some(id.into()),
        ..die_core::Args::default()
    };
    let err = h.run("orchid-log-show", &args("nope")).unwrap_err();
    assert_eq!(err.to_string(), "Not monitoring session nope");
    // Asked for, as Emacs's `Session ID: `.
    h.run("orchid-log-show", &die_core::Args::default())
        .unwrap();
    assert!(h.screen().contains("Session ID:"));
    h.type_keys(&format!("{ID}<CR>")).unwrap();
    h.run_until_idle();
    let name = chat::events_buffer(ID);
    assert_eq!(name, format!(" *orchid-log:{ID}*"));
    let log = h.find_buffer(&name).expect("the events' buffer");
    assert_eq!(text(&h, log), format!("{}\n", events.join("\n")));
    assert_eq!(h.window_list().len(), 2, "display-buffer's: beside");
    assert_eq!(h.focused_buffer(), Some(notes), "the focus stays");
    assert!(h.text(log).unwrap().settings().read_only);
    // What the chat reads next goes in too.
    let more = fixtures::message("e3", "assistant", "More.");
    fixtures::append(root.path(), ID, std::slice::from_ref(&more));
    h.advance(SLOW);
    assert_eq!(text(&h, log), format!("{}\n{more}\n", events.join("\n")));
    // Killing the chat stops it, as Emacs's monitor stopped.
    h.kill(chat);
    h.run_until_idle();
    assert_eq!(h.find_buffer(&name), None);
}

#[test]
fn a_tab_in_your_input_is_part_of_what_dot_puts_in_again() {
    let root = TempRoot::new("tab-dot");
    let fake = FakeRunner::new();
    let mut h = harness(&root, &fake);
    let (b, _) = opened(
        &mut h,
        &root,
        meta("idle"),
        &[fixtures::message("e1", "user", "x")],
    );
    let sep = chat::separator(Some("die"));
    let ends = |h: &Harness, what: &str| text(h, b).ends_with(&format!("{sep}\n{what}"));
    keys(&mut h, "Gia<Tab>b<Esc>");
    assert!(ends(&h, "a\tb\n"), "{:?}", text(&h, b));
    assert_eq!(h.register('.').map(|r| r.text.as_str()), Some("a\tb"));
    // `i`'s again, before the last character.
    keys(&mut h, "$.");
    assert!(ends(&h, "a\ta\tbb\n"), "{:?}", text(&h, b));
    keys(&mut h, "u");
    assert!(ends(&h, "a\tb\n"), "one undo takes back what `.` put in");
}

#[test]
fn a_turn_sent_from_elsewhere_shows_as_one_of_yours() {
    let root = TempRoot::new("elsewhere");
    let fake = FakeRunner::new();
    let mut h = harness(&root, &fake);
    let (b, w) = opened(
        &mut h,
        &root,
        meta("idle"),
        &[fixtures::message("e1", "user", "x")],
    );
    keys(&mut h, "idraft");
    // Emacs sends.
    meta("running").write(root.path());
    let sent = message_ago("e2", "user", "from emacs", Duration::ZERO);
    fixtures::append(root.path(), ID, &[sent]);
    h.advance(SLOW);
    let shown = rows(&mut h);
    let at = shown
        .iter()
        .position(|r| r.starts_with("[user]    [from emacs"))
        .unwrap();
    // As a send of yours lays it out: the stub, where its events go, then the indicator.
    assert_eq!(shown[at + 1], "");
    let row = &shown[at + 2];
    assert!(
        row.starts_with("[Processing: 0s]") || row.starts_with("[Processing: 1s]"),
        "{row}"
    );
    assert_eq!(shown[at + 3], chat::separator(Some("die")));
    assert_eq!(input(&h, b), "draft");
    assert_eq!(
        cursor(&h, b, w),
        text(&h, b).len(),
        "your cursor stays in your input"
    );
    assert_eq!(period(&mut h), Some(FAST));
    fixtures::append(
        root.path(),
        ID,
        &[fixtures::message("e3", "assistant", "seen here")],
    );
    h.advance(FAST);
    let t = text(&h, b);
    assert!(t.find("[agnt]    [seen here").unwrap() < t.find("[Processing").unwrap());
    meta("idle").write(root.path());
    h.advance(FAST);
    assert!(indicator_row(&mut h).unwrap().starts_with("[Finished: "));
    assert_eq!(
        cursor(&h, b, w),
        text(&h, b).len(),
        "a finish leaves your input alone"
    );
    keys(&mut h, "ed");
    assert_eq!(input(&h, b), "drafted");
}

// ------------------------------------------------------------------ a new session

/// What a create waits on: whether to make the session, or give up.
const WAIT: u8 = 0;
const MAKE: u8 = 1;
const GIVE_UP: u8 = 2;

/// orchid's part in a create, once `go` says `MAKE`: the session, made in `dir`.
fn creates(fake: &FakeRunner, root: &TempRoot, go: &Arc<AtomicU8>, id: &str, dir: &str) {
    let (root, go, id, dir) = (
        root.path().to_path_buf(),
        go.clone(),
        id.to_string(),
        dir.to_string(),
    );
    fake.on("create", move |_: &Call| {
        while go.load(Ordering::SeqCst) == WAIT {
            std::thread::sleep(Duration::from_millis(1));
        }
        if go.load(Ordering::SeqCst) == GIVE_UP {
            return Reply::error("given up");
        }
        let mut m = Meta::new(&id);
        (m.working_dir, m.agent_name, m.label) = (Some(dir.clone()), Some("Default".into()), None);
        m.write(&root);
        fixtures::append(&root, &id, &[]);
        Reply::ok(&json!({"id": id, "status": "idle", "agent": "Default"}).to_string())
    });
    fake.reply("set", Reply::ok(r#"{"updated":true}"#));
}

#[test]
fn a_new_sessions_chat_takes_what_you_type_while_orchid_makes_it() {
    let root = TempRoot::new("new");
    let fake = FakeRunner::new();
    let work = root.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let dir = work.display().to_string();
    let go = Arc::new(AtomicU8::new(WAIT));
    let new_id = "77777777-1111-2222-3333-4444444ab12c";
    creates(&fake, &root, &go, new_id, &dir);
    let mut h = harness(&root, &fake);
    chat::new_session(&mut h, "Default", &work).unwrap();
    let pending = h.focused_buffer().unwrap();
    assert_eq!(
        h.buffer_name(pending),
        Some("*orchid-chat-default-work-new*")
    );
    assert_eq!(
        text(&h, pending),
        format!("{}\n\n", chat::rule()),
        "no header yet"
    );
    keys_now(&mut h, "idraft<CR>");
    assert_eq!(h.state_of(pending), State::Insert);
    assert!(text(&h, pending).ends_with("\n\ndraft"));
    let refused = h.take_errors();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].contains("still making this session"),
        "RET waits: {refused:?}"
    );
    go.store(MAKE, Ordering::SeqCst);
    h.run_until_idle();
    assert!(h.take_errors().is_empty());
    assert!(!h.is_live(pending));
    let b = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(b), Some("*orchid-chat-default-work-ab12c*"));
    let t = text(&h, b);
    assert!(
        t.starts_with(&format!("Workspace: {dir}\nAgent: Default\n")),
        "{t}"
    );
    assert!(t.ends_with("\n\ndraft"), "{t:?}");
    assert_eq!(h.state_of(b), State::Insert, "typing goes on");
    keys(&mut h, "ed");
    assert!(text(&h, b).ends_with("\n\ndrafted"));
    let argvs: Vec<Vec<String>> = fake.argvs().into_iter().map(|a| a[2..].to_vec()).collect();
    assert_eq!(
        argvs,
        [
            vec!["create", "--agent", "Default", "--working-dir", &dir],
            vec!["set", new_id, "--label", "default-work"],
        ]
    );
    assert_eq!(fake.calls()[0].cwd.as_deref(), Some(work.as_path()));
    let tracked = h
        .with_ext(|o: &mut Orchid, ctx| o.chat_buffer(ctx, new_id))
        .unwrap();
    assert_eq!(tracked, Some(b));
}

#[test]
fn a_create_that_fails_says_so_in_the_pending_chat() {
    let root = TempRoot::new("new-fails");
    let fake = FakeRunner::new();
    fake.reply("create", Reply::error("agent nowhere has no connection"));
    let mut h = harness(&root, &fake);
    chat::new_session(&mut h, "nowhere", root.path()).unwrap();
    h.run_until_idle();
    let b = h.focused_buffer().unwrap();
    let t = text(&h, b);
    assert!(
        t.contains("[SYSTEM] Failed to create session:\nagent nowhere has no connection\n"),
        "{t}"
    );
    let tracked = h
        .with_ext(|o: &mut Orchid, _| o.tracked(b).is_some())
        .unwrap();
    assert!(!tracked, "nothing to bring back after a restart");
}

// ------------------------------------------------------------------ a restart

#[test]
fn a_restart_brings_the_chat_back_working_in_its_window() {
    let root = TempRoot::new("restart");
    let fake = FakeRunner::new();
    let mut h = harness(&root, &fake);
    let events = [
        fixtures::message("e1", "user", "hello there"),
        fixtures::message("e2", "assistant", "Hi!"),
    ];
    let (b, _) = opened(&mut h, &root, meta("idle"), &events);
    keys(&mut h, "iunsent words<Esc>");
    let before = text(&h, b);
    let session = h.session_json();
    drop(h);
    let fake = FakeRunner::new();
    sends(&fake, &root);
    let mut h = harness(&root, &fake);
    h.restore_session(&session).unwrap();
    h.run_until_idle();
    assert!(h.take_errors().is_empty(), "{:?}", h.messages);
    let b = h.find_buffer(NAME).unwrap();
    let w = h.focused().unwrap();
    assert_eq!(h.window_buffer(w), Some(b));
    assert_eq!(text(&h, b), before, "rebuilt as it was");
    assert_eq!(input(&h, b), "unsent words");
    let into = cursor(&h, b, w) - (text(&h, b).len() - "unsent words".len());
    assert_eq!(
        into,
        "unsent word".len(),
        "the cursor where it was in the input"
    );
    // A chat again: its sections, its follow, its send.
    let t = text(&h, b);
    h.text_mut(b)
        .unwrap()
        .set_cursor(w, t.find("[user]").unwrap());
    keys(&mut h, "<Tab>");
    assert!(rows(&mut h).contains(&"hello there".to_string()));
    assert_eq!(period(&mut h), Some(SLOW));
    fixtures::append(
        root.path(),
        ID,
        &[fixtures::message("e3", "assistant", "after")],
    );
    h.advance(SLOW);
    assert!(text(&h, b).contains("[agnt]    [after"));
    keys(&mut h, "G$a<CR>");
    assert_eq!(fake.argvs().pop().unwrap().last().unwrap(), "unsent words");
    let again = h.session_json();
    assert!(again.contains(r#""kind": "chat""#), "kept again");
}

#[test]
fn a_restart_brings_a_pending_chat_back_and_makes_its_session() {
    let root = TempRoot::new("restart-new");
    let fake = FakeRunner::new();
    let go = Arc::new(AtomicU8::new(WAIT));
    let new_id = "77777777-1111-2222-3333-4444444cd34e";
    creates(
        &fake,
        &root,
        &go,
        new_id,
        &root.path().display().to_string(),
    );
    let mut h = harness(&root, &fake);
    chat::new_session(&mut h, "Default", root.path()).unwrap();
    keys_now(&mut h, "ihalf<Esc>");
    let session = h.session_json();
    go.store(GIVE_UP, Ordering::SeqCst);
    drop(h);
    let fake = FakeRunner::new();
    let make = Arc::new(AtomicU8::new(MAKE));
    creates(
        &fake,
        &root,
        &make,
        new_id,
        &root.path().display().to_string(),
    );
    let mut h = harness(&root, &fake);
    h.restore_session(&session).unwrap();
    h.run_until_idle();
    assert!(h.take_errors().is_empty(), "{:?}", h.messages);
    let b = h
        .with_ext(|o: &mut Orchid, ctx| o.chat_buffer(ctx, new_id))
        .unwrap()
        .unwrap();
    assert!(text(&h, b).ends_with("\n\nhalf"), "{:?}", text(&h, b));
    assert_eq!(h.window_buffer(h.focused().unwrap()), Some(b));
    assert_eq!(fake.argvs()[0][2], "create");
}

#[test]
fn a_restart_on_a_session_thats_gone_says_so_and_keeps_your_input() {
    let root = TempRoot::new("restart-gone");
    let fake = FakeRunner::new();
    let mut h = harness(&root, &fake);
    opened(&mut h, &root, meta("idle"), &[]);
    keys(&mut h, "ikeep this<Esc>");
    let session = h.session_json();
    drop(h);
    std::fs::remove_dir_all(store::session_dir(root.path(), ID)).unwrap();
    let mut h = harness(&root, &fake);
    h.restore_session(&session).unwrap();
    h.run_until_idle();
    let b = h.find_buffer(NAME).unwrap();
    let t = text(&h, b);
    assert!(
        t.contains(&format!(
            "[SYSTEM] Session {ID} is gone (archived or deleted)"
        )),
        "{t}"
    );
    assert!(t.ends_with("\n\nkeep this"), "{t:?}");
    assert_eq!(period(&mut h), None, "not followed");
    let kept = h
        .with_ext(|o: &mut Orchid, _| o.tracked(b).map(|t| t.0))
        .unwrap();
    assert_eq!(kept, Some(Kind::Chat));
}

// ------------------------------------------------------------------ the poll

#[test]
fn no_chat_no_timer_and_a_killed_chat_stops_its_poll() {
    let root = TempRoot::new("pace");
    let fake = FakeRunner::new();
    sends(&fake, &root);
    let mut h = harness(&root, &fake);
    assert_eq!(h.next_timer(), None);
    let (b, _) = opened(&mut h, &root, meta("idle"), &[]);
    assert_eq!(period(&mut h), Some(SLOW));
    keys(&mut h, "igo<CR>");
    assert_eq!(period(&mut h), Some(FAST));
    assert!(h.kill(b));
    h.run_until_idle();
    assert_eq!(period(&mut h), None);
    assert_eq!(h.next_timer(), None, "nor the indicator's");
}
