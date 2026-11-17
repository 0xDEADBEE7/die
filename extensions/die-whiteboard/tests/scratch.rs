//! Scratch boards: `whiteboard` with no file (`\w`) opens one with no prompt and no file, in the
//! directory it was typed in, and from a board another, the lowest number free; `whiteboard-new`
//! always does. Nothing writes one by itself: no autosave, no ` *`, nothing at a kill. `S` and
//! `ZZ` ask `Save board as: ` and make it that file's board from then on; `E` asks `Export to: `,
//! `board.png` in its directory first, the answer's extension saying what's written; and
//! `whiteboard-open` writes it to a temporary file.

use die_test_cap as _;

mod common;

use common::*;
use die_core::{Args, Harness, ShowIn};
use die_whiteboard::export::decode_png;
use die_whiteboard::svg;
use die_whiteboard::{Whiteboard, file};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A scratch board opened from the notes buffer, whose directory is a test's own: `whiteboard`
/// with no file, as `\w` runs it, focused.
fn scratch(name: &str) -> Setup {
    let dir = TempDir::new(name);
    let mut h = harness();
    let notes = h.find_buffer("notes").unwrap();
    h.set_directory(notes, dir.path());
    h.run("whiteboard", &Args::default()).unwrap();
    h.run_until_idle();
    h.scene();
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    Setup {
        h,
        dir,
        buf,
        win,
        path: PathBuf::new(),
    }
}

/// Focus `buf` in the setup's window.
fn focus(s: &mut Setup, buf: die_core::BufferId) {
    s.h.show(buf, ShowIn::Current).unwrap();
    s.buf = buf;
    s.h.run_until_idle();
    s.h.scene();
}

