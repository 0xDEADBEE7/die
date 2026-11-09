//! What a long chat costs, measured: a session of about 2,000 events with big tool results, opened,
//! followed while a turn streams, typed in, and kept in a session file. Ignored, as it times what it
//! does and prints the figures: `cargo test -p die-orchid-client --test perf -- --ignored
//! --nocapture`.

use die_test_cap as _;

use die_core::{Args, BufferId, Config, Harness};
use die_orchid_client::cli::{Call, FakeRunner, Reply};
use die_orchid_client::fixtures::{self, Meta, TempRoot};
use die_orchid_client::poll::FAST;
use die_orchid_client::{Orchid, chat};
use die_vim::Vim;
use serde_json::json;
use std::path::Path;
use std::time::{Duration, Instant};

const ID: &str = "0f1e2d3c-aaaa-bbbb-cccc-123456789abc";

/// About `n` events in turns: your message, the agent's words, a bash call and its output (one in
/// ten 40 KB, the rest 1 KB), a read and its file (20 KB), and the agent's answer.
fn long_session(prefix: &str, n: usize) -> Vec<String> {
    let mut out = vec![];
    let mut turn = 0;
    while out.len() < n {
        let e = |k: &str| format!("{prefix}t{turn}-{k}");
        let big = turn % 10 == 0;
        let output = "test result: ok. 57 passed; 0 failed\n".repeat(if big { 1100 } else { 28 });
        let file = "fn main() { println!(\"hello\"); }\n".repeat(600);
        out.push(fixtures::message(
            &e("u"),
            "user",
            &format!("turn {turn}: run the tests and read main.rs"),
        ));
        out.push(fixtures::message(
            &e("a1"),
            "assistant",
            &"I'll run the tests first. ".repeat(80),
        ));
        out.push(fixtures::tool_call(
            &e("c1"),
            &e("k1"),
            "bash",
            json!({"cmd": "cargo test"}),
        ));
        out.push(fixtures::tool_result(&e("r1"), &e("k1"), json!(output)));
        out.push(fixtures::tool_call(
            &e("c2"),
            &e("k2"),
            "fs_read",
            json!({"paths": ["src/main.rs"]}),
        ));
        out.push(fixtures::tool_result(
            &e("r2"),
            &e("k2"),
            json!({"src/main.rs": file}),
        ));
        out.push(fixtures::message(
            &e("a2"),
            "assistant",
            &"All good. ".repeat(40),
        ));
        turn += 1;
    }
    out
}

fn harness(root: &Path, fake: &FakeRunner, session: Option<&Path>) -> Harness {
    let orchid = Orchid::for_tests(root, fake.clone());
    let config = Config {
        session: session.map(Path::to_path_buf),
        ..Config::default()
    };
    let mut h =
        Harness::with_config(vec![Box::new(Vim::default()), Box::new(orchid)], config).unwrap();
    h.open_text("notes", "").unwrap();
    h.set_window_size(80, 40);
    h
}

fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1000.0)
}

fn decorations(h: &Harness, b: BufferId) -> usize {
    h.text(b).unwrap().decorations().count()
}

