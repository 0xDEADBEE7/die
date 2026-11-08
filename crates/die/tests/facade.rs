//! What `die::run` does before it opens a window: die-vim is always there, the clock
//! is real, config errors all come back, files named at startup open, and one window
//! shows the first of them.

use die_test_cap as _;

use die::core::{Clock, Ctx, Registry, State, WindowPlacement};
use die::{Config, start};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::Duration;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("die-facade-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn screen(ctx: &mut Ctx) -> Vec<String> {
    let window = ctx.focused().unwrap();
    let (cols, rows) = ctx.window_size(window).unwrap();
    let place = WindowPlacement {
        window,
        col: 0,
        row: 0,
    };
    let s = ctx.compose(cols, rows + 2, &[place]);
    (0..s.rows)
        .map(|r| {
            let row = s.row(r).iter().map(|c| c.glyph.to_string());
            row.collect::<String>().trim_end().to_owned()
        })
        .collect()
}

fn with_bad_bindings() -> Config {
    let mut config = Config::default();
    config.core.register = Some(Box::new(|reg: &mut Registry| {
        reg.global_keymap().bind(State::Normal, "a", "no-such-one");
        reg.global_keymap().bind(State::Normal, "b", "no-such-two");
    }));
    config
}

#[test]
fn die_vim_is_always_registered() {
    let ctx = start(Config::default()).unwrap();
    assert!(ctx.command_doc("vim-insert").is_some());
}

#[test]
fn the_clock_is_real_or_timers_would_never_fire() {
    let ctx = start(Config::default()).unwrap();
    let a = ctx.now();
    std::thread::sleep(Duration::from_millis(3));
    assert!(ctx.now() > a, "a fake clock stands still");
}

#[test]
fn a_config_cant_put_a_fake_clock_back() {
    let mut config = Config::default();
    config.core.clock = Clock::default();
    let ctx = start(config).unwrap();
    let a = ctx.now();
    std::thread::sleep(Duration::from_millis(3));
    assert!(ctx.now() > a);
}

#[test]
fn every_problem_in_the_config_comes_back_at_once() {
    let errors = start(with_bad_bindings())
        .err()
        .expect("C8: every bound name exists");
    let all = errors.join("\n");
    assert!(
        all.contains("no-such-one") && all.contains("no-such-two"),
        "{all}"
    );
}

#[test]
fn run_says_why_it_cant_start() {
    assert_eq!(die::run(with_bad_bindings()), 2);
}

#[test]
fn with_no_files_one_window_shows_an_empty_buffer() {
    let ctx = start(Config::default()).unwrap();
    let b = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.buffer_name(b), Some("*scratch*"));
    assert_eq!(ctx.text(b).unwrap().len(), 0);
}

#[test]
fn the_first_file_named_is_the_one_on_screen_and_the_rest_are_open() {
    let d = Dir::new("two");
    let (a, b) = (d.file("a.txt", "first\n"), d.file("b.txt", "second\n"));
    let config = Config {
        files: vec![a.clone(), b],
        ..Config::default()
    };
    let mut ctx = start(config).unwrap();
    assert_eq!(screen(&mut ctx)[0], "first");
    assert!(ctx.find_buffer("b.txt").is_some());
    assert_eq!(ctx.file(ctx.focused_buffer().unwrap()).unwrap().path, a);
}

#[test]
fn a_relative_path_is_from_where_die_started() {
    let here = std::env::current_dir().unwrap();
    let d = Dir::new("relative");
    let p = d.file("c.txt", "relative\n");
    let up = here.components().count() - 1;
    let rel = std::iter::repeat_n("..", up)
        .collect::<PathBuf>()
        .join(p.strip_prefix("/").unwrap());
    let config = Config {
        files: vec![rel],
        ..Config::default()
    };
    let mut ctx = start(config).unwrap();
    assert_eq!(screen(&mut ctx)[0], "relative");
    assert_eq!(ctx.file(ctx.focused_buffer().unwrap()).unwrap().path, p);
}

#[test]
fn a_missing_file_opens_empty_as_core_says() {
    let d = Dir::new("missing");
    let p = d.0.join("new.txt");
    let config = Config {
        files: vec![p.clone()],
        ..Config::default()
    };
    let ctx = start(config).unwrap();
    let b = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.text(b).unwrap().len(), 0);
    assert_eq!(ctx.file(b).unwrap().path, p);
}

#[test]
fn a_file_that_wont_open_is_logged_and_the_editor_starts() {
    let d = Dir::new("bad");
    let (dir, ok) = (d.0.clone(), d.file("ok.txt", "fine\n"));
    let config = Config {
        files: vec![dir.clone(), ok],
        ..Config::default()
    };
    let mut ctx = start(config).unwrap();
    assert_eq!(screen(&mut ctx)[0], "fine", "the next file is on screen");
    let name = dir.display().to_string();
    let logged = ctx.messages.iter().any(|m| m.contains(&name));
    assert!(logged, "{:?}", ctx.messages);
    let plain = format!("{name}: not a regular file");
    assert_eq!(ctx.messages, [plain], "for a person, not Rust's Debug");
}