/// What's in a directory, by name.
fn listed(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A path as the prompts put it on their line, `~` for the home directory.
fn line(path: &Path) -> String {
    file::abbreviate(path, std::env::home_dir().as_deref())
}

/// The frame's last row: a question with no candidates under it, its start scrolled off when
/// it's longer than the row.
fn asked(h: &mut Harness) -> String {
    let scene = h.scene();
    scene.row_text(scene.rows - 1)
}

/// What the board says on the status line.
fn said(s: &mut Setup) -> String {
    board(&mut s.h, s.buf).borrow().status()
}

fn elements_in(path: &Path) -> usize {
    svg::read(&std::fs::read_to_string(path).unwrap())
        .unwrap()
        .len()
}

#[test]
fn whiteboard_opens_a_scratch_board_that_nothing_writes() {
    let mut s = scratch("open");
    assert_eq!(s.h.prompt_text(), None, "no prompt");
    assert_eq!(s.h.buffer_name(s.buf), Some("*whiteboard: scratch*"));
    assert_eq!(
        s.h.directory(s.buf),
        Some(s.dir.path()),
        "the notes' directory"
    );
    assert_eq!(said(&mut s), "scratch  pen·1  medium  0 elements");
    assert!(status(&mut s).contains("scratch  pen·1  medium  0 elements"));
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    keys(&mut s.h, "2");
    assert_eq!(said(&mut s), "scratch  pen·2  medium  1 element", "no ` *`");
    assert_eq!(s.h.next_timer(), None, "no autosave");
    s.h.advance(Duration::from_secs(3));
    assert!(listed(s.dir.path()).is_empty(), "nothing written");
    // From a board, another; from text, the board used last; `whiteboard-new` always another.
    let first = s.buf;
    s.h.run("whiteboard", &Args::default()).unwrap();
    let second = s.h.focused_buffer().unwrap();
    assert_ne!(second, first);
    assert_eq!(s.h.buffer_name(second), Some("*whiteboard: scratch 2*"));
    assert!(elements(&mut s.h, second).is_empty());
    assert_eq!(s.h.directory(second), Some(s.dir.path()), "the board's");
    let notes = s.h.find_buffer("notes").unwrap();
    s.h.show(notes, ShowIn::Current).unwrap();
    s.h.run("whiteboard", &Args::default()).unwrap();
    assert_eq!(s.h.focused_buffer(), Some(second), "the one used last");
    assert_eq!(s.h.prompt_text(), None);
    s.h.show(notes, ShowIn::Current).unwrap();
    s.h.run("whiteboard-new", &Args::default()).unwrap();
    let third = s.h.focused_buffer().unwrap();
    assert_eq!(s.h.buffer_name(third), Some("*whiteboard: scratch 3*"));
    // A kill discards it, writes nothing, and its name is the next one's.
    focus(&mut s, first);
    keys(&mut s.h, ":bd<CR>");
    assert!(!s.h.is_live(first));
    assert!(s.h.take_errors().is_empty());
    assert!(
        listed(s.dir.path()).is_empty(),
        "nothing written at the kill"
    );
    assert_eq!(s.h.next_timer(), None);
    s.h.run("whiteboard-new", &Args::default()).unwrap();
    let again = s.h.focused_buffer().unwrap();
    assert_eq!(
        s.h.buffer_name(again),
        Some("*whiteboard: scratch*"),
        "the lowest number free"
    );
    assert!(elements(&mut s.h, again).is_empty());
    assert_eq!(
        s.h.with_ext(|wb: &mut Whiteboard, _| wb.boards().len())
            .unwrap(),
        3
    );
}

#[test]
fn s_saves_a_scratch_board_as_a_file_whose_board_it_is_from_then_on() {
    let mut s = scratch("save");
    let dir = s.dir.path().to_path_buf();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("old.svg"), "").unwrap();
    std::fs::write(dir.join("notes.txt"), "").unwrap();
    keys(&mut s.h, "2r");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 40.0)]);
    keys(&mut s.h, "S");
    assert_eq!(
        s.h.prompt_text(),
        Some(format!("{}/", line(&dir)).as_str()),
        "from the board's directory"
    );
    assert_eq!(
        s.h.prompt_candidates(),
        ["old.svg", "sub/"],
        "boards, then directories"
    );
    keys(&mut s.h, "arch<CR>");
    let arch = dir.join("arch.svg");
    assert_eq!(elements_in(&arch), 1, "`.svg` added, and written");
    assert_eq!(s.h.buffer_name(s.buf), Some("*whiteboard: arch.svg*"));
    assert_eq!(s.h.directory(s.buf), Some(dir.as_path()));
    assert_eq!(said(&mut s), "arch.svg  pen·2  medium  1 element");
    let wrote = format!("Wrote {}", arch.display());
    assert!(s.h.messages.contains(&wrote), "{:?}", s.h.messages);
    // A file board now: a change is unsaved until the autosave writes it.
    gesture(&mut s, &[(10.0, 80.0), (60.0, 80.0)]);
    assert!(said(&mut s).ends_with(" *"), "{}", said(&mut s));
    assert!(s.h.next_timer().is_some());
    s.h.advance(Duration::from_secs(2));
    assert_eq!(elements_in(&arch), 2);
    let on = s.h.with_ext(|wb: &mut Whiteboard, _| wb.board_on(&arch));
    assert_eq!(on.unwrap(), Some(s.buf));
    // Another scratch board takes the number back. A file there is asked about; `n` keeps it.
    s.h.run("whiteboard", &Args::default()).unwrap();
    let other = s.h.focused_buffer().unwrap();
    assert_eq!(s.h.buffer_name(other), Some("*whiteboard: scratch*"));
    focus(&mut s, other);
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    keys(&mut s.h, "S<C-u>old.svg<CR>");
    let question = "old.svg exists; overwrite? (y or n)";
    assert!(asked(&mut s.h).ends_with(question), "{:?}", asked(&mut s.h));
    keys(&mut s.h, "n<CR>");
    assert_eq!(std::fs::read(dir.join("old.svg")).unwrap(), b"", "kept");
    assert_eq!(s.h.buffer_name(other), Some("*whiteboard: scratch*"));
    // A path another open board has is refused.
    let errors = keys_errors(&mut s.h, &format!("S<C-u>{}<CR>", arch.display()));
    assert!(
        errors
            .iter()
            .any(|e| e.contains("arch.svg is open in another board")),
        "{errors:?}"
    );
    assert_eq!(s.h.buffer_name(other), Some("*whiteboard: scratch*"));
    // `y` writes over it. A directory answered asks again from there.
    keys(&mut s.h, "S<C-u>old<CR>y<CR>");
    assert_eq!(elements_in(&dir.join("old.svg")), 1);
    assert_eq!(s.h.buffer_name(other), Some("*whiteboard: old.svg*"));
    s.h.run("whiteboard-new", &Args::default()).unwrap();
    let third = s.h.focused_buffer().unwrap();
    focus(&mut s, third);
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let sub = dir.join("sub");
    keys(&mut s.h, &format!("S<C-u>{}<M-CR>", sub.display()));
    let again = format!("{}/", sub.display());
    assert_eq!(s.h.prompt_text(), Some(again.as_str()));
    keys(&mut s.h, "deep<CR>");
    assert_eq!(elements_in(&sub.join("deep.svg")), 1);
    assert_eq!(s.h.directory(third), Some(sub.as_path()));
    // `S` on a file board writes, as it did.
    let arch_board = s.h.find_buffer("*whiteboard: arch.svg*").unwrap();
    focus(&mut s, arch_board);
    keys(&mut s.h, "S");
    assert_eq!(s.h.prompt_text(), None);
    assert_eq!(s.h.take_errors(), Vec::<String>::new());
}

