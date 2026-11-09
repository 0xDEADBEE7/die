//! Restarts, as `:rebuild` makes them: the session handed over to a file, DIE gone, and a new DIE
//! started on the same file, twice in a row. Each orchid buffer comes back working in its window:
//! a chat rebuilt from `events.jsonl` with its unsent input, its expanded sections and each
//! window's place, following again with its indicator if its turn still runs; a chat no window
//! shows, following too; a new session's chat that orchid hadn't made yet, made, or taken over if
//! orchid made it while DIE was down; a chat whose session went while DIE was down, saying so;
//! and the browser, drawn and searching as it was. orchid is faked under a temporary root, and the
//! session file is in a temporary directory.

use die_test_cap as _;

use die_core::{Args, BufferId, Config, Harness, Side, State, WindowId};
use die_orchid_client::browser::BUFFER;
use die_orchid_client::cli::{Call, FakeRunner, Reply};
use die_orchid_client::fixtures::{self, Meta, TempRoot};
use die_orchid_client::poll::{FAST, SLOW};
use die_orchid_client::store;
use die_orchid_client::{Orchid, chat};
use die_vim::Vim;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const ID: &str = "0f1e2d3c-aaaa-bbbb-cccc-123456789abc";
const NAME: &str = "*orchid-chat-default-die-89abc*";
const SECOND: Duration = Duration::from_secs(1);

/// DIE, started on the session file in `state`, as a restart starts it.
fn up(root: &TempRoot, state: &TempRoot, fake: &FakeRunner) -> Harness {
    let orchid = Orchid::for_tests(root.path(), fake.clone());
    let config = Config {
        session: Some(state.path().to_path_buf()),
        ..Config::default()
    };
    let exts: Vec<Box<dyn die_core::Extension>> = vec![Box::new(Vim::default()), Box::new(orchid)];
    let mut h = Harness::with_config(exts, config).unwrap();
    h.resume_session();
    h.set_window_size(80, 40);
    h.run_until_idle();
    let lost: Vec<&String> = h
        .messages
        .iter()
        .filter(|m| m.contains("didn't come back") || m.contains("dropped"))
        .collect();
    assert!(lost.is_empty(), "{lost:?}");
    h
}

/// `:rebuild`: the session handed over, and DIE gone, then started again.
fn restart(h: Harness, root: &TempRoot, state: &TempRoot, fake: &FakeRunner) -> Harness {
    let mut h = h;
    h.hand_over_session().unwrap();
    drop(h);
    up(root, state, fake)
}

/// orchid's part in a send: your message in `events.jsonl`, and the session running.
fn sends(fake: &FakeRunner, root: &TempRoot) {
    let root = root.path().to_path_buf();
    fake.on("send", move |call: &Call| {
        let message = call.args.last().unwrap().to_string_lossy().into_owned();
        let now = jiff::Timestamp::now().to_string();
        let fields = json!({"type": "message", "role": "user", "content": message});
        let line = fixtures::line("sent-1", &now, fixtures::MARGINAL, fields);
        fixtures::append(&root, ID, &[line]);
        running(&root, ID, true);
        Reply::ok(&json!({"id": ID, "status": "running", "pid": 4242}).to_string())
    });
}

fn running(root: &Path, id: &str, yes: bool) {
    let mut m = Meta::new(id);
    m.status = if yes { "running" } else { "idle" }.into();
    m.write(root);
}

fn text(h: &Harness, b: BufferId) -> String {
    h.text(b).unwrap().rope().to_string()
}

fn keys(h: &mut Harness, k: &str) {
    for k in h.type_keys(k).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
}

/// The line `w`'s cursor is on.
fn line_at(h: &Harness, w: WindowId) -> String {
    let b = h.window_buffer(w).unwrap();
    let t = h.text(b).unwrap();
    let at = t.cursor(w).unwrap();
    let line = t.line_of(at);
    let (start, end) = (t.line_start(line), t.line_start(line + 1).min(t.len()));
    t.rope()
        .byte_slice(start..end)
        .to_string()
        .trim_end()
        .to_string()
}

/// How far into the input `w`'s cursor is.
fn into_input(h: &Harness, b: BufferId, w: WindowId, input: &str) -> usize {
    let t = text(h, b);
    h.text(b).unwrap().cursor(w).unwrap() - (t.len() - input.len())
}

fn rows(h: &mut Harness) -> Vec<String> {
    h.screen().lines().map(String::from).collect()
}

fn period(h: &mut Harness) -> Option<Duration> {
    h.with_ext(|o: &mut Orchid, _| o.poll.period()).unwrap()
}

