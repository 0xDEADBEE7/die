//! The extension in a harness, on hand-written sessions under a temporary root, with orchid
//! faked: what registration does and doesn't do, the registry and its news, the commands this
//! package builds, `*Orchid Log*`, and what a restart keeps.

use die_test_cap as _;

use die_core::session::json;
use die_core::{Args, BufferId, Harness, Origin};
use die_orchid_client::cli::{FakeRunner, Reply};
use die_orchid_client::fixtures::{Meta, TempRoot};
use die_orchid_client::store::Status;
use die_orchid_client::{Kind, LIST_BUFFER, Orchid, StatusChange, log, refresh_sessions};
use die_vim::Vim;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Every command of the Emacs client's, from its `(interactive)` defuns, but for `M-p` and `M-n`'s
/// `orchid-chat-previous-input` and `orchid-chat-next-input`, which did nothing (call 903).
const COMMANDS: [&str; 32] = [
    "orchid",
    "orchid-open-session",
    "orchid-new-session",
    "orchid-list-sessions",
    "orchid-check-cli",
    "orchid-log-show",
    "orchid-session-browser-show",
    "orchid-session-browser-execute",
    "orchid-session-browser-mark-for-deletion",
    "orchid-session-browser-mark-for-kill",
    "orchid-session-browser-move-down",
    "orchid-session-browser-move-up",
    "orchid-session-browser-new",
    "orchid-session-browser-page-down",
    "orchid-session-browser-page-up",
    "orchid-session-browser-quit",
    "orchid-session-browser-refresh",
    "orchid-session-browser-search",
    "orchid-session-browser-search-backspace",
    "orchid-session-browser-search-clear",
    "orchid-session-browser-search-exit",
    "orchid-session-browser-search-self-insert",
    "orchid-session-browser-select",
    "orchid-session-browser-unmark",
    "orchid-chat-close",
    "orchid-chat-handle-tab",
    "orchid-chat-load-more-history",
    "orchid-chat-newline",
    "orchid-chat-send-input",
    "orchid-chat-show-session-browser",
    "orchid-collapsible-report-stats",
    "orchid-collapsible-toggle-at-point",
];

fn harness_with(orchid: Orchid) -> (Harness, BufferId) {
    let mut h = Harness::new(vec![Box::new(Vim::default()), Box::new(orchid)]).unwrap();
    let (buf, _) = h.open_text("notes", "").unwrap();
    (h, buf)
}

fn harness(root: &TempRoot, fake: &FakeRunner) -> (Harness, BufferId) {
    harness_with(Orchid::for_tests(root.path(), fake.clone()))
}

/// Run a command as `M-x` does, then everything it started.
fn run(h: &mut Harness, name: &str, text: Option<&str>) {
    let args = Args {
        text: text.map(String::from),
        ..Args::default()
    };
    h.run(name, &args).unwrap();
    h.run_until_idle();
}

fn last(h: &Harness) -> &str {
    h.messages.last().map_or("", String::as_str)
}

fn text(h: &Harness, buf: BufferId) -> String {
    h.text(buf).unwrap().rope().to_string()
}

#[test]
fn registering_reads_nothing_runs_nothing_and_sets_no_timer() {
    let root = TempRoot::new("register");
    let missing = root.path().join("not-yet");
    let fake = FakeRunner::new();
    let (mut h, _) = harness_with(Orchid::for_tests(&missing, fake.clone()));
    h.run_until_idle();
    for name in COMMANDS {
        assert_eq!(h.command_doc(name).map(|d| d.1), Some("orchid"), "{name}");
    }
    assert!(fake.calls().is_empty());
    assert!(!missing.exists(), "the root isn't made, or read");
    assert_eq!(h.next_timer(), None, "no work while idle");
    assert!(h.take_errors().is_empty());
}

