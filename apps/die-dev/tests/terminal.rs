//! Your config's terminal on a real PTY: `\e` starts `/bin/bash` as your Emacs's ghostel and
//! vterm do, interactive and not a login shell, so it reads `~/.bashrc`; bash reports `cd` and
//! keeps your `PROMPT_COMMAND` and `$?`; Esc pressed a few times at its prompt reaches nothing,
//! where it opened bash's "Display all 1773 possibilities?"; and the terminal is 80 columns,
//! centered, as a file is, until `\z` widens it to the window.
//!
//! A PTY's program starts through this binary's own trampoline, as it does through DIE's, so
//! this has no test harness: `main` is the trampoline first. bash would read your `~/.bashrc`,
//! so the test runs again in a process of its own, with a HOME and a ZDOTDIR of its own, which
//! DIE's login PATH reads too. Only macOS has terminals (D10).

use die_test_cap as _;

#[cfg(target_os = "macos")]
mod on_macos {
    use die::core::*;
    use die::start;
    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;

    const CHILD: &str = "DIE_DEV_TERMINAL_TEST_CHILD";
    const WAIT: Duration = Duration::from_secs(10);

    /// A `.bashrc` with a prompt of its own and a `PROMPT_COMMAND` that keeps `$?`, as a careful
    /// one does.
    const BASHRC: &str = r#"PS1='dev-bash$ '
mine() { local s=$?; echo "mine-$s"; return $s; }
PROMPT_COMMAND=mine
"#;

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

    fn wait_for(ctx: &mut Ctx, needle: &str) {
        wait(ctx, needle, |ctx| screen(ctx).contains(needle));
    }

    /// The test, in the process with its own HOME.
    fn leader_e_starts_bin_bash_reading_bashrc_and_reporting_cd() {
        let mut ctx = start(die_dev::config(vec![])).unwrap();
        ctx.type_keys("<leader>e");
        assert_eq!(ctx.state(), State::Passthrough, "{:?}", ctx.messages);
        wait_for(&mut ctx, "dev-bash$");
        ctx.type_keys("echo shell=$0; shopt -q login_shell || echo not-login<CR>");
        wait_for(&mut ctx, "shell=/bin/bash\nnot-login");
        // Esc a few times at the prompt, then `ls`: neither "Display all" nor `M-l`.
        ctx.type_keys("<Esc><Esc><Esc><Esc>");
        assert_eq!(ctx.state(), State::Normal);
        let _ = ctx.run_until(Duration::from_millis(300), |_| false);
        ctx.type_keys("ilx=ok; echo got-$lx<CR>");
        wait_for(&mut ctx, "got-ok");
        assert!(!screen(&mut ctx).contains("Display all"));
        ctx.type_keys("cd /usr/bin<CR>");
        let buf = ctx.focused_buffer().unwrap();
        wait(&mut ctx, "moved", |ctx| {
            ctx.directory(buf) == Some(Path::new("/usr/bin"))
        });
        let status = screen(&mut ctx).lines().nth(24).unwrap().to_string();
        assert!(status.starts_with(" /usr/bin/  *terminal*"), "{status:?}");
        // Your PROMPT_COMMAND still runs, and sees the command's status; so does one added after
        // DIE's hook, which keeps it.
        ctx.type_keys("(exit 3)<CR>");
        wait_for(&mut ctx, "mine-3");
        ctx.type_keys("echo status-$?<CR>");
        wait_for(&mut ctx, "status-3");
        ctx.type_keys("PROMPT_COMMAND=\"$PROMPT_COMMAND\"$'\\n''echo after-$?'<CR>");
        ctx.type_keys("(exit 4)<CR>");
        wait_for(&mut ctx, "after-4");
        ctx.type_keys("cd /<CR>");
        wait(&mut ctx, "moved again", |ctx| {
            ctx.directory(buf) == Some(Path::new("/"))
        });
        ctx.shutdown();
    }

    /// The test, in the process with its own HOME: in a window 120 columns wide, the terminal
    /// is your text's 80, centered, and `\z` in normal state widens it to the window and back,
    /// as `stty size` says.
    fn the_terminal_is_80_columns_centered_until_leader_z() {
        let mut ctx = start(die_dev::config(vec![])).unwrap();
        ctx.set_frame_size(120, 26);
        ctx.type_keys("<leader>e");
        wait_for(&mut ctx, "dev-bash$");
        let w = ctx.focused().unwrap();
        let area = ctx.surface_area(w).unwrap();
        assert_eq!((area.text_x, area.text_w, area.rows), (20, 80, 24));
        let prompt = screen(&mut ctx);
        let prompt = prompt.lines().find(|l| l.contains("dev-bash$")).unwrap();
        assert!(
            prompt.starts_with(&format!("{}dev-bash$", " ".repeat(20))),
            "{prompt:?}"
        );
        ctx.type_keys("echo one-$(stty size)<CR>");
        wait_for(&mut ctx, "one-24 80");
        let size = |ctx: &mut Ctx, keys: &str, want: &str| {
            ctx.type_keys("<Esc>");
            assert_eq!(ctx.state(), State::Normal);
            ctx.type_keys(keys);
            let _ = ctx.run_until(Duration::from_millis(100), |_| false);
            ctx.type_keys("i");
            assert_eq!(ctx.state(), State::Passthrough);
            ctx.type_keys(&format!("echo {want}-$(stty size)<CR>"));
        };
        size(&mut ctx, "<leader>z", "two");
        wait_for(&mut ctx, "two-24 120");
        assert_eq!(ctx.surface_area(w).unwrap().text_x, 0);
        size(&mut ctx, "<leader>z", "three");
        wait_for(&mut ctx, "three-24 80");
        ctx.shutdown();
    }

    pub fn main() {
        if std::env::var_os(CHILD).is_some() {
            leader_e_starts_bin_bash_reading_bashrc_and_reporting_cd();
            println!("test leader_e_starts_bin_bash_reading_bashrc_and_reporting_cd ... ok");
            the_terminal_is_80_columns_centered_until_leader_z();
            println!("test the_terminal_is_80_columns_centered_until_leader_z ... ok");
            return;
        }
        let home = std::env::temp_dir().join(format!("die-dev-terminal-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(".bashrc"), BASHRC).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .env(CHILD, "1")
            .env("HOME", &home)
            .env("ZDOTDIR", &home)
            .env("SHELL", "/bin/sh")
            .env("BASH_SILENCE_DEPRECATION_WARNING", "1")
            .env_remove("PROMPT_COMMAND")
            .env_remove("BASH_ENV")
            .env_remove("ENV")
            .status()
            .unwrap();
        let _ = std::fs::remove_dir_all(&home);
        if !status.success() {
            println!("test result: FAILED");
            std::process::exit(1);
        }
        println!("test result: 0 failed");
    }
}

fn main() {
    if let Some(code) = die::other_mode() {
        std::process::exit(code);
    }
    #[cfg(target_os = "macos")]
    on_macos::main();
}
