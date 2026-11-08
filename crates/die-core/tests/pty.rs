//! Real PTYs (contract §12): a shell that echoes, a resize `stty size` sees, the exit status, the
//! environment and the controlling terminal, a flood that doesn't hold up a key, no fd of one
//! terminal in another's program, and nothing left running after a close or at quit.
//!
//! A PTY's program starts through this binary's own trampoline, as it does through DIE's, so
//! this has no test harness: `main` is the trampoline first, then runs the tests itself.

use die_test_cap as _;

use die_core::pty::{BATCH, HANGUP_WAIT, trampoline};
use die_core::*;
use std::cell::RefCell;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ExitStatus;
use std::rc::Rc;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);

type Said = Rc<RefCell<Vec<u8>>>;
type Ended = Rc<RefCell<Option<io::Result<ExitStatus>>>>;

/// A program on a PTY, with what it said and how it ended.
struct Term {
    pty: Pty,
    said: Said,
    ended: Ended,
}

fn start(ctx: &mut Ctx, cmd: PtyCommand, size: PtySize) -> Term {
    let (said, ended) = (Said::default(), Ended::default());
    let (s, e) = (said.clone(), ended.clone());
    let cmd = cmd
        .on_output(move |_, bytes| {
            s.borrow_mut().extend_from_slice(bytes);
            Ok(())
        })
        .on_exit(move |_, status| {
            *e.borrow_mut() = Some(status);
            Ok(())
        });
    let pty = ctx.pty(cmd, size).expect("a PTY");
    Term { pty, said, ended }
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        cols,
        rows,
        width: cols * 8,
        height: rows * 16,
    }
}

fn sh(script: &str) -> PtyCommand {
    PtyCommand::new("/bin/sh").arg("-c").arg(script)
}

fn text(t: &Term) -> String {
    String::from_utf8_lossy(&t.said.borrow()).into_owned()
}

fn wait_for(ctx: &mut Ctx, t: &Term, needle: &str) {
    let found = ctx.run_until(WAIT, |_| text(t).contains(needle));
    assert!(found, "no {needle:?} in {:?}", text(t));
}

fn wait_end(ctx: &mut Ctx, t: &Term) -> ExitStatus {
    assert!(
        ctx.run_until(WAIT, |_| t.ended.borrow().is_some()),
        "it never ended: {:?}",
        text(t)
    );
    let ended = t.ended.borrow_mut().take().unwrap();
    ended.expect("it started")
}

fn alive(pid: u32) -> bool {
    // `kill -0`, without `unsafe`: ps exits 0 if the process exists (zombies included).
    let mut ps = std::process::Command::new("/bin/ps");
    ps.args(["-o", "stat=", "-p", &pid.to_string()]);
    match ps.output() {
        Ok(out) => {
            let stat = String::from_utf8_lossy(&out.stdout);
            out.status.success() && !stat.trim().is_empty() && !stat.trim().starts_with('Z')
        }
        Err(_) => false,
    }
}

fn gone_soon(pid: u32) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn editor() -> Ctx {
    Ctx::new(vec![]).unwrap()
}

fn a_shell_echoes_what_it_is_sent() {
    let mut ctx = editor();
    let t = start(&mut ctx, PtyCommand::new("/bin/sh"), size(80, 24));
    t.pty.write(b"echo hi-$((40+2))\n");
    wait_for(&mut ctx, &t, "hi-42");
    assert!(
        text(&t).contains("echo hi-$((40+2))"),
        "the terminal echoes the line typed"
    );
    t.pty.write(b"exit 3\n");
    assert_eq!(wait_end(&mut ctx, &t).code(), Some(3));
}

fn stty_size_sees_the_size_and_a_resize() {
    let mut ctx = editor();
    let t = start(&mut ctx, PtyCommand::new("/bin/sh"), size(80, 24));
    t.pty.write(b"stty size\n");
    wait_for(&mut ctx, &t, "24 80");
    t.pty.resize(size(101, 33)).unwrap();
    t.pty.write(b"stty size; exit\n");
    wait_for(&mut ctx, &t, "33 101");
    wait_end(&mut ctx, &t);
    assert!(
        t.pty.resize(size(1, 1)).is_ok(),
        "the PTY outlives its program until closed"
    );
    t.pty.close();
    assert!(t.pty.resize(size(1, 1)).is_err());
}

