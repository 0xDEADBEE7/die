//! A chat on the real orchid, offline: a new session made through the chat, `hello` sent from it,
//! the turn followed to its end from the files orchid writes, and a restart that brings the chat
//! back from them, as `:rebuild` does. Ignored, since it needs the
//! binary: `ORCHID_BIN=~/.local/bin/orchid cargo test -p die-orchid-client --test real_chat --
//! --ignored`. As `tests/orchid.rs` does, it runs in a process of its own with a HOME of its own
//! and no `ORCHID_` variables, on a root of its own, with orchid's built-in `echo` connection: no
//! network, no keys, and never your root.

use die_test_cap as _;

use die_core::{Config, Harness};
use die_orchid_client::cli::{Cmd, FakeRunner, ProcessRunner};
use die_orchid_client::fixtures::TempRoot;
use die_orchid_client::poll::FAST;
use die_orchid_client::store::{self, Status};
use die_orchid_client::{Orchid, chat};
use die_vim::Vim;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CHILD: &str = "DIE_ORCHID_TEST_REAL_CHAT_CHILD";

#[test]
#[ignore = "needs orchid: ORCHID_BIN=path/to/orchid"]
fn a_chat_talks_to_orchid_0_2_2() {
    let Some(bin) = std::env::var_os("ORCHID_BIN") else {
        panic!("ORCHID_BIN names no orchid");
    };
    if std::env::var_os(CHILD).is_none() {
        let home = TempRoot::new("real-chat-home");
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "a_chat_talks_to_orchid_0_2_2",
            "--ignored",
            "--nocapture",
        ]);
        cmd.env(CHILD, "1")
            .env("HOME", home.path())
            .env("ORCHID_BIN", &bin);
        for name in die_orchid_client::cli::SCRUBBED {
            cmd.env_remove(name);
        }
        let out = cmd.stdin(Stdio::null()).output().unwrap();
        let said = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success() && said.contains("1 passed"), "{said}");
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let root = home.join("root");
    let work = home.join("work");
    std::fs::create_dir_all(root.join("agents")).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    let work = work.canonicalize().unwrap();
    std::fs::write(
        root.join("agents/echo.json"),
        r#"{"prompt":"default","connection":"echo"}"#,
    )
    .unwrap();
    let bin = PathBuf::from(bin);
    let path = std::env::join_paths([
        bin.parent().unwrap().into(),
        PathBuf::from("/bin"),
        "/usr/bin".into(),
    ]);
    let runner = ProcessRunner {
        path: Some(path.unwrap()),
        env: vec![],
    };
    let state = home.join("state");
    let up = || {
        let mut orchid = Orchid::for_tests(&root, FakeRunner::new()).with_runner(runner.clone());
        orchid.options.program = bin.clone();
        let config = Config {
            session: Some(state.clone()),
            ..Config::default()
        };
        let exts: Vec<Box<dyn die_core::Extension>> =
            vec![Box::new(Vim::default()), Box::new(orchid)];
        let mut h = Harness::with_config(exts, config).unwrap();
        h.resume_session();
        h.set_window_size(80, 40);
        h
    };
    let mut h = up();
    h.open_text("notes", "").unwrap();

    // A new session's chat, then the session's.
    chat::new_session(&mut h, "echo", &work).unwrap();
    h.run_until_idle();
    assert!(h.take_errors().is_empty(), "{:?}", h.messages);
    let b = h.focused_buffer().unwrap();
    let name = h.buffer_name(b).unwrap().to_string();
    assert!(
        name.starts_with("*orchid-chat-echo-work-") && !name.ends_with("-new*"),
        "{name}"
    );
    let ids: Vec<String> = store::scan(&root).into_iter().map(|m| m.id).collect();
    assert_eq!(ids.len(), 1);
    let id = ids[0].clone();
    let meta = store::read_metadata(&root, &id).unwrap();
    assert_eq!(meta.label.as_deref(), Some("echo-work"), "set --label ran");

    // A message from the chat, and orchid's echo of it, followed in.
    h.type_keys("ihello<CR>").unwrap();
    let start = Instant::now();
    let text = |h: &Harness| h.text(b).unwrap().rope().to_string();
    while !text(&h).contains("[Finished: ") {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the turn never finished:\n{}",
            text(&h)
        );
        std::thread::sleep(Duration::from_millis(50));
        h.advance(FAST);
    }
    assert!(h.take_errors().is_empty(), "{:?}", h.messages);
    let t = text(&h);
    let user = t.find("[user]    [hello").expect(&t);
    let reply = t.find("[agnt]    [hello").expect(&t);
    let finished = t.find("[Finished: ").unwrap();
    assert!(user < reply && reply < finished, "{t}");
    assert_eq!(t.matches("[user]    [hello").count(), 1, "shown once");
    let meta = store::read_metadata(&root, &id).unwrap();
    assert_eq!(meta.status, Status::Idle);

    // A restart: the chat comes back from what orchid wrote, in its window, and follows again.
    h.type_keys("unsent<Esc>").unwrap();
    h.hand_over_session().unwrap();
    drop(h);
    let mut h = up();
    h.run_until_idle();
    assert!(h.take_errors().is_empty(), "{:?}", h.messages);
    let b = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(b), Some(name.as_str()));
    let text = |h: &Harness| h.text(b).unwrap().rope().to_string();
    let t = text(&h);
    assert!(
        t.contains("[user]    [hello") && t.contains("[agnt]    [hello"),
        "{t}"
    );
    assert!(
        t.contains("[session restored]") && t.ends_with("\n\nunsent"),
        "{t:?}"
    );

    // Done: archive it, which the chat says once.
    let cli = die_orchid_client::cli(&mut h).unwrap();
    assert_eq!(
        cli.run_now(&Cmd::Delete { id: id.clone() }, None).unwrap()["archived"],
        true
    );
    h.advance(Duration::from_secs(2));
    assert_eq!(text(&h).matches("is gone (archived or deleted)").count(), 1);
}
