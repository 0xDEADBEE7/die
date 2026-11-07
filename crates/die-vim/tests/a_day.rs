//! A day of editing DIE's own source, typed as a Vim user types it, on a copy of the source in a
//! temporary directory. Each test is one task, and says the exact text and cursor it ends with.
//! What the source holds decides some of them, so those work out what they expect from the copy.

use die_test_cap as _;

use die_core::{BufferId, Config, Harness, Registry, Settings, State, Wrap};
use die_vim::Vim;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The leader keys of `apps/die-dev`, which this crate can't depend on: it depends on this one.
fn owner_keys(reg: &mut Registry) {
    reg.set_leader("\\");
    let keys = reg.global_keymap();
    keys.bind(State::Normal, "<leader>x", "command-palette");
    keys.bind(State::Normal, "<leader>f", "find-file");
    keys.bind(State::Normal, "<leader>r", "grep");
    keys.bind(State::Normal, "<leader>]", "switch-buffer");
}

fn editor() -> Harness {
    let config = Config {
        register: Some(Box::new(owner_keys)),
        text_defaults: Settings {
            wrap: Wrap::Word,
            max_width: Some(80),
            center: true,
            ..Settings::default()
        },
        ..Config::default()
    };
    Harness::with_config(vec![Box::new(Vim::default())], config).expect("die-vim registers")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let there = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &there);
        } else {
            fs::copy(entry.path(), there).unwrap();
        }
    }
}

const SRC: &str = "crates/die-vim/src";

/// A morning's project: a copy of die-vim's source, and an editor with no window yet.
struct Day {
    root: PathBuf,
    h: Harness,
}

