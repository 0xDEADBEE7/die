//! A chat on a harness with die-vim, on hand-written sessions under a temporary root, with orchid
//! faked: the layout a restore makes, the collapsible sections and `TAB`, search and motions over
//! hidden text, the read-only parts, the slash menu, the keys that close it or go to the browser,
//! older history, and the stats. The Emacs client's `orchid-chat-session-test`,
//! `orchid-chat-header-test` and `orchid-collapsible-test` specs that hold for orchid 0.2 are here.

use die_test_cap as _;

use die_core::{Args, BufferId, Deco, Harness, State, WindowId, parse_keys};
use die_orchid_client::chat::{self, SLASH};
use die_orchid_client::cli::{FakeRunner, Reply};
use die_orchid_client::fixtures::{self, AT, MARGINAL, Meta, TempRoot};
use die_orchid_client::store::Metadata;
use die_orchid_client::{Orchid, face, parse, sections, test_zone};
use die_vim::Vim;
use serde_json::json;

const ID: &str = "0f1e2d3c-aaaa-bbbb-cccc-123456789abc";
const NAME: &str = "*orchid-chat-default-die-89abc*";

fn harness(orchid: Orchid) -> Harness {
    let mut h = Harness::new(vec![Box::new(Vim::default()), Box::new(orchid)]).unwrap();
    h.open_text("notes", "").unwrap();
    h
}

/// A chat on a session with these events, in an 80 by 40 window.
fn opened(root: &TempRoot, fake: &FakeRunner, events: &[String]) -> (Harness, BufferId, WindowId) {
    opened_with(
        Orchid::for_tests(root.path(), fake.clone()),
        root,
        Meta::new(ID),
        events,
    )
}

fn opened_with(
    orchid: Orchid,
    root: &TempRoot,
    meta: Meta,
    events: &[String],
) -> (Harness, BufferId, WindowId) {
    let mut h = harness(orchid);
    h.set_window_size(80, 40);
    meta.write(root.path());
    fixtures::append(root.path(), &meta.id, events);
    chat::open(&mut h, &meta.id).unwrap();
    h.run_until_idle();
    assert!(h.take_errors().is_empty());
    let (b, w) = (h.focused_buffer().unwrap(), h.focused().unwrap());
    (h, b, w)
}

fn text(h: &Harness, b: BufferId) -> String {
    h.text(b).unwrap().rope().to_string()
}

/// The window's rows, without the status and command lines.
fn rows(h: &mut Harness) -> Vec<String> {
    let screen = h.screen();
    let mut all: Vec<String> = screen.lines().map(String::from).collect();
    all.truncate(all.len().saturating_sub(2));
    while all.last().is_some_and(String::is_empty) {
        all.pop();
    }
    all
}

/// The errors typing `k` gave.
fn keys(h: &mut Harness, k: &str) -> Vec<String> {
    let log = h.type_keys(k).unwrap();
    log.into_iter().flat_map(|k| k.errors).collect()
}

/// Put the cursor on the `n`th `needle`, from 0.
fn on(h: &mut Harness, b: BufferId, w: WindowId, needle: &str, n: usize) -> usize {
    let t = text(h, b);
    let at = t.match_indices(needle).nth(n).expect(needle).0;
    h.text_mut(b).unwrap().set_cursor(w, at);
    at
}

fn cursor(h: &Harness, b: BufferId, w: WindowId) -> usize {
    h.text(b).unwrap().cursor(w).unwrap()
}

/// A stub as the fixtures' events show: at `AT`, with `MARGINAL`, in the test zone.
fn stub(label: &str, preview: &str) -> String {
    let at = AT.parse().unwrap();
    parse::stub_line(
        &format!("{label} [{preview}]"),
        Some(at),
        Some(MARGINAL),
        &test_zone(),
    )
}

fn faces_at(h: &Harness, b: BufferId, pos: usize) -> Vec<&'static str> {
    let t = h.text(b).unwrap();
    let f = t.decorations_at(pos).filter_map(|(_, d)| match d.deco {
        Deco::Face(f) => Some(f),
        _ => None,
    });
    f.collect()
}