fn hook_opens_a_term() -> Config {
    let mut config = Config::default();
    config.core.startup = Some(Box::new(|ctx: &mut Ctx| {
        let term = ctx.create_text("*term*", "");
        ctx.open_window(term, 80, 24)?;
        Ok(())
    }));
    config
}

#[test]
fn with_no_files_a_window_the_startup_hook_opened_stays_on_screen() {
    let ctx = start(hook_opens_a_term()).unwrap();
    let b = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.buffer_name(b), Some("*term*"));
    assert!(
        ctx.find_buffer("*scratch*").is_none(),
        "nothing asked for it"
    );
}

#[test]
fn a_file_named_takes_the_screen_from_the_startup_hooks_window() {
    let d = Dir::new("hook");
    let config = Config {
        files: vec![d.file("a.txt", "named\n")],
        ..hook_opens_a_term()
    };
    let mut ctx = start(config).unwrap();
    assert_eq!(screen(&mut ctx)[0], "named", "as after `after-init-hook`");
    assert!(ctx.find_buffer("*term*").is_some());
}

#[test]
fn where_die_started_is_pwd_when_that_names_the_real_directory() {
    let d = Dir::new("pwd");
    let real = d.0.join("real");
    let other = d.0.join("other");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let link = d.0.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let from = |pwd: Option<&OsStr>| die::started_dir_from(pwd, real.clone());
    assert_eq!(from(Some(link.as_os_str())), link, "through a symlink");
    assert_eq!(from(Some(real.as_os_str())), real);
    assert_eq!(from(None), real);
    assert_eq!(from(Some(other.as_os_str())), real, "a stale $PWD");
    assert_eq!(from(Some(OsStr::new("real"))), real, "a relative one");
    assert_eq!(from(Some(OsStr::new("/no/such/dir"))), real);
}

#[test]
fn the_status_text_field_reaches_the_status_line() {
    let config = Config {
        status_text: Some(Box::new(|name, _| format!("<{name}>"))),
        ..Config::default()
    };
    let mut ctx = start(config).unwrap();
    assert!(screen(&mut ctx).iter().any(|l| l == " <*scratch*>"));
}

#[test]
fn by_default_the_status_line_shows_the_buffers_name() {
    let mut ctx = start(Config::default()).unwrap();
    assert!(screen(&mut ctx).iter().any(|l| l == " *scratch*"));
}

#[test]
fn a_bundle_uses_its_own_fonts_unless_told_otherwise() {
    use die::render::{bundle_of, font_dir_from};
    let exe = PathBuf::from("/Apps/DIE.app/Contents/MacOS/die");
    assert_eq!(bundle_of(&exe), Some(PathBuf::from("/Apps/DIE.app")));
    assert_eq!(
        font_dir_from(None, Some(&exe)),
        PathBuf::from("/Apps/DIE.app/Contents/Resources/fonts")
    );
    assert_eq!(
        font_dir_from(Some("/elsewhere".into()), Some(&exe)),
        PathBuf::from("/elsewhere")
    );
    let plain = PathBuf::from("/repo/target/debug/die");
    assert_eq!(bundle_of(&plain), None);
    assert!(font_dir_from(None, Some(&plain)).ends_with("fonts"));
}

#[test]
fn m_x_opens_the_command_palette_in_normal_insert_replace_and_visual() {
    for keys in ["<A-x>", "i<A-x>", "R<A-x>", "v<A-x>"] {
        let mut ctx = start(Config::default()).unwrap();
        ctx.type_keys(keys);
        ctx.step();
        assert_eq!(ctx.prompt_text(), Some(""), "{keys}: {:?}", ctx.messages);
        assert!(ctx.messages.is_empty(), "{keys}: {:?}", ctx.messages);
    }
    let mut ctx = start(Config::default()).unwrap();
    let b = ctx.focused_buffer().unwrap();
    ctx.type_keys("iab<Esc><A-x>vim-undo<CR>");
    ctx.step();
    assert_eq!(ctx.prompt_text(), None);
    assert_eq!(ctx.text(b).unwrap().len(), 0, "{:?}", ctx.messages);
}

#[test]
fn colon_q_in_the_last_window_and_cmd_q_ask_the_loop_to_stop() {
    for keys in [":q<CR>", "<D-q>", "i<D-q>"] {
        let mut ctx = start(Config::default()).unwrap();
        ctx.type_keys(keys);
        ctx.step();
        assert!(ctx.quit_wanted(), "{keys}: {:?}", ctx.messages);
    }
}
