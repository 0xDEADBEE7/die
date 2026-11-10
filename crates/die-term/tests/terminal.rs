//! Terminal buffers on real PTYs (scope §6, D18): typing reaches the shell and its output the
//! cells; Esc leaves insert, on the alternate screen too, even with a fullscreen program's kitty
//! flags, and goes to the program in normal state (once a run, and not to the shell at its prompt
//! nor in line mode), and the toggle routes it; a resize reaches `stty size`; a paste is bracketed;
//! a flood doesn't hold up a key; the wheel scrolls back; the program's exit closes the buffer;
//! normal state is your Evil over the scrollback, follows output at the bottom and keeps the
//! program's colors; and nothing is left running after a quit, with no terminal in the session.
//!
//! A PTY's program starts through this binary's own trampoline, as it does through DIE's, so
//! this has no test harness: `main` is the trampoline first, then runs the tests itself.

use die_test_cap as _;

use die_core::pty::trampoline;
use die_core::*;
use die_term::{Escape, MouseTracking, NAME, Spec, TermSurface, Terminals, busy, open};
use die_vim::Vim;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);

/// A directory of its own to be HOME, so no profile of yours runs.
fn home() -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = &N;
    let dir = std::env::temp_dir().join(format!(
        "die-term-home-{}-{}",
        std::process::id(),
        n.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `/bin/sh` as a login shell with a HOME of its own, as `term` starts your shell.
fn sh_spec() -> Spec {
    let mut spec = Spec::shell(Path::new("/bin/sh"));
    spec.env = vec![
        ("HOME".into(), home().into()),
        ("ENV".into(), "".into()),
        ("LESS".into(), "".into()),
        ("BASH_SILENCE_DEPRECATION_WARNING".into(), "1".into()),
    ];
    spec
}

/// An editor with die-vim, as DIE has it, and a scratch buffer in an 80 × 24 window.
fn editor() -> Ctx {
    let exts: Vec<Box<dyn Extension>> =
        vec![Box::new(Vim::default()), Box::new(Terminals::default())];
    let mut ctx = Ctx::new(exts).unwrap();
    let b = ctx.create_text("*scratch*", "");
    // Short, so the status line has room for the name.
    ctx.set_directory(b, "/tmp");
    ctx.open_window(b, 80, 24).unwrap();
    ctx
}

/// An editor showing a shell, at its first prompt.
fn shell() -> (Ctx, BufferId) {
    let mut ctx = editor();
    let buf = open(&mut ctx, sh_spec(), ShowIn::Current).unwrap();
    // /etc/profile sets its own PS1, which ends in `$`.
    wait_until(&mut ctx, "at a prompt", |ctx| {
        screen(ctx).lines().any(|l| l.ends_with('$'))
    });
    (ctx, buf)
}

fn screen(ctx: &mut Ctx) -> String {
    let s = ctx.scene();
    let rows: Vec<String> = (0..s.rows).map(|r| s.row_text(r)).collect();
    rows.join("\n")
}

fn wait_for(ctx: &mut Ctx, needle: &str) {
    let found = ctx.run_until(WAIT, |ctx| screen(ctx).contains(needle));
    assert!(found, "no {needle:?} in\n{}", screen(ctx));
}

fn wait_until(ctx: &mut Ctx, what: &str, mut done: impl FnMut(&mut Ctx) -> bool) {
    let found = ctx.run_until(WAIT, |ctx| {
        ctx.scene();
        done(ctx)
    });
    assert!(found, "never {what}:\n{}", screen(ctx));
}

fn with<R>(ctx: &mut Ctx, buf: BufferId, f: impl FnOnce(&mut TermSurface) -> R) -> R {
    ctx.with_surface(buf, |s: &mut TermSurface, _| f(s))
        .unwrap()
}

fn alt(ctx: &mut Ctx, buf: BufferId) -> bool {
    with(ctx, buf, |s| s.term().alt_screen())
}

/// A job has the terminal, as its foreground group says.
fn wait_busy(ctx: &mut Ctx) {
    wait_until(ctx, "busy", |ctx| !busy(ctx).is_empty());
}

fn settle(ctx: &mut Ctx, for_: Duration) {
    let until = Instant::now() + for_;
    ctx.run_until(for_ + Duration::from_secs(1), |ctx| {
        ctx.scene();
        Instant::now() > until
    });
}

fn alive(pid: u32) -> bool {
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

/// The number after `tag` on the screen.
fn number_after(ctx: &mut Ctx, tag: &str) -> u32 {
    let s = screen(ctx);
    let at = s.rfind(tag).unwrap_or_else(|| panic!("no {tag} in\n{s}")) + tag.len();
    let digits: String = s[at..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap()
}

/// The wheel over the window, `dy` rows (up into the scrollback when negative).
fn wheel(dy: f32) -> Input {
    Input::Pointer(PointerEvent {
        kind: PointerKind::Scroll { dx: 0.0, dy },
        row: 5,
        col: 5,
        clicks: 0,
        mods: Mods::default(),
        px: None,
    })
}

/// The rows the focused window draws that aren't the snapshot's lines it says are there.
fn misdrawn(ctx: &mut Ctx, buf: BufferId) -> Vec<String> {
    let s = ctx.scene();
    let w = ctx.focused().unwrap();
    let t = ctx.text(buf).expect("a snapshot");
    let top = t.line_of(t.scroll(w).map_or(0, |s| s.0));
    let rows = ctx.window_size(w).unwrap().1;
    let lines = t.line_count() - 1; // the empty one after the last newline isn't drawn
    let rows = (0..rows).filter(|r| top + r < lines);
    rows.filter_map(|r| {
        let (drawn, line) = (s.row_text(r), t.rope().line(top + r).to_string());
        let (drawn, line) = (drawn.trim_end(), line.trim_end());
        (drawn != line).then(|| format!("row {r}: drawn {drawn:?}, line {line:?}"))
    })
    .collect()
}

fn typing_reaches_the_shell_and_its_output_the_cells() {
    let (mut ctx, buf) = shell();
    assert_eq!(
        ctx.state(),
        State::Passthrough,
        "a terminal starts in insert"
    );
    ctx.type_keys("echo hi-$((40+2))<CR>");
    wait_for(&mut ctx, "hi-42");
    let s = ctx.scene();
    let row = (0..24)
        .find(|r| s.row_text(*r) == "hi-42")
        .expect("a row of its own");
    assert_eq!(s.row(row)[0].glyph, Glyph::Char('h'));
    let status = s.row_text(24);
    assert!(
        status.contains(NAME) && status.ends_with("<I>"),
        "{status:?}"
    );
    // The directory is the buffer's, which the scratch buffer's was.
    ctx.type_keys("pwd<CR>");
    let dir = ctx.directory(buf).unwrap().to_path_buf();
    wait_for(&mut ctx, &dir.canonicalize().unwrap().display().to_string());
    assert!(ctx.text(buf).is_err(), "a surface, not text");
}

/// Ctrl-[ as the host delivers it, a `[` with Ctrl, which keymaps read as `<Esc>`.
fn ctrl_bracket() -> KeyEvent {
    KeyEvent::press(Key::Char('[')).mods(Mods {
        ctrl: true,
        ..Mods::default()
    })
}

fn esc_leaves_insert_on_the_main_screen_and_goes_to_the_program_in_normal() {
    let (mut ctx, _) = shell();
    ctx.type_keys("cat -v<CR>");
    wait_busy(&mut ctx);
    ctx.type_keys("<Esc>");
    assert_eq!(
        ctx.state(),
        State::Normal,
        "Esc on the main screen leaves insert"
    );
    assert!(screen(&mut ctx).lines().nth(24).unwrap().ends_with("<N>"));
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.state(), State::Normal, "Esc in normal stays there");
    ctx.type_keys("i");
    assert_eq!(ctx.state(), State::Passthrough, "i goes back to insert");
    ctx.type_keys("x<CR>");
    // cat reads a line at a time, so an Esc would only have been a byte at the start of its
    // next line, and normal state sent none: `x` alone, where it echoed `^[x`.
    wait_until(&mut ctx, "x read", |ctx| {
        screen(ctx).lines().filter(|l| *l == "x").count() == 2
    });
    assert!(!screen(&mut ctx).contains("^["), "{}", screen(&mut ctx));
    for key in ["a", "I", "A"] {
        ctx.type_keys("<Esc>");
        assert_eq!(ctx.state(), State::Normal);
        ctx.type_keys(key);
        assert_eq!(ctx.state(), State::Passthrough, "{key} goes back to insert");
    }
    // Ctrl-[ is Esc, as in your Emacs, so it leaves insert too.
    ctx.input(ctrl_bracket());
    ctx.run_until_idle();
    assert_eq!(ctx.state(), State::Normal, "Ctrl-[ leaves insert");
    ctx.type_keys("i<C-c>");
    wait_until(&mut ctx, "back at the prompt", |ctx| busy(ctx).is_empty());
}

/// `program` as the terminal's shell, interactive and not login, as ghostel and vterm start
/// bash, with a HOME and a ZDOTDIR of its own whose startup files set a prompt, at that prompt.
fn shell_at_its_prompt(program: &str) -> (Ctx, BufferId) {
    shell_at_its_prompt_with(program, "")
}

/// [`shell_at_its_prompt`], with `bashrc` after the prompt in the `.bashrc`.
fn shell_at_its_prompt_with(program: &str, bashrc: &str) -> (Ctx, BufferId) {
    let home = home();
    std::fs::write(home.join(".bashrc"), format!("PS1='bash-here$ '\n{bashrc}")).unwrap();
    std::fs::write(home.join(".zshrc"), "PS1='zsh-here$ '\n").unwrap();
    let mut spec = Spec {
        login: false,
        ..Spec::shell(Path::new(program))
    };
    spec.env = vec![
        ("HOME".into(), home.clone().into()),
        ("ZDOTDIR".into(), home.into()),
        ("BASH_SILENCE_DEPRECATION_WARNING".into(), "1".into()),
    ];
    let mut ctx = editor();
    let buf = open(&mut ctx, spec, ShowIn::Current).unwrap();
    wait_until(&mut ctx, "at its prompt", |ctx| {
        screen(ctx).lines().any(|l| l.ends_with("-here$"))
    });
    (ctx, buf)
}

/// What you hit: Esc pressed a few times at bash's prompt sent it Esc Esc, which bash 3.2 reads
/// as `complete` ("Display all 1773 possibilities?"), and a lone Esc that made the next key Meta,
/// as `M-l`. At the shell's own prompt normal state's Esc now sends nothing, in bash and zsh.
fn esc_in_normal_never_reaches_a_shell_at_its_prompt() {
    for program in ["/bin/bash", "/bin/zsh"] {
        let (mut ctx, _) = shell_at_its_prompt(program);
        assert!(busy(&mut ctx).is_empty(), "{program} at its prompt");
        ctx.type_keys("<Esc><Esc><Esc><Esc>");
        assert_eq!(ctx.state(), State::Normal);
        // Longer than zsh's KEYTIMEOUT, so a lone Esc would have been read as one.
        settle(&mut ctx, Duration::from_millis(500));
        ctx.type_keys("i");
        ctx.type_keys("lx=ok; echo got-$lx<CR>");
        wait_for(&mut ctx, "got-ok");
        let s = screen(&mut ctx);
        assert!(
            !s.contains("Display all") && !s.contains("not found"),
            "{program}:\n{s}"
        );
        // Nor with a count, which Esc cancels in Vim, nor after a register: `5<Esc>` sent bash
        // five, which listed every command, and the odd one made the next key Meta.
        ctx.type_keys("<Esc>5<Esc>3<Esc><Esc>\"a<Esc>");
        assert_eq!(ctx.state(), State::Normal);
        settle(&mut ctx, Duration::from_millis(500));
        ctx.type_keys("ilx=ok; echo again-$lx<CR>");
        wait_for(&mut ctx, "again-ok");
        let s = screen(&mut ctx);
        assert!(!s.contains("Display all"), "{program}:\n{s}");
    }
}

/// A job that doesn't read keys, as `sleep` or a build, leaves the terminal in line mode, where
/// an Esc is only a byte waiting for the next line: the prompt then read it as Meta and ate the
/// first letter typed, `iecho hi` running `cho hi`. Normal state's Esc sends none in line mode.
fn esc_in_normal_during_a_job_in_line_mode_waits_for_no_prompt() {
    for program in ["/bin/bash", "/bin/zsh"] {
        let (mut ctx, _) = shell_at_its_prompt(program);
        ctx.type_keys("sleep 1; echo slept-$((2+3))<CR>");
        wait_busy(&mut ctx);
        ctx.type_keys("<Esc><Esc>2<Esc>");
        assert_eq!(ctx.state(), State::Normal);
        wait_for(&mut ctx, "slept-5");
        wait_until(&mut ctx, "back at the prompt", |ctx| busy(ctx).is_empty());
        settle(&mut ctx, Duration::from_millis(300));
        ctx.type_keys("ilx=ok; echo got-$lx<CR>");
        wait_for(&mut ctx, "got-ok");
        let s = screen(&mut ctx);
        assert!(
            !s.contains("^[") && !s.contains("not found"),
            "{program}:\n{s}"
        );
    }
}

/// A raw reader in the foreground prints the bytes it reads as hex, up to an `x`.
const RAW: &str = r#"stty -icanon -echo
echo "raw-$1"
v=
while c=$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' '); [ "$c" != 78 ]; do v="$v $c"; done
stty sane
echo "read-$1:$v."
"#;

/// Normal state's Esc sends one a run: Escs with no other key between them send one, so pressing
/// it a few times to be sure of normal state doesn't interrupt Claude Code then rewind it. The
/// Esc that leaves insert sends nothing, a motion re-arms it, a held Esc's repeats send nothing
/// more, and nor do Ctrl-[s, which are Esc. `2<Esc>` sends two, as Claude Code's rewind wants, a
/// larger count two as well, and an Esc after a register only ends the prefix.
fn normal_states_esc_sends_one_a_run_and_a_count_at_most_two() {
    let (mut ctx, _) = shell();
    let reader = home().join("raw");
    std::fs::write(&reader, RAW).unwrap();
    let held = vec![KeyEvent::press(Key::Esc).kind(KeyKind::Repeat); 3];
    let ctrl = vec![ctrl_bracket(); 3];
    // The keys, then events after them, and the bytes the program reads.
    let cases = [
        ("four", "<Esc><Esc><Esc><Esc>", &[][..], " 1b"),
        ("two", "<Esc><Esc>", &[], " 1b"),
        ("held", "<Esc><Esc>", &held, " 1b"),
        ("ctrl", "<Esc>", &ctrl, " 1b"),
        ("count", "<Esc>2<Esc><Esc>", &[], " 1b 1b"),
        ("big", "<Esc>9<Esc>", &[], " 1b 1b"),
        ("register", "<Esc>\"a<Esc>", &[], ""),
        ("motion", "<Esc><Esc>k<Esc><Esc>", &[], " 1b 1b"),
    ];
    let mut wrong = vec![];
    for (name, keys, then, want) in cases {
        ctx.type_keys(&format!("sh '{}' {name}<CR>", reader.display()));
        wait_for(&mut ctx, &format!("raw-{name}"));
        wait_busy(&mut ctx);
        ctx.type_keys(keys);
        for ev in then {
            ctx.input(ev.clone());
        }
        ctx.run_until_idle();
        assert_eq!(ctx.state(), State::Normal, "{name}");
        ctx.type_keys("ix");
        wait_for(&mut ctx, &format!("read-{name}:"));
        let s = screen(&mut ctx);
        let got = s.lines().find(|l| l.starts_with(&format!("read-{name}:")));
        if got != Some(format!("read-{name}:{want}.").as_str()) {
            wrong.push(format!("{keys}: {got:?}, not{want}"));
        }
        wait_until(&mut ctx, "back at the prompt", |ctx| busy(ctx).is_empty());
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// A reader of two raw bytes on the alternate screen.
const ALT_RAW: &str = r#"printf '\033[?1049h'; stty -icanon -echo
echo alt-ready
v=$(dd bs=1 count=2 2>/dev/null | od -An -tx1); stty sane; printf '\033[?1049l'
echo read: $v
"#;

/// What you asked: Esc in insert goes to normal state on the alternate screen too, and normal
/// state's Esc then goes to the program, so Esc Esc leaves vim's insert mode and less reads it.
/// Normal state there is copy state over the program's screen alone.
fn esc_on_the_alternate_screen_goes_to_normal_and_esc_esc_reaches_vim_and_less() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("vim -u NONE<CR>");
    wait_until(&mut ctx, "in vim", |ctx| alt(ctx, buf));
    wait_busy(&mut ctx);
    wait_for(&mut ctx, "~");
    settle(&mut ctx, Duration::from_millis(300));
    ctx.type_keys("i<Esc>");
    assert_eq!(
        ctx.state(),
        State::Normal,
        "Esc on vim's screen leaves insert"
    );
    let text = ctx.text(buf).unwrap().rope().to_string();
    assert!(
        text.contains("\n~\n") && !text.contains("vim -u NONE"),
        "vim's screen alone: {text:?}"
    );
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.state(), State::Normal, "Esc in normal stays there");
    ctx.type_keys("i");
    settle(&mut ctx, Duration::from_millis(200));
    // Had the Esc not reached vim, still in its insert mode, this would be text in its buffer.
    ctx.type_keys(":q<CR>");
    wait_until(&mut ctx, "out of vim", |ctx| !alt(ctx, buf));
    ctx.type_keys("printf 'one\\ntwo\\n' | less<CR>");
    wait_until(&mut ctx, "in less", |ctx| alt(ctx, buf));
    wait_for(&mut ctx, "(END)");
    wait_busy(&mut ctx);
    ctx.type_keys("<Esc>");
    assert_eq!(
        ctx.state(),
        State::Normal,
        "Esc on less's screen leaves insert"
    );
    ctx.type_keys("<Esc>i");
    settle(&mut ctx, Duration::from_millis(200));
    // less reads Esc q as one command, so only the second q quits: it had the Esc.
    ctx.type_keys("q");
    settle(&mut ctx, Duration::from_millis(500));
    assert!(alt(&mut ctx, buf), "less read Esc q as one command");
    ctx.type_keys("q");
    wait_until(&mut ctx, "out of less", |ctx| !alt(ctx, buf));
    // And as bytes: the leaving Esc sends none, normal state's one. A script of its own, so the
    // reader is a job with the terminal: a `$(...)` at the prompt is in the shell's group.
    let reader = home().join("alt");
    std::fs::write(&reader, ALT_RAW).unwrap();
    ctx.type_keys(&format!("sh '{}'<CR>", reader.display()));
    wait_for(&mut ctx, "alt-ready");
    assert!(alt(&mut ctx, buf), "on the alternate screen");
    wait_busy(&mut ctx);
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.state(), State::Normal);
    ctx.type_keys("<Esc>ix");
    wait_for(&mut ctx, "read: 1b 78");
    // The DIE layer's Esc leaves as ever.
    ctx.type_keys("vim -u NONE<CR>");
    wait_until(&mut ctx, "in vim again", |ctx| alt(ctx, buf));
    ctx.type_keys("<D-Esc>");
    assert_eq!(ctx.state(), State::Normal, "Cmd-Esc always leaves");
    ctx.type_keys("i");
    settle(&mut ctx, Duration::from_millis(200));
    ctx.type_keys(":q<CR>");
    wait_until(&mut ctx, "out of vim again", |ctx| !alt(ctx, buf));
    let hints = ctx.messages.iter().filter(|m| m.contains("alternate"));
    assert_eq!(hints.count(), 0, "no hint: {:?}", ctx.messages);
}

/// The toggle: insert's Esc goes to normal state by default; `C-c C-r` sends it to the program,
/// for vim run in the terminal, and back. `1 C-c C-r` is evil-ghostel's auto, `2` the program
/// and `3` normal state, and from auto the toggle is back at normal state.
fn the_toggle_sends_insert_esc_to_the_program_and_a_count_picks_auto() {
    let (mut ctx, buf) = shell();
    let escape = |ctx: &mut Ctx| with(ctx, buf, |s| s.escape());
    assert_eq!(escape(&mut ctx), Escape::Normal, "the default");
    ctx.run("term-toggle-send-escape", &Args::default())
        .unwrap();
    assert_eq!(escape(&mut ctx), Escape::Program);
    assert_eq!(
        ctx.messages.last().unwrap(),
        "Esc in insert: to the program"
    );
    ctx.type_keys("cat -v<CR>");
    wait_busy(&mut ctx);
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.state(), State::Passthrough, "routed to the program");
    ctx.type_keys("y<CR>");
    wait_for(&mut ctx, "^[y");
    ctx.run("term-toggle-send-escape", &Args::default())
        .unwrap();
    assert_eq!(escape(&mut ctx), Escape::Normal);
    assert_eq!(
        ctx.messages.last().unwrap(),
        "Esc in insert: to normal state"
    );
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.state(), State::Normal, "back to normal state");
    // C-c C-r is the toggle's key in normal state, as evil-ghostel binds it.
    ctx.type_keys("<C-c><C-r>");
    assert_eq!(escape(&mut ctx), Escape::Program);
    ctx.type_keys("i<C-c>");
    wait_until(&mut ctx, "back at the prompt", |ctx| busy(ctx).is_empty());
    // The program's: insert's Esc leaves vim's insert mode, and DIE stays in insert.
    ctx.type_keys("vim -u NONE<CR>");
    wait_until(&mut ctx, "in vim", |ctx| alt(ctx, buf));
    wait_busy(&mut ctx);
    settle(&mut ctx, Duration::from_millis(300));
    ctx.type_keys("i<Esc>");
    assert_eq!(ctx.state(), State::Passthrough, "Esc went to vim");
    // Auto: to vim on its screen, and to normal state on the main screen.
    ctx.type_keys("<D-Esc>1<C-c><C-r>");
    assert_eq!(escape(&mut ctx), Escape::Auto);
    let said = ctx.messages.last().unwrap();
    assert_eq!(said, "Esc in insert: auto (to the program now)");
    ctx.type_keys("i<Esc>");
    assert_eq!(ctx.state(), State::Passthrough, "auto, on vim's screen");
    settle(&mut ctx, Duration::from_millis(200));
    // Had either Esc not reached vim, this would be text in its buffer.
    ctx.type_keys(":q<CR>");
    wait_until(&mut ctx, "out of vim", |ctx| !alt(ctx, buf));
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.state(), State::Normal, "auto, on the main screen");
    ctx.type_keys("<C-c><C-r>");
    assert_eq!(escape(&mut ctx), Escape::Normal, "from auto, the default");
    for (n, want) in [(2, Escape::Program), (3, Escape::Normal), (1, Escape::Auto)] {
        ctx.type_keys(&format!("{n}<C-c><C-r>"));
        assert_eq!(escape(&mut ctx), want, "{n} C-c C-r");
    }
    let said = ctx.messages.last().unwrap();
    assert_eq!(said, "Esc in insert: auto (to normal state now)");
    let bad = Args {
        count: Some(4),
        ..Args::default()
    };
    assert!(ctx.run("term-toggle-send-escape", &bad).is_err());
}

