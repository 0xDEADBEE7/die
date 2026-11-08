//! The engine's answers to queries, what it asks of DIE, pastes and focus. The spike's known
//! gaps are pinned too, so the bump that closes one says so.

use die_test_cap as _;

use die_core::Rgb;
use die_term::{Event, PasteRefused, REPLIES_MAX, Term, XTVERSION};

fn term() -> Term {
    let mut t = Term::new(100, 30, (18, 36)).unwrap();
    let ansi = std::array::from_fn(|i| Rgb(i as u8 * 10, 0, 0));
    t.set_colors(Rgb(0xd0, 0xd0, 0xd0), Rgb(0x10, 0x20, 0x30), None, &ansi);
    t
}

fn reply(t: &mut Term, query: &str) -> String {
    t.feed(query.as_bytes());
    t.take_replies().escape_ascii().to_string()
}

#[test]
fn queries_are_answered_as_die() {
    let mut t = term();
    let cases = [
        ("\x1b[c", "\\x1b[?62;22c".to_string()),
        ("\x1b[>c", "\\x1b[>1;0;0c".into()),
        ("\x1b[=c", "\\x1bP!|00000000\\x1b\\\\".into()),
        ("\x1b[5n", "\\x1b[0n".into()),
        ("\x1b[3;7H\x1b[6n", "\\x1b[3;7R".into()),
        ("\x1b[>q", format!("\\x1bP>|{XTVERSION}\\x1b\\\\")),
        ("\x1b[14t", format!("\\x1b[4;{};{}t", 30 * 36, 100 * 18)),
        ("\x1b[16t", "\\x1b[6;36;18t".into()),
        ("\x1b[18t", "\\x1b[8;30;100t".into()),
        ("\x1b]10;?\x07", "\\x1b]10;rgb:d0d0/d0d0/d0d0\\x07".into()),
        ("\x1b]11;?\x07", "\\x1b]11;rgb:1010/2020/3030\\x07".into()),
        ("\x1b]4;3;?\x07", "\\x1b]4;3;rgb:1e1e/0000/0000\\x07".into()),
        ("\x1b[?2026$p", "\\x1b[?2026;2$y".into()),
        ("\x1b[?u", "\\x1b[?0u".into()),
        // XTGETTCAP's terminal name is TERM's.
        (
            "\x1bP+q544e\x1b\\",
            "\\x1bP1+r544E=787465726D2D323536636F6C6F72\\x1b\\\\".into(),
        ),
    ];
    for (q, want) in cases {
        assert_eq!(reply(&mut t, q), want, "{q:?}");
    }
    assert!(
        !XTVERSION.contains("ghostty"),
        "yazi would take that for Ghostty"
    );
    assert_eq!(reply(&mut t, "plain text"), "", "nothing else writes back");
}

#[test]
fn in_band_resize_reports() {
    let mut t = term();
    assert_eq!(reply(&mut t, "\x1b[?2048h"), "\\x1b[48;30;100;1080;1800t");
    t.resize(50, 10, (20, 40));
    let r = t.take_replies().escape_ascii().to_string();
    assert_eq!(r, "\\x1b[48;10;50;400;1000t", "in physical pixels");
}

#[test]
fn title_directory_bell_and_notifications() {
    let mut t = term();
    t.feed(b"\x1b]2;my title\x07\x1b]7;file://host.local/Users/me/proj\x1b\\\x07");
    t.feed(b"\x1b]0;other\x1b\\\x1b]9;plain\x07\x1b]777;notify;Build;done ok\x07");
    assert_eq!(
        t.take_events(),
        [
            Event::Title("my title".into()),
            Event::Pwd("file://host.local/Users/me/proj".into()),
            Event::Bell,
            Event::Title("other".into()),
            Event::Notify {
                title: String::new(),
                body: "plain".into()
            },
            Event::Notify {
                title: "Build".into(),
                body: "done ok".into()
            },
        ]
    );
    assert_eq!(
        (t.title(), t.pwd()),
        ("other", "file://host.local/Users/me/proj")
    );
    assert!(t.take_events().is_empty(), "taken once");
    // ConEmu's form gives a bare path.
    t.feed(b"\x1b]9;9;/tmp\x07");
    assert_eq!(t.take_events(), [Event::Pwd("/tmp".into())]);
    assert_eq!(t.pwd(), "/tmp");
}

#[test]
fn kitty_graphics_are_off_until_die_draws_them() {
    // Programs ask, then send DA1: the DA1 answer alone says no, so they show text instead of
    // images nothing would draw.
    let mut t = term();
    let query = "\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c";
    assert_eq!(reply(&mut t, query), "\\x1b[?62;22c");
    let show = "\x1b_Gi=32,s=1,v=1,a=T,f=24;AAAA\x1b\\";
    assert_eq!(reply(&mut t, show), "", "nor a transmit and display");
}