#[test]
fn zz_on_a_scratch_board_asks_where_then_closes_its_window() {
    let mut s = scratch("zz");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    keys(&mut s.h, "<C-w>s");
    assert_eq!(s.h.window_list().len(), 2);
    keys(&mut s.h, "ZZ");
    assert!(s.h.prompt_text().is_some());
    keys(&mut s.h, "<Esc>");
    assert_eq!(s.h.window_list().len(), 2, "a cancel closes nothing");
    assert_eq!(s.h.buffer_name(s.buf), Some("*whiteboard: scratch*"));
    keys(&mut s.h, "ZZpic<CR>");
    assert_eq!(s.h.window_list().len(), 1, "closed, as :x does");
    assert_eq!(
        elements_in(&s.dir.path().join("pic.svg")),
        1,
        "written first"
    );
    assert!(s.h.is_live(s.buf));
    assert_eq!(s.h.buffer_name(s.buf), Some("*whiteboard: pic.svg*"));
}

#[test]
fn export_asks_where_and_the_extension_says_what_it_writes() {
    let mut s = scratch("export");
    let dir = s.dir.path().to_path_buf();
    keys(&mut s.h, "2");
    gesture(&mut s, &[(0.0, 0.0), (100.0, 0.0)]);
    keys(&mut s.h, "E");
    let png = dir.join("board.png");
    assert_eq!(
        s.h.prompt_text(),
        Some(line(&png).as_str()),
        "board.png, here"
    );
    keys(&mut s.h, "<CR>");
    s.h.run_until_idle();
    let raster = decode_png(&std::fs::read(&png).unwrap()).unwrap();
    assert_eq!((raster.width, raster.height), (136, 36));
    let said = s.h.messages.last().unwrap().clone();
    assert!(
        said.starts_with(&format!("Wrote {}", png.display())),
        "{said}"
    );
    // Again: from where it went, and no question, since this board wrote it.
    keys(&mut s.h, "E");
    assert_eq!(s.h.prompt_text(), Some(line(&png).as_str()));
    keys(&mut s.h, "<CR>");
    assert_eq!(s.h.prompt_text(), None, "not asked to overwrite");
    s.h.run_until_idle();
    assert!(s.h.messages.last().unwrap().starts_with("Wrote "));
    // `M-x whiteboard-export svg` on a scratch board asks, with that extension.
    let args = Args {
        text: Some("svg".into()),
        ..Args::default()
    };
    s.h.run("whiteboard-export", &args).unwrap();
    let plain = dir.join("board.export.svg");
    assert_eq!(s.h.prompt_text(), Some(line(&plain).as_str()));
    keys(&mut s.h, "<Esc>");
    // Another board: `board-2.png`, since `board.png` is there.
    s.h.run("whiteboard", &Args::default()).unwrap();
    let other = s.h.focused_buffer().unwrap();
    focus(&mut s, other);
    gesture(&mut s, &[(0.0, 20.0), (50.0, 20.0)]);
    keys(&mut s.h, "E");
    assert_eq!(
        s.h.prompt_text(),
        Some(line(&dir.join("board-2.png")).as_str())
    );
    // `.svg` is the picture without the dialect.
    keys(
        &mut s.h,
        &format!("<C-u>{}<CR>", dir.join("pic.svg").display()),
    );
    let text = std::fs::read_to_string(dir.join("pic.svg")).unwrap();
    assert!(text.contains("<svg") && !text.contains("data-wb"), "{text}");
    // Anything else is refused, and a board's own file is.
    let errors = keys_errors(
        &mut s.h,
        &format!("E<C-u>{}<CR>", dir.join("pic.txt").display()),
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("Export to a .png or an .svg, not pic.txt")),
        "{errors:?}"
    );
    assert!(!dir.join("pic.txt").exists());
    let board_file = dir.join("f.svg");
    let notes = s.h.find_buffer("notes").unwrap();
    s.h.show(notes, ShowIn::Current).unwrap();
    open(&mut s.h, &board_file);
    s.h.run("whiteboard-save", &Args::default()).unwrap();
    focus(&mut s, other);
    let errors = keys_errors(&mut s.h, &format!("E<C-u>{}<CR>", board_file.display()));
    assert!(
        errors
            .iter()
            .any(|e| e.contains("f.svg is a board's own file")),
        "{errors:?}"
    );
    // Another's export is asked about; `n` leaves it.
    let before = std::fs::read(&png).unwrap();
    keys(&mut s.h, &format!("E<C-u>{}<CR>", png.display()));
    let question = "/board.png exists; overwrite? (y or n)";
    assert!(asked(&mut s.h).ends_with(question), "{:?}", asked(&mut s.h));
    keys(&mut s.h, "n<CR>");
    s.h.run_until_idle();
    assert_eq!(std::fs::read(&png).unwrap(), before);
    assert_eq!(s.h.take_errors(), Vec::<String>::new());
}

