//! A chat on a stand-in for orchid, `tests/fake-orchid`, a POSIX sh script run by its absolute path
//! through the real runner, as your orchid is run: a new session through `orchid-new-session` and
//! its `Agent: ` prompt, a message sent and its echo followed in until the indicator finishes, a
//! send orchid refuses, a `/kill` that hangs until the timeout stops it and what it started, and the
//! session archived. Along the way: the PATH the child gets, stdout apart from stderr, exit codes,
//! and the `ORCHID_` variables an orchid tool that started DIE would leave, which the child never
//! sees. It runs in a process of its own, with a HOME of its own and those variables set, since
//! the environment is the process's. Neither your orchid nor your login shell runs.

use die_test_cap as _;

use die_core::{Args, Harness};
use die_orchid_client::cli::{Cmd, FakeRunner, ProcessRunner};
use die_orchid_client::fixtures::TempRoot;
use die_orchid_client::poll::{FAST, SLOW};
use die_orchid_client::store;
use die_orchid_client::{Orchid, cli};
use die_vim::Vim;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CHILD: &str = "DIE_ORCHID_TEST_E2E_CHILD";
const ID: &str = "e2e00000-0000-0000-0000-000000000001";

fn keys(h: &mut Harness, k: &str) {
    for k in h.type_keys(k).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
}

/// Whether process `pid` is still there.
fn alive(pid: &str) -> bool {
    let mut probe = Command::new("/bin/kill");
    probe.args(["-0", pid]).stderr(Stdio::null());
    probe.status().is_ok_and(|s| s.success())
}

#[test]
fn a_chat_runs_on_a_fake_orchid_through_the_real_runner() {
    if std::env::var_os(CHILD).is_none() {
        let home = TempRoot::new("e2e-home");
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "a_chat_runs_on_a_fake_orchid_through_the_real_runner",
        ])
        .arg("--nocapture")
        .env(CHILD, "1")
        .env("HOME", home.path())
        .env("ORCHID_SESSION_ID", "parent-session")
        .env("ORCHID_CONFIG", "/nowhere/orchid")
        .env("ORCHID_HOOK_DEPTH", "3");
        let out = cmd.stdin(Stdio::null()).output().unwrap();
        let said = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success() && said.contains("1 passed"), "{said}");
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let root = home.join("root");
    let work = home.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake-orchid");
    assert!(script.is_absolute());
    let runner = ProcessRunner {
        path: Some("/bin:/usr/bin".into()),
        env: vec![],
    };
    let mut orchid = Orchid::for_tests(&root, FakeRunner::new()).with_runner(runner);
    orchid.options.program = script;
    orchid.options.timeout = Duration::from_secs(2);
    let mut h = Harness::new(vec![Box::new(Vim::default()), Box::new(orchid)]).unwrap();
    let (notes, _) = h.open_text("notes", "").unwrap();
    h.set_directory(notes, &work);
    h.set_window_size(80, 40);

    // A new session, of an agent orchid says is valid, in this buffer's directory.
    h.run("orchid-new-session", &Args::default()).unwrap();
    h.run_until_idle();
    assert_eq!(h.prompt_candidates(), ["echo"], "not the broken one");
    keys(&mut h, "echo<CR>");
    h.run_until_idle();
    let b = h.focused_buffer().unwrap();
    assert_eq!(
        h.buffer_name(b),
        Some("*orchid-chat-echo-work-00001*"),
        "{:?}",
        h.messages
    );
    let meta = store::read_metadata(&root, ID).unwrap();
    assert_eq!(meta.label.as_deref(), Some("echo-work"), "set --label ran");
    assert_eq!(meta.working_dir.as_deref(), work.to_str());
    assert_eq!(meta.parent, None, "no ORCHID_SESSION_ID reached it");
    let seen = |name: &str| std::fs::read_to_string(root.join(name)).unwrap();
    assert_eq!(seen("fake-env"), "unset unset unset\n");
    assert_eq!(seen("fake-path"), "/bin:/usr/bin\n");

    // A message, and orchid's echo of it, followed in until the turn is over. The chatter on
    // stderr isn't the answer, which is stdout's.
    keys(&mut h, "ihello<CR>");
    let text = |h: &Harness| h.text(b).unwrap().rope().to_string();
    let start = Instant::now();
    while !text(&h).contains("[Finished: ") {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the turn never finished:\n{}",
            text(&h)
        );
        std::thread::sleep(Duration::from_millis(20));
        h.advance(FAST);
    }
    let t = text(&h);
    let user = t.find("[user]    [hello").expect(&t);
    let echo = t.find("[agnt]    [echo: hello").expect(&t);
    assert!(user < echo && echo < t.find("[Finished: ").unwrap(), "{t}");
    assert_eq!(t.matches("[user]    [hello").count(), 1, "shown once");

    // A send orchid refuses: its message, from stderr, as it exits 1.
    keys(&mut h, "fail please<CR>");
    let t = text(&h);
    assert!(
        t.contains("[SYSTEM] Send failed:\nthe echo connection refused it\n"),
        "{t}"
    );

    // /kill: a stop that hangs is stopped at the timeout, and so is what it started.
    keys(&mut h, "/kill<CR>");
    let said = h.messages.last().cloned().unwrap_or_default();
    assert_eq!(
        said,
        "[orchid] kill: orchid took longer than 2 s, and was stopped"
    );
    let sleeper = seen("fake-sleeper");
    let start = Instant::now();
    while alive(sleeper.trim()) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "its child outlived it"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    // Archived, which the chat says once.
    let cli = cli(&mut h).unwrap();
    let out = cli.run_now(&Cmd::Delete { id: ID.into() }, None).unwrap();
    assert_eq!(out["archived"], true);
    h.advance(SLOW);
    assert_eq!(text(&h).matches("is gone (archived or deleted)").count(), 1);
    assert!(h.take_errors().is_empty(), "{:?}", h.messages);
}
