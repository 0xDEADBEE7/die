//! The rendered view through the editor: a markdown file's key shows it rendered and raw again
//! over the same text, the view follows the text and the width, a restart brings it back, and a
//! link goes where it leads. Nothing is written outside a temporary directory, and nothing is
//! opened outside DIE: the opener is a script that writes down what it was given.

use die_test_cap as _;

use die_core::{BufferId, Config, Extension, Harness, Registry, Settings, ShowIn, State};
use die_markdown::{MODE, Markdown, Options, View};
use die_vim::Vim;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const SAMPLE: &str =
    "# Title\n\nSome *italic* and **bold** with `code`.\n\n- one\n- [x] done\n\n> quoted\n\n---\n";

/// A fresh directory for one test.
fn tree(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("die-markdown-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

/// die-vim and the extension, with the toggle on `\m` in the mode's keymap as your config has
/// it, 40 columns of text in a window of 60 by 20, on an empty buffer.
fn harness(options: Options) -> Harness {
    let exts: Vec<Box<dyn Extension>> =
        vec![Box::new(Vim::default()), Box::new(Markdown::new(options))];
    let config = Config {
        register: Some(Box::new(|reg: &mut Registry| {
            // Your leader has other keys everywhere, so `\` alone only starts one.
            let keys = reg.global_keymap();
            keys.bind(State::Normal, "<leader>z", "toggle-max-width");
            let keys = reg.mode_keymap(MODE).unwrap();
            keys.bind(State::Normal, "<leader>m", "markdown-toggle");
        })),
        text_defaults: Settings {
            max_width: Some(40),
            ..Settings::default()
        },
        ..Config::default()
    };
    let mut h = Harness::with_config(exts, config).unwrap();
    h.open_text("*scratch*", "").unwrap();
    h.set_window_size(60, 20);
    h
}

/// Open a file in the focused window.
fn open(h: &mut Harness, path: &Path) -> BufferId {
    let buf = h.open_file(path).unwrap();
    h.show(buf, ShowIn::Current).unwrap();
    h.run_until_idle();
    buf
}

/// Type keys that must all work.
fn keys(h: &mut Harness, k: &str) {
    for k in h.type_keys(k).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
}

/// The text rows of the screen, down to the last one with anything on it.
fn rows(h: &mut Harness) -> Vec<String> {
    let screen = h.screen();
    let mut rows: Vec<String> = screen.lines().take(18).map(String::from).collect();
    while rows.last().is_some_and(|r| r.is_empty()) {
        rows.pop();
    }
    rows
}

fn text(h: &Harness, buf: BufferId) -> String {
    h.text(buf).unwrap().rope().to_string()
}

fn rendered(h: &mut Harness, buf: BufferId) -> bool {
    h.with_mode(buf, |v: &mut View, _| v.is_rendered())
        .unwrap_or(false)
}

#[test]
fn the_key_shows_a_markdown_file_rendered_and_raw_again_over_the_same_text() {
    let dir = tree("toggle");
    fs::write(dir.join("notes.md"), SAMPLE).unwrap();
    fs::write(dir.join("notes.txt"), SAMPLE).unwrap();
    let mut h = harness(Options::default());
    let buf = open(&mut h, &dir.join("notes.md"));
    let raw = rows(&mut h);
    assert_eq!(raw, SAMPLE.lines().collect::<Vec<_>>());
    assert!(!rendered(&mut h, buf));

    keys(&mut h, "\\m");
    assert!(rendered(&mut h, buf));
    let rule = "─".repeat(40);
    let shown = [
        "Title",
        "",
        "Some italic and bold with code.",
        "",
        "• one",
        "☑ done",
        "",
        "▎ quoted",
        "",
        rule.as_str(),
    ];
    assert_eq!(rows(&mut h), shown);
    assert_eq!(text(&h, buf), SAMPLE, "the text is the file's, untouched");
    assert!(!h.text(buf).unwrap().modified());

    keys(&mut h, "\\m");
    assert!(!rendered(&mut h, buf));
    assert_eq!(rows(&mut h), raw);
    assert_eq!(h.text(buf).unwrap().decorations().count(), 0);

    // A file that isn't markdown has no such key, and the command still works on it by name.
    let plain = open(&mut h, &dir.join("notes.txt"));
    let typed = h.type_keys("\\m").unwrap();
    assert!(!typed.last().unwrap().errors.is_empty(), "{typed:?}");
    assert_eq!(rows(&mut h), raw);
    keys(&mut h, ":markdown-toggle<CR>");
    assert!(rendered(&mut h, plain));
    assert_eq!(rows(&mut h), shown);
}

#[test]
fn the_view_is_for_reading_and_follows_the_file_and_the_width() {
    let dir = tree("follows");
    let path = dir.join("notes.md");
    fs::write(&path, SAMPLE).unwrap();
    let mut h = harness(Options::default());
    let buf = open(&mut h, &path);
    keys(&mut h, "\\m");

    // The file changes on disk under a buffer with nothing unsaved: reloaded, and rendered.
    fs::write(&path, "## Changed *on disk*\n\n1. a [link](x.md)\n").unwrap();
    // A moment old, so the look doesn't take it for a write still going on.
    let written = fs::File::options().write(true).open(&path).unwrap();
    let a_while_ago = SystemTime::now() - Duration::from_secs(10);
    written.set_modified(a_while_ago).unwrap();
    h.check_files();
    h.run_until_idle();
    assert_eq!(rows(&mut h), ["Changed on disk", "", "1. a link"]);

    // No key of yours changes the text while it's rendered: `x` on a link's last letter would
    // take the address you can't see with it.
    let before = text(&h, buf);
    for edit in ["/link<CR>ex", "~", "dd", "ihello<Esc>", "u"] {
        let typed = h.type_keys(edit).unwrap();
        let refused = typed.iter().flat_map(|k| &k.errors);
        assert!(
            refused.clone().any(|e| e.contains("read-only")),
            "{edit}: {typed:?}"
        );
        assert_eq!(text(&h, buf), before, "{edit}");
    }
    // Raw is where you edit, and what you wrote there is rendered when you come back.
    keys(
        &mut h,
        "<Esc>\\mGo<CR>and `more` **text**<CR><CR>---<Esc>\\m",
    );
    let rule = "─".repeat(40);
    let shown = [
        "Changed on disk",
        "",
        "1. a link",
        "",
        "and more text",
        "",
        rule.as_str(),
    ];
    assert_eq!(rows(&mut h), shown);
    assert!(text(&h, buf).ends_with("\nand `more` **text**\n\n---\n"));

    // A rule is as wide as the text: a narrower window shortens it.
    h.set_window_size(30, 20);
    h.run_until_idle();
    assert_eq!(rows(&mut h).last().unwrap(), &"─".repeat(30));
}

#[test]
fn a_restart_brings_the_rendered_view_back() {
    let dir = tree("restart");
    fs::write(dir.join("a.md"), SAMPLE).unwrap();
    fs::write(dir.join("b.md"), "**raw** stays raw\n").unwrap();
    let mut h = harness(Options::default());
    open(&mut h, &dir.join("b.md"));
    open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m");
    let before = rows(&mut h);
    let json = h.session_json();

    let mut again = harness(Options::default());
    again.restore_session(&json).unwrap();
    again.run_until_idle();
    assert_eq!(rows(&mut again), before);
    let a = again.focused_buffer().unwrap();
    assert_eq!(again.buffer_name(a), Some("a.md"));
    assert!(rendered(&mut again, a));
    let b = again.find_buffer("b.md").unwrap();
    assert!(!rendered(&mut again, b));
    assert_eq!(again.session_json(), json, "and keeps as the same session");
    // Read-only was the view's, not the buffer's: raw again, it takes your edits.
    // The cursor came back on the `T` the view had moved it to, off the hidden `# `.
    keys(&mut again, "\\mx");
    assert_eq!(text(&again, a), SAMPLE.replacen('T', "", 1));
}

#[test]
fn a_link_goes_to_a_heading_a_file_or_the_opener() {
    let dir = tree("links");
    let said = dir.join("opened");
    let opener = dir.join("opener");
    let script = format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", said.display());
    fs::write(&opener, script).unwrap();
    fs::set_permissions(&opener, fs::Permissions::from_mode(0o755)).unwrap();
    let a = "# A\n\nSee [the notes](b.md#second-part), [the site](https://example.com/x) and [below](#end).\n\n## End\n\nMail <me@example.com>, [gone](b.md#nope) or [line](b.md:3).\n";
    fs::write(dir.join("a.md"), a).unwrap();
    fs::write(dir.join("b.md"), "# B\n\ntext\n\n## Second part\n\nmore\n").unwrap();
    let options = Options {
        opener: Some(opener.display().to_string()),
        ..Options::default()
    };
    let mut h = harness(options);
    let first = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m");

    // A file and a heading in it: in this window, on the heading's line, rendered as this was.
    keys(&mut h, "/notes<CR><CR>");
    let second = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(second), Some("b.md"));
    assert!(rendered(&mut h, second));
    let w = h.focused().unwrap();
    let at = h.text(second).unwrap().cursor(w).unwrap();
    assert_eq!(h.text(second).unwrap().line_of(at), 4);
    // And back, as from any jump.
    keys(&mut h, "<C-o>");
    assert_eq!(h.focused_buffer(), Some(first));

    // A heading here.
    keys(&mut h, "/below<CR><CR>");
    let at = h.text(first).unwrap().cursor(w).unwrap();
    assert_eq!(h.text(first).unwrap().line_of(at), 4);

    // A web address goes to the opener, and DIE shows nothing new.
    keys(&mut h, "gg/site<CR><CR>");
    h.run_until_idle();
    assert_eq!(fs::read_to_string(&said).unwrap(), "https://example.com/x");
    assert_eq!(h.focused_buffer(), Some(first));

    // An address in angle brackets is mail to it.
    keys(&mut h, "/me@<CR><CR>");
    h.run_until_idle();
    assert_eq!(fs::read_to_string(&said).unwrap(), "mailto:me@example.com");

    // A heading the file hasn't got: the file at its top, and a word about it.
    keys(&mut h, "/gone<CR><CR>");
    assert_eq!(h.focused_buffer(), Some(second));
    assert_eq!(
        h.text(second).unwrap().cursor(w),
        Some(2),
        "on the B of `# B`"
    );
    assert!(h.messages.iter().any(|m| m == "No heading #nope"));
    // And a line, as `die b.md:3` reads it.
    keys(&mut h, "<C-o>/line<CR><CR>");
    let at = h.text(second).unwrap().cursor(w).unwrap();
    assert_eq!(h.text(second).unwrap().line_of(at), 2);
    keys(&mut h, "<C-o>");
    assert_eq!(h.focused_buffer(), Some(first));

    // Off a link, the key is die-vim's own, and by name the command says there's none.
    keys(&mut h, "gg");
    let err = h.run("markdown-follow-link", &Default::default());
    assert_eq!(err.unwrap_err().to_string(), "No link here");
}