/// A fullscreen program, as Claude Code's fullscreen mode and btop are: on the alternate
/// screen, with the kitty keyboard flags pushed (31: every key as an escape code, releases
/// too), SGR mouse tracking and bracketed paste, then reading raw and logging every byte.
const FULLSCREEN: &str = r#"printf '\033[?1049h\033[>31u\033[?1000h\033[?1006h\033[?2004h\033[H'
printf 'fullscreen top\r\n'
stty raw -echo
printf 'fullscreen-ready\r\n'
exec dd bs=1 of="$1" 2>/dev/null
"#;

/// A shell after `seq 1 30`, running [`FULLSCREEN`], ready, and what it has logged so far.
fn fullscreen() -> (Ctx, BufferId, impl Fn() -> String) {
    let (mut ctx, buf) = shell();
    ctx.type_keys("seq 1 30<CR>");
    wait_for(&mut ctx, "\n30\n");
    let dir = home();
    let (script, log) = (dir.join("fullscreen"), dir.join("log"));
    std::fs::write(&script, FULLSCREEN).unwrap();
    ctx.type_keys(&format!(
        "sh '{}' '{}'<CR>",
        script.display(),
        log.display()
    ));
    wait_for(&mut ctx, "fullscreen-ready");
    wait_busy(&mut ctx);
    let logged =
        move || String::from_utf8_lossy(&std::fs::read(&log).unwrap_or_default()).into_owned();
    (ctx, buf, logged)
}