fn the_exit_status_comes_after_the_last_output() {
    let mut ctx = editor();
    let t = start(&mut ctx, sh("printf last; exit 7"), size(80, 24));
    assert_eq!(wait_end(&mut ctx, &t).code(), Some(7));
    assert_eq!(text(&t), "last", "all of it, before the exit");
    let t = start(&mut ctx, sh("kill -9 $$"), size(80, 24));
    assert_eq!(wait_end(&mut ctx, &t).signal(), Some(9));
    let t = start(
        &mut ctx,
        PtyCommand::new("no-such-program-here"),
        size(80, 24),
    );
    assert_eq!(
        wait_end(&mut ctx, &t).code(),
        Some(127),
        "as a shell says it"
    );
    assert!(text(&t).contains("no-such-program-here"), "{:?}", text(&t));
    let e = ctx.pty(
        PtyCommand::new("/bin/sh").current_dir("/no/such/dir"),
        size(80, 24),
    );
    assert!(e.is_err(), "no such directory");
}

fn it_runs_where_and_with_what_it_was_told() {
    let mut ctx = editor();
    let dir = std::env::temp_dir().canonicalize().unwrap();
    let script = "echo \"T=$TERM C=$COLORTERM P=$TERM_PROGRAM V=$TERM_PROGRAM_VERSION X=$X \
                  L=${COLUMNS-unset} K=${KITTY_WINDOW_ID-unset}\"; echo \"D=$(pwd -P)\"; \
                  echo \"PATH=$PATH\"; echo \"N=$0\"";
    let cmd = sh(script).current_dir(&dir).env("X", "yes").arg0("named");
    let t = start(&mut ctx, cmd, size(80, 24));
    wait_end(&mut ctx, &t);
    let said = text(&t);
    let want = format!(
        "T=xterm-256color C=truecolor P=DIE V={} X=yes L=unset K=unset",
        env!("CARGO_PKG_VERSION")
    );
    assert!(said.contains(&want), "{said}");
    assert!(said.contains(&format!("D={}", dir.display())), "{said}");
    let path = die_core::process::path().to_string_lossy().into_owned();
    assert!(
        said.contains(&format!("PATH={path}")),
        "the login shell's: {said}"
    );
    assert!(said.contains("N=named"), "{said}");
    let t = start(
        &mut ctx,
        sh("echo \"[$HOME]\"").env_remove("HOME"),
        size(80, 24),
    );
    wait_end(&mut ctx, &t);
    assert!(text(&t).contains("[]"), "{}", text(&t));
}

fn it_leads_a_session_on_its_own_controlling_terminal() {
    let mut ctx = editor();
    let t = start(&mut ctx, PtyCommand::new("/bin/sh").arg("-i"), size(80, 24));
    let pid = t.pty.pid().unwrap_or_else(|| {
        ctx.run_until(WAIT, |_| t.pty.pid().is_some());
        t.pty.pid().unwrap()
    });
    t.pty.write(b"ps -o stat= -o tty= -p $$\n");
    let found = ctx.run_until(WAIT, |_| text(&t).contains("Ss"));
    assert!(found, "a session leader in the foreground: {:?}", text(&t));
    assert_eq!(
        t.pty.foreground_group(),
        Some(pid),
        "at its prompt, the shell's own"
    );
    t.pty.write(b"sleep 3\n");
    let away = ctx.run_until(WAIT, |_| t.pty.foreground_group().is_some_and(|g| g != pid));
    assert!(
        away,
        "a job of its own while it runs, as job control has it"
    );
    drop(t);
    let script = "echo on; sleep 1; stty -echo; echo off; sleep 5";
    let t = start(&mut ctx, sh(script), size(80, 24));
    wait_for(&mut ctx, &t, "on");
    assert_eq!(t.pty.echoes(), Some(true));
    wait_for(&mut ctx, &t, "off");
    assert_eq!(t.pty.echoes(), Some(false), "as at a password prompt");
}

fn a_program_that_doesnt_read_holds_up_nothing() {
    let mut ctx = editor();
    let t = start(&mut ctx, sh("sleep 5"), size(80, 24));
    let start = Instant::now();
    for _ in 0..64 {
        t.pty.write(&[b'x'; 16 << 10]);
    }
    ctx.run_until_idle();
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "{:?}",
        start.elapsed()
    );
    let pid = t.pty.pid();
    drop(t);
    if let Some(pid) = pid {
        assert!(gone_soon(pid), "hung up and gone");
    }
}