/// What the indicator says, `Processing` or `Finished`, and its seconds.
fn indicator(h: &Harness, b: BufferId) -> Option<(String, u64)> {
    let t = text(h, b);
    let line = t
        .lines()
        .find(|l| l.starts_with("[Processing: ") || l.starts_with("[Finished: "))?;
    let (said, rest) = line[1..].split_once(": ")?;
    let secs = rest.split('s').next()?.parse().ok()?;
    Some((said.to_string(), secs))
}

/// Three turns, one with a file map whose files are sections of their own.
fn history() -> Vec<String> {
    let files = json!({"src/a.rs": "fn a() {}\n", "src/b.rs": "fn b() {}\n"});
    vec![
        fixtures::message("e1", "user", "first question"),
        fixtures::message("e2", "assistant", "First answer, at some length."),
        fixtures::tool_call("e3", "k1", "bash", json!({"cmd": "cargo test"})),
        fixtures::tool_result("e4", "k1", json!("test result: ok\n")),
        fixtures::tool_call(
            "e5",
            "k2",
            "fs_read",
            json!({"paths": ["src/a.rs", "src/b.rs"]}),
        ),
        fixtures::tool_result("e6", "k2", files),
        fixtures::message("e7", "user", "second question"),
        fixtures::message("e8", "assistant", "Second answer."),
    ]
}

/// Put `w`'s cursor on the `n`th line starting `start`.
fn on(h: &mut Harness, w: WindowId, start: &str, n: usize) {
    let b = h.window_buffer(w).unwrap();
    let t = text(h, b);
    let mut at = 0;
    let found = t
        .split_inclusive('\n')
        .filter_map(|l| {
            let here = at;
            at += l.len();
            l.starts_with(start).then_some(here)
        })
        .nth(n)
        .expect(start);
    h.text_mut(b).unwrap().set_cursor(w, found);
}

#[test]
fn a_chat_comes_back_twice_where_it_was_with_its_sections_input_and_turn() {
    let (root, state) = (
        TempRoot::new("restart-root"),
        TempRoot::new("restart-state"),
    );
    let fake = FakeRunner::new();
    sends(&fake, &root);
    Meta::new(ID).write(root.path());
    fixtures::append(root.path(), ID, &history());
    let mut h = up(&root, &state, &fake);
    h.open_text("notes", "").unwrap();
    chat::open(&mut h, ID).unwrap();
    h.run_until_idle();
    let b = h.focused_buffer().unwrap();
    let top = h.focused().unwrap();
    let below = h.split_window(top, Side::Below).unwrap();
    // A turn sent from here, running, with an event in.
    h.focus(below);
    keys(&mut h, "Gimy question<CR>");
    fixtures::append(
        root.path(),
        ID,
        &[fixtures::message("e9", "assistant", "Working on it.")],
    );
    h.advance(FAST);
    keys(&mut h, "unsent words<Esc>");
    // Above, the first answer and the second file read, expanded, the cursor on the answer.
    h.focus(top);
    on(&mut h, top, "[rslt]", 1);
    keys(&mut h, "<Tab>");
    on(&mut h, top, "[src/b.rs]", 0);
    keys(&mut h, "<Tab>");
    on(&mut h, top, "[agnt]", 0);
    keys(&mut h, "<Tab>");
    h.focus(below);
    let reading = line_at(&h, top);
    assert!(reading.starts_with("[agnt]    [First answer"), "{reading}");
    assert!(indicator(&h, b).is_some());
    let typed = into_input(&h, b, below, "unsent words");
    // While DIE is down, the turn goes on.
    let call = json!({"cmd": "ls -la"});
    fixtures::append(
        root.path(),
        ID,
        &[fixtures::tool_call("e10", "k3", "bash", call)],
    );
    let mut again = restart(h, &root, &state, &fake);
    for round in ["first", "second"] {
        let h = &mut again;
        let b = h
            .find_buffer(NAME)
            .unwrap_or_else(|| panic!("{round}: the chat"));
        let [top, below] = h.window_list()[..] else {
            panic!("{round}: two windows");
        };
        assert_eq!(h.window_buffer(top), Some(b), "{round}");
        assert_eq!(h.window_buffer(below), Some(b), "{round}");
        assert_eq!(h.focused(), Some(below), "{round}");
        let t = text(h, b);
        assert!(t.ends_with("\n\nunsent words"), "{round}: {t:?}");
        assert_eq!(into_input(h, b, below, "unsent words"), typed, "{round}");
        assert_eq!(
            line_at(h, top),
            reading,
            "{round}: the window reading stays on it"
        );
        let screen = rows(h).join("\n");
        assert!(
            screen.contains("Assistant: First answer, at some length."),
            "{round}:\n{screen}"
        );
        assert!(
            screen.contains("fn b() {}"),
            "{round}: the file expanded inside its result:\n{screen}"
        );
        assert!(
            !screen.contains("fn a() {}"),
            "{round}: the other one not:\n{screen}"
        );
        // Following, its turn running, and its indicator ticking.
        let (line, secs) = indicator(h, b).expect("the indicator");
        assert_eq!(line, "Processing", "{round}");
        assert!(
            t.contains("[bash]    [ls -la "),
            "{round}: what came while DIE was down: {t}"
        );
        assert_eq!(period(h), Some(FAST), "{round}");
        h.advance(SECOND * 2);
        assert!(indicator(h, b).unwrap().1 >= secs + 2, "{round}: ticking");
        assert_eq!(h.state_of(b), State::Normal);
        if round == "first" {
            again = restart(again, &root, &state, &fake);
        }
    }
    // The turn ends, and the chat sends again.
    let h = &mut again;
    let b = h.find_buffer(NAME).unwrap();
    fixtures::append(
        root.path(),
        ID,
        &[fixtures::message("e11", "assistant", "Done.")],
    );
    running(root.path(), ID, false);
    h.advance(FAST);
    let t = text(h, b);
    assert!(t.contains("[agnt]    [Done."), "{t}");
    assert_eq!(indicator(h, b).unwrap().0, "Finished");
    assert_eq!(period(h), Some(SLOW));
    keys(h, "A<CR>");
    let argv = fake.argvs().pop().unwrap();
    assert_eq!(argv[argv.len() - 2..], ["--", "unsent words"]);
    assert!(h.take_errors().is_empty(), "{:?}", h.messages);
}