impl Day {
    fn new(name: &str) -> Day {
        let root = std::env::temp_dir().join(format!("die-a-day-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &root.join(SRC),
        );
        let root = root.canonicalize().unwrap();
        Day { root, h: editor() }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(SRC).join(name)
    }

    /// The file on disk, as this morning's copy holds it.
    fn disk(&self, name: &str) -> String {
        fs::read_to_string(self.path(name)).unwrap()
    }

    /// A new file in the copy.
    fn seed(&self, name: &str, text: &str) {
        fs::write(self.path(name), text).unwrap();
    }

    /// Open a file from outside, as `die file` does, in a window for it or the focused window.
    fn open(&mut self, name: &str) -> BufferId {
        let path = self.path(name);
        let b = self.h.open_file(&path).unwrap();
        match self.h.focused() {
            Some(_) => {
                self.h.show(b, die_core::ShowIn::Current).unwrap();
            }
            None => {
                let (cols, rows) = self.h.frame_size();
                let w = self.h.open_window(b, cols, rows - 2).unwrap();
                self.h.focus(w);
            }
        }
        b
    }

    /// Type keys that must work.
    fn keys(&mut self, keys: &str) {
        let log = self.h.type_keys(keys).unwrap();
        let errors: Vec<_> = log.into_iter().filter(|k| !k.errors.is_empty()).collect();
        assert!(errors.is_empty(), "{keys}: {errors:?}");
        self.h.run_until_idle();
    }

    /// Type keys, and return the errors they gave.
    fn failing(&mut self, keys: &str) -> Vec<String> {
        let log = self.h.type_keys(keys).unwrap();
        log.into_iter().flat_map(|k| k.errors).collect()
    }

    fn text(&self) -> String {
        let b = self.h.focused_buffer().unwrap();
        self.h.text(b).unwrap().rope().to_string()
    }

    /// The focused buffer's name, and the cursor's line and byte in that line, from 1.
    fn at(&self) -> (String, usize, usize) {
        let (w, b) = (self.h.focused().unwrap(), self.h.focused_buffer().unwrap());
        let name = self.h.buffer_name(b).unwrap().to_string();
        let t = self.h.text(b).unwrap();
        let pos = t.cursor(w).unwrap();
        let line = t.line_of(pos);
        (name, line + 1, pos - t.line_start(line) + 1)
    }

    /// What `session_json` keeps, on a new editor, as a restart gives it.
    fn restart(&mut self) {
        let saved = self.h.session_json();
        let mut next = editor();
        next.restore_session(&saved).unwrap();
        self.h = next;
    }
}

impl Drop for Day {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The line and byte, from 1, of the `n`th place `needle` is in `text`, from 0.
fn place(text: &str, needle: &str, n: usize) -> (usize, usize) {
    let (at, _) = text.match_indices(needle).nth(n).expect("the needle");
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    (line, at - before.rfind('\n').map_or(0, |i| i + 1) + 1)
}

/// `assert_eq!`, for whole files: the first line that differs, not two copies of the file.
#[track_caller]
fn same(got: &str, want: &str) {
    if got != want {
        let (g, w) = (got.lines(), want.lines());
        let (n, (a, b)) = g
            .zip(w)
            .enumerate()
            .find(|(_, (a, b))| a != b)
            .unwrap_or((usize::MAX, ("<ran out of lines>", "<ran out of lines>")));
        panic!("line {}:\n  got  {a:?}\n  want {b:?}", n.wrapping_add(1));
    }
}

/// What `cargo fmt` does: new bytes in place, finished a moment ago, with a time of their own.
fn rewrite(path: &Path, text: &str) {
    let was = fs::metadata(path).unwrap().modified().unwrap();
    fs::write(path, text).unwrap();
    let f = fs::File::options().write(true).open(path).unwrap();
    let newest = SystemTime::now() - Duration::from_secs(1);
    f.set_modified(newest.max(was + Duration::from_secs(1)))
        .unwrap();
}

/// Rename a function across files: `:grep`, the results' `:cn`, `*`, `cio`, `n` and `.`, then `:s`,
/// and `gf` from the results for the last file. A word stops at `_` in your Evil, so it's `io`.
#[test]
fn rename_a_function_across_files() {
    let mut d = Day::new("rename");
    let (ex, lib, window) = (d.disk("ex.rs"), d.disk("lib.rs"), d.disk("window.rs"));
    let count = ex.matches("quit_app").count();
    assert!(count >= 2, "the task needs two in ex.rs");
    d.open("window.rs");

    d.keys(":grep quit_app<CR>");
    assert_eq!(d.at().0, "window.rs", "the focus stays");
    d.keys(":cn<CR>");
    let first = place(&ex, "quit_app", 0);
    assert_eq!(d.at(), ("ex.rs".to_string(), first.0, first.1));

    d.keys("*");
    let second = place(&ex, "quit_app", 1);
    assert_eq!(d.at(), ("ex.rs".to_string(), second.0, second.1));
    d.keys("Ncioleave_app<Esc>");
    for _ in 1..count {
        d.keys("n.");
    }
    d.keys(":w<CR>");
    same(&d.disk("ex.rs"), &ex.replace("quit_app", "leave_app"));
    let last = place(&ex, "quit_app", count - 1);
    assert_eq!(
        d.at(),
        ("ex.rs".to_string(), last.0, last.1 + "leave_app".len() - 1)
    );

    // Through the rest of the results: the lines are where they were, so `:s` needs no search.
    for _ in 0..count {
        d.keys(":cn<CR>");
    }
    assert_eq!(d.at().0, "lib.rs");
    d.keys(":s/quit_app/leave_app/<CR>:w<CR>");
    same(&d.disk("lib.rs"), &lib.replace("quit_app", "leave_app"));

    // To the results, and `gf` on the line naming the last file.
    d.keys("<C-w>wgg/window.rs<CR>");
    assert_eq!(d.at().0, "*grep*");
    d.keys("gf");
    let call = place(&window, "quit_app", 0);
    assert_eq!(d.at(), ("window.rs".to_string(), call.0, call.1));
    d.keys("cioleave_app<Esc>/quit_app<CR>.:w<CR>");
    same(
        &d.disk("window.rs"),
        &window.replace("quit_app", "leave_app"),
    );
}

/// Add a match arm: yank one and change it.
#[test]
fn add_a_match_arm_by_yanking_and_changing_one() {
    let mut d = Day::new("arm");
    let ex = d.disk("ex.rs");
    d.open("ex.rs");
    let arm = "        \"b\" | \"buffer\" => buffer(ctx, rest),\n";
    let (line, _) = place(&ex, arm, 0);

    d.keys("/\"b\" <CR>yyp");
    assert_eq!(
        d.at(),
        ("ex.rs".to_string(), line + 1, 9),
        "the pasted line"
    );
    d.keys("ci\"bn<Esc>");
    d.keys("f|wci\"bnext<Esc>");
    d.keys("f=wcwnext_buffer<Esc>");
    d.keys("lci(ctx<Esc>");
    let new = "        \"bn\" | \"bnext\" => next_buffer(ctx),\n";
    assert_eq!(d.text(), ex.replace(arm, &format!("{arm}{new}")));
    assert_eq!(d.at().1, line + 1);
    assert_eq!(d.at().2, new.find("ctx)").unwrap() + 3);

    // Five changes, and each is one `u`.
    d.keys("uuuu");
    assert_eq!(d.text(), ex.replace(arm, &format!("{arm}{arm}")));
    d.keys("u");
    assert_eq!(d.text(), ex);
}

/// Move a function to the other file with `dap` and `p` across a split.
#[test]
fn move_a_function_across_a_split() {
    let mut d = Day::new("move");
    let (ex, window) = (d.disk("ex.rs"), d.disk("window.rs"));
    let doc = "/// Quit, which refuses while a file has unsaved changes.";
    let start = window.find(doc).expect("the function");
    let end = start + window[start..].find("\n\n").expect("its end") + 2;
    let function = &window[start..end];
    d.open("ex.rs");

    d.keys(":vs<CR>");
    d.keys(":e window.rs<CR>");
    d.keys("/Quit, which<CR>dap");
    assert_eq!(d.text(), format!("{}{}", &window[..start], &window[end..]));
    d.keys("<C-w>wG");
    assert_eq!(d.at().0, "ex.rs");
    let last = d.at().1;
    d.keys("p");
    assert_eq!(d.at(), ("ex.rs".to_string(), last + 1, 1));
    d.keys(":wa<CR>");
    assert_eq!(d.disk("ex.rs"), format!("{ex}{function}"));
    assert_eq!(
        d.disk("window.rs"),
        format!("{}{}", &window[..start], &window[end..])
    );
}

/// Reflow a doc comment by hand: `J` takes the next line's leader along, so `dW` takes that off;
/// then the long line is broken with `D`, `o` and `C-r"`.
#[test]
fn reflow_a_doc_comment_by_hand() {
    let mut d = Day::new("reflow");
    d.seed(
        "doc.rs",
        "    /// The lines `:s` looks at,\n    /// which are the cursor's line\n    /// unless a range says otherwise.\n    fn lines() {}\n",
    );
    d.open("doc.rs");

    d.keys("JldW");
    assert_eq!(
        d.text(),
        "    /// The lines `:s` looks at, which are the cursor's line\n    /// unless a range says otherwise.\n    fn lines() {}\n"
    );
    assert_eq!(d.at(), ("doc.rs".to_string(), 1, 34));
    d.keys("JldW");
    assert_eq!(
        d.text(),
        "    /// The lines `:s` looks at, which are the cursor's line unless a range says otherwise.\n    fn lines() {}\n"
    );
    d.keys("0/unless<CR>Do/// <C-r>\"<Esc>");
    assert_eq!(
        d.text(),
        "    /// The lines `:s` looks at, which are the cursor's line \n    /// unless a range says otherwise.\n    fn lines() {}\n"
    );
    assert_eq!(d.at().1, 2);
    d.keys("k$x");
    assert_eq!(
        d.text(),
        "    /// The lines `:s` looks at, which are the cursor's line\n    /// unless a range says otherwise.\n    fn lines() {}\n"
    );
    assert_eq!(d.at(), ("doc.rs".to_string(), 1, 60));
}

/// Edit a table with a visual block: drop a column, change one, put text in front and behind.
#[test]
fn edit_a_table_with_a_visual_block() {
    let mut d = Day::new("table");
    let table = "| Crate    | Was   | Now   |\n\
                 | die-core | 5,450 | 9,970 |\n\
                 | layout   | 888   | 914   |\n\
                 | session  | none  | 1,086 |\n";
    d.seed("table.md", table);
    d.open("table.md");

    d.keys("f|<C-v>3jf|hd");
    let dropped = "| Crate    | Now   |\n\
                   | die-core | 9,970 |\n\
                   | layout   | 914   |\n\
                   | session  | 1,086 |\n";
    assert_eq!(d.text(), dropped);
    assert_eq!(d.at(), ("table.md".to_string(), 1, 12));

    d.keys("0<C-v>3j$A <<Esc>");
    let appended = "| Crate    | Now   | <\n\
                    | die-core | 9,970 | <\n\
                    | layout   | 914   | <\n\
                    | session  | 1,086 | <\n";
    assert_eq!(d.text(), appended);

    d.keys("gg03w<C-v>3j4lcsize<Esc>");
    let changed = "| Crate    | size | <\n\
                   | die-core | size | <\n\
                   | layout   | size | <\n\
                   | session  | size | <\n";
    assert_eq!(d.text(), changed);
    assert_eq!(
        d.at(),
        ("table.md".to_string(), 1, 17),
        "on the last character typed"
    );

    d.keys("uu");
    assert_eq!(d.text(), dropped);
    d.keys("u");
    assert_eq!(d.text(), table);
}

/// A macro over 20 lines of real source, and one `u` for all the runs of a counted `@`.
#[test]
fn a_macro_over_twenty_lines() {
    let mut d = Day::new("macro");
    let lib = d.disk("lib.rs");
    d.open("lib.rs");
    let commented = |upto: usize| -> String {
        lib.split_inclusive('\n')
            .enumerate()
            .map(|(i, l)| {
                if (4..upto).contains(&i) {
                    format!("// {l}")
                } else {
                    l.to_string()
                }
            })
            .collect()
    };
    d.keys(":5<CR>qa0i// <Esc>jq");
    assert_eq!(d.text(), commented(5));
    assert_eq!(d.at().1, 6);
    d.keys("19@a");
    assert_eq!(d.text(), commented(24));
    assert_eq!(d.at().1, 25);
    d.keys("@@");
    assert_eq!(d.text(), commented(25));
    d.keys("uu");
    assert_eq!(d.text(), commented(5));
    assert_eq!(d.at().1, 6);
    d.keys("<C-r>");
    assert_eq!(d.text(), commented(24));
}

/// `C-o` back after a search, and `Tab` forward again, by the same places.
#[test]
fn c_o_goes_back_after_a_search() {
    let mut d = Day::new("jump");
    let window = d.disk("window.rs");
    d.open("window.rs");
    let at = |n: usize| {
        let (line, byte) = place(&window, "fn ", n);
        ("window.rs".to_string(), line, byte)
    };
    let top = ("window.rs".to_string(), 1, 1);
    d.keys("/fn <CR>");
    assert_eq!(d.at(), at(0));
    d.keys("nn");
    assert_eq!(d.at(), at(2));
    d.keys("<C-o>");
    assert_eq!(d.at(), at(1));
    d.keys("<C-o><C-o>");
    assert_eq!(d.at(), top);
    d.keys("<Tab>");
    assert_eq!(d.at(), at(0));
    // A change where `C-o` left us, and on: the search still has its pattern.
    d.keys("n");
    assert_eq!(d.at(), at(1));
    d.keys("x<C-o>");
    assert_eq!(d.at(), at(0));
}

/// Insert `C-n`: an identifier from this buffer, from another, and the next candidate.
#[test]
fn insert_c_n_completes_an_identifier() {
    let mut d = Day::new("complete");
    let window = d.disk("window.rs");
    d.open("ex.rs");
    d.open("window.rs");
    let (line, _) = place(&window, "pub fn quit_app", 0);

    d.keys("/pub fn quit_app<CR>Odrop_cha<C-n>(ctx, b)<Esc>");
    let new = "drop_changes(ctx, b)\n";
    let after = window.replacen("pub fn quit_app", &format!("{new}pub fn quit_app"), 1);
    assert_eq!(d.text(), after);
    assert_eq!(d.at(), ("window.rs".to_string(), line, new.len() - 1));

    d.keys("ofind_file_at<C-n><Esc>");
    let line2 = "find_file_at_point\n";
    let after = window.replacen(
        "pub fn quit_app",
        &format!("{new}{line2}pub fn quit_app"),
        1,
    );
    assert_eq!(d.text(), after, "from the other file");
    assert_eq!(d.at(), ("window.rs".to_string(), line + 1, line2.len() - 1));

    d.keys("uu");
    assert_eq!(d.text(), window);
    assert_eq!(d.failing(":noh<CR>"), Vec::<String>::new());
}

/// An undo chain through every kind of change, and redo back up it.
#[test]
fn undo_chains_after_all_of_that() {
    let mut d = Day::new("undo");
    let window = d.disk("window.rs");
    d.open("window.rs");
    let mut seen = vec![window.clone()];
    for keys in [
        "dd",
        "ciwWindow<Esc>",
        "yyjp",
        "qqA;<Esc>jq",
        "3@q",
        ":%s/the/THE/g<CR>",
        "<C-v>jlrx",
        "ggVjd",
        "Gop<Esc>",
        "ggJ",
        "dap",
    ] {
        d.keys(keys);
        let now = d.text();
        assert_ne!(seen.last(), Some(&now), "{keys} changes something");
        seen.push(now);
    }
    // `qq..q` made one change, and `3@q` one.
    for want in seen.iter().rev().skip(1) {
        d.keys("u");
        assert_eq!(&d.text(), want);
    }
    assert!(!d.h.text(d.h.focused_buffer().unwrap()).unwrap().modified());
    assert_eq!(
        d.at(),
        ("window.rs".to_string(), 1, 1),
        "back where it began"
    );
    for want in seen.iter().skip(1) {
        d.keys("<C-r>");
        assert_eq!(&d.text(), want);
    }
    // Undone again, as far as it goes, and a new change from the middle.
    d.keys("uuuu");
    d.keys("Ox<Esc>u");
    assert_eq!(&d.text(), &seen[seen.len() - 5]);
}

/// `:e!` over a morning's edits, with the cursor kept and `u` to bring them back.
#[test]
fn e_bang_reverts_and_u_brings_it_back() {
    let mut d = Day::new("revert");
    let window = d.disk("window.rs");
    d.open("window.rs");
    d.keys("3Gdd");
    d.keys("Gonew line<Esc>");
    let edited = d.text();
    d.keys(":9<CR>0");
    d.keys(":e!<CR>");
    same(&d.text(), &window);
    assert_eq!(d.at(), ("window.rs".to_string(), 10, 1), "on the same text");
    assert!(!d.h.text(d.h.focused_buffer().unwrap()).unwrap().modified());
    d.keys("u");
    assert_eq!(d.text(), edited);
    d.keys(":e!<CR>x:w<CR>");
    assert_eq!(d.disk("window.rs"), d.text());
}

/// `cargo fmt` rewrites a clean buffer, which follows, and a dirty one, which refuses to be
/// written over, until `:e!` or `:w!`.
#[test]
fn cargo_fmt_underneath_a_clean_buffer_and_a_dirty_one() {
    let mut d = Day::new("fmt");
    let unformatted = "fn main( ) {\nlet a=1;\nlet b=2;\n}\n";
    let formatted = "fn main() {\n    let a = 1;\n    let b = 2;\n}\n";
    d.seed("clean.rs", unformatted);
    d.seed("dirty.rs", unformatted);
    let (clean, dirty) = (d.path("clean.rs"), d.path("dirty.rs"));
    d.open("dirty.rs");
    d.keys("Go// mine<Esc>");
    d.open("clean.rs");
    d.keys("3G");

    rewrite(&clean, formatted);
    rewrite(&dirty, formatted);
    d.h.check_files();
    assert_eq!(d.text(), formatted, "a clean buffer follows");
    assert_eq!(
        d.at(),
        ("clean.rs".to_string(), 3, 1),
        "and the cursor keeps its line"
    );
    assert_eq!(d.h.take_errors(), Vec::<String>::new());
    d.keys("^cwconst<Esc>:w<CR>");
    assert_eq!(
        fs::read_to_string(&clean).unwrap(),
        formatted.replace("let b", "const b")
    );

    d.keys(":b dirty.rs<CR>");
    let mine = format!("{unformatted}// mine\n");
    assert_eq!(d.text(), mine, "a dirty one stays");
    let said =
        d.h.messages
            .iter()
            .any(|m| m.contains("dirty.rs changed on disk"));
    assert!(said, "{:?}", d.h.messages);
    let refused = d.failing(":w<CR>");
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].contains("changed on disk"), "{refused:?}");
    assert_eq!(fs::read_to_string(&dirty).unwrap(), formatted);