#[test]
#[ignore = "measures; prints its figures"]
fn a_long_chat_costs() {
    let root = TempRoot::new("perf");
    let state = TempRoot::new("perf-state");
    let fake = FakeRunner::new();
    let at = root.path().to_path_buf();
    fake.on("send", move |_: &Call| {
        let mut m = Meta::new(ID);
        m.status = "running".into();
        m.write(&at);
        Reply::ok(&json!({"id": ID, "status": "running", "pid": 4242}).to_string())
    });
    let events = long_session("", 2000);
    Meta::new(ID).write(root.path());
    fixtures::append(root.path(), ID, &events);
    let size = std::fs::metadata(die_orchid_client::store::events_path(root.path(), ID))
        .unwrap()
        .len();
    println!("events.jsonl: {} events, {} KB", events.len(), size / 1024);

    let mut h = harness(root.path(), &fake, Some(state.path()));
    h.resume_session();
    let began = Instant::now();
    chat::open(&mut h, ID).unwrap();
    h.run_until_idle();
    h.screen();
    println!("open: {}", ms(began.elapsed()));
    let b = h.focused_buffer().unwrap();
    println!(
        "chat: {} KB of text, {} decorations",
        h.text(b).unwrap().len() / 1024,
        decorations(&h, b)
    );
    let began = Instant::now();
    let json = h.session_json();
    println!(
        "session.json: {} KB, made in {}",
        json.len() / 1024,
        ms(began.elapsed())
    );

    // Typing in the input, idle.
    h.type_keys("Gi").unwrap();
    let began = Instant::now();
    for _ in 0..50 {
        h.type_keys("x").unwrap();
    }
    println!("a key, idle: {}", ms(began.elapsed() / 50));

    // A turn: sent, then events stream in, one every 2 s, while you type between them.
    h.type_keys("<CR>").unwrap();
    h.type_keys("next").unwrap();
    let file = state.path().join("session.json");
    h.flush_session();
    let mut last = std::fs::read(&file).unwrap_or_default();
    let (mut writes, mut written) = (0, 0);
    let (mut stream, mut keys, mut n) = (Duration::ZERO, Duration::ZERO, 0u32);
    let turn = long_session("live-", 140);
    for line in turn.iter().take(120) {
        fixtures::append(root.path(), ID, std::slice::from_ref(line));
        let began = Instant::now();
        h.advance(FAST);
        stream += began.elapsed();
        let began = Instant::now();
        h.type_keys("y").unwrap();
        keys += began.elapsed();
        n += 1;
        for _ in 0..3 {
            h.advance(FAST);
            h.flush_session();
            let now = std::fs::read(&file).unwrap_or_default();
            if now != last {
                writes += 1;
                written += now.len();
                last = now;
            }
        }
    }
    let secs = n as f64 * 2.0;
    println!("a streamed event: {}", ms(stream / n));
    println!("a key while streaming: {}", ms(keys / n));
    println!(
        "session writes while streaming: {writes} in {secs} s, {} KB in all, the last {} KB",
        written / 1024,
        last.len() / 1024
    );
    println!(
        "chat now: {} KB of text, {} decorations",
        h.text(b).unwrap().len() / 1024,
        decorations(&h, b)
    );

    // Everything loaded, in two windows: what a key and a look at the session cost then.
    for _ in 0..8 {
        h.run("orchid-chat-load-more-history", &Args::default())
            .unwrap();
    }
    let w = h.focused().unwrap();
    h.split_window(w, die_core::Side::Below).unwrap();
    let began = Instant::now();
    let json = h.session_json();
    println!(
        "all loaded: {} KB of text, {} decorations; session.json {} KB, made in {}",
        h.text(b).unwrap().len() / 1024,
        decorations(&h, b),
        json.len() / 1024,
        ms(began.elapsed())
    );
    let began = Instant::now();
    for _ in 0..20 {
        h.type_keys("z").unwrap();
    }
    println!("a key, all loaded: {}", ms(began.elapsed() / 20));
    assert!(h.take_errors().is_empty(), "{:?}", h.messages);
}

/// What the browser's scan costs, every 2 s while a window shows it, with 500 sessions.
#[test]
#[ignore = "measures; prints its figures"]
fn a_scan_of_500_sessions() {
    let root = TempRoot::new("perf-scan");
    for n in 0..500 {
        Meta::new(&format!("{n:08}-aaaa-bbbb-cccc-123456789abc")).write(root.path());
    }
    let began = Instant::now();
    let found = die_orchid_client::store::scan(root.path());
    assert_eq!(found.len(), 500);
    println!("a scan of 500 sessions: {}", ms(began.elapsed()));
}

/// How often an idle DIE wakes: with no orchid buffer, and with an idle chat open.
#[test]
#[ignore = "measures; prints its figures"]
fn idle_wakes() {
    let root = TempRoot::new("perf-idle");
    let state = TempRoot::new("perf-idle-state");
    let fake = FakeRunner::new();
    Meta::new(ID).write(root.path());
    fixtures::append(root.path(), ID, &long_session("", 20));
    let mut h = harness(root.path(), &fake, Some(state.path()));
    h.resume_session();
    let wakes = |h: &mut Harness, over: Duration| {
        let end = h.now() + over;
        let mut n = 0;
        while let Some(at) = h.next_timer().filter(|at| *at <= end) {
            let d = at.saturating_sub(h.now());
            h.advance(d.max(Duration::from_millis(1)));
            n += 1;
        }
        let rest = end.saturating_sub(h.now());
        h.advance(rest);
        n
    };
    wakes(&mut h, Duration::from_secs(60));
    println!(
        "no orchid buffer: {} wakes a minute",
        wakes(&mut h, Duration::from_secs(60))
    );
    chat::open(&mut h, ID).unwrap();
    h.run_until_idle();
    wakes(&mut h, Duration::from_secs(60));
    println!(
        "an idle chat open: {} wakes a minute",
        wakes(&mut h, Duration::from_secs(60))
    );
}
