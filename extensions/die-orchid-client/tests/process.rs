//! The real runner's plumbing, on a stand-in for orchid written here as a POSIX sh script: the
//! PATH it's found on, `--config` first, stdout apart from stderr, exit codes, the directory it
//! runs in, the environment it doesn't inherit, and its timeout. These run in a process of their
//! own, since the environment is the process's: a HOME of their own, and the `ORCHID_` variables
//! an orchid tool that started DIE would leave. Neither your orchid nor your login shell runs.

use die_test_cap as _;

use die_core::Harness;
use die_orchid_client::cli::{Call, Cmd, FakeRunner, Outcome, ProcessRunner, Runner, argv};
use die_orchid_client::fixtures::TempRoot;
use die_orchid_client::{Orchid, default_root};
use die_vim::Vim;
use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// What the parent sets for the child, which an orchid tool would leave in DIE's environment.
const CHILD: &str = "DIE_ORCHID_TEST_CHILD";

/// Run the test `name` again in a process of its own, with HOME `home` and `env`, and say whether
/// it passed.
fn in_own_process(name: &str, home: &Path, env: &[(&str, &str)]) {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", name, "--nocapture"])
        .env(CHILD, "1")
        .env("HOME", home);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.stdin(Stdio::null()).output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("1 passed"), "{said}");
}

/// The stand-in: `list` says its environment, its directory and the root it was given; `send`
/// echoes its message; `agent` fails as orchid does; `stop` hangs.
const SCRIPT: &str = r#"#!/bin/sh
if [ "$1" != "--config" ]; then
  echo '{"error":"command_failed","message":"--config must come first"}' >&2
  exit 1
fi
root=$2
shift 2
case "$1" in
  --version) echo "orchid 0.2.2 (codex compatibility 1)" ;;
  list)
    echo "stderr is apart" >&2
    printf '{"home":"%s","pwd":"%s","root":"%s","session":"%s","config":"%s","depth":"%s"}\n' \
      "$HOME" "$(pwd -P)" "$root" "${ORCHID_SESSION_ID-unset}" "${ORCHID_CONFIG-unset}" "${ORCHID_HOOK_DEPTH-unset}" ;;
  send) printf '{"id":"%s","status":"running","message":"%s"}\n' "$3" "$5" ;;
  agent)
    echo '{"error":"command_failed","message":"agent default: no prompt"}' >&2
    exit 1 ;;
  stop) exec sleep 30 ;;
esac
"#;

/// The stand-in, as `orchid` in a directory of its own.
fn stand_in(home: &Path) -> PathBuf {
    let dir = home.join("bin");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("orchid");
    std::fs::write(&path, SCRIPT).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

/// Run `cmd` through the extension, as a command would, and wait for it.
fn run(h: &mut Harness, cmd: Cmd, cwd: Option<PathBuf>) -> Outcome {
    let got: Rc<RefCell<Option<Outcome>>> = Rc::default();
    let into = got.clone();
    let cli = die_orchid_client::cli(h).unwrap();
    cli.run(h, cmd, cwd, move |_, out| {
        *into.borrow_mut() = Some(out);
        Ok(())
    })
    .detach();
    h.run_until_idle();
    got.take().expect("the job's done ran")
}

#[test]
fn the_runner_finds_orchid_scrubs_its_environment_and_reads_its_answers() {
    if std::env::var_os(CHILD).is_none() {
        let home = TempRoot::new("process-home");
        let env = [
            ("ORCHID_SESSION_ID", "parent-session"),
            ("ORCHID_CONFIG", "/nowhere/orchid"),
            ("ORCHID_HOOK_DEPTH", "3"),
        ];
        let name = "the_runner_finds_orchid_scrubs_its_environment_and_reads_its_answers";
        in_own_process(name, home.path(), &env);
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let bin = stand_in(&home);
    let root = home.join("root");
    let work = home.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let path = std::env::join_paths([bin.clone(), "/bin".into(), "/usr/bin".into()]).unwrap();
    let runner = ProcessRunner {
        path: Some(path),
        env: vec![],
    };
    assert_eq!(runner.find(Path::new("orchid")), Some(bin.join("orchid")));
    assert_eq!(runner.find(Path::new("no-such-orchid")), None);
    let orchid = Orchid::for_tests(&root, FakeRunner::new()).with_runner(runner.clone());
    let mut h = Harness::new(vec![Box::new(Vim::default()), Box::new(orchid)]).unwrap();
    // The environment: orchid's variables gone, the HOME the process has, the directory asked for.
    let said = run(&mut h, Cmd::List, Some(work.clone())).unwrap();
    assert_eq!(said["session"], "unset");
    assert_eq!(said["config"], "unset");
    assert_eq!(said["depth"], "unset");
    assert_eq!(said["home"], home.display().to_string());
    assert_eq!(
        said["pwd"],
        work.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(
        said["root"],
        root.display().to_string(),
        "--config first, with the root"
    );
    // A message that starts with a dash is still the message.
    let sent = Cmd::Send {
        id: "s1".into(),
        message: "-x".into(),
    };
    let said = run(&mut h, sent, None).unwrap();
    assert_eq!(
        (said["id"].as_str(), said["message"].as_str()),
        (Some("s1"), Some("-x"))
    );
    let version = run(&mut h, Cmd::Version, None).unwrap();
    assert_eq!(version, "orchid 0.2.2 (codex compatibility 1)");
    // Its error is its message, from stderr.
    let e = run(&mut h, Cmd::Agent, None).unwrap_err();
    assert_eq!(e.message, "agent default: no prompt");
    assert_eq!(e.reply.code, Some(1));
    // Past its timeout, it's stopped.
    let call = Call {
        program: "orchid".into(),
        args: argv(&root, &Cmd::Stop { id: "s1".into() }),
        cwd: None,
        timeout: Duration::from_millis(300),
    };
    let start = Instant::now();
    let reply = runner.run(&call);
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(reply.code, None);
    let e = reply.result().unwrap_err();
    assert_eq!(e.message, "orchid took longer than 0.3 s, and was stopped");
    // Not there at all.
    let gone = ProcessRunner {
        path: Some("/nowhere".into()),
        env: vec![],
    };
    let reply = gone.run(&Call {
        program: "orchid".into(),
        ..call
    });
    assert_eq!(
        reply.result().unwrap_err().message,
        "orchid isn't on the PATH"
    );
    assert!(h.take_errors().is_empty());
    assert!(
        !root.exists(),
        "the stand-in made nothing, and neither did the client"
    );
}

#[test]
fn the_root_is_orchids_config_variable_then_home() {
    if std::env::var_os(CHILD).is_none() {
        let home = TempRoot::new("root-home");
        let name = "the_root_is_orchids_config_variable_then_home";
        in_own_process(name, home.path(), &[("ORCHID_CONFIG", "/nowhere/orchid")]);
        in_own_process(name, home.path(), &[]);
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let want = match std::env::var_os("ORCHID_CONFIG") {
        Some(root) => PathBuf::from(root),
        None => home.join(".config/orchid"),
    };
    assert_eq!(default_root(), want);
    // At registration it isn't even worked out, and asked for, it's only a path.
    let orchid = Orchid::new(Default::default()).with_runner(FakeRunner::new());
    let mut h = Harness::new(vec![Box::new(Vim::default()), Box::new(orchid)]).unwrap();
    let root = h
        .with_ext(|o: &mut Orchid, _| o.root().to_path_buf())
        .unwrap();
    assert_eq!(root, want);
    assert!(!home.join(".config").exists());
}