    d.keys(":e!<CR>");
    assert_eq!(d.text(), formatted);
    d.keys("u");
    assert_eq!(d.text(), mine);
    assert!(d.h.text(d.h.focused_buffer().unwrap()).unwrap().modified());
    d.keys(":w!<CR>");
    assert_eq!(fs::read_to_string(&dirty).unwrap(), mine);
}

/// A restart half way: splits, unsaved text, `n`, `.`, `@a`, `u` and `C-o` all go on, and `:grep`
/// works after it. Its results don't come back: core doesn't keep them.
#[test]
fn a_restart_half_way_and_the_keys_after_it_still_work() {
    let mut d = Day::new("restart");
    let (ex, window) = (d.disk("ex.rs"), d.disk("window.rs"));
    d.open("ex.rs");
    d.keys(":vs<CR>:e window.rs<CR>");
    d.keys("/quit_app<CR>cioleave_app<Esc>");
    d.keys("qa0i// <Esc>jq");
    let edited = window.replacen("quit_app", "leave_app", 1);
    let (changed, _) = place(&window, "quit_app", 0);
    let edited: String = edited
        .split_inclusive('\n')
        .enumerate()
        .map(|(i, l)| {
            if i + 1 == changed {
                format!("// {l}")
            } else {
                l.to_string()
            }
        })
        .collect();
    same(&d.text(), &edited);
    let cursor = ("window.rs".to_string(), changed + 1, 1);
    assert_eq!(d.at(), cursor);

    d.restart();
    assert_eq!(d.h.window_list().len(), 2);
    same(&d.text(), &edited);
    assert_eq!(d.at(), cursor);
    assert!(d.h.text(d.h.focused_buffer().unwrap()).unwrap().modified());

    // `n` keeps its pattern, and `.` is the macro's insert, whose `u` takes back only that.
    d.keys("n");
    let next = place(&edited, "quit_app", 0);
    assert_eq!(d.at(), ("window.rs".to_string(), next.0, next.1));
    d.keys(".");
    let (l, c) = next;
    assert_eq!(
        d.at(),
        ("window.rs".to_string(), l, c + 2),
        "typed where the cursor was"
    );
    d.keys("u");
    same(&d.text(), &edited);
    d.keys("0@a");
    let col = edited.lines().nth(l).unwrap().len().clamp(1, 3);
    assert_eq!(
        d.at(),
        ("window.rs".to_string(), l + 1, col),
        "the macro's own j"
    );
    d.keys("u");
    same(&d.text(), &edited);
    // Each `u` is a jump, as in your Evil, so `n` is three back.
    d.keys("<C-o><C-o><C-o>");
    assert_eq!(d.at().1, changed + 1, "from before the search");

    // `:grep` and `:cn` from here, and `:wa` writes what was unsaved.
    d.keys(":grep close_or_quit<CR>:cn<CR>");
    assert_eq!(d.at().0, "ex.rs");
    assert_eq!(d.at().1, place(&ex, "close_or_quit", 0).0);
    d.keys(":wa<CR>");
    same(&d.disk("window.rs"), &edited);
    d.keys("gg0x:w<CR>");
    same(&d.disk("ex.rs"), &ex[1..]);
}