/// What you hit with Claude Code's fullscreen mode: its Esc no longer goes to it from insert,
/// whatever it asked of the terminal. The leaving Esc and its release send nothing, nor does a
/// click in normal state, and normal state's next Esc sends one, as the flags encode it: a
/// press and its release. More Escs send nothing, and Esc Esc from insert sends one again.
fn a_fullscreen_program_loses_esc_to_normal_state_and_gets_one_from_the_next() {
    let (mut ctx, buf, logged) = fullscreen();
    let modes = with(&mut ctx, buf, |s| s.term().modes());
    let asked = modes.alt_screen && modes.kitty_flags == 31 && modes.bracketed_paste;
    assert!(asked && modes.mouse != MouseTracking::None, "{modes:?}");
    ctx.type_keys("<Esc>");
    ctx.input(KeyEvent::press(Key::Esc).kind(KeyKind::Release));
    ctx.run_until_idle();
    assert_eq!(ctx.state(), State::Normal, "Esc leaves insert");
    let text = ctx.text(buf).unwrap().rope().to_string();
    assert!(
        text.starts_with("fullscreen top\nfullscreen-ready\n") && !text.contains("30"),
        "the program's screen alone: {text:?}"
    );
    ctx.scene();
    for kind in [
        PointerKind::Press(Button::Left),
        PointerKind::Release(Button::Left),
    ] {
        ctx.handle(Input::Pointer(PointerEvent {
            kind,
            row: 0,
            col: 3,
            clicks: 1,
            mods: Mods::default(),
            px: None,
        }));
    }
    ctx.run_until_idle();
    let one = "\x1b[27u\x1b[27;1:3u";
    ctx.type_keys("<Esc>");
    wait_until(&mut ctx, "an Esc logged", |_| !logged().is_empty());
    ctx.type_keys("<Esc><Esc>");
    settle(&mut ctx, Duration::from_millis(300));
    assert_eq!(logged(), one, "one Esc, from normal state's first");
    ctx.type_keys("i<Esc><Esc>");
    let two = one.repeat(2);
    wait_until(&mut ctx, "a second Esc logged", |_| {
        logged().len() >= two.len()
    });
    settle(&mut ctx, Duration::from_millis(300));
    assert_eq!(logged(), two, "one more from Esc Esc in insert");
    assert_eq!(ctx.state(), State::Normal);
}

/// The wheel goes to a program that tracks the mouse in normal state too, as in insert and as
/// your ghostel sends it in every Evil state, so Claude Code's fullscreen mode scrolls its
/// transcript after an Esc. Its screen has no scrollback for copy state to scroll.
fn the_wheel_in_normal_state_goes_to_a_program_that_tracks_the_mouse() {
    let (mut ctx, _, logged) = fullscreen();
    ctx.set_cell_pixels(CellPixels {
        width: 8,
        height: 16,
        scale: 1.0,
    });
    ctx.scene();
    ctx.handle(wheel(-3.0));
    ctx.run_until_idle();
    let up = "\x1b[<64;6;6M".repeat(3);
    let found = ctx.run_until(Duration::from_secs(3), |_| logged() == up);
    assert!(found, "the wheel in insert: {:?}", logged());
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.state(), State::Normal);
    ctx.scene();
    ctx.handle(wheel(3.0));
    ctx.run_until_idle();
    let down = up + &"\x1b[<65;6;6M".repeat(3);
    let found = ctx.run_until(Duration::from_secs(3), |_| logged() == down);
    assert!(found, "the wheel in normal state: {:?}", logged());
    assert_eq!(ctx.state(), State::Normal);
}

