//! die-term in the DIE you run, on real PTYs (scope D18): Cmd-Q asks first while a program runs
//! in a terminal, a program's OSC 52 copy is a kill `p` pastes, and a terminal's normal state has
//! `:` and the window keys.
//!
//! A PTY's program starts through this binary's own trampoline, as it does through DIE's, so
//! this has no test harness: `main` is the trampoline first, then runs the tests itself. Only
//! macOS has terminals (D10).

use die_test_cap as _;

#[cfg(target_os = "macos")]
mod on_macos {
    use die::core::*;
    use die::term::{NAME, Spec, busy, open};
    use die::{Config, start};
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(10);

    /// DIE as you run it, with a login `/bin/sh` of its own in a terminal, at its prompt.
    fn shell() -> (Ctx, BufferId) {
        let mut ctx = start(Config::default()).unwrap();
        let home = std::env::temp_dir().join(format!("die-terminals-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let mut spec = Spec::new(NAME, "/bin/sh");
        spec.login = true;
        spec.env = vec![
            ("HOME".into(), home.into()),
            ("BASH_SILENCE_DEPRECATION_WARNING".into(), "1".into()),
        ];
        let buf = open(&mut ctx, spec, ShowIn::Current).unwrap();
        wait(&mut ctx, "a prompt", |ctx| {
            screen(ctx).lines().any(|l| l.ends_with('$'))
        });
        (ctx, buf)
    }

    fn screen(ctx: &mut Ctx) -> String {
        let s = ctx.scene();
        let rows: Vec<String> = (0..s.rows).map(|r| s.row_text(r)).collect();
        rows.join("\n")
    }

    fn wait(ctx: &mut Ctx, what: &str, mut done: impl FnMut(&mut Ctx) -> bool) {
        let found = ctx.run_until(WAIT, |ctx| {
            ctx.scene();
            done(ctx)
        });
        assert!(found, "never {what}:\n{}", screen(ctx));
    }

    pub fn cmd_q_asks_while_a_program_runs_in_a_terminal() {
        let (mut ctx, _) = shell();
        ctx.type_keys("sleep 1000<CR>");
        wait(&mut ctx, "busy", |ctx| busy(ctx) == [NAME]);
        ctx.type_keys("<D-q>");
        assert!(!ctx.quit_wanted(), "it asks first");
        assert!(ctx.prompt_text().is_some(), "{:?}", ctx.messages);
        let asked = screen(&mut ctx)
            .lines()
            .last()
            .unwrap_or_default()
            .to_string();
        let want = "A program is running in *terminal*. Quit anyway? (y or n)";
        assert!(asked.starts_with(want), "{asked:?}");
        ctx.type_keys("n<CR>");
        assert!(!ctx.quit_wanted());
        ctx.type_keys("<D-q>y<CR>");
        assert!(ctx.quit_wanted(), "{:?}", ctx.messages);
        // A shell at its prompt isn't asked about.
        let (mut ctx, _) = shell();
        ctx.type_keys("<D-q>");
        assert!(ctx.quit_wanted(), "{:?}", ctx.messages);
    }

    pub fn an_osc_52_copy_is_a_kill_that_p_pastes() {
        let (mut ctx, _) = shell();
        ctx.type_keys("printf '\\033]52;c;Y29waWVk\\a'<CR>");
        wait(&mut ctx, "copied", |ctx| {
            ctx.register('"').is_some_and(|r| r.text == "copied")
        });
        assert_eq!(ctx.clipboard().unwrap().text, "copied");
        let b = ctx.create_text("t", "");
        ctx.show(b, ShowIn::Current).unwrap();
        ctx.type_keys("p");
        assert_eq!(ctx.text(b).unwrap().rope().to_string(), "copied");
    }

    pub fn normal_state_has_the_ex_line_and_the_window_keys() {
        let (mut ctx, buf) = shell();
        ctx.type_keys("<Esc>");
        assert_eq!(ctx.state(), State::Normal);
        ctx.type_keys("<C-w>v");
        assert_eq!(ctx.window_list().len(), 2, "{:?}", ctx.messages);
        ctx.type_keys("<C-w>l");
        assert_eq!(ctx.focused_buffer(), Some(buf), "both show the terminal");
        ctx.type_keys(":q<CR>");
        assert_eq!(ctx.window_list().len(), 1, "{:?}", ctx.messages);
        assert!(ctx.is_live(buf));
    }
}

fn main() {
    if let Some(code) = die::other_mode() {
        std::process::exit(code);
    }
    #[cfg(target_os = "macos")]
    {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let tests: &[(&str, fn())] = &[
            (
                "cmd_q_asks_while_a_program_runs_in_a_terminal",
                on_macos::cmd_q_asks_while_a_program_runs_in_a_terminal,
            ),
            (
                "an_osc_52_copy_is_a_kill_that_p_pastes",
                on_macos::an_osc_52_copy_is_a_kill_that_p_pastes,
            ),
            (
                "normal_state_has_the_ex_line_and_the_window_keys",
                on_macos::normal_state_has_the_ex_line_and_the_window_keys,
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
        println!("test result: {failed} failed");
        std::process::exit(i32::from(failed > 0));
    }
}