const OTHER: &str = "11111111-2222-3333-4444-5555555def01";
const GONE: &str = "99999999-8888-7777-6666-555555544433";
const NEW: &str = "77777777-1111-2222-3333-4444444cd34e";

/// orchid's part in a create: the session `id`, made in `dir`.
fn creates(fake: &FakeRunner, root: &TempRoot, id: &str, dir: &Path) {
    let (root, id, dir) = (root.path().to_path_buf(), id.to_string(), dir.to_path_buf());
    fake.on("create", move |_: &Call| {
        made(&root, &id, &dir);
        Reply::ok(&json!({"id": id, "status": "idle", "agent": "default"}).to_string())
    });
    fake.reply("set", Reply::ok(r#"{"updated":true}"#));
}

/// A create that hasn't answered when DIE goes, and then makes nothing.
fn hangs(fake: &FakeRunner, go: &Arc<AtomicBool>) {
    let go = go.clone();
    fake.on("create", move |_: &Call| {
        while !go.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        Reply::error("DIE went")
    });
}

/// The session orchid makes for a new chat: no label yet, no events.
fn made(root: &Path, id: &str, dir: &Path) {
    let mut m = Meta::new(id);
    (m.working_dir, m.label) = (Some(dir.display().to_string()), None);
    m.created_at = jiff::Timestamp::now().to_string();
    m.write(root);
}

fn creates_made(fake: &FakeRunner) -> usize {
    fake.argvs().iter().filter(|a| a[2] == "create").count()
}

/// Keys, each taken in turn without waiting for a job, as a create that hasn't answered is out.
fn keys_now(h: &mut Harness, k: &str) {
    for ev in die_core::parse_keys(k).unwrap() {
        h.input(ev);
        while h.step() {}
    }
}

/// The browser's search line.
fn query(h: &Harness, browser: BufferId) -> String {
    text(h, browser)
        .lines()
        .nth(2)
        .unwrap_or_default()
        .trim_end()
        .to_string()
}

#[test]
fn the_browser_a_hidden_chat_a_new_one_and_a_gone_one_come_back_twice() {
    let (root, state) = (
        TempRoot::new("restart-all"),
        TempRoot::new("restart-all-state"),
    );
    let work = root.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let (first, go) = (FakeRunner::new(), Arc::new(AtomicBool::new(false)));
    hangs(&first, &go);
    for (id, label) in [(ID, "alpha"), (OTHER, "beta"), (GONE, "gamma")] {
        let mut m = Meta::new(id);
        m.label = Some(label.into());
        m.write(root.path());
        fixtures::append(root.path(), id, &[fixtures::message("e1", "user", "hello")]);
    }
    let mut h = up(&root, &state, &first);
    let (notes, left) = h.open_text("notes", "").unwrap();
    let right = h.split_window(left, Side::Right).unwrap();
    // The browser, searching; a chat no window shows; one on a session that goes while DIE is
    // down; and a new session's that orchid hasn't made.
    h.focus(right);
    h.run("orchid", &Args::default()).unwrap();
    h.run_until_idle();
    keys(&mut h, "/al");
    h.focus(left);
    chat::open(&mut h, OTHER).unwrap();
    chat::open(&mut h, GONE).unwrap();
    keys(&mut h, "ikept for gone<Esc>");
    h.show(notes, die_core::ShowIn::Current).unwrap();
    chat::new_session(&mut h, "default", &work).unwrap();
    keys_now(&mut h, "ifor the new one<Esc>");
    h.hand_over_session().unwrap();
    drop(h);
    go.store(true, Ordering::SeqCst);
    std::fs::remove_dir_all(store::session_dir(root.path(), GONE)).unwrap();
    let fake = FakeRunner::new();
    creates(&fake, &root, NEW, &work);
    let mut again = up(&root, &state, &fake);
    for round in ["first", "second"] {
        let h = &mut again;
        let [left, right] = h.window_list()[..] else {
            panic!("{round}: two windows")
        };
        // The browser, searching as it was.
        let browser = h.find_buffer(BUFFER).unwrap();
        assert_eq!(h.window_buffer(right), Some(browser), "{round}");
        assert_eq!(query(h, browser), "al", "{round}");
        let listed = text(h, browser);
        assert!(
            listed.contains("alpha") && !listed.contains("gamma"),
            "{round}: {listed}"
        );
        // The new session's chat, made once, with what was typed.
        let new = h.window_buffer(left).unwrap();
        assert_eq!(
            h.buffer_name(new),
            Some("*orchid-chat-default-work-cd34e*"),
            "{round}"
        );
        assert!(text(h, new).ends_with("\n\nfor the new one"), "{round}");
        assert_eq!(
            creates_made(&fake),
            1,
            "{round}: one session, not one each start"
        );
        // The chat no window shows follows its session.
        let other = h.find_buffer("*orchid-chat-default-die-def01*").unwrap();
        let line = format!("{round} reply");
        let event = fixtures::message(&format!("o-{round}"), "assistant", &line);
        fixtures::append(root.path(), OTHER, &[event]);
        h.advance(SLOW);
        assert!(
            text(h, other).contains(&format!("[agnt]    [{line}")),
            "{round}"
        );
        // The gone one says so, once, and keeps what was typed.
        let gone = h.find_buffer("*orchid-chat-default-die-44433*").unwrap();
        let t = text(h, gone);
        assert_eq!(
            t.matches("is gone (archived or deleted)").count(),
            1,
            "{round}: {t}"
        );
        assert!(t.ends_with("\n\nkept for gone"), "{round}: {t:?}");
        // The browser's keys still work, and it's still searching.
        h.focus(right);
        keys(h, "<BS>");
        assert_eq!(query(h, browser), "a", "{round}");
        keys(h, "l");
        h.focus(left);
        if round == "first" {
            again = restart(again, &root, &state, &fake);
        }
    }
    assert!(again.take_errors().is_empty(), "{:?}", again.messages);
}

/// orchid made the session while DIE restarted, so its answer never came: the chat takes it over
/// rather than make a second.
#[test]
fn a_session_orchid_made_while_die_was_down_is_the_new_chats() {
    let (root, state) = (
        TempRoot::new("restart-made"),
        TempRoot::new("restart-made-state"),
    );
    let work = root.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let (first, go) = (FakeRunner::new(), Arc::new(AtomicBool::new(false)));
    hangs(&first, &go);
    let mut h = up(&root, &state, &first);
    h.open_text("notes", "").unwrap();
    chat::new_session(&mut h, "default", &work).unwrap();
    keys_now(&mut h, "idraft<Esc>");
    h.hand_over_session().unwrap();
    // orchid makes it as DIE goes, and its answer goes with DIE.
    made(root.path(), NEW, &work);
    drop(h);
    go.store(true, Ordering::SeqCst);
    let fake = FakeRunner::new();
    fake.reply("set", Reply::ok(r#"{"updated":true}"#));
    let mut h = up(&root, &state, &fake);
    let b = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(b), Some("*orchid-chat-default-work-cd34e*"));
    assert!(text(&h, b).ends_with("\n\ndraft"));
    let argvs: Vec<String> = fake.argvs().iter().map(|a| a[2].clone()).collect();
    assert_eq!(argvs, ["set"], "no second create");
    assert!(h.take_errors().is_empty(), "{:?}", h.messages);
}