/// `Save board as: ` and `Export to: ` write the name typed, or the path `E` filled in: `<CR>`
/// never turns a new name into a listed file it flex matches, which it would write over.
#[test]
fn a_new_name_is_the_file_written_not_one_it_matches() {
    let mut s = scratch("typed");
    let dir = s.dir.path().to_path_buf();
    std::fs::write(dir.join("archive.svg"), "").unwrap();
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    keys(&mut s.h, "Sarch");
    assert_eq!(s.h.prompt_candidates(), ["archive.svg"], "listed");
    assert_eq!(s.h.prompt_chosen(), None, "but not chosen");
    keys(&mut s.h, "<CR>");
    assert_eq!(s.h.prompt_text(), None, "nothing asked");
    assert_eq!(elements_in(&dir.join("arch.svg")), 1);
    assert_eq!(std::fs::read(dir.join("archive.svg")).unwrap(), b"");
    // `E RET` writes what it shows, `arch.png`, though `arch-v1.png` matches it.
    std::fs::write(dir.join("arch-v1.png"), "v1").unwrap();
    keys(&mut s.h, "E");
    let png = dir.join("arch.png");
    assert_eq!(s.h.prompt_text(), Some(line(&png).as_str()));
    keys(&mut s.h, "<CR>");
    assert_eq!(s.h.prompt_text(), None, "nothing asked");
    s.h.run_until_idle();
    assert!(decode_png(&std::fs::read(&png).unwrap()).is_ok());
    assert_eq!(std::fs::read(dir.join("arch-v1.png")).unwrap(), b"v1");
    // A new name typed over the last export's is a new file, and the last export stays.
    let before = std::fs::read(&png).unwrap();
    gesture(&mut s, &[(10.0, 40.0), (60.0, 40.0)]);
    keys(
        &mut s.h,
        &format!("E<C-u>{}<CR>", dir.join("a.png").display()),
    );
    assert_eq!(s.h.prompt_text(), None, "nothing asked");
    s.h.run_until_idle();
    assert!(decode_png(&std::fs::read(dir.join("a.png")).unwrap()).is_ok());
    assert_eq!(std::fs::read(&png).unwrap(), before, "not written over");
    let wrote = format!("Wrote {}", dir.join("a.png").display());
    assert!(s.h.messages.last().unwrap().starts_with(&wrote));
    assert_eq!(s.h.take_errors(), Vec::<String>::new());
}

