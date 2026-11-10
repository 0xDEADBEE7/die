//! Your orchid client through your config, as you'd use it: `\o`, `RET` on a session, typing and
//! sending, the turn's events coming in, `TAB`, `S-Tab` back to the browser and `q`, over DIE's
//! own extensions with your keys, your Evil, your text defaults (80 columns, centered) and `\z`,
//! across splits and buffer switches. The extension is the one your config registers, made for a
//! test: its root under a temporary directory and orchid faked, as yours would read your
//! `~/.config/orchid`. Nothing runs orchid.

use die_test_cap as _;

use die::core::session::json;
use die::core::{BufferId, Harness, WindowId};
use die_orchid_client::Orchid;
use die_orchid_client::browser::BUFFER;
use die_orchid_client::cli::{Call, FakeRunner, Reply};
use die_orchid_client::fixtures::{self, Meta, TempRoot};
use die_orchid_client::poll::{FAST, SLOW};
use std::time::Duration;

const ID: &str = "0f1e2d3c-aaaa-bbbb-cccc-123456789abc";
const CHAT: &str = "*orchid-chat-default-die-89abc*";

/// Your config's keys, theme and text defaults over DIE's own extensions, as `die::start` puts
/// them together, with your config's orchid made for a test.
fn harness(root: &TempRoot, fake: &FakeRunner) -> Harness {
    let config = die_dev::config(vec![]);
    let names: Vec<&str> = config.extensions.iter().map(|e| e.name()).collect();
    assert_eq!(names, ["orchid", "whiteboard"], "your config's extensions");
    let mut exts = die::built_in();
    exts.push(Box::new(Orchid::for_tests(root.path(), fake.clone())));
    exts.push(Box::new(die_whiteboard::Whiteboard::for_tests()));
    let mut h = Harness::with_config(exts, config.core).unwrap();
    h.open_text("notes", "my notes\n").unwrap();
    h.set_window_size(120, 40);
    h
}

/// orchid's part in a send: your message in `events.jsonl`, and the session running.
fn sends(fake: &FakeRunner, root: &TempRoot) {
    let root = root.path().to_path_buf();
    fake.on("send", move |call: &Call| {
        let message = call.args.last().unwrap().to_string_lossy().into_owned();
        let fields = json!({"type": "message", "role": "user", "content": message});
        let line = fixtures::line("sent", fixtures::AT, fixtures::MARGINAL, fields);
        fixtures::append(&root, ID, &[line]);
        status(&root, "running");
        Reply::ok(&json!({"id": ID, "status": "running", "pid": 4242}).to_string())
    });
}

fn status(root: &std::path::Path, status: &str) {
    let mut m = Meta::new(ID);
    m.status = status.into();
    m.write(root);
}

fn keys(h: &mut Harness, k: &str) {
    for k in h.type_keys(k).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
}

fn text(h: &Harness, b: BufferId) -> String {
    h.text(b).unwrap().rope().to_string()
}

fn rows(h: &mut Harness) -> Vec<String> {
    h.screen().lines().map(String::from).collect()
}

fn period(h: &mut Harness) -> Option<Duration> {
    h.with_ext(|o: &mut Orchid, _| o.poll.period()).unwrap()
}

/// The screen row that holds `needle`, and the column its text starts at.
fn find(h: &mut Harness, needle: &str) -> Option<(String, usize)> {
    let all = rows(h);
    let row = all.into_iter().find(|r| r.contains(needle))?;
    let col = row.chars().take_while(|c| *c == ' ').count();
    Some((row, col))
}

/// How often the editor wakes over `over`, by its timers.
fn wakes(h: &mut Harness, over: Duration) -> usize {
    let end = h.now() + over;
    let mut n = 0;
    while let Some(at) = h.next_timer().filter(|at| *at <= end) {
        h.advance(at.saturating_sub(h.now()).max(Duration::from_millis(1)));
        n += 1;
    }
    h.advance(end.saturating_sub(h.now()));
    n
}

fn focused(h: &Harness) -> (WindowId, BufferId) {
    let w = h.focused().unwrap();
    (w, h.window_buffer(w).unwrap())
}