fn a_flood_doesnt_hold_up_a_key_or_the_frame() {
    struct Keys(Rc<RefCell<Option<Instant>>>);
    impl Extension for Keys {
        fn name(&self) -> &'static str {
            "keys"
        }
        fn register(&mut self, reg: &mut Registry) {
            let at = self.0.clone();
            reg.command("note", "", move |_, _| {
                *at.borrow_mut() = Some(Instant::now());
                Ok(())
            });
            reg.global_keymap().bind(State::Normal, "x", "note");
        }
    }
    let ran = Rc::new(RefCell::new(None));
    let mut ctx = Ctx::new(vec![Box::new(Keys(ran.clone()))]).unwrap();
    let b = ctx.create_text("t", "");
    ctx.open_window(b, 80, 24).unwrap();
    // A terminal engine slower than `yes`: 4 ms a batch, so output always waits.
    let (got, most) = (Rc::new(RefCell::new(0)), Rc::new(RefCell::new(0)));
    let (g, m) = (got.clone(), most.clone());
    let cmd = PtyCommand::new("yes")
        .arg("a flood of output")
        .on_output(move |_, bytes| {
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(4) {}
            *g.borrow_mut() += bytes.len();
            let mut m = m.borrow_mut();
            *m = (*m).max(bytes.len());
            Ok(())
        });
    let pty = ctx.pty(cmd, size(80, 24)).unwrap();
    let flowing = ctx.run_until(WAIT, |_| *got.borrow() > 4 * BATCH);
    assert!(flowing, "{} bytes", got.borrow());
    // As the host drives it: turns, then a frame, with a key typed meanwhile.
    let mut slowest = Duration::ZERO;
    let typed = Instant::now();
    ctx.input(KeyEvent::press(Key::Char('x')));
    let mut turns = 0;
    while ran.borrow().is_none() && turns < 1000 {
        let turn = Instant::now();
        ctx.run_ready();
        ctx.scene();
        slowest = slowest.max(turn.elapsed());
        turns += 1;
    }
    let at = ran.borrow().expect("the key ran");
    assert!(turns <= 2, "the key waited {turns} turns");
    let waited = at.duration_since(typed);
    assert!(
        waited < Duration::from_millis(250),
        "the key waited {waited:?}"
    );
    for _ in 0..20 {
        let turn = Instant::now();
        ctx.run_ready();
        slowest = slowest.max(turn.elapsed());
    }
    assert!(
        slowest < Duration::from_millis(250),
        "a turn took {slowest:?}"
    );
    assert!(ctx.pty_output_waiting(), "so the host doesn't sleep on it");
    assert!(*most.borrow() <= BATCH, "a batch of {}", most.borrow());
    let pid = pty.pid().unwrap();
    drop(pty);
    assert!(gone_soon(pid), "yes ends with its terminal");
}

fn all_of_a_floods_output_comes_before_its_exit() {
    let mut ctx = editor();
    let (got, tail, ended) = (Rc::new(RefCell::new(0)), Said::default(), Ended::default());
    let at_exit = Rc::new(RefCell::new(None));
    let (g, t) = (got.clone(), tail.clone());
    let (g2, e, a) = (got.clone(), ended.clone(), at_exit.clone());
    // 600,000 lines of `line`, each `\r\n` on the terminal, into a consumer slower than they come,
    // so the reader fills and waits while the program exits.
    let cmd = sh("yes line | head -c 3000000; printf END")
        .on_output(move |_, bytes| {
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(2) {}
            *g.borrow_mut() += bytes.len();
            let mut t = t.borrow_mut();
            t.extend_from_slice(bytes);
            let keep = t.len().saturating_sub(16);
            t.drain(..keep);
            Ok(())
        })
        .on_exit(move |_, status| {
            *e.borrow_mut() = Some(status);
            *a.borrow_mut() = Some(*g2.borrow());
            Ok(())
        });
    let _pty = ctx.pty(cmd, size(80, 24)).unwrap();
    let done = ctx.run_until(WAIT * 3, |_| at_exit.borrow().is_some());
    assert!(done, "{} bytes", got.borrow());
    assert_eq!(
        *at_exit.borrow(),
        Some(3_600_003),
        "every byte before the exit"
    );
    ctx.run_until_idle();
    assert_eq!(*got.borrow(), 3_600_003, "and none after it");
    assert!(String::from_utf8_lossy(&tail.borrow()).ends_with("line\r\nEND"));
    assert_eq!(ended.borrow_mut().take().unwrap().unwrap().code(), Some(0));
}

fn no_fd_of_one_terminal_reaches_anothers_program() {
    let mut ctx = editor();
    let first = start(&mut ctx, PtyCommand::new("/bin/sh"), size(80, 24));
    let _file = std::fs::File::open("/dev/null").unwrap();
    let t = start(&mut ctx, sh("ls -l /dev/fd/"), size(80, 24));
    wait_end(&mut ctx, &t);
    let said = text(&t);
    // Each fd, with its type: the terminal's 0, 1 and 2, and past them only the directories `ls`
    // has open itself. Another terminal's fds, or /dev/null, would be devices.
    let fds: Vec<(char, u32)> = said
        .lines()
        .filter(|l| !l.starts_with("total"))
        .filter_map(|l| {
            Some((
                l.chars().next()?,
                l.split_whitespace().last()?.parse().ok()?,
            ))
        })
        .collect();
    assert!(fds.iter().take(3).all(|f| f.0 == 'c'), "{said}");
    assert!(fds.iter().skip(3).all(|f| f.0 == 'd'), "{said}");
    assert_eq!(
        fds.iter().take(3).map(|f| f.1).collect::<Vec<_>>(),
        [0, 1, 2],
        "{said}"
    );
    drop(first);
}