/// An open board's file is that board's by any path to it: through a link to its directory, or
/// in another case where the disk ignores case. `S` and `E` refuse it, and `:whiteboard` goes to
/// the board.
#[test]
fn an_open_board_s_file_by_another_path_is_still_its() {
    let mut s = scratch("alias");
    let dir = s.dir.path().to_path_buf();
    let scratch_board = s.buf;
    let link = dir.join("link");
    std::os::unix::fs::symlink(&dir, &link).unwrap();
    let arch = dir.join("arch.svg");
    open(&mut s.h, &arch);
    let arch_board = s.h.focused_buffer().unwrap();
    focus(&mut s, arch_board);
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    keys(&mut s.h, "S");
    open(&mut s.h, &link.join("arch.svg"));
    assert_eq!(
        s.h.focused_buffer(),
        Some(arch_board),
        "the board open on it"
    );
    let count = |h: &mut Harness| h.with_ext(|wb: &mut Whiteboard, _| wb.boards().len());
    assert_eq!(count(&mut s.h).unwrap(), 2);
    focus(&mut s, scratch_board);
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    gesture(&mut s, &[(10.0, 30.0), (60.0, 30.0)]);
    let mut aliases = vec![link.join("arch")];
    if dir.join("ARCH.svg").exists() {
        aliases.push(dir.join("ARCH"));
    }
    for alias in aliases {
        let errors = keys_errors(&mut s.h, &format!("S<C-u>{}<CR>", alias.display()));
        let refused = errors
            .iter()
            .any(|e| e.contains("is open in another board"));
        assert!(refused, "{}: {errors:?}", alias.display());
        assert_eq!(s.h.prompt_text(), None, "not asked to overwrite");
        let to = format!("E<C-u>{}.svg<CR>", alias.display());
        let errors = keys_errors(&mut s.h, &to);
        let refused = errors.iter().any(|e| e.contains("is a board's own file"));
        assert!(refused, "{}: {errors:?}", alias.display());
    }
    assert_eq!(
        s.h.buffer_name(scratch_board),
        Some("*whiteboard: scratch*")
    );
    assert_eq!(elements_in(&arch), 1, "the board's file as it was");
}

/// A board's file is an `.svg`: `S` adds it to `diagram.png`. And `whiteboard-export png` won't
/// write a PNG over a board's own file, as a board opened as `x.png` has.
#[test]
fn a_board_s_file_is_never_its_own_png_export() {
    let mut s = scratch("own");
    let dir = s.dir.path().to_path_buf();
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    keys(&mut s.h, "Sdiagram.png<CR>");
    assert_eq!(elements_in(&dir.join("diagram.png.svg")), 1);
    assert!(!dir.join("diagram.png").exists());
    let name = s.h.buffer_name(s.buf);
    assert_eq!(name, Some("*whiteboard: diagram.png.svg*"));
    let png = Args {
        text: Some("png".into()),
        ..Args::default()
    };
    s.h.run("whiteboard-export", &png).unwrap();
    s.h.run_until_idle();
    let beside = std::fs::read(dir.join("diagram.png.png")).unwrap();
    assert!(decode_png(&beside).is_ok());
    assert_eq!(elements_in(&dir.join("diagram.png.svg")), 1);
    // A board opened on a `.png` name: its PNG beside it is its own file, refused.
    let x = dir.join("x.png");
    open(&mut s.h, &x);
    let xb = s.h.focused_buffer().unwrap();
    focus(&mut s, xb);
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    keys(&mut s.h, "S");
    let err = s.h.run("whiteboard-export", &png).unwrap_err().to_string();
    assert!(err.contains("x.png is a board's own file"), "{err}");
    s.h.run_until_idle();
    assert_eq!(elements_in(&x), 1, "still the board");
}

/// `E` writes its last export again without asking only while the file is as that export left
/// it: once anything else has written it, here another board saved there and closed, it asks.
#[test]
fn the_last_export_is_asked_about_once_something_else_wrote_it() {
    let mut s = scratch("stale");
    let dir = s.dir.path().to_path_buf();
    let first = s.buf;
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let pic = dir.join("pic.svg");
    keys(&mut s.h, &format!("E<C-u>{}<CR>", pic.display()));
    assert!(svg::read(&std::fs::read_to_string(&pic).unwrap()).is_err());
    keys(&mut s.h, "E");
    assert_eq!(s.h.prompt_text(), Some(line(&pic).as_str()));
    keys(&mut s.h, "<CR>");
    assert_eq!(s.h.prompt_text(), None, "as it left it: not asked");
    // Another board saved over it, then closed.
    s.h.run("whiteboard", &Args::default()).unwrap();
    let other = s.h.focused_buffer().unwrap();
    focus(&mut s, other);
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    gesture(&mut s, &[(10.0, 30.0), (60.0, 30.0)]);
    keys(&mut s.h, "S<C-u>pic<CR>");
    let question = "pic.svg exists; overwrite? (y or n)";
    assert!(asked(&mut s.h).ends_with(question), "{:?}", asked(&mut s.h));
    keys(&mut s.h, "y<CR>:bd<CR>");
    assert!(!s.h.is_live(other));
    assert_eq!(elements_in(&pic), 2);
    focus(&mut s, first);
    keys(&mut s.h, "E");
    assert_eq!(s.h.prompt_text(), Some(line(&pic).as_str()));
    keys(&mut s.h, "<CR>");
    assert!(asked(&mut s.h).ends_with(question), "{:?}", asked(&mut s.h));
    keys(&mut s.h, "n<CR>");
    assert_eq!(elements_in(&pic), 2, "the other board's file kept");
    assert_eq!(s.h.take_errors(), Vec::<String>::new());
}