/// A key let go after Esc left insert still has its release sent, as kitty's flag 2 asks, so
/// the program never thinks it's held: a letter rolled over into Esc, and Ctrl-['s Ctrl.
fn a_key_let_go_in_normal_state_has_its_release_sent() {
    let (mut ctx, _, logged) = fullscreen();
    let a = Key::Char('a');
    ctx.input(KeyEvent::press(a));
    ctx.input(KeyEvent::press(Key::Esc));
    ctx.input(KeyEvent::press(a).kind(KeyKind::Release));
    ctx.input(KeyEvent::press(Key::Esc).kind(KeyKind::Release));
    ctx.run_until_idle();
    assert_eq!(ctx.state(), State::Normal, "rolled over into Esc");
    let rolled = "\x1b[97;;97u\x1b[97;1:3u";
    let found = ctx.run_until(Duration::from_secs(3), |_| logged() == rolled);
    assert!(found, "a's release: {:?}", logged());
    ctx.type_keys("i");
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    let control = Key::Modifier(ModifierKey::Control);
    ctx.input(KeyEvent::press(control).mods(ctrl));
    ctx.input(ctrl_bracket());
    ctx.input(ctrl_bracket().kind(KeyKind::Release));
    ctx.input(KeyEvent::press(control).kind(KeyKind::Release));
    ctx.run_until_idle();
    assert_eq!(ctx.state(), State::Normal, "Ctrl-[ leaves insert");
    let held = format!("{rolled}\x1b[57442;5u\x1b[57442;1:3u");
    let found = ctx.run_until(Duration::from_secs(3), |_| logged() == held);
    assert!(found, "Ctrl's release: {:?}", logged());
    // A key normal state gives nobody (F12 is unbound there, and the terminal takes no key in
    // copy state though it declares every one) sends no release either: the program never saw
    // its press.
    let f12 = Key::F(12);
    ctx.input(KeyEvent::press(f12));
    ctx.input(KeyEvent::press(f12).kind(KeyKind::Release));
    ctx.run_until_idle();
    assert_eq!(ctx.state(), State::Normal);
    assert!(
        ctx.messages.last().is_some_and(|m| m.contains("undefined")),
        "{:?}",
        ctx.messages
    );
    settle(&mut ctx, Duration::from_millis(300));
    assert_eq!(logged(), held, "no release for a press it never saw");
}

/// `C-c C-c` in normal state sends the program C-c, as your ghostel's map has it in every input
/// mode, and `C-c C-z`, `C-c C-\` and `C-c C-d` theirs, as the bytes, which the terminal's line
/// discipline makes signals of: so it interrupts a job without going back to insert.
fn c_c_c_c_in_normal_state_sends_the_program_c_c() {
    let (mut ctx, _, logged) = fullscreen();
    ctx.type_keys("<Esc><C-c><C-c><C-c><C-z><C-c><C-\\><C-c><C-d>");
    let found = ctx.run_until(Duration::from_secs(3), |_| logged() == "\x03\x1a\x1c\x04");
    assert!(found, "{:?}: {:?}", logged(), ctx.messages);
    assert_eq!(ctx.state(), State::Normal);
    let (mut ctx, _) = shell();
    ctx.type_keys("sleep 1000<CR>");
    wait_busy(&mut ctx);
    ctx.type_keys("<Esc><C-c><C-c>");
    wait_until(&mut ctx, "sleep interrupted", |ctx| busy(ctx).is_empty());
    assert_eq!(ctx.state(), State::Normal);
}

/// A program leaving the alternate screen while normal state's cursor is up its screen sends the
/// window to the terminal's cursor, as `G` does, since the alternate screen's line numbers say
/// nothing of the scrollback's lines.
fn leaving_the_alternate_screen_takes_normal_state_to_the_terminals_cursor() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("seq 1 3000<CR>");
    wait_for(&mut ctx, "\n3000\n");
    let dir = home();
    let (script, go) = (dir.join("alt"), dir.join("go"));
    let alt_then_back = "printf '\\033[?1049h\\033[H'; seq -f 'alt %g' 1 20\n\
                         while [ ! -e \"$1\" ]; do sleep 0.05; done; printf '\\033[?1049l'\n";
    std::fs::write(&script, alt_then_back).unwrap();
    ctx.type_keys(&format!("sh '{}' '{}'<CR>", script.display(), go.display()));
    wait_for(&mut ctx, "alt 20");
    ctx.type_keys("<Esc>gg");
    assert_eq!(snapshot(&mut ctx, buf).1, "alt 1");
    std::fs::write(&go, "").unwrap();
    wait_until(&mut ctx, "back at a prompt", |ctx| {
        !alt(ctx, buf) && snapshot(ctx, buf).0.contains("\n3000\n")
    });
    settle(&mut ctx, Duration::from_millis(300));
    let (_, at) = snapshot(&mut ctx, buf);
    assert!(at.ends_with('$'), "on the terminal's cursor: {at:?}");
    cursor_row_shows(&mut ctx, buf, &at);
    assert_eq!(misdrawn(&mut ctx, buf), Vec::<String>::new());
}

/// `\z` in normal state reflows the terminal's lines, and the cursor stays on its text, as in a
/// file: on a line's start through a widening, and on its column through a narrowing.
fn toggle_max_width_in_normal_state_keeps_the_cursor_on_its_text() {
    let (mut ctx, buf) = shell();
    let w = ctx.focused().unwrap();
    ctx.resize(w, 120, 24).unwrap();
    ctx.change_settings(buf, |s| (s.max_width, s.center) = (Some(80), false))
        .unwrap();
    let digits = "0123456789".repeat(8);
    ctx.type_keys(&format!(
        "clear; for i in 0 1 2 3 4 5 6 7 8 9; do echo L$i-{digits}-E$i; done; echo DONE-$((1+1))<CR>"
    ));
    wait_until(&mut ctx, "a prompt after DONE-2", |ctx| {
        let s = screen(ctx);
        let after = s.split("\nDONE-2\n").nth(1).unwrap_or_default();
        after.lines().next().is_some_and(|l| l.ends_with('$'))
    });
    let toggle = |ctx: &mut Ctx, cols: usize| {
        ctx.run("toggle-max-width", &Args::default()).unwrap();
        wait_until(ctx, "resized", |ctx| {
            with(ctx, buf, |s| s.term().size().0) == cols as u16
        });
        settle(ctx, Duration::from_millis(100));
    };
    ctx.type_keys("<Esc>gg/L3-<CR>");
    let l3 = format!("L3-{}", &digits[..77]);
    cursor_row_shows(&mut ctx, buf, &l3);
    toggle(&mut ctx, 120);
    let whole = format!("L3-{digits}-E3");
    cursor_row_shows(&mut ctx, buf, &whole);
    assert_eq!(misdrawn(&mut ctx, buf), Vec::<String>::new());
    ctx.type_keys("yy");
    assert_eq!(ctx.register('"').unwrap().text, format!("{whole}\n"));
    ctx.type_keys("$");
    toggle(&mut ctx, 80);
    cursor_row_shows(&mut ctx, buf, "789-E3");
    let t = ctx.text(buf).unwrap();
    let at = t.cursor(w).unwrap();
    assert_eq!(t.display_col(at), 5, "the column it had in its line");
    let above = t.rope().line(t.line_of(at) - 1).to_string();
    assert_eq!(above.trim_end(), l3, "the row before is its line's first");
    assert_eq!(misdrawn(&mut ctx, buf), Vec::<String>::new());
}

fn a_resize_reaches_stty_size_and_exit_closes_the_buffer() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("stty size<CR>");
    wait_for(&mut ctx, "24 80");
    let w = ctx.focused().unwrap();
    ctx.resize(w, 100, 30).unwrap();
    ctx.type_keys("clear; stty size<CR>");
    wait_for(&mut ctx, "30 100");
    // Two windows on one terminal: it takes the smaller one's size.
    let below = ctx.split_window(w, Side::Below).unwrap();
    assert_eq!(ctx.window_buffer(below), Some(buf));
    let rows = ctx
        .window_size(w)
        .unwrap()
        .1
        .min(ctx.window_size(below).unwrap().1);
    ctx.type_keys("clear; stty size<CR>");
    wait_for(&mut ctx, &format!("{rows} 100"));
    ctx.close_window(below).unwrap();
    ctx.type_keys("clear; stty size<CR>");
    wait_for(&mut ctx, "30 100");
    ctx.type_keys("exit<CR>");
    wait_until(&mut ctx, "closed", |ctx| !ctx.is_live(buf));
    assert!(
        ctx.messages.iter().all(|m| !m.contains("exited")),
        "a clean exit says nothing: {:?}",
        ctx.messages
    );
    assert!(
        ctx.focused_buffer().is_some(),
        "the window shows another buffer"
    );
    let (mut ctx, buf) = shell();
    ctx.type_keys("exit 3<CR>");
    wait_until(&mut ctx, "closed", |ctx| !ctx.is_live(buf));
    let said = format!("{NAME}: sh exited with 3");
    assert!(ctx.messages.contains(&said), "{:?}", ctx.messages);
}

