//! The session browser in a harness with die-vim, on hand-written sessions under a temporary
//! root, with orchid faked: seek's layout at 78 columns and in narrower windows, every key,
//! searching, the marks and their `yes`, the live redraws, opening and starting chats, and a
//! restart.

use die_test_cap as _;

use die_core::session::json;
use die_core::{Args, BufferId, Harness, Origin, Wrap};
use die_orchid_client::browser::{self, BUFFER};
use die_orchid_client::cli::{Call, FakeRunner, Reply};
use die_orchid_client::fixtures::{Meta, TempRoot, append, message};
use die_orchid_client::poll::SLOW;
use die_orchid_client::store::{self, Status};
use die_orchid_client::{Kind, Orchid, face, log, seek};
use die_vim::Vim;
use jiff::{SignedDuration, Timestamp};

fn harness(root: &TempRoot, fake: &FakeRunner) -> (Harness, BufferId) {
    let orchid = Orchid::for_tests(root.path(), fake.clone());
    let mut h = Harness::new(vec![Box::new(Vim::default()), Box::new(orchid)]).unwrap();
    let (notes, _) = h.open_text("notes", "").unwrap();
    (h, notes)
}

/// `minutes` ago, as orchid writes a time.
fn ago(minutes: i64) -> String {
    (Timestamp::now() - SignedDuration::from_secs(minutes * 60)).to_string()
}

/// A session labelled `label` (none for None), with `status`, changed `minutes` ago, and a line
/// of history, so a chat can open on it.
fn session(root: &TempRoot, id: &str, label: Option<&str>, status: &str, minutes: i64) -> Meta {
    let mut m = Meta::new(id);
    m.label = label.map(String::from);
    m.status = status.into();
    m.updated_at = ago(minutes);
    m.write(root.path());
    append(root.path(), id, &[message("e1", "user", "hello")]);
    m
}

/// `orchid`, as `\o` runs it, and everything it started.
fn orchid(h: &mut Harness) -> BufferId {
    h.run("orchid", &Args::default()).unwrap();
    h.run_until_idle();
    h.find_buffer(BUFFER).expect("the browser")
}

fn keys(h: &mut Harness, keys: &str) {
    for k in h.type_keys(keys).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
}

fn lines(h: &mut Harness) -> Vec<String> {
    h.screen().lines().map(String::from).collect()
}

/// The text of the row drawn selected, without its padding.
fn selected(h: &mut Harness, buf: BufferId) -> String {
    let faces = browser::faces(h, buf).unwrap();
    let row = faces.iter().find(|(_, f)| *f == seek::face::SELECTED);
    row.map_or(String::new(), |(t, _)| t.trim_end().to_string())
}

fn last(h: &Harness) -> &str {
    h.messages.last().map_or("", String::as_str)
}

fn rule(n: usize) -> String {
    "─".repeat(n)
}

/// The row a session would have, as the screen shows it, without trailing spaces.
fn row(label: &str, status: &str, updated: &str) -> String {
    format!("{label:<30}  {status:<10} {updated}")
}

