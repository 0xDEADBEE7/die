//! Your config, as `init.el` has it: the file's directory in the status line, leader
//! `\`, mono-dark, Ioskeley Mono Term with braille pinned, wrap, width and line numbers
//! by pattern, and only bindings whose commands exist (C8, at every checkpoint).

use die_test_cap as _;

use die::core::{Ctx, LineNumbers, Rgb, Settings, WindowPlacement, Wrap};
use die::render::font::{Draw, RangeRule};
use die_dev::{Args, config, directory, parse_args, status_text};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("die-dev-config-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn screen(ctx: &mut Ctx, cols: usize, rows: usize) -> Vec<String> {
    let window = ctx.focused().unwrap();
    ctx.resize(window, cols, rows - 2).unwrap();
    let place = WindowPlacement {
        window,
        col: 0,
        row: 0,
    };
    let s = ctx.compose(cols, rows, &[place]);
    (0..rows)
        .map(|r| {
            let row = s.row(r).iter().map(|c| c.glyph.to_string());
            row.collect::<String>().trim_end().to_owned()
        })
        .collect()
}

fn settings(file: &str) -> Settings {
    // Two tests ask about the same name at once, and each gets a directory of its own.
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let d = dir(&format!(
        "settings-{}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let path = d.join(file);
    std::fs::write(&path, "x\n").unwrap();
    let ctx = die::start(config(vec![path])).unwrap();
    let s = ctx.text(ctx.focused_buffer().unwrap()).unwrap();
    let s = s.settings().clone();
    std::fs::remove_dir_all(&d).unwrap();
    s
}

#[test]
fn every_binding_names_a_command_that_exists() {
    die::start(config(vec![])).unwrap();
}

#[test]
fn the_startup_snapshot() {
    let d = dir("snapshot");
    let path = d.join("notes.txt");
    std::fs::write(&path, "hello\nworld\n").unwrap();
    let mut ctx = die::start(config(vec![path])).unwrap();
    let got = screen(&mut ctx, 90, 6);
    let pad = " ".repeat(5);
    let home = std::env::home_dir();
    let status = format!(" {}/", directory(&d, home.as_deref()));
    let want = [
        format!("{pad}hello"),
        format!("{pad}world"),
        String::new(),
        String::new(),
        status,
        String::new(),
    ];
    assert_eq!(got, want);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn mono_dark_is_the_theme() {
    let ctx = die::start(config(vec![])).unwrap();
    let d = ctx.face_style("default").unwrap();
    assert_eq!(d.bg, Some(Rgb::hex("#1a1a1a")));
}

#[test]
fn the_leader_is_backslash() {
    let ctx = die::start(config(vec![])).unwrap();
    let backslash = die::core::parse_keys("\\").unwrap()[0].chord();
    assert_eq!(ctx.leader(), backslash);
}

#[test]
fn leader_z_toggles_the_width_as_visual_fill_column_mode_does() {
    let mut ctx = die::start(config(vec![])).unwrap();
    let b = ctx.focused_buffer().unwrap();
    assert_eq!(ctx.text(b).unwrap().settings().max_width, Some(80));
    ctx.type_keys("\\z");
    let off = ctx.text(b).unwrap().settings().max_width;
    assert_eq!(off, None, "{:?}", ctx.messages);
    ctx.type_keys("\\z");
    assert_eq!(ctx.text(b).unwrap().settings().max_width, Some(80));
}

#[test]
fn text_wraps_at_80_centered_everywhere_as_visual_fill_column_does() {
    for file in ["a.txt", "a.py", "Makefile"] {
        let s = settings(file);
        assert_eq!(s.wrap, Wrap::Word, "{file}");
        assert_eq!(s.max_width, Some(80), "{file}");
        assert!(s.center, "{file}");
    }
}

#[test]
fn line_numbers_are_on_for_the_modes_your_init_el_turns_them_on_for() {
    for file in [
        // `set-auto-mode` under `emacs -Q` (30.2) for the modes `init.el` hooks, and those
        // derived from them, as `derived-mode-p` has it. It folds case when nothing matches
        // exactly.
        "a.py", "a.pyi", "a.pyw", "a.c", "a.h", "a.i", "a.lex", "a.y", "a.C", "a.H", "a.cc",
        "a.cpp", "a.cxx", "a.c++", "a.hh", "a.hpp", "a.hxx", "a.h++", "a.ii", "a.el", "a.js",
        "a.jsx", "a.html", "a.htm", "a.xhtml", "a.shtml", "a.shtm", "a.css", "a.scss", "a.less",
        "a.PY", "a.Html", "a.JS",
    ] {
        assert_eq!(settings(file).line_numbers, LineNumbers::Visual, "{file}");
    }
    for file in [
        "a.txt", "a.md", "a.rs", "Makefile", "py", "a.PY.bak", "a.mjs", "a.cjs", "a.json", "a.ts",
        "a.xml", "a.xht", "a.sass", "a.inl", "a.m",
    ] {
        assert_eq!(settings(file).line_numbers, LineNumbers::Off, "{file}");
    }
}

#[test]
fn the_font_is_ioskeley_mono_term_at_16_with_braille_pinned_to_it() {
    let c = config(vec![]);
    assert_eq!(c.font.points, 16.0);
    let term = c.font.faces[0].file_name().unwrap().to_str().unwrap();
    assert!(term.starts_with("IoskeleyMonoTerm-Regular"), "{term}");
    let braille = |r: &&RangeRule| r.range.contains(&'\u{2800}') && r.range.contains(&'\u{28FF}');
    let rule = c.font.rules.iter().find(braille).expect("a braille rule");
    assert_eq!(rule.draw, Draw::Primary);
}

#[test]
fn the_directory_shows_as_your_mode_line_does() {
    let home = Some(Path::new("/Users/me"));
    assert_eq!(directory(Path::new("/Users/me/src/a"), home), "~/src/a");
    assert_eq!(directory(Path::new("/Users/me"), home), "~");
    let other = directory(Path::new("/Users/me2/src"), home);
    assert_eq!(other, "/Users/me2/src", "another user's home isn't yours");
    assert_eq!(directory(Path::new("/tmp"), home), "/tmp");
    assert_eq!(directory(Path::new("/tmp"), None), "/tmp");
}

#[test]
fn a_buffer_visiting_no_file_shows_where_die_started() {
    let f = status_text(Path::new("/started/here"), None);
    assert_eq!(f("*scratch*", None), "/started/here/");
}

#[test]
fn a_file_in_the_home_directory_shows_a_tilde_and_a_slash() {
    let d = dir("home-status");
    let path = d.join("a.txt");
    std::fs::write(&path, "x\n").unwrap();
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.open_file(&path).unwrap();
    let f = status_text(Path::new("/started"), Some(&d));
    assert_eq!(f("a.txt", ctx.file(b)), "~/");
    let f = status_text(Path::new("/started"), None);
    assert_eq!(f("a.txt", ctx.file(b)), format!("{}/", d.display()));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_file_at_the_root_shows_one_slash() {
    assert_eq!(directory(Path::new("/"), None), "/");
    let mut ctx = Ctx::new(vec![]).unwrap();
    // Opens empty, or as whatever's there. Only its path matters here.
    let b = ctx
        .open_file(Path::new("/die-no-such-file-at-the-root"))
        .unwrap();
    let f = status_text(Path::new("/started"), None);
    assert_eq!(f("x", ctx.file(b)), "/");
}

#[test]
fn arguments_are_files_and_checks() {
    let parse = |a: &[&str]| parse_args(a.iter().map(|s| s.to_string()));
    let args = |check: Option<&[&str]>, files: &[&str]| Args {
        check: check.map(|c| c.iter().map(|s| s.to_string()).collect()),
        files: files.iter().map(PathBuf::from).collect(),
        launch: Default::default(),
    };
    assert_eq!(parse(&[]), Ok(args(None, &[])));
    assert_eq!(parse(&["a", "b"]), Ok(args(None, &["a", "b"])));
    assert_eq!(
        parse(&["--check", "idle,scale"]),
        Ok(args(Some(&["idle", "scale"]), &[]))
    );
    assert_eq!(
        parse(&["--check", "show", "a.txt"]),
        Ok(args(Some(&["show"]), &["a.txt"]))
    );
    assert!(parse(&["--check"]).is_err());
    assert!(parse(&["--check", "nope"]).unwrap_err().contains("nope"));
    assert!(parse(&["--wat"]).unwrap_err().contains("--wat"));
    assert_eq!(parse(&["--", "--wat"]), Ok(args(None, &["--wat"])));
    assert!(parse(&["-h"]).unwrap_err().contains("-h"));
    assert!(parse(&["-x", "a.txt"]).unwrap_err().contains("-x"));
    assert_eq!(parse(&["-"]), Ok(args(None, &["-"])), "a file named -");
    assert_eq!(parse(&["--", "-h"]), Ok(args(None, &["-h"])));
}

/// Started through a symlink: the status line and a relative file keep the path you came
/// by, as Emacs's `default-directory` does. In a child, which can have its own directory.
#[test]
fn starting_through_a_symlink_shows_the_path_you_came_by() {
    if std::env::var_os("DIE_SYMLINK_CHILD").is_some() {
        let link = std::env::var_os("PWD").map(PathBuf::from).unwrap();
        let shown = format!(" {}/", directory(&link, std::env::home_dir().as_deref()));
        let mut ctx = die::start(config(vec![])).unwrap();
        assert_eq!(screen(&mut ctx, 300, 6)[4], shown, "a buffer with no file");
        let mut ctx = die::start(config(vec![PathBuf::from("f.txt")])).unwrap();
        assert_eq!(screen(&mut ctx, 300, 6)[4], shown, "a relative file");
        let file = ctx.file(ctx.focused_buffer().unwrap()).unwrap();
        assert_eq!(file.path, link.join("f.txt"));
        return;
    }
    let d = dir("symlink");
    let real = d.join("real");
    let link = d.join("link");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("f.txt"), "x\n").unwrap();
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "starting_through_a_symlink_shows_the_path_you_came_by",
        ])
        .current_dir(&link)
        .env("PWD", &link)
        .env("DIE_SYMLINK_CHILD", "1")
        .output()
        .unwrap();
    std::fs::remove_dir_all(&d).unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
}
