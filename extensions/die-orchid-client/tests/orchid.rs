//! The real orchid, offline: that the client reads what orchid 0.2.2 writes, and that what it
//! runs, orchid takes. Ignored, since it needs the binary: give it as `ORCHID_BIN`, as in
//! `ORCHID_BIN=~/.local/bin/orchid cargo test -p die-orchid-client --test orchid -- --ignored`.
//! It runs in a process of its own with a HOME of its own and no `ORCHID_` variables, on a root
//! of its own, with orchid's built-in `echo` connection: no network, no keys, and never your root.

use die_test_cap as _;

use die_core::Harness;
use die_orchid_client::cli::{Cmd, FakeRunner, ProcessRunner};
use die_orchid_client::fixtures::TempRoot;
use die_orchid_client::parse::{self, Shown};
use die_orchid_client::store::{self, Event, EventKind, Status};
use die_orchid_client::{Orchid, test_zone};
use die_vim::Vim;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CHILD: &str = "DIE_ORCHID_TEST_REAL_CHILD";

#[test]
#[ignore = "needs orchid: ORCHID_BIN=path/to/orchid"]
fn orchid_0_2_2_writes_what_the_client_reads() {
    let Some(bin) = std::env::var_os("ORCHID_BIN") else {
        panic!("ORCHID_BIN names no orchid");
    };
    if std::env::var_os(CHILD).is_none() {
        let home = TempRoot::new("real-home");
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "orchid_0_2_2_writes_what_the_client_reads",
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
    // orchid's own pattern for an offline agent (its `.test-config/agents/echo.json`).
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
    let mut orchid = Orchid::for_tests(&root, FakeRunner::new()).with_runner(runner);
    orchid.options.program = bin;
    let mut h = Harness::new(vec![Box::new(Vim::default()), Box::new(orchid)]).unwrap();
    let cli = die_orchid_client::cli(&mut h).unwrap();

    let version = cli.run_now(&Cmd::Version, None).unwrap();
    assert!(
        version.as_str().unwrap().starts_with("orchid 0.2."),
        "{version}"
    );
    let agents = cli.run_now(&Cmd::Agent, None).unwrap();
    let echo = agents["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "echo")
        .cloned();
    assert_eq!(echo.unwrap()["valid"], true, "{agents}");

    let create = Cmd::Create {
        agent: Some("echo".into()),
        label: Some("echo-work".into()),
        working_dir: Some(work.clone()),
    };
    let made = cli.run_now(&create, None).unwrap();
    let id = made["id"].as_str().unwrap().to_string();
    let meta = store::read_metadata(&root, &id).unwrap();
    assert_eq!(meta.status, Status::Idle);
    assert_eq!(meta.agent_name.as_deref(), Some("echo"));
    assert_eq!(meta.label.as_deref(), Some("echo-work"));
    assert_eq!(
        meta.working_dir.as_deref(),
        Some(&*work.canonicalize().unwrap().to_string_lossy())
    );
    assert!(meta.created_at.is_some() && meta.updated_at.is_some());

    // A message starting with a dash, past `--`.
    let sent = cli
        .run_now(
            &Cmd::Send {
                id: id.clone(),
                message: "-hello there".into(),
            },
            None,
        )
        .unwrap();
    assert!(
        ["running", "queued"].contains(&sent["status"].as_str().unwrap()),
        "{sent}"
    );
    let start = Instant::now();
    let meta = loop {
        let meta = store::read_metadata(&root, &id).unwrap();
        if !meta.status.is_running() && meta.pid.is_none() {
            break meta;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the echo turn never ended"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(meta.status, Status::Idle, "{:?}", meta.termination_reason);

    // Every line reads, as a known kind, with its id, time and tokens.
    let events_path = store::events_path(&root, &id);
    let text = std::fs::read_to_string(&events_path).unwrap();
    let events: Vec<Event> = text.lines().map(|l| Event::parse(l).expect(l)).collect();
    for e in &events {
        assert!(!matches!(e.kind, EventKind::Other { .. }), "{e:?}");
        assert!(
            e.event_id.is_some() && e.timestamp.is_some() && e.token_usage.is_some(),
            "{e:?}"
        );
    }
    let said: Vec<(&str, &str)> = events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Message { role, content } => Some((role.as_str(), content.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        said,
        [("user", "-hello there"), ("assistant", "-hello there")]
    );
    let restored = store::restore(&events_path, 5 << 20, 250, |e| parse::shows(e, true)).unwrap();
    assert_eq!(restored.events.len(), 2);
    assert_eq!(restored.end, text.len() as u64);
    for e in &restored.events {
        let Shown::Sections(s) = parse::shown(e, true, &test_zone()) else {
            panic!("{e:?}")
        };
        assert_eq!(parse::width(&s[0].stub), 80, "{:?}", s[0].stub);
    }

    let set = Cmd::Set {
        id: id.clone(),
        label: Some("echo-renamed".into()),
        working_dir: None,
    };
    assert_eq!(cli.run_now(&set, None).unwrap()["updated"], true);
    let listed = cli.run_now(&Cmd::List, None).unwrap();
    assert_eq!(listed["sessions"][0]["label"], "echo-renamed");
    assert_eq!(store::scan(&root).len(), 1);
    let stopped = cli.run_now(&Cmd::Stop { id: id.clone() }, None).unwrap();
    assert_eq!(stopped["status"], "idle", "an idle session stays idle");
    let deleted = cli.run_now(&Cmd::Delete { id: id.clone() }, None).unwrap();
    assert_eq!(deleted["archived"], true);
    assert!(store::scan(&root).is_empty());
    let e = cli
        .run_now(
            &Cmd::Stop {
                id: "no-such-session".into(),
            },
            None,
        )
        .unwrap_err();
    assert!(
        !e.message.is_empty() && e.message != "command_failed",
        "{e}"
    );
}