#[test]
fn leader_o_a_chat_a_turn_and_back_through_your_config() {
    let root = TempRoot::new("die-dev-orchid");
    let fake = FakeRunner::new();
    sends(&fake, &root);
    Meta::new(ID).write(root.path());
    fixtures::append(
        root.path(),
        ID,
        &[
            fixtures::message("e1", "user", "hello"),
            fixtures::message("e2", "assistant", "Hi! How can I help?"),
        ],
    );
    let mut h = harness(&root, &fake);
    assert_eq!(h.next_timer(), None, "nothing of orchid's runs before \\o");

    // \o: the browser in this window, at 78 columns centred in your 80.
    keys(&mut h, "\\o");
    let (w, b) = focused(&h);
    assert_eq!(h.buffer_name(b), Some(BUFFER));
    assert_eq!(h.window_list(), [w], "no split");
    let (title, margin) = find(&mut h, "Orchid Sessions").unwrap();
    assert_eq!(title.trim().chars().count(), 78, "{title}");
    assert_eq!(margin, 20, "centred: (120 - 80) / 2");
    assert!(find(&mut h, "default-die").is_some(), "the session's row");
    assert_eq!(period(&mut h), Some(SLOW));

    // RET: its chat in this window, and the browser gone.
    keys(&mut h, "<CR>");
    let (w2, chat) = focused(&h);
    assert_eq!((w2, h.buffer_name(chat)), (w, Some(CHAT)));
    assert!(h.find_buffer(BUFFER).is_none());
    let (sep, col) = find(&mut h, "──  die  ──").unwrap();
    assert_eq!(
        (col, sep.trim().chars().count()),
        (20, 80),
        "80 columns, centred"
    );
    // \z: the margins off and on again; the chat's lines stay whole either way.
    keys(&mut h, "\\z");
    assert_eq!(find(&mut h, "──  die  ──").unwrap().1, 0);
    keys(&mut h, "\\z");
    assert_eq!(find(&mut h, "──  die  ──").unwrap().1, 20);

    // Type and send, then the turn's events come in while you type the next.
    keys(&mut h, "Gihello there<CR>");
    let argv = fake.argvs().pop().unwrap();
    assert_eq!(argv[2..], ["send", "--id", ID, "--", "hello there"]);
    assert!(find(&mut h, "[Processing: ").is_some());
    assert_eq!(period(&mut h), Some(FAST));
    keys(&mut h, "and then");
    fixtures::append(
        root.path(),
        ID,
        &[
            fixtures::message("e3", "assistant", "Let me look."),
            fixtures::tool_call("e4", "k1", "bash", json!({"cmd": "ls"})),
            fixtures::tool_result("e5", "k1", json!("Cargo.toml\nsrc\n")),
        ],
    );
    h.advance(FAST);
    let t = text(&h, chat);
    let at = |s: &str| t.find(s).unwrap_or_else(|| panic!("{s}: {t}"));
    assert!(at("[user]    [hello there") < at("[agnt]    [Let me look."));
    assert!(at("[rslt]") < at("[Processing: "));
    assert!(t.ends_with("\n\nand then"), "your input stays put: {t:?}");
    assert_eq!(t.matches("[user]    [hello there").count(), 1);

    // TAB on the agent's stub shows what it said.
    keys(&mut h, "<Esc>?Let me look<CR>");
    keys(&mut h, "<Tab>");
    assert!(find(&mut h, "Assistant: Let me look.").is_some());

    // A split, and the chat hidden in both: it goes on following, quickly while it runs.
    keys(&mut h, "<C-w>s");
    assert_eq!(h.window_list().len(), 2);
    keys(&mut h, "\\]notes<CR>");
    keys(&mut h, "<C-w>j\\]notes<CR>");
    let shown: Vec<_> = h
        .window_list()
        .iter()
        .map(|w| h.window_buffer(*w))
        .collect();
    assert!(!shown.contains(&Some(chat)), "hidden");
    fixtures::append(
        root.path(),
        ID,
        &[fixtures::message("e6", "assistant", "Two files.")],
    );
    status(root.path(), "idle");
    h.advance(FAST);
    let t = text(&h, chat);
    assert!(t.contains("[agnt]    [Two files."), "{t}");
    assert!(t.contains("[Finished: "), "{t}");
    assert_eq!(period(&mut h), Some(SLOW), "slow once the turn's over");

    // Back on the chat, S-Tab to the browser, and q.
    keys(&mut h, "\\]*orchid-chat<CR>");
    assert_eq!(focused(&h).1, chat);
    keys(&mut h, "<S-Tab>");
    assert_eq!(h.buffer_name(focused(&h).1), Some(BUFFER));
    keys(&mut h, "q");
    assert!(h.find_buffer(BUFFER).is_none());
    assert_ne!(h.buffer_name(focused(&h).1), Some(BUFFER));

    // Idle with the chat open: only orchid's slow poll; with it closed, nothing.
    assert_eq!(wakes(&mut h, Duration::from_secs(60)), 30);
    let (_, b) = focused(&h);
    if b != chat {
        keys(&mut h, "\\]*orchid-chat<CR>");
    }
    keys(&mut h, "<C-c><C-q>");
    assert!(!h.is_live(chat));
    assert_eq!(period(&mut h), None);
    assert_eq!(h.next_timer(), None, "no orchid buffer, no timer");
    assert_eq!(wakes(&mut h, Duration::from_secs(60)), 0);
    assert!(h.take_errors().is_empty(), "{:?}", h.messages);
}