#[test]
fn check_cli_says_what_it_found_and_wants_0_2() {
    let root = TempRoot::new("check");
    let fake = FakeRunner::new();
    let (mut h, _) = harness(&root, &fake);
    fake.reply(
        "--version",
        Reply::ok("orchid 0.2.2 (codex compatibility 1)"),
    );
    run(&mut h, "orchid-check-cli", None);
    assert_eq!(
        last(&h),
        "orchid CLI found: orchid 0.2.2 (codex compatibility 1)"
    );
    let root_arg = root.path().display().to_string();
    assert_eq!(
        fake.argvs(),
        [vec!["--config".to_string(), root_arg, "--version".into()]]
    );
    fake.reply("--version", Reply::ok("orchid 0.1.9"));
    run(&mut h, "orchid-check-cli", None);
    assert_eq!(
        last(&h),
        "orchid CLI found: orchid 0.1.9, but this client needs orchid 0.2 or later"
    );
    fake.reply("--version", Reply::error("no"));
    run(&mut h, "orchid-check-cli", None);
    assert_eq!(last(&h), "orchid CLI found but version check failed");
    fake.set_missing(true);
    run(&mut h, "orchid-check-cli", None);
    assert_eq!(last(&h), "orchid CLI not found in PATH. Please install it.");
    assert_eq!(fake.calls().len(), 3, "nothing runs when it isn't there");
}

#[test]
fn the_registry_tells_its_watchers_of_each_status_change() {
    let root = TempRoot::new("news");
    let fake = FakeRunner::new();
    let (mut h, _) = harness(&root, &fake);
    let heard: Rc<RefCell<Vec<StatusChange>>> = Rc::default();
    let into = heard.clone();
    let watch = h
        .with_ext(|o: &mut Orchid, _| {
            o.on_status_change(move |_, c| {
                into.borrow_mut().push(c.clone());
                Ok(())
            })
        })
        .unwrap();
    let change = |id: &str, was, now| StatusChange {
        id: id.into(),
        was,
        now,
    };
    Meta::new("a").write(root.path());
    let mut b = Meta::new("b");
    b.status = "running".into();
    b.write(root.path());
    let refreshed = Rc::new(RefCell::new(false));
    let seen = refreshed.clone();
    let heard_first = heard.clone();
    refresh_sessions(&mut h, move |_| {
        // The news came first.
        *seen.borrow_mut() = heard_first.borrow().len() == 2;
        Ok(())
    })
    .unwrap()
    .detach();
    h.run_until_idle();
    assert!(*refreshed.borrow());
    assert_eq!(
        heard.take(),
        [
            change("a", None, Some(Status::Idle)),
            change("b", None, Some(Status::Running))
        ]
    );
    // The same status again is no news; a new one is.
    let mut a = h
        .with_ext(|o: &mut Orchid, _| o.session("a").cloned())
        .unwrap()
        .unwrap();
    a.label = Some("renamed".into());
    h.with_ext(|o: &mut Orchid, ctx| o.update_session(ctx, a.clone()))
        .unwrap();
    h.run_until_idle();
    assert!(heard.borrow().is_empty());
    a.status = Status::Running;
    h.with_ext(|o: &mut Orchid, ctx| o.update_session(ctx, a))
        .unwrap();
    h.run_until_idle();
    assert_eq!(
        heard.take(),
        [change("a", Some(Status::Idle), Some(Status::Running))]
    );
    // A session archived is gone, and a scan says what the files say.
    std::fs::remove_dir_all(root.path().join("sessions/b")).unwrap();
    refresh_sessions(&mut h, |_| Ok(())).unwrap().detach();
    h.run_until_idle();
    let gone = change("b", Some(Status::Running), None);
    assert_eq!(
        heard.take(),
        [gone, change("a", Some(Status::Running), Some(Status::Idle))]
    );
    let names = h.with_ext(|o: &mut Orchid, _| o.sessions().keys().cloned().collect::<Vec<_>>());
    assert_eq!(names.unwrap(), ["a"]);
    // Unsubscribed.
    drop(watch);
    h.with_ext(|o: &mut Orchid, ctx| o.forget_session(ctx, "a"))
        .unwrap();
    h.run_until_idle();
    assert!(heard.borrow().is_empty());
    assert!(
        fake.calls().is_empty(),
        "the registry reads files, not the CLI"
    );
}