#[test]
fn the_browser_draws_seeks_layout_at_78_columns_in_this_window() {
    let root = TempRoot::new("browser-layout");
    let fake = FakeRunner::new();
    let (mut h, notes) = harness(&root, &fake);
    session(&root, "s-alpha", Some("alpha-chat"), "idle", 5);
    session(&root, "s-beta", Some("beta"), "running", 0);
    let mut gamma = session(&root, "s-gamma", None, "idle", 180);
    gamma.agent_name = Some("developer".into());
    gamma.write(root.path());
    let long = "a very long label that goes past thirty columns";
    session(&root, "s-old", Some(long), "failed", 2 * 24 * 60);
    let chat = h.create_text("a chat", "");
    let track = |o: &mut Orchid, _: &mut die_core::Ctx| {
        o.track(chat, Kind::Chat, json!({"id": "s-alpha"}));
    };
    h.with_ext(track).unwrap();
    let buf = orchid(&mut h);
    assert_eq!(h.window_list().len(), 1, "never a split");
    assert_eq!(h.focused_buffer(), Some(buf));
    let title = format!("{} Orchid Sessions {}", rule(30), rule(31));
    let header = row("LABEL", "STATUS", "UPDATED");
    let expected = [
        title.as_str(),
        &rule(78),
        "",
        &rule(78),
        &header,
        &rule(78),
        &row("beta", "IDLE•RUN", "just now"),
        &row("alpha-chat", "ACTIVE", "5 minutes ago"),
        &row("developer-die", "IDLE", "3 hours ago"),
        &row("a very long label that goes ..", "IDLE", "2 days ago"),
    ];
    let screen = lines(&mut h);
    assert_eq!(screen[..expected.len()], expected, "{}", screen.join("\n"));
    assert!(screen[expected.len()..24].iter().all(String::is_empty));
    let faces = browser::faces(&mut h, buf).unwrap();
    let faces: Vec<(&str, &str)> = faces.iter().map(|(t, f)| (t.trim_end(), *f)).collect();
    assert_eq!(
        faces,
        [
            (title.as_str(), "bold"),
            (&rule(78), "shadow"),
            (&rule(78), "shadow"),
            (&header, "bold"),
            (&rule(78), "shadow"),
            (&row("beta", "IDLE•RUN", "just now"), "highlight"),
            ("ACTIVE", face::BROWSER_ACTIVE),
            ("5 minutes ago", "shadow"),
            ("IDLE", face::BROWSER_IDLE),
            ("3 hours ago", "shadow"),
            ("IDLE", face::BROWSER_IDLE),
            ("2 days ago", "shadow"),
        ]
    );
    // The selected row is padded to the width, and wholly highlight.
    let padded = browser::faces(&mut h, buf).unwrap()[5].0.clone();
    assert_eq!(padded.chars().count(), 78);
    let t = h.text(buf).unwrap();
    let s = t.settings();
    assert!(s.read_only && !s.cursor_visible && s.wrap == Wrap::Off);
    assert!(!t.modified());
    assert!(
        fake.calls().is_empty(),
        "the browser reads files, not the CLI"
    );
    // A user edit is refused; the browser's own go in.
    let edit = h
        .text_mut(buf)
        .unwrap()
        .tx(Origin::User)
        .insert(0, "x")
        .commit();
    assert!(edit.is_err());
    // `q` kills it, and the window goes on to another buffer, as a kill does.
    keys(&mut h, "q");
    assert_eq!(h.find_buffer(BUFFER), None);
    assert!(h.focused_buffer().is_some_and(|b| b == notes || b == chat));
    assert_eq!(h.window_list().len(), 1);
}

#[test]
fn with_no_sessions_it_says_no_results() {
    let root = TempRoot::new("browser-empty");
    let (mut h, _) = harness(&root, &FakeRunner::new());
    orchid(&mut h);
    let screen = lines(&mut h);
    assert_eq!(screen[6], "No results");
    assert_eq!(screen[7], "");
}