fn a_paste_is_bracketed_when_the_program_asks() {
    let (mut ctx, _) = shell();
    ctx.type_keys("printf '\\033[?2004h'; cat -v<CR>");
    wait_busy(&mut ctx);
    ctx.set_register(
        'a',
        Register {
            kind: RegKind::Char,
            text: "two\nlines".into(),
        },
    );
    let a = Args {
        register: Some('a'),
        ..Args::default()
    };
    ctx.run("surface-paste", &a).unwrap();
    ctx.type_keys("<CR>");
    wait_for(&mut ctx, "^[[200~two");
    wait_for(&mut ctx, "lines^[[201~");
    ctx.type_keys("<C-c>printf '\\033[?2004l'<CR>");
    wait_until(&mut ctx, "back at the prompt", |ctx| busy(ctx).is_empty());
    // Unbracketed, a line break could run a command, so it asks first.
    ctx.set_register(
        'b',
        Register {
            kind: RegKind::Char,
            text: "echo pasted-$((2+3))\n".into(),
        },
    );
    let b = Args {
        register: Some('b'),
        ..Args::default()
    };
    ctx.run("surface-paste", &b).unwrap();
    assert!(ctx.prompt_text().is_some(), "it asks");
    ctx.type_keys("y<CR>");
    wait_for(&mut ctx, "pasted-5");
}

/// `n` frames as the host draws them, turns then the frame: the slowest, and the mean.
fn frames(ctx: &mut Ctx, n: u32) -> (Duration, Duration) {
    let (mut slowest, start) = (Duration::ZERO, Instant::now());
    for _ in 0..n {
        let turn = Instant::now();
        ctx.run_ready();
        ctx.scene();
        slowest = slowest.max(turn.elapsed());
    }
    (slowest, start.elapsed() / n)
}

/// Frames until `key` has run, as `ran` says: how many, and how long since it was typed.
fn key_in_flood(ctx: &mut Ctx, key: KeyEvent, ran: impl Fn(&mut Ctx) -> bool) -> (u32, Duration) {
    let (typed, mut turns) = (Instant::now(), 0);
    ctx.input(key);
    while !ran(ctx) && turns < 1000 {
        ctx.run_ready();
        ctx.scene();
        turns += 1;
    }
    assert!(ran(ctx), "the key never ran");
    (turns, typed.elapsed())
}

fn a_flood_doesnt_hold_up_a_key() {
    let (mut ctx, buf) = shell();
    let start = Instant::now();
    ctx.type_keys("yes | head -c 50000000; echo flood-$((6*7))<CR>");
    wait_busy(&mut ctx);
    wait_for(&mut ctx, "y\ny\ny");
    // In insert, as the host drives it: turns, then a frame, with a key typed meanwhile.
    let (insert_slowest, insert_mean) = frames(&mut ctx, 2000);
    let esc = KeyEvent::press(Key::Esc);
    let (turns, waited) = key_in_flood(&mut ctx, esc, |ctx| ctx.state() == State::Normal);
    // Normal state follows the flood, reading what's new at each frame.
    let (normal_slowest, normal_mean) = frames(&mut ctx, 200);
    let i = KeyEvent::press(Key::Char('i'));
    let (back, back_waited) = key_in_flood(&mut ctx, i, |ctx| ctx.state() == State::Passthrough);
    while !screen(&mut ctx).contains("flood-42") && start.elapsed() < Duration::from_secs(90) {
        ctx.run_ready();
    }
    let took = start.elapsed();
    assert!(screen(&mut ctx).contains("flood-42"), "the flood ended");
    println!(
        "  flood: 50 MB in {took:?}; in insert {insert_mean:?} a frame, at most \
         {insert_slowest:?}; Esc ran after {turns} frame(s), {waited:?}; in normal \
         {normal_mean:?} a frame, at most {normal_slowest:?}; i ran after {back} frame(s), \
         {back_waited:?}"
    );
    assert!(
        turns <= 2 && back <= 2,
        "a key waited {turns} or {back} frames"
    );
    let most = Duration::from_millis(250);
    assert!(
        waited < most && back_waited < most,
        "{waited:?}, {back_waited:?}"
    );
    assert!(insert_slowest < most && normal_slowest < most);
    // Typing in a flood: C-c stops one, and the prompt comes back. Typed without waiting for
    // idle, which a flood never is.
    for key in [Key::Char('y'), Key::Char('e'), Key::Char('s'), Key::Enter] {
        ctx.input(KeyEvent::press(key));
    }
    wait_busy(&mut ctx);
    wait_for(&mut ctx, "y\ny\ny");
    let typed = Instant::now();
    ctx.input(KeyEvent::press(Key::Char('c')).mods(Mods {
        ctrl: true,
        ..Mods::default()
    }));
    let mut turns = 0;
    while !busy(&mut ctx).is_empty() && typed.elapsed() < WAIT {
        ctx.run_ready();
        ctx.scene();
        turns += 1;
    }
    let stopped = typed.elapsed();
    assert!(busy(&mut ctx).is_empty(), "C-c stopped it");
    println!("  flood: C-c had the prompt back in {stopped:?}, {turns} frames");
    assert!(ctx.is_live(buf));
}

/// Output that parses slowly, DECALN filling the screen again and again, is parsed a few ms a
/// turn, so a key still goes in the next frame.
fn slow_output_holds_a_turn_for_a_few_ms() {
    let (mut ctx, _) = shell();
    let dir = home();
    let slow = "\x1b#8".repeat(300_000);
    std::fs::write(dir.join("slow"), slow).unwrap();
    // Typed and waited for in turns as the host takes them: idle, which `type_keys` waits for,
    // comes only once it's all parsed, with `cat` long gone.
    let cmd = format!("cat {}/slow; echo slow-$((3*3))", dir.display());
    for c in cmd.chars() {
        ctx.input(KeyEvent::press(Key::Char(c)));
    }
    ctx.input(KeyEvent::press(Key::Enter));
    let start = Instant::now();
    while !screen(&mut ctx).contains("EEEEEEEE") && start.elapsed() < WAIT {
        ctx.run_ready();
    }
    assert!(!screen(&mut ctx).contains("slow-9"), "still parsing");
    let (slowest, mean) = frames(&mut ctx, 50);
    let esc = KeyEvent::press(Key::Esc);
    let (turns, waited) = key_in_flood(&mut ctx, esc, |ctx| ctx.state() == State::Normal);
    ctx.type_keys("i");
    wait_for(&mut ctx, "slow-9");
    println!(
        "  slow output: {mean:?} a frame, at most {slowest:?}; Esc ran after {turns} frame(s), \
         {waited:?}"
    );
    assert!(turns <= 2, "the key waited {turns} frames");
    assert!(
        slowest < Duration::from_millis(100),
        "a frame took {slowest:?}"
    );
}

fn the_wheel_scrolls_back_and_a_key_returns_to_the_bottom() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("seq 1 200<CR>");
    wait_for(&mut ctx, "\n200\n");
    ctx.scene();
    ctx.handle(wheel(-40.5));
    ctx.run_until_idle();
    let v = with(&mut ctx, buf, |s| s.term().viewport());
    assert!(!v.at_bottom, "{v:?}");
    let s = screen(&mut ctx);
    assert!(!s.contains("\n200\n"), "scrolled back:\n{s}");
    ctx.handle(wheel(-0.5));
    ctx.run_until_idle();
    let up = with(&mut ctx, buf, |s| s.term().viewport().top);
    assert_eq!(up + 1, v.top, "half rows add up");
    ctx.type_keys("x");
    assert!(with(&mut ctx, buf, |s| s.term().viewport().at_bottom));
}

fn normal_state_is_your_evil_over_the_scrollback() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("seq 1 200<CR>");
    wait_for(&mut ctx, "\n200\n");
    ctx.type_keys("<Esc>gg");
    let s = screen(&mut ctx);
    assert!(s.contains("\n1\n2\n3\n"), "the scrollback's top:\n{s}");
    ctx.type_keys("/^150$<CR>yy");
    assert_eq!(ctx.register('"').unwrap().text, "150\n");
    ctx.type_keys("G");
    assert!(screen(&mut ctx).contains("\n200\n"), "back at the bottom");
    ctx.type_keys("x");
    assert!(
        ctx.messages.last().unwrap().contains("read-only"),
        "{:?}",
        ctx.messages
    );
    ctx.type_keys("i");
    assert_eq!(ctx.state(), State::Passthrough);
    assert!(ctx.text(buf).is_err(), "no snapshot in insert");
    assert!(with(&mut ctx, buf, |s| s.term().viewport().at_bottom));
}

/// Copy state's snapshot, and the cursor's line in it.
fn snapshot(ctx: &mut Ctx, buf: BufferId) -> (String, String) {
    ctx.scene();
    let w = ctx.focused().unwrap();
    let t = ctx.text(buf).expect("a snapshot");
    let line = t.line_of(t.cursor(w).unwrap());
    let at = t.rope().line(line).to_string().trim_end().to_string();
    (t.rope().to_string(), at)
}