fn closing_hangs_up_and_then_kills() {
    let mut ctx = editor();
    let t = start(
        &mut ctx,
        sh("trap '' HUP; echo ready; sleep 600"),
        size(80, 24),
    );
    wait_for(&mut ctx, &t, "ready");
    let pid = t.pty.pid().unwrap();
    let closed = Instant::now();
    t.pty.close();
    assert!(gone_soon(pid), "killed once the hangup didn't end it");
    assert!(closed.elapsed() >= HANGUP_WAIT, "{:?}", closed.elapsed());
    ctx.run_until_idle();
    assert!(t.ended.borrow().is_none(), "no callback after a close");
    let t = start(&mut ctx, sh("echo ready; sleep 600"), size(80, 24));
    wait_for(&mut ctx, &t, "ready");
    let pid = t.pty.pid().unwrap();
    let closed = Instant::now();
    drop(t);
    assert!(gone_soon(pid), "a hangup ends it");
    assert!(closed.elapsed() < HANGUP_WAIT * 3, "{:?}", closed.elapsed());
}

fn nothing_is_left_running_after_shutdown() {
    let mut ctx = editor();
    let deaf = start(
        &mut ctx,
        sh("trap '' HUP; echo ready; sleep 600"),
        size(80, 24),
    );
    let plain = start(&mut ctx, PtyCommand::new("/bin/sh").arg("-i"), size(80, 24));
    plain.pty.write(b"sleep 600 & echo started\n");
    wait_for(&mut ctx, &deaf, "ready");
    wait_for(&mut ctx, &plain, "started");
    let job = std::process::Command::new("/usr/bin/pgrep")
        .args(["-P", &plain.pty.pid().unwrap().to_string(), "sleep"])
        .output()
        .unwrap();
    let job: u32 = String::from_utf8_lossy(&job.stdout)
        .trim()
        .parse()
        .expect("its job");
    let pids = [deaf.pty.pid().unwrap(), plain.pty.pid().unwrap(), job];
    ctx.shutdown();
    for pid in pids {
        assert!(gone_soon(pid), "{pid} is still running");
    }
    drop((deaf, plain));
}

type Test = fn();

fn main() {
    if let Some(code) = trampoline() {
        std::process::exit(code);
    }
    let tests: &[(&str, Test)] = &[
        (
            "a_shell_echoes_what_it_is_sent",
            a_shell_echoes_what_it_is_sent,
        ),
        (
            "stty_size_sees_the_size_and_a_resize",
            stty_size_sees_the_size_and_a_resize,
        ),
        (
            "the_exit_status_comes_after_the_last_output",
            the_exit_status_comes_after_the_last_output,
        ),
        (
            "it_runs_where_and_with_what_it_was_told",
            it_runs_where_and_with_what_it_was_told,
        ),
        (
            "it_leads_a_session_on_its_own_controlling_terminal",
            it_leads_a_session_on_its_own_controlling_terminal,
        ),
        (
            "a_program_that_doesnt_read_holds_up_nothing",
            a_program_that_doesnt_read_holds_up_nothing,
        ),
        (
            "a_flood_doesnt_hold_up_a_key_or_the_frame",
            a_flood_doesnt_hold_up_a_key_or_the_frame,
        ),
        (
            "all_of_a_floods_output_comes_before_its_exit",
            all_of_a_floods_output_comes_before_its_exit,
        ),
        (
            "no_fd_of_one_terminal_reaches_anothers_program",
            no_fd_of_one_terminal_reaches_anothers_program,
        ),
        (
            "closing_hangs_up_and_then_kills",
            closing_hangs_up_and_then_kills,
        ),
        (
            "nothing_is_left_running_after_shutdown",
            nothing_is_left_running_after_shutdown,
        ),
    ];
    let wanted: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let mut failed = 0;
    for (name, test) in tests {
        if !wanted.is_empty() && !wanted.iter().any(|w| name.contains(w.as_str())) {
            continue;
        }
        match catch_unwind(AssertUnwindSafe(test)) {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                println!("test {name} ... FAILED");
                failed += 1;
            }
        }
    }
    println!("test result: {} failed", failed);
    std::process::exit(i32::from(failed > 0));
}
