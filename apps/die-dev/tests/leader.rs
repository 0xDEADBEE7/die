//! Your leader keys and the prompts they open, end to end: the keys go through your bindings and
//! your Evil's keymaps, with the clipboard faked.

use die_test_cap as _;

use die::core::{BufferId, Harness};
use die::vim::Vim;
use die_dev::config;
use std::path::PathBuf;

fn harness(text: &str) -> (Harness, BufferId) {
    let exts: Vec<Box<dyn die::core::Extension>> = vec![Box::new(Vim::default())];
    let mut h = Harness::with_config(exts, config(vec![]).core).unwrap();
    let (b, _) = h.open_text("t", text).unwrap();
    (h, b)
}

fn typed(h: &mut Harness, keys: &str) {
    let errors: Vec<_> = h.type_keys(keys).unwrap();
    let errors: Vec<_> = errors.into_iter().flat_map(|k| k.errors).collect();
    assert!(errors.is_empty(), "{keys}: {errors:?}");
}

fn text(h: &Harness, b: BufferId) -> String {
    h.text(b).unwrap().rope().to_string()
}

fn help(h: &Harness) -> String {
    text(h, h.find_buffer("*help*").expect("*help* is there"))
}

fn tree(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("die-dev-leader-{name}-{}", std::process::id()));
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(d.join("sub/found.rs"), "found\n").unwrap();
    d.canonicalize().unwrap()
}

#[test]
fn leader_x_runs_the_command_chosen_from_the_palette() {
    let (mut h, b) = harness("abc\n");
    typed(&mut h, "x");
    assert_eq!(text(&h, b), "bc\n");
    typed(&mut h, "\\xvim-undo");
    assert_eq!(h.prompt_chosen(), Some("vim-undo"));
    typed(&mut h, "<CR>");
    assert_eq!(text(&h, b), "abc\n");
    assert_eq!(h.prompt_text(), None);
}

#[test]
fn leader_f_goes_down_from_this_files_directory_and_opens_the_file() {
    let d = tree("find");
    let (mut h, _) = harness("");
    let here = h.open_file(&d.join("here.txt")).unwrap();
    h.show(here, die::core::ShowIn::Current).unwrap();
    typed(&mut h, "\\f");
    let start = h.prompt_text().unwrap().to_string();
    assert!(start.ends_with('/'), "{start}");
    assert!(h.prompt_candidates().contains(&"sub/"));
    typed(&mut h, "sub/fnd<CR>");
    let found = h.focused_buffer().unwrap();
    assert_eq!(text(&h, found), "found\n");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn leader_bracket_goes_back_and_leaves_a_modified_buffer_as_it_was() {
    let (mut h, b) = harness("one\ntwo\n");
    let other = h.create_text("other", "");
    typed(&mut h, "jx\\]other<CR>");
    assert_eq!(h.focused_buffer(), Some(other));
    typed(&mut h, "\\]");
    assert_eq!(h.prompt_chosen(), Some("t"), "the last buffer comes first");
    typed(&mut h, "<CR>");
    assert_eq!(h.focused_buffer(), Some(b));
    assert_eq!(text(&h, b), "one\nwo\n", "the change is still there");
    assert!(h.text(b).unwrap().modified());
    typed(&mut h, "x");
    assert_eq!(text(&h, b), "one\no\n", "and so is the cursor");
}

#[test]
fn leader_hk_describes_the_whole_sequence_without_running_it() {
    let (mut h, b) = harness("abc\n");
    typed(&mut h, "\\hk\\x");
    assert_eq!(h.prompt_text(), None, "the palette didn't open");
    assert_eq!(h.focused_buffer(), Some(b), "the focus stays");
    assert!(help(&h).contains("runs command-palette"), "{}", help(&h));
    typed(&mut h, "\\hkx");
    assert_eq!(text(&h, b), "abc\n", "x deleted nothing");
    assert!(help(&h).contains("x in Normal runs"), "{}", help(&h));
    let screen = h.screen();
    assert_eq!(
        screen.split('\n').next_back(),
        Some(""),
        "the question has gone"
    );
    assert_eq!(h.window_list().len(), 2, "the same window, used again");
}

#[test]
fn leader_hc_shows_a_commands_doc_and_your_key_for_it() {
    let (mut h, b) = harness("abc\n");
    typed(&mut h, "\\hcfind-file<CR>");
    assert_eq!(h.focused_buffer(), Some(b));
    let shown = help(&h);
    assert!(shown.contains("Open a file"), "{shown}");
    assert!(shown.contains("<leader>f in Normal"), "{shown}");
}

/// `<D-v>` on `:`, on `/` and in the candidates prompts, with what you'd have copied elsewhere.
#[test]
fn cmd_v_pastes_a_word_or_a_path_into_every_prompt() {
    let d = tree("paste");
    let (mut h, b) = harness("foo bar foo\n");
    h.fake_clipboard().copy_from_outside("bar\n");

    typed(&mut h, ":s/<D-v>/X/<CR>");
    assert_eq!(
        text(&h, b),
        "foo X foo\n",
        "on :, and its last newline goes"
    );

    h.fake_clipboard().copy_from_outside("foo");
    typed(&mut h, "$/<D-v><CR>");
    assert_eq!(h.text(b).unwrap().cursor(h.focused().unwrap()), Some(0));
    assert_eq!(h.prompt_history("search").last().unwrap(), "foo", "on /");

    let other = h.create_text("other-buffer", "");
    h.fake_clipboard().copy_from_outside("other-b");
    typed(&mut h, "\\]<D-v>");
    assert_eq!(h.prompt_text(), Some("other-b"));
    assert_eq!(
        h.prompt_candidates(),
        ["other-buffer"],
        "it filters the list"
    );
    typed(&mut h, "<CR>");
    assert_eq!(h.focused_buffer(), Some(other));

    let path = d.join("sub/found.rs");
    h.fake_clipboard()
        .copy_from_outside(&format!("{}\n", path.display()));
    typed(&mut h, "\\f<D-v>");
    assert_eq!(h.prompt_chosen(), Some("found.rs"));
    typed(&mut h, "<CR>");
    let found = h.focused_buffer().unwrap();
    assert_eq!(text(&h, found), "found\n", "over the line it started with");
    std::fs::remove_dir_all(&d).unwrap();
}