#[test]
fn open_session_finds_one_by_label_too() {
    let root = TempRoot::new("open");
    let fake = FakeRunner::new();
    let (mut h, notes) = harness(&root, &fake);
    let mut m = Meta::new("0f1e2d3c-aaaa-bbbb-cccc-123456789abc");
    m.label = Some("my-label".into());
    m.write(root.path());
    run(&mut h, "orchid-open-session", Some("my-label"));
    let chat = h.focused_buffer().unwrap();
    assert_eq!(
        h.buffer_name(chat),
        Some("*orchid-chat-default-die-89abc*"),
        "{:?}",
        h.messages
    );
    assert_eq!(last(&h), "Opened session: my-label");
    // Asked for, by id: the chat open on it.
    h.show(notes, die_core::ShowIn::Current).unwrap();
    h.run("orchid-open-session", &Args::default()).unwrap();
    let screen = h.screen();
    assert!(screen.contains("Session ID or label:"), "{screen}");
    h.type_keys("0f1e2d3c-aaaa-bbbb-cccc-123456789abc<CR>")
        .unwrap();
    h.run_until_idle();
    assert_eq!(h.focused_buffer(), Some(chat));
    assert_eq!(
        last(&h),
        "Opened session: 0f1e2d3c-aaaa-bbbb-cccc-123456789abc",
        "what you typed, as Emacs said it"
    );
    run(&mut h, "orchid-open-session", Some("nope"));
    assert!(h.take_errors().is_empty());
    assert_eq!(last(&h), "Failed to open session: Session 'nope' not found");
}