/// `:w` to a read-only file refuses and loses nothing, and `:w!` replaces it, as in Vim. Emacs
/// would make the buffer read-only as it opened, and that wouldn't stop `:w!` either.
#[test]
fn w_to_a_read_only_file() {
    use std::os::unix::fs::PermissionsExt;
    let mut d = Day::new("readonly");
    d.seed("locked.rs", "one\n");
    let locked = d.path("locked.rs");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o444)).unwrap();
    // Root writes any file, so this only means something when a write really fails
    // (`cargo xtask ci --linux` runs its tests as root).
    if fs::OpenOptions::new().append(true).open(&locked).is_ok() {
        return;
    }
    d.open("locked.rs");
    d.keys("x");
    assert_eq!(d.text(), "ne\n");
    let refused = d.failing(":w<CR>");
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].contains("locked.rs"), "{refused:?}");
    assert_eq!(fs::read_to_string(&locked).unwrap(), "one\n");
    assert_eq!(d.text(), "ne\n");
    assert!(d.h.text(d.h.focused_buffer().unwrap()).unwrap().modified());
    assert_eq!(d.at(), ("locked.rs".to_string(), 1, 1));
    d.keys(":w!<CR>");
    assert_eq!(fs::read_to_string(&locked).unwrap(), "ne\n");
}