/// The row the cursor is drawn on shows the cursor's line: the window shows the engine's cells
/// where the snapshot is.
fn cursor_row_shows(ctx: &mut Ctx, buf: BufferId, line: &str) {
    let s = ctx.scene();
    let row = s.row_text(s.cursors[0].row);
    assert_eq!(row.trim_end(), line, "the cursor's row");
    assert_eq!(snapshot(ctx, buf).1, line, "the cursor's line");
}

fn seq_20000_then_search_yank_and_back_to_the_bottom() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("seq 1 20000<CR>");
    wait_until(&mut ctx, "a prompt after 20000", |ctx| {
        let s = screen(ctx);
        let after = s.split("\n20000\n").nth(1).unwrap_or_default();
        after.lines().next().is_some_and(|l| l.ends_with('$'))
    });
    let took = Instant::now();
    ctx.type_keys("<Esc>");
    ctx.scene();
    let took = took.elapsed();
    let (text, at) = snapshot(&mut ctx, buf);
    let lines = text.lines().count();
    println!("  copy state: Esc over {lines} lines took {took:?}, with its frame");
    assert!((9_000..=10_001).contains(&lines), "{lines} lines");
    assert!(at.ends_with('$'), "on the shell's cursor: {at:?}");
    ctx.type_keys("gg");
    let first = text.lines().next().unwrap().to_string();
    assert_eq!(
        ctx.scene().row_text(0),
        first,
        "the scrollback's top on screen"
    );
    ctx.type_keys("/19999<CR>");
    cursor_row_shows(&mut ctx, buf, "19999");
    ctx.type_keys("yy");
    assert_eq!(ctx.register('"').unwrap().text, "19999\n");
    ctx.type_keys("G");
    assert!(
        snapshot(&mut ctx, buf).1.ends_with('$'),
        "back at the bottom"
    );
    assert!(with(&mut ctx, buf, |s| s.term().viewport().at_bottom));
    ctx.type_keys("k0vey");
    assert_eq!(ctx.register('"').unwrap().text, "20000");
    assert_eq!(ctx.state(), State::Normal);
}

fn new_output_is_followed_at_the_bottom_and_not_after_k() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("sleep 0.3; echo later; echo again<CR>");
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.state(), State::Normal);
    wait_until(&mut ctx, "the output followed", |ctx| {
        let (text, at) = snapshot(ctx, buf);
        text.contains("later\nagain\n") && at.ends_with('$')
    });
    let (text, at) = snapshot(&mut ctx, buf);
    assert_eq!(
        text.lines().last().unwrap().trim_end(),
        at,
        "on the last line"
    );
    cursor_row_shows(&mut ctx, buf, &at);
    ctx.type_keys("i");
    ctx.type_keys("sleep 0.3; echo second<CR>");
    ctx.type_keys("<Esc>k");
    let before = snapshot(&mut ctx, buf);
    let row = ctx.scene().cursors[0].row;
    wait_for(&mut ctx, "\nsecond\n");
    settle(&mut ctx, Duration::from_millis(200));
    let after = snapshot(&mut ctx, buf);
    assert_eq!(after.1, before.1, "frozen after k, on its line");
    assert_eq!(ctx.scene().cursors[0].row, row, "and its row");
    assert!(
        after.0.contains("\nsecond\n"),
        "the text is read all the same"
    );
    cursor_row_shows(&mut ctx, buf, &before.1);
    assert_eq!(misdrawn(&mut ctx, buf), Vec::<String>::new());
    ctx.type_keys("G");
    let (text, at) = snapshot(&mut ctx, buf);
    assert!(text.contains("\nsecond\n"), "G caught up:\n{text}");
    assert!(at.ends_with('$'), "{at:?}");
    cursor_row_shows(&mut ctx, buf, &at);
}

fn a_frozen_window_draws_what_it_yanks_through_a_clear_and_a_flood() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("seq 1 300<CR>");
    wait_for(&mut ctx, "\n300\n");
    // macOS's `clear` empties the scrollback too, as `cargo watch -c` does.
    ctx.type_keys("sleep 0.5; clear; echo fresh-$((5*5))<CR>");
    ctx.type_keys("<Esc>50k");
    assert_eq!(misdrawn(&mut ctx, buf), Vec::<String>::new());
    wait_until(&mut ctx, "cleared", |ctx| {
        let t = with(ctx, buf, |s| s.term().text());
        t.lines.iter().any(|l| l.text == "fresh-25")
    });
    settle(&mut ctx, Duration::from_millis(200));
    assert_eq!(misdrawn(&mut ctx, buf), Vec::<String>::new(), "after clear");
    ctx.type_keys("yy");
    let s = ctx.scene();
    let shown = s.row_text(s.cursors[0].row);
    let yanked = ctx.register('"').unwrap().text.clone();
    assert_eq!(
        yanked.trim_end(),
        shown.trim_end(),
        "yy takes the row it's drawn on"
    );
    // Frozen at the top while a flood pushes the oldest lines out of the scrollback.
    ctx.type_keys("i");
    ctx.type_keys("seq 1 12000<CR>");
    wait_for(&mut ctx, "\n12000\n");
    ctx.type_keys("sleep 0.3; seq 100000 112000; echo end-$((2+1))<CR>");
    ctx.type_keys("<Esc>gg");
    wait_until(&mut ctx, "flooded", |ctx| {
        let t = with(ctx, buf, |s| s.term().text());
        t.lines.iter().any(|l| l.text == "end-3")
    });
    settle(&mut ctx, Duration::from_millis(200));
    assert_eq!(
        misdrawn(&mut ctx, buf),
        Vec::<String>::new(),
        "after the flood"
    );
    ctx.type_keys("yy");
    let top = ctx.scene().row_text(0);
    let yanked = ctx.register('"').unwrap().text.clone();
    assert_eq!(yanked.trim_end(), top.trim_end());
}

fn esc_after_the_wheel_keeps_the_place_it_scrolled_to() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("seq 1 500<CR>");
    wait_for(&mut ctx, "\n500\n");
    ctx.scene();
    ctx.handle(wheel(-200.0));
    ctx.run_until_idle();
    let rows: Vec<String> = screen(&mut ctx)
        .lines()
        .take(24)
        .map(String::from)
        .collect();
    assert!(!rows.iter().any(|r| r == "500"), "scrolled back");
    ctx.type_keys("<Esc>");
    let now: Vec<String> = screen(&mut ctx)
        .lines()
        .take(24)
        .map(String::from)
        .collect();
    assert_eq!(now, rows, "the same rows in normal state");
    assert_eq!(
        ctx.scene().cursors[0].row,
        23,
        "the cursor on the last of them"
    );
    cursor_row_shows(&mut ctx, buf, &rows[23]);
    assert_eq!(misdrawn(&mut ctx, buf), Vec::<String>::new());
    ctx.type_keys("yy");
    assert_eq!(ctx.register('"').unwrap().text, format!("{}\n", rows[23]));
}

fn g_goes_to_the_terminals_cursor_and_p_takes_a_count() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("seq 1 30<CR>");
    wait_until(&mut ctx, "a prompt after 30", |ctx| {
        let s = screen(ctx);
        let after = s.split("\n30\n").nth(1).unwrap_or_default();
        after.lines().next().is_some_and(|l| l.ends_with('$'))
    });
    ctx.type_keys("<Esc>");
    let w = ctx.focused().unwrap();
    ctx.type_keys("gg0G");
    let cursor = |ctx: &mut Ctx| ctx.text(buf).unwrap().cursor(w);
    let at = cursor(&mut ctx).unwrap();
    assert_eq!(
        Some(at),
        ctx.surface_cursor(buf),
        "on the terminal's cursor"
    );
    let col = ctx.text(buf).unwrap().display_col(at);
    assert!(col > 0, "the prompt's end, not the line's start");
    // On it, the cursor goes with it.
    ctx.type_keys("i");
    ctx.type_keys("sleep 0.3; echo later<CR>");
    ctx.type_keys("<Esc>gg0G");
    wait_until(&mut ctx, "followed", |ctx| {
        let (text, at) = snapshot(ctx, buf);
        text.contains("\nlater\n") && at.ends_with('$')
    });
    assert_eq!(cursor(&mut ctx), ctx.surface_cursor(buf));
    let at = cursor(&mut ctx).unwrap();
    let col = ctx.text(buf).unwrap().display_col(at);
    assert!(col > 0, "the prompt's end, not the line's start");
    ctx.type_keys("3G");
    let t = ctx.text(buf).unwrap();
    assert_eq!(t.line_of(t.cursor(w).unwrap()), 2, "a count is a line");
    // `"a3p` pastes three times, in one piece. (`3"ap` is once, as in your Evil, whose `"`
    // takes the count.)
    ctx.type_keys("i");
    // Pasted only once `stty` has turned the echo off, or the terminal would echo it as well.
    ctx.type_keys("clear; stty -icanon -echo; echo \"ca\"t-ready; cat -v<CR>");
    wait_for(&mut ctx, "cat-ready");
    wait_busy(&mut ctx);
    let ab = Register {
        kind: RegKind::Char,
        text: "AB".into(),
    };
    ctx.set_register('a', ab);
    ctx.type_keys("<Esc>\"a3p");
    wait_for(&mut ctx, "ABABAB");
    assert!(!screen(&mut ctx).contains("ABABABAB"));
    ctx.type_keys("i<C-c>stty sane<CR>");
}