#[test]
fn replies_waiting_for_the_pty_are_capped() {
    // A program that floods queries and never reads: past the cap, new answers are dropped.
    let mut t = term();
    let flood = |t: &mut Term| {
        let da = b"\x1b[c".repeat(20_000);
        for _ in 0..10 {
            let mut b = &da[..];
            while !b.is_empty() {
                b = &b[t.feed(b)..];
            }
        }
        let replies = t.take_replies();
        let n = replies.len();
        assert!(n <= REPLIES_MAX && n > REPLIES_MAX - 16, "{n}");
        assert!(replies.ends_with(b"\x1b[?62;22c"), "whole answers only");
    };
    flood(&mut t);
    assert_eq!(
        reply(&mut t, "\x1b[5n"),
        "\\x1b[0n",
        "taken, they come again"
    );
    // A paste isn't a reply: it comes whole however long it is, and the cap holds after it.
    let long = "x".repeat(REPLIES_MAX + 10);
    assert_eq!(t.paste(&long, false).map(|p| p.len()), Ok(long.len()));
    flood(&mut t);
}

#[test]
fn clipboard_writes_reach_die_and_reads_are_denied() {
    let mut t = term();
    t.feed(b"\x1b]52;c;aGVsbG8gY2xpcA==\x07");
    assert_eq!(
        t.take_events(),
        [Event::Clipboard {
            primary: false,
            text: "hello clip".into()
        }]
    );
    t.feed(b"\x1b]52;p;cHJpbWFyeQ==\x07");
    assert_eq!(
        t.take_events(),
        [Event::Clipboard {
            primary: true,
            text: "primary".into()
        }]
    );
    // A read: nothing reaches DIE, and the program learns nothing of the clipboard.
    let r = reply(&mut t, "\x1b]52;c;?\x07");
    assert!(t.take_events().is_empty());
    assert!(!r.contains("aGVsbG8"), "{r}");
}

#[test]
fn pastes() {
    let mut t = term();
    assert_eq!(t.paste("hello", false), Ok(b"hello".to_vec()));
    assert_eq!(
        t.paste("rm -rf ~\n", false),
        Err(PasteRefused),
        "a newline runs it"
    );
    assert_eq!(
        t.paste("a\nb", true),
        Ok(b"a\rb".to_vec()),
        "allowed, newlines are returns"
    );
    assert_eq!(
        t.paste("x\x1by", false),
        Ok(b"x y".to_vec()),
        "control bytes are spaces"
    );
    t.feed(b"\x1b[?2004h");
    assert!(t.modes().bracketed_paste);
    assert_eq!(
        t.paste("a\nb", false),
        Ok(b"\x1b[200~a\nb\x1b[201~".to_vec()),
        "bracketed, a newline is safe"
    );
    assert_eq!(
        t.paste("x\x1b[201~y", false),
        Err(PasteRefused),
        "the bracket's own end"
    );
    // Replies waiting for the PTY stay apart from the paste's bytes.
    t.feed(b"\x1b[5n");
    assert_eq!(t.paste("p", false), Ok(b"\x1b[200~p\x1b[201~".to_vec()));
    assert_eq!(t.take_replies(), b"\x1b[0n");
}

#[test]
fn focus_reports() {
    let mut t = term();
    assert_eq!(t.focus(true), b"", "only when asked for");
    t.feed(b"\x1b[?1004h");
    assert!(t.modes().focus_events);
    assert_eq!(t.focus(true), b"\x1b[I");
    assert_eq!(t.focus(false), b"\x1b[O");
}

#[test]
fn known_gaps_until_ghostty_adds_them() {
    // OSC 99 (kitty notifications) is dropped, and OSC 66 (text sizing) too (scope §6).
    let mut t = term();
    t.feed(b"\x1b]99;i=1:d=0;kitty notif\x1b\\");
    assert!(
        t.take_events().is_empty(),
        "OSC 99 now reaches a callback: add it"
    );
    t.feed(b"\x1b]66;s=2;BIG\x1b\\");
    let text = t.text();
    assert!(
        !text.lines.iter().any(|l| l.text.contains("BIG")),
        "OSC 66 now draws"
    );
    // The title stack (CSI 22 t, CSI 23 t).
    t.feed(b"\x1b]2;one\x07\x1b[22t\x1b]2;two\x07\x1b[23t");
    assert_eq!(t.title(), "two", "the title stack now pops");
}