/// Getting around the open files with `C-^`, `:bn`, `:bp`, `:up`, `:bd` and `ZZ`.
#[test]
fn going_round_the_open_files() {
    let mut d = Day::new("buffers");
    let (ex, lib) = (d.disk("ex.rs"), d.disk("lib.rs"));
    d.open("ex.rs");
    d.open("lib.rs");
    d.open("window.rs");

    d.keys("<C-^>");
    assert_eq!(d.at().0, "lib.rs", "the one used before");
    d.keys("<C-^>");
    assert_eq!(d.at().0, "window.rs", "and back");
    d.keys(":bn<CR>");
    assert_eq!(
        d.at().0,
        "ex.rs",
        "the order they opened, round to the first"
    );
    d.keys(":bn<CR>");
    assert_eq!(d.at().0, "lib.rs");
    d.keys(":bp<CR>:bp<CR>");
    assert_eq!(d.at().0, "window.rs");
    d.keys(":b lib.rs<CR>x:up<CR>");
    same(&d.disk("lib.rs"), &lib[1..]);

    d.keys(":b ex.rs<CR>x");
    let refused = d.failing(":bd<CR>");
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].contains("unsaved"), "{refused:?}");
    d.keys(":vs<CR>");
    assert_eq!(d.h.window_list().len(), 2);
    d.keys(":bd!<CR>");
    assert_eq!(d.h.find_buffer("ex.rs"), None);
    assert_eq!(
        d.h.window_list().len(),
        1,
        "the windows that showed it close"
    );
    same(&d.disk("ex.rs"), &ex);

    d.keys("xZZ");
    assert!(d.h.quit_wanted());
    let name = d.at().0;
    same(&d.disk(&name), &d.text());
}