fn normal_state_keeps_the_programs_colors() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("printf 'plain \\033[31mred\\033[0m\\n'<CR>");
    wait_until(&mut ctx, "red printed", |ctx| {
        screen(ctx).lines().any(|l| l == "plain red")
    });
    let row = |ctx: &mut Ctx| {
        let s = ctx.scene();
        let r = (0..s.rows).find(|&r| s.row_text(r) == "plain red").unwrap();
        s.row(r).to_vec()
    };
    let before = row(&mut ctx);
    assert_ne!(before[6].fg, before[0].fg, "red is red");
    ctx.type_keys("<Esc>");
    assert_eq!(row(&mut ctx), before, "the same cells in normal state");
    ctx.type_keys("/^plain<CR>v$");
    let after = row(&mut ctx);
    let selection = ctx.face_style("selection").and_then(|s| s.bg).unwrap();
    assert_eq!((after[0].bg, after[6].bg), (selection, selection));
    assert_eq!(after[6].fg, before[6].fg, "the selection keeps the red");
    for keys in ["<Esc>x", "dd", "o", "cw", "rx", "J"] {
        ctx.type_keys(keys);
        let said = ctx.messages.last().unwrap();
        assert!(said.contains("read-only"), "{keys}: {said}");
        assert_eq!(ctx.state(), State::Normal, "{keys}");
    }
    assert!(snapshot(&mut ctx, buf).0.contains("plain red\n"));
}

fn mouse_reports_go_to_a_program_that_tracks_the_mouse() {
    let (mut ctx, _) = shell();
    ctx.set_cell_pixels(CellPixels {
        width: 8,
        height: 16,
        scale: 1.0,
    });
    ctx.type_keys("printf '\\033[?1000h\\033[?1006h'; cat -v<CR>");
    wait_busy(&mut ctx);
    settle(&mut ctx, Duration::from_millis(100));
    ctx.scene();
    let click = |kind| {
        Input::Pointer(PointerEvent {
            kind,
            row: 2,
            col: 4,
            clicks: 1,
            mods: Mods::default(),
            px: None,
        })
    };
    ctx.handle(click(PointerKind::Press(Button::Left)));
    ctx.handle(click(PointerKind::Release(Button::Left)));
    ctx.run_until_idle();
    wait_for(&mut ctx, "^[[<0;5;3M^[[<0;5;3m");
    ctx.type_keys("<C-c>printf '\\033[?1000l'<CR>");
}

fn osc_52_reaches_the_clipboard_and_osc_7_the_directory() {
    let (mut ctx, buf) = shell();
    ctx.type_keys("printf '\\033]52;c;aGVsbG8gZnJvbSBvc2M1Mg==\\a'<CR>");
    wait_until(&mut ctx, "copied", |ctx| {
        ctx.clipboard()
            .is_some_and(|r| r.text == "hello from osc52")
    });
    let dir = PathBuf::from("/usr/bin");
    let url = format!("file://localhost{}", dir.display());
    ctx.type_keys(&format!("printf '\\033]7;{url}\\a'<CR>"));
    wait_until(&mut ctx, "moved", |ctx| {
        ctx.directory(buf) == Some(dir.as_path())
    });
    let status = screen(&mut ctx).lines().nth(24).unwrap().to_string();
    assert!(status.starts_with(" /usr/bin/  *terminal*"), "{status:?}");
    ctx.type_keys("printf '\\033]2;a title\\a'<CR>");
    wait_for(&mut ctx, " /usr/bin/  a title");
}

fn term_opens_your_shell_and_term_switch_finds_the_first() {
    let terminals = Terminals::default().with_shell("/bin/sh");
    let exts: Vec<Box<dyn Extension>> = vec![Box::new(Vim::default()), Box::new(terminals)];
    let mut ctx = Ctx::new(exts).unwrap();
    let b = ctx.create_text("*scratch*", "");
    ctx.open_window(b, 80, 24).unwrap();
    ctx.run("term", &Args::default()).unwrap();
    let first = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.buffer_name(first), Some(NAME));
    assert_eq!(ctx.state(), State::Passthrough);
    ctx.type_keys("echo $0<CR>");
    wait_for(&mut ctx, "-sh");
    ctx.run("terminal", &Args::default()).unwrap();
    let second = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.buffer_name(second), Some("*terminal*<2>"));
    ctx.run("term-switch", &Args::default()).unwrap();
    assert_eq!(ctx.focused_buffer(), Some(first));
    ctx.kill(first);
    ctx.run("term-switch", &Args::default()).unwrap();
    let third = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.buffer_name(third), Some(NAME), "a new first one");
    assert_ne!(third, first);
    ctx.show(b, ShowIn::Current).unwrap();
    assert!(
        ctx.run("term-send-escape", &Args::default()).is_err(),
        "not a terminal"
    );
    // `:term cmd` runs it through the shell, as Vim's `:terminal cmd` does.
    ctx.type_keys(":term printf 'ran-%s as %s' $((40+2)) $0; sleep 5<CR>");
    let ran = ctx.focused_buffer().unwrap();
    assert_ne!(ran, b);
    assert_eq!(ctx.state(), State::Passthrough);
    wait_for(&mut ctx, "ran-42 as /bin/sh");
    assert!(busy(&mut ctx).contains(&ctx.buffer_name(ran).unwrap().to_string()));
    ctx.kill(ran);
}

fn zsh_reports_its_directory_and_reads_your_startup_files_as_ever() {
    let home = home();
    for file in [".zshenv", ".zprofile", ".zshrc", ".zlogin"] {
        let line = format!("print -r -- {file} >> \"$HOME/order\"\n");
        std::fs::write(home.join(file), line).unwrap();
    }
    let mut spec = Spec::shell(std::path::Path::new("/bin/zsh"));
    spec.env = vec![
        ("HOME".into(), home.clone().into()),
        ("ZDOTDIR".into(), home.clone().into()),
    ];
    let mut ctx = editor();
    let buf = open(&mut ctx, spec, ShowIn::Current).unwrap();
    let prompt = |ctx: &mut Ctx| screen(ctx).lines().any(|l| l.ends_with(" %"));
    wait_until(&mut ctx, "a zsh prompt", prompt);
    let order = std::fs::read_to_string(home.join("order")).unwrap();
    assert_eq!(order, ".zshenv\n.zprofile\n.zshrc\n.zlogin\n");
    ctx.type_keys("echo zd=$ZDOTDIR<CR>");
    wait_for(&mut ctx, &format!("zd={}", home.display()));
    ctx.type_keys("cd /usr/bin<CR>");
    wait_until(&mut ctx, "moved", |ctx| {
        ctx.directory(buf) == Some(std::path::Path::new("/usr/bin"))
    });
    let status = screen(&mut ctx).lines().nth(24).unwrap().to_string();
    assert!(status.starts_with(" /usr/bin/  *terminal*"), "{status:?}");
    // With no ZDOTDIR of yours, none is left for its programs.
    if std::env::var_os("ZDOTDIR").is_none() {
        let mut spec = Spec::shell(std::path::Path::new("/bin/zsh"));
        spec.env = vec![("HOME".into(), home.clone().into())];
        open(&mut ctx, spec, ShowIn::Current).unwrap();
        wait_until(&mut ctx, "another prompt", prompt);
        ctx.type_keys("echo zd=${ZDOTDIR-none}<CR>");
        wait_for(&mut ctx, "zd=none");
    }
}

/// A `.bashrc` that exports `PROMPT_COMMAND`, as a common history line does, hands DIE's hook
/// on to a bash started in the terminal, which has no such function: it said "command not
/// found" at every prompt.
fn a_nested_bash_says_nothing_of_dies_hook() {
    let export = "export PROMPT_COMMAND=\"history -a${PROMPT_COMMAND:+; $PROMPT_COMMAND}\"\n";
    let (mut ctx, _) = shell_at_its_prompt_with("/bin/bash", export);
    ctx.type_keys("bash<CR>");
    wait_busy(&mut ctx);
    ctx.type_keys("echo nested-$((1+1))<CR>");
    wait_for(&mut ctx, "nested-2");
    settle(&mut ctx, Duration::from_millis(300));
    ctx.type_keys("echo again-$((2+2))<CR>");
    wait_for(&mut ctx, "again-4");
    settle(&mut ctx, Duration::from_millis(300));
    let s = screen(&mut ctx);
    assert!(!s.contains("__die_osc7"), "{s}");
    ctx.type_keys("exit<CR>");
    wait_until(&mut ctx, "back at the prompt", |ctx| busy(ctx).is_empty());
}

/// macOS reads a PTY's foreground group as 0 until the program has made it its terminal, which
/// rustix took for a pid: a panic in a test build, undefined in a release one. Esc in normal
/// state asks at once, so a terminal can be asked from the moment it opens.
fn busy_can_be_asked_from_the_moment_a_terminal_opens() {
    let mut ctx = editor();
    let buf = open(&mut ctx, sh_spec(), ShowIn::Current).unwrap();
    let start = Instant::now();
    let mut asked = 0;
    while !screen(&mut ctx).lines().any(|l| l.ends_with('$')) && start.elapsed() < WAIT {
        assert!(
            busy(&mut ctx).is_empty(),
            "a shell that's starting isn't busy"
        );
        asked += 1;
        ctx.run_ready();
    }
    assert!(asked > 1, "asked {asked} times");
    assert!(ctx.is_live(buf));
    ctx.type_keys("sleep 5<CR>");
    wait_busy(&mut ctx);
}