#[test]
fn j_k_p_and_the_pages_move_the_selection_and_scroll() {
    let root = TempRoot::new("browser-keys");
    let (mut h, _) = harness(&root, &FakeRunner::new());
    for i in 0..25 {
        let id = format!("s{i:02}");
        session(&root, &id, Some(&format!("session-{i:02}")), "idle", i);
    }
    let buf = orchid(&mut h);
    let at = |h: &mut Harness| {
        selected(h, buf)
            .split_whitespace()
            .next()
            .unwrap()
            .to_string()
    };
    assert_eq!(at(&mut h), "session-00");
    keys(&mut h, "jj");
    assert_eq!(at(&mut h), "session-02");
    keys(&mut h, "k");
    assert_eq!(at(&mut h), "session-01");
    // `p` is up, as in the Emacs keymap; Evil's list left it out and it pasted (B6).
    keys(&mut h, "p");
    assert_eq!(at(&mut h), "session-00");
    keys(&mut h, "k");
    assert_eq!(at(&mut h), "session-00", "no further than the first");
    // A page is the rows a 24-row window shows under seek's 6 lines of chrome: 18.
    keys(&mut h, "jJ");
    assert_eq!(at(&mut h), "session-19");
    let screen = lines(&mut h);
    assert!(screen[6].starts_with("session-02"), "{}", screen.join("\n"));
    assert!(screen[23].starts_with("session-19"));
    keys(&mut h, "J");
    assert_eq!(at(&mut h), "session-24", "no further than the last");
    keys(&mut h, "K");
    assert_eq!(at(&mut h), "session-06");
    assert!(
        lines(&mut h)[6].starts_with("session-06"),
        "scrolled up to it"
    );
    keys(&mut h, "K");
    assert_eq!(at(&mut h), "session-00");
}

#[test]
fn r_and_g_read_the_sessions_again_and_q_quits() {
    let root = TempRoot::new("browser-refresh");
    let (mut h, notes) = harness(&root, &FakeRunner::new());
    session(&root, "s1", Some("first"), "idle", 10);
    let buf = orchid(&mut h);
    session(&root, "s2", Some("second"), "idle", 0);
    keys(&mut h, "r");
    assert_eq!(lines(&mut h)[6], row("second", "IDLE", "just now"));
    std::fs::remove_dir_all(store::session_dir(root.path(), "s2")).unwrap();
    keys(&mut h, "g");
    assert_eq!(lines(&mut h)[6], row("first", "IDLE", "10 minutes ago"));
    assert_eq!(lines(&mut h)[7], "");
    assert_eq!(
        selected(&mut h, buf),
        row("first", "IDLE", "10 minutes ago")
    );
    // `q` kills it, and the window goes back to what it showed.
    keys(&mut h, "q");
    assert_eq!(h.find_buffer(BUFFER), None);
    assert_eq!(h.focused_buffer(), Some(notes));
    assert_eq!(h.next_timer(), None, "no orchid buffer, no timer");
}

/// orchid's `delete`, as the fake does it: the session's directory goes, as orchid archives it.
fn deletes(root: &TempRoot) -> impl Fn(&Call) -> Reply + Send + Sync + 'static {
    let root = root.path().to_path_buf();
    move |call: &Call| {
        let id = call.strings().last().unwrap().clone();
        std::fs::remove_dir_all(store::session_dir(&root, &id)).unwrap();
        Reply::ok(&json!({"id": id, "archived": true}).to_string())
    }
}

