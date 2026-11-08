//! What a day throws at an editor. A hang is the failure here, so each test has a watchdog.

use die_test_cap as _;

use die_core::{Harness, Input, parse_keys};
use die_vim::Vim;
use std::time::Duration;

fn editor(text: &str) -> (Harness, die_core::BufferId) {
    let mut h = Harness::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    let (b, _) = h.open_text("t", text).unwrap();
    (h, b)
}

fn watchdog(secs: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(secs));
        eprintln!("hung for {secs} s");
        std::process::abort();
    });
}

fn keys(h: &mut Harness, k: &str) -> Vec<String> {
    let log = h.type_keys(k).unwrap();
    log.into_iter().flat_map(|k| k.errors).collect()
}

fn text(h: &Harness, b: die_core::BufferId) -> String {
    h.text(b).unwrap().rope().to_string()
}

#[test]
fn macros_that_call_themselves_end_soon() {
    watchdog(120);
    let (mut h, b) = editor("abc\ndef\n");
    keys(&mut h, "qa@aq");
    assert!(keys(&mut h, "@a").iter().any(|e| e.contains("Too many")));
    keys(&mut h, "qeqqfqqe@f@fqqf@e@eq");
    assert!(keys(&mut h, "@e").iter().any(|e| e.contains("Too many")));
    assert_eq!(text(&h, b), "abc\ndef\n");
    assert_eq!(keys(&mut h, "x"), Vec::<String>::new());
}

#[test]
fn a_typed_c_g_ends_a_macro() {
    watchdog(120);
    let (mut h, b) = editor("abc\n");
    keys(&mut h, "qbix<Esc>@bq");
    let typed = text(&h, b).len();
    for k in parse_keys("@b<C-g>").unwrap() {
        h.handle(Input::Key(k));
    }
    while h.run_ready() {}
    assert_eq!(text(&h, b).len(), typed, "it stopped before its first key");
    assert_eq!(h.messages.last().map(String::as_str), Some("Quit"));
}

#[test]
fn a_line_of_megabytes_moves_and_edits() {
    watchdog(120);
    let line = "a".repeat(5_000_000);
    let (mut h, b) = editor(&line);
    h.set_window_size(100, 50);
    keys(&mut h, "w");
    assert_eq!(h.text(b).unwrap().line_count(), 1);
    keys(&mut h, "0wbx$xiy<Esc>uu/a<CR>n:noh<CR>");
    assert_eq!(text(&h, b).len(), line.len() - 1);
}

#[test]
fn marks_by_the_million_stay_usable() {
    watchdog(120);
    let marks = format!("e{}\n", "\u{301}".repeat(2_500_000));
    let (mut h, b) = editor(&marks);
    h.set_window_size(100, 50);
    h.screen();
    keys(&mut h, "l$h0xu");
    assert_eq!(text(&h, b), marks);
    let flags = format!("{}\n", "\u{1f1fa}\u{1f1f8}".repeat(600_000));
    let (mut h, b) = editor(&flags);
    h.set_window_size(100, 50);
    keys(&mut h, "$xu0");
    assert_eq!(text(&h, b), flags);
}

#[test]
fn whole_file_operators_finish_and_undo() {
    watchdog(300);
    let lines: String = (0..300_000)
        .map(|i| format!("  line {i} words\n"))
        .collect();
    let (mut h, b) = editor(&lines);
    for op in [
        "gg>G",
        "ggVGrX",
        "gg<C-v>G$d",
        "ggg~G",
        ":%s/line/x/g<CR>",
        ":%s/\\n/;/<CR>",
    ] {
        assert_eq!(keys(&mut h, op), Vec::<String>::new(), "{op}");
        assert_ne!(text(&h, b), lines, "{op}");
        keys(&mut h, "u");
        assert_eq!(text(&h, b), lines, "u after {op}");
        keys(&mut h, "<C-r>");
        assert_ne!(text(&h, b), lines, "<C-r> after {op}");
        keys(&mut h, "u");
        assert_eq!(text(&h, b), lines, "the last u after {op}");
    }
}

#[test]
fn r_over_a_selection_goes_by_clusters_across_the_ropes_chunks() {
    let piece = "abc e\u{301} 日本 🙂\r\n\n";
    let lines = piece.repeat(5000);
    let (mut h, b) = editor(&lines);
    keys(&mut h, "ggVGrX");
    let want = "XXXXXXXXXXX\n\n".repeat(5000);
    assert_eq!(text(&h, b), want);
    assert_eq!(h.text(b).unwrap().line_count(), 10_001);
}

#[test]
fn star_on_a_word_too_long_for_a_pattern_says_so() {
    watchdog(120);
    let (mut h, _) = editor(&"a".repeat(100_000));
    let e = keys(&mut h, "*");
    assert!(e.iter().any(|e| e.contains("too long")), "{e:?}");
    assert_eq!(keys(&mut h, "x"), Vec::<String>::new());
}

#[test]
fn a_huge_register_stays_out_of_the_session() {
    let (mut h, _) = editor(&"a".repeat(2 << 20));
    keys(&mut h, "yy\"_ddishort text<Esc>yy");
    let session = h.session_json();
    assert!(session.contains("short text"));
    assert!(session.len() < 1 << 20, "{} bytes", session.len());
}