/// The text under each read-only range, in order.
fn read_only(h: &Harness, b: BufferId) -> Vec<String> {
    let t = h.text(b).unwrap();
    let mut ranges: Vec<_> = t
        .decorations()
        .filter(|(_, d)| matches!(d.deco, Deco::ReadOnly))
        .map(|(_, d)| d.range.clone())
        .collect();
    ranges.sort_by_key(|r| r.start);
    let s = t.rope().to_string();
    ranges.into_iter().map(|r| s[r].to_string()).collect()
}

fn session_events() -> Vec<String> {
    vec![
        fixtures::message("e1", "user", "hello there"),
        fixtures::message("e2", "assistant", "Hi! How can I help?"),
        fixtures::tool_call("e3", "c1", "bash", json!({"cmd": "ls -la"})),
        fixtures::tool_result("e4", "c1", json!("a\nb")),
        fixtures::message("e5", "user", "again"),
        fixtures::failure("e6", "connection refused"),
    ]
}

// ------------------------------------------------------------------ the layout

#[test]
fn a_restore_lays_the_chat_out_as_emacs_did() {
    let root = TempRoot::new("layout");
    let fake = FakeRunner::new();
    let (mut h, b, w) = opened(&root, &fake, &session_events());
    assert_eq!(h.buffer_name(b), Some(NAME));
    let sep = chat::separator(Some("die"));
    assert_eq!(sep, format!("{}  die  {}", "─".repeat(36), "─".repeat(37)));
    let rule = "─".repeat(80);
    assert_eq!(
        stub("[user]", "hello there"),
        "[user]    [hello there                                    ]    [ 63k] [11:16:46]"
    );
    let restored = chat::restored_line(4_200_000);
    assert_eq!(
        restored,
        format!("[session restored]{}[ 4200k]\n", " ".repeat(54))
    );
    let want = [
        "Workspace: /tmp/die\nAgent: default\n".to_string(),
        sep.clone(),
        "\n\n".into(),
        stub("[user]", "hello there"),
        "\nhello there\n".into(),
        stub("[agnt]", "Hi! How can I help?"),
        "\n\n".into(),
        stub("[bash]", "ls -la"),
        "\n\n".into(),
        stub("[rslt]", &"─".repeat(47)),
        "\n  Call ID: c1\n  Result:\n  a\n  b\n".into(),
        // Before each of your messages that follows another event: Emacs's separator event and
        // the newline after every event.
        format!("\n{rule}\n\n\n"),
        stub("[user]", "again"),
        "\nagain\n".into(),
        stub("[fail]", "connection refused"),
        "\n  Message: connection refused\n".into(),
        "\n".into(),
        restored.clone(),
        sep.clone(),
        "\n\n".into(),
    ]
    .concat();
    assert_eq!(text(&h, b), want);
    // Collapsed, each event is its stub, 80 columns.
    let shown = rows(&mut h);
    let want_rows = [
        "Workspace: /tmp/die",
        "Agent: default",
        &sep,
        "",
        &stub("[user]", "hello there"),
        &stub("[agnt]", "Hi! How can I help?"),
        &stub("[bash]", "ls -la"),
        &stub("[rslt]", &"─".repeat(47)),
        "",
        &rule,
        "",
        "",
        &stub("[user]", "again"),
        &stub("[fail]", "connection refused"),
        "",
        restored.trim_end(),
        &sep,
    ];
    assert_eq!(shown, want_rows);
    for r in &shown[4..8] {
        assert_eq!(parse::width(r), 80, "{r}");
    }
    // Faces: the header's values, the separators, each kind of stub, the restored line.
    let t = text(&h, b);
    let pos = |s: &str| t.find(s).unwrap();
    assert_eq!(faces_at(&h, b, pos("/tmp/die")), [face::HEADER_WORKSPACE]);
    assert_eq!(faces_at(&h, b, pos("default\n")), [face::HEADER_AGENT]);
    assert!(faces_at(&h, b, pos("Workspace")).is_empty());
    assert_eq!(faces_at(&h, b, pos(&sep)), [face::SEPARATOR]);
    assert_eq!(faces_at(&h, b, pos("[user]")), [face::USER_STUB]);
    assert_eq!(faces_at(&h, b, pos("[agnt]")), [face::STUB]);
    assert_eq!(faces_at(&h, b, pos("[fail]")), [face::FAILURE_STUB]);
    assert_eq!(faces_at(&h, b, pos("[session restored]")), ["shadow"]);
    // Read-only exactly where Emacs had it: the restored line and the separator after it.
    assert_eq!(read_only(&h, b), [restored, sep]);
    // In the selected window, at the end, in normal state, as Emacs's `(goto-char (point-max))`.
    assert_eq!(h.window_buffer(w), Some(b));
    assert_eq!(cursor(&h, b, w), t.len());
    assert_eq!(h.state_of(b), State::Normal);
}