#[test]
fn marks_run_only_once_yes_is_typed_out_and_failed_ones_stay() {
    let root = TempRoot::new("browser-marks");
    let fake = FakeRunner::new();
    let (mut h, _) = harness(&root, &fake);
    session(&root, "s-a", Some("a"), "idle", 1);
    session(&root, "s-b", Some("b"), "running", 2);
    session(&root, "s-c", Some("c"), "idle", 3);
    let buf = orchid(&mut h);
    // D marks and moves down; S marks a running session and moves down.
    keys(&mut h, "DS");
    let screen = lines(&mut h);
    assert_eq!(screen[6], row("a", "DEL", "1 minute ago"));
    assert_eq!(screen[7], row("b", "KILL", "2 minutes ago"));
    assert!(selected(&mut h, buf).starts_with('c'));
    let faces = browser::faces(&mut h, buf).unwrap();
    let marked: Vec<(&str, &str)> = faces[5..11].iter().map(|(t, f)| (t.as_str(), *f)).collect();
    assert_eq!(
        marked,
        [
            ("a", "error"),
            ("DEL", "error"),
            ("1 minute ago", "error"),
            ("b", "warning"),
            ("KILL", "warning"),
            ("2 minutes ago", "warning"),
        ]
    );
    // Not on an idle session.
    keys(&mut h, "S");
    assert_eq!(last(&h), "Session has no running process to kill");
    assert!(selected(&mut h, buf).starts_with('c'), "and it stays");
    // u takes a mark off, staying; S puts it back.
    keys(&mut h, "ku");
    assert_eq!(lines(&mut h)[7], row("b", "IDLE•RUN", "2 minutes ago"));
    assert!(selected(&mut h, buf).starts_with('b'));
    keys(&mut h, "S");
    // A bare RET is no answer, and a cancel isn't yes.
    keys(&mut h, "x");
    let screen = h.screen();
    assert!(
        screen.contains("Execute 2 marked operation(s)? (yes or no)"),
        "{screen}"
    );
    keys(&mut h, "<CR>");
    assert_eq!(last(&h), "Please answer yes or no.");
    assert_eq!(h.prompt_text(), Some(""));
    keys(&mut h, "<Esc>");
    assert_eq!(h.prompt_text(), None);
    keys(&mut h, "x");
    keys(&mut h, "no<CR>");
    assert!(fake.calls().is_empty(), "nothing ran");
    assert_eq!(lines(&mut h)[6], row("a", "DEL", "1 minute ago"));
    // yes: each in turn, in the order they were marked.
    fake.on("delete", deletes(&root));
    fake.reply("stop", Reply::error("no worker is running"));
    keys(&mut h, "x");
    keys(&mut h, "yes<CR>");
    h.run_until_idle();
    let root_arg = root.path().display().to_string();
    let argv = |cmd: &str, id: &str| {
        vec![
            "--config".to_string(),
            root_arg.clone(),
            cmd.into(),
            id.into(),
        ]
    };
    assert_eq!(fake.argvs(), [argv("delete", "s-a"), argv("stop", "s-b")]);
    assert_eq!(last(&h), "Deleted 1 session, 1 failed");
    let screen = lines(&mut h);
    assert_eq!(
        screen[6],
        row("b", "KILL", "2 minutes ago"),
        "the failed mark stays"
    );
    assert_eq!(screen[7], row("c", "IDLE", "3 minutes ago"));
    assert_eq!(screen[8], "", "the deleted session's gone");
    let log = h.find_buffer(log::BUFFER).unwrap();
    let logged = h.text(log).unwrap().rope().to_string();
    assert!(
        logged.contains("stop s-b failed: no worker is running"),
        "{logged}"
    );
    // Stopped this time; and with no marks, x asks nothing.
    fake.reply("stop", Reply::ok(r#"{"id":"s-b","status":"cancelled"}"#));
    keys(&mut h, "x");
    keys(&mut h, "yes<CR>");
    h.run_until_idle();
    assert_eq!(last(&h), "Killed 1 process");
    assert_eq!(lines(&mut h)[6], row("b", "IDLE•RUN", "2 minutes ago"));
    keys(&mut h, "x");
    assert_eq!(h.prompt_text(), None);
}

#[test]
fn search_filters_chars_in_order_and_keeps_or_clears_its_query() {
    let root = TempRoot::new("browser-search");
    let (mut h, _) = harness(&root, &FakeRunner::new());
    session(&root, "s1", Some("default-die"), "idle", 1);
    session(&root, "s2", Some("developer-api"), "running", 2);
    session(&root, "s3", Some("notes"), "idle", 3);
    let buf = orchid(&mut h);
    keys(&mut h, "/");
    let faces = browser::faces(&mut h, buf).unwrap();
    assert_eq!(faces[2], (" ".repeat(78), seek::face::SEARCH));
    // Every printable key is the query's, `j` too.
    keys(&mut h, "dvp");
    let screen = lines(&mut h);
    assert_eq!(screen[2], "dvp");
    assert_eq!(screen[6], row("developer-api", "IDLE•RUN", "2 minutes ago"));
    assert_eq!(screen[7], "");
    keys(&mut h, "j");
    assert_eq!(lines(&mut h)[6], "No results");
    keys(&mut h, "<BS>");
    assert_eq!(lines(&mut h)[2], "dvp");
    // The status counts: `run` finds the running one.
    keys(&mut h, "<BS><BS><BS>run");
    assert_eq!(
        lines(&mut h)[6],
        row("developer-api", "IDLE•RUN", "2 minutes ago")
    );
    // RET stops, keeping the query: j moves again. (notes has a `d` and an `e` in its IDLE, and
    // its time a `-`, but no `d` after it.)
    keys(&mut h, "<BS><BS><BS>-d<CR>");
    let screen = lines(&mut h);
    assert_eq!(screen[2], "-d");
    let two = [
        row("default-die", "IDLE", "1 minute ago"),
        row("developer-api", "IDLE•RUN", "2 minutes ago"),
        String::new(),
    ];
    assert_eq!(screen[6..9], two);
    assert_eq!(
        browser::faces(&mut h, buf).unwrap()[2].1,
        "shadow",
        "no search face"
    );
    keys(&mut h, "j");
    assert!(selected(&mut h, buf).starts_with("developer-api"));
    // a searches too; Esc stops as RET does, and q there was a letter.
    keys(&mut h, "aq<Esc>");
    assert_eq!(lines(&mut h)[2], "-dq");
    assert_eq!(lines(&mut h)[6], "No results");
    assert!(h.find_buffer(BUFFER).is_some());
    // d clears it, and goes back to the first row.
    keys(&mut h, "d");
    let screen = lines(&mut h);
    assert_eq!(screen[2], "");
    assert_eq!(screen[8], row("notes", "IDLE", "3 minutes ago"));
    assert!(selected(&mut h, buf).starts_with("default-die"));
    // C-g clears it too, while searching.
    keys(&mut h, "jj/no<C-g>");
    assert_eq!(lines(&mut h)[2], "");
    assert!(selected(&mut h, buf).starts_with("default-die"));
    keys(&mut h, "j");
    assert!(
        selected(&mut h, buf).starts_with("developer-api"),
        "searching stopped"
    );
    // A space is a letter of the query.
    keys(&mut h, "/<Space>");
    assert_eq!(lines(&mut h)[6], "No results");
}

#[test]
fn a_status_that_changes_redraws_it_while_it_lives() {
    let root = TempRoot::new("browser-live");
    let (mut h, notes) = harness(&root, &FakeRunner::new());
    session(&root, "s-a", Some("a"), "idle", 1);
    let mut b = session(&root, "s-b", Some("b"), "idle", 2);
    let buf = orchid(&mut h);
    let period = |h: &mut Harness| h.with_ext(|o: &mut Orchid, _| o.poll.period()).unwrap();
    assert_eq!(period(&mut h), Some(SLOW));
    keys(&mut h, "j");
    assert!(selected(&mut h, buf).starts_with('b'));
    // A turn starts elsewhere, as from Emacs: the next slow tick sees it.
    b.status = "running".into();
    b.updated_at = ago(0);
    b.write(root.path());
    h.advance(SLOW);
    let screen = lines(&mut h);
    assert_eq!(screen[6], row("b", "IDLE•RUN", "just now"));
    assert!(
        selected(&mut h, buf).starts_with('b'),
        "the selection went with its session"
    );
    // A chat hears of it first, as it reads metadata.json, and the browser redraws then.
    let mut info = h
        .with_ext(|o: &mut Orchid, _| o.session("s-b").cloned())
        .unwrap()
        .unwrap();
    info.status = Status::Idle;
    h.with_ext(|o: &mut Orchid, ctx| o.update_session(ctx, info))
        .unwrap();
    h.run_until_idle();
    assert_eq!(lines(&mut h)[6], row("b", "IDLE", "just now"));
    // Not shown, it doesn't look.
    h.show(notes, die_core::ShowIn::Current).unwrap();
    session(&root, "s-c", Some("c"), "idle", 0);
    h.advance(SLOW);
    let known = h
        .with_ext(|o: &mut Orchid, _| o.session("s-c").is_some())
        .unwrap();
    assert!(!known);
    orchid(&mut h);
    assert!(lines(&mut h)[6].starts_with('c'));
    // Killed, it stops.
    keys(&mut h, "q");
    assert_eq!(period(&mut h), None);
    assert_eq!(h.next_timer(), None);
}

#[test]
fn a_narrower_window_draws_it_narrower_and_a_short_one_scrolls() {
    let root = TempRoot::new("browser-narrow");
    let (mut h, _) = harness(&root, &FakeRunner::new());
    for (i, label) in ["one", "two", "three", "four", "five", "six"]
        .iter()
        .enumerate()
    {
        session(&root, &format!("s{i}"), Some(label), "idle", i as i64);
    }
    let buf = orchid(&mut h);
    h.set_window_size(60, 24);
    let screen = lines(&mut h);
    assert_eq!(
        screen[0],
        format!("{} Orchid Sessions {}", rule(21), rule(22))
    );
    assert_eq!(screen[1], rule(60));
    assert_eq!(screen[4], row("LABEL", "STATUS", "UPDATED"));
    let padded = &browser::faces(&mut h, buf).unwrap()[5].0;
    assert_eq!(
        padded.chars().count(),
        60,
        "the selected row fills the width"
    );
    // Narrower than the columns: their line is cut by the window, as truncate-lines cuts it.
    h.set_window_size(40, 24);
    let screen = lines(&mut h);
    assert_eq!(
        screen[0],
        format!("{} Orchid Sessions {}", rule(11), rule(12))
    );
    assert_eq!(screen[4], format!("{:<32}STATUS", "LABEL"));
    assert_eq!(screen[6], format!("{:<32}IDLE", "one"));
    // Ten rows high: four rows under the chrome, scrolled to the selection.
    h.set_window_size(80, 10);
    let screen = lines(&mut h);
    assert_eq!(
        screen[0],
        format!("{} Orchid Sessions {}", rule(30), rule(31))
    );
    assert!(screen[9].starts_with("four"), "{}", screen.join("\n"));
    keys(&mut h, "jjjj");
    let screen = lines(&mut h);
    assert!(
        screen[6].starts_with("two") && screen[9].starts_with("five"),
        "{}",
        screen.join("\n")
    );
    assert!(selected(&mut h, buf).starts_with("five"));
}

#[test]
fn ret_opens_the_chat_here_and_kills_the_browser() {
    let root = TempRoot::new("browser-open");
    let fake = FakeRunner::new();
    let (mut h, _) = harness(&root, &fake);
    session(&root, "s-a", Some("a"), "idle", 1);
    session(&root, "s-b", Some("b"), "idle", 2);
    // A chat already open on b: RET goes to it.
    let chat = h.create_text("b's chat", "");
    let track = |o: &mut Orchid, _: &mut die_core::Ctx| {
        o.track(chat, Kind::Chat, json!({"id": "s-b"}));
    };
    h.with_ext(track).unwrap();
    orchid(&mut h);
    assert_eq!(lines(&mut h)[7], row("b", "ACTIVE", "2 minutes ago"));
    keys(&mut h, "j<CR>");
    assert_eq!(h.find_buffer(BUFFER), None, "killed");
    assert_eq!(h.focused_buffer(), Some(chat));
    assert_eq!(h.window_list().len(), 1);
    // None open on a: RET opens one, in this window, and the browser goes all the same.
    orchid(&mut h);
    keys(&mut h, "<CR>");
    h.run_until_idle();
    assert_eq!(h.find_buffer(BUFFER), None);
    assert!(h.take_errors().is_empty());
    let opened = h
        .with_ext(|o: &mut Orchid, ctx| o.chat_buffer(ctx, "s-a").is_some())
        .unwrap();
    // The chat package's chat, or until it lands, its stub's word for it.
    assert!(
        opened || h.messages.iter().any(|m| m.contains("s-a")),
        "{:?}",
        h.messages
    );
    assert_eq!(h.window_list().len(), 1);
}

#[test]
fn n_asks_for_a_valid_agent_and_starts_in_the_browsers_directory() {
    let root = TempRoot::new("browser-new");
    let fake = FakeRunner::new();
    let (mut h, notes) = harness(&root, &fake);
    let dir = root.path().join("work");
    std::fs::create_dir_all(&dir).unwrap();
    h.set_directory(notes, &dir);
    let agents = json!({"agents": [
        {"name": "default", "prompt": "default", "connection": "echo", "valid": true},
        {"name": "broken", "prompt": "gone", "connection": "nowhere", "valid": false},
    ]});
    fake.reply("agent", Reply::ok(&agents.to_string()));
    fake.on("create", |_: &Call| {
        Reply::error("this test doesn't make sessions")
    });
    let buf = orchid(&mut h);
    assert_eq!(
        h.directory(buf),
        Some(dir.as_path()),
        "the browser's is the buffer's it came from"
    );
    keys(&mut h, "n");
    h.run_until_idle();
    assert_eq!(h.prompt_candidates(), ["default"]);
    assert!(h.screen().contains("Agent:"));
    keys(&mut h, "def<CR>");
    h.run_until_idle();
    // The chat package's create, or until it lands, its stub's word for it.
    let shown = dir.display().to_string();
    let created = fake.argvs().iter().any(|a| {
        a.contains(&"create".to_string())
            && a.contains(&shown)
            && a.contains(&"default".to_string())
    });
    let said = h
        .messages
        .iter()
        .any(|m| m.contains("default") && m.contains(&shown));
    assert!(created || said, "{:?} {:?}", fake.argvs(), h.messages);
    assert_eq!(fake.argvs()[0][2], "agent");
}

#[test]
fn a_restart_brings_it_back_working_with_its_query_selection_and_marks() {
    let root = TempRoot::new("browser-restart");
    let fake = FakeRunner::new();
    let (mut h, _) = harness(&root, &fake);
    session(&root, "s-a", Some("alpha"), "idle", 1);
    session(&root, "s-b", Some("beta"), "idle", 2);
    session(&root, "s-c", Some("gamma"), "idle", 3);
    let buf = orchid(&mut h);
    keys(&mut h, "jD/a");
    let before = lines(&mut h);
    assert_eq!(before[2], "a");
    let session_json = h.session_json();
    let mut again = Harness::new(vec![
        Box::new(Vim::default()),
        Box::new(Orchid::for_tests(root.path(), fake.clone())),
    ])
    .unwrap();
    again.restore_session(&session_json).unwrap();
    again.run_until_idle();
    assert!(again.take_errors().is_empty(), "{:?}", again.messages);
    let buf2 = again.find_buffer(BUFFER).expect("the browser came back");
    assert_eq!(again.focused_buffer(), Some(buf2), "in its window");
    assert_eq!(lines(&mut again)[..9], before[..9]);
    assert_eq!(
        browser::faces(&mut again, buf2).unwrap(),
        browser::faces(&mut h, buf).unwrap()
    );
    // Still searching, and its keys work.
    keys(&mut again, "l");
    assert_eq!(lines(&mut again)[2], "al");
    keys(&mut again, "<CR>k");
    assert!(selected(&mut again, buf2).starts_with("beta"));
    let poll = again.with_ext(|o: &mut Orchid, _| o.poll.period()).unwrap();
    assert_eq!(poll, Some(SLOW), "following again");
    let s = again.text(buf2).unwrap().settings();
    assert!(s.read_only && !s.cursor_visible);
}