#[test]
fn whiteboard_open_writes_a_scratch_board_to_a_temporary_file() {
    let mut s = scratch("browser");
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let buf = s.buf;
    let tmp =
        s.h.with_ext(|wb: &mut Whiteboard, ctx| wb.file_to_open(ctx, buf))
            .unwrap()
            .unwrap();
    assert!(tmp.starts_with(std::env::temp_dir()), "{}", tmp.display());
    assert_eq!(elements_in(&tmp), 1);
    let _ = std::fs::remove_file(&tmp);
    assert!(listed(s.dir.path()).is_empty(), "nothing beside it");
    // A file board's is its file, written now.
    let path = s.dir.path().join("f.svg");
    open(&mut s.h, &path);
    let buf = s.h.focused_buffer().unwrap();
    gesture(&mut s, &[(10.0, 10.0), (60.0, 10.0)]);
    let shown =
        s.h.with_ext(|wb: &mut Whiteboard, ctx| wb.file_to_open(ctx, buf))
            .unwrap()
            .unwrap();
    assert_eq!(shown, path);
    assert_eq!(elements_in(&path), 1);
}

/// Core looks at the session every half second while you work, and a scratch board is kept
/// whole in it: what that costs with a board of 1,000 strokes, printed (`--nocapture`) and held
/// to a loose budget, the test profile being lightly optimized.
#[test]
fn a_scratch_board_of_a_thousand_strokes_in_the_session() {
    use die_whiteboard::model::{Board, Point, Shape};
    use std::time::Instant;
    let mut big = Board::default();
    let mut seed: u32 = 7;
    let mut rnd = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    for i in 0..1000 {
        let (mut x, mut y) = (rnd() * 400.0, rnd() * 280.0);
        let mut points = vec![Point::new(x, y)];
        for _ in 0..19 {
            x += rnd() * 6.0 - 3.0;
            y += rnd() * 6.0 - 3.0;
            points.push(Point::new(x, y));
        }
        let shape = Shape::Stroke {
            points,
            highlighter: false,
        };
        big.add(shape, (i % 6 + 1) as u8, 1);
    }
    let mut s = scratch("thousand");
    let dir = s.dir.path().to_path_buf();
    let time = |h: &mut Harness| {
        let t = Instant::now();
        let json = h.session_json();
        (t.elapsed(), json.len())
    };
    let (empty, _) = time(&mut s.h);
    let buf =
        s.h.with_ext(|wb: &mut Whiteboard, ctx| wb.scratch(ctx, &dir, big, None))
            .unwrap();
    let (first, bytes) = time(&mut s.h);
    // Unchanged by its revision, the picture isn't written out again: a stroke taken away behind
    // the revision's back is still in the session. Once the revision moves, it's written again.
    board(&mut s.h, buf).borrow_mut().board.elements.pop();
    let (again, kept) = time(&mut s.h);
    assert_eq!(kept, bytes, "the picture kept, not written out again");
    board(&mut s.h, buf).borrow_mut().redraw();
    let (changed, fewer) = time(&mut s.h);
    assert!(
        fewer < bytes,
        "written again: {fewer} bytes, {bytes} before"
    );
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    println!(
        "the session as JSON with a scratch board of 1,000 strokes kept: {:.2} ms the first time ({bytes} bytes), {:.2} ms unchanged, {:.2} ms changed; {:.2} ms with an empty one",
        ms(first),
        ms(again),
        ms(changed),
        ms(empty)
    );
    assert!(bytes > 100_000, "the board is in it: {bytes} bytes");
    assert!(first < Duration::from_millis(200), "{first:?}");
}