#[test]
fn with_nothing_to_restore_its_the_header_and_a_separator() {
    let root = TempRoot::new("empty");
    let fake = FakeRunner::new();
    let (mut h, b, _) = opened(&root, &fake, &[]);
    let sep = chat::separator(Some("die"));
    assert_eq!(
        text(&h, b),
        format!("Workspace: /tmp/die\nAgent: default\n{sep}\n\n")
    );
    assert!(
        read_only(&h, b).is_empty(),
        "no restored line, no read-only"
    );
    assert_eq!(rows(&mut h).len(), 3);
}

#[test]
fn the_header_says_the_workspace_and_the_agent() {
    let root = TempRoot::new("header");
    let fake = FakeRunner::new();
    let mut m = Meta::new(ID);
    m.agent_name = Some("developer".into());
    m.working_dir = Some("/home/dev/code".into());
    let orchid = Orchid::for_tests(root.path(), fake.clone());
    let (h, b, _) = opened_with(orchid, &root, m, &[]);
    let t = text(&h, b);
    assert!(
        t.starts_with("Workspace: /home/dev/code\nAgent: developer\n"),
        "{t}"
    );
    assert!(t.contains(&chat::separator(Some("code"))));
    assert!(!t.contains("Policy:"), "0.2 has agents");
    assert_eq!(h.buffer_name(b), Some("*orchid-chat-developer-code-89abc*"));
    // With neither, as a session from before 0.2: N/A, default, and a plain separator.
    let root = TempRoot::new("header-none");
    let mut m = Meta::new(ID);
    (m.agent_name, m.working_dir) = (None, None);
    let orchid = Orchid::for_tests(root.path(), fake.clone());
    let (h, b, _) = opened_with(orchid, &root, m, &[]);
    let t = text(&h, b);
    assert!(
        t.starts_with(&format!(
            "Workspace: N/A\nAgent: default\n{}\n",
            chat::rule()
        )),
        "{t}"
    );
    assert_eq!(h.buffer_name(b), Some("*orchid-chat-default-none-89abc*"));
}