/// `\f`, `\x`, `\]` and `:kill-buffer`, and the keys that went to a killed buffer.
#[test]
fn leader_keys_and_kill_buffer() {
    let mut d = Day::new("leader");
    let (ex, window) = (d.disk("ex.rs"), d.disk("window.rs"));
    d.open("ex.rs");

    d.keys("\\fwindow.rs<CR>");
    assert_eq!(d.at(), ("window.rs".to_string(), 1, 1));
    assert_eq!(d.text(), window);
    d.keys("x");

    d.keys("\\xvim-undo<CR>");
    assert_eq!(d.text(), window);
    d.keys("G");

    d.keys("\\]<CR>");
    assert_eq!(d.at(), ("ex.rs".to_string(), 1, 1), "the most recent other");
    d.keys("\\]<CR>");
    assert_eq!(d.at().0, "window.rs", "and back");
    assert_eq!(d.at().1, window.lines().count(), "where it was");

    d.keys(":kill-buffer<CR><CR>");
    assert_eq!(d.at(), ("ex.rs".to_string(), 1, 1));
    assert_eq!(d.h.find_buffer("window.rs"), None);
    assert_eq!(d.text(), ex);
    // A jump into what was killed is skipped or told, and doesn't panic.
    let told = d.failing("<C-o>");
    assert!(told.iter().all(|e| !e.contains("panic")), "{told:?}");
    assert_eq!(d.at().0, "ex.rs");
    d.keys("x");
    assert_ne!(d.text(), ex);

    // A dirty buffer asks first, and a no leaves it.
    d.keys(":kill-buffer<CR><CR>n");
    assert_eq!(d.at().0, "ex.rs");
    assert_ne!(d.text(), ex);
}