#[test]
fn new_session_offers_the_valid_agents_and_starts_here() {
    let root = TempRoot::new("new");
    let fake = FakeRunner::new();
    let (mut h, notes) = harness(&root, &fake);
    h.set_directory(notes, root.path());
    let agents = json!({"agents": [
        {"name": "default", "prompt": "default", "connection": "echo", "valid": true},
        {"name": "broken", "prompt": "gone", "connection": "nowhere", "valid": false, "error": "no prompt gone"},
        {"name": "developer", "prompt": "dev", "connection": "echo", "valid": true},
    ]});
    fake.reply("agent", Reply::ok(&agents.to_string()));
    run(&mut h, "orchid-new-session", None);
    assert_eq!(h.prompt_candidates(), ["default", "developer"]);
    let screen = h.screen();
    assert!(screen.contains("Agent:"), "{screen}");
    h.type_keys("dev<CR>").unwrap();
    h.run_until_idle();
    let dir = root.path().display().to_string();
    let create = fake.argvs().pop().unwrap();
    assert_eq!(
        create[2..],
        ["create", "--agent", "developer", "--working-dir", &dir]
    );
    let pending = h.buffer_name(h.focused_buffer().unwrap()).unwrap();
    assert!(
        pending.starts_with("*orchid-chat-developer-") && pending.ends_with("-new*"),
        "{pending}"
    );
    h.take_errors();
    fake.reply("agent", Reply::ok(r#"{"agents":[]}"#));
    run(&mut h, "orchid-new-session", None);
    assert_eq!(h.prompt_text(), None);
    assert_eq!(
        h.take_errors(),
        ["No agents available; check `orchid agent' and your CLI configuration"]
    );
}

/// `orchid agent` answers while you're typing in a prompt of your own: yours stays, and `Agent: `
/// comes once it's closed, as Emacs's synchronous call never took a prompt from you.
#[test]
fn the_agent_prompt_waits_for_one_you_opened_meanwhile() {
    let root = TempRoot::new("agent-wait");
    let fake = FakeRunner::new();
    let (mut h, notes) = harness(&root, &fake);
    h.set_directory(notes, root.path());
    let go = Arc::new(AtomicBool::new(false));
    let gate = go.clone();
    let agents = json!({"agents": [{"name": "default", "valid": true}]}).to_string();
    fake.on("agent", move |_| {
        while !gate.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Reply::ok(&agents)
    });
    h.run("orchid-new-session", &Args::default()).unwrap();
    for ev in die_core::parse_keys(":ech").unwrap() {
        h.input(ev);
        while h.step() {}
    }
    assert_eq!(h.prompt_text(), Some("ech"));
    go.store(true, Ordering::SeqCst);
    h.run_until_idle();
    h.advance(std::time::Duration::from_secs(1));
    assert_eq!(h.prompt_text(), Some("ech"), "what you were typing stays");
    assert!(h.take_errors().is_empty());
    h.type_keys("<Esc>").unwrap();
    assert_eq!(h.prompt_text(), None);
    h.advance(std::time::Duration::from_millis(200));
    assert_eq!(h.prompt_candidates(), ["default"], "then it asks");
    assert!(h.screen().contains("Agent:"));
    h.type_keys("default<CR>").unwrap();
    h.run_until_idle();
    let create = fake.argvs().pop().unwrap();
    assert_eq!(create[2..4], ["create", "--agent"]);
}

#[test]
fn list_sessions_lists_them_oldest_first_in_a_buffer_of_its_own() {
    let root = TempRoot::new("list");
    let fake = FakeRunner::new();
    let (mut h, notes) = harness(&root, &fake);
    run(&mut h, "orchid-list-sessions", None);
    assert_eq!(last(&h), "No sessions found");
    let mut newer = Meta::new("s-new");
    newer.label = Some("newer".into());
    newer.created_at = "2026-07-23T00:00:00Z".into();
    newer.write(root.path());
    let mut older = Meta::new("s-old");
    older.label = Some("older".into());
    older.write(root.path());
    run(&mut h, "orchid-list-sessions", None);
    let buf = h.find_buffer(LIST_BUFFER).unwrap();
    assert_eq!(
        text(&h, buf),
        "Available sessions:\n  older (s-old)\n  newer (s-new)\n"
    );
    assert_eq!(last(&h), "2 sessions, in *orchid-list-sessions*");
    // display-buffer's: below, and the focus stays.
    assert_eq!(h.window_list().len(), 2);
    assert_eq!(h.focused_buffer(), Some(notes));
}

#[test]
fn the_log_keeps_its_last_lines() {
    let root = TempRoot::new("log");
    let mut orchid = Orchid::for_tests(root.path(), FakeRunner::new());
    orchid.options.log_lines = 3;
    let (mut h, notes) = harness_with(orchid);
    assert_eq!(h.find_buffer(log::BUFFER), None, "made at the first line");
    for i in 1..=5 {
        log::log(&mut h, &format!("line {i}")).unwrap();
    }
    let buf = h.find_buffer(log::BUFFER).unwrap();
    let lines: Vec<String> = text(&h, buf).lines().map(String::from).collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    for (line, n) in lines.iter().zip(3..) {
        let (time, said) = line.split_at(11);
        assert!(
            time.starts_with('[') && time.ends_with("] ") && time.as_bytes()[3] == b':',
            "{line}"
        );
        assert_eq!(said, format!("line {n}"));
    }
    let t = h.text(buf).unwrap();
    assert!(!t.modified() && t.settings().read_only);
    assert!(
        !h.session_json().contains("Orchid Log"),
        "not kept over a restart"
    );
    // A user edit is refused; the log's own goes in.
    let edit = h
        .text_mut(buf)
        .unwrap()
        .tx(Origin::User)
        .insert(0, "x")
        .commit();
    assert!(edit.is_err());
    // It isn't shown: you go to it as to any buffer, as in Emacs.
    assert_eq!(h.window_list().len(), 1);
    assert_eq!(h.focused_buffer(), Some(notes));
}

#[test]
fn a_restart_keeps_each_tracked_buffer_by_kind_until_its_killed() {
    let root = TempRoot::new("kept");
    let fake = FakeRunner::new();
    let (mut h, chat) = harness(&root, &fake);
    let browser = h.create_text("*orchid-sessions*", "");
    let typed = h
        .text_mut(chat)
        .unwrap()
        .tx(Origin::User)
        .insert(0, "draft")
        .commit();
    typed.unwrap();
    h.with_ext(|o: &mut Orchid, _| {
        // A chat keeps what the buffer has when the session looks, as its unsent input.
        let input = |ctx: &die_core::Ctx, buf, state: &serde_json::Value| {
            let text = ctx.text(buf).ok()?.rope().to_string();
            Some(json!({"id": state["id"], "input": text}))
        };
        o.track_with(chat, Kind::Chat, json!({"id": "s1"}), input);
        o.track(browser, Kind::Browser, json!({}));
    })
    .unwrap();
    let found =
        h.with_ext(|o: &mut Orchid, ctx| (o.chat_buffer(ctx, "s1"), o.chat_buffer(ctx, "s2")));
    assert_eq!(found.unwrap(), (Some(chat), None));
    let session = h.session_json();
    let kept = |name: &str| {
        let parsed: serde_json::Value = serde_json::from_str(&session).unwrap();
        let buffers = parsed["buffers"].as_array().unwrap().clone();
        let b = buffers.into_iter().find(|b| b["name"] == name);
        b.map(|b| b["extensions"]["orchid"].clone())
    };
    assert_eq!(
        kept("notes"),
        Some(json!({"kind": "chat", "state": {"id": "s1", "input": "draft"}}))
    );
    assert_eq!(
        kept("*orchid-sessions*"),
        Some(json!({"kind": "browser", "state": {}}))
    );
    // It comes back through the restore with no error, into another workspace.
    let (mut again, _) = harness(&root, &fake);
    again.restore_session(&session).unwrap();
    again.run_until_idle();
    assert!(again.take_errors().is_empty());
    assert!(
        !again
            .messages
            .iter()
            .any(|m| m.contains("didn't come back")),
        "{:?}",
        again.messages
    );
    // Killed, it's forgotten.
    assert!(h.kill(chat));
    h.run_until_idle();
    let tracked = h
        .with_ext(|o: &mut Orchid, ctx| (o.tracked(chat).is_some(), o.buffers(ctx, Kind::Browser)));
    assert_eq!(tracked.unwrap(), (false, vec![browser]));
    assert!(!h.session_json().contains(r#""kind": "chat""#));
}

#[test]
fn yes_or_no_wants_the_word_typed_out() {
    let root = TempRoot::new("yes");
    let (mut h, _) = harness(&root, &FakeRunner::new());
    let said: Rc<RefCell<Vec<bool>>> = Rc::default();
    let ask = |h: &mut Harness| {
        let into = said.clone();
        die_orchid_client::yes_or_no(h, "Execute 2 marked operation(s)? ", move |_, yes| {
            into.borrow_mut().push(yes);
            Ok(())
        });
    };
    ask(&mut h);
    let screen = h.screen();
    assert!(
        screen.contains("Execute 2 marked operation(s)? (yes or no)"),
        "{screen}"
    );
    // A bare RET, a `y`, and a `yes` with a space before it are no answer: it asks again, as
    // `yes-or-no-p` does, which doesn't trim.
    for keys in ["<CR>", "y<CR>", " yes<CR>", "yes <CR>"] {
        h.type_keys(keys).unwrap();
        assert!(said.borrow().is_empty(), "{keys}");
        assert_eq!(last(&h), "Please answer yes or no.");
        assert_eq!(h.prompt_text(), Some(""), "{keys}");
    }
    h.type_keys("yes<CR>").unwrap();
    assert_eq!(*said.borrow(), [true]);
    ask(&mut h);
    h.type_keys("no<CR>").unwrap();
    assert_eq!(*said.borrow(), [true, false]);
    // In any case, as `yes-or-no-p` downcases the answer.
    for (keys, yes) in [("YES<CR>", true), ("Yes<CR>", true), ("No<CR>", false)] {
        ask(&mut h);
        h.type_keys(keys).unwrap();
        assert_eq!(said.borrow().last(), Some(&yes), "{keys}");
    }
    said.borrow_mut().truncate(2);
    // A cancel answers nothing.
    ask(&mut h);
    h.type_keys("<Esc>").unwrap();
    assert_eq!(h.prompt_text(), None);
    assert_eq!(*said.borrow(), [true, false]);
}