#[test]
fn a_chat_is_named_for_its_agent_workspace_and_id_and_made_unique() {
    let m = |json: &str| Metadata::parse(json).unwrap();
    let name = |json: &str| chat::buffer_name(&m(json));
    assert_eq!(
        name(
            r#"{"id":"abc12345","status":"idle","agent_name":"Developer","working_dir":"/home/user/MyProject/"}"#
        ),
        "*orchid-chat-developer-myproject-12345*"
    );
    assert_eq!(
        name(r#"{"id":"abc12345","status":"idle","agent_name":"developer"}"#),
        "*orchid-chat-developer-none-12345*"
    );
    assert_eq!(
        name(r#"{"id":"ab1","status":"idle"}"#),
        "*orchid-chat-default-none-ab1*"
    );
    // A name that's taken gets core's `<2>`; opening the session again goes to its chat.
    let root = TempRoot::new("names");
    let fake = FakeRunner::new();
    let mut h = harness(Orchid::for_tests(root.path(), fake.clone()));
    let other = h.create_text(NAME, "someone else's");
    Meta::new(ID).write(root.path());
    chat::open(&mut h, ID).unwrap();
    let b = h.focused_buffer().unwrap();
    assert_ne!(b, other);
    let name = h.buffer_name(b).unwrap();
    assert!(name.starts_with(NAME) && name != NAME, "{name}");
    let before = h.buffer_list().len();
    h.show(other, die_core::ShowIn::Current).unwrap();
    chat::open(&mut h, ID).unwrap();
    assert_eq!(h.focused_buffer(), Some(b));
    assert_eq!(h.buffer_list().len(), before);
    // No such session: Emacs's error, and nothing made.
    let err = chat::open(&mut h, "nope").unwrap_err().to_string();
    assert!(err.starts_with("Failed to open session nope"), "{err}");
    assert_eq!(h.buffer_list().len(), before);
    assert!(
        fake.calls().is_empty(),
        "opening reads files, it runs nothing"
    );
}

// ------------------------------------------------------------------ sections

#[test]
fn tab_on_a_stub_expands_and_collapses_it() {
    let root = TempRoot::new("tab");
    let fake = FakeRunner::new();
    let events = [fixtures::message("e1", "user", "line one\nline two")];
    let (mut h, b, w) = opened(&root, &fake, &events);
    let user = stub("[user]", "line one line two");
    on(&mut h, b, w, "[user]", 0);
    assert!(keys(&mut h, "<Tab>").is_empty());
    let shown = rows(&mut h);
    let at = shown.iter().position(|r| *r == user).unwrap();
    assert_eq!(shown[at + 1..at + 3], ["line one", "line two"]);
    // Again: collapsed.
    assert!(keys(&mut h, "<Tab>").is_empty());
    assert_eq!(rows(&mut h)[at + 1], "");
    // From inside the detail, it collapses too, and the cursor goes to the stub.
    keys(&mut h, "<Tab>");
    let stub_at = on(&mut h, b, w, "[user]", 0);
    // The stub's preview has it too: the detail's is the second.
    on(&mut h, b, w, "line two", 1);
    assert!(keys(&mut h, "<Tab>").is_empty());
    assert_eq!(cursor(&h, b, w), stub_at);
    assert_eq!(rows(&mut h)[at + 1], "");
}

#[test]
fn toggle_at_point_toggles_the_section_there_and_does_nothing_elsewhere() {
    let root = TempRoot::new("toggle-at-point");
    let fake = FakeRunner::new();
    let events = [fixtures::message("e1", "user", "the detail")];
    let (mut h, b, w) = opened(&root, &fake, &events);
    let toggle = |h: &mut Harness| {
        h.run("orchid-collapsible-toggle-at-point", &Args::default())
            .unwrap();
    };
    on(&mut h, b, w, "[user]", 0);
    toggle(&mut h);
    assert!(rows(&mut h).contains(&"the detail".to_string()));
    toggle(&mut h);
    assert!(!rows(&mut h).contains(&"the detail".to_string()));
    // In the input, nothing: no tab, no word on what TAB is for.
    let (before, said) = (text(&h, b), h.messages.len());
    let end = before.len();
    h.text_mut(b).unwrap().set_cursor(w, end);
    toggle(&mut h);
    assert_eq!(text(&h, b), before);
    assert_eq!(h.messages.len(), said);
}

#[test]
fn a_stub_with_tabs_control_characters_or_emoji_takes_one_row() {
    let root = TempRoot::new("widths");
    let fake = FakeRunner::new();
    let events = [
        fixtures::message("e1", "assistant", "col1\tcol2\tcol3 and more text"),
        fixtures::message(
            "e2",
            "assistant",
            "⚠️ Two tests fail on main; ✔️ the rest pass",
        ),
        fixtures::message("e3", "assistant", "crlf reply\r\nsecond line"),
        fixtures::tool_call("e4", "c1", "bash", json!({"cmd": "printf 'a\tb'"})),
    ];
    let (mut h, _, _) = opened(&root, &fake, &events);
    let shown = rows(&mut h);
    let stubs: Vec<&String> = shown
        .iter()
        .filter(|r| r.starts_with("[agnt]") || r.starts_with("[bash]"))
        .collect();
    assert_eq!(stubs.len(), 4, "{shown:#?}");
    for r in stubs {
        assert!(r.ends_with("[ 63k] [11:16:46]"), "{r:?}");
    }
    assert!(
        !shown
            .iter()
            .any(|r| r.trim_start().starts_with("[11:16:46]")),
        "no time wrapped onto a row of its own: {shown:#?}"
    );
}

#[test]
fn a_chat_takes_the_directory_it_was_opened_from() {
    let root = TempRoot::new("chat-dir");
    let fake = FakeRunner::new();
    let (from, work) = (root.path().join("from"), root.path().join("work"));
    for d in [&from, &work] {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut h = harness(Orchid::for_tests(root.path(), fake.clone()));
    let notes = h.focused_buffer().unwrap();
    h.set_directory(notes, &from);
    let mut m = Meta::new(ID);
    m.working_dir = Some(work.display().to_string());
    m.write(root.path());
    fixtures::append(root.path(), ID, &[fixtures::message("e1", "user", "x")]);
    chat::open(&mut h, ID).unwrap();
    h.run_until_idle();
    let b = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(b), Some("*orchid-chat-default-work-89abc*"));
    assert_eq!(
        h.directory(b),
        Some(from.as_path()),
        "as get-buffer-create gave it the current buffer's, not the session's"
    );
}

#[test]
fn a_lazy_detail_is_made_on_the_first_expand_only() {
    let root = TempRoot::new("lazy");
    let fake = FakeRunner::new();
    let events = [fixtures::message("e1", "assistant", "the reply's text")];
    let (mut h, b, w) = opened(&root, &fake, &events);
    assert!(!text(&h, b).contains("Assistant: "), "not made yet");
    let before = sections::stats();
    on(&mut h, b, w, "[agnt]", 0);
    keys(&mut h, "<Tab>");
    let t = text(&h, b);
    assert!(t.contains("]\nAssistant: the reply's text\n"), "{t}");
    let at = t.find("Assistant: ").unwrap();
    assert_eq!(faces_at(&h, b, at), [face::ASSISTANT]);
    assert!(
        rows(&mut h)
            .iter()
            .any(|r| r == "Assistant: the reply's text")
    );
    keys(&mut h, "<Tab><Tab>");
    assert_eq!(text(&h, b), t, "made once");
    let after = sections::stats();
    assert_eq!(after.materialized, before.materialized + 1);
    assert!(after.created >= 1);
}

#[test]
fn a_results_files_are_sections_inside_it() {
    let root = TempRoot::new("nested");
    let fake = FakeRunner::new();
    let content = json!({"./a.rs": "fn a() {}", "./b.rs": {"error": "Is a directory"}});
    let events = [fixtures::tool_result("e1", "c9", content)];
    let (mut h, b, w) = opened(&root, &fake, &events);
    on(&mut h, b, w, "[rslt]", 0);
    keys(&mut h, "<Tab>");
    let shown = rows(&mut h);
    let at = shown.iter().position(|r| r.starts_with("[rslt]")).unwrap();
    assert_eq!(
        shown[at + 1..at + 5],
        ["  Call ID: c9", "  Results:", "[./a.rs]", "[./b.rs]"]
    );
    on(&mut h, b, w, "[./a.rs]", 0);
    keys(&mut h, "<Tab>");
    let shown = rows(&mut h);
    assert_eq!(
        shown[at + 3..at + 7],
        ["[./a.rs]", "fn a() {}", "", "[./b.rs]"]
    );
    on(&mut h, b, w, "[./b.rs]", 0);
    keys(&mut h, "<Tab>");
    assert!(rows(&mut h).contains(&"[error] Is a directory".to_string()));
    // Collapsing the result hides its files, open or not.
    on(&mut h, b, w, "[rslt]", 0);
    keys(&mut h, "<Tab>");
    let shown = rows(&mut h);
    assert!(!shown.iter().any(|r| r.starts_with("[./")), "{shown:?}");
}

#[test]
fn tab_in_insert_state_toggles_too_and_in_the_input_its_a_tab() {
    let root = TempRoot::new("tab-insert");
    let fake = FakeRunner::new();
    let events = [fixtures::message("e1", "user", "detail")];
    let (mut h, b, w) = opened(&root, &fake, &events);
    // At the end of the stub's line, in insert state.
    on(&mut h, b, w, "[11:16:46]", 0);
    keys(&mut h, "A");
    assert_eq!(h.state_of(b), State::Insert);
    assert!(keys(&mut h, "<Tab>").is_empty());
    assert!(rows(&mut h).contains(&"detail".to_string()));
    keys(&mut h, "<Esc>G");
    let t = text(&h, b);
    h.text_mut(b).unwrap().set_cursor(w, t.len());
    keys(&mut h, "ia<Tab>b<Esc>");
    assert!(text(&h, b).ends_with("\n\na\tb"), "{:?}", text(&h, b));
    // In normal state too, as Emacs's TAB inserted there.
    keys(&mut h, "<Tab>");
    assert!(text(&h, b).ends_with("\n\na\t\tb"), "{:?}", text(&h, b));
    // Anywhere else, a word on what TAB is for.
    on(&mut h, b, w, "[session restored]", 0);
    assert!(keys(&mut h, "<Tab>").is_empty());
    assert_eq!(h.messages.last().unwrap(), chat::TAB_ELSEWHERE);
}

#[test]
fn search_opens_the_section_it_lands_in_and_motions_skip_hidden_text() {
    let root = TempRoot::new("search");
    let fake = FakeRunner::new();
    // Past what the stub's preview shows.
    let long = format!("{}\nthe secret word", "x".repeat(60));
    let events = [
        fixtures::message("e1", "user", &long),
        fixtures::message("e2", "assistant", "reply"),
    ];
    let (mut h, b, w) = opened(&root, &fake, &events);
    // j from the stub goes past its hidden detail to the next stub.
    let user = on(&mut h, b, w, "[user]", 0);
    keys(&mut h, "j");
    let agnt = text(&h, b).find("[agnt]").unwrap();
    assert_eq!(cursor(&h, b, w), agnt);
    keys(&mut h, "k");
    assert_eq!(cursor(&h, b, w), user);
    // A search that lands inside expands it.
    assert!(keys(&mut h, "gg/secret<CR>").is_empty());
    let at = text(&h, b).find("secret").unwrap();
    assert_eq!(cursor(&h, b, w), at);
    assert!(rows(&mut h).contains(&"the secret word".to_string()));
}

// ------------------------------------------------------------------ read-only

#[test]
fn the_read_only_parts_refuse_your_edits_and_take_the_chats() {
    let root = TempRoot::new("read-only");
    let fake = FakeRunner::new();
    let (mut h, b, w) = opened(&root, &fake, &session_events());
    let before = text(&h, b);
    let sep = chat::separator(Some("die"));
    on(&mut h, b, w, &sep, 1);
    let errors = keys(&mut h, "dd");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("read-only"), "{errors:?}");
    on(&mut h, b, w, "[session restored]", 0);
    assert_eq!(keys(&mut h, "x").len(), 1);
    assert_eq!(text(&h, b), before);
    // The rest of the transcript is yours to edit, as in Emacs.
    on(&mut h, b, w, "Workspace", 0);
    assert!(keys(&mut h, "x").is_empty());
    assert!(text(&h, b).starts_with("orkspace"));
    // And the chat's own edits always go in.
    fixtures::append(
        root.path(),
        ID,
        &[fixtures::message("e7", "assistant", "late")],
    );
    assert!(h.advance(die_orchid_client::poll::SLOW).is_empty());
    assert!(text(&h, b).contains(&stub("[agnt]", "late")));
}

// ------------------------------------------------------------------ the slash menu

#[test]
fn slash_alone_on_an_input_line_opens_the_menu() {
    let root = TempRoot::new("slash");
    let fake = FakeRunner::new();
    let (mut h, b, _) = opened(&root, &fake, &[fixtures::message("e1", "user", "x")]);
    let before = text(&h, b);
    assert!(keys(&mut h, "i/").is_empty());
    assert_eq!(h.prompt_text(), Some(""));
    assert!(
        h.screen().contains("1/6    /"),
        "the menu, as vertico showed it"
    );
    assert_eq!(h.prompt_candidates(), SLASH);
    assert_eq!(
        SLASH,
        [
            "config",
            "exit  (quit)",
            "kill",
            "logs",
            "metadata",
            "new  (clear)"
        ]
    );
    // It isn't in the text, and a cancel does nothing.
    assert_eq!(text(&h, b), before);
    keys(&mut h, "<C-g>");
    assert_eq!(h.prompt_text(), None);
    assert_eq!(text(&h, b), before);
    assert_eq!(h.state_of(b), State::Insert, "back where it was");
    // Anywhere else on the line it's a slash.
    keys(&mut h, "a/b");
    assert_eq!(h.prompt_text(), None);
    assert!(text(&h, b).ends_with("\n\na/b"));
    // On the input's blank line too, at its start.
    keys(&mut h, "<Esc>k0i/");
    assert_eq!(h.prompt_text(), Some(""));
    keys(&mut h, "<Esc>");
}

#[test]
fn the_slash_commands_do_what_emacs_did() {
    let root = TempRoot::new("slash-do");
    let fake = FakeRunner::new();
    let work = root.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut m = Meta::new(ID);
    m.working_dir = Some(work.display().to_string());
    m.agent_name = Some("developer".into());
    let orchid = Orchid::for_tests(root.path(), fake.clone());
    let (mut h, b, w) = opened_with(orchid, &root, m, &[]);
    std::fs::write(die_orchid_client::store::logs_path(root.path(), ID), "{}\n").unwrap();
    // /metadata and /logs visit the session's files, here.
    keys(&mut h, "i/metad<CR>");
    let file = h.window_buffer(w).unwrap();
    let path = h.file(file).unwrap().path.clone();
    assert_eq!(
        path,
        die_orchid_client::store::metadata_path(root.path(), ID)
    );
    h.show(b, die_core::ShowIn::Current).unwrap();
    keys(&mut h, "<Esc>i/logs<CR>");
    let file = h.window_buffer(w).unwrap();
    assert_eq!(
        h.file(file).unwrap().path,
        die_orchid_client::store::logs_path(root.path(), ID)
    );
    h.show(b, die_core::ShowIn::Current).unwrap();
    // /config asks for a file from the root.
    let dir = h.directory(b).map(std::path::Path::to_path_buf);
    keys(&mut h, "<Esc>i/config<CR>");
    let line = h.prompt_text().unwrap().to_string();
    let root_line = format!("{}/", root.path().display());
    assert!(
        root_line.ends_with(line.trim_start_matches('~')),
        "{line} for {root_line}"
    );
    assert_eq!(
        h.directory(b).map(std::path::Path::to_path_buf),
        dir,
        "its own is put back"
    );
    keys(&mut h, "<C-g>");
    // /kill stops the session.
    fake.reply("stop", Reply::ok(r#"{"id":"x","status":"cancelled"}"#));
    keys(&mut h, "<Esc>i/kill<CR>");
    h.run_until_idle();
    assert_eq!(h.messages.last().unwrap(), "[orchid] kill: stopped");
    let last = fake.argvs().pop().unwrap();
    assert_eq!(last[2..], ["stop".to_string(), ID.to_string()]);
    fake.reply("stop", Reply::error("no worker"));
    keys(&mut h, "<Esc>i/kill<CR>");
    h.run_until_idle();
    assert_eq!(h.messages.last().unwrap(), "[orchid] kill: no worker");
    // /new and /clear: a session with the same agent, in the same directory.
    keys(&mut h, "<Esc>i/clear<CR>");
    h.run_until_idle();
    let create = fake.argvs().pop().unwrap();
    let dir = work.display().to_string();
    assert_eq!(
        create[2..],
        ["create", "--agent", "developer", "--working-dir", &dir]
    );
    let pending = h.focused_buffer().unwrap();
    assert!(h.is_live(b), "the old chat stays");
    assert_ne!(pending, b);
    h.take_errors();
    // /quit and /exit kill the chat.
    h.show(b, die_core::ShowIn::Current).unwrap();
    keys(&mut h, "<Esc>i/quit<CR>");
    assert!(!h.is_live(b));
}

// ------------------------------------------------------------------ close and back

#[test]
fn c_c_c_q_closes_and_c_c_c_l_or_s_tab_go_to_the_browser() {
    let root = TempRoot::new("keys");
    let fake = FakeRunner::new();
    let (mut h, b, _) = opened(&root, &fake, &[]);
    for (state, keys_in) in [(State::Normal, ""), (State::Insert, "i")] {
        keys(&mut h, keys_in);
        assert_eq!(h.state_of(b), state);
        for k in ["<C-c><C-q>", "<C-c><C-l>", "<S-Tab>", "<Tab>", "<S-CR>"] {
            let d = h.describe_key(&parse_keys(k).unwrap()).unwrap();
            assert!(
                format!("{d:?}").contains("mode orchid-chat"),
                "{k} in {state:?}: {d:?}"
            );
        }
        keys(&mut h, "<Esc>");
    }
    // RET is die-vim's in normal state, as Evil's motion map had it; the chat's in insert.
    let d = h.describe_key(&parse_keys("<CR>").unwrap()).unwrap();
    assert!(!format!("{d:?}").contains("orchid-chat"), "{d:?}");
    // The browser, in this window, and the chat stays.
    let w = h.focused().unwrap();
    for k in ["<C-c><C-l>", "<S-Tab>"] {
        assert!(keys(&mut h, k).is_empty(), "{k}");
        let shown = h.window_buffer(w).and_then(|s| h.buffer_name(s));
        assert_eq!(shown, Some(die_orchid_client::browser::BUFFER), "{k}");
        assert_eq!(h.window_list().len(), 1, "never a split");
        assert!(h.is_live(b));
        h.show(b, die_core::ShowIn::Current).unwrap();
    }
    let browser = h.find_buffer(die_orchid_client::browser::BUFFER).unwrap();
    assert!(h.kill(browser));
    assert!(keys(&mut h, "<C-c><C-q>").is_empty());
    assert!(!h.is_live(b));
    assert!(fake.calls().is_empty(), "the session goes on");
    h.run_until_idle();
    let tracked = h
        .with_ext(|o: &mut Orchid, ctx| o.chat_buffer(ctx, ID))
        .unwrap();
    assert_eq!(tracked, None);
    assert_eq!(
        h.with_ext(|o: &mut Orchid, _| o.poll.period()).unwrap(),
        None
    );
}

// ------------------------------------------------------------------ older history, and stats

#[test]
fn older_history_loads_above_in_order() {
    let root = TempRoot::new("history");
    let fake = FakeRunner::new();
    let mut orchid = Orchid::for_tests(root.path(), fake.clone());
    orchid.options.restore_max_events = 2;
    let events: Vec<String> = (1..=3)
        .flat_map(|i| {
            [
                fixtures::message(&format!("u{i}"), "user", &format!("question {i}")),
                fixtures::message(&format!("a{i}"), "assistant", &format!("answer {i}")),
            ]
        })
        .collect();
    let (mut h, b, _) = opened_with(orchid, &root, Meta::new(ID), &events);
    // The stubs' previews, in order.
    let order = |h: &Harness| -> Vec<String> {
        let t = text(h, b);
        let stubs = t
            .lines()
            .filter(|l| l.starts_with("[user]") || l.starts_with("[agnt]"));
        stubs.map(|l| l[11..58].trim().to_string()).collect()
    };
    assert_eq!(order(&h), ["question 3", "answer 3"]);
    let more = |h: &mut Harness| {
        let args = Args {
            buffer: Some(b),
            ..Args::default()
        };
        h.run("orchid-chat-load-more-history", &args).unwrap();
    };
    more(&mut h);
    assert_eq!(h.messages[h.messages.len() - 2], "Loading 2 more events...");
    assert_eq!(h.messages.last().unwrap(), "Loaded 2 more events");
    assert_eq!(
        order(&h),
        ["question 2", "answer 2", "question 3", "answer 3"]
    );
    more(&mut h);
    assert_eq!(
        order(&h),
        [
            "question 1",
            "answer 1",
            "question 2",
            "answer 2",
            "question 3",
            "answer 3"
        ]
    );
    // Each turn under a separator, as a restore lays them out: a blank line before the rule and
    // two after.
    let rule = format!("\n\n{}\n\n\n[user]", chat::rule());
    assert_eq!(text(&h, b).matches(&rule).count(), 2, "{}", text(&h, b));
    more(&mut h);
    assert_eq!(h.messages.last().unwrap(), "No more history to load");
}

#[test]
fn the_stats_go_in_a_buffer_beside() {
    let root = TempRoot::new("stats");
    let fake = FakeRunner::new();
    let (mut h, b, _) = opened(&root, &fake, &[]);
    h.run("orchid-collapsible-report-stats", &Args::default())
        .unwrap();
    let stats = h.find_buffer(chat::STATS_BUFFER).unwrap();
    let t = text(&h, stats);
    assert!(t.starts_with("Lazy Formatting Stats:\n  Created: "), "{t}");
    assert_eq!(h.window_list().len(), 2);
    assert_eq!(h.focused_buffer(), Some(b), "the focus stays");
}