/// A directory with a line break, a tab, a BEL, an Esc or a `%` in its name is reported, each
/// escaped as `%XX`: a BEL ended the OSC early, and the rest of the name went to the screen.
fn the_shells_report_a_directory_with_control_characters() {
    for program in ["/bin/bash", "/bin/zsh"] {
        let (mut ctx, buf) = shell_at_its_prompt(program);
        ctx.type_keys("mkdir -p \"$HOME\"/odd && cd \"$HOME\"/odd<CR>");
        let odd = |ctx: &mut Ctx| ctx.directory(buf).is_some_and(|d| d.ends_with("odd"));
        wait_until(&mut ctx, "in odd", odd);
        let name = "nl\nt\tbel\u{7}pc%esc\u{1b}x";
        ctx.type_keys("d=$'nl\\nt\\tbel\\apc%esc\\ex'; mkdir \"$d\" && cd \"$d\"<CR>");
        wait_until(&mut ctx, "in the odd one", |ctx| {
            ctx.directory(buf).is_some_and(|d| d.ends_with(name))
        });
        // What a BEL would have left of the name after it, `%` escaped.
        settle(&mut ctx, Duration::from_millis(200));
        let s = screen(&mut ctx);
        assert!(!s.contains("pc%25"), "{program}:\n{s}");
    }
}

/// The terminal keeps UTF-8 when it erases: Backspace over `é` in a program reading a line takes
/// both its bytes, as vterm's `stty iutf8` has it.
fn backspace_in_line_mode_erases_a_whole_character() {
    let (mut ctx, _) = shell();
    ctx.type_keys("head -n 1 | od -An -tx1<CR>");
    wait_busy(&mut ctx);
    ctx.input(KeyEvent::committed("é"));
    ctx.type_keys("<BS>a<CR>");
    wait_until(&mut ctx, "read", |ctx| busy(ctx).is_empty());
    let s = screen(&mut ctx);
    let bytes = |l: &str| l.split_whitespace().collect::<Vec<_>>() == ["61", "0a"];
    assert!(s.lines().any(bytes), "{s}");
}

fn a_quit_leaves_nothing_running_and_the_session_no_terminal() {
    let (mut ctx, buf) = shell();
    assert!(
        busy(&mut ctx).is_empty(),
        "a shell at its prompt isn't busy"
    );
    ctx.type_keys("sleep 1000 & echo job-$!<CR>");
    wait_for(&mut ctx, "job-");
    let job = number_after(&mut ctx, "job-");
    ctx.type_keys("echo shell-$$<CR>");
    wait_for(&mut ctx, "shell-");
    let sh = number_after(&mut ctx, "shell-");
    ctx.type_keys("sleep 1001<CR>");
    wait_busy(&mut ctx);
    assert_eq!(busy(&mut ctx), [NAME]);
    let json = ctx.session_json();
    let mut again = editor();
    again.restore_session(&json).unwrap();
    assert!(
        again.find_buffer(NAME).is_none(),
        "no dead terminal comes back"
    );
    assert!(again.focused_buffer().is_some());
    assert!(ctx.is_live(buf));
    ctx.shutdown();
    assert!(gone_soon(sh), "the shell {sh} is still running");
    assert!(gone_soon(job), "its background job {job} is still running");
}

type Test = fn();

fn main() {
    if let Some(code) = trampoline() {
        std::process::exit(code);
    }
    let tests: &[(&str, Test)] = &[
        (
            "typing_reaches_the_shell_and_its_output_the_cells",
            typing_reaches_the_shell_and_its_output_the_cells,
        ),
        (
            "esc_leaves_insert_on_the_main_screen_and_goes_to_the_program_in_normal",
            esc_leaves_insert_on_the_main_screen_and_goes_to_the_program_in_normal,
        ),
        (
            "esc_in_normal_never_reaches_a_shell_at_its_prompt",
            esc_in_normal_never_reaches_a_shell_at_its_prompt,
        ),
        (
            "normal_states_esc_sends_one_a_run_and_a_count_at_most_two",
            normal_states_esc_sends_one_a_run_and_a_count_at_most_two,
        ),
        (
            "esc_in_normal_during_a_job_in_line_mode_waits_for_no_prompt",
            esc_in_normal_during_a_job_in_line_mode_waits_for_no_prompt,
        ),
        (
            "esc_on_the_alternate_screen_goes_to_normal_and_esc_esc_reaches_vim_and_less",
            esc_on_the_alternate_screen_goes_to_normal_and_esc_esc_reaches_vim_and_less,
        ),
        (
            "the_toggle_sends_insert_esc_to_the_program_and_a_count_picks_auto",
            the_toggle_sends_insert_esc_to_the_program_and_a_count_picks_auto,
        ),
        (
            "a_fullscreen_program_loses_esc_to_normal_state_and_gets_one_from_the_next",
            a_fullscreen_program_loses_esc_to_normal_state_and_gets_one_from_the_next,
        ),
        (
            "the_wheel_in_normal_state_goes_to_a_program_that_tracks_the_mouse",
            the_wheel_in_normal_state_goes_to_a_program_that_tracks_the_mouse,
        ),
        (
            "a_key_let_go_in_normal_state_has_its_release_sent",
            a_key_let_go_in_normal_state_has_its_release_sent,
        ),
        (
            "c_c_c_c_in_normal_state_sends_the_program_c_c",
            c_c_c_c_in_normal_state_sends_the_program_c_c,
        ),
        (
            "leaving_the_alternate_screen_takes_normal_state_to_the_terminals_cursor",
            leaving_the_alternate_screen_takes_normal_state_to_the_terminals_cursor,
        ),
        (
            "toggle_max_width_in_normal_state_keeps_the_cursor_on_its_text",
            toggle_max_width_in_normal_state_keeps_the_cursor_on_its_text,
        ),
        (
            "a_resize_reaches_stty_size_and_exit_closes_the_buffer",
            a_resize_reaches_stty_size_and_exit_closes_the_buffer,
        ),
        (
            "a_paste_is_bracketed_when_the_program_asks",
            a_paste_is_bracketed_when_the_program_asks,
        ),
        ("a_flood_doesnt_hold_up_a_key", a_flood_doesnt_hold_up_a_key),
        (
            "slow_output_holds_a_turn_for_a_few_ms",
            slow_output_holds_a_turn_for_a_few_ms,
        ),
        (
            "the_wheel_scrolls_back_and_a_key_returns_to_the_bottom",
            the_wheel_scrolls_back_and_a_key_returns_to_the_bottom,
        ),
        (
            "normal_state_is_your_evil_over_the_scrollback",
            normal_state_is_your_evil_over_the_scrollback,
        ),
        (
            "seq_20000_then_search_yank_and_back_to_the_bottom",
            seq_20000_then_search_yank_and_back_to_the_bottom,
        ),
        (
            "new_output_is_followed_at_the_bottom_and_not_after_k",
            new_output_is_followed_at_the_bottom_and_not_after_k,
        ),
        (
            "a_frozen_window_draws_what_it_yanks_through_a_clear_and_a_flood",
            a_frozen_window_draws_what_it_yanks_through_a_clear_and_a_flood,
        ),
        (
            "esc_after_the_wheel_keeps_the_place_it_scrolled_to",
            esc_after_the_wheel_keeps_the_place_it_scrolled_to,
        ),
        (
            "g_goes_to_the_terminals_cursor_and_p_takes_a_count",
            g_goes_to_the_terminals_cursor_and_p_takes_a_count,
        ),
        (
            "normal_state_keeps_the_programs_colors",
            normal_state_keeps_the_programs_colors,
        ),
        (
            "mouse_reports_go_to_a_program_that_tracks_the_mouse",
            mouse_reports_go_to_a_program_that_tracks_the_mouse,
        ),
        (
            "osc_52_reaches_the_clipboard_and_osc_7_the_directory",
            osc_52_reaches_the_clipboard_and_osc_7_the_directory,
        ),
        (
            "term_opens_your_shell_and_term_switch_finds_the_first",
            term_opens_your_shell_and_term_switch_finds_the_first,
        ),
        (
            "zsh_reports_its_directory_and_reads_your_startup_files_as_ever",
            zsh_reports_its_directory_and_reads_your_startup_files_as_ever,
        ),
        (
            "a_nested_bash_says_nothing_of_dies_hook",
            a_nested_bash_says_nothing_of_dies_hook,
        ),
        (
            "busy_can_be_asked_from_the_moment_a_terminal_opens",
            busy_can_be_asked_from_the_moment_a_terminal_opens,
        ),
        (
            "the_shells_report_a_directory_with_control_characters",
            the_shells_report_a_directory_with_control_characters,
        ),
        (
            "backspace_in_line_mode_erases_a_whole_character",
            backspace_in_line_mode_erases_a_whole_character,
        ),
        (
            "a_quit_leaves_nothing_running_and_the_session_no_terminal",
            a_quit_leaves_nothing_running_and_the_session_no_terminal,
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
        let start = Instant::now();
        match catch_unwind(AssertUnwindSafe(test)) {
            Ok(()) => println!("test {name} ... ok ({:?})", start.elapsed()),
            Err(_) => {
                println!("test {name} ... FAILED");
                failed += 1;
            }
        }
    }
    println!("test result: {failed} failed");
    std::process::exit(i32::from(failed > 0));
}
