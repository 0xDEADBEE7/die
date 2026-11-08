//! `buffer-menu` and its keys, against what batch Emacs 30.2 showed with evil-collection's keys
//! (docs/m0-log.md has the run). Emacs's rows there, from the same buffers:
//!
//! ```text
//! .   alpha.txt                16                  ~/files/alpha.txt
//!   * beta.rs                  19                  ~/files/beta.rs
//!     gamma.md                 15                  ~/files/gamma.md
//! ```
//!
//! Batch Emacs prints no mode names; DIE's Mode is Fundamental. Without die-vim here, a test
//! puts the cursor itself where `j` and `k` would.

use die_test_cap as _;

use die_core::*;
use std::path::{Path, PathBuf};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("die-menu-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for f in ["alpha.txt", "beta.rs", "gamma.md"] {
            std::fs::write(d.join(f), format!("hello {f}\n")).unwrap();
        }
        Dir(d.canonicalize().unwrap())
    }
    fn path(&self, f: &str) -> PathBuf {
        self.0.join(f)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Batch Emacs's buffers: gamma, beta with `more ` typed, then alpha, each opened in turn, and
/// `*notes*`, read-only, which DIE lists by when it was made. `*messages*` is last and `.` is
/// alpha, where the menu opens.
fn editor(name: &str) -> (Harness, Dir, WindowId) {
    let dir = Dir::new(name);
    let mut h = Harness::new(vec![]).unwrap();
    h.messages.push("start".into());
    let (notes, w) = h.open_text("*notes*", "abc").unwrap();
    h.text_mut(notes).unwrap().settings_mut().read_only = true;
    h.create_text(" *internal*", "");
    for f in ["gamma.md", "beta.rs", "alpha.txt"] {
        let b = h.open_file(&dir.path(f)).unwrap();
        h.show(b, ShowIn::Current).unwrap();
        if f == "beta.rs" {
            let t = h.text_mut(b).unwrap();
            t.tx(Origin::User).insert(0, "more ").commit().unwrap();
        }
    }
    (h, dir, w)
}

fn menu(h: &Harness) -> BufferId {
    h.find_buffer("*Buffer List*").expect("the menu is there")
}

fn text(h: &Harness, b: BufferId) -> String {
    h.text(b).unwrap().rope().to_string()
}

/// The menu's rows, the titles left out.
fn rows(h: &Harness) -> Vec<String> {
    text(h, menu(h)).lines().skip(1).map(String::from).collect()
}

fn row(marks: &str, name: &str, size: usize, file: &Path) -> String {
    let file = file.display();
    format!("{marks} {name:<19} {size:>7} Fundamental      {file}")
        .trim_end()
        .into()
}

/// The cursor's line and column in the menu, the titles being line 0.
fn at(h: &Harness, w: WindowId) -> (usize, usize) {
    let t = h.text(menu(h)).unwrap();
    let pos = t.cursor(w).unwrap();
    let line = t.line_of(pos);
    (line, pos - t.line_start(line))
}

fn goto(h: &mut Harness, w: WindowId, line: usize, col: usize) {
    let b = menu(h);
    let t = h.text_mut(b).unwrap();
    let pos = t.line_start(line) + col;
    t.set_cursor(w, pos);
}

fn keys(h: &mut Harness, keys: &str) -> Vec<String> {
    let log = h.type_keys(keys).unwrap();
    log.into_iter().flat_map(|k| k.errors).collect()
}

fn opened(name: &str) -> (Harness, Dir, WindowId) {
    let (mut h, dir, w) = editor(name);
    h.run("buffer-menu", &Args::default()).unwrap();
    (h, dir, w)
}

#[test]
fn the_listing_has_emacss_columns_marks_and_widths() {
    let (mut h, dir, w) = opened("listing");
    let b = menu(&h);
    assert_eq!(h.focused_buffer(), Some(b), "in this window");
    let all = text(&h, b);
    // As Emacs's header line shows them: Buffer at 4, Size ending at 31, Mode at 32, File at 49.
    let titles = format!(
        "CRM Buffer{}Size Mode{}File",
        " ".repeat(17),
        " ".repeat(13)
    );
    assert_eq!(all.lines().next(), Some(titles.as_str()));
    assert_eq!(
        rows(&h),
        [
            row(".  ", "alpha.txt", 16, &dir.path("alpha.txt")),
            row("  *", "beta.rs", 19, &dir.path("beta.rs")),
            row("   ", "gamma.md", 15, &dir.path("gamma.md")),
            row(" % ", "*notes*", 3, Path::new("")),
            row(" % ", "*messages*", "start\n".len(), Path::new("")),
        ],
        "no space-named buffer, nor the menu itself"
    );
    assert_eq!(at(&h, w), (1, 0), "the cursor starts on the first row");
    assert!(h.text(b).unwrap().settings().read_only);
    assert_eq!(
        h.messages.last().map(String::as_str),
        Some("Commands: d, s, x, u; go, gO, 1, 2, m, gv; ~, %; q to quit")
    );
    // The titles take the header line's face, the status line's here, and names are bold.
    let dump = h.style_dump();
    let styles: Vec<&str> = dump.lines().skip_while(|l| *l != "style").collect();
    assert!(
        styles[1].starts_with(&format!("|{}b", "a".repeat(53))),
        "{dump}"
    );
    assert!(styles[2].starts_with("|bbbbcccccccccb"), "{dump}");
    assert!(dump.contains("a #d0d0d0 on #282828\n") && dump.contains("c #d0d0d0 on #101010 bold"));

    // The name column grows with a long name and a wide window, a name past it runs into the
    // size's room, and one too long for that is cut, as Emacs prints them.
    let long = "a-name-of-twenty-six-chars";
    h.create_text(long, "");
    h.set_window_size(160, 20);
    keys(&mut h, "gr");
    let row = rows(&h).into_iter().find(|r| r.contains("a-name")).unwrap();
    assert_eq!(
        row,
        format!("    {long}       0 Fundamental"),
        "26 wide, the longest name's, as 160 columns allow up to 38"
    );
    h.set_window_size(80, 20);
    keys(&mut h, "gr");
    let row = rows(&h).into_iter().find(|r| r.contains("a-name")).unwrap();
    assert_eq!(
        row, "    a-name-of-twenty-six-ch…  0 Fundamental",
        "19 wide, and alpha's 16 below takes two of the size's 7"
    );
}

#[test]
fn d_s_m_and_u_mark_and_move_down_and_unmark_takes_the_dot_too() {
    let (mut h, _dir, w) = opened("marks");
    keys(&mut h, "dsm");
    let marks: Vec<String> = rows(&h).iter().map(|r| r[..3].to_string()).collect();
    assert_eq!(marks[..4], ["D  ", "  S", ">  ", " % "]);
    assert_eq!(at(&h, w), (4, 0));
    goto(&mut h, w, 1, 5);
    keys(&mut h, "u");
    assert_eq!(
        &rows(&h)[0][..3],
        "   ",
        "the `.` goes with the D, as in Emacs"
    );
    assert_eq!(at(&h, w), (2, 0));
    keys(&mut h, "u");
    assert_eq!(&rows(&h)[1][..3], "  *", "unsaved shows again");
    keys(&mut h, "<C-k>");
    assert_eq!(&rows(&h)[2][..1], "D", "C-k is d");
    keys(&mut h, "U");
    assert!(rows(&h).iter().all(|r| !r.starts_with(['D', '>'])));
    assert_eq!(at(&h, w), (4, 0), "U stays");

    // A count: d marks that many, C-d marks going up, u goes up.
    let (mut h, _dir, w) = opened("counts");
    let three = Args {
        count: Some(3),
        ..Args::default()
    };
    h.run("buffer-menu-delete", &three).unwrap();
    let marks: Vec<_> = rows(&h).iter().map(|r| r[..1].to_string()).collect();
    assert_eq!(marks, ["D", "D", "D", " ", " "]);
    assert_eq!(at(&h, w), (4, 0));
    goto(&mut h, w, 5, 0);
    h.run("buffer-menu-delete-backwards", &three).unwrap();
    assert_eq!(&rows(&h)[3][..1], "D");
    assert_eq!(&rows(&h)[4][..1], "D");
    assert_eq!(at(&h, w), (2, 0));
    h.run("buffer-menu-unmark", &three).unwrap();
    assert_eq!(&rows(&h)[1][..3], "  *");
    assert_eq!(at(&h, w), (1, 0), "a count moves u up one");

    // On the titles, d only moves; at the end it stays.
    goto(&mut h, w, 0, 0);
    keys(&mut h, "d");
    assert_eq!(at(&h, w), (1, 0));
    goto(&mut h, w, 6, 0);
    keys(&mut h, "d");
    assert_eq!(at(&h, w), (6, 0));
}

#[test]
fn delete_backs_up_and_unmarks_but_not_past_the_first_row() {
    let (mut h, _dir, w) = opened("backup");
    keys(&mut h, "ddd");
    keys(&mut h, "<Del>");
    assert_eq!(at(&h, w), (3, 0));
    assert_eq!(&rows(&h)[2][..1], " ");
    keys(&mut h, "<Del><Del>");
    assert_eq!(at(&h, w), (1, 0));
    assert!(rows(&h)[..2].iter().all(|r| !r.starts_with('D')));
    keys(&mut h, "d<Del><Del>");
    assert_eq!(at(&h, w), (0, 0), "up to the titles, which unmark nothing");
}

#[test]
fn tilde_and_percent_change_the_buffer_and_its_column() {
    let (mut h, dir, w) = opened("toggles");
    let beta = h.find_buffer("beta.rs").unwrap();
    goto(&mut h, w, 2, 6);
    keys(&mut h, "~");
    assert!(!h.text(beta).unwrap().modified());
    assert_eq!(&rows(&h)[1][..3], "   ");
    assert_eq!(at(&h, w), (2, 6), "it stays");
    let three = Args {
        count: Some(3),
        ..Args::default()
    };
    h.run("buffer-menu-not-modified", &three).unwrap();
    assert!(
        h.text(beta).unwrap().modified(),
        "a count marks it modified"
    );
    keys(&mut h, "%");
    assert!(h.text(beta).unwrap().settings().read_only);
    assert_eq!(&rows(&h)[1][..3], " %*");
    keys(&mut h, "%");
    assert!(!h.text(beta).unwrap().settings().read_only);
    goto(&mut h, w, 0, 0);
    assert_eq!(keys(&mut h, "~"), ["No buffer on this line"]);
    let alpha = h.find_buffer("alpha.txt").unwrap();
    h.kill(alpha);
    goto(&mut h, w, 1, 0);
    assert_eq!(keys(&mut h, "%"), ["This buffer has been killed"]);
    drop(dir);
}

#[test]
fn x_saves_s_kills_d_asks_for_unsaved_changes_and_keeps_the_cursor_on_its_row() {
    let (mut h, dir, w) = opened("execute");
    // Batch Emacs, `d s d s x` from the top: alpha and gamma go, beta is written, and the cursor
    // stays on the row it was on.
    keys(&mut h, "dsds");
    assert_eq!(at(&h, w), (5, 0));
    keys(&mut h, "x");
    assert_eq!(
        std::fs::read_to_string(dir.path("beta.rs")).unwrap(),
        "more hello beta.rs\n"
    );
    assert!(h.find_buffer("alpha.txt").is_none());
    assert!(h.find_buffer("gamma.md").is_none());
    let names: Vec<_> = rows(&h)
        .iter()
        .map(|r| r[4..].split(' ').next().unwrap().to_string())
        .collect();
    assert_eq!(names, ["beta.rs", "*notes*", "*messages*"]);
    assert_eq!(&rows(&h)[0][..3], "   ", "saved");
    assert_eq!(
        at(&h, w),
        (3, 0),
        "*messages*'s row, as Emacs's point stays"
    );
    assert_eq!(h.focused_buffer(), Some(menu(&h)));

    // A modified file asks, as kill-buffer does: no leaves its D, and a cancel stops there.
    let (mut h, _dir, w) = opened("ask");
    let beta = h.find_buffer("beta.rs").unwrap();
    keys(&mut h, "dd");
    h.messages.clear();
    goto(&mut h, w, 3, 0);
    keys(&mut h, "x");
    assert!(
        h.screen()
            .contains("Buffer beta.rs modified; kill anyway? (yes or no)")
    );
    assert_eq!(rows(&h).len(), 4, "alpha's row is gone already");
    keys(&mut h, "no<CR>");
    assert!(h.is_live(beta));
    assert_eq!(&rows(&h)[0][..3], "D *");
    assert_eq!(at(&h, w), (2, 0), "gamma, still");
    keys(&mut h, "x<Esc>");
    assert!(h.is_live(beta));
    keys(&mut h, "xyes<CR>");
    assert!(!h.is_live(beta));
    assert_eq!(rows(&h).len(), 3);

    // S on a buffer with no file and nothing to save is done with; with something to save, it
    // says so and stays.
    let notes = h.find_buffer("*notes*").unwrap();
    h.text_mut(notes).unwrap().set_modified(true);
    goto(&mut h, w, 2, 0);
    keys(&mut h, "sx");
    assert!(
        h.messages
            .iter()
            .any(|m| m.starts_with("Error saving *notes*")),
        "{:?}",
        h.messages
    );
    assert_eq!(&rows(&h)[1][..3], " %S");
}

#[test]
fn x_drops_the_rows_of_buffers_killed_elsewhere() {
    let (mut h, _dir, _) = opened("dead");
    let gamma = h.find_buffer("gamma.md").unwrap();
    h.kill(gamma);
    assert_eq!(rows(&h).len(), 5, "its row stays until x or gr");
    keys(&mut h, "x");
    assert_eq!(rows(&h).len(), 4);
}

#[test]
fn t_lists_only_files_and_gr_lists_again_without_the_dot_but_with_the_marks() {
    let (mut h, _dir, w) = opened("refresh");
    keys(&mut h, "dm");
    goto(&mut h, w, 4, 2);
    keys(&mut h, "T");
    assert_eq!(
        h.messages.last().map(String::as_str),
        Some("Showing only file-visiting buffers.")
    );
    let marks: Vec<_> = rows(&h).iter().map(|r| r[..3].to_string()).collect();
    assert_eq!(marks, ["   ", "> *", "   "], "D and `.` go, > stays");
    assert_eq!(at(&h, w), (1, 0), "*notes* isn't listed, so the first row");
    goto(&mut h, w, 3, 9);
    keys(&mut h, "T");
    assert_eq!(rows(&h).len(), 5);
    assert_eq!(at(&h, w), (3, 9), "gamma's row, at the same column");
    let notes = h.find_buffer("*notes*").unwrap();
    h.text_mut(notes).unwrap().settings_mut().read_only = false;
    keys(&mut h, "gr");
    assert_eq!(&rows(&h)[3][..3], "   ", "gr reads the buffers again");
}

#[test]
fn big_x_buries_the_buffer_and_moves_its_row_to_the_end() {
    let (mut h, _dir, w) = opened("bury");
    keys(&mut h, "X");
    let names: Vec<_> = rows(&h)
        .iter()
        .map(|r| r[4..13].trim().to_string())
        .collect();
    assert_eq!(
        names,
        ["beta.rs", "gamma.md", "*notes*", "*messages", "alpha.txt"]
    );
    assert!(rows(&h)[4].starts_with('.'), "the row keeps its marks");
    assert_eq!(at(&h, w), (1, 0));
    assert_eq!(
        h.messages.last().map(String::as_str),
        Some("Buffer buried.")
    );
    let alpha = h.find_buffer("alpha.txt").unwrap();
    assert_eq!(h.buffer_list().last(), Some(&alpha));
    keys(&mut h, "gr");
    assert!(
        rows(&h).last().unwrap().contains("alpha.txt"),
        "and stays there"
    );
    let beta = h.find_buffer("beta.rs").unwrap();
    h.kill(beta);
    keys(&mut h, "X");
    assert_eq!(
        h.messages.last().map(String::as_str),
        Some("Buffer is dead; removing from list.")
    );
    assert_eq!(rows(&h).len(), 4);
}

#[test]
fn o_sorts_by_the_column_at_the_cursor_and_again_the_other_way() {
    let (mut h, _dir, w) = opened("sort");
    let names = |h: &Harness| -> Vec<String> {
        rows(h)
            .iter()
            .map(|r| r[4..].split(' ').next().unwrap().to_string())
            .collect()
    };
    goto(&mut h, w, 3, 26);
    keys(&mut h, "o");
    assert_eq!(
        names(&h),
        ["beta.rs", "alpha.txt", "gamma.md", "*messages*", "*notes*"]
    );
    let titles = text(&h, menu(&h)).lines().next().unwrap().to_string();
    let (a, b) = (" ".repeat(15), " ".repeat(13));
    assert_eq!(titles, format!("CRM Buffer{a}Size ▼ Mode{b}File"));
    assert_eq!(at(&h, w), (3, 26), "gamma's row");
    keys(&mut h, "S");
    assert_eq!(
        names(&h)[..3],
        ["*notes*", "*messages*", "gamma.md"],
        "smallest first"
    );
    goto(&mut h, w, 1, 4);
    let by_name = Args {
        count: Some(3),
        ..Args::default()
    };
    h.run("buffer-menu-sort", &by_name).unwrap();
    assert_eq!(
        names(&h),
        ["*messages*", "*notes*", "alpha.txt", "beta.rs", "gamma.md"]
    );
    assert!(text(&h, menu(&h)).starts_with("CRM Buffer ▼"));
    keys(&mut h, "o");
    assert_eq!(names(&h)[0], "gamma.md");
    assert!(text(&h, menu(&h)).starts_with("CRM Buffer ▲"));
    let big = Args {
        count: Some(7),
        ..Args::default()
    };
    let bad = h.run("buffer-menu-sort", &big).unwrap_err();
    assert_eq!(bad.to_string(), "Invalid column number");
    goto(&mut h, w, 6, 0);
    assert_eq!(keys(&mut h, "o"), ["Cannot sort by nil"]);
    // By C: the one-letter columns get no arrow.
    goto(&mut h, w, 2, 0);
    keys(&mut h, "do");
    assert!(text(&h, menu(&h)).starts_with("CRM Buffer  "));
    assert!(rows(&h)[0].starts_with(' '));
    assert!(rows(&h)[4].starts_with('D'), "a D sorts after a space");
}

#[test]
fn go_return_and_g_big_o_show_the_buffer_here_or_in_another_window() {
    let (mut h, _dir, w) = opened("show");
    let beta = h.find_buffer("beta.rs").unwrap();
    goto(&mut h, w, 2, 0);
    keys(&mut h, "go");
    assert_eq!(h.focused_buffer(), Some(beta));
    assert_eq!(h.window_list().len(), 1);
    h.run("buffer-menu", &Args::default()).unwrap();
    goto(&mut h, w, 2, 0);
    keys(&mut h, "<CR>");
    let alpha = h.find_buffer("alpha.txt").unwrap();
    assert_eq!(h.focused_buffer(), Some(alpha), "<CR> is go");

    // gO: a split below when the window is alone, as batch Emacs's was, and the window the
    // buffer's in after that.
    h.run("buffer-menu", &Args::default()).unwrap();
    let m = menu(&h);
    goto(&mut h, w, 3, 0);
    keys(&mut h, "gO");
    let gamma = h.find_buffer("gamma.md").unwrap();
    let windows = h.window_list();
    assert_eq!(windows.len(), 2);
    assert_eq!(h.window_buffer(windows[0]), Some(m), "the menu stays above");
    assert_eq!(h.focused_buffer(), Some(gamma));
    h.focus(w);
    goto(&mut h, w, 3, 0);
    keys(&mut h, "gO");
    assert_eq!(h.window_list().len(), 2, "the window that has it");
    h.focus(w);
    goto(&mut h, w, 2, 0);
    keys(&mut h, "gO");
    assert_eq!(h.window_list().len(), 2, "the other window");
    assert_eq!(h.focused_buffer(), Some(beta));
    let away = h.run("buffer-menu-other-window", &Args::default());
    assert_eq!(away.unwrap_err().to_string(), "Not in *Buffer List*");
}

#[test]
fn one_two_and_gv_lay_out_the_frame_as_emacs_does() {
    let (mut h, _dir, w) = opened("layout");
    let get = |h: &Harness, n: &str| h.find_buffer(n).unwrap();
    let (alpha, beta, gamma) = (
        get(&h, "alpha.txt"),
        get(&h, "beta.rs"),
        get(&h, "gamma.md"),
    );
    goto(&mut h, w, 2, 0);
    keys(&mut h, "gO");
    h.focus(w);
    keys(&mut h, "1");
    assert_eq!(h.window_list(), [w], "alone");
    assert_eq!(h.focused_buffer(), Some(beta));
    assert_eq!(
        h.buffer_list().last(),
        Some(&menu(&h)),
        "the menu is buried"
    );

    // 2: the buffer before in this window, and this line's in a new one below, focused.
    h.run("buffer-menu", &Args::default()).unwrap();
    goto(&mut h, w, 3, 0);
    keys(&mut h, "2");
    let windows = h.window_list();
    let shown: Vec<_> = windows.iter().map(|w| h.window_buffer(*w)).collect();
    assert_eq!(shown, [Some(beta), Some(gamma)]);
    assert_eq!(h.focused(), Some(windows[1]));

    // gv: this line's alone, then each marked > below it, focus on the first.
    h.focus(w);
    h.run("buffer-menu", &Args::default()).unwrap();
    goto(&mut h, w, 2, 0);
    keys(&mut h, "m");
    goto(&mut h, w, 4, 0);
    keys(&mut h, "m");
    goto(&mut h, w, 3, 0);
    keys(&mut h, "gv");
    let windows = h.window_list();
    let shown: Vec<_> = windows.iter().map(|w| h.window_buffer(*w)).collect();
    let notes = get(&h, "*notes*");
    assert_eq!(shown, [Some(alpha), Some(gamma), Some(notes)]);
    assert_eq!(h.focused(), Some(windows[0]));
    assert!(
        !text(&h, menu(&h)).lines().any(|l| l.starts_with('>')),
        "the marks go"
    );
}

/// Each window's text rows, top to bottom. A window's status line is its last row.
fn heights(h: &Harness) -> Vec<usize> {
    let windows = h.window_list();
    windows
        .iter()
        .map(|w| h.window_size(*w).unwrap().1)
        .collect()
}

#[test]
fn gv_splits_as_emacs_does_and_changes_nothing_when_the_rows_run_out() {
    // Batch Emacs's frame has 24 rows over the echo area. `j m m m m`, then `g g g v` on the
    // first row: each window but the last has 24 / 5 rows, mode line included, and the last
    // the rest, 4 4 4 4 8. With five others, 24 / 6 gives each 4.
    let (mut h, _dir, w) = opened("gv");
    h.set_window_size(80, 23);
    goto(&mut h, w, 2, 0);
    keys(&mut h, "mmmm");
    goto(&mut h, w, 1, 0);
    assert!(keys(&mut h, "gv").is_empty());
    assert_eq!(
        heights(&h),
        [3, 3, 3, 3, 7],
        "a row each for the status line"
    );
    let names: Vec<_> = h
        .window_list()
        .iter()
        .map(|w| h.window_buffer(*w))
        .collect();
    let names: Vec<_> = names.iter().map(|b| h.buffer_name(b.unwrap())).collect();
    assert_eq!(
        names,
        [
            Some("alpha.txt"),
            Some("beta.rs"),
            Some("gamma.md"),
            Some("*notes*"),
            Some("*messages*")
        ]
    );
    h.create_text("extra", "");
    h.run("buffer-menu", &Args::default()).unwrap();
    keys(&mut h, "mmmmmm");
    goto(&mut h, w, 1, 0);
    assert!(keys(&mut h, "gv").is_empty());
    assert_eq!(heights(&h), [3; 6]);

    // Six rows hold three windows of a row each, but not four or five, where Emacs makes windows
    // of a mode line alone. Then the windows and the marks stay as they were.
    let (mut h, _dir, w) = opened("gv-small");
    h.set_window_size(80, 5);
    keys(&mut h, "mmmmm");
    goto(&mut h, w, 1, 0);
    assert_eq!(keys(&mut h, "gv"), ["That window is too small to split"]);
    assert_eq!(h.window_list(), [w]);
    assert!(rows(&h).iter().all(|r| r.starts_with('>')));
    goto(&mut h, w, 2, 0);
    keys(&mut h, "uu");
    goto(&mut h, w, 1, 0);
    assert!(keys(&mut h, "gv").is_empty());
    assert_eq!(heights(&h), [1, 1, 1]);
}

#[test]
fn other_windows_on_the_menu_go_to_its_first_row_when_it_lists_again() {
    // Batch Emacs, with the menu in a second window at line 3, column 4: `g r` in the first puts
    // the second at the start, which is the first row under the header line.
    let (mut h, _dir, w) = opened("two");
    let below = h.split_window(w, Side::Below).unwrap();
    goto(&mut h, below, 3, 4);
    goto(&mut h, w, 2, 6);
    keys(&mut h, "gr");
    assert_eq!(at(&h, below), (1, 0));
    assert_eq!(at(&h, w), (2, 6), "the window it ran in stays on its row");
    goto(&mut h, below, 3, 4);
    keys(&mut h, "d");
    assert_eq!(at(&h, below), (3, 4), "a mark changes only its character");
}

#[test]
fn o_on_a_cut_names_size_sorts_by_size() {
    // The 30-character name is cut to 21, as `big`'s 5-digit size below leaves it 2 of Size's 7,
    // so its size, 2, is at column 30.
    let mut h = Harness::new(vec![]).unwrap();
    let (_, w) = h.open_text("small", "x").unwrap();
    h.open_text("big", &"y".repeat(12345)).unwrap();
    h.open_text("a-very-long-buffer-name-thirty", "zz").unwrap();
    h.run("buffer-menu", &Args::default()).unwrap();
    assert_eq!(rows(&h)[0], ".   a-very-long-buffer-n…     2 Fundamental");
    goto(&mut h, w, 1, 30);
    keys(&mut h, "o");
    assert!(
        text(&h, menu(&h))
            .lines()
            .next()
            .unwrap()
            .contains("Size ▼")
    );
    assert!(rows(&h)[0].contains("big"), "{:?}", rows(&h));
}

#[test]
fn a_line_break_in_a_name_or_path_shows_as_emacs_describes_it_and_each_row_stays_one_line() {
    let dir = Dir::new("break");
    let path = dir.0.join("two\nlines");
    std::fs::write(&path, "x\n").unwrap();
    let mut h = Harness::new(vec![]).unwrap();
    let (keep, _) = h.open_text("keep-me", "precious").unwrap();
    let two = h.open_file(&path).unwrap();
    h.show(two, ShowIn::Current).unwrap();
    h.run("buffer-menu", &Args::default()).unwrap();
    let file = dir.0.join("two^Jlines");
    assert_eq!(
        rows(&h)[0],
        row(".  ", "two^Jlines", 2, &file),
        "{:?}",
        rows(&h)
    );
    assert_eq!(rows(&h).len(), 2, "a row each for two^Jlines and keep-me");
    keys(&mut h, "dx");
    assert!(!h.is_live(two));
    assert!(h.is_live(keep), "the row marked is the buffer killed");
}

#[test]
fn evils_insert_and_edit_keys_do_nothing_there_as_your_evil_collection_has_them() {
    // Batch Emacs's Buffer List binds each of these to `ignore` in normal state. Unbound here,
    // with no die-vim, each would say it's undefined.
    let (mut h, _dir, w) = opened("silenced");
    let before = text(&h, menu(&h));
    assert!(keys(&mut h, "iIaAcCDrOpPJ=<lt>><Insert>").is_empty());
    assert_eq!(text(&h, menu(&h)), before);
    assert_eq!(at(&h, w), (1, 0));
    assert_eq!(h.state(), State::Normal);
}

#[test]
fn q_and_zz_go_back_to_what_the_window_showed_and_bury_the_menu() {
    let (mut h, _dir, w) = opened("quit");
    let alpha = h.find_buffer("alpha.txt").unwrap();
    keys(&mut h, "q");
    assert_eq!(h.focused_buffer(), Some(alpha));
    assert_eq!(h.buffer_list().last(), Some(&menu(&h)));
    h.run("buffer-menu", &Args::default()).unwrap();
    h.run("buffer-menu", &Args::default()).unwrap();
    keys(&mut h, "ZZ");
    assert_eq!(
        h.focused_buffer(),
        Some(alpha),
        "a second menu over the first goes back too"
    );
    h.kill(alpha);
    let m = menu(&h);
    h.show(m, ShowIn::Current).unwrap();
    keys(&mut h, "q");
    let beta = h.find_buffer("beta.rs").unwrap();
    assert_eq!(
        h.focused_buffer(),
        Some(beta),
        "other-buffer's, with that one gone"
    );
    assert_eq!(h.window_list(), [w]);
}

#[test]
fn a_count_lists_only_files_and_the_marks_greater_than_stay_when_it_opens_again() {
    let (mut h, _dir, _) = editor("files");
    let one = Args {
        count: Some(1),
        ..Args::default()
    };
    h.run("buffer-menu", &one).unwrap();
    assert_eq!(rows(&h).len(), 3);
    keys(&mut h, "md");
    h.run("buffer-menu", &Args::default()).unwrap();
    let marks: Vec<_> = rows(&h).iter().map(|r| r[..1].to_string()).collect();
    assert_eq!(
        marks,
        [">", " ", " ", " ", " "],
        "as Emacs reads > from the old listing"
    );
}

#[test]
fn the_menu_isnt_kept_by_the_session() {
    let (h, _dir, _) = opened("session");
    let json = h.session_json();
    assert!(!json.contains("Buffer List"), "{json}");
}
